# MK5-Rust Upstream Parity & Bug Remediation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Incorporate all 19 pure-Rust upstream protocol, HTTP streaming, audio decoder, TAS5805M hardware initialization, and target loop fixes (`RUST-01..RUST-19`) into the `rust/` Cargo workspace on branch `MK5-Rust` without modifying legacy C files.

**Architecture:** We partition the 19 pure-Rust findings across 6 self-contained tasks in 3 conflict-free execution waves. Wave 1 upgrades the three foundational leaf crates (`squeezelite-proto`, `squeezelite-tas58xx`, `squeezelite-decoders`) in parallel across strictly disjoint directories. Wave 2 upgrades the mid-layer `louder-esp32-app` modules (`board.rs`+`amp.rs`+`pipeline.rs` and `network.rs`) in parallel across disjoint files. Wave 3 integrates all Wave 1 and Wave 2 contracts into `coordinator.rs` and `main.rs`.

**Tech Stack:** Rust 2021 (`#![forbid(unsafe_code)]`, `#![no_std]` core crates), `embedded-hal 1.0`, `heapless 0.8`, `claxon 0.4`, `lewton 0.10`, `ogg 0.8`, `nanomp3-core 0.2`, `syom 0.7`, `proptest`.

**Spec:** `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/code_analysis_report.md` (and `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-003-upstream-protocol-parity.md` + `WO-002-upstream-git-commits.md`)

**Execution Waves:**
- **Wave 1 (Parallel Leaf Crates — Disjoint Directories)**:
  - **Task 1**: `squeezelite-proto` (`rust/crates/squeezelite-proto/src/lib.rs`)
  - **Task 2**: `squeezelite-tas58xx` (`rust/crates/squeezelite-tas58xx/src/lib.rs`)
  - **Task 3**: `squeezelite-decoders` (`rust/crates/squeezelite-decoders/src/lib.rs`)
- **Wave 2 (Parallel Application Modules — Disjoint Files in `louder-esp32-app`, Blocked by Wave 1)**:
  - **Task 4**: `board.rs`, `amp.rs`, & `pipeline.rs` (`rust/crates/louder-esp32-app/src/{board.rs,amp.rs,pipeline.rs}`) — Blocked by Task 2, Task 3
  - **Task 5**: `network.rs` (`rust/crates/louder-esp32-app/src/network.rs`) — Blocked by Task 1
- **Wave 3 (Application Coordinator & Firmware Loop Integration — Blocked by Wave 1 & Wave 2)**:
  - **Task 6**: `coordinator.rs` & `main.rs` (`rust/crates/louder-esp32-app/src/{coordinator.rs,main.rs}`) — Blocked by Task 1, Task 4, Task 5

## Global Constraints

- **Pure-Rust Scope**: Modify ONLY files inside `/home/console-admin/Github/squeezelite-esp32/rust/crates/`. Never modify legacy C files under `components/`, `main/`, or `cyd-controller/`.
- **Dependency Invariance**: Do NOT modify `rust/Cargo.toml` or `rust/crates/*/Cargo.toml` dependency versions.
- **Safety & Lint Gate**: Preserve `#![forbid(unsafe_code)]` across all crates. Every task must pass `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace` and `/home/console-admin/.cargo/bin/cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets --all-features -- -D warnings` with zero warnings.
- **Backwards Compatibility**: Preserve all 206 existing unit, integration, and proptest fuzz tests while adding new unit tests for every fix.

## Review Focus

