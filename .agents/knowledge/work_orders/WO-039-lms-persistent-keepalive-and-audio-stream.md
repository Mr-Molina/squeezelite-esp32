# Work Order: WO-039 — Wire Persistent LMS SlimProto Keepalive & `smoltcp` HTTP Audio Stream Socket (`--stage-only`)

**Work Order ID:** WO-039  
**Timestamp:** 2026-10-24T11:08:00-04:00  
**Assigned Subagent Role:** Rust App & Hardware Implementer (`Model: "flash"`, `TypeName: "self"`)  
**Target Files:**
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
- `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`
**Backup Files:**
- `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_coordinator.rs.bak`
- `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_main.rs.bak`
**Defect IDs:** Persistent LMS Session & Audio Streaming (`CloseWait` Reconnect, Multi-Frame Drain, `setd` `"BASEMENT"` Name Reply, `strm` `'t'` Timestamp Echo, `smoltcp` HTTP Audio Stream Socket)  
**Task Goal:** Update `src/coordinator.rs` and `src/main.rs` (`xtensa_hw`) so that `louder-esp32-app` maintains a persistent Lyrion Music Server (`10.0.0.10:3483`) session and streams live HTTP PCM audio (`10.0.0.10:9000`) through `StreamPipeline` -> `Esp32HardwareI2sDma`, verify all host tests, Clippy, and `xtensa-esp32-none-elf` release compilation, and package + audit the `--stage-only` OTA binary `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`.  

---

## 1. Context & Scope
- **Problem Statement:** During the 1h56m Gate 5B soak on `172.22.1.15`, three specific gaps in the `xtensa_hw` SlimProto socket loop (`src/main.rs:954-1017`) caused LMS (`10.0.0.10:3483`) to drop `"BASEMENT"` after initial registration:
  1. **Half-Closed `smoltcp` TCP State**: When `10.0.0.10:3483` sends a TCP `FIN`, `slim_sock` enters `tcp::State::CloseWait` where `slim_sock.is_open()` remains `true` (`!slim_sock.may_recv()`). Checking only `if !slim_sock.is_open()` prevented reconnecting.
  2. **Multi-Frame Coalesced TCP Segments & `setd` Player Name**: LMS sends `vers`, `setd` (`id=0` 1-byte query `[0x00]`), `audg`, and `strm` in coalesced TCP segments. `src/main.rs` parsed only one frame per `recv_slice` and did not send `SETD` `"BASEMENT"` (`SlimprotoBuilder::build_setd_name`). Additionally, in `src/coordinator.rs:452`, `ServerCommand::SetDevice { sample_rate }` matched 1-byte `setd` `[0x00]` (`sample_rate == 0`) before checking `cmd.device_setting()`.
  3. **LMS `strm` `'t'` Timestamp Echo & HTTP Audio Stream Socket**: On `StreamCommandType::Timestamp`, LMS expects `replay_gain` echoed back in `server_timestamp` (`StatBuilder::build_stat_with_metrics`), and on `StreamCommandType::Start` (`strm` `'s'`), LMS expects the player to open an HTTP TCP connection to `server_ip:server_port` (`10.0.0.10:9000`), send `http_headers`, forward `RESP` headers (`SlimprotoBuilder::build_resp`), and stream PCM audio into `StreamPipeline`.
- **Single Variable Constraint (Rule 5):** Modify only `src/coordinator.rs` and `src/main.rs` and stage the updated `.bin` (`--stage-only`). Do NOT send any HTTP request to `172.22.1.15` during this Work Order.
- **Pre-Modification Integrity (Rule 31, Rule 35):** Back up `src/coordinator.rs` to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_coordinator.rs.bak` before editing.
- **Prohibited Actions (Rule 3, Rule 23):** Never touch `/dev/ttyUSB0`. Never send any mutating HTTP request to `172.22.1.15`. Never use `sed`, `awk`, or `cat` for file modifications.

---

## 2. Hypothesis & Predicted Outcome (Rule 1, Rule 6)
- **Hypothesis:** Wiring an accumulation buffer (`slim_acc_buf`) with a `while` loop draining all complete `SlimprotoParser::parse_frame` frames, replying to `setd` (`id=0`) with `SlimprotoBuilder::build_setd_name("BASEMENT", ...)` (plus sending initial `SETD` `"BASEMENT"` right after `HELO`), echoing `replay_gain` via `StatBuilder::build_stat_with_metrics` on `StreamCommandType::Timestamp`, aborting/reconnecting `slim_sock` when `!slim_sock.may_recv()` in an established/closing state, and adding a 4th `smoltcp` socket (`audio_sock`) for `strm` `'s'` HTTP PCM streaming will compile cleanly on both host and `xtensa-esp32-none-elf` and pass `verify_staged_ota_bin.py`.
- **Predicted Outcome:** `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `rustup run esp cargo build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`, `espflash save-image`, and `verify_staged_ota_bin.py` all exit `0`.
- **Falsification Criteria:** Any test failure, Clippy warning, or `xtensa` build error falsifies the implementation.

