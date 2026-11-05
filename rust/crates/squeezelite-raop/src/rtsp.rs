//! RTSP 1.0 protocol handler and SDP parser for AirPlay 1 (RAOP).
//!
//! Handles TCP port 5000 requests: OPTIONS, ANNOUNCE, SETUP, RECORD,
//! SET_PARAMETER (volume, progress, binary DMAP metadata, JPEG/PNG artwork),
//! FLUSH, TEARDOWN, and generates conforming responses.

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use crate::crypto::base64_decode;
use crate::crypto::base64_encode;
use crate::crypto::CryptoError;
use crate::crypto::RsaPrivateKey;
use crate::dacp::DacpClient;
use crate::dacp::DacpCommand;
use crate::dmap::ArtworkMimeType;
use crate::dmap::CoverArtwork;
use crate::dmap::DmapError;
use crate::dmap::DmapMetadata;
use crate::dmap::TrackProgress;
use crate::rtp::parse_rtp_info;

/// RTSP protocol errors.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RtspError {
    /// Request line was malformed.
    MalformedRequestLine,
    /// Header line was malformed.
    MalformedHeader,
    /// Method is unrecognized.
    UnknownMethod,
    /// Missing required CSeq header.
    MissingCSeq,
    /// Content length header invalid or missing when body expected.
    InvalidContentLength,
    /// SDP parsing error.
    MalformedSdp,
    /// Session setup failed.
    InvalidSessionState,
    /// Cryptographic error during key decryption or response generation.
    Crypto(CryptoError),
    /// Binary DMAP metadata parsing error.
    Dmap(DmapError),
}

impl fmt::Display for RtspError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedRequestLine => {
                write!(f, "Malformed RTSP request line")
            }
            Self::MalformedHeader => write!(f, "Malformed RTSP header line"),
            Self::UnknownMethod => write!(f, "Unknown RTSP method"),
            Self::MissingCSeq => write!(f, "Missing CSeq header"),
            Self::InvalidContentLength => {
                write!(f, "Invalid Content-Length header")
            }
            Self::MalformedSdp => write!(f, "Malformed SDP payload"),
            Self::InvalidSessionState => {
                write!(f, "Invalid RTSP session state")
            }
            Self::Crypto(e) => write!(f, "Crypto error: {}", e),
            Self::Dmap(e) => write!(f, "DMAP error: {}", e),
        }
    }
}

impl From<CryptoError> for RtspError {
    fn from(err: CryptoError) -> Self {
        Self::Crypto(err)
    }
}

impl From<DmapError> for RtspError {
    fn from(err: DmapError) -> Self {
        Self::Dmap(err)
    }
}

/// Standard RTSP 1.0 Methods supported in RAOP.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RtspMethod {
    Options,
    Announce,
    Setup,
    Record,
    SetParameter,
    Flush,
    Teardown,
    GetParameter,
    Pause,
    Unknown,
}

impl RtspMethod {
    /// Parses an RTSP method token.
    pub fn parse(token: &str) -> Self {
        match token {
            "OPTIONS" => Self::Options,
            "ANNOUNCE" => Self::Announce,
            "SETUP" => Self::Setup,
            "RECORD" => Self::Record,
            "SET_PARAMETER" => Self::SetParameter,
            "FLUSH" => Self::Flush,
            "TEARDOWN" => Self::Teardown,
            "GET_PARAMETER" => Self::GetParameter,
            "PAUSE" => Self::Pause,
            _ => Self::Unknown,
        }
    }

    /// Returns the string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Options => "OPTIONS",
            Self::Announce => "ANNOUNCE",
            Self::Setup => "SETUP",
            Self::Record => "RECORD",
            Self::SetParameter => "SET_PARAMETER",
            Self::Flush => "FLUSH",
            Self::Teardown => "TEARDOWN",
            Self::GetParameter => "GET_PARAMETER",
            Self::Pause => "PAUSE",
            Self::Unknown => "UNKNOWN",
        }
    }
}

/// Parsed RTSP 1.0 request structure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RtspRequest<'a> {
    pub method: RtspMethod,
    pub uri: &'a str,
    pub cseq: u32,
    pub content_type: Option<&'a str>,
    pub content_length: usize,
    pub transport: Option<&'a str>,
    pub apple_challenge: Option<&'a str>,
    pub dacp_id: Option<&'a str>,
    pub active_remote: Option<&'a str>,
    pub rtp_info: Option<&'a str>,
    pub body: &'a [u8],
}

