# Work Order: WO-003 — Upstream GitHub Issues/PRs & C-to-Rust Protocol/Hardware Parity Audit

**Work Order ID:** WO-003  
**Timestamp:** 2026-10-20T20:22:30-04:00  
**Assigned Subagent Role:** Research Specialist (Upstream GitHub & Protocol Parity)  
**Target Files:** `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-003-upstream-protocol-parity.md`  
**Backup Files:** `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/WO-003-upstream-protocol-parity.md.bak`  
**Defect IDs:** N/A  
**Task Goal:** Audit upstream GitHub issues/PRs (`sle118/squeezelite-esp32`, `ralph-irving/squeezelite`, `sonocotta/esp32-audio-dock`) and perform deep code comparison between upstream C `squeezelite-esp32` and `rust/crates/*` on `MK5-Rust` to identify bugs, missing protocol handling, or hardware patches to incorporate.  

---

## 1. Context & Scope
- **Problem Statement:** `MK5-Rust` implements an 8-crate pure-Rust firmware stack (`rust/crates/*`) targeting Louder-ESP32 (`ESP32-WROVER-N8R8`), Louder-ESP32-Plus (`ESP32-S3-WROOM-N8R8`), and Cheap Yellow Display (`ESP32-2432S028R`). We must compare the pure-Rust crates against upstream C `squeezelite-esp32` (`components/squeezelite/`, `components/codecs/`, `components/audio/`, `components/wifi-manager/`) and upstream GitHub (`sle118/squeezelite-esp32`, `ralph-irving/squeezelite`, `sonocotta/esp32-audio-dock`) to find any bugs, protocol gaps, or hardware patches that should be incorporated into `MK5-Rust`.
- **Single Variable Constraint (Rule 5):** Read-only analysis of repository code; only modify `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-003-upstream-protocol-parity.md` to record fulfillment.
- **Pre-Modification Integrity (Rule 31):** Do not modify any source code files.
- **Prohibited Actions (Rule 23, Rule 24):** Do NOT use `sed`, `awk`, or `cat` for file modifications. Use strict absolute paths.
- **Verification Requirement (Rule 21, Rule 52):** Cite exact file paths, line numbers, and upstream URLs/references for every finding.

---

## 2. Hypothesis & Predicted Outcome (Rule 1, Rule 6)
- **Hypothesis:** Deep comparison between upstream C (`slimproto.c`, `stream.c`, `decode.c`, `output_i2s.c`, `tas5805m.c`) + upstream GitHub issues/PRs and `rust/crates/*` will uncover specific protocol commands, HTTP streaming edge cases (redirects, chunked transfer, ICY metadata interval stripping, reconnect/reboot handling), decoder edge cases, or TAS5805M/ESP32-S3 hardware initialization details that can be incorporated into `MK5-Rust`.
- **Predicted Outcome:** Categorized gap & bug analysis report mapping each upstream behavior/patch to exact Rust crate files and line numbers.
- **Falsification Criteria:** 100% parity already exists across all protocol, streaming, decoder, and hardware subsystems.

---

## 3. Detailed Instructions
1. Search upstream GitHub (`sle118/squeezelite-esp32`, `sonocotta/esp32-audio-dock`, `ralph-irving/squeezelite`) via `search_web` for recent bug fixes, PRs, TAS5805M initialization updates, ESP32-S3 I2S/DMA fixes, and SlimProto protocol updates.
2. Read the Rust crates under `/home/console-admin/Github/squeezelite-esp32/rust/crates/` (`squeezelite-proto`, `squeezelite-audio-core`, `squeezelite-tas58xx`, `squeezelite-decoders`, `squeezelite-cyd-link`, `squeezelite-config`, `louder-esp32-app`, `cyd-controller-app`) and compare them against `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/` and `/home/console-admin/Github/squeezelite-esp32/components/audio/`.
3. Specifically check:
   - SlimProto opcodes & flags (`strm` flags/autostart/threshold/transition, `audg` 32-bit dB vs fixed-point gain, `aude` spdif/dac enable, `setd` player name read/write, `STAT` fields like `STMd`/`STMn`/`STMo`/`STMl`/`STMu`/`STMs`/`STMt`, `STOP_SERVER_REBOOT` flag handling).
   - HTTP streaming (`HttpAudioStream` in `louder-esp32-app/src/network.rs`: HTTP 301/302/307/308 redirects, ICY `icy-metaint` metadata block stripping during streaming, custom HTTP headers from `strm` request string, connection retry/timeout).
   - Audio pipeline & decoders (24-bit FLAC/PCM shift scaling, sample-rate transitions across tracks, replaygain / crossfade, `louder-esp32-app` `main.rs` target entrypoint vs `cyd-controller-app` bare-metal `esp-hal` wiring).
   - TAS5805M (`squeezelite-tas58xx`: book/page selection, DSP/BTL configuration, fault clear registers, thermal/overcurrent flags).
