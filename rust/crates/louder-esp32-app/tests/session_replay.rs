//! Behavior-preservation replay fixture (Task 4).
//!
//! Drives the host `PlayerCoordinator` through a scripted LMS session against mock
//! SlimProto and HTTP servers, parses every outbound SlimProto frame into a
//! [`ClientMessage`], and compares the ordered opcode sequence with
//! `fixtures/strm_session_baseline.txt`.
//!
//! Since Task 7 the session is driven only through `PlayerCoordinator::tick` over the real
//! host adapters (`SlimprotoClient` as `SlimTransport`, `HttpAudioStream` as `StreamSource`);
//! the fixture itself is unchanged.
//!
//! The fixture is the regression net for the coordinator refactor: it must keep passing.
//! A legitimate behavior change updates the fixture with a comment line naming the task
//! (Task 11 re-pinned it to C squeezelite's STAT/DSCO sequencing).
#![cfg(not(target_arch = "xtensa"))]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use louder_esp32_app::coordinator::PlayerCoordinator;
use louder_esp32_app::io_traits::{MockBackend, SlimTransport, TransportError};
use louder_esp32_app::network::{HttpAudioStream, SlimprotoClient};
use louder_esp32_app::pipeline::{ActiveDecoder, StreamPipeline};
use louder_esp32_app::NullAudioSink;
use squeezelite_decoders::PcmDecoder;

const BASELINE_FIXTURE: &str = include_str!("fixtures/strm_session_baseline.txt");
const MAC: [u8; 6] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
const DEADLINE: Duration = Duration::from_secs(5);

/// Placeholder `strm` server port, replaced with the mock HTTP server's real port.
const HTTP_PORT_PLACEHOLDER: u16 = 0xFFFF;

/// One scripted event in a replayed LMS session.
enum ScriptStep {
    /// A complete SlimProto server frame sent to the player on the control connection.
    Server(Vec<u8>),
    /// Raw audio bytes written to the HTTP stream connection.
    StreamBytes(Vec<u8>),
    /// The HTTP stream server closes the connection (end of stream).
    StreamEof,
    /// Advance the simulated clock by this many milliseconds and run one coordinator tick.
    AdvanceMs(u32),
}

/// An outbound SlimProto frame, as seen by the mock LMS server.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ClientMessage {
    Stat([u8; 4]),
    Resp,
    Meta,
    Setd,
    Dsco(u8),
    Helo,
}

impl ClientMessage {
    /// Fixture line form: `HELO`, `STAT:STMc`, `RESP`, `DSCO:0`, ...
    fn to_line(&self) -> String {
        match self {
            Self::Stat(code) => format!("STAT:{}", String::from_utf8_lossy(code)),
            Self::Resp => "RESP".to_string(),
            Self::Meta => "META".to_string(),
            Self::Setd => "SETD".to_string(),
            Self::Dsco(reason) => format!("DSCO:{reason}"),
            Self::Helo => "HELO".to_string(),
        }
    }
}

fn parse_client_frames(mut bytes: &[u8]) -> Vec<ClientMessage> {
    let mut messages = Vec::new();
    while !bytes.is_empty() {
        assert!(bytes.len() >= 8, "truncated frame header: {bytes:?}");
        let len = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
        let payload = &bytes[8..8 + len];
        let message = match &bytes[0..4] {
            b"HELO" => ClientMessage::Helo,
            b"STAT" => ClientMessage::Stat([payload[0], payload[1], payload[2], payload[3]]),
            b"RESP" => ClientMessage::Resp,
            b"META" => ClientMessage::Meta,
            b"SETD" => ClientMessage::Setd,
            b"DSCO" => ClientMessage::Dsco(payload[0]),
            other => panic!("unexpected outbound opcode {other:?}"),
        };
        messages.push(message);
        bytes = &bytes[8 + len..];
    }
    messages
}

