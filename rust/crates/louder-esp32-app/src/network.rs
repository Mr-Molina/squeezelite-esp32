//! LMS Auto-Discovery and Network Protocol Client
//!
//! Works across host environments and ESP32 with `std::net` (esp-idf-svc runtime).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::time::Duration;

use squeezelite_proto::{
    DiscoveredServer, DiscoveryBuilder, DiscoveryParser, ParseError, ServerCommand,
    SlimprotoBuilder, SlimprotoParser, StatEvent,
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
    model: &'static str,
    acc_buf: Vec<u8>,
}

impl SlimprotoClient {
    /// Connects to LMS server and completes the initial HELO handshake.
    pub fn connect(server_addr: SocketAddr, mac: [u8; 6], model: &'static str) -> std::io::Result<Self> {
        let mut stream = TcpStream::connect_timeout(&server_addr, Duration::from_secs(5))?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(Duration::from_millis(50)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        stream.set_nonblocking(true)?;

        // Build and send initial HELO packet
        let mut helo_buf = [0u8; 256];
        let caps = "Model=LouderESP32Plus,mp3,flac,aac,ogg,pcm,MaxSampleRate=192000";
        let helo_len = SlimprotoBuilder::build_helo(&mac, 12, caps, &mut helo_buf)
            .map_err(|_| std::io::Error::other("HELO build error"))?;

        stream.write_all(&helo_buf[..helo_len])?;

        Ok(Self {
            stream,
            mac,
            model,
            acc_buf: Vec::with_capacity(8192),
        })
    }

    /// Returns the player MAC address.
    pub fn mac(&self) -> &[u8; 6] {
        &self.mac
    }

    /// Returns the player model string.
    pub fn model(&self) -> &'static str {
        self.model
    }

    /// Returns the remote peer address of the connected LMS server.
    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.stream.peer_addr()
    }

    /// Sends a STAT heartbeat or event update to LMS.
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

        self.stream.write_all(&buf[..len])
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
        const MAX_ACC_BUF: usize = 8192;

        // 1. If self.acc_buf does not yet have a complete frame, read available bytes from self.stream
        let has_complete_frame = match Self::peek_frame_len(&self.acc_buf) {
            Ok(len) => self.acc_buf.len() >= len,
            Err(_) => false,
        };

        if !has_complete_frame {
            let mut temp_buf = [0u8; 1024];
            match self.stream.read(&mut temp_buf) {
                Ok(0) => return Err(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "Server disconnected")),
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
                Err(e) => return Err(e),
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

        // Single parse site at the end!
        let (cmd, _) = SlimprotoParser::parse_frame(&scratch[..consumed])
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "Malformed frame"))?;
        Ok(Some(cmd))
    }
}

/// Pure-Rust HTTP Audio Stream Client for fetching streamed audio over TCP.
pub struct HttpAudioStream {
    stream: TcpStream,
    header_buf: Vec<u8>,
    pending_body: Vec<u8>,
    headers_parsed: bool,
    eof: bool,
}

impl HttpAudioStream {
    pub const MAX_HTTP_HEADER_LEN: usize = 4096;

    /// Connects to the HTTP streaming audio server and issues the initial GET request.
    pub fn connect(server_addr: SocketAddr, http_headers: &str) -> std::io::Result<Self> {
        let mut stream = TcpStream::connect_timeout(&server_addr, Duration::from_secs(5))?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(Duration::from_millis(50)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        stream.set_nonblocking(true)?;

        if !http_headers.is_empty() {
            stream.write_all(http_headers.as_bytes())?;
            if !http_headers.ends_with("\r\n\r\n") {
                if http_headers.ends_with("\r\n") {
                    stream.write_all(b"\r\n")?;
                } else {
                    stream.write_all(b"\r\n\r\n")?;
                }
            }
        }

        Ok(Self {
            stream,
            header_buf: Vec::with_capacity(512),
            pending_body: Vec::with_capacity(2048),
            headers_parsed: false,
            eof: false,
        })
    }

    /// Returns `true` if the server disconnected (EOF) and all pending body bytes have been drained.
    pub fn is_eof(&self) -> bool {
        self.eof && self.pending_body.is_empty()
    }

    /// Returns `true` once the HTTP response headers have been successfully parsed and verified.
    pub fn headers_parsed(&self) -> bool {
        self.headers_parsed
    }

    /// Polls audio stream bytes non-blockingly into `out`.
    ///
    /// During the initial header phase, incoming chunks are parsed to strip the HTTP response
    /// header and verify HTTP/200 status. Once parsed, raw audio stream bytes are written directly
    /// to `out`.
    pub fn poll_audio_bytes(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }

        if !self.pending_body.is_empty() {
            let n = out.len().min(self.pending_body.len());
            out[..n].copy_from_slice(&self.pending_body[..n]);
            self.pending_body.drain(..n);
            return Ok(n);
        }

        if self.eof {
            return Ok(0);
        }

        if !self.headers_parsed {
            let mut temp = [0u8; 1024];
            let n = match self.stream.read(&mut temp) {
                Ok(0) => {
                    self.eof = true;
                    return Ok(0);
                }
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    return Ok(0);
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

            let delim_match = self
                .header_buf
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|p| (p, 4))
                .or_else(|| {
                    self.header_buf
                        .windows(2)
                        .position(|w| w == b"\n\n")
                        .map(|p| (p, 2))
                });

            if let Some((pos, delim_len)) = delim_match {
                let header_slice = &self.header_buf[..pos];
                let is_valid_proto =
                    header_slice.starts_with(b"HTTP/1.") || header_slice.starts_with(b"ICY ");
                let is_200 = header_slice.windows(4).any(|w| w == b" 200");
                if !is_valid_proto || !is_200 {
                    self.header_buf.clear();
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Non-200 HTTP response",
                    ));
                }

                let body_start = pos + delim_len;
                if body_start < self.header_buf.len() {
                    self.pending_body.extend_from_slice(&self.header_buf[body_start..]);
                }
                self.header_buf.clear();
                self.headers_parsed = true;

                let copied = out.len().min(self.pending_body.len());
                if copied > 0 {
                    out[..copied].copy_from_slice(&self.pending_body[..copied]);
                    self.pending_body.drain(..copied);
                }
                return Ok(copied);
            } else {
                return Ok(0);
            }
        }

        // Once self.headers_parsed is true and self.pending_body is empty:
        match self.stream.read(out) {
            Ok(0) => {
                self.eof = true;
                Ok(0)
            }
            Ok(n) => Ok(n),
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                Ok(0)
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

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

            let resp = b"HTTP/1.0 404 Not Found\r\nContent-Length: 0\r\n\r\n";
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
}