impl<'a> RtspRequest<'a> {
    /// Parses an incoming RTSP 1.0 request buffer.
    pub fn parse(buffer: &'a [u8]) -> Result<Self, RtspError> {
        // Find header-body separator (\r\n\r\n)
        let sep = buffer
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or(RtspError::MalformedHeader)?;

        let header_str =
            core::str::from_utf8(&buffer[..sep]).map_err(|_| RtspError::MalformedHeader)?;
        let body = &buffer[sep + 4..];

        let mut lines = header_str.lines();
        let request_line = lines.next().ok_or(RtspError::MalformedRequestLine)?;

        let mut req_parts = request_line.split_whitespace();
        let method_str = req_parts.next().ok_or(RtspError::MalformedRequestLine)?;
        let uri = req_parts.next().ok_or(RtspError::MalformedRequestLine)?;
        let version = req_parts.next().ok_or(RtspError::MalformedRequestLine)?;

        if !version.starts_with("RTSP/1.") {
            return Err(RtspError::MalformedRequestLine);
        }

        let method = RtspMethod::parse(method_str);

        let mut cseq = None;
        let mut content_type = None;
        let mut content_length = 0usize;
        let mut transport = None;
        let mut apple_challenge = None;
        let mut dacp_id = None;
        let mut active_remote = None;
        let mut rtp_info = None;

        for line in lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let colon_idx = trimmed.find(':').ok_or(RtspError::MalformedHeader)?;
            let key = trimmed[..colon_idx].trim();
            let value = trimmed[colon_idx + 1..].trim();

            if key.eq_ignore_ascii_case("CSeq") {
                cseq = value.parse::<u32>().ok();
            } else if key.eq_ignore_ascii_case("Content-Type") {
                content_type = Some(value);
            } else if key.eq_ignore_ascii_case("Content-Length") {
                let len = value
                    .parse::<usize>()
                    .map_err(|_| RtspError::InvalidContentLength)?;
                content_length = len;
            } else if key.eq_ignore_ascii_case("Transport") {
                transport = Some(value);
            } else if key.eq_ignore_ascii_case("Apple-Challenge") {
                apple_challenge = Some(value);
            } else if key.eq_ignore_ascii_case("DACP-ID") {
                dacp_id = Some(value);
            } else if key.eq_ignore_ascii_case("Active-Remote") {
                active_remote = Some(value);
            } else if key.eq_ignore_ascii_case("RTP-Info") {
                rtp_info = Some(value);
            }
        }

        let cseq_val = cseq.ok_or(RtspError::MissingCSeq)?;

        if body.len() < content_length {
            return Err(RtspError::InvalidContentLength);
        }
        let body_slice = &body[..content_length];

        Ok(Self {
            method,
            uri,
            cseq: cseq_val,
            content_type,
            content_length,
            transport,
            apple_challenge,
            dacp_id,
            active_remote,
            rtp_info,
            body: body_slice,
        })
    }
}

/// Parsed Session Description Protocol (SDP) parameters from ANNOUNCE.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SdpSession {
    pub rsaaeskey: Option<Vec<u8>>,
    pub aesiv: Option<[u8; 16]>,
    pub fmtp: Option<String>,
    pub rtpmap: Option<String>,
}

impl SdpSession {
    /// Parses an SDP body into session cryptographic and codec configuration.
    pub fn parse(sdp_text: &str) -> Result<Self, RtspError> {
        let mut session = Self::default();

        for line in sdp_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Some(rest) = trimmed.strip_prefix("a=rsaaeskey:") {
                let key_b64 = rest.trim();
                if key_b64.len() > 512 {
                    return Err(RtspError::MalformedSdp);
                }
                let decoded = base64_decode(key_b64).map_err(RtspError::Crypto)?;
                session.rsaaeskey = Some(decoded);
            } else if let Some(rest) = trimmed.strip_prefix("a=aesiv:") {
                let decoded = base64_decode(rest.trim()).map_err(RtspError::Crypto)?;
                if decoded.len() != 16 {
                    return Err(RtspError::MalformedSdp);
                }
                let mut iv = [0u8; 16];
                iv.copy_from_slice(&decoded);
                session.aesiv = Some(iv);
            } else if let Some(rest) = trimmed.strip_prefix("a=fmtp:") {
                session.fmtp = Some(rest.trim().to_string());
            } else if let Some(rest) = trimmed.strip_prefix("a=rtpmap:") {
                session.rtpmap = Some(rest.trim().to_string());
            }
        }

