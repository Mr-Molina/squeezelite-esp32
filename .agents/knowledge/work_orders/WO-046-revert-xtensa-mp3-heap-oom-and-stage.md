# Work Order: WO-046 — Remediate `xtensa` `Mp3Decoder` Heap OOM & Stack Frame Sizing (`--stage-only`)

**Work Order ID:** WO-046  
**Timestamp:** 2026-10-24T13:57:30-04:00  
**Assigned Subagent Role:** Rust App & Hardware Implementer (`Model: "flash"`, `TypeName: "self"`)  
**Target Files:**
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/Cargo.toml`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
- `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`
**Backup Files:**
- `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_Cargo.toml.bak2`
- `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_pipeline.rs.bak`
- `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_main.rs.bak`

---

## 1. Root-Cause Diagnosis & Single-Variable Scope (Rule 4, Rule 5, Rule 6)
- **Differential Analysis (`WO-045` vs `WO-040`):**
  1. `WO-045` proved that `AUD-02` (TAS5805M I2C fault thrashing) is **100% resolved** (`i2c_acks == 15` across `21s` vs `18,966,497` previously) and that Gate 1 (`otadata_prearmed_factory: true`) automatically recovers `172.22.1.15` to Factory `recovery` (`0x10000`, `"recovery": 1`) on any crash.
  2. When LMS (`10.0.0.10`) started streaming `"Ibiza Global Radio MP3"` at `t = +5.5s` in `WO-045`, because `HELO` advertised `,pcm,mp3` (`AUD-08`), LMS sent `format = AudioFormat::Mp3` (`'m'`) instead of server-transcoded PCM (`'p'`).
  3. `Box::new(Mp3Decoder::new())` (`squeezelite-decoders/src/lib.rs:514-521`) constructs `nanomp3_core::Decoder::new()` (`23,040` bytes on the CPU stack) and allocates `23,040 + 9,216 = 32,256` bytes on the `64 KB` `esp_alloc` Wi-Fi heap (which only has `~14 KB` free after `esp-radio` initialization), AND `Mp3Decoder::decode` requires `pcm_out.len() >= 2304` whereas `PCM_SCRATCH_SAMPLES` on `xtensa` is `512`. This triggered an immediate OOM/stack panic on `StreamCommandType::Start`.
  4. In `WO-040`, when `HELO` advertised `,pcm` only, LMS (`10.0.0.10`) transcoded `"Ibiza Global Radio MP3"` on the server into uncompressed 16-bit 44.1 kHz Stereo PCM (`format = 'p'`), and `PcmDecoder` (0 bytes heap, 6 bytes stack) ran for 43 minutes without crashing.
- **Hypothesis & Predicted Outcome:**
  - Reverting `AUD-08` (advertising `,pcm` only in `HELO` and gating `ActiveDecoder::Mp3` to `#[cfg(not(target_arch = "xtensa"))]`), extracting the `16,384`-byte `nvs_buf` into an `#[inline(never)]` helper function so its `16 KB` stack frame is freed before Wi-Fi/pipeline init, and keeping `stream_storage`/`output_storage`/`audio_rx_buf` at `4,096` bytes (`29 KB` total stack reduction in `run_hardware_firmware()`) while retaining `AUD-01..AUD-07` (`dma_tx_stream_buffer!(8192, 1024)` in static `.bss`, `ensure_running()`, `pump_silence_if_needed()`, `VOL_UNITY_0DB`, `tx_right_first = 1` channel swap, 8-step drain, and SlimProto EOF lifecycle) will compile cleanly, pass all 5 `--stage-only` gates, and eliminate the `StreamCommandType::Start` OOM/stack panic.
- **Prohibited Actions:** Never touch `/dev/ttyUSB0`. Do NOT send any HTTP requests to `172.22.1.15` during this `--stage-only` Work Order. Never use `sed`, `awk`, or `cat` to edit files.

---

## 2. Detailed Instructions

### Step 1: Update `rust/crates/louder-esp32-app/Cargo.toml`
Under `[target.'cfg(target_arch = "xtensa")'.dependencies]`, change:
```toml
squeezelite-decoders = { path = "../squeezelite-decoders", default-features = false, features = ["alloc"] }
```
back to:
```toml
squeezelite-decoders = { path = "../squeezelite-decoders", default-features = false }
```

### Step 2: Update `rust/crates/louder-esp32-app/src/pipeline.rs`
Restore `#[cfg(not(target_arch = "xtensa"))]` on `Mp3(Box<squeezelite_decoders::Mp3Decoder>)` in `enum ActiveDecoder`, `impl AudioDecoder for ActiveDecoder`, and `impl ActiveDecoder::sample_rate(&self)`, and remove the unused `#[cfg(target_arch = "xtensa")] extern crate alloc; use alloc::boxed::Box;` from `src/pipeline.rs`.

### Step 3: Update `rust/crates/louder-esp32-app/src/main.rs`
1. In `mod xtensa_hw`:
   - Remove `extern crate alloc;`, `use alloc::boxed::Box;`, and `use squeezelite_decoders::Mp3Decoder;`.
