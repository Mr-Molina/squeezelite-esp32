# Phase 0+1 Design: Foundation & Correct Player

**Status:** Approved design, pending spec review
**Parent:** [2026-10-29-rust-full-parity-roadmap.md](../plans/2026-10-29-rust-full-parity-roadmap.md)
**Baseline:** branch `MK5-Rust` at `7dbbced`; parity gaps per [2026-10-29-rust-parity-analysis.md](../plans/2026-10-29-rust-parity-analysis.md)

## 1. Context and goals

The Rust firmware's slimproto/player logic exists twice: a tested host-only `PlayerCoordinator` path (`coordinator.rs`, `network.rs`, both heavily `#[cfg(not(target_arch = "xtensa"))]`) and a ~2,500-line untested device re-implementation inline in `xtensa_hw::run_hardware_firmware()` (`louder-esp32-app/src/main.rs:427–2919`). The device path lacks STMn, `strm a`, META/RESP forwarding, and every playback-semantics feature (gapless, crossfade, replay gain, sync timing, thresholds).

**Phase 0 goal:** one protocol engine, one task runtime, no baked-in deployment constants, UDP-capable network stack.
**Phase 1 goal:** full slimproto + playback-semantics parity with the C firmware for the LMS player itself, on device.

**Non-goals (later phases):** new codecs (P2), discovery/mDNS/provisioning/web-UI/OTA (P3), BT changes (P4), AirPlay (P5), Spotify (P6), displays/peripherals/multi-DAC (P7). smoltcp UDP is *enabled* in P0 but only consumed from P3.

**Owner-confirmed decisions:** pure-Rust bare-metal (no ESP-IDF C); embassy async tasks as the concurrency model; correct-player-first sequencing; full-platform parity as the program end-state.

## 2. Spikes (before implementation plan freeze)

| ID | Question | Pass criteria | Fallback |
|---|---|---|---|
| S1 | Do embassy-executor tasks coexist with `esp_rtos::start` + esp-radio 1.0.0-beta.1 on ESP32 hardware (both cores)? | 2 async tasks per core + Wi-Fi STA associate + smoltcp TCP echo run 10 min without WDT reset | esp-rtos native threads; if that fails, structured cooperative loop (design §3 keeps the same module boundaries either way) |
| S2 | smoltcp with `socket-udp`+`socket-dns` features: flash/RAM delta | Stage-only build fits existing `.bss`/`.stack` budgets with ≥8 KB headroom | Feature-gate UDP off until P3 and re-budget |
| S3 | I2S APLL clocking at 176.4 k/192 k with 32-bit slots via esp-hal 1.2.2 / direct register writes | Measured BCK within 100 ppm at 44.1/48/88.2/96/176.4/192 k | Cap `MaxSampleRate` at highest verified rate; HELO derives from the verified table (honesty rule still satisfied) |

Spike code is throwaway; findings are recorded in the implementation plan.

## 3. Phase 0 architecture

### 3.1 Task layout (embassy, one executor per core)

**Core 0 — control & network**
| Task | Replaces | Notes |
|---|---|---|
| `net_stack` | inline smoltcp poll | owns `Interface`+`SocketSet`; wakes on radio IRQ/timers |
| `slimproto_session` | inline LMS handling | connect → HELO/SETD → drive `PlayerCoordinator`; reconnect w/ backoff+jitter |
| `audio_fetch` | inline HTTP stream code | `StreamSource` impl over smoltcp; fills stream ring |
| `http_control` | inline server | existing endpoints, unchanged behavior |
| `bt_a2dp` | inline BT poll | existing stack, wrapped as a task; behavior unchanged in P0/P1 |
| `cyd_bridge` | inline UART poll | unchanged protocol |
| `config_writer` | reboot-only persistence | debounced flush (§3.5) |
| `watchdog_lease` | inline checks | feeds WDT, enforces network-lease resets (unchanged policy) |

