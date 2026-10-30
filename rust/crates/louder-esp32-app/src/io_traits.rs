//! I/O seams for the player core: Slimproto transport, HTTP audio stream source, and audio
//! backend traits, plus `no_std` fakes and a reusable conformance suite.
//!
//! The module is `no_std`-clean (it uses `core` and `heapless` only) so it builds for both the
//! host and `xtensa-esp32-none-elf`. Host adapters (`std::net`) and device adapters (smoltcp)
//! implement the traits; [`LoopbackTransport`], [`FakeStreamSource`] and [`MockBackend`] are the
//! in-memory implementations used by tests. [`run_stream_source_conformance`] pins the
//! behavioural contract of [`StreamSource`] so every implementation can be checked against the
//! same cases.

pub mod conformance;

pub use conformance::run_stream_source_conformance;

use heapless::{Deque, Vec};

/// IPv4 socket address (the player is IPv4-only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetAddr {
    pub ip: [u8; 4],
    pub port: u16,
}

/// Errors from a [`SlimTransport`].
#[derive(Debug, PartialEq)]
pub enum TransportError {
    /// The peer closed the connection (or it was never established).
    Disconnected,
    /// The operation cannot make progress right now (e.g. send queue full); retry later.
    WouldBlock,
    /// Any other I/O failure, including a frame that does not fit the caller's buffer.
    Io,
}

/// Errors from a [`StreamSource`].
#[derive(Debug, PartialEq)]
pub enum StreamError {
    /// The TCP connection could not be established.
    ConnectFailed,
    /// The connection dropped before the response body completed.
    Disconnected,
    /// More than [`MAX_REDIRECTS`] redirects were followed.
    TooManyRedirects,
    /// The response was not usable (non-2xx status, malformed headers or chunk framing, ...).
    BadResponse,
    /// Connect phase only: the connection is not established yet; call `connect` again.
    WouldBlock,
}

/// Errors from an [`AudioBackend`].
#[derive(Debug, PartialEq)]
pub enum ClockError {
    /// The hardware cannot generate this sample rate.
    UnsupportedRate(u32),
}

/// Maximum number of HTTP redirects a [`StreamSource`] follows; the next one yields
/// [`StreamError::TooManyRedirects`].
pub const MAX_REDIRECTS: usize = 3;

/// Capacity of a captured HTTP response head (C squeezelite `MAX_HEADER` parity).
pub const MAX_RESPONSE_HEAD: usize = 4096;
/// Capacity of an ICY metadata block; the largest block ICY can carry is 255 * 16 = 4080 bytes.
pub const MAX_ICY_METADATA: usize = 4096;

/// Raw HTTP response head as returned by [`StreamSource::take_response_headers`].
pub type HeaderBuf = Vec<u8, MAX_RESPONSE_HEAD>;
/// ICY metadata block as returned by [`StreamSource::take_icy_metadata`].
pub type MetaBuf = Vec<u8, MAX_ICY_METADATA>;

/// Message-framed Slimproto control connection (TCP port 3483 to the server).
///
/// Length-prefix framing is the adapter's concern: `poll_frame` yields one complete Slimproto
/// frame per call.
pub trait SlimTransport {
    /// Returns `Ok(Some(n))` when one complete frame of `n` bytes (possibly 0) was copied into
    /// `buf`, `Ok(None)` when no complete frame is available right now, and an error otherwise.
    /// A frame larger than `buf` is dropped and reported as [`TransportError::Io`]. Frames that
    /// were already received remain deliverable after the peer disconnects; once drained,
    /// [`TransportError::Disconnected`] is returned.
    fn poll_frame(&mut self, buf: &mut [u8]) -> Result<Option<usize>, TransportError>;
    /// Queues one frame for sending.
    fn send(&mut self, frame: &[u8]) -> Result<(), TransportError>;
    /// Whether the underlying connection is currently established.
    fn connected(&self) -> bool;
}

