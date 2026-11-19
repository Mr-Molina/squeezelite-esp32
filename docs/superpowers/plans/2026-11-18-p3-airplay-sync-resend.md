# P3 AirPlay RTP Sync, Jitter Reorder & Resend (`feat/airplay-sync-resend`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire AirPlay 1 (RAOP) UDP control (`6001`) and timing (`6002`) sockets, sequence-gap resend requests (`0x80 0xD5`) with `0x56`/`0xD6` retransmit recovery, `RtpJitterBuffer` out-of-order reordering, and NTP-to-local-clock `SyncDriftTracker` (`-1`/`0`/`+1` stereo frame stuff/drop) into `AirPlayService` (`rust/crates/louder-esp32-app/src/services/airplay.rs`), delivering underrun-clean, drift-bounded (`< 50 ms`) playback through `ServiceAudioHost` and verified on `DEVBENCH`.

**Architecture:** Anchored to the F4 `DeviceService` seam (`rust/crates/louder-esp32-app/src/services/mod.rs`) and `docs/mk5-rust-services.md:235, 273-277`.
- **Reuse of `squeezelite-raop` Primitives:** `squeezelite-raop/src/rtp.rs` and `rtsp.rs` already provide `RtpAudioReceiver`, `RtpJitterBuffer` (`push_packet`, `drain_ready_with`, `missing_sequences_into`, `advance_missing`, `flush`, `recycle_entry`), `RtpControlPacket` (`Sync(RtpSyncPacket)` `0x54` and `Retransmit { control_seq, inner_packet }` `0x56`), `RtpResendRequest` / `build_resend_request`, `NtpTimingPacket` (`0x52`/`0x53`), `SyncDriftTracker` (`record_ntp_timing`, `record_rtp_sync`, `compute_time_adjustment`, `apply_stereo_adjustment`), and `RtspServer` (`control_port() == 6001`, `timing_port() == 6002`, `client_control_port()`, `client_timing_port()`, `last_flush_seq()`, `last_flush_rtptime()`). P3 wires these into `AirPlayService` rather than reimplementing codecs.
- **Socket Budget (`2` New UDP Sockets within `SOCKET_SLOTS = 16`):** `AirPlayService` currently allocates `1` TCP (`5000` RTSP) and `1` UDP (`6000` RTP audio) socket (`src/services/airplay.rs:27-31, 328-350`). P3 adds `1` UDP control socket (`6001`, `cport`) and `1` UDP timing socket (`6002`, `tport`) via `ctx.add_socket(...)`, using `+2` slots per `docs/superpowers/plans/2026-11-17-parity-branch-distribution.md:468` and `docs/superpowers/specs/2026-11-17-parity-branch-distribution-design.md:183` (remaining well within `SOCKET_SLOTS = 16`).
- **PSRAM Heap Discipline:** `RtpJitterBuffer` (sized for RAOP buffering, e.g., `64` or `128` slots), `SyncDriftTracker`, `RtpAudioReceiver`, sender `IpEndpoint` / resend tracking state, scratch vectors (`missing_seqs: Vec<u16>`, `drift_pcm_scratch: Vec<i16>`), and all `smoltcp::socket::udp::PacketBuffer` metadata/payload slices **MUST** live on the PSRAM heap (`Box::new`, `Vec`, `Box::leak(...into_boxed_slice())`). Zero new `.data`, `.bss`, or `.dram2_uninit` statics, and zero `.iram1` / `#[ram]` placements.
- **Bounded Core 0 Superloop Execution:** `poll_rtp`, `poll_control`, and `poll_timing` cap per-lap packet drains (e.g., `for _ in 0..4` or `0..8`) and coalesce resend requests with rate limiting so Core 0 never stalls past the `16 KB` (`~46 ms`) I2S DMA window.
- **Host-Testable Coordinator Layer:** Extract a platform-free `AirPlaySyncPipeline` (inside `src/services/airplay.rs`, without `#[cfg(target_arch = "xtensa")]` guards on the state machine!) so `cargo test` exercises RTSP `SETUP`/`FLUSH`/`RECORD`/`TEARDOWN`, jitter reordering, missing-sequence resend emission, `0x56` retransmit ingestion, `0x52`/`0x53` NTP RTT measurement, `0x54` sync anchoring, and `SyncDriftTracker` frame stuff/drop feeding into `ServiceAudioHost`.