---

## 3. Detailed Instructions
1. Back up `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs` to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_coordinator.rs.bak`.
2. In `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs`:
   - In `ServerCommand::SetDevice { sample_rate }` (around line 452), check `if let Some(setting) = cmd.device_setting()` when `sample_rate <= u8::MAX as u32`: if `setting.id == 0`, call `client.send_setd_name(&self.player_name)?;` and return `Ok(None)` without overwriting `self.current_sample_rate` with `0`.
   - Add a unit test verifying that a 1-byte `setd` `[0x00]` query triggers a `SETD` player name response without setting `current_sample_rate` to `0`.
3. In `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (`xtensa_hw` module):
   - Import `squeezelite_decoders::PcmDecoder` and `squeezelite_proto::{sample_rate_from_index, AudioFormat, AutostartMode, DisconnectReason, Endianness, ParseError, StatBuilder}` inside `xtensa_hw`.
   - Expand `socket_storage` from 3 to 4 sockets (`dhcp_handle`, `http_handle`, `slim_handle`, `audio_handle` with `audio_rx_buf = [0u8; 4096]` and `audio_tx_buf = [0u8; 1024]`).
   - In the `slim_sock` handler:
     - Detect half-closed / closing states: if `slim_sock.is_open()` and `matches!(slim_sock.state(), tcp::State::CloseWait | tcp::State::TimeWait | tcp::State::Closing | tcp::State::LastAck | tcp::State::FinWait1 | tcp::State::FinWait2)`, call `slim_sock.abort()` and set `helo_sent = false`.
     - When `slim_sock.may_send()` and `!helo_sent`:
       - Send `SlimprotoBuilder::build_helo(&BASEMENT_PLAYER_MAC, 12, "Model=SqueezeESP32,ModelName=SqueezeESP32,AccuratePlayPoints=1,HasDigitalOut=1,MaxSampleRate=48000,pcm", &mut helo_buf)`.
       - Immediately also send `SlimprotoBuilder::build_setd_name(coordinator.player_name.as_str(), &mut setd_buf)` (`"BASEMENT"`) and `StatBuilder::build_stat_with_metrics(StatEvent::Heartbeat, ...)` so LMS immediately registers the player name `"BASEMENT"` and initial status.
     - Use a persistent accumulation buffer `slim_acc_buf = [0u8; 1536]` (`slim_acc_len`) to read from `slim_sock.recv_slice(&mut slim_acc_buf[slim_acc_len..])` and drain all complete frames in a `while slim_acc_len >= 6` loop using `SlimprotoParser::parse_frame(&slim_acc_buf[..slim_acc_len])`:
       - Check `if let Some(setting) = cmd.device_setting()` FIRST: if `setting.id == 0`, update `coordinator.player_name` if `!setting.data.is_empty()` (trimming trailing `\0`), and send `SlimprotoBuilder::build_setd_name(coordinator.player_name.as_str(), &mut tx_buf)`.
       - Handle `ServerCommand::AudioGain { gain_left, gain_right, digital_atten, .. }`: compute effective gain and call `pipeline.set_volume(eff_gain_left, eff_gain_right)`.
       - Handle `ServerCommand::Stream`:
         - `StreamCommandType::Timestamp` (`strm` `'t'`): immediately send `StatBuilder::build_stat_with_metrics(StatEvent::Heartbeat, pipeline.stream_buffer_available() as u32, pipeline.output_buffer_available() as u32, pipeline.frames_played, coordinator.current_sample_rate, coordinator.stream_bytes_received, now_ms as u32, replay_gain, &mut stat_buf)`.
         - `StreamCommandType::Start` (`strm` `'s'`): flush `pipeline`, configure `PcmDecoder` (bits, channels, endianness) and sample rate (`PlayerCoordinator::decode_pcm_sample_rate_hz(pcm_sample_rate).unwrap_or_else(|| sample_rate_from_index(pcm_sample_rate))`), store `http_headers` into `audio_req_buf` (`[u8; 512]`), abort and connect `audio_sock` to `(server_ip_or_default, server_port)`, send `StatEvent::Connected` (`STMc`), set `coordinator.state = PlayerState::Playing`, and call `let _ = amp.start_playback_via_hiz();`.
         - `StreamCommandType::Pause`: set `coordinator.state = PlayerState::Paused`, `pipeline.sink.set_mute(true)`, send `StatEvent::TrackPaused` (`STMp`).
         - `StreamCommandType::Resume`: set `coordinator.state = PlayerState::Playing`, `pipeline.sink.set_mute(false)`, send `StatEvent::TrackResumed` (`STMr`).
         - `StreamCommandType::Stop`: set `coordinator.state = PlayerState::Stopped`, abort `audio_sock`, `pipeline.sink.set_mute(true)`, send `StatEvent::TrackStopped` (`STMq`).
         - `StreamCommandType::Flush`: abort `audio_sock`, `pipeline.flush()`, send `StatEvent::StreamFlushed` (`STMf`).
       - Handle `ServerCommand::Flush`: abort `audio_sock`, `pipeline.flush()`, send `StatEvent::StreamFlushed` (`STMf`).
       - Handle `ServerCommand::Unknown`: check `cmd.audio_enable()` and `cmd.stream_continue()`.
   - Drive `audio_sock` on each loop iteration when `audio_stream_active` is `true`:
     - Once `audio_sock.may_send()` and `!audio_req_sent`: send `&audio_req_buf[..audio_req_len]` and set `audio_req_sent = true`.
     - While `audio_sock.can_recv()` and `pipeline.stream_buffer.available_write() >= 512`:
       - Read up to `512` bytes from `audio_sock`.
       - Strip HTTP response headers up to `\r\n\r\n`, send `SlimprotoBuilder::build_resp` + `StatEvent::TrackStarted` (`STMs`) on `slim_sock`, and push audio body bytes into `pipeline.ingest_stream_data(...)`, incrementing `coordinator.stream_bytes_received`.
     - When `audio_req_sent` and `!audio_sock.may_recv()` (HTTP stream EOF):
       - Abort `audio_sock`, set `audio_stream_active = false`, and send `StatEvent::DecodeReady` (`STMd`) + `SlimprotoBuilder::build_dsco(DisconnectReason::ConnectionClosed, ...)` on `slim_sock`.
