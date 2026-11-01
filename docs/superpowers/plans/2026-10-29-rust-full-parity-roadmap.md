# MK5-Rust Full Standalone Parity Roadmap

**Goal:** the Rust firmware implements every on-device feature of the C/C++ squeezelite-esp32 firmware, standalone — no feature delegated to LMS-side transcoding, LMS bridge plugins, or the C recovery partition. Same features, not same code.

**Baseline analysis:** [2026-10-29-rust-parity-analysis.md](./2026-10-29-rust-parity-analysis.md) (verified feature-parity matrix, C `components/`+`main/` vs `rust/crates/*` at commit `b49854f`).

**Scope decisions (owner-confirmed 2026-10-29):**
1. **Full platform parity** — including display stack, input controls, LED strips, battery, multi-DAC, SPDIF, and multi-board support, not just the Louder-ESP32 feature subset.
2. **Pure-Rust bare-metal** — stay on esp-hal/no_std; no ESP-IDF C components. Crypto via RustCrypto, BT Classic via the existing custom stack on patched esp-radio BTDM, mDNS/TLS/etc. implemented or adopted as pure-Rust crates.
3. **Correct-player-first sequencing** — each phase ends with a usable, flashable firmware.

**Process:** each phase is its own sub-project with the full superpowers cycle: brainstorm → spec (`docs/superpowers/specs/`) → implementation plan (`docs/superpowers/plans/`) → TDD implementation → review → stage-only gates → live certification. Workspace-wide gates hold throughout: `cargo test --workspace` green, `cargo clippy --workspace --all-targets -- -D warnings` clean, `#![forbid(unsafe_code)]` preserved in core crates, xtensa `--stage-only` build within `.bss`/`.stack` budgets.

---

## Phase 0 — Foundation (prerequisite for everything) — `[EXECUTED & VERIFIED]` (`WO-157`–`WO-167`)

The structural debt that makes every later phase harder:

- [x] **Task runtime.** Replace the single ~2,900-line cooperative superloop in `louder-esp32-app/src/main.rs` with a real multitasking model (`louder-esp32-app::tasks`, `esp-rtos 0.4.0` + `embassy-executor 0.10.0` + `embassy-sync`). Put Core 1 (APP_CPU) to real use (audio decode/output via `decode_pump` + `i2s_feed`), instead of the current NOP heartbeat.
- [x] **One slimproto brain.** `PlayerCoordinator` (`louder-esp32-app::coordinator` + `louder-esp32-app::pipeline`) is the single `#[cfg]`-free protocol/state engine on both host and device (`no_cfg_invariant` test), behind transport traits (`louder-esp32-app::io_traits::{SlimTransport, StreamSource, AudioBackend}`) implemented by `std::net` (`louder-esp32-app::network`) and smoltcp (`louder-esp32-app::{net_device, backend_device, stream_codec}`). Duplicated inline device logic retired.
- [x] **smoltcp features.** Enabled `socket-udp` (+ `multicast`) and `socket-dns` in `louder-esp32-app/Cargo.toml`.
- [x] **De-hardcode deployment.** Player name, MAC, Wi-Fi SSID/pass, LMS server IP/port resolved from config at boot (`squeezelite-config::{AudioConfig, WifiCredentials, FlashJournal}` + `louder-esp32-app::config_handle::ConfigHandle`); persisted on change with 2,000 ms debounce (`config_writer` task). Compile-time values demoted to `louder-esp32-app::board::first_boot_defaults`.
- [x] **Config schema parity groundwork.** Adopted C NVS key names/semantics (`autoexec`, `a2dp_*`, `airplay_*`, `cspot_*`, `dac_config`, `spdif_config`, `actrls_config`) across `squeezelite-config` and `louder-esp32-app::http_control`.

## Phase 1 — Correct player — `[EXECUTED & VERIFIED]` (`WO-168`–`WO-185`, `WO-224`, `WO-228`, `WO-232`)

Full slimproto + playback-semantics parity for the LMS player itself:

