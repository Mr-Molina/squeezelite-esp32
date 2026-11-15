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
- **LMS Server Target**: Lyrion Music Server at `10.0.0.50` (SlimProto TCP/UDP `3483`, HTTP/CLI `9000`, `http://10.0.0.50:9000/jsonrpc.js`)

## Firmware Partition State
- **Active Running Partition**: `ota_0` (`0x160000`, `2,686,976` B / `0x290000` capacity)
- **Running Image**: Pure-Rust bare-metal `louder-esp32-app` (`xtensa-esp32-none-elf`, `"I2S-4MFlash-Rust"` / `"Squeezelite-ESP32"`, code baseline commit `579ffcde`, SHA-256 `954f842661487857dd95aa89eaeb1a4468a80228049b878340c314d3dfd92fa0`, size `2,007,568` B = `74.71%` of `ota_0`)
- **Recovery / Factory Partition**: `factory` (`0x10000`, untouched C baseline `I2S-4MFlash-1681`)
- **Gate 1 Anti-Brick Status (`WO-2026-1115-002`)**: `otadata_prearmed_factory: false` (single-shot OTA from `I2S-4MFlash-1681` recovery; `/messages.json` `seq 3` at `uptime_ms=12842` confirmed `"boot: ota_0 confirmed healthy; otadata pinned to ota_0"`, transitioning `otadata_prearmed_factory` from `true` to `false`; sampled pre-45s `uptime_ms=14931` and post-50s `uptime_ms=68658` past 46s boot lease)
- **Gate 2 HTTP & Zeroconf Cache (`WO-2026-1115-002`, `--interface eno1`)**: `GET /` (`200`, `32,058` B, TTFB `0.079219 s`), `/config.json` (`200`, `settings_seq=113`), `/messages.json` (`200`, `4` messages), `/commands.json` (`200`), `GET /?action=getInfo` TTFB `0.505121 s` (`505.121 ms`) cold prime / `0.014481 s` (`14.481 ms < 100 ms`) cached (`ZEROCONF_DH_CACHE`).
- **Live Health, Invariant 49 & `HW1` Metrics (`WO-2026-1115-002` Wave 3/3R, muted `vol_pct: 0`, restored `mode="stop"`, `mixer volume=50`, `tas_automuted=true`)**:
  - Core/PSRAM/I2C: `recovery: 0`, `rust_engine: true`, `core1_active: true` (`core1_loops=2,822,000` at Wave 3R close), `psram_free_bytes + psram_used_bytes = 1,441,212 + 2,753,092 = 4,194,304` B (`4 MB`), `tas_pll_locked: true`, `i2c_nacks: 0` (`read=0, write=0, write_read=0`), `disconnect_count: 0`.
  - FLAC (`45.000 s` / `1,984,500`-frame native `10.0.0.50` file, `15.629 s` window): `frames_played` rate `44,102.63` fps (`44,080..44,120` PASS), `i2c_acks/s = 1.3437` (`<= 3.05` PASS), `decode_us_max = 25,718` us (`< 46,000` us, `decode_us_first = 624` us), EOF tight-poll `peak_flac_frames_played = 1,984,500 / 1,984,500` (`100.000%` PASS), PSRAM TCP windows (`:3483` `snd_wnd:8192`, `:9000` `snd_wnd:52594`, `0` transcoders).
  - MP3 (Ibiza Global Radio, `15.612 s` window): `frames_played` rate `44,101.46` fps (`44,080..44,120` PASS), `i2c_acks/s = 1.5373` (`<= 3.05` PASS), `decode_us_max = 14,013` us (`< 46,000` us, `decode_us_first = 949` us; Wave 3 initial MP3 `decode_us_max = 12,384` us, `decode_us_first = 865` us, `1.4933` ACKs/s).
  - Network (`eno1`): `ping -I eno1 -c 20 -i 0.1 172.22.1.15` `20/20` (`0%` loss), RTT `min/avg/max/mdev = 2.539/8.785/27.055/6.688 ms`.

## Hardware Safety Constraints (Binding)
1. **NEVER touch `/dev/ttyUSB0`** on `pc-1` (connected to separate CYD display board: `ESP32-D0WDQ6-V3`, MAC `d4:d4:da:5a:e7:3c`).
2. **NEVER call `POST http://172.22.1.15/reboot_ota.json`**.
3. **NEVER flash OTA without `--stage-only` pre-flight image validation and user authorization**.
