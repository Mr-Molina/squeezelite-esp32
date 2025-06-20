# Squeezelite ESP32 - VS Code Development Setup

This project is now configured for optimal development in Visual Studio Code with all build tools and IntelliSense working properly.

## Project Status ✅

- ✅ **Build System**: Working correctly with protobuf file generation
- ✅ **VS Code Tasks**: All configured (Build, Clean, Flash, Monitor)
- ✅ **C/C++ IntelliSense**: Configured with proper include paths
- ✅ **ESP-IDF Integration**: Fully configured
- ✅ **Debugging**: Ready for ESP32 debugging

## VS Code Configuration Files Added/Updated

### `.vscode/settings.json`

- ESP-IDF paths configured
- C/C++ compile commands set to use build output
- File associations and search exclusions optimized
- Enhanced IntelliSense settings

### `.vscode/c_cpp_properties.json` ✨ NEW

- Proper compiler path for ESP32 toolchain
- Include paths for components and ESP-IDF
- Preprocessor defines for ESP32 platform
- C11/C++17 standards configured

### `.vscode/tasks.json`

- Build task using `./build.sh`
- Clean task using `./clean.sh`
- Flash task using `./flash.sh`
- Monitor task using `./monitor.sh`

### `.vscode/launch.json`

- ESP32 debug configuration ready

### `.vscode/extensions.json` ✨ NEW

- Recommended extensions for ESP32 development
- C/C++ tools, CMake support, Python support

### `squeezelite-esp32.code-workspace` ✨ NEW

- Workspace file for easy project opening
- Terminal defaults to project directory

## Quick Start

1. **Open the workspace**: Double-click `squeezelite-esp32.code-workspace`
2. **Install recommended extensions** when prompted
3. **Build**: Use Ctrl+Shift+P → "Tasks: Run Task" → "Build"
4. **Flash**: Use Ctrl+Shift+P → "Tasks: Run Task" → "Flash"
5. **Monitor**: Use Ctrl+Shift+P → "Tasks: Run Task" → "Monitor"

## VS Code Tasks Available

### Essential Tasks

- **Build** - Build the project (default build task - Ctrl+Shift+B)
- **Clean** - Clean build artifacts
- **Flash** - Flash firmware to ESP32
- **Monitor** - Monitor serial output

### Troubleshooting Tasks

- **Flash (Manual Boot Mode)** - Interactive guided flash process for problematic connections
- **Setup Serial Permissions** - Fix serial port permission issues

## Build Scripts Available

1. Ensure the project has been built at least once
2. Check that `build/compile_commands.json` exists
3. Reload VS Code window (Ctrl+Shift+P → "Developer: Reload Window")

### If build tasks are not found:

1. Open the workspace file (`squeezelite-esp32.code-workspace`)
2. Ensure you're in the correct working directory

### For ESP-IDF issues:

- Verify ESP-IDF paths in `.vscode/settings.json`
- Check that ESP-IDF v4.3.5 is properly installed
- Ensure Python environment is activated

## Build Scripts Available

- `./build.sh` - Full build with protobuf generation
- `./clean.sh` - Clean build artifacts
- `./flash.sh` - Flash firmware to device
- `./monitor.sh` - Monitor serial output

All scripts are integrated as VS Code tasks and can be run from the Command Palette.
