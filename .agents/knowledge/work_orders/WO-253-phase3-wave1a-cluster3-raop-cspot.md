# Work Order: WO-253 — Phase 3 Wave 1A Cluster 3 Remediation (`squeezelite-raop` & `squeezelite-cspot`)

- **Work Order ID**: `WO-253`
- **Timestamp**: `2026-11-01T09:14:00-05:00`
- **Assigned Subagent Role**: `Cluster 3 AirPlay 1 & Spotify Connect Remediation Implementer` (`Model: "flash"`, `TypeName: "self"`)
- **Target Host / Component**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-raop/src/{crypto,rtsp,rtp,dmap,dacp}.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cspot/src/{crypto,mercury,spirc,zeroconf,cdn}.rs`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Strict Disjoint Scope
Remediate all **14 findings** (`SEC-401`..`404`, `LOGIC-401`..`409`, `PERF-401`) from `WO-248` (`CODE-AUD-4`) exclusively in `squeezelite-raop` and `squeezelite-cspot`. **Do NOT modify any files outside those 2 crates.**

## 2. Findings to Remediate
1. **`SEC-401` (`CRITICAL`, `squeezelite-cspot/src/mercury.rs:366-372`)**:
   - In `MercuryPacket::decode`, validate `if pos + part_count.saturating_mul(2) > data.len() { return Err(MercuryError::TruncatedPacket); }` *before* calling `Vec::with_capacity`, and cap pre-allocation capacity to `part_count.min(64)`.
2. **`LOGIC-401` (`HIGH`, `squeezelite-cspot/src/zeroconf.rs:778-796`)**:
   - Add `last_user: Option<ZeroConfUser>` (with getter `pub fn last_user(&self) -> Option<&ZeroConfUser>` and `pub fn take_last_user(&mut self) -> Option<ZeroConfUser>`) to `ZeroConfServer`, and in `handle_request` (`addUser` branch), construct and store `self.last_user = Some(ZeroConfUser { username, auth_type, auth_data })` instead of discarding `(_auth_type, _auth_data)`.
3. **`LOGIC-402` (`HIGH`, `squeezelite-raop/src/rtp.rs:756-760, 788-792`)**:
   - In `RtpJitterBuffer::insert`, when `self.initialized`, compute `let d = seq_diff(seq, cur_r);` and if `d < 0`, immediately return `InsertOutcome::TooLate` before casting `d as usize`.
4. **`LOGIC-403` (`HIGH`, `squeezelite-raop/src/dmap.rs:343-356`)**:
   - In `TrackProgress::elapsed_ms` and `duration_ms`, replace `self.current_rtp.saturating_sub(self.start_rtp)` and `self.end_rtp.saturating_sub(self.start_rtp)` with `.wrapping_sub(self.start_rtp)`.
5. **`LOGIC-404` (`HIGH`, `squeezelite-cspot/src/cdn.rs:257-267`)**:
   - In `AudioKeyDecryptor::decrypt_audio_chunk`, track `let start_offset = self.byte_offset as usize;` before decrypting; if `self.strip_spotify_header && start_offset < SPOTIFY_HEADER_SIZE`, strip `let strip = (SPOTIFY_HEADER_SIZE - start_offset).min(buf.len());` across fragmented initial chunks until all 167 header bytes have been stripped.
6. **`SEC-402` (`HIGH`, `squeezelite-raop/src/dacp.rs:159-171`)**:
   - In `DacpClient::build_raw_action_request`, reject (`return None`) if `action`, `dacp_id`, or `active_remote` is empty or contains any control character (`b < 0x20 || b == 0x7F`, including `\r` and `\n`).
7. **`SEC-403` (`MEDIUM`, `squeezelite-raop/src/crypto.rs:381-385` & `squeezelite-cspot/src/crypto.rs:349-354`)**:
   - In `montgomery_mod_pow`, always compute `montgomery_mul(&acc, &base_m, n, n0_inv)` into `next` on every bit iteration and select between `next` and `acc` using a bitwise mask (`let mask = 0u32.wrapping_sub(bit);`) over the limbs to eliminate data-dependent branching.
8. **`LOGIC-405` (`MEDIUM`, `squeezelite-cspot/src/zeroconf.rs:603-621`)**:
   - In `url_decode`, push decoded raw bytes into a `Vec<u8>` and convert at the end via `String::from_utf8_lossy(&bytes).into_owned()` so multi-byte UTF-8 sequences (e.g., `%C3%A9`) are preserved.
9. **`LOGIC-406` (`MEDIUM`, `squeezelite-cspot/src/{mercury,spirc,zeroconf}.rs`)**:
   - In `skip_unknown_field` (across `mercury.rs:329-343`, `spirc.rs:241-255`, `zeroconf.rs:584-598`), handle protobuf wire type `1` (64-bit fixed: advance `*pos += 8` with bounds check) and wire type `5` (32-bit fixed: advance `*pos += 4` with bounds check).
10. **`LOGIC-407` (`MEDIUM`, `squeezelite-raop/src/rtp.rs:1032-1043`)**:
    - In `SyncDriftTracker::record_rtp_sync`, compute signed deltas `let dt_ntp = (ntp_timestamp as i64).wrapping_sub(prev_ntp as i64);` and `let drtp = i64::from(rtp_timestamp.wrapping_sub(prev_rtp) as i32);`, returning `SyncCorrection::Hold` if `dt_ntp <= 0`.
11. **`LOGIC-408` (`MEDIUM`, `squeezelite-cspot/src/spirc.rs:885-895`)**:
    - In `SpircController::set_track_queue`, add an `else { self.current_track = None; }` branch when `tracks` is empty.
12. **`SEC-404` (`LOW`, `squeezelite-raop/src/crypto.rs:1024-1064`)**:
    - In `base64_decode`, reject (`return Err(CryptoError::InvalidBase64)`) if `clean.len() % 4 == 1`.
13. **`PERF-401` (`LOW`, `squeezelite-raop/src/crypto.rs:336-353`)**:
    - In `montgomery_r2_mod_n`, replace the `Vec<u32>` allocations inside the `for _ in 0..(num_limbs * 64)` loop with fixed stack arrays `[0u32; MAX_LIMBS]` (matching `squeezelite-cspot/src/crypto.rs`).
14. **`LOGIC-409` (`LOW`, `squeezelite-raop/src/rtsp.rs:205-207`)**:
    - Add `InvalidContentLength` to `RtspError` and return `Err(RtspError::InvalidContentLength)` when the `Content-Length` header value fails `.parse::<usize>()`.

## 3. Verification & Rules
- Use `CARGO_TARGET_DIR=/tmp/target-wo253` to avoid `.cargo-lock` contention:
  - `CARGO_TARGET_DIR=/tmp/target-wo253 /home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-raop -p squeezelite-cspot`
  - `CARGO_TARGET_DIR=/tmp/target-wo253 /home/console-admin/.cargo/bin/cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-raop -p squeezelite-cspot --all-targets -- -D warnings`
  - `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-raop/src --strict`
  - `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cspot/src --strict`
- Write a subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101_wo253_cluster3_implementer.md` and update the Fulfillment Section of this Work Order before returning.
- Ensure `manage_task(Action: "list")` returns zero active background tasks (`Rule 21`) before completing.

## 4. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent Role**: `Cluster 3 AirPlay 1 & Spotify Connect Remediation Implementer`
- **Subagent ID**: `ea2f7503-1d9e-481a-91e5-e49d9b1631fd`
- **Completion Timestamp**: `2026-11-01T09:41:00-05:00`
- **Handoff Summary Path**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101_wo253_cluster3_implementer.md`
- **Findings Remediated**: All 14 findings (`SEC-401`..`404`, `LOGIC-401`..`409`, `PERF-401`) remediated exclusively in `squeezelite-raop` and `squeezelite-cspot`.
- **Verification Evidence**:
  - `cargo test -p squeezelite-raop -p squeezelite-cspot`: 55 passed (30 cspot, 25 raop), 0 failed.
  - `cargo clippy -p squeezelite-raop -p squeezelite-cspot --all-targets -- -D warnings`: 0 warnings, 0 errors.
  - `google_rust_audit.py --strict`: 0 errors, 0 warnings across both crates.
