//! LMS Auto-Discovery and Network Protocol Client
//!
//! Works across host environments and ESP32 with `std::net` (esp-idf-svc runtime).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::time::Duration;

use crate::io_traits::{
    HeaderBuf, MetaBuf, NetAddr, SlimTransport, StreamError, StreamSource, TransportError,
};
use crate::stream_codec::{
    find_head_end, request_host, ChunkedDecoder, IcyStripper, RedirectPolicy, RedirectTarget,
    ResponseHead,
};
use squeezelite_proto::{
    DiscoveredServer, DiscoveryBuilder, DiscoveryParser, DisconnectReason, ParseError, ServerCommand,
    SlimprotoBuilder, SlimprotoParser, StatBuilder, StatEvent,
};

/// LMS Auto-Discovery Client over UDP broadcast (Port 3483).
pub struct LmsDiscovery;

impl LmsDiscovery {
    /// Broadcasts an LMS discovery query and waits up to `timeout` for a server response.
    pub fn discover_server(timeout: Duration) -> std::io::Result<(SocketAddr, DiscoveredServer)> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.set_broadcast(true)?;
        socket.set_read_timeout(Some(timeout))?;

        let mut query = [0u8; 32];
        let qlen = DiscoveryBuilder::build_query(&mut query)
            .map_err(|_| std::io::Error::other("Query build error"))?;

        // Broadcast to port 3483
        socket.send_to(&query[..qlen], "255.255.255.255:3483")?;

        let mut resp_buf = [0u8; 512];
        loop {
            let (bytes_read, src_addr) = socket.recv_from(&mut resp_buf)?;

            if let Some(server) = DiscoveryParser::parse_response(&resp_buf[..bytes_read]) {
                return Ok((src_addr, server));
            }
        }
    }
}

/// Active SlimProto TCP Streaming Connection.
pub struct SlimprotoClient {
    stream: TcpStream,
    mac: [u8; 6],
    model: String,
    acc_buf: Vec<u8>,
    /// Cleared once the peer closes the connection or a write leaves a frame half-sent.
    connected: bool,
}

impl SlimprotoClient {
    /// Connects to LMS server and completes the initial HELO handshake.
    ///
    /// Sends `HELO` while the socket is still in blocking mode (with write timeout)
    /// and transitions to non-blocking mode only after `send_helo` succeeds (`HALNET-403`).
    pub fn connect(server_addr: SocketAddr, mac: [u8; 6], model: &str) -> std::io::Result<Self> {
        Self::connect_with_options(server_addr, mac, model, false, 0)
    }

    /// Connects to LMS server with extended reconnect and byte offset options (`HALNET-403`).
    pub fn connect_with_options(
        server_addr: SocketAddr,
        mac: [u8; 6],
        model_name: &str,
        reconnect: bool,
        bytes_received: u64,
    ) -> std::io::Result<Self> {
        let stream = TcpStream::connect_timeout(&server_addr, Duration::from_secs(5))?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(Duration::from_millis(50)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;

        let mut client = Self {
            stream,
            mac,
            model: model_name.to_string(),
            acc_buf: Vec::with_capacity(8192),
            connected: true,
        };

        // HALNET-403: Send HELO while socket is still in blocking mode, then enable non-blocking mode
        client.send_helo_with_options(mac, model_name, reconnect, bytes_received)?;
        client.stream.set_nonblocking(true)?;

        Ok(client)
    }

    /// Builds and sends the initial `HELO` handshake packet (`HALNET-403`).
    pub fn send_helo(&mut self, mac: [u8; 6]) -> std::io::Result<()> {
        let model = self.model.clone();
        self.send_helo_with_options(mac, &model, false, 0)
    }

    /// Builds and sends the `HELO` handshake packet with reconnect flag and received byte offset.
    pub fn send_helo_with_options(
        &mut self,
        mac: [u8; 6],
        model_name: &str,
        reconnect: bool,
        bytes_received: u64,
    ) -> std::io::Result<()> {
        let mut helo_buf = [0u8; 512];
        let model_part = if model_name.is_empty() {
            "Model=LouderESP32Plus".to_string()
        } else if model_name.starts_with("Model=") {
            model_name.to_string()
        } else {
            format!("Model={model_name}")
        };
        let caps = format!("{model_part},mp3,flac,aac,ogg,pcm,MaxSampleRate=192000");
        let helo_len = SlimprotoBuilder::build_helo_with_options(
            &mac,
            12,
            &caps,
            reconnect,
            bytes_received,
            &mut helo_buf,
        )
        .map_err(|_| std::io::Error::other("HELO build error"))?;

        Self::write_all_bounded(&mut self.stream, &helo_buf[..helo_len])
    }

    /// Writes all bytes in `buf` to `stream`, retrying bounded times on `WouldBlock` (`HALNET-403`).
    fn write_all_bounded(stream: &mut TcpStream, buf: &[u8]) -> std::io::Result<()> {
        let mut written = 0;
        Self::write_all_tracked(stream, buf, &mut written)
    }

    /// Like [`Self::write_all_bounded`], additionally reporting in `offset` how many bytes were
    /// accepted by the socket, so a caller can tell a clean failure from a half-written frame.
    fn write_all_tracked(
        stream: &mut TcpStream,
        buf: &[u8],
        offset: &mut usize,
    ) -> std::io::Result<()> {
        let mut retries = 0u8;
        while *offset < buf.len() {
            match stream.write(&buf[*offset..]) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::WriteZero,
                        "failed to write whole buffer",
                    ));
                }
                Ok(n) => {
                    *offset += n;
                    retries = 0;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    retries += 1;
                    if retries > 16 {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::WouldBlock,
                            "non-blocking write retry limit exceeded",
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Returns the player MAC address.
    pub fn mac(&self) -> &[u8; 6] {
        &self.mac
    }

    /// Returns the player model string.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Returns the remote peer address of the connected LMS server.
    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.stream.peer_addr()
    }

    /// Sends a STAT heartbeat or event update to LMS (`HALNET-403`).
    pub fn send_stat(
        &mut self,
        event: StatEvent,
        stream_buf_fullness: u32,
        output_buf_fullness: u32,
        frames_played: u32,
        timestamp_ms: u32,
    ) -> std::io::Result<()> {
        let mut buf = [0u8; 64];
        let len = SlimprotoBuilder::build_stat(
            event,
            stream_buf_fullness,
            output_buf_fullness,
            frames_played,
            timestamp_ms,
            &mut buf,
        )
        .map_err(|_| std::io::Error::other("STAT build error"))?;

        Self::write_all_bounded(&mut self.stream, &buf[..len])
    }

    /// Sends a STAT telemetry packet with explicit metrics including custom sample rate,
    /// 64-bit frames played, server timestamp, and 64-bit byte counter.
    #[allow(clippy::too_many_arguments)]
    pub fn send_stat_with_metrics(
        &mut self,
        event: StatEvent,
        stream_buf_fullness: u32,
        output_buf_fullness: u32,
        frames_played: u64,
        sample_rate: u32,
        bytes_received: u64,
        jiffies_ms: u32,
        server_timestamp: u32,
    ) -> std::io::Result<()> {
        let mut buf = [0u8; 128];
        let len = StatBuilder::build_stat_with_metrics(
            event,
            stream_buf_fullness,
            output_buf_fullness,
            frames_played,
            sample_rate,
            bytes_received,
            jiffies_ms,
            server_timestamp,
            &mut buf,
        )
        .map_err(|_| std::io::Error::other("STAT build error"))?;

        Self::write_all_bounded(&mut self.stream, &buf[..len])
    }

    /// Sends an HTTP response packet (`RESP`) transmitting received HTTP headers to LMS.
    pub fn send_resp(&mut self, headers: &[u8]) -> std::io::Result<()> {
        let mut buf = vec![0u8; 8 + headers.len()];
        let len = SlimprotoBuilder::build_resp(headers, &mut buf)
            .map_err(|_| std::io::Error::other("RESP build error"))?;
        Self::write_all_bounded(&mut self.stream, &buf[..len])
    }

    /// Sends a disconnect notification packet (`DSCO`) transmitting disconnect reason to LMS.
    pub fn send_dsco(&mut self, reason: DisconnectReason) -> std::io::Result<()> {
        let mut buf = [0u8; 16];
        let len = SlimprotoBuilder::build_dsco(reason, &mut buf)
            .map_err(|_| std::io::Error::other("DSCO build error"))?;
        Self::write_all_bounded(&mut self.stream, &buf[..len])
    }

    /// Sends a stream metadata packet (`META`) forwarding stream metadata (e.g. ICY) to LMS.
    pub fn send_meta(&mut self, metadata: &[u8]) -> std::io::Result<()> {
        let mut buf = vec![0u8; 8 + metadata.len()];
        let len = SlimprotoBuilder::build_meta(metadata, &mut buf)
            .map_err(|_| std::io::Error::other("META build error"))?;
        Self::write_all_bounded(&mut self.stream, &buf[..len])
    }

    /// Sends a player name device setting packet (`SETD` with id 0x00) to LMS.
    pub fn send_setd_name(&mut self, name: &str) -> std::io::Result<()> {
        let mut buf = vec![0u8; 9 + name.len()];
        let len = SlimprotoBuilder::build_setd_name(name, &mut buf)
            .map_err(|_| std::io::Error::other("SETD build error"))?;
        Self::write_all_bounded(&mut self.stream, &buf[..len])
    }

    /// Inspects the frame header in the buffer to determine expected frame length without borrowing scratch.
    fn peek_frame_len(buf: &[u8]) -> Result<usize, ParseError> {
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
            2usize.checked_add(length).ok_or(ParseError::PayloadTooLarge)
        } else {
            if buf.len() < 8 {
                return Err(ParseError::BufferTooShort);
            }
            let length = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
            if length > SlimprotoParser::MAX_FRAME_LENGTH {
                return Err(ParseError::PayloadTooLarge);
            }
            8usize.checked_add(length).ok_or(ParseError::PayloadTooLarge)
        }
    }

