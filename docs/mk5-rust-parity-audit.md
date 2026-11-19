# MK5-Rust: feature-parity audit and next backlog

Audited: 2026-11-18, branch `MK5-Rust` at `e409596d`.
Audience: the owner, and a coding agent picking up the next round of work.

**Method.** Four read-only passes over the legacy C tree (`components/`, `main/`) and the Rust
workspace (`rust/`), one per feature area, followed by spot-checks of each pass's headline claims
against the code. Nothing was built, flashed or run. Every status below comes from reading source.
Items that could not be settled from source are listed in section 7 and marked UNVERIFIED where they
appear.

**Status vocabulary.**

| Status | Meaning |
|---|---|
| WIRED | Implemented and reachable in the real firmware (xtensa, default features = `board-louder-esp32`). |
| HOST-ONLY | Implemented and tested, but never compiled into or never driven by the firmware. |
| PARTIAL | Some of the behaviour is on device; the row says what is missing. |
| MISSING | No Rust equivalent on device. |
| N/A | Legacy feature that is irrelevant to this target; the row says why. |

---

## 1. Summary

The Rust firmware is a solid LMS player for one board. On the Louder ESP32 it plays FLAC, MP3 and
PCM/WAV/AIFF from LMS with gapless, crossfade, replay gain, a 10-band EQ, native TAS5805M control,
a status/config web UI and the CYD UART link. That path is the one certified on BASEMENT.

Outside that path, much of what the earlier parity documents mark "IMPLEMENTED & VERIFIED" is
host-tested library code that the firmware never runs. The single most important structural fact:

> **The firmware does not run `tasks.rs`.** On xtensa, `main()` calls
> `xtensa_hw::run_hardware_firmware()`, which loops `pump_device_phase()` on core 0
> (`main.rs` ~4659). `tasks::run_core0_executor` has no caller. Modules declared with
> `#[allow(dead_code)]` in `main.rs` — `console`, `controls`, `dac_platform`, `display_gds`,
> `eth_w5500`, `power_boards` — and `wifi::WifiManager`, plus the OTA streaming half of
> `ota_safety.rs`, are not referenced from the device code.

Consequences by area:

| Area | On device today | Not on device |
|---|---|---|
| LMS playback | FLAC, MP3, PCM; gapless; fades; replay gain; EQ; volume; most SlimProto | AAC/Vorbis/ALAC/Opus are compiled in but not advertised; server discovery; resampling; idle amp power-off |
| AirPlay | Discovery, RTSP, RSA/AES, ALAC, volume — probably plays (hardware UNVERIFIED) | Jitter buffer, resend, timing/sync, metadata, artwork, remote control |
| Bluetooth sink | Real BR/EDR controller, pairing, AVDTP, SBC — probably plays (hardware UNVERIFIED) | Bond persistence, PIN config, applying phone volume, metadata, transport control |
| Bluetooth source | — | All of it (test-only) |
| Spotify | Discovery only | Login, session, control, audio fetch; no TLS client exists in the workspace |
| Web UI / REST | Status, player controls, EQ, single-key config edits | Firmware upload (fake), Wi-Fi scan (fake), most console commands (fake), hardware config (stored, ignored) |
| Platform | Wi-Fi STA, DHCP, anti-brick boot gates, watchdog | Provisioning with no credentials, soft-AP, OTA, legacy settings migration, factory reset, console, telnet, Ethernet |
| Peripherals | TAS5805M amp, CYD link (partial) | Displays, LEDs, buttons, rotary, IR, battery, sleep, GPIO expanders, runtime pin config, other DACs, SPDIF, ESP32-S3 |

The Rust firmware today depends on the legacy recovery app for firmware updates, for provisioning a
device that has no stored Wi-Fi credentials, and as the anti-brick fallback target.

---

## 2. Defects and hazards found (not just gaps)

These are behaviours that are wrong or misleading on the certified board today, as read from the
code. Each is small to fix relative to its impact. None has been reproduced on hardware.

