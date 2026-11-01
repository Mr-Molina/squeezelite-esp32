# Work Order: WO-213 — Phase 3: Pure-Rust On-Device Spotify Connect (`squeezelite-cspot`) Engine Implementation

- **Work Order ID**: `WO-213`
- **Timestamp**: `2026-10-29T10:30:00-04:00`
- **Assigned Subagent Role**: `Rust no_std CSpot Implementer (Spotify Connect)`
- **Target Host / Component**: `rust/crates/squeezelite-cspot/`, `rust/crates/louder-esp32-app/`, and `rust/Cargo.toml`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Hypothesis
Implement the pure-Rust `#![no_std]` Spotify Connect (`cspot`) engine in `rust/crates/squeezelite-cspot/` matching the functionality of upstream C `components/spotify/cspot/`:
1. ZeroConf mDNS & Local HTTP Receiver (`zeroconf.rs`):
   - Generates binary mDNS response for `_spotify-connect._tcp.local` on UDP 5353 (TXT: `CPath=/`, `VERSION=1.0`, `Stack=SP`).
   - Implements Spotify ZeroConf HTTP endpoint (`GET /?action=getInfo` and `POST /?action=addUser`) to receive username, encrypted blob, and client public key from official Spotify apps on LAN.
2. Cryptography Engine (`shannon.rs`):
   - Implements the pure-Rust Shannon stream cipher & MAC according to the official spec (`Shannon.cpp` in `components/spotify/cspot/src/`).
   - Implements Diffie-Hellman / Curve25519 key exchange to establish encrypted transport with Spotify AP servers.
3. Spirc / Mercury State Machine (`spirc.rs`, `mercury.rs`):
   - Handles Spirc protocol frames: `kPlay`, `kPause`, `kNext`, `kPrev`, `kSetVolume`, `kNotify`, track URI loading.
   - Handles Mercury request/response framing for track metadata and audio key retrieval.
4. CDN Audio Chunk Receiver & Vorbis Feeder (`cdn.rs`):
   - Fetches audio file chunks from Spotify CDN URLs over HTTP.
   - Feeds Ogg Vorbis bitstream into `squeezelite-decoders` (`vendor/lewton`).
   - Emits 16-bit 44.1 kHz stereo PCM into `StreamPipeline`.
5. Host Unit & Golden Vector Tests:
   - Shannon cipher encryption/decryption against golden vectors.
   - ZeroConf getInfo / addUser parsing and blob extraction.
   - Spirc state machine lifecycle.
6. Stage-Only Build Verification:
   - Integrate into workspace under `rust/Cargo.toml`.
   - `cargo test --workspace` -> 0 failures.
   - `python3 google_rust_audit.py ... --strict` -> 0 errors, 0 warnings.
   - `cargo clippy --workspace --all-targets -- -D warnings` -> 0 warnings.
   - Verify `xtensa-esp32-none-elf` build: `.stack >= 36,000` B and `.dram2_uninit <= 98,768` B.

## 2. Hard Safety Constraints
1. **Strict Serial Isolation (`single-target-troubleshooting.md` / Invariant 47)**: **NEVER** open, probe, reset, or flash `/dev/ttyUSB0` (separate CYD board).
2. **Forbidden Endpoint (`Invariant 42`)**: **NEVER** call `POST http://172.22.1.15/reboot_ota.json`.
3. **1-Flash Circuit Breaker**: Flash is deferred to `WO-214`. Do NOT flash in `WO-213`.
4. **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` returns zero active tasks before completing.
5. **Memory Section Limits**: `.stack >= 36,000` B, `.dram2_uninit <= 98,768` B.

## 3. Implementation Steps

1. Create `rust/crates/squeezelite-cspot`:
   - `Cargo.toml`: `#![no_std]` library crate with `alloc` feature. Dependencies: `squeezelite-decoders`, `heapless`, `log`, `proptest`.
   - Register in `rust/Cargo.toml` workspace members and `rust/crates/louder-esp32-app/Cargo.toml`.
2. Implement `shannon.rs`:
   - Shannon stream cipher with 16 32-bit registers ($R_0..R_{15}$), nonlinear filter function, key expansion, and MAC calculation matching `components/spotify/cspot/src/Shannon.cpp`.
   - Golden test verifying bit-for-bit output against reference test vectors.
3. Implement `zeroconf.rs`:
   - mDNS packet generator for `_spotify-connect._tcp.local`.
   - Local HTTP server handler:
     - `action=getInfo`: JSON response with `deviceID`, `remoteName = "BASEMENT"`, `publicKey`, `deviceType = "AudioDongle"`.
     - `action=addUser`: parses form-urlencoded `userName`, `blob`, `clientKey`. Decrypts blob to extract reusable credentials.
