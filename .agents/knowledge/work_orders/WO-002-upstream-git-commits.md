# Work Order: WO-002 — Upstream Git Branches Commit-by-Commit Audit (`upstream/master-v4.3`, `upstream/master`, `origin/My_Version`)

**Work Order ID:** WO-002  
**Timestamp:** 2026-10-20T20:22:30-04:00  
**Assigned Subagent Role:** Research Specialist (Upstream Git Commits)  
**Target Files:** `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-002-upstream-git-commits.md`  
**Backup Files:** `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/WO-002-upstream-git-commits.md.bak`  
**Defect IDs:** N/A  
**Task Goal:** Audit all commits in `MK5-Rust..upstream/master-v4.3`, `MK5-Rust..upstream/master`, and `MK5-Rust..origin/My_Version` to identify upstream bug fixes, patches, updates, and upgrades needed in `MK5-Rust` (both in the pure-Rust `rust/` crates and the C tree).  

---

## 1. Context & Scope
- **Problem Statement:** `MK5-Rust` diverged from `upstream/master-v4.3` at merge-base `bd0e0702b0fb3b59ec65a068285bc04126c476e2`. Since then, `upstream/master-v4.3` has accumulated 47 non-merge commits (up to `ee1720c7`, `build-number-1780`), and `upstream/master` and `origin/My_Version` also have commits. We must inspect every upstream commit and classify whether its fix/feature is needed in `MK5-Rust` (`rust/crates/*` and/or C tree).
- **Single Variable Constraint (Rule 5):** Read-only analysis of repository code; only modify `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-002-upstream-git-commits.md` to record fulfillment.
- **Pre-Modification Integrity (Rule 31):** Do not modify any source code files.
- **Prohibited Actions (Rule 23, Rule 24):** Do NOT use `sed`, `awk`, or `cat` for file modifications. Use strict absolute paths (`/home/console-admin/Github/squeezelite-esp32`).
- **Verification Requirement (Rule 21, Rule 52):** Cite exact commit hashes, file paths, and line numbers for every finding.

---

## 2. Hypothesis & Predicted Outcome (Rule 1, Rule 6)
- **Hypothesis:** Several upstream commits in `bd0e0702..upstream/master-v4.3` and `upstream/master` fix real protocol, reconnect, HTTP redirect, trackId, stream-open failure, equalizer, or ESP32-S3 bugs that have direct equivalents or missing behaviors in `MK5-Rust` (`rust/crates/*` and/or C tree).
- **Predicted Outcome:** A complete commit-by-commit classification table identifying which upstream commits are (A) applicable to `rust/crates/*`, (B) applicable to the C tree on `MK5-Rust`, or (C) already fixed / not applicable.
- **Falsification Criteria:** Every upstream commit is purely CI/README/prebuilt-binary noise with zero functional impact.

---

## 3. Detailed Instructions
1. Use `git -C /home/console-admin/Github/squeezelite-esp32` to inspect the diffs of all functional commits in `bd0e0702b0fb3b59ec65a068285bc04126c476e2..upstream/master-v4.3` (including `a66f0ed8`, `caabf166`, `aa4a3557`, `90b696a1`, `1a90d518`, `cdc02474`, `de9fe7fc`, `370b799b`, `d3e86f47`, `c08d8278`, `174a6cc2`, `b86122f3`, `aec68d07`, `6c863cbe`, `5ee507eb`, `df6348e2`, `24ab9999`, `08bd83e8`, `745ff233`, `e89c3c9b`, `eae16839`, `feba031b`, etc.), `MK5-Rust..upstream/master`, and `MK5-Rust..origin/My_Version`.
2. Cross-check each upstream patch against:
   - The pure-Rust implementation in `/home/console-admin/Github/squeezelite-esp32/rust/crates/` (`squeezelite-proto`, `squeezelite-audio-core`, `squeezelite-tas58xx`, `squeezelite-decoders`, `squeezelite-cyd-link`, `squeezelite-config`, `louder-esp32-app`, `cyd-controller-app`).
   - The C implementation on `MK5-Rust` (`/home/console-admin/Github/squeezelite-esp32/components/`, `/home/console-admin/Github/squeezelite-esp32/main/`).