/// Non-blocking HTTP audio stream client.
///
/// Conventions (checked by [`run_stream_source_conformance`]):
/// * `connect` sends `http_request` verbatim to `addr`, reads the response headers, follows up
///   to [`MAX_REDIRECTS`] redirects (`301/302/303/307/308`, same-origin or absolute `Location`)
///   and succeeds only on `200`/`206`. It may return [`StreamError::WouldBlock`] while the
///   connection is still being established; the caller then repeats the same `connect` call.
/// * `poll_bytes` never blocks and never returns `WouldBlock`. `Ok(0)` means "no audio bytes
///   available right now", which is *not* end of stream: use [`StreamSource::is_eof`].
///   `poll_bytes` yields de-chunked audio with any ICY metadata blocks stripped.
/// * `is_eof` is `true` only once the response ended cleanly and every byte has been delivered.
///   A connection that drops early is reported as [`StreamError::Disconnected`] instead.
/// * `take_response_headers` returns the raw headers of the final (non-redirect) response,
///   including the terminating blank line, once (at most [`MAX_RESPONSE_HEAD`] bytes; a longer
///   head is cut at a line boundary and re-terminated).
/// * `take_icy_metadata` returns the most recent complete metadata block (trailing NUL padding
///   removed, at most [`MAX_ICY_METADATA`] bytes, so every ICY block fits), once. Partial blocks are never exposed. An
///   `icy-metaint` response header configures the interval; `set_icy_metaint` overrides it
///   (`0` disables stripping).
/// * After `abort`, `poll_bytes` returns [`StreamError::Disconnected`] until the next `connect`.
pub trait StreamSource {
    fn connect(&mut self, addr: NetAddr, http_request: &[u8]) -> Result<(), StreamError>;
    fn poll_bytes(&mut self, out: &mut [u8]) -> Result<usize, StreamError>;
    fn take_response_headers(&mut self) -> Option<HeaderBuf>;
    fn take_icy_metadata(&mut self) -> Option<MetaBuf>;
    fn set_icy_metaint(&mut self, interval: usize);
    fn is_eof(&self) -> bool;
    fn abort(&mut self);
}

/// Audio output hardware (I2S clock, amplifier volume/mute/power, DMA state).
pub trait AudioBackend {
    fn set_sample_rate(&mut self, hz: u32) -> Result<(), ClockError>;
    fn set_hw_volume(&mut self, left: u8, right: u8);
    fn set_mute(&mut self, mute: bool);
    fn set_power(&mut self, on: bool);
    fn dma_outstanding_frames(&self) -> u32;
}

// ---------------------------------------------------------------------------
// LoopbackTransport
// ---------------------------------------------------------------------------

/// Largest frame a [`LoopbackTransport`] can hold: a `RESP`/`META` frame carrying a full
/// [`MAX_RESPONSE_HEAD`] / [`MAX_ICY_METADATA`] payload behind its 8-byte header.
pub const LOOPBACK_FRAME_CAP: usize = 8 + MAX_RESPONSE_HEAD;
/// Number of frames each direction of a [`LoopbackTransport`] can queue.
pub const LOOPBACK_QUEUE_DEPTH: usize = 8;

/// One queued frame.
pub type Frame = Vec<u8, LOOPBACK_FRAME_CAP>;

/// In-memory [`SlimTransport`]: frames injected by a test come out of `poll_frame`, frames
/// passed to `send` are recorded for [`LoopbackTransport::pop_sent`]. In echo mode (see
/// [`LoopbackTransport::echo`]) every sent frame is additionally delivered back to `poll_frame`.
///
/// Compiled in all builds; uses fixed heapless queues, no allocation.
pub struct LoopbackTransport {
    rx: Deque<Frame, LOOPBACK_QUEUE_DEPTH>,
    tx: Deque<Frame, LOOPBACK_QUEUE_DEPTH>,
    connected: bool,
    echo: bool,
}

impl LoopbackTransport {
    /// Connected transport with independent receive and send queues.
    pub const fn new() -> Self {
        Self { rx: Deque::new(), tx: Deque::new(), connected: true, echo: false }
    }

    /// Connected transport that also feeds every sent frame back into the receive queue.
    pub const fn echo() -> Self {
        Self { rx: Deque::new(), tx: Deque::new(), connected: true, echo: true }
    }

    /// Queues a frame to be returned by `poll_frame` (as if received from the server).
    pub fn inject_frame(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        let f = Frame::from_slice(frame).map_err(|_| TransportError::Io)?;
        self.rx.push_back(f).map_err(|_| TransportError::WouldBlock)
    }

    /// Takes the oldest frame the code under test has sent, if any.
    pub fn pop_sent(&mut self) -> Option<Frame> {
        self.tx.pop_front()
    }

    /// Number of sent frames not yet taken with [`LoopbackTransport::pop_sent`].
    pub fn sent_len(&self) -> usize {
        self.tx.len()
    }

