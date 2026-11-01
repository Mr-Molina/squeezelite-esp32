# Work Order: WO-235 — Wave 10C: Remediate `.dram2_uninit` `init_static_pipeline()`, PSRAM `esp_alloc::HEAP` Region 0 Priority, `Resampler::new_boxed()`, & `.stack >= 36,000` B (`--stage-only`)

- **Work Order ID**: `WO-235`
- **Timestamp**: `2026-11-01T01:30:00-04:00`
- **Assigned Subagent Role**: `Rust Bare-Metal Memory & Boot Remediation Implementer` (`TypeName: "self"`)
- **Target Host / Component**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/resampler.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Hypothesis
Remediate the exact boot rollback root causes isolated by `WO-233` on `"BASEMENT"` (`172.22.1.15`):
1. **Fix `init_static_pipeline()` in `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (`1602-1661`)**:
   - Call `ptr.write_bytes(0, 1);` at the start of the `unsafe` block so `STATIC_PIPELINE` in `.dram2_uninit` is completely zeroed before field writes.
   - Explicitly initialize the 4 fields added in `WO-228`:
     - `core::ptr::addr_of_mut!((*ptr).eq_gains_db).write([0.0; 10]);`
     - `core::ptr::addr_of_mut!((*ptr).loudness_level).write(0);`
     - `core::ptr::addr_of_mut!((*ptr).active_sample_rate_hz).write(44_100);`
     - `core::ptr::addr_of_mut!((*ptr).resampler).write(None);`
2. **Restore 4 MB External PSRAM as `esp_alloc::HEAP` Region 0 in `main.rs` (`2365-2402`)**:
   - Matching the proven working `WO-202` / `WO-209` / `WO-214` (`local/wo157-wo214-codecs-airplay-spotify`) heap initialization order:
     1. Run `let psram_bytes = unsafe { init_and_map_psram_4mb() };` **FIRST** and register `EXTMEM_ORIGIN` (`0x3F80_0000`, `4,194,304` B, `esp_alloc::MemoryCapability::External.into()`) as **Region 0** of `esp_alloc::HEAP`!
     2. Register `esp_alloc::heap_allocator!(size: ...)` (`Internal` DRAM Region 1) **SECOND**.
     3. Register `STATIC_BT_HEAP` (`Internal` DRAM Region 2) **THIRD**.
   - Rationale: In `esp_alloc`, standard Rust `GlobalAlloc::alloc` (`Box::new`, `vec!`) allocates from Region 0 first, whereas `esp-rtos` and `esp-radio` (`libbtdm_app.a` + Wi-Fi) explicitly request `MemoryCapability::Internal`. When `Internal` DRAM was Region 0, `squeezelite_decoders::prewarm_opus()` + `raop_rtsp` + `raop_rtp` + `spot_spirc` exhausted the `Internal` DRAM heap before `init_wifi()` ran! With 4 MB PSRAM as Region 0, all Rust `Box`/`Vec` allocations land in PSRAM and 100% of Internal DRAM is preserved for Wi-Fi + Bluetooth.
3. **Eliminate the 13 KB `Resampler` Stack Frame (`squeezelite-audio-core/src/resampler.rs` & `louder-esp32-app/src/pipeline.rs:1349`)**:
   - In `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/resampler.rs`:
     - Add `#[cfg(any(feature = "alloc", feature = "std", test))]` (or use `extern crate alloc;` in `squeezelite-audio-core` if `alloc` is available, or box `history` / `filter_table` or provide an `init_in_place(ptr: *mut Self, ...)` / `new_boxed(...) -> Option<Box<Self>>` in `pipeline.rs` using `Box::<Resampler>::new_uninit()` + `ptr.write_bytes(0, 1)` + field writes + `retune()`!) so `<StreamPipeline>::configure_resampler` constructs `Resampler` in-place on the PSRAM heap without ever placing a `12,984` B `Resampler` temporary on the stack (`entry a1, 13008` must drop to `< 512` B!).
     - Note: Check `squeezelite-audio-core/Cargo.toml`: `squeezelite-audio-core` has `#![forbid(unsafe_code)]`, or if `Resampler` stores `history` and `filter_table` or has `pub fn reset_and_retune(&mut self, from_hz: u32, to_hz: u32, quality: ResampleQuality) -> bool`, or if `Resampler` uses `alloc::boxed::Box<[[i32; 512]; 2]>` / `alloc::boxed::Box<[[i32; MAX_TAPS]; NUM_PHASES]>` (which makes `core::mem::size_of::<Resampler>() <= 64` bytes with zero `unsafe` code!), check how clean that is: if `history: Box<[[i32; 512]; 2]>` (`vec![[0i32; 512]; 2]`) and `filter_table: Box<[[i32; MAX_TAPS]; NUM_PHASES]>` (or `Vec<[i32; MAX_TAPS]>`) are heap-allocated inside `Resampler` (with `extern crate alloc;` in `squeezelite-audio-core`), then `size_of::<Resampler>()` is `< 64` bytes, `Resampler::new(...)` uses `< 256` bytes of stack, `#![forbid(unsafe_code)]` stays 100% intact, and all existing `Resampler::new(...)` callers and unit/integration tests work unchanged!
