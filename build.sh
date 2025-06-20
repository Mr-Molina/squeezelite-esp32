#!/bin/bash

# ESP-IDF Environment Setup and Build Script
export IDF_PATH="/home/console-admin/esp-idf-v4.3.5"
export PYTHON="/home/console-admin/.espressif/python_env/idf4.3_py3.11_env/bin/python"
source "$IDF_PATH/export.sh" > /dev/null 2>&1

cd /home/console-admin/Documents/code/squeezelite-esp32-v4.3

echo "ESP-IDF Environment:"
echo "IDF_PATH: $IDF_PATH"
echo "Python: $(which python)"
echo "idf.py version: $(idf.py --version)"

echo ""
echo "Checking protobuf version compatibility..."
# Ensure protobuf is compatible with nanopb (requires < 4.0.0)
PROTOBUF_VERSION=$(python -c "import google.protobuf; print(google.protobuf.__version__)" 2>/dev/null || echo "unknown")
if [[ "$PROTOBUF_VERSION" =~ ^[4-9] ]]; then
    echo "Installing compatible protobuf version..."
    pip install "protobuf<4.0.0"
fi

echo ""
echo "Building project..."
idf.py build
