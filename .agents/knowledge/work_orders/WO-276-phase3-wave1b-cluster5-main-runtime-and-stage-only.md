# Work Order: WO-276 — Perf Phase 3 Wave 1B Cluster 5: `main.rs` Bare-Metal Runtime Integration, `64 KiB` PSRAM TCP Window, `I2S0` DMA Buffer Expansion, Post-Connect `PowerSaveMode::None`, IRAM Placement, & `--stage-only` Build

- **Work Order ID**: `WO-276`
- **Timestamp**: `2026-11-01T14:51:00-05:00`
- **Assigned Subagent Role**: `Cluster 5 Bare-Metal Runtime Integrator & Stage-Only Packager` (`Model: "pro"`, `TypeName: "self"`)
- **Target Host / Component**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
  - Full workspace verification & `xtensa-esp32-none-elf` `--stage-only` build
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Scope
Integrate all Phase 3 performance remediations into `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (`NET-401`, `HW-PERF-202`, `NET-402`, `HW-PERF-103`, `CPU-102`, `CPU-103d`, `CPU-104c`, `PROT-501`, `PROT-502`, `PROT-503`, `PROT-504`, `NET-404`), run full workspace tests/Clippy/GRS static audit, build the `xtensa-esp32-none-elf` release image, verify `.stack >= 36,000` B and `.iram0.text <= 131,072` B, and package + verify the `--stage-only` OTA binary (`/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-perf.bin`).
- **STRICT SAFETY**: Do NOT flash or reboot `172.22.1.15`. Never touch `/dev/ttyUSB0`. Use `edit_file` (never `sed`/`awk`/`cat`).

## 2. Mandatory Remediations in `main.rs`
0. **`PROT-504` & `PROT-502` Field Initialization in `init_static_pipeline()` (`main.rs:1692-1702`) & `EspBtdmVhciTransport`**:
   - **CRITICAL (`WO-275` integration)**: In `init_static_pipeline()` (`main.rs:1695`), after `core::ptr::addr_of_mut!((*ptr).resampler).write(None);`, explicitly write the two new `StreamPipeline` fields added by `WO-275`:
     ```rust
     core::ptr::addr_of_mut!((*ptr).output_threshold_frames).write(4_096);
     core::ptr::addr_of_mut!((*ptr).threshold_satisfied).write(false);
     ```
     (`4,096` stereo frames = `~92.8 ms` pre-buffer cushion in `output_buffer` before `I2S0` DMA drainage starts!).
   - Also check `impl HciTransport for ...` in `main.rs` and implement `fn can_read(&self) -> bool` (returning true when the VHCI RX queue has a packet available, e.g. `!VHCI_RX_QUEUE_EMPTY` or checking the ring head != tail) so `bt_sink.poll()` does not allocate/zero a 1,024-byte stack array when the VHCI RX ring is empty!
1. **`NET-401` & `HW-PERF-202` (`CRITICAL`, `main.rs:1089-1096, 2116-2126`) — Expand `audio_rx_buf` (`65,536` B) & `slim_rx_buf` (`8,192` B) in PSRAM**:
   - In `bring_up()` (`main.rs:2116-2126`), when `psram_avail_bytes >= 655_360` (`4 MB` PSRAM active as `esp_alloc::HEAP` Region 0), allocate:
     - `let slim_rx_vec = alloc::vec![0u8; 8_192];` (`8 KiB` SlimProto receive window = 8 MSS segments, eliminating `:3483` retransmissions)
     - `let audio_rx_vec = alloc::vec![0u8; 65_536];` (`64 KiB` HTTP audio receive window = **16x larger TCP window**, increasing WAN/LAN TCP throughput ceiling by 16x!)
     and pass `SocketBuffer::new(&mut slim_rx_vec[..])` and `SocketBuffer::new(&mut audio_rx_vec[..])` (or if `psram_avail_bytes < 655_360`, fall back to `&mut fw_bufs.slim_rx_buf[..]` and `&mut fw_bufs.audio_rx_buf[..]`).
2. **`NET-402` (`CRITICAL`, `main.rs:3380-3442, 3909-3921`) — Immediate Egress `smol_iface.poll(...)` After Draining `audio_sock`**:
   - In `run_single_loop_step` (`main.rs:3909-3921`), immediately after `slim_session_phase(s, now_ms);` (step 6), call:
     ```rust
     let _ = s.smol_iface.poll(
         SmolInstant::from_millis(i64::try_from(now_ms).unwrap_or(0)),
         s.smol_dev,
         s.sockets,
     );
     ```
     so that TCP Window Update ACKs (`rcv_wnd`) and `STAT` replies are transmitted onto the wire immediately after `poll_stream` drains `audio_sock` into `stream_buffer`, without waiting for `output_decode_phase`.
3. **`HW-PERF-103` (`MEDIUM`, `main.rs:2941-2978`) — Re-apply `PowerSaveMode::None` After Wi-Fi Association**:
   - In `wifi_and_lease_phase` (`main.rs:2950-2960`), when `wifi_connected` is `true` and `!s.wifi_connected_logged`:
     Immediately call `let _ = s.wifi_controller.set_power_saving(PowerSaveMode::None);` so the Espressif 802.11 stack stays continuously awake (`WIFI_PS_NONE`) after `connect_async()` instead of reverting to 100 ms `WIFI_PS_MIN_MODEM` DTIM sleep!
4. **`CPU-102` (`HIGH`, `main.rs:1865-1885`) — Expand Hardware `I2S0` DMA Buffer (`tx_buffer`) in `.dram2_uninit`**:
   - Currently `let (_, _, tx_buffer, tx_descriptors) = dma_tx_stream_buffer!(8192, 1024);` places `8,192` B (`23.22 ms`) in `.bss` (which shrinks `.stack` in `dram_seg`).
   - Instead, place a static `24,576`-byte (`24 KiB` = `3,072` stereo frames = **`69.66 ms` at 44.1 kHz**, **3x deeper hardware DMA cushion**!) buffer in `.dram2_uninit` (`#[link_section = ".dram2_uninit"] static mut DMA_TX_BUF: [u8; 24576] = [0u8; 24576];`, which is internal DMA-capable SRAM `0x3FFE_0000..0x4000_0000` and costs **0 bytes** in `.bss`/`.stack`!), and construct `DmaTxStreamBuf::new(&mut tx_descriptors, dma_tx_slice)` with `[DmaDescriptor::EMPTY; 25]` (`24` chunks of `1024` B + 1)!
   - Note: removing the `8,192` B `tx_buffer` from `.bss` (`dma_tx_stream_buffer!`) will **increase `.stack` by `~8,000` bytes** (from `37,392` B to `~45,000` B)!