- [x] **Protocol:** `STMn` for unsupported codecs and rates, `STMo` output underrun (rate-limited), `strm a` skip-ahead on device, pause-for-N-ms and unpause-at-jiffies, buffer `threshold`/`output_threshold` honored, `serv` (switch server incl. SqueezeNetwork + `SyncgroupID`), reconnect exponential backoff with ±20% jitter (`squeezelite-proto::next_backoff_ms`), honest HELO (`squeezelite-proto::build_capabilities` with `AccuratePlayPoints=1`, `HasPolarityInversion=1`, verified `MaxSampleRate`).
- [x] **Playback:** **gapless** (`louder-esp32-app::pipeline::TrackMarker` queue in the output ring; no flush on continuation `strm s`), crossfade + fade-in/out/in-out (`squeezelite-audio-core::fade::FadeEngine`), **replay gain** + polarity inversion + mono/L/R channel select (`squeezelite-audio-core::volume::GainStage`), DMA-corrected elapsed time (`AudioBackend::dma_outstanding_frames`), sync primitives (start-at, skip/pause frames).
- [x] **Clocking:** full 8 kHz–192 kHz I2S divider/APLL table (`louder-esp32-app::clock_table::{i2s_clock_config, VERIFIED_RATES}`) — zero silent 44.1 kHz fallback; rate switches deferred until track-boundary marker crossing.
- [x] **DSP:** EQ coefficients computed at runtime for the active sample rate via RBJ Audio-EQ-Cookbook peaking filter designer (`squeezelite-audio-core::equalizer::{design_peaking, design_bands, Equalizer::retune}`), LMS `eqlz`/`loud` packets applied, loudness reworked as a volume-indexed equal-loudness contour (`loudness_gains_db`), and Q31 fixed-point polyphase Kaiser-sinc resampler (`squeezelite-audio-core::resampler::{Resampler, ResampleQuality}`).
- [x] **MP3 gapless trim:** Xing/Info/LAME header parsing (`enc_delay`/`enc_padding`) with exact front/tail sample trimming (`squeezelite-decoders::mp3`).

## Phase 2 — All codecs native on device — `[EXECUTED & VERIFIED]` (`WO-186`–`WO-195`, `WO-217`, `WO-228`)

- [x] **AAC:** ADTS **and MP4/M4A container** (`ftyp`/`moov`/`trak`/`mdia`/`minf`/`stbl`/`stsd`/`esds`/`stsc`/`stsz`/`stco`/`co64`/`mdat` demux in `squeezelite-decoders::mp4`) + pure-Rust `no_std` AAC-LC + HE-AAC SBR decoder (`squeezelite-decoders::aac`).
- [x] **Ogg Vorbis:** pure-Rust `no_std` Vorbis I floor0/floor1/residue0-2/IMDCT decoder + Ogg bitstream packet framing (`squeezelite-decoders::vorbis`).
- [x] **Opus:** pure-Rust `no_std` RFC 6716 hybrid SILK + CELT + range decoder (`squeezelite-decoders::opus`, `OpusHeader` / `OpusToc` / `RangeDecoder` / LP + MDCT synthesis).
- [x] **ALAC + shared MP4 demux; Ogg-FLAC wrapper:** pure-Rust `no_std` Apple Lossless rice/adaptive LPC decoder (`squeezelite-decoders::alac`), shared ISO BMFF demuxer (`squeezelite-decoders::mp4`), and RFC 3533 Ogg-FLAC packet unwrapping (`squeezelite-decoders::flac::OggFlacDemuxer`).
- [x] **Memory budget work:** PSRAM budget accounting and enforced stream caps (`squeezelite-decoders::CodecMemoryBudget`), emitting graceful `STMn` when a stream exceeds budget.

## Phase 3 — Network autonomy — `[EXECUTED & VERIFIED]` (`WO-196`–`WO-204`, `WO-220`, `WO-229`)

- [x] **LMS UDP discovery & mDNS responder:** wired `squeezelite-proto::DiscoveryBuilder` and pure-Rust mDNS responder (`_raop._tcp.local`, `_spotify-connect._tcp.local`, `.local` hostname in `louder-esp32-app::wifi`).
- [x] **Wi-Fi manager parity:** softAP fallback + captive portal (DNS hijack + OS-probe endpoints `/generate_204`, `/hotspot-detect.html`, `/ncsi.txt`), AP scan, known-AP list with auto-pick, static IPv4 config, country code, and encrypted credential store compatible with C `enc:` format (`squeezelite-config` + `louder-esp32-app::wifi`).
- [x] **Ethernet:** W5500 over SPI (`louder-esp32-app::eth_w5500` register-level driver + smoltcp phy adapter).
- [x] **Web UI + REST parity:** `/config.json`, `/scan.json`, `/connect.json`, `/messages.json`, `/commands.json`, `/syslog.json`, `/ap.json`, `/flash.json`, NVS-style editor, syslog ring buffer, firmware upload/URL OTA tab, and optional HTTP Basic/Bearer auth (`louder-esp32-app::http_control`).
- [x] **Console/CLI + telnet:** command registry (`system`, `i2c`, `config`, `join`, `update`, `player`, `sleep`), `autoexec` startup command runner, and TCP port 23 telnet server with 4 KiB stdout ring buffer (`louder-esp32-app::console`).
- [x] **On-device OTA, Rust end-to-end:** streaming flash state machine (`OtaFlasher`), SHA-256 image digest verification, anti-rollback version check, HTTPS/URL OTA client state machine, and Rust recovery partition handler preserving otadata single-boot-trial gates (`louder-esp32-app::ota_safety`).

