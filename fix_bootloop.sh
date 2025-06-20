#!/bin/bash

# ESP32 Boot Loop Fix Script
echo "====================================="
echo "ESP32 BOOT LOOP RECOVERY"
echo "====================================="
echo ""
echo "Error: invalid header: 0xffffffff"
echo "This indicates corrupted or incomplete flash data."
echo ""

# Check if build directory exists
if [ ! -d "build" ]; then
    echo "❌ Build directory not found. Running build first..."
    ./build.sh
    if [ $? -ne 0 ]; then
        echo "❌ Build failed. Cannot proceed with recovery."
        exit 1
    fi
fi

echo "RECOVERY PROCEDURE:"
echo "=================="
echo "1. 🧹 Complete flash erase"
echo "2. 🔧 Flash bootloader first"
echo "3. 📊 Flash partition table"
echo "4. 💾 Flash main firmware"
echo ""

echo "Put ESP32 in boot mode:"
echo "- Hold BOOT button"
echo "- Press and release RESET button"
echo "- Keep holding BOOT until told to release"
echo ""
read -p "Press ENTER when ESP32 is in boot mode..."

# Set up environment
source /home/console-admin/esp-idf-v4.3.5/export.sh > /dev/null 2>&1

echo ""
echo "Step 1: Erasing entire flash..."
echo "==============================="
if esptool.py --port /dev/ttyUSB0 --baud 115200 erase_flash; then
    echo "✅ Flash erase successful"
else
    echo "❌ Flash erase failed. Check connections and boot mode."
    exit 1
fi

echo ""
echo "Step 2: Flashing bootloader..."
echo "=============================="
cd build
if esptool.py --port /dev/ttyUSB0 --baud 115200 write_flash 0x1000 bootloader/bootloader.bin; then
    echo "✅ Bootloader flash successful"
else
    echo "❌ Bootloader flash failed"
    exit 1
fi

echo ""
echo "Step 3: Flashing partition table..."
echo "==================================="
if esptool.py --port /dev/ttyUSB0 --baud 115200 write_flash 0x8000 partition_table/partition-table.bin; then
    echo "✅ Partition table flash successful"
else
    echo "❌ Partition table flash failed"
    exit 1
fi

echo ""
echo "Step 4: Flashing OTA data..."
echo "============================"
if esptool.py --port /dev/ttyUSB0 --baud 115200 write_flash 0xd000 ota_data_initial.bin; then
    echo "✅ OTA data flash successful"
else
    echo "❌ OTA data flash failed"
    exit 1
fi

echo ""
echo "Step 5: Flashing recovery firmware..."
echo "====================================="
if esptool.py --port /dev/ttyUSB0 --baud 115200 write_flash 0x10000 recovery.bin; then
    echo "✅ Recovery firmware flash successful"
else
    echo "❌ Recovery firmware flash failed"
    exit 1
fi

echo ""
echo "Step 6: Flashing main application..."
echo "===================================="
if esptool.py --port /dev/ttyUSB0 --baud 115200 write_flash 0x150000 squeezelite.bin; then
    echo "✅ Main application flash successful"
else
    echo "❌ Main application flash failed"
    exit 1
fi

echo ""
echo "🎉 RECOVERY COMPLETE!"
echo "==================="
echo "You can now release the BOOT button."
echo "The ESP32 should reset and boot normally."
echo ""
echo "Next steps:"
echo "1. Release the BOOT button if still held"
echo "2. Press the RESET button to restart normally"
echo "3. Run 'Monitor' task to verify boot messages"
echo ""
echo "Expected boot sequence:"
echo "- ESP32 should show 'ets Jul 29 2019 12:21:46'"  
echo "- Should NOT show 'invalid header: 0xffffffff'"
echo "- Should show recovery or squeezelite startup messages"
echo ""
echo "If the ESP32 still boot loops:"
echo "1. Check power supply (stable 3.3V)"
echo "2. Verify flash size in sdkconfig (should match your ESP32)"
echo "3. Try a different ESP32 board"
echo ""
echo "Monitor serial output to verify boot:"
echo "./monitor.sh"

cd ..
