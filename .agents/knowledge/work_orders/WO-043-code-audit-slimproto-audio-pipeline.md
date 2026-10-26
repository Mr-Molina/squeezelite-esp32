# Work Order: WO-043 — Deep Code & Protocol Audit: SlimProto `strm`, LMS Codec Negotiation, `audio_sock` HTTP Stream & `frames_played == 512` (`Micro-Auditor 3`)

**Work Order ID:** WO-043  
**Timestamp:** 2026-10-24T12:17:30-04:00  
**Assigned Subagent Role:** SlimProto & Audio Stream Pipeline Auditor (`Model: "pro"`, `TypeName: "research"`)  
**Target Files (Read-Only):**
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs`
**Task Goal:** Perform a zero-tolerance `/deep-code-audit` and live LMS read-only telemetry inspection to determine why `frames_played` on `172.22.1.15` (`"BASEMENT"`) stopped at **exactly `512`** and why no continuous audio stream played.

---

## 1. Context & Scope
- Live target `172.22.1.15` (`"BASEMENT"`, MAC `c8:2e:18:be:b8:34`) is running pure-Rust `louder-esp32-app` (`d445a736`).
- After the user attempted to play audio, `GET http://172.22.1.15/status.json` reported:
  `"frames_played": 512, "amp_mode": "Play", "i2c_acks": 18966497, "i2c_nacks": 0, "uptime_ms": 2580008`.
- **Prohibited Actions (Rule 3):** NEVER touch or query `/dev/ttyUSB0`. Do NOT send any mutating requests to `172.22.1.15` or `10.0.0.10`. Read-only JSON-RPC status queries to `10.0.0.10:9000` are permitted.

---

## 2. Specific Audit Questions to Answer with File + Line Citations
1. **Why did `frames_played` advance to `512` and stop?**
   - Trace every line of `src/main.rs`, `src/coordinator.rs`, and `src/pipeline.rs` involved in `strm` `'s'`, `audio_sock`, `pipeline.ingest_stream_data`, `run_firmware_tick`, `coordinator.process_audio_step`, and `pipeline.step_decode_and_output`.
   - Look closely at:
     - What is the size of `stream_storage` and `output_storage` in `src/main.rs`? (`[0u8; 4096]`?!)
     - What happens when `run_firmware_tick` runs on each loop iteration in `src/main.rs`?!
     - Look at `run_firmware_tick` in `src/main.rs` around line 340–410! What does `run_firmware_tick` pass as `server_command: Option<ServerCommand>` when called from the `xtensa_hw` main loop (`run_firmware_tick(&mut coordinator, &mut pipeline, &mut amp, &mut slim_client, None, now_ms)`)?!
     - And inside `run_firmware_tick` / `coordinator.process_audio_step(&mut pipeline)`:
       - When does `coordinator.process_audio_step` decode audio? Only when `coordinator.state == PlayerState::Playing`?
       - Look at what `coordinator.process_audio_step(&mut pipeline)` does when `pipeline.stream_buffer.is_empty()` or when `output_buffer` drains or when `stream_finished` is true/false!
       - Wait: what happens in `audio_sock` in `src/main.rs` right after `audio_sock` connects (`audio_req_sent = false`)?!
       - Look at lines 1135–1145 of `src/main.rs`:
         ```rust
         if !audio_req_sent && audio_sock.may_send() { ... audio_req_sent = true; }
         if audio_req_sent && !audio_sock.may_recv() {
             audio_sock.abort();
             audio_stream_active = false;
             ...
         }
         ```
         WAIT A SECOND! In `smoltcp`, when `audio_sock.connect(...)` is called, what is `audio_sock.may_recv()` while TCP handshake (`SynSent`) is in progress vs after `Established`, AND what happens when `audio_sock` finishes receiving or when `stream_buffer` (`4096` bytes) fills up?!
       - Even better: trace the EXACT math of `512` frames! How many bytes is `512` stereo 16-bit frames (`512 * 4 = 2048` bytes) or how many calls to `step_decode_and_output` produce `512` frames?
2. **Live LMS Player Status & Track Codec Inspection**:
   - Query `http://10.0.0.10:9000/jsonrpc.js` with read-only command `["c8:2e:18:be:b8:34", ["status", "-", 10, "tags:uB,a,c,d,e,g,i,j,k,l,m,n,o,p,q,r,s,t,u,v,w,x,y"]]` to see what track is in the `"BASEMENT"` playlist, what audio format/codec (`type`, `bitrate`, `samplerate`, `samplesize`) it is, and what state LMS thinks `"BASEMENT"` is in.
   - Check what codecs `louder-esp32-app` advertises in `HELO` (`src/main.rs`) vs what decoders exist in `squeezelite-decoders` (`PcmDecoder`, `FlacDecoder`, `Mp3Decoder`, `VorbisDecoder`, `AacDecoder`) and what decoder `src/main.rs` actually instantiates on `strm` `'s'`!
3. **Why did `slim_sock` reconnect from port `60832` to `50331`?**
   - Check `src/main.rs` for any blocking operation, buffer overflow in `slim_acc_buf`, unparsed frame in `slim_acc_buf` (`ParseError`), or `STAT` reporting issue that caused LMS or `slim_sock` to reset/reconnect during playback.

---

## 3. Deliverable
Return a structured Markdown audit report with `[CRITICAL]`, `[HIGH]`, `[MEDIUM]`, `[LOW]` findings, exact `file:line` citations, mathematical proof of why `frames_played == 512`, and concrete remediation code specifications.

---

## 4. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Subagent Conversation ID:** `e8da7e51-d275-4dee-bbc9-de93994ebc56`
- **Execution Timestamp:** 2026-10-24T12:35:19-04:00
- **Key Findings:**
  1. `AUDIT-001` (`[CRITICAL]`, `src/main.rs:1576-1610`): Premature `is_eof` handling on `audio_sock` aborts `audio_sock` and immediately sends `STMd` (`DecodeReady`) before `pipeline.stream_buffer` is drained by the decoder, and never sends `STMu` (`Underrun` / track finished) when `pipeline.output_buffer` finishes draining.
  2. `AUDIT-002` (`[HIGH]`, `src/main.rs:729-730, 833`): `stream_storage` and `output_storage` are only `4,096` bytes (`23 ms` headroom) and `slim_acc_buf` is `1,536` bytes (below `MAX_FRAME_LENGTH = 4,096`).
  3. `AUDIT-003` (`[MEDIUM]`, `src/main.rs:1057, 1175`): `StreamCommandType::Start` does not reset `pipeline.frames_played = 0` or `coordinator.frames_played = 0`, and active playback `STAT("STMt")` interval is `5,000 ms` instead of `1,000 ms` (`slimproto.c:890`).
  4. Live LMS JSON-RPC verified `"BASEMENT"` (`c8:2e:18:be:b8:34`) queued track `"01 Basement Deep Groove 122bpm 8A"` (`FLAC (DJMix)`, transcoded by LMS to `audio/L16` 16-bit big-endian stereo PCM on HTTP port `9000`).
- **Fulfillment Status:** `[EXECUTED & VERIFIED]`