    /// Polls the network stream, accumulating incoming TCP chunks,
    /// and parses the next available Slimproto frame (PROTO-001, PROTO-002).
    ///
    /// Copies the active frame into `scratch` so that `ServerCommand<'a>` borrows
    /// from `scratch` rather than `self`, avoiding borrow-checker contention.
    pub fn poll_command<'a>(&mut self, scratch: &'a mut [u8]) -> std::io::Result<Option<ServerCommand<'a>>> {
        let Some(consumed) = self.poll_raw_frame(scratch)? else {
            return Ok(None);
        };

        // Single parse site at the end!
        let (cmd, _) = SlimprotoParser::parse_frame(&scratch[..consumed])
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "Malformed frame"))?;
        Ok(Some(cmd))
    }

    /// Accumulates incoming TCP data and copies the next complete frame (length prefix
    /// included, exactly as received) into `scratch`, returning its length.
    ///
    /// Shared by [`Self::poll_command`] and the [`SlimTransport`] adapter.
    fn poll_raw_frame(&mut self, scratch: &mut [u8]) -> std::io::Result<Option<usize>> {
        const MAX_ACC_BUF: usize = 8192;

        // 1. If self.acc_buf does not yet have a complete frame, read available bytes from self.stream
        let has_complete_frame = match Self::peek_frame_len(&self.acc_buf) {
            Ok(len) => self.acc_buf.len() >= len,
            Err(_) => false,
        };

        if !has_complete_frame {
            let mut temp_buf = [0u8; 1024];
            match self.stream.read(&mut temp_buf) {
                Ok(0) => {
                    self.connected = false;
                    return Err(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "Server disconnected"));
                }
                Ok(n) => {
                    if self.acc_buf.len() + n > MAX_ACC_BUF {
                        self.acc_buf.clear();
                        return Err(std::io::Error::new(std::io::ErrorKind::OutOfMemory, "Accumulation buffer overflow"));
                    }
                    self.acc_buf.extend_from_slice(&temp_buf[..n]);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
                    // Non-blocking would block or timed out
                }
                Err(e) => {
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::ConnectionAborted
                            | std::io::ErrorKind::BrokenPipe
                            | std::io::ErrorKind::NotConnected
                            | std::io::ErrorKind::UnexpectedEof
                    ) {
                        self.connected = false;
                    }
                    return Err(e);
                }
            }
        }

        if self.acc_buf.is_empty() {
            return Ok(None);
        }

        // 2. Determine frame length from self.acc_buf
        let consumed = match Self::peek_frame_len(&self.acc_buf) {
            Ok(len) => {
                if self.acc_buf.len() < len {
                    return Ok(None);
                }
                len
            }
            Err(ParseError::BufferTooShort) => return Ok(None),
            Err(_) => {
                self.acc_buf.clear();
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Malformed frame"));
            }
        };

        // If scratch buffer is too small, drain the oversized frame FIRST to unwedge
        if scratch.len() < consumed {
            self.acc_buf.drain(..consumed);
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Scratch buffer too small for frame",
            ));
        }

        // Copy active frame into scratch and drain from accumulation buffer
        scratch[..consumed].copy_from_slice(&self.acc_buf[..consumed]);
        self.acc_buf.drain(..consumed);

        Ok(Some(consumed))
    }
}

impl SlimTransport for SlimprotoClient {
    /// Delivers one raw frame per call. Frames already accumulated stay deliverable after the
    /// peer closes (the socket is only read when no complete frame is buffered); the disconnect
    /// surfaces once they are drained.
    fn poll_frame(&mut self, buf: &mut [u8]) -> Result<Option<usize>, TransportError> {
        use std::io::ErrorKind;
        self.poll_raw_frame(buf).map_err(|e| match e.kind() {
            ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::BrokenPipe
            | ErrorKind::NotConnected
            | ErrorKind::UnexpectedEof => TransportError::Disconnected,
            ErrorKind::WouldBlock => TransportError::WouldBlock,
            _ => TransportError::Io,
        })
    }

    /// Sends one frame verbatim. `WouldBlock` is returned only when nothing was written, so the
    /// caller can retry the whole frame; a failure after a partial write would desynchronise the
    /// byte stream, so it closes the connection instead and reports `Disconnected`.
    fn send(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        use std::io::ErrorKind;
        if !self.connected {
            return Err(TransportError::Disconnected);
        }
        let mut written = 0;
        match Self::write_all_tracked(&mut self.stream, frame, &mut written) {
            Ok(()) => Ok(()),
            Err(e) if written == 0 && e.kind() == ErrorKind::WouldBlock => {
                Err(TransportError::WouldBlock)
            }
            Err(_) => {
                self.connected = false;
                let _ = self.stream.shutdown(std::net::Shutdown::Both);
                Err(TransportError::Disconnected)
            }
        }
    }