3. Append the complete findings and Fulfillment Section to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-002-upstream-git-commits.md` using `edit_file`.

---

## 4. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Execution Timestamp:** 2026-10-20T20:31:00-04:00
- **Files Modified / Created (Absolute Paths):**
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-002-upstream-git-commits.md`
- **Backup Verification:** Verified `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/WO-002-upstream-git-commits.md.bak` exists prior to editing.
- **Test / Verification Command & Results:**
  - `git -C /home/console-admin/Github/squeezelite-esp32 log --oneline --no-merges bd0e0702b0fb3b59ec65a068285bc04126c476e2..upstream/master-v4.3` -> 47 non-merge commits (56 total with 9 merges).
  - `git -C /home/console-admin/Github/squeezelite-esp32 log --oneline --no-merges MK5-Rust..upstream/master` -> 66 non-merge commits (82 total).
  - `git -C /home/console-admin/Github/squeezelite-esp32 log --oneline --no-merges MK5-Rust..origin/My_Version` -> 4 commits.
  - `git -C /home/console-admin/Github/squeezelite-esp32 show --stat` and full diff inspection across all functional commits.
- **Observed Outcome vs Predicted Outcome:** Confirmed hypothesis: out of 47 non-merge commits in `bd0e0702..upstream/master-v4.3`, 24 are CI/README/prebuilt-binary noise and 23 are functional commits. Several critical fixes in `upstream/master-v4.3`, protocol extensions in `upstream/master`, and 2 bug fixes in `origin/My_Version` are missing from `MK5-Rust` (across both the pure-Rust `rust/crates/*` workspace and the C tree).
- **Fulfillment Status:** `[EXECUTED & VERIFIED]`
- **Summary Path:** N/A (Read-Only repository audit; only work order file updated)
- **Handoff Path:** `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-002-upstream-git-commits.md`

---

## 5. Comprehensive Commit-by-Commit Audit Findings

### 5.1 Executive Summary of Actionable Upstream Items for `MK5-Rust`