    /// Simulates the peer closing the connection.
    pub fn disconnect(&mut self) {
        self.connected = false;
    }

    /// Re-establishes the connection (queued frames are kept).
    pub fn reconnect(&mut self) {
        self.connected = true;
    }
}

impl Default for LoopbackTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl SlimTransport for LoopbackTransport {
    fn poll_frame(&mut self, buf: &mut [u8]) -> Result<Option<usize>, TransportError> {
        match self.rx.pop_front() {
            Some(frame) => {
                if frame.len() > buf.len() {
                    return Err(TransportError::Io);
                }
                buf[..frame.len()].copy_from_slice(&frame);
                Ok(Some(frame.len()))
            }
            None if self.connected => Ok(None),
            None => Err(TransportError::Disconnected),
        }
    }

    fn send(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        if !self.connected {
            return Err(TransportError::Disconnected);
        }
        let f = Frame::from_slice(frame).map_err(|_| TransportError::Io)?;
        if self.tx.is_full() || (self.echo && self.rx.is_full()) {
            return Err(TransportError::WouldBlock);
        }
        if self.echo {
            // Capacity checked above, so neither push can fail.
            let _ = self.rx.push_back(f.clone());
        }
        let _ = self.tx.push_back(f);
        Ok(())
    }

    fn connected(&self) -> bool {
        self.connected
    }
}

// ---------------------------------------------------------------------------
// MockBackend
// ---------------------------------------------------------------------------

/// [`AudioBackend`] that records what it was told. Accepts exactly the sample rates in
/// [`crate::clock_table::VERIFIED_RATES`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MockBackend {
    pub rate: u32,
    pub muted: bool,
    pub powered: bool,
    pub hw_vol: (u8, u8),
    pub outstanding: u32,
}

impl MockBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl AudioBackend for MockBackend {
    fn set_sample_rate(&mut self, hz: u32) -> Result<(), ClockError> {
        if crate::clock_table::i2s_clock_config(hz).is_none() {
            return Err(ClockError::UnsupportedRate(hz));
        }
        self.rate = hz;
        Ok(())
    }

    fn set_hw_volume(&mut self, left: u8, right: u8) {
        self.hw_vol = (left, right);
    }

    fn set_mute(&mut self, mute: bool) {
        self.muted = mute;
    }

    fn set_power(&mut self, on: bool) {
        self.powered = on;
    }

    fn dma_outstanding_frames(&self) -> u32 {
        self.outstanding
    }
}

// ---------------------------------------------------------------------------
// FakeStreamSource
// ---------------------------------------------------------------------------

/// Scripted server response: each segment models the bytes of one TCP read, so tests can split
/// headers, chunk framing and ICY blocks at arbitrary points.
pub type Segments = &'static [&'static [u8]];

#[derive(Clone, Copy)]
struct Route {
    path: &'static [u8],
    segments: Segments,
}

const MAX_ROUTES: usize = 16;
const MAX_CHUNK_SIZE: usize = 1_048_576;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Streaming,
    Done,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChunkState {
    Size { acc: usize, digits: bool, ext: bool },
    Data(usize),
    DataCr,
    Trailer { line_len: usize },
    End,
}

const CHUNK_START: ChunkState = ChunkState::Size { acc: 0, digits: false, ext: false };

#[derive(Clone, Copy, PartialEq, Eq)]
enum IcyState {
    Audio,
    MetaLen,
    Meta { remaining: usize },
}

/// In-memory [`StreamSource`] that plays back scripted HTTP responses selected by the request
/// path. It simulates redirect following, header capture, chunked decoding and ICY stripping at
/// wire level, delivering at most one scripted segment per `poll_bytes` call.
///
/// Unknown paths behave like a `404` ([`StreamError::BadResponse`]). The connect address is
/// recorded but otherwise ignored; `Location` values are resolved by path only.
pub struct FakeStreamSource {
    routes: Vec<Route, MAX_ROUTES>,
    /// When `true`, `connect` fails with [`StreamError::ConnectFailed`].
    pub fail_connect: bool,
    /// Address passed to the most recent `connect` call.
    pub last_addr: Option<NetAddr>,
    /// Number of `connect` calls so far.
    pub connects: u32,
    /// Redirects followed by the most recent successful `connect`.
    pub redirects_followed: usize,

