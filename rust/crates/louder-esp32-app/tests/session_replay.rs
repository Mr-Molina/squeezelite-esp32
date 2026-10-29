//! Behavior-preservation replay fixture (Task 4).
//!
//! Drives the host `PlayerCoordinator` through a scripted LMS session against mock
//! SlimProto and HTTP servers, parses every outbound SlimProto frame into a
//! [`ClientMessage`], and compares the ordered opcode sequence with
//! `fixtures/strm_session_baseline.txt`.
//!
//! The fixture is the regression net for the coordinator refactor: it must keep passing.
//! A legitimate behavior change updates the fixture with a comment line naming the task.
#![cfg(not(target_arch = "xtensa"))]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use louder_esp32_app::coordinator::PlayerCoordinator;
use louder_esp32_app::network::SlimprotoClient;
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

/// Ticks the coordinator until `done` holds.
fn tick_until(
    coordinator: &mut PlayerCoordinator,
    pipeline: &mut Pipeline<'_>,
    client: &mut SlimprotoClient,
    now_ms: u32,
    what: &str,
    done: impl Fn(&PlayerCoordinator) -> bool,
) {
    let start = Instant::now();
    while !done(coordinator) {
        coordinator.tick(client, pipeline, now_ms);
        assert!(start.elapsed() < DEADLINE, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Ticks until four consecutive ticks produce no output frames, so deferred STAT events flush.
fn quiesce(
    coordinator: &mut PlayerCoordinator,
    pipeline: &mut Pipeline<'_>,
    client: &mut SlimprotoClient,
    now_ms: u32,
) {
    let mut idle = 0;
    for _ in 0..10_000 {
        if coordinator.tick(client, pipeline, now_ms) == 0 {
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
    let mut client = SlimprotoClient::connect(lms_addr, MAC, "TestPlayer").unwrap();
    let mut lms_conn = accept_with_deadline(&lms_listener);

    let mut coordinator = PlayerCoordinator::new(MAC, "TestPlayer");
    let mut stream_storage = [0u8; 8192];
    let mut output_storage = [0u8; 8192];
    let mut pipeline = StreamPipeline::new(
        &mut stream_storage,
        &mut output_storage,
        ActiveDecoder::Pcm(PcmDecoder::new(16, 2, false)),
        NullAudioSink::new(44100),
        44100,
    );

    let mut http_conn: Option<TcpStream> = None;
    let mut now_ms: u32 = 0;
    let mut stream_bytes_sent: u64 = 0;
    let mut scratch = [0u8; 2048];

    for step in script {
        match step {
            ScriptStep::Server(frame) => {
                let mut frame = frame.clone();
                if &frame[0..4] == b"strm" && frame[8 + 18..8 + 20] == HTTP_PORT_PLACEHOLDER.to_be_bytes() {
                    frame[8 + 18..8 + 20].copy_from_slice(&http_port.to_be_bytes());
                }
                lms_conn.write_all(&frame).unwrap();

                let start = Instant::now();
                let command = loop {
                    if let Some(cmd) = client.poll_command(&mut scratch).unwrap() {
                        break cmd;
                    }
                    assert!(start.elapsed() < DEADLINE, "timed out waiting for server command");
                    std::thread::sleep(Duration::from_millis(1));
                };
                coordinator
                    .handle_command(command, &mut pipeline, &mut client, now_ms)
                    .unwrap();

                if coordinator.audio_stream.is_some() && http_conn.is_none() {
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
                tick_until(&mut coordinator, &mut pipeline, &mut client, now_ms, "stream bytes", |c| {
                    c.stream_bytes_received >= stream_bytes_sent
                });
                quiesce(&mut coordinator, &mut pipeline, &mut client, now_ms);
            }
            ScriptStep::StreamEof => {
                drop(http_conn.take().expect("StreamEof before a stream was started"));
                tick_until(&mut coordinator, &mut pipeline, &mut client, now_ms, "stream EOF", |c| {
                    c.is_stream_eof()
                });
                quiesce(&mut coordinator, &mut pipeline, &mut client, now_ms);
            }
            ScriptStep::AdvanceMs(ms) => {
                now_ms = now_ms.wrapping_add(*ms);
                coordinator.tick(&mut client, &mut pipeline, now_ms);
            }
        }
    }

    // Closing the client lets the mock server read everything the player sent up to EOF.
    drop(pipeline);
    drop(coordinator);
    drop(client);
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
        // one heartbeat interval elapses
        ScriptStep::AdvanceMs(1000),
        // strm 'q'
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
