# MK5-Rust: remaining work (agent hand-off)

Audience: a coding agent picking this up cold. Everything you need is in this
file plus the code; you do not need the conversation that produced it.

State when written: branch `MK5-Rust` at `e35b2e24`, pushed to `origin`.
Line numbers below were checked at that commit; they drift, so locate code by
the function or symbol named next to each one.

---

## 1. Context

**Repository.** Rust port of squeezelite (a Logitech Media Server player) for
ESP32. All Rust lives under `rust/` (a Cargo workspace). Key crates:

| Crate | Role |
|---|---|
| `louder-esp32-app` | Firmware + host app: coordinator (SlimProto state machine), pipeline (stream ring → decoder → resampler/EQ → output ring → I2S), device main loop (`src/main.rs`), AirPlay/BT/HTTP glue |
| `squeezelite-decoders` | PCM, FLAC, ALAC, AAC, Vorbis, Opus, Ogg demux |
| `nanomp3-core` | Pure-Rust minimp3 port |
| `squeezelite-audio-core` | Resampler, equalizer, volume/gain, fade, SPSC ring buffer |
| `squeezelite-raop` | AirPlay (RTSP, RTP, AES-CBC, RSA) |
| `squeezelite-cspot` | Spotify Connect pieces (mostly not wired into the app yet) |
| `squeezelite-proto` | SlimProto parser/encoder |
| `squeezelite-tas58xx` | TAS5805M amplifier driver |

**Device facts that drive priorities.**
- Target: ESP32 (Xtensa LX6, 240 MHz, single-precision FPU, no 64-bit divide
  instruction, slow flash-cached rodata) and ESP32-S3. Bare metal, `esp-hal 1.0`.
- The firmware build (default features, no `std-codecs`) ships only: the static
  MP3 decoder, FLAC, PCM, AirPlay (ALAC), Bluetooth A2DP (SBC). AAC, Vorbis and
  Opus are **host-only**. Spotify (`squeezelite-cspot` CDN/AES/Shannon) and
  `RtpJitterBuffer` are **not wired into the app**.
- Everything runs on core 0 in one superloop: `pump_device_phase`
  (`main.rs:4072`). Core 1 only spins a heartbeat (`app_core_loop`,
  `main.rs:1192`).
- I2S DMA buffer is 16 KB = 2048 stereo frames ≈ 46 ms at 44.1 kHz
  (`I2S_DMA_BUFFER_BYTES`, `main.rs`). Anything that blocks the loop longer
  than that underruns audio.
- Firmware decode feeds at most `READ_SCRATCH_BYTES` = 1728 bytes per decode
  call; PCM scratch is 2304 samples (`pipeline.rs` top).
- Release profile: `opt-level = 2`, `lto = false`, `codegen-units = 1`
  (`rust/Cargo.toml:64`), with `opt-level = 3` overrides for the audio crates.

**Project rules to respect.**
- `.agents/INFRASTRUCTURE_INVARIANTS.md` — in particular **Invariant 46**: only
  whole 8-byte stereo frames may be pushed into the I2S DMA. Read the file
  before touching `main.rs` DMA code or the boot/flash paths (Invariants 47/48).
- `.agents/PATTERN_LIBRARY.yaml` records past failure patterns (e.g.
  `PAT-2026-1025-004`).
- Decoder output must stay **bit-exact** against the reference decoders used in
  tests (claxon for FLAC). Do not "improve" rounding without updating that
  contract deliberately (see T03 for what happens otherwise).
- Do not flash hardware, push, or merge unless the user asks. One task = one
  commit, conventional message (`perf(...)`, `fix(...)`), body explaining why.

**What landed recently (do not redo).**
- `59ada368` `write_dma` refuses sub-8-byte pushes (Invariant 46).
- `3cdbcd08` FLAC: frame decode deferred until the frame is complete
  (STREAMINFO max frame size or next CRC-checked header); new
  `AudioDecoder::finish_input()` called by the coordinator at `stream_eof`;
  32-bit LPC path uses libFLAC's bound; faster Rice/bit reads.
- `e35b2e24` `pump_decode_paced`: decode bursts top up the DMA between steps
  when it is below 75 % full; `record_decode_us` is now called on the device so
  `decode_us_max` on the status page is real.

