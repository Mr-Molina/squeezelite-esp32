# Work Order: WO-230 — Wave 8B: Phase 4 Bluetooth Completion — AVRCP Controller/Target, Persistent Link-Key Bonding, & A2DP Source (`-o BT`) SBC Encoder (`bt_a2dp.rs`)

- **Work Order ID**: `WO-230`
- **Timestamp**: `2026-11-01T00:18:30-04:00`
- **Assigned Subagent Role**: `Rust Phase 4 Bluetooth Implementer` (`TypeName: "self"`)
- **Target Host / Component**: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Hypothesis
Complete all remaining Phase 4 items in `/home/console-admin/Github/squeezelite-esp32/docs/superpowers/plans/2026-10-29-rust-full-parity-roadmap.md` (§Phase 4) inside `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs`:
1. **AVRCP 1.6 Controller + Target (`PSM 0x0017` AVCTP)**:
   - Implement bidirectional absolute volume (`0x50` `SetAbsoluteVolume` `0x00..=0x7F` <-> `0..=100%` and `0x31` `RegisterNotification` `EVENT_VOLUME_CHANGED = 0x0D` interim/changed responses).
   - Implement `PassThrough` (`0x7C`) command builder & parser for `Play` (`0x44`), `Stop` (`0x45`), `Pause` (`0x46`), `Forward`/`Next` (`0x4B`), and `Backward`/`Prev` (`0x4C`).
   - Implement `GetElementAttributes` (`0x20` PDU on Company ID `0x001958` Bluetooth SIG) parser & response builder for track metadata (`Title = 0x01`, `Artist = 0x02`, `Album = 0x03`, `DurationMs = 0x07` -> `AvrcpTrackMetadata`) so metadata flows to CYD/display and LMS idle display.
2. **Persistent Link-Key Bonding & Configurable Security/Name**:
   - Implement `BondingStore` (4-slot LRU `[BondedPeer; 4]` with 6-byte `bd_addr` + 16-byte `link_key` + `key_type` + hex/binary serialization for `FlashJournal` key `"bt.bonds"`).
   - Handle HCI `LinkKeyRequest` (`0x17`) -> `LinkKeyRequestReply` (`OGF 0x01, OCF 0x000B`) when `bd_addr` is in `BondingStore` (or `LinkKeyRequestNegativeReply` `0x000C` otherwise), and capture HCI `LinkKeyNotification` (`0x18`) into `BondingStore`.
   - Support configurable Bluetooth device name and SSP/PIN policy (`BtSecurityPolicy`).
3. **A2DP Source (`-o BT`) & Pure-Rust SBC Encoder**:
   - Implement `SbcEncoder` (pure-Rust `#![forbid(unsafe_code)]` 8-subband, 16-block Stereo/Joint-Stereo SBC polyphase analysis filterbank + scale-factor calculator + bitpool allocator + frame packer producing valid `0x9C`-syncword SBC frames at 44.1 kHz, round-trip verified through `SbcDecoder`!).
   - Implement `BtA2dpSourceState` & inquiry/scan matcher (`InquiryResult` / Extended Inquiry Response `EIR` name parser matching configured target sink name/MAC) + AVDTP Source signaling sequence (`Discover` -> `GetCapabilities` -> `SetConfiguration` -> `Open` -> `Start` -> RTP/SBC media packet builder).

## 2. Hard Safety Constraints & Disjoint Ownership
1. **Strict Absolute Paths (`Rule 24`)**: Every file path and command must begin with `/`. Never use `sed`, `awk`, or `cat` for file modifications (`Rule 23`).
2. **Disjoint File Ownership**:
   - You **OWN** strictly `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs`.
   - Do **NOT** modify any other file in the repository!
   - Keep 100% of existing public types and methods on `BtA2dpSink` backward-compatible so `main.rs` continues to compile without breakage.
3. **Google Rust Style & API Guidelines (`GRS-001`–`GRS-012`)**:
   - `#![forbid(unsafe_code)]`, single-module `use` (`GRS-001`), `#[non_exhaustive]` on public structs/enums (`GRS-009`), zero `.unwrap()` in non-test code (`GRS-010`), no `get_` prefix on getters (`GRS-011`), and line widths `<= 100` code / `<= 80` comments (`GRS-012`).
4. **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` returns zero active tasks before completing.

## 3. Verification & Deliverables
1. Add comprehensive unit tests in `bt_a2dp.rs` covering AVRCP `SetAbsoluteVolume`, `EVENT_VOLUME_CHANGED`, `PassThrough` (`Play`/`Pause`/`Next`/`Prev`), `GetElementAttributes` metadata parsing, `BondingStore` link-key lookup/persistence + HCI `LinkKeyRequestReply`, `SbcEncoder` -> `SbcDecoder` 44.1 kHz stereo audio roundtrip, and A2DP Source EIR name matching + AVDTP initiator state machine.
2. Run `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs --strict` (`0 error(s), 0 warning(s)`).
3. Append Fulfillment Summary to §4 of this file and write subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo230_wave8b_phase4_bluetooth.md`.