1. **Inline ICY Metadata Split Across TCP Reads (`RUST-11`)**: When an ICY metadata block (`1` length byte `L` + `16 * L` bytes) straddles two `TcpStream::read` calls in `HttpAudioStream::poll_audio_bytes`, the state machine must buffer partial metadata bytes across polls without leaking metadata into audio output or losing sync.Pinned by `test_http_audio_stream_icy_metaint_split_across_reads` in Task 5.
2. **HTTP Stream Open Failure on Slow/Dropped Wi-Fi (`RUST-03` / `90b696a1`)**: When `server_port > 0` in `strm` `'s'` and `HttpAudioStream::connect` fails (e.g., connection refused), `PlayerCoordinator::handle_command` must NOT enter `PlayerState::Buffering` or emit `STMs`; it must transition to `PlayerState::Stopped` and emit `StatEvent::NotSupported` (`STMn`) + `DSCO(DisconnectReason::Unreachable)`. Pinned by `test_coordinator_stream_open_failure_emits_stmn_and_dsco` in Task 6.
3. **Vorbis Multi-Packet MDCT Overlap-Add Continuity (`RUST-13`)**: Decoding multiple consecutive Ogg Vorbis audio packets across separate `VorbisDecoder::decode` calls must preserve `PreviousWindowRight` overlap-add state and drain consumed bytes without re-initializing `OggStreamReader` from byte 0. Pinned by `test_vorbis_multi_packet_stateful_continuity` in Task 3.
4. **End-of-Track `STMd` (Decode Complete) vs `STMu` (Output Drained) Ordering (`RUST-02`)**: When `HttpAudioStream` reaches EOF while decoded PCM remains in `pipeline.output_buffer`, `PlayerCoordinator` must emit `STMd` + `DSCO(ConnectionClosed)` first, and emit `STMu` (`TrackUnderrun`) only after `output_buffer` and `pcm_pending_samples` drain to 0. Pinned by `test_coordinator_eof_emits_stmd_then_stmu_after_output_drain` in Task 6.
5. **TAS5805M Book/Page Selection From Non-Zero Page (`RUST-18`)**: Selecting a book/page via `Tas58xx::select_book_page` must write `PAGE_SELECT (0x00) = 0x00` *before* `BOOK_SELECT (0x7F) = book` and `PAGE_SELECT (0x00) = page`. Pinned by `test_select_book_page_writes_page_zero_before_book` in Task 2.

---

### Task 1: `squeezelite-proto` Outbound Builders (`RESP`, `DSCO`, `META`, `SETD`), `AutostartMode`, `StatEvent` Extensions (`STMu`, `STMc`, `STMf`), & Extended Opcodes (`RUST-01`, `RUST-02`, `RUST-04`, `RUST-05`, `RUST-06`, `RUST-07`, `RUST-08`, `RUST-10`, `RUST-16`)

**Files:**
- Modify: `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs:76-91, 194-333, 411-455, 561-783`
- Test: `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs` (inline `mod tests`)

**Dependencies:**
- Blocked By: None
- Parallel Wave: Wave 1

**Interfaces:**
- Consumes: None (`#![no_std]` leaf crate)
- Produces:
  - `pub enum AutostartMode { Off, Auto, DirectOff, DirectAuto }` with `pub const fn from_byte(b: u8) -> Self`, `pub const fn is_auto(self) -> bool`, `pub const fn requires_cont(self) -> bool`
  - `pub const STRM_FLAG_CONT_WAIT: u8 = 0x20;` and `pub const STRM_FLAG_SERVER_REBOOT: u8 = 0x40;` (encoded in `strm` `flags` when `payload[1]` is `b'2'` or `b'3'` so `AutostartMode::from_strm(autostart: bool, flags: u8) -> AutostartMode` recovers all 4 modes without breaking existing `ServerCommand::Stream` pattern matches)
  - `pub enum DisconnectReason { ConnectionClosed = 0, ResetByLocal = 1, ResetByRemote = 2, Unreachable = 3, TimedOut = 4 }`
  - `StatEvent` variants preserved for full backward compatibility with `tests/proto_fuzz.rs:25` (`StatEvent::StreamUnderrun` continues to map to `*b"STMl"`) plus new variants:
    - `StatEvent::TrackUnderrun` (`opcode_bytes() -> *b"STMu"`)
    - `StatEvent::Connected` (`opcode_bytes() -> *b"STMc"`)
    - `StatEvent::StreamFlushed` (`opcode_bytes() -> *b"STMf"`)
    - `impl StatEvent { pub const BUFFER_THRESHOLD: Self = Self::StreamUnderrun; }`
  - `ServerCommand::equalizer_gains(&self) -> Option<&'a [u8]>` (`opcode == *b"eqlz"`) and `ServerCommand::loudness_gain(&self) -> Option<u8>` (`opcode == *b"loud"`)
  - `SlimprotoBuilder::build_resp(headers: &[u8], out: &mut [u8]) -> Result<usize, ParseError>`
  - `SlimprotoBuilder::build_dsco(reason: DisconnectReason, out: &mut [u8]) -> Result<usize, ParseError>`
  - `SlimprotoBuilder::build_meta(metadata: &[u8], out: &mut [u8]) -> Result<usize, ParseError>`
  - `SlimprotoBuilder::build_setd_name(name: &str, out: &mut [u8]) -> Result<usize, ParseError>`
  - `SlimprotoBuilder::build_helo_with_options(mac: &[u8; 6], device_id: u8, capabilities: &str, reconnect: bool, bytes_received: u64, out: &mut [u8]) -> Result<usize, ParseError>`
  - `StatBuilder::build_stat_with_metrics(event: StatEvent, stream_buffer_fullness: u32, output_buffer_fullness: u32, frames_played: u64, sample_rate: u32, bytes_received: u64, jiffies_ms: u32, server_timestamp: u32, out: &mut [u8]) -> Result<usize, ParseError>`
  - Updated `sample_rate_from_index(index: u8) -> u32` supporting `10 | b':' => 88200` and ASCII `'0'..='9'`.

