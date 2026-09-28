# Local Development Environment Guide: Squeezelite-ESP32

This guide details the local development, compilation, and flashing workflows for the hardened **`squeezelite-esp32`** firmware on Windows, WSL, and Linux.

---

## 1. Architectural Architecture & Toolchain Boundary

Building `squeezelite-esp32` requires **ESP-IDF v4.3.5** (Git commit `6d04316cbe4dc35ea7e4885e9821bd9958ac996d`) and Xtensa GCC `8.4.0`. Modern host environments running ESP-IDF v5.x cannot build this codebase natively due to breaking API changes in FreeRTOS, TCP/IP stack (`tcpip_adapter`), RMT driver, and Bluetooth stacks.

To ensure deterministic, reproducible builds without contaminating the host system, the local development environment is partitioned into two cooperating layers:

```
┌──────────────────────────────────────────────────────────────────┐
│ HOST SYSTEM (Windows 11)                                          │
│                                                                  │
│  • VS Code IDE (IntelliSense, Code Navigation, Task Runner)      │
│  • Host Serial Flasher: Python 3 + esptool.py v4.8.1 (COMx)      │
│  • Node.js & Git CLI                                             │
└────────────────┬─────────────────────────────────────────────────┘
                 │ Volume Mount: /workspace/squeezelite-esp32
                 ▼
┌──────────────────────────────────────────────────────────────────┐
│ CONTAINER ENVIRONMENT (WSL2 Podman / Docker)                    │
│ Image: docker.io/sle118/squeezelite-esp32-idfv435                │
│                                                                  │
│  • ESP-IDF v4.3.5 Toolchain + Xtensa GCC 8.4.0                   │
│  • Python 3.8 Virtualenv (/opt/esp/python_env/idf4.3_py3.8_env)  │
│  • Node.js / NPM (Webapp React & Webpack Bundler)                │
│  • Ninja, CMake 3.16, Protobuf Compiler (protoc)                 │
└──────────────────────────────────────────────────────────────────┘
```

---

## 2. Quickstart Workflows

### Method A: VS Code Dev Containers (Recommended IDE Experience)
1. Open this repository folder in **VS Code**.
2. When prompted (or press `F1` and select `Dev Containers: Reopen in Container`), click **Reopen in Container**.
3. VS Code will spin up the official `sle118/squeezelite-esp32-idfv435` container with all extensions, paths, and compilers pre-mounted.
4. Press `Ctrl+Shift+B` to run any build task directly.

---

### Method B: PowerShell Unified Build Tool (`scripts/build.ps1`)
From any PowerShell terminal on Windows:

```powershell
# 1. Verify environment health & status
.\scripts\build.ps1 -Status

# 2. Build default I2S 16-bit firmware
.\scripts\build.ps1

# 3. Build specific hardware targets
.\scripts\build.ps1 -Target Muse          # Raspiaudio Muse Luxe
.\scripts\build.ps1 -Target SqueezeAmp     # SqueezeAmp Hardware
.\scripts\build.ps1 -Target I2S-S3         # ESP32-S3 Target

# 4. Build 32-bit audio pipeline variant
.\scripts\build.ps1 -Target I2S-4MFlash -Depth 32

# 5. Compile Web frontend (React / Webpack)
.\scripts\build.ps1 -Webapp

# 6. Interactive ESP-IDF Menuconfig
.\scripts\build.ps1 -Menuconfig

# 7. Open an interactive container bash shell
.\scripts\build.ps1 -Interactive

# 8. Clean build directory
.\scripts\build.ps1 -Clean

# 9. Flash compiled firmware to connected ESP32
.\scripts\build.ps1 -Flash                # Auto-detects COM port
.\scripts\build.ps1 -Flash -Port COM3     # Explicit COM port
```

---

### Method C: VS Code Task Runner (`Ctrl+Shift+B`)
Pre-configured tasks are available under **Terminal -> Run Task...** (or `Ctrl+Shift+B`):
- **Build Firmware (I2S-4MFlash - Default)** *(Default Build Task)*
- **Build Firmware (Muse Luxe)**
- **Build Firmware (SqueezeAmp)**
- **Build Firmware (ESP32-S3)**
- **Build Webapp (Frontend)**
- **Interactive Menuconfig (IDF)**
- **Flash Firmware to ESP32**
- **Clean Build Directory**
- **Environment Status & Diagnostic Check**

---

### Method D: WSL / Linux Native Script (`scripts/build.sh`)
Inside WSL or Linux:

```bash
# Build default I2S firmware
./scripts/build.sh

# Build specific target
./scripts/build.sh Muse

# Build webapp
./scripts/build.sh --webapp

# Interactive menuconfig
./scripts/build.sh --menuconfig

# Open interactive bash terminal
./scripts/build.sh --interactive
```

---

## 3. Hardware Targets & Artifact Outputs

When a build completes, output binaries are placed in `build/`:

| File | Description | Flash Offset |
| :--- | :--- | :--- |
| `build/bootloader/bootloader.bin` | Second-stage bootloader | `0x1000` |
| `build/partition_table/partition-table.bin` | Partition table (`partitions.csv`) | `0x8000` |
| `build/squeezelite.bin` | Application binary | `0x10000` |
| `build/flash_project_args` | Arguments file consumed by `esptool.py` | — |
| `build/size_components.txt` | Detailed component memory usage report | — |

---

## 4. Frontend Web Application Development

The device web interface lives in [`components/wifi-manager/webapp/`](file:///s:/Github/squeezelite-esp32/components/wifi-manager/webapp):
- **Source**: [`components/wifi-manager/webapp/src/`](file:///s:/Github/squeezelite-esp32/components/wifi-manager/webapp/src)
- **Compiled Assets**: `components/wifi-manager/webapp/dist/`
- **C Embedding**: Embedded into C headers/source (`webpack.c` / `webpack.h`) as gzipped byte arrays.

To develop the frontend:
1. Run `.\scripts\build.ps1 -Webapp` to compile and bundle the web interface.
2. The Webpack build automatically optimizes and gzip-compresses HTML/JS/CSS assets and updates `webpack.c`.
3. Re-build the firmware (`.\scripts\build.ps1`) to include the updated web assets into flash.

---

## 5. Serial Flashing & Diagnostics

Flashing is managed on the host through Python `esptool.py`:
- Host Python path: `C:\Users\jmolina\.espressif\python_env\idf5.4_py3.11_env\Scripts\esptool.py`
- Flashing parameters: `--chip esp32 --baud 921600 write_flash @flash_project_args`

If manual flashing via `esptool` is desired:
```powershell
cd S:\Github\squeezelite-esp32\build
& "C:\Users\jmolina\.espressif\python_env\idf5.4_py3.11_env\Scripts\python.exe" -m esptool --chip esp32 --port COM3 --baud 921600 write_flash "@flash_project_args"
```
