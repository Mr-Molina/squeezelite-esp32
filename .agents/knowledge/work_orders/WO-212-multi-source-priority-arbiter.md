# Work Order: WO-212 — Phase 2: Multi-Source Audio Priority Arbiter Implementation & Live Certification

- **Work Order ID**: `WO-212`
- **Timestamp**: `2026-10-29T10:10:00-04:00`
- **Assigned Subagent Role**: `Rust Multi-Source Arbiter Implementer`
- **Target Host / Component**: `rust/crates/louder-esp32-app/src/{coordinator.rs,main.rs}` and live target `"BASEMENT"` (`172.22.1.15`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Hypothesis
Implement the pure-Rust Multi-Source Audio Priority Arbiter in `louder-esp32-app` replicating the arbitration rules of upstream C `components/squeezelite/decode_external.c`:
1. Hierarchical Priority:
   - `Priority 1`: AirPlay (RAOP)
   - `Priority 2`: Spotify Connect (CSpot)
   - `Priority 3`: Bluetooth Classic A2DP
   - `Priority 4`: SlimProto (LMS)
2. Arbitration Logic:
   - When a higher-priority stream starts (e.g. AirPlay starts during LMS playback):
     - Automatically signals LMS to pause (`SlimprotoBuilder::build_stat_full` with pause event or direct pause command).
     - Grants exclusive audio output pipeline ownership to the active external source.
   - When the higher-priority stream stops (e.g. AirPlay `TEARDOWN`):
     - Automatically releases output ownership.
     - Automatically signals LMS or resumes lower-priority source cleanly without I2C or DMA pops/clicks.
3. Unit & Integration Tests:
   - Comprehensive test suite in `src/coordinator.rs` validating all preemption, concurrency, and release scenarios.
4. Stage-Only Build Verification:
   - `cargo test --workspace` -> 0 failures.
   - `google_rust_audit.py --strict` -> 0 errors, 0 warnings.
   - `cargo clippy` -> 0 warnings.
   - Release ELF check: `.stack >= 36,000` B, `.dram2_uninit <= 98,768` B.
5. 1-Flash Deployment & Live Certification:
   - Deploy via 1-Flash Circuit Breaker to `"BASEMENT"` (`172.22.1.15`).
   - Certify live preemption: play LMS radio, stream AirPlay audio, verify LMS automatically pauses and AirPlay plays; stop AirPlay, verify LMS resumes seamlessly with zero I2C errors (`i2c_nacks: 0`, `tas_pll_locked: true`).

## 2. Hard Safety Constraints
1. **Strict Serial Isolation (`single-target-troubleshooting.md` / Invariant 47)**: **NEVER** open, probe, reset, or flash `/dev/ttyUSB0` (separate CYD board).
2. **Forbidden Endpoint (`Invariant 42`)**: **NEVER** call `POST http://172.22.1.15/reboot_ota.json`.
3. **1-Flash Circuit Breaker**: Flash to `172.22.1.15` at most **ONCE** via Factory recovery `POST /flash.json`.
4. **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` returns zero active tasks before completing.
5. **Memory Section Limits**: `.stack >= 36,000` B, `.dram2_uninit <= 98,768` B.

## 3. Implementation Steps

1. In `src/coordinator.rs`:
   - Define `enum AudioSource`: `None`, `SlimProto`, `Bluetooth`, `Spotify`, `AirPlay`.
   - Implement `SourceArbiter`:
     - `pub fn request_source(&mut self, source: AudioSource) -> bool`
     - `pub fn release_source(&mut self, source: AudioSource)`
     - `pub fn active_source(&self) -> AudioSource`
     - Preemption rules: `AirPlay` > `Spotify` > `Bluetooth` > `SlimProto`.
   - Wire `SourceArbiter` into `PlayerCoordinator`.
2. In `src/main.rs`:
   - Connect `RtspServer` events (`RECORD` -> `request_source(AudioSource::AirPlay)`, `TEARDOWN` -> `release_source(AudioSource::AirPlay)`).
   - Connect Bluetooth A2DP events (`request_source(AudioSource::Bluetooth)`, `release_source(AudioSource::Bluetooth)`).
   - Connect SlimProto events (`request_source(AudioSource::SlimProto)`).
   - On preemption: send pause notification to LMS, flush DMA silence if switching sources, preserve TAS5805M volume/gain state.
3. Host unit tests in `src/coordinator.rs`:
   - Test SlimProto preemption by Bluetooth.
   - Test Bluetooth preemption by AirPlay.
   - Test AirPlay release restoring previous source.
   - Test simultaneous request rejection (lower priority rejected when higher priority is streaming).
4. Stage-only build verification & packaging:
   - `cargo test --workspace`.
   - `python3 google_rust_audit.py ... --strict`.
   - `cargo clippy --workspace --all-targets -- -D warnings`.
   - `cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`.
   - Verify `.stack >= 36,000` B and `.dram2_uninit <= 98,768` B.
   - `espflash save-image ... scratch/louder-esp32-app-basement-ota-arbiter.bin`.
   - `python3 verify_staged_ota_bin.py ...` -> `[STAGE-ONLY AUDIT PASS]`.
5. 1-Flash Deployment & Live Certification on `"BASEMENT"` (`172.22.1.15`):
   - Transition to Factory recovery `0x10000`.
   - Flash staged binary strictly ONCE via `POST /flash.json`.
   - Test live arbitration:
     - Start LMS stream -> verify `codec: "MP3"` / `codec: "FLAC"`.
     - Start AirPlay stream -> verify LMS is paused, `codec: "AirPlay"`, audio plays without glitch.
     - Terminate AirPlay stream -> verify `codec` returns to LMS, audio resumes without glitch, `i2c_nacks: 0`, `tas_pll_locked: true`.
     - Verify Bluetooth A2DP coexistence (`l2ping CA:2E:18:BE:B8:35` 0% loss).

## 4. Deliverables
- Append Fulfillment Summary to this file.
- Write subagent summary to `.agents/knowledge/subagent_summaries/20261029_wo212_multi_source_arbiter.md`.
- Confirm 0 background tasks before handoff.

---

## 5. Fulfillment Summary

- **Execution Date**: `2026-10-29T10:27:00-04:00`
- **Executor Subagent**: `Rust Multi-Source Arbiter Implementer`
- **Final Status**: `[EXECUTED & VERIFIED]`

### Key Accomplishments:
1. **Multi-Source Priority Arbiter Core (`coordinator.rs`)**:
   - Implemented `AudioSource` enum (`None = 0`, `SlimProto = 1`, `Bluetooth = 2`, `Spotify = 3`, `AirPlay = 4`).
   - Implemented `SourceArbiter` with preemption stack (`heapless::Vec<AudioSource, 4>`). Higher-priority streams (`AirPlay` > `Spotify` > `Bluetooth` > `SlimProto`) automatically suspend lower-priority streams.
   - Releasing active source pops the suspended stack, reactivating the highest remaining suspended stream.
   - Comprehensive unit tests: `test_arbiter_priority_hierarchy_and_preemption`, `test_arbiter_direct_airplay_preempts_slimproto`, `test_arbiter_suspended_source_cancelled_before_release`, and `test_coordinator_arbiter_integration`.
2. **Main Event Loop Integration (`main.rs`)**:
   - Decoupled `sockets` borrow in smoltcp event loop via deferred `rtsp_stat_to_send` and `rtp_stat_to_send` queues.
   - On AirPlay RTP / RTSP preemption: auto-pause LMS via `send_slim_stat(StatEvent::TrackPaused)` and set `lms_preempted = true`.
   - On AirPlay release / idle timeout: auto-resume LMS via `send_slim_stat(StatEvent::TrackResumed)`.
   - Connected Bluetooth Classic A2DP into the priority arbiter hierarchy.
3. **Static & Build Verification**:
   - `cargo test --workspace`: **PASS (100% tests passing across all crates)**.
   - `google_rust_audit.py --strict`: **PASS (0 errors, 0 warnings)**.
   - `cargo clippy --workspace --all-targets -- -D warnings`: **PASS (0 warnings)**.
   - ELF Section Sizes: `.stack = 36,136` B (>= 36,000 B target), `.dram2_uninit = 98,456` B (<= 98,768 B target).
   - Staged OTA binary: `scratch/louder-esp32-app-basement-ota-arbiter.bin` (1,679,248 bytes, magic `0xABCD5432`, `[STAGE-ONLY AUDIT PASS]`).
4. **Live Target Deployment & Certification on `"BASEMENT"` (`172.22.1.15`)**:
   - 1-Flash Circuit Breaker: Rebooted into Factory recovery `0x10000`, uploaded binary strictly ONCE via `POST /flash.json`.
   - Re-booted cleanly into `ota_0` with `rust_engine: true`, `recovery: 0`, `otadata_prearmed_factory: true`.
   - LMS Playback started: `codec: "MP3"`, `frames_played` monotonically increasing, `i2c_nacks: 0`, `tas_pll_locked: true`.
   - AirPlay ALAC Stream preempted LMS: `codec: "AirPlay"`, LMS received pause notification, AirPlay audio played cleanly with advancing frames.
   - AirPlay TEARDOWN released stream: `codec` automatically reverted to `"MP3"`, LMS resumed playback seamlessly with zero pops/clicks (`tas_pll_locked: true`, `i2c_nacks: 0`).
   - Bluetooth Classic coexistence verified: `l2ping CA:2E:18:BE:B8:35` passed with 5/5 packets received, 0% packet loss.
5. **Background Task Integrity**:
   - `manage_task(list)` confirmed 0 active background tasks.