    phase: Phase,
    segments: Segments,
    seg_idx: usize,
    seg_off: usize,
    headers: Option<HeaderBuf>,
    chunked: bool,
    chunk: ChunkState,
    icy_metaint: usize,
    icy_remaining: usize,
    icy_state: IcyState,
    icy_accum: MetaBuf,
    icy_meta: Option<MetaBuf>,
}

impl FakeStreamSource {
    /// Empty fake with no routes.
    pub const fn new() -> Self {
        Self {
            routes: Vec::new(),
            fail_connect: false,
            last_addr: None,
            connects: 0,
            redirects_followed: 0,
            phase: Phase::Idle,
            segments: &[],
            seg_idx: 0,
            seg_off: 0,
            headers: None,
            chunked: false,
            chunk: CHUNK_START,
            icy_metaint: 0,
            icy_remaining: 0,
            icy_state: IcyState::Audio,
            icy_accum: Vec::new(),
            icy_meta: None,
        }
    }

    /// Fake serving [`conformance::ROUTES`], the data [`run_stream_source_conformance`] expects.
    pub fn with_conformance_routes() -> Self {
        let mut s = Self::new();
        for &(path, segments) in conformance::ROUTES {
            assert!(s.add_route(path, segments), "route table full");
        }
        s
    }

    /// Scripts the response for requests to `path`. Returns `false` if the route table is full.
    pub fn add_route(&mut self, path: &'static [u8], segments: Segments) -> bool {
        self.routes.push(Route { path, segments }).is_ok()
    }

    fn reset_stream(&mut self) {
        self.phase = Phase::Idle;
        self.segments = &[];
        self.seg_idx = 0;
        self.seg_off = 0;
        self.headers = None;
        self.chunked = false;
        self.chunk = CHUNK_START;
        self.icy_metaint = 0;
        self.icy_remaining = 0;
        self.icy_state = IcyState::Audio;
        self.icy_accum.clear();
        self.icy_meta = None;
    }

    fn find_route(&self, path: &[u8]) -> Option<Route> {
        self.routes.iter().find(|r| r.path == path).copied()
    }

    /// One wire byte through the chunk decoder; `Some` is a payload byte.
    fn chunk_step(&mut self, b: u8) -> Result<Option<u8>, StreamError> {
        match self.chunk {
            ChunkState::Size { acc, digits, ext } => match b {
                b'\r' => {}
                b'\n' => {
                    if !digits {
                        return Err(StreamError::BadResponse);
                    }
                    self.chunk = if acc == 0 {
                        ChunkState::Trailer { line_len: 0 }
                    } else {
                        ChunkState::Data(acc)
                    };
                }
                b';' => self.chunk = ChunkState::Size { acc, digits, ext: true },
                _ if ext => {}
                _ => {
                    let d = (b as char).to_digit(16).ok_or(StreamError::BadResponse)? as usize;
                    let acc = acc.checked_mul(16).ok_or(StreamError::BadResponse)? + d;
                    if acc > MAX_CHUNK_SIZE {
                        return Err(StreamError::BadResponse);
                    }
                    self.chunk = ChunkState::Size { acc, digits: true, ext };
                }
            },
            ChunkState::Data(rem) => {
                self.chunk = if rem == 1 { ChunkState::DataCr } else { ChunkState::Data(rem - 1) };
                return Ok(Some(b));
            }
            ChunkState::DataCr => match b {
                b'\r' => {}
                b'\n' => self.chunk = CHUNK_START,
                _ => return Err(StreamError::BadResponse),
            },
            ChunkState::Trailer { line_len } => match b {
                b'\r' => {}
                b'\n' if line_len == 0 => self.chunk = ChunkState::End,
                b'\n' => self.chunk = ChunkState::Trailer { line_len: 0 },
                _ => self.chunk = ChunkState::Trailer { line_len: line_len + 1 },
            },
            ChunkState::End => {}
        }
        Ok(None)
    }

