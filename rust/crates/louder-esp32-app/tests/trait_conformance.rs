//! Conformance tests for the `io_traits` fakes and the host (`std::net`) adapters.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use louder_esp32_app::io_traits::conformance::ROUTES;
use louder_esp32_app::io_traits::{
    run_stream_source_conformance, AudioBackend, ClockError, FakeStreamSource, HeaderBuf,
    LoopbackTransport, MetaBuf, MockBackend, NetAddr, SlimTransport, StreamError, StreamSource,
    TransportError, MAX_RESPONSE_HEAD,
};
use louder_esp32_app::network::{HttpAudioStream, SlimprotoClient};
use louder_esp32_app::{SlimprotoBuilder, SlimprotoParser};
use squeezelite_proto::{ServerCommand, StreamCommandType};

#[test]
fn test_loopback_roundtrip_frames() {
    let mut t = LoopbackTransport::echo();
    assert!(t.connected());
    let mut buf = [0u8; 64];
    assert_eq!(t.poll_frame(&mut buf), Ok(None));

    t.send(b"HELO").unwrap();
    t.send(b"STAT-and-more").unwrap();
    t.send(&[]).unwrap();

    let n = t.poll_frame(&mut buf).unwrap().unwrap();
    assert_eq!(&buf[..n], b"HELO");
    let n = t.poll_frame(&mut buf).unwrap().unwrap();
    assert_eq!(&buf[..n], b"STAT-and-more");
    // Zero-length frames are real frames, not "nothing available".
    assert_eq!(t.poll_frame(&mut buf), Ok(Some(0)));
    assert_eq!(t.poll_frame(&mut buf), Ok(None));
}

#[test]
fn test_loopback_injected_and_sent_are_independent() {
    let mut t = LoopbackTransport::new();
    t.inject_frame(b"strm").unwrap();
    t.send(b"HELO").unwrap();
    let mut buf = [0u8; 16];
    let n = t.poll_frame(&mut buf).unwrap().unwrap();
    assert_eq!(&buf[..n], b"strm");
    assert_eq!(t.poll_frame(&mut buf), Ok(None));
    let sent = t.pop_sent().unwrap();
    assert_eq!(&sent[..], b"HELO");
    assert!(t.pop_sent().is_none());
}

#[test]
fn test_loopback_disconnect_semantics() {
    let mut t = LoopbackTransport::new();
    t.inject_frame(b"last").unwrap();
    t.disconnect();
    assert!(!t.connected());
    let mut buf = [0u8; 16];
    // Frames already queued are still delivered, then Disconnected.
    let n = t.poll_frame(&mut buf).unwrap().unwrap();
    assert_eq!(&buf[..n], b"last");
    assert_eq!(t.poll_frame(&mut buf), Err(TransportError::Disconnected));
    assert_eq!(t.send(b"x"), Err(TransportError::Disconnected));
}

#[test]
fn test_loopback_small_buffer_is_io_error() {
    let mut t = LoopbackTransport::new();
    t.inject_frame(&[7u8; 32]).unwrap();
    let mut small = [0u8; 8];
    assert_eq!(t.poll_frame(&mut small), Err(TransportError::Io));
}

#[test]
fn test_fake_source_passes_conformance() {
    run_stream_source_conformance(FakeStreamSource::with_conformance_routes);
}

#[test]
fn test_fake_source_connect_failure_and_abort() {
    let mut s = FakeStreamSource::with_conformance_routes();
    s.fail_connect = true;
    let addr = NetAddr { ip: [10, 0, 0, 1], port: 9000 };
    assert_eq!(s.connect(addr, b"GET /plain HTTP/1.0\r\n\r\n"), Err(StreamError::ConnectFailed));

    let mut s = FakeStreamSource::with_conformance_routes();
    s.connect(addr, b"GET /plain HTTP/1.0\r\n\r\n").unwrap();
    s.abort();
    let mut out = [0u8; 8];
    assert_eq!(s.poll_bytes(&mut out), Err(StreamError::Disconnected));
    assert!(!s.is_eof());
}

#[test]
fn test_fake_source_unknown_path_is_bad_response() {
    let mut s = FakeStreamSource::with_conformance_routes();
    let addr = NetAddr { ip: [10, 0, 0, 1], port: 9000 };
    assert_eq!(s.connect(addr, b"GET /nope HTTP/1.0\r\n\r\n"), Err(StreamError::BadResponse));
}

#[test]
fn test_mock_backend_records_and_rejects_unsupported_rate() {
    let mut b = MockBackend::default();
    assert_eq!(b.set_sample_rate(44_100), Ok(()));
    assert_eq!(b.rate, 44_100);
    assert_eq!(b.set_sample_rate(12_345), Err(ClockError::UnsupportedRate(12_345)));
    assert_eq!(b.rate, 44_100);
    b.set_hw_volume(3, 4);
    b.set_mute(true);
    b.set_power(true);
    b.outstanding = 99;
    assert_eq!((b.hw_vol, b.muted, b.powered), ((3, 4), true, true));
    assert_eq!(b.dma_outstanding_frames(), 99);
}

// ---------------------------------------------------------------------------
// Host adapters: HttpAudioStream as StreamSource
// ---------------------------------------------------------------------------

fn net_addr(a: SocketAddr) -> NetAddr {
    match a {
        SocketAddr::V4(v4) => NetAddr { ip: v4.ip().octets(), port: v4.port() },
        SocketAddr::V6(_) => panic!("IPv4 only"),
    }
}

/// Reads one HTTP request head (up to the blank line) and returns the request path.
fn read_request_path(conn: &mut TcpStream) -> Option<Vec<u8>> {
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if conn.read(&mut b).ok()? == 0 {
            return None;
        }
        head.push(b[0]);
    }
    let line_end = head.iter().position(|&c| c == b'\r')?;
    let line = &head[..line_end];
    let mut parts = line.split(|&c| c == b' ');
    parts.next()?;
    parts.next().map(<[u8]>::to_vec)
}

