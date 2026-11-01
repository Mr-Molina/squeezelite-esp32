# Subagent Summary: WO-233 — Wave 10A Live `"BASEMENT"` (`172.22.1.15`) OTA Deployment & 6-Gate Full Multi-Protocol Certification

- **Work Order ID**: `WO-233`
- **Subagent Role**: `Extension Certifier` (`92e605c6-2351-418f-8416-f0324ebabd6f`)
- **Timestamp**: `2026-11-01T01:19:00-04:00`
- **Overall Status**: `[BLOCKED / ERROR]` (1-Flash Circuit Breaker engaged after Gate 1 rollback to Factory recovery `0x10000`; root causes isolated in ELF & `main.rs`/`pipeline.rs`)

## 1. Files Created, Modified, or Deleted (Absolute Paths)
1. **Created**: `/home/console-admin/.gemini/antigravity/brain/92e605c6-2351-418f-8416-f0324ebabd6f/scratch/analyze_elf.py`
   - **Rationale**: Read-only forensic ELF section, symbol, and Xtensa `entry a1, imm12*8` stack-frame analyzer to isolate the Gate 1 boot rollback root cause without touching hardware a second time.
2. **Created**: `/home/console-admin/.gemini/antigravity/brain/92e605c6-2351-418f-8416-f0324ebabd6f/scratch/DIAGNOSTIC_LOG.md.bak` and `/home/console-admin/.gemini/antigravity/brain/92e605c6-2351-418f-8416-f0324ebabd6f/scratch/WO-233.md.bak`
   - **Rationale**: Rule 35 pre-modification backups.
3. **Modified**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/DIAGNOSTIC_LOG.md`
   - **Rationale**: Logged `WO-233` single-shot OTA flash execution, Gate 1 rollback observation, 1-Flash Circuit Breaker engagement, and the two forensic root causes isolated in `main.rs` and `pipeline.rs`.
4. **Modified**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-233-wave10a-live-basement-full-parity-certification.md`
   - **Rationale**: Updated status to `[BLOCKED / ERROR]` and appended complete §4 Fulfillment Summary.
5. **Created**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo233_wave10a_live_basement_cert.md`
   - **Rationale**: Rule 17 subagent handoff summary.

## 2. Execution & Gate Status Breakdown
- **Pre-Flight Check**: `[EXECUTED & VERIFIED]`
  - Verified `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-full-parity.bin` size = `1,793,424` B, SHA-256 = `23175117f50167b8ef1996707eafe7d4391da1e940609708e5ac8e8822580bea`.
  - Pre-flash `GET http://172.22.1.15/status.json` reported `"recovery": 0`, `"version": "I2S-4MFlash-Rust"`, `"uptime_ms": 224201334`.
- **1-Flash OTA Deployment**: `[EXECUTED & VERIFIED]`
  - Transitioned `"BASEMENT"` (`172.22.1.15`) to Factory recovery (`POST http://172.22.1.15/recovery.json`), verified `"recovery": 1` (`"version": "I2S-4MFlash-1681"`), and uploaded the staged binary strictly once via `POST http://172.22.1.15/flash.json` (`File uploaded. Flashing started.`).
- **Gate 1 — Boot Lease & Hardware Health (`> 45s`)**: `[BLOCKED / ERROR]`
  - After waiting 60 seconds for flash + `ota_0` boot + 45s lease, `GET http://172.22.1.15/status.json` returned:
    `{"project_name":"Squeezelite-ESP32","platform_name":"ESP32","version":"I2S-4MFlash-1681","recovery":1,"Jack":false,"Voltage":0,"disconnect_count":0,"avg_conn_time":0,"bt_status":0,"bt_sub_status":0,"is_i2c_locked":false,"urc":0,"if":"wifi","ip":"172.22.1.15","netmask":"255.255.255.0","gw":"172.22.1.1","ssid":"iot","rssi":-55}`
  - The `ota_0` firmware executed the Gate 1 `otadata` pre-arm (`0xD000..0xF000` erased to `0xFF`) at the start of `bring_up()` and then panicked/reset back into Factory recovery (`0x10000`).
  - Per the **1-Flash Circuit Breaker**, halted all further flashing immediately. `"BASEMENT"` (`172.22.1.15`) remains online and safe in Factory recovery (`"recovery": 1`).