    /// One payload byte through the ICY stripper; `Some` is an audio byte.
    fn icy_step(&mut self, b: u8) -> Option<u8> {
        if self.icy_metaint == 0 {
            return Some(b);
        }
        match self.icy_state {
            IcyState::Audio => {
                self.icy_remaining = self.icy_remaining.saturating_sub(1);
                if self.icy_remaining == 0 {
                    self.icy_state = IcyState::MetaLen;
                }
                Some(b)
            }
            IcyState::MetaLen => {
                let len = b as usize * 16;
                if len == 0 {
                    self.icy_remaining = self.icy_metaint;
                    self.icy_state = IcyState::Audio;
                } else {
                    self.icy_accum.clear();
                    self.icy_state = IcyState::Meta { remaining: len };
                }
                None
            }
            IcyState::Meta { remaining } => {
                // Every ICY block (<= 4080 bytes) fits the capacity; push cannot fail.
                let _ = self.icy_accum.push(b);
                if remaining == 1 {
                    let end = self.icy_accum.iter().rposition(|&c| c != 0).map_or(0, |i| i + 1);
                    if end > 0 {
                        self.icy_meta = Vec::from_slice(&self.icy_accum[..end]).ok();
                    }
                    self.icy_remaining = self.icy_metaint;
                    self.icy_state = IcyState::Audio;
                } else {
                    self.icy_state = IcyState::Meta { remaining: remaining - 1 };
                }
                None
            }
        }
    }
}

impl FakeStreamSource {
    /// Request/redirect loop of `connect`. The response head is accumulated in `self.headers`
    /// (no head-sized stack buffer); the caller clears it on error.
    fn follow_routes(&mut self, http_request: &[u8]) -> Result<(), StreamError> {
        let mut path: Vec<u8, 128> =
            Vec::from_slice(request_path(http_request).ok_or(StreamError::BadResponse)?)
                .map_err(|_| StreamError::BadResponse)?;
        loop {
            let route = self.find_route(&path).ok_or(StreamError::BadResponse)?;

            // Read through the end of the header block.
            let hdr = self.headers.insert(HeaderBuf::new());
            let (mut idx, mut off) = (0usize, 0usize);
            'headers: loop {
                let seg = route.segments.get(idx).ok_or(StreamError::BadResponse)?;
                while off < seg.len() {
                    hdr.push(seg[off]).map_err(|_| StreamError::BadResponse)?;
                    off += 1;
                    if hdr.ends_with(b"\r\n\r\n") {
                        break 'headers;
                    }
                }
                idx += 1;
                off = 0;
            }

            match status_code(hdr).ok_or(StreamError::BadResponse)? {
                200 | 206 => {
                    let chunked = header_value(hdr, b"transfer-encoding")
                        .is_some_and(|v| v.eq_ignore_ascii_case(b"chunked"));
                    let metaint = header_value(hdr, b"icy-metaint").and_then(parse_usize);
                    self.chunked = chunked;
                    if let Some(m) = metaint {
                        self.set_icy_metaint(m);
                    }
                    self.segments = route.segments;
                    self.seg_idx = idx;
                    self.seg_off = off;
                    self.phase = Phase::Streaming;
                    return Ok(());
                }
                301 | 302 | 303 | 307 | 308 => {
                    let loc = header_value(hdr, b"location").ok_or(StreamError::BadResponse)?;
                    let next: Result<Vec<u8, 128>, _> = Vec::from_slice(location_path(loc));
                    self.redirects_followed += 1;
                    if self.redirects_followed > MAX_REDIRECTS {
                        return Err(StreamError::TooManyRedirects);
                    }
                    path = next.map_err(|_| StreamError::BadResponse)?;
                }
                _ => return Err(StreamError::BadResponse),
            }
        }
    }
}

impl Default for FakeStreamSource {
    fn default() -> Self {
        Self::new()
    }
}

/// Request path: second whitespace-separated token of the request line.
fn request_path(request: &[u8]) -> Option<&[u8]> {
    let line = request.split(|&b| b == b'\n').next()?;
    let mut tokens = line.split(|&b| b == b' ').filter(|t| !t.is_empty());
    tokens.next()?;
    tokens.next()
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

/// Value of header `name` (case-insensitive) in a raw header block, whitespace-trimmed.
fn header_value<'a>(headers: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    headers.split(|&b| b == b'\n').skip(1).find_map(|line| {
        let colon = line.iter().position(|&b| b == b':')?;
        trim(&line[..colon]).eq_ignore_ascii_case(name).then(|| trim(&line[colon + 1..]))
    })
}

fn parse_usize(s: &[u8]) -> Option<usize> {
    if s.is_empty() {
        return None;
    }
    s.iter().try_fold(0usize, |acc, &b| {
        if b.is_ascii_digit() {
            acc.checked_mul(10)?.checked_add(usize::from(b - b'0'))
        } else {
            None
        }
    })
}

