//! Device implementations of the I/O traits over smoltcp-style TCP sockets.
//!
//! [`SmoltcpSlimTransport`] ([`SlimTransport`]) and [`SmoltcpStreamSource`] ([`StreamSource`]) are
//! written against [`TcpSocketOps`], the small non-blocking subset of `smoltcp::socket::tcp::Socket`
//! they need. The firmware implements it for a real smoltcp socket in `tasks.rs` (smoltcp is a
//! device-only dependency); host tests implement it with an in-memory socket shim and run the
//! shared conformance suite against the very code the device runs.
//!
//! Both adapters keep their persistent state in a separate value (`St`, any
//! `BorrowMut<State>`): the firmware keeps the state in its static superloop state and wraps the
//! socket borrowed for one coordinator tick, tests own both.
//!
//! The HTTP parsing is shared with the host adapter through [`crate::stream_codec`].

extern crate alloc;

use alloc::vec::Vec;
use core::borrow::BorrowMut;

use squeezelite_proto::{ParseError, SlimprotoParser};

use crate::io_traits::{
    HeaderBuf, MetaBuf, NetAddr, SlimTransport, StreamError, StreamSource, TransportError,
    MAX_RESPONSE_HEAD,
};
use crate::stream_codec::{
    find_head_end, parse_ipv4_authority, request_host, ChunkedDecoder, IcyStripper,
    RedirectPolicy, RedirectTarget, ResponseHead,
};

/// Connection state of a TCP socket, as the adapters need it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TcpPhase {
    /// No connection (never opened, aborted, refused, reset or timed out).
    Closed,
    /// SYN sent, not established yet.
    Connecting,
    /// Established; both directions open.
    Established,
    /// The peer sent FIN: data already received can still be read, nothing more will arrive.
    PeerClosed,
}

/// Non-blocking TCP socket operations (the subset of `smoltcp::socket::tcp::Socket` used here).
pub trait TcpSocketOps {
    /// Current connection state.
    fn phase(&self) -> TcpPhase;
    /// Starts connecting to `remote` from a fresh local endpoint. `false` if the attempt could
    /// not be started at all (no local address, invalid endpoint, ...).
    fn connect(&mut self, remote: NetAddr) -> bool;
    /// Drops the connection immediately (RST); the socket is `Closed` afterwards.
    fn abort(&mut self);
    /// Size of the transmit buffer.
    fn send_capacity(&self) -> usize;
    /// Bytes queued in the transmit buffer.
    fn send_queue(&self) -> usize;
    /// Queues as much of `data` as fits; returns the number of bytes queued.
    fn send_slice(&mut self, data: &[u8]) -> usize;
    /// Bytes ready to be read.
    fn recv_queue(&self) -> usize;
    /// Size of the receive buffer.
    fn recv_capacity(&self) -> usize;
    /// Copies received bytes into `buf` without consuming them.
    fn peek_slice(&mut self, buf: &mut [u8]) -> usize;
    /// Moves received bytes into `buf`.
    fn recv_slice(&mut self, buf: &mut [u8]) -> usize;
}

/// Consumes up to `n` received bytes; returns how many were consumed.
fn discard<S: TcpSocketOps>(sock: &mut S, mut n: usize) -> usize {
    let mut scratch = [0u8; 64];
    let mut total = 0;
    while n > 0 {
        let got = sock.recv_slice(&mut scratch[..n.min(64)]);
        if got == 0 {
            break;
        }
        total += got;
        n -= got;
    }
    total
}

// ---------------------------------------------------------------------------
// Slimproto control connection
// ---------------------------------------------------------------------------

/// Total length (header included) of the server frame starting at `buf`.
///
/// Server frames carry a 2-byte big-endian length (covering the opcode and payload) before a
/// 4-letter opcode; the 8-byte form (opcode + 4-byte length) is accepted as well, as by the
/// host client. [`ParseError::BufferTooShort`] means more bytes are needed.
pub fn server_frame_len(buf: &[u8]) -> Result<usize, ParseError> {
    if buf.len() < 6 {
        return Err(ParseError::BufferTooShort);
    }
    if !buf[0].is_ascii_alphabetic() && buf[2..6].iter().all(|b| b.is_ascii_alphabetic()) {
        let length = u16::from_be_bytes([buf[0], buf[1]]) as usize;
        if length < 4 {
            return Err(ParseError::InvalidFormat);
        }
        if length - 4 > SlimprotoParser::MAX_FRAME_LENGTH {
            return Err(ParseError::PayloadTooLarge);
        }
        Ok(2 + length)
    } else {
        if buf.len() < 8 {
            return Err(ParseError::BufferTooShort);
        }
        let length = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
        if length > SlimprotoParser::MAX_FRAME_LENGTH {
            return Err(ParseError::PayloadTooLarge);
        }
        Ok(8 + length)
    }
}