        Ok(session)
    }
}

/// Conforming RTSP 1.0 response builder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RtspResponse {
    pub status_code: u16,
    pub status_text: &'static str,
    pub cseq: u32,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl RtspResponse {
    /// Creates a generic 200 OK RTSP response.
    pub fn ok(cseq: u32) -> Self {
        let headers = vec![(
            "Audio-Jack-Status".to_string(),
            "connected; type=analog".to_string(),
        )];
        Self {
            status_code: 200,
            status_text: "OK",
            cseq,
            headers,
            body: Vec::new(),
        }
    }

    /// Builds standard OPTIONS response announcing supported public verbs.
    pub fn options(cseq: u32) -> Self {
        let mut resp = Self::ok(cseq);
        let verbs = concat!(
            "ANNOUNCE, SETUP, RECORD, PAUSE, FLUSH, TEARDOWN, ",
            "OPTIONS, GET_PARAMETER, SET_PARAMETER"
        );
        resp.headers.push(("Public".to_string(), verbs.to_string()));
        resp
    }

    /// Builds standard ANNOUNCE response with optional Apple-Response auth.
    pub fn announce(cseq: u32, apple_response: Option<&str>) -> Self {
        let mut resp = Self::ok(cseq);
        if let Some(auth) = apple_response {
            resp.headers
                .push(("Apple-Response".to_string(), auth.to_string()));
        }
        resp
    }

    /// Builds standard SETUP response with ports and session ID.
    pub fn setup(cseq: u32, server_port: u16, control_port: u16, timing_port: u16) -> Self {
        let mut resp = Self::ok(cseq);
        let transport = format!(
            "RTP/AVP/UDP;unicast;mode=record;\
             server_port={};control_port={};timing_port={}",
            server_port, control_port, timing_port
        );
        resp.headers.push(("Transport".to_string(), transport));
        resp.headers.push(("Session".to_string(), "1".to_string()));
        resp
    }

    /// Builds standard RECORD response with audio latency indication.
    pub fn record(cseq: u32, latency: u32) -> Self {
        let mut resp = Self::ok(cseq);
        resp.headers
            .push(("Audio-Latency".to_string(), latency.to_string()));
        resp
    }

    /// Builds standard SET_PARAMETER response.
    pub fn set_parameter(cseq: u32) -> Self {
        Self::ok(cseq)
    }

    /// Builds standard FLUSH response.
    pub fn flush(cseq: u32) -> Self {
        Self::ok(cseq)
    }

    /// Builds standard TEARDOWN response with connection close.
    pub fn teardown(cseq: u32) -> Self {
        let mut resp = Self::ok(cseq);
        resp.headers
            .push(("Connection".to_string(), "close".to_string()));
        resp
    }

    /// Serializes response to standard RTSP 1.0 byte stream.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let status_line = format!("RTSP/1.0 {} {}\r\n", self.status_code, self.status_text);
        out.extend_from_slice(status_line.as_bytes());

        let cseq_line = format!("CSeq: {}\r\n", self.cseq);
        out.extend_from_slice(cseq_line.as_bytes());

        for (k, v) in &self.headers {
            let clean_v: String = v.chars().filter(|&c| c != '\r' && c != '\n').collect();
            let header_line = format!("{}: {}\r\n", k, clean_v);
            out.extend_from_slice(header_line.as_bytes());
        }

        if !self.body.is_empty() {
            let cl_line = format!("Content-Length: {}\r\n", self.body.len());
            out.extend_from_slice(cl_line.as_bytes());
        }

        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&self.body);
        out
    }
}

/// AirPlay 1 Server Connection State.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ServerState {
    Idle,
    Announced,
    Configured,
    Streaming,
    Paused,
}

/// Pure-Rust RTSP 1.0 Server Engine.
pub struct RtspServer {
    state: ServerState,
    rsa_key: RsaPrivateKey,
    sdp_session: Option<SdpSession>,
    aes_session_key: Option<[u8; 16]>,
    aes_iv: Option<[u8; 16]>,
    server_audio_port: u16,
    server_control_port: u16,
    server_timing_port: u16,
    client_control_port: u16,
    client_timing_port: u16,
    volume_linear: f32,
    latency_frames: u32,
    mac_address: [u8; 6],
    ip_address: [u8; 4],
    dacp_id: Option<String>,
    active_remote: Option<String>,
    metadata: Option<DmapMetadata>,
    artwork: Option<CoverArtwork>,
    progress: Option<TrackProgress>,
    last_flush_seq: Option<u16>,
    last_flush_rtptime: Option<u32>,
    torn_down: bool,
}

