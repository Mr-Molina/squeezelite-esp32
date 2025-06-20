#!/bin/bash

# ESP32 Flash Diagnostic Script
echo "====================================="
echo "ESP32 FLASH DIAGNOSTIC"
echo "====================================="
echo ""
echo "This script will read the flash contents to diagnose boot loop issues."
echo ""

echo "Put ESP32 in boot mode:"
echo "- Hold BOOT button"
echo "- Press and release RESET button"
echo "- Keep holding BOOT until analysis is complete"
echo ""
read -p "Press ENTER when ESP32 is in boot mode..."

echo ""
echo "Reading flash contents..."
echo "========================"

# Check bootloader at 0x1000
echo "1. Checking bootloader (0x1000):"
if esptool.py --port /dev/ttyUSB0 read_flash 0x1000 16 bootloader_check.bin 2>/dev/null; then
    hexdump -C bootloader_check.bin | head -1
    if hexdump -C bootloader_check.bin | grep -q "ff ff ff ff"; then
        echo "   ❌ Bootloader appears blank (0xFFFFFFFF)"
    else
        echo "   ✅ Bootloader appears to have data"
    fi
    rm -f bootloader_check.bin
else
    echo "   ❌ Failed to read bootloader area"
fi

echo ""
echo "2. Checking partition table (0x8000):"
if esptool.py --port /dev/ttyUSB0 read_flash 0x8000 16 partition_check.bin 2>/dev/null; then
    hexdump -C partition_check.bin | head -1
    if hexdump -C partition_check.bin | grep -q "ff ff ff ff"; then
        echo "   ❌ Partition table appears blank (0xFFFFFFFF)"
    else
        echo "   ✅ Partition table appears to have data"
    fi
    rm -f partition_check.bin
else
    echo "   ❌ Failed to read partition table area"
fi

echo ""
echo "3. Checking main application (0x10000):"
if esptool.py --port /dev/ttyUSB0 read_flash 0x10000 16 app_check.bin 2>/dev/null; then
    hexdump -C app_check.bin | head -1
    if hexdump -C app_check.bin | grep -q "ff ff ff ff"; then
        echo "   ❌ Application appears blank (0xFFFFFFFF)"
    else
        echo "   ✅ Application appears to have data"
    fi
    rm -f app_check.bin
else
    echo "   ❌ Failed to read application area"
fi

echo ""
echo "4. Getting chip info:"
esptool.py --port /dev/ttyUSB0 chip_id 2>/dev/null || echo "   ❌ Failed to get chip info"

echo ""
echo "5. Flash size detection:"
esptool.py --port /dev/ttyUSB0 flash_id 2>/dev/null || echo "   ❌ Failed to detect flash"

echo ""
echo "DIAGNOSIS COMPLETE"
echo "=================="
echo "You can now release the BOOT button."
echo ""
echo "If any areas show 0xFFFFFFFF, those sections are blank and need to be flashed."
echo "Use the 'Fix Boot Loop' task to recover the ESP32."
