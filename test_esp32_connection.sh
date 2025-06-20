#!/bin/bash

# ESP32 Connection Test Script
echo "ESP32 Hardware Connection Test"
echo "=============================="

# Set up environment
source /home/console-admin/esp-idf-v4.3.5/export.sh > /dev/null 2>&1

echo "1. Checking USB device..."
lsusb | grep -i "cp210\|ch340\|ftdi\|serial" || echo "   No common USB-to-serial devices found"

echo ""
echo "2. Checking serial port..."
if [ -e /dev/ttyUSB0 ]; then
    ls -la /dev/ttyUSB0
else
    echo "   /dev/ttyUSB0 not found!"
    echo "   Available tty devices:"
    ls -la /dev/tty* | grep -E "(USB|ACM)" || echo "   No USB serial devices found"
fi

echo ""
echo "3. Testing basic ESP32 communication..."
echo "   (This may take a few seconds...)"

# Try to get chip info
if timeout 10s idf.py -p /dev/ttyUSB0 chip_id 2>/dev/null; then
    echo "   ✅ ESP32 communication successful!"
else
    echo "   ❌ ESP32 communication failed"
    echo ""
    echo "Troubleshooting suggestions:"
    echo "- Check USB cable (use data cable, not charge-only)"
    echo "- Try holding BOOT button and pressing RESET"
    echo "- Disconnect any external hardware from ESP32"
    echo "- Try a different USB port"
    echo "- Check power supply (should be stable 3.3V)"
fi

echo ""
echo "4. Recent kernel messages (may show USB connection issues):"
dmesg | tail -5 | grep -i "usb\|tty" || echo "   No recent USB/TTY messages"