| # | Finding | Where | Why it matters |
|---|---|---|---|
| H1 | Settings are written to flash at runtime in the default build. `Cargo.toml` says that with `runtime-flash-persist` off, changes reach flash only on the pre-reboot path. The live loop writes sector `0x3FE000` 2 s after any debounced change (for example an LMS volume change). The feature flag only selects the offset. The "not while audio plays" hazard gate exists only in dead `tasks.rs`. | `main.rs` ~4634-4655; `config_handle.rs` `DEBOUNCE_MS` | `Cargo.toml` records that runtime flash I/O deadlocked this board (WO-109). The documented safety design is not the one running. Needs an owner decision and a soak test. |
| H2 | `POST /flash.json` does not flash anything. It answers "starting_ota", flushes config, re-pins `ota_0` and reboots into the same image. The request body is capped at 1 KB and never parsed. | `main.rs` ~3907; `http_control.rs` ~3006 | The web UI's firmware-upload button appears to work and silently does nothing. |
| H3 | Wi-Fi credentials set from the web UI are not used at the next boot. Boot calls `resolve_boot_wifi` (legacy NVS, then compile-time default), not `resolve_boot_wifi_with_journal`. | `main.rs` ~1116 | A device moved to a new network from the web UI returns to the old SSID after reboot, then trips the lease watchdog. |
| H4 | No stored Wi-Fi credentials means `software_reset()` in a loop, with nothing to configure the device. | `main.rs` ~1120 | Safe only while a legacy recovery app sits in the factory partition. |
| H5 | Source arbitration is inverted relative to legacy. In C, an LMS `strm s` disconnects the external source. In Rust, an active AirPlay/Spotify/Bluetooth source blocks LMS: the `strm` is silently dropped. Rust also preempts by fixed priority (AirPlay > Spotify > BT > LMS) where C was first-come. | `coordinator.rs` ~1904, `SourceArbiter` | A phone left connected can stop LMS from playing, with no feedback. May be intended; it is undocumented. |
| H6 | AirPlay advertises `md=0,1,2` (metadata, artwork, progress), but the RTSP receive buffer is cleared once it passes 4096 bytes. An artwork body will overflow it and the remainder is parsed as new requests. | `main.rs` ~4284 | Likely session corruption when a sender pushes cover art. |
| H7 | The Bluetooth sink claims AVRCP absolute volume, stores the value, and never applies it. | `bt_a2dp.rs` `avrcp_volume_pct`; no reader in `main.rs` | The phone's volume slider may do nothing. |
| H8 | Bluetooth bonds are never saved or loaded on device. | `bt_a2dp.rs` `BondingStore`; no caller outside the file | Phones must re-pair after every reboot. |
| H9 | Web console commands report "Success" without acting. `restart`, `recovery`, `restart_ota`, `join`, `update` and `sleep` only log; `i2cdetect/get/set/dump` return canned text with no bus access. | `http_control.rs` `execute_web_cli_command` ~1243-1400 | Diagnostics that lie are worse than none. |
| H10 | Default `dac_config` shown to users says `mute=17:0`; the firmware drives PWDN on GPIO33. | `http_control.rs:538`, `web_ui.rs:275` vs `power_boards.rs:300` | Wrong pin documentation served from the device. |
| H11 | The web UI saves `dac_config`, `i2c_config`, `spi_config`, `rotary_config`, `display_config`, `spdif_config`, `led_*`, `ap_name`, `enable_telnet` and `fwurl`, and nothing reads them. | `config_handle.rs` override table | Settings appear to save and have no effect. |
| H12 | No server-silence timeout on the SlimProto socket (legacy reconnects after 35 s). | `main.rs` slim session | A half-open link leaves the player "connected" indefinitely. |
| H13 | STAT reports signal strength `0xFFFF`, voltage 0, and invented buffer sizes. | `squeezelite-proto` `build_stat_with_metrics` | LMS shows wrong player diagnostics. |
| H14 | `strm` flag `0x20` is reused as "wait for `cont`"; legacy reads it as "use SSL". | `squeezelite-proto` `STRM_FLAG_CONT_WAIT` | Latent: if LMS ever sets it with autostart 1, playback waits forever. |
| H15 | CYD Next/Previous are no-ops, and Play/Pause only change local state; LMS is not told and keeps streaming. | `coordinator.rs` `handle_cyd_command` ~2481 | The touchscreen transport buttons do not control playback. |
| H16 | HTTP config edits can be lost on power loss or panic. `flush_now()` marks the handle clean without writing flash; the image waits for a later debounced commit or a clean reboot. | `main.rs` `flush_settings_before_reboot`, `custom_halt` | Edits made from the web UI may not survive a power cut. |
| H17 | Panic after `ota_0` is pinned reboots into the same image with no crash counter. | `main.rs:546` `custom_halt`; `CrashCounterPolicy` host-only | A boot-looping image that gets past Wi-Fi and DHCP is never handed to recovery. |

---

## 3. Parity by area

Only rows that are not plain WIRED are listed in full. WIRED features are summarised per area.

### 3.1 Core LMS playback

