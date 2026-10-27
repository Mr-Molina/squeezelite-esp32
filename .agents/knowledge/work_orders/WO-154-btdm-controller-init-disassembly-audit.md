# Work Order `WO-154`: Disassemble `libbtdm_app.a` & Compare ESP-IDF `esp32/bt.c` Against `esp-radio` `btdm.rs` / `os_adapter_esp32.rs`

- **ID**: `WO-154`
- **Timestamp**: `2026-10-26T23:07:00-04:00`
- **Subagent Role**: BTDM Controller Disassembly & ESP-IDF `bt.c` Comparison Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Objectives
In `WO-153`, `louder-esp32-app` booted `ota_0` on `172.22.1.15`, acquired its Wi-Fi/DHCP lease, and then triggered a reset/panic when initializing `BleConnector::new(bt_p, Default::default())` in `ESP_BT_MODE_BTDM` (`3`) mode, causing Gate 1/2 to safely roll back to Factory recovery (`"recovery": 1`).

Perform a read-only disassembly and ESP-IDF comparison audit of `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/src/ble/{btdm.rs,os_adapter_esp32.rs}` and `libbtdm_app.a`:
1. **Check `patch_apply()` in `os_adapter_esp32.rs:169-181`**:
   - Currently `patch_apply()` only calls `config_btdm_funcs_reset()` and `config_ble_funcs_reset()`.
   - Does `libbtdm_app.a` export `config_bt_funcs_reset()`? In ESP-IDF `components/bt/controller/esp32/bt.c`, does `patch_apply()` call `config_bt_funcs_reset()` when Classic BT (`BTDM` / `BR/EDR`) is enabled? What happens if `config_bt_funcs_reset()` is NOT called before `btdm_controller_init` / `btdm_controller_enable(ESP_BT_MODE_BTDM)`?
2. **Disassemble `btdm_controller_init` & `btdm_controller_enable` in `libbtdm_app.a`**:
   - Locate `libbtdm_app.a` in `/home/console-admin/.cargo/registry/src/.../esp-wifi-sys-...` and disassemble `btdm_controller_init` and `btdm_controller_enable`.
   - Check every condition in `btdm_controller_init(config_mask, config_opts)` that can return non-zero or crash when `config_opts.mode == 3` (`ESP_BT_MODE_BTDM`):
     - What `config_mask` bits does `btdm_controller_init` check? What does `btdm_config_mask_load()` in ESP-IDF return for Dual-Mode `BTDM` with VHCI?
     - What `esp_bt_controller_config_t` fields (`controller_task_stack_size`, `ble_max_conn`, `bt_max_acl_conn`, `bt_sco_datapath`, `bt_max_sync_conn`, `magic`, etc.) are validated?
     - How much internal heap (`malloc_internal` / `malloc`) does `btdm_controller_init` allocate in `ESP_BT_MODE_BTDM` mode? (Check if `44 KB + 18 KB = 62 KB` internal heap after Wi-Fi init is enough, or if `malloc_internal` returned `NULL` because Wi-Fi already consumed ~45 KB of internal heap!)
3. **Check `malloc` / `malloc_internal` in `rust/vendor/esp-radio/src/ble/mod.rs:38-45`**:
   - In `rust/vendor/esp-radio/src/ble/mod.rs:38-45`, both `malloc` and `malloc_internal` call `malloc_internal` (or `esp_alloc::HEAP`). Wait: why did `malloc_internal` fail or succeed? Check how much `Internal` heap Wi-Fi (`WifiController::new`) + BTDM (`btdm_controller_init`) require together, and whether `malloc` (non-internal) in `ble/mod.rs` can fall back to PSRAM or if we can free more `Internal` DRAM (`dram_seg` / `dram2_seg`) or reduce `ControllerConfig` static/dynamic Wi-Fi buffer counts!
4. **Check `set_isr` / `set_isr13` & `interrupt_l3_disable` / `interrupt_l3_restore` in `btdm.rs` / `os_adapter_esp32.rs`**:
   - Does `btdm_controller_init` or `btdm_controller_enable` call `set_isr` with an interrupt number other than `5`, `7`, `8` (which currently panics at `os_adapter_esp32.rs:560`: `_ => panic!("set_isr - unsupported interrupt number {}", n)`)?

Deliver a concise, exact root-cause report with every failure point identified in `btdm.rs` / `os_adapter_esp32.rs`.

## 2. Fulfillment Section
*(To be completed by subagent)*