**Tech Stack:** Rust `no_std` (`alloc`, `bytemuck`, `smoltcp`), `squeezelite-raop`, host toolchain `1.92.0`, Espressif `esp` toolchain `1.97.0.0`, `devbench.py`, `check_section_budget.py`, `google_rust_audit.py`.

**Branch & Worktree Isolation:**
- Branch: `feat/airplay-sync-resend` created from post-F4 `MK5-Rust` (`foundation/service-seams` merged).
- Worktree: isolated git worktree on `MK5-Rust`; rebase onto `MK5-Rust` before `DEVBENCH` check and again before opening the PR (`gh pr create --repo Mr-Molina/squeezelite-esp32 --base MK5-Rust`).
- Never push directly to `MK5-Rust` and never merge.

**Hardware Lock & Safety Rules:**
- Acquire lock before flashing `DEVBENCH`: `python rust/scripts/devbench.py acquire feat/airplay-sync-resend` (release with `release` when finished).
- Pre-flash MAC check: `espflash board-info --port COM13` must equal `ac:15:18:fd:93:9c`. Never flash `BASEMENT` (`c8:2e:18:be:b8:34`) or `CYD` (`d4:d4:da:5a:e7:3c`).
- Never touch `BASEMENT` (`172.22.1.15`), `/dev/ttyUSB0` on `pc-1`, or `POST /reboot_ota.json`.
- At most 2 flash attempts per check (`devbench.py flash`). Build with `SL_DEVICE_NAME=DEVBENCH`.

**Memory & Socket Budget Gates (P3 Allowance = `1,148 B` DRAM, `0 B` IRAM, `2` UDP Sockets):**
- Sockets: `+2` UDP sockets (`6001` control + `6002` timing; `<= 16` `SOCKET_SLOTS`, leaving headroom even alongside P1/P2/P6).
- `IRAM` (`.vectors + .rwtext + .rwtext.wifi`): `<= 119,976 B` (Hard Invariant 48: `<= 131,072 B`; P3 IRAM allowance `0 B`).
- `.dram2_uninit`: `<= 89,048 B` (may raise `--max-dram2` / `--min-stack` defaults in `rust/scripts/check_section_budget.py` by at most `1,148 B` total DRAM only if needed, never beyond Invariant 48).
- `.stack`: `>= 41,256 B` (Hard Invariant 48: `>= 28,000 B`).
- Internal heap (`bring_up::HEAP + STATIC_BT_HEAP`): `>= 65,536 B` (zero allowance).

---

### Task 1: Build Platform-Free `AirPlaySyncPipeline` (Jitter Buffer + Resend Scheduler + NTP/Sync Drift) in `src/services/airplay.rs`

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/services/airplay.rs` (promote `RtpAudioReceiver` and add `AirPlaySyncPipeline` out of `#[cfg(target_arch = "xtensa")]` so all sync/resend/jitter logic runs under host `cargo test`)

**Context:** Currently in `src/services/airplay.rs:62-66, 284-302`, `raop_rtp: Box<RtpAudioReceiver>` is gated by `#[cfg(target_arch = "xtensa")]`, packets bypass `RtpJitterBuffer`, `RtspMethod::Flush` (`line 245`) flushes external audio buffers without forwarding `rtsp.last_flush_seq()`/`last_flush_rtptime()` to `RtpJitterBuffer::flush`, and `SyncDriftTracker` / `RtpResendRequest` / `NtpTimingPacket` are not instantiated.

- [ ] **Step 1: Write failing host unit tests in `src/services/airplay.rs` (`mod tests`)**

Add tests exercising the platform-free `AirPlaySyncPipeline`:
1. `airplay_jitter_reorders_out_of_order_packets_and_emits_resend_on_gap`:
   - Configure unencrypted or test-key ALAC `fmtp` (`"96 352 0 16 40 10 14 2 255 0 0 44100"`), push seq `10` (decoded PCM buffered), skip seq `11`, push seq `12` and `13`.
   - Call `collect_resend_requests(now_ms, &mut out_reqs)`: assert it emits `RtpResendRequest { missed_seq_start: 11, missed_count: 1, .. }` and rate-limits duplicate requests for seq `11` within the resend cooldown (e.g., `40 ms`).
   - Feed `0xD6` retransmit reply (`unwrap_retransmit_packet` layout: 4-byte `0x80 0xD6 <ctrl_seq>` + RTP packet seq `11`) via `ingest_control_packet(&pkt, now_ms)`.
   - Drain ready PCM frames (`drain_ready_pcm`): assert seq `10, 11, 12, 13` drain strictly in ascending sequence order (`recovered_packets() == 1`).
