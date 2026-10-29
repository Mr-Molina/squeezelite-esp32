# MK5-Rust Full Standalone Parity Roadmap

**Goal:** the Rust firmware implements every on-device feature of the C/C++ squeezelite-esp32 firmware, standalone — no feature delegated to LMS-side transcoding, LMS bridge plugins, or the C recovery partition. Same features, not same code.

**Baseline analysis:** [2026-10-29-rust-parity-analysis.md](./2026-10-29-rust-parity-analysis.md) (verified feature-parity matrix, C `components/`+`main/` vs `rust/crates/*` at commit `b49854f`).

**Scope decisions (owner-confirmed 2026-10-29):**
1. **Full platform parity** — including display stack, input controls, LED strips, battery, multi-DAC, SPDIF, and multi-board support, not just the Louder-ESP32 feature subset.
2. **Pure-Rust bare-metal** — stay on esp-hal/no_std; no ESP-IDF C components. Crypto via RustCrypto, BT Classic via the existing custom stack on patched esp-radio BTDM, mDNS/TLS/etc. implemented or adopted as pure-Rust crates.
3. **Correct-player-first sequencing** — each phase ends with a usable, flashable firmware.

**Process:** each phase is its own sub-project with the full superpowers cycle: brainstorm → spec (`docs/superpowers/specs/`) → implementation plan (`docs/superpowers/plans/`) → TDD implementation → review → stage-only gates → live certification. Workspace-wide gates hold throughout: `cargo test --workspace` green, `cargo clippy --workspace --all-targets -- -D warnings` clean, `#![forbid(unsafe_code)]` preserved in core crates, xtensa `--stage-only` build within `.bss`/`.stack` budgets.

---

## Phase 0 — Foundation (prerequisite for everything)

The structural debt that makes every later phase harder:

- **Task runtime.** Replace the single ~2,900-line cooperative superloop in `louder-esp32-app/src/main.rs` with a real multitasking model (decision in Phase 0 spec: embassy async executors vs. esp-rtos threads vs. structured cooperative scheduler). Put Core 1 (APP_CPU) to real use (audio decode/output), instead of the current NOP heartbeat.
- **One slimproto brain.** `PlayerCoordinator` (currently host-only) becomes the single protocol/state engine on both host and device, behind transport traits implemented by `std::net` (host/tests) and smoltcp (device). Retire the duplicated inline device logic in `run_hardware_firmware()`. Tests must exercise the code that ships.
- **smoltcp features.** Enable `socket-udp` (+ multicast) and `socket-dns`. Prerequisite for discovery, mDNS, captive portal, NTP-style timing.
- **De-hardcode deployment.** Player name, MAC, Wi-Fi SSID/pass, LMS server IP/port come from config at boot (`AudioConfig`/`WifiCredentials` already exist — wire them); persist on change (debounced), not only via HTTP reboot endpoints. Compile-time values remain only as first-boot defaults.
- **Config schema parity groundwork.** Adopt C NVS key names/semantics where they map (`autoexec`-equivalent startup options, `a2dp_*`, `airplay_*`, `cspot_*`, `dac_config`-style strings) so later phases and the web UI have a stable store.

## Phase 1 — Correct player

Full slimproto + playback-semantics parity for the LMS player itself:

- Protocol: `STMn` for unsupported codecs (replace mute-and-pretend), `STMo`, `strm a` skip-ahead on device, pause-for-N-ms and unpause-at-jiffies, buffer `threshold`/`output_threshold` honored, `serv` (switch server incl. SqueezeNetwork + SyncgroupID), reconnect backoff parity, honest HELO (drop `HasDigitalOut` until SPDIF exists; correct MaxSampleRate; add `HasPolarityInversion`, `Balance` once true).
- Playback: **gapless** (track-start boundaries in the output ring; stop flushing on every `strm s`), crossfade + fade-in/out/in-out, **replay gain** (current/next track, applied), polarity inversion, mono/L/R channel select, DMA-corrected elapsed time (`device_frames` equivalent), sync primitives (start-at, skip/pause frames).
- Clocking: full 8 kHz–192 kHz I2S divider/APLL table — no silent 44.1 kHz fallback.
- DSP: EQ coefficients computed at runtime for the active sample rate (RBJ), LMS `eqlz`/`loud` packets applied, loudness reworked as a volume-indexed equal-loudness contour, resample16-equivalent polyphase resampler (`-r`/`-R` semantics).
- MP3 gapless trim (LAME/Xing tag) moves here if not done with Phase 2.

