//! Behavioural conformance suite for [`StreamSource`](super::StreamSource) implementations.
//!
//! The suite only drives the `StreamSource` API. It selects what the "server" returns through
//! the request path, so the implementation under test must be connected to a server (or scripted
//! fake) that serves [`ROUTES`]: [`FakeStreamSource`](super::FakeStreamSource) does so directly,
//! and a socket shim only needs a tiny HTTP server replaying the same segments (one `write` +
//! flush per segment, so segment boundaries become TCP read boundaries) with the connection
//! closed after the last one. `Location` values are same-origin paths; a shim may rewrite them
//! to absolute URLs of its own origin. Everything is `no_std` and allocation free.

use super::{NetAddr, StreamError, StreamSource, MAX_REDIRECTS};
use heapless::Vec;

/// Audio payload of `/plain` (48 bytes).
pub const PLAIN_BODY: &[u8] = b"0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF";
/// Audio payload of `/icy` and `/icy-split` with `icy-metaint` 16 (48 bytes).
pub const ICY_AUDIO: &[u8] = b"AAAAAAAAAAAAAAAABBBBBBBBBBBBBBBBCCCCCCCCCCCCCCCC";
/// The single metadata block carried by `/icy` and `/icy-split`, trailing NUL padding removed.
pub const ICY_META: &[u8] = b"StreamTitle='Song One';";
/// De-chunked payload of `/chunked` (27 bytes).
pub const CHUNKED_BODY: &[u8] = b"0123456789ABCDEFabcdefghXYZ";
/// ICY interval used by the ICY cases.
pub const ICY_METAINT: usize = 16;

/// Scripted server: `(request path, response segments)`.
///
/// * `/plain`: `200`, `Content-Type: audio/mpeg`; headers split mid-line across reads; body is
///   [`PLAIN_BODY`].
/// * `/icy`: `200`, [`ICY_AUDIO`] with a 32-byte metadata block (`StreamTitle` NUL padded) after
///   the first 16 audio bytes and an empty block after the next 16, all in one read.
/// * `/icy-split`: same bytes, split between audio and the length byte, inside the metadata
///   block, and between the empty block and the last audio run.
/// * `/chunked`: `Transfer-Encoding: chunked` (framing split across reads, including inside a
///   chunk-size line), de-chunked payload is [`CHUNKED_BODY`].
/// * `/r1`..`/r4`: `302` to `/plain` (`/r1`) or to the previous route (`/r2` -> `/r1`, ...).
/// * Any other path: `404`.
pub const ROUTES: &[(&[u8], &[&[u8]])] = &[
    (
        b"/plain",
        &[
            b"HTTP/1.0 200 OK\r\nContent-Ty",
            b"pe: audio/mpeg\r\n\r\n0123456789AB",
            b"CDEF0123456789ABCDEF",
            b"0123456789ABCDEF",
        ],
    ),
    (
        b"/icy",
        &[b"HTTP/1.0 200 OK\r\nContent-Type: audio/mpeg\r\n\r\nAAAAAAAAAAAAAAAA\x02StreamTitle='Song One';\0\0\0\0\0\0\0\0\0BBBBBBBBBBBBBBBB\0CCCCCCCCCCCCCCCC"],
    ),
    (
        b"/icy-split",
        &[
            b"HTTP/1.0 200 OK\r\nContent-Type: audio/mpeg\r\n\r\nAAAAAAAAAAAAAAAA",
            b"\x02StreamTitl",
            b"e='Song One';\0\0\0\0\0\0\0\0\0BBBBBBBBBBBBBBBB\0",
            b"CCCCCCCCCCCCCCCC",
        ],
    ),
    (
        b"/chunked",
        &[
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: audio/mpeg\r\n\r\n1",
            b"0\r\n01234567",
            b"89ABCDEF\r",
            b"\n8\r\nabcd",
            b"efgh\r\n3\r\nXYZ\r\n",
            b"0\r\n\r\n",
        ],
    ),
    (b"/r1", &[b"HTTP/1.0 302 Found\r\nLocation: /plain\r\n\r\n"]),
    (b"/r2", &[b"HTTP/1.0 302 Found\r\nLocation: /r1\r\n\r\n"]),
    (b"/r3", &[b"HTTP/1.0 302 Found\r\nLocation: /r2\r\n\r\n"]),
    (b"/r4", &[b"HTTP/1.0 302 Found\r\nLocation: /r3\r\n\r\n"]),
];

const ADDR: NetAddr = NetAddr { ip: [10, 0, 0, 1], port: 9000 };
/// Upper bound on poll/connect iterations before a case is declared stuck.
const MAX_POLLS: usize = 512;

fn request(path: &[u8]) -> Vec<u8, 128> {
    let mut r = Vec::new();
    for part in [&b"GET "[..], path, b" HTTP/1.0\r\nHost: 10.0.0.1:9000\r\n\r\n"] {
        r.extend_from_slice(part).expect("request fits");
    }
    r
}

