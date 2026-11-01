# Work Order: WO-248 — Phase 1 `CODE-AUD-4`: AirPlay 1 (`squeezelite-raop`) & Spotify Connect (`squeezelite-cspot`) Protocol & Crypto Micro-Auditor

- **Work Order ID**: `WO-248`
- **Timestamp**: `2026-11-01T08:43:00-05:00`
- **Assigned Subagent Role**: `AirPlay 1 & Spotify Connect Protocol & Crypto Micro-Auditor` (`Model: "pro"`, `TypeName: "research"`, read-only)
- **Target Host / Component**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-raop/src/{lib,crypto,rtsp,rtp,dmap,dacp,mdns}.rs` (`3,994` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cspot/src/{lib,crypto,mercury,shannon,spirc,zeroconf,cdn}.rs` (`4,368` LOC)
- **Status**: `[AUDIT COMPLETE]`

## 1. Objective & Micro-Auditor Contract
1. **Role**: `AirPlay 1 & Spotify Connect Protocol & Crypto Micro-Auditor` (`CODE-AUD-4`)
2. **Scope**: Strictly `squeezelite-raop/src/*` and `squeezelite-cspot/src/*`.
3. **Focus Areas & Pre-Identified Hotspots**:
   - Audit Phase 5 (`squeezelite-raop`) implementation (`WO-210`, `WO-226`):
     - `rtp.rs`: Control packet (`0x54` Sync, `0x56` Retransmit Response), Timing (`0x52`/`0x53`), Resend request builder (`0x55`, 8-byte header), 64-slot `RtpJitterBuffer` sequence wrap-around (`u16::wrapping_sub`), `SyncDriftTracker` sample Insert/Drop hysteresis.
     - `dmap.rs`, `dacp.rs`, `rtsp.rs`, `crypto.rs`: DMAP TLV recursion/bounds (`mlit`/`minm`/`asar`/`asal`), `SET_PARAMETER` `image/jpeg`/`image/png` artwork & `progress:` parsing, DACP client request builder, RSA-OAEP/AES-128-CBC bounds.
   - Audit Phase 6 (`squeezelite-cspot`) implementation (`WO-211`, `WO-227`):
     - `zeroconf.rs` & `crypto.rs`: `ReusableCredentials` (`APWelcome` `0xAC` parsing & NVS hex serialization), DH/PBKDF2/HMAC-SHA1/AES-128-CTR bounds.
     - `mercury.rs` & `cdn.rs`: `MercuryHeader` protobuf wire parser, multi-part `MercuryPacket` (`0xB2`..`0xB5`), `AudioKeyPacket` (`0x0C`..`0x0E`), `AudioKeyDecryptor` 128-bit big-endian IV increment & `0xA7` (`167`-byte) Spotify Ogg header strip.
     - `spirc.rs` & `shannon.rs`: `SpircEvent::Seek`/`Volume`/`Next`/`Prev` and `TrackFinished` notification framing.
4. **Constraint**: **Read-only code inspection. Do NOT modify any files.**
5. **Output Schema**: Use the standardized 11-field code finding schema (`/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`) with OWASP, CWE, SOC 2, and `GRS-*` compliance tags.

## 2. Deliverables
Return your complete structured findings in your final message.

## 3. Fulfillment Section
- **Status**: `[AUDIT COMPLETE]`
- **Subagent ID**: `8ce832f8-4886-4095-8368-12b6c94c3cc9`
- **Summary**:
  - Audited `squeezelite-raop` and `squeezelite-cspot` (`8,362` LOC) and reported 14 findings (`1 CRITICAL`, `5 HIGH`, `5 MEDIUM`, `3 LOW`): `SEC-401` (`mercury.rs` unbounded `Vec::with_capacity(part_count)`), `LOGIC-401` (`zeroconf.rs` `addUser` discarding parsed credentials), `LOGIC-402` (`rtp.rs` negative `seq_diff` cast to `usize`), `LOGIC-403` (`dmap.rs` `TrackProgress` `saturating_sub` vs `wrapping_sub`), `LOGIC-404` (`cdn.rs` fragmented 167B Ogg header strip), `SEC-402` (`dacp.rs` CRLF header injection), `SEC-403` (Montgomery ladder branch), `LOGIC-405` (`zeroconf.rs` UTF-8 `url_decode`), `LOGIC-406` (protobuf wire types 1 & 5), `LOGIC-407` (`rtp.rs` `SyncDriftTracker` unsigned underflow), `LOGIC-408` (`spirc.rs` empty queue `current_track`), `SEC-404` (`base64_decode` `len % 4 == 1`), `PERF-401` (`crypto.rs` `R^2 mod N` allocation), and `LOGIC-409` (`rtsp.rs` malformed `Content-Length`).