## Phase 2 — All codecs native on device

- AAC: ADTS **and MP4/M4A container** (esds/stsc), HE-AAC SBR (port/extend `syom` to no_std+PSRAM or new decoder).
- Ogg Vorbis: no_std decoder (port lewton or Tremor-style rewrite).
- **Opus:** no production pure-Rust decoder exists — this is an RFC 6716 implementation sub-project (SILK+CELT). Explicit spike before committing to schedule.
- ALAC + shared MP4 demux; Ogg-FLAC wrapper for the native FLAC decoder.
- Memory budget work: all decoders PSRAM-resident, enforced caps, graceful `STMn` when a stream exceeds budget.

## Phase 3 — Network autonomy

- LMS **UDP discovery** (builders already exist in `squeezelite-proto` — wire them), **mDNS responder** (hostname + later `_raop._tcp`, `_spotify-connect._tcp`).
- **Wi-Fi manager parity:** softAP fallback + captive portal (DNS hijack + OS-probe endpoints), scan, known-AP list with auto-pick, static IP, country code, encrypted credential store (own scheme; keep read-compat with C `enc:` format).
- Ethernet: W5500 over SPI (smoltcp-native). LAN8720 RMII requires esp-hal EMAC support — spike; may be descoped with justification.
- **Web UI + REST parity:** `/config.json`, `/scan.json`, `/connect.json`, `/messages.json`, `/commands.json`, NVS-style editor, syslog view, firmware tab; config forms for audio/hardware/system; auth consideration (C has none — decide deliberately).
- **Console/CLI + telnet:** command registry (system, i2c tools, config get/set/erase, join, update, player control), `autoexec`-style startup commands, telnet server with stdout ring buffer.
- **On-device OTA, Rust end-to-end:** HTTPS client (no_std TLS — spike: embedded-tls/rustls-no_std), streaming flash of `ota_0`, **Rust recovery app** in the factory partition replacing the C recovery (upload + URL OTA + LMS proxy discovery), crash counters → guided recovery, preserving the current otadata single-boot-trial gates.

## Phase 4 — Bluetooth completion

- **AVRCP** controller + target for the sink: absolute volume (both directions), play/pause/next/prev passthrough, track metadata (→ CYD/display + LMS idle display).
- **Persistent bonding** (link keys in FlashJournal), configurable PIN/SSP policy, configurable device name.
- **A2DP source** (`-o BT` output): SBC *encoder*, inquiry/scan by configured sink name, connection state machine, LMS rate lock at 44.1 kHz.
- Output arbitration parity with `decode_external.c`: BT-sink/AirPlay/cspot take over the output path and return it to LMS cleanly; sink status surfaced to LMS.

## Phase 5 — AirPlay (RAOP) receiver