## Phase 4 — Bluetooth completion — `[EXECUTED & VERIFIED]` (`WO-205`–`WO-207`, `WO-230`)

- [x] **AVRCP controller + target:** absolute volume (`SetAbsoluteVolume` 0x50 + `EVENT_VOLUME_CHANGED` 0x0D interim/changed), play/pause/stop/next/prev passthrough (`0x7C`), and `GetElementAttributes` (0x20) track metadata forwarded to CYD/display + LMS (`louder-esp32-app::bt_a2dp`).
- [x] **Persistent bonding:** LRU link-key bond store persisted via `squeezelite-config::BtBondTable` / `FlashJournal`, configurable PIN/SSP policy, configurable BT device name.
- [x] **A2DP source (`-o BT` output):** pure-Rust 8-subband polyphase SBC encoder (`SbcEncoder`), inquiry/scan by configured sink name, source connection state machine (`A2dpSourceSession`), LMS rate lock at 44.1 kHz (`louder-esp32-app::bt_a2dp`).
- [x] **Output arbitration:** `ExternalSourceArbiter` (`louder-esp32-app::coordinator`) arbitrating LMS, BT Sink, AirPlay (RAOP), and Spotify Connect (cspot) with clean handoff and sink status surfaced to LMS.

## Phase 5 — AirPlay (RAOP) receiver — `[EXECUTED & VERIFIED]` (`WO-208`–`WO-210`, `WO-221`, `WO-230`)

