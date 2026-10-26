# Work Order `WO-060`: `/deep-code-audit` Micro-Auditor 4 — SlimProto `audio_sock` HTTP Streaming, Direct-Stream vs Proxied Radio, & Buffer Starvation Audit

- **ID**: `WO-060`
- **Timestamp**: `2026-10-24T17:42:00-04:00`
- **Subagent Role**: SlimProto HTTP Audio Socket & Radio Stream Auditor (`TypeName: "research"`, `Model: "pro"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Scope
Conduct a zero-tolerance `/deep-code-audit` of the `audio_sock` HTTP streaming engine in `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (lines 1100–1780):
1. Trace every state transition of `audio_sock` from `StreamCommandType::Start` (`strm 's'`) through HTTP request transmission, HTTP header parsing (`audio_header_buf`), ICY `metaint` stripping, `pipeline.ingest_stream_data`, `pipeline.process_decode_step`, and EOF/close handling:
   - What happens when `audio_header_buf` (`[u8; 1024]`) receives an HTTP response whose headers exceed `1,024` bytes (`audio_header_len >= audio_header_buf.len()`)? Look at lines 1730–1760 of `src/main.rs`!
   - What happens when `pipeline.stream_buffer.available_write() == 0` while `audio_sock.can_recv()` is true?
   - What happens when `pipeline.process_decode_step()` is only called **once** per `run_firmware_tick` (or how many times is `process_decode_step()` called vs `audio_sock.recv()` in `main.rs`?)!
   - What happens when `audio_sock` enters `CloseWait` or `!audio_sock.is_active()` while `stream_buffer` still has bytes?
   - Why did `frames_played` stop at `297,255` (`~6.74` seconds) in the live run?
2. Return structured findings (`F-STREAM-*`) with exact file/line citations and root-cause proof.

## 2. Mandatory Constraints
- **Read-Only Audit**: Do NOT modify repository files.
- **NEVER touch `/dev/ttyUSB0`** (Rule 3).

## 3. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `3eb7aa19-eeb8-4084-91f9-44d10f0ee0fc` (`SlimProto HTTP Audio Socket & Radio Stream Auditor`)
- **Key Findings (`F-STREAM-001`..`F-STREAM-006`)**:
  - `F-STREAM-001`: `audio_header_buf` (`[u8; 1024]`) treats buffer full (`>= 1024` B) as `is_header_complete = true`, feeding unparsed HTTP response headers (`1025..`) into `pipeline.stream_buffer`.
  - `F-STREAM-002`: `main.rs` only sets `icy_meta_interval` from SlimProto `cont` and never parses `icy-metaint:` from HTTP response headers in `audio_header_buf` (needed when `autostart` is `'1'`).
  - `F-STREAM-003`: `slim_tx_buf` (`1,024` B) is smaller than max `RESP` packet (`8 + 1024 = 1032` B) plus queued `STAT` frames; `slim_sock.send_slice(&resp_buf[..resp_len])` truncates `RESP` silently, corrupting the SlimProto stream and causing LMS's 6-second response timer to abort the stream (`297,255` frames = `6.74s`).
  - `F-STREAM-004`: `HELO` omits `CanHTTPS=1` so LMS proxies `https://` streams when conversion rules allow.
  - `F-STREAM-005`: Emits `STMu` on buffer drain, signaling track end to LMS (`mode: "stop"`).
  - `F-STREAM-006`: `run_firmware_tick` (`main.rs:56-68`) only calls `pipeline.process_decode_step()` when `output_buffer.available_read() == 0` instead of keeping `output_buffer` continuously filled to capacity!
