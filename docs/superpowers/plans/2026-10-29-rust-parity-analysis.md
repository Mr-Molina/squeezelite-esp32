# Feature-Parity Analysis: squeezelite-esp32 (C/C++) vs MK5-Rust

**Branch analyzed:** `MK5-Rust` (head `b49854f`), Rust workspace at `rust/` (~36k lines, 9 crates, 305 tests all passing).
**Baseline:** the C/C++ firmware in `components/` + `main/` on the same branch (which is `master-v4.3` plus MK5-only additions: `components/cyd_link/`, slimproto reconnect backoff, encrypted Wi-Fi/cspot credentials, HTTPS-only OTA with anti-rollback).

---

## 1. Executive summary

The two codebases solve different problems today:

- The **C firmware** is a universal, configurable audio platform: any I2S DAC / SPDIF / BT-source output, 7 codec families, AirPlay, Spotify Connect, BT sink, 11 display drivers, buttons/rotary/IR, LED strips, battery, telnet, captive-portal Wi-Fi manager, full web UI, console CLI, recovery-partition OTA.
- The **Rust firmware** is a single-purpose, single-board LMS player for the Sonocotta Louder-ESP32 (ESP32-WROVER + TAS5805M): hardcoded pins, hardcoded player name/MAC/SSID/LMS-IP, slimproto over smoltcp, native FLAC/MP3/PCM decode, 10-band EQ, BT A2DP sink (SBC), a small status/control web page, CYD touchscreen companion, and an anti-brick boot scheme that leans on the C recovery partition.