**WIRED:** HELO and reconnect; `strm` start/stop/flush/pause/unpause/skip/status; `cont`, `codc`,
`aude`, `audg` (with balance), `setd` name, `serv`, `eqlz`; STAT events STMs/d/l/u/o/n/t/p/r/f/c;
RESP, META (ICY), DSCO; HTTP stream fetch with redirects and chunked transfer; buffer thresholds;
512 KiB / 2000 KiB PSRAM rings; gapless with LAME trim; crossfade and fades; replay gain; polarity
and channel select; 10-band EQ; PCM/WAV/AIFF, FLAC (and Ogg-FLAC), MP3; sample-rate switching at
track boundaries; TAS5805M power, mute, volume, fault recovery.

| Feature | Legacy | Rust | Status | What is missing |
|---|---|---|---|---|
| AAC, Ogg Vorbis, ALAC, Opus | `helix-aac.c`, `vorbis.c`, `alac.c`, `opus.c` | `squeezelite-decoders` (syom, lewton, alac, opus-rs), `select_decoder` | PARTIAL | Compiled into the firmware through the `alloc` feature and selectable, but `HELO_CODECS = "flc,pcm,wav,aif,mp3"`, so LMS never sends them. Device decode with firmware scratch sizes (2304 samples, 1728-byte reads) is untested; all decode tests run with `std-codecs`. |
| LMS server discovery | `slimproto.c:discover_server` | `squeezelite-proto` `DiscoveryBuilder`; `network.rs` (std) | HOST-ONLY | Device uses the configured IP or the first-boot default `10.0.0.10`. |
| Resampling / max sample rate | `resample16.c`, `-R`, `-Z` | `audio-core` resampler; `set_max_sample_rate` | HOST-ONLY | `maxrate` is stored in config and never pushed to the coordinator. |
| Sample rates above 48 kHz | `output_i2s.c` APLL | `clock_table.rs` (13 rates to 192 kHz) | WIRED, UNVERIFIED | HELO advertises `MaxSampleRate=192000`; the clock table says rates above 48 kHz await BCK measurement. Only 44.1 kHz is evidenced on hardware. |
| Output idle power-off (`-C 30`) | `slimproto.c:886`, `output_i2s.c` | — | MISSING | Amp is stopped only when `aude` disables the DAC. |
| Sync: pause-for-N-ms, unpause-at | `output.c` PAUSE_FRAMES, START_AT | `pause_until_ms`, `unpause_at_jiffies` | PARTIAL | Loop-lap granularity; no frame-exact silence insertion or DMA discard. Multi-room accuracy UNVERIFIED. |
| Server-silence timeout | `slimproto.c:776` | — | MISSING | See H12. |
| STAT field accuracy | `sendSTAT` | `build_stat_with_metrics` | PARTIAL | See H13. |
| `dsco` from server | `process_dsco` | parsed, unhandled | MISSING | |
| `loud` level restore at boot | `equalizer.c` | `set_loudness_level` | PARTIAL | Boot forces level 5 or 0 from a bool. |
| `audo` output routing | `output_i2s.c` | `dac_platform::AudioOutputPolicy` | MISSING on device | Dead module. |
| Ogg comment metadata → META | `stream.c` | — | MISSING | Matters once Ogg formats are advertised. |
| SPDIF output | `output_i2s.c` `spdif_convert` | `dac_platform::SpdifBmcEncoder` | HOST-ONLY | Dead module. |
| Other DAC drivers (TAS57xx, TAS5713, AC101, WM8978, generic `dac_controlset`) | `components/squeezelite/*` | `dac_platform::DacModel` | MISSING on device | Stub init tables, not ports. |
| Visualization export | `output_visu.c` | — | MISSING | No producer in the pipeline; no FFT anywhere in `rust/`. |
| Decode and output on separate tasks | FreeRTOS tasks | single core-0 superloop | PARTIAL | This is `D1` in `docs/mk5-rust-todo.md`. |
| HTTPS streams, local files, WMA/DSD/faad, CS4265 | various | — | N/A | Not built in the legacy firmware either. |

Stale constants worth removing: `main.rs:62 BASEMENT_HELO_CAPABILITIES` and `board.rs:97` list
`aac,ogg,alc,ops` but are used only by tests.

### 3.2 AirPlay (RAOP)

**WIRED (hardware UNVERIFIED):** mDNS `_raop._tcp` announce and query reply; RTSP server on TCP
5000 (one connection); Apple-Challenge with RSA-2048; AES key unwrap and AES-128-CBC payload
decrypt; ALAC decode into the shared pipeline; volume; stall and teardown handling.