/// Writes each segment with its own `write` + flush, pausing between them so the segment
/// boundaries tend to become TCP read boundaries, then closes the connection.
fn write_segments(conn: &mut TcpStream, segments: &[&[u8]]) {
    for seg in segments {
        if conn.write_all(seg).is_err() || conn.flush().is_err() {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Mock HTTP server replaying `ROUTES` verbatim (relative `Location` values included); any
/// other path gets a `404`. Runs for the life of the test process.
fn spawn_conformance_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut conn) = conn else { continue };
            std::thread::spawn(move || {
                let Some(path) = read_request_path(&mut conn) else { return };
                match ROUTES.iter().find(|(p, _)| *p == &path[..]) {
                    Some((_, segments)) => write_segments(&mut conn, segments),
                    None => write_segments(&mut conn, &[b"HTTP/1.0 404 Not Found\r\n\r\n"]),
                }
            });
        }
    });
    addr
}

/// The conformance suite always connects to its fixed script address; this shim points the real
/// `HttpAudioStream` at the local mock server instead and yields briefly when a poll has
/// nothing to deliver so the suite's bounded poll loops do not outrun the server.
struct ToMock {
    inner: HttpAudioStream,
    mock: NetAddr,
}

impl StreamSource for ToMock {
    fn connect(&mut self, _addr: NetAddr, http_request: &[u8]) -> Result<(), StreamError> {
        StreamSource::connect(&mut self.inner, self.mock, http_request)
    }
    fn poll_bytes(&mut self, out: &mut [u8]) -> Result<usize, StreamError> {
        let n = StreamSource::poll_bytes(&mut self.inner, out)?;
        if n == 0 && !StreamSource::is_eof(&self.inner) {
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(n)
    }
    fn take_response_headers(&mut self) -> Option<HeaderBuf> {
        StreamSource::take_response_headers(&mut self.inner)
    }
    fn take_icy_metadata(&mut self) -> Option<MetaBuf> {
        StreamSource::take_icy_metadata(&mut self.inner)
    }
    fn set_icy_metaint(&mut self, interval: usize) {
        StreamSource::set_icy_metaint(&mut self.inner, interval);
    }
    fn is_eof(&self) -> bool {
        StreamSource::is_eof(&self.inner)
    }
    fn abort(&mut self) {
        StreamSource::abort(&mut self.inner);
    }
}

#[test]
fn test_http_audio_stream_passes_conformance() {
    let mock = net_addr(spawn_conformance_server());
    run_stream_source_conformance(|| ToMock { inner: HttpAudioStream::new(), mock });
}

/// One-shot server: replies with `segments` (then closes unless `hold` is set, in which case
/// the connection stays open until the client closes it).
fn spawn_scripted_server(segments: Vec<Vec<u8>>, hold: bool) -> NetAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = net_addr(listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        if read_request_path(&mut conn).is_none() {
            return;
        }
        let refs: Vec<&[u8]> = segments.iter().map(Vec::as_slice).collect();
        write_segments(&mut conn, &refs);
        if hold {
            let mut b = [0u8; 16];
            while matches!(conn.read(&mut b), Ok(n) if n > 0) {}
        }
    });
    addr
}

const REQ: &[u8] = b"GET /s HTTP/1.0\r\nHost: test\r\n\r\n";

fn connect_blocking<S: StreamSource>(s: &mut S, addr: NetAddr) -> Result<(), StreamError> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match s.connect(addr, REQ) {
            Err(StreamError::WouldBlock) if Instant::now() < deadline => {}
            other => return other,
        }
    }
}

