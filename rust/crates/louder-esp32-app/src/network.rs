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
        let caps = "Model=LouderESP32Plus,mp3,flac,pcm,MaxSampleRate=192000";
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
}