4. Implement `spirc.rs` & `mercury.rs`:
   - Minimal Protobuf decoder for Spirc commands (`kPlay`, `kPause`, `kSeek`, `kSetVolume`).
   - Mercury request packet builder and response parser for track AES audio key requests.
5. Implement `cdn.rs`:
   - Streaming in-memory Ogg Vorbis packet demuxer feeding directly into `squeezelite_decoders::VorbisDecoder`.
   - PCM output buffer feeding into `StreamPipeline`.
6. Host unit tests in `rust/crates/squeezelite-cspot/tests/`:
   - Test Shannon cipher encryption/decryption roundtrip.
   - Test ZeroConf getInfo / addUser responses.
   - Test Spirc frame parsing and state transitions.
7. Verification & Audit:
   - `cargo test --workspace` -> 0 failures.
   - `python3 google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cspot/src --strict` -> 0 errors, 0 warnings.
   - `cargo clippy --workspace --all-targets -- -D warnings` -> 0 warnings.
   - `cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`.
   - Verify `.stack >= 36,000` B and `.dram2_uninit <= 98,768` B.

## 4. Deliverables
- Append Fulfillment Summary to this file.
- Write subagent summary to `.agents/knowledge/subagent_summaries/20261029_wo213_cspot_engine.md`.
- Confirm 0 background tasks before handoff.


---

## 5. Fulfillment Summary

- **Completed At**: `2026-10-29T10:40:00-04:00`
- **Fulfillment Status**: `[EXECUTED & VERIFIED]`

### Implemented Components
1. **`rust/crates/squeezelite-cspot/`**:
   - `Cargo.toml`: Pure-Rust `#![no_std]` library crate with `alloc` feature, integrated into workspace `rust/Cargo.toml` and `rust/crates/louder-esp32-app/Cargo.toml`.
   - `src/lib.rs`: Restrictive module and clean public re-exports following `GRS-003`.
   - `src/shannon.rs`: Pure-Rust Shannon stream cipher & MAC implementation matching upstream `Shannon.cpp` (16 32-bit registers, nonlinear filter function, key expansion, MAC accumulation).
   - `src/crypto.rs`: Oakley Group 1 Diffie-Hellman (RFC 2409, 768-bit MODP), pure-Rust SHA-1, HMAC-SHA1, PBKDF2-HMAC-SHA1, AES-128-CTR, AES-192-ECB, and RFC 4648 Base64.
   - `src/zeroconf.rs`: ZeroConf mDNS responder packet encoder on UDP 5353 (`_spotify-connect._tcp.local`, TXT: `CPath=/`, `VERSION=1.0`, `Stack=SP`) and HTTP endpoint handler (`action=getInfo` and `action=addUser` login blob decryption).
   - `src/spirc.rs`: Spirc command parsing and state machine (kPlay, kPause, kNext, kPrev, kSetVolume, kNotify, track URI loading).
   - `src/mercury.rs`: Mercury request/response framing for track metadata and audio key retrieval (command 0x0C and responses 0x0D/0x0E).
   - `src/cdn.rs`: CDN audio chunk decryptor (audio AES IV `0x72, 0xe0...` + `big_num_add`) and streaming Vorbis bitstream feeder into `squeezelite_decoders::VorbisDecoder`.

### Verification Evidence
1. **Host Unit & Golden Vector Tests**:
   - Command: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-cspot`
   - Result: 21 passed; 0 failed. Shannon cipher verified bit-for-bit against upstream C++ `Shannon.cpp` golden vectors.
2. **Workspace Regression Tests**:
   - Command: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace`
   - Result: 0 failures across all crates and doc-tests.
3. **Google Rust Style Strict Audit**:
   - Command: `python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cspot/src --strict`
   - Result: `0 error(s), 0 warning(s)` (PASS).
4. **Workspace Clippy**:
   - Command: `cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets -- -D warnings`
   - Result: 0 warnings, 0 errors. Exit code `0`.
5. **Xtensa ESP32 Release Build**:
   - Command: `cd /home/console-admin/Github/squeezelite-esp32/rust && source /home/console-admin/export-esp.sh && unset CARGO_TARGET_XTENSA_ESP32_NONE_ELF_RUSTFLAGS && cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`
   - Result: Exit code `0` (PASS).
6. **Memory Budgets**:
   - `.stack`: `0x008d28` = 36,136 B (`>= 36,000` B budget, PASS).
   - `.dram2_uninit`: `0x018098` = 98,456 B (`<= 98,768` B budget, PASS).
7. **Task Concurrency Barrier**:
   - Command: `manage_task(list)`
   - Result: 0 active background tasks.
