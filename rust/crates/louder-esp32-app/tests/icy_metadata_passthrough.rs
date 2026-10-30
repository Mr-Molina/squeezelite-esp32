//! ICY metadata end to end: a real HTTP server, the host `HttpAudioStream` adapter, the
//! coordinator `tick`, and the `META` frame it sends to LMS. The largest block ICY can carry
//! (255 * 16 = 4080 bytes) must reach LMS un-truncated.
#![cfg(not(target_arch = "xtensa"))]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use louder_esp32_app::coordinator::PlayerCoordinator;
use louder_esp32_app::io_traits::{LoopbackTransport, MockBackend};
use louder_esp32_app::network::HttpAudioStream;
use louder_esp32_app::pipeline::{ActiveDecoder, StreamPipeline};
use louder_esp32_app::NullAudioSink;
use squeezelite_decoders::PcmDecoder;

/// `strm s` (8-byte header form): PCM 16-bit / 44.1 kHz / stereo, autostart, threshold 0.
fn strm_start(port: u16) -> Vec<u8> {
    let mut payload = [0u8; 24];
    payload[0] = b's';
    payload[1] = b'1';
    payload[2] = b'p';
    payload[3] = b'1';
    payload[4] = b'3';
    payload[5] = b'2';
    payload[6] = b'1';
    payload[18..20].copy_from_slice(&port.to_be_bytes());
    payload[20..24].copy_from_slice(&[127, 0, 0, 1]);
    let mut frame = b"strm".to_vec();
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(&payload);
    frame
}

#[test]
fn test_max_size_icy_block_reaches_lms_untruncated() {
    // Metadata block: 255 length units = 4080 bytes, no NUL padding (nothing is trimmed).
    let mut meta = b"StreamTitle='".to_vec();
    meta.resize(4078, b'm');
    meta.extend_from_slice(b"';");
    assert_eq!(meta.len(), 255 * 16);

    let mut response = b"HTTP/1.0 200 OK\r\nContent-Type: audio/L16\r\nicy-metaint: 16\r\n\r\n".to_vec();
    response.extend_from_slice(&[0u8; 16]);
    response.push(255);
    response.extend_from_slice(&meta);
    response.extend_from_slice(&[0u8; 16]);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut byte = [0u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            conn.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        conn.write_all(&response).unwrap();
    });

    let mut coordinator = PlayerCoordinator::new([0x11, 0x22, 0x33, 0x44, 0x55, 0x66], "Icy");
    let mut lms = LoopbackTransport::new();
    let mut stream = HttpAudioStream::new();
    let mut backend = MockBackend::new();
    let mut stream_storage = vec![0u8; 8192];
    let mut output_storage = vec![0u8; 8192];
    let mut pipeline = StreamPipeline::new(
        &mut stream_storage,
        &mut output_storage,
        ActiveDecoder::Pcm(PcmDecoder::new(16, 2, false)),
        NullAudioSink::new(44100),
        44100,
    );

    lms.inject_frame(&strm_start(port)).unwrap();
    let mut meta_frame = None;
    let deadline = Instant::now() + Duration::from_secs(5);
    while meta_frame.is_none() {
        assert!(Instant::now() < deadline, "no META frame");
        coordinator
            .tick(&mut lms, &mut stream, &mut backend, &mut pipeline, 0)
            .unwrap();
        while let Some(frame) = lms.pop_sent() {
            if &frame[0..4] == b"META" {
                meta_frame = Some(frame);
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }

    let frame = meta_frame.unwrap();
    let len = u32::from_be_bytes([frame[4], frame[5], frame[6], frame[7]]) as usize;
    assert_eq!(len, 4080);
    assert_eq!(&frame[8..], &meta[..], "META payload is the whole ICY block");
    server.join().unwrap();
}