/// Builds a server `strm` frame (8-byte header + 24-byte payload) for PCM 44.1 kHz / 16-bit stereo.
fn strm_frame(command: u8, autostart: u8, replay_gain: u32) -> Vec<u8> {
    let mut payload = [0u8; 24];
    payload[0] = command;
    payload[1] = autostart;
    payload[2] = b'p'; // PCM
    payload[3] = b'1'; // 16-bit
    payload[4] = b'3'; // 44.1 kHz
    payload[5] = b'2'; // stereo
    payload[6] = b'1'; // little endian
    payload[7] = 0; // threshold_kb
    payload[14..18].copy_from_slice(&replay_gain.to_be_bytes());
    payload[18..20].copy_from_slice(&HTTP_PORT_PLACEHOLDER.to_be_bytes());
    payload[20..24].copy_from_slice(&[127, 0, 0, 1]);

    let mut frame = Vec::with_capacity(8 + payload.len());
    frame.extend_from_slice(b"strm");
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(&payload);
    frame
}

/// Deterministic 16-bit stereo little-endian PCM ramp.
fn pcm_bytes(len: usize) -> Vec<u8> {
    (0..len / 4)
        .flat_map(|i| {
            let l = (i as i16).wrapping_mul(3).to_le_bytes();
            let r = (i as i16).wrapping_mul(-3).to_le_bytes();
            [l[0], l[1], r[0], r[1]]
        })
        .collect()
}

fn accept_with_deadline(listener: &TcpListener) -> TcpStream {
    listener.set_nonblocking(true).unwrap();
    let start = Instant::now();
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(DEADLINE)).unwrap();
                stream.set_write_timeout(Some(DEADLINE)).unwrap();
                return stream;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(start.elapsed() < DEADLINE, "timed out waiting for connection");
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(e) => panic!("accept failed: {e}"),
        }
    }
}

/// Accepts the player's HTTP stream connection, consumes the request and answers `200 OK`.
fn accept_http_stream(listener: &TcpListener) -> TcpStream {
    let mut conn = accept_with_deadline(listener);
    let mut request = Vec::new();
    let mut byte = [0u8; 1];
    while !request.ends_with(b"\r\n\r\n") {
        conn.read_exact(&mut byte).expect("HTTP request header");
        request.push(byte[0]);
    }
    conn.write_all(b"HTTP/1.0 200 OK\r\nContent-Type: audio/L16\r\n\r\n")
        .unwrap();
    conn
}

type Pipeline<'a> = StreamPipeline<'a, NullAudioSink>;

/// The player's control connection, counting the server frames handed to the coordinator so the
/// script knows when a command has been consumed.
struct Lms {
    client: SlimprotoClient,
    frames_delivered: usize,
}

impl SlimTransport for Lms {
    fn poll_frame(&mut self, buf: &mut [u8]) -> Result<Option<usize>, TransportError> {
        let frame = self.client.poll_frame(buf)?;
        if frame.is_some() {
            self.frames_delivered += 1;
        }
        Ok(frame)
    }

    fn send(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        self.client.send(frame)
    }

    fn connected(&self) -> bool {
        self.client.connected()
    }
}

/// Everything one coordinator tick drives.
struct Player<'a> {
    coordinator: PlayerCoordinator,
    pipeline: Pipeline<'a>,
    lms: Lms,
    stream: HttpAudioStream,
    backend: MockBackend,
}

impl Player<'_> {
    fn tick(&mut self, now_ms: u32) {
        self.coordinator
            .tick(&mut self.lms, &mut self.stream, &mut self.backend, &mut self.pipeline, now_ms)
            .expect("coordinator tick");
    }
}