**Core 1 — audio (replaces the NOP heartbeat loop)**
| Task | Role |
|---|---|
| `decode_pump` | stream ring → `ActiveDecoder` → DSP (EQ/loudness/fade/replay-gain/volume) → output ring |
| `i2s_feed` | output ring → I2S DMA; silence when empty; maintains DMA-outstanding counter for elapsed-time correction |

Data stays on the existing SPSC byte rings (PSRAM: 512 KB stream / 128 KB output). Control uses embassy-sync channels:
- `ctrl: Channel<CoordinatorCmd>` (coordinator → audio: Start{params}, Pause, Unpause, Flush, SkipAhead{ms}, SetGain, SetEq, …)
- `evt: Channel<AudioEvent>` (audio → coordinator: FirstFrameWritten, TrackBoundaryCrossed{idx}, OutputDrained, Underrun, DecoderError{kind})

Flash I/O keeps the existing invariants: DRAM-only buffers, `multicore_auto_park`, and Core 1 parking rules are unchanged. Boot gates (otadata pre-arm, panic→reset, 20 s WDT) are untouched.

### 3.2 One protocol engine: traits

`PlayerCoordinator` becomes `#[cfg]`-free and generic over three narrow traits (new module `louder-esp32-app/src/io_traits.rs`, host impls in `network.rs`, device impls in new `net_device.rs`):

```rust
pub trait SlimTransport {
    fn poll_frame(&mut self, buf: &mut [u8]) -> Result<Option<usize>, TransportError>; // framed read, non-blocking
    fn send(&mut self, frame: &[u8]) -> Result<(), TransportError>;
    fn connected(&self) -> bool;
}

pub trait StreamSource {
    fn connect(&mut self, addr: SocketAddr, http_request: &[u8]) -> Result<(), StreamError>;
    fn poll_bytes(&mut self, out: &mut [u8]) -> Result<usize, StreamError>; // ICY stripped; headers captured
    fn take_response_headers(&mut self) -> Option<HeaderBuf>;   // for RESP
    fn take_icy_metadata(&mut self) -> Option<MetaBuf>;         // for META
    fn set_icy_metaint(&mut self, interval: usize);
    fn is_eof(&self) -> bool;
    fn abort(&mut self);
}

pub trait AudioBackend {
    fn set_sample_rate(&mut self, hz: u32) -> Result<(), ClockError>; // refuses unsupported rates
    fn set_hw_volume(&mut self, left: u8, right: u8);                  // TAS 0x4C/0x4D
    fn set_mute(&mut self, mute: bool);
    fn set_power(&mut self, on: bool);                                 // aude amp stop/start
    fn dma_outstanding_frames(&self) -> u32;                           // elapsed correction
}
```

Host: `SlimTransport`/`StreamSource` over `std::net` (current `SlimprotoClient`/`HttpAudioStream`, adapted); `AudioBackend` mock. Device: smoltcp socket handles + `AmpManager`/I2S registers. The device `StreamSource` gains the host's redirect-following (301/302/307/308, ≤3 hops) so behavior converges; chunked transfer-encoding support likewise.

`run_hardware_firmware()` shrinks to: bring-up (clocks, PSRAM, flash, radio, I2S, I2C/amp, UART) + config load + task spawn. Target ≤400 lines. The inline protocol/stream/state code is deleted, not kept behind a flag.

### 3.3 Device/host parity rule (standing invariant)

New CI-enforced rule: `grep -R "cfg(not(target_arch" crates/louder-esp32-app/src/{coordinator,pipeline}.rs` must return nothing. Platform differences live only in trait impls and bring-up. Any future feature must land in the coordinator/pipeline, never inline in a target loop.

### 3.4 De-hardcoding deployment constants