    fn connected(&self) -> bool {
        self.connected
    }
}

fn parse_http_location(location: &str) -> std::io::Result<(SocketAddr, String, String)> {
    let s = location.trim().trim_matches('"').trim_matches('\'').trim();
    let without_scheme = if let Some(stripped) = s.strip_prefix("http://") {
        stripped
    } else if let Some(stripped) = s.strip_prefix("HTTP://") {
        stripped
    } else if s.len() >= 7 && s[..7].eq_ignore_ascii_case("http://") {
        &s[7..]
    } else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Unsupported URL scheme in Location: {s}"),
        ));
    };

    let (host_port, path) = match without_scheme.find('/') {
        Some(idx) => (&without_scheme[..idx], &without_scheme[idx..]),
        None => (without_scheme, "/"),
    };

    use std::net::ToSocketAddrs;
    let host_port_with_default = if host_port.starts_with('[') {
        if let Some(bracket_end) = host_port.find(']') {
            let after_bracket = &host_port[bracket_end + 1..];
            if after_bracket.starts_with(':') {
                host_port.to_string()
            } else {
                format!("{host_port}:80")
            }
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Unclosed bracket in IPv6 host: {host_port}"),
            ));
        }
    } else if host_port.contains(':') {
        host_port.to_string()
    } else {
        format!("{host_port}:80")
    };
    let addrs: Vec<SocketAddr> = host_port_with_default.to_socket_addrs()?.collect();
    let addr = addrs.into_iter().next().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Could not resolve host: {host_port}"),
        )
    })?;

    Ok((addr, host_port.to_string(), path.to_string()))
}

/// Opens a TCP connection to `addr` and writes `request` while the socket is still blocking
/// (1500ms connect/write timeout, `HALNET-410`); only then switches it to non-blocking mode
/// (`HALNET-404`).
fn open_stream(addr: SocketAddr, request: &[u8]) -> std::io::Result<TcpStream> {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(1500))?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_millis(50)))?;
    stream.set_write_timeout(Some(Duration::from_millis(1500)))?;
    stream.write_all(request)?;
    stream.set_nonblocking(true)?;
    Ok(stream)
}

/// Value of the `Host:` header in `request`, falling back to the socket address.
fn host_of_request(request: &str, addr: SocketAddr) -> String {
    request_host(request.as_bytes())
        .map(|h| String::from_utf8_lossy(h).into_owned())
        .unwrap_or_else(|| addr.to_string())
}

/// Pure-Rust HTTP Audio Stream Client for fetching streamed audio over TCP.
///
/// Two ways to use it:
/// * the original API: [`HttpAudioStream::connect`] (blocking connect, returns a connected
///   stream) and [`HttpAudioStream::poll_audio_bytes`];
/// * the [`StreamSource`] trait: [`HttpAudioStream::new`] creates an idle stream and the trait's
///   `connect` (re)connects it. The inherent `is_eof`, `take_response_headers`,
///   `take_icy_metadata` and `set_icy_metaint` shadow the trait methods on the concrete type
///   and keep their original `Vec`-based signatures; call them through `StreamSource::` for the
///   trait semantics (notably, the trait `is_eof` is never true for a connection that dropped
///   early, while the inherent one reports any socket close).
pub struct HttpAudioStream {
    /// `None` while idle, after `abort`, and after a failed connect.
    stream: Option<TcpStream>,
    header_buf: Vec<u8>,
    headers_parsed: bool,
    eof: bool,

    // Redirect & request tracking
    request_template: String,
    redirects: RedirectPolicy,
    response_headers: Option<Vec<u8>>,
    /// Address and `Host` value of the server currently being read; same-origin (relative)
    /// `Location` values resolve against these.
    cur_addr: Option<SocketAddr>,
    cur_host: String,

    // Chunked transfer decoding
    is_chunked: bool,
    chunk: ChunkedDecoder,
    raw_body_buf: Vec<u8>,
    unchunked_buf: Vec<u8>,
    /// Body bytes still owed according to `Content-Length` (not chunked).
    content_remaining: Option<u64>,

    // ICY metadata handling
    icy: IcyStripper,

    // Final pure audio buffer
    audio_buf: Vec<u8>,
    total_audio_bytes: u64,

    // `StreamSource` bookkeeping
    /// The socket closed before the response was complete (truncated chunked/`Content-Length`
    /// body, or closed during the headers). `eof` is still set for the original API.
    early_drop: bool,
    /// A `StreamSource::connect` is in progress (`WouldBlock` was returned).
    connecting: bool,
    /// Precise error for the next `io::Error` that `poll_audio_bytes` returns.
    fault: Option<StreamError>,
}

impl Default for HttpAudioStream {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpAudioStream {
    pub const MAX_REDIRECTS: usize = 3;
    pub const MAX_HTTP_HEADER_LEN: usize = 4096;
    pub const MAX_BODY_BUF_LEN: usize = 65_536;
    pub const MAX_CHUNK_SIZE: usize = 1_048_576;

    /// Creates an idle, unconnected stream; use [`StreamSource::connect`] to start a request.
    pub fn new() -> Self {
        Self {
            stream: None,
            header_buf: Vec::with_capacity(512),
            headers_parsed: false,
            eof: false,

            request_template: String::new(),
            redirects: RedirectPolicy::new(),
            response_headers: None,
            cur_addr: None,
            cur_host: String::new(),

            is_chunked: false,
            chunk: ChunkedDecoder::new(),
            raw_body_buf: Vec::with_capacity(2048),
            unchunked_buf: Vec::with_capacity(2048),
            content_remaining: None,

            icy: IcyStripper::new(),

            audio_buf: Vec::with_capacity(2048),
            total_audio_bytes: 0,

            early_drop: false,
            connecting: false,
            fault: None,
        }
    }

    /// Connects to the HTTP streaming audio server and issues the initial GET request.
    ///
    /// Uses a 1500ms connect/write timeout (`HALNET-410`) and writes the full HTTP request
    /// while the socket is still in blocking mode before enabling non-blocking mode (`HALNET-404`).
    pub fn connect(server_addr: SocketAddr, http_headers: &str) -> std::io::Result<Self> {
        let mut request = http_headers.as_bytes().to_vec();
        if !http_headers.is_empty() && !http_headers.ends_with("\r\n\r\n") {
            if http_headers.ends_with("\r\n") {
                request.extend_from_slice(b"\r\n");
            } else {
                request.extend_from_slice(b"\r\n\r\n");
            }
        }
        let stream = open_stream(server_addr, &request)?;

        let mut this = Self::new();
        this.stream = Some(stream);
        this.request_template = http_headers.to_string();
        this.cur_addr = Some(server_addr);
        this.cur_host = host_of_request(http_headers, server_addr);
        Ok(this)
    }

    /// Returns `true` if the server disconnected (EOF) and all pending audio bytes have been drained.
    pub fn is_eof(&self) -> bool {
        self.eof && self.audio_buf.is_empty() && self.unchunked_buf.is_empty()
    }

    /// Returns `true` once the HTTP response headers have been successfully parsed and verified.
    pub fn headers_parsed(&self) -> bool {
        self.headers_parsed
    }

    /// Returns the captured raw HTTP response headers, if available.
    pub fn take_response_headers(&mut self) -> Option<Vec<u8>> {
        self.response_headers.take()
    }

