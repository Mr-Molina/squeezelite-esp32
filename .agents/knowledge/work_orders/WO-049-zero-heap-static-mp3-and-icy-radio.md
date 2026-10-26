# Work Order `WO-049`: Implement Zero-Heap `.bss` `StaticMp3Decoder`, Partial-Frame Guard, & ICY Radio Support (`--stage-only`)

- **ID**: `WO-049`
- **Timestamp**: `2026-10-24T14:47:00-04:00`
- **Subagent Role**: Rust App & Hardware Implementer (`Model: "flash"`)
- **Status**: `[IN PROGRESS]`

## 1. Objective & Scope
Enable native MP3 Internet Radio (`"Ibiza Global Radio MP3"`, Shoutcast/Icecast streams with `cont` `metaint` ICY metadata) and WAV/AIFF playback on `172.22.1.15` (`"BASEMENT"`) with **zero heap allocation (`0` B on the `64 KB` Wi-Fi heap)** and **zero stack temporaries (`0` B on the CPU stack)**.

### Mandatory File Changes

1. **`/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs`**:
   - Add a `no_std`, zero-`alloc` `pub struct StaticMp3Decoder` (unconditionally available regardless of `feature = "alloc"`):
     ```rust
     /// Zero-heap, `no_std`, `const fn`-initializable MP3 decoder for embedded targets (`xtensa`).
     ///
     /// Decodes directly into `pcm_out` (`>= 2304` `i32` samples) with zero heap allocations
     /// and resets in O(1) via `nanomp3_core::__private::init` without constructing a 23 KB temporary.
     pub struct StaticMp3Decoder {
         pub sample_rate: u32,
         decoder: nanomp3_core::Decoder,
         id3_skip_remaining: usize,
     }

     impl Default for StaticMp3Decoder {
         fn default() -> Self {
             Self::new()
         }
     }

     impl StaticMp3Decoder {
         pub const fn new() -> Self {
             Self {
                 sample_rate: 44100,
                 decoder: nanomp3_core::Decoder::new(),
                 id3_skip_remaining: 0,
             }
         }

         /// Resets the decoder state in-place without constructing a 23 KB temporary on the stack.
         pub fn reset_in_place(&mut self) {
             self.sample_rate = 44100;
             nanomp3_core::__private::init(&mut self.decoder);
             self.id3_skip_remaining = 0;
         }
     }
     ```
   - Implement `AudioDecoder for StaticMp3Decoder`:
     - Check `if pcm_out.len() < nanomp3_core::MAX_SAMPLES_PER_FRAME { return Err(DecoderError::BufferOverflow); }`.
     - Handle `id3_skip_remaining > 0` and `encoded_in.starts_with(b"ID3")` (same logic as `Mp3Decoder`).
     - If `encoded_in.len() < 4 { return Err(DecoderError::NeedMoreData); }`.
     - **Partial-frame & sync-alignment guard** (prevents `nanomp3_core` from wiping `dec.st` and discarding partial MP3 frames at TCP chunk boundaries):
       ```rust
       let mut sync_offset = None;
       let mut idx = 0;
       while idx + 4 <= encoded_in.len() {
           let h = nanomp3_core::__private::Header::read(&encoded_in[idx..]);
           if h.valid() {
               sync_offset = Some((idx, h));
               break;
           }
           idx += 1;
       }

       let slice_to_decode = match sync_offset {
           None => {
               let skip = encoded_in.len().saturating_sub(3);
               if skip > 0 {
                   return Ok((skip, 0));
               }
               return Err(DecoderError::NeedMoreData);
           }
           Some((offset, _)) if offset > 0 => {
               return Ok((offset, 0));
           }
           Some((0, h)) => {
               let frame_bytes = h.frame_bytes(0) + h.padding();
               if frame_bytes > 0 {
                   if encoded_in.len() < frame_bytes + 4 {
                       return Err(DecoderError::NeedMoreData);
                   }
                   let next_h = nanomp3_core::__private::Header::read(&encoded_in[frame_bytes..]);
                   if !h.compare(next_h) {
                       return Ok((1, 0));
                   }
                   &encoded_in[..frame_bytes + 4]
               } else {
                   encoded_in
               }
           }
           _ => unreachable!(),
       };
       ```
     - Call `let (consumed, result) = self.decoder.decode(slice_to_decode, pcm_out);` (writing `i32` samples directly into `pcm_out`!).
     - On `Ok(info)`:
       - If `info.sample_rate > 0 { self.sample_rate = info.sample_rate; }`.
       - `let channels = info.channels.num() as usize;`
       - `let frames_produced = info.samples_produced;`
       - `let max_frames = pcm_out.len() / 2;`
       - `if max_frames < frames_produced { return Err(DecoderError::BufferOverflow); }`
       - If `channels == 1`: expand mono in-place in reverse:
         ```rust
         for i in (0..frames_produced).rev() {
             let s = pcm_out[i];
             pcm_out[i * 2] = s;
             pcm_out[i * 2 + 1] = s;
         }
         ```
       - (For `channels == 2`, `pcm_out[..frames_produced * 2]` is already interleaved stereo `i32`!)
       - Return `Ok((consumed, frames_produced))`.
     - Handle `Err(DecodeError::NoFrame)`, `Err(DecodeError::ReservoirUnavailable(_))`, and `Err(_)` identically to `Mp3Decoder`.
     - In `fn flush(&mut self)`, call `self.reset_in_place()`.
   - Add a unit test `test_static_mp3_decoder_partial_frame_and_id3` in `squeezelite-decoders/src/lib.rs` verifying:
     1) `StaticMp3Decoder::new()` and `flush()` / `reset_in_place()`.
     2) When a valid MPEG-1 Layer III 128 kbps 44.1 kHz frame header (`[0xFF, 0xFB, 0x90, 0x00]`, `frame_bytes = 417`) is presented with `< 421` bytes (e.g., `250` bytes), `StaticMp3Decoder::decode` returns `Err(DecoderError::NeedMoreData)` instead of discarding the partial frame!
     3) Leading non-sync bytes before `[0xFF, 0xFB, 0x90, 0x00]` return `Ok((offset, 0))` to align the frame header to index `0`.