#### Verification Steps (Pattern A: Standard TDD):
- [ ] **Step 1: Write failing unit tests in `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs`**
  Add tests:
  - `test_build_resp_dsco_meta_and_setd_name`: verifies `RESP` (`[b"RESP", len_be, headers]`), `DSCO` (`[b"DSCO", 0, 0, 0, 1, reason]`), `META` (`[b"META", len_be, meta]`), and `SETD` (`[b"SETD", len_be, 0x00, name_bytes]`).
  - `test_stat_event_stmu_stmc_stmf_and_build_stat_with_metrics`: verifies `StatEvent::TrackUnderrun.opcode_bytes() == *b"STMu"`, `StatEvent::Connected.opcode_bytes() == *b"STMc"`, `StatEvent::StreamFlushed.opcode_bytes() == *b"STMf"`, `StatEvent::StreamUnderrun.opcode_bytes() == *b"STMl"`, `elapsed_ms` calculation at `96_000` Hz, `bytes_received`, and `server_timestamp` at payload offset `47..51` (frame offset `55..59`).
  - `test_autostart_mode_and_88200_sample_rate`: verifies `AutostartMode` `'0'..'3'` roundtrip via `parse_frame` (`STRM_FLAG_CONT_WAIT` set for `'2'` and `'3'`), `sample_rate_from_index(b':') == 88200`, and `eqlz`/`loud` helper accessors.
  - `test_build_helo_with_options_sets_reconnect_bit_and_bytes`: verifies `out[32..34] == 0x4000u16.to_be_bytes()` and `out[34..42] == bytes_received.to_be_bytes()`.
- [ ] **Step 2: Run test suite to confirm failure**
  Run: `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-proto`
  Expected: FAIL (missing symbols `AutostartMode`, `DisconnectReason`, `TrackUnderrun`, `build_resp`, `build_dsco`, `build_meta`, `build_setd_name`).
- [ ] **Step 3: Implement protocol types, parsers, and builders in `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs`**
  Implement all types and methods listed in **Interfaces** above. In `SlimprotoParser::parse_strm`, set `let flags = if payload[1] == b'2' || payload[1] == b'3' { payload[11] | STRM_FLAG_CONT_WAIT } else { payload[11] };` so `AutostartMode::from_strm(autostart, flags)` distinguishes all 4 modes while keeping `ServerCommand::Stream` fields unchanged.
- [ ] **Step 4: Run tests and clippy to verify pass**
  Run: `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-proto && /home/console-admin/.cargo/bin/cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-proto --all-targets -- -D warnings`
  Expected: PASS (`0` warnings).

---

### Task 2: `squeezelite-tas58xx` Register Page/Book Ordering, `0x2C` Address & `Hi-Z` Staging (`RUST-17`, `RUST-18`)

**Files:**
- Modify: `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs:16-30, 229-346, 348-430`
- Test: `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs` (inline `mod tests`)

**Dependencies:**
- Blocked By: None
- Parallel Wave: Wave 1

**Interfaces:**
- Consumes: `embedded_hal::i2c::I2c`
- Produces:
  - `pub const TAS5805M_I2C_ADDR_DEFAULT: u8 = 0x2C;` (standard Sonocotta Louder-ESP32 TAS5805M 7-bit address)
  - `Tas58xx<I2C, Standby>::select_book_page(&mut self, book: u8, page: u8) -> Result<(), DriverError<E>>` (writes `[PAGE_SELECT, 0x00]`, `[BOOK_SELECT, book]`, `[PAGE_SELECT, page]`)
  - `Tas58xx<I2C, Play>::select_book_page(&mut self, book: u8, page: u8) -> Result<(), DriverError<E>>`
  - `Tas58xx<I2C, Standby>::stage_hiz_and_clear_faults(&mut self) -> Result<(), DriverError<E>>` (writes `[DEVICE_CTRL_2, CTRL2_MODE_HIZ]`, `[FAULT_CLEAR, FAULT_CLEAR_ANALOG]`)
  - `Tas58xx<I2C, Standby>::try_start_playback_via_hiz(mut self) -> Result<Tas58xx<I2C, Play>, (Self, DriverError<E>)>` (writes `[DEVICE_CTRL_2, CTRL2_MODE_HIZ]`, `[FAULT_CLEAR, FAULT_CLEAR_ANALOG]`, `[DEVICE_CTRL_2, CTRL2_MODE_PLAY_MUTE]`, `[DEVICE_CTRL_2, CTRL2_MODE_PLAY]`)