**Overall parity (player use-case, on-device):** roughly **40–50%** of what the C firmware does *for this one board*, and much less of the platform as a whole. The core LMS play/pause/stop/volume path with FLAC/MP3/PCM genuinely works end-to-end (and has been live-certified against a real LMS per the branch's work orders), but playback *semantics* (gapless, crossfade, replay gain, sync timing, buffering thresholds), service discovery, provisioning, on-device OTA, and the entire AirPlay/Spotify/display/controls surface are missing.

**A critical structural finding:** the Rust workspace has **two diverging implementations**. The host (`std`) build — `network.rs`, `coordinator.rs` — implements significantly more of slimproto (STMn error paths, `strm a` skip-ahead, META/RESP forwarding, LMS UDP discovery, HTTP redirects, AAC/Vorbis decode) than the device (`xtensa`) build, where everything is re-implemented inline in `run_hardware_firmware()` (`main.rs:427–2919`, ~2,500 lines). Tests exercise the host path; hardware runs the device path. Closing *that internal* gap is the cheapest parity win available.

---

## 2. Parity matrix

Legend: ✅ parity (for practical purposes) · 🟡 partial · ❌ missing · ⬜ N/A by design.
"Device" = the xtensa build that actually runs on hardware. "Host" = the std build used by tests.

### 2.1 Slimproto protocol

| Feature | C firmware | Rust device | Rust host | Notes |
|---|---|---|---|---|
| `strm s/p/u/q/f/t` | ✅ | ✅ | ✅ | Device handles all six |
| `strm a` (skip ahead) | ✅ | ❌ ignored | ✅ | `coordinator.rs:445` only |
| `strm s` autostart 0–3 / `cont` wait | ✅ | ✅ | ✅ | |
| Pause-for-N-ms / unpause-at-jiffies | ✅ | ❌ ignored | ❌ | Breaks multi-room sync & precise pause |
| `?` codec placeholder + `codc` | ✅ | 🟡 `codc` yes | 🟡 | |
| `audg` | ✅ sw gain + adjust flag | 🟡 | 🟡 | Device maps gain→TAS 0x4C register; `digital_atten`/preamp ignored |
| `aude` (+ power GPIO) | ✅ | 🟡 mute/amp-stop only | 🟡 | No power-GPIO concept |
| `setd` id 0 (name get/set) | ✅ persisted | 🟡 RAM only | 🟡 | Rust name resets to "BASEMENT" on reboot |
| `setd` 0xfe (display dims) | ✅ | ⬜ | ⬜ | No display |
| `serv` (switch server, SN) | ✅ | ❌ | ❌ parse-only | |
| `eqlz` / `loud` (LMS plugin EQ) | ✅ applied | ❌ ignored | 🟡 enables EQ only | Rust has its own EQ but LMS can't drive it |
| `grfe/grfb/grfs/grfg/grfa`, `visu`, `vfdc`, `ledv/ledd` | ✅ | ⬜/❌ | ⬜/❌ | Display/LED-strip protocol absent |
| STAT events | STMs,d,t,l,u,o,n,f,c,p,r | STMt,c,p,r,q,f,l,s,d,u | + STMn | Device never sends **STMn** (unsupported codec→muted PCM instead!), nor STMo; no STAT RSSI/battery fields (hardcoded) |
| `RESP` / `META` | ✅ both | 🟡 RESP only | ✅ | Device discards ICY metadata instead of META-forwarding |
| `BUTN` / `IR` / `ANIC` | ✅ | 🟡 IR builder used by web UI "remote" | ❌ | Clever: Rust web buttons send NEC IR codes to LMS |
| UDP discovery (port 3483) | ✅ | ❌ hardcoded 10.0.0.10 | ✅ | smoltcp built without UDP; `DiscoveryBuilder` exists but unwired on device |
| Reconnect behavior | ✅ backoff+jitter [MK5] | 🟡 flat 1 s retry | 🟡 | |
| Sync groups (`SyncgroupID`, start-at) | ✅ | ❌ | ❌ | |

### 2.2 Codecs (on-device decode)

| Codec | C | Rust device | Rust host |
|---|---|---|---|
| FLAC (+Ogg-FLAC) | ✅ libFLAC | ✅ custom RFC-9639 decoder (CRC-8/16, LPC≤32, mono/stereo, block≤4096, ~192 KB heap → needs PSRAM) | ✅ |
| MP3 | ✅ libmad (Xing/LAME gapless) | ✅ nanomp3 static, zero-heap; +Layer I/II; **no LAME gapless trim** | ✅ |
| PCM/WAV/AIFF | ✅ | ✅ incl. header strip, 8/16/24/32-bit BE/LE | ✅ |
| AAC (ADTS+MP4, SBR) | ✅ helix | ❌ (std-only `syom`, ADTS only — no MP4 container) | ✅ |
| Ogg Vorbis | ✅ Tremor | ❌ (std-only `lewton`) | ✅ |
| Opus | ✅ | ❌ | ❌ |
| ALAC (MP4) | ✅ | ❌ | ❌ |
| HELO caps advertised | per build, up to all 7 | `flc,pcm,wav,aif,mp3` (honest) | `mp3,flac,aac,ogg,pcm` |

LMS transcodes what isn't advertised, so unsupported codecs still *play* — at the cost of LAN bandwidth, server CPU, and loss of native-rate passthrough.

### 2.3 Playback semantics

| Feature | C | Rust |
|---|---|---|
| Gapless playback | ✅ | ❌ — every `strm s` flushes both ring buffers (`main.rs:2175`, `pipeline.rs:491`); audible gap between tracks |
| Crossfade / fade in/out | ✅ 4 modes | ❌ `trans_type/duration` parsed, unused |
| Replay gain | ✅ applied w/ cross-track | ❌ never applied |
| Polarity inversion, mono/L/R channel select | ✅ | ❌ |
| Buffer thresholds (`threshold_kb`, `output_threshold`) | ✅ | ❌ ignored on device |
| Resampling | ✅ resample16 (16-bit build) | ❌ |
| Elapsed-time accuracy | DMA-corrected (`device_frames`) | frames-written only; `AccuratePlayPoints=1` claim is optimistic |
| Buffers | 2 MB stream / ~3.5 MB output (PSRAM) | 512 KB / 128 KB PSRAM (6+6 KB fallback w/o PSRAM) |

### 2.4 Outputs & hardware

| Feature | C | Rust |
|---|---|---|
| I2S DAC output | ✅ any pins, APLL, MCLK | ✅ fixed pins (BCLK26/WS25/DO22), fractional divider, software L/R swap |
| Sample rates | 8k–192k (16-bit build) | 11.025k–96k; **8k/176.4k/192k silently fall back to the 44.1 kHz divider** (pitch error) |
| DAC controller drivers | TAS57xx, TAS5713, AC101, WM8978, ES8388, ES8311, generic-JSON | TAS5805M/TAS5825M only — but far deeper than C (telemetry FS/BCK/PLL/automute, analog gain, fault decode+auto-recovery, spread spectrum, Hi-Z staging) |
| SPDIF | ✅ software BMC | ❌ (yet HELO claims `HasDigitalOut=1`) |
| BT A2DP **source** (-o BT) | ✅ | ❌ |
| BT A2DP **sink** | ✅ Bluedroid, AVRCP ct+tg, abs volume, metadata→display | 🟡 impressive custom zero-heap HCI/L2CAP/SDP/AVDTP/SBC stack on patched esp-radio BTDM; **AVRCP is ack-only stub, bonds RAM-only (lost at reboot), no abs-volume, no LMS arbitration/status** |
| Output arbitration (BT/AirPlay/cspot vs LMS) | ✅ `decode_external.c` | 🟡 BT ignored while LMS streams; nothing else exists |
| Dual-core use | ✅ FreeRTOS tasks across cores | ❌ Core 1 runs a NOP heartbeat; everything cooperative on Core 0 |

### 2.5 DSP

| Feature | C | Rust |
|---|---|---|
| 10-band EQ | ✅ esp_equalizer, LMS-driven, 4 rates, 16-bit only | ✅ own RBJ biquads, Schur-Cohn stability check, 5 presets — but **coefficients precomputed for 44.1 kHz only** (wrong centers at 48k+), not LMS-driven |
| Loudness | ✅ volume-dependent per-band contour | 🟡 per-sample polynomial on amplitude — not tied to volume; different (and questionable) behavior |
| Visualizer/VU export | ✅ FFT/VU → display/LED | ❌ |

### 2.6 Network, provisioning, system

| Feature | C | Rust |
|---|---|---|
| Wi-Fi provisioning | ✅ captive portal + AP fallback + scan UI + known-AP list | ❌ STA only; reads C firmware's NVS creds (incl. MK5 `enc:` XOR scheme) else compile-time SSID; resets to recovery if neither works |
| Ethernet (LAN8720/W5500/DM9051) | ✅ | ❌ |
| Static IP / DNS | ✅ | ❌ DHCP only, no DNS resolver |
| mDNS | ✅ (hostname, `_raop`, `_spotify-connect`) | ❌ none (no UDP in smoltcp build) |
| Telnet / console CLI / autoexec | ✅ | ❌ |
| HTTP web UI | ✅ full config webapp (Wi-Fi, audio, hw, NVS editor, OTA, syslog) | 🟡 single dashboard page: status JSON + transport/volume/EQ-preset/analog-gain buttons; no auth; can't configure Wi-Fi, name, LMS IP, loudness, BT |
| HTTP API | `/ap /scan /config /status /messages /commands /connect /reboot* /recovery /flash` | `/status.json /cmd.json /recovery.json /reboot.json /reboot_ota.json` |
| AirPlay (RAOP) | ✅ full receiver (RSA/AES, RTP resend, DACP, artwork) | ❌ zero code |
| Spotify Connect (cspot) | ✅ | ❌ zero code |
| LMS CLI (9090) control | ✅ | ❌ (ports only displayed in status) |

### 2.7 OTA, recovery, persistence

| Feature | C | Rust |
|---|---|---|
| OTA download/flash | ✅ recovery app: URL (HTTPS-only [MK5], anti-rollback) + file upload + LMS proxy discovery | ❌ cannot receive firmware at all; relies on rebooting into the C recovery partition |
| Anti-brick | ✅ crash counters → guided factory | ✅ arguably stronger: otadata pre-erase at boot (single-boot trial), panic→reset→recovery, 45 s/60 s network lease watchdog, 20 s HW WDT |
| Settings store | ✅ cJSON-over-NVS, ~45 keys, deferred commit | 🟡 own CRC'd FlashJournal, **7 keys** (vol/eq/again/loud/name/lms_ip/port — last three ignored at boot); **persisted only via the HTTP reboot endpoints — a power cycle loses changes** |
| NVS compat with C firmware | — | 🟡 read-only (Wi-Fi creds), good for dual-boot coexistence |

### 2.8 Peripherals & UX (C-only domain)

Displays (11 driver families + LMS graphics protocol + scrolling/artwork), buttons/rotary/IR-receiver with JSON action mapping, GPIO expanders, LED status + WS2812 VU strips, battery ADC→STAT, jack detect/speaker fault, deep-sleep/power management, multi-board Kconfig targets (SqueezeAMP, Muse, T-WATCH): **all absent in Rust** — ⬜/❌ depending on whether CYD replaces them for your use.

The CYD companion is the one place Rust *exceeds* C UX-wise on this board pairing (ILI9341 touch now-playing UI), but it is fed starvation rations: the device never sends `META=` (no metadata source — ICY is discarded, `grfe`/`META` unhandled), duration is always 0, and CYD Next/Prev **do nothing** (`coordinator.rs:740-783`), while the C `cyd_link` component forwards toggle/next/prev/vol to real player actions.

---

## 3. Correctness findings (parity-adjacent bugs)

1. **Unsupported codec → silence instead of STMn** (`main.rs:2240`): device mutes a PCM decoder rather than telling LMS "not supported"; LMS thinks it's playing. C sends STMn so the server skips/transcodes.
2. **False HELO claims**: `HasDigitalOut=1` (no SPDIF), `AccuratePlayPoints=1` (no DMA latency correction), `MaxSampleRate=96000` OK, but rates the divider table lacks (8k; 176.4k/192k from FLAC) silently play at 44.1 kHz clock → wrong pitch.
3. **`/reboot.json` mislabeled**: module comment says it boots recovery; code maps it to persist-ota0-and-reboot (`http_control.rs:535`).
4. **EQ presets only correct at 44.1 kHz**; applied unchanged at 48/96 kHz.
5. **Loudness ≠ loudness**: amplitude-dependent per-sample gain, not an equal-loudness contour vs volume.
6. **I2C errors swallowed** (`Esp32I2cBus`, `main.rs:564`): amp failures degrade to shadow-register fiction; only ACK/NACK counters hint at trouble.
7. **BT bonds RAM-only** → re-pair after every reboot; fixed PIN "0000"; auto-confirm SSP accepts any pairing attempt (security + UX gap vs C).
8. **Stale docs**: "FLAC via claxon" comments (`decoders/lib.rs:7`, `pipeline.rs:432`) — the real decoder is custom; `ota_safety.rs:8` claims the settings partition is read-only from Rust, but the device writes 0x3FE000.
9. **Deployment constants baked into the binary** (player "BASEMENT", MAC, SSID "iot", LMS 10.0.0.10) — fine for the bench target, fatal for anyone else flashing it; config fields that would fix this exist (`AudioConfig.lms_ip/name`) but are ignored at boot.

---

## 4. Recommended path to parity (prioritized)

**Tier 1 — make the existing player correct (small, high value):**
1. Unify device/host paths: port the coordinator's STMn error path, `strm a`, META/RESP forwarding into `run_hardware_firmware`, or better, refactor the device loop to drive `PlayerCoordinator` so tests cover what ships.
2. Send STMn for unsupported codecs; drop false HELO flags; reject or correctly clock 8k/176.4k/192k.
3. Apply `AudioConfig.player_name` / `lms_ip` at boot; persist settings on change (debounced), not only at reboot.
4. Pause-interval / unpause-at-jiffies handling (prereq for sync), and honor `threshold`/`output_threshold`.

**Tier 2 — audible feature gaps:**
5. Gapless: stop flushing on `strm s` when the stream is a continuation; track stream boundaries in the output ring (C's `track_start` pointer model).
6. Replay gain application and crossfade (you already parse both fields).
7. Per-rate EQ coefficient computation (RBJ formulas at runtime) + wire `eqlz`/`loud` from LMS; rework loudness as a volume-indexed contour.

**Tier 3 — ecosystem features:**
8. Enable smoltcp UDP: LMS discovery (code already exists in `squeezelite-proto`), then mDNS.
9. AAC on device: `syom` is ADTS-only and std; either port it to no_std+PSRAM or add MP4 demux; Vorbis likewise (lewton is the heavy one). Until then advertising-less transcoding is a reasonable stopgap.
10. AVRCP real implementation (abs volume + passthrough + metadata → finally gives CYD its `META=`), persistent bond store in the FlashJournal.
11. Wi-Fi provisioning: softAP + minimal captive page writing `WifiCredentials` (crate support already exists, unused).
12. On-device OTA upload endpoint in the Rust app (or formalize the "C recovery does OTA" contract in docs).

**Tier 4 — platform scope (decide deliberately whether you want it):** AirPlay receiver, Spotify Connect, SPDIF, BT source, displays/IR/rotary/battery, multi-DAC abstraction, S3 support. Each is a project; none blocks the "best-in-class Louder-ESP32 LMS player" goal.

---

## 5. Where Rust already beats C

Worth keeping in mind:
- TAS5805M driver depth (telemetry, fault auto-recovery, analog gain, type-state init) far exceeds C's `dac_57xx.c`.
- Anti-brick OTA gating (otadata pre-erase single-boot trial + network lease watchdog) is more deterministic than C's crash counters.
- The custom no_std FLAC decoder, zero-heap MP3, CRC'd dual-sector FlashJournal, 305-test suite with fuzzing, and `#![forbid(unsafe_code)]` core crates are all assets the C side has no equivalent of.
- The from-scratch BR/EDR A2DP stack over patched BTDM esp-radio is genuinely novel (esp-radio upstream is BLE-only) — it just needs AVRCP and bonding to reach C's sink parity.
