# Phase 0+1: Foundation & Correct Player — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One `#[cfg]`-free slimproto engine running under embassy tasks on both cores, with full playback-semantics parity (gapless, fades/crossfade, replay gain, sync timing, thresholds, STMn/STMo, honest HELO, full clock table, runtime EQ, resampler) on the device.

**Architecture:** Extract `SlimTransport`/`StreamSource`/`AudioBackend` traits so `PlayerCoordinator` + `StreamPipeline` become the only protocol/audio engine; host (`std::net`) and device (smoltcp) implement the traits. Device bring-up shrinks to init + embassy task spawning (Core 0 network/control, Core 1 audio). Playback semantics land as pure, host-tested units (marker queue, fade engine, gain stage, clock table, RBJ designer, resampler) wired into the pipeline.

**Tech Stack:** Rust 2021/2024 no_std, esp-hal 1.2.2, esp-rtos 0.4, esp-radio 1.0.0-beta.1 (vendored), embassy-executor + embassy-time + embassy-sync (versions fixed by Spike S1), smoltcp 0.12 (+`socket-udp`,`socket-dns`), heapless 0.8.

**Spec:** `docs/superpowers/specs/2026-10-29-phase0-1-foundation-correct-player-design.md`

## Global Constraints

- Workspace gates after every task: `cargo test --workspace` green and `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean (run from `rust/`).
- `#![forbid(unsafe_code)]` stays in `squeezelite-proto`, `squeezelite-audio-core`, `squeezelite-config`, `squeezelite-decoders`, `squeezelite-cyd-link`, `squeezelite-tas58xx`.
- Device (xtensa) stage builds must keep Core 0 `.stack` = 36,528 B and fit current `.bss` budget with ≥8 KB headroom (spec S2); stage command per `WO-034` workflow (`espflash save-image`).
- No `#[cfg(...target_arch...)]` in `louder-esp32-app/src/{coordinator.rs,pipeline.rs}` from Task 7 on (enforced by test added in Task 7).
- Flash I/O buffers in internal DRAM only; `FlashStorage` with `.multicore_auto_park()`; otadata pre-arm, 20 s WDT, and network-lease reset policies unchanged.
- Anti-brick rule: never call `POST /reboot_ota.json` on hardware unless the owner asked.
- Commits: conventional style (`feat|fix|test|refactor(scope): ...`); end every commit message with the session attribution footer (Co-Authored-By + Claude-Session lines used by this session); no model IDs in code or commits.
- Unit tests live in inline `mod tests` per existing crate convention; integration tests in `crates/<crate>/tests/`.

## Review Focus

Failure modes the spec implies but no obvious task test exercised — each now pinned in its owning task:

1. `strm s` (gapless continuation) arriving while **paused** — marker must queue, no `STMs`, no unpause. → Task 12 `test_marker_queued_while_paused_no_stms`.
2. ICY metadata block **split across TCP reads** on the device `StreamSource` (host already pins this; device re-implements it). → Task 9 conformance suite rerun (`icy_split_across_reads` case) against `SmoltcpStreamSource` via loopback fake.
3. `audg` arriving **during a crossfade overlap** — volume change must not reset or spike fade gain. → Task 14 `test_gain_change_during_fade_preserves_fade_progress`.
4. **Rate change at a track boundary** while DMA still drains the old track — old-track frames play at the old rate; switch happens only after drain. → Task 15 `test_rate_switch_deferred_until_marker_crossed`.
5. Config flash write racing audio/coordinator mutation — dirty-flag writer must serialize through `ConfigHandle` and never persist a torn struct. → Task 10 `test_config_writer_debounce_and_atomic_snapshot`.

---

### Task 1: Spike S1 — embassy coexistence (throwaway)

**Files:**
- Create: `rust/spikes/s1-embassy-coex/` (own crate, NOT a workspace member; delete after findings recorded)

**Interfaces:** none (output is a findings section appended to this plan file under "Spike findings").

- [ ] **Step 1:** Build a minimal xtensa binary: `esp_rtos::start(...)` + esp-radio Wi-Fi STA associate + embassy executor with 2 tasks per core (1 Hz heartbeat print, smoltcp TCP echo on 7777). Pin embassy-executor/embassy-time versions compatible with esp-hal 1.2.2 (document exact versions chosen).
- [ ] **Step 2:** Run on hardware 10 min: no WDT reset, both cores' heartbeats advance, echo works. If hardware is unavailable this session, run the longest feasible QEMU/host-sim subset and mark the hardware step as a pending gate for the first live cert.
- [ ] **Step 3:** Record findings (versions, executor setup, interrupt config, gotchas) in this plan under "Spike findings"; delete `rust/spikes/s1-embassy-coex/`; commit the plan edit (`docs(spike): record S1 findings`).
- [ ] **Step 4 (fallback):** If coexistence fails, record failure mode and repeat with esp-rtos threads; if that fails, record "structured cooperative loop" decision. Tasks 8–9 consume whichever model S1 selects; their interfaces don't change.

### Task 2: Spike S2 — smoltcp UDP/DNS budget

**Files:**
- Modify: `rust/crates/louder-esp32-app/Cargo.toml` (smoltcp features += `socket-udp`, `socket-dns`, `multicast` — exact feature names per smoltcp 0.12 docs)

- [ ] **Step 1:** Add features; `cargo build -p louder-esp32-app` (host) green.
- [ ] **Step 2:** Stage xtensa build; compare `.bss`/flash vs baseline (`b49854f` stage numbers). Expected: ≤8 KB `.bss` growth before adding sockets (sockets come in Task 8).
- [ ] **Step 3:** Record deltas under "Spike findings". If over budget, gate features behind `feature = "udp"` (default-off until P3) and record that.
- [ ] **Step 4:** Commit (`feat(net): enable smoltcp udp/dns features within budget`).

### Task 3: Spike S3 — I2S clock table verification

**Files:**
- Create: `rust/crates/louder-esp32-app/src/clock_table.rs`
- Modify: `rust/crates/louder-esp32-app/src/lib.rs` (add `pub mod clock_table;`)

**Interfaces:**
- Produces: `pub struct I2sClockCfg { pub clkm_div_num: u8, pub clkm_div_a: u8, pub clkm_div_b: u8, pub bck_div: u8, pub use_apll: bool }`; `pub fn i2s_clock_config(rate_hz: u32) -> Option<I2sClockCfg>`; `pub const VERIFIED_RATES: &[u32]`; `pub fn max_verified_rate() -> u32`.