4. **Restore `.stack >= 36,000` B & `.dram2_uninit <= 98,768` B in `main.rs`**:
   - Currently `.stack` in `dram0_0_seg` is `27,232` B (`8,768` B short of `36,000` B) because `SOCKET_STORAGE` (`2,760` B), `MDNS_UDP_BUFFERS` (`2,180` B), `RTP_UDP_META_BUFFERS` (`164` B), `CYD_BRIDGE` (`448` B), `SMOL_IFACE` (`424` B), `COORDINATOR` (`328` B), `RUNTIME_STATE` (`496` B), and `AMP` (`344` B) (`7,144` B total) are in `.data`/`.bss` (`dram0_0_seg`), and `esp_alloc::heap_allocator!(size: 46 * 1024)` takes `47,104` B in `.bss`.
   - Since `init_amp_i2c`, `init_cyd_uart`, `init_coordinator`, `init_net_stack`, and `RUNTIME_STATE` ALL execute **AFTER** `init_and_map_psram_4mb()` registers 4 MB PSRAM as Region 0:
     - Allocate `SOCKET_STORAGE`, `MDNS_UDP_BUFFERS`, `RTP_UDP_META_BUFFERS`, `CYD_BRIDGE`, `SMOL_IFACE`, `COORDINATOR`, `AMP`, `RUNTIME_STATE` (and `SERVER_FRAME_BUF` `2,048` B, `STATIC_HTTP_RESP_BUF` `1,536` B, `STATIC_HTTP_TX_BUF` `1,536` B) via `alloc::boxed::Box::leak(alloc::boxed::Box::new(...))` in 4 MB PSRAM!
     - Set `esp_alloc::heap_allocator!(size: 43 * 1024)` (`44,032` B in `.bss` `dram0_0_seg`) and `STATIC_BT_HEAP: MaybeUninit<[u8; 22 * 1024]>` (`22,528` B in `.dram2_uninit`, giving `43 KB + 22 KB = 65 KB` of 100% dedicated `Internal` DRAM heap for Wi-Fi + BTDM!).
     - Verify with `xtensa-esp32-elf-size -A` that `.stack >= 36,000` B and `.dram2_uninit <= 98,768` B!
5. **Restore Multi-Protocol `/status.json` & Preemption Parity in `main.rs` & `coordinator.rs`**:
   - In `main.rs::http_serve_phase`:
     - Compute `psram_free_bytes` and `psram_used_bytes` from `esp_alloc::HEAP.stats().region_stats` where `r.size > 1024 * 1024` (matching `WO-214`), so `/status.json` reports exact 4 MB PSRAM region stats (`free + used == 4,194,304`).
     - Compute `codec_name` by checking `s.coordinator.active_source()` (`AudioSource::AirPlay => "AirPlay"`, `AudioSource::Bluetooth => "Bluetooth"`, `AudioSource::Spotify => "Spotify"`) and `s.raop_streaming` before falling back to `s.pipeline.decoder`.
     - Handle `HttpControlAction::SpotifyPlay` and `HttpControlAction::SpotifyStop` in `http_serve_phase`.
   - In `main.rs::multi_proto_poll` & `coordinator.rs::poll_stream`:
     - In `coordinator.rs::poll_stream`: if `self.external_playback || matches!(self.arbiter.active_source(), AudioSource::AirPlay | AudioSource::Spotify | AudioSource::Bluetooth)` return `Ok(0)` immediately so LMS HTTP stream polling does not overwrite `pipeline.stream_buffer` during external playback.
     - In `main.rs::multi_proto_poll`:
       - Announce Spotify Connect mDNS on port `80` (where `http_serve_phase` serves `/?action=getInfo` and `/?action=addUser` via `ZeroConfServer::handle_request`) and respond to mDNS `_raop` / `_spotify-connect` queries.
       - On RTSP `RtspMethod::Announce`: configure `s.raop_rtp` crypto (`configure_crypto` / `disable_crypto`) and ALAC (`configure_alac`).
       - On UDP `6000` RTP packet receipt: if `s.raop_rtp.process_packet` succeeds and `s.coordinator.request_source(AudioSource::AirPlay)` succeeds, automatically activate AirPlay playback (`PcmDecoder::new(16, 2, false)`, `begin_external_audio()`, `begin_external_playback()`, `s.raop_streaming = true`) even if packets arrive directly, and auto-release after `3,000` ms RTP silence when buffers drain, restoring `s.pipeline.use_mp3_decoder()` + `s.coordinator.end_external_playback()` if `active_source() == AudioSource::SlimProto`.
       - On TCP `4070` (`spot_sock`): on `SpircEvent::Play | SpircEvent::LoadTrack`, configure `PcmDecoder::new(16, 2, false)` (or `VorbisDecoder` if data starts with `b"OggS"`), send `s.spot_spirc.build_notify_frame(frame.seq_nr, "BASEMENT")` back on `spot_sock`, and on `SpircEvent::Pause` release `AudioSource::Spotify` and restore `use_mp3_decoder()` + `end_external_playback()` if returning to `SlimProto`.

