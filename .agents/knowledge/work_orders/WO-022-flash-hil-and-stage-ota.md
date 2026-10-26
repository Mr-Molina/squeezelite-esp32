# Work Order: WO-022 — Physical ESP32 (`/dev/ttyUSB0`) HIL Verification & OTA Binary Staging (`--stage-only`)

- **Work Order ID:** `WO-022`
- **Timestamp:** `2026-10-20T23:22:00-04:00`
- **Assigned Role:** `Extension Certifier / Infrastructure Administrator`
- **Status:** `[EXECUTED & VERIFIED]`

## 1. Objective & Scope
1. Clean up `/home/console-admin/Github/squeezelite-esp32/handoff.md` by moving it to `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/handoff_wo021.md`.
2. Package the bare-metal `louder-esp32-app` release ELF into a staged ESP32 `.bin` firmware image (`--stage-only` per `/home/console-admin/.gemini/config/rules/staging-before-restart.md`):
   - Output path: `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/louder-esp32-app-basement-ota.bin`
   - Compute exact file size (bytes) and SHA-256 digest (`sha256sum`).
3. Flash `louder-esp32-app` onto the locally connected `ESP32-D0WDQ6-V3` at `/dev/ttyUSB0` via `/home/console-admin/.cargo/bin/espflash flash --port /dev/ttyUSB0 /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app` and capture the live UART0 (`115200` baud) output to empirically verify `[HIL 1/5]` through `[HIL 5/5]` and `[HIL VERDICT] UNCONDITIONAL HARDWARE PASS` on physical silicon.
4. Flash `cyd-controller-app` onto `/dev/ttyUSB0` via `/home/console-admin/.cargo/bin/espflash flash --port /dev/ttyUSB0 /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/cyd-controller-app` and capture its live UART0 (`115200` baud) output to empirically verify its `[HIL 1/5]` through `[HIL 5/5]` and `[HIL VERDICT] UNCONDITIONAL HARDWARE PASS`, leaving `/dev/ttyUSB0` running the live CYD display firmware.
5. **CRITICAL SAFETY CONSTRAINT (`staging-before-restart.md`)**: Do NOT send any HTTP POST, OTA upload, or reboot command to `172.22.1.15` (`"BASEMENT"`). Only stage the `.bin` locally and verify `172.22.1.15` remains online via a read-only `GET http://172.22.1.15/status.json` check.

## 2. Verification Steps
1. Run `espflash save-image --chip esp32` for `louder-esp32-app` -> `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/louder-esp32-app-basement-ota.bin` and record `ls -lh` and `sha256sum`.
2. Flash `louder-esp32-app` to `/dev/ttyUSB0` and capture serial output using a Python script in `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/capture_serial.py` (using `pyserial` or `/dev/ttyUSB0` raw termios at `115200` baud after pulsing RTS/EN to reset the board).
3. Flash `cyd-controller-app` to `/dev/ttyUSB0` and capture its serial output using the same script.
4. Check `git -C /home/console-admin/Github/squeezelite-esp32 status --short` to ensure no untracked scratch/handoff files remain in the repo root.

---

## Fulfillment Section

- **Fulfillment Timestamp:** `2026-10-20T23:26:00-04:00`
- **Assigned Subagent:** `Extension Certifier / Infrastructure Administrator`
- **Status:** `[EXECUTED & VERIFIED]`

### Execution Summary & Results
1. **Repo Clean-Up**:
   - `handoff.md` moved from repo root to `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/handoff_wo021.md`.
   - `git status --short` verified zero untracked scratch/handoff files in repository root.

2. **OTA Binary Packaging (`--stage-only`)**:
   - Packaged image: `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/louder-esp32-app-basement-ota.bin`
   - Exact size: `175,744 bytes` (172 KiB, 4.26% of 4MB partition).
   - SHA-256 digest: `0a4d759a1a7e8563217e24eec8e6bc5ec26f8370f6301818b44ecf50ee293149`.
   - Safety compliance (`staging-before-restart.md`): Zero mutating requests sent to `172.22.1.15`. Verified node online via `GET http://172.22.1.15/status.json` (`SSID: iot`, `RSSI: -51`, `LMS: 10.0.0.10:9000`).

3. **Physical Silicon Verification (`louder-esp32-app`)**:
   - Flashed to `/dev/ttyUSB0` via `espflash flash`.
   - Captured UART boot output at 115200 baud via `capture_serial.py`.
   - `[HIL 1/5]` Dual-Sector NVS Flash & BASEMENT Config: PASS
   - `[HIL 2/5]` GPIO33 PWDN & I2C0 (400kHz) TAS5805M (0x2D) Hi-Z Init: PASS
   - `[HIL 3/5]` Pure-Rust WAV/PCM Pipeline + 10-Band EQ + EspI2sSink: PASS
   - `[HIL 4/5]` SlimProto HELO/STAT/STRM Frame Builder & Parser: PASS
   - `[HIL 5/5]` Physical UART2 (TX=4, RX=36 @ 460800) CYD Telemetry Bridge: PASS
   - **Verdict:** `[HIL VERDICT] UNCONDITIONAL HARDWARE PASS` (5/5 tests passed).

4. **Physical Silicon Verification & Restoration (`cyd-controller-app`)**:
   - Flashed to `/dev/ttyUSB0` via `espflash flash`.
   - Captured UART boot output at 115200 baud via `capture_serial.py`.
   - `[HIL 1/5]` ILI9341 Display & Backlight Init: PASS (cmds=2219, spi_bytes=707978, flushes=732, bl=100%)
   - `[HIL 2/5]` XPT2046 Touch Bus & Calibration: PASS (irq_low=true, raw_adc=(0, 0), center_cal=(159, 119))
   - `[HIL 3/5]` CYD Link Telemetry & UI Render: PASS (events=5, track="Bohemian Rhapsody", progress=40%, time=02:22/05:54, badge=ONLINE)
   - `[HIL 4/5]` Touch Hit-Boxes & 10Hz Throttle: PASS (prev=OK, toggle=OK, next=OK, throttle_50ms=SUPPRESSED, release=CMD=VOL:100)
   - `[HIL 5/5]` Heartbeat Timeout & Resync Poll: PASS (timeout_at=6.5s, link_active=false, resync_frame="CMD=STATUS\n")
   - **Verdict:** `[HIL VERDICT] UNCONDITIONAL HARDWARE PASS` (5/5 tests passed).
   - Board left in operational CYD event loop (`[READY] CYD Display Reset to STANDBY/OFFLINE — Entering Real-Time UART2 & Touch Loop`).

5. **Rule 21 Completion Barrier**:
   - Verified `manage_task(list)` reports zero running tasks before declaring completion.
   - Subagent summary written to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/2026-10-20_wo022_hardware_hil_and_ota_stage.md`.
