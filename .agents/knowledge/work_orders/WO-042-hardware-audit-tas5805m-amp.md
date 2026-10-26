# Work Order: WO-042 — Deep Hardware & Code Audit: TAS5805M (`0x2D`) Amplifier Registers, Clock-Fault State Machine, GPIO33 PWDN & Volume (`Micro-Auditor 2`)

**Work Order ID:** WO-042  
**Timestamp:** 2026-10-24T12:17:30-04:00  
**Assigned Subagent Role:** TAS5805M Amplifier & Hardware Power/Clock Auditor (`Model: "pro"`, `TypeName: "research"`)  
**Target Files (Read-Only):**
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/amp.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/board.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`
- `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/adac_tas5805m.c` (or all TAS5805M/DAC files under `components/`)
**Task Goal:** Perform a zero-tolerance `/deep-hardware-audit` and `/deep-code-audit` inspection of the TI TAS5805M (`0x2D`) I2C register sequence, `GPIO33` (`PWDN_N`), `GPIO34` (`FAULT_N`), clock-fault latching when `BCK`/`WS` are absent at boot, digital volume (`0x4C`), and software mute/volume in `louder-esp32-app`.

---

## 1. Context & Scope
- Live target `172.22.1.15` (`"BASEMENT"`, Louder-ESP32 board) is running pure-Rust `louder-esp32-app` (`d445a736`).
- Live NVS configuration on `172.22.1.15` (captured in `WO-016`):
  - `set_GPIO="33=vcc"`
  - `i2c_config="scl=27,sda=21,speed=400000,port=1"`
  - `dac_config="model=I2S,bck=26,ws=25,do=22,sda=21,scl=27,i2c=45"` (`0x2D` = 45)
  - `dac_controlset={"init":[{"reg":3,"val":2},{"reg":3,"val":3}],"poweron":[{"reg":3,"val":3}],"poweroff":[{"reg":3,"val":0}]}`
- **Prohibited Actions (Rule 3):** NEVER touch or query `/dev/ttyUSB0`. Read-only analysis only.

---

## 2. Specific Audit Questions to Answer with File + Line Citations
1. **TAS5805M Clock-Fault (`CLK_FAULT`) & Boot Ordering**:
   - In `src/main.rs`, `amp.initialize(32)` is called at boot **before** `I2S0` transmits any audio frames (so `BCK=GPIO26` and `WS=GPIO25` are NOT toggling when `0x03 = 0x02` Hi-Z and `0x03 = 0x03` Play are written, or `BCK`/`WS` stop whenever `I2S0` is not actively inside a one-shot DMA transfer).
   - According to the TI TAS5805M datasheet and C driver (`components/`), what happens when `DEVICE_CTRL_2` (`0x03`) is set to `0x02` (Hi-Z) / `0x03` (Play) without continuous I2S `SCLK`/`LRCLK`, or when `SCLK`/`LRCLK` stops between DMA buffers?
   - Furthermore, check `amp.check_and_recover_faults(now_ms)` in `src/main.rs`: how does it detect faults? It checks `fault_pin.is_low()` (`GPIO34`)! Wait: on `172.22.1.15`, does NVS `dac_config` or `set_GPIO` configure `GPIO34` as a wired `FAULT_N` pin, or is `GPIO34` floating LOW or HIGH on `172.22.1.15`? Wait: if `GPIO34` is floating LOW (because `GPIO34` on ESP32 is input-only and has NO internal pull-up resistor in silicon!), what happens in `src/main.rs` every 500ms if `fault_pin.is_low()` is true?!
   - CHECK THIS IMMEDIATELY: On ESP32 hardware (`GPIO34..39` are input-only pads with NO internal pull-up/pull-down resistors!), if `GPIO34` is not wired or is low, or if `amp.check_and_recover_faults` runs, what does it do to `amp` and `pipeline.sink`?!
2. **TAS5805M Register Sequence & Digital Volume (`0x4C`) vs Software Gain (`pipeline.set_volume`)**:
   - Trace the exact I2C bytes written by `Tas5805mDriver::init_sequence`, `AmpManager::initialize`, `AmpManager::start_playback_via_hiz`, and `AmpManager::set_volume`.
   - What value is written to register `0x4C` (`DIG_VOL_CTL`) at boot (`amp.initialize(32)`) and during playback? In TAS5805M, `0x30` (48) = `0.0 dB` (unity gain), `0x20` (32) = `+8.0 dB`, and `dac_controlset` in NVS only writes `[{"reg":3,"val":2},{"reg":3,"val":3}]` leaving `0x4C` at hardware default `0x30` (`0 dB`). Does `squeezelite-tas58xx` write anything else that alters data format (`0x33` `SAP_CTRL1`), BCK ratio, DSP page/book, or mute (`0x03`)?
3. **Software Mute & Volume State in `StreamPipeline` and `run_firmware_tick`**:
   - Trace `pipeline.sink.is_muted()` and `pipeline.gain_left` / `pipeline.gain_right` from boot through `strm` `'s'` and `run_firmware_tick` in `src/main.rs`, `src/coordinator.rs`, and `src/pipeline.rs`. Is `pipeline.sink` left muted or re-muted anywhere?

---

## 3. Deliverable
Return a structured Markdown audit report with `[CRITICAL]`, `[HIGH]`, `[MEDIUM]`, `[LOW]` findings, exact `file:line` citations, root-cause proof, and concrete remediation code specifications.

---

## 4. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Subagent Conversation ID:** `3119f58e-e033-4ab8-b65e-1645a96ee274`
- **Execution Timestamp:** 2026-10-24T12:26:13-04:00
- **Key Findings:**
  1. `AMP-AUD-01` (`[CRITICAL]`, `src/main.rs:63-67, 724-725`, `squeezelite-tas58xx/src/lib.rs:477-487`): Calling `amp.start_playback_via_hiz()` at boot before continuous `BCK`/`WS` clocks are running trips `FAULT1_CLOCK_FAULT` (`0x71` bit 2) in the TAS5805M (`0x2D`), and `run_firmware_tick` calls `amp.check_faults()` + `amp.recover_from_fault()` (`0x03=0x02` Hi-Z -> `0x78=0x80` -> `0x03=0x03` Play) on every loop iteration (`18,966,497` I2C ACKs in 43m), continuously forcing the amplifier into Hi-Z.
  2. `AMP-AUD-02` (`[CRITICAL]`, `src/hal.rs:243-246`, `src/main.rs:584-587`): `EspI2sSink::set_mute(true)` sets `self.driver.set_tx_enabled(false)`, bypassing DMA writes and starving `BCK`/`WS` instead of allowing `EspI2sSink::write_frames` (`src/hal.rs:179-183`) to push zero-filled PCM silence frames.
  3. `AMP-AUD-03` (`[HIGH]`, `src/amp.rs:92`): `AmpManager::for_active_board` initializes hardware register `0x4C` (`DIG_VOL_CTRL`) to `VOL_DEFAULT_MINUS_24DB` (`0x60` = `-24.0 dB`) instead of `VOL_UNITY_0DB` (`0x30` = `0.0 dB` matching the C `dac_controlset` baseline).
- **Fulfillment Status:** `[EXECUTED & VERIFIED]`
