#!/bin/bash

# Clean script for squeezelite-esp32 project
# This script sets up the ESP-IDF environment and cleans the project

set -e

echo "Setting up ESP-IDF environment..."
export IDF_PATH="/home/console-admin/esp-idf-v4.3.5"
export PYTHON="/home/console-admin/.espressif/python_env/idf4.3_py3.11_env/bin/python"
source "$IDF_PATH/export.sh" > /dev/null 2>&1

cd /home/console-admin/Documents/code/squeezelite-esp32-v4.3

echo ""
echo "Cleaning project..."
idf.py clean