impl RtspServer {
    /// Creates a new RTSP server state engine.
    pub fn new(rsa_key: RsaPrivateKey, mac_address: [u8; 6], ip_address: [u8; 4]) -> Self {
        Self {
            state: ServerState::Idle,
            rsa_key,
            sdp_session: None,
            aes_session_key: None,
            aes_iv: None,
            server_audio_port: 6000,
            server_control_port: 6001,
            server_timing_port: 6002,
            client_control_port: 0,
            client_timing_port: 0,
            volume_linear: 1.0,
            latency_frames: 11025,
            mac_address,
            ip_address,
            dacp_id: None,
            active_remote: None,
            metadata: None,
            artwork: None,
            progress: None,
            last_flush_seq: None,
            last_flush_rtptime: None,
            torn_down: false,
        }
    }

    /// Current server lifecycle state.
    pub fn state(&self) -> ServerState {
        self.state
    }

    /// Sets the server IPv4 address.
    pub fn set_ip_address(&mut self, ip: [u8; 4]) {
        self.ip_address = ip;
    }

    /// Decrypted 16-byte AES audio session key (if session announced).
    pub fn session_key(&self) -> Option<&[u8; 16]> {
        self.aes_session_key.as_ref()
    }

    /// 16-byte AES audio IV (if session announced).
    pub fn aes_iv(&self) -> Option<&[u8; 16]> {
        self.aes_iv.as_ref()
    }

    /// Current volume on linear scale 0.0 to 1.0.
    pub fn volume(&self) -> f32 {
        self.volume_linear
    }

    /// Active SDP session parameters.
    pub fn sdp_session(&self) -> Option<&SdpSession> {
        self.sdp_session.as_ref()
    }

    /// Server UDP audio receiving port.
    pub fn audio_port(&self) -> u16 {
        self.server_audio_port
    }

    /// Server UDP control port.
    pub fn control_port(&self) -> u16 {
        self.server_control_port
    }

    /// Server UDP timing port.
    pub fn timing_port(&self) -> u16 {
        self.server_timing_port
    }

    /// Sender's UDP control port extracted from `SETUP` `Transport` header.
    #[must_use]
    pub fn client_control_port(&self) -> u16 {
        self.client_control_port
    }

    /// Sender's UDP timing port extracted from `SETUP` `Transport` header.
    #[must_use]
    pub fn client_timing_port(&self) -> u16 {
        self.client_timing_port
    }

    /// Latest DMAP track metadata (`minm`, `asar`, `asal`, `astm`) received via
    /// `SET_PARAMETER`.
    #[must_use]
    pub fn metadata(&self) -> Option<&DmapMetadata> {
        self.metadata.as_ref()
    }

    /// Takes the latest DMAP track metadata out of the server state.
    pub fn take_metadata(&mut self) -> Option<DmapMetadata> {
        self.metadata.take()
    }

    /// Latest cover artwork (`image/jpeg` or `image/png`) received via
    /// `SET_PARAMETER`.
    #[must_use]
    pub fn artwork(&self) -> Option<&CoverArtwork> {
        self.artwork.as_ref()
    }

    /// Takes the latest cover artwork out of the server state.
    pub fn take_artwork(&mut self) -> Option<CoverArtwork> {
        self.artwork.take()
    }

    /// Latest track playback progress (`progress: start/curr/end`) received via
    /// `SET_PARAMETER`.
    #[must_use]
    pub fn progress(&self) -> Option<&TrackProgress> {
        self.progress.as_ref()
    }

    /// Sender's `DACP-ID` header value, if announced.
    #[must_use]
    pub fn dacp_id(&self) -> Option<&str> {
        self.dacp_id.as_deref()
    }

    /// Sender's `Active-Remote` token header value, if announced.
    #[must_use]
    pub fn active_remote(&self) -> Option<&str> {
        self.active_remote.as_deref()
    }

    /// Returns a configured `DacpClient` if `Active-Remote` has been captured.
    #[must_use]
    pub fn dacp_client(&self) -> Option<DacpClient> {
        let active_remote = self.active_remote.as_deref()?;
        let dacp_id = self.dacp_id.as_deref().unwrap_or("");
        Some(DacpClient::new(dacp_id, active_remote))
    }

