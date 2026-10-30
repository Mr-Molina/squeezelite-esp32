//! Pure HTTP audio-response parsing shared by every [`StreamSource`](crate::io_traits::StreamSource)
//! implementation: the host `std::net` adapter (`network::HttpAudioStream`) and the device
//! adapter ([`crate::net_device::SmoltcpStreamSource`]).
//!
//! Everything here is a byte-in/byte-out state machine with no I/O:
//! * [`ChunkedDecoder`]: `Transfer-Encoding: chunked` framing, decoded in place;
//! * [`IcyStripper`]: removes ICY (`icy-metaint`) metadata blocks from the audio, in place, and
//!   keeps the most recent complete block;
//! * [`RedirectPolicy`]: the redirect hop limit, `Location` classification and the rewritten
//!   request for the next hop;
//! * [`ResponseHead`] and [`find_head_end`]: status line and header lookups on a raw response
//!   head.
//!
//! Each machine accepts its input split at any byte boundary (a TCP read may end anywhere), which
//! is what lets the device adapter feed it straight from smoltcp socket reads.

extern crate alloc;

use alloc::vec::Vec;

use crate::io_traits::{NetAddr, StreamError, MAX_REDIRECTS};

/// Largest chunk size accepted by [`ChunkedDecoder`]; a larger size line is a framing error.
pub const MAX_CHUNK_SIZE: usize = 1_048_576;

// ---------------------------------------------------------------------------
// ChunkedDecoder
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChunkState {
    /// Chunk-size line. `invalid` defers a bad digit until the line is complete (a size line
    /// split across reads is only judged once its newline arrived).
    Size { acc: usize, digits: bool, gap: bool, ext: bool, invalid: bool },
    /// Inside chunk data, this many payload bytes left.
    Data(usize),
    /// After chunk data: expecting `\r\n` (or a bare `\n`); `cr` once the `\r` was seen.
    DataEnd { cr: bool },
    /// After the last (zero-size) chunk: trailer lines until an empty line.
    Trailer { line_len: usize },
    /// The empty line after the trailer was seen; any further byte is ignored.
    Done,
}

const CHUNK_START: ChunkState =
    ChunkState::Size { acc: 0, digits: false, gap: false, ext: false, invalid: false };

/// Incremental `Transfer-Encoding: chunked` decoder.
///
/// Size lines are hexadecimal with optional surrounding blanks and `;extension`s, terminated by
/// `\r\n` or a bare `\n`. Chunk data is followed by `\r\n` (or `\n`). After the zero-size chunk,
/// trailer lines are skipped up to the terminating empty line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkedDecoder {
    state: ChunkState,
}

impl Default for ChunkedDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl ChunkedDecoder {
    /// Decoder at the start of the first chunk-size line.
    pub const fn new() -> Self {
        Self { state: CHUNK_START }
    }

    /// Back to the start of the first chunk-size line.
    pub fn reset(&mut self) {
        self.state = CHUNK_START;
    }

    /// Decodes the raw body bytes in `buf` in place: the chunk payload they carry is moved to
    /// the front of `buf` and its length returned. Every input byte is consumed (bytes after the
    /// terminating empty line are ignored). A framing error is [`StreamError::BadResponse`].
    pub fn decode_in_place(&mut self, buf: &mut [u8]) -> Result<usize, StreamError> {
        let (mut r, mut w) = (0usize, 0usize);
        while r < buf.len() {
            if let ChunkState::Data(rem) = self.state {
                // Bulk copy of payload bytes.
                let take = rem.min(buf.len() - r);
                buf.copy_within(r..r + take, w);
                r += take;
                w += take;
                self.state = if take == rem { ChunkState::DataEnd { cr: false } } else { ChunkState::Data(rem - take) };
                continue;
            }
            let b = buf[r];
            r += 1;
            self.state = match self.state {
                ChunkState::Size { acc, digits, gap, ext, invalid } => match b {
                    b'\n' => {
                        if invalid || !digits {
                            return Err(StreamError::BadResponse);
                        }
                        if acc == 0 {
                            ChunkState::Trailer { line_len: 0 }
                        } else {
                            ChunkState::Data(acc)
                        }
                    }
                    b'\r' => self.state,
                    _ if ext => self.state,
                    b';' => ChunkState::Size { acc, digits, gap, ext: true, invalid },
                    b' ' | b'\t' => ChunkState::Size { acc, digits, gap: digits, ext, invalid },
                    _ => match (b as char).to_digit(16) {
                        Some(d) if !gap && !invalid => {
                            match acc.checked_mul(16).map(|a| a + d as usize) {
                                Some(acc) if acc <= MAX_CHUNK_SIZE => {
                                    ChunkState::Size { acc, digits: true, gap, ext, invalid }
                                }
                                _ => ChunkState::Size { acc, digits, gap, ext, invalid: true },
                            }
                        }
                        _ => ChunkState::Size { acc, digits, gap, ext, invalid: true },
                    },
                },
                ChunkState::DataEnd { cr } => match b {
                    b'\r' if !cr => ChunkState::DataEnd { cr: true },
                    b'\n' => CHUNK_START,
                    _ => return Err(StreamError::BadResponse),
                },
                ChunkState::Trailer { line_len } => match b {
                    b'\r' => self.state,
                    b'\n' if line_len == 0 => ChunkState::Done,
                    b'\n' => ChunkState::Trailer { line_len: 0 },
                    _ => ChunkState::Trailer { line_len: line_len + 1 },
                },
                ChunkState::Done => ChunkState::Done,
                ChunkState::Data(_) => unreachable!("handled above"),
            };
        }
        Ok(w)
    }