5. **`CPU-103d` (`HIGH`, `main.rs:795-835, 3840-3868`) — Place Hot DMA & Decode Loop Functions in IRAM**:
   - Add `#[esp_hal::ram]` to `Esp32HardwareI2sDma::write_dma` and `output_decode_phase` in `main.rs`.
   - Check `llvm-readelf -S` after building `xtensa-esp32-none-elf`: verify `.iram0.text` (`0x40080000..0x400A0000`, max `131,072` B = `0x20000`) fits with headroom! (If `.iram0.text` exceeds `131,072` B, drop `#[link_section = ".iram1"]` from the largest function `mp3d_dct_ii` so `.iram0.text` stays safely `<= 129,000` B).
6. **`CPU-104c` (`CRITICAL`, `main.rs:3877-3887`) — Add 30-Second Fault-Recovery Cooldown & `has_critical_amp_fault()` to `amp_and_cyd_health_phase`**:
   - In `main.rs:3877-3887` (`amp_and_cyd_health_phase`):
     - Currently:
       ```rust
       if let Ok((fault, _telemetry)) = s.amp.check_faults_and_telemetry() {
           if !fault.is_clean() {
               let _ = s.amp.recover_from_fault();
           }
       }
       ```
     - Change `!fault.is_clean()` to `fault.has_critical_amp_fault()` AND gate `s.amp.recover_from_fault()` on a 30-second cooldown (`now_ms.wrapping_sub(*s.last_fault_recovery_ms) >= 30_000`, updating `*s.last_fault_recovery_ms = now_ms`), matching `tasks.rs:1535`! This completely stops the 2-second `Hi-Z -> Clear -> Play` amplifier cycling during playback!