---

## 2. Commands (run from `rust/`)

```bash
# Host tests (decoders need --all-features for the std codecs)
cargo test -p squeezelite-decoders --all-features --no-fail-fast
cargo test -p louder-esp32-app --no-fail-fast
cargo test --workspace --all-features --no-fail-fast

# Host lint (must stay clean)
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Firmware type-check (Espressif toolchain is installed as `+esp`)
cargo +esp check -p louder-esp32-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc
cargo +esp clippy --workspace --target xtensa-esp32-none-elf -Zbuild-std=core,alloc -- -D warnings

cargo fmt --all
```

**Known baseline failures at `e35b2e24`** (all predate the last three commits;
T01–T03 fix them):
- `squeezelite-decoders` integration tests: `adts_lc_fixture_decodes_to_golden_pcm`,
  `opus_fixture_decodes_successfully`, `ogg_vorbis_fixture_decodes_to_golden_pcm`.
- `louder-esp32-app` bin test: `test_run_firmware_tick_with_network_recovers_amp_fault_and_syncs_rate`.
- Xtensa clippy stops at `squeezelite-audio-core/src/ringbuffer.rs:118`.

**Measuring on device.** The HTTP status page reports `decode_us_max` (worst
decode step, µs, since the decoder was installed). Use it for before/after
numbers on any decoder or DSP task; state in the commit whether a gain was
measured or only reasoned.

---

## 3. Tasks

Format: **ID — title** · priority (P0 highest) · effort (S < 1 h, M = hours,
L = a day+) · where · problem · fix · done when.

### Phase 0 — needs a human with hardware

**HW1 — Field-check the three recent commits** · P0 · S · device
- Flash `MK5-Rust`. Verify: (a) start/skip MP3 tracks — no clicks at track start
  (pacing, `e35b2e24`); (b) play a FLAC track to the end — final ~100 ms is not
  cut off (`finish_input`, `3cdbcd08`); (c) `decode_us_max` is non-zero and
  plausible (MP3 a few ms; FLAC well under the old value).
- An agent cannot do this; list it for the user and do not block other tasks on
  it, except T09/T13 measurements.

### Phase 1 — restore a green baseline (do these first)

**T01 — Fix the Xtensa clippy `manual checked division` lint** · P0 · S
- Where: `squeezelite-audio-core/src/ringbuffer.rs:118` (the `frame_bytes == 0`
  check before `(self.available_write() / frame_bytes) * frame_bytes`).
- Fix: use `checked_div`, e.g.
  `self.available_write().checked_div(frame_bytes).map_or(0, |n| n * frame_bytes)`
  (keep the existing zero behaviour).
- Done when: the Xtensa clippy command above passes for the whole workspace, or
  stops only on further pre-existing lints, which you then fix the same way and
  list in the commit.

**T02 — Repair `test_run_firmware_tick_with_network_recovers_amp_fault_and_syncs_rate`** · P0 · S
- Where: test in `louder-esp32-app/src/main.rs` (bin target, host build); panics
  in `embedded-hal-mock` I2C expectations.
- Cause: commit `87793784` made the TAS58xx driver cache the selected
  book/page (`ensure_page0_book0`, `squeezelite-tas58xx/src/lib.rs`), so
  redundant `PAGE_SELECT/BOOK_SELECT/PAGE_SELECT` writes disappeared. The app
  tests in `amp.rs` were updated; this one was not.
- Fix: update the test's I2C expectation list to the cached sequence. Do not
  change driver behaviour to satisfy the test unless you find the cache is
  actually wrong (if so, stop and report).
- Done when: `cargo test -p louder-esp32-app` is fully green.

**T03 — Resolve the AAC / Opus / Vorbis golden-PCM failures** · P1 · M
- Where: `squeezelite-decoders/tests/{aac,opus,vorbis}_alloc_equivalence.rs`
  (golden assertions at ~lines 139 / 100 / 105) and the decoders in
  `squeezelite-decoders/src/lib.rs`.
