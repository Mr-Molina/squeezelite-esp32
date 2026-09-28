#!/usr/bin/env bash
# ==============================================================================
# Squeezelite-ESP32 Containerized Build & Development Script (Linux/WSL/macOS)
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
IMAGE_NAME="docker.io/sle118/squeezelite-esp32-idfv435"
TARGET="${1:-I2S-4MFlash}"
DEPTH="${DEPTH:-16}"

# Detect container runtime
if command -v podman >/dev/null 2>&1; then
    CONTAINER_BIN="podman"
elif command -v docker >/dev/null 2>&1; then
    CONTAINER_BIN="docker"
else
    echo "ERROR: Neither podman nor docker found in PATH." >&2
    exit 1
fi

echo "=========================================================="
echo " Squeezelite-ESP32 Containerized Build"
echo " Runtime: ${CONTAINER_BIN}"
echo " Image  : ${IMAGE_NAME}"
echo " Target : ${TARGET} (${DEPTH}-bit)"
echo "=========================================================="

case "${1:-}" in
    --webapp)
        echo "Building Webapp frontend..."
        "${CONTAINER_BIN}" run --rm \
            -v "${PROJECT_DIR}:/workspace/squeezelite-esp32:z" \
            -w "/workspace/squeezelite-esp32" \
            "${IMAGE_NAME}" \
            bash -c "cd components/wifi-manager/webapp && npm install && npm run build"
        ;;
    --interactive|-it)
        echo "Launching interactive bash shell..."
        "${CONTAINER_BIN}" run --rm -it \
            -v "${PROJECT_DIR}:/workspace/squeezelite-esp32:z" \
            -w "/workspace/squeezelite-esp32" \
            "${IMAGE_NAME}" \
            bash
        ;;
    --menuconfig)
        echo "Launching idf.py menuconfig..."
        "${CONTAINER_BIN}" run --rm -it \
            -v "${PROJECT_DIR}:/workspace/squeezelite-esp32:z" \
            -w "/workspace/squeezelite-esp32" \
            "${IMAGE_NAME}" \
            bash -c "source /opt/esp/idf/export.sh && idf.py menuconfig"
        ;;
    --clean)
        echo "Cleaning build directory..."
        "${CONTAINER_BIN}" run --rm \
            -v "${PROJECT_DIR}:/workspace/squeezelite-esp32:z" \
            -w "/workspace/squeezelite-esp32" \
            "${IMAGE_NAME}" \
            bash -c "source /opt/esp/idf/export.sh && idf.py fullclean"
        ;;
    *)
        echo "Building firmware for target '${TARGET}'..."
        "${CONTAINER_BIN}" run --rm \
            -v "${PROJECT_DIR}:/workspace/squeezelite-esp32:z" \
            -w "/workspace/squeezelite-esp32" \
            "${IMAGE_NAME}" \
            bash -c "export TARGET_BUILD_NAME='${TARGET}' && export DEPTH='${DEPTH}' && bash ./buildFirmware.sh"
        ;;
esac