- [ ] **Step 1:** Write failing tests `test_clock_config_exact_rates` (for each of 8000, 11025, 12000, 16000, 22050, 24000, 32000, 44100, 48000, 88200, 96000, 176400, 192000: `i2s_clock_config(r)` is `Some`, and reconstructed BCK = r×64 within 100 ppm) and `test_unknown_rate_is_none` (`i2s_clock_config(44101) == None`).
- [ ] **Step 2:** Run `cargo test -p louder-esp32-app clock_table` — FAIL (module missing).
- [ ] **Step 3:** Implement: derive divider math from the existing fractional-divider code in `main.rs:882-912` (160 MHz PLL path) and add APLL configs where the integer path can't hit 100 ppm (176.4 k/192 k, 11.025 k family). Pure function, host-testable.
- [ ] **Step 4:** Tests pass; clippy clean.
- [ ] **Step 5 (hardware gate):** On first live cert (Task 18), measure BCK at 44.1/48/96/176.4/192 k; drop unverifiable rates from `VERIFIED_RATES` (HELO honesty makes any outcome correct). Record in "Spike findings".
- [ ] **Step 6:** Commit (`feat(clock): verified I2S clock table for 8k-192k`).

### Task 4: Behavior-preservation replay fixture

**Files:**
- Create: `rust/crates/louder-esp32-app/tests/session_replay.rs`
- Create: `rust/crates/louder-esp32-app/tests/fixtures/strm_session_baseline.txt`