#### Verification Steps (Pattern A: Standard TDD):
- [ ] **Step 1: Write failing unit tests in `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs`**
  Add tests:
  - `test_select_book_page_writes_page_zero_before_book`: verifies `I2cTrans::write(TAS5805M_I2C_ADDR_DEFAULT, vec![registers::PAGE_SELECT, 0x00])`, then `[registers::BOOK_SELECT, 0x8C]`, then `[registers::PAGE_SELECT, 0x2A]`.
  - `test_try_start_playback_via_hiz_sequence`: verifies `CTRL2_MODE_HIZ (0x02)` -> `FAULT_CLEAR (0x80)` -> `CTRL2_MODE_PLAY_MUTE (0x0B)` -> `CTRL2_MODE_PLAY (0x03)`.
- [ ] **Step 2: Run test suite to confirm failure**
  Run: `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-tas58xx`
  Expected: FAIL (missing `TAS5805M_I2C_ADDR_DEFAULT`, `select_book_page`, `stage_hiz_and_clear_faults`, `try_start_playback_via_hiz`).
- [ ] **Step 3: Implement `TAS5805M_I2C_ADDR_DEFAULT`, `select_book_page`, `stage_hiz_and_clear_faults`, and `try_start_playback_via_hiz` in `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs`**
  Preserve existing `try_init_into_standby` and `try_start_playback` transaction sequences so existing mock tests in `squeezelite-tas58xx` and `louder-esp32-app` remain unbroken.
- [ ] **Step 4: Run tests and clippy to verify pass**
  Run: `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-tas58xx && /home/console-admin/.cargo/bin/cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-tas58xx --all-targets -- -D warnings`
  Expected: PASS (`0` warnings).

---

### Task 3: `squeezelite-decoders` Stateful FLAC/Vorbis Decoding, 8-Bit Shift, WAV/AIFF PCM Header Stripping & Sample Rate Exposure (`RUST-13`, `RUST-14`, `RUST-15`, `RUST-16`)

**Files:**
- Modify: `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs:79-128, 316-432, 434-638, 762-942`
- Test: `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs` (inline `mod tests`)

**Dependencies:**
- Blocked By: None
- Parallel Wave: Wave 1

**Interfaces:**
- Consumes: `claxon`, `lewton`, `ogg`, `nanomp3_core`, `syom`
- Produces:
  - `PcmDecoder`:
    - Fields: `pub bits_per_sample: u8`, `pub channels: u8`, `pub big_endian: bool`, `pub sample_rate: Option<u32>`, `header_stripped: bool`
    - Automatically detects and strips 44+ byte WAV (`RIFF....WAVEfmt ...data`) and AIFF (`FORM....AIFFCOMM...SSND`) headers on initial `decode()`, updating `bits_per_sample`, `channels`, `big_endian`, and `sample_rate`.
  - `Mp3Decoder`:
    - Adds `pub sample_rate: u32` (initialized to `44100`, updated to `info.sample_rate` on each decoded frame).
  - `FlacDecoder`:
    - `FlacByteReader::read` copies `avail.len().min(buf.len())` bytes in bulk (removing `.min(1)` clamp).
    - Fixes 8-bit sample shift to `8 => 24` (`<< 24`).
    - Strips optional Ogg page wrapper prefix before `b"fLaC"` if present in `stream_buf`.
  - `VorbisDecoder`:
    - Preserves `packets_decoded: usize` across `decode()` calls so `OggStreamReader` skips already-decoded audio packets without re-emitting them or corrupting Ogg page boundaries, and compacts `stream_buf` only when an entire Ogg stream finishes or `flush()` is called (while keeping `MAX_STREAM_BUF` protection).

#### Verification Steps (Pattern A: Standard TDD):
- [ ] **Step 1: Write failing unit tests in `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs`**
  Add tests:
  - `test_pcm_decoder_strips_wav_header_and_updates_config`: feeds a 44-byte `RIFF`/`WAVE` header (48000 Hz, 16-bit LE stereo) followed by 2 stereo frames (8 bytes), asserting `consumed == 52`, `frames == 2`, `decoder.sample_rate == Some(48000)`, `decoder.big_endian == false`.
  - `test_pcm_decoder_strips_aiff_header`: feeds a 54-byte `FORM`/`AIFF` header (`COMM` + `SSND`) followed by 1 stereo frame (4 bytes), asserting `consumed == 58`, `frames == 1`, `decoder.big_endian == true`.
  - `test_flac_bulk_reader_and_8bit_shift`: verifies `FlacByteReader::read` reads full buffer slices (`> 1` byte) in a single call and 8-bit shift equals `24`.
  - `test_vorbis_multi_packet_stateful_continuity`: verifies `VorbisDecoder` tracks `packets_decoded` across calls and `Mp3Decoder` exposes `pub sample_rate: u32`.