/// Ticks the coordinator until `done` holds.
fn tick_until(player: &mut Player<'_>, now_ms: u32, what: &str, done: impl Fn(&Player<'_>) -> bool) {
    let start = Instant::now();
    while !done(player) {
        player.tick(now_ms);
        assert!(start.elapsed() < DEADLINE, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Ticks until four consecutive ticks produce no output frames, so deferred STAT events flush.
fn quiesce(player: &mut Player<'_>, now_ms: u32) {
    let mut idle = 0;
    for _ in 0..10_000 {
        player.tick(now_ms);
        if player.coordinator.last_tick_frames() == 0 {
            idle += 1;
            if idle == 4 {
                return;
            }
        } else {
            idle = 0;
        }
    }
    panic!("coordinator never went idle");
}

/// Replays `script` against a fresh host coordinator and returns every outbound frame, in order.
fn run_scripted_session(script: &[ScriptStep]) -> Vec<ClientMessage> {
    let lms_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let http_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let lms_addr: SocketAddr = lms_listener.local_addr().unwrap();
    let http_port = http_listener.local_addr().unwrap().port();

    // The connect completes against the listen backlog; HELO is sent during connect.
    let client = SlimprotoClient::connect(lms_addr, MAC, "TestPlayer").unwrap();
    let mut lms_conn = accept_with_deadline(&lms_listener);

    let mut stream_storage = [0u8; 8192];
    let mut output_storage = [0u8; 8192];
    let mut player = Player {
        coordinator: PlayerCoordinator::new(MAC, "TestPlayer"),
        pipeline: StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Pcm(PcmDecoder::new(16, 2, false)),
            NullAudioSink::new(44100),
            44100,
        ),
        lms: Lms { client, frames_delivered: 0 },
        stream: HttpAudioStream::new(),
        backend: MockBackend::new(),
    };

    let mut http_conn: Option<TcpStream> = None;
    let mut now_ms: u32 = 0;
    let mut stream_bytes_sent: u64 = 0;

    for step in script {
        match step {
            ScriptStep::Server(frame) => {
                let mut frame = frame.clone();
                if &frame[0..4] == b"strm" && frame[8 + 18..8 + 20] == HTTP_PORT_PLACEHOLDER.to_be_bytes() {
                    frame[8 + 18..8 + 20].copy_from_slice(&http_port.to_be_bytes());
                }
                lms_conn.write_all(&frame).unwrap();

                let delivered = player.lms.frames_delivered + 1;
                tick_until(&mut player, now_ms, "server command", |p| {
                    p.lms.frames_delivered >= delivered
                });

                if player.coordinator.stream_active() && http_conn.is_none() {
                    http_conn = Some(accept_http_stream(&http_listener));
                }
            }
            ScriptStep::StreamBytes(bytes) => {
                http_conn
                    .as_mut()
                    .expect("StreamBytes before a stream was started")
                    .write_all(bytes)
                    .unwrap();
                stream_bytes_sent += bytes.len() as u64;
                tick_until(&mut player, now_ms, "stream bytes", |p| {
                    p.coordinator.stream_bytes_received >= stream_bytes_sent
                });
                quiesce(&mut player, now_ms);
            }
            ScriptStep::StreamEof => {
                drop(http_conn.take().expect("StreamEof before a stream was started"));
                tick_until(&mut player, now_ms, "stream EOF", |p| p.coordinator.is_stream_eof());
                quiesce(&mut player, now_ms);
            }
            ScriptStep::AdvanceMs(ms) => {
                now_ms = now_ms.wrapping_add(*ms);
                player.tick(now_ms);
            }
        }
    }

    // Closing the client lets the mock server read everything the player sent up to EOF.
    drop(player);
    let mut outbound = Vec::new();
    lms_conn.read_to_end(&mut outbound).unwrap();
    parse_client_frames(&outbound)
}

fn fixture_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

#[test]
fn test_baseline_session_replay() {
    let script = [
        // strm 't': timestamp probe echoes replay_gain
        ScriptStep::Server(strm_frame(b't', b'0', 0x1234)),
        // strm 's': PCM 44.1/16, autostart '1', threshold 0
        ScriptStep::Server(strm_frame(b's', b'1', 0)),
        ScriptStep::StreamBytes(pcm_bytes(32 * 1024)),
        ScriptStep::StreamEof,
        // one heartbeat interval elapses (no STMt: output stopped after STMu)
        ScriptStep::AdvanceMs(1000),
        // strm 'q' (no reply in C)
        ScriptStep::Server(strm_frame(b'q', b'0', 0)),
    ];

    let actual: Vec<String> = run_scripted_session(&script)
        .iter()
        .map(ClientMessage::to_line)
        .collect();
    let expected = fixture_lines(BASELINE_FIXTURE);

    assert_eq!(
        actual, expected,
        "outbound opcode sequence diverged from tests/fixtures/strm_session_baseline.txt"
    );
}
