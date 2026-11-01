# Work Order: WO-270 — Phase 1 `CODE-PERF-5`: Multi-Protocol Polling (`multi_proto_poll`, `bt_poll_phase`, `squeezelite-proto`, `raop`, `cspot`) & Peripherals Overhead Auditor

- **Work Order ID**: `WO-270`
- **Timestamp**: `2026-11-01T13:37:00-05:00`
- **Assigned Subagent Role**: `Streaming Protocols & Multi-Protocol Poll Overhead Auditor (CODE-PERF-5)` (`Model: "pro"`, `TypeName: "research"`)
- **Target Host / Component**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/{bt_a2dp.rs,controls.rs,display_gds.rs,console.rs}`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-raop/src/**`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cspot/src/**`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-config/src/lib.rs`
- **Status**: `[AUDIT COMPLETE]`

## 1. Objective & Read-Only Constraint
Audit `multi_proto_poll`, `bt_poll_phase`, `squeezelite-proto`, `squeezelite-raop`, `squeezelite-cspot`, and `squeezelite-config` for main-loop polling overhead, unnecessary work when protocols are idle, and SlimProto `strm`/`STAT` flow control (`PROT-501`, `PROT-502`, ...).
- **Constraint**: Read-only code inspection. Do NOT modify any files.

## 2. Inspection Checklist
1. **`multi_proto_poll` & `bt_poll_phase` Overhead on Every Main-Loop Iteration (`main.rs` & `bt_a2dp.rs`)**:
   - Inspect `multi_proto_poll` and `bt_poll_phase` in `main.rs` (and `tasks.rs`):
     - What work is executed on EVERY iteration of the main loop for AirPlay RTSP/RTP, mDNS, Telnet CLI (`23`), Spotify Connect, and Bluetooth HCI (`bt_poll_phase`) even when those protocols have zero active traffic?
     - Does `bt_poll_phase` or `multi_proto_poll` perform expensive socket checks, buffer allocations/zeroing, or HCI polls on every loop iteration?
2. **SlimProto `strm` Direct-Stream vs Proxy Negotiation & Buffer Thresholds (`squeezelite-proto` & `coordinator.rs`)**:
   - Inspect how `squeezelite-proto` and `coordinator.rs` handle `strm` frames when `server_ip == 0` (external WAN URL) vs `server_ip != 0` (LMS IP `10.0.0.10`), how `autostart` / `threshold` (KB before playback starts) is enforced before unpausing decoding, and whether decoding starts IMMEDIATELY on the first 1 KB packet before pre-buffering `threshold` KB of stream data!
   - WAIT A SECOND! Check that carefully: in `coordinator.rs` / `pipeline.rs`, when a new HTTP stream starts, does the player wait until `stream_buffer.available_read() >= threshold_bytes` (as required by SlimProto `strm` `threshold` field, e.g. `20` KB or `255` KB!) before starting playback, OR does it start decoding and playing on the very first TCP packet (`~1.4` KB = `~70` ms of MP3 audio), causing the playback buffer to hover near `0` bytes and underrun on every Wi-Fi/WAN jitter spike?!
3. **`squeezelite-config` FlashJournal / NVS Access on Hot Paths**:
   - Verify whether any runtime command (`AUDG`, `strm`, `STAT`, or `http_control`) triggers synchronous `FlashJournal::validate_sector` (`WO-251` `LOGIC-504`) or CRC scans on the audio/network hot path.

## 3. Fulfillment Summary (`7e86e4bb` — `[AUDIT COMPLETE]`)
- **`PROT-501` (`HIGH`, `main.rs:3474-3518`)**: Wildcard mDNS query check `else if payload.len() >= 12 && (payload[2] & 0x80) == 0` in `multi_proto_poll` fires on every standard mDNS query on the LAN, allocating `String`/`Vec<u8>` via `format!` and broadcasting unsolicited AirPlay + Spotify mDNS packets continuously during playback.
- **`PROT-502` (`HIGH`, `bt_a2dp.rs:4454-4464`, `main.rs:2846-2895`)**: `bt_sink.poll()` zeroes a `1,024`-byte stack array `[0u8; 1024]` and polls VHCI on every superloop tick (`~4,000+` times/sec) even when Bluetooth is idle; throttle to `>= 2 ms` when `connected_peer.is_none()` and avoid zeroing the unused slice.
- **`PROT-503` (`MEDIUM`, `main.rs:3654, 3726`)**: AirPlay RTP and Spotify handlers allocate `alloc::vec![0u8; 1536]` and `alloc::vec![0u8; 1024]` on the heap per packet instead of using stack/static scratch buffers.
- **`PROT-504` (`CRITICAL`, `coordinator.rs:1870-1876, 2130-2138`, `pipeline.rs:218-220`)**: When `strm` has `threshold == 0` (or small threshold), `stream_reached_threshold` (`self.stream_bytes_received > 0`) transitions from `Buffering` to `Playing` on the very first `1.4 KB` TCP packet (`~35–70 ms` of audio), AND `output_threshold_frames` (`pipeline.rs:218`) is never enforced in `pump_output`. Enforce a minimum pre-buffer threshold (`>= 16,384`–`32,768` B) and gate `pump_output` until `output_threshold_frames` is buffered.
- **`PROT-505` (`LOW`, `main.rs:3730-3744`)**: `cspot_audio.decrypt_chunk` uses a 128-byte chunk loop (`12` calls per 1460-byte packet).
- **`PROT-506` (`LOW` / Verified Safe)**: `FlashJournal` / NVS writes are strictly confined to explicit Web UI `/config.json` / CLI `save` paths and never run on `AUDG`, `strm`, or `STAT` hot paths.