## 2. Hard Safety Constraints
1. **Stage-Only Mandate (`Invariant 42` / `single-target-troubleshooting.md`)**: Do **NOT** flash `172.22.1.15` in this work order (`WO-236` will perform the single-shot OTA flash after `WO-235` is verified) and **NEVER** touch `/dev/ttyUSB0`.
2. **Pre-Modification Backups (`Rule 35`)**: Back up any modified files to `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/` before editing.
3. **Google Rust Style & API Guidelines (`GRS-001`–`GRS-012`)**: Must pass `google_rust_audit.py --strict` (`0` errors, `0` warnings) and `cargo clippy --workspace --all-targets -- -D warnings` (`0` warnings).
4. **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` returns zero active tasks before completing.

## 3. Verification Gates (`--stage-only`)
1. **Gate 1**: `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace` (`0` failures).
2. **Gate 2**: `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py --strict` across all 11 workspace crate `src/` directories (`0` errors, `0` warnings).
3. **Gate 3**: `/home/console-admin/.cargo/bin/cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets -- -D warnings` (`0` warnings).
4. **Gate 4**: Bare-metal `xtensa-esp32-none-elf` release build (`--no-default-features --features board-louder-esp32`):
   - Verify `.stack >= 36,000` B!
   - Verify `.dram2_uninit <= 98,768` B!
   - Run `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/92e605c6-2351-418f-8416-f0324ebabd6f/scratch/analyze_elf.py` and verify `<StreamPipeline>::configure_resampler` frame is `< 512` B (no `13,008` B stack frame!).
5. **Gate 5**: Package staged OTA binary to `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-full-parity.bin`, verify header/size/SHA-256, and update `.agents/cve_audit_ledger/.manifest_hash` if `Cargo.toml`/`Cargo.lock` changed.

## 4. Deliverables
- Append Fulfillment Summary to §5 of this Work Order.
- Write subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo235_wave10c_boot_psram_stack_fix.md`.

---

## 5. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent Conversation ID**: `e80326f4-ad1c-429f-a969-1741711a6be9`
- **Pre-Modification Backups (`Rule 35`)**:
  - `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/resampler.rs.wo235.bak`
  - `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/pipeline.rs.wo235.bak`
  - `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/coordinator.rs.wo235.bak`
  - `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/main.rs.wo235.bak`
  - `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/http_control.rs.wo235.bak`