    /// The zero-size last chunk was seen (its trailer may still be incomplete).
    pub fn last_chunk_seen(&self) -> bool {
        matches!(self.state, ChunkState::Trailer { .. } | ChunkState::Done)
    }

    /// The response body is complete: last chunk and the empty line ending its trailer.
    pub fn is_done(&self) -> bool {
        self.state == ChunkState::Done
    }
}

// ---------------------------------------------------------------------------
// IcyStripper
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IcyState {
    /// Audio bytes; `IcyStripper::remaining` counts down to the next length byte.
    Audio,
    /// The next byte is a metadata length (in 16-byte units).
    MetaLen,
    /// Inside a metadata block, this many bytes left.
    Meta { remaining: usize },
}

/// Removes ICY metadata from an audio byte stream.
///
/// With an interval `n` (`icy-metaint`), the stream is `n` audio bytes, one length byte `L`,
/// `L * 16` metadata bytes, `n` audio bytes, ... A block with `L = 0` carries no metadata. The
/// most recent complete, non-empty block (trailing NUL padding removed) is kept until taken;
/// partial blocks are never exposed. Interval 0 disables stripping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IcyStripper {
    metaint: usize,
    remaining: usize,
    state: IcyState,
    accum: Vec<u8>,
    ready: Option<Vec<u8>>,
}

impl Default for IcyStripper {
    fn default() -> Self {
        Self::new()
    }
}

impl IcyStripper {
    /// Stripper with stripping disabled (interval 0).
    pub const fn new() -> Self {
        Self { metaint: 0, remaining: 0, state: IcyState::Audio, accum: Vec::new(), ready: None }
    }

    /// Sets the interval and restarts counting at the beginning of an audio run (`0` disables
    /// stripping). A partial block in progress is dropped; a complete one stays available.
    pub fn set_metaint(&mut self, interval: usize) {
        self.metaint = interval;
        self.remaining = interval;
        self.state = IcyState::Audio;
        self.accum.clear();
    }

    /// Current interval (`0` = disabled).
    pub fn metaint(&self) -> usize {
        self.metaint
    }

    /// Strips metadata from `buf` in place: the audio bytes are moved to the front of `buf` and
    /// their count returned.
    pub fn strip_in_place(&mut self, buf: &mut [u8]) -> usize {
        if self.metaint == 0 {
            return buf.len();
        }
        let (mut r, mut w) = (0usize, 0usize);
        while r < buf.len() {
            match self.state {
                IcyState::Audio => {
                    let take = self.remaining.min(buf.len() - r);
                    buf.copy_within(r..r + take, w);
                    r += take;
                    w += take;
                    self.remaining -= take;
                    if self.remaining == 0 {
                        self.state = IcyState::MetaLen;
                    }
                }
                IcyState::MetaLen => {
                    let len = buf[r] as usize * 16;
                    r += 1;
                    if len == 0 {
                        self.remaining = self.metaint;
                        self.state = IcyState::Audio;
                    } else {
                        self.accum.clear();
                        self.state = IcyState::Meta { remaining: len };
                    }
                }
                IcyState::Meta { remaining } => {
                    let take = remaining.min(buf.len() - r);
                    self.accum.extend_from_slice(&buf[r..r + take]);
                    r += take;
                    if take == remaining {
                        self.finish_block();
                        self.remaining = self.metaint;
                        self.state = IcyState::Audio;
                    } else {
                        self.state = IcyState::Meta { remaining: remaining - take };
                    }
                }
            }
        }
        w
    }