- Cause (likely): `87793784` changed float→i32 conversion to round-half-away
  (`to_i32` closures in the Vorbis/Opus paths), switched Vorbis to
  `InterleavedSamples<f32>`, and changed AAC output handling, so the stored
  golden PCM no longer matches.
- Do: confirm the cause by diffing a few samples (expect ±1 LSB differences).
  Then pick one: (a) if the new conversion is intended, regenerate the golden
  fixtures and document why in the commit; (b) if the goldens represent a
  reference decoder (check how they were produced — git log of the fixture
  files), revert the conversion to match. Prefer (b) if the goldens came from
  an external reference.
- Done when: `cargo test -p squeezelite-decoders --all-features` is fully green.
  Host-only codecs, so no firmware impact.

### Phase 2 — correctness

**T04 — Crossfade: restore the "missing frames" debug assert** · P2 · S
- Where: `louder-esp32-app/src/pipeline.rs:940`, `scale_chunk` (crossfade
  branch): `let to_process = (frames * OUTPUT_FRAME_BYTES).min(self.write_scratch.len());`
  followed by `debug_assert_eq!(got, to_process, ...)`.
- Problem: the clamp is dead code today (`process_output_step` already limits
  chunks to `write_scratch.len()`), but it weakened the assert: if chunk sizes
  ever exceed the scratch, only part of the chunk is crossfaded and the assert
  still passes.
- Fix: drop the clamp and assert `got == frames * OUTPUT_FRAME_BYTES` again, or
  keep the clamp and add `debug_assert!(frames * OUTPUT_FRAME_BYTES <= self.write_scratch.len())`.
- Done when: crossfade tests pass; assert covers the full chunk.

**T05 — Vorbis/Opus `has_pending_pcm` true on any partial Ogg page** · P2 · S · host-only
- Where: `squeezelite-decoders/src/lib.rs` — `VorbisDecoder::has_pending_pcm`
  (~3847) and `OpusDecoder::has_pending_pcm` (~4700):
  `... || self.stream_buf.len() >= 27`.
- Problem: after most ingests a partial page sits in `stream_buf`, so the
  coordinator stops feeding input (`pump_decode` breaks on pending) and the
  pipeline calls `decode(&[])`, which returns `NeedMoreData`. Possible stall or
  spin.
- Fix: report pending only when a complete page is buffered (parse `n_segs` and
  lacing at bytes 26.. and compare with `stream_buf.len()`), or only for pending
  PCM / queued packets.
- Done when: a test feeds a Vorbis and an Opus fixture in 1728-byte chunks
  through `StreamPipeline` + coordinator pumps and gets all frames out with no
  iteration cap hit.

### Phase 3 — device performance (ordered by expected impact)

Cost estimates below are reasoned from the code, not measured. Measure with
`decode_us_max` or cycle counters where you can.

**T06 — Resampler: remove the per-frame 64-bit divide; cheaper inner loop** · P1 · M
- Where: `squeezelite-audio-core/src/resampler.rs`. `phase_num: u64` (87),
  `advance_phase` (284), `eval_frame` line 302:
  `((self.phase_num * (NUM_PHASES as u64)) << 16) / u64::from(self.to_hz)`;
  inner loop from 330 (`dot0_l/dot0_r/dot1_l/dot1_r`).
- Problem: Xtensa has no 64-bit divide → `__udivdi3` call on every output frame.
  The interpolation (added in `87793784`) does 4 widening i32×i32→i64
  multiply-adds per tap where interpolating the coefficient first needs 3 (or
  fewer, see below).
- Fix: `phase_num < to_hz <= ~2^19`, so compute in u32:
  `p = phase_num * NUM_PHASES; idx = p / to; rem = p - idx * to;`
  `frac_q16 = ((rem << 12) / to) << 4` (12-bit fraction is enough; check
  `rem << 12` cannot overflow for the max `to_hz` × `NUM_PHASES`, else use
  `(rem as u64 * 65536 / to)` only once per frame — still cheaper than now).
  Make `phase_num` `u32`. Per tap, interpolate the coefficient
  (`c = c0 + (((c1 - c0) as i64 * frac) >> 16) as i32`) and accumulate one
  product per channel.