/// Persistent state of a [`SmoltcpSlimTransport`]: bytes of a dropped frame still to skip.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SlimLinkState {
    discard: usize,
}

impl SlimLinkState {
    pub const fn new() -> Self {
        Self { discard: 0 }
    }

    /// Forgets a half-skipped frame (call when the connection is re-established).
    pub fn reset(&mut self) {
        self.discard = 0;
    }
}

/// [`SlimTransport`] over the LMS control socket.
///
/// Frames are cut directly out of the socket's receive buffer (`peek` the header, `recv` the
/// frame once it is complete), so no accumulation buffer is needed. A frame larger than the
/// caller's buffer or the socket's receive buffer is skipped and reported once as
/// [`TransportError::Io`]; an unparsable header skips one byte (resynchronisation) and is also
/// `Io`. `send` writes whole frames only: [`TransportError::WouldBlock`] when the transmit
/// buffer cannot take the frame right now, `Io` when it never could.
pub struct SmoltcpSlimTransport<S, St = SlimLinkState> {
    sock: S,
    state: St,
}

impl<S: TcpSocketOps, St: BorrowMut<SlimLinkState>> SmoltcpSlimTransport<S, St> {
    pub fn new(sock: S, state: St) -> Self {
        Self { sock, state }
    }

    pub fn socket(&self) -> &S {
        &self.sock
    }

    pub fn socket_mut(&mut self) -> &mut S {
        &mut self.sock
    }

    /// Nothing complete to deliver: `Ok(None)` while the link is up, `Disconnected` once it is
    /// gone (frames already received were delivered first).
    fn nothing_now(&mut self) -> Result<Option<usize>, TransportError> {
        match self.sock.phase() {
            TcpPhase::Connecting | TcpPhase::Established => Ok(None),
            TcpPhase::PeerClosed | TcpPhase::Closed => {
                let left = self.sock.recv_queue();
                discard(&mut self.sock, left);
                Err(TransportError::Disconnected)
            }
        }
    }

    /// Starts skipping a frame of `len` bytes.
    fn drop_frame(&mut self, len: usize) -> Result<Option<usize>, TransportError> {
        let skipped = discard(&mut self.sock, len);
        self.state.borrow_mut().discard = len - skipped;
        Err(TransportError::Io)
    }
}

impl<S: TcpSocketOps, St: BorrowMut<SlimLinkState>> SlimTransport for SmoltcpSlimTransport<S, St> {
    fn poll_frame(&mut self, buf: &mut [u8]) -> Result<Option<usize>, TransportError> {
        let pending = self.state.borrow().discard;
        if pending > 0 {
            let skipped = discard(&mut self.sock, pending);
            self.state.borrow_mut().discard = pending - skipped;
            if skipped < pending {
                return self.nothing_now();
            }
        }

        let mut header = [0u8; 8];
        let have = self.sock.peek_slice(&mut header);
        let len = match server_frame_len(&header[..have]) {
            Ok(len) => len,
            Err(ParseError::BufferTooShort) => return self.nothing_now(),
            Err(_) => {
                discard(&mut self.sock, 1);
                return Err(TransportError::Io);
            }
        };
        if len > buf.len() || len > self.sock.recv_capacity() {
            return self.drop_frame(len);
        }
        if self.sock.recv_queue() < len {
            return self.nothing_now();
        }
        let n = self.sock.recv_slice(&mut buf[..len]);
        debug_assert_eq!(n, len, "recv_queue promised a complete frame");
        Ok(Some(n))
    }

    fn send(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        if self.sock.phase() != TcpPhase::Established {
            return Err(TransportError::Disconnected);
        }
        let capacity = self.sock.send_capacity();
        if frame.len() > capacity {
            return Err(TransportError::Io);
        }
        if frame.len() > capacity - self.sock.send_queue() {
            return Err(TransportError::WouldBlock);
        }
        let n = self.sock.send_slice(frame);
        if n < frame.len() {
            // Unreachable with an honest socket; a torn frame would desynchronise the link.
            self.sock.abort();
            return Err(TransportError::Disconnected);
        }
        Ok(())
    }

    fn connected(&self) -> bool {
        self.sock.phase() == TcpPhase::Established
    }
}

