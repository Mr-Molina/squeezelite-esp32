# Work Order `WO-167`: `/deep-hardware-audit` Phase 0b — Hardware Audit Architect

- **ID**: `WO-167`
- **Timestamp**: `2026-10-27T17:16:30-04:00`
- **Subagent Role**: Hardware Audit Architect (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
You are the **Hardware Audit Architect** (`/deep-hardware-audit` Phase 0b).
Read `/home/console-admin/.gemini/config/skills/deep-hardware-audit/references/discovery-protocol.md` (§2), `/home/console-admin/.gemini/config/skills/deep-hardware-audit/references/checklist-seeds.md`, and `/home/console-admin/.gemini/config/skills/deep-hardware-audit/references/finding-schema.md`.

### Merged Raw Hardware Scan Report (`WO-162`..`WO-165`)
```yaml
Raw Hardware Scan Report:
  Target Profile: Hybrid (Embedded ESP32 Target "BASEMENT" 172.22.1.15 + Remote LMS Appliance 10.0.0.10 + Local Workstation pc-1)
  1. Embedded Target "BASEMENT" (172.22.1.15):
     - SoC: ESP32-D0WDQ6 / WROVER (Sonocotta Louder-ESP32), 4 MB SPI Flash, 4 MB Quad-SPI PSRAM
     - Active Partition: ota_0 (0x160000), version "I2S-4MFlash-Rust", recovery: 0, otadata_prearmed_factory: true
     - Uptime: 26,335,695 ms (~7.31 hours continuous uptime, zero resets)
     - Wi-Fi: SSID "iot", IP 172.22.1.15/24, RSSI -48 dBm
     - PSRAM & Dual-Core: psram_used_bytes = 655,360 (640 KB), psram_free_bytes = 3,538,944 (3,456 KB); core1_active = true, core1_loop_rate = 3,584.5 Hz
     - Audio & TAS5805M (I2C0 0x2D): amp_mode = "Play", tas_fs_hz = 44100, tas_bck_ratio = 64, tas_pll_locked = true, tas_automuted = true, vol_pct = 5, again_code = 8 (-4.0 dB), i2c_acks = 92,630 (+7/3s), i2c_nacks = 0, frames_played = 1,984,500 (delta = 0, idle)
     - Bluetooth (BTDM): bt_enabled = true, bt_connected = false, bt_streaming = false, bt_rx_pkts = 17, bt_frames = 0
     - Firmware Revision & HELO Anomaly:
       * Active running firmware on 172.22.1.15 advertises: "Model=SqueezeESP32,ModelName=SqueezeESP32,AccuratePlayPoints=1,HasDigitalOut=1,MaxSampleRate=48000,pcm,wav,aif,mp3" (missing ',flc' and MaxSampleRate=96000).
       * Compiled Xtensa ELF at /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app (1,542,848 B, SHA-256 43508503..., .stack = 36,920 B, .dram2_uninit = 98,264 B) contains updated HELO ("...MaxSampleRate=96000,flc,pcm,wav,aif,mp3") and pure-Rust FlacDecoder in 4 MB PSRAM, but staged .bin was cleaned up at previous session end and has not yet been flashed to 172.22.1.15.
  2. Remote LMS Appliance (10.0.0.10 - Debian 13 aarch64, Linux 6.12.33-v8+, 906 MiB RAM):
     - CRITICAL ANOMALY: lyrionmusicserver (PID 9717) is pegged at 96.4% CPU in 'Rs' state, continuously logging Slim::Player::TranscodingHelper::enabledFormat debug entries to /var/log/squeezeboxserver/server.log and saturating TCP port 9000 listen backlog (4097/4096), causing HTTP JSON-RPC (http://10.0.0.10:9000/jsonrpc.js) and UPnP MediaRenderer.xml requests to time out!
     - Transcoding / Playlist Prefs Anomaly: In /var/lib/squeezeboxserver/prefs/server.prefs, disabledformats contains flc-wav-*-c8:2e:18:be:b8:34, flc-mp3-*-c8:2e:18:be:b8:34, flc-aif-*-c8:2e:18:be:b8:34 while player c8:2e:18:be:b8:34 is still running pre-flc firmware and has file:///var/lib/squeezeboxserver/cache/test_stereo_16bit.flac queued at mixer_volume: 5.
     - Sibling Bridges & Memory: avahi-publish-service (PID 9887, A1C6A30629A8@BASEMENT _raop._tcp 36297) and squeeze2cast (PID 10672) are running, but shairport_helper is NOT RUNNING, and swap usage is 345 MiB / 453 MiB (76%).
  3. Local Workstation (pc-1 - Debian 13 x86_64, Linux 6.12.113+deb13-amd64, i5-9600, 31 GiB RAM):
     - Serial Port Safety: /dev/ttyUSB0 (CH340 1a86:7523, separate CYD board) has passive-only udev rule (/etc/udev/rules.d/99-esp32-wrover.rules, 0 RUN+= hooks).
     - Bluetooth Adapter: hci0 (04:7F:0E:01:21:B3, Barrot Tech BT 5.4/6.0) UP RUNNING PSCAN; journalctl shows benign obexd UUID registration warning.
```

### Task Deliverables
Slice this hybrid hardware environment into **6 tightly-scoped Hardware Micro-Auditors** (`Model: "pro"`, `TypeName: "research"`) in a single parallel cohort covering:
1. `HW-AUD-01`: **LMS `10.0.0.10` Daemon CPU Spin, Listen Backlog (`4097/4096`), `TranscodingHelper` Log Flood & `disabledformats` Micro-Auditor**
2. `HW-AUD-02`: **LMS `10.0.0.10` Sibling Bridges (`ShairTunes2W` AirPlay `shairport_helper`, `UPnP/DLNA`, `CastBridge`, `SqueezeDSP`) & Memory/Swap Micro-Auditor**
3. `HW-AUD-03`: **`"BASEMENT"` (`172.22.1.15`) ESP32 Cold-Boot Resiliency, `RTC_WDT`, PSRAM `SPI0`/`SPI1`, Dual-Core `APP_CPU` & `.stack`/`.dram2_uninit` Micro-Auditor** (including `rust-pro` Bare-Metal `esp-hal 1.2.2` Cold-Boot & MMIO invariants!)
4. `HW-AUD-04`: **`"BASEMENT"` (`172.22.1.15`) Audio Clock (`I2S0`), TI `TAS5805M` (`0x2D` `automuted`/`vol_pct: 5`), & Native Lossless `FlacDecoder` PSRAM Pipeline Micro-Auditor**
5. `HW-AUD-05`: **`"BASEMENT"` (`172.22.1.15`) Dual-Mode Bluetooth (`BTDM` Classic A2DP + BLE `CA:2E:18:BE:B8:35`), 2.4 GHz Wi-Fi Coexistence & Local `hci0` RF Micro-Auditor**
6. `HW-AUD-06`: **Local Workstation (`pc-1`) Toolchain, Staged OTA Binary Packaging (`--stage-only`), & Passive Serial (`/dev/ttyUSB0`) Safety Micro-Auditor**

Return the complete **Hardware Committee Roster YAML** directly in your final response. Do NOT modify any files or system states.

## 2. Fulfillment Section
*(To be completed by subagent)*
