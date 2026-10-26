# Subagent Summary: WO-023 — Audit `esp-hal 1.2.2`, `esp-storage 0.10.0`, `esp-bootloader-esp-idf 0.6.0`, and `esp-radio`/`esp-alloc`

- **Timestamp:** `2026-10-21T07:13:30-04:00`
- **Role:** `Discovery Scout / Research Specialist`
- **Work Order:** `WO-023`
- **Status:** `[EXECUTED & VERIFIED]`

## 1. Files Modified
- `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-023-audit-esp-hal-wifi-i2s-ota-crates.md`: Populated the complete technical findings in the Fulfillment Section and marked status as `[EXECUTED & VERIFIED]`.

## 2. Key Findings & Architecture Decisions

1. **`esp-hal 1.2.2` I2S & DMA**:
   - `peripherals.I2S0` and `peripherals.DMA_I2S0` are gated behind `feature = "unstable"`. Enabling `features = ["esp32", "unstable"]` is mandatory.
   - `I2s::new(peripherals.I2S0, peripherals.DMA_I2S0, tdm_config)` initializes the I2S master peripheral.
   - Pins are bound using `i2s.i2s_tx.with_bclk(...).with_ws(...).with_dout(...).build()`.
   - `tx.write(dma_buf)` returns `I2sTxDmaTransfer`, which provides `wait()` for blocking and `is_done()` for polling.
   - `dma_tx_buffer!(size)` allocates statically linked DMA descriptors and buffer.
   - `esp_hal::system::software_reset() -> !` is a safe, stable API calling ROM reset.

2. **`esp-storage 0.10.0`**:
   - Initialized via `FlashStorage::new(peripherals.FLASH)`.
   - Inherent methods `read()`, `write()`, `erase()` operate on any raw flash offset (`0x9000` NVS, `0xD000` otadata, `0x3F0000` settings).
   - `write()` automatically handles sector read-modify-write.

3. **`esp-bootloader-esp-idf 0.6.0`**:
   - `read_partition_table(&mut flash, &mut buffer)` parses partition entries.
   - `ota.set_current_app_partition(AppPartitionSubType::Factory)` resets active boot partition to recovery (`factory` at `0x10000`) by writing `0xFF` to `otadata` slots.

4. **Wi-Fi & Networking (`esp-radio 1.0.0-beta.1`)**:
   - `esp-wifi` was superseded by `esp-radio` for `esp-hal 1.2.x`!
   - `esp-radio = "1.0.0-beta.1"` depends on `esp-hal = "~1.2.0"`, exactly matching `esp-hal = 1.2.2`.
   - In `esp-radio`, `smoltcp` has been removed in favor of `embassy-net` (`embassy-net-driver = "0.2.0"`).
   - `esp-alloc = "0.11.0"` provides the heap allocator required by `esp-radio`.
   - Bare-metal Wi-Fi and heap drivers require internal unsafe blocks; `#![forbid(unsafe_code)]` must be updated to `#![deny(unsafe_code)]` with `// SAFETY:` contracts or isolated to a hardware adapter module.

5. **Wi-Fi Credential Retrieval on Target `172.22.1.15`**:
   - Squeezelite-esp32 stores Wi-Fi STA credentials in NVS partition `0x9000` under namespace `"config"`, keys `"ssid"` and `"password"`. Passwords may be prefixed with `"enc:v1:"` (XOR keyed by SHA256 of salt + MAC).
   - Recommended strategy: compile-time `option_env!("SL_WIFI_SSID")` / `option_env!("SL_WIFI_PASS")` fallback combined with runtime NVS scan or raw `0x3F0000` (`settings`) sector read.
