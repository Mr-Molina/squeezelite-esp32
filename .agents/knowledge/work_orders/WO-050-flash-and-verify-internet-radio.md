# Work Order `WO-050`: Flash Pure-Rust OTA Binary (`9f17dbdc`) to `172.22.1.15` (`ota_0`) & Verify Live Internet Radio (`"Ibiza Global Radio MP3"`) Playback

- **ID**: `WO-050`
- **Timestamp**: `2026-10-24T15:28:00-04:00`
- **Subagent Role**: Extension Certifier (`Model: "flash"`)
- **Status**: `[BLOCKED / ERROR]`

## 1. Objective & Scope
Execute `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_wo050_internet_radio.py` to:
1. Transition `172.22.1.15` (`"BASEMENT"`) to Factory `recovery` (`0x10000`, `"recovery": 1`) via `POST http://172.22.1.15/recovery.json`.
2. Stream the staged pure-Rust binary `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (`749,808` bytes, SHA-256 `edb766d245ed19fec5b2b3f6e6276662085028df6b81e4ed88499fb0c4ed8a33`, commit `9f17dbdc`) to `ota_0` (`0x160000`) via `POST http://172.22.1.15/flash.json`.
3. Verify `172.22.1.15` boots `louder-esp32-app` (`"version": "I2S-4MFlash-Rust"`, `"recovery": 0`, `"otadata_prearmed_factory": true`) and registers `"BASEMENT"` (`c8:2e:18:be:b8:34`) with LMS `10.0.0.10:9000`.
4. Play `"Ibiza Global Radio MP3"` (`https://cdn-peer032.streaming-pro.com:8025/ibizaglobalradio.mp3`) on `"BASEMENT"` and verify `"frames_played"` in `GET http://172.22.1.15/status.json` advances continuously past `512` (`> 150,000` frames over 20s) with `"amp_mode": "Play"`, `"i2c_nacks": 0`, and zero OOM/stack reset.

## 2. Mandatory Constraints
- **NEVER touch `/dev/ttyUSB0`** (Rule 3).
- **NEVER call `POST /reboot_ota.json`** (keep `otadata_prearmed_factory: true` single-boot trial mode).
- Follow Rule 21 (wait for all background tasks to finish and verify exit code `0` before handoff).

## 3. Fulfillment Section
- **Status**: `[BLOCKED / ERROR]`
- **Subagent Role**: Extension Certifier (`Model: "flash"`)
- **Completion Timestamp**: 2026-10-24T15:45:00-04:00
- **Target Device**: `172.22.1.15` (`"BASEMENT"`, MAC `c8:2e:18:be:b8:34`)
- **Staged OTA Binary Path**: `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`
- **Binary Size**: 749,808 bytes
- **SHA-256**: `edb766d245ed19fec5b2b3f6e6276662085028df6b81e4ed88499fb0c4ed8a33`
- **Commit**: `9f17dbdcf1700e562fc9101a3717a14d96d4b62f`

### Execution Summary & Failure Signature
1. **Flash Upload**: Successfully streamed `749,808` bytes via `POST http://172.22.1.15/flash.json` while device was in Factory recovery `0x10000` (`"recovery": 1`). HTTP 200 OK received (`File uploaded. Flashing started.`).
2. **Boot Failure & Anti-Brick Protection**:
   - The ESP32 erased and flashed `ota_0` (`0x160000`), then rebooted at `t = ~18s`.
   - Upon jumping to `ota_0`, `louder-esp32-app` failed to complete early initialization and never started Wi-Fi or served HTTP.
   - Because Gate 1 pre-armed Factory recovery (`otadata` erased to `0xFF` / single-boot trial mode), the ESP-IDF hardware bootloader safely rolled back to Factory recovery `0x10000` (`"recovery": 1`, `"version": "I2S-4MFlash-1681"`).
   - Target `172.22.1.15` remains 100% online, unbricked, and reachable on Wi-Fi `"iot"`.
3. **Root Cause Analysis (Proved via ELF Memory Map & Disassembly)**:
   - In `esp-hal-1.2.2/ld/esp32/memory.x`, the internal DRAM segment `dram_seg` has fixed size `192 KB` (`0x30000` bytes, `0x3FFB0000..0x3FFE0000`).
   - `.stack` is dynamically defined in `stack.x` as `_stack_end = ABSOLUTE(.)` (end of `.bss`) to `_stack_start = 0x3FFE0000`.
   - In commit `9f17dbdc`, moving static buffers into `.bss` (`BSS_BUFFERS`: 39,936 B) and `STATIC_MP3_DEC` into `.data` (22,792 B) expanded RAM consumption by `62.7 KB`, shrinking available `.stack` from ~90.5 KB down to only **27,824 bytes**.
   - In `run_hardware_firmware()`:
     - `load_nvs_wifi_credentials` was inlined by the compiler, placing `nvs_buf: [u8; 16384]` (16.0 KB) directly onto the main stack frame.
     - `StreamPipeline` was expanded with `PCM_SCRATCH_SAMPLES = 2304` (9,216 B) and `READ_SCRATCH_BYTES = 2048` (2,048 B), increasing `StreamPipeline` stack size to **12,600 bytes**.
     - Combined stack requirement for `nvs_buf` and `pipeline` alone is `16,384 + 12,600 = 28,984 bytes`.
     - `28,984 bytes > 27,824 bytes available stack`.
   - The stack pointer immediately overflowed into `.bss` (`0x3FFD934F`), corrupting FreeRTOS and Wi-Fi data structures and triggering an early boot panic before network initialization.

### Remediation Prescription for Next Implementation Task
1. Eliminate `nvs_buf` (16 KB) from the stack by reading NVS using the pre-allocated `stream_storage` + `output_storage` in `FirmwareBssBuffers` (or using a union in `FirmwareBssBuffers`), adding 0 bytes to `.bss` and 0 bytes to the stack.
2. Move `pcm_scratch` (9.2 KB) and `read_scratch` (2.0 KB) from `StreamPipeline`'s stack frame into `FirmwareBssBuffers`, passing slices into `StreamPipeline::new()`.
3. Optimize `FirmwareBssBuffers` (`slim_tx_buf` from 4096 -> 1024, `audio_req_buf` from 2048 -> 512, `audio_header_buf` from 2048 -> 1024) to reclaim 5,632 bytes for `.stack`.
4. Resulting stack usage drops to < 2.0 KB against a ~22 KB `.stack` section (leaving > 20 KB stack headroom).