2. `airplay_flush_drops_stale_sequences_and_advances_unrecovered_gap`:
   - Buffer seq `20` and `22` (missing `21`). Advance time past max gap hold deadline -> `advance_missing` unblocks `22` without stalling playback indefinitely.
   - Apply `flush(25, 88_200)` -> clears sequences `< 25` cleanly.
3. `airplay_ntp_timing_and_sync_drive_drift_adjustment_into_audio_host`:
   - Generate timing request (`NtpTimingPacket::new_request_ms(1_000)`), feed `NtpTimingPacket::new_response(1_000, remote_ntp, remote_ntp)` at `local_now_ms = 1_012` (`RTT = 12 ms <= 100 ms`) -> `is_ntp_anchored() == true`.
   - Feed `RtpSyncPacket::new(true, 4, rtp_lat, ...)` -> `drift_tracker.is_synced() == true`.
   - Simulate positive/negative drift beyond `2 ms` window (`DEFAULT_DRIFT_WINDOW_MS = 2`) and verify `apply_stereo_adjustment` produces `-1` (drop 1 stereo frame) or `+1` (stuff 1 stereo frame) samples ingested cleanly into mock `ServiceAudioHost::ingest_external_pcm`.
4. `airplay_on_link_down_clears_jitter_resend_and_sync_state`:
   - Call `on_link(false)` -> verifies `jitter`, `resend` state, `drift_tracker`, and `sender_ip` are reset cleanly.

- [ ] **Step 2: Run tests and observe failure**

Run (from `rust/`):
```powershell
cargo test -p louder-esp32-app services::airplay::tests
```
Expected: FAIL (`AirPlaySyncPipeline` / methods do not exist yet).

- [ ] **Step 3: Implement `AirPlaySyncPipeline` in `src/services/airplay.rs`**

1. Make `raop_rtp: Box<RtpAudioReceiver>` and `sync_pipeline: Box<AirPlaySyncPipeline>` unconditional (PSRAM-heap allocated in `AirPlayService::new()`, zero `#[cfg(target_arch = "xtensa")]` on the platform-free state machine).
2. Inside `AirPlaySyncPipeline` (PSRAM-heap):
   - `jitter: RtpJitterBuffer` (`capacity = 64` or `128`, preallocated in `Box::new`)
   - `drift: SyncDriftTracker` (`RAOP_SAMPLE_RATE_HZ = 44_100`, `DEFAULT_DRIFT_WINDOW_MS = 2`)
   - `missing_scratch: Vec<u16>`, `pcm_adjusted_scratch: Vec<i16>`, `resend_reqs_sent: u64`, `next_resend_req_seq: u16`, `last_resend_ms: u64`, `last_timing_req_ms: u64`, `timing_req_interval_ms: u64` (`1_000 ms` initial / `3_000 ms` steady-state), `sender_ip: Option<[u8; 4]>`.
   - `ingest_audio_packet(&mut self, pkt: &[u8], now_ms: u64)`: decrypt + ALAC-decode via `RtpAudioReceiver::process_packet`, push decoded PCM into `RtpJitterBuffer::push_pcm(hdr, pcm)` (or store raw RTP payload/decoded PCM uniformly — `push_pcm` reuses slot `Vec<i16>` capacity without re-allocating!).
   - `ingest_control_packet(&mut self, pkt: &[u8], now_ms: u64)`: parse `RtpControlPacket::parse(pkt)` -> `Sync(sync)` calls `self.drift.record_ntp_timing` / `record_rtp_sync(&sync, now_ms)`; `Retransmit { inner_packet, .. }` passes `inner_packet` through `ingest_audio_packet`.
   - `ingest_timing_packet(&mut self, pkt: &[u8], now_ms: u64)`: parse `NtpTimingPacket::parse(pkt)` -> if `payload_type == RTP_PT_TIMING_RESPONSE`, call `self.drift.record_ntp_timing(&timing, now_ms as u32)`; if `payload_type == RTP_PT_TIMING_REQUEST`, build response packet cleanly.
   - `collect_resend_requests(&mut self, now_ms: u64, out: &mut Vec<RtpResendRequest>)`: call `self.jitter.missing_sequences_into(&mut self.missing_scratch)`, coalesce contiguous missing runs into `(missed_seq_start, missed_count)` `RtpResendRequest` packets, and rate-limit per cooldown (`>= 40 ms`) so control UDP is never flooded.
   - `drain_to_audio_host(&mut self, ctx: &mut ServiceCtx<'_>, now_ms: u64)`: if buffered depth >= startup prebuffer or gap deadline expired (`advance_missing()`), drain ready entries via `self.jitter.drain_ready_with`, apply `self.drift.compute_time_adjustment(entry.header.timestamp, now_ms)` + `SyncDriftTracker::apply_stereo_adjustment`, and feed `bytemuck::cast_slice(&self.pcm_adjusted_scratch)` into `ctx.ingest_external_pcm(...)` via `ServiceAudioHost`.
   - Wire `RtspMethod::Flush` -> `self.sync_pipeline.flush(seq, rtptime)` using `rtsp.last_flush_seq()` & `rtsp.last_flush_rtptime()`, plus `ctx.flush_external_buffers()`.
   - Update `status(&self, out: &mut StatusExtras)` to push `airplay_streaming` (`bool`), `airplay_resend_reqs` (`u64`), `airplay_recovered` (`u64`), `airplay_frames_stuffed` (`u64`), and `airplay_frames_dropped` (`u64`).