/// Polls until the stream ends: `Ok(bytes)` on clean EOF, `Err((bytes_before, error))` otherwise.
fn read_to_end<S: StreamSource>(s: &mut S) -> Result<Vec<u8>, (Vec<u8>, StreamError)> {
    let mut body = Vec::new();
    let mut buf = [0u8; 16];
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match s.poll_bytes(&mut buf) {
            Ok(n) => body.extend_from_slice(&buf[..n]),
            Err(e) => return Err((body, e)),
        }
        if s.is_eof() {
            return Ok(body);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("stream neither ended nor failed");
}

#[test]
fn test_http_audio_stream_truncated_chunked_is_disconnected_not_eof() {
    // Closed between chunks, before the terminating zero-size chunk.
    let head = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
    let addr = spawn_scripted_server(vec![head.clone(), b"5\r\nhello\r\n".to_vec()], false);
    let mut s = HttpAudioStream::new();
    connect_blocking(&mut s, addr).unwrap();
    let (body, err) = read_to_end(&mut s).unwrap_err();
    assert_eq!(body, b"hello", "bytes received before the drop are still delivered");
    assert_eq!(err, StreamError::Disconnected);
    assert!(!StreamSource::is_eof(&s), "a truncated stream must never report EOF");
    // The error is sticky.
    let mut out = [0u8; 8];
    assert_eq!(s.poll_bytes(&mut out), Err(StreamError::Disconnected));

    // Closed in the middle of a chunk's data.
    let addr = spawn_scripted_server(vec![head, b"a\r\nabc".to_vec()], false);
    let mut s = HttpAudioStream::new();
    connect_blocking(&mut s, addr).unwrap();
    let (body, err) = read_to_end(&mut s).unwrap_err();
    assert_eq!(body, b"abc");
    assert_eq!(err, StreamError::Disconnected);
    assert!(!StreamSource::is_eof(&s));
}

#[test]
fn test_http_audio_stream_short_content_length_is_disconnected() {
    let seg = b"HTTP/1.0 200 OK\r\nContent-Length: 100\r\n\r\nabc".to_vec();
    let addr = spawn_scripted_server(vec![seg], false);
    let mut s = HttpAudioStream::new();
    connect_blocking(&mut s, addr).unwrap();
    let (body, err) = read_to_end(&mut s).unwrap_err();
    assert_eq!(body, b"abc");
    assert_eq!(err, StreamError::Disconnected);
    assert!(!StreamSource::is_eof(&s));
}

#[test]
fn test_http_audio_stream_drop_during_headers_is_disconnected() {
    let addr = spawn_scripted_server(vec![b"HTTP/1.0 200 OK\r\nContent-Ty".to_vec()], false);
    let mut s = HttpAudioStream::new();
    assert_eq!(connect_blocking(&mut s, addr), Err(StreamError::Disconnected));
    let mut out = [0u8; 8];
    assert_eq!(s.poll_bytes(&mut out), Err(StreamError::Disconnected));
    assert!(!StreamSource::is_eof(&s));
}

#[test]
fn test_http_audio_stream_clean_close_is_eof() {
    // Close-delimited body (no length, not chunked) and an exact Content-Length both end cleanly.
    for head in [&b"HTTP/1.0 200 OK\r\n\r\n"[..], b"HTTP/1.0 200 OK\r\nContent-Length: 5\r\n\r\n"] {
        let mut seg = head.to_vec();
        seg.extend_from_slice(b"hello");
        let addr = spawn_scripted_server(vec![seg], false);
        let mut s = HttpAudioStream::new();
        connect_blocking(&mut s, addr).unwrap();
        assert_eq!(read_to_end(&mut s).unwrap(), b"hello");
        assert!(StreamSource::is_eof(&s));
    }
}

#[test]
fn test_http_audio_stream_abort_errors_until_reconnect() {
    let seg = b"HTTP/1.0 200 OK\r\n\r\nabcdef".to_vec();
    let addr = spawn_scripted_server(vec![seg.clone()], true);
    let mut s = HttpAudioStream::new();
    connect_blocking(&mut s, addr).unwrap();
    let mut out = [0u8; 3];
    let deadline = Instant::now() + Duration::from_secs(5);
    while s.poll_bytes(&mut out) == Ok(0) {
        assert!(Instant::now() < deadline, "no data arrived");
        std::thread::sleep(Duration::from_millis(1));
    }

    s.abort();
    for _ in 0..3 {
        assert_eq!(s.poll_bytes(&mut out), Err(StreamError::Disconnected));
    }
    assert!(!StreamSource::is_eof(&s), "abort is not a clean end of stream");

    // The next connect brings the source back to life.
    let addr = spawn_scripted_server(vec![seg], false);
    connect_blocking(&mut s, addr).unwrap();
    assert_eq!(read_to_end(&mut s).unwrap(), b"abcdef");
}

#[test]
fn test_http_audio_stream_idle_and_abort_before_connect() {
    let mut s = HttpAudioStream::new();
    let mut out = [0u8; 8];
    assert_eq!(s.poll_bytes(&mut out), Err(StreamError::Disconnected));
    s.abort(); // harmless on an idle source
    assert_eq!(s.poll_bytes(&mut out), Err(StreamError::Disconnected));
    assert!(!StreamSource::is_eof(&s));
}

#[test]
fn test_http_audio_stream_connect_refused() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = net_addr(l.local_addr().unwrap());
    drop(l);
    let mut s = HttpAudioStream::new();
    assert_eq!(connect_blocking(&mut s, addr), Err(StreamError::ConnectFailed));
}

#[test]
fn test_http_audio_stream_absolute_location_redirect() {
    let target = spawn_scripted_server(vec![b"HTTP/1.0 200 OK\r\n\r\nxyz".to_vec()], false);
    let redirect = format!(
        "HTTP/1.0 303 See Other\r\nLocation: http://127.0.0.1:{}/other\r\n\r\n",
        target.port
    );
    let addr = spawn_scripted_server(vec![redirect.into_bytes()], false);
    let mut s = HttpAudioStream::new();
    connect_blocking(&mut s, addr).unwrap();
    assert_eq!(read_to_end(&mut s).unwrap(), b"xyz");
}

// ---------------------------------------------------------------------------
// Host adapters: SlimprotoClient as SlimTransport
// ---------------------------------------------------------------------------

const MAC: [u8; 6] = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];

fn strm_t_frame() -> [u8; 30] {
    // 2-byte length (opcode + payload) + `strm` + 24-byte payload with command 't'.
    let mut f = [0u8; 30];
    f[0..2].copy_from_slice(&28u16.to_be_bytes());
    f[2..6].copy_from_slice(b"strm");
    f[6] = b't';
    f[7] = b'0';
    f[8] = b'm';
    f
}

fn poll_until_frame(t: &mut impl SlimTransport, buf: &mut [u8]) -> Result<usize, TransportError> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(n) = t.poll_frame(buf)? {
            return Ok(n);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("no frame arrived");
}

fn slim_pair() -> (SlimprotoClient, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let client = SlimprotoClient::connect(addr, MAC, "TestModel").unwrap();
    let (mut server, _) = listener.accept().unwrap();
    server.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    // The connect handshake already sent a HELO.
    let mut helo = [0u8; 8];
    server.read_exact(&mut helo).unwrap();
    assert_eq!(&helo[0..4], b"HELO");
    let len = u32::from_be_bytes([helo[4], helo[5], helo[6], helo[7]]) as usize;
    let mut rest = vec![0u8; len];
    server.read_exact(&mut rest).unwrap();
    (client, server)
}

