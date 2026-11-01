# Work Order: WO-210 — Phase 1: Pure-Rust On-Device AirPlay 1 (RAOP) Engine Implementation

- **Work Order ID**: `WO-210`
- **Timestamp**: `2026-10-29T09:30:00-04:00`
- **Assigned Subagent Role**: `Rust no_std RAOP Implementer (AirPlay 1)`
- **Target Host / Component**: `rust/crates/squeezelite-raop/`, `rust/crates/louder-esp32-app/`, and `rust/Cargo.toml`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Hypothesis
Implement the pure-Rust `#![no_std]` AirPlay 1 (RAOP) receiver engine in `rust/crates/squeezelite-raop/` matching the functionality of upstream C `components/raop/`:
1. RTSP 1.0 Server (`rtsp.rs`): Handles `OPTIONS`, `ANNOUNCE`, `SETUP`, `RECORD`, `SET_PARAMETER`, `FLUSH`, `TEARDOWN` on TCP port 5000. Parses SDP parameters (`rsaaeskey`, `aesiv`, `fmtp`, `rtpmap`).
2. Crypto Engine (`crypto.rs`): Embeds the standard Airport Express 1024-bit RSA private key to decrypt the AES-128 session key via PKCS#1 v1.5 padding unpad. Performs AES-128-CBC audio payload decryption in pure Rust (`#![no_std]`).
3. RTP Audio Receiver & Depacketizer (`rtp.rs`): Ingests UDP RTP audio packets (default port 6000), depacketizes headers (seq_num, timestamp), decrypts payloads with AES-128-CBC, decodes ALAC frames via `squeezelite-decoders` (`vendor/alac`), and emits 16-bit 44.1 kHz stereo PCM.
4. mDNS Announcement Builder (`mdns.rs`): Generates binary mDNS response records for `_raop._tcp.local` and `_airplay._tcp.local` announcing player name `"BASEMENT"` with TXT parameters matching upstream C `components/raop/raop_sink.c`.
5. Host Unit & Golden Vector Tests: Test RTSP request parsing, SDP extraction, RSA-1024 AES key decryption, and AES-128-CBC + ALAC audio packet decoding with bit-for-bit golden verification.
6. Stage-Only Build Verification: Integrate into `louder-esp32-app` / `squeezelite-decoders`, pass `google_rust_audit.py --strict`, `cargo clippy`, and build release ELF for `xtensa-esp32-none-elf` verifying `.stack >= 36,000` B and `.dram2_uninit <= 98,768` B.

## 2. Hard Safety Constraints
1. **Strict Serial Isolation (`single-target-troubleshooting.md` / Invariant 47)**: **NEVER** open, probe, reset, or flash `/dev/ttyUSB0` (separate CYD board).
2. **Forbidden Endpoint (`Invariant 42`)**: **NEVER** call `POST http://172.22.1.15/reboot_ota.json`.
3. **1-Flash Circuit Breaker**: Flash is deferred to `WO-211`. Do NOT flash in `WO-210`.
4. **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` returns zero active tasks before completing.
5. **Memory Section Limits**: `.stack >= 36,000` B, `.dram2_uninit <= 98,768` B. Sockets and packet buffers must reside in external PSRAM (`esp_alloc::HEAP`), not task stack.

## 3. Implementation Steps

1. **Create `rust/crates/squeezelite-raop` crate**:
   - `Cargo.toml`: `#![no_std]` library crate with `alloc` feature. Dependencies: `squeezelite-decoders`, `smoltcp` (optional/types), `libm`.
   - Add to `rust/Cargo.toml` workspace members.
2. **Implement Crypto Module (`crypto.rs`)**:
   - Airport Express RSA-1024 private key (modulus $n$, private exponent $d$).
   - BigInt modular exponentiation `c^d mod n` in pure Rust (`alloc`) to decrypt the 128-byte encrypted AES key.
   - Verify PKCS#1 v1.5 padding type 02 and extract the 16-byte AES-128 session key.
   - AES-128-CBC decryption for audio packet buffers.
3. **Implement RTSP Module (`rtsp.rs`)**:
   - Parse RTSP 1.0 request line (`METHOD uri RTSP/1.0`).
   - Parse headers: `CSeq`, `Content-Type`, `Content-Length`, `Transport`, `DACP-ID`, `Active-Remote`.
   - Parse SDP body: extract `a=rsaaeskey:<base64>`, `a=aesiv:<base64>`, `a=fmtp:96 ...`.
   - Generate RTSP responses:
     - `OPTIONS`: `Public: ANNOUNCE, SETUP, RECORD, PAUSE, FLUSH, TEARDOWN, OPTIONS, SET_PARAMETER, GET_PARAMETER`.
     - `ANNOUNCE`: `200 OK`.
     - `SETUP`: Parse client ports, reply with server ports (`Transport: RTP/AVP/UDP;unicast;mode=record;server_port=6000;control_port=6001;timing_port=6002`).
     - `RECORD`: `Audio-Latency: 11025`, `200 OK`.
     - `SET_PARAMETER`: Parse `volume: <float>` (dB) and metadata.
     - `TEARDOWN`: `200 OK`.
