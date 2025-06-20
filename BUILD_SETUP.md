# SqueezeliteESP32 Build Setup for VS Code

This document describes the setup completed to build the squeezelite-esp32 project in VS Code on Debian 12.

## Environment Setup

### ESP-IDF Installation

- ESP-IDF v4.3.5 installed at: `/home/console-admin/esp-idf-v4.3.5`
- Python virtual environment: `/home/console-admin/.espressif/python_env/idf4.3_py3.11_env`
- ESP32 toolchain installed and configured

### Dependencies Resolved

The following missing dependencies were cloned and configured:

1. **esp-dsp**: Cloned to `components/esp-dsp/`
2. **libtelnet**: Cloned to `components/telnet/libtelnet/`
3. **UML-State-Machine-in-C**: Cloned to `components/wifi-manager/UML-State-Machine-in-C/`
4. **protobuf**: Downgraded to v3.20.3 for nanopb compatibility

### VS Code Configuration

#### Settings (.vscode/settings.json)

- ESP-IDF path configured
- Python interpreter set to ESP-IDF virtual environment
- Tools path configured for ESP32 toolchain

#### Tasks (.vscode/tasks.json)

- Build task: `ESP-IDF: Build`
- Clean task: `ESP-IDF: Clean`
- Flash task: `ESP-IDF: Flash`
- Monitor task: `ESP-IDF: Monitor`

#### Debug Configuration (.vscode/launch.json)

- ESP32 debugging configured for OpenOCD

## Building the Project

### Using the Build Script

```bash
./build.sh
```

### Manual Build

```bash
source ~/esp-idf-v4.3.5/export.sh
idf.py build
```

### From VS Code

1. Open VS Code in the project directory
2. Press `Ctrl+Shift+P` and run "Tasks: Run Task"
3. Select "ESP-IDF: Build"

## Build Output

When successful, the build produces:

- `build/squeezelite.bin` - Main firmware
- `build/recovery.bin` - Recovery firmware
- `build/bootloader/bootloader.bin` - Bootloader

## Flashing

```bash
idf.py -p /dev/ttyUSB0 flash
```

## Monitoring

```bash
idf.py -p /dev/ttyUSB0 monitor
```

## Key Issues Resolved

### Protobuf Compatibility

The nanopb generator requires protobuf < 4.0.0 due to the removal of `MakeClass` function. The build script automatically ensures a compatible version is installed.

### Missing Submodules

Several git submodules were missing and were manually cloned:

- ESP-DSP for audio processing
- libtelnet for telnet server functionality
- UML State Machine for WiFi manager

### Environment Variables

All necessary ESP-IDF environment variables are properly configured in VS Code tasks and the build script.

## Troubleshooting

If you encounter build issues:

1. Ensure ESP-IDF v4.3.5 is properly installed
2. Check that all submodules are present in the components directory
3. Verify protobuf version is < 4.0.0
4. Run `idf.py clean` and try building again

## Project Status

✅ **COMPLETE**: The project now builds successfully and all dependencies are resolved.