    /// A metadata block is complete: keep it (NUL padding removed) unless it is empty.
    fn finish_block(&mut self) {
        let end = self.accum.iter().rposition(|&c| c != 0).map_or(0, |i| i + 1);
        if end > 0 {
            self.accum.truncate(end);
            self.ready = Some(core::mem::take(&mut self.accum));
        } else {
            self.accum.clear();
        }
    }

    /// Most recent complete metadata block, if one arrived since the last take.
    pub fn metadata(&self) -> Option<&[u8]> {
        self.ready.as_deref()
    }

    /// Takes the most recent complete metadata block.
    pub fn take_metadata(&mut self) -> Option<Vec<u8>> {
        self.ready.take()
    }

    /// Drops the stored block, any partial block and the interval (connection closed).
    pub fn clear(&mut self) {
        *self = Self::new();
    }
}

// ---------------------------------------------------------------------------
// RedirectPolicy
// ---------------------------------------------------------------------------

/// Where a `Location` header points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedirectTarget<'a> {
    /// `/path`: same server, new path.
    SameOrigin { path: &'a [u8] },
    /// `http://authority/path` (`path` is `/` when the URL has none).
    Absolute { authority: &'a [u8], path: &'a [u8] },
}

/// Redirect handling shared by the stream sources: `301/302/303/307/308` are followed up to
/// [`MAX_REDIRECTS`] hops; the next one is [`StreamError::TooManyRedirects`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RedirectPolicy {
    followed: usize,
}

impl RedirectPolicy {
    /// Policy with no hop followed yet.
    pub const fn new() -> Self {
        Self { followed: 0 }
    }

    /// Whether `status` is a redirect this policy follows.
    pub fn is_redirect(status: u16) -> bool {
        matches!(status, 301 | 302 | 303 | 307 | 308)
    }

    /// Hops followed so far.
    pub fn followed(&self) -> usize {
        self.followed
    }

    /// Accounts for one more hop: [`StreamError::TooManyRedirects`] when [`MAX_REDIRECTS`]
    /// hops were already followed.
    pub fn follow(&mut self) -> Result<(), StreamError> {
        if self.followed >= MAX_REDIRECTS {
            return Err(StreamError::TooManyRedirects);
        }
        self.followed += 1;
        Ok(())
    }

    /// Classifies a `Location` value (surrounding blanks and quotes ignored): `/path` (not
    /// `//...`) is same-origin, `http://...` (any case) is absolute, anything else is
    /// [`StreamError::BadResponse`].
    pub fn classify(location: &[u8]) -> Result<RedirectTarget<'_>, StreamError> {
        let loc = trim_quotes(trim(location));
        if loc.starts_with(b"/") && !loc.starts_with(b"//") {
            return Ok(RedirectTarget::SameOrigin { path: loc });
        }
        const SCHEME: &[u8] = b"http://";
        if loc.len() >= SCHEME.len() && loc[..SCHEME.len()].eq_ignore_ascii_case(SCHEME) {
            let rest = &loc[SCHEME.len()..];
            let (authority, path) = match rest.iter().position(|&b| b == b'/') {
                Some(i) => (&rest[..i], &rest[i..]),
                None => (rest, &b"/"[..]),
            };
            if !authority.is_empty() {
                return Ok(RedirectTarget::Absolute { authority, path });
            }
        }
        Err(StreamError::BadResponse)
    }

    /// The request for the next hop: `template`'s request line with `path` as its target, every
    /// other non-empty line kept (trimmed) except `Host`, which becomes `Host: host` (appended
    /// when the template had none); lines joined by `\r\n` and ended by a blank line. An empty
    /// template yields `GET <path> HTTP/1.0` + `Host`.
    pub fn rewrite_request(template: &[u8], host: &[u8], path: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(template.len() + host.len() + path.len() + 32);
        let push_host = |out: &mut Vec<u8>| {
            out.extend_from_slice(b"Host: ");
            out.extend_from_slice(host);
        };
        if template.is_empty() {
            out.extend_from_slice(b"GET ");
            out.extend_from_slice(path);
            out.extend_from_slice(b" HTTP/1.0\r\n");
            push_host(&mut out);
            out.extend_from_slice(b"\r\n\r\n");
            return out;
        }
        let mut first = true;
        let mut host_found = false;
        for line in template.split(|&b| b == b'\n').map(trim).filter(|l| !l.is_empty()) {
            if !out.is_empty() {
                out.extend_from_slice(b"\r\n");
            }
            if first {
                first = false;
                let mut parts = line.split(|b| b.is_ascii_whitespace()).filter(|p| !p.is_empty());
                let method = parts.next().unwrap_or(b"GET");
                let _target = parts.next();
                let proto = parts.next().unwrap_or(b"HTTP/1.0");
                out.extend_from_slice(method);
                out.push(b' ');
                out.extend_from_slice(path);
                out.push(b' ');
                out.extend_from_slice(proto);
            } else if line.len() >= 5 && line[..5].eq_ignore_ascii_case(b"host:") {
                push_host(&mut out);
                host_found = true;
            } else {
                out.extend_from_slice(line);
            }
        }
        if !host_found {
            if !out.is_empty() {
                out.extend_from_slice(b"\r\n");
            }
            push_host(&mut out);
        }
        out.extend_from_slice(b"\r\n\r\n");
        out
    }
}