| Capability | Legacy | Rust | Status | What is missing |
|---|---|---|---|---|
| Control port 6001 (sync, retransmit) and timing port 6002 | `rtp.c` | `rtp.rs` `RtpSyncPacket`, `NtpTimingPacket` | HOST-ONLY | SETUP advertises both ports; no socket is bound. `SOCKET_SLOTS = 8` is fully used. |
| Jitter/reorder buffer, resend requests, latency hold | `rtp.c` | `RtpJitterBuffer` | HOST-ONLY | Packets are decoded in arrival order. Confirms the "not wired" note in the todo. |
| Clock sync and drift correction | `rtp.c` | `SyncDriftTracker` | HOST-ONLY | No A/V or multi-room sync. |
| DMAP metadata, progress | `raop.c`, `dmap_parser.c` | `dmap.rs`, `RtspServer::take_metadata` | HOST-ONLY | Parsed, never consumed. |
| Artwork | `raop.c` | `CoverArtwork` | PARTIAL, likely broken | See H6. |
| DACP remote control | `raop.c:746` | `dacp.rs` | HOST-ONLY | No mDNS query or outbound connection on device. |
| AirPlay name and port config | `raop_sink.c` | — | PARTIAL | Port fixed at 5000; name from `host_name`/`player_name`. |
| PCM (`cn=0`) payloads | `rtp.c` | — | UNVERIFIED | Advertised; device always configures ALAC. |

### 3.3 Bluetooth

**WIRED (hardware UNVERIFIED):** BR/EDR controller bring-up through the vendored, patched
`esp-radio` in BTDM mode over VHCI; HCI init; incoming ACL with SSP Just Works; SDP A2DP-sink
record; AVDTP signalling and media; SBC decode into the pipeline at 44.1 or 48 kHz; enable flag.

| Capability | Legacy | Rust | Status | What is missing |
|---|---|---|---|---|
| Bond persistence | Bluedroid NVS | `BondingStore` | HOST-ONLY | See H8. |
| Legacy PIN (`bt_sink_pin`) | `bt_app_sink.c` | `update_pin_code` | PARTIAL | Hard-coded "0000"; setter never called. |
| Device name change at runtime | `bt_app_sink.c` | `effective_bt_name` | PARTIAL | Applied to policy; HCI name and EIR sent once at init; SDP name fixed. |
| AVRCP absolute volume | `BT_SINK_VOLUME` | `avrcp_volume_pct` | PARTIAL | See H7. |
| AVRCP controller (play/pause/next/prev to phone) | `bt_app_sink.c:102` | `send_avrcp_passthrough` | HOST-ONLY | Never called on device. |
| AVRCP metadata, play status, position | `bt_app_sink.c:362` | `request_avrcp_track_metadata` | HOST-ONLY | Nothing reaches CYD or LMS. |
| Sink volume persistence | `bt_sink_volume` | — | MISSING | |
| A2DP source (LMS → BT speaker) | `bt_app_source.c`, `output_bt.c` | `BtA2dpSourceState`, `SbcEncoder` | HOST-ONLY | Used only under `#[cfg(test)]`; no config keys. |

Platform risk: Wi-Fi and BTDM share one radio and everything runs on one core-0 loop. Neither A2DP
throughput nor Wi-Fi stability under coexistence is evidenced.

### 3.4 Spotify Connect

| Capability | Legacy | Rust | Status | What is missing |
|---|---|---|---|---|
| mDNS `_spotify-connect._tcp` | cspot | `zeroconf.rs`, `main.rs` | WIRED | |
| ZeroConf `getInfo` | `Shim.cpp` | `ZeroConfServer`, served on port 80 | WIRED | `device_id` is hard-coded, not derived from the MAC. |
| ZeroConf `addUser` | `LoginBlob.cpp` | `decode_blob` | PARTIAL | Blob is decrypted, then discarded; `take_last_user()` has no caller. |
| Stored credentials | `Shim.cpp` | `ReusableCredentials` | HOST-ONLY | |
| Access-point connection, handshake, login | `Session.cpp` | — | MISSING | No outbound AP client exists. Device port 4070 is a listener, which real clients never use. |
| Shannon cipher, Mercury, audio key | cspot | `shannon.rs`, `mercury.rs` | HOST-ONLY | Not referenced by the app. |
| Spirc control | `SpircHandler.cpp` | `spirc.rs` | Non-functional | Fed from the inbound 4070 socket; real Spirc travels inside the encrypted AP link. |
| CDN fetch | `CDNAudioFile.cpp` | `cdn.rs` (decrypt only) | MISSING | No HTTP client, and no TLS stack in `Cargo.lock`. |
| Bitrate setting, metadata, artwork | `Shim.cpp` | — | MISSING / HOST-ONLY | |

Verdict: discovery works; nothing else does. Reaching playback needs an outbound AP client, a TLS
client, a CDN fetcher and credential storage.

### 3.5 Network, platform and configuration

