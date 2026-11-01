# Work Order: WO-238 — Phase 0a Hardware Scout 1: `"BASEMENT"` (`172.22.1.15`) 7.5-Hour Soak Telemetry, Memory, Bus & Radio Inventory

- **Work Order ID**: `WO-238`
- **Timestamp**: `2026-11-01T08:33:00-05:00`
- **Assigned Subagent Role**: `Hardware, Bus & Radio Soak Topology Scout` (`Model: "flash"`, `TypeName: "self"`, read-only)
- **Target Host / Component**: `"BASEMENT"` (`172.22.1.15`, Wi-Fi MAC `c8:2e:18:be:b8:34`, BT MAC `CA:2E:18:BE:B8:35`) & `pc-1` passive USB/Bluetooth inventory
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Strict Discovery Guardrails
Execute a fast (<60s), strictly read-only Phase 0a hardware inventory of `"BASEMENT"` (`172.22.1.15`) after its 7.5-hour continuous soak:
1. Query all read-only HTTP telemetry and configuration surfaces on `172.22.1.15`:
   - `GET http://172.22.1.15/status.json` (2 samples spaced 3 seconds apart to compute instantaneous `frames_played/sec`, `core1_loops/sec`, `i2c_acks/sec`, `psram_free_bytes`, `psram_used_bytes`, `i2c_nacks`, `tas_pll_locked`, `tas_automuted`, `uptime_ms`).
   - `GET http://172.22.1.15/config.json`
   - `GET http://172.22.1.15/scan.json`
   - `GET http://172.22.1.15/messages.json`
   - `GET http://172.22.1.15/commands.json`
   - `GET http://172.22.1.15/?action=getInfo` (Spotify Connect ZeroConf)
2. Probe AirPlay 1 (RAOP) RTSP port `5000` with a read-only `OPTIONS * RTSP/1.0` request (`CSeq: 1`).
3. Probe Bluetooth Classic / BLE health on `CA:2E:18:BE:B8:35` via `/usr/bin/sudo -n /usr/bin/l2ping -c 1 CA:2E:18:BE:B8:35` and `/usr/bin/sdptool browse CA:2E:18:BE:B8:35`.
4. Passively list `/dev/serial/by-id/` and `/sys/bus/usb/devices/*/product` on `pc-1` (strictly NO opening or probing `/dev/ttyUSB0`).

## 2. Hard Safety Constraints
- **Read-Only Mandate**: Do NOT send any state-mutating requests (`/cmd.json`, `/flash.json`, `/recovery.json`, `/reboot.json`, `/reboot_ota.json`, RTSP `RECORD`).
- **Serial Isolation (`single-target-troubleshooting.md`)**: NEVER open or probe `/dev/ttyUSB0`.
- **Strict Absolute Paths (`Rule 24`)**: Every path and command must begin with `/`.
- **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` is empty before completing.

## 3. Deliverables
- Append YAML inventory summary to §4 of `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-238-phase0a-hardware-scout-basement-soak.md` and return the YAML summary in your final message.

---

## 4. Fulfillment Section

### 4.1 Execution Overview
- **Subagent**: `Hardware, Bus & Radio Soak Topology Scout` (`WO-238`)
- **Execution Timestamp**: `2026-11-01T08:36:11-05:00`
- **Execution Mode**: Strictly read-only, non-intrusive probe (<60s duration).
- **Serial Safety**: `/dev/ttyUSB0` was NEVER touched or opened; inspected passively via `/dev/serial/by-id/` and `/sys/bus/usb/devices/`.
- **Status**: `[EXECUTED & VERIFIED]`

### 4.2 Hardware, Telemetry & Bus Inventory
```yaml
target: BASEMENT
ip: 172.22.1.15
mac_wifi: c8:2e:18:be:b8:34
mac_bt: CA:2E:18:BE:B8:35
firmware:
  project: Squeezelite-ESP32
  platform: ESP32
  version: I2S-4MFlash-Rust
  recovery: 0
  rust_engine: true
  otadata_prearmed_factory: true
soak_telemetry:
  uptime_ms: 27623149
  uptime_hours: 7.673
  sample_interval_sec: 5.608
  frames_played: 138695417
  frames_played_per_sec: 42248.22
  core1_loops: 109673828
  core1_loops_per_sec: 3970.4
  psram_free_bytes: 1519160
  psram_used_bytes: 2675144
  codec: MP3
  codec_sample_rate: 44100
  codec_channels: 2
  codec_bit_depth: 16
  volume_pct: 51
  amp_mode: Play
i2c_tas5805m_bus_health:
  total_i2c_acks: 165470
  total_i2c_nacks: 0
  i2c_acks_per_sec: 6.95
  i2c_nacks_per_sec: 0.0
  tas_fs_hz: 44100
  tas_bck_ratio: 64
  tas_pll_locked: true
  tas_automuted: false
  again_code: 8
network_and_services:
  wifi:
    ssid: iot
    rssi_dbm: -54
    gw: 172.22.1.1
    netmask: 255.255.255.0
    scan_ap_count: 1
  lms:
    server_ip: 10.0.0.10
    cli_port: 9090
    http_port: 9000
  spotify_zeroconf:
    version: 2.7.1
    brand: cspot
    model: BASEMENT
    device_id: 142137fd329622137a149016c82e18beb834
    device_type: AudioDongle
  airplay_raop_rtsp:
    port: 5000
    status_line: RTSP/1.0 200 OK
    public_methods:
    - ANNOUNCE, SETUP, RECORD, PAUSE, FLUSH, TEARDOWN, OPTIONS, GET_PARAMETER, SET_PARAMETER
  bluetooth:
    status_in_telemetry:
      bt_enabled: true
      bt_connected: false
      bt_streaming: false
      bt_rx_pkts: 75
      bt_frames: 0
    l2ping:
      exit_code: 0
      result: "Ping: CA:2E:18:BE:B8:35 from 04:7F:0E:01:21:B3 (data size 44) ...\n44 bytes from CA:2E:18:BE:B8:35 id 0 time 64.05ms\n1 sent, 1 received, 0% loss"
    sdptool:
      exit_code: 0
      records:
      - Browsing CA:2E:18:BE:B8:35 ...
      - 'Service Name: BASEMENT'
      - 'Service RecHandle: 0x10001'
      - 'Service Class ID List:'
      - '  "Audio Sink" (0x110b)'
      - 'Protocol Descriptor List:'
      - '  "L2CAP" (0x0100)'
      - '    PSM: 25'
      - '  "AVDTP" (0x0019)'
      - '    uint16: 0x0103'
      - 'Profile Descriptor List:'
      - '  "Advanced Audio" (0x110d)'
      - '    Version: 0x0103'
host_pc1_passive_usb_serial:
  dev_serial_by_id:
    usb-1a86_USB_Serial-if00-port0: ../../ttyUSB0
  usb_product_strings:
    /sys/bus/usb/devices/1-1.1/product: 802.11ac WLAN Adapter
    /sys/bus/usb/devices/1-1.2/product: USB Receiver
    /sys/bus/usb/devices/1-1.3/product: USB3.0 Card Reader
    /sys/bus/usb/devices/1-1.4/product: UGREEN BT6.0 Adapter
    /sys/bus/usb/devices/1-1/product: USB2.0 Hub
    /sys/bus/usb/devices/1-10/product: G502 X
    /sys/bus/usb/devices/1-12/product: USB Serial
    /sys/bus/usb/devices/1-8/product: NEEWER UM04
    /sys/bus/usb/devices/usb1/product: xHCI Host Controller
    /sys/bus/usb/devices/usb2/product: xHCI Host Controller
```