- RTSP server (OPTIONS/ANNOUNCE/SETUP/RECORD/PAUSE/FLUSH/TEARDOWN/GET_PARAMETER/SET_PARAMETER), Apple-Challenge RSA-1024 (PKCS#1 v1.5 + OAEP-SHA1), AES-128-CBC RTP payload decrypt, RTP data/control/timing sockets with resend requests and jitter buffer, ALAC 44.1/16 (from Phase 2), volume/progress/DMAP metadata + JPEG artwork, DACP remote-control back-channel, sync-error correction windows, `_raop._tcp` mDNS registration.

## Phase 6 — Spotify Connect (cspot rewrite in Rust)

- ZeroConf discovery (`_spotify-connect._tcp` + HTTP `addUser` endpoint), credential flow with encrypted reusable credentials, AP connection (Shannon cipher, Mercury protobuf), AES-CTR audio key + Ogg Vorbis 96/160/320 (decoder from Phase 2), playback control/volume/seek/track events, artwork download, TLS for API/CDN fetches (shared with Phase 3 TLS). Largest single phase; expect sub-decomposition at spec time.

## Phase 7 — Peripheral & platform parity

- **Display stack:** GDS-equivalent graphics core over embedded-graphics; drivers SSD1306/SH1106/SH1122/SSD132x/SSD1322/SSD1327/SSD1351/SSD1675/ST7735/ST7789/ILI9341 (adopt existing Rust driver crates where possible); LMS protocol `vfdc`/`grfe`/`grfb`/`grfs`/`grfg`/`grfa`/`visu` + `ANIC` + `SETD 0xfe`; scroller; JPEG artwork decode; metadata displayer for BT/AirPlay/Spotify; display-bus arbitration.
- **Controls:** buttons (debounce/long-press/shifter), rotary encoders (incl. knob-only and volume-lock modes), IR receiver (RMT, NEC + RC5) → `IR` frames + local maps, GPIO expanders (PCA9535/PCA85xx/MCP23017/MCP23S17/MPR121/AW9523), JSON `actrls_config` action mapping with remap, raw-`BUTN` vs CLI mode.
- **Indicators & sensors:** green/red status LEDs (PWM + WS2812), LED-strip VU/spectrum (`ledv`/`ledd`), battery ADC → STAT, jack detect + speaker fault, visualizer export (VU/FFT via esp-dsp-equivalent).
- **Audio platform:** multi-DAC trait (adac parity): TAS57xx/TAS5713/AC101/WM8978/ES8388/ES8311 + generic JSON control-set DAC; SPDIF software BMC encoder; `dac_config`/`spdif_config` string compatibility; mute GPIO / jack-mutes-amp / `audo`.
- **Power:** sleep config (inactivity, GPIO sleep/wake, battery threshold, spurious-wake), deep/light sleep console commands, `aude` power GPIO.
- **Boards:** SqueezeAMP, Muse, T-WATCH2020 targets; ESP32-S3 support (the `board-louder-esp32-s3` feature becomes real).

---

## Dependency graph

Phase 0 → Phase 1 → (Phase 2 ∥ Phase 3) → Phase 4 → (Phase 5 ∥ Phase 6); Phase 7 parallelizable after Phase 0 (display/controls) and Phase 1 (audio platform parts). mDNS+TLS from Phase 3 are prerequisites for Phases 5–6; Vorbis/ALAC from Phase 2 are prerequisites for Phases 6/5 respectively.

## Risk register

| Risk | Phase | Mitigation |
|---|---|---|
| No mature pure-Rust Opus decoder | 2 | Dedicated spike; schedule as own sub-project; interim: omit `ops` from HELO (only remaining transcode fallback, explicitly temporary) |
| no_std TLS (OTA, Spotify CDN): embedded-tls is TLS 1.3-only; rustls no_std maturity | 3, 6 | Spike early in Phase 3 against real endpoints (GitHub releases, Spotify CDN) |
| LAN8720 RMII EMAC support in esp-hal | 3 | Spike; W5500 covers Ethernet parity if RMII blocked |
| Patched esp-radio BTDM (Classic BT) maintenance across upgrades | 4 | Pin + vendor (already done); document patch; track upstream esp-radio Classic-BT work |
| Memory: concurrent stacks (BT + mDNS + web + player) in 4 MB PSRAM / ~300 KB DRAM | all | Per-phase `.bss`/heap budget gates in stage-only checks; features registered but inactive until used (C model) |
| esp-rtos/embassy coexistence with esp-radio beta | 0 | Decided by Phase 0 spike before spec freeze |
| Single-maintainer program size | all | Phase gates produce flashable firmware; each phase independently valuable |

## Status

- [ ] Phase 0 — Foundation (spec in progress)
- [ ] Phase 1 — Correct player (spec in progress, combined with Phase 0)
- [ ] Phase 2 — Codecs
- [ ] Phase 3 — Network autonomy
- [ ] Phase 4 — Bluetooth completion
- [ ] Phase 5 — AirPlay
- [ ] Phase 6 — Spotify Connect
- [ ] Phase 7 — Peripheral platform