**WIRED:** Wi-Fi STA connect and 5 s reconnect loop; DHCP client; mDNS announces for AirPlay and
Spotify; dead-man network lease watchdog; reading legacy Wi-Fi credentials from the `nvs`
partition; debounced settings journal; 20 s hardware watchdog; app descriptor.

| Capability | Legacy | Rust | Status | What is missing |
|---|---|---|---|---|
| Provisioning with no credentials | soft-AP + captive portal | — | MISSING | See H4. `wifi::SoftApConfig` and `CaptivePortalDns` are host-only. |
| Wi-Fi scan | `/scan.json` | stub | MISSING | Returns only the current SSID with placeholder BSSID and channel. |
| Multiple stored APs | `aplist` | `KnownApList` | HOST-ONLY | One credential is used. |
| Web-set Wi-Fi across reboot | NVS | journal | PARTIAL | See H3. |
| Static IP, DHCP hostname, DNS resolver | lwIP | `StaticIpConfig` (host-only); none | MISSING | No DNS socket is created, so no hostname-based URL can work. |
| `<host>.local` A-record answer | mdns | — | UNVERIFIED | Likely not answered. |
| Legacy `settings` NVS keys | `platform_config.c` | — | MISSING | A legacy-configured device keeps only its Wi-Fi credentials; host name, LMS server, EQ and the rest fall back to compile-time defaults. |
| Settings store location | `settings` partition | journal at `0x3FE000` (page 14 of `settings`) | WIRED | Collision with legacy NVS use of that page is UNVERIFIED. |
| Override table | whole NVS | 20 slots, key ≤24, value ≤64 | PARTIAL | One key per POST; legacy multi-key batches lose all but the first. |
| Factory reset / erase config | `nvs_erase_configuration` | — | MISSING | |
| Serial console, telnet, log levels | `platform_console`, `telnet` | `console.rs` | HOST-ONLY | |
| Ethernet (W5500, LAN8720, DM9051) | `network_ethernet*.c` | `eth_w5500.rs` | HOST-ONLY / MISSING | Whether the Louder board has a PHY is UNVERIFIED. |
| Reset-reason reporting, task monitor | `esp_app_main.c`, `monitor.c` | — | MISSING / PARTIAL | PSRAM stats only. |

### 3.6 OTA, recovery and boot safety

**WIRED:** otadata erased at every boot so a reset before Wi-Fi and DHCP falls back to the factory
app; `ota_0` pinned after a healthy boot; `/recovery.json` hands off to the legacy recovery app;
`/reboot.json`.

| Capability | Legacy | Rust | Status | What is missing |
|---|---|---|---|---|
| OTA by upload | `flash_post_handler` (recovery app) | `OtaFlasher`, `feed_ota_chunk` | HOST-ONLY | See H2. |
| OTA by URL | `squeezelite-ota.c`, `fwurl` | `RecoveryModeController::begin_url_ota` | HOST-ONLY | No HTTP client or DNS on device. |
| Image header check | `ota_header_check` | `verify_ota_binary_header` | HOST-ONLY | |
| Crash counter → recovery | `esp_app_main.c:326` | `CrashCounterPolicy` | HOST-ONLY | See H17. |
| Recovery UI | factory app | — | N/A today | With no legacy factory image there is no recovery at all; bootloader behaviour in that case is UNVERIFIED. |

`partitions.csv` has `factory` (recovery) and one `ota_0` slot. A Rust-only device needs a decision
on layout before real OTA can exist: a second OTA slot, or a small Rust recovery image.

### 3.7 Web UI and REST

**WIRED:** `/` (self-contained Rust UI), `/status.json`, `/cmd.json`, `/eq.json`, `/config.json`
GET, `/messages.json` (RAM ring), `/reboot.json`, `/recovery.json`, Spotify ZeroConf on port 80.
One connection at a time; requests capped at 1024 bytes; non-LAN `Origin` is refused.

| Endpoint / tab | Status | Note |
|---|---|---|
| `POST /flash.json`, Updates tab | MISSING (fake) | H2 |
| `/scan.json`, `/ap.json`, WiFi tab | Stub | Fake scan list; join works live only (H3); disconnect is cosmetic |
| `POST /config.json`, NVS editor | PARTIAL | One key per request; edits the 20-slot override table, not NVS |
| `POST /commands.json`, Console tab | Mostly stub | H9 |
| Hardware tab | No effect | H11 |
| `POST /reboot_ota.json` | WIRED | Never call it on BASEMENT (standing safety rule) |

### 3.8 Peripherals and board support

**WIRED:** TAS5805M native driver — typestate power control, PLL/format, digital volume, mute,
analog gain, Hi-Z staging, fault polling every 2 s with auto-recovery, telemetry. This is ahead of
legacy, which had no TAS58xx driver. Software EQ and loudness. CYD UART link at 460800 baud.