    /// Returns the latest parsed ICY metadata block, if available.
    pub fn take_icy_metadata(&mut self) -> Option<Vec<u8>> {
        self.icy.take_metadata()
    }

    /// Configures the ICY metadata interval in audio bytes.
    pub fn set_icy_metaint(&mut self, interval: usize) {
        self.icy.set_metaint(interval);
    }

    /// Returns current ICY metadata interval.
    pub fn icy_metaint(&self) -> usize {
        self.icy.metaint()
    }

    /// Returns total audio stream bytes polled by caller.
    pub fn total_audio_bytes(&self) -> u64 {
        self.total_audio_bytes
    }

    /// De-chunks (when chunked) and ICY-strips everything in `raw_body_buf` into `audio_buf`
    /// (shared state machines: [`ChunkedDecoder`], [`IcyStripper`]).
    fn process_raw_body(&mut self) -> std::io::Result<()> {
        if self.is_chunked {
            let n = self.chunk.decode_in_place(&mut self.raw_body_buf).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid chunked framing")
            })?;
            self.unchunked_buf.extend_from_slice(&self.raw_body_buf[..n]);
            self.raw_body_buf.clear();
            if self.chunk.is_done() {
                self.eof = true;
            }
        } else {
            self.unchunked_buf.append(&mut self.raw_body_buf);
        }
        let n = self.icy.strip_in_place(&mut self.unchunked_buf);
        self.audio_buf.extend_from_slice(&self.unchunked_buf[..n]);
        self.unchunked_buf.clear();
        Ok(())
    }

    /// Header phase: reads and parses the HTTP response head, following redirects. Returns
    /// `Ok(true)` once the final `200`/`206` response head has been consumed (any body bytes that
    /// arrived with it stay in `raw_body_buf`, unprocessed), and `Ok(false)` while more data is
    /// needed or if the socket closed (`eof`/`early_drop` are set in that case).
    fn drive_headers(&mut self) -> std::io::Result<bool> {
        while !self.headers_parsed {
            let mut temp = [0u8; 1024];
            let Some(stream) = self.stream.as_mut() else {
                return Err(std::io::ErrorKind::NotConnected.into());
            };
            let n = match stream.read(&mut temp) {
                Ok(0) => {
                    self.eof = true;
                    self.early_drop = true;
                    return Ok(false);
                }
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    return Ok(false);
                }
                Ok(n) => n,
                Err(e) => return Err(e),
            };

            if self.header_buf.len() + n > Self::MAX_HTTP_HEADER_LEN {
                self.header_buf.clear();
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "HTTP header too large",
                ));
            }

            self.header_buf.extend_from_slice(&temp[..n]);

            let Some(head_len) = find_head_end(&self.header_buf) else {
                return Ok(false);
            };
            let head = ResponseHead(&self.header_buf[..head_len]);
            let Some(code) = head.status() else {
                self.header_buf.clear();
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Non-200 HTTP response",
                ));
            };

            // Redirects: 301, 302, 303, 307, 308 (shared `RedirectPolicy`)
            if RedirectPolicy::is_redirect(code) {
                if let Err(e) = self.redirects.follow() {
                    self.header_buf.clear();
                    self.fault = Some(e);
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Too many redirects",
                    ));
                }

                let location = head.header(b"location").ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Redirect response without Location header",
                    )
                })?;
                let target = RedirectPolicy::classify(location).map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Unsupported Location in redirect",
                    )
                })?;

                // Same-origin Location (`/path`) stays on the current server; anything else
                // is an absolute `http://` URL, resolved here (host names included).
                let (new_addr, host_port, new_path) = match (target, self.cur_addr) {
                    (RedirectTarget::SameOrigin { path }, Some(addr)) => (
                        addr,
                        self.cur_host.clone(),
                        String::from_utf8_lossy(path).into_owned(),
                    ),
                    _ => parse_http_location(&String::from_utf8_lossy(location))?,
                };
                let new_request = RedirectPolicy::rewrite_request(
                    self.request_template.as_bytes(),
                    host_port.as_bytes(),
                    new_path.as_bytes(),
                );

                let new_stream = open_stream(new_addr, &new_request).inspect_err(|_| {
                    self.fault = Some(StreamError::ConnectFailed);
                })?;

                self.stream = Some(new_stream);
                self.cur_addr = Some(new_addr);
                self.cur_host = host_port;
                self.header_buf.clear();
                continue;
            }

            // Verify 200 or 206 status (HALNET-406)
            if code != 200 && code != 206 {
                self.header_buf.clear();
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Non-200 HTTP response",
                ));
            }

            self.is_chunked = head.is_chunked();
            if let Some(metaint) = head.icy_metaint() {
                self.icy.set_metaint(metaint);
            }
            // Declared body length (identity encoding only), to tell truncation from EOF
            let content_length = if self.is_chunked { None } else { head.content_length() };

            // Capture raw response headers (including delimiter)
            self.response_headers = Some(self.header_buf[..head_len].to_vec());

            let leftover = self.header_buf.len() - head_len;
            if leftover > 0 {
                self.raw_body_buf.extend_from_slice(&self.header_buf[head_len..]);
            }
            self.content_remaining = content_length.map(|l| l.saturating_sub(leftover as u64));
            self.header_buf.clear();
            self.headers_parsed = true;
            return Ok(true);
        }
        Ok(true)
    }

    /// Polls audio stream bytes non-blockingly into `out`.
    ///
    /// During the initial header phase, incoming chunks are parsed to strip the HTTP response
    /// header, follow redirects (`301`, `302`, `307`, `308`), and verify HTTP 200 / 206 status (`HALNET-406`).
    /// Once parsed, raw audio stream bytes are de-chunked and stripped of ICY metadata before
    /// being written directly to `out`, returning `Ok(0)` with `is_eof() == true` when finished (`HALNET-405`).
    pub fn poll_audio_bytes(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }

        // 1. Drain pending audio
        if !self.audio_buf.is_empty() {
            let n = out.len().min(self.audio_buf.len());
            out[..n].copy_from_slice(&self.audio_buf[..n]);
            self.audio_buf.drain(..n);
            self.total_audio_bytes += n as u64;
            return Ok(n);
        }

        if self.is_eof() {
            return Ok(0);
        }

        // 2. Initial header parsing and redirect following
        if !self.headers_parsed {
            if !self.drive_headers()? {
                return Ok(0);
            }
            self.process_raw_body()?;
            if !self.audio_buf.is_empty() {
                let copied = out.len().min(self.audio_buf.len());
                out[..copied].copy_from_slice(&self.audio_buf[..copied]);
                self.audio_buf.drain(..copied);
                self.total_audio_bytes += copied as u64;
                return Ok(copied);
            }
        }

        // 3. Read body from TCP stream
        let mut temp = [0u8; 2048];
        let Some(stream) = self.stream.as_mut() else {
            return Err(std::io::ErrorKind::NotConnected.into());
        };
        match stream.read(&mut temp) {
            Ok(0) => {
                // Settle bytes that arrived with the headers first, so the decoder state below
                // reflects everything received.
                self.process_raw_body()?;
                self.eof = true;
                let complete = if self.is_chunked {
                    // A last-chunk was seen (a missing final CRLF is tolerated).
                    self.chunk.last_chunk_seen()
                } else {
                    self.content_remaining.is_none_or(|r| r == 0)
                };
                if !complete {
                    self.early_drop = true;
                }
            }
            Ok(n) => {
                if let Some(r) = self.content_remaining.as_mut() {
                    *r = r.saturating_sub(n as u64);
                }
                if self.raw_body_buf.len() + n > Self::MAX_BODY_BUF_LEN {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::OutOfMemory,
                        "raw_body_buf exceeded MAX_BODY_BUF_LEN",
                    ));
                }
                self.raw_body_buf.extend_from_slice(&temp[..n]);
            }
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                // No new bytes
            }
            Err(e) => return Err(e),
        }

        self.process_raw_body()?;

        if !self.audio_buf.is_empty() {
            let n = out.len().min(self.audio_buf.len());
            out[..n].copy_from_slice(&self.audio_buf[..n]);
            self.audio_buf.drain(..n);
            self.total_audio_bytes += n as u64;
            Ok(n)
        } else {
            Ok(0)
        }
    }
}

