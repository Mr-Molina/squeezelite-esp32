# Subagent Fulfillment Summary: `WO-152`
- **Work Order ID**: `WO-152`
- **Role**: `louder-esp32-app` Bluetooth A2DP Integrator & `--stage-only` Packager
- **Timestamp**: `2026-10-26T23:58:00-04:00`
- **Status**: `[EXECUTED & VERIFIED]`

---

## 1. Files Created, Modified, or Deleted
| File Path (Absolute) | Operation | Rationale |
| --- | --- | --- |
| `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs` | Modified | (1) Fixed `handle_l2cap_frame` to pass `ch.remote_cid` instead of `cid` (`ch.local_cid`) to `handle_sdp_request`, `handle_avdtp_signaling`, and `handle_avrcp`, ensuring outbound L2CAP responses target peer SCID correctly. (2) Increased `BtA2dpSink::poll` packet drain loop from 8 to 16 packets per tick. (3) Added `assert_eq!` on `remote_cid` (`0x0050` and `0x0060`) in unit tests. (4) Fixed `OPCODE_WRITE_PAGE_SCAN_ACTIVITY = 0x0C1C`. (5) Fixed `Advertising_Channel_Map` index in `InitStep::LeSetAdvertisingParameters`. (6) Coalesced back-to-back L2CAP signaling responses (`CID 0x0001`). (7) Added `reset_in_place(&mut self)`. |
| `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/src/ble/os_adapter_esp32.rs` | Modified | Added `config_bt_funcs_reset()` in `patch_apply()` to initialize the Classic BT ROM function tables (`r_ip_funcs_p[0xb70 / 4]`), preventing NULL pointer dereference in `btdm_controller_task`. Sized `Config::default()` with `max_connections: 1` and `scan_duplicate_list_count: 20` to reduce controller heap consumption by ~3.5 KB. |
| `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/src/ble/btdm.rs` | Modified | Hardened BTDM OS adapter: mapped mutex operations to `crate::compat::mutex::*`, implemented `btdm_lpcycles_2_hus`, stubbed sleep check/enter/exit, replaced custom queue with common queue, restored `(1 << 3) | (1 << 4)` in `btdm_controller_init`, wired `interrupt_l3_disable` and `interrupt_l3_restore` to `interrupt_disable()` and `interrupt_enable()`, and added `yield_task()` and early return `0` on timeout in `send_hci`. |
| `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/ld/esp32_provides.x` | Modified | Added ROM symbol addresses (`PROVIDE(ld_iscan_evt_start_cbk = 0x4003b58c);`, `PROVIDE(ld_page_evt_start_cbk = 0x4003cf40);`, `PROVIDE(ld_pscan_evt_start_cbk = 0x4003e924);`, `PROVIDE(lm_get_total_pkt_type = 0x4004f928);`) for Bluetooth Link Controller ROM routines omitted in `esp-rom-sys`. |
| `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/src/ble/mod.rs` | Modified | Increased `MAX_HCI_PACKET_LEN` from `259` to `1024` and clamped packet copying in `read_next` to prevent panics on oversized incoming frames. |
| `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs` | Modified | Exported `pub mod bt_a2dp;` and re-exported `BtA2dpSink`, `HciTransport`, `AvdtpStreamState`, `InitStep`, and `SbcDecoder`. |
| `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs` | Modified | Added `"bt_enabled"`, `"bt_connected"`, `"bt_streaming"`, `"bt_rx_pkts"`, `"bt_frames"` to `HttpStatusSnapshot`, `Default::default()`, and `write_status_json` serialization. Updated unit test buffer assertions. |
| `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` | Modified | Placed `STATIC_BT_SINK` in `.dram2_uninit` as `core::mem::MaybeUninit`, initialized in place via `write_bytes(0, 1)` and `reset_in_place()`, rebalanced internal heap (46 KB in `dram_seg`, 18 KB `STATIC_BT_HEAP` in `dram2_seg`, and 12 KB `ring_storage`), implemented `Esp32VhciTransport` with length-checked `write_packet`, deferred `BleConnector::new` post-DHCPv4 lease (`assigned_ip.is_some()`), configured `WifiController` with 6 static rx buffers and 16 dynamic rx/tx buffers, polled `bt_sink`, fed decoded stereo PCM into `pipeline.ingest_stream_data` with sample rate syncing, amplifier activation, and telemetry updates. |
| `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-152-wire-bt-a2dp-and-stage-only.md` | Modified | Updated Work Order status to `[EXECUTED & VERIFIED]` with full gate verification evidence and Revision 4 fix records in Fulfillment Section. |
| `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md` | Modified | Updated centralized work order ledger for `WO-152` to `[EXECUTED & VERIFIED]`. |
| `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/verify_staged_ota_bin.py` | Created | Standalone Python audit script verifying ESP32 binary headers, partition capacity, and SHA-256 for staged OTA image. |
| `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin` | Created | Packaged OTA firmware binary generated via `espflash save-image`. |