- [ ] **Step 2: Run test suite to confirm failure**
  Run: `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-decoders`
  Expected: FAIL.
- [ ] **Step 3: Implement decoder enhancements in `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs`**
  1. In `PcmDecoder`: add `pub sample_rate: Option<u32>` and `header_stripped: bool` (keeping `PcmDecoder::new` a `const fn`). Add `try_strip_container_header(&mut self, data: &[u8]) -> Result<usize, DecoderError>` handling `RIFF`/`WAVE` and `FORM`/`AIFF`.
  2. In `Mp3Decoder`: add `pub sample_rate: u32` (default `44100`), updated via `if info.sample_rate > 0 { self.sample_rate = info.sample_rate; }`.
  3. In `FlacByteReader::read`: replace `let n = avail.len().min(buf.len()).min(1);` with `let n = avail.len().min(buf.len());`, and in `FlacDecoder::decode` map `8 => 24` in `shift`.
  4. In `VorbisDecoder`: maintain persistent `packets_decoded: usize` so `OggStreamReader` advances past `packets_decoded` audio packets before emitting new packets (preserving `PreviousWindowRight` MDCT overlap-add state between consecutive packets!).
- [ ] **Step 4: Run tests and clippy to verify pass**
  Run: `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-decoders && /home/console-admin/.cargo/bin/cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-decoders --all-targets -- -D warnings`
  Expected: PASS (`0` warnings).

---

### Task 4: `louder-esp32-app` Board Pinout Alignment, `AmpManager` `Hi-Z`/Page Helpers & `StreamPipeline` Dynamic Sample Rate (`RUST-15`, `RUST-17`, `RUST-18`)

**Files:**
- Modify: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/board.rs:12-95`
- Modify: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/amp.rs:170-280`
- Modify: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs:17-50, 148-193`
- Test: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs` and `amp.rs` (inline `mod tests`)

**Dependencies:**
- Blocked By: Task 2, Task 3
- Parallel Wave: Wave 2

**Interfaces:**
- Consumes:
  - `squeezelite_tas58xx::{TAS5805M_I2C_ADDR_DEFAULT, Tas58xx::select_book_page, Tas58xx::stage_hiz_and_clear_faults}` (from Task 2)
  - `PcmDecoder::sample_rate`, `Mp3Decoder::sample_rate`, `FlacDecoder::sample_rate`, `AacDecoder::sample_rate`, `VorbisDecoder::sample_rate` (from Task 3)
- Produces:
  - `board::pins::{CYD_UART_TX, CYD_UART_RX, TAS5805M_PRIMARY_I2C_ADDR, TAS58XX_I2C_ADDR}` (`4` / `36` on ESP32-WROVER avoiding PSRAM GPIO16/17; `43` / `44` on ESP32-S3; `TAS5805M_PRIMARY_I2C_ADDR = 0x2C` primary TAS5805M address while preserving `TAS58XX_I2C_ADDR` so existing `amp.rs:290` tests remain unbroken) and `BoardModel::cyd_uart_pins(self) -> (u8, u8)`
  - `AmpManager::select_book_page(&mut self, book: u8, page: u8) -> Result<(), DriverError<E>>` and `AmpManager::stage_hiz(&mut self) -> Result<(), DriverError<E>>`
  - `ActiveDecoder::sample_rate(&self) -> Option<u32>`
  - Automatic update of `self.sample_rate` and `self.sink.set_sample_rate(rate)` in `StreamPipeline::process_decode_step` when `self.decoder.sample_rate()` changes.

#### Verification Steps (Pattern A: Standard TDD):
- [ ] **Step 1: Write failing unit tests in `pipeline.rs` and `amp.rs`**
  - In `pipeline.rs`: `test_pipeline_propagates_dynamic_decoder_sample_rate` feeds a 48000 Hz WAV header + PCM samples into `StreamPipeline` initialized at 44100 Hz and asserts `pipeline.sample_rate == 48000` and `pipeline.sink.sample_rate == 48000`.
  - In `amp.rs`: `test_amp_manager_select_book_page_and_stage_hiz` verifies `AmpManager::select_book_page` and `AmpManager::stage_hiz` I2C transactions and `BoardModel::LouderEsp32Wrover.cyd_uart_pins() == (4, 36)`.
