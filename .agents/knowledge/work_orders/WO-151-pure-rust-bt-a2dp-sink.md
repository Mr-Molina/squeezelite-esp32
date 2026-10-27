# Work Order `WO-151`: Pure-Rust `src/bt_a2dp.rs` Dual-Mode HCI + L2CAP + SDP + AVDTP + High-Bitrate A2DP SBC Audio Sink

- **ID**: `WO-151`
- **Timestamp**: `2026-10-26T21:15:00-04:00`
- **Subagent Role**: Pure-Rust Bluetooth Classic A2DP & SBC Decoder Implementer (`Model: "pro"`, `TypeName: "self"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Strict File Scope (100% Disjoint from `WO-150`)
You may ONLY create/modify:
1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs` (new file)
2. Your fulfillment summary in this file and `.agents/knowledge/subagent_summaries/20261026_wo151_bt_a2dp_implementer.md`.
- **DO NOT modify** `Cargo.toml`, `src/lib.rs`, `src/main.rs`, or `src/http_control.rs` (`WO-150` is editing `Cargo.toml` and `src/main.rs` in parallel; `WO-152` will wire `pub mod bt_a2dp;` into `src/lib.rs` and `src/main.rs`).
- To run unit tests on `bt_a2dp.rs` without modifying `src/lib.rs`, run `/home/console-admin/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc --edition 2021 --test /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs -o /home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/test_bt_a2dp && /home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/test_bt_a2dp`.
- **NEVER touch `/dev/ttyUSB0`**.

## 2. Technical Requirements for `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs`
The module must be `no_std`-compatible (`use core::...`, zero `std` outside `#[cfg(test)]`), `#![forbid(unsafe_code)]`, and zero-heap (fixed-size buffers so the entire struct can live in `.dram2_uninit` or PSRAM).

### A. HCI Transport & Controller Initialization State Machine (`BtA2dpSink`)
Define a clean trait `pub trait HciTransport { fn read_packet(&mut self, buf: &mut [u8]) -> usize; fn write_packet(&mut self, data: &[u8]) -> bool; }` so `BtA2dpSink` is 100% testable with mock HCI packets and wraps `BleConnector` on `xtensa`.
On startup (and after any controller reset), step through the Dual-Mode initialization sequence (waiting for `HCI_Command_Complete` `0x0E` or `100 ms` timeout per step, respecting `Num_HCI_Command_Packets`):
1. `HCI_Reset` (`OGF=0x03, OCF=0x0003` -> opcode `0x0C03`).
2. `HCI_Set_Event_Mask` (`0x0C01`) -> `0x3DBFF807FFFBFFFF` (enables Inquiry Complete, Connection Complete, Connection Request, Disconnection Complete, Encryption Change, Read Remote Supported Features, Read Remote Extended Features, PIN Code Request, Link Key Request, Link Key Notification, IO Capability Request/Response, User Confirmation Request, Simple Pairing Complete, LE Meta Event).
3. `HCI_Write_Local_Name` (`0x0C13`, 248-byte parameter) -> `"BASEMENT\0..."`.
4. `HCI_Write_Class_Of_Device` (`0x0C24`, 3 bytes) -> `[0x14, 0x04, 0x24]` (`0x240414`: Major Service = Audio `0x200000` + Rendering `0x040000`, Major Device = Audio/Video `0x0400`, Minor = Loudspeaker `0x14`).
5. `HCI_Write_Simple_Pairing_Mode` (`0x0C56`, 1 byte) -> `[0x01]` (Enabled).
6. `HCI_Write_Extended_Inquiry_Response` (`0x0C52`, 241 bytes: `FEC_Required = 0x00` + 240-byte EIR data):
   - EIR AD structure 1: Complete Local Name (`0x09`) `"BASEMENT"`.
   - EIR AD structure 2: Complete List of 16-bit Service Class UUIDs (`0x03`): `0x110B` (Audio Sink), `0x110D` (Advanced Audio Distribution), `0x110E` (A/V Remote Control), `0x110C` (A/V Remote Control Target).
   - EIR AD structure 3: Tx Power Level (`0x0A`): `0x09` (+9 dBm).
7. `HCI_Write_Inquiry_Mode` (`0x0C45`, 1 byte) -> `[0x02]` (Inquiry Result with RSSI or Extended Inquiry Result format).
8. `HCI_Write_Page_Scan_Activity` (`0x0C18`, 4 bytes) -> `Interval = 0x0800` (`1.28s`), `Window = 0x0012` (`11.25ms`) and `HCI_Write_Page_Scan_Type` (`0x0C47`, 1 byte) -> `[0x01]` (Interlaced Scan).
9. `HCI_Write_Inquiry_Scan_Activity` (`0x0C1E`, 4 bytes) -> `Interval = 0x0800`, `Window = 0x0012` and `HCI_Write_Inquiry_Scan_Type` (`0x0C43`, 1 byte) -> `[0x01]` (Interlaced Scan).
10. `HCI_Write_Default_Link_Policy_Settings` (`0x080F`, 2 bytes) -> `0x0007` (Enable Role Switch + Hold + Sniff).
11. `HCI_Write_Scan_Enable` (`0x0C1A`, 1 byte) -> `[0x03]` (`Inquiry Scan + Page Scan` enabled!).
12. BLE Advertising setup so BLE scanners also see `"BASEMENT"`:
    - `HCI_LE_Set_Advertising_Parameters` (`0x2006`): `Min=0x0100`, `Max=0x0200`, `AdvType=0x00` (`ADV_IND`), `OwnAddrType=0x00`, `ChannelMap=0x07`.
    - `HCI_LE_Set_Advertising_Data` (`0x2008`): Flags (`0x01`: `0x1A` LE General Discoverable + Simultaneous LE and BR/EDR to Same Device Capable Controller/Host) + Complete 16-bit UUID `0x110B` + Complete Local Name `"BASEMENT"`.
    - `HCI_LE_Set_Scan_Response_Data` (`0x2009`): Complete Local Name `"BASEMENT"`.
    - `HCI_LE_Set_Advertising_Enable` (`0x200A`): `[0x01]`.

### B. HCI Event Handling (`0x04`) & SSP "Just Works" Auto-Pairing
- `0x04` `Connection_Request` (`bd_addr: [u8; 6], cod: [u8; 3], link_type: u8`):
  - If `link_type == 0x01` (ACL): send `HCI_Accept_Connection_Request` (`0x0409`) with `bd_addr` and `Role = 0x01` (Remain Slave, or if Master required fallback cleanly).
- `0x03` `Connection_Complete` (`status, handle, bd_addr, link_type, enc_enabled`):
  - If `status == 0x00`: record active `acl_handle = handle & 0x0FFF` and `peer_bd_addr = bd_addr`.
- `0x05` `Disconnection_Complete` (`status, handle, reason`):
  - Reset L2CAP channels, AVDTP state, and ACL reassembly buffer for that handle, and re-assert `HCI_Write_Scan_Enable(0x03)` (`0x0C1A`) so `BASEMENT` is immediately discoverable and connectable again!
- `0x16` `PIN_Code_Request` (`bd_addr`):
  - Send `HCI_PIN_Code_Request_Reply` (`0x040D`) with length `4`, PIN `b"0000"`.
- `0x17` `Link_Key_Request` (`bd_addr`):
  - Look up `bd_addr` in an 8-entry bond table (`[(bd_addr, [u8; 16], valid); 8]`). If found, send `HCI_Link_Key_Request_Reply` (`0x040B`); otherwise send `HCI_Link_Key_Request_Negative_Reply` (`0x040C`) with `bd_addr` (triggering SSP pairing).
- `0x18` `Link_Key_Notification` (`bd_addr, link_key: [u8; 16], key_type`):
  - Save `(bd_addr, link_key)` in the 8-entry bond table (replacing existing entry for `bd_addr` or oldest slot).
- `0x31` `IO_Capability_Request` (`bd_addr`):
  - Send `HCI_IO_Capability_Request_Reply` (`0x042B`) with `bd_addr`, `IO_Capability = 0x03` (`NoInputNoOutput`), `OOB_Data_Present = 0x00`, `Authentication_Requirements = 0x02` (`MITM Protection Not Required - Dedicated/General Bonding`).
- `0x33` `User_Confirmation_Request` (`bd_addr, numeric_value`):
  - Send `HCI_User_Confirmation_Request_Reply` (`0x042C`) with `bd_addr`.
- `0x13` `Number_Of_Completed_Packets`: track controller ACL buffer credits.

### C. HCI ACL Reassembly (`0x02`) & L2CAP Engine (`CID 0x0001` + Dynamic Channels)
- Reassemble multi-fragment HCI ACL packets:
  - Header: `handle_and_flags: u16` (`handle = raw & 0x0FFF`, `pb_flag = (raw >> 12) & 0x03`), `acl_len: u16`.
  - When `pb_flag == 0b10` or `0b00` (Start of L2CAP PDU): read `l2cap_len = u16::from_le_bytes([payload[0], payload[1]])` and `l2cap_cid = u16::from_le_bytes([payload[2], payload[3]])`. If `payload.len() >= 4 + l2cap_len`, dispatch immediately; otherwise copy into a `1,024`-byte reassembly buffer and append `pb_flag == 0b01` continuing fragments until `reassembly_len == 4 + l2cap_len`.
- On `L2CAP_CID_SIGNALING` (`0x0001`), iterate through one or more signaling commands in the L2CAP payload (`code: u8, ident: u8, cmd_len: u16`):
  1. `0x08` `Echo_Request`: reply with `0x09` `Echo_Response` (same `ident`, echoing data bytes -> makes `sudo l2ping C8:2E:18:BE:B8:36` pass with 0% loss!).
  2. `0x0A` `Information_Request` (`info_type: u16`):
     - `info_type == 0x0002` (Extended Features Mask): reply `0x0B` `Information_Response` with `Result = 0x0000`, `Data = [0x80, 0x00, 0x00, 0x00]` (Fixed Channels supported).
     - `info_type == 0x0003` (Fixed Channels Supported): reply `0x0B` `Information_Response` with `Result = 0x0000`, `Data = [0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]` (L2CAP Signaling `CID 0x0001`).
     - Any other `info_type`: reply `0x0B` with `Result = 0x0001` (Not supported).
  3. `0x02` `Connection_Request` (`psm: u16, scid: u16`):
     - Support `PSM 0x0001` (SDP), `PSM 0x0019` (AVDTP — first open channel on `0x0019` is `AvdtpSignaling`, second open channel on `0x0019` is `AvdtpMedia`), and `PSM 0x0017` (AVRCP Control).
     - Allocate local `dcid` (`0x0040..0x0047`), reply with `0x03` `Connection_Response` (`dcid, scid, result = 0x0000, status = 0x0000`), AND immediately queue/send our outbound `0x04` `Configuration_Request` (`dcid = peer_scid, flags = 0x0000, options = [0x01, 0x02, mtu_lo, mtu_hi]` with MTU `895` `0x037F`) so bidirectional L2CAP config completes without waiting!
     - For unknown PSM: reply `0x03` `Connection_Response` with `result = 0x0002` (PSM not supported).
  4. `0x04` `Configuration_Request` (`dcid: u16, flags: u16, options: &[u8]`):
     - Reply with `0x05` `Configuration_Response` (`scid = peer_scid, flags = flags & 0x0001, result = 0x0000` Success, empty options). If we haven't sent our outbound `Configuration_Request` for this channel yet, send it now.
  5. `0x05` `Configuration_Response`: mark channel configured.
  6. `0x06` `Disconnection_Request` (`dcid: u16, scid: u16`):
     - Reply with `0x07` `Disconnection_Response` (`dcid, scid`) and free the channel slot (if `AvdtpMedia` or `AvdtpSignaling` closed, update AVDTP state accordingly).

### D. SDP Server (`PSM 0x0001`)
Support all 3 SDP request PDUs so both `sdptool browse`, BlueZ `bluetoothd`, Android, and iOS work:
- Parse `pdu_id: u8, tid: u16, param_len: u16`.
- Expose 2 Service Records:
  - **Record 1 (`0x00010001`) — A2DP Audio Sink (`0x110B`)**:
    - `0x0000` (ServiceRecordHandle): `Uint32(0x00010001)` (`[0x09, 0x00, 0x00, 0x0A, 0x00, 0x01, 0x00, 0x01]`)
    - `0x0001` (ServiceClassIDList): `DES { UUID16(0x110B) }` (`[0x09, 0x00, 0x01, 0x35, 0x03, 0x19, 0x11, 0x0B]`)
    - `0x0004` (ProtocolDescriptorList): `DES { DES { UUID16(0x0100 L2CAP), Uint16(0x0019) }, DES { UUID16(0x0019 AVDTP), Uint16(0x0103) } }` (`[0x09, 0x00, 0x04, 0x35, 0x10, 0x35, 0x06, 0x19, 0x01, 0x00, 0x09, 0x00, 0x19, 0x35, 0x06, 0x19, 0x00, 0x19, 0x09, 0x01, 0x03]`)
    - `0x0005` (BrowseGroupList): `DES { UUID16(0x1002 PublicBrowseRoot) }` (`[0x09, 0x00, 0x05, 0x35, 0x03, 0x19, 0x10, 0x02]`)
    - `0x0009` (BluetoothProfileDescriptorList): `DES { DES { UUID16(0x110D AdvancedAudioDistribution), Uint16(0x0103) } }` (`[0x09, 0x00, 0x09, 0x35, 0x08, 0x35, 0x06, 0x19, 0x11, 0x0D, 0x09, 0x01, 0x03]`)
    - `0x0100` (ServiceName): `Text("BASEMENT")` (`[0x09, 0x01, 0x00, 0x25, 0x08, b'B', b'A', b'S', b'E', b'M', b'E', b'N', b'T']`)
    - `0x0311` (SupportedFeatures): `Uint16(0x0001)` (`[0x09, 0x03, 0x11, 0x09, 0x00, 0x01]`)
- Check searched UUIDs in `ServiceSearchPattern`:
  - If pattern contains `0x110B` (AudioSink), `0x110D` (A2DP), `0x0019` (AVDTP), `0x0100` (L2CAP), or `0x1002` (PublicBrowseRoot), match Record `0x00010001`! If pattern only requests an unsupported UUID (like HFP `0x111E` or PnP `0x1200`), return an empty match (`TotalServiceRecordCount = 0` for `0x02`, or empty `DES { }` `[0x35, 0x00]` for `0x06`) so BlueZ/Android doesn't confuse A2DP with HFP!
- Handle `0x02` (`SDP_ServiceSearchRequest` -> `0x03` `SDP_ServiceSearchResponse`), `0x04` (`SDP_ServiceAttributeRequest` -> `0x05` `SDP_ServiceAttributeResponse`), and `0x06` (`SDP_ServiceSearchAttributeRequest` -> `0x07` `SDP_ServiceSearchAttributeResponse`).

### E. AVDTP 1.3 Signaling & Media (`PSM 0x0019`) + AVRCP (`PSM 0x0017`)
- On `AvdtpSignaling` channel:
  - `tx_label = packet[0] & 0xF0`, `msg_type = packet[0] & 0x03`, `sig_id = packet[1] & 0x3F`.
  - If `msg_type == 0x00` (Command):
    - `0x01` `DISCOVER`: reply `[tx_label | 0x02, 0x01, 0x04 | (in_use << 1), 0x08]` (`SEID = 1`, Audio Sink).
    - `0x02` `GET_CAPABILITIES`: reply `[tx_label | 0x02, 0x02, 0x01, 0x00, 0x07, 0x06, 0x00, 0x00, 0x3F, 0xFF, 2, 53]`.
    - `0x0C` `GET_ALL_CAPABILITIES`: reply `[tx_label | 0x02, 0x0C, 0x01, 0x00, 0x07, 0x06, 0x00, 0x00, 0x3F, 0xFF, 2, 53, 0x08, 0x00]` (includes Delay Reporting `0x08, 0x00`).
    - `0x03` `SET_CONFIGURATION` / `0x05` `RECONFIGURE`: scan capability bytes for `0x07, 0x06, 0x00, 0x00, oct0, oct1, min_bp, max_bp` to extract `sample_rate_hz` (`48_000` if `(oct0 & 0x10) != 0` else `44_100`), reply `[tx_label | 0x02, sig_id]`.
    - `0x06` `OPEN`: set state `Open`, reply `[tx_label | 0x02, 0x06]`.
    - `0x07` `START`: set state `Streaming`, reply `[tx_label | 0x02, 0x07]`.
    - `0x08` `CLOSE`: set state `Idle`, reply `[tx_label | 0x02, 0x08]`.
    - `0x09` `SUSPEND`: set state `Open`, reply `[tx_label | 0x02, 0x09]`.
    - `0x0A` `ABORT`: set state `Idle`, reply `[tx_label | 0x02, 0x0A]`.
    - `0x0D` `DELAYREPORT`: reply `[tx_label | 0x02, 0x0D]`.
- On `AvrcpControl` channel (`PSM 0x0017`):
  - If peer sends AVCTP command (`cr == 0`), reply with AVCTP response (`cr = 1`, `ctype = 0x09` Accepted or `0x0C` Stable) so peer AVRCP state machine is satisfied.

### F. Pure-Rust A2DP SBC Audio Decoder (`SbcDecoder`)
Implement a complete, standards-compliant Bluetooth A2DP SBC decoder (`syncword == 0x9C`, 4 or 8 subbands, 4/8/12/16 blocks, Mono/Dual-Channel/Stereo/Joint-Stereo, Loudness/SNR bit allocation, and 4-band / 8-band polyphase synthesis filterbank per A2DP Spec Appendix B) that decodes RTP media packets on `AvdtpMedia` into 16-bit signed stereo PCM (`i16` / little-endian `u8` bytes) and invokes a caller-provided callback `on_pcm: FnMut(&[u8], u32)` with the decoded stereo PCM bytes and sample rate (`44100` or `48000` Hz).

### G. Unit Tests
Include at least 8 thorough unit tests in `src/bt_a2dp.rs` testing:
1. HCI initialization sequence generation (`Reset` -> `Scan_Enable(0x03)` + BLE advertising).
2. HCI `Connection_Request`, `IO_Capability_Request`, `User_Confirmation_Request`, and `Link_Key_Request` / `Link_Key_Notification` auto-pairing.
3. L2CAP `Echo_Request` -> `Echo_Response` (`l2ping`) and `Information_Request`.
4. L2CAP multi-fragment ACL reassembly (`PB = 0b10` + `PB = 0b01`).
5. SDP `ServiceSearchAttributeRequest` matching `0x110B` (`AudioSink`) vs rejecting `0x111E` (`HFP`).
6. AVDTP `DISCOVER` -> `GET_ALL_CAPABILITIES` -> `SET_CONFIGURATION` -> `OPEN` -> `START` -> `SUSPEND` -> `CLOSE`.
7. SBC frame header parsing, bit allocation, and synthesis filterbank decoding of a valid `0x9C` SBC frame into 128 stereo PCM frames (`512` bytes).
8. End-to-end RTP + A2DP SBC media packet decoding over `AvdtpMedia` channel.

## 3. Fulfillment Section
- **Fulfillment Status**: `[EXECUTED & VERIFIED]`
- **Timestamp**: `2026-10-26T22:05:00-04:00`
- **Implemented File**: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs` (2,668 lines).
- **Architectural Summary**:
  1. **Dual-Mode HCI Controller Initialization**: Implemented step-by-step state machine (`InitStep`) issuing `HCI_Reset`, `Set_Event_Mask`, `Write_Local_Name("BASEMENT")`, `Write_Class_Of_Device(0x240414)` (Major Service Audio/Rendering, Major Device Audio/Video, Minor Loudspeaker), `Write_Simple_Pairing_Mode(1)`, `Write_Extended_Inquiry_Response` (Complete Local Name `"BASEMENT"`, UUIDs `0x110B`, `0x110D`, `0x110E`, `0x110C`, Tx Power +9 dBm), `Write_Inquiry_Mode(2)`, `Write_Page_Scan_Activity` (1.28s / 11.25ms), `Write_Page_Scan_Type(1)` (Interlaced), `Write_Inquiry_Scan_Activity`, `Write_Inquiry_Scan_Type(1)` (Interlaced), `Write_Default_Link_Policy_Settings(0x0007)`, `Write_Scan_Enable(0x03)` (Inquiry + Page scan), and LE Advertising setup (`ADV_IND`, Flags `0x1A`, UUID `0x110B`, Name `"BASEMENT"`, Advertising Enable).
  2. **Classic Auto-Pairing & SSP "Just Works"**: Auto-replies to `Connection_Request` with `Accept_Connection_Request`, handles `PIN_Code_Request` with `"0000"`, manages 8-entry bond table (`BondEntry`) answering `Link_Key_Request` (or negative reply), records `Link_Key_Notification`, replies to `IO_Capability_Request` with `NoInputNoOutput`, and approves `User_Confirmation_Request`. On `Disconnection_Complete`, channels reset and `Write_Scan_Enable(0x03)` is immediately re-asserted.
  3. **HCI ACL Multi-Fragment Reassembly & L2CAP Engine**: Complete reassembly across `PB = 0b10`/`0b00` (First fragment) and `PB = 0b01` (Continuing fragments) into a fixed 1,024-byte buffer. L2CAP signaling handles `Echo_Request` -> `Echo_Response` (enabling `l2ping`), `Information_Request` (Extended Features Mask & Fixed Channels), bidirectional `Connection_Request` / `Configuration_Request` / `Disconnection_Request` for SDP (`PSM 0x0001`), AVDTP (`PSM 0x0019`), and AVRCP (`PSM 0x0017`).
  4. **SDP Server**: Responds to `ServiceSearchRequest`, `ServiceAttributeRequest`, and `ServiceSearchAttributeRequest` exposing Service Record `0x00010001` (A2DP Audio Sink `0x110B`, `0x110D`, AVDTP `0x0019`, L2CAP `0x0100`, PublicBrowseRoot `0x1002`, name `"BASEMENT"`). Rejects non-matching UUIDs (e.g. HFP `0x111E`) with empty match to prevent BlueZ / Android profile misdetection.
  5. **AVDTP 1.3 Signaling**: Handles `DISCOVER` (exposing SEID 1, Audio Sink), `GET_CAPABILITIES` and `GET_ALL_CAPABILITIES` (with Delay Reporting), `SET_CONFIGURATION` and `RECONFIGURE` (extracting sample rate 44.1kHz vs 48kHz), `OPEN`, `START`, `SUSPEND`, `CLOSE`, `ABORT`, and `DELAYREPORT`. Also accepts AVRCP commands.
  6. **Standards-Compliant Pure-Rust SBC Audio Decoder**: `#![forbid(unsafe_code)]`, `no_std`-compatible, zero-heap. Supports syncword `0x9C`, 4 and 8 subbands, 4/8/12/16 blocks, Mono/Dual/Stereo/Joint-Stereo modes, SNR and Loudness bit allocation, dequantization, butterfly mixing, and 4/8-band polyphase synthesis filterbank with exact BlueZ integer scaling and circular delay line buffer `V[2][170]`. Decodes RTP SBC frames and dispatches 16-bit signed stereo PCM to caller `on_pcm: FnMut(&[u8], u32)`.
  7. **Verification**: 100% of standalone unit tests pass with zero warnings via:
     `rustc --edition 2021 --test -D warnings /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs -o .../test_bt_a2dp`
     Output: `test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`.
