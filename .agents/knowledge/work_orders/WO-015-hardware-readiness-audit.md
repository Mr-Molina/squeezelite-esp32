# Work Order: WO-015 — Real Hardware, Serial Port, & ESP32 Rust Toolchain Readiness Audit

**Work Order ID:** WO-015  
**Timestamp:** 2026-10-20T22:02:45-04:00  
**Assigned Subagent Role:** Extension Certifier / Discovery Scout  
**Target Files:** `/home/console-admin/Github/squeezelite-esp32/rust/` (Read-Only Hardware & Toolchain Audit)  
**Backup Files:** N/A (Read-Only Audit)  
**Defect IDs:** `HW-PREP-01`  
**Task Goal:** Audit physical serial/USB devices connected to this host, installed ESP32 Rust cross-compilation and flashing utilities (`espup`, `espflash`, `esptool`, `~/export-esp.sh`, `.cargo/config.toml`), and local LAN LMS server reachability (`3483`) to prepare `MK5-Rust` for real hardware testing.

---

## 1. Scope & Instructions
1. Check connected serial and USB hardware (without running `dmesg` or `journalctl` unfiltered):
   - Check `/dev/ttyUSB*`, `/dev/ttyACM*`, and `/dev/serial/by-id/*`.
   - Run `lsusb` (if available) to identify connected USB-UART bridges (e.g., CP210x, CH340, FTDI, ESP32-S3 native USB JTAG/Serial).
   - If a serial port is present and `esptool.py` / `espflash` is installed, run a non-destructive `board-info` / `chip_id` read (if safe) or report available tools.
2. Check host ESP32 Rust toolchain & flashing binaries:
   - Check `command -v espup`, `command -v espflash`, `command -v cargo-espflash`, `command -v esptool.py`, `command -v esptool`, `command -v idf.py`.
   - Check `/home/console-admin/export-esp.sh`, `/home/console-admin/.rustup/toolchains/`, and `/home/console-admin/.cargo/bin/`.
   - Check if any `.cargo/config.toml` or `rust-toolchain.toml` files exist anywhere under `/home/console-admin/Github/squeezelite-esp32/rust/`.
3. Check local network state for Lyrion Music Server (LMS) / SlimProto testing:
   - Check host IP addresses (`ip -br addr`).
   - Check if UDP/TCP port `3483` or `9000` is listening locally or reachable on the LAN.
4. Append your findings to the Fulfillment Section below and return a concise structured report to the Orchestrator.

---

## 4. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Execution Timestamp:** 2026-10-20T22:07:00-04:00
- **Files Modified / Created (Absolute Paths):**
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-015-hardware-readiness-audit.md` (Updated Fulfillment Section)
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/2026-10-20_wo015_hardware_readiness_audit.md` (Subagent Summary)
- **Fulfillment Status:** `[EXECUTED & VERIFIED]`

### Audit Ground-Truth Summary

#### 1. Physical Serial & Connected Hardware
- **Connected Serial Device:** `/dev/ttyUSB0` (Symlinks: `/dev/ttyUSB-esp32 -> ttyUSB0`, `/dev/serial/by-id/usb-1a86_USB_Serial-if00-port0 -> ../../ttyUSB0`).
- **USB-to-UART Bridge:** QinHeng Electronics CH340 (`Bus 001 Device 035: ID 1a86:7523`).
- **Permissions / Access:** Device node owned by `root:dialout` with explicit POSIX ACL `user:console-admin:rw-` allowing direct read/write access without root privileges.
- **Non-Destructive Chip Interrogation (`esptool.py` v4.12.0):**
  - **Chip Type:** ESP32 (ESP32-D0WDQ6-V3, revision v3.0, Dual-core Xtensa LX6 @ 240MHz).
  - **Features:** WiFi, Bluetooth, Dual Core, 240MHz, VRef calibration in efuse.
  - **Crystal Frequency:** 40MHz.
  - **MAC Address:** `d4:d4:da:5a:e7:3c`.
  - **Flash Memory:** 4MB SPI Flash (Manufacturer ID: `5e`, Device ID: `4016`, Voltage: 3.3V).

#### 2. Host ESP32 Rust Toolchain & Flashing Binaries
- **Host Rust Toolchain:** `rustc 1.99.1`, `cargo 1.99.1` (active default toolchain: `stable-x86_64-unknown-linux-gnu`).
- **Installed Targets in Rustup:** `aarch64-unknown-linux-gnu`, `riscv32imac-unknown-none-elf`, `riscv32imc-unknown-none-elf`, `x86_64-unknown-linux-gnu`.
- **Xtensa Toolchain Status:** Xtensa targets (`xtensa-esp32-none-elf` / `xtensa-esp32-espidf`) are NOT currently installed in rustup.
- **ESP Flash & Build Tools:**
  - `esptool.py`: Operational at `/home/console-admin/.espressif/python_env/idf5.5_py3.13_env/bin/esptool.py` (v4.12.0).
  - `espup`: Not installed.
  - `espflash` / `cargo-espflash`: Not installed.
  - `~/export-esp.sh`: Not present.
  - `/home/console-admin/Github/squeezelite-esp32/rust/.cargo/config.toml`: Configured for Xtensa targets (`xtensa-esp32-none-elf`, `xtensa-esp32s3-none-elf`, `xtensa-esp32-espidf`, `xtensa-esp32s3-espidf`) with runner `espflash flash --monitor` and `ESP_IDF_VERSION = "v5.4.1"`.
- **Readiness Gap:** To compile MK5-Rust for the attached physical `ESP32-D0WDQ6-V3` chip and flash it via `cargo run` / `cargo-espflash`, the Xtensa Rust toolchain must be installed (`cargo install espup && espup install`) along with `espflash` (`cargo install espflash`).

#### 3. LAN & Lyrion Music Server (LMS) Discovery
- **Local Host IP:** `10.0.0.90` (interface `eno1`).
- **Live LMS Server Discovered:** `10.0.0.10` (`lyrion.molinadeveloping.com`, MAC `B8:27:EB:4B:78:45`, Raspberry Pi).
- **Service Verification:**
  - **SlimProto Control (TCP 3483):** OPEN and accepting connections.
  - **SlimProto Discovery (UDP 3483):** OPEN and active.
  - **LMS Web / API (TCP 9000):** OPEN (`HTTP/1.1 200 OK`, `Server: Lyrion Music Server (9.1.1 - 1779815526)`).