#### A. Actionable Items for Pure-Rust Crates (`/home/console-admin/Github/squeezelite-esp32/rust/crates/`)
1. **HTTP Audio Stream Open Failure & Mid-Stream HTTP Error Handling (cf. `90b696a1`, `08bd83e8`, `5ee507eb`)**:
   - **Location**: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs:192-196` and `414-417`, `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs:273-300, 373-388`.
   - **Gap**:
     - When `server_port > 0` during `StreamCommandType::Start`, if `HttpAudioStream::connect(...)` returns `Err(_)` (e.g., slow Wi-Fi or connection refused, cf. `90b696a1`), `coordinator.rs:192-196` silently leaves `self.audio_stream = None` and transitions into `PlayerState::Buffering` (where it deadlocks forever because `is_eof` is `false` when `audio_stream.is_none()`) or `PlayerState::Playing` (emitting a false `STMs` `TrackStarted` event instead of aborting/reporting stream connection failure).
     - Similarly, if `poll_http_stream` (`coordinator.rs:414-417`) encounters an HTTP read/status error (such as a non-200 status or broken socket), it sets `self.audio_stream = None` and returns `Err(e)`, which `process_audio_step` (`coordinator.rs:460`) ignores via `let _ = self.poll_http_stream(pipeline);`, leaving `self.state` stuck in `Buffering` forever if `stream_buffer` has not reached `buffer_threshold_bytes`.
2. **Ogg FLAC (`AudioFormat::Flac` in Ogg Container) & Vorbis Header Comment Bounds (cf. `c08d8278`, `370b799b`)**:
   - **Location**: `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs:514-540` (`FlacDecoder`) and `806-930` (`VorbisDecoder`).
   - **Assessment**:
     - `VorbisDecoder` uses pure-Rust `lewton::inside_ogg::OggStreamReader` with `MAX_STREAM_BUF` (64 KB) bounds checking (`lib.rs:806`), so it is immune to the C `stream.c:287-309` 8-bit pointer dereference bug (`*p` instead of `read_le32(p)`) and 2-byte `MAX_HEADER` overflow fixed in `c08d8278`.
     - However, `FlacDecoder` (`lib.rs:514`) uses `claxon::FlacReader::new` which only parses raw native `fLaC` streams, not Ogg-encapsulated FLAC (`OggS` header with `pcm_sample_size == b'o'`, cf. `slimproto.c:470` and `c08d8278`).
3. **Slimproto Custom Equalizer/Loudness/Audio-Output Opcodes (`eqlz`, `loud`, `audo`) & `HELO` Capabilities (cf. `5e11ccbd`, `6b368a7c`, `f08bde8d`, `174a6cc2`)**:
   - **Location**: `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs:241-332, 394-406`, `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs:385`, `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs:78`.
   - **Gap**:
     - `squeezelite-audio-core` already implements a 32-bit-capable 10-band `Equalizer<10>` and `horner_loudness` (`/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/equalizer.rs:50-189`), and is immune to the uninitialized `loudness_gain` bug fixed in C by `174a6cc2`.
     - However, `squeezelite-proto` and `PlayerCoordinator::handle_command` do not yet parse or wire the SqueezeESP32 `eqlz` (10-band `i8` gains) or `loud` (`u8` loudness) opcodes into `pipeline.equalizer`.

---

#### B. Actionable Items for the C Codebase on `MK5-Rust` (`/home/console-admin/Github/squeezelite-esp32/components/` & `main/`)

| Priority | Upstream Commit(s) | Subsystem / Target Files on `MK5-Rust` | Defect / Missing Update on `MK5-Rust` |
|---|---|---|---|
| **P0 (Memory Corruption / Crash)** | `174a6cc2`, `c08d8278` | `/home/console-admin/Github/squeezelite-esp32/components/raop/rtp.c:411-431` | `alac_decode(u16_t *outsize)` casts `u16_t *outsize` (`&abuf->len`) to `(unsigned int *)outsize` when calling `alac_to_pcm()`, causing a **32-bit write into a 16-bit struct field** and corrupting adjacent heap memory. |
| **P0 (DMA Heap Overflow)** | `aa4a3557` | `/home/console-admin/Github/squeezelite-esp32/components/display/ILI9341.c:96,163,272`<br>`/home/console-admin/Github/squeezelite-esp32/components/display/SSD1351.c:86,153,212`<br>`/home/console-admin/Github/squeezelite-esp32/components/display/ST77xx.c:93,160,236` | Compares `optr - Private->iRAM < PAGE_BLOCK` (4096) instead of `optr - Private->iRAM <= Private->iRAMSize - ChunkSize`. When `Private->iRAMSize < PAGE_BLOCK`, `memcpy` overflows the internal DMA buffer (`issue #558`). |
| **P0 (Stack Overflow / Crash)** | `a66f0ed8` | `/home/console-admin/Github/squeezelite-esp32/components/telnet/telnet.c:559-562` | `stdout_fstat(int fd, struct stat *st)` only sets `st->st_mode = S_IFCHR;` without `memset(st, 0, sizeof(*st));`. Uninitialized `st->st_blksize` causes newlib `vfprintf` to allocate an arbitrary buffer size, crashing task stacks when telnet is active. |
| **P0 (Use-After-Free on Task Exit)** | `c08d8278`, `d3e86f47` | `/home/console-admin/Github/squeezelite-esp32/components/spotify/cspot/bell/main/utilities/include/BellTask.h:40-97` | `bell::Task::~Task()` immediately calls `heap_caps_free(xStack)` while the FreeRTOS task is still executing `vTaskDelete(NULL)`. Upstream defers freeing `xStack` and `xTaskBuffer` via a 5000ms FreeRTOS one-shot timer (`xTimerCreate` + `xTimerDelete` in `deleteTaskCallback`). |
| **P1 (Audio Corruption / White Noise)** | `6c863cbe` | `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/decode_external.c:84-116` | Missing `bytes &= ~0x03;` 4-byte sample alignment, checks `if (len && !space)` instead of `if (len && (!space || !bytes))`, and advances `_buf_inc_writep(outputbuf, space)` on timeout with unaligned `space`, causing stereo phase/byte misalignment (white noise) on cspot/Bluetooth/AirPlay sinks. |
| **P1 (Ogg/FLAC Stream Header Bug)** | `c08d8278`, `370b799b` | `/home/console-admin/Github/squeezelite-esp32/components/squeezelite/stream.c:239-240, 287-309` | Lines 290 and 295 dereference `char *p` as `*p` (8-bit signed `char`) instead of `read_le32(p)` (32-bit little-endian `u32_t`) when parsing Ogg Vorbis/FLAC comment lengths/counts, and line 305 checks `stream.header_len + len > MAX_HEADER` instead of `stream.header_len + len + 2 > MAX_HEADER` (2-byte buffer overflow when appending `\r\n`). Line 240 also has a duplicate `LOG_INFO`. |
| **P1 (Format-String & `va_list` Bugs)** | `174a6cc2`, `c08d8278` | `/home/console-admin/Github/squeezelite-esp32/components/squeezelite-ota/squeezelite-ota.c:226`<br>`/home/console-admin/Github/squeezelite-esp32/components/services/messaging.c:295-308, 332-381` | 1. `squeezelite-ota.c:226` calls `messaging_post_message(type, MESSAGING_CLASS_OTA, json_msg)` without `"%s"`, exposing a format-string vulnerability if `json_msg` contains `%`.<br>2. `messaging.c:300,307` (`vmessaging_post_message`) reuses `va` across `vsnprintf` and `vsprintf` without `va_copy`.<br>3. `messaging.c:332-381` skips `va_end(va)` if `malloc_init_external` returns `NULL`. |
| **P1 (Unintended Recovery Mode Boot-Loop)** | `745ff233` | `/home/console-admin/Github/squeezelite-esp32/components/platform_console/app_squeezelite/cmd_squeezelite.c:85-91`<br>`/home/console-admin/Github/squeezelite-esp32/components/tools/platform_esp32.h:35` | `squeezelite_restart_thread()` calls `esp_restart()` without resetting `halSTORAGE_RebootCounterUpdate(0);`. Five intentional LMS/plugin restarts trigger NVS recovery partition boot. |
| **P1 (cspot Stability & Track Queue Fixes)** | `aec68d07`, `1a90d518`, `90b696a1`, `de9fe7fc`, `cdc02474`, `24ab9999`, `df6348e2`, `caabf166` | `/home/console-admin/Github/squeezelite-esp32/components/spotify/Shim.cpp`<br>`/home/console-admin/Github/squeezelite-esp32/components/spotify/cspot/...` | 8 upstream commits fixing: (1) `PlainConnection.cpp` `EINTR` `break` -> `continue` packet corruption (`aec68d07`); (2) `cJSON_Parse` buffer over-read on `std::string_view` -> `cJSON_ParseWithLength` & non-recursive `MercurySession::reconnect()` (`1a90d518`); (3) `TrackPlayer.cpp` uncaught exception on slow Wi-Fi stream open (`90b696a1`); (4) `Shim.cpp` AP connect retry loop (`de9fe7fc`, `cdc02474`); (5) `TrackQueue` `abandonned` flag, `streamTrackId` vs `playTrackId`, and `capabilities_count` (`24ab9999`, `df6348e2`, `caabf166`). |
| **P2 (ESP32-S3 Build & Console + WM8978)** | `eae16839`, `8829966c`, `b86122f3` | `/home/console-admin/Github/squeezelite-esp32/CMakeLists.txt:61`<br>`/home/console-admin/Github/squeezelite-esp32/components/telnet/telnet.c:143`<br>`/home/console-admin/Github/squeezelite-esp32/components/squeezelite/wm8978/wm8978.c:64` | 1. `CMakeLists.txt:61` hardcodes `xtensa-esp32-elf-objcopy` instead of `${CMAKE_OBJCOPY}`.<br>2. `telnet.c:143` opens `/dev/uart/0` instead of `/dev/console`.<br>3. `wm8978.c:64` writes `102` instead of `6` to register `49`.<br>4. Missing `build-scripts/S3-i2s-sdkconfig.defaults` and `build-scripts/_esp32s3-core-defaults`. |
| **P2 (`origin/My_Version` Memory Leak & IP Change Bug)** | `93480def` | `/home/console-admin/Github/squeezelite-esp32/main/esp_app_main.c:154-164, 446-454` | 1. `esp_app_main.c:446` leaks `bypass_wm` (`config_alloc_get_default`) on normal boot (`else` branch never calls `free(bypass_wm)`).<br>2. `esp_app_main.c:154-164` (`cb_connection_got_ip`) checks `if (ip.addr && ipInfo.ip.addr != ip.addr)` but never assigns `ip.addr = ipInfo.ip.addr;`, so IP address changes are never detected. |