| Capability | Legacy | Rust | Status | What is missing |
|---|---|---|---|---|
| Runtime pin configuration (`set_GPIO`, `dac_config`, `i2c_config`, `spi_config`, …) | `accessors.c` | compile-time typed pins | MISSING | Architectural: every pin is a typed esp-hal peripheral. |
| Displays (SSD1306, SH1106, SSD132x, SSD1351, SSD1675, ST77xx, ILI9341), GDS core, fonts | `components/display` | `display_gds.rs` | HOST-ONLY | Framebuffer and init tables only; no bus I/O, no fonts. |
| LMS display frames (`grfe`, `grfb`, `grfs`, `grfg`, `grfa`, `vfdc`, `visu`) | `displayer.c` | `LmsDisplayCommand` | HOST-ONLY | Dropped on device as unknown opcodes. |
| VU meter, spectrum, artwork JPEG | `displayer.c`, `tjpgd` | — | MISSING | No FFT and no JPEG decoder in `rust/`. |
| Status LEDs, WS2812 strip, LED visualizer | `led.c`, `led_strip` | `controls.rs` enums | MISSING / HOST-ONLY | No LEDC or RMT use. |
| Buttons, `actrls_config` | `buttons.c`, `audio_controls.c` | `ButtonDebouncer`, `ActrlsMap` | HOST-ONLY, incompatible | Different key and token names from legacy; cannot parse legacy nested JSON. |
| Rotary encoder | `rotary_encoder.c` | `RotaryEncoder` | HOST-ONLY | No push switch or long-press modes. |
| IR receive | `infrared.c` | `IrDecoder` | HOST-ONLY | No RMT capture. |
| GPIO expanders | `gpio_exp.c` | `GpioExpanderKind` | HOST-ONLY | Descriptors only. |
| Battery ADC, low-battery handling | `battery.c` | `BatteryMonitor` | HOST-ONLY | No ADC use. |
| Deep sleep and wake sources | `services.c` | `SleepManager` | MISSING on device | No RTC sleep call anywhere. |
| Jack detect, speaker fault GPIO | `monitor.c` | flags only | HOST-ONLY | TAS FAULT pin (GPIO34) is configured and never read after boot; faults come from I2C polling. |
| TAS on-chip DSP (biquads, DRC, PBTL/mono) | — | — | MISSING | Enhancement, not a regression. |
| TAS hard power-down | — | `try_enter_deep_sleep` | HOST-ONLY | PWDN is held high permanently. |
| `board-louder-esp32-s3` | `I2S-S3` config | `board.rs` | BROKEN | `compile_error!` at `main.rs:2760`; esp-hal features are hard-coded to `esp32`. |
| Other boards (SqueezeAmp, Muse, T-Watch) | `targets/`, build scripts | `BoardProfile` | HOST-ONLY | Descriptor strings only. |

CYD link detail:

| Capability | Status | What is missing |
|---|---|---|
| UART link, heartbeat, volume commands | WIRED | Pins and baud are fixed; no `cyd_config`. |
| Transport commands | PARTIAL | H15. |
| Metadata | PARTIAL | ICY `StreamTitle` only; no album, no LMS track metadata. |
| Progress and duration | PARTIAL | Duration is always 0. |
| Full sync on request, Wi-Fi/IP status | HOST-ONLY / MISSING | `sync_cyd_telemetry` is called only from tests. |
| Wire format | Incompatible with the legacy C CYD | Rust host sends text (`STATE=P`); the C controller parses only NDJSON. Rust CYD parses both. |
| Rust CYD controller app (ILI9341, XPT2046, UI) | WIRED (separate firmware) | Backlight is on/off only. Build not run in this audit. |

---

## 4. Corrections to earlier documents

These documents overstate what runs on the device. Treat their "IMPLEMENTED & VERIFIED" marks as
"implemented and host-tested" unless this audit says WIRED.