- **Files Modified**:
  1. [resampler.rs](file:///home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/resampler.rs):
     - Added `extern crate alloc; use alloc::boxed::Box;`.
     - Converted `coeffs_q30` (`Box<[[i32; MAX_TAPS]]>`) and `history` (`Box<[[i32; 2]]>`) to heap-allocated boxed slices (`alloc::vec![...].into_boxed_slice()`) and packed scalar fields so `core::mem::size_of::<Resampler>() <= 64` bytes (48 B on 32-bit Xtensa, 64 B on 64-bit host), eliminating the `13,008` B stack frame in `<StreamPipeline>::configure_resampler` while keeping `#![forbid(unsafe_code)]` intact.
  2. [coordinator.rs](file:///home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs):
     - Updated `begin_external_playback()` to preserve `self.stream = StreamLink::Open` and `self.current_format` when an active LMS stream is preempted by AirPlay, Spotify, or Bluetooth, and updated `end_external_playback()` to restore `PlayerState::Playing` when `self.arbiter.active_source() == AudioSource::SlimProto && self.stream == StreamLink::Open`.
     - Updated `poll_stream()` to return `Ok(0)` immediately when `self.external_playback || matches!(self.arbiter.active_source(), AudioSource::AirPlay | AudioSource::Spotify | AudioSource::Bluetooth)`.
     - Updated `evaluate_state()` to return early when `self.external_playback` is `true`.
  3. [main.rs](file:///home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs):
     - Fixed `init_static_pipeline()` (`1602-1661`) by calling `ptr.write_bytes(0, 1);` before field writes and explicitly initializing `eq_gains_db` (`[0.0; 10]`), `loudness_level` (`0`), `active_sample_rate_hz` (`44_100`), and `resampler` (`None`), while allocating `http_resp_buf` and `http_tx_buf` (`2 x 1,536` B) in PSRAM.
     - Reordered `esp_alloc::HEAP` initialization in `bring_up()` so `init_and_map_psram_4mb()` runs **FIRST** and registers `EXTMEM_ORIGIN` (`0x3F80_0000`, `4,194,304` B, `MemoryCapability::External`) as **Region 0**, followed by `esp_alloc::heap_allocator!(size: 43 * 1024)` (`Internal` DRAM Region 1) and `STATIC_BT_HEAP` (`22 * 1024`, `Internal` DRAM Region 2), preserving 100% of the `65 KB` internal DRAM heap for `esp-rtos` and `esp-radio` (`Wi-Fi` + `BLE`/`BTDM`).
     - Moved post-PSRAM statics (`AMP`, `CYD_BRIDGE`, `SERVER_FRAME_BUF`, `COORDINATOR`, `SMOL_IFACE`, `SOCKET_STORAGE`, `MDNS_UDP_BUFFERS`, `RTP_UDP_META_BUFFERS`, `RUNTIME_STATE`) to `Box::leak(Box::new(...))` in PSRAM, expanding `.stack` in `dram0_0_seg` from `27,232` B to **`37,416` B** (`>= 36,000` B) and reducing `.dram2_uninit` to **`95,256` B** (`<= 98,768` B).
     - Updated `http_serve_phase()`, `bt_poll_phase()`, `net_poll_phase()`, `multi_proto_poll()`, and `restore_suspended_or_stop()` for 4 MB PSRAM region stats (`r.size > 1024 * 1024`), active-source `codec_name` (`AirPlay`, `Spotify`, `Bluetooth`, or `ActiveDecoder`), `HttpControlAction::SpotifyPlay`/`SpotifyStop`, mDNS multicast join/query responder, RTSP `ANNOUNCE`/`RECORD`/`TEARDOWN`, RTP UDP 6000 direct activation + 3,000 ms silence release, and automatic SlimProto decoder restoration on preemption release.
- **5-Gate Verification Evidence (`--stage-only`)**:
  1. **Gate 1 (`cargo test --workspace`)**: `[EXECUTED & VERIFIED]` — `703` passed, `0` failed (`exit code 0`).
  2. **Gate 2 (`google_rust_audit.py --strict` across all 11 workspace crate `src/` directories)**: `[EXECUTED & VERIFIED]` — `0 error(s), 0 warning(s)` (`exit code 0`).
  3. **Gate 3 (`cargo clippy --workspace --all-targets -- -D warnings`)**: `[EXECUTED & VERIFIED]` — `0` warnings (`exit code 0`).
  4. **Gate 4 (`xtensa-esp32-none-elf` release build, `xtensa-esp32-elf-size -A`, & `analyze_elf.py`)**: `[EXECUTED & VERIFIED]`
     - `.stack`: **`37,416` B** (`>= 36,000` B requirement)
     - `.dram2_uninit`: **`95,256` B** (`<= 98,768` B ceiling, `3,512` B margin)
     - `.bss`: `72,544` B | `.data`: `18,992` B | `.text`: `1,406,697` B | `.rodata`: `207,552` B
     - `<StreamPipeline>::configure_resampler` frame: `< 512` B (eliminated `13,008` B stack frame)
     - `bring_up` frame: `704` B | `init_static_pipeline` frame: `416` B | `multi_proto_poll` frame: `992` B
  5. **Gate 5 (`--stage-only` OTA Binary Packaging & Header Audit)**: `[EXECUTED & VERIFIED]`
     - Staged Path: `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-full-parity.bin`
     - Size: `1,794,896` bytes (`43.47%` of 4 MB flash; `<= 1,835,008` B `ota_0` limit, `40,112` B headroom)
     - SHA-256: `e830ed90119d536f8c6eadb817000e371ee3afcf7476a9dcbb0797db96b5ef74`
     - Header Verification: `img_magic=0xE9`, `app_magic=0xABCD5432`, `version='I2S-4MFlash-Rust'`, `project='Squeezelite-ESP32'` (`[STAGE-ONLY AUDIT PASS]`).
     - Zero live hardware flashing (`172.22.1.15`) or serial port (`ttyUSB0`) interaction performed.
