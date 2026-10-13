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

        let mut buf = [0u8; 128];
        let (bytes_read, src_addr) = socket.recv_from(&mut buf)?;

        if let Some(server) = DiscoveryParser::parse_response(&buf[..bytes_read]) {
            Ok((src_addr, server))
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Unrecognized discovery response",
            ))
        }
    }
}

/// Active SlimProto TCP Streaming Connection.
pub struct SlimprotoClient {
    stream: TcpStream,
    mac: [u8; 6],
    model: &'static str,
}

impl SlimprotoClient {
    /// Connects to LMS server and completes the initial HELO handshake.
    pub fn connect(server_addr: SocketAddr, mac: [u8; 6], model: &'static str) -> std::io::Result<Self> {
        let mut stream = TcpStream::connect_timeout(&server_addr, Duration::from_secs(5))?;
        stream.set_nodelay(true)?;

        // Build and send initial HELO packet
        let mut helo_buf = [0u8; 256];
        let caps = "Model=LouderESP32Plus,mp3,flac,pcm,MaxSampleRate=192000";
        let helo_len = SlimprotoBuilder::build_helo(&mac, 12, caps, &mut helo_buf)
            .map_err(|_| std::io::Error::other("HELO build error"))?;

        stream.write_all(&helo_buf[..helo_len])?;

        Ok(Self { stream, mac, model })
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
        let mut buf = [0u8; 32];
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

    /// Reads from the socket and attempts to parse a complete server command.
    pub fn poll_command<'a>(&mut self, rx_buf: &'a mut [u8]) -> std::io::Result<Option<ServerCommand<'a>>> {
        let bytes_read = match self.stream.read(rx_buf) {
            Ok(0) => return Err(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "Server disconnected")),
            Ok(n) => n,
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
                return Ok(None)
            }
            Err(e) => return Err(e),
        };

        match SlimprotoParser::parse_frame(&rx_buf[..bytes_read]) {
            Ok((cmd, _consumed)) => Ok(Some(cmd)),
            Err(ParseError::BufferTooShort) => Ok(None),
            Err(_) => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Malformed frame")),
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

        // 2. Read STAT packet
        let n2 = server_stream.read(&mut buf).unwrap();
        assert_eq!(n2, 28);
        assert_eq!(&buf[0..4], b"STAT");
        assert_eq!(&buf[8..12], b"STMt");

        handle.join().unwrap();
    }
}