- Done when: `resampler_quality` tests (`squeezelite-audio-core/tests/`) pass
  with unchanged thresholds; add a unit test that the new phase math matches the
  old one for all phases at 44.1→48 k and 96→48 k within 1 LSB of `frac_q16`.
- Only matters while the resampler is enabled (coordinator.rs ~1819-1843).

**T07 — Hot audio kernels compiled at opt-level 2; EQ loses IRAM placement** · P1 · S (+ measurement)
- Where: `rust/Cargo.toml` profiles; `squeezelite-audio-core/src/equalizer.rs:542-544`
  (`#[cfg_attr(target_arch = "xtensa", link_section = ".iram1")]` on an
  `#[inline]` generic `process_interleaved`).
- Problem: with `lto = false`, `#[inline]`/generic functions (resampler
  `process`/`eval_frame`, `GainStage::apply`, `Equalizer<BANDS>::process_interleaved`)
  are code-generated in `louder-esp32-app`, which builds at opt-level 2, so the
  `opt-level = 3` overrides on the audio crates do not reach them. An inlined
  function also does not keep its `.iram1` section.
- Fix (pick one, measure): add `[profile.release.package.louder-esp32-app] opt-level = 3`;
  or mark the three kernels `#[inline(never)]` (and make the EQ call
  non-generic, `Equalizer<10>`) so they are emitted in their own crate and keep
  `.iram1`. Check IRAM/flash size before and after (`espflash` / linker map) —
  IRAM is limited.
- Done when: firmware builds in release for both ESP32 and ESP32-S3, size delta
  recorded in the commit, tests green.

**T08 — Gain stage: drop the 64-bit clamp when gain ≤ 1.0; remove dead fast path** · P2 · S
- Where: `squeezelite-audio-core/src/volume.rs:195` `GainStage::apply` (live path,
  called from `pipeline.rs` `scale_chunk`) uses `gain()` (`fade.rs:34`):
  `(i64 * i64).clamp(-MAX_SCALESAMPLE, MAX_SCALESAMPLE) >> 16` per sample.
  `scale_samples_inplace` (`volume.rs:241`) got a "≤ 1.0 fast path" in
  `87793784` but has **no production callers** (tests only).
- Fix: in `apply`, when both gains satisfy `-FIXED_ONE < g <= FIXED_ONE`, use
  `((i64::from(g) * i64::from(s)) >> 16) as i32` (the clamp provably cannot
  fire; keep the clamped path otherwise). Keep C-parity comments. Then either
  delete `scale_samples_inplace` and its tests or document it as test-only.
- Done when: add a test that `apply` output equals the clamped `gain()` path
  for gains {0, 0x8000, 0xFFFF, 0x10000, -0xFFFF} over samples including
  `i32::MIN`/`i32::MAX`.

**T09 — AirPlay AES-128-CBC: T-table (or hardware) decrypt** · P1 · M
- Where: `squeezelite-raop/src/crypto.rs` — `Aes128Cbc::decrypt_block` (877),
  `INV_S_BOX` (774), `GF_MUL_*` tables (836, `pub const`), CBC XOR (995).
  Called per RTP packet via `RtpAudioReceiver::process_packet` (`rtp.rs`), from
  `main.rs` ~3821.
- Problem: byte-wise InvShiftRows/InvSubBytes/InvMixColumns (~80 table lookups
  per round, 9 rounds) with tables in flash. ~125 packets/s × ~60-88 blocks →
  est. 5-8 % of a core during AirPlay.
- Fix: equivalent inverse cipher with one `static TD0: [u32; 256]` plus
  `rotate_right(8/16/24)`, state as 4 `u32` columns, round keys 1-9
  pre-transformed with InvMixColumns in `new()`, `INV_S_BOX` for the last round.
  Put tables in internal RAM on Xtensa (`#[cfg_attr(target_arch = "xtensa", link_section = ".dram1...")]`
  — check what section names the linker script accepts). Alternative: the
  ESP32 AES peripheral via esp-hal (bigger change; only if T-tables are not
  enough).
- Done when: existing AES/CBC tests pass; add a test against an FIPS-197 /
  NIST SP 800-38A CBC decrypt vector; record measured or estimated gain.