fn status_code(headers: &[u8]) -> Option<u16> {
    let line = headers.split(|&b| b == b'\n').next()?;
    let mut tokens = line.split(|&b| b == b' ').filter(|t| !t.is_empty());
    tokens.next()?;
    u16::try_from(parse_usize(trim(tokens.next()?))?).ok()
}

/// Path part of a `Location` value: `/path` as-is, `http://host[:port]/path` reduced to `/path`
/// (`/` when the URL has no path).
fn location_path(loc: &[u8]) -> &[u8] {
    match loc.windows(3).position(|w| w == b"://") {
        Some(i) => {
            let rest = &loc[i + 3..];
            rest.iter().position(|&b| b == b'/').map_or(&b"/"[..], |p| &rest[p..])
        }
        None => loc,
    }
}

impl StreamSource for FakeStreamSource {
    fn connect(&mut self, addr: NetAddr, http_request: &[u8]) -> Result<(), StreamError> {
        self.reset_stream();
        self.connects += 1;
        self.last_addr = Some(addr);
        self.redirects_followed = 0;
        if self.fail_connect {
            return Err(StreamError::ConnectFailed);
        }
        let result = self.follow_routes(http_request);
        if result.is_err() {
            self.headers = None;
        }
        result
    }

    fn poll_bytes(&mut self, out: &mut [u8]) -> Result<usize, StreamError> {
        match self.phase {
            Phase::Idle => return Err(StreamError::Disconnected),
            Phase::Done => return Ok(0),
            Phase::Streaming => {}
        }
        if out.is_empty() {
            return Ok(0);
        }
        let Some(&seg) = self.segments.get(self.seg_idx) else {
            // Script exhausted: a clean end unless chunked framing never terminated.
            return if self.chunked && self.chunk != ChunkState::End {
                self.phase = Phase::Idle;
                Err(StreamError::Disconnected)
            } else {
                self.phase = Phase::Done;
                Ok(0)
            };
        };

        let mut n = 0;
        while self.seg_off < seg.len() && n < out.len() && self.chunk != ChunkState::End {
            let b = seg[self.seg_off];
            self.seg_off += 1;
            let payload = if self.chunked { self.chunk_step(b)? } else { Some(b) };
            if let Some(audio) = payload.and_then(|p| self.icy_step(p)) {
                out[n] = audio;
                n += 1;
            }
        }
        if self.seg_off >= seg.len() {
            self.seg_idx += 1;
            self.seg_off = 0;
        }
        let ended = if self.chunked {
            self.chunk == ChunkState::End
        } else {
            self.seg_idx >= self.segments.len()
        };
        if ended {
            self.phase = Phase::Done;
        }
        Ok(n)
    }

    fn take_response_headers(&mut self) -> Option<HeaderBuf> {
        self.headers.take()
    }

    fn take_icy_metadata(&mut self) -> Option<MetaBuf> {
        self.icy_meta.take()
    }

    fn set_icy_metaint(&mut self, interval: usize) {
        self.icy_metaint = interval;
        self.icy_remaining = interval;
        self.icy_state = IcyState::Audio;
    }

    fn is_eof(&self) -> bool {
        self.phase == Phase::Done
    }

