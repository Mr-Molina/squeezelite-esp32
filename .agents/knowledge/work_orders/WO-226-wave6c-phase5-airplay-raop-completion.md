# Work Order: WO-226 — Wave 6C: Phase 5 AirPlay (RAOP) Completion — RTP Control/Timing/Resend Jitter Buffer, DMAP/Artwork, DACP, & Sync Drift Correction (`squeezelite-raop`)

- **Work Order ID**: `WO-226`
- **Timestamp**: `2026-10-31T23:48:30-04:00`
- **Assigned Subagent Role**: `Rust AirPlay RAOP Protocol Implementer` (`TypeName: "self"`)
- **Target Host / Component**: `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-raop/`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Hypothesis
Complete all remaining Phase 5 items in `/home/console-admin/Github/squeezelite-esp32/docs/superpowers/plans/2026-10-29-rust-full-parity-roadmap.md` (§Phase 5) inside `squeezelite-raop`:
1. **RTP Control (`0x54` Sync & `0x56` Retransmit Reply) & Timing (`0x52` / `0x53` NTP Timing) Sockets + Resend Request Builder (`0x55`) & Bounded Jitter Buffer**:
   - Parse RTP control sync packets (`PT = 0x54` / `0xD4`) and retransmit response packets (`PT = 0x56` / `0xD6`, unwrapping the embedded original RTP audio packet at offset `+4`).
   - Build RTP resend request packets (`0x80 0xD5`, seq, missed_seq_start, missed_count) when a sequence gap is detected.
   - Implement a bounded PSRAM/heap jitter & reorder ring (`RtpJitterBuffer`) that orders out-of-order/retransmitted packets by 16-bit wrapping sequence number (`seq_diff`), emits resend requests for missing sequences within a bounded window, and flushes cleanly on RTSP `FLUSH` (`rtp-info: seq=...;rtptime=...`).
   - Parse and build NTP timing request/response packets (`PT = 0x52` / `0x53`).
2. **DMAP Metadata (`daap.songalbum`, `daap.songartist`, `daap.songname`, `minm`, `asar`, `asal`) & Artwork (`image/jpeg` / `image/png`)**:
   - Extend RTSP `SET_PARAMETER` handling to parse binary DMAP (`application/x-dmap-tagged`) TLV trees (`mlit`/`minm`/`asar`/`asal`/`astm`) and raw cover artwork payloads (`image/jpeg`, `image/png`), plus `text/parameters` progress (`progress: start/curr/end`).
3. **DACP Remote-Control Client (`Active-Remote` / `DACP-ID`)**:
   - Parse `DACP-ID` and `Active-Remote` headers from RTSP requests and build HTTP GET `/ctrl-int/1/<command>` requests (`playpause`, `nextitem`, `previtem`, `volumeup`, `volumedown`, `mutetoggle`, `stop`) with `Active-Remote` header.
4. **Sync-Error Drift Correction Window**:
   - Implement `SyncDriftTracker` comparing RTP timestamps + NTP sync anchors against local DAC consumption to compute single-frame stuff/drop (`-1`, `0`, `+1` stereo frame adjustment) when drift exceeds the 2 ms window.

## 2. Hard Safety Constraints & Disjoint Ownership
1. **Strict Absolute Paths (`Rule 24`)**: Every file path and command must begin with `/`. Never use `sed`, `awk`, or `cat` for file modifications (`Rule 23`).
2. **Disjoint File Ownership**:
   - You **OWN** strictly `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-raop/**`.
   - Do **NOT** modify any other crate or `/home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml`.
3. **Google Rust Style & API Guidelines (`GRS-001`–`GRS-012`)**:
   - `#![forbid(unsafe_code)]`, `lib.rs` pure `mod` + `pub use` (`GRS-003`), single-module `use` (`GRS-001`), `#[non_exhaustive]` on public structs/enums (`GRS-009`), zero `.unwrap()` in non-test code (`GRS-010`), no `get_` prefix on getters (`GRS-011`), and line widths `<= 100` code / `<= 80` comments (`GRS-012`).