4. Append the detailed findings and Fulfillment Section to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-003-upstream-protocol-parity.md` using `edit_file`.

---

## 4. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Execution Timestamp:** 2026-10-20T20:28:30-04:00
- **Files Modified / Created (Absolute Paths):**
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-003-upstream-protocol-parity.md` (updated with audit findings & fulfillment section)
- **Backup Verification:** Verified `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/WO-003-upstream-protocol-parity.md.bak` exists and matches baseline prior to edit.
- **Test / Verification Command & Results:** Read-only code inspection across all 8 Rust crates under `/home/console-admin/Github/squeezelite-esp32/rust/crates/`, C reference baseline under `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/`, and 6 web queries against upstream GitHub (`sle118/squeezelite-esp32`, `ralph-irving/squeezelite`, `sonocotta/esp32-audio-dock`) `[EXECUTED & VERIFIED]`.
- **Observed Outcome vs Predicted Outcome:** Hypothesis confirmed. Identified 24 concrete protocol, HTTP streaming, audio decoder, TAS5805M register initialization, and bare-metal target wiring gaps/bugs across the 8 Rust crates when compared against the C reference implementation and upstream hardware documentation.
- **Fulfillment Status:** `[EXECUTED & VERIFIED]`
- **Summary Path:** N/A (Read-Only audit; only Work Order file updated)
- **Handoff Path:** `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-003-upstream-protocol-parity.md`

---

## 5. Detailed Upstream & C-to-Rust Parity Audit Findings

### Category A: SlimProto Protocol & Coordinator State Machine (`squeezelite-proto` & `louder-esp32-app/src/coordinator.rs`)

1. **[CRITICAL] Missing Outbound Client Packets (`RESP`, `DSCO`, `META`, `SETD`)**
   - **C Baseline:** `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/slimproto.c` lines `241-341` (`sendDSCO`, `sendRESP`, `sendMETA`, `sendSETDName`) and lines `804-821`, `910`, `918-919`.
   - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs` (lines `1-15`, `670-782`) and `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs` (lines `130-165`).
   - **Gap/Bug:**
     - `RESP`: Upstream sends raw HTTP response headers received from the stream server back to LMS in a `RESP` packet (`opcode = "RESP"`). When `strm.autostart` is `'2'` or `'3'`, LMS waits for `RESP` before sending `cont` or `codc`. `squeezelite-proto` mentions `RESP` in module docs (`lib.rs:7`) but implements no builder for `RESP`, and `HttpAudioStream` discards response headers (`network.rs:394`).
     - `DSCO`: Upstream sends `DSCO` (`DISCONNECT_OK = 0`, `LOCAL_DISCONNECT = 1`, `REMOTE_DISCONNECT = 2`, `UNREACHABLE = 3`, `TIMEOUT = 4`) when the HTTP stream socket closes. Without `DSCO`, LMS does not know the player finished downloading the stream.
     - `META`: Upstream sends ICY / Ogg stream metadata changes via `META`.
     - `SETD`: Upstream responds to `setd` (id `0`) player-name queries/updates via `sendSETDName`.

2. **[CRITICAL] Missing `STMu` (Track Underrun / End-of-Track) vs `STMl` (Buffer Threshold Reached) Confusion**
   - **C Baseline:** `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/slimproto.c` lines `825-845` (`STMl`, `STMd`, `STMn`) and lines `872-884` (`STMu`, `STMo`).
   - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs` lines `577-611` (`StatEvent`) and `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs` lines `451`, `552`, `571-580`.
   - **Gap/Bug:**
     - In SlimProto:
       - `STMf` = stream/output flushed
       - `STMc` = stream connect initiated
       - `STMl` = buffer threshold reached (used when `autostart == 0` or `2` so LMS knows player is buffered and ready to unpause)
       - `STMd` = decode complete / ready for next track (triggers LMS gapless next-track `strm` `'s'`)
       - `STMn` = decoder error / format not supported
       - `STMs` = output track started playing
       - `STMu` = output buffer underrun after decode complete (signals end-of-track to LMS)
       - `STMo` = output buffer underrun while still streaming
     - In `squeezelite-proto/src/lib.rs:585, 598`, `StatEvent::StreamUnderrun` is mapped to `b"STMl"`, and **`b"STMu"` and `b"STMo"` do not exist in `StatEvent` at all**!
     - In `coordinator.rs:451, 571-580`, when a stream reaches EOF (`n == 0`), it sets `self.pending_underrun = true` and immediately sends `StatEvent::StreamUnderrun` (`STMl`) on the next tick—even while decoded audio frames are still in `output_buffer`, and never sends `STMd` (when decode finishes) or `STMu` (when `output_buffer` drains). Consequently, LMS never receives `STMd` or `STMu` to advance to the next track in a playlist.