/// Parses an `ipv4[:port]` authority (port 80 when absent). Host names are not resolved here
/// (`None`).
pub fn parse_ipv4_authority(authority: &[u8]) -> Option<NetAddr> {
    let (host, port) = match authority.iter().position(|&b| b == b':') {
        Some(i) => (&authority[..i], u16::try_from(parse_decimal(&authority[i + 1..])?).ok()?),
        None => (authority, 80),
    };
    let mut ip = [0u8; 4];
    let mut parts = host.split(|&b| b == b'.');
    for octet in ip.iter_mut() {
        *octet = u8::try_from(parse_decimal(parts.next()?)?).ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(NetAddr { ip, port })
}

/// Unsigned decimal (digits only, at least one).
fn parse_decimal(s: &[u8]) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    s.iter().try_fold(0u64, |acc, &b| {
        if b.is_ascii_digit() {
            acc.checked_mul(10)?.checked_add(u64::from(b - b'0'))
        } else {
            None
        }
    })
}

fn trim(mut s: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = s {
        if first.is_ascii_whitespace() {
            s = rest;
        } else {
            break;
        }
    }
    while let [rest @ .., last] = s {
        if last.is_ascii_whitespace() {
            s = rest;
        } else {
            break;
        }
    }
    s
}

/// Strips one layer of surrounding `"` / `'` quotes and the blanks inside them.
fn trim_quotes(s: &[u8]) -> &[u8] {
    let mut s = s;
    while let [b'"' | b'\'', rest @ ..] = s {
        s = rest;
    }
    while let [rest @ .., b'"' | b'\''] = s {
        s = rest;
    }
    trim(s)
}

/// Value of header `name` (case-insensitive) in a raw head or request (first line skipped).
fn header_in<'a>(raw: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    raw.split(|&b| b == b'\n').skip(1).find_map(|line| {
        let colon = line.iter().position(|&b| b == b':')?;
        trim(&line[..colon]).eq_ignore_ascii_case(name).then(|| trim(&line[colon + 1..]))
    })
}

// ---------------------------------------------------------------------------
// Response head
// ---------------------------------------------------------------------------

/// Length of the response head at the start of `buf` (through the first `\r\n\r\n` or `\n\n`,
/// whichever ends first), or `None` while it is incomplete.
pub fn find_head_end(buf: &[u8]) -> Option<usize> {
    (1..buf.len()).find_map(|i| {
        if buf[i] != b'\n' {
            return None;
        }
        let blank_line = buf[i - 1] == b'\n' || (i >= 3 && &buf[i - 3..i] == b"\r\n\r");
        blank_line.then_some(i + 1)
    })
}

/// Read-only view of a raw HTTP response head.
#[derive(Clone, Copy, Debug)]
pub struct ResponseHead<'a>(pub &'a [u8]);

impl<'a> ResponseHead<'a> {
    /// Status code of an `HTTP/1.x` or `ICY` status line.
    pub fn status(&self) -> Option<u16> {
        let line = self.0.split(|&b| b == b'\n').next()?;
        if !(line.starts_with(b"HTTP/1.") || line.starts_with(b"ICY ")) {
            return None;
        }
        let code = line.split(|b| b.is_ascii_whitespace()).filter(|t| !t.is_empty()).nth(1)?;
        u16::try_from(parse_decimal(code)?).ok()
    }