2. Add an `#[inline(never)]` helper function inside `mod xtensa_hw` right before `run_hardware_firmware()`:
   ```rust
   #[inline(never)]
   fn load_nvs_wifi_credentials(
       flash: &mut FlashStorage<'_>,
       sta_mac: &[u8; 6],
   ) -> Result<louder_esp32_app::ota_safety::NvsWifiCredentials, louder_esp32_app::ota_safety::NvsCredentialError> {
       let mut nvs_buf = [0xFFu8; 16_384];
       let _ = flash.read(NVS_PARTITION_OFFSET, &mut nvs_buf);
       resolve_wifi_credentials(
           &nvs_buf,
           sta_mac,
           option_env!("SL_WIFI_SSID").or(Some(DEFAULT_WIFI_SSID)),
           option_env!("SL_WIFI_PASS"),
       )
   }
   ```
   And in `run_hardware_firmware()`, replace the inline `let mut nvs_buf = [0xFFu8; 16_384]; ... resolve_wifi_credentials(...)` with `load_nvs_wifi_credentials(&mut flash, &sta_mac)` followed by `wdt.feed();`. This frees `16,384` bytes of main stack frame immediately after NVS is read!
3. In `run_hardware_firmware()`:
   - Set `let mut stream_storage = [0u8; 4096];`, `let mut output_storage = [0u8; 4096];`, `let mut audio_rx_buf = [0u8; 4096];`, and `let mut slim_acc_buf = [0u8; 1536];` (matching the proven stack footprint of `WO-040`, while `dma_tx_stream_buffer!(8192, 1024)` remains `8,192` bytes in static `.bss`).
4. In `HELO` (`SlimprotoBuilder::build_helo`), advertise:
   `"Model=SqueezeESP32,ModelName=SqueezeESP32,AccuratePlayPoints=1,HasDigitalOut=1,MaxSampleRate=48000,pcm"` (ONLY `,pcm` so LMS transcodes compressed formats on the server to uncompressed PCM).
5. In `StreamCommandType::Start`:
   - Replace the `match format { AudioFormat::Mp3 => ... }` block with zero-heap `PcmDecoder`:
     ```rust
     let bits = PlayerCoordinator::normalize_pcm_bits(pcm_sample_size);
     let channels = PlayerCoordinator::normalize_pcm_channels(pcm_channels);
     let is_be = matches!(pcm_endianness, Endianness::BigEndian);
     let pcm_dec = PcmDecoder::new(bits, channels, is_be);
     pipeline.set_decoder(ActiveDecoder::Pcm(pcm_dec));
     ```
   - Replace `let _ = amp.stop_playback(); let _ = amp.clear_faults(); let _ = amp.start_playback_via_hiz();` with:
     ```rust
     pipeline.sink.set_mute(false);
     pipeline.sink.driver.pump_silence_if_needed(false);
     let _ = amp.clear_faults();
     if amp.mode() != louder_esp32_app::AmpMode::Play {
         let _ = amp.start_playback_via_hiz();
     }
     ```
     (and remove the duplicate `let _ = amp.start_playback_via_hiz();` at the end of `StreamCommandType::Start`).

### Step 4: Run All 5 Verification Gates
1. `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace`
2. `/home/console-admin/.cargo/bin/cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets -- -D warnings`
3. `/bin/bash -c "source /home/console-admin/export-esp.sh && /home/console-admin/.cargo/bin/rustup run esp cargo build --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app"`
4. `/home/console-admin/.cargo/bin/espflash save-image --chip esp32 /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`
5. `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_staged_ota_bin.py`

### Step 5: Commit & Document
Commit to `MK5-Rust` (`git commit -m "fix(louder-esp32-app): use server-transcoded PCM on xtensa to eliminate Mp3Decoder heap OOM and isolate NVS stack frame"`), write a Rule 17 subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_140000_wo046_pcm_stack_fix.md`, and complete the Fulfillment Section below.

---

## 3. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Execution Timestamp:** 2026-10-24T14:04:00-04:00
- **Files Modified / Created (Absolute Paths):**
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/Cargo.toml` (modified)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs` (modified)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (modified)
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (created)
- **Test / Verification Command & Results:**
  1. `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace` -> PASS (178 tests passed, 0 failed).
  2. `cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets -- -D warnings` -> PASS (0 warnings).
  3. `/bin/bash -c "source /home/console-admin/export-esp.sh && /home/console-admin/.cargo/bin/rustup run esp cargo build --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app"` -> PASS (exit code 0).
  4. `/home/console-admin/.cargo/bin/espflash save-image --chip esp32 /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` -> PASS (Image successfully saved, 683,712 bytes, 16.56% of partition).
  5. `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_staged_ota_bin.py` -> PASS (`[STAGE-ONLY AUDIT PASS]` sha256 `6dc2e22504ceeb4a39eec5847967bb31890abd840c73ce63bceeffeafa290b38`, project `Squeezelite-ESP32`, version `I2S-4MFlash-Rust`).
- **Observed Outcome vs Predicted Outcome:**
  - Predicted: Host tests pass, clippy passes, Xtensa binary compiles cleanly with pure PCM decoder, image passes all OTA validation checks.
  - Observed: Exactly matched predicted outcome. Commit `7248a3c5` recorded on `MK5-Rust`.
- **Fulfillment Status:** [EXECUTED & VERIFIED]
- **Summary Path:** `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_140000_wo046_pcm_stack_fix.md`
- **Handoff Path:** `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-046-revert-xtensa-mp3-heap-oom-and-stage.md`