Boot config resolution order (each field independently):
1. `FlashJournal` `AudioConfig`/`WifiCredentials` (0x3F0000 settings partition, existing dual-sector format — now actually used for name/LMS/Wi-Fi);
2. C-firmware NVS at 0x9000 (Wi-Fi creds incl. `enc:` scheme — read-only compat, as today);
3. compile-time defaults (`board.rs` constants), demoted to first-boot seeds.

Applied at boot: `player_name` (announced via `SETD` after HELO, and returned on `setd` id-0 queries), `lms_ip:port`, `wifi` credentials, volume/EQ/AGAIN/loudness (as today). MAC: efuse STA MAC everywhere (HELO + smoltcp); the `BASEMENT_PLAYER_MAC` constant is deleted. `setd` id-0 writes persist the name.

### 3.5 Persistence on change

`config_writer` task: coordinator/HTTP/CYD mark fields dirty through a `ConfigHandle` (embassy-sync mutex over the in-RAM `AudioConfig`); the task debounces 2 s after the last change, then journals to flash (DRAM bounce buffer, Core-1 parked per invariant). Explicit flush before any reboot endpoint. Wear bound: ≤1 write per 2 s burst, dual-sector ping-pong as implemented in `squeezelite-config` (the device starts using sector B properly).

### 3.6 smoltcp features

Enable `socket-udp`, `socket-dns`, multicast. Socket set grows to: DHCP, HTTP server, slimproto, audio, +2 UDP placeholders (discovery/mDNS, unused until P3). Budget verified by S2.

## 4. Phase 1 design

### 4.1 Gapless and track boundaries

Output ring gains a marker queue (`heapless::Deque<TrackMarker, 4>`):

```rust
struct TrackMarker {
    bytes_from_write_pos: u64,  // distance ahead of current write position
    sample_rate: u32,
    replay_gain: u32,           // 16.16, 0 = none
    output_threshold: u32,      // frames before STMs for this track
}
```

- `strm s` while output still has data → **no flush**; decoder switches to the new stream; a `TrackMarker` is pushed at the current write position. (The stream ring *is* flushed only if the server said so via `flsh`/`q` first — normal LMS gapless keeps it.)
- `decode_pump` decrements the active marker's distance as it consumes; on crossing: apply new rate (if changed: drain DMA, switch clock), swap replay gain, emit `TrackBoundaryCrossed` → coordinator emits `STMs`.
- Flush (and marker-queue clear) happens only on `strm q`, `flsh`, `serv`, or disconnect.
- Rate-change-at-boundary is the one audible compromise: a short DMA drain pause, same as C.

### 4.2 Fades and crossfade

`FadeEngine` stage in `decode_pump`, operating on i32 frames before volume:
- Modes: None, FadeIn, FadeOut, FadeInOut, Crossfade (from `strm` `trans_type`), duration from `trans_period`.
- Fade in/out/in-out: linear-in-dB ramp over N frames around track boundaries (markers give the positions).
- Crossfade: on `strm s` with crossfade mode and a draining track, `decode_pump` rewinds the output ring by `min(remaining, fade_frames)` (the ring already supports `rewind_read`) and mixes new-track frames into the overlap with complementary gains, saturating-add. Requires both tracks at the same rate; if rates differ, degrade to fade-in-out (C behaves equivalently via output-buffer constraints).

### 4.3 Gain path

Composite per-channel gain in `VolumeScale`: `gain = volume(16.16) × replay_gain(16.16) × fade(16.16)`, recomputed on change, applied in one multiply pass. Additions: polarity inversion (negate samples; from `strm` flags bits 0–1), channel select mono/L/R (flags bits 2–3). `audg` honors `digital_atten == 0` → unity software gain (hardware TAS volume still tracks, as today).

### 4.4 Protocol completeness (device)