**Interfaces:**
- Produces: `fn run_scripted_session(script: &[ScriptStep]) -> Vec<ClientMessage>` test helper; `enum ScriptStep { Server(Vec<u8>), StreamBytes(Vec<u8>), AdvanceMs(u32) }`; `enum ClientMessage { Stat([u8;4]), Resp, Meta, Setd, Dsco(u8), Helo }` (parsed from the coordinator's outbound frames).

- [ ] **Step 1:** Write `test_baseline_session_replay`: scripted session = HELO → `strm t` → `strm s` (PCM 44.1/16, autostart '1', threshold 0) → 32 KB PCM bytes → EOF → `strm q`. Assert the ordered outbound opcode sequence matches the current host coordinator's behavior exactly (capture it by running once and freezing into the fixture file; the test then compares against the fixture).
- [ ] **Step 2:** Run; freeze fixture; test passes against unmodified code. This fixture is the regression net for Tasks 5–11: it must keep passing (updates to it require a comment line in the fixture naming the task that legitimately changed behavior).
- [ ] **Step 3:** Commit (`test(coordinator): freeze baseline strm session fixture`).

### Task 5: `io_traits` + loopback fakes + conformance suite

**Files:**
- Create: `rust/crates/louder-esp32-app/src/io_traits.rs`
- Modify: `rust/crates/louder-esp32-app/src/lib.rs` (add `pub mod io_traits;` — no cfg gate)
- Test: inline `mod tests` + `rust/crates/louder-esp32-app/tests/trait_conformance.rs`

**Interfaces:**
- Produces (exact, no_std, used by every later task):
```rust
pub struct NetAddr { pub ip: [u8; 4], pub port: u16 }
#[derive(Debug, PartialEq)] pub enum TransportError { Disconnected, WouldBlock, Io }
#[derive(Debug, PartialEq)] pub enum StreamError { ConnectFailed, Disconnected, TooManyRedirects, BadResponse, WouldBlock }
#[derive(Debug, PartialEq)] pub enum ClockError { UnsupportedRate(u32) }

pub trait SlimTransport {
    fn poll_frame(&mut self, buf: &mut [u8]) -> Result<Option<usize>, TransportError>;
    fn send(&mut self, frame: &[u8]) -> Result<(), TransportError>;
    fn connected(&self) -> bool;
}
pub trait StreamSource {
    fn connect(&mut self, addr: NetAddr, http_request: &[u8]) -> Result<(), StreamError>;
    fn poll_bytes(&mut self, out: &mut [u8]) -> Result<usize, StreamError>;
    fn take_response_headers(&mut self) -> Option<heapless::Vec<u8, 1024>>;
    fn take_icy_metadata(&mut self) -> Option<heapless::Vec<u8, 256>>;
    fn set_icy_metaint(&mut self, interval: usize);
    fn is_eof(&self) -> bool;
    fn abort(&mut self);
}
pub trait AudioBackend {
    fn set_sample_rate(&mut self, hz: u32) -> Result<(), ClockError>;
    fn set_hw_volume(&mut self, left: u8, right: u8);
    fn set_mute(&mut self, mute: bool);
    fn set_power(&mut self, on: bool);
    fn dma_outstanding_frames(&self) -> u32;
}
pub struct LoopbackTransport { /* VecDeque-free: heapless ring, test-only but compiled always */ }
pub struct FakeStreamSource { /* scripted bytes + headers + icy + redirect script */ }
pub struct MockBackend { pub rate: u32, pub muted: bool, pub powered: bool, pub hw_vol: (u8,u8), pub outstanding: u32 }
```
- Conformance suite: `pub fn run_stream_source_conformance<S: StreamSource>(mk: impl FnMut() -> S)` exercising: plain stream, ICY metaint stripping, **ICY block split across reads**, chunked transfer-encoding, redirect (≤3) and `TooManyRedirects` at 4, header capture, EOF.

- [ ] **Step 1:** Write failing conformance tests running the suite against `FakeStreamSource` and transport tests against `LoopbackTransport` (`test_loopback_roundtrip_frames`, `test_fake_source_passes_conformance`).
- [ ] **Step 2:** `cargo test -p louder-esp32-app trait_conformance` — FAIL.
- [ ] **Step 3:** Implement module + fakes + suite.
- [ ] **Step 4:** Tests pass; clippy clean.
- [ ] **Step 5:** Commit (`feat(io): transport/stream/backend traits with conformance suite`).

### Task 6: Host trait impls

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/network.rs`
- Test: `rust/crates/louder-esp32-app/tests/trait_conformance.rs` (extend)

**Interfaces:**
- Consumes: Task 5 traits.
- Produces: `impl SlimTransport for SlimprotoClient` (non-blocking framed reads; existing framing logic reused), `impl StreamSource for HttpAudioStream` (existing redirect/chunked/ICY code behind the trait), `impl AudioBackend for MockBackend` (moves to io_traits if simpler). Existing public APIs on both types stay (host tests elsewhere still use them).

- [ ] **Step 1:** Write failing test `test_http_audio_stream_passes_conformance` (suite from Task 5 against a `std::net` mock server factory) and `test_slimproto_client_as_transport` (HELO out, scripted `strm t` in via `poll_frame`).
- [ ] **Step 2:** Run — FAIL (traits unimplemented).
- [ ] **Step 3:** Implement the impls as thin adapters over existing methods; set sockets non-blocking for `WouldBlock` semantics.
- [ ] **Step 4:** Tests + full workspace suite pass (fixture from Task 4 untouched).
- [ ] **Step 5:** Commit (`feat(net): host SlimTransport/StreamSource impls`).

### Task 7: Coordinator unification (de-cfg)

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/coordinator.rs` (remove every `#[cfg(not(target_arch = "xtensa"))]`; generics)
- Modify: `rust/crates/louder-esp32-app/src/pipeline.rs` (remove cfg gates except the Aac/Vorbis `ActiveDecoder` variants, which move behind `feature = "std-codecs"` instead of target_arch)
- Create: `rust/crates/louder-esp32-app/tests/no_cfg_invariant.rs`
- Modify: `rust/crates/louder-esp32-app/Cargo.toml` (`std-codecs` feature, default on for host via dev profile; off for xtensa)

**Interfaces:**
- Consumes: Task 5 traits.
- Produces (later tasks call exactly these):
```rust
impl PlayerCoordinator {
    pub fn tick<S: AudioOutputSink, T: SlimTransport, R: StreamSource, B: AudioBackend>(
        &mut self, transport: &mut T, source: &mut R, backend: &mut B,
        pipeline: &mut StreamPipeline<'_, S>, now_ms: u32,
    ) -> Result<(), TransportError>;
}
```
  `tick` subsumes today's `handle_command`/`poll_http_stream`/`evaluate_state`/`check_heartbeat` (they become private, trait-generic helpers). CYD entry points (`handle_cyd_command`, `sync_cyd_telemetry`, `cyd_playback_state`) keep their current signatures.

- [ ] **Step 1:** Write `no_cfg_invariant.rs`: reads `src/coordinator.rs` and `src/pipeline.rs` at compile-test time (`include_str!`), asserts neither contains `"target_arch"`. Run — FAIL.
- [ ] **Step 2:** Refactor: replace direct `SlimprotoClient`/`HttpAudioStream` field use with trait generics on `tick`; route amp volume/mute/power/rate through `AudioBackend`; `handle_command`'s TCP use becomes `T: SlimTransport`.
- [ ] **Step 3:** Port existing coordinator tests to call `tick` with `LoopbackTransport`/`FakeStreamSource`/`MockBackend`. Behavior assertions unchanged.
- [ ] **Step 4:** Full workspace suite green, **Task 4 fixture unchanged**, no_cfg test passes, clippy clean (both default and `--no-default-features`).
- [ ] **Step 5:** Commit (`refactor(coordinator): single cfg-free engine over io traits`).

### Task 8: Device embassy skeleton (behavior-preserving re-wrap)

**Files:**
- Create: `rust/crates/louder-esp32-app/src/tasks.rs` (embassy task fns + channel types)
- Modify: `rust/crates/louder-esp32-app/src/main.rs` (xtensa: spawn executors; keep existing inline logic inside the new tasks for now)
- Modify: `rust/crates/louder-esp32-app/Cargo.toml` (embassy deps per S1 findings)

**Interfaces:**
- Produces:
```rust
pub enum CoordinatorCmd { Start(StreamParams), Pause, Unpause, Flush, SkipAheadMs(u32), SetGain{l:u32,r:u32}, SetMute(bool) }
pub enum AudioEvent { FirstFrameWritten, TrackBoundaryCrossed, OutputDrained, Underrun, DecoderError(u8) }
pub static CTRL: embassy_sync::channel::Channel<CriticalSectionRawMutex, CoordinatorCmd, 8>;
pub static EVT:  embassy_sync::channel::Channel<CriticalSectionRawMutex, AudioEvent, 8>;
```
  Task fns (spec §3.1 names): `net_stack`, `slimproto_session`, `audio_fetch`, `http_control`, `bt_a2dp`, `cyd_bridge`, `config_writer` (stub until Task 10), `watchdog_lease` on Core 0; `decode_pump`, `i2s_feed` on Core 1.

- [ ] **Step 1:** Mechanical extraction: move each functional block of `run_hardware_firmware`'s loop into its task fn, cooperating via the existing shared state (this step changes structure, not behavior; protocol logic still inline). Grow the smoltcp socket set by the 2 UDP placeholder sockets from spec §3.6 (allocated, unused until P3) so the budget measured now is the real one. **S1 sequencing note:** keep the existing manual core-1 start path (`cpu_ctrl.start_app_core` + MMU/cache copy) switchable behind a cfg/feature until the S1 owner gate passes; an early owner flash after this task is the cheap de-risk (see S1 findings).
- [ ] **Step 2:** Host build + workspace tests green (tasks.rs is device-only glue; keep it compiling on host with the loopback fakes where cheap, else `#[cfg(target_arch)]` is permitted in `tasks.rs`/`main.rs` only).
- [ ] **Step 3:** Stage xtensa build; `.stack`/`.bss` within Global Constraints budget.
- [ ] **Step 4:** Commit (`refactor(app): embassy task skeleton on both cores`).

### Task 9: Device trait impls + inline-code deletion

**Files:**
- Create: `rust/crates/louder-esp32-app/src/net_device.rs` (`SmoltcpSlimTransport`, `SmoltcpStreamSource`)
- Create: `rust/crates/louder-esp32-app/src/backend_device.rs` (`Esp32AudioBackend` over `AmpManager` + clock_table + DMA counters)
- Modify: `rust/crates/louder-esp32-app/src/main.rs` (delete inline protocol/stream/state logic; `slimproto_session` drives `PlayerCoordinator::tick`; target ≤400 lines of bring-up + spawn)
- Test: `tests/trait_conformance.rs` (extend)

**Interfaces:**
- Consumes: Tasks 3, 5, 7, 8.
- Produces: `SmoltcpStreamSource` gains host parity: redirects (≤3), chunked decoding, ICY split-across-reads (all shared where possible by moving the pure parsing state machines from `network.rs` into a new `stream_codec` module both impls use — Create: `rust/crates/louder-esp32-app/src/stream_codec.rs` with `pub struct IcyStripper`, `pub struct ChunkedDecoder`, `pub struct RedirectPolicy`).

- [ ] **Step 1:** Write failing tests: `IcyStripper`/`ChunkedDecoder`/`RedirectPolicy` unit tests (reuse assertions from existing network.rs tests) + conformance suite run against a `SmoltcpStreamSource` built over an in-memory socket shim.
- [ ] **Step 2:** Run — FAIL.
- [ ] **Step 3:** Implement `stream_codec`, refactor host `HttpAudioStream` onto it, implement the two smoltcp types and `Esp32AudioBackend` (`set_sample_rate` → `i2s_clock_config` or `Err(ClockError::UnsupportedRate)`; the 44.1 k fallback branch is deleted).
- [ ] **Step 4:** Delete the inline device protocol code from `main.rs`; `slimproto_session` = connect → HELO/SETD → loop `coordinator.tick(...)`.
- [ ] **Step 5:** Workspace tests + fixture green; stage build within budget; grep `main.rs` for `b"STM"` returns nothing (all STAT emission lives in the coordinator).
- [ ] **Step 6:** Commit (`refactor(device): smoltcp trait impls, inline engine deleted`).

### Task 10: De-hardcode identity + persistence-on-change

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/board.rs` (constants demoted to `pub mod first_boot_defaults`), `src/main.rs` (boot resolution order), `src/tasks.rs` (`config_writer` real), `src/http_control.rs` (drop persist-only-on-reboot)
- Create: `rust/crates/louder-esp32-app/src/config_handle.rs`
- Test: inline + `tests/config_persistence.rs`

**Interfaces:**
- Consumes: `squeezelite_config::{AudioConfig, FlashJournal, KeyValueStorage}`.
- Produces: `pub struct ConfigHandle` (mutex-guarded in-RAM `AudioConfig` + dirty flag): `pub fn update(&self, f: impl FnOnce(&mut AudioConfig))`, `pub fn snapshot_if_dirty(&self) -> Option<AudioConfig>`, `pub fn mark_clean(&self)`; `pub const DEBOUNCE_MS: u32 = 2000;` `config_writer` task: on dirty, wait until 2000 ms since last update, snapshot, journal, mark clean; explicit `pub fn flush_now(&self)` called before any reboot action.

- [ ] **Step 1 (Review Focus #5):** Write failing `test_config_writer_debounce_and_atomic_snapshot`: three `update`s at t=0/500/900 ms → exactly one journal write at t=2900 ms whose snapshot equals the final state; an `update` racing `snapshot_if_dirty` never yields a mixed struct (loom-style interleave with threads on host). Also `test_boot_resolution_order`: journal value beats NVS beats default, per field.
- [ ] **Step 2:** Run — FAIL.
- [ ] **Step 3:** Implement; boot applies `player_name` (SETD), `lms_ip:port` (session target), Wi-Fi, vol/eq/again/loud. Delete `BASEMENT_PLAYER_MAC`; HELO + smoltcp both use efuse STA MAC. `setd` id-0 set path calls `config.update(|c| c.set_player_name(..))`. While in `http_control.rs`: expose the amp I2C NACK count per-operation-class in `/status.json` (spec §4.8 — field `i2c_nack_by_op`), no behavior change to the bus wrapper.
- [ ] **Step 4:** Tests pass; stage build; commit (`feat(config): boot-time identity from flash, debounced persistence`).

### Task 11: Protocol completeness (P1-A)

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/coordinator.rs`, `rust/crates/squeezelite-proto/src/lib.rs`
- Test: inline `mod tests` both crates + `tests/session_replay.rs` (new scripted cases)

**Interfaces:**
- Consumes: Task 7 `tick`.
- Produces in `squeezelite-proto`: `StatEvent::OutputUnderrun` (`*b"STMo"`), `ServerCommand::switch_server()` extended to return `(NetAddr, Option<&[u8]> /* syncgroup */)`, `pub fn build_capabilities(model_name: &str, max_rate: u32, syncgroup: Option<&str>, codecs: &str, out: &mut heapless::String<256>) -> Result<(), ParseError>` (emits `Model=squeezeesp32,ModelName=<..>,AccuratePlayPoints=1,HasPolarityInversion=1,MaxSampleRate=<..>,<codecs>[,SyncgroupID=<..>]` — **no `HasDigitalOut`**), `pub fn next_backoff_ms(attempt: u32, seed: u32) -> u32` (1000×2^attempt capped 30000, ±20 % jitter from seed).
- Produces in coordinator: pause/unpause deadlines (`pause_until_ms: Option<u64>`, `unpause_at_jiffies: Option<u32>`), `awaiting_threshold: Option<u32>`, device-path STMn/STMo/`strm a`/META/RESP (host logic now shared — mostly wiring + tests).

- [ ] **Step 1:** Failing tests: `test_unknown_codec_sends_stmn_and_stops` (format byte `b'z'` → STMn, state Stopped, backend muted-not-playing), `test_connect_failure_sends_stmn_dsco_unreachable`, `test_unsupported_rate_sends_stmn` (backend returns `Err(UnsupportedRate)`), `test_output_underrun_emits_stmo_rate_limited` (two underruns 5 s apart → 1 STMo), `test_pause_interval_resumes_after_ms` (strm p interval=250 → playing again at now+250 without server unpause), `test_unpause_at_jiffies`, `test_threshold_gates_autostart` (threshold=16 KB: no decode until 16384 stream bytes), `test_serv_reconnects_with_syncgroup_in_helo`, `test_backoff_sequence_capped_with_jitter`, `test_icy_meta_forwarded_as_meta_and_resp_once`, `test_capabilities_string_exact` (full exact string for max_rate=96000, codecs `"flc,pcm,wav,aif,mp3"`).
- [ ] **Step 2:** Run — FAIL.
- [ ] **Step 3:** Implement (proto builders first, then coordinator wiring; `strm a` consumes `ms × rate/1000 × 8` output bytes — reuse host logic). ICY metadata additionally feeds the CYD: `sync_cyd_telemetry` emits `CydMessage::Meta{artist, title}` split on `" - "` best-effort (spec §4.4); add assertion to `test_icy_meta_forwarded_as_meta_and_resp_once` that one CYD Meta message is produced.
- [ ] **Step 4:** All green incl. fixture (fixture gains the new STMc→STMs ordering only if behavior legitimately changed — then update fixture with comment naming this task); clippy clean.
- [ ] **Step 5:** Commit (`feat(proto): STMn/STMo, pause timing, thresholds, serv, honest HELO, backoff`).

### Task 12: Gapless track markers (P1-B)

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/pipeline.rs`, `src/coordinator.rs`
- Test: inline + proptest in `rust/crates/louder-esp32-app/tests/marker_fuzz.rs`

**Interfaces:**
- Produces:
```rust
pub struct TrackMarker { pub bytes_ahead: u64, pub sample_rate: u32, pub replay_gain: u32, pub output_threshold_frames: u32 }
impl StreamPipeline<'_, S> {
    pub fn queue_track_marker(&mut self, m: TrackMarker) -> Result<(), MarkerError>; // cap 4
    pub fn active_marker_crossed(&mut self) -> Option<TrackMarker>; // polled by decode step
}
```
  `flush()` clears the marker queue. Coordinator: `strm s` with `output_buffer_available() > 0` and no explicit preceding flush → `queue_track_marker` + decoder switch, **no flush**; emits `STMs` on `TrackBoundaryCrossed` (threshold-gated).

- [ ] **Step 1:** Failing tests: `test_gapless_two_tracks_no_flush_one_stms_each` (two PCM tracks with distinct byte patterns; assert sink received every byte of both, exactly 2 STMs, second at the boundary sample index), `test_marker_queued_while_paused_no_stms` (Review Focus #1), `test_flush_clears_markers`, proptest `marker_distances_never_negative_after_rewind`.
- [ ] **Step 2:** Run — FAIL.
- [ ] **Step 3:** Implement: marker distance decremented in `process_output_step` by bytes consumed; crossing returns the marker to the pipeline caller.
- [ ] **Step 4:** Green + clippy; commit (`feat(pipeline): gapless track-boundary markers`).

### Task 13: FadeEngine (P1-B)

**Files:**
- Create: `rust/crates/squeezelite-audio-core/src/fade.rs`
- Modify: `rust/crates/squeezelite-audio-core/src/lib.rs` (`pub mod fade;`), `louder-esp32-app/src/pipeline.rs` (wire stage), `src/coordinator.rs` (map `strm` trans_type/period)
- Test: inline `mod tests` in fade.rs

**Interfaces:**
- Produces:
```rust
pub enum FadeMode { None, In, Out, InOut, Cross }
pub struct FadeEngine { .. }
impl FadeEngine {
    pub fn configure(&mut self, mode: FadeMode, duration_frames: u32);
    pub fn arm_at_boundary(&mut self, frames_until_boundary: u64);
    pub fn gain_q16(&self) -> u32;                       // current fade gain
    pub fn apply(&mut self, frames: &mut [i32]);          // stereo interleaved, advances position
    pub fn mix_crossfade(&mut self, old_tail: &[i32], new_head: &mut [i32]); // complementary gains, saturating add
}
```
  Pipeline: crossfade path uses `AudioRingBuffer::rewind_read(overlap_bytes)` only when both tracks share a sample rate; otherwise `FadeMode::InOut` (degradation per spec §4.2).

- [ ] **Step 1:** Failing tests: `test_fade_in_ramp_monotonic_and_reaches_unity` (ramp linear in dB: gain at 50 % duration ≈ −(depth/2) dB within 0.5 dB), `test_fade_out_reaches_silence`, `test_crossfade_mix_complementary` (old×g + new×(1−g) energy; endpoints exact), `test_crossfade_degrades_to_inout_on_rate_change`.
- [ ] **Step 2:** Run — FAIL. **Step 3:** Implement (Q16.16 gain, dB ramp via per-frame multiplier table of 32 segments). **Step 4:** Green + clippy. **Step 5:** Commit (`feat(audio): fade engine with crossfade mixing`).

### Task 14: Composite gain stage (P1-B)

**Files:**
- Modify: `rust/crates/squeezelite-audio-core/src/volume.rs`, `louder-esp32-app/src/pipeline.rs`, `src/coordinator.rs`
- Test: inline

**Interfaces:**
- Produces:
```rust
pub enum ChannelMode { Stereo, MonoMix, LeftOnly, RightOnly }
pub struct GainStage { pub volume_l_q16: u32, pub volume_r_q16: u32, pub replay_gain_q16: u32, pub fade_q16: u32, pub invert_polarity: bool, pub channel_mode: ChannelMode }
impl GainStage {
    pub fn effective_q16(&self) -> (u64, u64);            // volume×replay×fade per channel
    pub fn apply(&self, samples: &mut [i32]);              // includes polarity + channel select
}
```
  `VolumeScale` remains for BT path; pipeline switches to `GainStage` and refreshes `fade_q16` from `FadeEngine::gain_q16()` once per processed block. Coordinator: `strm` flags bits 0–1 → polarity, bits 2–3 → channel mode; replay_gain field (16.16, 0 = unity) → `replay_gain_q16`; `audg` with `digital_atten == 0` and nonzero gain → unity software volume (hardware TAS volume still applied via `AudioBackend::set_hw_volume`).

- [ ] **Step 1:** Failing tests: `test_composite_gain_exact_fixture` (volume 0x8000 = 0.5, replay gain 0x20000 = 2.0, fade unity → sample 1000 → exactly 1000), `test_polarity_inversion_negates`, `test_channel_select_left_duplicates_left`, `test_digital_atten_zero_forces_unity_sw_gain`, `test_gain_change_during_fade_preserves_fade_progress` (Review Focus #3: set fade to 50 %, change volume, assert `fade_q16` unchanged and output reflects both).
- [ ] **Step 2:** FAIL. **Step 3:** Implement (i64 intermediate, saturate to i32). **Step 4:** Green; commit (`feat(audio): composite gain with replay gain, polarity, channel select`).

### Task 15: Clock enforcement + elapsed correction (P1-C)

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/backend_device.rs`, `src/coordinator.rs`, `src/tasks.rs` (`i2s_feed` DMA accounting)
- Test: inline (MockBackend-driven)