    /// Builds an HTTP/1.0 `GET /ctrl-int/1/<command>` DACP request using the
    /// active session's `Active-Remote` and `DACP-ID` headers.
    #[must_use]
    pub fn build_dacp_command(&self, cmd: DacpCommand) -> Option<Vec<u8>> {
        self.dacp_client()
            .and_then(|client| client.build_command_request(cmd))
    }

    /// Sequence number from the most recent `FLUSH` (or `RECORD`) `RTP-Info`.
    #[must_use]
    pub fn last_flush_seq(&self) -> Option<u16> {
        self.last_flush_seq
    }

    /// RTP timestamp from the most recent `FLUSH` (or `RECORD`) `RTP-Info`.
    #[must_use]
    pub fn last_flush_rtptime(&self) -> Option<u32> {
        self.last_flush_rtptime
    }

    /// Parses client ports from Transport header: `control_port=X;timing_port=Y`.
    fn extract_client_ports(&mut self, transport_str: &str) {
        for part in transport_str.split(';') {
            let trimmed = part.trim();
            if let Some(val) = trimmed.strip_prefix("control_port=") {
                if let Ok(p) = val.parse::<u16>() {
                    self.client_control_port = p;
                }
            } else if let Some(val) = trimmed.strip_prefix("timing_port=") {
                if let Ok(p) = val.parse::<u16>() {
                    self.client_timing_port = p;
                }
            }
        }
    }

    /// Captures `DACP-ID` and `Active-Remote` headers when present on any RTSP
    /// request.
    fn capture_dacp_headers(&mut self, req: &RtspRequest<'_>) {
        if let Some(id) = req.dacp_id {
            let trimmed = id.trim();
            if !trimmed.is_empty() {
                self.dacp_id = Some(trimmed.to_string());
            }
        }
        if let Some(remote) = req.active_remote {
            let trimmed = remote.trim();
            if !trimmed.is_empty() {
                self.active_remote = Some(trimmed.to_string());
            }
        }
    }

    /// Generates Apple-Response from Apple-Challenge header value.
    fn generate_apple_response(&self, challenge_b64: &str) -> Result<String, RtspError> {
        let raw_chal = base64_decode(challenge_b64).map_err(RtspError::Crypto)?;
        let mut buf = [0u8; 32];
        let copy_len = core::cmp::min(raw_chal.len(), 16);
        buf[..copy_len].copy_from_slice(&raw_chal[..copy_len]);

        let p_ip = copy_len;
        buf[p_ip..p_ip + 4].copy_from_slice(&self.ip_address);

        let p_mac = p_ip + 4;
        buf[p_mac..p_mac + 6].copy_from_slice(&self.mac_address);

        let signed = self
            .rsa_key
            .sign_apple_challenge(&buf)
            .map_err(RtspError::Crypto)?;
        let b64 = base64_encode(&signed);
        let trimmed_b64 = b64.trim_end_matches('=').to_string();
        Ok(trimmed_b64)
    }

    fn handle_set_parameter(&mut self, req: &RtspRequest<'_>) -> Result<(), RtspError> {
        let is_dmap = req.content_type.is_some_and(|ct| {
            let media = ct.split(';').next().unwrap_or(ct).trim();
            media.eq_ignore_ascii_case("application/x-dmap-tagged")
        });

        if is_dmap {
            let meta = DmapMetadata::parse(req.body)?;
            self.metadata = Some(meta);
            return Ok(());
        }

        if let Some(mime) = req
            .content_type
            .and_then(ArtworkMimeType::from_content_type)
        {
            let rtp_ts = req.rtp_info.and_then(|info| parse_rtp_info(info).1);
            let copy_len = core::cmp::min(req.body.len(), 65_536);
            self.artwork = Some(CoverArtwork::new(
                mime,
                req.body[..copy_len].to_vec(),
                rtp_ts,
            ));
            return Ok(());
        }

        if let Ok(body_str) = core::str::from_utf8(req.body) {
            for line in body_str.lines() {
                let trimmed = line.trim();
                if let Some(val) = trimmed.strip_prefix("volume:") {
                    if let Ok(db) = val.trim().parse::<f32>() {
                        if !db.is_finite() {
                            continue;
                        }
                        // -144.0 dB = mute (0.0), 0.0 dB = max (1.0)
                        if db <= -144.0 {
                            self.volume_linear = 0.0;
                        } else {
                            let v = 1.0 + db / 30.0;
                            self.volume_linear = v.clamp(0.0, 1.0);
                        }
                    }
                } else if trimmed.starts_with("progress:") {
                    if let Some(prog) = TrackProgress::parse(trimmed) {
                        self.progress = Some(prog);
                    }
                }
            }
        }
        Ok(())
    }