---

## 2. Gate Verification Evidence

### Gate A: Unit and Integration Tests
- **Command**: `cargo test --workspace`
- **Result**: `0` failures across all crates. 112 tests passed in `louder-esp32-app` (including 10 tests in `bt_a2dp` with `remote_cid` assertions).

### Gate B: Clippy Linter Check
- **Command**: `cargo clippy --workspace --all-targets -- -D warnings`
- **Result**: `0` warnings. Exit code `0`.

### Gate C: Xtensa Hardware Build
- **Command**: `cd rust && source /home/console-admin/export-esp.sh && unset CARGO_TARGET_XTENSA_ESP32_NONE_ELF_RUSTFLAGS && cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`
- **Result**: Finished `release` profile [optimized] in 7.49s. Exit code `0`.

### Gate D: Section Sizing Audit
- **Command**: `readelf -S /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app`
- **Evidence**:
  - `[23] .stack`: NOBITS `3ffd6f80 027868 009080` -> `0x009080` = `36,992` bytes (`>= 36,000` bytes required, PASS).
  - `[24] .dram2_uninit`: NOBITS `3ffe7e40 027e40 017fd8` -> `0x017fd8` = `98,264` bytes (`<= 98,768` bytes limit, PASS).

### Gate E: Staged OTA Binary Audit
- **Command**: `espflash save-image --chip esp32 --flash-size 4mb --flash-mode dio --flash-freq 40mhz ...` and `verify_staged_ota_bin.py`
- **Evidence**:
  - Binary Path: `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin`
  - Size: `942,144` bytes (`22.82%` of `4,128,768` bytes; partition limit `2,686,976` bytes / `0x290000`).
  - SHA-256: `5a135bc87776ff8b8ebeec2f6fb3a87e8985d49412cf31560271c241be355bda`
  - `img_magic`: `0xE9` (`ESP_IMAGE_HEADER_MAGIC`, PASS).
  - `app_desc_magic`: `0xABCD5432` (`ESP_APP_DESC_MAGIC_WORD`, PASS).
  - `version`: `'I2S-4MFlash-Rust'` (PASS).
  - `project_name`: `'Squeezelite-ESP32'` (PASS).
  - Audit Verdict: `[STAGE-ONLY AUDIT PASS]`.

---

## 3. Known Risks & Untested Assumptions
1. **Live BTDM Coexistence Invariants**: In `--stage-only` mode, the firmware has not been flashed to `172.22.1.15`. Real-time co-existence between the Wi-Fi task and the BTDM controller on physical RF hardware will be verified in `WO-153`.
2. **Classic A2DP ACL Buffer Tuning**: SBC audio streaming throughput under heavy Wi-Fi traffic depends on timely task preemption. The bounded retry loops in `send_hci` with `yield_task()` prevent lockup, but RF coexistence scheduling requires live over-the-air validation.
