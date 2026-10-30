//! Conformance tests for the `io_traits` fakes (host only).

use louder_esp32_app::io_traits::{
    run_stream_source_conformance, AudioBackend, ClockError, FakeStreamSource, LoopbackTransport,
    MockBackend, NetAddr, SlimTransport, StreamError, StreamSource, TransportError,
};

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
