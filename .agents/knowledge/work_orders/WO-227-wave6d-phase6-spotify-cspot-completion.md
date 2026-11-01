# Work Order: WO-227 — Wave 6D: Phase 6 Spotify Connect (`cspot`) Completion — Reusable Credentials, Mercury Protobuf, AES-128-CTR Audio Decryptor, & Track/Seek Events (`squeezelite-cspot`)

- **Work Order ID**: `WO-227`
- **Timestamp**: `2026-10-31T23:48:30-04:00`
- **Assigned Subagent Role**: `Rust Spotify Connect Protocol Implementer` (`TypeName: "self"`)
- **Target Host / Component**: `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cspot/`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Hypothesis
Complete all remaining Phase 6 items in `/home/console-admin/Github/squeezelite-esp32/docs/superpowers/plans/2026-10-29-rust-full-parity-roadmap.md` (§Phase 6) inside `squeezelite-cspot`:
1. **Reusable Credentials & Encrypted Persistence**:
   - Support extracting AP `APWelcome` (`0xAC`) reusable credentials (`canonical_username`, `auth_type = AUTHENTICATION_STORED_SPOTIFY_CREDENTIALS`, `auth_data` blob) and serializing/deserializing them with AES-128-CTR + HMAC-SHA1 authenticated encryption for `FlashJournal` storage (`ReusableCredentials`).
2. **Mercury Protobuf & AP Packet Framing (`0xB2`..`0xB5`, `0x0D`..`0x0E` AudioKey)**:
   - Implement `MercuryHeader` protobuf encoder/decoder (`uri`, `method` `"SUB"`/`"UNSUB"`/`"SEND"`/`"GET"`, `status_code`, `content_type`), multipart `MercuryPacket` assembly/serialization (`cmd = 0xB2` `MercuryReq`, `0xB3` `MercurySub`, `0xB4` `MercuryUnsub`, `0xB5` `MercuryEvent`), and `AudioKey` request/response packets (`cmd = 0x0C` `RequestKey` with 20-byte `file_id` + 16-byte `track_gid` + 4-byte `seq`, `cmd = 0x0D` `AesKey` 16-byte key response, `cmd = 0x0E` `AesKeyError`).
3. **AES-128-CTR Audio Chunk Decryptor (`AudioKeyDecryptor`)**:
   - Implement Spotify's exact AES-128-CTR audio stream decryptor using the standard Spotify 16-byte base IV (`0x72, 0xe0, 0x67, 0xfb, 0xdd, 0xcb, 0xcf, 0x77, 0xeb, 0xe8, 0xbc, 0x64, 0x3f, 0x63, 0x0d, 0x93`), incrementing the 128-bit big-endian counter per 16-byte AES block (`block_index = byte_offset / 16`), and stripping the 167-byte (`0xA7`) Spotify Ogg header offset when seeking/starting at byte 0.
4. **Extended `Spirc` Playback Control, Volume, Seek, Track Metadata & Artwork URL**:
   - Extend `SpircFrame` / `SpircStateMachine` with `PositionMs` / `Seek` (`MessageType::Seek`), `Volume` (`0..=65535`), `Next`/`Prev` track transitions, track metadata (`title`, `artist`, `album`, `duration_ms`, `artwork_image_id` -> CDN URL builder `https://i.scdn.co/image/<hex_id>`), and `TrackEnded` event generation.

## 2. Hard Safety Constraints & Disjoint Ownership
1. **Strict Absolute Paths (`Rule 24`)**: Every file path and command must begin with `/`. Never use `sed`, `awk`, or `cat` for file modifications (`Rule 23`).
2. **Disjoint File Ownership**:
   - You **OWN** strictly `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cspot/**`.
   - Do **NOT** modify any other crate or `/home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml`.
3. **Google Rust Style & API Guidelines (`GRS-001`–`GRS-012`)**:
   - `#![forbid(unsafe_code)]`, `lib.rs` pure `mod` + `pub use` (`GRS-003`), single-module `use` (`GRS-001`), `#[non_exhaustive]` on public structs/enums (`GRS-009`), zero `.unwrap()` in non-test code (`GRS-010`), no `get_` prefix on getters (`GRS-011`), and line widths `<= 100` code / `<= 80` comments (`GRS-012`).