3. **[HIGH] Premature `STMs` (`TrackStarted`) Emission Before HTTP Stream Connect / Output Start**
   - **C Baseline:** `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/slimproto.c` lines `449`, `473`, `858-862` (`output.track_started` triggers `_sendSTMs = true`).
   - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs` lines `244-273`, `554-569`.
   - **Gap/Bug:** On `strm` `'s'`, C sends `STMf` and `STMc`, waits for decoding and output buffering, and only sends `STMs` when the output stage actually begins consuming the new track's frames (`output.track_started`). In `coordinator.rs:254-261`, when `effective_threshold == 0` and `autostart == true`, `STMs` is sent synchronously inside `handle_command` before a single byte is read from the HTTP stream or decoded. And if `autostart == false` (`'0'`), `coordinator.rs:264-270` sends `STMp` immediately instead of waiting for the stream buffer to reach `threshold` and sending `STMl` (`slimproto.c:828-831`).

4. **[HIGH] `strm` `autostart` Collapses `'0'..'3'` into `bool`, Losing `cont_wait` (`'2'`, `'3'`)**
   - **C Baseline:** `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/slimproto.c` lines `447`, `457`, `471`, `501-521`, `828-836`.
   - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs` line `417`.
   - **Gap/Bug:** `let autostart = payload[1] == b'1' || payload[1] == b'3';` discards whether `autostart` was `'2'` or `'3'` (which require sending `RESP` and waiting for the server `cont` command before starting decoding/output).

5. **[HIGH] `strm` `'t'` (Timestamp / Latency Probe) Ignored and `STAT.server_timestamp` Hardcoded to `0`**
   - **C Baseline:** `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/slimproto.c` lines `234`, `371-373`.
   - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs` line `338`, `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs` lines `137-157`, `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs` line `778`.
   - **Gap/Bug:** LMS periodically sends `strm` with `command = b't'` and places a server timestamp in the `replay_gain` field (`offset 14..18`). The player must respond with a `STAT` (`"STMt"`) packet echoing that exact 32-bit value in `STAT.server_timestamp` (`bytes 45..49` of the `STAT` payload). In `coordinator.rs:338`, `StreamCommandType::Timestamp` falls into `_ => Ok(None)` (no `STMt` reply sent), and `SlimprotoBuilder::build_stat_full` hardcodes `server_timestamp: 0`. This breaks LMS clock-drift / RTT calculation and multi-player synchronization.

6. **[HIGH] `STAT` Packet `elapsed_seconds`, `elapsed_ms`, and `bytes_received` Miscomputed in `SlimprotoBuilder::build_stat_full`**
   - **C Baseline:** `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/slimproto.c` lines `176-237` (`sendSTAT`).
   - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs` lines `641-666` (`StatBuilder::build_stat`) and lines `758-781` (`SlimprotoBuilder::build_stat_full`), `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs` lines `137-157`.
   - **Gap/Bug:**
     - `SlimprotoBuilder::build_stat_full` sets `bytes_received: frames_played as u64` instead of the cumulative HTTP stream bytes received (`status.stream_bytes`), and sets `elapsed_seconds: timestamp_ms / 1000` and `elapsed_ms: timestamp_ms`, where `timestamp_ms` passed from `coordinator.rs` is `now_ms` (monotonic system uptime / jiffies), NOT track elapsed playback time! As a result, the LMS web UI progress bar jumps to the ESP32's system uptime instead of track position `(frames_played * 1000) / current_sample_rate`.
     - Furthermore, `StatBuilder::build_stat` (`lib.rs:647`) hardcodes `44_100` Hz when computing `elapsed_ms` instead of using the active track's `sample_rate`.