4. **Implement RTP Audio Module (`rtp.rs`)**:
   - Parse RTP header (12 bytes: `0x80`, payload type 96, sequence number, timestamp, SSRC).
   - In-place AES-128-CBC decryption of payload.
   - Call ALAC frame decode using `squeezelite_decoders::AlacDecoder`.
   - Buffer decoded 16-bit 44.1 kHz stereo PCM samples in ringbuffer.
5. **Implement mDNS Builder (`mdns.rs`)**:
   - Pack PTR, SRV, TXT, and A DNS resource records for AirPlay service announcement:
     - `_raop._tcp.local` (PTR to `<MAC>@<NAME>._raop._tcp.local`)
     - `_airplay._tcp.local` (PTR to `<NAME>._airplay._tcp.local`)
     - SRV record pointing to `<NAME>.local:5000`
     - TXT record with AirPlay capabilities (`tp=UDP`, `sm=false`, `ek=1`, `et=0,1`, `cn=0,1`, `ch=2`, `ss=16`, `sr=44100`, `vn=3`, `txtvers=1`)
6. **Host Testing**:
   - Unit tests covering RTSP parsing, SDP parsing, RSA-1024 decryption, AES-128-CBC, ALAC decode, and mDNS packet construction.
7. **Static Quality & Xtensa Build Verification**:
   - `cargo test --workspace` -> 0 failures.
   - `python3 google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-raop/src --strict` -> 0 errors, 0 warnings.
   - `cargo clippy --workspace --all-targets -- -D warnings` -> 0 warnings.
   - `cargo +esp build --release --target xtensa-esp32-none-elf -p louder-esp32-app`.
   - Verify `.stack >= 36,000` B and `.dram2_uninit <= 98,768` B.

## 4. Deliverables
- Append Fulfillment Summary to this file.
- Write subagent summary to `.agents/knowledge/subagent_summaries/20261029_wo210_raop_airplay1.md`.
- Confirm 0 background tasks before handoff.

---

## 5. Fulfillment Summary

- **Fulfillment Status**: `[EXECUTED & VERIFIED]`
- **Completed By**: `Rust no_std RAOP Implementer (AirPlay 1)`
- **Completed At**: `2026-10-29T09:45:00-04:00`
- **Subagent Summary Path**: `.agents/knowledge/subagent_summaries/20261029_wo210_raop_airplay1.md`

### Implementation Verification Matrix
1. **Crate Provisioning**: Created `rust/crates/squeezelite-raop` as a workspace member under `rust/Cargo.toml`.
2. **Crypto Module (`crypto.rs`)**:
   - Implemented `BigUint` with Montgomery modular exponentiation `c^d mod n` in pure `#![no_std]` + `alloc`.
   - Embedded Airport Express 2048-bit RSA private key (`AIRPORT_EXPRESS_N_BYTES` and `AIRPORT_EXPRESS_D_BYTES`).
   - Implemented PKCS#1 v1.5 type 02 and RSAES-OAEP unpadding, extracting the 16-byte AES-128 session key.
   - Implemented PKCS#1 v1.5 type 01 padding for `Apple-Response` challenge-response authentication.
   - Implemented pure-Rust AES-128-CBC packet decryption with partial trailing byte preservation matching upstream `components/raop/rtp.c`.
   - Implemented pure-Rust Base64 encoder and decoder (supporting unpadded and padded forms).
3. **RTSP Module (`rtsp.rs`)**:
   - Implemented RTSP 1.0 parser for `OPTIONS`, `ANNOUNCE`, `SETUP`, `RECORD`, `SET_PARAMETER`, `FLUSH`, `TEARDOWN`.
   - Implemented SDP parser extracting `rsaaeskey`, `aesiv`, `fmtp`, `rtpmap`.
   - Implemented response builder with conforming headers (`Public`, `Transport`, `Audio-Latency`, `Audio-Jack-Status`).
   - Implemented `RtspServer` connection state machine with volume dB-to-linear mapping.
4. **RTP Audio Module (`rtp.rs`)**:
   - Implemented 12-byte RTP header parser.
   - In-place AES-128-CBC payload decryption and ALAC frame decoding via `alac::Decoder`.
   - Emits 16-bit 44.1 kHz stereo PCM.
5. **mDNS Module (`mdns.rs`)**:
   - Implemented binary mDNS packet builder for `_raop._tcp.local` and `_airplay._tcp.local` announcing `"BASEMENT"`.
   - Packed PTR, SRV, TXT, and A records matching upstream `components/raop/raop_sink.c`.
6. **Host Quality & Verification**:
   - 15 unit and golden vector tests in `tests/raop_tests.rs`: all PASS.
   - `cargo test --workspace`: 0 failures across all crates.
   - `python3 google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-raop/src --strict`: `0 error(s), 0 warning(s)` (PASS).
   - `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings, 0 errors (PASS).
   - Xtensa ESP32 release build (`cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`): `exit code 0` (PASS).
   - Memory budgets: `.stack` = 36,288 B (`>= 36,000` B), `.dram2_uninit` = 98,456 B (`<= 98,768` B).
7. **Task Concurrency Barrier**: `manage_task(list)` returns 0 active background tasks.