**T10 — RSA-2048 on AirPlay connect: CRT and no per-step allocation** · P2 · M
- Where: `squeezelite-raop/src/crypto.rs` — `montgomery_mod_pow` (317),
  `mont_reduce` (260, allocates: `t[k..].to_vec()`, `n.to_vec()`, `res.sub`),
  `mul_limbs` (295, `vec![0u32; ..]`). Called by `decrypt_raw` (722) and
  `sign_apple_challenge` (745), from `rtsp.rs` OPTIONS/ANNOUNCE handling, inline
  in the network poll.
- Problem: full 2048-bit constant-time ladder, ~16k heap allocations per
  operation; estimated ≥1 s per call, twice per connect, stalling the loop
  (and audio, see the 46 ms DMA budget).
- Fix: store p, q, dp, dq, qinv for the (public) AirPort Express key and use CRT
  (two 1024-bit exponentiations, ~4× faster); fixed stack scratch
  (`[u32; 2 * MAX_LIMBS]`) instead of Vec; fused CIOS Montgomery multiply.
  Keep it constant-time.
- Done when: existing RSA tests pass (signature and decrypt results identical);
  add a test that CRT result == non-CRT result on random inputs.

**T11 — FLAC: 32-bit sample scratch** · P2 · M
- Where: `squeezelite-decoders/src/lib.rs` — `subframe_scratch: Box<[i64]>`
  (2179, 2 × 4608 samples = 73.7 KB), `decode_subframe`, fixed predictors,
  wasted-bits shift, `decorrelate_samples` (2080).
- Problem: all sample math in i64 (two registers + carry on Xtensa).
- Fix: i32 scratch for streams ≤ 24 bits (side channel ≤ 25 bits; fixed
  order-4 intermediates fit, as in libFLAC); keep an i64 path only for 32-bit
  FLAC (or return `UnsupportedFeature` for it, if no fixture needs it). The
  LPC 32-bit fast path from `3cdbcd08` stays; the i64 LPC fallback must still
  read i32 history correctly.
- Done when: `test_flac_cross_validation_all_fixtures` (claxon, bit-exact) and
  all FLAC tests pass; heap saving noted in the commit.

**T12 — FLAC: decorrelate lazily instead of copying through `pending_pcm`** · P2 · M · do after T11
- Where: `lib.rs` ~2232 (`pending_pcm: Vec::with_capacity(16384)`, 64 KB
  reserved) and the block output loop ~2770-2790 (`for i in to_write..block_size`
  pushes into `pending_pcm`; `decorrelate_samples` matches channel mode per
  sample).
- Problem: with 2304-sample output on firmware, most of each 4096-sample block
  is copied to `pending_pcm` then copied out again.
- Fix: keep `(scratch_pos, block_size, channel_assignment, shift)` and emit
  straight from `subframe_scratch` on later calls (the decode loop already stops
  while samples are pending, so the scratch is not overwritten). Hoist the
  channel-mode `match` out of the loop: one loop per mode over
  `pcm_out.chunks_exact_mut(2)`. Remove the 64 KB reservation.
- Done when: FLAC tests (including the constrained-buffer and
  `finish_input` tests) pass bit-exact.

**T13 — Equalizer: bands-outer loop, no per-sample finiteness branch** · P2 · M
- Where: `squeezelite-audio-core/src/equalizer.rs` `process_interleaved` (544);
  sample loop with `for &i in active_bands` at 583.
- Problem: per frame it reloads coefficients/history for every band, pays bounds
  checks and an `is_finite` branch per band × channel × sample. Est. 5-8 % of a
  core with EQ on.
- Fix: convert the chunk to an `[f32; N]` scratch once, run each active band
  over the whole chunk (coefficients and state in locals), convert back; check
  finiteness once per band after the chunk (where the subnormal flush already
  runs) and reset that band's state if not finite.
- Done when: equalizer tests pass; add a test that output equals the current
  implementation within 1 LSB on a swept sine with 10 active bands + loudness.

**T14 — Output path: one fewer copy per sample** · P3 · M
- Where: `louder-esp32-app/src/hal.rs:214` `EspI2sSink::write_frames` packs
  i32→bytes into `dma_scratch`; `main.rs` `Esp32HardwareI2sDma::write_dma`
  copies again into a 512-byte scratch to swap L/R words, then pushes.
