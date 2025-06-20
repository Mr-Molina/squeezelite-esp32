# Squeezelite ESP32 - Development Summary

## Project Status ✅
- **Build System**: Working with protobuf generation
- **VS Code Integration**: Complete with IntelliSense  
- **Flash Issue**: Hardware timing problem - use manual boot mode

## Quick Start
1. **Build**: Ctrl+Shift+B or "Build" task
2. **Flash**: "Flash (Manual Boot Mode)" task (for hardware issues)
3. **Monitor**: "Monitor" task

## VS Code Tasks
- **Build** - Build project
- **Clean** - Clean build  
- **Flash** - Flash to ESP32
- **Monitor** - Serial monitor
- **Flash (Manual Boot Mode)** - For flash issues
- **Fix Boot Loop** - Recover from "invalid header" boot loops
- **Diagnose Flash** - Check flash contents
- **Setup Serial Permissions** - Fix permissions

## Flash Issues?
### Permission Errors:
- Use "Setup Serial Permissions" task

### Communication Errors ("Packet content transfer stopped"):
1. Use "Flash (Manual Boot Mode)" task
2. Hold BOOT button, press RESET, start flash, release BOOT when connecting

### Boot Loop ("invalid header: 0xffffffff"): ✅ **SOLUTION AVAILABLE**
1. Use "Fix Boot Loop" task - **PROVEN TO WORK** 
   - Complete flash erase and step-by-step recovery
   - All flash sections verified with hash checking
2. Use "Diagnose Flash" task to check what's in flash memory
3. After recovery, press RESET and use "Monitor" task to verify normal boot

## Files Reference
- `manual_flash.sh` - Interactive flash helper
- `setup_serial.sh` - Fix serial permissions  
- `COMPLETE_SOLUTION.md` - Detailed troubleshooting
- `VSCODE_SETUP.md` - VS Code configuration details