    /// Processes an incoming RTSP request and produces a conforming response.
    pub fn handle_request(&mut self, req: &RtspRequest<'_>) -> Result<RtspResponse, RtspError> {
        self.capture_dacp_headers(req);

        match req.method {
            RtspMethod::Options => Ok(RtspResponse::options(req.cseq)),
            RtspMethod::Announce => {
                let sdp_str =
                    core::str::from_utf8(req.body).map_err(|_| RtspError::MalformedSdp)?;
                let sdp = SdpSession::parse(sdp_str)?;

                if let Some(ref enc_key) = sdp.rsaaeskey {
                    let key = self
                        .rsa_key
                        .decrypt_session_key(enc_key)
                        .map_err(RtspError::Crypto)?;
                    self.aes_session_key = Some(key);
                }
                if let Some(iv) = sdp.aesiv {
                    self.aes_iv = Some(iv);
                }

                let auth_resp = if let Some(chal) = req.apple_challenge {
                    Some(self.generate_apple_response(chal)?)
                } else {
                    None
                };

                self.sdp_session = Some(sdp);
                self.state = ServerState::Announced;
                self.torn_down = false;
                Ok(RtspResponse::announce(req.cseq, auth_resp.as_deref()))
            }
            RtspMethod::Setup => {
                if let Some(transport) = req.transport {
                    self.extract_client_ports(transport);
                }
                self.state = ServerState::Configured;
                self.torn_down = false;
                Ok(RtspResponse::setup(
                    req.cseq,
                    self.server_audio_port,
                    self.server_control_port,
                    self.server_timing_port,
                ))
            }
            RtspMethod::Record => {
                if self.torn_down {
                    return Err(RtspError::InvalidSessionState);
                }
                if let Some(info) = req.rtp_info {
                    let (seq, rtptime) = parse_rtp_info(info);
                    if seq.is_some() {
                        self.last_flush_seq = seq;
                    }
                    if rtptime.is_some() {
                        self.last_flush_rtptime = rtptime;
                    }
                }
                self.state = ServerState::Streaming;
                Ok(RtspResponse::record(req.cseq, self.latency_frames))
            }
            RtspMethod::SetParameter => {
                self.handle_set_parameter(req)?;
                Ok(RtspResponse::set_parameter(req.cseq))
            }
            RtspMethod::GetParameter => Ok(RtspResponse::ok(req.cseq)),
            RtspMethod::Flush => {
                if let Some(info) = req.rtp_info {
                    let (seq, rtptime) = parse_rtp_info(info);
                    self.last_flush_seq = seq;
                    self.last_flush_rtptime = rtptime;
                }
                Ok(RtspResponse::flush(req.cseq))
            }
            RtspMethod::Pause => {
                self.state = ServerState::Paused;
                Ok(RtspResponse::ok(req.cseq))
            }
            RtspMethod::Teardown => {
                self.state = ServerState::Idle;
                self.torn_down = true;
                self.aes_session_key = None;
                self.aes_iv = None;
                self.sdp_session = None;
                self.dacp_id = None;
                self.active_remote = None;
                self.metadata = None;
                self.artwork = None;
                self.progress = None;
                self.last_flush_seq = None;
                self.last_flush_rtptime = None;
                Ok(RtspResponse::teardown(req.cseq))
            }
            RtspMethod::Unknown => Err(RtspError::UnknownMethod),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_rejected_after_teardown() {
        let key = RsaPrivateKey::from_be_bytes(&[1], &[1]);
        let mut server = RtspServer::new(key, [0; 6], [127, 0, 0, 1]);

        let teardown_raw = b"TEARDOWN rtsp://127.0.0.1/1 RTSP/1.0\r\nCSeq: 1\r\n\r\n";
        let teardown_req = RtspRequest::parse(teardown_raw).expect("parse teardown");
        assert!(server.handle_request(&teardown_req).is_ok());

        let record_raw = b"RECORD rtsp://127.0.0.1/1 RTSP/1.0\r\nCSeq: 2\r\n\r\n";
        let record_req = RtspRequest::parse(record_raw).expect("parse record");
        assert!(matches!(
            server.handle_request(&record_req),
            Err(RtspError::InvalidSessionState)
        ));
    }
}