4. Run all verification commands:
   - `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace`
   - `/home/console-admin/.cargo/bin/cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets -- -D warnings`
   - `/bin/bash -c "source /home/console-admin/export-esp.sh && /home/console-admin/.cargo/bin/rustup run esp cargo build --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app"`
   - `/home/console-admin/.cargo/bin/espflash save-image --chip esp32 /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`
   - `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_staged_ota_bin.py`
5. Commit your changes to `MK5-Rust` (`git commit -m "feat(louder-esp32-app): wire persistent LMS multi-frame keepalive, setd name reply, and smoltcp HTTP audio stream"`), write a Rule 17 subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_111000_wo039_lms_stream.md`, and complete the Fulfillment Section below.

---

## 4. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Execution Timestamp:** 2026-10-24T11:24:00-04:00
- **Files Modified / Created (Absolute Paths):**
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs` (Modified)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (Modified)
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (Generated & Audited)
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_111000_wo039_lms_stream.md` (Created)
- **Backup Verification:**
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_coordinator.rs.bak` (Verified present: 86,871 bytes)
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_main.rs.bak` (Verified present: 48,849 bytes)
- **Test / Verification Command & Results:**
  - `cargo test --manifest-path rust/Cargo.toml --workspace` -> Exit code 0, 178+ tests passed
  - `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets -- -D warnings` -> Exit code 0, 0 warnings
  - `rustup run esp cargo build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app` -> Exit code 0, 0 warnings
  - `espflash save-image --chip esp32 ... louder-esp32-app-basement-ota.bin` -> Exit code 0 (678,176 bytes)
  - `python3 .../verify_staged_ota_bin.py` -> Exit code 0, SHA-256 `63b8c46cde45f7cf72de1bb70ab1b725b70d1d2b4210f241c16846005a0b7783`
- **Observed Outcome vs Predicted Outcome:** All verification steps matched hypothesis exactly (exit code 0 across all 5 verification commands).
- **Fulfillment Status:** `[EXECUTED & VERIFIED]`
- **Summary Path:** `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_111000_wo039_lms_stream.md`
- **Handoff Path:** [WO-039-lms-persistent-keepalive-and-audio-stream.md](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-039-lms-persistent-keepalive-and-audio-stream.md)