2. **`/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`**:
   - Set on `#[cfg(target_arch = "xtensa")]`:
     ```rust
     #[cfg(target_arch = "xtensa")]
     pub const PCM_SCRATCH_SAMPLES: usize = 2304;
     ```
     and
     ```rust
     #[cfg(target_arch = "xtensa")]
     pub const READ_SCRATCH_BYTES: usize = 2048;
     ```
   - In `pub enum ActiveDecoder`:
     Add on `#[cfg(target_arch = "xtensa")]`:
     ```rust
     #[cfg(target_arch = "xtensa")]
     Mp3(&'static mut squeezelite_decoders::StaticMp3Decoder),
     ```
     and wire `Self::Mp3(d)` in `AudioDecoder::decode`, `AudioDecoder::flush`, and `ActiveDecoder::sample_rate()` on `#[cfg(target_arch = "xtensa")]` (just like `#[cfg(not(target_arch = "xtensa"))]`).

3. **`/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`**:
   - In `run_firmware_tick` (`#[cfg(target_arch = "xtensa")]`, lines 56-63):
     Allow `coordinator.process_audio_step(pipeline)` to advance when `pipeline.has_pending_pcm()`, and before `process_audio_step` (when `coordinator.state == louder_esp32_app::PlayerState::Playing && pipeline.output_buffer.available_read() == 0 && !pipeline.has_pending_pcm()`), run up to 4 decode steps while `matches!(pipeline.process_decode_step(), Ok(c) if c > 0) && pipeline.output_buffer.available_read() == 0 && !pipeline.has_pending_pcm()` so ID3 tags, leading non-sync bytes, and `ReservoirUnavailable` warmup frames are skipped immediately in a single tick.
   - In `mod xtensa_hw`:
     - Add a zero-heap `.bss` helper for `StaticMp3Decoder`:
       ```rust
       #[allow(unsafe_code)]
       fn take_static_mp3_decoder() -> &'static mut squeezelite_decoders::StaticMp3Decoder {
           static mut STATIC_MP3_DEC: squeezelite_decoders::StaticMp3Decoder =
               squeezelite_decoders::StaticMp3Decoder::new();
           unsafe {
               let dec = &mut *core::ptr::addr_of_mut!(STATIC_MP3_DEC);
               dec.reset_in_place();
               dec
           }
       }
       ```
     - Move the fixed-size runtime buffers out of `run_hardware_firmware()`'s stack frame into a `.bss` struct (`FirmwareBssBuffers`) with a `#[allow(unsafe_code)] fn take_firmware_bss_buffers() -> &'static mut FirmwareBssBuffers`:
       - `stream_storage: [u8; 8192]`
       - `output_storage: [u8; 8192]`
       - `http_rx_buf: [u8; 1024]`
       - `http_tx_buf: [u8; 1024]`
       - `slim_rx_buf: [u8; 2048]`
       - `slim_tx_buf: [u8; 4096]`
       - `audio_rx_buf: [u8; 8192]`
       - `audio_tx_buf: [u8; 1024]`
       - `slim_acc_buf: [u8; 2048]`
       - `audio_req_buf: [u8; 2048]`
       - `audio_header_buf: [u8; 2048]`
     - In `HELO` (`src/main.rs:1126`):
       Advertise:
       `"Model=SqueezeESP32,ModelName=SqueezeESP32,AccuratePlayPoints=1,HasDigitalOut=1,MaxSampleRate=48000,pcm,wav,aif,mp3"`
     - In `StreamCommandType::Start`:
       - Reset `icy_meta_interval = 0; icy_meta_next = 0; icy_meta_left = 0;`
       - Always drop any previous `ActiveDecoder::Mp3` borrow first by setting `pipeline.decoder = ActiveDecoder::Pcm(PcmDecoder::new(bits, channels, is_be));`
       - If `format == squeezelite_proto::AudioFormat::Mp3`, call `pipeline.set_decoder(ActiveDecoder::Mp3(take_static_mp3_decoder()));`, else `pipeline.set_decoder(ActiveDecoder::Pcm(PcmDecoder::new(bits, channels, is_be)));`.
       - Copy up to `audio_req_buf.len()` (`2048` bytes) of `http_headers`.
     - In `ServerCommand::Unknown { opcode, payload }`:
       - If `let Some((_spdif, dac_enable)) = cmd.audio_enable()` -> handle as before.
       - Else if `let Some(metaint) = cmd.stream_continue()` (`cont` packet):
         ```rust
         coordinator.awaiting_cont = false;
         icy_meta_interval = metaint as usize;
         icy_meta_next = metaint as usize;
         icy_meta_left = 0;
         if !threshold_sent && !coordinator.autostart_mode.is_auto() {
             threshold_sent = true;
             let mut stat_buf = [0u8; 128];
             if let Ok(stat_len) = StatBuilder::build_stat_with_metrics(
                 StatEvent::BUFFER_THRESHOLD,
                 pipeline.stream_buffer_available() as u32,
                 pipeline.output_buffer_available() as u32,
                 pipeline.frames_played,
                 coordinator.current_sample_rate,
                 coordinator.stream_bytes_received,
                 now_ms as u32,
                 0,
                 &mut stat_buf,
             ) {
                 let slim_sock = sockets.get_mut::<tcp::Socket>(slim_handle);
                 if slim_sock.may_send() {
                     let _ = slim_sock.send_slice(&stat_buf[..stat_len]);
                 }
             }
         }
         ```
       - Else if `opcode == [b'c', b'o', b'd', b'c'] && !payload.is_empty()` (`codc` packet):
         ```rust
         let codc_fmt = squeezelite_proto::AudioFormat::from_byte(payload[0]);
         coordinator.current_format = Some(codc_fmt);
         pipeline.decoder = ActiveDecoder::Pcm(PcmDecoder::new(16, 2, true));
         if codc_fmt == squeezelite_proto::AudioFormat::Mp3 {
             pipeline.set_decoder(ActiveDecoder::Mp3(take_static_mp3_decoder()));
         } else {
             let bits = if payload.len() > 1 { PlayerCoordinator::normalize_pcm_bits(payload[1]) } else { 16 };
             let channels = if payload.len() > 3 { PlayerCoordinator::normalize_pcm_channels(payload[3]) } else { 2 };
             let is_be = if payload.len() > 4 { matches!(Endianness::from_byte(payload[4]), Endianness::BigEndian) } else { true };
             pipeline.set_decoder(ActiveDecoder::Pcm(PcmDecoder::new(bits, channels, is_be)));
         }
         ```
     - In `audio_sock` HTTP response header parsing (`!audio_headers_stripped`):
       Use `audio_sock.recv(|rx_buf| ...)` so we dequeue from `audio_sock` **strictly up to the end of `\r\n\r\n` or `\n\n`** (leaving byte `0` of the HTTP body untouched in `audio_sock` until `audio_headers_stripped && !coordinator.awaiting_cont`!):
       - When `\r\n\r\n` or `\n\n` (or buffer limit) is reached:
         - `audio_headers_stripped = true;`
         - Send `RESP` (`&audio_header_buf[..audio_header_len]`) to LMS over `slim_sock` using a `2064`-byte `resp_buf`.
         - If `!threshold_sent && !coordinator.autostart_mode.is_auto() && !coordinator.awaiting_cont`, send `STMl` (`BUFFER_THRESHOLD`).
     - In `audio_sock` body reading (`else if !audio_http_eof && !coordinator.awaiting_cont`):
       - Loop while `sockets.get_mut::<tcp::Socket>(audio_handle).can_recv()`:
         - If `icy_meta_interval > 0 && icy_meta_next == 0`:
           - If `icy_meta_left == 0`:
             Read 1 byte `c` from `audio_sock`. If `n == 0 { break; }`.
             `icy_meta_left = (c as usize) * 16;`
             If `icy_meta_left == 0 { icy_meta_next = icy_meta_interval; continue; }`
           - If `icy_meta_left > 0`:
             let `to_skip = icy_meta_left.min(audio_rx_chunk.len());`
             Read `n = audio_sock.recv_slice(&mut audio_rx_chunk[..to_skip]).unwrap_or(0);`
             If `n == 0 { break; }`
             `icy_meta_left -= n;`
             If `icy_meta_left == 0 { icy_meta_next = icy_meta_interval; }`
             `continue;`
         - Else (reading audio payload bytes):
           `let avail_write = pipeline.stream_buffer.available_write();`
           `if avail_write < 512 { break; }`
           `let max_chunk = if icy_meta_interval > 0 { audio_rx_chunk.len().min(icy_meta_next).min(avail_write) } else { audio_rx_chunk.len().min(avail_write) };`
           `if max_chunk == 0 { break; }`
           `let n = sockets.get_mut::<tcp::Socket>(audio_handle).recv_slice(&mut audio_rx_chunk[..max_chunk]).unwrap_or(0);`
           `if n > 0 {`
               `let ingested = pipeline.ingest_stream_data(&audio_rx_chunk[..n]).unwrap_or(0);`
               `coordinator.stream_bytes_received += ingested as u64;`
               `if icy_meta_interval > 0 { icy_meta_next = icy_meta_next.saturating_sub(ingested); }`
           `} else { break; }`
     - At EOF (`audio_stream_active && audio_http_eof`):
       - Before checking `DecodeReady` (`STMd`), if `!pipeline.has_pending_pcm() && pipeline.stream_buffer.available_read() > 0`, if `matches!(pipeline.process_decode_step(), Err(squeezelite_decoders::DecoderError::NeedMoreData))` then call `pipeline.stream_buffer.clear();` so trailing un-decodable ID3v1 tags or partial tail bytes at HTTP EOF don't block `STMd`.
       - Also require `!pipeline.has_pending_pcm()` in both the `DecodeReady` (`STMd`) check and the `TrackUnderrun` (`STMu`) check.

## 2. Verification & `--stage-only` Gates (Mandatory)
1. `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace` (`exit code 0`)
2. `cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets -- -D warnings` (`exit code 0`)
3. `source /home/console-admin/export-esp.sh && cd /home/console-admin/Github/squeezelite-esp32/rust && rustup run esp cargo build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app` (`exit code 0`)
4. `espflash save-image --chip esp32 /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (`exit code 0`)
5. `python3 /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_staged_ota_bin.py` (`[STAGE-ONLY AUDIT PASS]`, `exit code 0`)
6. Commit the changes on `MK5-Rust` and record the commit hash, binary size, and SHA-256 in the Fulfillment Summary.
7. **NEVER touch `/dev/ttyUSB0` or `172.22.1.15` in this `--stage-only` work order.**

