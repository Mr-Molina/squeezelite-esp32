# Work Order: WO-083 — Rust Workspace, Codecs, PSRAM, Dual-Core, Networking & Feature-Parity Deep Audit

| Field | Value |
| :--- | :--- |
| **Work Order ID** | `WO-083` |
| **Timestamp** | `2026-10-25T11:06:00-04:00` |
| **Assigned Role** | Rust Workspace, Codec & Connectivity Architect (`Model: "pro"`, `TypeName: "research"`) |
| **Target Scope** | All 8 crates in `rust/crates/*` (`squeezelite-proto`, `squeezelite-audio-core`, `squeezelite-tas58xx`, `squeezelite-decoders`, `squeezelite-cyd-link`, `squeezelite-config`, `louder-esp32-app`, `cyd-controller-app`) and `esp-hal 1.0.0` / `esp-alloc 0.9.0` / `esp-radio 0.17.0` |
| **Status** | `[IN PROGRESS]` |

## 1. Objective
Audit our entire 8-crate Rust workspace and installed `esp-*` crate APIs (`~/.cargo/registry/src/`) to identify every implemented crate feature, codec, DSP module, protocol handler, and hardware capability (`PSRAM`, `Dual-Core APP_CPU`, `Wi-Fi PowerSave`, `Bluetooth VHCI`) that was built in the workspace or supported by `esp-hal`/`esp-radio` but left disabled, stubbed, or unused on the live `xtensa` binary (`louder-esp32-app`).

## 2. Required Inspection Areas
1. **`esp-hal 1.0.0` & `esp-alloc 0.9.0` 4 MB PSRAM Exact API**:
   - Inspect `~/.cargo/registry/src/*/esp-hal-1.0.0/` and `~/.cargo/registry/src/*/esp-alloc-0.9.0/` to verify the exact syntax and feature flags (`psram`, `esp_hal::psram::PsramConfig`, `esp_alloc::psram_allocator!`) for `ESP32` (`xtensa-esp32-none-elf`), how much PSRAM is mapped (`2 MB` vs `4 MB` via `psram`), and which buffers can be moved to `Box`/`Vec` in PSRAM vs which must remain in internal DMA SRAM (`tx_buffer` for `I2S0` DMA).
2. **`esp-hal 1.0.0` Multi-Core (`APP_CPU` Core 1) Exact API**:
   - Inspect `~/.cargo/registry/src/*/esp-hal-1.0.0/src/system.rs` (or `cpu_control`) to verify the exact API to start Core 1 (`APP_CPU`) on ESP32 and how to pin the real-time audio decode + DSP + `I2S0` DMA loop to Core 1 while Core 0 handles `esp-radio` Wi-Fi/BT + `smoltcp` TCP/IP + HTTP + SlimProto.
3. **Unused / Disabled Workspace Features on `xtensa`**:
   - **`squeezelite-decoders`**: `FlacDecoder` (`claxon`), `AacDecoder` (`syom`), `VorbisDecoder` (`lewton`), and `Mp3Decoder` (`Box<nanomp3_core::Decoder>`) are all gated with `#[cfg(not(target_arch = "xtensa"))]` in `pipeline.rs`! Verify how enabling `alloc` + 4 MB PSRAM allows enabling **all 5 codecs (`PCM/WAV/AIFF`, `MP3`, `FLAC`, `AAC`, `Ogg Vorbis`)** natively on the ESP32 (`HELO` advertising `,pcm,wav,aif,flc,mp3,aac,ogg`).
   - **`squeezelite-audio-core`**: 10-band biquad `Equalizer<10>` and `Horner` loudness compensation curve — currently left at flat 0 dB with no Web UI or SlimProto/NVS controls!
   - **`squeezelite-config`**: `FlashJournal` (atomic dual-sector CRC32 NVS persistence at `0x140000` `settings` partition), `AudioConfig`, `WifiCredentials` — currently unused on `xtensa`!
   - **`esp-radio 0.17.0`**: Check `WifiController` power-save settings (`PowerSaveMode::None` to eliminate DTIM jitter), mDNS / hostname advertisement on `smoltcp`, and `ble` / `bt` VHCI controller coexist capabilities once PSRAM frees internal DRAM.

## 3. Deliverable
Return a structured Markdown report with exact file/line citations and exact `esp-hal 1.0.0` / `esp-alloc 0.9.0` / `esp-radio 0.17.0` code patterns for every unused capability so we can synthesize a comprehensive optimization & maximization plan.