- Fix: let the sink write already-swapped frames (e.g. an `I2sDmaDriver`
  associated const `SWAP_LR: bool`, or a `write_frames_i32` method with a
  default impl) so `write_dma` pushes `data` directly. Also hoist the
  per-sample `self.muted` branch out of the loop.
- Done when: keep Invariant 46 (whole frames only); hal/DMA tests pass;
  loopback test shows identical byte stream.

**T15 — mDNS: answer queries only, cache the reply, bound the loop** · P3 · S
- Where: `louder-esp32-app/src/main.rs:3651` `while mdns_sock.can_recv()`:
  four `windows().any()` scans per packet, no QR-bit check (responses from
  Apple TVs etc. trigger our multicast announcement), announcement rebuilt on
  heap each time (`MdnsConfig { player_name: String::from("BASEMENT"), .. }`),
  unbounded loop.
- Fix: skip unless `payload.len() >= 12 && payload[2] & 0x80 == 0`; cache the
  built announcements in `DeviceRuntimeState` (rebuild on IP change); rate-limit
  multicast replies to ≤ 1/s per service (RFC 6762); cap packets per lap (e.g. 4).
  Also: the player name is hard-coded `"BASEMENT"` here — check whether it
  should come from config.
- Done when: unit-test the query/response filter on captured packet bytes.

**T16 — Bound smoltcp polling and top up DMA mid-lap** · P3 · M
- Where: `main.rs` `net_poll_phase` (3077, `s.smol_iface.poll(...)` at 3083)
  and a second poll in `pump_device_phase` (4084); output is only refilled in
  `output_decode_phase` (4003) at the end of the lap.
- Problem: `Interface::poll` drains all ingress (unbounded per smoltcp docs); a
  burst plus BT/SBC work can exceed the 46 ms DMA budget before the refill.
- Fix: bounded ingress (`poll_ingress_single` loop, e.g. 8 packets, then
  `poll_egress`) and a cheap `s.coordinator.pump_output(s.pipeline, 2)` after
  `slim_session_phase`.
- Done when: Xtensa check passes; no behaviour change in host tests; note that
  hardware validation is needed (HW1-style).

**T17 — Bluetooth SBC synthesis without 64-bit MACs** · P3 · M
- Where: `louder-esp32-app/src/bt_a2dp.rs:1889` `synthesize_eight`
  (`acc += SYNMATRIX8[i][sb] as i64 * (sb_sample[..] as i64)`, 128 + 80 MACs
  per block per channel).
- Fix: exploit `SYNMATRIX8` row symmetry (as libsbc does) to halve the matrix
  MACs; 32-bit accumulation with libsbc-style prescaling where ranges allow.
- Done when: SBC decode tests produce identical PCM (or document an accepted
  ≤ 1 LSB difference with justification).

**T18 — Stream ingest: fewer copies and callbacks** · P3 · M
- Where: `coordinator.rs:2231` `let mut chunk = [0u8; 1024];` →
  `source.poll_bytes` → `pipeline.ingest_stream_data`; `take_stream_side_data`
  runs every 1 KB iteration; `stream_codec.rs:110` and `:294`
  `buf.copy_within(r..r + take, w)` runs even when `r == w`.
- Fix: skip `copy_within` when `r == w` (trivial, do first); hoist
  `take_stream_side_data` out of the inner loop; optionally a contiguous
  write-slice API on the ring plus `recv_with` on the socket trait to read
  straight into the stream ring.
- Done when: stream/ICY/chunked tests pass.

### Phase 4 — cleanup from the review of `87793784`

**T19 — TAS58xx: use the page/book cache consistently** · P3 · S
- Where: `squeezelite-tas58xx/src/lib.rs` — `configure_silicon_maximization`
  (449, 920), `set_analog_gain` (461, 932), `read_telemetry` (467, 938) still
  call `select_page0_book0()` (always 3 I2C writes); everything else uses
  `ensure_page0_book0()`.