## 3. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent Role**: Firmware Implementation Specialist
- **Completion Timestamp**: 2026-10-24T15:25:22-04:00
- **Git Commit**: `9f17dbdcf1700e562fc9101a3717a14d96d4b62f` (branch: `MK5-Rust`)
- **Staged OTA Binary Path**: `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`
- **Binary Size**: 749,808 bytes (27.91% of `ota_0`, 18.16% of 4MB flash image)
- **SHA-256**: `edb766d245ed19fec5b2b3f6e6276662085028df6b81e4ed88499fb0c4ed8a33`
- **Validation Audit**: `[STAGE-ONLY AUDIT PASS]` (img_magic=0xE9 app_desc_magic=0xABCD5432 project_name='Squeezelite-ESP32' version='I2S-4MFlash-Rust' idf_ver='4.3.0')

### Verification Results
1. Gate 1 (`cargo test --workspace`): PASS (179 tests passed, 0 failed)
2. Gate 2 (`cargo clippy --workspace --all-targets -- -D warnings`): PASS (0 warnings)
3. Gate 3 (`cargo build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`): PASS (Exit code 0)
4. Gate 4 (`espflash save-image --chip esp32`): PASS (Exit code 0, 749,808 bytes)
5. Gate 5 (`verify_staged_ota_bin.py`): PASS (`[STAGE-ONLY AUDIT PASS]`, Exit code 0)
6. Scope Constraint: Strictly `--stage-only`. `/dev/ttyUSB0` and `172.22.1.15` were never accessed.

