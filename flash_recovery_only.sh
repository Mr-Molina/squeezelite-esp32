#!/bin/bash

######################################
# ESP32 RECOVERY FLASH PROCEDURE
######################################

echo "======================================"
echo "ESP32 RECOVERY-ONLY FLASH PROCEDURE"
echo "======================================"
echo

# Check if required files exist
required_files=(
    "build/bootloader/bootloader.bin"
    "build/partition_table/partition-table.bin"
    "build/ota_data_initial.bin"
    "build/recovery.bin"
)

missing_files=()
for file in "${required_files[@]}"; do
    if [ ! -f "$file" ]; then
        missing_files+=("$file")
    fi
done

if [ ${#missing_files[@]} -eq 0 ]; then
    echo "✅ All required firmware files found"
else
    echo "❌ Missing firmware files:"
    for file in "${missing_files[@]}"; do
        echo "   - $file"
    done
    echo
    echo "Please run './build.sh' first to compile the firmware."
    exit 1
fi

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
echo "Starting recovery flash process..."
echo "================================"

# Method 1: Flash only the recovery partition
echo "Flashing RECOVERY IMAGE ONLY (factory partition)..."
echo "---------------------------------------------------"
cd /home/console-admin/esp-idf-v4.3.5
source export.sh > /dev/null 2>&1
cd /home/console-admin/Documents/code/squeezelite-esp32-v4.3

# Flash only the recovery image and essential partitions
python $IDF_PATH/components/esptool_py/esptool/esptool.py \
    --chip esp32 \
    --port /dev/ttyUSB0 \
    --baud 115200 \
    --before default_reset \
    --after hard_reset \
    write_flash \
    --flash_mode dio \
    --flash_freq 40m \
    --flash_size 4MB \
    0x1000 build/bootloader/bootloader.bin \
    0x8000 build/partition_table/partition-table.bin \
    0xd000 build/ota_data_initial.bin \
    0x10000 build/recovery.bin

if [ $? -eq 0 ]; then
    echo "✅ RECOVERY IMAGE FLASHED SUCCESSFULLY!"
    echo
    echo "NEXT STEPS:"
    echo "==========="
    echo "1. 🔌 Reset your ESP32 (press reset button or power cycle)"
    echo "2. 📶 The ESP32 will boot into RECOVERY MODE"
    echo "3. 🌐 Connect to the WiFi AP: 'squeezelite-XXXXXX'"
    echo "4. 🖥️  Open browser to: http://192.168.4.1"
    echo "5. ⚙️  Configure WiFi and upload the squeezelite.bin via OTA"
    echo
    echo "📋 The recovery mode provides:"
    echo "   - WiFi configuration"
    echo "   - OTA firmware updates"
    echo "   - Basic system recovery"
    echo
    echo "📁 To update to the main squeezelite application:"
    echo "   - Use the web interface to upload build/squeezelite.bin"
    echo "   - Or use the console command: ota_update"
    echo
else
    echo "❌ Recovery flash failed!"
    echo
    echo "TROUBLESHOOTING:"
    echo "==============="
    echo "1. 🔄 Try the entire procedure again from the beginning"
    echo "2. 🔌 Try a different USB cable (ensure it's a data cable)"
    echo "3. 🔌 Try a different USB port"
    echo "4. 🔋 Check power supply (3.3V stable)"
    echo "5. 🧹 Run 'ESP32: Erase Flash' task first, then try again"
    echo "6. 📖 Check hardware documentation for your specific ESP32 board"
    echo
    echo "If problems persist, this may be a hardware issue with the ESP32 board."
    exit 1
fi
