# Work Order: WO-016 — Live Target Scan & Build Context Extraction (`172.22.1.15`)

**Work Order ID:** WO-016  
**Timestamp:** 2026-10-20T22:10:45-04:00  
**Assigned Subagent Role:** Extension Certifier / Discovery Scout  
**Target Files:** `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-016-scan-target-172-22-1-15.md` (Read-Only Network & Endpoint Discovery)  
**Backup Files:** N/A (Read-Only)  
**Defect IDs:** `HW-PREP-02`  
**Task Goal:** Scan `172.22.1.15` across L2/L3/L7 and query the LMS server (`10.0.0.10:9000`) to extract the complete hardware and build context for `172.22.1.15` (chip model, MAC address, DAC/I2C/I2S/GPIO configuration, NVS config keys, partition/OTA endpoints, and active services).

---

## 1. Scope & Instructions
1. **Routing & Reachability to `172.22.1.15`**:
   - Run `ip route get 172.22.1.15` and `ping -c 3 -W 2 172.22.1.15`.
   - Check ARP/neighbor table (`ip neigh show 172.22.1.15`) if directly reachable.
2. **Port & Service Scan on `172.22.1.15`**:
   - Scan common ports and full TCP range on `172.22.1.15` (e.g., `80`, `443`, `22`, `23`, `3483`, `8080`, `81`, `3232`, `5353`, etc.).
3. **Squeezelite-ESP32 / Web UI / REST / Telnet / SSH Interrogation on `172.22.1.15`**:
   - If HTTP (port `80` or other port) is open, query HTTP headers and Squeezelite-ESP32 REST endpoints:
     - `http://172.22.1.15/`
     - `http://172.22.1.15/config.json`
     - `http://172.22.1.15/status.json`
     - `http://172.22.1.15/aplist.json`
     - `http://172.22.1.15/messages.json`
     - Any other linked endpoints or JSON APIs.
   - If Telnet (`23`) or SSH (`22`) is open, inspect banner / non-destructive system info.
4. **Cross-Reference with Live LMS Server (`10.0.0.10:9000`)**:
   - Query `http://10.0.0.10:9000/jsonrpc.js` with `{"id":1,"method":"slim.request","params":["-",["serverstatus",0,100]]}` (local LAN query) to see all connected/known players, their IP (`172.22.1.15`), MAC address, model, firmware version, and player name.
5. **Synthesize Complete Build Context**:
   - Document exact MCU/board type (`ESP32` WROVER vs `ESP32-S3` vs other), flash/PSRAM config, DAC/I2S/I2C pinouts (`dac_config`, `set_GPIO`, `tas5805m`), Wi-Fi subnet/SSID context, player name, MAC address, and OTA/flashing method.
6. Update the Fulfillment Section of `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-016-scan-target-172-22-1-15.md` to `[EXECUTED & VERIFIED]` and return the complete structured report to the Orchestrator.

---

## 4. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Execution Timestamp:** 2026-10-20T22:15:45-04:00
- **Files Modified / Created (Absolute Paths):**
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-016-scan-target-172-22-1-15.md` (Updated Fulfillment)
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-016-scan-target-172-22-1-15.md.bak` (Backup)
- **Fulfillment Status:** `[EXECUTED & VERIFIED]`

### Empirical Findings & Hardware Profile for `172.22.1.15`

#### 1. L2 / L3 Network Reachability
- **Route:** `172.22.1.15 via 10.0.0.1 dev eno1 src 10.0.0.90` (VLAN / routed hop)
- **ICMP Ping:** 3 transmitted, 3 received, 0% packet loss (RTT min/avg/max = 17.9 / 45.0 / 62.7 ms)
- **Active Wi-Fi Association:** SSID `iot` (BSSID `ba:fb:e4:d7:70:10`, Channel 11, RSSI -51 dBm)
- **MAC Address:** `c8:2e:18:be:b8:34` (Espressif OUI `c8:2e:18`)

#### 2. L4 Port & Service Discovery (Nmap Scan Ports 1-10000 + Custom)
- **Port 53/tcp:** `open domain` (DNS responder)
- **Port 80/tcp:** `open http` (ESP32 HTTP server with REST JSON endpoints, gzip enabled)
- **Port 5000/tcp:** `open upnp` (AirPlay RTSP service)
- **Telnet (Port 23):** Closed (telnet disabled in config)
- **SSH (Port 22):** Closed