- `docs/superpowers/plans/2026-10-29-rust-parity-analysis.md` and
  `docs/superpowers/plans/2026-10-29-rust-full-parity-roadmap.md`:
  - "Dual-core embassy tasks" and "Core 1 decode_pump + i2s_feed": core 1 runs a NOP heartbeat.
  - HELO advertising `aac,ogg,ops,alc`: the device advertises `flc,pcm,wav,aif,mp3`.
  - UDP discovery, resampler, 7 DAC families, SPDIF, `audo`, jack auto-mute, visualizer export,
    RSSI and battery in STAT: host-only, dead code or absent on device.
  - Soft-AP, captive DNS, known-AP list, static IP, W5500, full REST API, telnet/CLI, streaming and
    URL OTA, Rust recovery handling: host-only or stubbed on device.
  - Display, controls, IR, LEDs, battery, sleep and the LMS display opcodes: host-only or absent.
  - Bluetooth source, persistent bonds, AVRCP metadata forwarding: not on device.
  - Named symbols that do not exist: `CodecMemoryBudget`, `BtBondTable`, `A2dpSourceSession`,
    `ExternalSourceArbiter` (the real type is `SourceArbiter`), `GdsDisplayManager`,
    `BoardDescriptor::from_target`, `ExternalDac`.
  - Factual slips: AirPlay RSA is 2048-bit, not 1024; Spotify DH is Oakley group 1, not X25519;
    there is no HTTP Range CDN fetcher, JPEG decoder or FFT; the mDNS responder is in `main.rs`,
    not `wifi.rs`.
- `docs/superpowers/plans/2026-11-05-web-interface-feature-parity.md`: says the firmware uploader is
  wired. The upload is not processed.
- `docs/superpowers/plans/2026-09-29-cyd-uart-management-link.md`: specifies 115200 baud and NDJSON;
  the code is 460800 baud and the Rust host speaks the text format.
- `rust/crates/louder-esp32-app/Cargo.toml`, `runtime-flash-persist` comment: describes the off
  state as "no runtime flash writes". See H1.
- `docs/mk5-rust-todo.md` section 1: "AAC, Vorbis and Opus are host-only" is stale; they are
  compiled into the firmware and not advertised. Line references to the superloop have drifted
  (now ~4659).
- `.agents/knowledge/ACTIVE_BUGS.md`: says `cargo-audit` is not installed; it is, and the last scan
  was clean.

---

## 5. Next backlog

Sizes: S = under a day, M = a few days, L = a week or more, XL = multi-week. "HW" = needs the device
to verify. IDs use the `P` prefix to stay clear of `T01`-`T27`, `HW1` and `D1`.

### Phase A — make the certified board honest and safe (do first)

| ID | Task | Fixes | Size | HW |
|---|---|---|---|---|
| P01 | Decide the runtime flash-write policy and make code, `Cargo.toml` comment and docs agree. Either gate the default-build write (no write while playing, or reboot-path only) or certify the current behaviour with a power-cycle soak. | H1, H16 | M | Yes |
| P02 | Make `/flash.json` truthful: return an explicit "not supported, use recovery" error and hide or relabel the upload control until real OTA exists. | H2 | S | No |
| P03 | Use journal Wi-Fi credentials at boot (`resolve_boot_wifi_with_journal`), with fallback to legacy NVS if the journal network fails before the lease deadline. | H3 | S | Yes |
| P04 | Web console: make `restart`/`recovery` act, implement real I2C ops on the amp bus or remove them, drop commands that cannot work, and return real status. | H9 | S-M | I2C only |
| P05 | Correct served pin metadata (`mute=33:0`) and hide or mark read-only the hardware settings that have no effect. | H10, H11 | S | No |
| P06 | AirPlay RTSP: stream or bound request bodies properly, or stop advertising artwork (`md=1`). | H6 | S | Yes |
| P07 | Bluetooth sink quick wins: apply AVRCP volume, persist and load bonds, wire `bt_sink_pin`. | H7, H8 | S each | Yes |
| P08 | Source arbitration: decide the policy (LMS wins, as legacy; or external wins, as now) and document it. If external wins, tell LMS instead of silently dropping `strm`. | H5 | S-M | Yes |
| P09 | SlimProto robustness: server-silence timeout; real RSSI and buffer sizes in STAT; handle `dsco`; restore `loud` level; resolve the `0x20` flag overload. | H12-H14 | S each | Partly |
| P10 | CYD: send transport commands to LMS; emit full sync and Wi-Fi/IP status; fill duration. | H15 | M | CYD |
| P11 | Documentation: mark the two 2026-10-29 parity documents as superseded by this audit; fix the `Cargo.toml` comment, todo section 1 and `ACTIVE_BUGS.md`; remove the stale HELO test constants. | Section 4 | S | No |

### Phase B — LMS features users will notice

| ID | Task | Size | HW |
|---|---|---|---|
| P12 | Validate AAC, Vorbis, ALAC and Opus on device with firmware scratch sizes, then advertise each one that passes in `HELO_CODECS`. | S + M | Yes |
| P13 | LMS server discovery over smoltcp UDP, using the existing proto builder and parser. | S-M | Yes |
| P14 | Output idle power-off (legacy `-C 30`): put the amp in standby after an idle period. | S | Yes |
| P15 | Wire `maxrate` and the resampler; certify 88.2-192 kHz clocks or lower the advertised `MaxSampleRate`. | S + M | Yes |
| P16 | Frame-accurate pause-for and start-at for synced groups. | M | Yes, ideally two players |
| P17 | AirPlay quality: bind 6001/6002, wire `RtpJitterBuffer`, resend and `SyncDriftTracker`; forward metadata and progress; DACP. Needs socket-slot budget. | M-L | Yes |
| P18 | Bluetooth AVRCP controller and metadata to CYD and LMS. | M | Yes |