impl HttpAudioStream {
    /// Maps an error from the original `io::Result` API onto a [`StreamError`].
    fn stream_error(&mut self, e: &std::io::Error) -> StreamError {
        use std::io::ErrorKind;
        if let Some(fault) = self.fault.take() {
            return fault;
        }
        match e.kind() {
            ErrorKind::InvalidData | ErrorKind::InvalidInput | ErrorKind::OutOfMemory => {
                StreamError::BadResponse
            }
            _ => StreamError::Disconnected,
        }
    }

    /// Drops the connection and every buffered byte.
    fn close(&mut self) {
        if let Some(stream) = self.stream.take() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        self.connecting = false;
        self.headers_parsed = false;
        self.eof = false;
        self.early_drop = false;
        self.header_buf.clear();
        self.raw_body_buf.clear();
        self.unchunked_buf.clear();
        self.audio_buf.clear();
        self.response_headers = None;
        let _ = self.icy.take_metadata();
    }
}

/// `connect` waits up to this long for the response head before returning
/// [`StreamError::WouldBlock`] (the caller repeats the same call).
const CONNECT_POLL_BUDGET: Duration = Duration::from_millis(100);

impl StreamSource for HttpAudioStream {
    /// Redirects are followed here; `Location` may be absolute (`http://host[:port]/path`) or
    /// same-origin (`/path`). Repeating a call that returned [`StreamError::WouldBlock`] resumes
    /// the same request.
    ///
    /// **This host implementation blocks.** Opening a connection ([`open_stream`]) takes a
    /// blocking `connect` (1500 ms timeout) plus a blocking `write_all` of the request (1500 ms
    /// write timeout), i.e. up to ~3 s. The first call does that once, and every redirect hop
    /// followed inside a single call repeats it (up to [`HttpAudioStream::MAX_REDIRECTS`] hops),
    /// so one call can block for ~12 s in the worst case. Only the wait for the response head
    /// is bounded by [`CONNECT_POLL_BUDGET`] (100 ms between hops), after which `WouldBlock`
    /// is returned. This blocking profile is specific to the `std::net` host adapter; the
    /// device (smoltcp) implementation is independent and does not share it.
    fn connect(&mut self, addr: NetAddr, http_request: &[u8]) -> Result<(), StreamError> {
        if !self.connecting {
            let sock = SocketAddr::from((addr.ip, addr.port));
            *self = Self::new();
            let stream = open_stream(sock, http_request).map_err(|_| StreamError::ConnectFailed)?;
            let request = String::from_utf8_lossy(http_request).into_owned();
            self.cur_host = host_of_request(&request, sock);
            self.request_template = request;
            self.cur_addr = Some(sock);
            self.stream = Some(stream);
            self.connecting = true;
        }

        let deadline = std::time::Instant::now() + CONNECT_POLL_BUDGET;
        loop {
            match self.drive_headers() {
                Ok(true) => {
                    self.connecting = false;
                    return Ok(());
                }
                Ok(false) if self.eof => {
                    // Closed before the response head was complete.
                    self.close();
                    return Err(StreamError::Disconnected);
                }
                Ok(false) => {
                    if std::time::Instant::now() >= deadline {
                        return Err(StreamError::WouldBlock);
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(e) => {
                    let err = self.stream_error(&e);
                    self.close();
                    return Err(err);
                }
            }
        }
    }

    /// Never blocks and never returns `WouldBlock`. Bytes received before a drop are delivered
    /// first; then a connection that closed early (truncated chunked or `Content-Length` body)
    /// yields [`StreamError::Disconnected`] instead of EOF.
    fn poll_bytes(&mut self, out: &mut [u8]) -> Result<usize, StreamError> {
        if self.stream.is_none() || self.connecting {
            return Err(StreamError::Disconnected);
        }
        match self.poll_audio_bytes(out) {
            Ok(0) if self.early_drop && self.audio_buf.is_empty() && self.unchunked_buf.is_empty() => {
                Err(StreamError::Disconnected)
            }
            Ok(n) => Ok(n),
            Err(e) => {
                let err = self.stream_error(&e);
                // Decoder state is unreliable after an error; make the failure sticky.
                self.close();
                Err(err)
            }
        }
    }

    /// Raw head of the final response. A head longer than
    /// [`MAX_RESPONSE_HEAD`](crate::io_traits::MAX_RESPONSE_HEAD) bytes is cut at the last line
    /// boundary that leaves room for the terminating blank line (never inside a header line, so
    /// no partial value such as `icy-metaint: 1` survives), then the blank line is appended. With
    /// the current capacities this cannot trigger through the socket path
    /// (`MAX_HTTP_HEADER_LEN` rejects longer heads first); it guards the trait contract.
    fn take_response_headers(&mut self) -> Option<HeaderBuf> {
        let raw = HttpAudioStream::take_response_headers(self)?;
        let mut out = heapless::Vec::new();
        if raw.len() <= out.capacity() {
            let _ = out.extend_from_slice(&raw);
        } else {
            // `raw[..cut]` must leave 4 bytes for "\r\n\r\n"; cut before the last CRLF at or
            // before that limit.
            let limit = out.capacity() - 4;
            let cut = raw[..limit + 2].windows(2).rposition(|w| w == b"\r\n").unwrap_or(0);
            let _ = out.extend_from_slice(&raw[..cut]);
            let _ = out.extend_from_slice(b"\r\n\r\n");
        }
        Some(out)
    }

    fn take_icy_metadata(&mut self) -> Option<MetaBuf> {
        let raw = HttpAudioStream::take_icy_metadata(self)?;
        let mut out = heapless::Vec::new();
        let _ = out.extend_from_slice(&raw[..raw.len().min(out.capacity())]);
        Some(out)
    }

    fn set_icy_metaint(&mut self, interval: usize) {
        HttpAudioStream::set_icy_metaint(self, interval);
    }

    fn is_eof(&self) -> bool {
        HttpAudioStream::is_eof(self) && !self.early_drop
    }

    fn abort(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn test_take_response_headers_cuts_long_head_at_line_boundary() {
        // Every pad line ends in 'E', so a cut inside a line is visible. The head is far over
        // MAX_RESPONSE_HEAD (unreachable through the socket path, which caps heads first).
        let limit = crate::io_traits::MAX_RESPONSE_HEAD;
        let mut head = String::from("HTTP/1.0 200 OK\r\nContent-Type: audio/mpeg\r\n");
        let mut i = 0;
        while head.len() < limit + 900 {
            head.push_str(&format!("X-Pad-{i:03}: {}E\r\n", "v".repeat(30 + i % 7)));
            i += 1;
        }
        head.push_str("icy-metaint: 16000\r\n\r\n");

        let mut s = HttpAudioStream::new();
        s.response_headers = Some(head.clone().into_bytes());
        let h = StreamSource::take_response_headers(&mut s).expect("headers captured");
        assert!(h.len() <= limit);
        assert!(h.len() > limit - 100, "should keep as much as fits, got {}", h.len());
        assert!(h.starts_with(b"HTTP/1.0 200 OK\r\n"), "status line intact");
        assert!(h.ends_with(b"\r\n\r\n"), "terminating blank line present");
        let text = core::str::from_utf8(&h).unwrap();
        let body = text.strip_suffix("\r\n\r\n").unwrap();
        let mut lines = body.split("\r\n");
        assert_eq!(lines.next(), Some("HTTP/1.0 200 OK"));
        let mut n = 0;
        for line in lines {
            let (name, value) = line.split_once(": ").expect("complete `name: value` line");
            assert!(!name.is_empty());
            if name.starts_with("X-Pad-") {
                assert!(value.ends_with('E'), "partial header value: {line:?}");
            }
            assert!(head.contains(&format!("{line}\r\n")), "line not from the response: {line:?}");
            n += 1;
        }
        assert!(n > 50, "kept {n} header lines");
        assert!(!text.contains("icy-metaint"), "dropped lines are dropped whole");
        assert!(StreamSource::take_response_headers(&mut s).is_none(), "taken once");
    }

    #[test]
    fn test_slimproto_client_handshake_and_stat() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server_addr = listener.local_addr().unwrap();

        let mac = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];

        // Spawn client thread
        let handle = std::thread::spawn(move || {
            let mut client = SlimprotoClient::connect(server_addr, mac, "TestModel").unwrap();
            std::thread::sleep(Duration::from_millis(50));
            client
                .send_stat(StatEvent::Heartbeat, 1000, 2000, 44100, 500)
                .unwrap();
        });

        // Accept connection on mock server
        let (mut server_stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 128];

        // 1. Read HELO packet
        let n1 = server_stream.read(&mut buf).unwrap();
        assert!(n1 >= 18);
        assert_eq!(&buf[0..4], b"HELO");
        assert_eq!(&buf[10..16], &mac);

        // 2. Read STAT packet (61 bytes: 8 header + 53 payload)
        let n2 = server_stream.read(&mut buf).unwrap();
        assert_eq!(n2, 61);
        assert_eq!(&buf[0..4], b"STAT");
        assert_eq!(&buf[8..12], b"STMt");

        handle.join().unwrap();
    }

    #[test]
    fn test_slimproto_client_partial_reads_and_multi_frame() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server_addr = listener.local_addr().unwrap();

        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];

