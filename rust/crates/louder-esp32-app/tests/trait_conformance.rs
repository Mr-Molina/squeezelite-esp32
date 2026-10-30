//! Conformance tests for the `io_traits` fakes and the host (`std::net`) adapters.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use louder_esp32_app::io_traits::conformance::ROUTES;
use louder_esp32_app::io_traits::{
    run_stream_source_conformance, AudioBackend, ClockError, FakeStreamSource, LoopbackTransport,
    MockBackend, NetAddr, SlimTransport, StreamError, StreamSource, TransportError,
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
    fn take_response_headers(&mut self) -> Option<heapless::Vec<u8, 1024>> {
        StreamSource::take_response_headers(&mut self.inner)
    }
    fn take_icy_metadata(&mut self) -> Option<heapless::Vec<u8, 256>> {
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