---

### 5.2 Branch 1: `bd0e0702b0fb3b59ec65a068285bc04126c476e2..upstream/master-v4.3` (47 Non-Merge Commits)

#### Table 1A: Functional Commits in `bd0e0702..upstream/master-v4.3` (23 Commits)

| Commit | Subject | Files Touched Upstream | Status in `MK5-Rust` (`rust/` Crates) | Status in `MK5-Rust` (C Tree) |
|---|---|---|---|---|
| `e89c3c9b` | `fix: setting wrong state on reconnect` | `components/wifi-manager/network_manager_handlers.c` | **Already Handled**: `rust/crates/louder-esp32-app/src/wifi.rs:101,148` sets `WifiState::Connecting` on autoconnect/begin_connect. | **Already Fixed**: `components/wifi-manager/network_manager_handlers.c:1109` already uses `Wifi_Active_State[WIFI_CONNECTING_STATE]`. |
| `745ff233` | `fix: avoid recovery after intentional LMS reboot` | `components/platform_console/app_squeezelite/cmd_squeezelite.c`, `components/tools/platform_esp32.h` | **N/A (No Boot Counter Yet)**: `louder-esp32-app` uses dual-sector NVS journal (`hal.rs:447`) without a 5-strike recovery counter. | **NEEDED**: `cmd_squeezelite.c:85-91` calls `esp_restart()` without `halSTORAGE_RebootCounterUpdate(0)`. |
| `08bd83e8` | `Fix two HTTP/telnet crashes` | `components/telnet/telnet.c`, `components/wifi-manager/http_server_handlers.c` | **N/A (Telnet) / Check HTTP**: `louder-esp32-app` has no telnet server; `HttpAudioStream` (`network.rs:373`) bounds headers to 4096B. | **Already Fixed**: `telnet.c:536-542` is non-blocking (`xRingbufferSend(..., 0)`) and `http_server_handlers.c:1155-1159` dynamically sizes redirect buffer. |
| `5ee507eb` | `Fix buffer size calculation for redirect URL` | `components/wifi-manager/http_server_handlers.c` | **N/A**: No captive-portal redirect handler in `rust/crates/*`. | **Already Fixed**: `components/wifi-manager/http_server_handlers.c:1155-1159` uses `strlen(location_prefix) + strlen(ap_ip_address) + 2`. |
| `24ab9999` | `attempt at fixing trackId on natural boundary` | `components/spotify/Shim.cpp`, `cspot` (`TrackPlayer.cpp`, `TrackQueue.cpp/.h`) | **N/A**: Pure-Rust workspace does not include `cspot`. | **NEEDED**: `components/spotify/Shim.cpp` and `cspot` lack `streamTrackId` vs `playTrackId` separation. |
| `df6348e2` | `fix previous commit` | `components/spotify/Shim.cpp` | **N/A**: Pure-Rust workspace does not include `cspot`. | **NEEDED**: Fixes typo `data.trackId` -> `streamTrackId` from `24ab9999` in `components/spotify/Shim.cpp:200`. |
| `eae16839` | `s3 config` | `build-scripts/S3-i2s-sdkconfig.defaults`, `_esp32s3-core-defaults`, `components/telnet/telnet.c` | **Already Supported**: `rust/crates/louder-esp32-app/src/board.rs:25-36` natively supports `board-louder-esp32-s3`. | **NEEDED**: `components/telnet/telnet.c:143` still uses `/dev/uart/0` instead of `/dev/console`; S3 sdkconfig defaults missing. |
| `feba031b` | `Upgrade GitHub workflow container to idf 4.4.8` | `.github/workflows/build.yml` | **N/A**: CI container config for ESP-IDF C build. | **Optional CI Update**: `.github/workflows/build.yml` on `MK5-Rust` still references `idfv43-2:4.3.5`. |
| `9473a63c` | `Disable parallel build ...` | `.github/workflows/build.yml` | **N/A** | **Superseded by `2978b408`**. |
| `2978b408` | `Rollback removal of parallel builds` | `.github/workflows/build.yml` | **N/A** | **Optional CI Update** (pairs with `feba031b`). |
| `6c863cbe` | `fix external decoder mis-alignment (cspot white noise)` | `components/squeezelite/decode_external.c` | **Already Handled**: `rust/crates/louder-esp32-app/src/pipeline.rs:203` & `hal.rs:203` enforce strict 8-byte stereo frame alignment (`HALNET-402`). | **NEEDED**: `components/squeezelite/decode_external.c:84-116` lacks `bytes &= ~0x03;` and corrupts ringbuffer alignment on timeout. |
| `aec68d07` | `fix PlainConnection.cpp (blocking changes)` | `components/spotify/cspot/bell/.../PlainConnection.cpp` | **N/A**: `rust/crates/louder-esp32-app/src/network.rs:101` properly continues on `ErrorKind::Interrupted`. | **NEEDED**: `PlainConnection.cpp:113` executes `break` instead of `continue` on `EINTR`/`EAGAIN`, truncating packets. |
| `b86122f3` | `see CHANGELOG` (WM8978 stereo + CMake objcopy) | `CMakeLists.txt`, `components/squeezelite/wm8978/wm8978.c`, `plugin/SqueezeESP32.zip` | **N/A**: Pure-Rust crates target TAS5805M/TAS5825M (`squeezelite-tas58xx`). | **NEEDED**: `CMakeLists.txt:61` hardcodes `xtensa-esp32-elf-objcopy` (should be `${CMAKE_OBJCOPY}`); `wm8978.c:64` has `102` instead of `6`. |
| `8829966c` | `Add files via upload` | `build-scripts/S3-i2s-sdkconfig.defaults` | **N/A** | **NEEDED** (for ESP32-S3 C builds): updates `S3-i2s-sdkconfig.defaults`. |
| `174a6cc2` | `various bug fixes around compiler changes...` | `messaging.c`, `rtp.c`, `squeezelite-ota.c`, `equalizer.c`, `Shim.cpp`, `telnet.c` | **Already Safe**: `squeezelite-audio-core/src/equalizer.rs` initializes all state to `0.0`. | **PARTIALLY NEEDED**: `equalizer.c:86,100` and `telnet.c:496` are already fixed on `MK5-Rust`, but **`rtp.c:423` (16-bit pointer corruption)**, **`squeezelite-ota.c:226` (format string)**, **`messaging.c` (`va_end`/`va_copy`)**, and **`Shim.cpp`** are NOT fixed on `MK5-Rust`. |
| `c08d8278` | `Wait 5 seconds to free stack from PSRAM when deleting Bell task` | `rtp.c`, `messaging.c`, `BellTask.h`, `stream.c` | **Check Ogg FLAC**: `VorbisDecoder` (`lewton`) is safe; `FlacDecoder` (`claxon`) only supports native `fLaC`, not Ogg-wrapped FLAC. | **NEEDED**: Fixes `rtp.c:411-431` (`alac_decode` `u16_t` return), `stream.c:287-309` (`read_le32(p)` Ogg comment parsing + `MAX_HEADER` +2 check), `messaging.c` `va_end`, and `BellTask.h` deferred stack free. |
| `d3e86f47` | `Delete timer when callback is fired` | `components/spotify/cspot/bell/main/utilities/include/BellTask.h` | **N/A** | **NEEDED**: Adds `xTimerDelete(xTimer, 0);` inside `deleteTaskCallback` in `BellTask.h:57` so the one-shot FreeRTOS timer does not leak. |
| `370b799b` | `cleanup comment` | `components/squeezelite/stream.c` | **N/A** | **NEEDED**: Removes duplicate `LOG_INFO("OggS not at expected position...")` at `components/squeezelite/stream.c:240`. |
| `de9fe7fc` | `Try 5 times when first connecting to AP` | `components/spotify/Shim.cpp` | **N/A** | **NEEDED**: Retries `connectWithRandomAp()` up to 5 times in `Shim.cpp:259-277`. |
| `cdc02474` | `remove break` | `components/spotify/Shim.cpp` | **N/A** | **NEEDED**: Removes premature `break` after 5 failed AP attempts in `Shim.cpp:273`. |
| `1a90d518` | `move reconnect from recursive to iterative` | `AccessKeyFetcher.cpp`, `ApResolve.cpp`, `LoginBlob.cpp`, `MercurySession.cpp/.h`, `TrackQueue.cpp` | **N/A** | **NEEDED**: Replaces `cJSON_Parse(sv.data())` with `cJSON_ParseWithLength(sv.data(), sv.size())` and makes `MercurySession::reconnect()` iterative. |
| `90b696a1` | `handle file open failure when wifi is slow` | `components/spotify/cspot/src/TrackPlayer.cpp` | **ACTIONABLE EQUIVALENT**: `rust/crates/louder-esp32-app/src/coordinator.rs:192-196` ignores `HttpAudioStream::connect` `Err`, leaving player wedged in `Buffering` or false `Playing`. | **NEEDED**: Wraps `track->getAudioFile()` and `currentTrackStream->openStream()` in `try/catch` in `TrackPlayer.cpp:176-188`. |
| `aa4a3557` | `fixe https://github.com/sle118/squeezelite-esp32/issues/558` | `components/display/ILI9341.c`, `SSD1351.c`, `ST77xx.c` | **Already Safe**: `rust/crates/cyd-controller-app/src/drivers.rs:310-326` uses a fixed `[u8; 1024]` stack chunk with exact bounds check (`buf_idx == CHUNK_SIZE`). | **NEEDED**: Fixes DMA heap buffer overflow when `Private->iRAMSize < PAGE_BLOCK` across `ILI9341.c`, `SSD1351.c`, and `ST77xx.c`. |
| `caabf166` | `abandonned track is ignored on track boundaries...` | `PlaybackState.cpp`, `TrackPlayer.cpp`, `TrackQueue.cpp/.h` | **N/A** | **NEEDED**: Marks failed tracks `abandonned = true` so `notifyTrackReached` does not hang, and sets `capabilities_count = capabilityIndex` instead of hardcoded `8` in `PlaybackState.cpp:133`. |
| `a66f0ed8` | `fix telnet crashes` | `components/telnet/telnet.c` | **N/A**: No telnet in `rust/crates/*`. | **NEEDED**: Adds `memset(st, 0, sizeof(*st));` in `stdout_fstat()` at `components/telnet/telnet.c:560`. |