- **Gates 2–6**: `[BLOCKED / ERROR]` (Blocked by Gate 1 boot rollback).

## 3. Definitive Root Causes & Remediation Prescription
1. **Root Cause 1 (CRITICAL — Uninitialized `StreamPipeline` Fields in `.dram2_uninit`)**:
   - In `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs` (lines 330–337), `WO-228` added 4 fields to `StreamPipeline`:
     - `pub eq_gains_db: [f32; 10]`
     - `pub loudness_level: u8`
     - `pub active_sample_rate_hz: u32`
     - `pub resampler: Option<Box<Resampler>>`
   - In `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (lines 1602–1661), `init_static_pipeline()` initializes `STATIC_PIPELINE: MaybeUninit<HwStreamPipeline>` in `.dram2_uninit` (`0x3fff0380`, `14,144` B) field-by-field via `core::ptr::addr_of_mut!((*ptr).field).write(...)` **without** first zeroing `STATIC_PIPELINE` (`ptr.write_bytes(0, 1)`) and **without** initializing `eq_gains_db`, `loudness_level`, `active_sample_rate_hz`, or `resampler`.
   - Because `.dram2_uninit` holds arbitrary SRAM bytes across warm reboots, `resampler: Option<Box<Resampler>>` (which uses the non-null pointer niche) is interpreted as `Some(Box<Resampler>)` pointing to a garbage address. Calling `amp_apply_boot_config()` -> `retune_equalizer()` or any `resampler` method/drop immediately causes undefined behavior (`LoadProhibited` / `StoreProhibited` / invalid heap free panic) and triggers `custom_halt()` -> `software_reset()` into Factory recovery.
   - **Fix**: In `init_static_pipeline()` (`main.rs:1602`), call `ptr.write_bytes(0, 1);` before field writes AND explicitly write:
     - `core::ptr::addr_of_mut!((*ptr).eq_gains_db).write([0.0; 10]);`
     - `core::ptr::addr_of_mut!((*ptr).loudness_level).write(0);`
     - `core::ptr::addr_of_mut!((*ptr).active_sample_rate_hz).write(44_100);`
     - `core::ptr::addr_of_mut!((*ptr).resampler).write(None);`
2. **Root Cause 2 (HIGH — `Resampler` 13 KB Stack Construction & `.stack` / Internal Heap Regression)**:
   - ELF static analysis (`analyze_elf.py`) revealed:
     - `.stack` in `louder-esp32-app` is only `27,232` B (`0x3ffd95a0`, size `0x006a60`), violating the `>= 36,000` B stack threshold (Invariant 47).
     - `<StreamPipeline>::configure_resampler` (`0x4013d4f4`) has an `entry a1, 13008` (`13,008` B) stack frame because `Box::new(Resampler::new(...))` constructs the `~12.5 KB` `Resampler` struct (containing `history: [[f32; 512]; 2]` = `4,096` B and `filter_table: [[f32; 32]; 64]` = `8,192` B) on the stack before copying to the heap.
     - `STATIC_BT_HEAP` in `.dram2_uninit` (`18 KB`) + `.bss` heap (`46 KB`) = `64 KB` total `Internal` DRAM heap, which is tight for Wi-Fi + Dual-Mode BTDM coexistence (`libbtdm_app.a` + `esp-radio`).
   - **Fix**:
     - Provide an in-place heap constructor `Resampler::new_boxed(...) -> Option<Box<Self>>` (allocating the `8,192` B filter table or `Resampler` directly in PSRAM/heap via `Box::new_zeroed()` / `Vec` without constructing `12.5 KB` on the stack).
     - Reclaim `.bss` / `.dram2_uninit` space (e.g., move large static buffers such as `STATIC_PIPELINE` `14,144` B or `NVS_PARTITION_SCRATCH` `4,096` B or `STATIC_CSPOT` `4,464` B to PSRAM `ExternalMemory` heap on startup) to restore `.stack >= 36,000` B and sufficient `Internal` DRAM heap for Wi-Fi + BTDM.