#[test]
fn test_slimproto_client_as_transport() {
    let (mut client, mut server) = slim_pair();
    assert!(SlimTransport::connected(&client));
    let mut buf = [0u8; 256];
    assert_eq!(SlimTransport::poll_frame(&mut client, &mut buf), Ok(None));

    // HELO out through `send`: bytes reach the server verbatim.
    let mut helo = [0u8; 128];
    let n = SlimprotoBuilder::build_helo_with_options(&MAC, 12, "Model=T,mp3", false, 0, &mut helo)
        .unwrap();
    SlimTransport::send(&mut client, &helo[..n]).unwrap();
    let mut got = vec![0u8; n];
    server.read_exact(&mut got).unwrap();
    assert_eq!(got, &helo[..n]);
    assert_eq!(&got[0..4], b"HELO");

    // Scripted `strm t` in through `poll_frame`: the raw frame, length prefix included.
    let frame = strm_t_frame();
    server.write_all(&frame).unwrap();
    let n = poll_until_frame(&mut client, &mut buf).unwrap();
    assert_eq!(&buf[..n], &frame[..]);
    let (cmd, consumed) = SlimprotoParser::parse_frame(&buf[..n]).unwrap();
    assert_eq!(consumed, n);
    match cmd {
        ServerCommand::Stream { command, .. } => assert_eq!(command, StreamCommandType::Timestamp),
        other => panic!("expected strm, got {other:?}"),
    }
    assert_eq!(SlimTransport::poll_frame(&mut client, &mut buf), Ok(None));
}

#[test]
fn test_slimproto_transport_split_and_coalesced_frames() {
    let (mut client, mut server) = slim_pair();
    let frame = strm_t_frame();
    let flsh = [b'f', b'l', b's', b'h', 0, 0, 0, 0];
    let mut buf = [0u8; 64];

    // A frame split across TCP segments is only delivered once complete.
    server.write_all(&frame[..10]).unwrap();
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(SlimTransport::poll_frame(&mut client, &mut buf), Ok(None));
    server.write_all(&frame[10..]).unwrap();
    let n = poll_until_frame(&mut client, &mut buf).unwrap();
    assert_eq!(&buf[..n], &frame[..]);

    // Two frames in one segment come out one per call, in order, without further socket data.
    let mut both = Vec::new();
    both.extend_from_slice(&flsh);
    both.extend_from_slice(&frame);
    server.write_all(&both).unwrap();
    let n = poll_until_frame(&mut client, &mut buf).unwrap();
    assert_eq!(&buf[..n], &flsh[..]);
    assert_eq!(SlimTransport::poll_frame(&mut client, &mut buf), Ok(Some(frame.len())));
    assert_eq!(&buf[..frame.len()], &frame[..]);
    assert_eq!(SlimTransport::poll_frame(&mut client, &mut buf), Ok(None));
}

#[test]
fn test_slimproto_transport_small_buffer_drops_frame_then_recovers() {
    let (mut client, mut server) = slim_pair();
    let frame = strm_t_frame();
    let flsh = [b'f', b'l', b's', b'h', 0, 0, 0, 0];
    let mut both = Vec::new();
    both.extend_from_slice(&frame);
    both.extend_from_slice(&flsh);
    server.write_all(&both).unwrap();

    let mut small = [0u8; 16];
    let deadline = Instant::now() + Duration::from_secs(5);
    let err = loop {
        match SlimTransport::poll_frame(&mut client, &mut small) {
            Ok(None) => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            other => break other,
        }
    };
    assert_eq!(err, Err(TransportError::Io));
    assert!(SlimTransport::connected(&client), "an oversized frame is not a disconnect");
    assert_eq!(SlimTransport::poll_frame(&mut client, &mut small), Ok(Some(flsh.len())));
    assert_eq!(&small[..flsh.len()], &flsh[..]);
}

#[test]
fn test_slimproto_transport_disconnect_drains_then_errors() {
    let (mut client, mut server) = slim_pair();
    let frame = strm_t_frame();
    server.write_all(&frame).unwrap();
    server.write_all(&frame).unwrap();
    drop(server);

    let mut buf = [0u8; 64];
    // Frames received before the peer closed are still delivered ...
    assert_eq!(poll_until_frame(&mut client, &mut buf), Ok(frame.len()));
    assert_eq!(poll_until_frame(&mut client, &mut buf), Ok(frame.len()));
    // ... and only then does the disconnect surface.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match SlimTransport::poll_frame(&mut client, &mut buf) {
            Ok(None) => {
                assert!(Instant::now() < deadline, "disconnect never reported");
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(e) => {
                assert_eq!(e, TransportError::Disconnected);
                break;
            }
            Ok(Some(_)) => panic!("unexpected extra frame"),
        }
    }
    assert!(!SlimTransport::connected(&client));
    assert_eq!(SlimTransport::send(&mut client, b"HELO"), Err(TransportError::Disconnected));
}

// ---------------------------------------------------------------------------
// Trait-doc truncation limits
// ---------------------------------------------------------------------------

#[test]
fn test_http_audio_stream_long_headers_survive_up_to_cap() {
    // A ~3.5 KB head (far over the old 1024-byte cap) arrives whole. The line-boundary cut for
    // heads over MAX_RESPONSE_HEAD is unit-tested in `network::tests`.
    let mut head = String::from("HTTP/1.0 200 OK\r\nContent-Type: audio/mpeg\r\n");
    let mut i = 0;
    while head.len() < 3500 {
        head.push_str(&format!("X-Pad-{i:03}: {}E\r\n", "v".repeat(30 + i % 7)));
        i += 1;
    }
    head.push_str("icy-metaint: 16000\r\n\r\n");
    assert!(head.len() < MAX_RESPONSE_HEAD);
    let addr = spawn_scripted_server(vec![head.clone().into_bytes()], true);
    let mut s = HttpAudioStream::new();
    connect_blocking(&mut s, addr).unwrap();

    let h = StreamSource::take_response_headers(&mut s).expect("headers captured");
    assert_eq!(&h[..], head.as_bytes(), "head delivered un-truncated");
}