7. **[MEDIUM] `strm` `'p'` (Timed Pause), `'u'` (Timed Unpause `start_at`), and `'a'` (Skip-Ahead) `replay_gain` Interval Ignored**
   - **C Baseline:** `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/slimproto.c` lines `396-435` and `output.c` lines `69-109`.
   - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs` lines `275-312`, `338`.
   - **Gap/Bug:**
     - In `strm` `'p'`, if `replay_gain != 0`, it specifies a pause duration in milliseconds (`OUTPUT_PAUSE_FRAMES` inserting silence frames without sending `STMp`). `coordinator.rs:275` always pauses indefinitely and sends `STMp`.
     - In `strm` `'u'`, if `replay_gain != 0`, it specifies a target `jiffies` timestamp (`OUTPUT_START_AT`) at which to unpause.
     - In `strm` `'a'`, `replay_gain` specifies milliseconds of audio frames to skip ahead (`OUTPUT_SKIP_FRAMES`). `coordinator.rs:338` ignores `StreamCommandType::SkipAhead`.

8. **[MEDIUM] `audg` Digital Attenuation (`adjust`) Flag & `strm.replay_gain` / `strm.flags` Polarity/Channel Mode Ignored**
   - **C Baseline:** `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/slimproto.c` lines `488-489`, `555-571` and `output.c` lines `54-58`.
   - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs` lines `341-355`.
   - **Gap/Bug:**
     - In `slimproto.c:569`, `audg->adjust` (`digital_atten` in `ServerCommand::AudioGain`) gates whether `gain_left`/`gain_right` are applied; when `digital_atten == false` (fixed-volume output mode), gain must be unity (`0x10000`). `coordinator.rs:341` ignores `digital_atten`.
     - In `output.c:54-57`, `strm.replay_gain` is multiplied with `gainL`/`gainR` (`gain(output.gainL, output.current_replay_gain)`), and `strm.flags & 0x03` controls left/right phase polarity inversion (`invert`) while `(strm.flags & 0x0c) >> 2` controls stereo/mono/left/right channel routing. `coordinator.rs` ignores `replay_gain` and `flags`.

9. **[MEDIUM] Unhandled Server Commands in `coordinator.rs` (`aude`, `cont`, `codc`, `setd`, `serv`, `vers`)**
   - **C Baseline:** `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/slimproto.c` lines `501-651`.
   - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs` line `385`.
   - **Gap/Bug:** `squeezelite-proto` provides parser helpers on `ServerCommand::Unknown` (`parse_aude`, `parse_cont`, `parse_setd_player_name`, `parse_serv`, `parse_vers`), but `coordinator.rs:385` matches `ServerCommand::Unknown { .. } => Ok(None)`, ignoring:
     - `aude` (DAC / S/PDIF enable/disable — should mute/sleep or wake the TAS5805M amplifier),
     - `cont` (stream continuation & `metaint` update for `autostart` 2/3),
     - `setd` (player name query/update),
     - `serv` (LMS server switch / multi-room sync group migration).

10. **[LOW] `HELO` Reconnect Bit (`0x4000`) & Stream Byte Count Not Populated**
    - **C Baseline:** `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/slimproto.c` lines `140-174`.
    - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs` lines `675-703`.
    - **Gap/Bug:** On reconnect after a dropped TCP session, upstream sets `wlan_channellist = 0x4000` (bit 14 reconnect indicator) and populates `bytes_received` in `HELO` so LMS can resume without interrupting playback (`STOP_SERVER_REBOOT` / reconnect handling). `SlimprotoBuilder::build_helo` zeroes bytes `16..44`.

