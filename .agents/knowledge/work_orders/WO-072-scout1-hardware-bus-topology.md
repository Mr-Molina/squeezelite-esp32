# Work Order `WO-072`: `/deep-hardware-audit` Phase 0a Scout 1 — Hardware, Bus & Storage Topology Scout

- **ID**: `WO-072`
- **Timestamp**: `2026-10-25T09:40:00-04:00`
- **Subagent Role**: Hardware, Bus & Storage Topology Scout (`Model: "flash"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
You are Phase 0a **Scout 1 (Hardware, Bus & Storage Topology Scout)** for `/deep-hardware-audit`.
Enumerate the live physical/bus/network topology across:
1. **Embedded Target `"BASEMENT"` (`172.22.1.15`, MAC `c8:2e:18:be:b8:34`, Sonocotta Louder-ESP32 `ESP32-WROVER`)**:
   - Query `curl -s http://172.22.1.15/status.json` and `ping -c 3 -W 1 172.22.1.15`.
   - Record Wi-Fi RSSI (`dBm`), IP/netmask/gateway, uptime, `frames_played`, `amp_mode`, `i2c_acks`, `i2c_nacks`.
   - Summarize the board's physical buses from `rust/crates/louder-esp32-app/src/board.rs` and `main.rs` (`I2C0` `GPIO21`/`27` -> TAS5805M `0x2D`, `I2S0` `GPIO26`/`25`/`22`, `GPIO33` `PWDN`, `GPIO34` `FAULT`, `UART2` `GPIO4`/`36`, `4 MB` SPI Flash + `4 MB` PSRAM).
2. **LMS / Bridge Host `10.0.0.10` & Local Workstation (`10.0.0.90`)**:
   - Via read-only SSH (`ssh -o BatchMode=yes 10.0.0.10 "lscpu | head -n 15; free -h; lsblk; df -h / /var/lib/squeezeboxserver; ip -br addr; ip route"`) and local `ip -br addr; ip route`, map the L2/L3 network path between `10.0.0.10` (`10.0.0.0/24`) and `172.22.1.15` (`172.22.1.0/24` IoT VLAN) — specifically checking whether multicast (mDNS `224.0.0.251:5353` and SSDP `239.255.255.250:1900`) crosses between `10.0.0.10` and `172.22.1.0/24`!
3. **Strict Constraints**:
   - Read-only inventory only (<60s).
   - **NEVER touch `/dev/ttyUSB0`** (Rule 3).
   - Return pure structured YAML inventory.

## 2. Fulfillment Section
*(To be completed)*