- [ ] **Step 4: Run host tests + strict Rust audit**

Run (from `rust/`):
```powershell
cargo test -p louder-esp32-app services::airplay::tests
python C:\Users\jmolina\.gemini\config\plugins\rust-pro\skills\rust-pro\scripts\google_rust_audit.py crates/louder-esp32-app/src --strict
```
Expected: All PASS, 0 audit violations.

- [ ] **Step 5: Commit**

```powershell
git add rust/crates/louder-esp32-app/src/services/airplay.rs
git commit -m "feat(airplay): add PSRAM-backed jitter reorder, resend scheduler, and NTP sync pipeline"
```

---

### Task 2: Wire `#[cfg(target_arch = "xtensa")]` UDP Control (`6001`) & Timing (`6002`) Sockets (`AirPlayService`)

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/services/airplay.rs`

**Context:** Extend `AirPlayService`'s `#[cfg(target_arch = "xtensa")]` transport section cleanly within `SOCKET_SLOTS = 16`:
- `AIRPLAY_CONTROL_PORT: u16 = 6001` (`rtsp.control_port()`)
- `AIRPLAY_TIMING_PORT: u16 = 6002` (`rtsp.timing_port()`)
- Both sockets allocate `PacketBuffer` metadata & payload slices on the PSRAM heap via `Box::leak` inside `DeviceService::init`.
- Capture the RTSP TCP peer IPv4 (`rtsp_sock.remote_endpoint()`) on `SETUP` / `RECORD` (or the RTP audio sender IP from `rtp_sock.recv_slice` `(n, ep)`) so outbound resend requests target `(sender_ip, rtsp.client_control_port())` and periodic NTP timing requests (`NtpTimingPacket::new_request_ms`) target `(sender_ip, rtsp.client_timing_port())`.

- [ ] **Step 1: Add `control_handle` and `timing_handle` UDP sockets in `AirPlayService::init` (`#[cfg(target_arch = "xtensa")]`)**

1. Allocate PSRAM `UdpControlBuffers` (e.g., `4096 B` RX / `512 B` TX, `rx_meta: 8`, `tx_meta: 4`) and `UdpTimingBuffers` (e.g., `512 B` RX / `512 B` TX, `rx_meta: 4`, `tx_meta: 2`) via `Box::leak`.
2. Call `ctx.add_socket(...)` for `control_handle` and `timing_handle` in `AirPlayService::init`.
3. In `reset_transport(ctx)`: close `control_handle` and `timing_handle` alongside `rtsp_handle` and `rtp_handle`, and reset `sync_pipeline`.