#### Table 1B: Non-Functional Commits in `bd0e0702..upstream/master-v4.3` (24 Commits — `[SKIPPED / UNMODIFIED]` Not Applicable)
- **Automated CI build-number & `components/ehs/libehs.a` binary-only commits (19 commits)**:
  `4d6e8f27`, `f3c7d787`, `2d1ed52d`, `1f393fc4`, `ab53087d`, `8134bdff`, `e4a69df6`, `89799e08`, `f9a0e40f`, `dcde01ae`, `6d0699c6`, `67deef03`, `4da2429c`, `8d25d27b`, `0b609266`, `814b3388`, `fb0b14d7`, `395d8fc6`, `ee1720c7`.
- **README / CHANGELOG / `.gitmodules` URL formatting commits (5 commits)**:
  `a0cc0a99` (`Update README.md`), `4e733b07` (`Update CHANGELOG`), `2d0db912` (`Update README.md`), `161029fd` (`Update README.md`), `4964d215` (`.gitmodules` trailing `.git` URL normalization).

---

### 5.3 Branch 2: `MK5-Rust..upstream/master` (66 Non-Merge Commits / 82 Total)

1. **Legacy ESP-IDF v3.1.5 Firmware Commits (`29a74b9c`..`4517e904`, v0.5.640–v0.5.668)**:
   - **Verification**: Cross-checked `components/squeezelite/stream.c`, `opus.c`, `vorbis.c`, `alac.c`, `buffer.c`, `helix-aac.c`, `pcm.c`, `slimproto.c`, `output_i2s.c`, and `components/services/battery.c` on `MK5-Rust`.
   - **Result**: All C firmware fixes from `upstream/master` (including `f08bde8d` battery `STAT` voltage reporting, `c85b4294` FLAC-in-Ogg, `ac5d00a8` Opus/Vorbis headers, `eb3eb554` AAC wrap, `ce7140d0` ALAC memmove, `50868b6b` PCM shift) were already merged into `master-v4.3` prior to `bd0e0702` and are present in `MK5-Rust`'s C tree.