4. **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` returns zero active tasks before completing.

## 3. Execution Steps
1. Inspect all files in `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cspot/src/`.
2. Implement reusable credential encryption/serialization, Mercury protobuf framing + `AudioKey` (`0x0C`/`0x0D`/`0x0E`) packets, `AudioKeyDecryptor` (AES-128-CTR with big-endian counter addition + `0xA7` Spotify Ogg header skip), and extended `Spirc` seek/volume/metadata/artwork support while preserving 100% backward compatibility with existing `squeezelite-cspot` APIs used by `louder-esp32-app`.
3. Add comprehensive unit tests covering `ReusableCredentials` encrypt/decrypt + tamper rejection, `MercuryHeader` + multipart `MercuryPacket` roundtrip, `AudioKey` request/response parsing, `AudioKeyDecryptor` arbitrary byte-offset decryption + `0xA7` header stripping, and `Spirc` seek/volume/track metadata transitions.
4. Run `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-cspot` (`0` failures) and `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cspot/src --strict` (`0 error(s), 0 warning(s)`).
5. Append Fulfillment Summary to §4 of this file and write subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo227_wave6d_cspot_phase6.md`.

---

## 4. Fulfillment Section

- **Status**: `[EXECUTED & VERIFIED]`
- **Summary of Changes**:
  1. **`ReusableCredentials` (`/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cspot/src/zeroconf.rs`)**:
     - Implemented `APWelcome` (`0xAC`) protobuf credential extraction (`from_ap_welcome`, `from_ap_welcome_packet`) and encoding (`encode_ap_welcome`, `encode_login_credentials`) with `AUTHENTICATION_STORED_SPOTIFY_CREDENTIALS = 0x01`.
     - Implemented AES-128-CTR + HMAC-SHA1 authenticated encryption and constant-time MAC verification (`encrypt`, `decrypt`, `encrypt_base64`, `decrypt_base64`) for `FlashJournal` persistence.
  2. **`MercuryHeader`, `MercuryPacket` (`0xB2`..`0xB5`), & `AudioKeyPacket` (`0x0C`..`0x0E`) (`/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cspot/src/mercury.rs`)**:
     - Extended `MercuryHeader` with `status_code` (field 4) and `content_type` (field 2) alongside `uri` (field 1) and `method` (field 3), with full protobuf `encode`/`decode`.
     - Implemented `MercuryPacket` (`0xB2` `MercuryReq`/`Send`, `0xB3` `MercurySub`/`Sub`, `0xB4` `MercuryUnsub`/`Unsub`, `0xB5` `MercuryEvent`/`Subres`) single-packet codec and multipart fragment reassembly (`assemble_multipart`).
     - Implemented `AudioKeyPacket` (`0x0C` `RequestKey`, `0x0D` `AesKey`, `0x0E` `AesKeyError`) encoder and decoder.
  3. **`AudioKeyDecryptor` (`/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cspot/src/cdn.rs`)**:
     - Implemented `AudioKeyDecryptor` using Spotify's 16-byte base IV (`AUDIO_AES_IV`), 128-bit big-endian counter addition per 16-byte block (`counter_iv`), arbitrary byte-offset decryption (`decrypt_in_place`), stateful streaming (`decrypt_next`, `seek`, `seek_ogg`), and automatic stripping of the 167-byte (`0xA7`) Spotify Ogg header (`decrypt_chunk`, `strip_ogg_header`).
  4. **Extended `Spirc` (`/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cspot/src/spirc.rs`)**:
     - Implemented `TrackMetadata` (`title`, `artist`, `album`, `duration_ms`, `artwork_image_id`) with `metadata.proto` protobuf `encode_protobuf`/`decode_protobuf` and CDN URL builder (`https://i.scdn.co/image/<hex_id>`).
     - Extended `SpircFrame` with `seek`, `volume`, `next`, `prev`, and `encode`, and extended `SpircStateMachine` with `0..=65535` volume clamping (`Volume`, `VolumeUp`, `VolumeDown`), `Seek`, `Next`/`Prev` queue transitions, and `TrackEnded` event generation (`notify_track_ended`, `advance_position_ms`).
- **Verification Evidence**:
  - `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-cspot`: **26 passed, 0 failed**.
  - `/home/console-admin/.cargo/bin/cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-cspot --no-deps --all-targets -- -D warnings`: **0 warnings, 0 errors**.
  - `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cspot/src --strict`: **`0 error(s), 0 warning(s)`**.
- **Subagent Summary Path**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo227_wave6d_cspot_phase6.md`

