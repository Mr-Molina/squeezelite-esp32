# Work Order `WO-183`: `/deep-code-audit` Phase 1 (`AUD-10`) — Vendor `esp-radio` BTDM Dual-Mode Controller & OS Adapters Auditor

- **ID**: `WO-183`
- **Timestamp**: `2026-10-27T17:26:00-04:00`
- **Subagent Role**: `AUD-10` — Vendor ESP-Radio BTDM Dual-Mode Controller & OS Adapters Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Scope & Instructions
You are **Micro-Auditor `AUD-10`** (`/deep-code-audit` Phase 1 Cohort 2 + `rust-pro`).
Read `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md` and `/home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/SKILL.md`.

### Assigned Files & Line Ranges (STRICT BOUNDARY)
- `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/src/ble/btdm.rs` (459 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/src/ble/mod.rs` (312 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/src/ble/os_adapter_esp32.rs` (696 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/ld/esp32_provides.x` (62 lines)

### Domain Checklist & Hardware / `rust-pro` Invariants to Verify
1. **BTDM Dual-Mode Controller, OS Adapter & `DRAM2` Memory Budget**:
   - Audit `btdm_controller_init` and `btdm_controller_enable(esp_bt_mode_t_ESP_BT_MODE_BTDM)` (`mode = 3`) in `btdm.rs` and `os_adapter_esp32.rs`.
   - Audit `STATIC_BT_HEAP` (`34,816` B in `.dram2_uninit`), bump allocator alignment, `osi_funcs_ro` wrapper callbacks (`semphr_take`, `mutex_lock`, `queue_send`, `task_create`), and `config_bt_funcs_reset()` in `patch_apply()`.
   - Verify `ld/esp32_provides.x` ROM symbol bindings (`btdm_osi_funcs_register`, `r_ld_acl_*`, `r_lm_*`, `r_lc_*`).
2. **Output Format**:
   - Return ONLY a structured YAML `findings:` list matching `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`. Do NOT modify any files.

## 2. Fulfillment Section
- **Subagent ID**: `a6128c5c-d133-4418-a7a8-c35bceeb2239`
- **Status**: `[EXECUTED & VERIFIED]`
- **Summary**: Audited `vendor/esp-radio/src/ble/{btdm,mod,os_adapter_esp32}.rs` and `ld/esp32_provides.x`. Identified 12 findings (`MEM-001` bounded `rx_queue` depth in `notify_host_recv` to prevent heap exhaustion; `MEM-002` `MAX_HCI_PACKET_LEN` `1024 -> 1048` for 3-DH5 ACL frames; `LOGIC-001` `btdm_controller_enable` return status check; `MEM-003` null-check `handle` in `task_create`; `INFRA-001` deduplicate `EXTERN`/`PROVIDE` lines in `esp32_provides.x`).
