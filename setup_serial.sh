#!/bin/bash

# ESP32 Serial Port Setup Script
# This script fixes common serial port permission issues on Linux

echo "ESP32 Serial Port Setup"
echo "======================="

# Check if running as root
if [[ $EUID -eq 0 ]]; then
   echo "Please run this script as a normal user (not with sudo)"
   echo "The script will prompt for sudo password when needed"
   exit 1
fi

echo "1. Adding user to dialout group..."
sudo usermod -a -G dialout $USER

echo "2. Installing udev rules for ESP32 devices..."
sudo cp 99-esp32-usb.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger

echo "3. Setting temporary permissions for current session..."
if [ -e /dev/ttyUSB0 ]; then
    sudo chmod 666 /dev/ttyUSB0
    echo "   Set permissions for /dev/ttyUSB0"
fi

echo ""
echo "Setup complete!"
echo ""
echo "IMPORTANT: You need to log out and log back in (or reboot) for the"
echo "group membership changes to take effect permanently."
echo ""
echo "For the current session, you can now flash your ESP32."
echo ""
echo "Available serial devices:"
ls -la /dev/tty* 2>/dev/null | grep -E "(USB|ACM)" || echo "  No USB serial devices found"
echo ""
echo "Your current groups:"
groups $USER
