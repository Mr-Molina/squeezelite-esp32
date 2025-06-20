#!/bin/bash

######################################
# ESP32 SQUEEZELITE OTA FLASH
######################################

echo "======================================"
echo "ESP32 SQUEEZELITE OTA FLASH PROCEDURE"
echo "======================================"
echo

# Check if required files exist
if [ ! -f "build/squeezelite.bin" ]; then
    echo "❌ Missing squeezelite.bin file"
    echo "Please run './build.sh' first to compile the firmware."
    exit 1
fi

echo "✅ Squeezelite firmware file found"
echo

echo "📋 This script flashes the squeezelite.bin to the OTA_0 partition"
echo "   Make sure recovery firmware is already flashed!"
echo

echo "HARDWARE PREPARATION:"
echo "===================="
echo "1. 🔌 Ensure ESP32 is connected via USB"
echo "2. 🔋 Ensure stable power supply"
echo "3. 🔗 Disconnect any external hardware from ESP32 GPIO pins"
echo

echo "MANUAL BOOT MODE PROCEDURE:"
echo "==========================="
echo "Option A - Using BOOT/RESET buttons:"
echo "1. Hold down the BOOT button (GPIO0) on your ESP32"
echo "2. Press and release the RESET button while holding BOOT"
echo "3. You should now be in boot mode"
echo
echo "Option B - Using jumper wires (if no buttons):"
echo "1. Connect GPIO0 to GND"
echo "2. Connect EN (or RST) to GND briefly, then release"
echo "3. ESP32 is now in boot mode"
echo

read -p "Press ENTER when ESP32 is in boot mode and ready to flash..."

echo
echo "Starting squeezelite OTA flash process..."
echo "========================================="

# Setup ESP-IDF environment
cd /home/console-admin/esp-idf-v4.3.5
source export.sh > /dev/null 2>&1
cd /home/console-admin/Documents/code/squeezelite-esp32-v4.3

# Use the ESP-IDF flash target for squeezelite
echo "Flashing squeezelite.bin to OTA_0 partition (0x150000)..."
echo "--------------------------------------------------------"

idf.py squeezelite-flash

if [ $? -eq 0 ]; then
    echo "✅ SQUEEZELITE APPLICATION FLASHED SUCCESSFULLY!"
    echo
    echo "NEXT STEPS:"
    echo "==========="
    echo "1. 🔌 Reset your ESP32 (press reset button or power cycle)"
    echo "2. 🏠 ESP32 will boot into RECOVERY mode first"
    echo "3. 🔄 Use the recovery console or web interface to switch to squeezelite"
    echo "4. 🎵 Once switched, ESP32 will run the full squeezelite application"
    echo
    echo "📋 To switch to squeezelite application:"
    echo "   - Via console: 'restart_ota' command"
    echo "   - Via web: Use the OTA management interface"
    echo "   - Via code: Call guided_restart_ota() function"
    echo
else
    echo "❌ Squeezelite OTA flash failed!"
    echo
    echo "TROUBLESHOOTING:"
    echo "==============="
    echo "1. 🔄 Make sure recovery firmware is flashed first"
    echo "2. 🔌 Try a different USB cable (ensure it's a data cable)"
    echo "3. 🔌 Try a different USB port"
    echo "4. 🔋 Check power supply (3.3V stable)"
    echo "5. 🧹 Run 'ESP32: Erase Flash' task first, then reflash recovery"
    echo
    echo "If problems persist, try flashing recovery first with './flash_recovery_only.sh'"
    exit 1
fi