// ---------------------------------------------------------------------------
// HTTP audio stream
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StreamPhase {
    /// No request (never connected, aborted, or failed).
    Idle,
    /// TCP handshake of the current hop in progress.
    TcpConnecting,
    /// Writing the request of the current hop.
    SendingRequest,
    /// Reading the response head of the current hop.
    ReadingHead,
    /// Delivering the body of the final response.
    Body,
    /// The body ended cleanly; every byte was delivered.
    Done,
}

/// Persistent state of a [`SmoltcpStreamSource`] (request, response head, body framing).
///
/// Buffers are heap `Vec`s sized by what actually arrives (typically a few hundred bytes), so
/// the firmware does not reserve [`MAX_RESPONSE_HEAD`] / ICY-sized static buffers for them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpStreamState {
    phase: StreamPhase,
    addr: NetAddr,
    /// `Host` value of the current server (same-origin redirects keep it).
    host: Vec<u8>,
    /// The request as given to `connect` (template for redirect hops).
    template: Vec<u8>,
    /// The request of the current hop.
    request: Vec<u8>,
    sent: usize,
    head: Vec<u8>,
    head_ready: bool,
    redirects: RedirectPolicy,
    chunked: bool,
    chunk: ChunkedDecoder,
    remaining: Option<u64>,
    icy: IcyStripper,
}

impl Default for HttpStreamState {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpStreamState {
    pub const fn new() -> Self {
        Self {
            phase: StreamPhase::Idle,
            addr: NetAddr { ip: [0; 4], port: 0 },
            host: Vec::new(),
            template: Vec::new(),
            request: Vec::new(),
            sent: 0,
            head: Vec::new(),
            head_ready: false,
            redirects: RedirectPolicy::new(),
            chunked: false,
            chunk: ChunkedDecoder::new(),
            remaining: None,
            icy: IcyStripper::new(),
        }
    }

    /// Back to idle, releasing every buffer.
    fn clear(&mut self) {
        *self = Self::new();
    }

    fn connecting(&self) -> bool {
        matches!(
            self.phase,
            StreamPhase::TcpConnecting | StreamPhase::SendingRequest | StreamPhase::ReadingHead
        )
    }
}

/// Outcome of reading response-head bytes.
enum HeadRead {
    Complete,
    Incomplete,
    TooLarge,
}

/// [`StreamSource`] over the audio TCP socket: non-blocking `connect` (TCP handshake, request,
/// response head, redirects, each resumed by repeating the call while it returns
/// [`StreamError::WouldBlock`]), chunked decoding, ICY stripping and early-drop detection.
///
/// End of body: chunked framing complete, `Content-Length` reached, or (close-delimited) the
/// peer's FIN. A FIN before the framing completed, or a reset at any point, is
/// [`StreamError::Disconnected`] after every byte received so far was delivered. Redirect
/// `Location`s must be same-origin or carry an IPv4 literal (no resolver on the device; a host
/// name is [`StreamError::ConnectFailed`]).
pub struct SmoltcpStreamSource<S, St = HttpStreamState> {
    sock: S,
    state: St,
}

impl<S: TcpSocketOps, St: BorrowMut<HttpStreamState>> SmoltcpStreamSource<S, St> {
    pub fn new(sock: S, state: St) -> Self {
        Self { sock, state }
    }

    pub fn socket(&self) -> &S {
        &self.sock
    }

    pub fn socket_mut(&mut self) -> &mut S {
        &mut self.sock
    }

    /// Drops the connection and the request; returns `err`.
    fn fail<T>(&mut self, err: StreamError) -> Result<T, StreamError> {
        self.sock.abort();
        self.state.borrow_mut().clear();
        Err(err)
    }

    /// Reads head bytes (never past the end of the head) into `state.head`.
    fn read_head(&mut self) -> HeadRead {
        let st = self.state.borrow_mut();
        let mut scratch = [0u8; 128];
        loop {
            let room = MAX_RESPONSE_HEAD - st.head.len();
            if room == 0 {
                return HeadRead::TooLarge;
            }
            let n = self.sock.peek_slice(&mut scratch[..room.min(128)]);
            if n == 0 {
                return HeadRead::Incomplete;
            }
            let start = st.head.len();
            st.head.extend_from_slice(&scratch[..n]);
            // Re-scan from just before the new bytes (a terminator may straddle reads).
            let from = start.saturating_sub(3);
            if let Some(end) = find_head_end(&st.head[from..]).map(|e| from + e) {
                st.head.truncate(end);
                self.sock.recv_slice(&mut scratch[..end - start]);
                return HeadRead::Complete;
            }
            self.sock.recv_slice(&mut scratch[..n]);
        }
    }