- [ ] **Step 2: Run test suite to confirm failure**
  Run: `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app --lib`
  Expected: FAIL.
- [ ] **Step 3: Implement changes in `board.rs`, `amp.rs`, and `pipeline.rs`**
  Implement `cyd_uart_pins`, `TAS5805M_PRIMARY_I2C_ADDR`, `AmpManager::select_book_page`, `AmpManager::stage_hiz`, `ActiveDecoder::sample_rate`, and dynamic sample rate propagation in `StreamPipeline::process_decode_step`.
- [ ] **Step 4: Run tests and clippy to verify pass**
  Run: `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app --lib pipeline && /home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app --lib amp`
  Expected: PASS.

---

### Task 5: `louder-esp32-app/src/network.rs` HTTP `RESP` Headers, ICY `icy-metaint` Stripping, HTTP Redirects, Chunked Encoding & SlimProto Outbound Methods (`RUST-01`, `RUST-10`, `RUST-11`, `RUST-12`)

**Files:**
- Modify: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs:100-160, 257-425`
- Test: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs` (inline `mod tests`)

**Dependencies:**
- Blocked By: Task 1
- Parallel Wave: Wave 2

**Interfaces:**
- Consumes:
  - `squeezelite_proto::{DisconnectReason, SlimprotoBuilder, StatBuilder, StatEvent, StatReport}` (from Task 1)
- Produces:
  - `SlimprotoClient`:
    - `pub fn connect_with_options(server_addr: SocketAddr, mac: [u8; 6], model_name: &str, reconnect: bool, bytes_received: u64) -> std::io::Result<Self>`
    - `pub fn send_stat_with_metrics(&mut self, event: StatEvent, stream_buf_fullness: u32, output_buf_fullness: u32, frames_played: u64, sample_rate: u32, bytes_received: u64, jiffies_ms: u32, server_timestamp: u32) -> std::io::Result<()>`
    - `pub fn send_resp(&mut self, headers: &[u8]) -> std::io::Result<()>`
    - `pub fn send_dsco(&mut self, reason: DisconnectReason) -> std::io::Result<()>`
    - `pub fn send_meta(&mut self, metadata: &[u8]) -> std::io::Result<()>`
    - `pub fn send_setd_name(&mut self, name: &str) -> std::io::Result<()>`
  - `HttpAudioStream`:
    - `pub fn take_response_headers(&mut self) -> Option<Vec<u8>>`
    - `pub fn take_icy_metadata(&mut self) -> Option<Vec<u8>>`
    - `pub fn set_icy_metaint(&mut self, interval: usize)`
    - `pub fn icy_metaint(&self) -> usize`
    - `pub fn total_audio_bytes(&self) -> u64`
    - Automatic HTTP `301`/`302`/`307`/`308` `Location: http://<ip>:<port>/<path>` redirect following (up to `MAX_REDIRECTS = 3`).
    - Automatic `Transfer-Encoding: chunked` de-chunking and `icy-metaint` inline metadata stripping in `poll_audio_bytes`.

#### Verification Steps (Pattern A: Standard TDD):
- [ ] **Step 1: Write failing unit tests in `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs`**
  Add tests:
  - `test_http_audio_stream_icy_metaint_split_across_reads`: mock server sends `HTTP/1.0 200 OK\r\nicy-metaint: 4\r\n\r\n` followed by 4 audio bytes `b"ABCD"`, 1 length byte `0x01` (16 bytes metadata `b"StreamTitle='X';"`), and 4 audio bytes `b"EFGH"` split across two writes; asserts `poll_audio_bytes` yields `b"ABCDEFGH"`, `total_audio_bytes() == 8`, and `take_icy_metadata().unwrap() == b"StreamTitle='X';"` (and `take_response_headers()` returns the raw HTTP response header).
  - `test_http_audio_stream_follows_302_redirect`: mock redirect server returns `HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:<port2>/audio.pcm\r\n\r\n`, second mock server returns `HTTP/1.0 200 OK\r\n\r\n\x11\x22\x33\x44`; asserts `HttpAudioStream` follows the redirect and reads `[0x11, 0x22, 0x33, 0x44]`.
  - `test_http_audio_stream_chunked_transfer_encoding`: mock server sends `HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nABCD\r\n2\r\nEF\r\n0\r\n\r\n`; asserts `poll_audio_bytes` yields `b"ABCDEF"` and reaches `is_eof() == true`.
- [ ] **Step 2: Run test suite to confirm failure**
  Run: `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app --lib network`
  Expected: FAIL.
