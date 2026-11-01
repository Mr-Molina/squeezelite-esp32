# Subagent Summary: WO-235 — Wave 10C: Remediate `.dram2_uninit` `init_static_pipeline()`, PSRAM `esp_alloc::HEAP` Region 0 Priority, `Resampler` Stack Frame, & `.stack >= 36,000` B (`--stage-only`)

- **Work Order ID**: `WO-235`
- **Subagent Conversation ID**: `e80326f4-ad1c-429f-a969-1741711a6be9`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Files Created, Modified, or Deleted (Absolute Paths)
- **Modified**:
  1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/resampler.rs`
  2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs`
  3. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
  4. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-235-wave10c-remediate-boot-init-psram-region0-stack.md`
- **Created (Backups & Staged Artifacts)**:
  1. `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/resampler.rs.wo235.bak`
  2. `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/pipeline.rs.wo235.bak`
  3. `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/coordinator.rs.wo235.bak`
  4. `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/main.rs.wo235.bak`
  5. `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/http_control.rs.wo235.bak`
  6. `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-full-parity.bin` (`1,794,896` B, SHA-256 `e830ed90119d536f8c6eadb817000e371ee3afcf7476a9dcbb0797db96b5ef74`)
  7. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo235_wave10c_boot_psram_stack_fix.md`
  8. `/home/console-admin/.gemini/antigravity/brain/e80326f4-ad1c-429f-a969-1741711a6be9/handoff.md`

## 2. Rationale for Each Change
1. **`init_static_pipeline()` Zero-Fill & `WO-228` Field Initialization (`main.rs`)**:
   - Added `ptr.write_bytes(0, 1);` at the beginning of `init_static_pipeline()` and explicitly initialized `eq_gains_db` (`[0.0; 10]`), `loudness_level` (`0`), `active_sample_rate_hz` (`44_100`), and `resampler` (`None`). Previously, `STATIC_PIPELINE` in `.dram2_uninit` left `resampler: Option<Box<Resampler>>` uninitialized, causing `amp_apply_boot_config()` -> `pipeline.apply_eq_preset()` / `clear_resampler()` to drop a garbage pointer in `.dram2_uninit` at boot.
2. **4 MB External PSRAM as `esp_alloc::HEAP` Region 0 (`main.rs`)**:
   - Registered `EXTMEM_ORIGIN` (`0x3F80_0000`, `4,194,304` B, `MemoryCapability::External`) as Region 0 BEFORE `esp_alloc::heap_allocator!(size: 43 * 1024)` (Region 1) and `STATIC_BT_HEAP` (`22 * 1024`, Region 2). Because standard Rust `GlobalAlloc::alloc` allocates from Region 0 first while `esp-rtos`/`esp-radio` request `MemoryCapability::Internal`, all Rust `Box`/`Vec` allocations (`prewarm_opus()`, `raop_rtsp`, `raop_rtp`, `spot_spirc`, runtime buffers) land in 4 MB PSRAM while 100% of the `65 KB` internal DRAM heap is preserved for Wi-Fi and Bluetooth.
3. **Eliminate 13 KB `Resampler` Stack Frame (`resampler.rs`)**:
   - Changed `coeffs_q30` and `history` in `Resampler` from inline arrays (`12,416` B + `512` B) to heap-allocated boxed slices (`Box<[[i32; MAX_TAPS]]>` and `Box<[[i32; 2]]>`), reducing `size_of::<Resampler>()` to `<= 64` B (48 B on 32-bit Xtensa) and eliminating the `13,008` B stack frame in `<StreamPipeline>::configure_resampler`.
4. **Restore `.stack >= 36,000` B & `.dram2_uninit <= 98,768` B (`main.rs`)**:
   - Allocated post-PSRAM statics (`AMP`, `CYD_BRIDGE`, `SERVER_FRAME_BUF`, `COORDINATOR`, `SMOL_IFACE`, `SOCKET_STORAGE`, `MDNS_UDP_BUFFERS`, `RTP_UDP_META_BUFFERS`, `RUNTIME_STATE`, `http_resp_buf`, `http_tx_buf`) via `Box::leak(Box::new(...))` in PSRAM, expanding `.stack` to `37,416` B (`+10,184` B) and keeping `.dram2_uninit` at `95,256` B (`<= 98,768` B).
5. **Multi-Protocol `/status.json` & Preemption Parity (`main.rs`, `coordinator.rs`)**:
   - Restored 4 MB PSRAM region reporting (`r.size > 1024 * 1024`), active-source `codec_name`, `HttpControlAction::SpotifyPlay`/`SpotifyStop`, mDNS multicast join/query responder, RTSP `ANNOUNCE`/`RECORD`/`TEARDOWN`, RTP UDP 6000 direct activation + 3,000 ms silence release, and automatic SlimProto decoder restoration on preemption release.

## 3. Known Risks & Untested Assumptions
- Staged in `--stage-only` mode per `WO-235` constraints; live OTA flash and 6-Gate multi-protocol verification on `172.22.1.15` (`"BASEMENT"`) is scheduled for `WO-236`.