### Phase C — run without the legacy recovery app

| ID | Task | Size | HW |
|---|---|---|---|
| P19 | Decide the partition and recovery strategy for a Rust-only device (second OTA slot vs small Rust recovery image), then implement real OTA by upload using the host-tested `OtaFlasher`. | L | Yes |
| P20 | Provisioning: soft-AP, captive DNS and AP-mode HTTP when no credentials exist. | L | Yes |
| P21 | Crash counter and recovery fallback; define boot behaviour when no factory app exists. | M | Yes |
| P22 | Settings: read the legacy `settings` NVS once to migrate; confirm the journal page cannot collide; raise or restructure the 20-slot limit; factory reset. | M-L | Soak |
| P23 | Network basics: DNS resolver, DHCP hostname, static IP, real scan, known-AP list. | M | Yes |
| P24 | Serial or telnet console, log levels, reset-reason reporting. | M | Partly |

### Phase D — large or optional (owner decisions)

| ID | Task | Size | Note |
|---|---|---|---|
| D1 | Move decode and output to core 1. | L | Existing item, still owner-gated. Requires `esp-rtos-core1` certified on hardware first. |
| P25 | Spotify Connect: outbound AP client, TLS client, credential storage, CDN fetch. | XL | Decide whether it is in scope at all. No TLS stack exists today. |
| P26 | Bluetooth source (LMS → BT speaker). | L | |
| P27 | Display stack and LMS display protocol. | L | Only if a display will be attached. |
| P28 | Buttons, rotary, IR, LEDs, battery, sleep, GPIO expanders. | L | Only for boards that have them; needs a legacy-compatible config schema. |
| P29 | Runtime pin configuration, other DACs, SPDIF, Ethernet, ESP32-S3. | L each | Multi-board support; an architectural change from typed pins. |
| P30 | Dead code: decide whether to keep `tasks.rs` and the host-only modules as the future architecture (and wire them under `D1`) or prune them. They add maintenance cost and made earlier parity claims look complete. | S decision | |

### Suggested order

1. P11 and P02, P05: cheap, no hardware, and they stop the device and the docs from misleading.
2. P01: the flash-write policy is the one finding with brick or deadlock potential.
3. P03, P09, P04: robustness on the certified board.
4. P12-P14: the most visible LMS gains for the least work.
5. Owner decisions before any Phase C or D work: Rust-only device or keep legacy recovery (P19-P21);
   Spotify in or out (P25); which boards beyond the Louder ESP32 matter (P27-P29); `D1`.

---

## 6. What this audit did not do

- No build of the xtensa firmware, no host test run, no device access.
- No measurement of image size against the 4 MB layout with all sources linked.
- No end-to-end trace of the legacy web app's requests against the Rust server beyond the endpoint
  list.

## 7. Open questions (UNVERIFIED)

1. Does AirPlay, Bluetooth sink or Spotify discovery work on hardware at all? No test record was
   found. The BASEMENT certification covered LMS FLAC and MP3.
2. How long do RSA-2048 (AirPlay) and Oakley DH (Spotify `getInfo`) take on the core-0 loop, and do
   they cause audio underruns or approach the 20 s watchdog?
3. Do the 88.2, 96, 176.4 and 192 kHz I2S clocks produce correct BCK on this board?
4. Do AAC, Vorbis, ALAC and Opus decode in real time on device with firmware scratch sizes? Does
   the vendored `syom` decode HE-AAC SBR?
5. What does the ESP32 bootloader do when otadata is erased and the factory partition is empty?
6. Does the legacy firmware ever write page 14 of the `settings` partition, where the Rust journal
   lives? Is the journal's Wi-Fi password stored in plaintext?
7. Is the certified module 4 MB flash / 4 MB PSRAM (per the partition table and the state ledger) or
   the N8R8 named in `board.rs`? Which amp is fitted at `0x2D`, TAS5805M or TAS5825M?
8. Does the Louder ESP32 PCB expose any LED, button, IR, display header, jack or Ethernet PHY? The
   N/A calls in section 3.8 depend on the answer.
9. Does LMS ever set `strm` flag `0x20` for this player?
10. Is the inverted source arbitration (H5) intentional?