- **STMn**: unknown codec byte, decoder-init failure, unsupported-by-clock-table sample rate, or stream-connect failure (+ `DSCO(Unreachable)`); state → Stopped. Removes the mute-and-pretend path (`main.rs:2240`).
- **STMo**: output-ring underrun while Playing (rate-limited to 1/10 s).
- **`strm a`**: drop N ms from the output ring (device parity with `coordinator.rs:445`).
- **`strm p` with interval / `strm u` with jiffies**: embassy-time `Instant` deadlines in the coordinator; resume accuracy target ±2 ms (vs jiffies clock).
- **Thresholds**: autostart waits for `threshold` KB in the stream ring; `STMs` additionally gated by `output_threshold` (per-track, carried in the marker).
- **`serv`**: parse addr (+ `SyncgroupID`), disconnect, reconnect to new server; SqueezeNetwork address form accepted; syncgroup echoed in HELO caps thereafter.
- **META/RESP**: device forwards captured HTTP response headers (RESP) and ICY inline metadata (META) — also feeds CYD `META=` lines (artist/title split on ` - ` best-effort, as C's displayer does for ICY).
- **Reconnect**: exponential backoff 1→30 s with ±20 % jitter (parity with C MK5), HELO `reconnect` bit + `bytes_received` carryover.
- **HELO honesty**: capability string built at boot from reality: `Model=squeezeesp32`, `ModelName=<configured model name, default SqueezeESP32 — distinct from the player name, which travels via SETD>`, `AccuratePlayPoints=1` (true after §4.6), `MaxSampleRate=<highest S3-verified rate>`, `HasPolarityInversion=1`, codecs `flc,pcm,wav,aif,mp3` (unchanged until P2). `HasDigitalOut` is **dropped** until SPDIF exists (P7).

### 4.5 Clocking

`ClockTable` module: for each supported rate {8, 11.025, 12, 16, 22.05, 24, 32, 44.1, 48, 88.2, 96, 176.4, 192} kHz compute APLL/divider settings for 64×fs BCK (32-bit slots, stereo); verified set determined by spike S3. `AudioBackend::set_sample_rate` returns `Err` for anything outside the verified set → coordinator sends STMn. The silent 44.1 k fallback is deleted.

### 4.6 Elapsed time

`frames_played_reported = frames_pushed_to_dma − dma_outstanding_frames()`, where `i2s_feed` tracks bytes queued vs. DMA descriptors completed. STAT elapsed derives from that at the active rate; target accuracy ±1 DMA buffer (≈ 23 ms at 44.1 k, matching C's `device_frames` approach).

### 4.7 DSP

- **Runtime RBJ**: `equalizer.rs` gains `design_peaking(f0, q, gain_db, fs)` (f32, Schur-Cohn check retained); coefficients recomputed on rate change and on gain change. The five fixed 44.1 k preset tables are replaced by preset *gain vectors* + runtime design.
- **LMS `eqlz`**: 10 signed gains → EQ bands (clamped ±12 dB, matching plugin range); `loud` level 0–10 stored.
- **Loudness**: volume-indexed per-band contour (C model): `band_boost_db = loudness_polynomial(band) × (1 − volume_fraction) × level/10`, folded into the EQ gain vector; the per-sample `horner_loudness` amplitude hack is deleted.
- **Resampler** (final P1 milestone): fixed-point polyphase windowed-sinc, quality basic/low (13-tap)/medium (21-tap) + optional linear interpolation, i32 path; engaged when stream rate ∉ verified clock table or user caps rates (config keys mirroring `-r`/`-R`/`-Z`). Mono-per-channel processing, PSRAM scratch.

### 4.8 Error handling

- Decoder errors: first error → attempt resync (MP3/FLAC already do); persistent (>N frames or init) → `DecoderError` → STMn + Stopped.
- Transport errors: slimproto socket error → reconnect loop (backoff); audio socket error mid-stream → `DSCO(reason)` + STMd-path per current host semantics.
- Clock/amp errors: `set_sample_rate` failure → STMn; amp fault → existing 2 s poll + auto-recovery, now surfaced as a counter in `/status.json` (I2C error swallowing in `Esp32I2cBus` stays in P0/P1 scope *only* for counting: NACK counters are now also exposed per-operation so faults aren't fully silent).
- All error paths emit one rate-limited log line; no panics in steady-state paths (panics remain boot-time only; panic → reset → recovery gate unchanged).

## 5. Testing

**Host (cargo test, mock LMS + mock sinks):**
1. Gapless: two-track PCM sequence → no flush, exactly one STMs per track at marker crossing, zero dropped/duplicated frames (checksum both tracks).
2. STMn: unknown codec, connect-refused, unsupported rate.
3. Threshold-gated autostart and output_threshold-gated STMs.
4. Pause-for-interval and unpause-at-jiffies on a virtual clock (±1 tick).
5. `strm a` drops the right byte count; `serv` reconnects and re-HELOs with syncgroup.
6. Replay-gain/fade/polarity/channel math: sample-exact fixtures.
7. Crossfade: overlap energy monotonicity + length.
8. RBJ design vs. reference coefficients (tolerance 1e-6) at 44.1/48/96 k; loudness contour vector tests.
9. Resampler: SNR ≥ 90 dB (medium) for in-band sine at 44.1→48 k and 96→44.1 k; THD bound for basic/low.
10. Trait-conformance suite run against both host impls and a loopback fake, so device impls get the same suite on target later.
11. Proptest: marker-queue invariants under random strm sequences; config dirty/flush interleavings.

**Device gates:**
- Stage-only: `.bss`/`.stack` within current budgets (36,528 B stack intact), binary ≤ `ota_0` size, S2 headroom.
- Live cert (existing WO workflow): gapless album (FLAC 44.1/16 and 24/96), pause/resume precision audible-click-free, `eqlz` from LMS plugin audibly applied, STMn visible in LMS for a deliberately broken stream, power-cycle persistence of name/volume/EQ, 1 h soak with zero watchdog resets, CYD shows ICY `META=` on an internet-radio stream.

**Exit criteria:** parity-analysis §2.1 rows (except `grf*`/`visu`/BT/display rows deferred to P4/P7), §2.3 all rows, and §3 findings 1–6/8/9 resolved on **device**; workspace tests + clippy clean; both spikes' findings recorded.

## 6. Work breakdown (for writing-plans)

1. Spikes S1–S3.
2. P0-A: embassy task skeleton + bring-up extraction (behavior-preserving re-wrap of existing inline logic).
3. P0-B: trait extraction + coordinator unification + inline-code deletion (largest step; done when the cfg-grep invariant passes).
4. P0-C: de-hardcoding + persistence-on-change.
5. P1-A: protocol completeness (STMn/STMo/`strm a`/timing/thresholds/`serv`/META/RESP/HELO).
6. P1-B: gapless markers + fades/crossfade + gain path.
7. P1-C: clock table + elapsed correction.
8. P1-D: runtime EQ/loudness + `eqlz`/`loud`.
9. P1-E: resampler.
10. Live certification + parity-analysis refresh.

Steps 5–9 each land behind green workspace tests; device cert after 7 and again after 10.

## 7. Risks specific to this phase

| Risk | Mitigation |
|---|---|
| Embassy/esp-rtos coexistence fails (S1) | Fallback ladder in §2; module boundaries identical either way |
| Coordinator unification regresses live-certified behavior | P0-B is behavior-preserving by test: host suite + a recorded strm-session replay fixture captured from the current device build before refactor |
| Crossfade rewind interacts with marker queue | Proptest invariant: markers never point behind read pos after rewind; crossfade degrades to fade when it would |
| APLL 176.4/192 k unstable with shared PSRAM clocking | S3 decides; HELO honesty makes any outcome correct |
| Flash wear from persistence-on-change | 2 s debounce + dirty-only writes + dual-sector journal; worst realistic case ≪ 10³ writes/day vs 10⁵ endurance |