#### 3. LMS Server (`10.0.0.10:9000`) Player Cross-Reference
- **Player Name:** `"BASEMENT"`
- **Model Name:** `"SqueezeESP32"` (model: `squeezeesp32`)
- **Player ID / MAC:** `c8:2e:18:be:b8:34`
- **Connected Socket:** `172.22.1.15:56830`
- **Connection Status:** Connected = 1, Power = 1, Signal Strength = 79%
- **Firmware Version Reported by LMS:** `v1.0-1681-16` (16-bit build 1681)

#### 4. System & Hardware Profile (Extracted via `/config.json` & `/status.json`)
- **MCU Platform:** ESP32 (Xtensa dual-core ESP32-D0WD / ESP32-WROVER architecture, NOT ESP32-S3)
- **Project Name:** `Squeezelite-ESP32`
- **Firmware Target / Version:** `I2S-4MFlash-1681` (Build target `I2S-4MFlash`, 16-bit depth)
- **Flash Size:** 4MB (`CONFIG_ESPTOOLPY_FLASHSIZE_4MB=y`)
- **PSRAM (SPIRAM):** Enabled on standard ESP32 pins:
  - `clk`: GPIO 17
  - `cs`: GPIO 16
  - `spiq_sd0_io`: GPIO 7
  - `spid_sd1_io`: GPIO 8
  - `spiwp_sd3_io`: GPIO 10
  - `spihd_sd2_io`: GPIO 9
- **Amplifier / Power Control Pin:**
  - `set_GPIO`: `"33=vcc"` (GPIO 33 controls power rail/VCC for external amp/DAC)
- **I2C Bus Configuration:**
  - `i2c_config`: `"scl=27,sda=21,speed=400000,port=1"`
  - SCL: GPIO 27, SDA: GPIO 21, Speed: 400 kHz (Fast Mode), Port: 1
- **DAC / Audio Configuration:**
  - `dac_config`: `"model=I2S,bck=26,ws=25,do=22,sda=21,scl=27,i2c=45"`
    - I2S BCK (Bit Clock): GPIO 26
    - I2S WS (Word Select / LRCLK): GPIO 25
    - I2S DO (Data Out): GPIO 22
    - I2C Address: `45` decimal = `0x2D` hex (TAS5805M default I2C address)
  - `dac_controlset`: `{"init":[{"reg":3,"val":2},{"reg":3,"val":3}],"poweron":[{"reg":3,"val":3}],"poweroff":[{"reg":3,"val":0}]}`
    - Matches TAS5805M Register 0x03 state transitions (0x02 = HiZ, 0x03 = Play, 0x00 = Deep Sleep)
- **Squeezelite Command Line (`autoexec1`):**
  `squeezelite -o i2s -s 10.0.0.10:3483 -r 44100-48000:500 -b 500:2000 -d all=info -n "BASEMENT"`
- **Active Services & Features:**
  - AirPlay: Enabled (Port 5000, name: `BASEMENT`)
  - Spotify / CSpot: Enabled (`{"deviceName":"BASEMENT","bitrate":160,"zeroConf":1,"volume":36863}`)
  - Bluetooth Sink / A2DP: Enabled (Sink Name: `SMSL BT4.2`, Device Name: `BASEMENT`, PIN: `1234`)
  - Telnet Server: Disabled

#### 5. Partition Table & OTA Mechanism
- **Partition Layout:** Uses `partitions.csv` (4MB layout):
  - `nvs`: 0x9000 (16 KB)
  - `otadata`: 0xD000 (8 KB)
  - `phy_init`: 0xF000 (4 KB)
  - `recovery`: 0x10000, size 0x150000 (1376 KB factory/recovery app)
  - `ota_0`: 0x160000, size 0x290000 (2624 KB main app partition)
  - `settings`: 0x3F0000, size 0x10000 (64 KB)
- **OTA Flash Endpoint:**
  - Web UI triggers `/reboot_ota.json` or `/recovery.json` to boot recovery mode.
  - In recovery mode, `/flash.json` accepts the new firmware binary.

