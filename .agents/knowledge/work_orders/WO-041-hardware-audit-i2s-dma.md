# Work Order: WO-041 — Deep Hardware & Code Audit: ESP32 `I2S0` DMA, Clocking, Pin Mux & Sample Format (`Micro-Auditor 1`)

**Work Order ID:** WO-041  
**Timestamp:** 2026-10-24T12:17:30-04:00  
**Assigned Subagent Role:** ESP32 `I2S0` DMA & Hardware Clocking Auditor (`Model: "pro"`, `TypeName: "research"`)  
**Target Files (Read-Only):**
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/board.rs`
- `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/output_i2s.c`
- `/home/console-admin/.cargo/registry/src/` (`esp-hal-1.2.2/src/i2s/`)
**Task Goal:** Perform a zero-tolerance `/deep-hardware-audit` and `/deep-code-audit` inspection of the ESP32 `I2S0` peripheral, DMA descriptor/buffer lifecycle, pin multiplexing (`BCK=GPIO26`, `WS=GPIO25`, `DOUT=GPIO22`), clock continuity, and PCM sample format in `louder-esp32-app`.

---

## 1. Context & Scope
- Live target `172.22.1.15` (`"BASEMENT"`) is running pure-Rust `louder-esp32-app` (`d445a736`).
- User reported: `"i cannot hear audio coming from the device"`.
- Live telemetry from `GET http://172.22.1.15/status.json` shows:
  `"frames_played": 512, "amp_mode": "Play", "i2c_acks": 18966497, "i2c_nacks": 0, "uptime_ms": 2580008`.
- **Prohibited Actions (Rule 3):** NEVER touch or query `/dev/ttyUSB0`. Read-only analysis only.

---

## 2. Specific Audit Questions to Answer with File + Line Citations
1. **Why did `frames_played` stop at `512` or how does `Esp32HardwareI2sDma::write_dma_bytes` interact with `esp-hal 1.2.2` DMA?**
   - Inspect `Esp32HardwareI2sDma` in `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` and the underlying `esp-hal 1.2.2` `i2s::master` implementation in `/home/console-admin/.cargo/registry/src/*/esp-hal-1.2.2/src/i2s/`.
   - Trace what `self.i2s_tx.write_dma(...)` / `write(...)` does in `esp-hal 1.2.2`. Does it block, or start a one-shot DMA transfer and drop it, or fail to re-arm descriptors, or stop `BCK`/`WS` clocks after each buffer?
   - Check how `dma_tx_buffer!(4096)` works in `esp-hal 1.2.2`: after `transfer.wait()` completes once, what state is `tx_buffers` (`DmaTxBuf`) in? Does `DmaTxBuf` in `esp-hal 1.2.2` require `set_length(...)` or does `write_dma` consume/reset descriptors?
2. **`I2S0` Protocol Mode (`TdmConfig::new_tdm_philips()` vs Standard 2-Channel Philips I2S) on ESP32 (`xtensa`)**:
   - Inspect how `I2s::new` is configured in `src/main.rs` (`DataFormat`, `TdmConfig` vs `Standard`, `Rate`, bit clock ratio, channel slots) and compare against `esp-hal 1.2.2` `src/i2s/master.rs` for `esp32` (`xtensa`) and the C reference in `components/squeezelite/output_i2s.c` / `adac_tas5805m` / NVS `dac_config="model=I2S,bck=26,ws=25,do=22,sda=21,scl=27,i2c=45"`.
   - On ESP32 (`xtensa`), what hardware registers does `esp-hal 1.2.2` configure for `I2S0`, and what format does `StreamPipeline` write into `Esp32HardwareI2sDma::write_dma_bytes` (16-bit vs 32-bit stereo samples, byte layout)?
3. **Continuous I2S Clocking Requirement for TAS5805M**:
   - Does `esp-hal` one-shot `write_dma` stop `BCK` (`GPIO26`) and `WS` (`GPIO25`) between transfers when idle, whereas `esp-hal` circular DMA (`write_dma_circular`) keeps `BCK` and `WS` continuously running?

---

## 3. Deliverable
Return a structured Markdown audit report with `[CRITICAL]`, `[HIGH]`, `[MEDIUM]`, `[LOW]` findings, exact `file:line` citations, root-cause proof, and concrete remediation code specifications.

---

## 4. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Subagent Conversation ID:** `c0b09298-42cd-4d83-be98-c9633a0b7193`
- **Execution Timestamp:** 2026-10-24T12:30:18-04:00
- **Key Findings:**
  1. `SEC-AUD-01` (`[CRITICAL]`, `src/main.rs:699-710`, `esp-hal-1.2.2/src/dma/buffers/mod.rs:1358-1360,1452-1523`): `DmaTxStreamBuf` (`dma_tx_stream_buffer!(4096, 1024)`) links 4 descriptors in a linear NULL-terminated list. Starting `i2s_tx.write(tx_buffer)` at boot drains all 4 descriptors (`512` frames) in `11.61 ms`, fires `TotalEof`, and halts the hardware `I2S0` DMA engine. When LMS streams audio later, `transfer.push()` fills the 4 `Owner::Cpu` descriptors (`4096` bytes = `512` frames), advancing `frames_played` to `512`, and sets their RAM owner bits to `Owner::Dma`; because hardware DMA is already halted (`transfer.is_done() == true`), they are never clocked out and `available_bytes()` returns `0` permanently.
  2. `SEC-AUD-02` (`[CRITICAL]`, `esp-hal-1.2.2/src/i2s/master/low_level/v1.rs:139`): `tx_stop_en = 1` stops `BCK` (`GPIO26`) and `WS` (`GPIO25`) whenever DMA halts, tripping TAS5805M clock fault.
  3. `SEC-AUD-03` (`[HIGH]`, `v1.rs:78-79`, `src/hal.rs:184-186`): `tx_right_first = 1` transmits the first 32-bit word on Right channel (`WS=High`) and the second 32-bit word on Left channel (`WS=Low`).
- **Fulfillment Status:** `[EXECUTED & VERIFIED]`