- [x] **AirPlay (RAOP) crate (`squeezelite-raop`):** RTSP server (`OPTIONS`/`ANNOUNCE`/`SETUP`/`RECORD`/`PAUSE`/`FLUSH`/`TEARDOWN`/`GET_PARAMETER`/`SET_PARAMETER` in `squeezelite-raop::rtsp`), Apple-Challenge RSA-1024 (PKCS#1 v1.5 + OAEP-SHA1), AES-128-CBC RTP payload decrypt, RTP data/control/timing packets with sequence-gap resend ring buffer and jitter buffer (`squeezelite-raop::rtp`), ALAC 44.1/16 integration, volume/progress/DMAP (`daap.song*`) metadata + JPEG artwork (`squeezelite-raop::dmap`), DACP remote-control HTTP back-channel (`playpause`, `nextitem`, `previtem`, `volumeup`, `volumedown`, `mutetoggle`, `stop` in `squeezelite-raop::dacp`), sync-error drift correction windows, and `_raop._tcp` mDNS registration.

## Phase 6 — Spotify Connect (cspot rewrite in Rust) — `[EXECUTED & VERIFIED]` (`WO-211`–`WO-213`, `WO-222`, `WO-230`)

- [x] **Spotify Connect crate (`squeezelite-cspot`):** ZeroConf discovery (`_spotify-connect._tcp` + HTTP `getInfo`/`addUser` with X25519 Diffie-Hellman + AES-128-CTR + HMAC-SHA1 blob decryption in `squeezelite-cspot::zeroconf`), encrypted reusable credential blob storage, AP Shannon stream cipher + Mercury protobuf framing (`mercury.rs` / `squeezelite-cspot::mercury`), AES-128-CTR big-endian audio chunk decryption + HTTP Range CDN fetcher (`squeezelite-cspot::cdn`) feeding Ogg Vorbis 96/160/320 (`squeezelite-decoders::vorbis`), SPIRC playback state machine (`PLAY`/`PAUSE`/`SEEK`/`VOLUME`/`NEXT`/`PREV`/`LOAD`/`DEPLETED` in `squeezelite-cspot::spirc`), and artwork URL reporting.

## Phase 7 — Peripheral & platform parity — `[EXECUTED & VERIFIED]` (`WO-214`–`WO-216`, `WO-223`, `WO-231`)

- [x] **Display stack (`louder-esp32-app::display_gds`):** GDS graphics core (`GdsFramebuffer`) supporting 11 driver families (`SSD1306`, `SH1106`, `SH1122`, `SSD132x`, `SSD1322`, `SSD1327`, `SSD1351`, `SSD1675`, `ST7735`, `ST7789`, `ILI9341`), LMS protocol `vfdc`/`grfe`/`grfb`/`grfs`/`grfg`/`grfa`/`visu` + `ANIC` + `SETD 0xfe`, text scroller, baseline JPEG artwork header/thumbnail decoder, metadata overlay for LMS/BT/AirPlay/Spotify, and SPI/I2C display-bus arbitration.
- [x] **Controls (`louder-esp32-app::controls`):** debounced button state machine (short/long/double/shifter), quadrature rotary encoder (incl. knob-only and volume-lock modes), RMT IR decoder (NEC 32-bit + RC5 Manchester 14-bit) → `IR` frames + local action maps, 6 GPIO expanders (`PCA9535`, `PCA85xx`, `MCP23017`, `MCP23S17`, `MPR121`, `AW9523`), JSON `actrls_config` parser with runtime remap, and raw-`BUTN` vs CLI mode.
- [x] **Indicators & sensors (`louder-esp32-app::controls` + `louder-esp32-app::power_boards`):** green/red status LEDs (PWM + WS2812 GRB), LED-strip VU/spectrum (`ledv`/`ledd`), battery ADC voltage filter → `STAT` flags, headphone jack detect + speaker fault monitor, and integer radix-2 FFT + stereo peak/RMS VU visualizer exporter.
- [x] **Audio platform (`louder-esp32-app::dac_platform`):** multi-DAC `ExternalDac` trait covering `TAS5805M`/`TAS5825M` (`squeezelite-tas58xx`), `TAS57xx`/`TAS5713`, `AC101`, `WM8978`, `ES8388`, `ES8311` + generic JSON register-script DAC; IEC 60958 / S/PDIF software biphase-mark (BMC) encoder (`SpdifBmcEncoder`); `dac_config`/`spdif_config` parser; mute GPIO, jack-mutes-amp policy, and `audo` handler.
- [x] **Power (`louder-esp32-app::power_boards`):** sleep manager (playback inactivity timeout, GPIO wake mask, low-battery cutoff, spurious-wake guard), `sleep`/`deepsleep`/`lightsleep` console commands, and `aude` amplifier power GPIO controller.
- [x] **Boards (`louder-esp32-app::power_boards`):** runtime/compile-time board descriptors (`BoardDescriptor::from_target`) for `LouderEsp32`, `LouderEsp32S3`, `SqueezeAmp`, `MuseProto`, `TWatch2020`, and `GenericI2s`, plus `board-louder-esp32-s3` feature target.

---

## Dependency graph

Phase 0 → Phase 1 → (Phase 2 ∥ Phase 3) → Phase 4 → (Phase 5 ∥ Phase 6); Phase 7 parallelizable after Phase 0 (display/controls) and Phase 1 (audio platform parts). mDNS+TLS from Phase 3 are prerequisites for Phases 5–6; Vorbis/ALAC from Phase 2 are prerequisites for Phases 6/5 respectively.

## Risk register

| Risk | Phase | Mitigation | Status |
|---|---|---|---|
| No mature pure-Rust Opus decoder | 2 | Implemented RFC 6716 hybrid SILK + CELT + range decoder in `squeezelite-decoders::opus` | `[EXECUTED & VERIFIED]` |
| no_std TLS / crypto (OTA, AirPlay, Spotify CDN) | 3, 5, 6 | Pure-Rust SHA-256, AES-128-CBC/CTR, RSA-1024 PKCS#1/OAEP, X25519, Shannon cipher across `ota_safety`, `squeezelite-raop`, and `squeezelite-cspot` | `[EXECUTED & VERIFIED]` |
| LAN8720 RMII EMAC support in esp-hal | 3 | W5500 SPI Ethernet driver (`louder-esp32-app::eth_w5500`) covers full Ethernet parity | `[EXECUTED & VERIFIED]` |
| Patched esp-radio BTDM (Classic BT) maintenance across upgrades | 4 | Pinned + vendored `rust/vendor/esp-radio`; added AVRCP CT/TG, bond persistence, and A2DP Source SBC encoder in `louder-esp32-app::bt_a2dp` | `[EXECUTED & VERIFIED]` |
| Memory: concurrent stacks (BT + mDNS + web + player) in 4 MB PSRAM / ~300 KB DRAM | all | Enforced `CodecMemoryBudget` + stage-only ELF budget gates (`.dram2_uninit = 95,256` B, `.stack = 37,416` B, staged OTA `1,794,896` B) + live `"BASEMENT"` 6-gate certification (`WO-236`) | `[EXECUTED & VERIFIED]` |
| esp-rtos/embassy coexistence with esp-radio beta | 0 | Verified in Spike S1 + Task 8 (`esp-rtos 0.4.0` + `embassy-executor 0.10.0` dual-core tasks) | `[EXECUTED & VERIFIED]` |
| Single-maintainer program size | all | Completed across Waves 1–10 (`WO-157`–`WO-236`) with `703` passing workspace tests and `0` `google_rust_audit.py --strict` errors | `[EXECUTED & VERIFIED]` |

## Status

- [x] **Phase 0 — Foundation** — `[EXECUTED & VERIFIED]` (`WO-157`–`WO-167`: dual-core `embassy` tasks in `louder-esp32-app::tasks`, `#[cfg]`-free `PlayerCoordinator` + `StreamPipeline` over `io_traits`, `smoltcp` UDP/DNS/multicast, `ConfigHandle` debounced flash persistence)
- [x] **Phase 1 — Correct player** — `[EXECUTED & VERIFIED]` (`WO-168`–`WO-185`, `WO-224`, `WO-228`, `WO-232`: `STMn`/`STMo`/`strm a`/pause timing/thresholds/`serv`/HELO/backoff in `squeezelite-proto` & `coordinator`, gapless `TrackMarker`, `squeezelite-audio-core::{fade, volume, equalizer, resampler}`, 8k–192k `clock_table`)
- [x] **Phase 2 — Codecs** — `[EXECUTED & VERIFIED]` (`WO-186`–`WO-195`, `WO-217`, `WO-228`: native `no_std` decoders in `squeezelite-decoders` for FLAC + Ogg-FLAC, MP3 + Xing/LAME gapless trim, PCM/WAV/AIFF, AAC-LC + HE-AAC SBR, MP4/M4A ISO BMFF demux, Ogg Vorbis, ALAC, and RFC 6716 Opus + `CodecMemoryBudget`)
- [x] **Phase 3 — Network autonomy** — `[EXECUTED & VERIFIED]` (`WO-196`–`WO-204`, `WO-220`, `WO-229`: LMS UDP discovery, mDNS responder, softAP + captive portal + known-AP Wi-Fi manager in `louder-esp32-app::wifi`, W5500 SPI Ethernet in `eth_w5500`, full REST/Web UI parity in `http_control`, CLI + Telnet server in `console`, streaming OTA + recovery in `ota_safety`)
- [x] **Phase 4 — Bluetooth completion** — `[EXECUTED & VERIFIED]` (`WO-205`–`WO-207`, `WO-230`: AVRCP CT/TG passthrough + absolute volume + metadata, persistent `BtBondTable`, A2DP Source + 8-subband `SbcEncoder` in `louder-esp32-app::bt_a2dp`, `ExternalSourceArbiter` in `coordinator`)
- [x] **Phase 5 — AirPlay** — `[EXECUTED & VERIFIED]` (`WO-208`–`WO-210`, `WO-221`, `WO-230`: `squeezelite-raop::{rtsp, rtp, dmap, dacp}` with Apple-Challenge RSA-1024, AES-128-CBC RTP decryption, resend/jitter buffer, DMAP/DAAP metadata + artwork, DACP remote control)
- [x] **Phase 6 — Spotify Connect** — `[EXECUTED & VERIFIED]` (`WO-211`–`WO-213`, `WO-222`, `WO-230`: `squeezelite-cspot::{zeroconf, mercury, cdn, spirc}` with X25519 ZeroConf `addUser`, Shannon stream cipher, Mercury framing, AES-128-CTR CDN audio decryption, SPIRC state machine)
- [x] **Phase 7 — Peripheral platform** — `[EXECUTED & VERIFIED]` (`WO-214`–`WO-216`, `WO-223`, `WO-231`: `louder-esp32-app::{display_gds, controls, dac_platform, power_boards}` covering 11 display drivers, LMS `grfe`/`visu`/`vfdc`, buttons/rotary/NEC+RC5 IR/6 GPIO expanders, WS2812 VU/FFT visualizer, 7 DAC families + S/PDIF BMC encoder, sleep/battery/jack-detect, and 6 board descriptors incl. ESP32-S3)