- [ ] **Step 2: Implement bounded `poll_control` (`6001`), `poll_timing` (`6002`), and updated `poll_rtp` (`6000`)**

1. `poll_control(ctx)`:
   - Bind `AIRPLAY_CONTROL_PORT` (`6001`) if closed.
   - Drain up to `4` datagrams (`for _ in 0..4`), passing each to `self.sync_pipeline.ingest_control_packet(&pkt_buf[..n], now_ms)`.
   - Collect pending `RtpResendRequest` items and transmit (`8`-byte `.to_bytes()`) to `(sender_ip, client_control_port)` if `client_control_port != 0` and `control_sock.can_send()`.
2. `poll_timing(ctx)`:
   - Bind `AIRPLAY_TIMING_PORT` (`6002`) if closed.
   - Drain up to `4` datagrams (`for _ in 0..4`), passing `0x53` responses to `ingest_timing_packet` and answering any `0x52` timing requests if received.
   - Periodically (every `1_000 ms` while `raop_streaming` and `client_timing_port != 0`) send `NtpTimingPacket::new_request_ms(now_ms as u32).to_bytes()` to `(sender_ip, client_timing_port)`.
3. `poll_rtp(ctx)`:
   - Drain up to `8` RTP audio packets (`for _ in 0..8`) into `self.sync_pipeline.ingest_audio_packet`, then call `self.sync_pipeline.drain_to_audio_host(ctx, now_ms)` to claim `AudioSource::AirPlay` (`44_100 Hz`, `AmpPower::EnsurePlay`) and feed jitter-ordered, drift-compensated PCM into `ServiceAudioHost`.
   - Preserve `RTP_IDLE_TIMEOUT_MS = 3_000` release guard (`!ctx.has_external_audio_queued()`).

- [ ] **Step 3: Run all 8 CI host & firmware gates + section budget + `google_rust_audit.py --strict`**

