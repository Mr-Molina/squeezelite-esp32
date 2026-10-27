# Work Order `WO-124`: `/deep-hardware-audit` Phase 1 Micro-Auditor 3 — I2C0 Bus Interconnect, TI TAS5805M Registers & Hardware Volume

- **ID**: `WO-124`
- **Timestamp**: `2026-10-25T20:31:00-04:00`
- **Subagent Role**: I2C0 Bus Interconnect, TI TAS5805M Registers & Hardware Volume Micro-Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions (8-Section Hardware Micro-Auditor Contract)
1. **Role**: I2C0 Bus Interconnect, TI TAS5805M Registers & Hardware Volume Micro-Auditor
2. **Target & Scope**: `[BASEMENT]` (`172.22.1.15`, Sonocotta Louder-ESP32 + TI `TAS5805M` at `I2C0 0x2D`, `SDA=GPIO21`, `SCL=GPIO27`, `PWDN=GPIO33`, `FAULT=GPIO34`).
3. **Layer Mandate**: Audit bottom-up: L1 (`GPIO33` PWDN / `GPIO34` FAULT pin electrical state, `TAS5805M` fault registers `0x70..0x73`) -> L2 (`I2C0` 400 kHz transaction rate: `3.94` polls/sec = `8` I2C ACKs per `2s`, `i2c_nacks: 0`, and I2C blocking time inside `run_hardware_firmware` main loop) -> L3 (`squeezelite-tas58xx/src/lib.rs` & `louder-esp32-app/src/amp.rs`: `configure_silicon_maximization` book/page selection, `0x53`/`0x54`/`0x6B` only writable in Hi-Z or Play, `0x4C` digital volume curve) -> L4 (Volume synchronization mismatch: why LMS `10.0.0.10` reports `mixer_volume: 10` while `172.22.1.15/status.json` reports `vol_pct: 80`!).
4. **Seeds**: `PCIe, USB & Peripheral Interconnects`, `Drivers & Kernel Modules`, `Embedded / MCU / IoT Hardware`. Compliance: `NIST-800-53:CM-6`, `HW-FAULT:BUS-DROPOUT`.
5. **Pre-Identified Scout Anomalies**:
   - Anomaly A: LMS `10.0.0.10` reports `mixer_volume: 10` for `c8:2e:18:be:b8:34`, while `172.22.1.15/status.json` reports `vol_pct: 80` (`again_code: 8`). Inspect how `audg` (`SlimProto`) volume vs `/cmd.json?act=vol...` vs initial boot default (`80%`) interact in `src/main.rs`.
   - Anomaly B: Verify whether `Tas58xx::configure_silicon_maximization()` in `squeezelite-tas58xx/src/lib.rs` writes `REG_DEVICE_CTRL_1 (0x02)`, `REG_ANA_CTRL (0x53)`, `REG_AGAIN (0x54)`, and `REG_SS_CTRL0 (0x6B)` while the chip is in `Hi-Z` (`0x02`) or `Play` (`0x03`), and whether `select_page0_book0()` is guaranteed before `read_telemetry()` / `set_digital_volume()`.
6. **Shell & Script Safety**: Read-only Linux commands (`curl -sS http://172.22.1.15/status.json`) and read `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs` and `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/{amp.rs,main.rs}`. **NEVER touch `/dev/ttyUSB0`**.
7. **Constraint**: Phase 1 — Read-only telemetry and source inspection. Do NOT modify any files or configs.
8. **Output Schema**: Report findings using the 14-field hardware finding schema from `/home/console-admin/.gemini/config/skills/deep-hardware-audit/references/finding-schema.md`.

## 2. Fulfillment Section
*(To be completed by subagent)*
