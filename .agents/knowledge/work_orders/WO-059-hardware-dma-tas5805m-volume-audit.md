# Work Order `WO-059`: `/deep-hardware-audit` Micro-Auditor 3 — ESP32 `I2S0` DMA (`pump_silence_if_needed`), TAS5805M Fault Counter (`i2c_acks: 280`), Volume (`audg`), & PSRAM Audit

- **ID**: `WO-059`
- **Timestamp**: `2026-10-24T17:42:00-04:00`
- **Subagent Role**: ESP32 I2S0 DMA, TAS5805M & PSRAM Hardware Auditor (`TypeName: "research"`, `Model: "pro"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Scope
Conduct a zero-tolerance bottom-up `/deep-hardware-audit` (L1 Physical/RF -> L2 I2C/I2S DMA Bus -> L3 Driver/Registers -> L4 Telemetry) on `172.22.1.15` (`"BASEMENT"`):
1. **Why `i2c_acks` increased from `183` to `280` (`+97` I2C ACKs = ~19 TAS5805M fault recovery cycles)**:
   - Inspect `Esp32HardwareI2sDma` (`rust/crates/louder-esp32-app/src/main.rs:580-705`) and `run_firmware_tick` (`src/main.rs:33-90`):
   - In `pump_silence_if_needed()` (`src/main.rs:597-608`), when `available_bytes() > 4096`, it pushes only **one `512`-byte silence buffer** (`static SILENCE: [u8; 512] = [0u8; 512];`) per tick!
   - Yet `tx_buffer` is `esp_hal::dma_tx_stream_buffer!(8192, 1024)` (`1,024`-byte descriptors), and in `esp-hal-1.2.2/src/dma/buffers/mod.rs:1553-1568`, `DmaTxStreamBuf::push` **only links a descriptor to hardware DMA (`Owner::Dma`) when a full `1,024`-byte chunk is filled**!
   - Verify whether pushing only `512` bytes when `available_bytes() > 4096` (or when `run_hardware_firmware` blocks in a slow operation or TCP poll) allows `I2S0` DMA to hit `TotalEof` (`transfer.is_done() == true`), stopping `BCK`/`WS` clocks (`tx_stop_en = 1`) and tripping TAS5805M `CLOCK_FAULT` (`0x71` bit 2), which triggers `amp.recover_from_fault()` (`+5` `i2c_acks` and an audible pop/mute dropout)!
2. **Hardware Volume & Mute Disconnection (`audg` vs TAS5805M `0x4C`)**:
   - On LMS `10.0.0.10:9000`, `"BASEMENT"` has `"mixer volume": 51`. Verify how `main.rs` processes `audg` (`eff_gain_left`, `eff_gain_right`) and why TAS5805M `0x4C` (`DIG_VOL_CTRL`) is left at `0x30` (`0 dB`) while software volume scales samples in `StreamPipeline`. Check whether `audg` scaling in `main.rs:1300-1314` has any bug (e.g., `gain_left` 16.16 fixed-point conversion to `VolumeScale` 0..100% or logarithmic vs linear curve!).
     - **WAIT**: Check `main.rs:1300-1314` right now! How does `main.rs` convert LMS `audg` `gain_left` (which in SlimProto is a **16.16 fixed-point linear multiplier where `0x0001_0000` (`65536`) = `100%` unity gain, and at `volume = 51` `gain_left` is only `~1800` out of `65536` = `2.7%`!**) into `pipeline.set_volume(...)` (`VolumeScale::from_percent`)?!
3. **External `4 MB` PSRAM (`SPIRAM`) on `172.22.1.15`**:
   - Inspect `esp-hal 1.2.2` and `esp-alloc 0.11.0` in `/home/console-admin/.cargo/registry/src/` to verify the exact API for initializing `psram` on ESP32 (`esp_hal::psram`) so `stream_storage` and `output_storage` can be expanded from `8 KB` to `256 KB+`.
4. Return structured findings (`F-HW-*`) with exact file/line citations and mathematical proof.

## 2. Mandatory Constraints
- **Read-Only Audit**: Do NOT modify repository files.
- **NEVER touch `/dev/ttyUSB0`** (Rule 3).

## 3. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `8619ce38-ad85-4c56-a8e1-3bf907f23ea4` (`ESP32 I2S0 DMA, TAS5805M & PSRAM Hardware Auditor`)
- **Key Findings (`F-HW-01`..`F-HW-05`)**:
  - `F-HW-01` & `F-HW-02`: `pump_silence_if_needed()` (`src/main.rs:611-622`) pushes a single `512`-byte buffer when `available_bytes() >= 6144`, which is `< 1024` bytes (the descriptor size in `dma_tx_stream_buffer!(8192, 1024)`), so `esp-hal`'s `advance_tx_stream_descriptors` leaves the descriptor owned by `Owner::Cpu` (`0` bytes committed to DMA!). With only `2,048` B (`5.8 ms`) remaining in DMA, any Wi-Fi jitter (> 23 ms on the `8 KB` `.bss` buffer) drains DMA to `TotalEof` (`is_tx_done() == true`), stopping `BCK`/`WS` clocks and tripping TAS5805M `CLOCK_FAULT` (`0x71` bit 2), which caused the `19..24` hardware fault recovery cycles (`+97` `i2c_acks`: `183 -> 280`) and audible dropouts/muting!
  - `F-HW-03` & `F-HW-04`: At `"mixer volume": 51`, LMS's logarithmic 50 dB curve sends `gain_left = 1842 / 65536` (`2.8%` linear amplitude = `-31.0 dB` whisper level), which is applied in software while TAS5805M `0x4C` (`DIG_VOL_CTRL`) is disconnected, discarding `5.15` bits of dynamic range and displaying `2%` on CYD.
  - `F-HW-05`: Verified `esp_hal::psram::Psram::new(peripherals.PSRAM, PsramConfig::default())` maps `4 MB` external PSRAM at `0x3F80_0000`, allowing `256 KB` `stream_storage` + `512 KB` `output_storage` (64x buffer expansion from `23 ms` to `1,486 ms`).