- Fix: switch them to `ensure_page0_book0()` unless there is a documented reason
  to force reselection (e.g. after reset — `reconfirm_book_page_and_clear_faults`
  already invalidates the cache). Update mock expectations in `amp.rs` and the
  driver tests.

**T20 — Remove the duplicate CDN IV function** · P4 · S
- `squeezelite-cspot/src/cdn.rs:199` `CdnAudioDecryptor::calculate_iv` has the
  same body as `AudioKeyDecryptor::counter_iv` (109). Make one call the other.

**T21 — Revert hand-rolled word XOR in AES CTR/CBC** · P4 · S
- `squeezelite-cspot/src/crypto.rs:883` `Aes128Ctr::process` and
  `squeezelite-raop/src/crypto.rs:995` (CBC) build `u32` words with
  `from_ne_bytes` just to XOR 16 bytes. Replace with
  `for (d, k) in chunk.iter_mut().zip(&keystream) { *d ^= *k }`. Skip T21 for
  the raop file if T09 rewrites it anyway.

**T22 — AES multiply tables: `static`, not `pub const`** · P4 · S
- `squeezelite-raop/src/crypto.rs:836` `GF_MUL_9/11/13/14`. Make them private
  `static`. Superseded by T09 if done first.

**T23 — Delete unused `apply_crossfade`** · P4 · S
- `squeezelite-audio-core/src/fade.rs:111`, an alias of `mix_crossfade` with no
  callers.

**T24 — Delete the duplicate `SlimprotoParser::OP_*` constants** · P4 · S
- `squeezelite-proto/src/lib.rs:482` onward: ten associated consts mirroring the
  module-level `OPCODE_*`, unused. Keep `OPCODE_*`.

### Phase 5 — host-only or not wired in (lowest priority)

**T25 — Vorbis/Opus/AAC allocation and copy churn** · P4 · M · host-only
- Vorbis/Opus: `Vec::new()` per page, `core::mem::take(&mut self.partial_packet)`
  drops capacity per packet, `packet_queue: VecDeque<Vec<u8>>` (lib.rs Vorbis
  ~3974-4009, Opus ~4813-4848). Decode in-page packets straight from
  `stream_buf`, keep `partial_packet` capacity with `.clear()`, replace the
  queue with one byte buffer + fixed `(off, len)` ring.
- AAC MP4 (~3410-3422): split direct/pending output like the ADTS path,
  `reserve` before pushing.

**T26 — cspot crypto when Spotify gets wired in** · P4 · M · not wired
- `squeezelite-cspot/src/crypto.rs` AES-CTR is byte-wise (share the T09 core,
  forward T-tables). `cdn.rs` `feed_and_decode` (~256-268) decrypts the whole
  chunk but reports only the decoder's `consumed`, so re-fed bytes are decrypted
  twice — track a decrypted watermark. `shannon.rs:96/106` shifts both 16-word
  arrays per keystream word — use a circular index.

**T27 — `RtpJitterBuffer` per-packet allocation** · P4 · S · not wired
- `squeezelite-raop/src/rtp.rs` `push`/`push_pcm` (`to_vec()` per packet),
  `drain_ready`/`missing_sequences` return Vecs. Pre-allocate slots if it is
  ever used by the app.

### Deferred (blocked)

**D1 — Move decode + output to core 1** · biggest structural win · L
- Core 1 runs only a heartbeat (`app_core_loop`, `main.rs:1192`). Moving
  `pump_decode_paced` + `pump_output` there (via the existing SPSC ring split
  handles) would free core 0 for Wi-Fi/BT/network.
- Blocked on the core-1 owner hardware gate described in
  `louder-esp32-app/Cargo.toml` (feature `esp-rtos-core1`) and Invariant 47.
  Do not start without explicit go-ahead from the user.

---

## 4. Suggested order

1. T01, T02 (green baseline), then T03.
2. T06, T07, T08 (cheap, device-wide DSP wins) — measure.
3. T09 if AirPlay matters to the user, T10 next to it.
4. T11 → T12 (FLAC), T13 (EQ).
5. T04, T05, T14-T19 as capacity allows; T20-T27 as a cleanup batch.

Report back per task: commit hash, tests run (with any still-failing names),
whether gains were measured (`decode_us_max`, cycles, size) or estimated.