4. **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` returns zero active tasks before completing.

## 3. Execution Steps
1. Inspect all files in `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-raop/src/`.
2. Implement the RTP control/timing/resend/jitter buffer, DMAP/artwork/progress parser, DACP remote-control request builder, and sync drift tracker while preserving 100% backward compatibility with existing `squeezelite-raop` public APIs used by `louder-esp32-app`.
3. Add thorough unit tests covering RTP sequence gap detection & `0xD5` resend generation, `0xD6` retransmit unwrapping, out-of-order jitter buffer reordering, DMAP `minm`/`asar`/`asal` TLV extraction, progress parsing, DACP command formatting, and drift correction.
4. Run `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-raop` (`0` failures) and `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-raop/src --strict` (`0 error(s), 0 warning(s)`).
5. Append Fulfillment Summary to §4 of this file and write subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo226_wave6c_raop_phase5.md`.

---

## 4. Fulfillment Section

- **Status**: `[EXECUTED & VERIFIED]`
- **Completed At**: `2026-11-01T00:03:30-04:00`
- **Subagent Summary Path**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo226_wave6c_raop_phase5.md`

### Summary of Implemented Components
1. **RTP Control, Timing, Resend Request & Bounded Jitter Buffer (`/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-raop/src/rtp.rs`)**:
   - Implemented `RtpSyncPacket` (`PT = 0x54` / `0xD4`) parser, serializer, and `effective_latency_frames()` calculation matching upstream `components/raop/rtp.c`.
   - Implemented `unwrap_retransmit_packet` and `RtpControlPacket` (`Sync` and `Retransmit` variants) unwrapping `PT = 0x56` / `0xD6` packets at offset `+4`, plus transparent `0x56` unwrapping in `RtpAudioReceiver::process_packet`.
   - Implemented `RtpResendRequest` and `build_resend_request` (`0x80 0xD5`, `request_seq`, `missed_seq_start`, `missed_count`).
   - Implemented `NtpTimingPacket` (`PT = 0x52` request / `0x53` response) 32-byte codec, round-trip time calculation, and `ntp_to_ms` / `ms_to_ntp` helpers.
   - Implemented bounded `RtpJitterBuffer` with 16-bit wrapping sequence arithmetic (`seq_diff`, `seq_after`, `seq_before`), sequence-gap detection emitting `RtpResendRequest`, out-of-order / retransmitted packet recovery, and RTSP `FLUSH` (`parse_rtp_info`, `flush(seq, rtptime)`).
2. **DMAP Binary Metadata, Cover Artwork & Track Progress (`/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-raop/src/dmap.rs` & `src/rtsp.rs`)**:
   - Implemented `DmapMetadata` recursive TLV parser and encoder for `application/x-dmap-tagged` (`mlit`, `mdcl`, `mlcl`, `mcon`, `cmst`, `adbs`, `apso` containers; `minm`/`cann` title, `asar`/`cana` artist, `asal`/`canl` album, `astm`/`cast` duration in ms).
   - Implemented `CoverArtwork` and `ArtworkMimeType` (`image/jpeg`, `image/png`) extraction with `RTP-Info` timestamp correlation on RTSP `SET_PARAMETER`.
   - Implemented `TrackProgress` parser (`progress: start/curr/end`) and millisecond conversion (`elapsed_ms`, `duration_ms`) on `SET_PARAMETER` (`text/parameters`).
3. **DACP Remote-Control Client (`/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-raop/src/dacp.rs` & `src/rtsp.rs`)**:
   - Implemented `DacpCommand` (`PlayPause`, `Play`, `Pause`, `PlayResume`, `Stop`, `NextItem`, `PrevItem`, `VolumeUp`, `VolumeDown`, `MuteToggle`, `BeginRew`, `BeginFf`) and `DacpClient` extracting `DACP-ID` and `Active-Remote` headers from RTSP requests and formatting HTTP/1.0 `GET /ctrl-int/1/<command>` and `setproperty?dmcp.device-volume=...` requests.
4. **Sync-Error Drift Correction Window (`/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-raop/src/rtp.rs`)**:
   - Implemented `SyncDriftTracker` anchoring NTP timing (`0x53`) and RTP sync (`0x54`) timestamps to local playback time and computing `-1` (drop 1 stereo frame), `0` (in sync), or `+1` (stuff 1 stereo frame) when drift exceeds the 2 ms window (`88` frames at 44.1 kHz), along with `SyncDriftTracker::apply_stereo_adjustment`.

### Verification Evidence
- `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-raop/src --strict` -> `0 error(s), 0 warning(s)` (exit code `0`).
- `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-raop` -> `21 passed; 0 failed` (exit code `0`).
- `/home/console-admin/.cargo/bin/cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-raop --all-targets -- -D warnings` -> `0` warnings, `0` errors (exit code `0`).