    fn abort(&mut self) {
        self.reset_stream();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADDR: NetAddr = NetAddr { ip: [127, 0, 0, 1], port: 80 };

    fn read_all(s: &mut FakeStreamSource) -> std::vec::Vec<u8> {
        let mut body = std::vec::Vec::new();
        for _ in 0..64 {
            let mut buf = [0u8; 16];
            let n = s.poll_bytes(&mut buf).unwrap();
            body.extend_from_slice(&buf[..n]);
            if s.is_eof() {
                break;
            }
        }
        body
    }

    #[test]
    fn location_path_forms() {
        assert_eq!(location_path(b"/a/b"), b"/a/b");
        assert_eq!(location_path(b"http://h:1/a/b"), b"/a/b");
        assert_eq!(location_path(b"http://h"), b"/");
    }

    #[test]
    fn request_path_second_token() {
        assert_eq!(request_path(b"GET /x?y=1 HTTP/1.0\r\n\r\n"), Some(&b"/x?y=1"[..]));
        assert_eq!(request_path(b"GET"), None);
    }

    #[test]
    fn icy_metaint_header_is_honoured_without_set_call() {
        static SEGS: Segments = &[b"HTTP/1.0 200 OK\r\nIcy-MetaInt: 4\r\n\r\nabcd\x01Hi\0\0\0\0\0\0\0\0\0\0\0\0\0\0wxyz\0"];
        let mut s = FakeStreamSource::new();
        assert!(s.add_route(b"/i", SEGS));
        s.connect(ADDR, b"GET /i HTTP/1.0\r\n\r\n").unwrap();
        assert_eq!(read_all(&mut s), b"abcdwxyz");
        assert_eq!(s.take_icy_metadata().as_deref(), Some(&b"Hi"[..]));
    }

    #[test]
    fn chunked_with_icy_and_extension() {
        static SEGS: Segments = &[
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: Chunked\r\n\r\n6;ext=1\r\nabcdef\r\n",
            b"5\r\n\x00ghij\r\n0\r\nX-T: 1\r\n\r\n",
        ];
        let mut s = FakeStreamSource::new();
        assert!(s.add_route(b"/c", SEGS));
        s.connect(ADDR, b"GET /c HTTP/1.1\r\n\r\n").unwrap();
        s.set_icy_metaint(6);
        assert_eq!(read_all(&mut s), b"abcdefghij");
        assert!(s.is_eof());
    }

    #[test]
    fn truncated_chunked_stream_is_disconnected_not_eof() {
        static SEGS: Segments = &[b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n8\r\nabcd"];
        let mut s = FakeStreamSource::new();
        assert!(s.add_route(b"/t", SEGS));
        s.connect(ADDR, b"GET /t HTTP/1.1\r\n\r\n").unwrap();
        let mut buf = [0u8; 16];
        assert_eq!(s.poll_bytes(&mut buf), Ok(4));
        assert!(!s.is_eof());
        assert_eq!(s.poll_bytes(&mut buf), Err(StreamError::Disconnected));
    }

    #[test]
    fn bad_chunk_size_is_bad_response() {
        static SEGS: Segments = &[b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n"];
        let mut s = FakeStreamSource::new();
        assert!(s.add_route(b"/b", SEGS));
        s.connect(ADDR, b"GET /b HTTP/1.1\r\n\r\n").unwrap();
        let mut buf = [0u8; 16];
        assert_eq!(s.poll_bytes(&mut buf), Err(StreamError::BadResponse));
    }

    #[test]
    fn redirect_without_location_and_error_status_are_bad_response() {
        static NOLOC: Segments = &[b"HTTP/1.0 302 Found\r\n\r\n"];
        static GONE: Segments = &[b"HTTP/1.0 404 Not Found\r\n\r\n"];
        let mut s = FakeStreamSource::new();
        assert!(s.add_route(b"/n", NOLOC));
        assert!(s.add_route(b"/g", GONE));
        assert_eq!(s.connect(ADDR, b"GET /n HTTP/1.0\r\n\r\n"), Err(StreamError::BadResponse));
        assert_eq!(s.connect(ADDR, b"GET /g HTTP/1.0\r\n\r\n"), Err(StreamError::BadResponse));
    }

    #[test]
    fn redirects_followed_is_recorded() {
        let mut s = FakeStreamSource::with_conformance_routes();
        s.connect(ADDR, b"GET /r3 HTTP/1.0\r\n\r\n").unwrap();
        assert_eq!(s.redirects_followed, 3);
        assert_eq!(s.connects, 1);
        assert_eq!(s.last_addr, Some(ADDR));
    }

    #[test]
    fn loopback_queue_full_is_would_block() {
        let mut t = LoopbackTransport::new();
        for _ in 0..LOOPBACK_QUEUE_DEPTH {
            t.send(b"x").unwrap();
        }
        assert_eq!(t.send(b"x"), Err(TransportError::WouldBlock));
        assert_eq!(t.sent_len(), LOOPBACK_QUEUE_DEPTH);
        assert_eq!(t.send(&[0u8; LOOPBACK_FRAME_CAP + 1]), Err(TransportError::Io));
        t.disconnect();
        t.reconnect();
        assert!(t.connected());
    }

    #[test]
    fn conformance_data_is_self_consistent() {
        assert_eq!(conformance::PLAIN_BODY.len(), 48);
        assert_eq!(conformance::ICY_AUDIO.len(), 48);
        assert_eq!(conformance::ICY_META.len(), 23);
        assert_eq!(conformance::CHUNKED_BODY.len(), 27);
    }
}
