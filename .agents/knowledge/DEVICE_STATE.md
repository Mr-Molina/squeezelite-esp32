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

---

# Dev module: "DEVBENCH" (`COM13` on the owner's Windows PC)

The shared test board. Recorded by `WO-F1-dev-module-bringup` on 2026-11-17. Everything above this line is BASEMENT and none of it applies here.

- **Hardware**: Louder-ESP32, ESP32 revision v3.1, 16 MB flash (the partition table uses the first 4 MB), 4 MB PSRAM
- **Port**: `COM13`, CH340 (`USB\VID_1A86&PID_7523`), serial log at 115200 baud
- **Wi-Fi MAC**: `ac:15:18:fd:93:9c` (this is `DEVBENCH_MAC` in `rust/scripts/devbench.py`; no other MAC may be flashed)
- **Network on 2026-11-17**: DHCP `172.25.6.102`. This network had no route to the LMS at `10.0.0.50`.
- **Device name**: `DEVBENCH`, set at build time with `SL_DEVICE_NAME=DEVBENCH`

## DEVBENCH partition layout (read from the module)

| Name | Offset | Size |
| :--- | :--- | :--- |
| `nvs` | `0x9000` | `0x4000` |
| `otadata` | `0xD000` | `0x2000` |
| `phy_init` | `0xF000` | `0x1000` |
| `recovery` (factory, C `I2S-4MFlash-1681`) | `0x10000` | `0x140000` |
| `ota_0` (Rust app) | `0x150000` | `0x2A0000` |
| `settings` | `0x3F0000` | `0x10000` |

`ota_0` is 64 KiB lower than the `0x160000` recorded for BASEMENT. The firmware does not depend on that offset.

## DEVBENCH install route and backup

- **Route A (owner decision 2026-11-17)**: the C bootloader, partition table and recovery that were already on the module stay. Agents write only the Rust app to `ota_0` (`devbench.py flash`) and the `ota_0` select sector at `0xD000` (`devbench.py select-ota0`). Never the bootloader, partition table, recovery, `nvs` or `settings`.
- **Original contents**: `C:\Users\jmolina\devbench-backup\devbench-original.bin`, 16,777,216 B, SHA-256 `aa579ae1375ffba9ee8b1a770da9ee7d791f6fc6ab900caf4d280db4466204c9`. Outside the repository; may hold credentials; never committed. Restores the module with `espflash write-bin --port COM13 0x0 <file>`.
- **Serial speed**: reads at 921600 baud corrupt on this adapter. Use 460800 or lower.

## DEVBENCH recovery markers

The firmware erases `otadata` at about 1.0 s after reset and writes the `ota_0` entry back when DHCP completes (about 3 to 4 s on this network). A reset between those two moments leaves `otadata` blank and the next boot is C recovery. That is an early reset, not a firmware failure.

| What happened | Serial log line |
| :--- | :--- |
| Rust image started | `boot: Loaded app from partition at offset 0x150000` |
| Fall-back armed | `[ANTI-BRICK GATE 1] otadata (0xD000..0xF000) erased & verified 0xFF (Factory recovery pre-armed)` |
| Past the risky window | `[DHCPv4] Acquired IP ...` on serial; `boot: ota_0 confirmed healthy; otadata pinned to ota_0` in `GET /messages.json` |
| Came up in recovery | `boot: Defaulting to factory image`, then `boot: Loaded app from partition at offset 0x10000`, `App version: I2S-4MFlash-1681`, `RECOVERY APPLICATION` |
| Over HTTP | Rust: `"recovery":0`, `"version":"I2S-4MFlash-Rust"`. Recovery: `"recovery":1`, `"version":"I2S-4MFlash-1681"` |

Way back from recovery: `python rust/scripts/devbench.py select-ota0 <branch>` under the lock. No reflash is needed if the image in `ota_0` is good.