#[test]
fn test_http_audio_stream_icy_metadata_over_300_bytes_untruncated() {
    // 20 * 16 = 320-byte metadata block (312 bytes + NUL padding) after 16 audio bytes; it used
    // to be cut to 256 bytes.
    let mut body = b"HTTP/1.0 200 OK\r\n\r\n".to_vec();
    body.extend_from_slice(&[b'A'; 16]);
    body.push(20);
    let mut meta = b"StreamTitle='".to_vec();
    meta.resize(310, b'x');
    meta.extend_from_slice(b"';");
    meta.resize(320, 0);
    body.extend_from_slice(&meta);
    body.extend_from_slice(&[b'B'; 16]);
    let addr = spawn_scripted_server(vec![body], false);
    let mut s = HttpAudioStream::new();
    connect_blocking(&mut s, addr).unwrap();
    s.set_icy_metaint(16);

    let mut audio = Vec::new();
    let mut icy = None;
    let mut buf = [0u8; 64];
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < deadline);
        let n = s.poll_bytes(&mut buf).unwrap();
        audio.extend_from_slice(&buf[..n]);
        icy = icy.or_else(|| StreamSource::take_icy_metadata(&mut s));
        if StreamSource::is_eof(&s) {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(audio.len(), 32);
    let m = icy.expect("metadata block");
    assert_eq!(m.len(), 312, "whole block, only the NUL padding removed");
    assert_eq!(&m[..], &meta[..312]);
}

// ---------------------------------------------------------------------------
// Device adapters (`net_device`) over an in-memory socket shim
// ---------------------------------------------------------------------------

use louder_esp32_app::io_traits::conformance;
use louder_esp32_app::net_device::{
    server_frame_len, HttpStreamState, SlimLinkState, SmoltcpSlimTransport, SmoltcpStreamSource,
    TcpPhase, TcpSocketOps,
};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

/// How the shim server ends a response once its segments are drained.
#[derive(Clone, Copy, PartialEq, Debug)]
enum ShimEnd {
    /// FIN: the socket reports `PeerClosed`.
    Fin,
    /// RST: the socket reports `Closed`.
    Rst,
    /// Keep the connection open.
    Hold,
}

/// In-memory stand-in for a smoltcp TCP socket plus the HTTP server behind it.
///
/// * `connect` puts the socket in `Connecting` for `connect_polls` `phase()` calls, then
///   `Established` (or `Closed` when `refuse` is set: a RST to the SYN).
/// * Bytes sent are the request; once its head is complete the server picks the route by path
///   (`ROUTES`, `extra` routes; anything else is a `404`) and queues its segments.
/// * Each segment is one TCP read: `recv`/`peek` never cross a segment boundary, the next
///   segment becomes readable once the current one is drained (HTTP mode). In `manual` mode the
///   pushed pieces accumulate in the receive buffer, one more per socket call.
/// * After the last segment the server ends per `end` (or the route's override).
/// * In `manual` mode (slimproto tests) there is no server: tests push rx bytes directly.
struct ShimSocket {
    phase: Cell<TcpPhase>,
    connect_polls: u32,
    polls_left: Cell<u32>,
    refuse: bool,
    end: ShimEnd,
    manual: bool,
    extra: Vec<(Vec<u8>, Vec<Vec<u8>>, ShimEnd)>,
    tx_cap: usize,
    rx_cap: usize,
    request: Vec<u8>,
    requests: Vec<Vec<u8>>,
    connects: Vec<NetAddr>,
    rx: RefCell<VecDeque<u8>>,
    pending: RefCell<VecDeque<Vec<u8>>>,
    route_end: ShimEnd,
    responded: bool,
    sent: Vec<u8>,
    queued: usize,
}

impl ShimSocket {
    fn server() -> Self {
        Self {
            phase: Cell::new(TcpPhase::Closed),
            connect_polls: 2,
            polls_left: Cell::new(0),
            refuse: false,
            end: ShimEnd::Fin,
            manual: false,
            extra: Vec::new(),
            tx_cap: 64,
            rx_cap: 4096,
            request: Vec::new(),
            requests: Vec::new(),
            connects: Vec::new(),
            rx: RefCell::new(VecDeque::new()),
            pending: RefCell::new(VecDeque::new()),
            route_end: ShimEnd::Fin,
            responded: false,
            sent: Vec::new(),
            queued: 0,
        }
    }

    fn manual() -> Self {
        Self {
            manual: true,
            phase: Cell::new(TcpPhase::Established),
            tx_cap: 256,
            rx_cap: 256,
            ..Self::server()
        }
    }

    fn route(mut self, path: &[u8], segments: &[&[u8]], end: ShimEnd) -> Self {
        self.extra.push((path.to_vec(), segments.iter().map(|s| s.to_vec()).collect(), end));
        self
    }

    fn push_rx(&mut self, bytes: &[u8]) {
        self.pending.borrow_mut().push_back(bytes.to_vec());
    }

    /// Makes the next segment readable once the current one is drained; after the last one,
    /// ends the connection as scripted.
    fn refill(&self) {
        let mut rx = self.rx.borrow_mut();
        if self.manual {
            // Like a real receive buffer: data that arrived accumulates (one piece per call).
            if let Some(seg) = self.pending.borrow_mut().pop_front() {
                rx.extend(seg);
            }
            return;
        }
        if rx.is_empty() {
            if let Some(seg) = self.pending.borrow_mut().pop_front() {
                rx.extend(seg);
            } else if self.responded && !self.manual && self.phase.get() == TcpPhase::Established {
                match self.route_end {
                    ShimEnd::Fin => self.phase.set(TcpPhase::PeerClosed),
                    ShimEnd::Rst => self.phase.set(TcpPhase::Closed),
                    ShimEnd::Hold => {}
                }
            }
        }
    }

