# Work Order `WO-056`: `/deep-code-audit` Micro-Auditor 4 — Core Codecs (FLAC/AAC/Vorbis on Xtensa), Web UI/REST API, NVS Config, & Hardware Peripherals Feature-Parity Audit

- **ID**: `WO-056`
- **Timestamp**: `2026-10-24T16:11:30-04:00`
- **Subagent Role**: Core Codecs, Web UI & Peripherals Feature-Parity Auditor (`TypeName: "research"`, `Model: "pro"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Scope
Conduct a zero-tolerance `/deep-code-audit` of all remaining functional gaps between C `squeezelite-esp32` (`components/`, `main/`) and the pure-Rust `rust/crates/*` firmware running on `172.22.1.15`:
1. **On-Device Audio Decoders (`rust/crates/squeezelite-decoders/` vs `rust/crates/louder-esp32-app/src/pipeline.rs`)**:
   - In `squeezelite-decoders`, pure-Rust `FlacDecoder` (`claxon`), `AacDecoder`, and `VorbisDecoder` exist and pass host tests, but in `louder-esp32-app/src/pipeline.rs` (`#[cfg(not(target_arch = "xtensa"))]`), `Flac`, `Aac`, and `Vorbis` are currently gated off on `xtensa` (only `PcmDecoder` for `pcm`/`wav`/`aif` and `StaticMp3Decoder` for `mp3` are enabled on `xtensa`). Audit their memory/heap footprint and what is needed to enable native `flac`, `aac`, and `ogg` on `xtensa` (or with PSRAM on ESP32-WROVER / ESP32-S3).
2. **Web UI & HTTP Control Server (`components/wifi-manager/` vs `rust/crates/louder-esp32-app/src/http_control.rs`)**:
   - Compare the C HTTP endpoints (`/status.json`, `/recovery.json`, `/reboot_ota.json`, `/reboot.json`, `/flash.json`, `/config.json`, `/ap.json`, `/connect.json`, Web UI HTML/JS) against `http_control.rs` (`GET /status.json`, `POST /recovery.json`, `POST /reboot_ota.json`, `POST /reboot.json`). Note: can `ota_0` also accept direct `POST /flash.json` or does `POST /recovery.json` -> `POST /flash.json` handle OTA?
3. **SlimProto Protocol Completeness (`rust/crates/squeezelite-proto/` & `louder-esp32-app/src/main.rs`)**:
   - Audit SlimProto opcodes (`strm` `'q'`/`'p'`/`'u'`/`'s'`/`'t'`/`'f'`, `audg` software+hardware volume & mute, `setd`, `aude`, `cont`, `codc`, `vers`, UDP discovery vs static LMS IP).
4. **Hardware Peripherals & Board Support (`rust/crates/louder-esp32-app/src/board.rs`, `amp.rs`, `cyd.rs`)**:
   - Audit TAS5805M DSP EQ/biquad/DRC registers, `ESP32-WROVER` PSRAM availability on `172.22.1.15` (does `"BASEMENT"` have PSRAM or is it a non-PSRAM ESP32-D0WDQ6/WROOM?), RGB LED (`WS2812`), Ethernet (`W5500`), and CYD UART bridge.

## 2. Mandatory Constraints
- **Read-Only Audit**: Do NOT modify any repository files.
- **NEVER touch `/dev/ttyUSB0`** (Rule 3).

## 3. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `49c195b9-c21d-4d32-9b71-774e04277e36` (`Core Codecs, Web UI & Peripherals Auditor`)
- **Key Findings (`FA-CORE-001`..`FA-CORE-013`)**:
  - `FA-CORE-001`..`003`: Native `FlacDecoder` (`claxon`), `AacDecoder` (`syom`), and `VorbisDecoder` (`lewton`) exist in `squeezelite-decoders` and pass host tests, but are `#[cfg(not(target_arch = "xtensa"))]` in `pipeline.rs` because `claxon`/`lewton` use `std::io::Read` and allocate `100-150 KB` heap buffers (requiring PSRAM on ESP32).
  - `FA-CORE-004`..`006`: `http_control.rs` implements `GET /status.json`, `POST /recovery.json`, `POST /reboot.json`, `POST /reboot_ota.json`, while missing the full Web UI (`webapp/dist/`) and NVS config/scan/console REST endpoints (`/config.json`, `/scan.json`, `/connect.json`, `/messages.json`, `/commands.json`). Direct `POST /flash.json` from `ota_0` is intentionally omitted (even in C) because erasing `ota_0` while executing XIP from `ota_0` crashes the instruction cache.
  - `FA-CORE-007`..`009`: SlimProto on `xtensa` uses static LMS IP `10.0.0.10:3483` (UDP port `3483` auto-discovery exists in `squeezelite-proto` `DiscoveryBuilder`/`DiscoveryParser` but is not yet wired in `main.rs`), software volume `audg` is not mirrored to TAS5805M `0x4C` hardware register, and `vers`/`serv`/`strm 'a'` are unimplemented.
  - `FA-CORE-010`..`013`: `172.22.1.15` (`ESP32-WROVER`) has `4 MB` external PSRAM (`SPIRAM`), which is currently uninitialized in `esp-alloc`, restricting `ring_storage` to `16 KB` `.bss` (`~92 ms` vs `2.5 MB` in C). TAS5805M biquad/DRC registers, `WS2812` RGB LED, W5500 Ethernet, and extended NVS keys (`dac_config`, `autoexec`, `lms_ip`) are not yet wired in `louder-esp32-app`.