    /// Acts on a complete head: `Ok(true)` = body ready, `Ok(false)` = redirect hop started.
    fn accept_head(&mut self) -> Result<bool, StreamError> {
        let st = self.state.borrow_mut();
        let head = ResponseHead(&st.head);
        let Some(status) = head.status() else {
            return Err(StreamError::BadResponse);
        };
        if RedirectPolicy::is_redirect(status) {
            st.redirects.follow()?;
            let location = head.header(b"location").ok_or(StreamError::BadResponse)?;
            let (addr, host, path) = match RedirectPolicy::classify(location)? {
                RedirectTarget::SameOrigin { path } => (st.addr, st.host.clone(), path.to_vec()),
                RedirectTarget::Absolute { authority, path } => (
                    parse_ipv4_authority(authority).ok_or(StreamError::ConnectFailed)?,
                    authority.to_vec(),
                    path.to_vec(),
                ),
            };
            st.request = RedirectPolicy::rewrite_request(&st.template, &host, &path);
            st.host = host;
            st.addr = addr;
            st.sent = 0;
            st.head.clear();
            if !self.sock.connect(addr) {
                return Err(StreamError::ConnectFailed);
            }
            st.phase = StreamPhase::TcpConnecting;
            return Ok(false);
        }
        if status != 200 && status != 206 {
            return Err(StreamError::BadResponse);
        }
        st.chunked = head.is_chunked();
        st.chunk.reset();
        st.remaining = if st.chunked { None } else { head.content_length() };
        if let Some(interval) = head.icy_metaint() {
            st.icy.set_metaint(interval);
        }
        st.head_ready = true;
        st.request = Vec::new();
        st.template = Vec::new();
        st.phase = StreamPhase::Body;
        if st.remaining == Some(0) {
            st.phase = StreamPhase::Done;
            self.sock.abort();
        }
        Ok(true)
    }

    /// The peer is gone and every received byte was read: clean end or early drop.
    fn connection_ended(&mut self) -> Result<usize, StreamError> {
        let st = self.state.borrow_mut();
        let complete = if st.chunked {
            // A missing CRLF after the last chunk is tolerated (as by the host adapter).
            st.chunk.last_chunk_seen()
        } else {
            st.remaining.is_none_or(|r| r == 0)
        };
        if complete {
            st.phase = StreamPhase::Done;
            self.sock.abort();
            Ok(0)
        } else {
            self.fail(StreamError::Disconnected)
        }
    }
}

impl<S: TcpSocketOps, St: BorrowMut<HttpStreamState>> StreamSource for SmoltcpStreamSource<S, St> {
    fn connect(&mut self, addr: NetAddr, http_request: &[u8]) -> Result<(), StreamError> {
        if !self.state.borrow().connecting() {
            self.sock.abort();
            let st = self.state.borrow_mut();
            st.clear();
            st.addr = addr;
            st.template = http_request.to_vec();
            st.request = http_request.to_vec();
            st.host = match request_host(http_request) {
                Some(h) => h.to_vec(),
                None => {
                    let mut h = Vec::new();
                    write_authority(&mut h, addr);
                    h
                }
            };
            if !self.sock.connect(addr) {
                return self.fail(StreamError::ConnectFailed);
            }
            self.state.borrow_mut().phase = StreamPhase::TcpConnecting;
        }

        loop {
            match self.state.borrow().phase {
                StreamPhase::TcpConnecting => match self.sock.phase() {
                    TcpPhase::Connecting => return Err(StreamError::WouldBlock),
                    TcpPhase::Established => {
                        self.state.borrow_mut().phase = StreamPhase::SendingRequest
                    }
                    TcpPhase::Closed | TcpPhase::PeerClosed => {
                        return self.fail(StreamError::ConnectFailed)
                    }
                },
                StreamPhase::SendingRequest => {
                    if self.sock.phase() != TcpPhase::Established {
                        return self.fail(StreamError::Disconnected);
                    }
                    let st = self.state.borrow_mut();
                    st.sent += self.sock.send_slice(&st.request[st.sent..]);
                    if st.sent < st.request.len() {
                        return Err(StreamError::WouldBlock);
                    }
                    st.phase = StreamPhase::ReadingHead;
                }
                StreamPhase::ReadingHead => match self.read_head() {
                    HeadRead::TooLarge => return self.fail(StreamError::BadResponse),
                    HeadRead::Incomplete => {
                        return match self.sock.phase() {
                            TcpPhase::Connecting | TcpPhase::Established => {
                                Err(StreamError::WouldBlock)
                            }
                            TcpPhase::PeerClosed | TcpPhase::Closed => {
                                self.fail(StreamError::Disconnected)
                            }
                        };
                    }
                    HeadRead::Complete => match self.accept_head() {
                        Ok(true) => return Ok(()),
                        Ok(false) => {}
                        Err(e) => return self.fail(e),
                    },
                },
                StreamPhase::Idle | StreamPhase::Body | StreamPhase::Done => {
                    unreachable!("connect leaves the loop before reaching a non-connect phase")
                }
            }
        }
    }