    fn serve(&mut self) {
        if self.manual || self.responded || !self.request.windows(4).any(|w| w == b"\r\n\r\n") {
            return;
        }
        self.responded = true;
        let line_end = self.request.iter().position(|&c| c == b'\r').unwrap();
        let path = self.request[..line_end].split(|&c| c == b' ').nth(1).unwrap().to_vec();
        self.requests.push(core::mem::take(&mut self.request));
        let (segments, end): (Vec<Vec<u8>>, ShimEnd) =
            if let Some((_, segs, end)) = self.extra.iter().find(|(p, _, _)| *p == path) {
                (segs.clone(), *end)
            } else if let Some((_, segs)) = ROUTES.iter().find(|(p, _)| *p == &path[..]) {
                (segs.iter().map(|s| s.to_vec()).collect(), self.end)
            } else {
                (vec![b"HTTP/1.0 404 Not Found\r\n\r\n".to_vec()], self.end)
            };
        self.pending.borrow_mut().extend(segments);
        self.route_end = end;
    }
}

impl TcpSocketOps for ShimSocket {
    fn phase(&self) -> TcpPhase {
        if self.phase.get() == TcpPhase::Connecting {
            if self.polls_left.get() == 0 {
                self.phase.set(if self.refuse { TcpPhase::Closed } else { TcpPhase::Established });
            } else {
                self.polls_left.set(self.polls_left.get() - 1);
            }
        }
        self.refill();
        self.phase.get()
    }

    fn connect(&mut self, remote: NetAddr) -> bool {
        self.abort();
        self.connects.push(remote);
        self.phase.set(TcpPhase::Connecting);
        self.polls_left.set(self.connect_polls);
        true
    }

    fn abort(&mut self) {
        self.phase.set(TcpPhase::Closed);
        self.request.clear();
        self.rx.borrow_mut().clear();
        self.pending.borrow_mut().clear();
        self.responded = false;
    }

    fn send_capacity(&self) -> usize {
        self.tx_cap
    }

    fn send_queue(&self) -> usize {
        self.queued
    }

    fn send_slice(&mut self, data: &[u8]) -> usize {
        if self.phase.get() != TcpPhase::Established {
            return 0;
        }
        let n = data.len().min(self.tx_cap - self.queued);
        self.request.extend_from_slice(&data[..n]);
        self.sent.extend_from_slice(&data[..n]);
        self.serve();
        n
    }

    fn recv_queue(&self) -> usize {
        self.refill();
        self.rx.borrow().len()
    }

    fn recv_capacity(&self) -> usize {
        self.rx_cap
    }

    fn peek_slice(&mut self, buf: &mut [u8]) -> usize {
        self.refill();
        let rx = self.rx.borrow();
        let n = buf.len().min(rx.len());
        for (dst, src) in buf.iter_mut().zip(rx.iter()) {
            *dst = *src;
        }
        n
    }

    fn recv_slice(&mut self, buf: &mut [u8]) -> usize {
        let n = self.peek_slice(buf);
        self.rx.borrow_mut().drain(..n);
        n
    }
}

type DeviceSource = SmoltcpStreamSource<ShimSocket, HttpStreamState>;

fn device_source(sock: ShimSocket) -> DeviceSource {
    SmoltcpStreamSource::new(sock, HttpStreamState::new())
}

#[test]
fn test_device_stream_source_passes_conformance() {
    run_stream_source_conformance(|| device_source(ShimSocket::server()));
}

#[test]
fn test_device_stream_source_conformance_over_keepalive_and_byte_writes() {
    // The server keeps the connection open after the body (so only framing can end it) and the
    // socket accepts a single request byte per send.
    run_stream_source_conformance(|| {
        let mut sock = ShimSocket::server();
        sock.tx_cap = 1;
        sock.connect_polls = 0;
        device_source(sock)
    });
}

const DEV_ADDR: NetAddr = NetAddr { ip: [10, 0, 0, 9], port: 9000 };

fn dev_connect(s: &mut DeviceSource, request: &[u8]) -> Result<(), StreamError> {
    for _ in 0..64 {
        match s.connect(DEV_ADDR, request) {
            Err(StreamError::WouldBlock) => {}
            other => return other,
        }
    }
    panic!("connect never resolved");
}

fn dev_read_to_end(s: &mut DeviceSource) -> Result<Vec<u8>, (Vec<u8>, StreamError)> {
    let mut body = Vec::new();
    let mut buf = [0u8; 16];
    for _ in 0..512 {
        match s.poll_bytes(&mut buf) {
            Ok(n) => body.extend_from_slice(&buf[..n]),
            Err(e) => return Err((body, e)),
        }
        if s.is_eof() {
            return Ok(body);
        }
    }
    panic!("stream neither ended nor failed");
}

#[test]
fn test_device_stream_connect_is_nonblocking_until_the_head_is_parsed() {
    let mut s = device_source(ShimSocket::server());
    assert_eq!(s.connect(DEV_ADDR, b"GET /plain HTTP/1.0\r\n\r\n"), Err(StreamError::WouldBlock));
    assert_eq!(s.socket().connects, vec![DEV_ADDR]);
    // Repeating the call resumes the same request (no second TCP connect).
    dev_connect(&mut s, b"GET /plain HTTP/1.0\r\n\r\n").unwrap();
    assert_eq!(s.socket().connects.len(), 1);
    assert_eq!(s.socket().requests, vec![b"GET /plain HTTP/1.0\r\n\r\n".to_vec()]);
    assert_eq!(dev_read_to_end(&mut s).unwrap(), conformance::PLAIN_BODY);
}

