# Work Order `WO-191`: `/deep-code-audit` Phase 3 (`Wave CODE-3`) — Bluetooth Classic A2DP Sink (`bt_a2dp.rs`) & Vendor `esp-radio` BTDM Remediation

- **ID**: `WO-191`
- **Timestamp**: `2026-10-27T18:28:00-04:00`
- **Subagent Role**: `Rust Bluetooth A2DP & BTDM Implementer` (`Model: "pro"`, `TypeName: "self"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Scope & Instructions
You are **Wave CODE-3 Implementer** (`/deep-code-audit` Phase 3 + `rust-pro`).
Read `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/audit_master_matrix.md` and `/home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/SKILL.md`.

### Strictly Assigned Files (DO NOT TOUCH ANY OTHER FILES)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/src/ble/btdm.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/src/ble/mod.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/src/ble/os_adapter_esp32.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/ld/esp32_provides.x`

### Required Functional & `rust-pro` Remediations
1. **`louder-esp32-app/src/bt_a2dp.rs`**:
   - **`CODE-BT-SSP-001` (`HIGH`, line 1400)**: In `EVT_IO_CAPABILITY_REQUEST`, change `reply[8] = 0x02;` to `reply[8] = 0x04; // MITM not required - General Bonding` (and update any test asserting `0x02` if present).
   - **`CODE-BT-HCI-001` (`HIGH`, lines 1320-1331)**: In `EVT_COMMAND_COMPLETE`, check `if params.len() >= 4 && params[3] == 0x00` before calling `self.on_command_complete(opcode)`. In `EVT_COMMAND_STATUS`, check `if params.len() >= 4 && params[0] == 0x00`.
   - **`CODE-BT-SBC-001` (`HIGH`, lines 392-452, 674, 772)**:
     - In `SbcDecoder::new()`, initialize `offset` with the staggered 8-subband circular offsets (`[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]` for both channels, matching `reset(8)`) OR initialize `subbands: 0` so the first `decode_frame` call invokes `self.reset(subbands)`! Initializing `offset: [[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]; 2]` in `SbcDecoder::new()` keeps `subbands: 8` AND pre-populates the exact `reset(8)` staggered offsets from construction!
     - In `SbcDecoder::calculate_bits` (lines 674 and 772), add `|| bitslice <= -32` to the `loop` break condition so abnormal/silent bitpools can never loop indefinitely or underflow `bitslice`.
   - **`CODE-BT-SBC-002` (`MEDIUM`, lines 621-678)**: In `calculate_bits` for Dual Channel (`mode == 1`), use `let ch_bitpool = if mode == 1 { if ch == 0 { (bitpool + 1) / 2 } else { bitpool / 2 } } else { bitpool };` inside the `for ch in 0..channels` loop.
   - **`CODE-BT-BOUNDS-001` (`MEDIUM`, lines 1253-1291, 1716)**:
     - In `send_cmd`: guard `if params.len() > 255 { return false; }`.
     - In `send_l2cap`: guard `if payload.len() > 1024 - 9 { return false; }`.
     - In `handle_connection_request`: wrap `self.next_local_cid = if self.next_local_cid >= 0xFF00 { 0x0040 } else { self.next_local_cid + 1 };`.
   - **`CODE-BT-ACL-001` (`HIGH`, lines 1468-1488)**: In `handle_hci_acl`, when starting reassembly of a partial L2CAP PDU, if `4 + l2cap_len > 1024`, call `self.acl_reassembly.reset(); return;` so `expected_len` can never exceed buffer capacity `1024`.
   - **`CODE-BT-SDP-001` (`HIGH`, lines 1998-2010)**: In `sdp_matches_audio_sink`, support 16-bit (`0x19`), 32-bit (`0x1A`), and 128-bit (`0x1C`, Base UUID `0000xxxx-0000-1000-8000-00805F9B34FB`) UUID queries for `0x110B`, `0x110D`, `0x0019`, `0x0100`, and `0x1002`.
   - **`CODE-BT-AVDTP-001` (`HIGH`, lines 2034-2144)**:
     - In `AVDTP_DISCOVER`: set `let in_use_flag = if self.avdtp_state != AvdtpStreamState::Idle { 0x02 } else { 0x00 };` (and keep `self.in_use` synchronized with `self.avdtp_state != AvdtpStreamState::Idle`).
     - In `handle_avdtp_signaling` wildcard `_ =>`: emit `General Reject` (`let rsp = [tx_label | 0x01, sig_id];`) instead of `Response Accept`.
     - In `handle_avdtp_media`: check `if self.avdtp_state != AvdtpStreamState::Streaming { return; }` at entry, and account for RTP extension header bit `(payload[0] & 0x10) != 0`.
   - **`GRS-012`**: Wrap all code lines `<= 100` cols and standalone comments `<= 80` cols across `bt_a2dp.rs`.
2. **`rust/vendor/esp-radio`**:
   - **`CODE-RADIO-BTDM-001`**:
     - In `vendor/esp-radio/src/ble/mod.rs`: increase `MAX_HCI_PACKET_LEN` from `1024` to `1048` (to accommodate full 3-DH5 `1029`-byte ACL frames).
     - In `vendor/esp-radio/src/ble/btdm.rs`:
       - In `notify_host_recv` (lines 75-91): guard `state.rx_queue.len() < 32` before pushing `packet` so a burst of HCI packets cannot exhaust internal DRAM heap.
       - In `task_create` (line 181): guard `if !handle.is_null() { *(handle as *mut usize) = task.as_ptr() as usize; }`.
     - In `vendor/esp-radio/ld/esp32_provides.x`: remove duplicate `EXTERN( __esp_radio_putchar );`, `EXTERN( __esp_radio_sleep );`, `EXTERN( __esp_radio_usleep );`, `PROVIDE( sleep = __esp_radio_sleep );`, and `PROVIDE( usleep = __esp_radio_usleep );` lines.
3. **Verification Gate**:
   - Run `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs --strict` and verify **0 errors and 0 warnings** (`exit code 0`).
   - Write a summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wave_code3.md`, update the Fulfillment Section of this Work Order, and send your completion report.

## 2. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent Summary Path**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wave_code3.md`
- **Key Deliverables & Verification**:
  1. `CODE-BT-SSP-001`: In `EVT_IO_CAPABILITY_REQUEST`, set `reply[8] = 0x04` (`General Bonding`) and verified via unit tests.
  2. `CODE-BT-HCI-001`: In `EVT_COMMAND_COMPLETE`, verified `params[3] == 0x00`; in `EVT_COMMAND_STATUS`, verified `params[0] == 0x00`.
  3. `CODE-BT-SBC-001`: In `SbcDecoder::new()`, initialized `subbands: 0` and pre-populated `offset` with staggered `reset(8)` indices `[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]`; bounded `bitslice` calculation with `|| bitslice <= -32`.
  4. `CODE-BT-SBC-002`: Implemented Dual-Channel (`mode == 1`) `ch_bitpool` splitting across channels.
  5. `CODE-BT-BOUNDS-001`: Guarded `send_cmd` (`params.len() > 255`), `send_l2cap` (`payload.len() > 1015`), and wrapped `next_local_cid` at `0xFF00 -> 0x0040`.
  6. `CODE-BT-ACL-001`: Added `4 + l2cap_len > 1024` guard invoking `acl_reassembly.reset()` on oversized L2CAP PDUs.
  7. `CODE-BT-SDP-001`: Expanded `sdp_matches_audio_sink` to match 16-bit (`0x19`), 32-bit (`0x1A`), and 128-bit (`0x1C`, Base UUID) queries for `0x110B`, `0x110D`, `0x0019`, `0x0100`, and `0x1002`.
  8. `CODE-BT-AVDTP-001`: Set `in_use_flag` from `self.avdtp_state != AvdtpStreamState::Idle`, returned General Reject (`0x01`) on wildcard unknown signaling, guarded `handle_avdtp_media` with `Streaming` state check, and parsed/skipped RTP extension headers.
  9. `CODE-RADIO-BTDM-001`: In `vendor/esp-radio`, bumped `MAX_HCI_PACKET_LEN` to 1048, bounded `rx_queue` to `< 32`, added null guard in `task_create`, and removed duplicate symbol lines in `ld/esp32_provides.x`.
  10. `GRS-012`: All code lines <= 100 columns and comments <= 80 columns across `bt_a2dp.rs`.
  11. `google_rust_audit.py --strict`: **0 errors, 0 warnings** (exit code 0).
  12. Unit tests: 10/10 passed via direct rustc test compilation. AST verified via nightly unpretty AST tree.
