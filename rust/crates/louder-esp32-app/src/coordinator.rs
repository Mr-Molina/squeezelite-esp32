//! Player Coordinator
//!
//! The single player engine shared by host and device. It orchestrates the SlimProto control
//! connection, the HTTP audio stream, the audio decoding pipeline, the amplifier/clock backend
//! and CYD display status updates.
//!
//! All I/O goes through the [`crate::io_traits`] seams: [`SlimTransport`] (control frames),
//! [`StreamSource`] (HTTP audio) and [`AudioBackend`] (I2S clock and amplifier). Platform
//! differences live in the trait implementations and in bring-up, never in this file (enforced by
//! `tests/no_cfg_invariant.rs`). [`PlayerCoordinator::tick`] is the one entry point that drives
//! the protocol; the CYD entry points ([`PlayerCoordinator::handle_cyd_command`],
//! [`PlayerCoordinator::sync_cyd_telemetry`], [`PlayerCoordinator::cyd_playback_state`]) are
//! independent of the network.

extern crate alloc;

use alloc::vec::Vec;

use squeezelite_audio_core::output::AudioOutputSink;
use squeezelite_audio_core::VolumeScale;
use squeezelite_cyd_link::{CydCommand, CydMessage, PlayState};
use squeezelite_decoders::{DecoderError, PcmDecoder};
use squeezelite_proto::{
    build_capabilities, sample_rate_from_index, AudioFormat, AutostartMode, DisconnectReason,
    Endianness, ServerCommand, SlimprotoBuilder, SlimprotoParser, StatBuilder, StatEvent,
    StreamCommandType, TransitionType, SQUEEZENETWORK_MARKER_IP, STRM_FLAG_SERVER_REBOOT, SYNCGROUP_ID_LEN,
};

use crate::amp::{lms_gain_to_hardware_volume_percent, lms_gain_to_tas5805m_vol_reg, percent_to_lms_gain};
use crate::config_handle::ConfigHandle;
use crate::io_traits::{
    AudioBackend, NetAddr, SlimTransport, StreamError, StreamSource, TransportError,
};
use crate::pipeline::{ActiveDecoder, ChannelMode, FadeMode, StreamPipeline};

/// Size of the stack buffer [`PlayerCoordinator::tick`] receives one server frame into. A larger
/// frame is dropped by the transport (reported as [`TransportError::Io`] and skipped).
pub const SERVER_FRAME_BUF_LEN: usize = 2048;

/// HTTP request sent when a `strm s` carries no request headers (`COORD-503`).
const DEFAULT_HTTP_REQUEST: &[u8] = b"GET /stream.mp3 HTTP/1.0\r\n\r\n";

/// Q16.16 unity gain (C `FIXED_ONE`).
const UNITY_GAIN: u32 = 0x1_0000;

/// Free output-ring space [`PlayerCoordinator::pump_decode`] requires before decoding.
pub const PUMP_DECODE_MIN_OUTPUT_SPACE: usize = 1024;

/// `ModelName` advertised in `HELO` (C: `-DMODEL_NAME=SqueezeESP32`, `CMakeLists.txt:5`). It is
/// the model, distinct from the player name (sent with `SETD`).
pub const HELO_MODEL_NAME: &str = "SqueezeESP32";

/// Codecs advertised in `HELO`: exactly the formats the firmware decodes (Phase 2 adds more).
pub const HELO_CODECS: &str = "flc,pcm,wav,aif,mp3";

/// `Firmware` advertised in `HELO` (C: `VERSION`, `squeezelite.h:31-35`).
pub const HELO_FIRMWARE: &str = concat!("v", env!("CARGO_PKG_VERSION"));

/// Capacity of the `HELO` capability string.
pub const HELO_CAPS_CAPACITY: usize = 256;

/// Undelivered CYD display updates held for [`PlayerCoordinator::take_cyd_message`]. One
/// tick produces at most three (a resolved stream connect, one server command, new ICY
/// metadata), so a caller that drains the queue after every tick never loses one.
pub const CYD_QUEUE_DEPTH: usize = 4;

/// C `OUTPUT_START_AT`: an unpause time further ahead than this starts output at once
/// (`output.c:101`).
const START_AT_MAX_AHEAD_MS: u32 = 10_000;

/// One queued CYD display update. Kept small (the coordinator is a static in `.bss`, which
/// the core-0 stack gives way to); [`PlayerCoordinator::take_cyd_message`] expands it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CydNote {
    State(PlayState),
    Volume(u8),
    /// The current ICY `StreamTitle` ([`PlayerCoordinator::track_metadata`]).
    Metadata,
}

/// Helper trait to construct `PcmDecoder` with standard configuration.
pub trait PcmDecoderExt {
    fn with_config(bits: u8, big_endian: bool, channels: u8) -> Self;
}

impl PcmDecoderExt for PcmDecoder {
    fn with_config(bits: u8, big_endian: bool, channels: u8) -> Self {
        PcmDecoder::new(bits, channels, big_endian)
    }
}

/// Playback operational state of the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerState {
    Disconnected,
    Connected,
    Buffering,
    Playing,
    Paused,
    Stopped,
}

impl PlayerState {
    #[allow(non_upper_case_globals)]
    pub const Streaming: PlayerState = PlayerState::Playing;
}

/// State of the coordinator's use of the [`StreamSource`].
#[derive(Debug, Clone, PartialEq, Eq)]
enum StreamLink {
    /// No stream (never started, stopped, flushed, or failed).
    Idle,
    /// `connect` returned [`StreamError::WouldBlock`]; it is repeated with the same arguments on
    /// every tick until it resolves.
    Connecting { addr: NetAddr, request: Vec<u8> },
    /// Connected; audio bytes are polled every tick.
    Open,
}

/// C `output.fade_mode = strm->transition_type - '0'` (`slimproto.c:486`): '1' crossfade, '2'
/// fade in, '3' fade out, '4' fade in and out; any other value is kept and arms nothing.
///
/// The variants map one to one, in declaration order, so this compiles to no lookup table
/// (esp-hal links switch tables into DRAM `.data`, which comes out of the core-0 stack).
///
/// Pre-existing parser divergence (doc only): `TransitionType::from_byte` also maps the raw
/// bytes 0x01-0x04 to modes (squeezelite-proto lib.rs:202-206), where C's `- '0'` gives values
/// that arm no fade. LMS sends ASCII '0'-'4'.
fn fade_mode_from_strm(transition: TransitionType) -> FadeMode {
    match transition {
        TransitionType::None => FadeMode::None,
        TransitionType::Crossfade => FadeMode::Cross,
        TransitionType::FadeIn => FadeMode::In,
        TransitionType::FadeOut => FadeMode::Out,
        TransitionType::FadeInOut => FadeMode::InOut,
        TransitionType::Unknown(_) => FadeMode::Other,
    }
}

/// The `strm s` fields [`PlayerCoordinator::start_stream`] uses.
struct StartParams<'a> {
    autostart: AutostartMode,
    flags: u8,
    format: AudioFormat,
    pcm_sample_size: u8,
    pcm_sample_rate: u8,
    pcm_channels: u8,
    big_endian: bool,
    threshold_kb: u16,
    /// C `output.threshold` (tenths of a second before output starts).
    output_threshold: u8,
    /// C `output.next_replay_gain` (16.16 fixed point).
    replay_gain: u32,
    /// C `output.fade_mode` (`transition_type - '0'`).
    fade_mode: FadeMode,
    /// C `output.fade_secs` (`transition_period`).
    fade_secs: u8,
    server_port: u16,
    server_ip: [u8; 4],
    http_headers: &'a [u8],
}

/// Orchestrates network streaming, audio decoding, and player telemetry.
///
/// The protocol state mirrors C squeezelite's three state machines (`slimproto.c` status loop):
/// the stream (`StreamLink`, [`Self::stream_eof`], the private "past threshold" flag = C
/// `STREAMING_HTTP`), the decoder (private ready/running flags = C `DECODE_READY` /
/// `DECODE_RUNNING`) and the output ([`Self::state`] plus the private "output started" flag:
/// `Playing` with output started = C `OUTPUT_RUNNING`).
pub struct PlayerCoordinator {
    pub state: PlayerState,
    pub mac: [u8; 6],
    pub model_name: &'static str,
    /// The software volume handed to the gain stage ([`StreamPipeline::set_volume`]): unity
    /// (or 0 for silence) when the amplifier does the volume, else the `audg` gains. The
    /// volume LMS asked for is [`StreamPipeline::requested_volume_q16`].
    pub volume: VolumeScale,
    pub frames_played: u64,
    pub last_heartbeat_ms: u32,
    pub heartbeat_interval_ms: u32,
    pub current_format: Option<AudioFormat>,
    pub current_sample_rate: u32,
    /// `strm s` threshold in bytes (C `stream.threshold`), clamped to the stream ring.
    pub buffer_threshold_bytes: usize,
    pub autostart: bool,
    pub autostart_mode: AutostartMode,
    pub awaiting_cont: bool,
    pub stream_bytes_received: u64,
    /// `STMd` was reported (or is pending) for the current track (C `DECODE_COMPLETE`).
    pub decode_complete_sent: bool,
    /// `STMd` waits for the status pass.
    pub pending_decode_ready: bool,
    /// `STMs` waits for the status pass.
    pub pending_track_started: bool,
    /// `STMu` waits for the status pass.
    pub pending_underrun: bool,
    /// `DSCO` waits for the status pass.
    pub pending_dsco: Option<DisconnectReason>,
    /// `STMl` (buffer threshold reached, autostart off) waits for the status pass.
    pub pending_buffer_threshold: bool,
    /// `STMo` (output underrun while streaming) waits for the status pass.
    pub pending_output_underrun: bool,
    /// `STMn` (decoder error) waits for the status pass.
    pub pending_not_supported: bool,
    /// Response head of the audio stream, forwarded to LMS as `RESP` on the next status pass.
    pub pending_resp: Option<Vec<u8>>,
    /// ICY metadata block, forwarded to LMS as `META` on the next status pass.
    pub pending_meta: Option<Vec<u8>>,
    pub dac_enabled: bool,
    pub server_rebooting: bool,
    pub player_name: heapless::String<64>,
    pub stream_eof: bool,
    /// IPv4 address of the LMS server. A `strm s` whose `server_ip` is `0.0.0.0` streams from
    /// this address (bring-up sets it to the control connection's peer; defaults to loopback).
    pub lms_ip: [u8; 4],
    /// Decode and sink output are scheduled by the caller through [`Self::pump_decode`] /
    /// [`Self::pump_output`] (device: DMA-first pump phases); [`Self::tick`] then only
    /// evaluates the play state and never decodes or writes to the sink itself. `false`
    /// (default, host): `tick` runs one decode + output step.
    pub external_audio_pump: bool,
    /// Runtime configuration the player name is persisted through (spec §3.4: `setd` id-0
    /// writes persist the name). `None` (host default): the name only lives in this struct.
    pub config: Option<&'static ConfigHandle>,
    /// `strm p` with an interval: output is silent until this time, then plays on by itself
    /// (C `OUTPUT_PAUSE_FRAMES`).
    pub pause_until_ms: Option<u32>,
    /// `strm u` with jiffies: output stays silent until this time (C `OUTPUT_START_AT`).
    pub unpause_at_jiffies: Option<u32>,
    stream: StreamLink,
    /// Why the stream socket ended, once it did (C `stream.disconnect`).
    stream_end: Option<DisconnectReason>,
    /// More than the threshold arrived (C `STREAMING_HTTP`, `stream.c:527`).
    stream_past_threshold: bool,
    /// A decoder for the current format is installed and not started (C `DECODE_READY`).
    decode_ready: bool,
    /// The decoder runs for the current track (C `DECODE_RUNNING`).
    decode_running: bool,
    /// The current track reached the sink (C output went `RUNNING`).
    output_started: bool,
    /// C's output went `RUNNING` (`strm u`, an elapsed `strm p` interval or `strm u` jiffies)
    /// while the track was still buffering: it plays as soon as the decoder starts, whatever
    /// the autostart mode. Rust decodes only while Playing, so the start waits for the
    /// threshold as C's decoder does (`slimproto.c:825-834`).
    output_start_requested: bool,
    /// The stream ended cleanly since the last state evaluation. C's `DISCONNECT_OK` start
    /// condition holds only on the one status pass that sees `DISCONNECT` (`slimproto.c:804-808`
    /// sets STOPPED right after, `825-827` tests the state captured on that pass).
    disconnect_ok_unseen: bool,
    /// `STMo` was reported for the current track (C `sentSTMo`).
    output_underrun_reported: bool,
    /// `STMu` was reported for the current track (C `sentSTMu`).
    track_underrun_reported: bool,
    /// The decoder failed on the current track (C `DECODE_ERROR`).
    decode_failed: bool,
    /// Another source (Bluetooth) feeds the pipeline: no LMS output logic runs.
    external_playback: bool,
    /// A `strm s` track is loaded (cleared by `strm q` and external playback). Without one
    /// (e.g. a CYD Play with nothing streamed) no LMS end-of-track event is reported.
    lms_track: bool,
    cyd_queue: heapless::Deque<CydNote, CYD_QUEUE_DEPTH>,
    /// `StreamTitle` value of the latest ICY metadata block (CYD `META=`).
    track_metadata: Option<Vec<u8>>,
    /// `serv`: the server to switch to (C `new_server`).
    server_switch: Option<[u8; 4]>,
    /// `serv` SyncgroupID for the next `HELO` (C `new_server_cap`).
    syncgroup: Option<[u8; SYNCGROUP_ID_LEN]>,
    last_tick_frames: usize,
    /// `STMl` was reported (or is pending) for the current track (C `sentSTMl`).
    buffer_threshold_reported: bool,
    /// Buffer server frames are received into (see [`Self::set_frame_buffer`]); `None` uses a
    /// [`SERVER_FRAME_BUF_LEN`]-byte stack buffer.
    frame_buffer: Option<&'static mut [u8]>,
}

impl PlayerCoordinator {
    /// Creates a new coordinator instance.
    pub fn new(mac: [u8; 6], model_name: &'static str) -> Self {
        let mut player_name = heapless::String::new();
        let safe_len = model_name.floor_char_boundary(64.min(model_name.len()));
        let _ = player_name.push_str(&model_name[..safe_len]);
        Self {
            state: PlayerState::Disconnected,
            mac,
            model_name,
            volume: VolumeScale::unity(),
            frames_played: 0,
            last_heartbeat_ms: 0,
            heartbeat_interval_ms: 1000,
            current_format: None,
            current_sample_rate: 44100,
            buffer_threshold_bytes: 0,
            autostart: true,
            autostart_mode: AutostartMode::Auto,
            awaiting_cont: false,
            stream_bytes_received: 0,
            decode_complete_sent: false,
            pending_decode_ready: false,
            pending_track_started: false,
            pending_underrun: false,
            pending_dsco: None,
            pending_buffer_threshold: false,
            pending_output_underrun: false,
            pending_not_supported: false,
            pending_resp: None,
            pending_meta: None,
            dac_enabled: true,
            server_rebooting: false,
            player_name,
            stream_eof: false,
            lms_ip: [127, 0, 0, 1],
            external_audio_pump: false,
            config: None,
            pause_until_ms: None,
            unpause_at_jiffies: None,
            stream: StreamLink::Idle,
            stream_end: None,
            stream_past_threshold: false,
            decode_ready: false,
            decode_running: false,
            output_started: false,
            output_start_requested: false,
            disconnect_ok_unseen: false,
            output_underrun_reported: false,
            track_underrun_reported: false,
            decode_failed: false,
            external_playback: false,
            lms_track: false,
            cyd_queue: heapless::Deque::new(),
            track_metadata: None,
            server_switch: None,
            syncgroup: None,
            last_tick_frames: 0,
            buffer_threshold_reported: false,
            frame_buffer: None,
        }
    }

    /// Receive server frames into `buffer` instead of a [`SERVER_FRAME_BUF_LEN`]-byte stack
    /// buffer (the firmware passes a static one, keeping 2 KB off the task stack). A frame
    /// longer than `buffer` is dropped by the transport.
    pub fn set_frame_buffer(&mut self, buffer: &'static mut [u8]) {
        self.frame_buffer = Some(buffer);
    }

    /// The `HELO` capability string (see [`build_capabilities`] for the field-by-field C
    /// comparison), with `syncgroup` appended as C's `var_cap` does.
    pub fn helo_capabilities(
        syncgroup: Option<&str>,
    ) -> Result<heapless::String<HELO_CAPS_CAPACITY>, TransportError> {
        let mut caps = heapless::String::new();
        build_capabilities(
            HELO_MODEL_NAME,
            HELO_FIRMWARE,
            crate::clock_table::max_verified_rate(),
            syncgroup,
            HELO_CODECS,
            &mut caps,
        )
        .map_err(|_| TransportError::Io)?;
        Ok(caps)
    }

    /// Announces the player on a freshly connected control link: `HELO` (this player's MAC,
    /// `device_id`, the capabilities from [`Self::helo_capabilities`]) and the player name
    /// (`SETD`). A player that was never connected becomes [`PlayerState::Connected`]; a
    /// reconnect leaves the play state alone.
    ///
    /// C parity: `reconnect` sets the `0x4000` flag and the stream byte count is carried over
    /// (`slimproto.c:158-160`); a pending `serv` SyncgroupID is sent with this one `HELO` and
    /// then dropped (`slimproto.c:1171-1174`). Unlike before, no heartbeat is armed: C sends
    /// no `STAT` after `HELO` until output runs or LMS asks with `strm t`.
    #[inline(never)]
    pub fn hello<T: SlimTransport>(
        &mut self,
        transport: &mut T,
        device_id: u8,
        reconnect: bool,
    ) -> Result<(), TransportError> {
        let syncgroup = self.syncgroup.as_ref().and_then(|id| core::str::from_utf8(id).ok());
        let caps = Self::helo_capabilities(syncgroup)?;
        let mut buf = [0u8; 44 + HELO_CAPS_CAPACITY];
        let len = SlimprotoBuilder::build_helo_with_options(
            &self.mac,
            device_id,
            &caps,
            reconnect,
            self.stream_bytes_received,
            &mut buf,
        )
        .map_err(|_| TransportError::Io)?;
        transport.send(&buf[..len])?;
        // Used for this one HELO (slimproto.c:1171-1174), once it went out.
        self.syncgroup = None;
        // Deliberate divergence: C sends no SETD after HELO (it answers `setd` queries only,
        // slimproto.c:589-593). The Rust firmware announces its name here since Task 9; LMS
        // accepts the unsolicited name.
        send_setd_name(transport, &self.player_name)?;
        if self.state == PlayerState::Disconnected {
            self.state = PlayerState::Connected;
        }
        Ok(())
    }

    /// Another audio source (device: Bluetooth A2DP) starts feeding the pipeline while no LMS
    /// stream plays: [`PlayerState::Playing`], with every per-track LMS event cleared, `STMs`
    /// marked as reported and a finished LMS stream detached, so nothing about it is reported
    /// to LMS (no LMS output runs, so no heartbeat either).
    pub fn begin_external_playback(&mut self) {
        self.reset();
        self.stream = StreamLink::Idle;
        self.buffer_threshold_bytes = 0;
        self.buffer_threshold_reported = true;
        self.output_underrun_reported = true;
        self.external_playback = true;
        self.state = PlayerState::Playing;
    }

    /// The external source stopped and its audio drained: [`PlayerState::Stopped`].
    pub fn end_external_playback(&mut self) {
        self.external_playback = false;
        self.state = PlayerState::Stopped;
    }

    /// Sets the stream EOF indicator.
    pub fn set_stream_eof(&mut self, eof: bool) {
        self.stream_eof = eof;
    }

    /// Returns `true` if the audio stream reached EOF.
    pub fn is_stream_eof(&self) -> bool {
        self.stream_eof
    }

    /// Returns `true` while an audio stream is connecting or connected.
    pub fn stream_active(&self) -> bool {
        !matches!(self.stream, StreamLink::Idle)
    }

    /// Takes the oldest undelivered CYD display update, if any. Call it until it returns
    /// `None` after every [`Self::tick`]: see [`CYD_QUEUE_DEPTH`].
    pub fn take_cyd_message(&mut self) -> Option<CydMessage> {
        Some(match self.cyd_queue.pop_front()? {
            CydNote::State(state) => CydMessage::PlaybackState(state),
            CydNote::Volume(pct) => CydMessage::Volume(pct),
            CydNote::Metadata => self.track_metadata_message()?,
        })
    }

    /// C `set_volume(left, right)` (output_embedded.c:118-126) for an `audg`.
    ///
    /// - A backend with hardware volume (the device's TAS5805M) does the attenuation, as C
    ///   skips `output.gainL/R` when its `volume_cb` handles the volume. Rust enhancement (Task
    ///   7 ruling; C has no TAS5805M driver): one register code from the louder channel drives
    ///   both amplifier channels. The balance C keeps with independent `gainL`/`gainR`
    ///   (output.c:54-55; `Balance=1`, embedded.h:47) is the software gain: each channel's
    ///   ratio to the louder one, `l * 65536 / max(l, r)` (Task 14 fix round), exactly unity
    ///   when `l == r`, so equal gains behave as Task 7. Both are 0 for a 0 gain, as C's I2S
    ///   volume callback asserts the mute GPIO on `(left | right) == 0` (output_i2s.c:496).
    /// - Otherwise (host, any backend without hardware volume) the gains go into the software
    ///   gain stage per channel, as C (output_embedded.c:120-124).
    ///
    /// Replay gain, fade, polarity and channel select are applied in software either way.
    pub fn set_volume<B: AudioBackend, S: AudioOutputSink>(
        &mut self,
        left: u32,
        right: u32,
        backend: &mut B,
        pipeline: &mut StreamPipeline<'_, S>,
    ) {
        // C parity: output_embedded.c:126 `equalizer_set_volume(left, right)` always sees the
        // requested volume; kept for the loudness contour (Task 16).
        pipeline.set_requested_volume(left, right);
        let (soft_left, soft_right) = if backend.has_hw_volume() {
            let louder = left.max(right);
            let reg = lms_gain_to_tas5805m_vol_reg(louder);
            backend.set_hw_volume(reg, reg);
            if louder == 0 {
                (0, 0)
            } else {
                // In u64: `l << 16` overflows u32 at l = 0x10000 (balanced full volume).
                let ratio = |g: u32| ((u64::from(g) << 16) / u64::from(louder)) as u32;
                (ratio(left), ratio(right))
            }
        } else {
            (left, right)
        };
        self.volume.gain_left = soft_left;
        self.volume.gain_right = soft_right;
        pipeline.set_volume(soft_left, soft_right);
    }

    /// A local volume change in percent (CYD, HTTP/IR, boot): the louder channel goes to
    /// `percent` on the LMS curve ([`percent_to_lms_gain`], so the amplifier register is
    /// `percent_to_attenuation(percent)`), the other keeps its ratio to it from the volume LMS
    /// last asked for, and the result goes through [`Self::set_volume`], the single volume
    /// entry. After a `(0, 0)` volume the balance is unknown and both channels get the level.
    pub fn set_volume_percent<B: AudioBackend, S: AudioOutputSink>(
        &mut self,
        percent: u8,
        backend: &mut B,
        pipeline: &mut StreamPipeline<'_, S>,
    ) {
        let level = percent_to_lms_gain(percent);
        let (left, right) = pipeline.requested_volume_q16();
        let louder = left.max(right);
        let (left, right) = if louder == 0 {
            (level, level)
        } else {
            let scale = |g: u32| (u64::from(level) * u64::from(g) / u64::from(louder)) as u32;
            (scale(left), scale(right))
        };
        self.set_volume(left, right, backend, pipeline);
    }

    /// Queues a CYD update. A full queue (a caller that stopped draining it) drops the oldest.
    fn push_cyd(&mut self, note: CydNote) {
        if self.cyd_queue.is_full() {
            self.cyd_queue.pop_front();
        }
        let _ = self.cyd_queue.push_back(note);
    }

    /// The server to switch to after a `serv`, once (C `new_server`, `slimproto.c:633-637`).
    /// The caller drops the control connection, connects to it and sends `HELO` with
    /// `reconnect = false` (C `slimproto.c:1096-1103`).
    pub fn take_server_switch(&mut self) -> Option<[u8; 4]> {
        self.server_switch.take()
    }

    /// `CydMessage::TrackMetadata` for the current ICY `StreamTitle`, split into artist and
    /// title on the first `" - "` (best effort: no separator means title only).
    fn track_metadata_message(&self) -> Option<CydMessage> {
        let text = core::str::from_utf8(self.track_metadata.as_deref()?).ok()?;
        let (artist, title) = text.split_once(" - ").unwrap_or(("", text));
        Some(CydMessage::TrackMetadata { artist: truncated(artist), title: truncated(title) })
    }

    /// Number of audio frames written to the output sink during the last [`Self::tick`].
    pub fn last_tick_frames(&self) -> usize {
        self.last_tick_frames
    }

    /// Resets transient stream coordinator state.
    pub fn reset(&mut self) {
        self.stream_eof = false;
        self.decode_complete_sent = false;
        self.pending_decode_ready = false;
        self.pending_track_started = false;
        self.pending_underrun = false;
        self.pending_output_underrun = false;
        self.pending_not_supported = false;
        self.pending_dsco = None;
        self.pending_buffer_threshold = false;
        self.pause_until_ms = None;
        self.unpause_at_jiffies = None;
        self.stream_end = None;
        self.stream_past_threshold = false;
        self.decode_ready = false;
        self.decode_running = false;
        self.output_started = false;
        self.output_start_requested = false;
        self.disconnect_ok_unseen = false;
        self.output_underrun_reported = false;
        self.track_underrun_reported = false;
        self.decode_failed = false;
        self.external_playback = false;
        self.lms_track = false;
        // A new track (`strm s`) or a stop (`strm q`) ends the previous ICY title.
        self.track_metadata = None;
    }

    /// Decodes SlimProto sample rate code into Hz.
    /// Supports both ASCII '0'..='9', numeric 0..=9 codes, and colon ':' / 10 for 88.2 kHz (`RUST-16`).
    pub fn decode_pcm_sample_rate_hz(code: u8) -> Option<u32> {
        match code {
            b'0' | 0 => Some(11025),
            b'1' | 1 => Some(22050),
            b'2' | 2 => Some(32000),
            b'3' | 3 => Some(44100),
            b'4' | 4 => Some(48000),
            b'5' | 5 => Some(8000),
            b'6' | 6 => Some(12000),
            b'7' | 7 => Some(16000),
            b'8' | 8 => Some(24000),
            b'9' | 9 => Some(96000),
            b':' | 10 => Some(88200),
            b';' | 11 => Some(176400),
            b'<' | 12 => Some(192000),
            _ => None,
        }
    }