---

## 4. Fulfillment Section

- **Status**: `[EXECUTED & VERIFIED]`
- **Timestamp**: `2026-11-01T00:47:00-04:00`
- **Subagent Summary Path**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo230_wave8b_phase4_bluetooth.md`
- **Files Modified**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs`
- **Summary of Changes**:
  1. **AVRCP 1.6 Controller + Target (`PSM 0x0017` AVCTP)**:
     - Added `AvrcpOperationId` (`Play` `0x44`, `Stop` `0x45`, `Pause` `0x46`, `Forward`/`NEXT` `0x4B`, `Backward`/`PREV` `0x4C`, `VolumeUp`, `VolumeDown`, `Mute`, `Rewind`, `FastForward`), `AvrcpButtonState`, `AvrcpMessageKind`, and `AvrcpTrackMetadata` (`title`, `artist`, `album`, `duration_ms`).
     - Implemented `avrcp_raw_to_volume_pct` and `volume_pct_to_avrcp_raw` (`0x00..=0x7F` <-> `0..=100%`), `build_avrcp_passthrough_cmd`, `build_avrcp_passthrough_rsp`, `parse_avrcp_passthrough`, `build_avrcp_set_absolute_volume_cmd`, `build_avrcp_set_absolute_volume_rsp`, `build_avrcp_register_volume_notify_cmd`, `build_avrcp_volume_notify_rsp`, `build_avrcp_get_element_attributes_cmd`, `build_avrcp_get_element_attributes_rsp`, and `parse_avrcp_element_attributes_rsp`.
     - Wired full bidirectional AVRCP handling in `BtA2dpSink::handle_avrcp` plus helper methods `avrcp_remote_cid`, `send_avrcp_passthrough`, `send_avrcp_absolute_volume`, `notify_avrcp_volume_changed`, and `request_avrcp_track_metadata`.
  2. **Persistent Link-Key Bonding & Security Policy**:
     - Added `BondedPeer` and 4-slot LRU `BondingStore` with compact binary (`<= 97` bytes, `"BTB1"` header) and ASCII hex serialization, plus `save_to_storage`, `load_from_storage`, `save_to_journal_sector`, and `load_from_journal_sector` under `FlashJournal` key `"bt.bonds"` (`BT_BONDS_CONFIG_KEY`).
     - Added `BtPairingPolicy` (`SspJustWorks`, `SspNumericComparison`, `LegacyPinOnly`, `BondedOnly`) and `BtSecurityPolicy` (configurable device name and PIN code), wired into `BtA2dpSink::step_init` and `BtA2dpSink::handle_hci_event` (`EVT_PIN_CODE_REQUEST`, `EVT_LINK_KEY_REQUEST` -> `OPCODE_LINK_KEY_REQUEST_REPLY` / `OPCODE_LINK_KEY_REQUEST_NEGATIVE_REPLY`, `EVT_LINK_KEY_NOTIFICATION`, `EVT_IO_CAPABILITY_REQUEST`, `EVT_USER_CONFIRMATION_REQUEST`).
  3. **A2DP Source (`-o BT`) & Pure-Rust SBC Encoder**:
     - Implemented zero-heap `#![forbid(unsafe_code)]` `SbcEncoder` (`8` subbands, `16` blocks, `44.1 kHz` Stereo & Joint-Stereo) using the mathematical time-reversed polyphase analysis adjoint of `SBC_PROTO_8_80M0`, `SBC_PROTO_8_80M1`, and `SYNMATRIX8`, achieving `> 60 dB` quantized round-trip SNR through `SbcDecoder` with zero extra lookup tables.
     - Implemented `parse_eir_local_name`, `eir_has_audio_sink_uuid`, `BtA2dpSourcePhase`, and `BtA2dpSourceState` (HCI GIAC Inquiry + `ExtendedInquiryResult` `0x2F` sink name/MAC matcher + `HCI_Create_Connection` `0x0405` + AVDTP Source initiator state machine `Discover` -> `GetCapabilities` -> `SetConfiguration` -> `Open` -> `Start` -> `Streaming` + `build_rtp_media_packet`).
- **Verification Evidence**:
  - `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs --strict`: `0 error(s), 0 warning(s)` (`exit code 0`).
  - `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --package louder-esp32-app --lib bt_a2dp`: `15 passed; 0 failed` (`exit code 0`).
  - `/home/console-admin/.cargo/bin/cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --package louder-esp32-app --lib -- -D warnings`: `0` warnings (`exit code 0`).