        let handle = std::thread::spawn(move || {
            let mut client = SlimprotoClient::connect(server_addr, mac, "TestModel").unwrap();
            let mut scratch = [0u8; 1024];

            // 1. First poll with partial data should yield None
            let cmd1 = client.poll_command(&mut scratch).unwrap();
            assert!(cmd1.is_none());

            // Give server thread time to send second chunk
            std::thread::sleep(Duration::from_millis(100));

            // 2. Second poll after completion of frame should yield Flush
            let cmd2 = client.poll_command(&mut scratch).unwrap();
            assert_eq!(cmd2, Some(ServerCommand::Flush));

            // 3. Third poll should yield the second concatenated frame (Flush) from buffer
            let cmd3 = client.poll_command(&mut scratch).unwrap();
            assert_eq!(cmd3, Some(ServerCommand::Flush));
        });

        let (mut server_stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 128];
        let _ = server_stream.read(&mut buf).unwrap(); // Consume HELO

        // Send half of a flsh frame (flsh packet is 4 bytes "flsh" + 4 bytes length 0)
        let flsh_frame = [b'f', b'l', b's', b'h', 0, 0, 0, 0];
        server_stream.write_all(&flsh_frame[..4]).unwrap();

        // Give client thread time to poll partial
        std::thread::sleep(Duration::from_millis(50));

        // Send second half + another full flsh frame concatenated
        let mut second_chunk = Vec::new();
        second_chunk.extend_from_slice(&flsh_frame[4..]); // Complete first frame
        second_chunk.extend_from_slice(&flsh_frame);       // Second concatenated frame
        server_stream.write_all(&second_chunk).unwrap();