7. **`PROT-501`, `PROT-502`, `PROT-503`, & `NET-404` (`main.rs:2846-2895, 3068-3072, 3474-3518, 3654, 3726`)**:
   - **`PROT-501` (`main.rs:3474-3518`)**: Remove the `else if payload.len() >= 12 && (payload[2] & 0x80) == 0` wildcard mDNS branch in `multi_proto_poll` so the ESP32 only replies when `_raop._tcp` or `_spotify-connect._tcp` is actually queried (plus periodic 60s announcement if desired).
   - **`PROT-503` (`main.rs:3654, 3726`)**: Replace `let mut pkt_buf = alloc::vec![0u8; 1536];` (line 3654) and `let mut buf = alloc::vec![0u8; 1024];` (line 3726) with stack/static scratch buffers so zero heap allocations occur per RTP/Spotify packet.
   - **`PROT-502` (`main.rs:2846-2895`)**: In `bt_poll_phase`, when `s.bt_sink.connected_peer().is_none()` and `!s.bt_source.is_streaming()`, throttle `bt_poll_phase` to run once every `2 ms` (`now_ms != *s.last_bt_idle_poll_ms` or `>= 2 ms`) instead of 4,000 times/sec.
   - **`NET-404` (`main.rs:3068-3072`)**: Increase `let mut req_buf = [0u8; 256];` in `http_serve_phase` to `[0u8; 1024]`.

## 3. Mandatory Verification Gates (`--stage-only`)
1. `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace` (`0` failures).
2. `cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets -- -D warnings` (`0` warnings).
3. `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates --strict` (`0` errors).
4. Xtensa release build & ELF section verification:
   - Build `xtensa-esp32-none-elf` release binary (`louder-esp32-app`).
   - Verify via `llvm-readelf -S`:
     - `.stack >= 36,000` B (Invariant 47)
     - `.iram0.text <= 131,072` B (`0x20000`)
     - `.dram2_uninit <= 131,072` B (`0x20000`)
5. Package `--stage-only` OTA binary to `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-perf.bin` via `espflash save-image --chip esp32` and run `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/repo_scratch_archive/verify_staged_ota_bin.py /home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-perf.bin` (`[STAGE-ONLY AUDIT PASS]`).

## 4. Fulfillment Section
- **Fulfillment Status**: `[EXECUTED & VERIFIED]`
- **Subagent Summary Path**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101_wo276_cluster5_main_runtime_stage_only.md`
- **Staged OTA Binary Path**: `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-perf.bin`
- **Staged Binary SHA-256**: `265872e611388dee54ce5d86c5e82b26d00aa7b96b168bb21d9f5ae07783a88c`
- **Gate 1 (Workspace Tests)**: `PASS` (318 unit + 43 CYD + 25 RAOP + 18 TAS + fuzz tests pass, 0 failures).
- **Gate 2 (Clippy)**: `PASS` (0 warnings across workspace).
- **Gate 3 (Google Rust Style)**: `PASS` (`0 error(s), 0 warning(s)` across all crate `src/` dirs).
- **Gate 4 (Xtensa Release & ELF Sections)**: `PASS` (`.stack = 37,296` B >= 36,000 B, `.iram0.text = 126,664` B <= 131,072 B, `.dram2_uninit = 96,472` B <= 98,768 B, Total `Internal` Heap = `65 KB`).
- **Gate 5 (`--stage-only` Packaging & Verification)**: `PASS` (`[STAGE-ONLY AUDIT PASS]`, size `1,887,440` B, 70.24% of `ota_0`).
- **Safety Boundary**: Zero flashing or rebooting to `172.22.1.15`, zero interaction with `/dev/ttyUSB0`. All changes verified offline.
