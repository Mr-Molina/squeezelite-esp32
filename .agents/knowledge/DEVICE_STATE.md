# Device State Ledger: "BASEMENT" (`172.22.1.15`)

- **Device Name**: `"BASEMENT"`
- **Hardware Architecture**: Sonocotta Louder-ESP32 (Standard ESP32-WROVER-E / ESP32-D0WDQ6 revision v3.0, dual-core Xtensa LX6 @ 240 MHz)
- **External PSRAM**: 4 MB Quad-SPI PSRAM mapped at `0x3F80_0000` (CLK=17, CS=16; Region 0 of `esp_alloc::HEAP`, 4,194,304 B total)
- **Amplifier IC**: Texas Instruments `TAS5805M` Class-D Amp at `I2C0` address `0x2D` (`SDA=GPIO21`, `SCL=GPIO27`, speed `400 kHz`)
- **Audio DMA**: Hardware `I2S0` DMA (`BCK=GPIO26`, `WS=GPIO25`, `DOUT=GPIO22`) with 8-byte stereo frame alignment
- **Control GPIOs**: `GPIO33` (PWDN / Amp Power), `GPIO34` (FAULT input)
- **Network Interface**: Wi-Fi 802.11 b/g/n STA on SSID `"iot"`
- **IP Address**: `172.22.1.15` (DHCP)
- **Wi-Fi MAC**: `c8:2e:18:be:b8:34`
- **Bluetooth MAC**: `CA:2E:18:BE:B8:35` (A2DP Sink `0x110b`, AVDTP `0x0103`)
- **LMS Server Target**: Lyrion Music Server at `10.0.0.10` (SlimProto TCP/UDP `3483`, HTTP/CLI `9000`)

## Firmware Partition State
- **Active Running Partition**: `ota_0` (`0x160000`, 2.5 MB capacity)
- **Running Image**: Pure-Rust bare-metal `louder-esp32-app` (`xtensa-esp32-none-elf`, commit `b6fef51e`, SHA-256 `27b64b1745c0259772b676d983a55d4e076e6313ea49aec927e39b7f40b31219`, size `1,904,368` B = `70.87%` of `ota_0`)
- **Recovery / Factory Partition**: `factory` (`0x10000`, untouched C baseline `I2S-4MFlash-1681`)
- **Gate 1 Anti-Brick Status**: `otadata_prearmed_factory: true` (`0xD000..0xF000` erased to `0xFF` for instant rollback on watchdog or boot panic)
- **Live Health Metrics**: `recovery: 0`, `rust_engine: true`, `core1_active: true`, `tas_pll_locked: true`, `i2c_nacks: 0`, steady-state playback rate `~44,101.56` fps (`100.0035%` of 44.1 kHz), `2.79` I2C ACKs/s (<= 3.05 target).

## Hardware Safety Constraints (Binding)
1. **NEVER touch `/dev/ttyUSB0`** on `pc-1` (connected to separate CYD display board: `ESP32-D0WDQ6-V3`, MAC `d4:d4:da:5a:e7:3c`).
2. **NEVER call `POST http://172.22.1.15/reboot_ota.json`**.
3. **NEVER flash OTA without `--stage-only` pre-flight image validation and user authorization**.