#[test]
fn test_device_stream_refused_connect_is_connect_failed() {
    let mut sock = ShimSocket::server();
    sock.refuse = true;
    let mut s = device_source(sock);
    assert_eq!(dev_connect(&mut s, b"GET /plain HTTP/1.0\r\n\r\n"), Err(StreamError::ConnectFailed));
    let mut buf = [0u8; 8];
    assert_eq!(s.poll_bytes(&mut buf), Err(StreamError::Disconnected));
}

#[test]
fn test_device_stream_drop_mid_chunked_body_is_disconnected_after_buffered_bytes() {
    let sock = ShimSocket::server().route(
        b"/t",
        &[b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n8\r\nabcd", b"ef"],
        ShimEnd::Fin,
    );
    let mut s = device_source(sock);
    dev_connect(&mut s, b"GET /t HTTP/1.1\r\n\r\n").unwrap();
    let (body, err) = dev_read_to_end(&mut s).unwrap_err();
    assert_eq!(body, b"abcdef", "bytes received before the drop are delivered first");
    assert_eq!(err, StreamError::Disconnected);
    assert!(!s.is_eof(), "an early drop is never EOF");
}

#[test]
fn test_device_stream_short_content_length_is_disconnected() {
    let sock = ShimSocket::server().route(
        b"/cl",
        &[b"HTTP/1.0 200 OK\r\nContent-Length: 10\r\n\r\n0123", b"45"],
        ShimEnd::Fin,
    );
    let mut s = device_source(sock);
    dev_connect(&mut s, b"GET /cl HTTP/1.0\r\n\r\n").unwrap();
    let (body, err) = dev_read_to_end(&mut s).unwrap_err();
    assert_eq!((body.as_slice(), err), (&b"012345"[..], StreamError::Disconnected));
}

#[test]
fn test_device_stream_reset_mid_close_delimited_body_is_disconnected() {
    let sock = ShimSocket::server().route(b"/rst", &[b"HTTP/1.0 200 OK\r\n\r\nabc"], ShimEnd::Rst);
    let mut s = device_source(sock);
    dev_connect(&mut s, b"GET /rst HTTP/1.0\r\n\r\n").unwrap();
    let (body, err) = dev_read_to_end(&mut s).unwrap_err();
    assert_eq!((body.as_slice(), err), (&b"abc"[..], StreamError::Disconnected));
}

#[test]
fn test_device_stream_content_length_reached_is_eof_without_close() {
    let sock = ShimSocket::server().route(
        b"/ka",
        &[b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhel", b"lo"],
        ShimEnd::Hold,
    );
    let mut s = device_source(sock);
    dev_connect(&mut s, b"GET /ka HTTP/1.1\r\n\r\n").unwrap();
    assert_eq!(dev_read_to_end(&mut s).unwrap(), b"hello");
    let mut buf = [0u8; 4];
    assert_eq!(s.poll_bytes(&mut buf), Ok(0), "EOF is sticky");
}

#[test]
fn test_device_stream_drop_during_head_is_disconnected() {
    let sock = ShimSocket::server().route(b"/h", &[b"HTTP/1.0 200 OK\r\nConte"], ShimEnd::Fin);
    let mut s = device_source(sock);
    assert_eq!(dev_connect(&mut s, b"GET /h HTTP/1.0\r\n\r\n"), Err(StreamError::Disconnected));
}

#[test]
fn test_device_stream_abort_errors_until_reconnect() {
    let mut s = device_source(ShimSocket::server());
    dev_connect(&mut s, b"GET /plain HTTP/1.0\r\n\r\n").unwrap();
    let mut buf = [0u8; 8];
    assert!(s.poll_bytes(&mut buf).unwrap() > 0);
    s.abort();
    for _ in 0..3 {
        assert_eq!(s.poll_bytes(&mut buf), Err(StreamError::Disconnected));
        assert!(!s.is_eof());
    }
    assert!(s.take_response_headers().is_none());
    dev_connect(&mut s, b"GET /plain HTTP/1.0\r\n\r\n").unwrap();
    assert_eq!(dev_read_to_end(&mut s).unwrap(), conformance::PLAIN_BODY);
}

#[test]
fn test_device_stream_redirects_rewrite_request_and_follow_absolute_ipv4() {
    let sock = ShimSocket::server()
        .route(b"/abs", &[b"HTTP/1.0 302 Found\r\nLocation: http://10.0.0.7:8000/plain\r\n\r\n"], ShimEnd::Fin)
        .route(b"/host", &[b"HTTP/1.0 302 Found\r\nLocation: http://lms.local/plain\r\n\r\n"], ShimEnd::Fin);
    let mut s = device_source(sock);
    dev_connect(&mut s, b"GET /r1 HTTP/1.0\r\nHost: 10.0.0.9:9000\r\nIcy-MetaData: 1\r\n\r\n").unwrap();
    assert_eq!(
        s.socket().requests.last().unwrap(),
        b"GET /plain HTTP/1.0\r\nHost: 10.0.0.9:9000\r\nIcy-MetaData: 1\r\n\r\n"
    );
    assert_eq!(s.socket().connects, vec![DEV_ADDR, DEV_ADDR], "same-origin hop reconnects");

    dev_connect(&mut s, b"GET /abs HTTP/1.0\r\n\r\n").unwrap();
    assert_eq!(*s.socket().connects.last().unwrap(), NetAddr { ip: [10, 0, 0, 7], port: 8000 });
    assert_eq!(s.socket().requests.last().unwrap(), b"GET /plain HTTP/1.0\r\nHost: 10.0.0.7:8000\r\n\r\n");
    assert_eq!(dev_read_to_end(&mut s).unwrap(), conformance::PLAIN_BODY);

    // No resolver on the device: a host-name Location cannot be followed.
    assert_eq!(dev_connect(&mut s, b"GET /host HTTP/1.0\r\n\r\n"), Err(StreamError::ConnectFailed));
}

#[test]
fn test_device_stream_side_data_by_callback_and_head_cap() {
    let mut s = device_source(ShimSocket::server());
    dev_connect(&mut s, b"GET /icy HTTP/1.0\r\n\r\n").unwrap();
    let mut head = Vec::new();
    assert!(s.take_response_headers_with(&mut |h| head.extend_from_slice(h)));
    assert!(head.starts_with(b"HTTP/1.0 200 OK") && head.ends_with(b"\r\n\r\n"));
    assert!(!s.take_response_headers_with(&mut |_| panic!("taken once")));
    s.set_icy_metaint(conformance::ICY_METAINT);
    let mut meta = Vec::new();
    let mut buf = [0u8; 64];
    while !s.is_eof() {
        s.poll_bytes(&mut buf).unwrap();
        s.take_icy_metadata_with(&mut |m| meta.extend_from_slice(m));
    }
    assert_eq!(meta, conformance::ICY_META);

    // A head over MAX_RESPONSE_HEAD is a bad response.
    let big: &'static [u8] = Box::leak(
        [b"HTTP/1.0 200 OK\r\n".as_slice(), &[b'x'; MAX_RESPONSE_HEAD]].concat().into_boxed_slice(),
    );
    let mut s = device_source(ShimSocket::server().route(b"/big", &[big], ShimEnd::Hold));
    assert_eq!(dev_connect(&mut s, b"GET /big HTTP/1.0\r\n\r\n"), Err(StreamError::BadResponse));
}

fn slim_frame(opcode: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    // Server -> client framing: 2-byte big-endian length (opcode + payload), opcode, payload.
    let mut f = ((payload.len() + 4) as u16).to_be_bytes().to_vec();
    f.extend_from_slice(opcode);
    f.extend_from_slice(payload);
    f
}

type DeviceSlim = SmoltcpSlimTransport<ShimSocket, SlimLinkState>;

fn device_slim() -> DeviceSlim {
    SmoltcpSlimTransport::new(ShimSocket::manual(), SlimLinkState::new())
}

#[test]
fn test_device_slim_transport_frames_split_and_coalesced() {
    let mut t = device_slim();
    let a = slim_frame(b"strm", &[b't'; 24]);
    let b = slim_frame(b"audg", &[0; 18]);
    let mut both = a.clone();
    both.extend_from_slice(&b);
    // Split: header across two reads, payload across a third.
    t.socket_mut().push_rx(&a[..3]);
    t.socket_mut().push_rx(&a[3..10]);
    t.socket_mut().push_rx(&a[10..]);
    // Coalesced: two frames in one read.
    t.socket_mut().push_rx(&both);
    let mut buf = [0u8; 2048];
    let mut got = Vec::new();
    for _ in 0..16 {
        if let Some(n) = t.poll_frame(&mut buf).unwrap() {
            got.push(buf[..n].to_vec());
        }
    }
    assert_eq!(got, vec![a.clone(), a, b]);
    assert_eq!(t.poll_frame(&mut buf), Ok(None));
    assert_eq!(server_frame_len(&got[2]), Ok(got[2].len()));
}

#[test]
fn test_device_slim_transport_oversized_frame_dropped_then_recovers() {
    let mut t = device_slim();
    let big = slim_frame(b"strm", &[7u8; 600]);
    let next = slim_frame(b"audg", &[1; 18]);
    t.socket_mut().push_rx(&big[..200]);
    t.socket_mut().push_rx(&big[200..]);
    t.socket_mut().push_rx(&next);
    let mut small = [0u8; 64];
    assert_eq!(t.poll_frame(&mut small), Err(TransportError::Io), "too big for the caller");
    let mut got = None;
    for _ in 0..8 {
        if let Some(n) = t.poll_frame(&mut small).unwrap() {
            got = Some(small[..n].to_vec());
            break;
        }
    }
    assert_eq!(got.unwrap(), next, "the stream resynchronises on the next frame");

    // A frame larger than the socket's receive buffer can never complete: dropped too.
    let mut t = device_slim();
    let huge = slim_frame(b"strm", &[7u8; 1000]);
    t.socket_mut().push_rx(&huge[..256]);
    let mut buf = [0u8; 2048];
    assert_eq!(t.poll_frame(&mut buf), Err(TransportError::Io));
}

#[test]
fn test_device_slim_transport_disconnect_drains_then_errors() {
    let mut t = device_slim();
    let a = slim_frame(b"strm", &[b'q'; 24]);
    t.socket_mut().push_rx(&a);
    t.socket_mut().push_rx(&a[..5]); // partial frame left behind by the closed peer
    t.socket_mut().phase.set(TcpPhase::PeerClosed);
    let mut buf = [0u8; 256];
    assert_eq!(t.poll_frame(&mut buf), Ok(Some(a.len())));
    assert_eq!(t.poll_frame(&mut buf), Err(TransportError::Disconnected));
    assert!(!t.connected());
    assert_eq!(t.send(b"HELO"), Err(TransportError::Disconnected));
}

#[test]
fn test_device_slim_transport_send_is_whole_frame() {
    let mut t = device_slim();
    assert!(t.connected());
    t.send(b"STAT0123").unwrap();
    assert_eq!(t.socket().sent, b"STAT0123");
    t.socket_mut().tx_cap = 4;
    assert_eq!(t.send(b"STAT0123"), Err(TransportError::Io), "can never fit the tx buffer");
    t.socket_mut().tx_cap = 16;
    t.socket_mut().sent.clear();
    t.socket_mut().queued = 12;
    assert_eq!(t.send(b"STAT0123"), Err(TransportError::WouldBlock), "no partial frames");
    assert!(t.socket().sent.is_empty());
}