Run (from `rust/` and repo root):
```powershell
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo +esp clippy -p louder-esp32-app -p cyd-controller-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc -- -D warnings
cargo +esp check -p louder-esp32-app --features esp-rtos-core1 --target xtensa-esp32-none-elf -Zbuild-std=core,alloc
cargo +esp build --release -p louder-esp32-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc
python scripts/check_section_budget.py target/xtensa-esp32-none-elf/release/louder-esp32-app
```
And from repo root:
```powershell
python -m unittest discover -s rust/scripts -p "test_*.py"
python C:\Users\jmolina\.gemini\config\plugins\rust-pro\skills\rust-pro\scripts\google_rust_audit.py rust/crates/louder-esp32-app/src --strict
```
*(If `check_section_budget.py` fails solely on `.dram2_uninit` or `.stack` by `<= 1,148 B` — P3's DRAM allowance from `docs/superpowers/specs/2026-11-17-parity-branch-distribution-design.md:183` — adjust `--max-dram2` / `--min-stack` defaults in `rust/scripts/check_section_budget.py` by at most `1,148 B` in the same PR and document the delta in the work-order file. Never raise `--max-iram` or lower `--min-heap`.)*

- [ ] **Step 4: Commit**

```powershell
git add rust/crates/louder-esp32-app/src/services/airplay.rs
git commit -m "feat(airplay): wire Xtensa UDP control 6001 and timing 6002 sockets with PSRAM buffers"
```

---

### Task 3: Live `DEVBENCH` 10-Minute AirPlay Soak & Resend/Drift Verification

**Files:**
- Create: `.agents/knowledge/work_orders/WO-P3-airplay-sync-resend.md`
- Modify: `.agents/sessions.md`, `docs/mk5-rust-services.md`, `docs/mk5-rust-todo.md`

- [ ] **Step 1: Rebase onto `MK5-Rust` and re-run all 8 CI + ELF gates**

```powershell
git fetch origin
git rebase origin/MK5-Rust
```
Re-run all 8 gates from Task 2 Step 3.

- [ ] **Step 2: Acquire `DEVBENCH` lock and verify MAC (`ac:15:18:fd:93:9c`)**

```powershell
python rust/scripts/devbench.py acquire feat/airplay-sync-resend
espflash board-info --port COM13
```
Confirm MAC is `ac:15:18:fd:93:9c`. Abort immediately if `c8:2e:18:be:b8:34` (`BASEMENT`) or `d4:d4:da:5a:e7:3c` (`CYD`). Never touch `BASEMENT` (`172.22.1.15`).

- [ ] **Step 3: Build `DEVBENCH` release image and flash via `devbench.py` (max 2 attempts)**

```powershell
$env:SL_DEVICE_NAME = 'DEVBENCH'
Push-Location rust
cargo +esp build --release -p louder-esp32-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc
Pop-Location
python rust/scripts/devbench.py flash feat/airplay-sync-resend rust/target/xtensa-esp32-none-elf/release/louder-esp32-app
```
Verify serial boot reaches `[ANTI-BRICK GATE 1]`, DHCP lease, and `"boot: ota_0 confirmed healthy; otadata pinned to ota_0"`.

- [ ] **Step 4: Run live 10-minute Apple sender AirPlay soak & packet-loss resend test on `DEVBENCH`**

1. Stream from an Apple sender (owner supplies) to `DEVBENCH` over AirPlay for `10` minutes continuously:
   - Verify no audible dropouts, no Core 0 watchdog resets, and clock drift stays `< 50 ms` (`airplay_frames_stuffed` / `airplay_frames_dropped` bounded in `/status.json`).
2. Induce or observe UDP packet loss on the test stream (or replay a synthetic gap-dropped stream) and verify resend requests (`0x80 0xD5`) are emitted and logged (`airplay_resend_reqs > 0`, `airplay_recovered > 0` in `/status.json` / serial log).
3. Verify `TEARDOWN` / idle timeout cleanly releases `AudioSource::AirPlay` and restores any suspended source without DMA glitches.
4. Release lock: `python rust/scripts/devbench.py release feat/airplay-sync-resend`.

- [ ] **Step 5: Record evidence, update docs/sessions, commit, and open PR**

Save serial log, `/status.json` telemetry, and `check_section_budget.py` output in `.agents/knowledge/work_orders/WO-P3-airplay-sync-resend.md`. Prepend session entry in `.agents/sessions.md` and update parity status in `docs/mk5-rust-todo.md` to `"Wired"` (never `"Certified"`). Commit and open PR against `MK5-Rust`:
```powershell
git add .agents/knowledge/work_orders/WO-P3-airplay-sync-resend.md .agents/sessions.md docs/mk5-rust-services.md docs/mk5-rust-todo.md
git commit -m "docs(p3): record DEVBENCH 10-minute AirPlay sync and resend soak verification"
git push -u origin feat/airplay-sync-resend
gh pr create --repo Mr-Molina/squeezelite-esp32 --base MK5-Rust --head feat/airplay-sync-resend
```

---

## Verification Checklist (Pre-PR Promotion)
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes (including `services::airplay::tests`, `squeezelite-raop` tests, `tests/service_registry_invariant.rs`, `tests/no_cfg_invariant.rs`, `tests/memory_budget_invariant.rs`)
- [ ] `python -m unittest discover -s rust/scripts -p "test_*.py"` passes
- [ ] `cargo +esp clippy -p louder-esp32-app -p cyd-controller-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc -- -D warnings` passes
- [ ] `cargo +esp check -p louder-esp32-app --features esp-rtos-core1 --target xtensa-esp32-none-elf -Zbuild-std=core,alloc` passes
- [ ] `cargo +esp build --release -p louder-esp32-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc` + `python scripts/check_section_budget.py target/xtensa-esp32-none-elf/release/louder-esp32-app` passes (`IRAM <= 119,976 B`, `.dram2_uninit <= 89,048 B` or `<= 90,196 B` within P3's `1,148 B` DRAM allowance, `.stack >= 41,256 B`, `internal heap >= 65,536 B`)
- [ ] `python C:\Users\jmolina\.gemini\config\plugins\rust-pro\skills\rust-pro\scripts\google_rust_audit.py rust/crates/louder-esp32-app/src --strict` passes (0 violations)
- [ ] Live `DEVBENCH` (`COM13`, MAC `ac:15:18:fd:93:9c`) check passed (`BASEMENT` untouched): 10-minute AirPlay stream with no audible dropouts, drift `< 50 ms`, resend requests visible when packets drop; lock released.
