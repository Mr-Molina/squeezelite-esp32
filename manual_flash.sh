#!/bin/bash

# ESP32 Manual Flash Helper Script
echo "======================================"
echo "ESP32 RECOVERY FLASH PROCEDURE"
echo "======================================"
echo ""
echo "📋 This script flashes the RECOVERY firmware only"
echo "   Use './flash_squeezelite_ota.sh' for the main application"
echo ""

# Check if build directory exists
if [ ! -d "build" ]; then
    echo "❌ Build directory not found. Please run 'build.sh' first."
    exit 1
fi

# Check if required files exist
required_files=(
    "build/bootloader/bootloader.bin"
    "build/partition_table/partition-table.bin" 
    "build/recovery.bin"
    "build/squeezelite.bin"
)

for file in "${required_files[@]}"; do
    if [ ! -f "$file" ]; then
        echo "❌ Required file not found: $file"
        echo "Please run 'build.sh' first to generate all firmware files."
        exit 1
    fi
done

echo "✅ All firmware files found"
echo ""

echo "HARDWARE PREPARATION:"
echo "===================="
echo "1. 🔌 Ensure ESP32 is connected via USB"
echo "2. 🔋 Ensure stable power supply"
echo "3. 🔗 Disconnect any external hardware from ESP32 GPIO pins"
echo ""

echo "MANUAL BOOT MODE PROCEDURE:"
echo "==========================="
echo "Option A - Using BOOT/RESET buttons:"
echo "1. Hold down the BOOT button (GPIO0) on your ESP32"
echo "2. Press and release the RESET button while holding BOOT"
echo "3. You should now be in boot mode"
echo ""
echo "Option B - Using jumper wires (if no buttons):"
echo "1. Connect GPIO0 to GND"
echo "2. Connect EN (or RST) to GND briefly, then release"
echo "3. ESP32 is now in boot mode"
echo ""

read -p "Press ENTER when ESP32 is in boot mode and ready to flash..."

echo ""
echo "Starting flash process..."
echo "========================"

# Set up ESP-IDF environment
source /home/console-admin/esp-idf-v4.3.5/export.sh > /dev/null 2>&1

echo "Method 1: Using idf.py (recommended)"
echo "-----------------------------------"
if idf.py -p /dev/ttyUSB0 -b 115200 flash; then
    echo ""
    echo "🎉 SUCCESS! Flash completed successfully."
    echo "You can now release the BOOT button (if still held)."
    echo "The ESP32 should reset automatically and start running."
    exit 0
else
    echo ""
    echo "⚠️  Method 1 failed. Trying alternative method..."
    echo ""
fi

echo "Method 2: Direct esptool"
echo "------------------------"
cd build
if esptool.py --port /dev/ttyUSB0 --baud 115200 write_flash \
    0x1000 bootloader/bootloader.bin \
    0x8000 partition_table/partition-table.bin \
    0x10000 recovery.bin \
    0x150000 squeezelite.bin; then
    echo ""
    echo "🎉 SUCCESS! Flash completed successfully."
    echo "You can now release the BOOT button (if still held)."
    echo "The ESP32 should reset automatically and start running."
    exit 0
else
    echo ""
    echo "❌ Both flash methods failed."
    echo ""
fi

echo "TROUBLESHOOTING:"
echo "==============="
echo "1. 🔄 Try the entire procedure again from the beginning"
echo "2. 🔌 Try a different USB cable (ensure it's a data cable)"
echo "3. 🔌 Try a different USB port"
echo "4. 🔋 Check power supply (3.3V stable)"
echo "5. 🧹 Run 'ESP32: Erase Flash' task first, then try again"
echo "6. 📖 Check hardware documentation for your specific ESP32 board"
echo ""
echo "If problems persist, this may be a hardware issue with the ESP32 board."

exit 1