    /// Normalizes SlimProto sample size code into bits per sample.
    pub fn normalize_pcm_bits(code: u8) -> u8 {
        match code {
            b'0' | 0 | 8 => 8,
            b'1' | 1 | 16 => 16,
            b'2' | 2 | 24 => 24,
            b'3' | 3 | 32 => 32,
            _ => 16,
        }
    }

    /// Normalizes SlimProto channel code into channel count.
    pub fn normalize_pcm_channels(code: u8) -> u8 {
        match code {
            b'1' | 1 => 1,
            b'2' | 2 => 2,
            _ => 2,
        }
    }

    /// Normalizes SlimProto endianness code (ASCII '0' or 0 is Big Endian).
    pub fn normalize_pcm_big_endian(code: u8) -> bool {
        code == b'0' || code == 0
    }

    /// Runs one coordinator cycle (`RUST-01..RUST-10`):
    ///
    /// 1. resumes a pending stream `connect` (one that returned [`StreamError::WouldBlock`]):
    ///    `STMc` either way, then `DSCO(3)` from the status pass if it failed;
    /// 2. receives at most one server frame from `transport` and handles it (any CYD update it
    ///    produces is queued for [`Self::take_cyd_message`]);
    /// 3. moves HTTP audio bytes from `source` into the pipeline stream buffer;
    /// 4. applies the output deadlines (`strm p` interval, `strm u` jiffies), evaluates the
    ///    playback state machine, decodes and writes audio to the sink;
    /// 5. runs the status pass ([`Self::check_heartbeat`], C `slimproto.c:774-920`): `RESP`,
    ///    `DSCO`, `STMs`, `STMd`, `STMt`, `STMl`, `STMu`, `STMo`, `STMn`, `META`.
    ///
    /// Malformed or oversized server frames are skipped. On [`TransportError::Disconnected`]
    /// from `transport` the audio steps still run (buffered audio keeps playing) and the error
    /// is returned without sending anything. Errors from sending are returned as is; deferred
    /// events that were not sent stay pending for the next tick.
    pub fn tick<S: AudioOutputSink, T: SlimTransport, R: StreamSource, B: AudioBackend>(
        &mut self,
        transport: &mut T,
        source: &mut R,
        backend: &mut B,
        pipeline: &mut StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<(), TransportError> {
        self.last_tick_frames = 0;
        self.resume_stream_connect(transport, source, pipeline, now_ms)?;
        let link_lost = self.receive_command(transport, source, backend, pipeline, now_ms)?;
        let _ = self.poll_stream(source, pipeline);
        self.apply_output_deadlines(pipeline, backend, now_ms);
        self.last_tick_frames = if self.external_audio_pump {
            // Decode/output belong to the caller's pump phases; only the play state moves here.
            self.evaluate_state(pipeline);
            0
        } else {
            self.process_audio_step(pipeline)
        };
        // C parity (slimproto.c:842-844, 907): a decoder that stopped with the stream still
        // open disconnects it, without a DSCO (stream_disconnect sets STOPPED).
        if self.decode_failed && self.stream == StreamLink::Open {
            self.close_stream(source);
        }

        if link_lost {
            return Err(TransportError::Disconnected);
        }
        self.check_heartbeat(transport, pipeline, now_ms)
    }

    /// Step 2 of [`Self::tick`]: receives at most one server frame and handles it. Returns
    /// whether the control link is gone. Uses the buffer from [`Self::set_frame_buffer`], or
    /// a stack buffer in a separate out-of-line frame that is gone again before decoding.
    #[inline(never)]
    fn receive_command<S: AudioOutputSink, T: SlimTransport, R: StreamSource, B: AudioBackend>(
        &mut self,
        transport: &mut T,
        source: &mut R,
        backend: &mut B,
        pipeline: &mut StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<bool, TransportError> {
        match self.frame_buffer.take() {
            Some(buffer) => {
                let result =
                    self.receive_into(buffer, transport, source, backend, pipeline, now_ms);
                self.frame_buffer = Some(buffer);
                result
            }
            None => self.receive_on_stack(transport, source, backend, pipeline, now_ms),
        }
    }

    /// [`Self::receive_command`] without a configured frame buffer.
    #[inline(never)]
    fn receive_on_stack<S: AudioOutputSink, T: SlimTransport, R: StreamSource, B: AudioBackend>(
        &mut self,
        transport: &mut T,
        source: &mut R,
        backend: &mut B,
        pipeline: &mut StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<bool, TransportError> {
        let mut frame = [0u8; SERVER_FRAME_BUF_LEN];
        self.receive_into(&mut frame, transport, source, backend, pipeline, now_ms)
    }

    /// Receives at most one server frame into `frame` and handles it.
    #[inline(never)]
    fn receive_into<S: AudioOutputSink, T: SlimTransport, R: StreamSource, B: AudioBackend>(
        &mut self,
        frame: &mut [u8],
        transport: &mut T,
        source: &mut R,
        backend: &mut B,
        pipeline: &mut StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<bool, TransportError> {
        match transport.poll_frame(frame) {
            Ok(Some(len)) => {
                if let Ok((cmd, _)) = SlimprotoParser::parse_frame(&frame[..len]) {
                    self.handle_command(cmd, transport, source, backend, pipeline, now_ms)?;
                }
                Ok(false)
            }
            Ok(None) | Err(TransportError::WouldBlock) | Err(TransportError::Io) => Ok(false),
            Err(TransportError::Disconnected) => Ok(true),
        }
    }

    /// Repeats a `connect` that previously returned [`StreamError::WouldBlock`].
    fn resume_stream_connect<S: AudioOutputSink, T: SlimTransport, R: StreamSource>(
        &mut self,
        transport: &mut T,
        source: &mut R,
        pipeline: &StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<(), TransportError> {
        let result = match &self.stream {
            StreamLink::Connecting { addr, request } => source.connect(*addr, request),
            _ => return Ok(()),
        };
        match result {
            Ok(()) => {
                self.stream = StreamLink::Open;
                self.report_stream_connected(transport, pipeline, now_ms)
            }
            Err(StreamError::WouldBlock) => Ok(()),
            Err(_) => self.report_stream_unreachable(transport, source, pipeline, now_ms),
        }
    }

    /// `STMc`: the stream connect finished (C sends it right after `stream_sock` returns,
    /// `slimproto.c:473`).
    fn report_stream_connected<S: AudioOutputSink, T: SlimTransport>(
        &mut self,
        transport: &mut T,
        pipeline: &StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<(), TransportError> {
        send_stat(
            transport,
            StatEvent::Connected,
            pipeline.stream_buffer_available() as u32,
            pipeline.output_buffer_available() as u32,
            pipeline.frames_played as u32,
            now_ms,
        )
    }

    /// A stream that never delivered audio failed to connect (TCP failure, bad status, failed
    /// redirect hop, port 0, ...), immediately or after `WouldBlock`. C parity: `stream_sock`
    /// marks the stream `DISCONNECT`/`UNREACHABLE` (`stream.c:684-688`), `process_strm` still
    /// sends `STMc` (`slimproto.c:473`), and the status loop sends `DSCO(3)` exactly once
    /// (`slimproto.c:804-808`, `910`). No `STMn`: that is reserved for decoder errors. The
    /// player is Stopped (CYD too).
    fn report_stream_unreachable<S: AudioOutputSink, T: SlimTransport, R: StreamSource>(
        &mut self,
        transport: &mut T,
        source: &mut R,
        pipeline: &StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<(), TransportError> {
        source.abort();
        self.stream = StreamLink::Idle;
        self.stream_end = Some(DisconnectReason::Unreachable);
        self.pending_dsco = Some(DisconnectReason::Unreachable);
        // The connect concerns the stream only: an output still RUNNING with the previous
        // track's tail plays it out (then STMu), as C never touches the output here.
        if !self.output_running() {
            self.state = PlayerState::Stopped;
            self.push_cyd(CydNote::State(PlayState::Stopped));
        }
        self.report_stream_connected(transport, pipeline, now_ms)
    }

    /// Installs the decoder for `format` (`strm s`, `codc`): MP3 from the pipeline's MP3 slot
    /// (see [`StreamPipeline::use_mp3_decoder`]), FLAC reusing the current FLAC decoder, PCM
    /// with the given layout. A format this build cannot decode gets a PCM decoder with the
    /// output muted (never the previous decoder, which would turn the bytes into noise) until
    /// the next `strm s`. Returns whether the format is decodable.
    fn select_decoder<S: AudioOutputSink>(
        pipeline: &mut StreamPipeline<'_, S>,
        format: AudioFormat,
        bits: u8,
        channels: u8,
        big_endian: bool,
    ) -> bool {
        let supported = match format {
            AudioFormat::Mp3 => pipeline.use_mp3_decoder(),
            AudioFormat::Flac => {
                match &mut pipeline.decoder {
                    // Same codec again (`codc`): restart the decoder at a frame boundary.
                    ActiveDecoder::Flac(current) => current.flush(),
                    _ => pipeline.set_decoder(ActiveDecoder::new_flac()),
                }
                true
            }
            AudioFormat::Aac => ActiveDecoder::new_aac().map(|d| pipeline.set_decoder(d)).is_some(),
            AudioFormat::Ogg => {
                ActiveDecoder::new_vorbis().map(|d| pipeline.set_decoder(d)).is_some()
            }
            AudioFormat::Pcm => {
                pipeline.set_decoder(ActiveDecoder::Pcm(PcmDecoder::with_config(
                    bits, big_endian, channels,
                )));
                true
            }
            AudioFormat::Opus | AudioFormat::Alac | AudioFormat::Unknown(_) => false,
        };
        if !supported {
            pipeline.set_decoder(ActiveDecoder::Pcm(PcmDecoder::with_config(bits, big_endian, channels)));
            pipeline.set_mute(true);
        }
        supported
    }

    /// Drops the stream-ring tail when the stream has ended and the decoder cannot make
    /// anything of what is left (a truncated last frame), so the track can finish
    /// (`STMd`/`STMu`) instead of waiting forever for bytes that will never come.
    fn drop_undecodable_tail<S: AudioOutputSink>(
        &self,
        pipeline: &mut StreamPipeline<'_, S>,
        decoded: &Result<usize, DecoderError>,
    ) {
        if self.stream_eof
            && matches!(decoded, Err(DecoderError::NeedMoreData))
            && !pipeline.has_pending_pcm()
        {
            pipeline.stream_buffer.flush();
        }
    }

    /// Records an output step. Playback crossing a track start makes `STMs` pending (C
    /// parity: output.c:152 `track_started`, taken by slimproto.c:858-860 and sent at 911)
    /// and the new track's rate current (C `status.current_sample_rate =
    /// output.current_sample_rate`, slimproto.c:854). Frames reaching the sink mark the
    /// output as running.
    fn note_output<S: AudioOutputSink>(&mut self, pipeline: &mut StreamPipeline<'_, S>, written: usize) {
        if pipeline.active_marker_crossed().is_some() {
            self.current_sample_rate = pipeline.sample_rate;
            if !self.external_playback {
                self.pending_track_started = true;
            }
        }
        if written > 0 && !self.external_playback {
            self.output_started = true;
        }
    }

    /// C decodes only while `DECODE_RUNNING` (decode.c:73): an LMS track whose
    /// decoder is still READY (before its threshold, or waiting for `strm u` with autostart
    /// off) is not decoded, even while the previous track plays out of the output buffer.
    fn decoder_may_run(&self) -> bool {
        !(self.lms_track && self.decode_ready)
    }

    /// Checks a decode step for a decoder error. C parity: a decoder returning `DECODE_ERROR`
    /// makes the status loop send `STMn`, stop decoding and disconnect the stream
    /// (`slimproto.c:838-845`). The Rust decoders report a stream they cannot decode at all as
    /// [`DecoderError::UnsupportedSampleFormat`]; corrupt frames are resynced past instead.
    /// Output already running drains (then `STMu`); a track that never reached the output
    /// stops at once.
    fn note_decode_result<S: AudioOutputSink>(
        &mut self,
        pipeline: &mut StreamPipeline<'_, S>,
        decoded: &Result<usize, DecoderError>,
    ) {
        if !matches!(decoded, Err(DecoderError::UnsupportedSampleFormat)) || self.decode_failed {
            return;
        }
        self.decode_failed = true;
        self.decode_running = false;
        self.decode_ready = false;
        self.pending_not_supported = true;
        // C decode.c:103 runs `_checkfade(false)` for DECODE_ERROR as for DECODE_COMPLETE.
        pipeline.fade_track_end();
        pipeline.stream_buffer.flush();
        if !self.output_started {
            self.state = PlayerState::Stopped;
            self.push_cyd(CydNote::State(PlayState::Stopped));
        }
    }

    /// Output half of the audio step, for platforms that schedule decoding and output
    /// separately (device: top up the I2S DMA before and after each decode burst). Moves
    /// decoded PCM still pending into the output ring and writes the ring to the sink, up to
    /// `max_rounds` sink writes, while [`PlayerState::Playing`] with the DAC enabled. Returns
    /// the frames written.
    pub fn pump_output<S: AudioOutputSink>(
        &mut self,
        pipeline: &mut StreamPipeline<'_, S>,
        max_rounds: usize,
    ) -> usize {
        if self.state != PlayerState::Playing {
            return 0;
        }
        let mut total = 0;
        for _ in 0..max_rounds {
            if pipeline.has_pending_pcm() && pipeline.output_buffer.available_write() >= 8 {
                let _ = pipeline.process_decode_step();
            }
            if !self.dac_enabled || pipeline.output_buffer.available_read() < 8 {
                break;
            }
            let written = pipeline.process_output_step().unwrap_or(0);
            self.note_output(pipeline, written);
            total += written;
            if written == 0 {
                break;
            }
        }
        if pipeline.has_pending_pcm() && pipeline.output_buffer.available_write() >= 8 {
            let _ = pipeline.process_decode_step();
        }
        self.frames_played = pipeline.frames_played;
        total
    }

    /// Decode half of the audio step (see [`Self::pump_output`]): while
    /// [`PlayerState::Playing`], with no decoded PCM pending and at least
    /// [`PUMP_DECODE_MIN_OUTPUT_SPACE`] bytes free in the output ring, decodes until one step
    /// produced audio, or for at most `max_steps` steps that only consumed input (resync,
    /// tag skip). After the stream ended an undecodable tail is dropped.
    pub fn pump_decode<S: AudioOutputSink>(
        &mut self,
        pipeline: &mut StreamPipeline<'_, S>,
        max_steps: usize,
    ) {
        if self.state != PlayerState::Playing || !self.decoder_may_run() {
            return;
        }
        for _ in 0..max_steps {
            if pipeline.has_pending_pcm()
                || pipeline.output_buffer.available_write() < PUMP_DECODE_MIN_OUTPUT_SPACE
                || pipeline.stream_buffer.available_read() == 0
            {
                break;
            }
            let before = pipeline.output_buffer.available_read();
            let decoded = pipeline.process_decode_step();
            self.note_decode_result(pipeline, &decoded);
            if self.decode_failed {
                break;
            }
            self.drop_undecodable_tail(pipeline, &decoded);
            if pipeline.has_pending_pcm() || pipeline.output_buffer.available_read() > before {
                break;
            }
            if !matches!(decoded, Ok(consumed) if consumed > 0) {
                break;
            }
        }
    }

    /// Drops a stream that failed after connecting and marks it ended (the decoder may still
    /// finish what arrived). C parity: a read error is `DISCONNECT`/`REMOTE_DISCONNECT`
    /// (`stream.c:364`, `stream.c:512`), reported as `DSCO(2)` by the status loop.
    fn fail_stream<R: StreamSource>(&mut self, source: &mut R) {
        self.stream_eof = true;
        source.abort();
        self.stream = StreamLink::Idle;
        self.stream_end = Some(DisconnectReason::ResetByRemote);
        self.pending_dsco = Some(DisconnectReason::ResetByRemote);
    }

    /// The stream ended cleanly. C parity: EOF on the socket is `DISCONNECT`/`DISCONNECT_OK`
    /// (`stream.c:355`, `stream.c:508`): the socket is closed and the status loop sends
    /// `DSCO(0)` right away (`slimproto.c:804-808`, `910`), not after the output drained.
    fn end_stream<R: StreamSource>(&mut self, source: &mut R) {
        self.stream_eof = true;
        source.abort();
        self.stream = StreamLink::Idle;
        self.stream_end = Some(DisconnectReason::ConnectionClosed);
        self.disconnect_ok_unseen = true;
        self.pending_dsco = Some(DisconnectReason::ConnectionClosed);
    }

    /// Drops the current stream, if any (`strm s`/`q`/`f`, `flsh`).
    fn close_stream<R: StreamSource>(&mut self, source: &mut R) {
        if self.stream_active() {
            source.abort();
        }
        self.stream = StreamLink::Idle;
    }

    /// Builds the HTTP request for a `strm s` from its header bytes (`COORD-503`): an empty
    /// header block requests `GET /stream.mp3 HTTP/1.0`; non-UTF-8 (e.g. Latin-1) headers are
    /// truncated to 1024 bytes with every non-ASCII byte replaced by `?`; the request is
    /// terminated with a blank line if the server did not include one.
    fn build_http_request(http_headers: &[u8]) -> Vec<u8> {
        let mut request: Vec<u8> = if http_headers.is_empty() {
            return DEFAULT_HTTP_REQUEST.to_vec();
        } else if core::str::from_utf8(http_headers).is_ok() {
            http_headers.to_vec()
        } else {
            http_headers
                .iter()
                .take(1024)
                .map(|&b| if b.is_ascii() { b } else { b'?' })
                .collect()
        };
        if !request.ends_with(b"\r\n\r\n") {
            if request.ends_with(b"\r\n") {
                request.extend_from_slice(b"\r\n");
            } else {
                request.extend_from_slice(b"\r\n\r\n");
            }
        }
        request
    }

    /// Handles one command from LMS, mutating the pipeline, sending the immediate replies and
    /// queueing CYD updates.
    fn handle_command<S: AudioOutputSink, T: SlimTransport, R: StreamSource, B: AudioBackend>(
        &mut self,
        cmd: ServerCommand<'_>,
        transport: &mut T,
        source: &mut R,
        backend: &mut B,
        pipeline: &mut StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<(), TransportError> {
        match cmd {
            ServerCommand::Stream {
                command,
                autostart,
                format,
                pcm_sample_size,
                pcm_sample_rate,
                pcm_channels,
                pcm_endianness,
                threshold_kb,
                spdif_enable: _,
                trans_duration,
                trans_type,
                flags,
                output_threshold,
                replay_gain,
                server_port,
                server_ip,
                http_headers,
            } => match command {
                StreamCommandType::Start => {
                    self.start_stream(
                        StartParams {
                            autostart: AutostartMode::from_strm(autostart, flags),
                            flags,
                            format,
                            pcm_sample_size,
                            pcm_sample_rate,
                            pcm_channels,
                            big_endian: matches!(pcm_endianness, Endianness::BigEndian),
                            threshold_kb,
                            output_threshold,
                            replay_gain,
                            fade_mode: fade_mode_from_strm(trans_type),
                            fade_secs: trans_duration,
                            server_port,
                            server_ip,
                            http_headers,
                        },
                        transport,
                        source,
                        backend,
                        pipeline,
                        now_ms,
                    )
                }
                StreamCommandType::Pause => {
                    // C parity (slimproto.c:395-410): the strm p "replay_gain" field is an
                    // interval in ms.
                    let interval = replay_gain;
                    if interval > 0 {
                        // OUTPUT_PAUSE_FRAMES in any output state (slimproto.c:399-401,
                        // output.c:86-96): silence for `interval`, then RUNNING by itself; no
                        // STMp. A track still buffering keeps buffering (silence already) and
                        // plays once started. Rust has no frame-exact silence insertion yet, so
                        // the deadline is checked every tick.
                        self.unpause_at_jiffies = None;
                        if self.state != PlayerState::Buffering {
                            self.state = PlayerState::Paused;
                        }
                        self.pause_until_ms = Some(now_ms.wrapping_add(interval));
                        return Ok(());
                    }
                    // Interval 0: output STOPPED and an STMp (slimproto.c:402-408).
                    self.pause_until_ms = None;
                    self.unpause_at_jiffies = None;
                    self.state = PlayerState::Paused;
                    self.buffer_threshold_bytes = 0;
                    pipeline.sink.set_mute(true);
                    backend.set_mute(true);
                    self.send_metrics_stat(transport, StatEvent::TrackPaused, pipeline, now_ms)?;
                    self.push_cyd(CydNote::State(PlayState::Paused));
                    Ok(())
                }
                StreamCommandType::Resume => {
                    // C parity (slimproto.c:423-434): STMr always; with jiffies the output
                    // waits (OUTPUT_START_AT) until that time, unless it is more than 10 s
                    // ahead (output.c:99-109).
                    let jiffies = replay_gain;
                    self.pause_until_ms = None;
                    let ahead = jiffies.wrapping_sub(now_ms) as i32;
                    if jiffies != 0 && ahead > 0 && ahead as u32 <= START_AT_MAX_AHEAD_MS {
                        self.unpause_at_jiffies = Some(jiffies);
                        // A track still buffering keeps buffering: threshold and STMd stay
                        // with the decoder (slimproto.c:825-845).
                        if self.state != PlayerState::Buffering {
                            self.state = PlayerState::Paused;
                        }
                    } else {
                        self.unpause_at_jiffies = None;
                        self.start_output(pipeline, backend);
                    }
                    self.send_metrics_stat(transport, StatEvent::TrackResumed, pipeline, now_ms)?;
                    self.push_cyd(CydNote::State(PlayState::Playing));
                    Ok(())
                }
                StreamCommandType::Stop => {
                    // C parity (slimproto.c:386-394): decode_flush, output_flush, frames_played
                    // = 0, stream_disconnect, streambuf flush, and no reply (the STMf there is
                    // only for 'f').
                    self.state = PlayerState::Stopped;
                    self.buffer_threshold_bytes = 0;
                    self.close_stream(source);
                    self.reset();
                    pipeline.flush();
                    pipeline.frames_played = 0;
                    self.frames_played = 0;
                    pipeline.sink.set_mute(true);
                    backend.set_mute(true);
                    self.push_cyd(CydNote::State(PlayState::Stopped));
                    Ok(())
                }
                StreamCommandType::Flush => self.flush_command(transport, source, pipeline, now_ms),
                StreamCommandType::Unknown(b't') => {
                    // RUST-06 / C parity (slimproto.c:371-373): strm t always answers STMt,
                    // echoing replay_gain as the server timestamp.
                    send_stat_with_metrics(
                        transport,
                        StatEvent::Heartbeat,
                        pipeline.stream_buffer_available() as u32,
                        pipeline.output_buffer_available() as u32,
                        pipeline.frames_played,
                        self.current_sample_rate,
                        self.stream_bytes_received,
                        now_ms,
                        replay_gain,
                    )
                }
                StreamCommandType::Unknown(b'a') => {
                    // C parity (slimproto.c:412-421, output.c:70-82): skip `interval` ms of
                    // output: consume up to interval * rate / 1000 frames of the output buffer
                    // (8-byte stereo i32 frames) and count them as played. No reply.
                    // Task 12: the skip stops at a pending track start (see
                    // `StreamPipeline::skip_output_frames`).
                    let frames = u64::from(replay_gain) * u64::from(pipeline.sample_rate) / 1000;
                    pipeline.skip_output_frames(frames);
                    self.frames_played = pipeline.frames_played;
                    // OUTPUT_SKIP_FRAMES replaced any START_AT / PAUSE_FRAMES wait and ends in
                    // RUNNING (slimproto.c:417, output.c:82).
                    let pausing = self.pause_until_ms.take().is_some();
                    let waiting = self.unpause_at_jiffies.take().is_some();
                    if pausing || waiting {
                        self.start_output(pipeline, backend);
                    }
                    Ok(())
                }
                StreamCommandType::Unknown(_) => Ok(()),
            },
            ServerCommand::AudioGain {
                gain_left,
                gain_right,
                digital_atten,
                ..
            } => {
                // C parity: slimproto.c:569 `set_volume(adjust ? gainL : FIXED_ONE, adjust ?
                // gainR : FIXED_ONE)`. `adjust` (digital_atten) 0 is LMS's fixed-volume mode
                // (an external amplifier sets the level): unity, the gains are ignored.
                let (left, right) = if digital_atten != 0 { (gain_left, gain_right) } else { (UNITY_GAIN, UNITY_GAIN) };
                self.set_volume(left, right, backend, pipeline);
                self.push_cyd(CydNote::Volume(lms_gain_to_hardware_volume_percent(left.max(right))));
                Ok(())
            }
            ServerCommand::SetDevice { sample_rate } => {
                if sample_rate <= u8::MAX as u32 {
                    // C parity (slimproto.c:589-593): the id-0 query is answered only when
                    // the player has a name.
                    if sample_rate == 0 && !self.player_name.is_empty() {
                        send_setd_name(transport, &self.player_name)?;
                    }
                    // 1-byte id != 0 query is NOT a sample rate; do not update current_sample_rate
                    return Ok(());
                }
                let bytes = sample_rate.to_be_bytes();
                if bytes[0] == 0 && bytes[1..].iter().all(|&b| (0x20..=0x7E).contains(&b)) {
                    if let Ok(name) = core::str::from_utf8(&bytes[1..]) {
                        self.set_player_name(name);
                    }
                    // C parity (slimproto.c:600-601): a name change is confirmed with SETD.
                    send_setd_name(transport, &self.player_name)?;
                    return Ok(());
                }
                // Not a C path (C's process_setd only handles id 0): a legacy Rust sample-rate
                // form. The clock refusing it keeps the previous clock; no STMn.
                self.current_sample_rate = sample_rate;
                let _ = pipeline.set_sample_rate(sample_rate);
                let _ = backend.set_sample_rate(sample_rate);
                Ok(())
            }
            ServerCommand::Flush => self.flush_command(transport, source, pipeline, now_ms),
            ServerCommand::Body { data } => {
                if self.state == PlayerState::Playing || self.state == PlayerState::Buffering {
                    let _ = pipeline.ingest_stream_data(data);
                }
                Ok(())
            }
            ServerCommand::Unknown { opcode, payload } => {
                if let Some((_spdif, dac_enable)) = cmd.audio_enable() {
                    self.dac_enabled = dac_enable;
                    pipeline.sink.set_mute(!dac_enable);
                    backend.set_power(dac_enable);
                } else if let Some(metaint) = cmd.stream_continue() {
                    // C parity (slimproto.c:501-521): autostart -= 2, STREAMING_WAIT ->
                    // STREAMING_BUFFERING with the ICY interval. STMl/start follow from the
                    // status logic.
                    self.awaiting_cont = false;
                    if metaint > 0 && self.stream_active() {
                        source.set_icy_metaint(metaint as usize);
                    }
                } else if opcode == *b"codc" && !payload.is_empty() {
                    // Codec change: format, sample size, rate, channels, endianness. C parity
                    // (slimproto.c:523-532): codec_open, so decode READY if the codec exists.
                    let format = AudioFormat::from_byte(payload[0]);
                    let bits = payload.get(1).map_or(16, |&b| Self::normalize_pcm_bits(b));
                    let channels = payload.get(3).map_or(2, |&b| Self::normalize_pcm_channels(b));
                    let is_be = payload
                        .get(4)
                        .is_none_or(|&b| matches!(Endianness::from_byte(b), Endianness::BigEndian));
                    self.current_format = Some(format);
                    self.decode_ready = Self::select_decoder(pipeline, format, bits, channels, is_be);
                    // C decode.c:287: codec_open starts a new stream (next track start).
                    pipeline.open_stream();
                } else if let Some((ip, syncgroup)) = cmd.switch_server() {
                    self.switch_server(ip, syncgroup);
                } else if let Some(setting) = cmd.device_setting() {
                    if setting.id == 0 {
                        // LMS NUL-terminates the name; never keep (or echo) the terminator.
                        let data = setting.data.split(|&b| b == 0).next().unwrap_or(&[]);
                        if !data.is_empty() {
                            if let Ok(name) = core::str::from_utf8(data) {
                                self.set_player_name(name);
                            }
                        }
                        // C parity (slimproto.c:600-601): a name change is confirmed with SETD.
                        send_setd_name(transport, &self.player_name)?;
                    }
                } else if opcode == *b"eqlz" || opcode == *b"loud" {
                    pipeline.equalizer.enabled = true;
                }
                Ok(())
            }
        }
    }

    /// `strm s` (C `process_strm`, `slimproto.c:436-499`).
    #[allow(clippy::too_many_arguments)]
    fn start_stream<S: AudioOutputSink, T: SlimTransport, R: StreamSource, B: AudioBackend>(
        &mut self,
        p: StartParams<'_>,
        transport: &mut T,
        source: &mut R,
        backend: &mut B,
        pipeline: &mut StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<(), TransportError> {
        // C parity: slimproto.c:449, STMf is the first reply, before the codec is opened and
        // the stream connected.
        self.send_metrics_stat(transport, StatEvent::StreamFlushed, pipeline, now_ms)?;

        // Task 12, C parity: `strm s` flushes only the stream buffer (stream_sock, stream.c:624,
        // 738) and opens the codec; the output buffer and output state are untouched, so the
        // previous track's tail keeps playing and the new track follows it gaplessly. The
        // new decoder's first frames set the track boundary (pipeline `TrackMarker`).
        // An output still RUNNING (Playing with audio out) stays Playing; otherwise the track
        // starts from Buffering as before.
        let output_continues = self.output_running();
        let pending_stms = self.pending_track_started;
        let was_external = self.external_playback;
        self.close_stream(source);
        self.reset();
        if output_continues {
            self.output_started = true;
            self.pending_track_started = pending_stms;
        }
        let rate_hint = Self::decode_pcm_sample_rate_hz(p.pcm_sample_rate)
            .unwrap_or_else(|| sample_rate_from_index(p.pcm_sample_rate));
        // C slimproto.c:484-485: output.threshold and next_replay_gain for the new track.
        pipeline.begin_stream(rate_hint, p.replay_gain, p.output_threshold);
        // C parity: slimproto.c:486-489, fade_mode, fade_secs, invert ((flags & 0x03) ==
        // 0x03) and channels ((flags & 0x0c) >> 2). The fade settings arm fades at the new
        // track's first decoded frames and at the end of its decoding.
        pipeline.set_transition(
            p.fade_mode,
            u32::from(p.fade_secs),
            p.flags & 0x03 == 0x03,
            ChannelMode::from_strm_flags(p.flags),
        );
        // Nothing of an earlier track left to play (or another source's audio, which LMS
        // replaces): the clock can switch now, and the frame counters start at 0
        // (COORD-506/509; C resets them at the track start, which then comes first).
        let output_idle = was_external || pipeline.output_buffer_available() == 0;
        if output_idle {
            pipeline.flush();
            pipeline.frames_played = 0;
            self.frames_played = 0;
        }
        self.pending_resp = None;
        self.pending_meta = None;
        self.buffer_threshold_reported = false;
        self.stream_bytes_received = 0;
        self.lms_track = true;
        // A mute for an undecodable previous track ends with it.
        pipeline.user_muted = false;

        self.autostart_mode = p.autostart;
        self.autostart = self.autostart_mode.is_auto();
        self.awaiting_cont = self.autostart_mode.requires_cont();
        self.server_rebooting = (p.flags & STRM_FLAG_SERVER_REBOOT) != 0;

        let sample_rate = rate_hint;
        self.current_format = Some(p.format);
        let bits = Self::normalize_pcm_bits(p.pcm_sample_size);
        let channels = Self::normalize_pcm_channels(p.pcm_channels);

        // codec_open (slimproto.c:455-463): '?' waits for `codc` and needs autostart >= 2.
        if p.format == AudioFormat::Unknown(b'?') {
            if !self.awaiting_cont {
                // "unknown codec requires autostart >= 2": nothing after STMf.
                self.state = PlayerState::Stopped;
                self.push_cyd(CydNote::State(PlayState::Stopped));
                return Ok(());
            }
        } else {
            // C decode.c:286-317: READY only if a codec for the format exists. A missing one
            // is no error (no STMn); the track simply never starts. (A format this build
            // cannot decode also gets a muted PCM decoder, see `select_decoder`.)
            self.decode_ready = Self::select_decoder(pipeline, p.format, bits, channels, p.big_endian);
            // codec_open: its first frames start the next track (C decode.c:287).
            pipeline.open_stream();
        }

        // The clock must support a PCM track's rate. C has no clean equivalent (RESAMPLE16
        // resamples; output_i2s.c:638-641 only logs a failed i2s_set_sample_rates): mapped to
        // DECODE_ERROR, i.e. STMn from the status pass (slimproto.c:840), with no stream.
        // Compressed formats learn their rate from the decoder, so the strm field is a hint.
        // With the previous track still to play the clock is only checked here: it switches
        // when playback reaches the new track (C output.c:154, pipeline track boundary).
        let clock_ok = if output_idle {
            self.current_sample_rate = sample_rate;
            backend.set_sample_rate(sample_rate).is_ok()
        } else {
            crate::clock_table::i2s_clock_config(sample_rate).is_some()
        };
        if !clock_ok && p.format == AudioFormat::Pcm {
            self.decode_ready = false;
            self.decode_failed = true;
            self.pending_not_supported = true;
            if !output_continues {
                self.state = PlayerState::Stopped;
                pipeline.sink.set_mute(true);
                backend.set_mute(true);
                self.push_cyd(CydNote::State(PlayState::Stopped));
            }
            return Ok(());
        }
        if output_idle {
            let _ = pipeline.set_sample_rate(sample_rate);
        }
        // Track start powers the amplifier (device: Hi-Z -> Play when idle), unless LMS
        // disabled the DAC (`aude`).
        if self.dac_enabled {
            backend.set_power(true);
        }

        // C stream.threshold = threshold * 1024 (slimproto.c:471). Clamped below the stream
        // ring (much smaller than C's), so a large threshold cannot deadlock.
        let requested_threshold = (p.threshold_kb as usize).saturating_mul(1024);
        let max_safe_threshold = pipeline.stream_buffer.capacity().saturating_sub(1024);
        self.buffer_threshold_bytes = if max_safe_threshold > 0 {
            requested_threshold.min(max_safe_threshold)
        } else {
            requested_threshold.min(pipeline.stream_buffer.capacity())
        };

        // stream_sock (slimproto.c:440-442, 468-471): ip 0 is the slimproto server; the port
        // is used as is. Port 0 cannot connect, so it is the connect failure C would see
        // (stream.c:684-688), without trying.
        let ip = if p.server_ip == [0, 0, 0, 0] { self.lms_ip } else { p.server_ip };
        let addr = NetAddr { ip, port: p.server_port };
        let request = Self::build_http_request(p.http_headers);
        let connected = if p.server_port == 0 {
            Err(StreamError::ConnectFailed)
        } else {
            source.connect(addr, &request)
        };
        match connected {
            Ok(()) => {
                self.stream = StreamLink::Open;
                self.report_stream_connected(transport, pipeline, now_ms)?;
            }
            // Response still pending: `STMc` once it resolves.
            Err(StreamError::WouldBlock) => {
                self.stream = StreamLink::Connecting { addr, request };
            }
            Err(_) => return self.report_stream_unreachable(transport, source, pipeline, now_ms),
        }

        // STREAMING_BUFFERING until more than the threshold arrived (stream.c:527). A RUNNING
        // output keeps playing meanwhile (gapless).
        if !output_continues {
            self.state = PlayerState::Buffering;
        }
        self.push_cyd(CydNote::State(PlayState::Playing));
        Ok(())
    }

    /// Output runs (again, C `OUTPUT_RUNNING`): unmuted unless LMS disabled the DAC, and
    /// [`PlayerState::Playing`]. A track still buffering stays Buffering with the start
    /// requested: it plays once its decoder starts at the threshold (`slimproto.c:825-834`),
    /// so the threshold and `STMd` keep their C semantics.
    fn start_output<S: AudioOutputSink, B: AudioBackend>(
        &mut self,
        pipeline: &mut StreamPipeline<'_, S>,
        backend: &mut B,
    ) {
        pipeline.sink.set_mute(!self.dac_enabled);
        backend.set_mute(!self.dac_enabled);
        if self.state == PlayerState::Buffering {
            self.output_start_requested = true;
            return;
        }
        self.state = PlayerState::Playing;
    }

    /// `strm f` and `flsh`. C parity (slimproto.c:374-385): `decode_flush`, the streaming
    /// part of the output is flushed, the stream disconnected, and `STMf` sent only if a
    /// stream was disconnected or output flushed. The "streaming part" is everything after
    /// the pending track start (C `output_flush_streaming`, output.c:461-470: `writep =
    /// track_start`): the next track queued behind a playing one, or a track whose output
    /// has not started. A track already playing drains (then `STMu`, as in C).
    fn flush_command<S: AudioOutputSink, T: SlimTransport, R: StreamSource>(
        &mut self,
        transport: &mut T,
        source: &mut R,
        pipeline: &mut StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<(), TransportError> {
        let had_stream = self.stream_active();
        // C decode_flush(false): the decoder stops; PCM it decoded but did not deliver yet is
        // audio C would not have decoded (its decoders write straight to outputbuf).
        pipeline.discard_decoder_backlog();
        let flushed_output = pipeline.flush_streaming();
        self.buffer_threshold_bytes = 0;
        self.close_stream(source);
        self.pending_track_started = false;
        self.pending_underrun = false;
        self.pending_decode_ready = false;
        self.decode_ready = false;
        self.decode_running = false;
        self.pause_until_ms = None;
        self.unpause_at_jiffies = None;
        if self.state == PlayerState::Buffering {
            self.state = PlayerState::Stopped;
        }
        pipeline.stream_buffer.flush();
        if had_stream || flushed_output {
            send_stat(transport, StatEvent::StreamFlushed, 0, 0, pipeline.frames_played as u32, now_ms)?;
        }
        Ok(())
    }

    /// `serv` (C `process_serv`, `slimproto.c:623-657`): remember the server to switch to and
    /// the SyncgroupID for the next `HELO` (a `serv` without one clears it).
    fn switch_server(&mut self, ip: [u8; 4], syncgroup: Option<&[u8]>) {
        if ip == SQUEEZENETWORK_MARKER_IP {
            // C resolves SQUEEZENETWORK by DNS (slimproto.c:633-634). This firmware has no
            // resolver yet (Phase 3 discovery), so the switch is ignored.
            return;
        }
        self.server_switch = Some(ip);
        self.syncgroup = syncgroup.and_then(|id| id.try_into().ok());
    }

    /// A `setd` id-0 name change (already cut at its first NUL): announced name, and persisted
    /// through [`Self::config`] (the journal keeps at most 32 bytes, cut on a `char` boundary).
    fn set_player_name(&mut self, name: &str) {
        self.player_name.clear();
        let _ = self.player_name.push_str(name);
        if let Some(config) = self.config {
            config.update(|c| {
                c.set_player_name(name);
            });
        }
    }

    /// Collects the response head and ICY metadata the source has captured (through the
    /// callback takes, so no 4 KB `HeaderBuf`/`MetaBuf` value lands on this stack).
    ///
    /// A new ICY `StreamTitle` also goes to the CYD (`META=`; spec §4.4), once per change.
    fn take_stream_side_data<R: StreamSource>(&mut self, source: &mut R) {
        let resp = &mut self.pending_resp;
        source.take_response_headers_with(&mut |head| *resp = Some(head.to_vec()));
        let meta = &mut self.pending_meta;
        let mut title = None;
        source.take_icy_metadata_with(&mut |block| {
            *meta = Some(block.to_vec());
            title = icy_stream_title(block).map(<[u8]>::to_vec);
        });
        if title.is_some() && title != self.track_metadata {
            self.track_metadata = title;
            self.push_cyd(CydNote::Metadata);
        }
    }

    /// Moves HTTP audio bytes from `source` into the pipeline stream buffer.
    ///
    /// Stops when no bytes are available right now (`HALNET-405`) or the stream buffer is full,
    /// records a clean end of stream in `stream_eof`, and drops the stream (also setting
    /// `stream_eof`) when `poll_bytes` fails (`COORD-508`). While a `cont` is awaited only the
    /// response head is collected: the body (and its ICY framing) starts after `cont`. Kept
    /// out of line so its read chunk is off the stack again before decoding starts.
    #[inline(never)]
    fn poll_stream<S: AudioOutputSink, R: StreamSource>(
        &mut self,
        source: &mut R,
        pipeline: &mut StreamPipeline<'_, S>,
    ) -> Result<usize, StreamError> {
        if self.awaiting_cont {
            if self.stream == StreamLink::Open {
                self.take_stream_side_data(source);
            }
            return Ok(0);
        }
        let mut total_written = 0;
        let mut chunk = [0u8; 1024];
        while self.stream == StreamLink::Open {
            self.take_stream_side_data(source);
            let avail_write = pipeline.stream_buffer.available_write();
            if avail_write == 0 {
                break;
            }
            let max_read = avail_write.min(chunk.len());
            match source.poll_bytes(&mut chunk[..max_read]) {
                Ok(0) => {
                    if source.is_eof() {
                        self.take_stream_side_data(source);
                        self.end_stream(source);
                    }
                    break;
                }
                Ok(n) => {
                    self.stream_bytes_received += n as u64;
                    let written = pipeline.ingest_stream_data(&chunk[..n]).unwrap_or(0);
                    total_written += written;
                    if written < n {
                        break;
                    }
                }
                Err(e) => {
                    self.fail_stream(source);
                    return Err(e);
                }
            }
        }
        if self.stream == StreamLink::Open {
            self.take_stream_side_data(source);
            if source.is_eof() {
                self.end_stream(source);
            }
        }
        Ok(total_written)
    }

    /// Output deadlines from `strm p` (interval) and `strm u` (jiffies), on the
    /// `now_ms` clock that `STAT` jiffies report (C `gettime_ms`).
    fn apply_output_deadlines<S: AudioOutputSink, B: AudioBackend>(
        &mut self,
        pipeline: &mut StreamPipeline<'_, S>,
        backend: &mut B,
        now_ms: u32,
    ) {
        // C OUTPUT_PAUSE_FRAMES ends: RUNNING again (output.c:88-90).
        if let Some(until) = self.pause_until_ms {
            if deadline_reached(now_ms, until) {
                self.pause_until_ms = None;
                self.start_output(pipeline, backend);
            }
        }
        // C OUTPUT_START_AT reached: RUNNING (output.c:101-102).
        if let Some(at) = self.unpause_at_jiffies {
            if deadline_reached(now_ms, at) {
                self.unpause_at_jiffies = None;
                self.start_output(pipeline, backend);
            }
        }
    }

    /// More than the threshold arrived (C `stream.bytes > stream.threshold`, `stream.c:527`).
    /// A full stream ring also counts: the Rust ring is smaller than C's, and the clamp in
    /// `strm s` keeps the threshold below it, so this only covers data fed outside `tick`.
    fn stream_reached_threshold<S: AudioOutputSink>(&self, pipeline: &StreamPipeline<'_, S>) -> bool {
        let threshold = self.buffer_threshold_bytes as u64;
        self.stream_bytes_received > threshold
            || pipeline.stream_buffer.available_read() as u64 > threshold
            || pipeline.stream_buffer.available_write() == 0
    }

    /// The play-state half of C's status loop (`slimproto.c:824-884`): start or `STMl` once the
    /// stream is past the threshold, `STMd` when the decoder used up an ended stream, `STMu`
    /// when output ran dry after that, `STMo` when it ran dry mid-stream.
    fn evaluate_state<S: AudioOutputSink>(&mut self, pipeline: &mut StreamPipeline<'_, S>) {
        self.frames_played = pipeline.frames_played;

        if !self.awaiting_cont
            && !self.stream_past_threshold
            && self.stream_active()
            && self.stream_reached_threshold(pipeline)
        {
            self.stream_past_threshold = true;
        }

        // C slimproto.c:825-834: stream STREAMING_HTTP (or ended with DISCONNECT_OK) and
        // decode READY -> decode RUNNING; autostart 0 sends STMl once (sentSTMl), autostart 1
        // starts the output. Autostart 2/3 wait for `cont` (awaiting_cont).
        // STREAMING_HTTP is "past the threshold with the stream still up"; DISCONNECT_OK
        // counts on this one pass only.
        let clean_end_this_pass = core::mem::take(&mut self.disconnect_ok_unseen);
        let streaming = self.stream_past_threshold && self.stream_active();
        // Task 12: the decoder state is independent of the output (C decode.state vs
        // output.state). A `strm s` that arrives while the previous track still plays leaves
        // the player Playing (output RUNNING); its decoder starts here too.
        if matches!(self.state, PlayerState::Buffering | PlayerState::Playing)
            && self.decode_ready
            && !self.awaiting_cont
            && (streaming || clean_end_this_pass)
        {
            self.decode_ready = false;
            self.decode_running = true;
            if !self.autostart_mode.is_auto() && !self.buffer_threshold_reported {
                self.buffer_threshold_reported = true;
                self.pending_buffer_threshold = true;
            }
            if self.state == PlayerState::Playing {
                // Output already RUNNING: `_start_output` changes nothing (slimproto.c:869).
            } else if self.autostart_mode.is_auto() || self.output_start_requested {
                self.state = PlayerState::Playing;
            } else {
                // Rust decodes only while Playing, so the decoder starts with `strm u` (C
                // already fills the output buffer here).
                self.state = PlayerState::Paused;
            }
            self.output_start_requested = false;
        }

        let output_empty =
            pipeline.output_buffer.available_read() == 0 && !pipeline.has_pending_pcm();

        // C slimproto.c:838-845 (DECODE_COMPLETE): the decoder used up a stream that ended.
        if self.state == PlayerState::Playing
            && self.decode_running
            && self.stream_eof
            && pipeline.stream_buffer.available_read() == 0
            && !pipeline.has_pending_pcm()
        {
            self.decode_running = false;
            self.decode_complete_sent = true;
            self.pending_decode_ready = true;
            // C decode.c:103: `if (output.fade_mode) _checkfade(false)` (fade-out, in-out).
            pipeline.fade_track_end();
        }

        // C slimproto.c:872-879: output RUNNING and empty, stream <= DISCONNECT, decode
        // STOPPED -> STMu once per track (sentSTMu), output STOPPED.
        if self.output_running()
            && !self.track_underrun_reported
            && output_empty
            && !self.stream_active()
            && !self.decode_running
        {
            self.track_underrun_reported = true;
            self.state = PlayerState::Stopped;
            self.pending_underrun = true;
        }

        // C slimproto.c:881-884: output RUNNING and empty while the stream is still
        // STREAMING_HTTP -> STMo, once per track (sentSTMo). Rust subset: "empty" also needs
        // the stream ring empty (nothing left to decode), because the Rust output ring is
        // routinely drained into the DMA between decode bursts; the DMA backlog itself is
        // counted once output-device frame counters exist (Task 15).
        if self.output_running()
            && !self.output_underrun_reported
            && output_empty
            && pipeline.stream_buffer.available_read() == 0
            && self.stream == StreamLink::Open
            && self.stream_past_threshold
            && !self.stream_eof
        {
            self.output_underrun_reported = true;
            self.pending_output_underrun = true;
        }
    }

    /// C `output.state == OUTPUT_RUNNING`: this track's output reached the sink and is not
    /// paused or stopped.
    fn output_running(&self) -> bool {
        self.lms_track && self.state == PlayerState::Playing && self.output_started
    }

    /// Executes one cycle of state evaluation, decoding and audio output on data already in the
    /// stream buffer (`tick` fills it from the stream source first). Returns the frames written.
    pub fn process_audio_step<S: AudioOutputSink>(
        &mut self,
        pipeline: &mut StreamPipeline<'_, S>,
    ) -> usize {
        self.evaluate_state(pipeline);

        if self.state != PlayerState::Playing {
            return 0;
        }

        // Try decoding a frame chunk
        if self.decoder_may_run() {
            let decoded = pipeline.process_decode_step();
            self.note_decode_result(pipeline, &decoded);
            self.drop_undecodable_tail(pipeline, &decoded);
        }

        // Write frames to the output sink
        let written = pipeline.process_output_step().unwrap_or(0);
        self.note_output(pipeline, written);
        self.frames_played = pipeline.frames_played;

        // Re-evaluate EOF / underrun transition after draining output buffer
        self.evaluate_state(pipeline);

        written
    }

    /// Builds the full set of CYD telemetry messages (`PlaybackState`, `Volume`, `Progress`
    /// and, once an ICY `StreamTitle` arrived, `TrackMetadata`) for status synchronization
    /// (`COORD-505`, spec §4.4).
    pub fn sync_cyd_telemetry<S: AudioOutputSink>(
        &self,
        pipeline: &StreamPipeline<S>,
    ) -> heapless::Vec<CydMessage, 4> {
        let mut msgs = heapless::Vec::new();
        let _ = msgs.push(CydMessage::PlaybackState(self.cyd_playback_state()));

        // The louder side of the requested volume, as the audg CYD note: a balance ratio in
        // the software gain must not read as a lower volume.
        let (left, right) = pipeline.requested_volume_q16();
        let _ = msgs.push(CydMessage::Volume(lms_gain_to_hardware_volume_percent(left.max(right))));

        let elapsed_secs = if self.current_sample_rate > 0 {
            (pipeline.frames_played / (self.current_sample_rate as u64)) as u32
        } else {
            0
        };
        let _ = msgs.push(CydMessage::Progress {
            elapsed_secs,
            duration_secs: 0,
        });
        if let Some(meta) = self.track_metadata_message() {
            let _ = msgs.push(meta);
        }

        msgs
    }

    /// Handles an interactive command from the CYD front panel display.
    pub fn handle_cyd_command<B: AudioBackend, S: AudioOutputSink>(
        &mut self,
        cmd: CydCommand,
        backend: &mut B,
        pipeline: &mut StreamPipeline<S>,
    ) -> Option<CydMessage> {
        match cmd {
            CydCommand::Play => {
                self.state = PlayerState::Playing;
                pipeline.sink.set_mute(false);
                Some(CydMessage::PlaybackState(PlayState::Playing))
            }
            CydCommand::Pause => {
                if self.state == PlayerState::Playing || self.state == PlayerState::Buffering {
                    self.state = PlayerState::Paused;
                    self.buffer_threshold_bytes = 0;
                    pipeline.sink.set_mute(true);
                }
                Some(CydMessage::PlaybackState(PlayState::Paused))
            }
            CydCommand::Toggle => {
                if self.state == PlayerState::Playing || self.state == PlayerState::Buffering {
                    self.state = PlayerState::Paused;
                    self.buffer_threshold_bytes = 0;
                    pipeline.sink.set_mute(true);
                    Some(CydMessage::PlaybackState(PlayState::Paused))
                } else {
                    self.state = PlayerState::Playing;
                    pipeline.sink.set_mute(false);
                    Some(CydMessage::PlaybackState(PlayState::Playing))
                }
            }
            CydCommand::SetVolume(vol) => {
                // Through the single volume entry: the device policy applies and the balance
                // LMS set is kept.
                self.set_volume_percent(vol, backend, pipeline);
                Some(CydMessage::Volume(vol))
            }
            CydCommand::QueryStatus => {
                Some(CydMessage::PlaybackState(self.cyd_playback_state()))
            }
            CydCommand::Next | CydCommand::Previous => None,
        }
    }

    /// The sending half of C's status loop (`slimproto.c:909-919`), in C's order: `DSCO`,
    /// `STMs`, `STMd`, `STMt`, `STMl`, `STMu`, `STMo`, `STMn`, then `META`. `RESP` goes first:
    /// in C the response head wakes the status loop on its own (`stream.c:436`), so it always
    /// precedes the body events that a single Rust tick may observe together with it.
    ///
    /// The `STMt` heartbeat is sent only while output is running, every 1000 ms
    /// (`slimproto.c:890-893`); an idle player sends nothing unsolicited.
    fn check_heartbeat<S: AudioOutputSink, T: SlimTransport>(
        &mut self,
        transport: &mut T,
        pipeline: &StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<(), TransportError> {
        if let Some(head) = self.pending_resp.take() {
            let (sent, head) = send_resp(transport, head);
            if sent == Err(TransportError::WouldBlock) {
                self.pending_resp = Some(head);
            }
            sent?;
        }

        if let Some(reason) = self.pending_dsco {
            send_dsco(transport, reason)?;
            self.pending_dsco = None;
        }
        if self.pending_track_started {
            self.send_metrics_stat(transport, StatEvent::TrackStarted, pipeline, now_ms)?;
            self.pending_track_started = false;
        }
        if self.pending_decode_ready {
            self.send_metrics_stat(transport, StatEvent::DecodeReady, pipeline, now_ms)?;
            self.pending_decode_ready = false;
        }
        if self.output_running()
            && now_ms.wrapping_sub(self.last_heartbeat_ms) > self.heartbeat_interval_ms
        {
            self.send_metrics_stat(transport, StatEvent::Heartbeat, pipeline, now_ms)?;
            self.last_heartbeat_ms = now_ms;
        }
        if self.pending_buffer_threshold {
            self.send_metrics_stat(transport, StatEvent::BUFFER_THRESHOLD, pipeline, now_ms)?;
            self.pending_buffer_threshold = false;
        }
        if self.pending_underrun {
            self.send_metrics_stat(transport, StatEvent::TrackUnderrun, pipeline, now_ms)?;
            self.pending_underrun = false;
        }
        if self.pending_output_underrun {
            self.send_metrics_stat(transport, StatEvent::OutputUnderrun, pipeline, now_ms)?;
            self.pending_output_underrun = false;
        }
        if self.pending_not_supported {
            self.send_metrics_stat(transport, StatEvent::NotSupported, pipeline, now_ms)?;
            self.pending_not_supported = false;
        }

        if let Some(meta) = &self.pending_meta {
            let sent = send_payload_frame(transport, meta, SlimprotoBuilder::build_meta);
            settle_pending(&mut self.pending_meta, sent)?;
        }
        Ok(())
    }

    /// Sends a `STAT` carrying the current buffer, frame, rate and byte metrics.
    fn send_metrics_stat<S: AudioOutputSink, T: SlimTransport>(
        &self,
        transport: &mut T,
        event: StatEvent,
        pipeline: &StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<(), TransportError> {
        send_stat_with_metrics(
            transport,
            event,
            pipeline.stream_buffer.available_read() as u32,
            pipeline.output_buffer.available_read() as u32,
            pipeline.frames_played,
            self.current_sample_rate,
            self.stream_bytes_received,
            now_ms,
            0,
        )
    }

    /// Maps current player state to CYD display playback state.
    pub fn cyd_playback_state(&self) -> PlayState {
        match self.state {
            PlayerState::Playing | PlayerState::Buffering => PlayState::Playing,
            PlayerState::Paused => PlayState::Paused,
            _ => PlayState::Stopped,
        }
    }
}

/// `now` is at or past `deadline` on the wrapping 32-bit millisecond clock.
fn deadline_reached(now: u32, deadline: u32) -> bool {
    now.wrapping_sub(deadline) as i32 >= 0
}

/// The value of `StreamTitle='...';` in an ICY metadata block (C `stream.c` hands the block
/// to LMS as is; the CYD wants the title).
fn icy_stream_title(block: &[u8]) -> Option<&[u8]> {
    const KEY: &[u8] = b"StreamTitle='";
    let start = block.windows(KEY.len()).position(|w| w == KEY)? + KEY.len();
    let rest = &block[start..];
    let end = rest.windows(2).position(|w| w == b"';").unwrap_or(rest.len());
    let title = &rest[..end];
    (!title.is_empty()).then_some(title)
}

/// `text` cut to fit a 64-byte CYD field, on a `char` boundary.
fn truncated(text: &str) -> heapless::String<64> {
    let mut out = heapless::String::new();
    let _ = out.push_str(&text[..text.floor_char_boundary(64.min(text.len()))]);
    out
}

/// Sends a `RESP`. A head the transport can never take (`Io`, e.g. larger than the device's
/// 2 KB control-socket buffer) is cut at a header-line boundary, halving the limit until it
/// fits, rather than dropped. Returns the outcome and the head as last tried.
fn send_resp<T: SlimTransport>(
    transport: &mut T,
    mut head: Vec<u8>,
) -> (Result<(), TransportError>, Vec<u8>) {
    loop {
        let sent = send_payload_frame(transport, &head, SlimprotoBuilder::build_resp);
        if sent != Err(TransportError::Io) || head.len() <= 64 {
            return (sent, head);
        }
        head = crate::stream_codec::truncate_head(&head, head.len() / 2);
    }
}

/// Outcome of sending a deferred `RESP`/`META`: sent or failed for good clears it, `WouldBlock`
/// keeps it for the next tick (a frame that can never be sent must not block the link).
fn settle_pending(
    pending: &mut Option<Vec<u8>>,
    sent: Result<(), TransportError>,
) -> Result<(), TransportError> {
    if sent != Err(TransportError::WouldBlock) {
        *pending = None;
    }
    sent
}

/// Sends a compact `STAT` (frame counter as byte counter, elapsed time from the timestamp).
fn send_stat<T: SlimTransport>(
    transport: &mut T,
    event: StatEvent,
    stream_buf_fullness: u32,
    output_buf_fullness: u32,
    frames_played: u32,
    timestamp_ms: u32,
) -> Result<(), TransportError> {
    let mut buf = [0u8; 64];
    let len = SlimprotoBuilder::build_stat(
        event,
        stream_buf_fullness,
        output_buf_fullness,
        frames_played,
        timestamp_ms,
        &mut buf,
    )
    .map_err(|_| TransportError::Io)?;
    transport.send(&buf[..len])
}

/// Sends a `STAT` with explicit metrics (sample rate, 64-bit frames and bytes, server timestamp).
#[allow(clippy::too_many_arguments)]
fn send_stat_with_metrics<T: SlimTransport>(
    transport: &mut T,
    event: StatEvent,
    stream_buf_fullness: u32,
    output_buf_fullness: u32,
    frames_played: u64,
    sample_rate: u32,
    bytes_received: u64,
    jiffies_ms: u32,
    server_timestamp: u32,
) -> Result<(), TransportError> {
    let mut buf = [0u8; 128];
    let len = StatBuilder::build_stat_with_metrics(
        event,
        stream_buf_fullness,
        output_buf_fullness,
        frames_played,
        sample_rate,
        bytes_received,
        jiffies_ms,
        server_timestamp,
        &mut buf,
    )
    .map_err(|_| TransportError::Io)?;
    transport.send(&buf[..len])
}

/// Sends a `DSCO` disconnect notification.
fn send_dsco<T: SlimTransport>(
    transport: &mut T,
    reason: DisconnectReason,
) -> Result<(), TransportError> {
    let mut buf = [0u8; 16];
    let len = SlimprotoBuilder::build_dsco(reason, &mut buf).map_err(|_| TransportError::Io)?;
    transport.send(&buf[..len])
}

/// Sends a `SETD` player name (setting id 0).
fn send_setd_name<T: SlimTransport>(transport: &mut T, name: &str) -> Result<(), TransportError> {
    let mut buf = alloc::vec![0u8; 9 + name.len()];
    let len = SlimprotoBuilder::build_setd_name(name, &mut buf).map_err(|_| TransportError::Io)?;
    transport.send(&buf[..len])
}

/// Sends an 8-byte-header frame wrapping `payload` (`RESP`, `META`).
fn send_payload_frame<T: SlimTransport, E>(
    transport: &mut T,
    payload: &[u8],
    build: fn(&[u8], &mut [u8]) -> Result<usize, E>,
) -> Result<(), TransportError> {
    let mut buf = alloc::vec![0u8; 8 + payload.len()];
    let len = build(payload, &mut buf).map_err(|_| TransportError::Io)?;
    transport.send(&buf[..len])
}

#[cfg(test)]
mod tests {
    //! Coordinator tests drive [`PlayerCoordinator::tick`] over the in-memory I/O fakes
    //! ([`LoopbackTransport`], [`FakeStreamSource`] behind [`GatedSource`], [`MockBackend`]).
    //! End-to-end coverage over real sockets lives in `tests/session_replay.rs`.
    use super::*;
    use crate::io_traits::{
        FakeStreamSource, HeaderBuf, LoopbackTransport, MetaBuf, MockBackend, Segments,
        LOOPBACK_QUEUE_DEPTH,
    };
    use std::boxed::Box;
    use squeezelite_audio_core::MockAudioSink;
    use std::string::{String, ToString};
    use std::vec;

    type TestPipeline<'a> = StreamPipeline<'a, MockAudioSink<64>>;

    fn pcm_pipeline<'a>(stream: &'a mut [u8], output: &'a mut [u8]) -> TestPipeline<'a> {
        StreamPipeline::new(
            stream,
            output,
            ActiveDecoder::Pcm(PcmDecoder::new(16, 2, true)),
            MockAudioSink::<64>::new(),
            44100,
        )
    }

    /// Server frame with the 8-byte header (`opcode`, big-endian payload length).
    fn server_frame(opcode: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut frame = Vec::with_capacity(8 + payload.len());
        frame.extend_from_slice(opcode);
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(payload);
        frame
    }

    /// `strm` frame for 16-bit / 44.1 kHz (code 3) / stereo / big-endian PCM, flags 0.
    fn strm_frame(
        command: u8,
        autostart: bool,
        threshold_kb: u8,
        replay_gain: u32,
        server_port: u16,
        server_ip: [u8; 4],
        http_headers: &[u8],
    ) -> Vec<u8> {
        let mut payload = vec![0u8; 24];
        payload[0] = command;
        payload[1] = if autostart { b'1' } else { b'0' };
        payload[2] = b'p';
        payload[3] = 16;
        payload[4] = 3;
        payload[5] = 2;
        payload[6] = b'0'; // big endian
        payload[7] = threshold_kb;
        payload[14..18].copy_from_slice(&replay_gain.to_be_bytes());
        payload[18..20].copy_from_slice(&server_port.to_be_bytes());
        payload[20..24].copy_from_slice(&server_ip);
        payload.extend_from_slice(http_headers);
        server_frame(b"strm", &payload)
    }

    fn simple_strm(command: u8) -> Vec<u8> {
        strm_frame(command, false, 0, 0, 0, [0, 0, 0, 0], &[])
    }

    fn audg_frame(gain_left: u32, gain_right: u32, digital_atten: u8) -> Vec<u8> {
        let mut payload = vec![0u8; 18];
        payload[8] = digital_atten;
        payload[10..14].copy_from_slice(&gain_left.to_be_bytes());
        payload[14..18].copy_from_slice(&gain_right.to_be_bytes());
        server_frame(b"audg", &payload)
    }

    /// [`FakeStreamSource`] whose connect and data can be held back, standing in for a server
    /// that has not answered yet. It also records the last HTTP request.
    struct GatedSource {
        inner: FakeStreamSource,
        hold_connect: bool,
        hold_data: bool,
        last_request: Vec<u8>,
    }

    impl GatedSource {
        fn new(path: &'static [u8], segments: Segments) -> Self {
            let mut inner = FakeStreamSource::new();
            assert!(inner.add_route(path, segments));
            Self { inner, hold_connect: false, hold_data: false, last_request: Vec::new() }
        }
    }

    impl StreamSource for GatedSource {
        fn connect(&mut self, addr: NetAddr, http_request: &[u8]) -> Result<(), StreamError> {
            self.last_request = http_request.to_vec();
            if self.hold_connect {
                return Err(StreamError::WouldBlock);
            }
            self.inner.connect(addr, http_request)
        }

        fn poll_bytes(&mut self, out: &mut [u8]) -> Result<usize, StreamError> {
            if self.hold_data {
                return Ok(0);
            }
            self.inner.poll_bytes(out)
        }

        fn take_response_headers(&mut self) -> Option<HeaderBuf> {
            self.inner.take_response_headers()
        }

        fn take_icy_metadata(&mut self) -> Option<MetaBuf> {
            self.inner.take_icy_metadata()
        }

        fn set_icy_metaint(&mut self, interval: usize) {
            self.inner.set_icy_metaint(interval);
        }

        fn is_eof(&self) -> bool {
            !self.hold_data && self.inner.is_eof()
        }

        fn abort(&mut self) {
            self.inner.abort();
        }
    }

    /// One outbound frame: opcode and payload.
    type Sent = ([u8; 4], Vec<u8>);

    /// Coordinator plus its fakes; collects every frame the coordinator sends.
    struct Rig {
        coord: PlayerCoordinator,
        lms: LoopbackTransport,
        src: GatedSource,
        backend: MockBackend,
        sent: Vec<Sent>,
    }

    static NO_ROUTE: Segments = &[];

    impl Rig {
        fn new(mac: [u8; 6], name: &'static str) -> Self {
            Self::with_source(mac, name, GatedSource::new(b"/none", NO_ROUTE))
        }

        fn with_source(mac: [u8; 6], name: &'static str, src: GatedSource) -> Self {
            Self {
                coord: PlayerCoordinator::new(mac, name),
                lms: LoopbackTransport::new(),
                src,
                backend: MockBackend::new(),
                sent: Vec::new(),
            }
        }

        fn drain_sent(&mut self) {
            while let Some(frame) = self.lms.pop_sent() {
                let opcode = [frame[0], frame[1], frame[2], frame[3]];
                self.sent.push((opcode, frame[8..].to_vec()));
            }
        }

        fn tick<S: AudioOutputSink>(
            &mut self,
            pipeline: &mut StreamPipeline<'_, S>,
            now_ms: u32,
        ) -> Result<(), TransportError> {
            let result =
                self.coord.tick(&mut self.lms, &mut self.src, &mut self.backend, pipeline, now_ms);
            self.drain_sent();
            result
        }

        /// Delivers one server frame and runs the tick that handles it.
        fn command<S: AudioOutputSink>(
            &mut self,
            pipeline: &mut StreamPipeline<'_, S>,
            frame: &[u8],
            now_ms: u32,
        ) -> Option<CydMessage> {
            self.lms.inject_frame(frame).unwrap();
            self.tick(pipeline, now_ms).unwrap();
            self.coord.take_cyd_message()
        }

        /// Outbound frames as `STAT:STMx`, `DSCO:n`, `RESP`, `SETD`, ...
        fn lines(&self) -> Vec<String> {
            self.sent
                .iter()
                .map(|(op, payload)| match op {
                    b"STAT" => format!("STAT:{}", String::from_utf8_lossy(&payload[0..4])),
                    b"DSCO" => format!("DSCO:{}", payload[0]),
                    other => String::from_utf8_lossy(other).to_string(),
                })
                .collect()
        }
    }

    // Big-endian 16-bit stereo PCM: (1, -256)... two frames, used by the short-stream tests.
    const TWO_FRAMES: &[u8] = &[0x01, 0x00, 0xFF, 0x00, 0x02, 0x00, 0xFE, 0x00];
    const OK_HEAD: &[u8] = b"HTTP/1.0 200 OK\r\nContent-Type: audio/basic\r\n\r\n";
    static SHORT_STREAM: Segments = &[OK_HEAD, TWO_FRAMES];

    #[test]
    fn test_coordinator_stream_and_audio_lifecycle() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let mut rig =
            Rig::with_source(mac, "TestPlayer", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // Start stream: autostart, PCM 44.1 kHz, threshold 0, body held back by the server.
        // C parity: STREAMING_BUFFERING until a byte arrives (stream.c:527).
        rig.src.hold_data = true;
        let cyd_msg = rig.command(&mut pipeline, &strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[]), 100);
        assert_eq!(cyd_msg, Some(CydMessage::PlaybackState(PlayState::Playing)));
        assert_eq!(rig.coord.state, PlayerState::Buffering);
        assert_eq!(rig.backend.rate, 44100);

        // Feed 2 stereo frames: (1000, -1000), (2000, -2000)
        let f1_l = (1000i16).to_be_bytes();
        let f1_r = (-1000i16).to_be_bytes();
        let f2_l = (2000i16).to_be_bytes();
        let f2_r = (-2000i16).to_be_bytes();
        let raw_audio = [
            f1_l[0], f1_l[1], f1_r[0], f1_r[1],
            f2_l[0], f2_l[1], f2_r[0], f2_r[1],
        ];

        // The tick that ingests the body starts, decodes and plays it, then emits the STMs
        rig.command(&mut pipeline, &server_frame(b"body", &raw_audio), 150);
        assert_eq!(rig.coord.state, PlayerState::Playing);
        assert_eq!(rig.coord.last_tick_frames(), 2);
        assert!(!rig.coord.pending_track_started);
        // The output then runs dry with the stream still open: STMo (slimproto.c:881).
        assert_eq!(rig.lines(), ["STAT:STMf", "STAT:STMc", "RESP", "STAT:STMs", "STAT:STMo"]);

        // Verify recorded samples
        let rec = pipeline.sink.recorded_samples();
        assert_eq!(rec[0], 1000 << 16);
        assert_eq!(rec[1], -1000 << 16);

        // Volume adjustment with digital_atten: 1 (gain 0.5 = -6 dB): the amplifier attenuates,
        // software gain stays at unity (device policy, no double attenuation)
        let cyd_vol = rig.command(&mut pipeline, &audg_frame(32768, 32768, 1), 200);
        assert_eq!(cyd_vol, Some(CydMessage::Volume(91)));
        let reg = lms_gain_to_tas5805m_vol_reg(32768);
        assert_eq!(reg, 0x39);
        assert_eq!(rig.backend.hw_vol, (reg, reg));
        assert_eq!((rig.coord.volume.gain_left, rig.coord.volume.gain_right), (65536, 65536));

        // Pause
        let cyd_pause = rig.command(&mut pipeline, &simple_strm(b'p'), 250);
        assert_eq!(cyd_pause, Some(CydMessage::PlaybackState(PlayState::Paused)));
        assert_eq!(rig.coord.state, PlayerState::Paused);
        assert!(rig.backend.muted);

        assert_eq!(
            rig.lines(),
            ["STAT:STMf", "STAT:STMc", "RESP", "STAT:STMs", "STAT:STMo", "STAT:STMp"]
        );
    }

    #[test]
    fn test_threshold_kb_clamping_and_full_buffer_exit() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let mut rig =
            Rig::with_source(mac, "TestPlayer", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        rig.coord.external_audio_pump = true; // observe the state, not the decode
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // threshold_kb = 255 -> 255 * 1024 = 261120 bytes requested.
        // Capacity is 2048 bytes. max_safe_threshold = 2048.saturating_sub(1024) = 1024.
        // effective_threshold should clamp to 1024, preventing buffer deadlock.
        start_held(&mut rig, &mut pipeline, true, 255);
        assert_eq!(rig.coord.buffer_threshold_bytes, 1024);

        // With empty buffer, coordinator remains in Buffering
        rig.tick(&mut pipeline, 20).unwrap();
        assert_eq!(rig.coord.state, PlayerState::Buffering);

        // C parity: more than the threshold is needed (stream.c:527 `bytes > threshold`).
        pipeline.ingest_stream_data(&[0x55u8; 1024]).unwrap();
        rig.tick(&mut pipeline, 30).unwrap();
        assert_eq!(rig.coord.state, PlayerState::Buffering);
        pipeline.ingest_stream_data(&[0x55u8; 1]).unwrap();
        rig.tick(&mut pipeline, 40).unwrap();
        assert_eq!(rig.coord.state, PlayerState::Playing);

        // Full-buffer exit when available_write() == 0 even before reaching threshold
        start_held(&mut rig, &mut pipeline, true, 1);
        rig.coord.buffer_threshold_bytes = 2000;
        // Fill buffer until completely full (capacity - 1 due to slot reservation)
        pipeline.ingest_stream_data(&[0xAAu8; 2047]).unwrap();
        assert_eq!(pipeline.stream_buffer.available_write(), 0);
        rig.tick(&mut pipeline, 50).unwrap();
        assert_eq!(rig.coord.state, PlayerState::Playing);
    }

    #[test]
    fn test_strm_threshold_kb_is_clamped_to_stream_buffer() {
        // The same clamp, through a real `strm s` (threshold_kb = 255, 2 KiB stream buffer)
        let mut rig = Rig::with_source(
            [0x11, 0x22, 0x33, 0x44, 0x55, 0x66],
            "TestPlayer",
            GatedSource::new(b"/stream.mp3", SHORT_STREAM),
        );
        rig.src.hold_data = true;
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        let msg = rig.command(&mut pipeline, &strm_frame(b's', true, 255, 0, 9000, [127, 0, 0, 1], &[]), 10);
        assert_eq!(msg, Some(CydMessage::PlaybackState(PlayState::Playing)));
        assert_eq!(rig.coord.buffer_threshold_bytes, 1024);
        assert_eq!(rig.coord.state, PlayerState::Buffering);
    }

    #[test]
    fn test_autostart_false_transitions_to_paused() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let mut rig =
            Rig::with_source(mac, "TestPlayer", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // autostart = false with buffering
        start_held(&mut rig, &mut pipeline, false, 0);
        rig.coord.buffer_threshold_bytes = 512;
        rig.tick(&mut pipeline, 20).unwrap();
        assert_eq!(rig.coord.state, PlayerState::Buffering);

        // More than the threshold arrives
        pipeline.ingest_stream_data(&[0x77u8; 513]).unwrap();
        rig.tick(&mut pipeline, 30).unwrap();
        // Because autostart is false, transitions to Paused and reports STMl (C autostart 0)
        assert_eq!(rig.coord.state, PlayerState::Paused);
        assert_eq!(rig.lines().last().map(String::as_str), Some("STAT:STMl"));
    }

    #[test]
    fn test_normalize_slimproto_pcm_wire_codes() {
        // 1. Sample rate decoding (both ASCII '0'..='9' and numeric 0..=9)
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'0'), Some(11025));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(0), Some(11025));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'1'), Some(22050));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(1), Some(22050));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'2'), Some(32000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(2), Some(32000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'3'), Some(44100));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(3), Some(44100));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'4'), Some(48000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(4), Some(48000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'5'), Some(8000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(5), Some(8000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'6'), Some(12000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(6), Some(12000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'7'), Some(16000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(7), Some(16000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'8'), Some(24000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(8), Some(24000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'9'), Some(96000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(9), Some(96000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b':'), Some(88200));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(10), Some(88200));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'x'), None);

        // 2. Sample size / bits normalization
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(b'0'), 8);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(0), 8);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(8), 8);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(b'1'), 16);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(1), 16);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(16), 16);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(b'2'), 24);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(2), 24);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(24), 24);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(b'3'), 32);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(3), 32);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(32), 32);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(99), 16); // Default fallback

        // 3. Channel count normalization
        assert_eq!(PlayerCoordinator::normalize_pcm_channels(b'1'), 1);
        assert_eq!(PlayerCoordinator::normalize_pcm_channels(1), 1);
        assert_eq!(PlayerCoordinator::normalize_pcm_channels(b'2'), 2);
        assert_eq!(PlayerCoordinator::normalize_pcm_channels(2), 2);
        assert_eq!(PlayerCoordinator::normalize_pcm_channels(0), 2); // Default fallback

        // 4. Endianness normalization
        assert!(PlayerCoordinator::normalize_pcm_big_endian(b'0'));
        assert!(PlayerCoordinator::normalize_pcm_big_endian(0));
        assert!(!PlayerCoordinator::normalize_pcm_big_endian(b'1'));
        assert!(!PlayerCoordinator::normalize_pcm_big_endian(1));
    }

    #[test]
    fn test_cyd_command_pause_and_toggle_during_buffering() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let mut coordinator = PlayerCoordinator::new(mac, "TestPlayer");

        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // 1. CydCommand::Pause during Buffering
        coordinator.state = PlayerState::Buffering;
        coordinator.buffer_threshold_bytes = 1024;
        pipeline.sink.set_mute(false);

        let msg = coordinator.handle_cyd_command(CydCommand::Pause, &mut MockBackend::new(), &mut pipeline);
        assert_eq!(msg, Some(CydMessage::PlaybackState(PlayState::Paused)));
        assert_eq!(coordinator.state, PlayerState::Paused);
        assert_eq!(coordinator.buffer_threshold_bytes, 0);
        assert!(pipeline.sink.muted);

        // 2. CydCommand::Toggle during Buffering
        coordinator.state = PlayerState::Buffering;
        coordinator.buffer_threshold_bytes = 2048;
        pipeline.sink.set_mute(false);

        let msg = coordinator.handle_cyd_command(CydCommand::Toggle, &mut MockBackend::new(), &mut pipeline);
        assert_eq!(msg, Some(CydMessage::PlaybackState(PlayState::Paused)));
        assert_eq!(coordinator.state, PlayerState::Paused);
        assert_eq!(coordinator.buffer_threshold_bytes, 0);
        assert!(pipeline.sink.muted);

        // 3. CydCommand::Toggle while Paused transitions back to Playing
        let msg = coordinator.handle_cyd_command(CydCommand::Toggle, &mut MockBackend::new(), &mut pipeline);
        assert_eq!(msg, Some(CydMessage::PlaybackState(PlayState::Playing)));
        assert_eq!(coordinator.state, PlayerState::Playing);
        assert!(!pipeline.sink.muted);
    }

    #[test]
    fn test_flush_resets_buffer_threshold_and_exits_buffering() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let mut rig =
            Rig::with_source(mac, "TestPlayer", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        start_held(&mut rig, &mut pipeline, true, 1);
        assert_eq!(rig.coord.state, PlayerState::Buffering);
        assert_eq!(rig.coord.buffer_threshold_bytes, 1024);
        rig.sent.clear();

        let msg = rig.command(&mut pipeline, &simple_strm(b'f'), 20);
        assert_eq!(msg, None);
        assert_eq!(rig.coord.state, PlayerState::Stopped);
        assert_eq!(rig.coord.buffer_threshold_bytes, 0);
        // C parity (slimproto.c:380): a stream was disconnected, so STMf.
        assert_eq!(rig.lines(), ["STAT:STMf"]);
    }

    #[test]
    fn test_coordinator_end_to_end_http_audio_streaming() {
        // 4 stereo frames of 16-bit big-endian PCM: (100,-100) (200,-200) (300,-300) (400,-400)
        static STREAM: Segments = &[
            OK_HEAD,
            b"\x00\x64\xff\x9c\x00\xc8\xff\x38\x01\x2c\xfe\xd4\x01\x90\xfe\x70",
        ];
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x88];
        let mut rig = Rig::with_source(mac, "TestPlayer", GatedSource::new(b"/stream.pcm", STREAM));

        let mut stream_storage = [0u8; 4096];
        let mut output_storage = [0u8; 4096];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        let start = strm_frame(
            b's',
            true,
            0,
            0,
            9000,
            [127, 0, 0, 1],
            b"GET /stream.pcm HTTP/1.0\r\nHost: 127.0.0.1:9000\r\n\r\n",
        );
        let cyd_msg = rig.command(&mut pipeline, &start, 100);
        assert_eq!(cyd_msg, Some(CydMessage::PlaybackState(PlayState::Playing)));
        assert_eq!(rig.src.inner.last_addr, Some(NetAddr { ip: [127, 0, 0, 1], port: 9000 }));

        // Tick until all 4 frames are decoded and recorded
        let mut total_frames = rig.coord.last_tick_frames();
        for _ in 0..100 {
            if total_frames >= 4 {
                break;
            }
            rig.tick(&mut pipeline, 100).unwrap();
            total_frames += rig.coord.last_tick_frames();
        }

        assert_eq!(total_frames, 4);
        let samples = pipeline.sink.recorded_samples();
        assert!(samples.len() >= 8);
        assert_eq!(samples[0], 100 << 16);
        assert_eq!(samples[1], -100 << 16);
        assert_eq!(samples[2], 200 << 16);
        assert_eq!(samples[3], -200 << 16);
        assert_eq!(samples[4], 300 << 16);
        assert_eq!(samples[5], -300 << 16);
        assert_eq!(samples[6], 400 << 16);
        assert_eq!(samples[7], -400 << 16);

        // Stop command: no reply (C slimproto.c:386-394), stream dropped
        rig.command(&mut pipeline, &simple_strm(b'q'), 200);
        assert!(!rig.coord.stream_active());
        assert_eq!(rig.coord.state, PlayerState::Stopped);

        assert_eq!(rig.lines()[..2], ["STAT:STMf", "STAT:STMc"]);
        assert!(!rig.lines().contains(&"STAT:STMq".to_string()));
    }

    #[test]
    fn test_short_eof_stream_exits_buffering_and_emits_stms_stmu() {
        // COORD-501, COORD-502, COORD-504:
        // A short track smaller than threshold_kb that hits HTTP EOF must:
        // 1. Transition out of Buffering -> Playing and emit STMs once PCM is written
        // 2. Once all decoded PCM frames are played out, transition Playing -> Stopped
        // 3. Emit DSCO(0) at EOF (stream.c:508), then STMd and STMu (C status-loop order)
        let mac = [0x22, 0x33, 0x44, 0x55, 0x66, 0x77];
        let mut rig =
            Rig::with_source(mac, "TestPlayer", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        rig.src.hold_data = true;

        let mut stream_storage = [0u8; 4096];
        let mut output_storage = [0u8; 4096];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // threshold_kb = 2 (2048 bytes) and empty http_headers (COORD-503 default request)
        let start = strm_frame(b's', true, 2, 0, 9000, [127, 0, 0, 1], &[]);
        rig.command(&mut pipeline, &start, 10);
        assert_eq!(rig.coord.state, PlayerState::Buffering);
        assert_eq!(rig.src.last_request, DEFAULT_HTTP_REQUEST);

        // The 2 frames (8 bytes, far below the threshold) arrive, then EOF
        rig.src.hold_data = false;
        for _ in 0..100 {
            if rig.coord.state == PlayerState::Stopped {
                break;
            }
            rig.tick(&mut pipeline, 20).unwrap();
        }

        assert_eq!(rig.coord.state, PlayerState::Stopped);
        assert_eq!(pipeline.frames_played, 2);
        assert_eq!(rig.coord.frames_played, 2);

        assert_eq!(
            rig.lines(),
            ["STAT:STMf", "STAT:STMc", "RESP", "DSCO:0", "STAT:STMs", "STAT:STMd", "STAT:STMu"]
        );
        assert!(rig.sent[2].1.starts_with(b"HTTP/1.0 200 OK"));
    }

    #[test]
    fn test_audio_gain_u64_no_overflow_and_sync_cyd_telemetry() {
        // COORD-505, COORD-507: Verify u32::MAX gain does not overflow and sync_cyd_telemetry
        // returns PlaybackState, Volume, and Progress.
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x99];
        let mut rig = Rig::new(mac, "TestPlayer");

        let mut stream_storage = [0u8; 512];
        let mut output_storage = [0u8; 512];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        let msg = rig.command(&mut pipeline, &audg_frame(u32::MAX, u32::MAX, 0), 50);
        assert_eq!(msg, Some(CydMessage::Volume(100)));
        assert_eq!(rig.backend.hw_vol, (0x30, 0x30)); // 0 dB

        rig.coord.state = PlayerState::Playing;
        pipeline.frames_played = 44100 * 12; // 12 seconds at 44.1 kHz
        let telemetry = rig.coord.sync_cyd_telemetry(&pipeline);
        assert_eq!(telemetry.len(), 3);
        assert_eq!(telemetry[0], CydMessage::PlaybackState(PlayState::Playing));
        assert_eq!(telemetry[1], CydMessage::Volume(100));
        assert_eq!(
            telemetry[2],
            CydMessage::Progress {
                elapsed_secs: 12,
                duration_secs: 0
            }
        );
    }

    #[test]
    fn test_latin1_headers_and_http_error_reports_unreachable() {
        // COORD-503, COORD-509: Non-UTF8 (Latin-1) headers are sent with each non-ASCII byte
        // replaced by '?'. An HTTP 404 answer arriving after the connect went pending is a
        // connect failure: STMc + DSCO(3), Stopped, no STMn (slimproto.c:473, stream.c:687-688).
        static NOT_FOUND: Segments = &[b"HTTP/1.0 404 Not Found\r\nContent-Length: 0\r\n\r\n"];
        let mac = [0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
        let mut rig = Rig::with_source(mac, "TestPlayer", GatedSource::new(b"/caf?.pcm", NOT_FOUND));
        rig.src.hold_connect = true; // the server has not answered yet

        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // Pre-populate pipeline with dirty state to verify Start flushes and resets frames_played (COORD-506, COORD-509)
        pipeline.ingest_stream_data(&[1, 2, 3, 4]).unwrap();
        pipeline.frames_played = 999;
        rig.coord.frames_played = 999;

        // Latin-1 invalid UTF-8 byte 0xE9 ('é') in HTTP request header (COORD-503)
        let start = strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], b"GET /caf\xe9.pcm HTTP/1.0\r\n\r\n");
        rig.command(&mut pipeline, &start, 100);
        assert_eq!(pipeline.frames_played, 0);
        assert_eq!(rig.coord.frames_played, 0);
        assert_eq!(pipeline.stream_buffer_available(), 0);
        assert!(rig.coord.stream_active());
        assert_eq!(rig.src.last_request, b"GET /caf?.pcm HTTP/1.0\r\n\r\n");
        assert_eq!(rig.lines(), ["STAT:STMf"], "STMc waits for the response");

        // The 404 answer arrives
        rig.src.hold_connect = false;
        rig.tick(&mut pipeline, 110).unwrap();
        assert!(!rig.coord.stream_active());
        assert_eq!(rig.coord.state, PlayerState::Stopped);
        assert_eq!(rig.coord.take_cyd_message(), Some(CydMessage::PlaybackState(PlayState::Stopped)));
        assert_eq!(rig.lines(), ["STAT:STMf", "STAT:STMc", "DSCO:3"]);
    }

    #[test]
    fn test_poll_error_after_connect_drops_stream() {
        // COORD-508: a stream that connected and then fails while polling (truncated chunked
        // body) is dropped and marked ended; it is not a connect failure. C parity: a read
        // error is REMOTE_DISCONNECT, DSCO(2) (stream.c:364/510).
        static TRUNCATED: Segments =
            &[b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n8\r\n\x01\x00\xff\x00"];
        let mac = [0x33, 0x44, 0x55, 0x66, 0x77, 0x89];
        let mut rig = Rig::with_source(mac, "TestPlayer", GatedSource::new(b"/stream.mp3", TRUNCATED));

        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        rig.command(&mut pipeline, &strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[]), 10);
        for _ in 0..10 {
            if !rig.coord.stream_active() {
                break;
            }
            rig.tick(&mut pipeline, 20).unwrap();
        }
        assert!(!rig.coord.stream_active());
        assert!(rig.coord.is_stream_eof());
        assert_eq!(rig.coord.stream_bytes_received, 4);
        assert_eq!(rig.lines()[..3], ["STAT:STMf", "STAT:STMc", "RESP"]);
        assert!(!rig.lines().contains(&"STAT:STMn".to_string()));
        assert_eq!(rig.lines().iter().filter(|l| *l == "DSCO:2").count(), 1, "{:?}", rig.lines());
    }

    #[test]
    fn test_redirect_failure_is_connect_failure_immediately_or_after_wouldblock() {
        // A redirect whose target cannot be fetched fails the connect. Whether that surfaces on
        // the first `connect` call or after `WouldBlock`, the result is identical.
        static REDIRECT: Segments = &[b"HTTP/1.0 302 Found\r\nLocation: /gone\r\n\r\n"];
        let run = |hold_connect: bool| {
            let mac = [0x33, 0x44, 0x55, 0x66, 0x77, 0x8A];
            let mut rig = Rig::with_source(mac, "TestPlayer", GatedSource::new(b"/r", REDIRECT));
            rig.src.hold_connect = hold_connect;
            let mut stream_storage = [0u8; 1024];
            let mut output_storage = [0u8; 1024];
            let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

            let start = strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], b"GET /r HTTP/1.0\r\n\r\n");
            let mut cyd = rig.command(&mut pipeline, &start, 10);
            if hold_connect {
                assert_eq!(cyd, Some(CydMessage::PlaybackState(PlayState::Playing)));
                assert_eq!(rig.lines(), ["STAT:STMf"]);
                rig.src.hold_connect = false;
                rig.tick(&mut pipeline, 20).unwrap();
                cyd = rig.coord.take_cyd_message();
            }
            assert_eq!(rig.src.inner.redirects_followed, 1);
            assert_eq!(cyd, Some(CydMessage::PlaybackState(PlayState::Stopped)));
            assert_eq!(rig.coord.state, PlayerState::Stopped);
            assert!(!rig.coord.stream_active());
            assert!(!rig.coord.is_stream_eof());
            rig.lines()
        };
        let immediate = run(false);
        let resumed = run(true);
        assert_eq!(immediate, ["STAT:STMf", "STAT:STMc", "DSCO:3"]);
        assert_eq!(resumed, immediate);
    }

    #[test]
    fn test_coordinator_defers_stms_until_pcm_output_written() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0xAA];
        let mut rig =
            Rig::with_source(mac, "TestPlayer", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        rig.src.hold_data = true;

        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        let start = strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[]);
        let msg = rig.command(&mut pipeline, &start, 100);
        assert_eq!(msg, Some(CydMessage::PlaybackState(PlayState::Playing)));
        assert_eq!(rig.coord.state, PlayerState::Buffering);
        // STMs NOT emitted prematurely before PCM output is written
        assert!(!rig.coord.pending_track_started);
        assert_eq!(rig.lines(), ["STAT:STMf", "STAT:STMc", "RESP"]);

        // PCM arrives: the tick that writes it to the sink emits the deferred STMs
        rig.src.hold_data = false;
        let mut written = 0;
        for _ in 0..100 {
            rig.tick(&mut pipeline, 150).unwrap();
            written = rig.coord.last_tick_frames();
            if written > 0 {
                break;
            }
        }
        assert!(written > 0);
        assert!(!rig.coord.pending_track_started);

        let lines = rig.lines();
        assert!(lines[..3] == ["STAT:STMf", "STAT:STMc", "RESP"], "{lines:?}");
        assert!(lines.contains(&"STAT:STMs".to_string()), "{lines:?}");
        assert!(rig.sent[2].1.starts_with(b"HTTP/1.0 200 OK"));
    }

    #[test]
    fn test_coordinator_eof_emits_stmd_then_stmu_after_output_drain() {
        // Steps the private stages one by one so the output buffer still holds frames when the
        // decoder finishes. C parity: DSCO(0) as soon as the socket hits EOF (stream.c:508),
        // STMd on DECODE_COMPLETE, STMu only after the running output drained (slimproto.c:872).
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0xBB];
        let mut rig =
            Rig::with_source(mac, "TestPlayer", GatedSource::new(b"/stream.mp3", SHORT_STREAM));

        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        let frame = strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[]);
        let (start_cmd, _) = SlimprotoParser::parse_frame(&frame).unwrap();
        rig.coord
            .handle_command(start_cmd, &mut rig.lms, &mut rig.src, &mut rig.backend, &mut pipeline, 10)
            .unwrap();

        // 1. Headers, body and EOF: RESP pending, DSCO(0) pending at once
        for _ in 0..100 {
            if rig.coord.is_stream_eof() {
                break;
            }
            let _ = rig.coord.poll_stream(&mut rig.src, &mut pipeline);
        }
        assert!(rig.coord.pending_resp.is_some());
        assert_eq!(rig.coord.pending_dsco, Some(DisconnectReason::ConnectionClosed));
        rig.coord.evaluate_state(&mut pipeline);
        assert_eq!(rig.coord.state, PlayerState::Playing, "clean EOF starts a short track");
        rig.coord.check_heartbeat(&mut rig.lms, &pipeline, 20).unwrap();

        // 2. Decode the stream into the output buffer until the decoder is done
        for _ in 0..100 {
            if rig.coord.decode_complete_sent {
                break;
            }
            let _ = pipeline.process_decode_step();
            rig.coord.evaluate_state(&mut pipeline);
        }
        assert!(rig.coord.pending_decode_ready);
        // Output buffer still holds frames
        assert!(pipeline.output_buffer.available_read() > 0);
        assert_eq!(rig.coord.state, PlayerState::Playing);
        rig.coord.check_heartbeat(&mut rig.lms, &pipeline, 30).unwrap();

        // 3. Output buffer drains
        assert_eq!(rig.coord.pump_output(&mut pipeline, 4), 2);
        assert_eq!(pipeline.output_buffer.available_read(), 0);
        rig.coord.evaluate_state(&mut pipeline);
        assert_eq!(rig.coord.state, PlayerState::Stopped);
        assert!(rig.coord.pending_underrun);
        rig.coord.check_heartbeat(&mut rig.lms, &pipeline, 40).unwrap();
        assert!(!rig.coord.pending_underrun);

        rig.drain_sent();
        assert_eq!(
            rig.lines(),
            ["STAT:STMf", "STAT:STMc", "RESP", "DSCO:0", "STAT:STMd", "STAT:STMs", "STAT:STMu"]
        );
        assert!(rig.sent[2].1.starts_with(b"HTTP/1.0 200 OK"));
        assert_eq!(rig.sent[3].1[0], DisconnectReason::ConnectionClosed as u8);
    }

    #[test]
    fn test_coordinator_timestamp_probe_and_setd_aude_cont() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0xCC];
        let mut rig = Rig::new(mac, "TestPlayer");

        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // 1. strm 't' (Timestamp probe) with replay_gain = 0x12345678
        rig.command(&mut pipeline, &strm_frame(b't', false, 0, 0x1234_5678, 0, [0, 0, 0, 0], &[]), 100);

        // 2. setd (Device setting id = 0, update player name to "Kitchen")
        rig.command(&mut pipeline, &server_frame(b"setd", b"\x00Kitchen"), 200);
        assert_eq!(rig.coord.player_name.as_str(), "Kitchen");

        // 3. aude (Audio enable: spdif = 1, dac = 0 -> mute dac, amp off)
        rig.command(&mut pipeline, &server_frame(b"aude", &[1, 0]), 300);
        assert!(!rig.coord.dac_enabled);
        assert!(pipeline.sink.muted);
        assert!(!rig.backend.powered);

        // aude (dac = 1 -> unmute dac, amp on)
        rig.command(&mut pipeline, &server_frame(b"aude", &[1, 1]), 350);
        assert!(rig.coord.dac_enabled);
        assert!(!pipeline.sink.muted);
        assert!(rig.backend.powered);

        // 4. cont (Stream continue: releases awaiting_cont)
        rig.coord.awaiting_cont = true;
        rig.command(&mut pipeline, &server_frame(b"cont", &8192u32.to_be_bytes()), 400);
        assert!(!rig.coord.awaiting_cont);

        assert_eq!(rig.lines(), ["STAT:STMt", "SETD"]);
        // STAT with server_timestamp = 0x12345678
        assert_eq!(&rig.sent[0].1[47..51], &0x1234_5678u32.to_be_bytes());
        // SETD name reply with "Kitchen"
        assert_eq!(rig.sent[1].1[0], 0x00);
        assert_eq!(&rig.sent[1].1[1..], b"Kitchen");
    }

    #[test]
    fn test_setd_one_byte_query_preserves_sample_rate_and_replies_name() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0xDD];
        let mut rig = Rig::new(mac, "Basement");
        assert_eq!(rig.coord.current_sample_rate, 44100);

        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // 1-byte setd [0x00] query from LMS
        rig.command(&mut pipeline, &server_frame(b"setd", &[0x00]), 100);

        assert_eq!(rig.coord.current_sample_rate, 44100);
        assert_eq!(rig.coord.player_name.as_str(), "Basement");

        // SETD name reply with "Basement"
        assert_eq!(rig.lines(), ["SETD"]);
        assert_eq!(rig.sent[0].1[0], 0x00);
        assert_eq!(&rig.sent[0].1[1..], b"Basement");
    }

    #[test]
    fn test_coordinator_audg_digital_atten_zero_unity_gain_fallback() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0xEE];
        let mut rig = Rig::new(mac, "Test");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // Initially unity gain (65536, 65536)
        assert_eq!(rig.coord.volume.gain_left, 65536);
        assert_eq!(rig.coord.volume.gain_right, 65536);

        // Task 14, re-pinned to C (slimproto.c:569): digital_atten (adjust) 0 is fixed volume,
        // `set_volume(FIXED_ONE, FIXED_ONE)` whatever the gains: unity in software and 0 dB on
        // the amplifier, never mute.
        rig.command(&mut pipeline, &audg_frame(0, 0, 0), 100);
        assert_eq!(rig.coord.volume.gain_left, 65536);
        assert_eq!(rig.coord.volume.gain_right, 65536);
        assert_eq!(rig.backend.hw_vol, (0x30, 0x30));

        // 4-byte player name [0, 'B', 'o', 'b'] updates player_name and replies with SETD without altering current_sample_rate
        rig.command(&mut pipeline, &server_frame(b"setd", &[0x00, b'B', b'o', b'b']), 200);
        assert_eq!(rig.coord.player_name.as_str(), "Bob");
        assert_eq!(rig.coord.current_sample_rate, 44100);

        // 1-byte id=4 DAC query does not alter current_sample_rate
        rig.command(&mut pipeline, &server_frame(b"setd", &[4]), 300);
        assert_eq!(rig.coord.current_sample_rate, 44100);
        assert_eq!(rig.lines(), ["SETD"]);

        // Sample rate decoding for 176.4k and 192k
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b';'), Some(176400));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(11), Some(176400));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'<'), Some(192000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(12), Some(192000));

        // Stream EOF tracking and reset
        rig.coord.set_stream_eof(true);
        assert!(rig.coord.is_stream_eof());
        rig.coord.reset();
        assert!(!rig.coord.is_stream_eof());
    }

    #[test]
    fn test_digital_atten_zero_forces_unity_sw_gain() {
        // C parity: slimproto.c:569 `set_volume(adjust ? gainL : FIXED_ONE, adjust ? gainR :
        // FIXED_ONE)`; the gains are ignored, mute included.
        for hw_volume in [true, false] {
            let mut rig = Rig::new([0x11, 0x22, 0x33, 0x44, 0x55, 0x07], "Test");
            rig.backend.hw_volume = hw_volume;
            let (mut s, mut o) = ([0u8; 1024], [0u8; 1024]);
            let mut pipeline = pcm_pipeline(&mut s, &mut o);
            // A real volume first, so unity is a change.
            rig.command(&mut pipeline, &audg_frame(0x4000, 0x2000, 1), 10);
            for (n, (l, r)) in [(0x4000u32, 0x2000u32), (0, 0), (0x1_0000, 0)].into_iter().enumerate() {
                let msg = rig.command(&mut pipeline, &audg_frame(l, r, 0), 20 + n as u32);
                assert_eq!(msg, Some(CydMessage::Volume(100)), "hw {hw_volume} ({l:#x}, {r:#x})");
                assert_eq!(
                    (pipeline.gain_stage.volume_l_q16, pipeline.gain_stage.volume_r_q16),
                    (0x1_0000, 0x1_0000)
                );
                assert_eq!(pipeline.requested_volume_q16(), (0x1_0000, 0x1_0000));
                if hw_volume {
                    // 0 dB on the TAS5805M (DIG_VOL 0x30), never 0xFF (mute).
                    assert_eq!(rig.backend.hw_vol, (0x30, 0x30));
                } else {
                    assert_eq!(rig.backend.hw_vol, (0, 0), "no hardware volume to drive");
                }
            }
        }
    }

    #[test]
    fn test_audg_balance_over_hardware_volume() {
        let mut rig = Rig::new([0x11, 0x22, 0x33, 0x44, 0x55, 0x09], "Test");
        let (mut s, mut o) = ([0u8; 1024], [0u8; 1024]);
        let mut pipeline = pcm_pipeline(&mut s, &mut o);
        let sw = |p: &TestPipeline<'_>| (p.gain_stage.volume_l_q16, p.gain_stage.volume_r_q16);
        // Balanced full volume: exactly unity, not 0 (u32 l << 16 would wrap to 0).
        rig.command(&mut pipeline, &audg_frame(0x1_0000, 0x1_0000, 1), 10);
        assert_eq!(sw(&pipeline), (0x1_0000, 0x1_0000));
        assert_eq!(rig.backend.hw_vol, (0x30, 0x30));
        // Above unity (LMS never sends it, but no wrap either).
        rig.command(&mut pipeline, &audg_frame(u32::MAX, u32::MAX, 1), 11);
        assert_eq!(sw(&pipeline), (0x1_0000, 0x1_0000));
        // Hard left / hard right: the amplifier from the louder side, the other side silent.
        rig.command(&mut pipeline, &audg_frame(0x8000, 0, 1), 12);
        assert_eq!(sw(&pipeline), (0x1_0000, 0));
        assert_eq!(rig.backend.hw_vol, (0x39, 0x39));
        rig.command(&mut pipeline, &audg_frame(0, 0x8000, 1), 13);
        assert_eq!(sw(&pipeline), (0, 0x1_0000));
        assert_eq!(rig.backend.hw_vol, (0x39, 0x39));
        // Partial balance: ratio exact to Q16.
        rig.command(&mut pipeline, &audg_frame(0x4000, 0x1_0000, 1), 14);
        assert_eq!(sw(&pipeline), (0x4000, 0x1_0000));
        assert_eq!(rig.backend.hw_vol, (0x30, 0x30));
        // The output applies it: right side at full, left at a quarter.
        let mut frame = [4000 << 16, 4000 << 16];
        pipeline.gain_stage.apply(&mut frame);
        assert_eq!(frame, [1000 << 16, 4000 << 16]);
    }

    #[test]
    fn test_local_volume_changes_keep_the_balance() {
        // CYD SetVolume (and the HTTP/IR and boot paths, through the same entry) set the level
        // through the hardware/software policy and keep LMS's L/R ratio.
        let mut rig = Rig::new([0x11, 0x22, 0x33, 0x44, 0x55, 0x0A], "Test");
        let (mut s, mut o) = ([0u8; 1024], [0u8; 1024]);
        let mut pipeline = pcm_pipeline(&mut s, &mut o);
        rig.command(&mut pipeline, &audg_frame(0x1_0000, 0x4000, 1), 10);
        assert_eq!((pipeline.gain_stage.volume_l_q16, pipeline.gain_stage.volume_r_q16), (0x1_0000, 0x4000));
        let msg = rig.coord.handle_cyd_command(CydCommand::SetVolume(50), &mut rig.backend, &mut pipeline);
        assert_eq!(msg, Some(CydMessage::Volume(50)));
        // The amplifier gets the 50 % register (same as percent_to_attenuation), the software
        // keeps the quarter on the right.
        assert_eq!(rig.backend.hw_vol, (crate::amp::percent_to_attenuation(50), crate::amp::percent_to_attenuation(50)));
        assert_eq!(pipeline.gain_stage.volume_l_q16, 0x1_0000);
        assert!(pipeline.gain_stage.volume_r_q16.abs_diff(0x4000) <= 2, "{}", pipeline.gain_stage.volume_r_q16);
        // The requested volume (Task 16's loudness input) follows: louder side at the 50 % level.
        let (l, r) = pipeline.requested_volume_q16();
        assert_eq!(lms_gain_to_hardware_volume_percent(l.max(r)), 50);
        assert!((u64::from(r) * 4).abs_diff(u64::from(l)) <= 4, "({l}, {r})");
        // The percent entry used by HTTP/IR and boot behaves the same.
        rig.coord.set_volume_percent(80, &mut rig.backend, &mut pipeline);
        assert_eq!(rig.backend.hw_vol.0, crate::amp::percent_to_attenuation(80));
        assert_eq!(pipeline.gain_stage.volume_l_q16, 0x1_0000);
        assert!(pipeline.gain_stage.volume_r_q16.abs_diff(0x4000) <= 2);
        // Without hardware volume the level goes into software, still balanced.
        rig.backend.hw_volume = false;
        rig.coord.set_volume_percent(100, &mut rig.backend, &mut pipeline);
        assert_eq!(pipeline.gain_stage.volume_l_q16, 0x1_0000);
        assert!(pipeline.gain_stage.volume_r_q16.abs_diff(0x4000) <= 2);
        // 0 % silences; balance is unknown after a (0, 0) and the next level is centred.
        rig.coord.set_volume_percent(0, &mut rig.backend, &mut pipeline);
        assert_eq!((pipeline.gain_stage.volume_l_q16, pipeline.gain_stage.volume_r_q16), (0, 0));
        rig.coord.set_volume_percent(100, &mut rig.backend, &mut pipeline);
        assert_eq!((pipeline.gain_stage.volume_l_q16, pipeline.gain_stage.volume_r_q16), (0x1_0000, 0x1_0000));
    }

    #[test]
    fn test_cyd_telemetry_volume_from_requested_volume() {
        // Hard-left balance must not read as half volume: the percent is the louder side's,
        // as the audg CYD note.
        let mut rig = Rig::new([0x11, 0x22, 0x33, 0x44, 0x55, 0x0B], "Test");
        let (mut s, mut o) = ([0u8; 1024], [0u8; 1024]);
        let mut pipeline = pcm_pipeline(&mut s, &mut o);
        let note = rig.command(&mut pipeline, &audg_frame(1842, 0, 1), 10);
        assert_eq!(note, Some(CydMessage::Volume(55)));
        assert_eq!(rig.coord.sync_cyd_telemetry(&pipeline)[1], CydMessage::Volume(55));
        rig.command(&mut pipeline, &audg_frame(0, 0x1_0000, 1), 20);
        assert_eq!(rig.coord.sync_cyd_telemetry(&pipeline)[1], CydMessage::Volume(100));
        rig.command(&mut pipeline, &audg_frame(0, 0, 1), 30);
        assert_eq!(rig.coord.sync_cyd_telemetry(&pipeline)[1], CydMessage::Volume(0));
    }

    #[test]
    fn test_audg_software_volume_without_hardware_volume() {
        // C parity: output_embedded.c:118-126, no volume_cb: output.gainL/R = the gains, per
        // channel (balance kept), applied by the gain stage.
        let mut rig = Rig::new([0x11, 0x22, 0x33, 0x44, 0x55, 0x08], "Test");
        rig.backend.hw_volume = false;
        let (mut s, mut o) = ([0u8; 1024], [0u8; 1024]);
        let mut pipeline = pcm_pipeline(&mut s, &mut o);
        let msg = rig.command(&mut pipeline, &audg_frame(1842, 900, 1), 10);
        assert_eq!(msg, Some(CydMessage::Volume(55)));
        assert_eq!((pipeline.gain_stage.volume_l_q16, pipeline.gain_stage.volume_r_q16), (1842, 900));
        assert_eq!((rig.coord.volume.gain_left, rig.coord.volume.gain_right), (1842, 900));
        assert_eq!(pipeline.requested_volume_q16(), (1842, 900));
        assert_eq!(rig.backend.hw_vol, (0, 0));
        // Zero is silence through the software gain (C: output.gainL = 0).
        rig.command(&mut pipeline, &audg_frame(0, 0, 1), 20);
        assert_eq!((pipeline.gain_stage.volume_l_q16, pipeline.gain_stage.volume_r_q16), (0, 0));
    }

    #[test]
    fn test_audg_hardware_volume_policy() {
        let mut rig = Rig::new([0x11, 0x22, 0x33, 0x44, 0x55, 0x04], "Test");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // Unequal channels: the louder one sets both TAS channels (Task 7), and the software
        // gain keeps the balance as the per-channel ratio to it (Task 14 fix round: C honors
        // independent L/R gains, output.c:54-55; Balance=1, embedded.h:47).
        // 1842 = LMS 51% slider (-31 dB) -> 55% -> 0x5D (-22.5 dB)
        let msg = rig.command(&mut pipeline, &audg_frame(1842, 900, 1), 10);
        assert_eq!(msg, Some(CydMessage::Volume(55)));
        assert_eq!(rig.backend.hw_vol, (0x5D, 0x5D));
        assert_eq!(pipeline.gain_stage.volume_l_q16, 65536);
        assert_eq!(pipeline.gain_stage.volume_r_q16, 900 * 65536 / 1842);
        // Equal channels: exactly unity in software, as Task 7.
        rig.command(&mut pipeline, &audg_frame(1842, 1842, 1), 15);
        assert_eq!((pipeline.gain_stage.volume_l_q16, pipeline.gain_stage.volume_r_q16), (65536, 65536));
        rig.command(&mut pipeline, &audg_frame(1842, 900, 1), 16);
        // The requested volume (C equalizer_set_volume's input, Task 16) is LMS's, per channel.
        assert_eq!(pipeline.requested_volume_q16(), (1842, 900));

        // Zero gain: amplifier muted, software gain 0
        let msg = rig.command(&mut pipeline, &audg_frame(0, 0, 1), 20);
        assert_eq!(msg, Some(CydMessage::Volume(0)));
        assert_eq!(rig.backend.hw_vol, (0xFF, 0xFF));
        assert_eq!((rig.coord.volume.gain_left, rig.coord.volume.gain_right), (0, 0));
    }

    #[test]
    fn test_backend_receives_rate_and_keeps_going_on_unsupported_rate() {
        let mut rig = Rig::new([0x11, 0x22, 0x33, 0x44, 0x55, 0x01], "Test");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // setd with a 4-byte sample rate: 48 kHz reaches the clock
        rig.command(&mut pipeline, &server_frame(b"setd", &48_000u32.to_be_bytes()), 10);
        assert_eq!(rig.coord.current_sample_rate, 48_000);
        assert_eq!(pipeline.sample_rate, 48_000);
        assert_eq!(rig.backend.rate, 48_000);

        // A rate the clock refuses keeps the previous clock; the coordinator state still
        // follows the server (no STMn yet).
        rig.command(&mut pipeline, &server_frame(b"setd", &12_345u32.to_be_bytes()), 20);
        assert_eq!(rig.coord.current_sample_rate, 12_345);
        assert_eq!(rig.backend.rate, 48_000);
        assert!(rig.sent.is_empty());
    }

    #[test]
    fn test_pending_connect_resumes_on_later_tick() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x02];
        let mut rig =
            Rig::with_source(mac, "TestPlayer", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        rig.src.hold_connect = true;

        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        let start = strm_frame(b's', true, 0, 0, 9000, [0, 0, 0, 0], &[]);
        rig.command(&mut pipeline, &start, 10);
        assert!(rig.coord.stream_active());
        assert_eq!(rig.lines(), ["STAT:STMf"], "STMc waits for the response");
        // server_ip 0.0.0.0 streams from the LMS address
        assert_eq!(rig.src.last_request, DEFAULT_HTTP_REQUEST);

        rig.tick(&mut pipeline, 20).unwrap();
        assert_eq!(rig.src.inner.connects, 0);

        rig.src.hold_connect = false;
        rig.tick(&mut pipeline, 30).unwrap();
        assert_eq!(rig.lines()[..3], ["STAT:STMf", "STAT:STMc", "RESP"]);
        assert_eq!(rig.src.inner.connects, 1);
        assert_eq!(rig.src.inner.last_addr, Some(NetAddr { ip: [127, 0, 0, 1], port: 9000 }));
        for _ in 0..10 {
            rig.tick(&mut pipeline, 40).unwrap();
        }
        assert_eq!(pipeline.frames_played, 2);
        assert!(rig.lines().contains(&"STAT:STMs".to_string()));
    }

    // -----------------------------------------------------------------------------------------
    // Task 9: device path through the coordinator (pumps, decoder provider, codc, STMl, ...)
    // -----------------------------------------------------------------------------------------

    /// `strm_frame` with another format byte.
    fn strm_frame_fmt(format: u8, autostart: bool, server_port: u16) -> Vec<u8> {
        let mut f = strm_frame(b's', autostart, 0, 0, server_port, [127, 0, 0, 1], &[]);
        f[8 + 2] = format;
        f
    }

    #[test]
    fn test_device_pumps_play_audio_and_stms_is_reported_once() {
        let mut rig = Rig::with_source([1; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.coord.external_audio_pump = true;
        start_held(&mut rig, &mut pipeline, true, 0);
        // Bytes reach the stream ring; the tick starts the track, the device pumps decode and
        // output itself.
        pipeline.ingest_stream_data(TWO_FRAMES).unwrap();
        rig.tick(&mut pipeline, 15).unwrap();
        assert_eq!(rig.coord.state, PlayerState::Playing);
        rig.coord.pump_decode(&mut pipeline, 4);
        assert_eq!(rig.coord.pump_output(&mut pipeline, 16), 2);
        assert_eq!(pipeline.frames_played, 2);
        rig.tick(&mut pipeline, 20).unwrap();
        rig.tick(&mut pipeline, 30).unwrap();
        let stms = rig.lines().iter().filter(|l| *l == "STAT:STMs").count();
        assert_eq!(stms, 1, "{:?}", rig.lines());
        // Output stays gated by the play state and the DAC enable.
        rig.coord.dac_enabled = false;
        pipeline.ingest_stream_data(TWO_FRAMES).unwrap();
        rig.coord.pump_decode(&mut pipeline, 4);
        assert_eq!(rig.coord.pump_output(&mut pipeline, 16), 0);
    }

    static TRUNCATED_TAIL: Segments = &[OK_HEAD, TWO_FRAMES, &[0x03, 0x00, 0xFD]];

    #[test]
    fn test_eof_with_undecodable_tail_still_ends_the_track() {
        // A stream ending in a partial frame used to leave the tail in the stream ring forever
        // (no STMd/STMu). The device firmware dropped it; the coordinator now does too.
        let mut rig = Rig::with_source([2; 6], "T", GatedSource::new(b"/stream.mp3", TRUNCATED_TAIL));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[]), 10);
        for t in 0..20 {
            rig.tick(&mut pipeline, 20 + t).unwrap();
        }
        assert_eq!(pipeline.frames_played, 2);
        assert_eq!(rig.coord.state, PlayerState::Stopped);
        assert_eq!(
            rig.lines(),
            ["STAT:STMf", "STAT:STMc", "RESP", "DSCO:0", "STAT:STMs", "STAT:STMd", "STAT:STMu"]
        );
    }

    #[test]
    fn test_pump_decode_drops_undecodable_tail_after_eof() {
        let mut rig = Rig::with_source([3; 6], "T", GatedSource::new(b"/stream.mp3", TRUNCATED_TAIL));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[]), 10);
        rig.src.hold_data = false;
        for _ in 0..10 {
            let _ = rig.coord.poll_stream(&mut rig.src, &mut pipeline);
        }
        assert!(rig.coord.stream_eof);
        rig.coord.evaluate_state(&mut pipeline);
        for _ in 0..4 {
            rig.coord.pump_decode(&mut pipeline, 4);
            rig.coord.pump_output(&mut pipeline, 16);
        }
        assert_eq!(pipeline.stream_buffer.available_read(), 0, "partial frame dropped");
    }

    #[test]
    fn test_codc_selects_the_decoder() {
        let mut rig = Rig::new([4; 6], "T");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &server_frame(b"codc", b"f"), 10);
        assert!(matches!(pipeline.decoder, ActiveDecoder::Flac(_)));
        assert_eq!(rig.coord.current_format, Some(AudioFormat::Flac));
        rig.command(&mut pipeline, &server_frame(b"codc", b"m"), 20);
        assert!(matches!(pipeline.decoder, ActiveDecoder::Mp3(_)));
        // PCM: format, sample size, rate, channels, endianness ('1' = little endian).
        rig.command(&mut pipeline, &server_frame(b"codc", b"p231\x01"), 30);
        match &pipeline.decoder {
            ActiveDecoder::Pcm(d) => {
                assert_eq!((d.bits_per_sample, d.channels, d.big_endian), (24, 1, false));
            }
            _ => panic!("PCM decoder expected"),
        }
        assert!(rig.lines().is_empty(), "codc has no reply");
    }

    #[test]
    fn test_unsupported_format_installs_pcm_muted_until_next_track() {
        let mut rig = Rig::new([5; 6], "T");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &strm_frame_fmt(b'm', true, 0), 10);
        assert!(matches!(pipeline.decoder, ActiveDecoder::Mp3(_)));
        // Opus is not decodable: never feed its bytes to the previous (MP3) decoder.
        rig.command(&mut pipeline, &strm_frame_fmt(b'u', true, 0), 20);
        assert!(matches!(pipeline.decoder, ActiveDecoder::Pcm(_)));
        assert!(pipeline.user_muted);
        pipeline.ingest_stream_data(TWO_FRAMES).unwrap();
        rig.tick(&mut pipeline, 30).unwrap();
        assert!(pipeline.sink.muted, "output stays muted");
        rig.command(&mut pipeline, &strm_frame_fmt(b'p', true, 0), 40);
        assert!(!pipeline.user_muted, "a supported track clears the mute");
    }

    #[test]
    fn test_mp3_decoder_comes_from_the_pipeline_slot_and_returns_to_it() {
        // Firmware builds have no heap MP3 decoder: bring-up parks the static one in the
        // pipeline's MP3 slot. The coordinator must use (and give back) that very decoder.
        let mut rig = Rig::new([6; 6], "T");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        let parked = Box::new(squeezelite_decoders::Mp3Decoder::new());
        let id: *const squeezelite_decoders::Mp3Decoder = &*parked;
        pipeline.mp3_spare = Some(parked);
        let current_mp3 = |p: &TestPipeline<'_>| match &p.decoder {
            ActiveDecoder::Mp3(d) => &**d as *const squeezelite_decoders::Mp3Decoder,
            _ => core::ptr::null(),
        };

        rig.command(&mut pipeline, &strm_frame_fmt(b'm', true, 0), 10);
        assert_eq!(current_mp3(&pipeline), id, "the parked decoder is used");
        assert!(pipeline.mp3_spare.is_none());

        rig.command(&mut pipeline, &strm_frame_fmt(b'f', true, 0), 20);
        assert!(matches!(pipeline.decoder, ActiveDecoder::Flac(_)));
        assert_eq!(pipeline.mp3_spare.as_deref().map(|d| d as *const _), Some(id), "parked again");

        rig.command(&mut pipeline, &strm_frame_fmt(b'm', true, 0), 30);
        rig.command(&mut pipeline, &strm_frame_fmt(b'm', true, 0), 40);
        assert_eq!(current_mp3(&pipeline), id, "MP3 after MP3 keeps the same decoder");
    }

    #[test]
    fn test_hello_sends_helo_and_setd_without_a_heartbeat() {
        let mut rig = Rig::new([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF], "Kitchen");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.coord.hello(&mut rig.lms, 12, false).unwrap();
        rig.drain_sent();
        assert_eq!(rig.lines(), ["HELO", "SETD"]);
        assert_eq!(&rig.sent[0].1[2..8], &[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        let caps = PlayerCoordinator::helo_capabilities(None).unwrap();
        assert!(rig.sent[0].1.ends_with(caps.as_bytes()));
        assert_eq!(&rig.sent[0].1[24..26], &[0, 0], "first connect: no reconnect bit");
        assert_eq!(&rig.sent[1].1[1..], b"Kitchen");
        assert_eq!(rig.coord.state, PlayerState::Connected);
        // C parity: no STAT after HELO until output runs or LMS asks (slimproto.c:890).
        rig.tick(&mut pipeline, 5_101).unwrap();
        assert_eq!(rig.lines(), ["HELO", "SETD"]);
    }

    #[test]
    fn test_helo_capabilities_are_honest() {
        // Field-by-field C comparison: squeezelite_proto::build_capabilities.
        let caps = PlayerCoordinator::helo_capabilities(None).unwrap();
        assert_eq!(
            caps.as_str(),
            concat!(
                "Model=squeezeesp32,AccuratePlayPoints=1,HasPolarityInversion=1,Balance=1,Depth=32,Firmware=v",
                env!("CARGO_PKG_VERSION"),
                ",ModelName=SqueezeESP32,MaxSampleRate=192000,flc,pcm,wav,aif,mp3"
            )
        );
        assert!(caps.contains(&format!("MaxSampleRate={}", crate::clock_table::max_verified_rate())));
    }

    #[test]
    fn test_resp_waits_for_room_instead_of_being_dropped() {
        let mut rig = Rig::with_source([7; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.src.hold_data = true;
        rig.lms.inject_frame(&strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[])).unwrap();
        // Fill the send queue so everything after STMf + STMc would block.
        for _ in 0..LOOPBACK_QUEUE_DEPTH - 2 {
            rig.lms.send(b"FILL\0\0\0\0").unwrap();
        }
        let _ = rig.coord.tick(&mut rig.lms, &mut rig.src, &mut rig.backend, &mut pipeline, 10);
        assert!(rig.coord.pending_resp.is_some(), "RESP kept for the next tick");
        rig.drain_sent();
        rig.sent.clear();
        rig.tick(&mut pipeline, 20).unwrap();
        assert_eq!(rig.lines()[0], "RESP");
    }

    #[test]
    fn test_non_autostart_stream_sends_stml_after_resp() {
        let mut rig = Rig::with_source([8; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &strm_frame(b's', false, 1, 0, 9000, [127, 0, 0, 1], &[]), 10);
        for t in 0..4 {
            rig.tick(&mut pipeline, 20 + t).unwrap();
        }
        // The 8-byte track ends (DSCO(0)) below the 1 KB threshold: DISCONNECT_OK starts it
        // like the threshold would, so STMl follows (slimproto.c:825-831).
        let lines = rig.lines();
        assert_eq!(lines, ["STAT:STMf", "STAT:STMc", "RESP", "DSCO:0", "STAT:STMl"], "{lines:?}");
    }

    #[test]
    fn test_cont_wait_holds_the_body_then_releases_it_with_metaint_and_stml() {
        static ICY_BODY: Segments = &[OK_HEAD, b"\x01\x00\xFF\x00\x01Hi\0\0\0\0\0\0\0\0\0\0\0\0\0\0\x02\x00\xFE\x00"];
        let mut rig = Rig::with_source([9; 6], "T", GatedSource::new(b"/stream.mp3", ICY_BODY));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        let mut start = strm_frame(b's', false, 0, 0, 9000, [127, 0, 0, 1], &[]);
        start[8 + 11] = squeezelite_proto::STRM_FLAG_CONT_WAIT; // flags
        rig.command(&mut pipeline, &start, 10);
        assert!(rig.coord.awaiting_cont);
        for t in 0..4 {
            rig.tick(&mut pipeline, 20 + t).unwrap();
        }
        assert_eq!(pipeline.stream_buffer.available_read(), 0, "no body before cont");
        assert_eq!(rig.lines(), ["STAT:STMf", "STAT:STMc", "RESP"]);
        rig.command(&mut pipeline, &server_frame(b"cont", &4u32.to_be_bytes()), 30);
        for t in 0..4 {
            rig.tick(&mut pipeline, 40 + t).unwrap();
        }
        assert_eq!(rig.coord.stream_bytes_received, 8, "ICY block stripped from the start");
        assert!(rig.lines().contains(&"STAT:STMl".to_string()), "{:?}", rig.lines());
        assert!(rig.lines().contains(&"META".to_string()));
    }

    #[test]
    fn test_track_start_powers_the_amplifier_when_the_dac_is_enabled() {
        let mut rig = Rig::new([10; 6], "T");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &strm_frame_fmt(b'p', true, 0), 10);
        assert!(rig.backend.powered);
        rig.command(&mut pipeline, &server_frame(b"aude", &[1, 0]), 20);
        rig.command(&mut pipeline, &strm_frame_fmt(b'p', true, 0), 30);
        assert!(!rig.backend.powered, "DAC disabled: track start does not power the amp");
    }

    #[test]
    fn test_external_playback_reports_nothing_to_lms() {
        let mut rig = Rig::with_source([11; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        // An LMS track plays to the end (STMd ... STMu leave per-track flags behind).
        rig.command(&mut pipeline, &strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[]), 10);
        for t in 0..20 {
            rig.tick(&mut pipeline, 20 + t).unwrap();
        }
        assert_eq!(rig.coord.state, PlayerState::Stopped);
        rig.sent.clear();
        // Bluetooth takes over the pipeline.
        rig.coord.begin_external_playback();
        pipeline.flush();
        pipeline.ingest_stream_data(TWO_FRAMES).unwrap();
        for t in 0..10 {
            rig.coord.pump_decode(&mut pipeline, 4);
            rig.coord.pump_output(&mut pipeline, 16);
            rig.tick(&mut pipeline, 100 + t).unwrap();
        }
        assert_eq!(rig.coord.state, PlayerState::Playing, "no LMS end-of-track logic");
        assert!(rig.lines().iter().all(|l| l == "STAT:STMt"), "{:?}", rig.lines());
        rig.coord.end_external_playback();
        assert_eq!(rig.coord.state, PlayerState::Stopped);
    }

    #[test]
    fn test_external_frame_buffer_receives_frames_and_bounds_their_size() {
        // The firmware hands the coordinator a static frame buffer (off the task stack).
        let mut rig = Rig::new([12; 6], "T");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.coord.set_frame_buffer(Box::leak(Box::new([0u8; 64])));
        rig.command(&mut pipeline, &server_frame(b"setd", b"\x00Den"), 10);
        assert_eq!(rig.coord.player_name.as_str(), "Den");
        // A frame larger than the buffer is dropped by the transport (Io) and skipped.
        let long_name = [b'x'; 80];
        let mut payload = vec![0u8];
        payload.extend_from_slice(&long_name);
        rig.command(&mut pipeline, &server_frame(b"setd", &payload), 20);
        assert_eq!(rig.coord.player_name.as_str(), "Den");
        assert_eq!(rig.lines(), ["SETD"]);
    }

    #[test]
    fn test_external_audio_pump_keeps_decode_and_output_out_of_tick() {
        // Device: the pump phases own decode/output; tick must not play audio on its own,
        // but state transitions (Buffering -> Playing, STMs from pumped output) still happen.
        let mut rig = Rig::with_source([13; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        rig.coord.external_audio_pump = true;
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        start_held(&mut rig, &mut pipeline, true, 1);
        assert_eq!(rig.coord.state, PlayerState::Buffering);
        pipeline.ingest_stream_data(&[0u8; 1100]).unwrap();
        rig.tick(&mut pipeline, 20).unwrap();
        assert_eq!(rig.coord.state, PlayerState::Playing, "threshold reached in tick");
        assert_eq!(pipeline.frames_played, 0, "tick wrote nothing to the sink");
        assert_eq!(pipeline.sink.recorded_samples().len(), 0);
        assert_eq!(pipeline.stream_buffer.available_read(), 1100, "tick decoded nothing");
        rig.coord.pump_decode(&mut pipeline, 4);
        assert!(rig.coord.pump_output(&mut pipeline, 16) > 0);
        rig.tick(&mut pipeline, 30).unwrap();
        assert!(rig.lines().contains(&"STAT:STMs".to_string()));
    }

    #[test]
    fn test_setd_name_is_cut_at_the_first_nul() {
        let mut rig = Rig::new([14; 6], "T");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &server_frame(b"setd", b"\x00Kitchen\x00\x00junk"), 10);
        assert_eq!(rig.coord.player_name.as_str(), "Kitchen");
        assert_eq!(&rig.sent[0].1[1..], b"Kitchen", "no NUL echoed back");
        // A name that is empty before its NUL keeps the current name.
        rig.command(&mut pipeline, &server_frame(b"setd", b"\x00\x00"), 20);
        assert_eq!(rig.coord.player_name.as_str(), "Kitchen");
    }

    #[test]
    fn test_setd_name_change_persists_trimmed_name_through_config_handle() {
        use crate::config_handle::ConfigHandle;
        let config: &'static ConfigHandle =
            Box::leak(Box::new(ConfigHandle::new(squeezelite_config::AudioConfig::default())));
        let mut rig = Rig::new([16; 6], "T");
        rig.coord.config = Some(config);
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // id-0 set: the NUL-trimmed name reaches the handle and marks it dirty.
        rig.command(&mut pipeline, &server_frame(b"setd", b"\x00Kitchen\x00\x00junk"), 10);
        assert_eq!(config.read(|c| c.player_name.clone()).as_str(), "Kitchen");
        assert!(config.is_dirty());
        assert!(config.snapshot_if_dirty().is_some());
        config.mark_clean();

        // id-0 query: replies, persists nothing.
        rig.command(&mut pipeline, &server_frame(b"setd", &[0x00]), 20);
        assert!(!config.is_dirty());
        // The same name again is not a change.
        rig.command(&mut pipeline, &server_frame(b"setd", b"\x00Kitchen"), 30);
        assert!(!config.is_dirty());

        // The 4-byte (`SetDevice`) name form persists too.
        rig.command(&mut pipeline, &server_frame(b"setd", &[0x00, b'B', b'o', b'b']), 40);
        assert_eq!(rig.coord.player_name.as_str(), "Bob");
        assert_eq!(config.read(|c| c.player_name.clone()).as_str(), "Bob");
        assert!(config.is_dirty());
    }

    #[test]
    fn test_codc_reselecting_flac_flushes_the_decoder() {
        let mut rig = Rig::new([15; 6], "T");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &server_frame(b"codc", b"f"), 10);
        let id = match &mut pipeline.decoder {
            ActiveDecoder::Flac(d) => {
                d.pending_pcm.extend_from_slice(&[1, 2, 3, 4]);
                d.stream_buf.extend_from_slice(b"stale");
                &**d as *const squeezelite_decoders::FlacDecoder
            }
            _ => panic!("FLAC expected"),
        };
        rig.command(&mut pipeline, &server_frame(b"codc", b"f"), 20);
        match &pipeline.decoder {
            ActiveDecoder::Flac(d) => {
                assert_eq!(&**d as *const _, id, "same decoder kept");
                assert!(d.pending_pcm.is_empty() && d.stream_buf.is_empty(), "and flushed");
            }
            _ => panic!("FLAC expected"),
        }
    }

    /// Transport whose send path refuses frames over `max` bytes (like a 2 KB socket buffer).
    struct SmallTx {
        inner: LoopbackTransport,
        max: usize,
    }

    impl SlimTransport for SmallTx {
        fn poll_frame(&mut self, buf: &mut [u8]) -> Result<Option<usize>, TransportError> {
            self.inner.poll_frame(buf)
        }
        fn send(&mut self, frame: &[u8]) -> Result<(), TransportError> {
            if frame.len() > self.max {
                return Err(TransportError::Io);
            }
            self.inner.send(frame)
        }
        fn connected(&self) -> bool {
            true
        }
    }

    #[test]
    fn test_oversized_resp_is_sent_truncated_at_a_line_boundary() {
        let mut head = b"HTTP/1.0 200 OK\r\n".to_vec();
        for i in 0..120 {
            head.extend_from_slice(format!("X-Pad-{i:03}: {}E\r\n", "v".repeat(20)).as_bytes());
        }
        head.extend_from_slice(b"\r\n");
        assert!(head.len() > 3000);
        let mut coord = PlayerCoordinator::new([16; 6], "T");
        coord.last_heartbeat_ms = 0;
        coord.pending_resp = Some(head.clone());
        let mut tx = SmallTx { inner: LoopbackTransport::new(), max: 2048 };
        let mut stream_storage = [0u8; 256];
        let mut output_storage = [0u8; 256];
        let pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        coord.check_heartbeat(&mut tx, &pipeline, 10).unwrap();
        assert!(coord.pending_resp.is_none());
        let frame = tx.inner.pop_sent().expect("RESP sent");
        assert_eq!(&frame[..4], b"RESP");
        let payload = &frame[8..];
        assert!(frame.len() <= 2048);
        assert!(payload.ends_with(b"E\r\n\r\n"), "cut after a whole header line, blank line added");
        assert!(head.starts_with(&payload[..payload.len() - 2]));
    }

    #[test]
    fn test_tick_reports_disconnect_but_keeps_playing() {
        let mut rig = Rig::with_source(
            [0x11, 0x22, 0x33, 0x44, 0x55, 0x03],
            "Test",
            GatedSource::new(b"/stream.mp3", SHORT_STREAM),
        );
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        start_held(&mut rig, &mut pipeline, true, 0);
        pipeline.ingest_stream_data(TWO_FRAMES).unwrap();
        rig.lms.disconnect();

        assert_eq!(rig.tick(&mut pipeline, 20), Err(TransportError::Disconnected));
        assert_eq!(rig.coord.last_tick_frames(), 2);
        // The deferred STMs stays pending for the reconnected link
        assert!(rig.coord.pending_track_started);
    }

    // -----------------------------------------------------------------------------------------
    // Task 11: C-parity protocol sequencing (slimproto.c status loop, process_strm, process_serv)
    // -----------------------------------------------------------------------------------------

    /// `strm` frame with an explicit format byte, autostart byte and flags.
    #[allow(clippy::too_many_arguments)]
    fn strm_full(
        command: u8,
        autostart: u8,
        format: u8,
        threshold_kb: u8,
        replay_gain: u32,
        server_port: u16,
        server_ip: [u8; 4],
    ) -> Vec<u8> {
        let mut f = strm_frame(command, false, threshold_kb, replay_gain, server_port, server_ip, &[]);
        f[8 + 1] = autostart;
        f[8 + 2] = format;
        f
    }

    /// Starts an autostart PCM track on `/stream.mp3` whose body is held back, and runs ticks
    /// until the stream is connected. The caller feeds the stream ring directly.
    fn start_held(rig: &mut Rig, pipeline: &mut TestPipeline<'_>, autostart: bool, threshold_kb: u8) {
        rig.src.hold_data = true;
        rig.command(pipeline, &strm_frame(b's', autostart, threshold_kb, 0, 9000, [127, 0, 0, 1], &[]), 10);
        assert!(rig.coord.stream_active());
    }

    /// [`MockBackend`] that refuses one sample rate, like a clock table without it.
    struct RefusingBackend {
        inner: MockBackend,
        refuse: u32,
    }

    impl AudioBackend for RefusingBackend {
        fn set_sample_rate(&mut self, hz: u32) -> Result<(), crate::io_traits::ClockError> {
            if hz == self.refuse {
                return Err(crate::io_traits::ClockError::UnsupportedRate(hz));
            }
            self.inner.set_sample_rate(hz)
        }
        fn set_hw_volume(&mut self, left: u8, right: u8) {
            self.inner.set_hw_volume(left, right);
        }
        fn has_hw_volume(&self) -> bool {
            self.inner.has_hw_volume()
        }
        fn set_mute(&mut self, mute: bool) {
            self.inner.set_mute(mute);
        }
        fn set_power(&mut self, on: bool) {
            self.inner.set_power(on);
        }
        fn dma_outstanding_frames(&self) -> u32 {
            self.inner.dma_outstanding_frames()
        }
    }

    fn drain_cyd(coord: &mut PlayerCoordinator) -> Vec<CydMessage> {
        core::iter::from_fn(|| coord.take_cyd_message()).collect()
    }

    #[test]
    fn test_connect_failure_sends_dsco_unreachable_once_no_stmn() {
        // C parity: STMf first (slimproto.c:449), STMc after stream_sock returns even when the
        // connect failed (slimproto.c:473), then DSCO(UNREACHABLE) once from the status loop
        // (stream.c:684-688, slimproto.c:804-808 + 910). No STMn: that is DECODE_ERROR only.
        let mut rig = Rig::new([0x21; 6], "T");
        rig.src.inner.fail_connect = true;
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        let msg = rig.command(&mut pipeline, &strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[]), 10);
        for t in 0..5 {
            rig.tick(&mut pipeline, 2_000 + t * 1_000).unwrap();
        }
        assert_eq!(rig.lines(), ["STAT:STMf", "STAT:STMc", "DSCO:3"]);
        assert_eq!(rig.coord.state, PlayerState::Stopped);
        assert_eq!(msg, Some(CydMessage::PlaybackState(PlayState::Stopped)));
        assert!(drain_cyd(&mut rig.coord).is_empty(), "only one CYD update");
    }

    #[test]
    fn test_port_zero_is_a_connect_failure_not_a_buffering_stall() {
        // C uses the strm port as is (slimproto.c:442, stream.c:677): port 0 never connects.
        // The Rust player used to sit in Buffering forever with threshold > 0.
        let mut rig = Rig::new([0x22; 6], "T");
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &strm_frame(b's', true, 1, 0, 0, [0, 0, 0, 0], &[]), 10);
        rig.tick(&mut pipeline, 20).unwrap();
        assert_eq!(rig.lines(), ["STAT:STMf", "STAT:STMc", "DSCO:3"]);
        assert_eq!(rig.coord.state, PlayerState::Stopped);
        assert_eq!(rig.src.inner.connects, 0);
    }

    #[test]
    fn test_unknown_codec_opens_no_decoder_and_sends_no_stmn() {
        // C parity: codec_open() for a format with no codec logs "codec not found" and leaves
        // decode STOPPED (decode.c:286-317): no DECODE_ERROR, so no STMn, and the track never
        // starts (slimproto.c:825 needs DECODE_READY). The stream itself still connects.
        let mut rig = Rig::with_source([0x23; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &strm_full(b's', b'1', b'z', 0, 0, 9000, [127, 0, 0, 1]), 10);
        for t in 0..10 {
            rig.tick(&mut pipeline, 20 + t).unwrap();
        }
        assert!(!rig.lines().contains(&"STAT:STMn".to_string()), "{:?}", rig.lines());
        assert_eq!(rig.lines()[..3], ["STAT:STMf", "STAT:STMc", "RESP"]);
        assert_eq!(rig.coord.state, PlayerState::Buffering, "never starts");
        assert_eq!(pipeline.frames_played, 0);
        assert!(pipeline.user_muted, "output muted, nothing played");
    }

    #[test]
    fn test_unsupported_rate_sends_stmn() {
        // The clock refuses the PCM rate. C has no clean equivalent (it resamples, or logs the
        // i2s_set_sample_rates failure and plays on, output_i2s.c:638-641); the accepted
        // mapping is DECODE_ERROR: STMn from the status loop (slimproto.c:840), no stream.
        let mut rig = Rig::with_source([0x24; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut backend = RefusingBackend { inner: MockBackend::new(), refuse: 44_100 };
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.lms.inject_frame(&strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[])).unwrap();
        rig.coord.tick(&mut rig.lms, &mut rig.src, &mut backend, &mut pipeline, 10).unwrap();
        rig.drain_sent();
        assert_eq!(rig.lines(), ["STAT:STMf", "STAT:STMn"]);
        assert_eq!(rig.coord.state, PlayerState::Stopped);
        assert!(backend.inner.muted, "muted, not playing");
        assert_eq!(rig.src.inner.connects, 0, "no stream for a track that cannot play");
        assert_eq!(drain_cyd(&mut rig.coord), [CydMessage::PlaybackState(PlayState::Stopped)]);
    }

    #[test]
    fn test_decoder_error_sends_stmn_and_closes_the_stream() {
        // DECODE_ERROR (here: a sample format the decoder refuses) -> STMn, the stream is
        // disconnected without a DSCO (slimproto.c:838-845, stream_disconnect sets STOPPED).
        let mut rig = Rig::with_source([0x25; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        start_held(&mut rig, &mut pipeline, true, 0);
        pipeline.set_decoder(ActiveDecoder::Pcm(PcmDecoder::new(12, 2, true)));
        pipeline.ingest_stream_data(TWO_FRAMES).unwrap();
        for t in 0..4 {
            rig.tick(&mut pipeline, 20 + t).unwrap();
        }
        let lines = rig.lines();
        assert_eq!(lines.iter().filter(|l| *l == "STAT:STMn").count(), 1, "{lines:?}");
        assert!(!lines.iter().any(|l| l.starts_with("DSCO")), "{lines:?}");
        assert!(!rig.coord.stream_active());
        assert_eq!(rig.coord.state, PlayerState::Stopped);
    }

    #[test]
    fn test_output_underrun_emits_stmo_once_per_track() {
        // C parity: STMo once per track (sentSTMo, reset at strm s, slimproto.c:474) when output
        // runs dry while the stream is still STREAMING_HTTP (slimproto.c:881-884).
        let mut rig = Rig::with_source([0x26; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        start_held(&mut rig, &mut pipeline, true, 0);
        for round in 0..3u32 {
            pipeline.ingest_stream_data(TWO_FRAMES).unwrap();
            for t in 0..4 {
                rig.tick(&mut pipeline, 20 + round * 5_000 + t).unwrap();
            }
        }
        assert_eq!(rig.coord.state, PlayerState::Playing);
        let stmo = rig.lines().iter().filter(|l| *l == "STAT:STMo").count();
        assert_eq!(stmo, 1, "{:?}", rig.lines());

        // The next track may report its own underrun.
        rig.sent.clear();
        start_held(&mut rig, &mut pipeline, true, 0);
        pipeline.ingest_stream_data(TWO_FRAMES).unwrap();
        for t in 0..4 {
            rig.tick(&mut pipeline, 30_000 + t).unwrap();
        }
        assert_eq!(rig.lines().iter().filter(|l| *l == "STAT:STMo").count(), 1, "{:?}", rig.lines());
    }

    #[test]
    fn test_pause_interval_resumes_after_ms() {
        // C parity: strm p with an interval is OUTPUT_PAUSE_FRAMES: silence for the interval,
        // then RUNNING again by itself, and no STMp (slimproto.c:395-410, output.c:86-96).
        let mut rig = Rig::with_source([0x27; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        start_held(&mut rig, &mut pipeline, true, 0);
        pipeline.ingest_stream_data(TWO_FRAMES).unwrap();
        rig.tick(&mut pipeline, 20).unwrap();
        assert_eq!(rig.coord.state, PlayerState::Playing);
        rig.sent.clear();
        let _ = drain_cyd(&mut rig.coord);

        rig.command(&mut pipeline, &strm_frame(b'p', false, 0, 250, 0, [0, 0, 0, 0], &[]), 1_000);
        assert_eq!(rig.coord.state, PlayerState::Paused);
        assert_eq!(rig.coord.pause_until_ms, Some(1_250));
        assert!(!rig.backend.muted, "silence, not a mute");
        rig.tick(&mut pipeline, 1_249).unwrap();
        assert_eq!(rig.coord.state, PlayerState::Paused);
        rig.tick(&mut pipeline, 1_250).unwrap();
        assert_eq!(rig.coord.state, PlayerState::Playing, "resumed without a server unpause");
        let lines = rig.lines();
        assert!(!lines.iter().any(|l| l == "STAT:STMp" || l == "STAT:STMr"), "{lines:?}");
        assert!(drain_cyd(&mut rig.coord).is_empty());
    }

    #[test]
    fn test_unpause_at_jiffies() {
        // C parity: strm u answers STMr and, with jiffies, starts output at that time
        // (slimproto.c:423-434, output.c:99-109); more than 10 s ahead starts at once.
        let mut rig = Rig::with_source([0x28; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        start_held(&mut rig, &mut pipeline, true, 0);
        pipeline.ingest_stream_data(TWO_FRAMES).unwrap();
        rig.tick(&mut pipeline, 20).unwrap();
        rig.command(&mut pipeline, &simple_strm(b'p'), 1_000);
        assert!(rig.backend.muted);
        rig.sent.clear();

        rig.command(&mut pipeline, &strm_frame(b'u', false, 0, 2_000, 0, [0, 0, 0, 0], &[]), 1_500);
        assert_eq!(rig.lines(), ["STAT:STMr"]);
        assert_eq!(rig.coord.state, PlayerState::Paused);
        assert_eq!(rig.coord.unpause_at_jiffies, Some(2_000));
        rig.tick(&mut pipeline, 1_999).unwrap();
        assert_eq!(rig.coord.state, PlayerState::Paused);
        assert!(rig.backend.muted);
        rig.tick(&mut pipeline, 2_000).unwrap();
        assert_eq!(rig.coord.state, PlayerState::Playing);
        assert!(!rig.backend.muted);

        // jiffies more than 10 s ahead: start now.
        rig.command(&mut pipeline, &simple_strm(b'p'), 3_000);
        rig.command(&mut pipeline, &strm_frame(b'u', false, 0, 3_000 + 10_001, 0, [0, 0, 0, 0], &[]), 3_000);
        assert_eq!(rig.coord.state, PlayerState::Playing);
        assert_eq!(rig.coord.unpause_at_jiffies, None);
    }

    #[test]
    fn test_threshold_gates_autostart() {
        // C parity: the stream leaves STREAMING_BUFFERING only once more than `threshold`
        // bytes arrived (stream.c:527); output starts there (slimproto.c:825-834).
        let mut rig = Rig::with_source([0x29; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = vec![0u8; 32 * 1024];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        start_held(&mut rig, &mut pipeline, true, 16);
        assert_eq!(rig.coord.buffer_threshold_bytes, 16_384);
        pipeline.ingest_stream_data(&[0u8; 16_384]).unwrap();
        rig.tick(&mut pipeline, 20).unwrap();
        assert_eq!(rig.coord.state, PlayerState::Buffering, "exactly threshold: not yet");
        assert_eq!(pipeline.stream_buffer.available_read(), 16_384, "no decode");
        pipeline.ingest_stream_data(&[0u8; 4]).unwrap();
        rig.tick(&mut pipeline, 30).unwrap();
        assert_eq!(rig.coord.state, PlayerState::Playing);
        assert!(pipeline.stream_buffer.available_read() < 16_388, "decoding");
    }

    #[test]
    fn test_stml_waits_for_threshold_and_ready_decoder() {
        // C parity: STMl only with autostart 0, the stream past the threshold and decode READY
        // (slimproto.c:825-831); not when the response is accepted. No STMp at strm s.
        let mut rig = Rig::with_source([0x2A; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 4096];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        start_held(&mut rig, &mut pipeline, false, 1);
        for t in 0..3 {
            rig.tick(&mut pipeline, 20 + t).unwrap();
        }
        assert_eq!(rig.lines(), ["STAT:STMf", "STAT:STMc", "RESP"]);
        pipeline.ingest_stream_data(&[0u8; 1025]).unwrap();
        rig.tick(&mut pipeline, 30).unwrap();
        rig.tick(&mut pipeline, 31).unwrap();
        assert_eq!(rig.lines(), ["STAT:STMf", "STAT:STMc", "RESP", "STAT:STMl"]);
        assert_eq!(rig.coord.state, PlayerState::Paused);
    }

    #[test]
    fn test_eof_sends_dsco_at_socket_close_before_stmd_and_stmu() {
        // C parity: DSCO(0) when the stream socket hits EOF (stream.c:508, slimproto.c:910),
        // not after the output drained; STMd on DECODE_COMPLETE, STMu once output ran dry.
        let mut rig = Rig::with_source([0x2B; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[]), 10);
        for t in 0..10 {
            rig.tick(&mut pipeline, 20 + t).unwrap();
        }
        assert_eq!(
            rig.lines(),
            ["STAT:STMf", "STAT:STMc", "RESP", "DSCO:0", "STAT:STMs", "STAT:STMd", "STAT:STMu"]
        );
        // STMu is once per track (sentSTMu): an unpause after the end does not repeat it.
        rig.sent.clear();
        rig.command(&mut pipeline, &simple_strm(b'u'), 40);
        rig.tick(&mut pipeline, 41).unwrap();
        assert_eq!(rig.lines(), ["STAT:STMr"]);
    }

    #[test]
    fn test_heartbeat_only_while_output_running() {
        // C parity: the 1 s STMt heartbeat only while output is RUNNING (slimproto.c:890-893);
        // an idle player sends nothing unsolicited, but always answers strm t.
        let mut rig = Rig::with_source([0x2C; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        for t in 0..6 {
            rig.tick(&mut pipeline, t * 1_000).unwrap();
        }
        assert!(rig.lines().is_empty(), "idle: {:?}", rig.lines());
        rig.command(&mut pipeline, &simple_strm(b't'), 6_000);
        assert_eq!(rig.lines(), ["STAT:STMt"]);
        rig.sent.clear();

        start_held(&mut rig, &mut pipeline, true, 0);
        pipeline.ingest_stream_data(&[0u8; 1024]).unwrap();
        rig.coord.last_heartbeat_ms = 7_000;
        rig.tick(&mut pipeline, 7_500).unwrap();
        rig.tick(&mut pipeline, 8_001).unwrap();
        rig.tick(&mut pipeline, 8_500).unwrap();
        let beats = rig.lines().iter().filter(|l| *l == "STAT:STMt").count();
        assert_eq!(beats, 1, "{:?}", rig.lines());
        // Paused output: no heartbeat.
        rig.command(&mut pipeline, &simple_strm(b'p'), 9_000);
        rig.sent.clear();
        rig.tick(&mut pipeline, 12_000).unwrap();
        assert!(rig.lines().is_empty(), "{:?}", rig.lines());
    }

    #[test]
    fn test_strm_q_sends_nothing_and_strm_f_only_reports_a_real_flush() {
        // C parity: strm q has no reply (slimproto.c:386-394); strm f answers STMf only when a
        // stream was disconnected or output flushed (slimproto.c:374-385).
        let mut rig = Rig::with_source([0x2D; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &simple_strm(b'f'), 10);
        assert!(rig.lines().is_empty(), "nothing to flush: {:?}", rig.lines());
        start_held(&mut rig, &mut pipeline, true, 0);
        rig.sent.clear();
        rig.command(&mut pipeline, &simple_strm(b'f'), 30);
        assert_eq!(rig.lines(), ["STAT:STMf"]);
        rig.sent.clear();
        start_held(&mut rig, &mut pipeline, true, 0);
        rig.sent.clear();
        let msg = rig.command(&mut pipeline, &simple_strm(b'q'), 50);
        assert!(rig.lines().is_empty(), "{:?}", rig.lines());
        assert_eq!(msg, Some(CydMessage::PlaybackState(PlayState::Stopped)));
        assert_eq!(rig.coord.state, PlayerState::Stopped);
        assert!(!rig.coord.stream_active());
    }

    #[test]
    fn test_strm_a_skips_ms_of_output_frames() {
        // C parity: skip_frames = interval * rate / 1000, consumed from the output buffer and
        // counted as played (slimproto.c:412-421, output.c:70-82). Output frames are 8 bytes.
        let mut rig = Rig::new([0x2E; 6], "T");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 4096];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        pipeline.output_buffer.write(&[0u8; 8 * 300]).unwrap();
        pipeline.sample_rate = 100_000; // 1 ms = 100 frames
        rig.command(&mut pipeline, &strm_frame(b'a', false, 0, 2, 0, [0, 0, 0, 0], &[]), 10);
        assert_eq!(pipeline.output_buffer.available_read(), 8 * 100);
        assert_eq!(pipeline.frames_played, 200);
        // Never more than the output holds.
        rig.command(&mut pipeline, &strm_frame(b'a', false, 0, 50, 0, [0, 0, 0, 0], &[]), 20);
        assert_eq!(pipeline.output_buffer.available_read(), 0);
        assert_eq!(pipeline.frames_played, 300);
        assert!(rig.lines().is_empty(), "no reply");
    }

    #[test]
    fn test_serv_reconnects_with_syncgroup_in_helo() {
        // C parity: process_serv keeps a 10-byte SyncgroupID for the next HELO only
        // (slimproto.c:633-656, 1171-1174); a serv without one clears it.
        let mut rig = Rig::new([0x2F; 6], "T");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        let mut payload = vec![10, 0, 0, 7];
        payload.extend_from_slice(b"0123456789");
        rig.command(&mut pipeline, &server_frame(b"serv", &payload), 10);
        assert_eq!(rig.coord.take_server_switch(), Some([10, 0, 0, 7]));
        assert_eq!(rig.coord.take_server_switch(), None);

        rig.coord.hello(&mut rig.lms, 12, false).unwrap();
        rig.coord.hello(&mut rig.lms, 12, true).unwrap();
        rig.drain_sent();
        let helos: Vec<&Vec<u8>> =
            rig.sent.iter().filter(|(op, _)| op == b"HELO").map(|(_, p)| p).collect();
        assert_eq!(helos.len(), 2);
        assert!(helos[0].ends_with(b",SyncgroupID=0123456789"));
        assert!(!helos[1].windows(11).any(|w| w == b"SyncgroupID"), "used once");
        assert_eq!(&helos[1][24..26], &0x4000u16.to_be_bytes(), "reconnect bit");

        // serv without a syncgroup clears a pending one.
        rig.command(&mut pipeline, &server_frame(b"serv", &payload), 20);
        rig.command(&mut pipeline, &server_frame(b"serv", &[10, 0, 0, 8]), 30);
        assert_eq!(rig.coord.take_server_switch(), Some([10, 0, 0, 8]));
        rig.sent.clear();
        rig.coord.hello(&mut rig.lms, 12, false).unwrap();
        rig.drain_sent();
        assert!(!rig.sent[0].1.windows(11).any(|w| w == b"SyncgroupID"));

        // SqueezeNetwork (0.0.0.1) needs DNS, which this firmware does not have: ignored.
        rig.command(&mut pipeline, &server_frame(b"serv", &[0, 0, 0, 1]), 40);
        assert_eq!(rig.coord.take_server_switch(), None);
    }

    #[test]
    fn test_icy_meta_forwarded_as_meta_and_resp_once() {
        static ICY: Segments = &[
            b"HTTP/1.0 200 OK\r\nicy-metaint: 4\r\n\r\n",
            b"\x01\x00\xFF\x00\x02StreamTitle='Daft Punk - Da Funk';\0\0\x02\x00\xFE\x00",
        ];
        let mut rig = Rig::with_source([0x30; 6], "T", GatedSource::new(b"/stream.mp3", ICY));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[]), 10);
        for t in 0..10 {
            rig.tick(&mut pipeline, 20 + t).unwrap();
        }
        let lines = rig.lines();
        assert_eq!(lines.iter().filter(|l| *l == "RESP").count(), 1, "{lines:?}");
        assert_eq!(lines.iter().filter(|l| *l == "META").count(), 1, "{lines:?}");
        let expected = CydMessage::TrackMetadata {
            artist: heapless::String::try_from("Daft Punk").unwrap(),
            title: heapless::String::try_from("Da Funk").unwrap(),
        };
        let cyd = drain_cyd(&mut rig.coord);
        assert_eq!(cyd.iter().filter(|m| matches!(m, CydMessage::TrackMetadata { .. })).count(), 1);
        assert!(cyd.contains(&expected), "{cyd:?}");
        assert!(rig.coord.sync_cyd_telemetry(&pipeline).contains(&expected));
    }

    #[test]
    fn test_cyd_queue_keeps_every_message_of_a_tick() {
        // A connect failure resolved in the same tick as an audg must not lose either update.
        let mut rig = Rig::new([0x31; 6], "T");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.src.hold_connect = true;
        rig.src.inner.fail_connect = true;
        rig.command(&mut pipeline, &strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[]), 10);
        let _ = drain_cyd(&mut rig.coord);
        rig.src.hold_connect = false;
        rig.lms.inject_frame(&audg_frame(65536, 65536, 1)).unwrap();
        rig.tick(&mut pipeline, 20).unwrap();
        assert_eq!(
            drain_cyd(&mut rig.coord),
            [CydMessage::PlaybackState(PlayState::Stopped), CydMessage::Volume(100)]
        );
    }

    // -----------------------------------------------------------------------------------------
    // Task 11 review fixes: C edge semantics
    // -----------------------------------------------------------------------------------------

    #[test]
    fn test_strm_a_ends_a_start_at_or_pause_frames_wait() {
        // C parity: strm a sets OUTPUT_SKIP_FRAMES over any START_AT / PAUSE_FRAMES wait, and
        // the skip ends in RUNNING (slimproto.c:417, output.c:82).
        for wait in *b"up" {
            let mut rig = Rig::with_source([0x40; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
            let mut stream_storage = [0u8; 2048];
            let mut output_storage = [0u8; 2048];
            let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
            start_held(&mut rig, &mut pipeline, true, 0);
            pipeline.ingest_stream_data(TWO_FRAMES).unwrap();
            rig.tick(&mut pipeline, 20).unwrap();
            assert_eq!(rig.coord.state, PlayerState::Playing);
            if wait == b'u' {
                rig.command(&mut pipeline, &simple_strm(b'p'), 1_000);
                rig.command(&mut pipeline, &strm_frame(b'u', false, 0, 5_000, 0, [0, 0, 0, 0], &[]), 1_000);
                assert_eq!(rig.coord.unpause_at_jiffies, Some(5_000));
            } else {
                rig.command(&mut pipeline, &strm_frame(b'p', false, 0, 4_000, 0, [0, 0, 0, 0], &[]), 1_000);
                assert_eq!(rig.coord.pause_until_ms, Some(5_000));
            }
            assert_eq!(rig.coord.state, PlayerState::Paused);
            rig.command(&mut pipeline, &strm_frame(b'a', false, 0, 10, 0, [0, 0, 0, 0], &[]), 1_100);
            assert_eq!(rig.coord.state, PlayerState::Playing, "after strm {}", wait as char);
            assert_eq!((rig.coord.pause_until_ms, rig.coord.unpause_at_jiffies), (None, None));
            assert!(!rig.backend.muted);
        }
    }

    #[test]
    fn test_strm_p_interval_applies_in_any_output_state() {
        // C parity: OUTPUT_PAUSE_FRAMES is set whatever the output state (slimproto.c:399-401)
        // and ends in RUNNING (output.c:88-90): a stopped output (strm p 0) plays again.
        let mut rig = Rig::with_source([0x41; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        start_held(&mut rig, &mut pipeline, true, 0);
        pipeline.ingest_stream_data(TWO_FRAMES).unwrap();
        rig.tick(&mut pipeline, 20).unwrap();
        rig.command(&mut pipeline, &simple_strm(b'p'), 1_000);
        assert_eq!(rig.coord.state, PlayerState::Paused);
        rig.command(&mut pipeline, &strm_frame(b'p', false, 0, 300, 0, [0, 0, 0, 0], &[]), 2_000);
        assert_eq!(rig.coord.pause_until_ms, Some(2_300));
        rig.tick(&mut pipeline, 2_299).unwrap();
        assert_eq!(rig.coord.state, PlayerState::Paused);
        rig.tick(&mut pipeline, 2_300).unwrap();
        assert_eq!(rig.coord.state, PlayerState::Playing);
        assert!(!rig.backend.muted);

        // While buffering, the track keeps buffering and plays once started, even with
        // autostart off (C output already RUNNING when the decoder starts).
        // Task 12 (C parity): this strm s arrives with the output RUNNING, which it never
        // touches (slimproto.c:436-489), so the player stays Playing; the threshold gates
        // the decoder (decode READY until slimproto.c:825-834), not the output.
        start_held(&mut rig, &mut pipeline, false, 1);
        rig.command(&mut pipeline, &strm_frame(b'p', false, 0, 100, 0, [0, 0, 0, 0], &[]), 3_000);
        rig.tick(&mut pipeline, 3_100).unwrap();
        assert_eq!(rig.coord.state, PlayerState::Playing);
        assert!(rig.coord.decode_ready && !rig.coord.decode_running, "threshold still gates the start");
        pipeline.ingest_stream_data(&[0u8; 1025]).unwrap();
        rig.tick(&mut pipeline, 3_200).unwrap();
        assert_eq!(rig.coord.state, PlayerState::Playing);
        assert!(rig.lines().contains(&"STAT:STMl".to_string()), "autostart 0 still sends STMl");
    }

    #[test]
    fn test_strm_u_jiffies_while_buffering_keeps_threshold_and_stmd() {
        // An unpause time that arrives before the threshold must not bypass it, and the
        // decoder must still report STMd (slimproto.c:825-845).
        let mut rig = Rig::with_source([0x42; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 4096];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        start_held(&mut rig, &mut pipeline, false, 1);
        rig.command(&mut pipeline, &strm_frame(b'u', false, 0, 500, 0, [0, 0, 0, 0], &[]), 100);
        assert_eq!(rig.coord.state, PlayerState::Buffering);
        rig.tick(&mut pipeline, 600).unwrap();
        assert_eq!(rig.coord.state, PlayerState::Buffering, "deadline passed, threshold not");
        assert_eq!(pipeline.frames_played, 0);
        // The body (8 bytes, below the threshold) and EOF arrive: DISCONNECT_OK starts it.
        rig.src.hold_data = false;
        for t in 0..10 {
            rig.tick(&mut pipeline, 700 + t).unwrap();
        }
        assert_eq!(pipeline.frames_played, 2);
        let lines = rig.lines();
        assert!(lines.contains(&"STAT:STMd".to_string()), "{lines:?}");
        assert!(lines.contains(&"STAT:STMu".to_string()), "{lines:?}");
    }

    #[test]
    fn test_new_track_or_stop_clears_the_icy_title() {
        static ICY: Segments = &[
            b"HTTP/1.0 200 OK\r\nicy-metaint: 4\r\n\r\n",
            b"\x01\x00\xFF\x00\x02StreamTitle='Daft Punk - Da Funk';\0\0\x02\x00\xFE\x00",
        ];
        let has_meta = |c: &PlayerCoordinator, p: &TestPipeline<'_>| {
            c.sync_cyd_telemetry(p).iter().any(|m| matches!(m, CydMessage::TrackMetadata { .. }))
        };
        for next in *b"sq" {
            let mut rig = Rig::with_source([0x43; 6], "T", GatedSource::new(b"/stream.mp3", ICY));
            let mut stream_storage = [0u8; 2048];
            let mut output_storage = [0u8; 2048];
            let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
            rig.command(&mut pipeline, &strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[]), 10);
            for t in 0..5 {
                rig.tick(&mut pipeline, 20 + t).unwrap();
            }
            assert!(has_meta(&rig.coord, &pipeline));
            if next == b's' {
                rig.src.hold_data = true;
                rig.command(&mut pipeline, &strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[]), 30);
            } else {
                rig.command(&mut pipeline, &simple_strm(b'q'), 30);
            }
            assert!(!has_meta(&rig.coord, &pipeline), "after strm {}", next as char);
        }
    }

    #[test]
    fn test_syncgroup_survives_a_failed_helo_send() {
        let mut rig = Rig::new([0x44; 6], "T");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        let mut payload = vec![10, 0, 0, 7];
        payload.extend_from_slice(b"0123456789");
        rig.command(&mut pipeline, &server_frame(b"serv", &payload), 10);
        let mut refusing = SmallTx { inner: LoopbackTransport::new(), max: 16 };
        assert_eq!(rig.coord.hello(&mut refusing, 12, false), Err(TransportError::Io));
        rig.coord.hello(&mut rig.lms, 12, false).unwrap();
        rig.drain_sent();
        assert!(rig.sent[0].1.ends_with(b",SyncgroupID=0123456789"), "kept for the HELO that went out");
    }

    #[test]
    fn test_clean_end_starts_a_track_only_on_the_pass_that_sees_it() {
        // C parity: the DISCONNECT_OK start condition holds on one status pass only
        // (slimproto.c:804-808, 825-827). A decoder that becomes ready later (codc after the
        // stream ended) never starts the track.
        let mut rig = Rig::with_source([0x45; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &strm_full(b's', b'1', b'z', 1, 0, 9000, [127, 0, 0, 1]), 10);
        for t in 0..5 {
            rig.tick(&mut pipeline, 20 + t).unwrap();
        }
        assert!(rig.coord.is_stream_eof());
        rig.command(&mut pipeline, &server_frame(b"codc", b"p1321"), 30);
        for t in 0..5 {
            rig.tick(&mut pipeline, 40 + t).unwrap();
        }
        assert_eq!(rig.coord.state, PlayerState::Buffering);
        assert_eq!(pipeline.frames_played, 0);
        assert!(!rig.lines().contains(&"STAT:STMs".to_string()), "{:?}", rig.lines());
    }

    #[test]
    fn test_setd_query_is_answered_only_with_a_name() {
        // C parity (slimproto.c:589-593): `if (strlen(player_name)) sendSETDName(...)`.
        let mut rig = Rig::new([0x46; 6], "");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &server_frame(b"setd", &[0x00]), 10);
        assert!(rig.lines().is_empty(), "{:?}", rig.lines());
        rig.command(&mut pipeline, &server_frame(b"setd", b"\x00Den"), 20);
        rig.command(&mut pipeline, &server_frame(b"setd", &[0x00]), 30);
        assert_eq!(rig.lines(), ["SETD", "SETD"]);
    }

    // ---------------------------------------------------------------------------------------
    // Task 12: gapless track boundaries (C output.track_start, output.c:126-166)
    // ---------------------------------------------------------------------------------------

    /// Sink recording every sample and the clock rate each write was made at.
    struct GaplessSink {
        samples: Vec<i32>,
        writes: Vec<(u32, usize)>,
        rate: u32,
        /// Frames accepted per write (a DMA queue with limited room).
        limit: usize,
    }

    impl AudioOutputSink for GaplessSink {
        type Error = squeezelite_audio_core::OutputError;

        fn write_frames(&mut self, frames: &[i32]) -> Result<usize, Self::Error> {
            let n = (frames.len() / 2).min(self.limit);
            self.samples.extend_from_slice(&frames[..n * 2]);
            self.writes.push((self.rate, n));
            Ok(n)
        }

        fn set_sample_rate(&mut self, rate_hz: u32) -> Result<(), Self::Error> {
            self.rate = rate_hz;
            Ok(())
        }

        fn set_mute(&mut self, _muted: bool) {}
    }

    type GaplessPipeline<'a> = StreamPipeline<'a, GaplessSink>;

    fn gapless_pipeline<'a>(stream: &'a mut [u8], output: &'a mut [u8]) -> GaplessPipeline<'a> {
        StreamPipeline::new(
            stream,
            output,
            ActiveDecoder::Pcm(PcmDecoder::new(16, 2, true)),
            GaplessSink { samples: Vec::new(), writes: Vec::new(), rate: 44_100, limit: usize::MAX },
            44_100,
        )
    }

    /// 16-bit big-endian stereo PCM with a per-track pattern: frame `i` is
    /// (track * 1000 + i, -(track * 1000 + i)).
    fn track_bytes(track: i16, frames: usize) -> &'static [u8] {
        let mut out = Vec::with_capacity(frames * 4);
        for i in 0..frames as i16 {
            let v = track * 1000 + i;
            out.extend_from_slice(&v.to_be_bytes());
            out.extend_from_slice(&(-v).to_be_bytes());
        }
        Box::leak(out.into_boxed_slice())
    }

    /// Every sample (left and right) a `track_bytes` stream decodes to.
    fn track_samples(track: i16, frames: usize) -> Vec<i32> {
        (0..frames as i16)
            .flat_map(|i| {
                let v = i32::from(track * 1000 + i);
                [v << 16, -v << 16]
            })
            .collect()
    }

    fn segments(body: &'static [u8]) -> Segments {
        Box::leak(vec![OK_HEAD, body].into_boxed_slice())
    }

    /// Rig serving `/t1`, `/t2`, ... with the given track bodies.
    fn gapless_rig(tracks: &[&'static [u8]]) -> Rig {
        const PATHS: [&[u8]; 4] = [b"/t1", b"/t2", b"/t3", b"/t4"];
        let mut src = GatedSource::new(PATHS[0], segments(tracks[0]));
        for (path, body) in PATHS[1..].iter().zip(&tracks[1..]) {
            assert!(src.inner.add_route(path, segments(body)));
        }
        Rig::with_source([0x11, 0x22, 0x33, 0x44, 0x55, 0x66], "Gapless", src)
    }

    /// `strm s` for PCM 16-bit stereo big-endian at `rate_code`, threshold 0, requesting `/t<n>`.
    fn strm_start(track: u8, rate_code: u8, autostart: bool) -> Vec<u8> {
        let mut payload = vec![0u8; 24];
        payload[0] = b's';
        payload[1] = if autostart { b'1' } else { b'0' };
        payload[2] = b'p';
        payload[3] = b'1';
        payload[4] = rate_code;
        payload[5] = b'2';
        payload[6] = b'0';
        payload[18..20].copy_from_slice(&9000u16.to_be_bytes());
        payload[20..24].copy_from_slice(&[127, 0, 0, 1]);
        payload.extend_from_slice(format!("GET /t{track} HTTP/1.0\r\n\r\n").as_bytes());
        server_frame(b"strm", &payload)
    }

    /// STAT/DSCO lines only (the other frames are not about track boundaries).
    fn status_lines(rig: &Rig) -> Vec<String> {
        rig.lines().into_iter().filter(|l| l.starts_with("STAT") || l.starts_with("DSCO")).collect()
    }

    /// Ticks (10 ms apart, from `*now`) until `done` holds; records, for every `STMs`, the
    /// sink frame index of the boundary it reports (frames written so far minus the frames
    /// played since the crossing, which reset at it).
    fn run_until(
        rig: &mut Rig,
        pipeline: &mut GaplessPipeline<'_>,
        now: &mut u32,
        stms_at: &mut Vec<usize>,
        done: impl Fn(&Rig, &GaplessPipeline<'_>) -> bool,
    ) {
        for _ in 0..2_000 {
            if done(rig, pipeline) {
                return;
            }
            let before = rig.sent.len();
            *now += 10;
            rig.tick(pipeline, *now).unwrap();
            if rig.coord.external_audio_pump {
                // The device ring's pump phases (tasks.rs: output, decode, output).
                rig.coord.pump_output(pipeline, 16);
                rig.coord.pump_decode(pipeline, 4);
                rig.coord.pump_output(pipeline, 16);
            }
            for (op, payload) in &rig.sent[before..] {
                if op == b"STAT" && &payload[0..4] == b"STMs" {
                    let sink_frames = pipeline.sink.samples.len() / 2;
                    stms_at.push(sink_frames - pipeline.frames_played as usize);
                }
            }
        }
        panic!("condition not reached; sent {:?}", rig.lines());
    }

    fn count(lines: &[String], line: &str) -> usize {
        lines.iter().filter(|l| *l == line).count()
    }

    #[test]
    fn test_gapless_two_tracks_no_flush_one_stms_each() {
        let (t1, t2) = (track_bytes(1, 600), track_bytes(2, 400));
        let mut rig = gapless_rig(&[t1, t2]);
        let (mut s, mut o) = (vec![0u8; 8192], vec![0u8; 16384]);
        let mut p = gapless_pipeline(&mut s, &mut o);
        let (mut now, mut stms_at) = (100, Vec::new());

        rig.command(&mut p, &strm_start(1, b'3', true), now);
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |r, _| {
            count(&r.lines(), "STAT:STMd") == 1
        });
        // Track 1 is fully decoded and still playing out of the output buffer.
        assert!(p.output_buffer_available() > 0);
        let played_before_strm = p.sink.samples.len();

        // LMS sends the next track on STMd. C: no output flush, output stays RUNNING.
        now += 10;
        rig.command(&mut p, &strm_start(2, b'3', true), now);
        assert_eq!(rig.coord.state, PlayerState::Playing);
        assert!(p.sink.samples.len() > played_before_strm, "output kept playing through strm s");
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |r, _| {
            count(&r.lines(), "STAT:STMu") == 1
        });

        // Every byte of both tracks reached the sink, in order, with nothing in between.
        let mut expected = track_samples(1, 600);
        expected.extend(track_samples(2, 400));
        assert_eq!(p.sink.samples, expected);
        // Exactly one STMs per track; the second one at the boundary sample index.
        assert_eq!(stms_at, [0, 600]);
        assert_eq!(
            status_lines(&rig),
            [
                "STAT:STMf", "STAT:STMc", "DSCO:0", "STAT:STMs", "STAT:STMd", // track 1
                "STAT:STMf", "STAT:STMc", "DSCO:0", "STAT:STMd", // track 2 queued behind it
                "STAT:STMs", // track 2 reaches the output
                "STAT:STMu",
            ]
        );
    }

    #[test]
    fn test_strm_s_transition_and_invert_fade_out_at_decode_complete() {
        use squeezelite_audio_core::fade::{FadeDir, FadeState};
        let mut rig = gapless_rig(&[track_bytes(1, 600)]);
        let (mut s, mut o) = (vec![0u8; 8192], vec![0u8; 16384]);
        let mut p = gapless_pipeline(&mut s, &mut o);
        let (mut now, mut stms_at) = (100, Vec::new());
        let mut frame = strm_start(1, b'3', true);
        // strm payload (after the 8-byte header): [9] transition_period, [10] transition_type,
        // [11] flags. C slimproto.c:486-488: fade out, 1 s, invert ((flags & 0x03) == 0x03).
        frame[8 + 9] = 1;
        frame[8 + 10] = b'3';
        frame[8 + 11] = 0x03;
        rig.command(&mut p, &frame, now);
        assert_eq!(p.fade.mode(), FadeMode::Out);
        assert!(p.gain_stage.invert_polarity);
        assert_eq!(p.gain_stage.channel_mode, ChannelMode::Stereo);
        assert_eq!(p.fade.state(), FadeState::Inactive, "fade out arms nothing at the start");
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |r, _| count(&r.lines(), "STAT:STMd") == 1);
        // C decode.c:103 `_checkfade(false)`: down over the last min(unread, 1 s) frames.
        assert_ne!(p.fade.state(), FadeState::Inactive);
        assert_eq!(p.fade.dir(), FadeDir::Down);
        let dur = p.fade.duration_frames() as usize;
        assert!(dur > 64 && dur < 600, "{dur}");
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |r, _| count(&r.lines(), "STAT:STMu") == 1);
        let left: Vec<i32> = p.sink.samples.iter().step_by(2).copied().collect();
        assert_eq!(left.len(), 600);
        // Inverted throughout (track 1's left samples are positive).
        assert!(left.iter().all(|&v| v < 0));
        // Unfaded before the fade, attenuated at its end.
        assert_eq!(left[600 - dur - 1], -((1000 + 599 - dur as i32) << 16));
        assert!(left[599] > -((1599 << 16) / 2), "{}", left[599]);
    }

    #[test]
    fn test_strm_s_flags_set_channel_mode_and_polarity() {
        // C parity: slimproto.c:488-489, invert = (flags & 0x03) == 0x03, channels = (flags &
        // 0x0c) >> 2 with MONO_LEFT 0x01, MONO_RIGHT 0x02 (squeezelite.h:663-664).
        for (flags, invert, mode) in [
            (0x00, false, ChannelMode::Stereo),
            (0x01, false, ChannelMode::Stereo),
            (0x03, true, ChannelMode::Stereo),
            (0x04, false, ChannelMode::LeftOnly),
            (0x08, false, ChannelMode::RightOnly),
            (0x0c, false, ChannelMode::MonoMix),
            (0x0f, true, ChannelMode::MonoMix),
        ] {
            let mut rig = gapless_rig(&[track_bytes(1, 600)]);
            let (mut s, mut o) = (vec![0u8; 8192], vec![0u8; 16384]);
            let mut p = gapless_pipeline(&mut s, &mut o);
            let mut frame = strm_start(1, b'3', true);
            frame[8 + 11] = flags;
            rig.command(&mut p, &frame, 100);
            assert_eq!((p.gain_stage.invert_polarity, p.gain_stage.channel_mode), (invert, mode), "flags {flags:#04x}");
        }
    }

    #[test]
    fn test_gapless_through_the_device_pump_path() {
        // Same engine on the device: decode and output run in the pump phases (Task 9).
        let (t1, t2) = (track_bytes(1, 600), track_bytes(2, 400));
        let mut rig = gapless_rig(&[t1, t2]);
        rig.coord.external_audio_pump = true;
        let (mut s, mut o) = (vec![0u8; 8192], vec![0u8; 16384]);
        let mut p = gapless_pipeline(&mut s, &mut o);
        // The DMA takes 8 frames per write, so decoding runs ahead of playback.
        p.sink.limit = 8;
        let (mut now, mut stms_at) = (100, Vec::new());

        rig.command(&mut p, &strm_start(1, b'3', true), now);
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |r, _| {
            count(&r.lines(), "STAT:STMd") == 1
        });
        assert!(p.output_buffer_available() > 0);
        now += 10;
        rig.command(&mut p, &strm_start(2, b'3', true), now);
        assert_eq!(rig.coord.state, PlayerState::Playing);
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |r, _| {
            count(&r.lines(), "STAT:STMu") == 1
        });
        let mut expected = track_samples(1, 600);
        expected.extend(track_samples(2, 400));
        assert_eq!(p.sink.samples, expected);
        assert_eq!(stms_at, [0, 600]);
        assert_eq!(count(&rig.lines(), "STAT:STMs"), 2);
    }

    #[test]
    fn test_marker_queued_while_paused_no_stms() {
        let (t1, t2) = (track_bytes(1, 600), track_bytes(2, 100));
        let mut rig = gapless_rig(&[t1, t2]);
        let (mut s, mut o) = (vec![0u8; 8192], vec![0u8; 16384]);
        let mut p = gapless_pipeline(&mut s, &mut o);
        let (mut now, mut stms_at) = (100, Vec::new());

        rig.command(&mut p, &strm_start(1, b'3', true), now);
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |r, _| {
            count(&r.lines(), "STAT:STMd") == 1
        });
        now += 10;
        rig.command(&mut p, &strm_start(2, b'3', true), now);
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |_, p| p.track_marker().is_some());
        // Pause with the next track queued behind the boundary (strm p, interval 0).
        now += 10;
        rig.command(&mut p, &simple_strm(b'p'), now);
        assert_eq!(rig.coord.state, PlayerState::Paused);
        let marker = *p.track_marker().unwrap();
        let sink_len = p.sink.samples.len();
        for _ in 0..50 {
            now += 10;
            rig.tick(&mut p, now).unwrap();
        }
        // C: a STOPPED output plays silence and never reaches track_start (output.c:111-127).
        assert_eq!(p.track_marker(), Some(&marker), "boundary must wait while paused");
        assert_eq!(p.sink.samples.len(), sink_len);
        assert_eq!(count(&rig.lines(), "STAT:STMs"), 1, "no STMs while paused");
        assert!(!rig.coord.pending_track_started);

        // Resume: the tail plays, then the boundary is crossed once.
        now += 10;
        rig.command(&mut p, &simple_strm(b'u'), now);
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |r, _| {
            count(&r.lines(), "STAT:STMu") == 1
        });
        assert_eq!(stms_at, [0, 600]);
        let mut expected = track_samples(1, 600);
        expected.extend(track_samples(2, 100));
        assert_eq!(p.sink.samples, expected);
    }

    #[test]
    fn test_strm_f_discards_only_next_track_and_current_keeps_playing() {
        let (t1, t2) = (track_bytes(1, 600), track_bytes(2, 100));
        let mut rig = gapless_rig(&[t1, t2]);
        let (mut s, mut o) = (vec![0u8; 8192], vec![0u8; 16384]);
        let mut p = gapless_pipeline(&mut s, &mut o);
        let (mut now, mut stms_at) = (100, Vec::new());

        rig.command(&mut p, &strm_start(1, b'3', true), now);
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |r, _| {
            count(&r.lines(), "STAT:STMd") == 1
        });
        now += 10;
        rig.command(&mut p, &strm_start(2, b'3', true), now);
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |_, p| p.track_marker().is_some());
        let lines_before = rig.sent.len();

        // strm f: C output_flush_streaming drops track 2 (writep = track_start) and reports
        // STMf because something was flushed, even though stream 2 already ended
        // (slimproto.c:374-383, output.c:461-470).
        now += 10;
        rig.command(&mut p, &simple_strm(b'f'), now);
        assert_eq!(rig.lines()[lines_before..], ["STAT:STMf"]);
        assert!(p.track_marker().is_none());
        assert_eq!(rig.coord.state, PlayerState::Playing, "the current track keeps playing");
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |r, _| {
            count(&r.lines(), "STAT:STMu") == 1
        });
        assert_eq!(p.sink.samples, track_samples(1, 600));
        assert_eq!(stms_at, [0]);

        // A second strm f has nothing left to flush and no stream: no STMf.
        let lines_before = rig.sent.len();
        now += 10;
        rig.command(&mut p, &simple_strm(b'f'), now);
        assert!(rig.lines()[lines_before..].is_empty());
    }

    #[test]
    fn test_strm_q_clears_everything() {
        let (t1, t2) = (track_bytes(1, 600), track_bytes(2, 100));
        let mut rig = gapless_rig(&[t1, t2]);
        let (mut s, mut o) = (vec![0u8; 8192], vec![0u8; 16384]);
        let mut p = gapless_pipeline(&mut s, &mut o);
        let (mut now, mut stms_at) = (100, Vec::new());

        rig.command(&mut p, &strm_start(1, b'3', true), now);
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |r, _| {
            count(&r.lines(), "STAT:STMd") == 1
        });
        now += 10;
        rig.command(&mut p, &strm_start(2, b'3', true), now);
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |_, p| p.track_marker().is_some());
        let (sent_before, sink_len) = (rig.sent.len(), p.sink.samples.len());

        // C slimproto.c:386-391: decode_flush + output_flush, no reply.
        now += 10;
        rig.command(&mut p, &simple_strm(b'q'), now);
        assert_eq!(rig.coord.state, PlayerState::Stopped);
        assert!(p.track_marker().is_none());
        assert_eq!(p.output_buffer_available(), 0);
        assert_eq!(p.frames_played, 0);
        for _ in 0..20 {
            now += 10;
            rig.tick(&mut p, now).unwrap();
        }
        assert!(rig.lines()[sent_before..].is_empty(), "{:?}", &rig.lines()[sent_before..]);
        assert_eq!(p.sink.samples.len(), sink_len);
        assert!(p.active_marker_crossed().is_none());
    }

    #[test]
    fn test_second_strm_s_before_boundary_overwrites_it() {
        let (t1, t2, t3) = (track_bytes(1, 600), track_bytes(2, 50), track_bytes(3, 80));
        let mut rig = gapless_rig(&[t1, t2, t3]);
        let (mut s, mut o) = (vec![0u8; 8192], vec![0u8; 16384]);
        let mut p = gapless_pipeline(&mut s, &mut o);
        let (mut now, mut stms_at) = (100, Vec::new());

        rig.command(&mut p, &strm_start(1, b'3', true), now);
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |r, _| {
            count(&r.lines(), "STAT:STMd") == 1
        });
        now += 10;
        rig.command(&mut p, &strm_start(2, b'3', true), now);
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |r, _| {
            count(&r.lines(), "STAT:STMd") == 2
        });
        // Boundary position in sink frames: frames played so far plus the frames ahead of it.
        let boundary_at =
            |p: &GaplessPipeline<'_>| p.sink.samples.len() / 2 + p.track_marker().unwrap().bytes_ahead as usize / 8;
        let first = boundary_at(&p);
        assert_eq!(first, 600);
        now += 10;
        rig.command(&mut p, &strm_start(3, b'3', true), now);
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |r, _| {
            count(&r.lines(), "STAT:STMd") == 3
        });
        // One pending boundary (C output.track_start): track 3's start replaced track 2's.
        assert_eq!(boundary_at(&p), first + 50);
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |r, _| {
            count(&r.lines(), "STAT:STMu") == 1
        });
        // strm s never flushes outputbuf, so track 2's audio still plays (C), but only one
        // track start is reported after track 1's: track 3's.
        let mut expected = track_samples(1, 600);
        expected.extend(track_samples(2, 50));
        expected.extend(track_samples(3, 80));
        assert_eq!(p.sink.samples, expected);
        assert_eq!(stms_at, [0, 650]);
    }

    #[test]
    fn test_rate_change_at_boundary_with_rate_delay_inserts_split_silence() {
        let (t1, t2) = (track_bytes(1, 600), track_bytes(2, 100));
        let mut rig = gapless_rig(&[t1, t2]);
        let (mut s, mut o) = (vec![0u8; 8192], vec![0u8; 16384]);
        let mut p = gapless_pipeline(&mut s, &mut o);
        p.boundary.rate_delay_ms = 20; // C `-r ...:20` (main.c:509)
        let (mut now, mut stms_at) = (100, Vec::new());

        rig.command(&mut p, &strm_start(1, b'3', true), now); // 44.1 kHz
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |r, _| {
            count(&r.lines(), "STAT:STMd") == 1
        });
        now += 10;
        rig.command(&mut p, &strm_start(2, b'4', true), now); // 48 kHz
        // The playing track keeps its clock until the boundary.
        assert_eq!(rig.coord.current_sample_rate, 44_100);
        assert_eq!(p.sink.rate, 44_100);
        assert_eq!(rig.backend.rate, 44_100);
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |r, _| {
            count(&r.lines(), "STAT:STMu") == 1
        });
        assert_eq!(rig.coord.current_sample_rate, 48_000);
        assert_eq!(p.sink.rate, 48_000);

        // C output.c:128-148: 441 silent frames at 44.1 kHz before the boundary, 480 at
        // 48 kHz after it.
        let (first_half, second_half) = (441, 480);
        let mut expected = track_samples(1, 600);
        expected.extend(std::iter::repeat_n(0, (first_half + second_half) * 2));
        expected.extend(track_samples(2, 100));
        assert_eq!(p.sink.samples, expected);
        let at = |rate: u32| p.sink.writes.iter().filter(|w| w.0 == rate).map(|w| w.1).sum::<usize>();
        assert_eq!((at(44_100), at(48_000)), (600 + first_half, second_half + 100));
        // STMs comes with the track start, after the first half of the silence.
        assert_eq!(stms_at.len(), 2);
    }

    #[test]
    fn test_connect_failure_of_next_track_lets_the_current_one_finish() {
        let t1 = track_bytes(1, 600);
        let mut rig = gapless_rig(&[t1]);
        let (mut s, mut o) = (vec![0u8; 8192], vec![0u8; 16384]);
        let mut p = gapless_pipeline(&mut s, &mut o);
        let (mut now, mut stms_at) = (100, Vec::new());

        rig.command(&mut p, &strm_start(1, b'3', true), now);
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |r, _| {
            count(&r.lines(), "STAT:STMd") == 1
        });
        // /t2 has no route: the connect fails (C DISCONNECT/UNREACHABLE, stream.c:684-688), but
        // the output keeps RUNNING: strm s never touches it.
        now += 10;
        rig.command(&mut p, &strm_start(2, b'3', true), now);
        assert_eq!(rig.coord.state, PlayerState::Playing);
        run_until(&mut rig, &mut p, &mut now, &mut stms_at, |r, _| {
            count(&r.lines(), "STAT:STMu") == 1
        });
        assert_eq!(p.sink.samples, track_samples(1, 600));
        assert_eq!(count(&rig.lines(), "DSCO:3"), 1);
    }
}