2. **SqueezeESP32 LMS Plugin Commits (`5e11ccbd`, `6b368a7c`, `3159edc2`, `3c418097`, `c576587d`, `c844ce01`, `a35b5204`, `ee7d2a49`, `3c317b0b`, `64d609cc`)**:
   - **Result**: Modify `plugin/SqueezeESP32/*` (Perl LMS plugin, subsequently removed in `64d609cc`).
   - **Protocol Parity Note for `MK5-Rust`**:
     - In `components/squeezelite/output_embedded.c:54-65`, the C handler for `eqlz` and `loud` packets does not verify packet length (`len >= 14` for `eqlz`, `len >= 5` for `loud`) before reading payload bytes.
     - In `rust/crates/squeezelite-proto/src/lib.rs` and `rust/crates/louder-esp32-app/src/coordinator.rs`, `eqlz` and `loud` opcodes can be parsed and connected to `StreamPipeline::equalizer` (`Equalizer<10>`) and `horner_loudness()`.

---

### 5.4 Branch 3: `MK5-Rust..origin/My_Version` (4 Commits)

| Commit | Subject | Files Touched | Applicability to `MK5-Rust` |
|---|---|---|---|
| `85e27ab3` | `Snapshot clean working tree before pull` | `.vscode/*`, `sdkconfig.defaults` | **Not Applicable**: Local IDE settings and board-specific `sdkconfig.defaults` for a 4MB ESP32-WROVER-CAM board. |
| `89243fc8` | `Complete hardware verification for Freenove ESP32-WROVER-CAM` | `sdkconfig.defaults`, `tools/flash_and_monitor.py`, `tools/quick_reset.py` | **Not Applicable**: Local serial flash scripts with hardcoded `/dev/ttyUSB0` paths. |
| `93480def` | `Bypass Wi-Fi Manager and hardcode credentials for Freenove stability` | `main/esp_app_main.c`, `sdkconfig.defaults` | **PARTIALLY APPLICABLE (2 C Bug Fixes in `main/esp_app_main.c`)**:<br>1. **Memory leak at `main/esp_app_main.c:446-454`**: `char *bypass_wm = config_alloc_get_default(NVS_TYPE_STR, "bypass_wm", "0", 0);` is allocated on the heap, checked in `if (bypass_wm && ...)`, and **never freed** in the `else` branch.<br>2. **Dead IP-change detection at `main/esp_app_main.c:154-164`**: In `cb_connection_got_ip()`, `static ip4_addr_t ip;` checks `if (ip.addr && ipInfo.ip.addr != ip.addr)` to warn/reboot on IP change, but **never assigns `ip.addr = ipInfo.ip.addr;`**, so `ip.addr` remains `0` forever.<br>*(Note: Do NOT port the hardcoded Wi-Fi SSID/password from `93480def`—violates Invariant 24).* |
| `80067169` | `Disable Jack, Speaker, and Headphone detection via Kconfig...` | `components/services/Kconfig.projbuild`, `accessors.c`, `battery.c`, `monitor.c`, `sdkconfig.defaults` | **Not Applicable**: Disables jack/battery/monitor services globally for a bare camera devkit. |