**Interfaces:**
- Consumes: Task 3 `clock_table`, Task 5 `AudioBackend`.
- Produces: coordinator elapsed: `frames_for_stat = frames_pushed.saturating_sub(backend.dma_outstanding_frames() as u64)`; rate switches requested only at marker crossings (`pending_rate: Option<u32>` applied when `active_marker_crossed()` fires).

- [ ] **Step 1:** Failing tests: `test_stat_elapsed_subtracts_dma_outstanding` (pushed 44100 frames, outstanding 1024 → STAT elapsed = 43076/44100 s → elapsed_ms 976), `test_rate_switch_deferred_until_marker_crossed` (Review Focus #4: gapless 44.1→48 k pair; MockBackend records `set_sample_rate(48000)` called only after the 44.1 k byte count fully consumed), `test_unverified_rate_refused_at_strm_s` (STMn, no clock call).
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** Green; stage build; commit (`feat(audio): marker-synced rate switching and DMA-corrected elapsed`).

### Task 16: Runtime EQ + LMS eqlz/loud + loudness contour (P1-D)

**Files:**
- Modify: `rust/crates/squeezelite-audio-core/src/equalizer.rs`, `louder-esp32-app/src/pipeline.rs`, `src/coordinator.rs`
- Test: inline

**Interfaces:**
- Produces:
```rust
pub fn design_peaking(f0_hz: f32, q: f32, gain_db: f32, fs_hz: f32) -> BiquadCoeffs;
pub fn design_bands(gains_db: &[f32; 10], fs_hz: f32) -> [BiquadCoeffs; 10]; // EQ_BANDS_HZ centers, Q=1.414
pub fn preset_gains_db(preset: EqPreset) -> [f32; 10];
pub fn loudness_gains_db(volume_percent: u8, level: u8) -> [f32; 10];        // C-model: boost ∝ (1 - vol/100) × level/10 per band polynomial
impl Equalizer<10> { pub fn retune(&mut self, gains_db: &[f32; 10], fs_hz: f32); } // design + stability check + state reset
```
  Coordinator: `eqlz` payload (10 i8 gains, clamp ±12 dB) → `retune`; `loud` level stored; volume changes re-fold loudness into the gain vector. Pipeline `set_sample_rate` triggers `retune` at the new fs. The per-sample `horner_loudness` path and the 5 static 44.1 k coefficient tables are **deleted** (presets become gain vectors).

- [ ] **Step 1:** Failing tests: `test_rbj_peaking_matches_reference_at_3_rates` (coefficients vs precomputed reference values at 44.1/48/96 k for f0=1 kHz, Q=1.414, +6 dB; tol 1e-6), `test_retune_on_rate_change_keeps_stability`, `test_eqlz_packet_retunes_bands` (gain vector observed), `test_loudness_contour_zero_at_full_volume` (`loudness_gains_db(100, 10) == [0.0; 10]`), `test_loudness_scales_with_level_and_volume` (monotonic in both).
- [ ] **Step 2:** FAIL. **Step 3:** Implement (f32 math; RBJ Audio-EQ-Cookbook peaking formulas). **Step 4:** Green + clippy; commit (`feat(dsp): runtime RBJ EQ, LMS eqlz/loud, volume-indexed loudness`).

### Task 17: Resampler (P1-E)

**Files:**
- Create: `rust/crates/squeezelite-audio-core/src/resampler.rs`
- Modify: lib.rs export; `louder-esp32-app/src/pipeline.rs` (stage between decoder and EQ); `src/coordinator.rs` (engage when stream rate ∉ `VERIFIED_RATES` or above configured max); `squeezelite-config` (`audio.maxrate` key, u32, default 0 = unlimited)
- Test: inline + `tests/resampler_quality.rs`

**Interfaces:**
- Produces:
```rust
pub enum ResampleQuality { Basic, Low, Medium }   // taps: zero-order-ish 7 / 13 / 21, parity with resample16
pub struct Resampler { .. }
impl Resampler {
    pub fn new(from_hz: u32, to_hz: u32, quality: ResampleQuality, interpolate: bool) -> Self;
    pub fn process(&mut self, input: &[i32], output: &mut [i32]) -> (usize /*consumed*/, usize /*produced*/);
    pub fn flush(&mut self, output: &mut [i32]) -> usize;
}
```
  Fixed-point Q31 coefficients, polyphase windowed-sinc (Kaiser β≈8), stereo interleaved, PSRAM-friendly state (≤16 KB per instance).

- [ ] **Step 1:** Failing quality tests: `test_sine_snr_medium_44k1_to_48k` → SNR ≥ 90 dB for 1 kHz sine; `test_sine_snr_downsample_96k_to_44k1` ≥ 90 dB (medium); `test_basic_quality_floor` ≥ 55 dB; `test_length_ratio_exact_over_10s` (produced/consumed ratio within 1 frame of to/from over 10 s of input); `test_engages_only_for_unverified_or_capped_rates` (coordinator-level with MockBackend).
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** Green (quality tests may be `#[cfg(not(debug_assertions))]`-gated for speed — run `--release` in the step). **Step 5:** Commit (`feat(dsp): fixed-point polyphase resampler`).

### Task 18: Live certification + parity refresh

**Files:**
- Modify: `docs/superpowers/plans/2026-10-29-rust-parity-analysis.md` (flip rows), `docs/superpowers/plans/2026-10-29-rust-full-parity-roadmap.md` (check P0/P1)

- [ ] **Step 1:** Stage build; verify budgets; record sizes.
- [ ] **Step 2 (hardware, owner-supervised):** flash via the established recovery→ota_0 workflow; run spec §5 device checklist: gapless album (44.1/16 + 24/96 FLAC), pause/resume precision, LMS `eqlz` audible, deliberate bad-stream → STMn visible in LMS, power-cycle persistence (name/vol/EQ), S3 rate measurements (Task 3 Step 5), 1 h soak no WDT, CYD shows ICY `META=` on internet radio, **S1 soak criteria** (see "Spike findings → S1 findings → Pending owner gate"): 10-min run with Wi-Fi associated, TCP echo on 7777 works, both cores' heartbeats advance, no WDT reset, core-1 start via `esp_rtos::start_second_core` with the manual MMU/cache workaround intact.
- [ ] **Step 3:** Update parity analysis rows + roadmap checkboxes; record S1–S3 findings if not already; commit (`docs(parity): P0+P1 certified, matrix refreshed`).

---

## Spike findings

(Recorded by Tasks 1–3 / 18 as they complete.)

### S1 findings (embassy coexistence) — Task 1

**Verdict:** an embassy-executor based variant compiles and links alongside `esp_rtos::start(...)` + esp-radio 1.0.0-beta.1 (Wi-Fi, with `ble`+`coex` features compiled in) for ESP32. Only Wi-Fi association code and a TCP echo task were written; BLE was compiled but never exercised. **Fallback trigger: not triggered at build/link level; runtime coexistence is unproven until the owner soak** (esp-rtos threads / cooperative loop remain the fallbacks if the soak fails). **Hardware 10-min soak: PENDING OWNER GATE** (no hardware in this session; must be proven at first live cert, Task 18, see "Pending owner gate" below). **QEMU:** not attempted; qemu-system-xtensa's ESP32 model cannot emulate the Wi-Fi radio that esp-radio drives, so a QEMU soak subset is infeasible for this spike and the owner-gate soak is the only meaningful runtime check.

**Sequencing risk:** core-1 start under esp-rtos is validated only at the Task 18 owner gate, but Tasks 8-9 build on it first. Recommendation: Task 8 keeps the existing manual core-1 start path switchable (cfg/feature) until the gate passes, and an early owner flash right after Task 8 is the cheap de-risk.

**Integration crate:** `esp-rtos 0.4.0` itself provides embassy support behind its `embassy` feature (changelog: "The `esp-hal-embassy` crate has been merged into `esp-rtos`"). It supplies the `embassy-time-driver` implementation (1 MHz tick, queue via `embassy-time-queue-utils`), the `__pender`, and `esp_rtos::embassy::{Executor, InterruptExecutor}`. Do NOT add `esp-hal-embassy`, and do NOT enable any `platform-*`/`arch-*` feature on `embassy-executor` (esp-rtos docs say so; build confirmed with none enabled).

**Exact versions (resolved against the existing `rust/Cargo.lock`, which is unchanged for all pre-existing crates):**

| crate | version | notes |
|---|---|---|
| esp-hal | 1.2.2 | `esp32`, `unstable` (unchanged) |
| esp-rtos | 0.4.0 | features `esp32`, `esp-radio`, `esp-alloc`, **`embassy`** (added) |
| esp-radio | 1.0.0-beta.1 | vendored `rust/vendor/esp-radio` via `[patch.crates-io]`; features `esp32 wifi ble coex unstable` |
| embassy-executor | 0.10.0 | default features only (`^0.10` is what esp-rtos requires) |
| embassy-time | 0.5.1 | default features (no `generic-queue-*`; esp-rtos owns the queue) |
| embassy-time-driver | 0.2.2 | pulled by esp-rtos with `tick-hz-1_000_000` |
| embassy-time-queue-utils | 0.3.2 | pulled by esp-rtos |
| embassy-net | **0.8.0** (`tcp`,`dhcpv4`,`medium-ethernet`) | pulls smoltcp **0.12.0** + heapless 0.8 = same as app. embassy-net 0.9.x needs smoltcp 0.13 / heapless 0.9: avoid |
| embassy-net-driver | 0.2.0 | `esp_radio::wifi::Interface` implements it directly (no glue) |
| embassy-sync | 0.8.0 (esp-rtos/esp-radio), 0.7.2 (embassy-net) | duplicates harmless |
| static_cell | 2.1.1 | executors, net resources, core-1 stack |

**Setup that built and linked** (`cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf`, xtensa GCC on PATH; exit 0):
1. `esp_hal::init`, heap (see gotcha 3), `esp_rtos::start(timg0.timer0, p.FROM_CPU_INTR0)` exactly as production does today.
2. `esp_rtos::start_second_core(p.CPU_CTRL, p.FROM_CPU_INTR1, STACK.take(), || { EXEC1.init(Executor::new()).run(|s| { spawn core-1 tasks }) })`. The core-1 executor MUST be created and run inside that closure (it panics if the scheduler is not active on the current CPU).
3. On core 0 (the main task, after `WifiController::new` + `Interface::station()` + `embassy_net::new`): `EXEC0.init(esp_rtos::embassy::Executor::new()).run(|s| { spawn ... })` replaces the superloop; never returns.
4. Task spawn syntax in embassy-executor 0.10: `s.spawn(task_fn(args).unwrap())` (task fns return `Result<SpawnToken, SpawnError>`). `#[embassy_executor::task(pool_size = 2)]` works for one task fn used on both cores.
5. Spike tasks: core 0 = heartbeat, `wifi_task` (`connect_async` / `wait_for_disconnect_async` loop), `net_task` (`embassy_net::Runner::run`), TCP echo on 7777 (`TcpSocket::accept`); core 1 = heartbeat + 250 ms worker. Echo rides embassy-net (smoltcp 0.12 inside), not the hand-rolled `SmolDevice` poll loop.
6. Linked ELF contains `__pender` and `_embassy_time_schedule_wake`. Size (release, spike): text 590 449, data 12 276, bss 184 320 (incl. 64 KiB `.dram2_uninit` heap and `.stack`).

**Interrupt/timer resources consumed:** TIMG0 `timer0` (esp-rtos scheduler tick AND embassy time driver, shared; no extra timer, no SYSTIMER on ESP32), `FROM_CPU_INTR0` (core-0 scheduler yield), `FROM_CPU_INTR1` (core-1 scheduler yield). Thread-mode embassy executors need NO interrupt (they sleep on a per-thread flag via the scheduler). `FROM_CPU_INTR2/3` remain free and are the only ones `esp_rtos::embassy::InterruptExecutor<2|3>` could use. TIMG0 `wdt` is separate and still feedable from a task (production feeds `timg0.wdt`).

**Gotchas:**
1. esp-radio `build.rs` panics "COEX is enabled but BLE is not": keep `ble` + `coex` together (production already does).
2. `esp_rtos::start_on_second_core_only` does NOT support Wi-Fi/BLE; use `start` + `start_second_core`.
3. DRAM budget: `heap_allocator!(size: 72 KiB)` + 16 KiB core-1 stack failed to LINK ("Main stack is smaller than 8192 bytes"). Linking passed with `heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64*1024)` + 24 KiB regular heap + 8 KiB core-1 stack (`.stack` = 0xFF00, `.bss` = 0xD100). The Task 8/9 refactor must re-budget stacks/heap against the production layout (production: 46 KiB heap + static pipeline buffers); check `.stack` stays above 8 KiB with `readelf -S`.
4. Production `run_hardware_firmware` in `main.rs` currently (a) parks APP_CPU at entry (`cpu_ctrl.park_core(AppCpu)` block), (b) copies the PRO flash MMU table and cache-control registers into the APP registers by hand (the DPORT flash-MMU/cache copy block, a PSRAM workaround), then (c) starts core 1 with `cpu_ctrl.start_app_core(APP_CORE_STACK, app_core_loop)`. Switching to `esp_rtos::start_second_core` (which calls esp-hal `start_app_core_with_stack_guard_offset`) likely means the manual MMU/cache copy must still run before `start_second_core`, and the park/start sequence must be re-validated. This is a hypothesis to validate at the owner gate, not a verified prescription, and is the single largest residual risk.
5. Dependency trial in `louder-esp32-app` (esp-rtos `+embassy`, `embassy-executor 0.10.0`, `embassy-time 0.5.1`, `embassy-net 0.8.0`, `static_cell 2`): `cargo +esp build -p louder-esp32-app --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf` finished OK (59 s), no version conflicts, still one smoltcp (0.12.0). Cargo.lock grows ~360 lines (cordyceps, embassy-executor-macros/darling, etc.). Trial reverted; Task 8 owns the real Cargo.toml change.
6. Duplicate-but-harmless versions: embassy-sync 0.6.2/0.7.2/0.8.0, heapless 0.8/0.9.3, embedded-io-async 0.6/0.7.

**Pending owner gate:** Hardware 10-min soak: PENDING OWNER GATE — verify on first live cert (Task 18): no WDT reset, both cores' heartbeats advance, echo on 7777 works with Wi-Fi associated, and gotcha 4 (core-1 start with the MMU/cache workaround intact). If it fails, record the failure mode and fall back per Task 1 Step 4 (esp-rtos threads, then structured cooperative loop); Tasks 8-9 interfaces do not change either way.

### S2 findings (smoltcp UDP/DNS feature budget) — Task 2

**Verdict: WITHIN BUDGET; features enabled unconditionally (no `udp` gate needed).** `.bss` growth is -24 bytes (0 net; identical `.bss` section), flash growth is +10 220 bytes (+0.38% of ota_0). No sockets are allocated yet (Task 8 adds the 2 UDP placeholders + DNS), so these numbers are the cost of the feature flags alone; the per-socket buffer cost lands as `.bss`/stack in Task 8 and must be re-measured there.

**Feature names (smoltcp 0.12.0, verified in `Cargo.toml` of the locked crate):** `socket-udp` (= `socket`), `socket-dns` (= `socket`, `proto-dns`), `multicast` (standalone, `= []`; enables IGMP join/leave + multicast group handling in `iface` for IPv4 with `proto-ipv4`). Final line in `rust/crates/louder-esp32-app/Cargo.toml` (xtensa-only dependency table): `features = ["medium-ethernet", "proto-ipv4", "proto-dhcpv4", "socket-tcp", "socket-dhcpv4", "socket-udp", "socket-dns", "multicast"]`. Notes: `socket-mdns` also exists in 0.12 (`= ["socket-dns"]`); it only adds mDNS-flavoured behaviour to the DNS socket (queries to 224.0.0.251) and was NOT enabled or measured; Task 8/P3 should add it only if the mDNS design needs a resolver rather than a hand-rolled responder over a plain UDP socket. DNS tunables (`dns-max-name-size-*`, `dns-max-result-count-*`, `dns-max-server-count-*`) are left at defaults. `Cargo.lock` is unchanged (feature flags only; no new crates).

**Measurement method:** `cargo +esp build --target xtensa-esp32-none-elf --release -p louder-esp32-app -Zbuild-std=core,alloc` (xtensa GCC on PATH), same HEAD (`1ad1197`), baseline built first, then features added; `xtensa-esp32-elf-size` / `xtensa-esp32-elf-readelf -SW` on `target/xtensa-esp32-none-elf/release/louder-esp32-app`. (`b49854f` stage numbers were unavailable; baseline is the pre-change build at HEAD.) Host `cargo build -p louder-esp32-app` is green but does not compile smoltcp at all (it is an xtensa-only dependency), so it proves nothing about the features; the xtensa build is the real check.

| metric (bytes) | baseline | with udp/dns/multicast | delta |
|---|---|---|---|
| `.text` (0xa6685 → 0xa89b1) | 681 605 | 690 609 | +9 004 |
| `.rodata` (0x17590 → 0x17a38) | 95 632 | 96 824 | +1 192 |
| `.rwtext` (IRAM) / `.rwtext.wifi` | 24 972 / 88 304 | same | 0 |
| `.data` (0x4068 → 0x4080) | 16 488 | 16 512 | +24 |
| `.data.wifi` / `.rodata.wifi` | 2 120 / 42 484 | same | 0 |
| **flash-resident total (size text+data)** | 952 913 | 963 133 | **+10 220 (+1.07%)** |
| `.bss` (0x12718) | 75 544 | 75 544 | **0** |
| `.noinit` | 0 | 0 | 0 |
| `.stack` (0x9038 → 0x9020) | 36 920 | 36 896 | -24 (layout side effect of the +24 `.data`) |
| `.dram2_uninit` (heap) | 98 264 | 98 264 | 0 |
| size(1) bss column (incl. stack + dram2) | 210 728 | 210 704 | -24 |
| ELF file size | 1 215 104 | n/a | n/a |

Flash headroom: 963 133 / 0x290000 (2 686 976) ota_0 = 35.8% used (baseline 35.5%), ~1.72 MB free. The `+24 B` `.data` is statics inside the smoltcp iface (multicast/UDP dispatch tables); the ~10 KB text is the interface's inbound UDP/IGMP/DNS-reply paths, which `smoltcp::iface` compiles in once the sockets features are on even with no socket in the set (so it is a floor, not a monomorphised-only cost).

**Gate result:** budget "≤8 KB `.bss` growth with ≥8 KB headroom" holds trivially (0 growth; `.stack` still 36 896 B, far above the 8 192 B link-time minimum noted in S1 gotcha 3). **Caveat for Task 8:** the budget that actually matters is the socket set: each `udp::Socket` costs its rx/tx `PacketMetadata` + payload buffers, and `dns::Socket` costs its query slots + server list; size them small (e.g. 4 metadata slots, 512 B payload for the LMS discovery socket) and re-measure `.bss`/`.stack` then.