        handle.join().unwrap();
    }

    #[test]
    fn test_slimproto_client_recovery_when_scratch_too_small() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server_addr = listener.local_addr().unwrap();

        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x77];

        let handle = std::thread::spawn(move || {
            let mut client = SlimprotoClient::connect(server_addr, mac, "TestModel").unwrap();

            // Give server time to send data
            std::thread::sleep(Duration::from_millis(50));

            // Scratch buffer of 16 bytes is too small for a 26-byte audg frame
            let mut small_scratch = [0u8; 16];
            let err = client.poll_command(&mut small_scratch).unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
            assert_eq!(err.to_string(), "Scratch buffer too small for frame");

            // Following frame is an 8-byte flsh frame, which should now be recoverable!
            let cmd = client.poll_command(&mut small_scratch).unwrap();
            assert_eq!(cmd, Some(ServerCommand::Flush));
        });

        let (mut server_stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 128];
        let _ = server_stream.read(&mut buf).unwrap(); // Consume HELO

        // Prepare a 26-byte audg frame followed by an 8-byte flsh frame
        let mut audg_frame = [0u8; 26];
        audg_frame[0..4].copy_from_slice(b"audg");
        audg_frame[4..8].copy_from_slice(&18u32.to_be_bytes()); // length = 18
        audg_frame[16] = 255;
        audg_frame[18..22].copy_from_slice(&0x00010000u32.to_be_bytes());
        audg_frame[22..26].copy_from_slice(&0x00010000u32.to_be_bytes());

        let flsh_frame = [b'f', b'l', b's', b'h', 0, 0, 0, 0];

        let mut data = Vec::new();
        data.extend_from_slice(&audg_frame);
        data.extend_from_slice(&flsh_frame);
        server_stream.write_all(&data).unwrap();

        handle.join().unwrap();
    }

    #[test]
    fn test_http_audio_stream_get_and_chunked_body() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server_addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let (mut server_stream, _) = listener.accept().unwrap();
            let mut req_buf = Vec::new();
            let mut chunk = [0u8; 128];
            while !req_buf.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = server_stream.read(&mut chunk).unwrap();
                if n == 0 {
                    break;
                }
                req_buf.extend_from_slice(&chunk[..n]);
            }
            let req_str = std::str::from_utf8(&req_buf).unwrap();
            assert!(req_str.starts_with("GET /stream.mp3 HTTP/1.0"));
            assert!(req_str.ends_with("\r\n\r\n"));

            // Send HTTP response headers + partial audio body
            let part1 = b"HTTP/1.0 200 OK\r\nContent-Type: audio/mpeg\r\n\r\n\x01\x02\x03\x04";
            server_stream.write_all(part1).unwrap();
            server_stream.flush().unwrap();

            std::thread::sleep(Duration::from_millis(30));

            // Send remaining audio body
            let part2 = b"\x05\x06\x07\x08";
            server_stream.write_all(part2).unwrap();
            server_stream.flush().unwrap();

            let _ = server_stream.shutdown(std::net::Shutdown::Write);
        });

        let mut stream = HttpAudioStream::connect(
            server_addr,
            "GET /stream.mp3 HTTP/1.0\r\nHost: 127.0.0.1",
        )
        .unwrap();

        assert!(!stream.headers_parsed());
        assert!(!stream.is_eof());

        let mut audio = Vec::new();
        let mut buf = [0u8; 16];

        let start = std::time::Instant::now();
        while !stream.is_eof() && start.elapsed() < Duration::from_secs(3) {
            match stream.poll_audio_bytes(&mut buf) {
                Ok(0) => std::thread::sleep(Duration::from_millis(5)),
                Ok(n) => audio.extend_from_slice(&buf[..n]),
                Err(e) => panic!("Unexpected error: {:?}", e),
            }
        }

        assert!(stream.headers_parsed());
        assert_eq!(audio, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(stream.is_eof());

        handle.join().unwrap();
    }

    #[test]
    fn test_http_audio_stream_rejects_non_200() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server_addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let (mut server_stream, _) = listener.accept().unwrap();
            let mut req_buf = [0u8; 512];
            let _ = server_stream.read(&mut req_buf).unwrap();

            // HALNET-406: Include " 200" in a header line on a 404 response to ensure only the status line is checked!
            let resp = b"HTTP/1.0 404 Not Found\r\nX-RateLimit: 200\r\nContent-Length: 0\r\n\r\n";
            server_stream.write_all(resp).unwrap();
            server_stream.flush().unwrap();
        });

        let mut stream = HttpAudioStream::connect(
            server_addr,
            "GET /missing.mp3 HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n",
        )
        .unwrap();

        let mut buf = [0u8; 16];
        let mut err = None;
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            match stream.poll_audio_bytes(&mut buf) {
                Ok(_) => std::thread::sleep(Duration::from_millis(5)),
                Err(e) => {
                    err = Some(e);
                    break;
                }
            }
        }

        let err = err.expect("Expected Non-200 HTTP response error");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("Non-200 HTTP response"));

        handle.join().unwrap();
    }

    #[test]
    fn test_http_audio_stream_accepts_206_partial_content() {
        // HALNET-406 & HALNET-405: 206 Partial Content must be accepted and clean EOF returns Ok(0)
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server_addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let (mut server_stream, _) = listener.accept().unwrap();
            let mut req_buf = [0u8; 512];
            let _ = server_stream.read(&mut req_buf).unwrap();

            let resp = b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-3/4\r\n\r\n\x10\x20\x30\x40";
            server_stream.write_all(resp).unwrap();
            server_stream.flush().unwrap();
            let _ = server_stream.shutdown(std::net::Shutdown::Write);
        });

        let mut stream = HttpAudioStream::connect(
            server_addr,
            "GET /range.mp3 HTTP/1.1\r\nRange: bytes=0-3\r\n\r\n",
        )
        .unwrap();

        let mut audio = Vec::new();
        let mut buf = [0u8; 16];
        let start = std::time::Instant::now();
        while !stream.is_eof() && start.elapsed() < Duration::from_secs(3) {
            match stream.poll_audio_bytes(&mut buf) {
                Ok(0) => std::thread::sleep(Duration::from_millis(5)),
                Ok(n) => audio.extend_from_slice(&buf[..n]),
                Err(e) => panic!("Unexpected error on 206 Partial Content: {:?}", e),
            }
        }

        assert!(stream.headers_parsed());
        assert!(stream.is_eof());
        assert_eq!(audio, vec![0x10, 0x20, 0x30, 0x40]);
        assert_eq!(stream.poll_audio_bytes(&mut buf).unwrap(), 0);

        handle.join().unwrap();
    }

    #[test]
    fn test_http_audio_stream_icy_metaint_split_across_reads() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server_addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let (mut server_stream, _) = listener.accept().unwrap();
            let mut req_buf = [0u8; 512];
            let _ = server_stream.read(&mut req_buf).unwrap();

            // HTTP 200 with icy-metaint: 4
            let resp_head = b"HTTP/1.0 200 OK\r\nicy-metaint: 4\r\n\r\n";
            server_stream.write_all(resp_head).unwrap();
            server_stream.flush().unwrap();

            // Part 1: 4 audio bytes "ABCD" + 1 byte 0x01 (len=1 -> 16 bytes) + 8 bytes "StreamTi"
            let mut part1 = Vec::new();
            part1.extend_from_slice(b"ABCD");
            part1.push(0x01); // 1 * 16 = 16 bytes metadata
            part1.extend_from_slice(b"StreamTi");
            server_stream.write_all(&part1).unwrap();
            server_stream.flush().unwrap();

            std::thread::sleep(Duration::from_millis(30));

            // Part 2: remaining 8 metadata bytes "tle='X';" + 4 audio bytes "EFGH"
            let mut part2 = Vec::new();
            part2.extend_from_slice(b"tle='X';");
            part2.extend_from_slice(b"EFGH");
            server_stream.write_all(&part2).unwrap();
            server_stream.flush().unwrap();

            let _ = server_stream.shutdown(std::net::Shutdown::Write);
        });

        let mut stream = HttpAudioStream::connect(server_addr, "GET /stream HTTP/1.0\r\n\r\n").unwrap();
        let mut audio = Vec::new();
        let mut buf = [0u8; 16];
        let start = std::time::Instant::now();
        while !stream.is_eof() && start.elapsed() < Duration::from_secs(3) {
            match stream.poll_audio_bytes(&mut buf) {
                Ok(0) => std::thread::sleep(Duration::from_millis(5)),
                Ok(n) => audio.extend_from_slice(&buf[..n]),
                Err(e) => panic!("Unexpected error: {:?}", e),
            }
        }

        assert_eq!(audio, b"ABCDEFGH");
        assert_eq!(stream.total_audio_bytes(), 8);
        assert_eq!(stream.take_icy_metadata(), Some(b"StreamTitle='X';".to_vec()));
        assert_eq!(
            stream.take_response_headers(),
            Some(b"HTTP/1.0 200 OK\r\nicy-metaint: 4\r\n\r\n".to_vec())
        );

        handle.join().unwrap();
    }

    #[test]
    fn test_http_audio_stream_follows_302_redirect() {
        let listener1 = TcpListener::bind("127.0.0.1:0").unwrap();
        let server1_addr = listener1.local_addr().unwrap();

        let listener2 = TcpListener::bind("127.0.0.1:0").unwrap();
        let server2_addr = listener2.local_addr().unwrap();

        // Redirect server (listener1)
        let handle1 = std::thread::spawn(move || {
            let (mut stream1, _) = listener1.accept().unwrap();
            let mut req = [0u8; 512];
            let _ = stream1.read(&mut req).unwrap();

            let resp = format!(
                "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{}/audio.pcm\r\n\r\n",
                server2_addr.port()
            );
            stream1.write_all(resp.as_bytes()).unwrap();
            stream1.flush().unwrap();
        });

        // Target server (listener2)
        let handle2 = std::thread::spawn(move || {
            let (mut stream2, _) = listener2.accept().unwrap();
            let mut req = [0u8; 512];
            let _ = stream2.read(&mut req).unwrap();

            let resp = b"HTTP/1.0 200 OK\r\n\r\n\x11\x22\x33\x44";
            stream2.write_all(resp).unwrap();
            stream2.flush().unwrap();
            let _ = stream2.shutdown(std::net::Shutdown::Write);
        });

        let mut stream = HttpAudioStream::connect(
            server1_addr,
            "GET /old.pcm HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n",
        )
        .unwrap();

        let mut audio = Vec::new();
        let mut buf = [0u8; 16];
        let start = std::time::Instant::now();
        while !stream.is_eof() && start.elapsed() < Duration::from_secs(3) {
            match stream.poll_audio_bytes(&mut buf) {
                Ok(0) => std::thread::sleep(Duration::from_millis(5)),
                Ok(n) => audio.extend_from_slice(&buf[..n]),
                Err(e) => panic!("Unexpected error: {:?}", e),
            }
        }

        assert_eq!(audio, vec![0x11, 0x22, 0x33, 0x44]);
        assert_eq!(stream.total_audio_bytes(), 4);

        handle1.join().unwrap();
        handle2.join().unwrap();
    }

    #[test]
    fn test_http_audio_stream_chunked_transfer_encoding() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server_addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut req = [0u8; 512];
            let _ = stream.read(&mut req).unwrap();

            let resp = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nABCD\r\n2\r\nEF\r\n0\r\n\r\n";
            stream.write_all(resp).unwrap();
            stream.flush().unwrap();
            let _ = stream.shutdown(std::net::Shutdown::Write);
        });

        let mut stream = HttpAudioStream::connect(
            server_addr,
            "GET /chunked.mp3 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        )
        .unwrap();

        let mut audio = Vec::new();
        let mut buf = [0u8; 16];
        let start = std::time::Instant::now();
        while !stream.is_eof() && start.elapsed() < Duration::from_secs(3) {
            match stream.poll_audio_bytes(&mut buf) {
                Ok(0) => std::thread::sleep(Duration::from_millis(5)),
                Ok(n) => audio.extend_from_slice(&buf[..n]),
                Err(e) => panic!("Unexpected error: {:?}", e),
            }
        }

        assert_eq!(audio, b"ABCDEF");
        assert_eq!(stream.total_audio_bytes(), 6);
        assert!(stream.is_eof());

        handle.join().unwrap();
    }

    #[test]
    fn test_slimproto_client_outbound_methods() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server_addr = listener.local_addr().unwrap();
        let mac = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC];

        let handle = std::thread::spawn(move || {
            let mut client = SlimprotoClient::connect_with_options(
                server_addr,
                mac,
                "CustomModel",
                true,
                12345,
            )
            .unwrap();

            client
                .send_stat_with_metrics(StatEvent::Connected, 100, 200, 44100, 44100, 12345, 500, 42)
                .unwrap();
            client.send_resp(b"HTTP/1.0 200 OK\r\n\r\n").unwrap();
            client.send_dsco(DisconnectReason::ResetByLocal).unwrap();
            client.send_meta(b"StreamTitle='Song';").unwrap();
            client.send_setd_name("Living Room").unwrap();
        });

        let (mut stream, _) = listener.accept().unwrap();
        let mut acc = Vec::new();

        let mut read_frame = |acc: &mut Vec<u8>| -> (Vec<u8>, Vec<u8>) {
            loop {
                if acc.len() >= 8 {
                    let len = u32::from_be_bytes([acc[4], acc[5], acc[6], acc[7]]) as usize;
                    if acc.len() >= 8 + len {
                        let header = acc[0..8].to_vec();
                        let payload = acc[8..8 + len].to_vec();
                        acc.drain(..8 + len);
                        return (header, payload);
                    }
                }
                let mut chunk = [0u8; 512];
                let n = stream.read(&mut chunk).unwrap();
                if n == 0 {
                    panic!("Unexpected EOF waiting for frame");
                }
                acc.extend_from_slice(&chunk[..n]);
            }
        };

        // 1. HELO with options (reconnect=true, bytes=12345)
        let (hdr1, payload1) = read_frame(&mut acc);
        assert_eq!(&hdr1[0..4], b"HELO");
        // wlan_flags at payload bytes 24..26 (offsets 32..34 from frame start)
        let wlan_flags = u16::from_be_bytes([payload1[24], payload1[25]]);
        assert_eq!(wlan_flags, 0x4000);
        let bytes_rx = u64::from_be_bytes([
            payload1[26], payload1[27], payload1[28], payload1[29],
            payload1[30], payload1[31], payload1[32], payload1[33],
        ]);
        assert_eq!(bytes_rx, 12345);

        // 2. STAT with metrics
        let (hdr2, payload2) = read_frame(&mut acc);
        assert_eq!(&hdr2[0..4], b"STAT");
        assert_eq!(&payload2[0..4], b"STMc");

        // 3. RESP
        let (hdr3, payload3) = read_frame(&mut acc);
        assert_eq!(&hdr3[0..4], b"RESP");
        assert_eq!(&payload3, b"HTTP/1.0 200 OK\r\n\r\n");

        // 4. DSCO
        let (hdr4, payload4) = read_frame(&mut acc);
        assert_eq!(&hdr4[0..4], b"DSCO");
        assert_eq!(payload4[0], DisconnectReason::ResetByLocal.to_byte());

        // 5. META
        let (hdr5, payload5) = read_frame(&mut acc);
        assert_eq!(&hdr5[0..4], b"META");
        assert_eq!(&payload5, b"StreamTitle='Song';");

        // 6. SETD name
        let (hdr6, payload6) = read_frame(&mut acc);
        assert_eq!(&hdr6[0..4], b"SETD");
        assert_eq!(payload6[0], 0x00);
        assert_eq!(&payload6[1..], b"Living Room");

        handle.join().unwrap();
    }

    #[test]
    fn test_parse_http_location_bracketed_ipv6() {
        let (addr1, host1, path1) = parse_http_location("http://[::1]/stream.mp3").unwrap();
        assert_eq!(addr1.ip(), std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST));
        assert_eq!(addr1.port(), 80);
        assert_eq!(host1, "[::1]");
        assert_eq!(path1, "/stream.mp3");

        let (addr2, host2, path2) = parse_http_location("http://[::1]:8000/stream.mp3").unwrap();
        assert_eq!(addr2.ip(), std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST));
        assert_eq!(addr2.port(), 8000);
        assert_eq!(host2, "[::1]:8000");
        assert_eq!(path2, "/stream.mp3");
    }

    #[test]
    fn test_is_eof_returns_true_when_socket_eof_and_unchunked_empty() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server_addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 128];
            let _ = stream.read(&mut buf);
            // Incomplete/malformed chunk header then disconnect
            stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\npartial_chunk_with_no_crlf").unwrap();
            let _ = stream.shutdown(std::net::Shutdown::Write);
        });

        let mut http = HttpAudioStream::connect(server_addr, "GET /stream HTTP/1.0").unwrap();
        let mut out = [0u8; 64];
        let start = std::time::Instant::now();
        while !http.is_eof() && start.elapsed() < Duration::from_secs(2) {
            let _ = http.poll_audio_bytes(&mut out);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(http.is_eof());
        // The incomplete chunk-size line is held by the chunk decoder, never delivered as audio
        // (before Task 9 these bytes sat in `raw_body_buf`).
        assert!(!http.chunk.last_chunk_seen());
        assert!(http.audio_buf.is_empty());
        handle.join().unwrap();
    }
}