---

### Category B: HTTP Audio Streaming (`louder-esp32-app/src/network.rs` vs `components/squeezelite/stream.c`)

11. **[CRITICAL] Missing ICY Metadata (`icy-metaint`) Stripping in `HttpAudioStream`**
    - **C Baseline:** `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/stream.c` lines `446-521`.
    - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs` lines `318-424`.
    - **Gap/Bug:** When playing internet radio streams (Icecast/Shoutcast) or LMS-proxied streams with metadata (`Icy-MetaData: 1`), the HTTP response includes `icy-metaint: <N>` (or LMS sends `metaint` in `cont`). Every `N` bytes of audio payload, the stream embeds 1 length byte `L` followed by `L * 16` bytes of ASCII metadata (`StreamTitle='...';`). `HttpAudioStream` does not parse `icy-metaint` or strip inline metadata blocks; it feeds the raw ASCII metadata bytes directly into the audio ring buffer, corrupting MP3/AAC/Ogg frames every `N` bytes and failing to forward metadata to LMS (`META`) or the CYD display.

12. **[HIGH] No HTTP 301/302/307/308 Redirect Handling or `Transfer-Encoding: chunked` Support**
    - **C Baseline:** `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/stream.c` (and LMS `RESP` redirect handling where LMS follows redirects if `RESP` 301/302 headers are returned to LMS, or client-side redirect handling).
    - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs` lines `380-405`.
    - **Gap/Bug:** `HttpAudioStream::poll_audio_bytes` immediately errors with `InvalidData ("Non-200 HTTP response")` on any HTTP status other than `200` or `206` (`network.rs:384`). Because `RESP` is also not sent to LMS (Finding #1), neither LMS nor `HttpAudioStream` can follow `301`/`302`/`307`/`308` redirects. Additionally, if an HTTP/1.1 server responds with `Transfer-Encoding: chunked`, chunk-size hex lines (`<hex>\r\n`) are not stripped before entering `stream_buffer`.

13. **[MEDIUM] Non-Blocking TCP `connect` Needed on Embedded Targets & `Host` Header Injection**
    - **C Baseline:** `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/stream.c` lines `292-331`, `555-626`.
    - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs` lines `332-357`.
    - **Gap/Bug:** `HttpAudioStream::connect` calls `TcpStream::connect_timeout(&addr, Duration::from_secs(3))` synchronously on the caller thread (`handle_command`), which can block the audio/coordinator loop for up to 3 seconds on slow radio servers. Also, `stream.c:601-618` appends `Host: <ip>:<port>\r\n` if not already present in the `strm` HTTP request headers.

---

### Category C: Audio Pipeline & Decoders (`squeezelite-decoders`, `squeezelite-audio-core`, `louder-esp32-app/src/pipeline.rs`)

14. **[CRITICAL] `VorbisDecoder` Re-Initializes `OggStreamReader::new` From Byte 0 on Every Decode Call**
    - **C Baseline:** `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/vorbis.c` lines `147-343`.
    - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs` lines `833-910`.
    - **Gap/Bug:** In `VorbisDecoder::decode` (`lib.rs:849`), `OggStreamReader::new(Cursor::new(&self.stream_buf))` is reconstructed from scratch on **every single `decode()` invocation**, re-parsing the 3 Vorbis identification/comment/setup headers and decoding one packet, then draining `self.header_len..consumed` (`lib.rs:902`). This has three severe bugs:
      1. **Broken MDCT overlap-add state:** Vorbis synthesis requires preserving the previous packet's right-hand window half (`pwr` overlap state inside `lewton`) to add with the current packet's left-hand window half. Recreating `OggStreamReader` on every packet zeroes the overlap buffer on every packet, causing audible clicks/discontinuities at every Vorbis packet boundary (~20ms).
      2. **Broken Ogg page framing after first page drain:** `OggStreamReader` reads Ogg pages (`OggS`), not raw Vorbis packets. Once `self.stream_buf.drain(self.header_len..consumed)` removes a partial Ogg page or page header, subsequent calls to `OggStreamReader::new` see corrupted Ogg page boundaries after `self.header_len`.
      3. **CPU/Heap thrashing:** Re-parsing the Vorbis codebook setup header (several KB of Huffman/VQ tables) on every 20ms audio frame will exhaust ESP32 CPU cycles and heap allocator bandwidth.

15. **[CRITICAL] `FlacDecoder` Re-Initializes `FlacReader::new` From Byte 0 on Every Decode Call with 1-Byte Reads**
    - **C Baseline:** `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/flac.c` lines `180-310`.
    - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs` lines `435-618`.
    - **Gap/Bug:**
      1. `FlacByteReader::read` (`lib.rs:445`) clamps every read to `.min(1)`—forcing `claxon` to issue one virtual `Read::read` call per single byte!
      2. `FlacDecoder::decode` (`lib.rs:513`) reconstructs `FlacReader::new` from the start of `self.stream_buf` (re-parsing `fLaC` + `STREAMINFO` metadata blocks) on every single frame decode, then drains `self.header_len..last_good_pos`.
      3. `FlacDecoder` should parse the `STREAMINFO` header once on stream start, and then use `claxon::frame::FrameReader` directly on the streaming byte buffer with bulk slice reads.

16. **[HIGH] Dynamic Sample Rate Discovered by Decoders (`FLAC`, `AAC`, `Vorbis`) Never Propagated to `StreamPipeline`**
    - **C Baseline:** `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/flac.c` line `256`, `vorbis.c` line `226`, `mad.c` line `312`, `helix-aac.c` line `417` (`output.next_sample_rate = decode_newstream(...)`).
    - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs` lines `148-192` and `coordinator.rs` lines `199-234`.
    - **Gap/Bug:** For compressed formats (`flc`, `aac`, `ogg`, `mp3`), LMS frequently sends `pcm_sample_rate = b'?'` in the `strm` packet because the container header carries the true sample rate (e.g., 48kHz, 88.2kHz, or 96kHz FLAC). In `coordinator.rs:199`, `pcm_sample_rate = b'?'` defaults to `44100` Hz. Although `FlacDecoder::sample_rate`, `AacDecoder::sample_rate`, and `VorbisDecoder::sample_rate` are updated when their headers are parsed, `ActiveDecoder` does not expose `sample_rate()`, and `StreamPipeline::process_decode_step` (`pipeline.rs:148-192`) never updates `self.sample_rate`, `self.equalizer`, or `self.output_sink.set_sample_rate(...)`!

17. **[MEDIUM] `FlacDecoder` 8-Bit Sample Left-Shift is `16` Instead of `24`**
    - **C Baseline:** `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/flac.c` lines `31`, `212` (`#define ALIGN8(n) (n << 24)`).
    - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs` lines `529-534`.
    - **Gap/Bug:** `let shift = match bps { 16 => 16, 24 => 8, 32 => 0, _ => 16 };` shifts 8-bit FLAC samples by `<< 16` instead of `<< 24`, making 8-bit FLAC streams 48 dB too quiet.

18. **[MEDIUM] `PcmDecoder` Missing WAV (`RIFF`/`WAVE`) & AIFF (`FORM`/`AIFF`) Container Header Stripping**
    - **C Baseline:** `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/pcm.c` lines `79-181` (`_check_header`).
    - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs` lines `96-308`.
    - **Gap/Bug:** When LMS streams a `.wav` or `.aif` file using format `'p'`, `pcm.c` checks the initial bytes for `RIFF....WAVE` (parsing `fmt ` sample rate/channels/bits and skipping to `data`) or `FORM....AIFF` (parsing `COMM` and skipping to `SSND`). `PcmDecoder` treats the WAV/AIFF header bytes as raw PCM samples (producing a loud click at track start and potentially wrong endianness/rate if the WAV header overrides `strm` fields).

19. **[MEDIUM] `PcmDecoder` 88.2 kHz (`b':'`) Sample Rate Code Missing in `decode_pcm_sample_rate_hz`**
    - **C Baseline:** `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/pcm.c` lines `65-67`, `442`.
    - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs` lines `89-101`.
    - **Gap/Bug:** In SlimProto `pcm.c:65-67`, `sample_rates[rate - '0']` maps index 10 (`b':'`, ASCII 58) to `88200` Hz (`88.2 kHz`). `coordinator.rs:89-101` only maps `0..=9` (`b'0'..=b'9'`) and falls back to `44100` Hz for `88200` Hz (`b':'`).

20. **[LOW] MP3 Gapless Playback (`Xing`/`Info`/`LAME` `enc_delay` & `enc_padding`) Not Stripped**
    - **C Baseline:** `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/mad.c` lines `123-166`, `280-306`.
    - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs` lines `338-425`.
    - **Gap/Bug:** `Mp3Decoder` strips `ID3v2` tags (`lib.rs:348-378`), but does not inspect the first MPEG frame for `Xing`/`Info`/`LAME` VBR/gapless headers to skip the encoder delay (`enc_delay` + 528 samples) and trailing padding (`enc_padding`).

---

### Category D: TAS5805M Driver & Hardware Target Wiring (`squeezelite-tas58xx`, `louder-esp32-app`, `cyd-controller-app`)

21. **[CRITICAL] `louder-esp32-app/src/board.rs` Uses Wrong Default I2C Address (`0x4C` instead of `0x2C`) & Wrong Pinout for Sonocotta Louder-ESP32**
    - **Upstream Reference:** `sonocotta/esp32-audio-dock` (Louder-ESP32 and Louder-ESP32-S3 hardware schematics & firmware).
    - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/board.rs` lines `12-36` vs `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs` lines `17-19` and `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-config/src/lib.rs` lines `147-160`.
    - **Gap/Bug:**
      - On the Sonocotta Louder-ESP32 / Louder-ESP32-S3 (`TAS5805M`), the 7-bit I2C address is **`0x2C`** (`TAS5805M_I2C_ADDR_DEFAULT = 0x2C` in `squeezelite-tas58xx/src/lib.rs:17`, or `0x2D` depending on the `ADR` resistor), whereas `louder-esp32-app/src/board.rs:35` defines `pub const TAS58XX_I2C_ADDR: u8 = 0x4C;` (which is an invalid/TAS5825M address and isn't even accepted by `squeezelite-config` validation `0x2C..=0x2F` at `squeezelite-config/src/lib.rs:437`!).
      - Furthermore, compare `board.rs` default GPIOs against Sonocotta's hardware pinouts:
        - **Louder-ESP32 (Classic ESP32-WROVER):** `I2S_BCLK = GPIO26`, `I2S_LRCLK = GPIO25`, `I2S_DOUT = GPIO22`, `TAS5805M_PWDN = GPIO21` (or `GPIO19`/`GPIO33` per revision; `squeezelite-config` uses `bck=26, ws=25, data_out=22, sda=21, scl=22/23, enable=19`), whereas `board.rs:13-30` hardcodes `I2S_BCLK_GPIO = 5`, `I2S_LRCLK_GPIO = 25`, `I2S_DOUT_GPIO = 26`, `I2C_SDA_GPIO = 21`, `I2C_SCL_GPIO = 22`, `TAS58XX_PWDN_GPIO = 18`, `TAS58XX_FAULT_GPIO = 19`! Note that `board.rs` should derive pin assignments from `squeezelite-config`'s `HardwareConfig::default_esp32_wrover()` / `HardwareConfig::default_esp32_s3()` (and verify exact Sonocotta Louder-ESP32 vs Louder-ESP32-S3 schematic pins).

22. **[HIGH] TAS5805M Register Initialization Order (`PAGE_SELECT` Before `BOOK_SELECT`, and `Hi-Z` Transition Before `PLAY`)**
    - **Upstream Reference:** TI TAS5805M Datasheet Section 7.5.1 / `sonocotta/esp32-audio-dock` TAS5805M startup sequence.
    - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs` lines `198-221`, `297-322`.
    - **Gap/Bug:**
      1. **Book/Page ordering bug (`lib.rs:199-203`):** `select_book_page` writes `REG_BOOK_SELECT (0x7F)` *before* `REG_PAGE_SELECT (0x00)`. On the TAS5805M I2C register map, register `0x7F` (`BOOK_SELECT`) only exists on **Page `0x00`**! If the chip was previously on a non-zero page, writing `0x7F` before writing `0x00 = 0x00` writes to register `0x7F` of the wrong page instead of switching books. `REG_PAGE_SELECT (0x00) = 0x00` MUST be written first, then `REG_BOOK_SELECT (0x7F) = book`, then `REG_PAGE_SELECT (0x00) = page`.
      2. **Missing `CTRL2_MODE_HIZ (0x02)` staging in `initialize()` (`lib.rs:301-321`):** Per TI TAS5805M startup requirements and `sonocotta/esp32-audio-dock`, after asserting `PWDN` HIGH (with I2S BCLK/LRCLK active) and waiting >= 5ms, the driver must select Page 0 / Book 0, transition `REG_DEVICE_CTRL_2 (0x03)` to **`CTRL2_MODE_HIZ (0x02)`** (waiting ~5ms for internal DSP/LDO stabilization), configure SAP/BTL/volume registers while in `Hi-Z`, clear any startup clock faults (`REG_FAULT_CLEAR (0x78) = 0x80`), and only then transition `REG_DEVICE_CTRL_2 (0x03)` to **`CTRL2_MODE_PLAY (0x03)`**. Currently, `initialize()` resets the chip (`0x01 = 0x11`), configures registers in Deep Sleep, and jumps directly to `CTRL2_MODE_PLAY (0x03)` without staging through `CTRL2_MODE_HIZ (0x02)` or clearing post-lock faults.

23. **[CRITICAL] `louder-esp32-app/src/main.rs` Uses Simulated Loopback Peripherals & Never Connects `SlimprotoClient` / `FirmwareCoordinator` in `run_firmware_tick`**
    - **Rust Comparison:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` lines `43-175` vs `/home/console-admin/Github/squeezelite-esp32/rust/crates/cyd-controller-app/src/main.rs` lines `35-232`.
    - **Gap/Bug:**
      - Unlike `cyd-controller-app/src/main.rs` (which wires real `esp_hal` SPI2, SPI3, UART2, and GPIO pins on `#[cfg(target_arch = "xtensa")]`), `louder-esp32-app/src/main.rs` unconditionally instantiates mock loopback peripherals (`MemFlashPartition`, `TargetI2cBus` with a 256-byte array, `LoopbackI2sDma`, `LoopbackUartPort`) on Xtensa hardware!
      - Furthermore, `run_firmware_tick` in `louder-esp32-app/src/main.rs:88-145` only calls `pipeline.process_decode_step(0)` and `amp.poll_and_recover()`—it never initializes Wi-Fi, never runs `LmsDiscovery::discover_server`, never connects `SlimprotoClient`, never calls `coordinator.handle_command` or `coordinator.poll_stream_to_pipeline`, and never drains CYD UART commands via `cyd_bridge.poll_commands_into_coordinator`!

24. **[MEDIUM] CYD UART Pin Conflict Between `squeezelite-config` Defaults and `board.rs` (`GPIO 17/16` vs PSRAM on ESP32-WROVER)**
    - **Rust Location:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/board.rs` lines `22-25` vs `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-config/src/lib.rs` lines `155-156`.
    - **Gap/Bug:** On `ESP32-WROVER-N8R8` (3.3V Octal/Quad PSRAM module), `GPIO 16` and `GPIO 17` are internally reserved for PSRAM (`CS#` and `CLK`)! `squeezelite-config/src/lib.rs:155-156` correctly uses `cyd_uart_tx: 4, cyd_uart_rx: 36` for `default_esp32_wrover()`, whereas `louder-esp32-app/src/board.rs:23-24` defines `CYD_UART_TX_GPIO: u8 = 17` and `CYD_UART_RX_GPIO: u8 = 16`, which will crash/corrupt PSRAM on an `ESP32-WROVER` module if configured as UART2 pins.