- [ ] **Step 3: Implement `SlimprotoClient` outbound methods and `HttpAudioStream` `RESP` capture, redirects, chunked decoding, and ICY metadata stripping in `network.rs`**
- [ ] **Step 4: Run tests and clippy to verify pass**
  Run: `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app --lib network`
  Expected: PASS.

---

### Task 6: `louder-esp32-app/src/coordinator.rs` & `src/main.rs` SlimProto Lifecycle (`STMf`/`STMc`/Deferred `STMs`), Stream Open Error Handling, `STMd`/`STMu` Separation & Target Loop Wiring (`RUST-01..RUST-10`, `RUST-16`, `RUST-19`)

**Files:**
- Modify: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs:47-755`
- Modify: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs:21-185`
- Test: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs` and `main.rs` (inline `mod tests`)

**Dependencies:**
- Blocked By: Task 1, Task 4, Task 5
- Parallel Wave: Wave 3 (runs after Wave 1 and Wave 2 complete)

**Interfaces:**
- Consumes:
  - `AutostartMode`, `DisconnectReason`, `StatEvent::{Connected, StreamFlushed, TrackStarted, DecodeReady, TrackUnderrun, NotSupported}`, `STRM_FLAG_SERVER_REBOOT` (from Task 1)
  - `SlimprotoClient::{send_resp, send_dsco, send_meta, send_setd_name, send_stat_with_metrics}` and `HttpAudioStream::{take_response_headers, take_icy_metadata, set_icy_metaint, total_audio_bytes}` (from Task 5)
  - `AmpManager::recover_from_fault` and `StreamPipeline::sample_rate` (from Task 4)
- Produces:
  - `PlayerCoordinator` fields & behavior:
    - `pub autostart_mode: AutostartMode`, `pub awaiting_cont: bool`, `pub stream_bytes_received: u64`, `pub decode_complete_sent: bool`, `pub pending_decode_ready: bool`, `pub pending_track_started: bool`, `pub pending_dsco: Option<DisconnectReason>`, `pub pending_resp: Option<Vec<u8>>`, `pub pending_meta: Option<Vec<u8>>`, `pub dac_enabled: bool`, `pub server_rebooting: bool`, `pub player_name: heapless::String<64>`
    - `decode_pcm_sample_rate_hz(b':') == Some(88200)` (`RUST-16`)
    - `StreamCommandType::Flush` (`strm` `'f'`): flushes pipeline and immediately emits `StatEvent::StreamFlushed` (`STMf`) (`RUST-04`).
    - `StreamCommandType::Start` (`strm` `'s'`):
      - When `server_port > 0` and `HttpAudioStream::connect` fails, sets `self.state = PlayerState::Stopped`, `self.pending_dsco = Some(DisconnectReason::Unreachable)`, sends `StatEvent::NotSupported` (`STMn`) + `DSCO(Unreachable)` (`RUST-03` / `90b696a1`), and returns `Ok(Some(CydMessage::PlaybackState(PlayState::Stopped)))`.
      - When `HttpAudioStream::connect` succeeds, emits `StatEvent::Connected` (`STMc`) (`RUST-04`).
      - Does NOT emit `StatEvent::TrackStarted` (`STMs`) prematurely inside `handle_command` even when `effective_threshold == 0`; instead, transitions to `PlayerState::Playing` (when `autostart && !requires_cont` and `effective_threshold == 0`, with `self.pending_track_started = false` until the first PCM frames are actually written to the I2S sink in `process_audio_step`, which sets `self.pending_track_started = true` and emits `STMs` in `check_heartbeat` / `tick`) (`RUST-04`).
    - `AutostartMode::DirectOff` (`'2'`) / `DirectAuto` (`'3'`): sets `self.awaiting_cont = true` and waits for `cont` before transitioning from `Buffering` to `Playing`/`Paused` (`RUST-05`).
    - `StreamCommandType::Timestamp` (`strm` `'t'`): immediately sends `STAT` (`StatEvent::Heartbeat`) with `server_timestamp = replay_gain` (`RUST-06`).
    - `StreamCommandType::SkipAhead` (`strm` `'a'`): consumes `(replay_gain as usize * (self.current_sample_rate as usize / 1000)) * 8` bytes from `pipeline.output_buffer` (`RUST-08`).
    - `ServerCommand::AudioGain`: respects `digital_atten` (`0` with non-zero gain -> unity `65536`) (`RUST-09`).
    - `ServerCommand::Unknown`: handles `aude` (`dac_enabled` + `pipeline.sink.set_mute`), `cont` (`awaiting_cont = false` + `set_icy_metaint`), `setd` (`id == 0` query/set `self.player_name` and `send_setd_name`), `eqlz`/`loud` (`pipeline.equalizer.enabled = true`) (`RUST-08`).
    - `tick` & `check_heartbeat`: emits `RESP`, `META`, deferred `STMs` (`TrackStarted` when `pending_track_started` is set after first PCM write), `STMd` (`DecodeReady`) + `DSCO(ConnectionClosed)` when HTTP stream reaches EOF and `stream_buffer` drains, and emits `STMu` (`TrackUnderrun`) when `output_buffer` and `pcm_pending_samples` drain (`RUST-01`, `RUST-02`, `RUST-04`, `RUST-07`).
    - Stabilizes `coordinator::tests::test_coordinator_stream_and_audio_lifecycle` using frame-delimited TCP reads (`read_slimproto_frame` helper reading 4-byte opcode + 4-byte BE length + payload) to eliminate TCP read coalescing flakiness.
  - `run_firmware_tick` and `run_firmware_tick_with_network` in `main.rs`:
    - Preserves `pub fn run_firmware_tick<S, U, I>(coordinator: &mut PlayerCoordinator, pipeline: &mut StreamPipeline<'_, S>, cyd_bridge: &mut CydUartBridge<U>, amp: &mut AmpManager<I>) -> usize` (delegating to `run_firmware_tick_with_network(..., None, 0)`) so existing `test_run_firmware_tick_processes_cyd_and_audio` compiles and passes unchanged.
    - Adds `pub fn run_firmware_tick_with_network<S, U, I>(coordinator: &mut PlayerCoordinator, pipeline: &mut StreamPipeline<'_, S>, cyd_bridge: &mut CydUartBridge<U>, amp: &mut AmpManager<I>, client: Option<&mut SlimprotoClient>, now_ms: u32) -> usize` which:
      1. Polls `coordinator.tick(client_ref, pipeline, now_ms)` when `client` is `Some` (or `coordinator.process_audio_step(pipeline)` when `None`).
      2. Synchronizes `coordinator.current_sample_rate = pipeline.sample_rate`.
      3. Synchronizes `coordinator.dac_enabled` with `amp` (`amp.pause()` when `!coordinator.dac_enabled`).
      4. Checks `amp.check_faults()` and invokes `amp.recover_from_fault()` when a hardware fault is detected (`RUST-19`).

#### Verification Steps (Pattern A: Standard TDD):
- [ ] **Step 1: Write failing unit tests in `coordinator.rs` and `main.rs`**
  - `test_coordinator_stream_open_failure_emits_stmn_and_dsco`: sends `strm` `'s'` with an unreachable closed port (`server_port = 1`); asserts `coordinator.state == PlayerState::Stopped` and mock LMS server receives `STAT` (`STMn`) and `DSCO` (`Unreachable = 3`).
  - `test_coordinator_defers_stms_until_pcm_output_written`: sends `strm` `'s'` with `threshold = 0`; asserts `STMc` is emitted on HTTP connect, `STMs` is NOT emitted before `process_audio_step` writes PCM frames to `sink`, and `STMs` IS emitted after `process_audio_step` writes the first decoded PCM frames (`RUST-04`).
  - `test_coordinator_eof_emits_stmd_then_stmu_after_output_drain`: streams a short PCM clip over HTTP, verifies `RESP` is sent on header parse, `STMd` + `DSCO(0)` are emitted when `stream_buffer` finishes decoding while `output_buffer` still holds frames, and `STMu` is emitted after `output_buffer` drains.
  - `test_coordinator_timestamp_probe_and_setd_aude_cont`: verifies `strm` `'t'` echoes `replay_gain` in `STAT.server_timestamp`, `setd` id `0` updates and replies with `SETD` player name, `aude` toggles `dac_enabled`, and `cont` releases `awaiting_cont`.
  - `test_run_firmware_tick_with_network_recovers_amp_fault_and_syncs_rate`: in `main.rs`, verifies `run_firmware_tick_with_network` polls `SlimprotoClient`, syncs `coordinator.current_sample_rate`, and triggers `amp.recover_from_fault()`.
- [ ] **Step 2: Run test suite to confirm failure**
  Run: `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app`
  Expected: FAIL.
- [ ] **Step 3: Implement `PlayerCoordinator` and `run_firmware_tick_with_network` updates in `coordinator.rs` and `main.rs` (including stabilizing `test_coordinator_stream_and_audio_lifecycle` frame reads)**
- [ ] **Step 4: Run full workspace test suite and strict Clippy gate**
  Run: `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace && /home/console-admin/.cargo/bin/cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets --all-features -- -D warnings`
  Expected: PASS (`0` failed tests, `0` clippy warnings).