fn connect<S: StreamSource>(s: &mut S, path: &[u8]) -> Result<(), StreamError> {
    let req = request(path);
    for _ in 0..MAX_POLLS {
        match s.connect(ADDR, &req) {
            Err(StreamError::WouldBlock) => {}
            other => return other,
        }
    }
    panic!("connect never left WouldBlock");
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

struct Drained {
    body: Vec<u8, 512>,
    icy: Vec<Vec<u8, 256>, 4>,
}

/// Polls until EOF with reads of at most `read_len` bytes, collecting audio and metadata.
fn drain<S: StreamSource>(s: &mut S, read_len: usize, metaint: usize) -> Drained {
    if metaint > 0 {
        s.set_icy_metaint(metaint);
    }
    let mut d = Drained { body: Vec::new(), icy: Vec::new() };
    let mut buf = [0u8; 64];
    for _ in 0..MAX_POLLS {
        let n = s.poll_bytes(&mut buf[..read_len]).expect("poll_bytes failed mid-stream");
        assert!(n <= read_len, "poll_bytes overran the output buffer");
        d.body.extend_from_slice(&buf[..n]).expect("body too long");
        if let Some(m) = s.take_icy_metadata() {
            d.icy.push(m).expect("too many metadata blocks");
        }
        if s.is_eof() {
            // EOF is sticky and yields no further data.
            assert_eq!(s.poll_bytes(&mut buf[..read_len]), Ok(0), "data after EOF");
            assert!(s.is_eof(), "EOF not sticky");
            return d;
        }
    }
    panic!("stream never reached EOF");
}

fn check_headers<S: StreamSource>(s: &mut S, status_prefix: &[u8], needle: &[u8]) {
    let h = s.take_response_headers().expect("response headers not captured");
    assert!(h.starts_with(status_prefix), "unexpected status line: {:?}", &h[..]);
    assert!(contains(&h, needle), "missing header {:?} in {:?}", needle, &h[..]);
    assert!(h.ends_with(b"\r\n\r\n"), "headers must include the terminating blank line");
    assert!(s.take_response_headers().is_none(), "headers must be taken once");
}

/// Runs every case against fresh sources from `mk`; panics on the first violation.
///
/// Cases: plain stream, header capture, EOF (not set early, sticky afterwards), ICY metaint
/// stripping, ICY block split across reads, chunked transfer-encoding, redirects (3 followed,
/// the 4th fails with [`StreamError::TooManyRedirects`]) and error status. Each streaming case
/// is repeated with 64, 7 and 1 byte output buffers.
pub fn run_stream_source_conformance<S: StreamSource>(mut mk: impl FnMut() -> S) {
    // Plain stream + header capture + EOF.
    for read_len in [64, 7, 1] {
        let mut s = mk();
        connect(&mut s, b"/plain").expect("connect /plain");
        assert!(!s.is_eof(), "EOF reported before any byte was read");
        check_headers(&mut s, b"HTTP/1.", b"Content-Type: audio/mpeg");
        let d = drain(&mut s, read_len, 0);
        assert_eq!(&d.body[..], PLAIN_BODY, "plain body (read_len {read_len})");
        assert!(d.icy.is_empty(), "no metadata expected without ICY");
    }

    // ICY stripping, including a block split across reads.
    for path in [&b"/icy"[..], b"/icy-split"] {
        for read_len in [64, 7, 1] {
            let mut s = mk();
            connect(&mut s, path).expect("connect icy route");
            let d = drain(&mut s, read_len, ICY_METAINT);
            assert_eq!(&d.body[..], ICY_AUDIO, "icy audio ({path:?}, read_len {read_len})");
            assert_eq!(d.icy.len(), 1, "exactly one complete metadata block ({path:?})");
            assert_eq!(&d.icy[0][..], ICY_META, "icy metadata ({path:?}, read_len {read_len})");
        }
    }

    // Chunked transfer-encoding, framing split across reads.
    for read_len in [64, 7, 1] {
        let mut s = mk();
        connect(&mut s, b"/chunked").expect("connect /chunked");
        let d = drain(&mut s, read_len, 0);
        assert_eq!(&d.body[..], CHUNKED_BODY, "chunked body (read_len {read_len})");
    }

    // Redirects: up to MAX_REDIRECTS followed, one more is refused.
    assert_eq!(MAX_REDIRECTS, 3, "suite scripts assume a limit of 3");
    for path in [&b"/r1"[..], b"/r2", b"/r3"] {
        let mut s = mk();
        connect(&mut s, path).expect("redirect within limit");
        check_headers(&mut s, b"HTTP/1.0 200", b"Content-Type: audio/mpeg");
        let d = drain(&mut s, 64, 0);
        assert_eq!(&d.body[..], PLAIN_BODY, "body after redirect from {path:?}");
    }
    let mut s = mk();
    assert_eq!(connect(&mut s, b"/r4"), Err(StreamError::TooManyRedirects));

    // Non-success status.
    let mut s = mk();
    assert_eq!(connect(&mut s, b"/missing"), Err(StreamError::BadResponse));
}