    /// Value of header `name` (case-insensitive), blanks trimmed.
    pub fn header(&self, name: &[u8]) -> Option<&'a [u8]> {
        header_in(self.0, name)
    }

    /// `Transfer-Encoding` names `chunked` (any case).
    pub fn is_chunked(&self) -> bool {
        self.header(b"transfer-encoding")
            .is_some_and(|v| v.windows(7).any(|w| w.eq_ignore_ascii_case(b"chunked")))
    }

    /// `icy-metaint` interval, if present and numeric.
    pub fn icy_metaint(&self) -> Option<usize> {
        usize::try_from(parse_decimal(self.header(b"icy-metaint")?)?).ok()
    }

    /// `Content-Length`, if present and numeric.
    pub fn content_length(&self) -> Option<u64> {
        parse_decimal(self.header(b"content-length")?)
    }
}

/// Value of the `Host` header in a raw request, blanks trimmed (`None` if absent or empty).
pub fn request_host(request: &[u8]) -> Option<&[u8]> {
    header_in(request, b"host").filter(|h| !h.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io_traits::MAX_ICY_METADATA;
    use std::vec;

    /// Feeds `input` to a fresh decoder in pieces of `step` bytes, collecting the payload.
    fn dechunk(input: &[u8], step: usize) -> (Result<Vec<u8>, StreamError>, ChunkedDecoder) {
        let mut d = ChunkedDecoder::new();
        let mut out = Vec::new();
        for piece in input.chunks(step) {
            let mut buf = piece.to_vec();
            match d.decode_in_place(&mut buf) {
                Ok(n) => out.extend_from_slice(&buf[..n]),
                Err(e) => return (Err(e), d),
            }
        }
        (Ok(out), d)
    }

    #[test]
    fn chunked_decodes_split_at_every_boundary() {
        let wire = b"10\r\n0123456789ABCDEF\r\n8;name=v\r\nabcdefgh\r\n3\r\nXYZ\r\n0\r\nX-T: 1\r\n\r\n";
        for step in 1..=wire.len() {
            let (out, d) = dechunk(wire, step);
            assert_eq!(out.unwrap(), b"0123456789ABCDEFabcdefghXYZ", "step {step}");
            assert!(d.is_done() && d.last_chunk_seen(), "step {step}");
        }
    }

    #[test]
    fn chunked_accepts_bare_lf_blanks_and_upper_hex() {
        let (out, d) = dechunk(b" A \nabcdefghij\n0\n\n", 3);
        assert_eq!(out.unwrap(), b"abcdefghij");
        assert!(d.is_done());
    }

    #[test]
    fn chunked_last_chunk_without_trailer_end_is_seen_not_done() {
        let (out, d) = dechunk(b"2\r\nhi\r\n0\r\n", 4);
        assert_eq!(out.unwrap(), b"hi");
        assert!(d.last_chunk_seen());
        assert!(!d.is_done());
    }

    #[test]
    fn chunked_ignores_bytes_after_done() {
        let (out, d) = dechunk(b"1\r\nA\r\n0\r\n\r\ngarbage", 64);
        assert_eq!(out.unwrap(), b"A");
        assert!(d.is_done());
    }

    #[test]
    fn chunked_framing_errors_are_bad_response() {
        for bad in [
            &b"zz\r\n"[..],          // not hex
            b"\r\n",                 // empty size line
            b"1 2\r\n",              // blank inside the size
            b"2\r\nabX",             // no CRLF after the data
            b"200001\r\n",           // over MAX_CHUNK_SIZE
            b"ffffffffffffffffff\r\n", // overflow
        ] {
            for step in [1, 64] {
                let (out, _) = dechunk(bad, step);
                assert_eq!(out, Err(StreamError::BadResponse), "{bad:?} step {step}");
            }
        }
    }

    #[test]
    fn chunked_bad_size_line_is_only_judged_once_complete() {
        // A size line still waiting for its newline is not an error yet.
        let (out, d) = dechunk(b"partial_chunk_with_no_crlf", 5);
        assert_eq!(out.unwrap(), b"");
        assert!(!d.last_chunk_seen());
    }

    fn strip(s: &mut IcyStripper, input: &[u8], step: usize) -> (Vec<u8>, Vec<Vec<u8>>) {
        let mut audio = Vec::new();
        let mut metas = Vec::new();
        for piece in input.chunks(step) {
            let mut buf = piece.to_vec();
            let n = s.strip_in_place(&mut buf);
            audio.extend_from_slice(&buf[..n]);
            if let Some(m) = s.take_metadata() {
                metas.push(m);
            }
        }
        (audio, metas)
    }

    const ICY_WIRE: &[u8] = b"AAAAAAAAAAAAAAAA\x02StreamTitle='Song One';\0\0\0\0\0\0\0\0\0BBBBBBBBBBBBBBBB\0CCCCCCCCCCCCCCCC";

    #[test]
    fn icy_strips_blocks_split_at_every_boundary() {
        for step in 1..=ICY_WIRE.len() {
            let mut s = IcyStripper::new();
            s.set_metaint(16);
            let (audio, metas) = strip(&mut s, ICY_WIRE, step);
            assert_eq!(audio, b"AAAAAAAAAAAAAAAABBBBBBBBBBBBBBBBCCCCCCCCCCCCCCCC", "step {step}");
            assert_eq!(metas, vec![b"StreamTitle='Song One';".to_vec()], "step {step}");
        }
    }

    #[test]
    fn icy_disabled_is_passthrough() {
        let mut s = IcyStripper::new();
        let (audio, metas) = strip(&mut s, ICY_WIRE, 7);
        assert_eq!(audio, ICY_WIRE);
        assert!(metas.is_empty());
    }

    #[test]
    fn icy_all_nul_block_is_not_metadata_and_latest_block_wins() {
        let mut s = IcyStripper::new();
        s.set_metaint(2);
        let mut buf = b"ab\x01one\0\0\0\0\0\0\0\0\0\0\0\0\0cd\x01\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0ef\x01two\0\0\0\0\0\0\0\0\0\0\0\0\0gh".to_vec();
        let n = s.strip_in_place(&mut buf);
        assert_eq!(&buf[..n], b"abcdefgh");
        assert_eq!(s.metadata(), Some(&b"two"[..]));
        assert_eq!(s.take_metadata(), Some(b"two".to_vec()));
        assert_eq!(s.take_metadata(), None);
    }

    #[test]
    fn icy_partial_block_is_never_exposed_and_set_metaint_restarts() {
        let mut s = IcyStripper::new();
        s.set_metaint(2);
        let mut buf = b"ab\x01Stre".to_vec();
        assert_eq!(s.strip_in_place(&mut buf), 2);
        assert_eq!(s.metadata(), None);
        // Re-arming restarts at an audio run of the new interval.
        s.set_metaint(3);
        let mut buf = b"xyz\0uvw".to_vec();
        let n = s.strip_in_place(&mut buf);
        assert_eq!(&buf[..n], b"xyzuvw");
        assert_eq!(s.metaint(), 3);
        s.clear();
        assert_eq!(s.metaint(), 0);
    }

    #[test]
    fn icy_max_block_fits_capacity() {
        let mut s = IcyStripper::new();
        s.set_metaint(1);
        let mut wire = vec![b'a', 0xFF];
        wire.extend(core::iter::repeat_n(b'm', 255 * 16));
        wire.push(b'b');
        let n = s.strip_in_place(&mut wire);
        assert_eq!(&wire[..n], b"ab");
        let block = s.take_metadata().unwrap();
        assert_eq!(block.len(), 4080);
        assert!(block.len() <= MAX_ICY_METADATA, "every ICY block fits a MetaBuf");
    }

    #[test]
    fn redirect_policy_follows_three_hops_then_refuses() {
        for status in [301u16, 302, 303, 307, 308] {
            assert!(RedirectPolicy::is_redirect(status), "{status}");
        }
        for status in [200u16, 206, 300, 304, 404] {
            assert!(!RedirectPolicy::is_redirect(status), "{status}");
        }
        let mut p = RedirectPolicy::new();
        for hop in 1..=MAX_REDIRECTS {
            assert_eq!(p.follow(), Ok(()));
            assert_eq!(p.followed(), hop);
        }
        assert_eq!(p.follow(), Err(StreamError::TooManyRedirects));
    }

    #[test]
    fn redirect_location_classification() {
        assert_eq!(
            RedirectPolicy::classify(b" /a/b?c=1 "),
            Ok(RedirectTarget::SameOrigin { path: b"/a/b?c=1" })
        );
        assert_eq!(
            RedirectPolicy::classify(b"\"HTTP://10.0.0.2:9000/s.mp3\""),
            Ok(RedirectTarget::Absolute { authority: b"10.0.0.2:9000", path: b"/s.mp3" })
        );
        assert_eq!(
            RedirectPolicy::classify(b"http://host"),
            Ok(RedirectTarget::Absolute { authority: b"host", path: b"/" })
        );
        for bad in [&b"//host/x"[..], b"https://h/x", b"ftp://h/x", b"relative/x", b""] {
            assert_eq!(RedirectPolicy::classify(bad), Err(StreamError::BadResponse), "{bad:?}");
        }
    }

    #[test]
    fn redirect_request_rewrite_matches_legacy_format() {
        assert_eq!(
            RedirectPolicy::rewrite_request(
                b"GET /old.pcm HTTP/1.1\r\nHost: 127.0.0.1\r\nIcy-MetaData: 1\r\n\r\n",
                b"10.0.0.2:9000",
                b"/new.pcm",
            ),
            b"GET /new.pcm HTTP/1.1\r\nHost: 10.0.0.2:9000\r\nIcy-MetaData: 1\r\n\r\n".to_vec()
        );
        // Missing Host is appended; blank lines dropped; bare-LF template accepted.
        assert_eq!(
            RedirectPolicy::rewrite_request(b"GET /x HTTP/1.0\n\nUser-Agent: t\n", b"h:1", b"/y"),
            b"GET /y HTTP/1.0\r\nUser-Agent: t\r\nHost: h:1\r\n\r\n".to_vec()
        );
        assert_eq!(
            RedirectPolicy::rewrite_request(b"", b"h", b"/p"),
            b"GET /p HTTP/1.0\r\nHost: h\r\n\r\n".to_vec()
        );
    }

    #[test]
    fn ipv4_authority_parsing() {
        assert_eq!(parse_ipv4_authority(b"10.0.0.2:9000"), Some(NetAddr { ip: [10, 0, 0, 2], port: 9000 }));
        assert_eq!(parse_ipv4_authority(b"192.168.1.20"), Some(NetAddr { ip: [192, 168, 1, 20], port: 80 }));
        for bad in [&b"host:80"[..], b"1.2.3:80", b"1.2.3.4.5", b"256.1.1.1", b"1.2.3.4:", b"1.2.3.4:x", b"1.2.3.4:70000"] {
            assert_eq!(parse_ipv4_authority(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn head_end_and_header_lookup() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: Chunked\r\nicy-metaint:  8192 \r\nContent-Length: 12\r\n\r\nBODY";
        let end = find_head_end(raw).unwrap();
        assert_eq!(&raw[end..], b"BODY");
        let head = ResponseHead(&raw[..end]);
        assert_eq!(head.status(), Some(200));
        assert!(head.is_chunked());
        assert_eq!(head.icy_metaint(), Some(8192));
        assert_eq!(head.content_length(), Some(12));
        assert_eq!(head.header(b"TRANSFER-ENCODING"), Some(&b"Chunked"[..]));
        assert_eq!(head.header(b"location"), None);

        assert_eq!(find_head_end(b"ICY 200 OK\n\nxx"), Some(12));
        assert_eq!(ResponseHead(b"ICY 200 OK\n\n").status(), Some(200));
        assert_eq!(find_head_end(b"HTTP/1.0 200 OK\r\nA: b\r\n"), None);
        // Only the status line decides the status.
        assert_eq!(ResponseHead(b"HTTP/1.0 404 Not Found\r\nX-RateLimit: 200\r\n\r\n").status(), Some(404));
        assert_eq!(ResponseHead(b"FOO 200 OK\r\n\r\n").status(), None);
        assert!(!ResponseHead(b"HTTP/1.0 200 OK\r\n\r\n").is_chunked());
    }

    #[test]
    fn request_host_lookup() {
        assert_eq!(request_host(b"GET / HTTP/1.0\r\nhost:  10.0.0.1:9000 \r\n\r\n"), Some(&b"10.0.0.1:9000"[..]));
        assert_eq!(request_host(b"GET / HTTP/1.0\r\n\r\n"), None);
        assert_eq!(request_host(b"GET / HTTP/1.0\r\nHost:\r\n\r\n"), None);
    }
}