    fn poll_bytes(&mut self, out: &mut [u8]) -> Result<usize, StreamError> {
        match self.state.borrow().phase {
            StreamPhase::Body => {}
            StreamPhase::Done => return Ok(0),
            _ => return Err(StreamError::Disconnected),
        }
        if out.is_empty() {
            return Ok(0);
        }
        loop {
            let limit = match self.state.borrow().remaining {
                Some(r) => usize::try_from(r).map_or(out.len(), |r| r.min(out.len())),
                None => out.len(),
            };
            let n = self.sock.recv_slice(&mut out[..limit]);
            if n == 0 {
                return match self.sock.phase() {
                    TcpPhase::Connecting | TcpPhase::Established => Ok(0),
                    TcpPhase::PeerClosed => self.connection_ended(),
                    TcpPhase::Closed => self.fail(StreamError::Disconnected),
                };
            }
            let st = self.state.borrow_mut();
            if let Some(r) = st.remaining.as_mut() {
                *r -= n as u64;
            }
            let mut audio = n;
            if st.chunked {
                match st.chunk.decode_in_place(&mut out[..audio]) {
                    Ok(len) => audio = len,
                    Err(e) => return self.fail(e),
                }
            }
            let audio = st.icy.strip_in_place(&mut out[..audio]);
            let ended = if st.chunked { st.chunk.is_done() } else { st.remaining == Some(0) };
            if ended {
                // Complete: release the connection (it is never reused).
                st.phase = StreamPhase::Done;
                self.sock.abort();
            }
            if audio > 0 || ended {
                return Ok(audio);
            }
            // Only framing / metadata bytes so far: keep reading.
        }
    }

    fn take_response_headers(&mut self) -> Option<HeaderBuf> {
        let mut out = HeaderBuf::new();
        // The head is capped at MAX_RESPONSE_HEAD while it is read, so it always fits.
        self.take_response_headers_with(&mut |h| {
            let _ = out.extend_from_slice(h);
        })
        .then_some(out)
    }

    fn take_icy_metadata(&mut self) -> Option<MetaBuf> {
        let mut out = MetaBuf::new();
        // An ICY block is at most 4080 bytes, so it always fits.
        self.take_icy_metadata_with(&mut |m| {
            let _ = out.extend_from_slice(m);
        })
        .then_some(out)
    }

    fn set_icy_metaint(&mut self, interval: usize) {
        self.state.borrow_mut().icy.set_metaint(interval);
    }

    fn is_eof(&self) -> bool {
        self.state.borrow().phase == StreamPhase::Done
    }

    fn abort(&mut self) {
        self.sock.abort();
        self.state.borrow_mut().clear();
    }

    fn take_response_headers_with(&mut self, f: &mut dyn FnMut(&[u8])) -> bool {
        let st = self.state.borrow_mut();
        if !st.head_ready {
            return false;
        }
        st.head_ready = false;
        let head = core::mem::take(&mut st.head);
        f(&head);
        true
    }

    fn take_icy_metadata_with(&mut self, f: &mut dyn FnMut(&[u8])) -> bool {
        match self.state.borrow_mut().icy.take_metadata() {
            Some(meta) => {
                f(&meta);
                true
            }
            None => false,
        }
    }
}

/// Appends `a.b.c.d:port` to `out`.
fn write_authority(out: &mut Vec<u8>, addr: NetAddr) {
    fn decimal(out: &mut Vec<u8>, mut v: u32) {
        let mut digits = [0u8; 10];
        let mut i = digits.len();
        loop {
            i -= 1;
            digits[i] = b'0' + (v % 10) as u8;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        out.extend_from_slice(&digits[i..]);
    }
    for (i, octet) in addr.ip.iter().enumerate() {
        if i > 0 {
            out.push(b'.');
        }
        decimal(out, u32::from(*octet));
    }
    out.push(b':');
    decimal(out, u32::from(addr.port));
}
