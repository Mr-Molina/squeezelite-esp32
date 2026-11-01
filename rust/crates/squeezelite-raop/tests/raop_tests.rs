//! Comprehensive unit and golden vector tests for pure-Rust AirPlay 1 (RAOP) engine.

use squeezelite_raop::crypto::airport_express_key;
use squeezelite_raop::crypto::base64_decode;
use squeezelite_raop::crypto::base64_encode;
use squeezelite_raop::crypto::montgomery_mod_pow;
use squeezelite_raop::crypto::unpad_pkcs1_v15_type2;
use squeezelite_raop::crypto::Aes128Cbc;
use squeezelite_raop::crypto::BigUint;
use squeezelite_raop::crypto::CryptoError;
use squeezelite_raop::crypto::RsaPrivateKey;
use squeezelite_raop::dacp::DacpClient;
use squeezelite_raop::dacp::DacpCommand;
use squeezelite_raop::dmap::ArtworkMimeType;
use squeezelite_raop::dmap::DmapError;
use squeezelite_raop::dmap::DmapMetadata;
use squeezelite_raop::dmap::TrackProgress;
use squeezelite_raop::mdns::MdnsBuilder;
use squeezelite_raop::mdns::MdnsConfig;
use squeezelite_raop::rtp::build_resend_request;
use squeezelite_raop::rtp::ms_to_ntp;
use squeezelite_raop::rtp::ntp_to_ms;
use squeezelite_raop::rtp::parse_rtp_info;
use squeezelite_raop::rtp::seq_diff;
use squeezelite_raop::rtp::unwrap_retransmit_packet;
use squeezelite_raop::rtp::NtpTimingPacket;
use squeezelite_raop::rtp::RtpAudioReceiver;
use squeezelite_raop::rtp::RtpControlPacket;
use squeezelite_raop::rtp::RtpHeader;
use squeezelite_raop::rtp::RtpJitterBuffer;
use squeezelite_raop::rtp::RtpReceiverError;
use squeezelite_raop::rtp::RtpResendRequest;
use squeezelite_raop::rtp::RtpSyncPacket;
use squeezelite_raop::rtp::SyncDriftTracker;
use squeezelite_raop::rtsp::RtspError;
use squeezelite_raop::rtsp::RtspMethod;
use squeezelite_raop::rtsp::RtspRequest;
use squeezelite_raop::rtsp::RtspServer;
use squeezelite_raop::rtsp::ServerState;

const GOLDEN_AES_KEY: [u8; 16] = [
    0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF,
    0x00,
];

// 1024-bit RSA Key Modulus (128 bytes)
const N1024_HEX: &str = concat!(
    "aa7aabcb5599f7a03fd7b3b7ae2c79363249a22aa1c474639c601d0e98e5220c",
    "6f8d82372254e81b131a9e7be08b76af04e1b9d1d13cdcf61eb6cfa2fb78b888",
    "783b6a6dc4232ff187128e980e88c8aae6910235c9fb4a363f64c1ad6a6dffce",
    "d92579f667a06b41995f0be98e8ecdaf836772d2193396ce1ccfa8c191dc9d53"
);

// 1024-bit RSA Private Exponent (128 bytes)
const D1024_HEX: &str = concat!(
    "413f601f1bad291c5932fa62d3b3cbd4955c9f4787d083e75803653ba0031a8b",
    "fdea002b8fe5454c8f7c41aa11dfeed5bf9d911c1fe41681d0719178c49c88ab",
    "7a8e84ba11baeb80e8f97df9878c9da36aec1f01a961005e8ad55db4ba2f44a2",
    "bf344e3ec06188be54858294d8408c7b77dbc98c6d02f97024755a93087c1641"
);

// Encrypted AES session key under 1024-bit key with PKCS#1 v1.5 padding
const CT1024_B64: &str = concat!(
    "hdGREqok0+EpO8q94l3/t6u3xT/jAaMVgJdUfSju5Th2ORVHl44YjQPYHXV3bT/v33N3",
    "lKaePvUrb1BZ6KJ019Z8N7OkiZNVT9+rNMBpZ7gRIukRYkmwxjS8qQP9bL8k+mhASWyP",
    "cAjUosPDzEfF3RdGUqSHmjQNeNM+FBFs6hs="
);

// Encrypted AES session key under 2048-bit Airport Express key with PKCS#1 v1.5 padding
const CT2048_PKCS1_B64: &str = concat!(
    "FqQK+xm2PQ0e+qwm4aFnbxq7wMcEi6VyUOKwPnYqROLFZ1ZtRdyam+eC8ocOVYVDiddV",
    "R1D2PPDCYeytdmu4f6hbXz16wta/9oGkjhtISBmKW/5X6sEhiTT9SDjnDpSNBEQ3dPE/",
    "K/zQknsTFKoEHMh2zu4phrysU6mZ6ZMTg8Mv32+HhlJQIK5CjH+e4i7jm616lpOeWkGB",
    "JFuFlwC2KlE884JtupLnP03R2YEQ30xjUxQ6AlCsN6BHrXFXwhSqMO3Tlc2QBjyI62Gr",
    "Al4BW9WJb2gmRF/gR/JzNX8D2r/00GUS4G7qM0NZo2KcBjb1JGqS964yFNNa2CTGDavV",
    "AA=="
);

// Encrypted AES session key under 2048-bit Airport Express key with OAEP padding
const CT2048_OAEP_B64: &str = concat!(
    "N+L5GvPLwoErSURs01KvbNP6CVp+T0CRP9pys3q6vbgHkn5GpslsU9JEcIcT4nzxLpjk",
    "Ir4uLNCE7RF70tQjxTxLXd4WZMuclBWeXxzC6lC0SQdUMWJf8Gn8EcQesumslJ+1xUr/",
    "CNMWwsaEwnJ2XIW7FBXv9gHSvs/JaKzIICzY+U0dJxRZ8XZmr81BqIo0JeotzfwhJhZi",
    "4fcDeFHaNO4zN9hPI4kRqC+Hg7PgiTEmBvtdm5lJnROUXoq4OaYJeyUPpIt9zeNeyGG4",
    "ZZsWQvN0uHTzFsE3FLj09KTntOQEORtJwdAIjHI3xcPObE0AHwW63JkFV8eZRQ5lSYTa",
    "cg=="
);

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chars = hex.chars();
    while let (Some(a), Some(b)) = (chars.next(), chars.next()) {
        let byte = u8::from_str_radix(&format!("{}{}", a, b), 16).expect("valid hex");
        out.push(byte);
    }
    out
}

#[test]
fn test_biguint_arithmetic_and_comparison() {
    let a = BigUint::from_u32(100);
    let b = BigUint::from_u32(250);
    let c = a.add(&b);
    assert_eq!(c, BigUint::from_u32(350));
    assert_eq!(c.sub(&a), b);
    assert!(b > a);
    assert!(c > b);

    let mul = a.mul(&b);
    assert_eq!(mul, BigUint::from_u32(25000));

    // Multi-limb
    let big1 = BigUint::from_be_bytes(&[0xFF, 0xFF, 0xFF, 0xFF, 0x01]);
    let big2 = BigUint::from_be_bytes(&[0x02]);
    let sum = big1.add(&big2);
    assert_eq!(sum.to_be_bytes(), vec![0xFF, 0xFF, 0xFF, 0xFF, 0x03]);
}

#[test]
fn test_montgomery_mod_pow_small() {
    let base = BigUint::from_u32(3);
    let exp = BigUint::from_u32(5);
    let modulus = BigUint::from_u32(7);
    let res = montgomery_mod_pow(&base, &exp, &modulus).expect("mod pow");
    // 3^5 = 243; 243 % 7 = 5
    assert_eq!(res, BigUint::from_u32(5));
}

#[test]
fn test_rsa_1024_session_key_decryption_pkcs1_v15() {
    let n_bytes = hex_to_bytes(N1024_HEX);
    let d_bytes = hex_to_bytes(D1024_HEX);
    let ct = base64_decode(CT1024_B64).expect("valid b64");

    let rsa = RsaPrivateKey::from_be_bytes(&n_bytes, &d_bytes);
    assert_eq!(rsa.modulus_bytes(), 128);

    let decrypted_key = rsa.decrypt_session_key(&ct).expect("decryption succeeds");
    assert_eq!(decrypted_key, GOLDEN_AES_KEY);
}

#[test]
fn test_rsa_2048_airport_express_key_pkcs1_v15() {
    let rsa = airport_express_key();
    assert_eq!(rsa.modulus_bytes(), 256);

    let ct = base64_decode(CT2048_PKCS1_B64).expect("valid b64");
    let decrypted_key = rsa.decrypt_session_key(&ct).expect("decryption succeeds");
    assert_eq!(decrypted_key, GOLDEN_AES_KEY);
}

#[test]
fn test_rsa_2048_airport_express_key_oaep() {
    let rsa = airport_express_key();
    let ct = base64_decode(CT2048_OAEP_B64).expect("valid b64");
    let decrypted_key = rsa.decrypt_session_key(&ct).expect("decryption succeeds");
    assert_eq!(decrypted_key, GOLDEN_AES_KEY);
}

#[test]
fn test_rsa_padding_validation_and_errors() {
    // Plaintext without 0x00, 0x02 prefix
    let invalid_prefix = [0x01, 0x01, 0xFF, 0x00, 0x01];
    assert_eq!(
        unpad_pkcs1_v15_type2(&invalid_prefix),
        Err(CryptoError::InvalidPadding)
    );

    // Plaintext with too short padding (< 8 bytes)
    let short_pad = [0x00, 0x02, 0xAA, 0xBB, 0x00, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    assert_eq!(
        unpad_pkcs1_v15_type2(&short_pad),
        Err(CryptoError::InvalidPadding)
    );

    // Plaintext with wrong key length (not 16 bytes)
    let wrong_len = [
        0x00, 0x02, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 1, 2, 3, 4, 5,
    ];
    assert_eq!(
        unpad_pkcs1_v15_type2(&wrong_len),
        Err(CryptoError::InvalidKeyLength)
    );
}

#[test]
fn test_aes128_cbc_audio_packet_decryption() {
    let iv = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f, 0x10,
    ];
    let ct = hex_to_bytes("7be957563e9135f57b777a5ec15efe9853af3807d7ba56b47b4c27e6904c2116212223");
    let pt_expected = hex_to_bytes("0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20212223");

    let mut buf = ct.clone();
    let cipher = Aes128Cbc::new(&GOLDEN_AES_KEY);
    cipher.decrypt_in_place(&iv, &mut buf);

    assert_eq!(buf, pt_expected);
}

#[test]
fn test_rtsp_options_parsing_and_response() {
    let req_bytes = concat!(
        "OPTIONS rtsp://192.168.1.50/123456 RTSP/1.0\r\n",
        "CSeq: 1\r\n",
        "User-Agent: iTunes/10.6\r\n\r\n"
    );

    let req = RtspRequest::parse(req_bytes.as_bytes()).expect("parse options");
    assert_eq!(req.method, RtspMethod::Options);
    assert_eq!(req.cseq, 1);

    let rsa = airport_express_key();
    let mut server = RtspServer::new(rsa, [0x24, 0x0A, 0xC4, 0x01, 0x02, 0x03], [192, 168, 1, 50]);
    let resp = server.handle_request(&req).expect("handle options");

    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.cseq, 1);
    let resp_bytes = resp.to_bytes();
    let resp_str = core::str::from_utf8(&resp_bytes).expect("utf8");
    assert!(resp_str.contains("Public: ANNOUNCE, SETUP, RECORD"));
    assert!(resp_str.contains("Audio-Jack-Status: connected; type=analog"));
}

#[test]
fn test_rtsp_announce_and_sdp_parsing() {
    let sdp_body = format!(
        concat!(
            "v=0\r\n",
            "o=iTunes 123456 0 IN IP4 192.168.1.100\r\n",
            "s=iTunes\r\n",
            "c=IN IP4 192.168.1.100\r\n",
            "t=0 0\r\n",
            "m=audio 0 RTP/AVP 96\r\n",
            "a=rtpmap:96 AppleLossless\r\n",
            "a=fmtp:96 352 0 16 40 10 14 2 255 0 0 44100\r\n",
            "a=rsaaeskey:{}\r\n",
            "a=aesiv:AQIDBAUGBwgJCgsMDQ4PEA==\r\n"
        ),
        CT2048_PKCS1_B64
    );

    let req_text = format!(
        concat!(
            "ANNOUNCE rtsp://192.168.1.50/123456 RTSP/1.0\r\n",
            "CSeq: 2\r\n",
            "Content-Type: application/sdp\r\n",
            "Content-Length: {}\r\n",
            "\r\n",
            "{}"
        ),
        sdp_body.len(),
        sdp_body
    );

    let req = RtspRequest::parse(req_text.as_bytes()).expect("parse announce");
    assert_eq!(req.method, RtspMethod::Announce);
    assert_eq!(req.cseq, 2);

    let rsa = airport_express_key();
    let mut server = RtspServer::new(rsa, [0x24, 0x0A, 0xC4, 0x01, 0x02, 0x03], [192, 168, 1, 50]);
    let resp = server.handle_request(&req).expect("handle announce");

    assert_eq!(resp.status_code, 200);
    assert_eq!(server.state(), ServerState::Announced);
    assert_eq!(server.session_key(), Some(&GOLDEN_AES_KEY));

    let iv = server.aes_iv().expect("aes iv");
    assert_eq!(iv, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
}

#[test]
fn test_rtsp_full_session_lifecycle() {
    let rsa = airport_express_key();
    let mut server = RtspServer::new(rsa, [0x24, 0x0A, 0xC4, 0x01, 0x02, 0x03], [192, 168, 1, 50]);

    // 1. SETUP
    let setup_req = concat!(
        "SETUP rtsp://192.168.1.50/123456 RTSP/1.0\r\n",
        "CSeq: 3\r\n",
        "Transport: RTP/AVP/UDP;unicast;mode=record;control_port=6001;timing_port=6002\r\n\r\n"
    );
    let req = RtspRequest::parse(setup_req.as_bytes()).expect("setup parse");
    let resp = server.handle_request(&req).expect("setup handle");
    assert_eq!(resp.status_code, 200);
    assert_eq!(server.state(), ServerState::Configured);
    let resp_bytes = resp.to_bytes();
    let resp_str = core::str::from_utf8(&resp_bytes).expect("utf8");
    assert!(resp_str.contains("server_port=6000"));

    // 2. RECORD
    let record_req = concat!(
        "RECORD rtsp://192.168.1.50/123456 RTSP/1.0\r\n",
        "CSeq: 4\r\n",
        "Range: ntp=0-\r\n\r\n"
    );
    let req = RtspRequest::parse(record_req.as_bytes()).expect("record parse");
    let resp = server.handle_request(&req).expect("record handle");
    assert_eq!(resp.status_code, 200);
    assert_eq!(server.state(), ServerState::Streaming);

    // 3. SET_PARAMETER volume
    let vol_body = "volume: -15.000000\r\n";
    let vol_req = format!(
        concat!(
            "SET_PARAMETER rtsp://192.168.1.50/123456 RTSP/1.0\r\n",
            "CSeq: 5\r\n",
            "Content-Type: text/parameters\r\n",
            "Content-Length: {}\r\n\r\n",
            "{}"
        ),
        vol_body.len(),
        vol_body
    );
    let req = RtspRequest::parse(vol_req.as_bytes()).expect("vol parse");
    let resp = server.handle_request(&req).expect("vol handle");
    assert_eq!(resp.status_code, 200);
    // -15 dB = 1.0 + (-15/30) = 0.5
    assert!((server.volume() - 0.5).abs() < 1e-4);

    // 4. FLUSH
    let flush_req = "FLUSH rtsp://192.168.1.50/123456 RTSP/1.0\r\nCSeq: 6\r\n\r\n";
    let req = RtspRequest::parse(flush_req.as_bytes()).expect("flush parse");
    let resp = server.handle_request(&req).expect("flush handle");
    assert_eq!(resp.status_code, 200);

    // 5. TEARDOWN
    let teardown_req = "TEARDOWN rtsp://192.168.1.50/123456 RTSP/1.0\r\nCSeq: 7\r\n\r\n";
    let req = RtspRequest::parse(teardown_req.as_bytes()).expect("teardown parse");
    let resp = server.handle_request(&req).expect("teardown handle");
    assert_eq!(resp.status_code, 200);
    assert_eq!(server.state(), ServerState::Idle);
}

#[test]
fn test_rtsp_errors() {
    let no_cseq = "OPTIONS rtsp://127.0.0.1/1 RTSP/1.0\r\n\r\n";
    assert_eq!(RtspRequest::parse(no_cseq.as_bytes()), Err(RtspError::MissingCSeq));

    let malformed = "NOT_A_VALID_REQUEST\r\n\r\n";
    assert_eq!(
        RtspRequest::parse(malformed.as_bytes()),
        Err(RtspError::MalformedRequestLine)
    );

    let invalid_cl = "OPTIONS rtsp://127.0.0.1/1 RTSP/1.0\r\nCSeq: 1\r\nContent-Length: not-a-number\r\n\r\n";
    assert_eq!(
        RtspRequest::parse(invalid_cl.as_bytes()),
        Err(RtspError::InvalidContentLength)
    );
}

#[test]
fn test_rtp_header_parsing_and_errors() {
    let mut pkt = [0u8; 16];
    pkt[0] = 0x80; // V=2, P=0, X=0, CC=0
    pkt[1] = 96; // Payload Type 96
    pkt[2] = 0x12; // Seq 0x1234 = 4660
    pkt[3] = 0x34;
    pkt[4] = 0x00; // Timestamp 0x00010000 = 65536
    pkt[5] = 0x01;
    pkt[6] = 0x00;
    pkt[7] = 0x00;
    pkt[8] = 0xAA; // SSRC
    pkt[9] = 0xBB;
    pkt[10] = 0xCC;
    pkt[11] = 0xDD;
    pkt[12] = 0xDE; // Payload
    pkt[13] = 0xAD;
    pkt[14] = 0xBE;
    pkt[15] = 0xEF;

    let (hdr, payload) = RtpHeader::parse(&pkt).expect("valid rtp");
    assert_eq!(hdr.payload_type, 96);
    assert_eq!(hdr.sequence_number, 4660);
    assert_eq!(hdr.timestamp, 65536);
    assert_eq!(hdr.ssrc, 0xAABBCCDD);
    assert_eq!(payload, &[0xDE, 0xAD, 0xBE, 0xEF]);

    // Short packet
    assert_eq!(
        RtpHeader::parse(&pkt[..10]),
        Err(RtpReceiverError::PacketTooShort)
    );

    // Wrong version
    let mut wrong_v = pkt;
    wrong_v[0] = 0x00;
    assert_eq!(
        RtpHeader::parse(&wrong_v),
        Err(RtpReceiverError::InvalidRtpVersion)
    );
}

#[test]
fn test_rtp_audio_receiver_configure_alac() {
    let mut rx = RtpAudioReceiver::new();
    assert_eq!(rx.packets_received(), 0);

    let fmtp = "96 352 0 16 40 10 14 2 255 0 0 44100";
    rx.configure_alac(fmtp).expect("alac init");
    rx.configure_crypto(&GOLDEN_AES_KEY, &[1u8; 16]);
}

#[test]
fn test_mdns_builder_announcement_packet() {
    let config = MdnsConfig {
        player_name: "BASEMENT".to_string(),
        mac_address: [0x24, 0x0A, 0xC4, 0x11, 0x22, 0x33],
        ip_address: [192, 168, 1, 50],
        port: 5000,
    };

    assert_eq!(config.mac_hex(), "240AC4112233");
    assert_eq!(config.host_name(), "BASEMENT.local");
    assert_eq!(
        config.raop_instance_name(),
        "240AC4112233@BASEMENT._raop._tcp.local"
    );

    let pkt = MdnsBuilder::build_announcement(&config);
    assert!(pkt.len() > 100);

    // Check DNS header:
    // ID = 0
    assert_eq!(&pkt[0..2], &[0, 0]);
    // Flags = 0x8400 (Response, Authoritative)
    assert_eq!(&pkt[2..4], &[0x84, 0x00]);
    // Questions = 0
    assert_eq!(&pkt[4..6], &[0, 0]);
    // Answers = 7
    assert_eq!(&pkt[6..8], &[0, 7]);

    // Check payload contains expected strings
    let pkt_str = String::from_utf8_lossy(&pkt);
    assert!(pkt_str.contains("_raop"));
    assert!(pkt_str.contains("_airplay"));
    assert!(pkt_str.contains("BASEMENT"));
    assert!(pkt_str.contains("tp=UDP"));
    assert!(pkt_str.contains("sr=44100"));
}

#[test]
fn test_base64_roundtrip() {
    let raw = b"Squeezelite-ESP32 Pure-Rust AirPlay 1 RAOP Engine!";
    let enc = base64_encode(raw);
    let dec = base64_decode(&enc).expect("decode");
    assert_eq!(dec, raw);

    // Unpadded test
    let raw2 = b"Hello";
    let enc2 = base64_encode(raw2);
    let unpadded = enc2.trim_end_matches('=');
    let dec2 = base64_decode(unpadded).expect("unpadded decode");
    assert_eq!(dec2, raw2);
}

fn make_raw_rtp_packet(seq: u16, timestamp: u32, payload: &[u8]) -> Vec<u8> {
    let hdr = RtpHeader {
        payload_type: 0x60,
        sequence_number: seq,
        timestamp,
        ssrc: 0x1122_3344,
    };
    let mut pkt = Vec::with_capacity(12 + payload.len());
    pkt.extend_from_slice(&hdr.to_bytes(false));
    pkt.extend_from_slice(payload);
    pkt
}

#[test]
fn test_rtp_control_sync_and_retransmit_unwrap() {
    // 1. Sync packet (0xD4 with first-sync extension bit 0x10 -> byte 0 = 0x90)
    let sync = RtpSyncPacket::new(
        true,
        7,
        100_000,
        0xE3B0_1234,
        0x8000_0000,
        177_175,
    );
    let wire = sync.to_bytes();
    assert_eq!(wire[0], 0x90);
    assert_eq!(wire[1], 0xD4);

    let parsed_ctrl =
        RtpControlPacket::parse(&wire).expect("parse sync control packet");
    match parsed_ctrl {
        RtpControlPacket::Sync(s) => {
            assert!(s.is_first_sync);
            assert_eq!(s.flags, 7);
            assert_eq!(s.rtp_timestamp_latency, 100_000);
            assert_eq!(s.ntp_seconds, 0xE3B0_1234);
            assert_eq!(s.ntp_fraction, 0x8000_0000);
            assert_eq!(s.rtp_timestamp_now, 177_175);
            // (177_175 - 100_000) + 11_025 (flags == 7) = 88_200 frames (2.0 s)
            assert_eq!(s.effective_latency_frames(), 88_200);
        }
        other => panic!("expected Sync variant, got: {other:?}"),
    }

    // 2. Retransmit reply (0xD6) wrapping an inner RTP audio packet at +4
    let inner_rtp = make_raw_rtp_packet(42, 42 * 352, &[0xAA, 0xBB, 0xCC, 0xDD]);
    let mut retransmit_pkt = vec![0x80, 0xD6, 0x00, 0x09];
    retransmit_pkt.extend_from_slice(&inner_rtp);

    let (ctrl_seq, unwrapped) =
        unwrap_retransmit_packet(&retransmit_pkt).expect("unwrap 0xD6");
    assert_eq!(ctrl_seq, 9);
    assert_eq!(unwrapped, &inner_rtp[..]);

    let ctrl_parsed =
        RtpControlPacket::parse(&retransmit_pkt).expect("parse 0xD6");
    assert_eq!(
        ctrl_parsed,
        RtpControlPacket::Retransmit {
            control_seq: 9,
            inner_packet: &inner_rtp[..],
        }
    );
}

#[test]
fn test_rtp_resend_request_builder_and_ntp_timing_codec() {
    // 0x80 0xD5 resend request
    let raw_req = build_resend_request(3, 1200, 5);
    assert_eq!(raw_req, [0x80, 0xD5, 0x00, 0x03, 0x04, 0xB0, 0x00, 0x05]);

    let parsed_resend =
        RtpResendRequest::parse(&raw_req).expect("parse 0xD5 resend");
    assert_eq!(parsed_resend.request_seq, 3);
    assert_eq!(parsed_resend.missed_seq_start, 1200);
    assert_eq!(parsed_resend.missed_count, 5);

    // NTP timing request (0x52 / 0xD2) and response (0x53 / 0xD3)
    let req_ntp = NtpTimingPacket::new_request_ms(50_000);
    let req_bytes = req_ntp.to_bytes();
    assert_eq!(req_bytes[0], 0x80);
    assert_eq!(req_bytes[1], 0xD2);
    assert_eq!(&req_bytes[2..4], &[0x00, 0x07]);

    let parsed_req = NtpTimingPacket::parse(&req_bytes).expect("parse 0x52");
    assert_eq!(parsed_req.payload_type, 0x52);
    assert_eq!(parsed_req.transmit_ntp, 50_000);

    let remote_rx = ms_to_ntp(123_456);
    let remote_tx = ms_to_ntp(123_457);
    let resp_ntp = NtpTimingPacket::new_response(50_000, remote_rx, remote_tx);
    let resp_bytes = resp_ntp.to_bytes();
    assert_eq!(resp_bytes[1], 0xD3);

    let parsed_resp = NtpTimingPacket::parse(&resp_bytes).expect("parse 0x53");
    assert_eq!(parsed_resp.payload_type, 0x53);
    assert_eq!(parsed_resp.reference_low_u32(), 50_000);
    assert_eq!(parsed_resp.roundtrip_ms(50_018), 18);
    assert_eq!(ntp_to_ms(parsed_resp.receive_ntp), 123_456);
}

#[test]
fn test_jitter_buffer_gap_detection_reordering_wrap_and_flush() {
    let mut jb = RtpJitterBuffer::new(32);

    // 1. Push seq 100 -> initial packet, no gap
    let p100 = make_raw_rtp_packet(100, 35_200, &[1]);
    assert_eq!(jb.push_packet(&p100).expect("push 100"), None);

    // 2. Push seq 103 -> missing 101 and 102 (gap of 2) -> emits 0xD5 resend!
    let p103 = make_raw_rtp_packet(103, 36_256, &[4]);
    let resend = jb
        .push_packet(&p103)
        .expect("push 103")
        .expect("gap should emit resend request");
    assert_eq!(resend.request_seq, 1);
    assert_eq!(resend.missed_seq_start, 101);
    assert_eq!(resend.missed_count, 2);
    assert_eq!(jb.missing_sequences(), vec![101, 102]);

    // Pop 100 (ready), next is 101 (missing -> returns None)
    let e100 = jb.pop_ready().expect("100 is ready");
    assert_eq!(e100.header.sequence_number, 100);
    assert!(jb.pop_ready().is_none(), "101 is still missing");

    // 3. Push 102 out of order, then 101 wrapped in a 0xD6 retransmit packet
    let p102 = make_raw_rtp_packet(102, 35_904, &[3]);
    assert_eq!(jb.push_packet(&p102).expect("push 102"), None);
    assert!(jb.pop_ready().is_none(), "still waiting for 101");

    let p101_inner = make_raw_rtp_packet(101, 35_552, &[2]);
    let mut p101_retransmit = vec![0x80, 0xD6, 0x00, 0x01];
    p101_retransmit.extend_from_slice(&p101_inner);
    assert_eq!(
        jb.push_packet(&p101_retransmit)
            .expect("push 0xD6 retransmit 101"),
        None
    );
    assert_eq!(jb.recovered_packets(), 2);

    // Now 101, 102, 103 drain in exact sequence order!
    let ready = jb.drain_ready();
    let seqs: Vec<u16> =
        ready.iter().map(|e| e.header.sequence_number).collect();
    assert_eq!(seqs, vec![101, 102, 103]);

    // 4. Test 16-bit sequence wrap-around across 0xFFFF -> 0x0000
    assert_eq!(seq_diff(0x0001, 0xFFFF), 2);
    let mut jb_wrap = RtpJitterBuffer::new(16);
    let p_fffe = make_raw_rtp_packet(0xFFFE, 1000, &[0xFE]);
    let p_0001 = make_raw_rtp_packet(0x0001, 2056, &[0x01]);
    assert_eq!(jb_wrap.push_packet(&p_fffe).expect("fffe"), None);
    let wrap_resend = jb_wrap
        .push_packet(&p_0001)
        .expect("0001")
        .expect("wrap gap resend");
    assert_eq!(wrap_resend.missed_seq_start, 0xFFFF);
    assert_eq!(wrap_resend.missed_count, 2); // 0xFFFF and 0x0000

    // 5. Test RTSP FLUSH (seq=500;rtptime=999000)
    let (f_seq, f_rtp) = parse_rtp_info("seq=500;rtptime=999000");
    assert_eq!(f_seq, Some(500));
    assert_eq!(f_rtp, Some(999_000));

    let mut jb_flush = RtpJitterBuffer::new(16);
    for seq in 498..=502 {
        let pkt = make_raw_rtp_packet(seq, u32::from(seq) * 352, &[seq as u8]);
        let _ = jb_flush.push_packet(&pkt).expect("push pre-flush");
    }
    jb_flush.flush(500, 999_000);
    // Stale packet 499 arriving after FLUSH is ignored
    let p499 = make_raw_rtp_packet(499, 499 * 352, &[0xFF]);
    assert_eq!(jb_flush.push_packet(&p499).expect("stale 499"), None);

    // Remaining buffered entries start cleanly at 500
    let post_flush: Vec<u16> = jb_flush
        .drain_ready()
        .iter()
        .map(|e| e.header.sequence_number)
        .collect();
    assert_eq!(post_flush, vec![500, 501, 502]);
}

#[test]
fn test_dmap_metadata_artwork_and_progress_set_parameter() {
    let rsa = airport_express_key();
    let mut server = RtspServer::new(
        rsa,
        [0x24, 0x0A, 0xC4, 0x01, 0x02, 0x03],
        [192, 168, 1, 50],
    );

    // 1. Binary DMAP metadata (`application/x-dmap-tagged`)
    let expected_meta = DmapMetadata::new()
        .with_title("So What")
        .with_artist("Miles Davis")
        .with_album("Kind of Blue")
        .with_duration_ms(562_000);
    let dmap_bytes = expected_meta.to_dmap_bytes();
    let parsed_direct = DmapMetadata::parse(&dmap_bytes).expect("parse dmap");
    assert_eq!(parsed_direct, expected_meta);

    let mut dmap_req = format!(
        concat!(
            "SET_PARAMETER rtsp://192.168.1.50/123456 RTSP/1.0\r\n",
            "CSeq: 10\r\n",
            "Content-Type: application/x-dmap-tagged\r\n",
            "Content-Length: {}\r\n\r\n"
        ),
        dmap_bytes.len()
    )
    .into_bytes();
    dmap_req.extend_from_slice(&dmap_bytes);

    let req = RtspRequest::parse(&dmap_req).expect("parse dmap rtsp");
    let resp = server.handle_request(&req).expect("handle dmap set_parameter");
    assert_eq!(resp.status_code, 200);
    assert_eq!(server.metadata(), Some(&expected_meta));

    // Truncated DMAP TLV returns DmapError::TruncatedPayload
    assert_eq!(
        DmapMetadata::parse(&dmap_bytes[..6]),
        Err(DmapError::TruncatedPayload)
    );

    // 2. Cover artwork (`image/jpeg` and `image/png`)
    let jpeg_bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46];
    let mut art_req = format!(
        concat!(
            "SET_PARAMETER rtsp://192.168.1.50/123456 RTSP/1.0\r\n",
            "CSeq: 11\r\n",
            "Content-Type: image/jpeg\r\n",
            "RTP-Info: rtptime=441000\r\n",
            "Content-Length: {}\r\n\r\n"
        ),
        jpeg_bytes.len()
    )
    .into_bytes();
    art_req.extend_from_slice(&jpeg_bytes);

    let req_art = RtspRequest::parse(&art_req).expect("parse artwork rtsp");
    server.handle_request(&req_art).expect("handle artwork");
    let art = server.artwork().expect("artwork stored");
    assert_eq!(art.mime_type, ArtworkMimeType::Jpeg);
    assert_eq!(art.data, jpeg_bytes);
    assert_eq!(art.rtp_timestamp, Some(441_000));

    // 3. Track progress (`text/parameters`: `progress: start/curr/end`)
    let prog_body = "progress: 10000/451000/1333000\r\n";
    let prog_req = format!(
        concat!(
            "SET_PARAMETER rtsp://192.168.1.50/123456 RTSP/1.0\r\n",
            "CSeq: 12\r\n",
            "Content-Type: text/parameters\r\n",
            "Content-Length: {}\r\n\r\n",
            "{}"
        ),
        prog_body.len(),
        prog_body
    );
    let req_prog = RtspRequest::parse(prog_req.as_bytes()).expect("parse prog");
    server.handle_request(&req_prog).expect("handle prog");
    let prog = server.progress().expect("progress stored");
    assert_eq!(prog, &TrackProgress::new(10_000, 451_000, 1_333_000));
    // (451_000 - 10_000) / 44_100 = 10.0 s = 10_000 ms
    assert_eq!(prog.elapsed_ms(44_100), 10_000);
    // (1_333_000 - 10_000) / 44_100 = 30.0 s = 30_000 ms
    assert_eq!(prog.duration_ms(44_100), 30_000);
}

#[test]
fn test_dacp_header_extraction_and_command_requests() {
    let rsa = airport_express_key();
    let mut server = RtspServer::new(
        rsa,
        [0x24, 0x0A, 0xC4, 0x01, 0x02, 0x03],
        [192, 168, 1, 50],
    );

    let record_req = concat!(
        "RECORD rtsp://192.168.1.50/123456 RTSP/1.0\r\n",
        "CSeq: 4\r\n",
        "DACP-ID: 1A2B3C4D5E6F7788\r\n",
        "Active-Remote: 987654321\r\n",
        "RTP-Info: seq=200;rtptime=88200\r\n\r\n"
    );
    let req = RtspRequest::parse(record_req.as_bytes()).expect("parse record");
    server.handle_request(&req).expect("handle record");

    assert_eq!(server.dacp_id(), Some("1A2B3C4D5E6F7788"));
    assert_eq!(server.active_remote(), Some("987654321"));

    let dacp = DacpClient::from_rtsp_request(&req).expect("dacp client");
    assert_eq!(dacp.mdns_instance_prefix(), "iTunes_Ctrl_1A2B3C4D5E6F7788");

    for (cmd, expected_path) in [
        (DacpCommand::PlayPause, "/ctrl-int/1/playpause"),
        (DacpCommand::NextItem, "/ctrl-int/1/nextitem"),
        (DacpCommand::PrevItem, "/ctrl-int/1/previtem"),
        (DacpCommand::VolumeUp, "/ctrl-int/1/volumeup"),
        (DacpCommand::VolumeDown, "/ctrl-int/1/volumedown"),
        (DacpCommand::MuteToggle, "/ctrl-int/1/mutetoggle"),
        (DacpCommand::Stop, "/ctrl-int/1/stop"),
    ] {
        let http_bytes = server
            .build_dacp_command(cmd)
            .expect("build dacp command");
        let http_str = core::str::from_utf8(&http_bytes).expect("utf8");
        assert!(http_str.starts_with(&format!("GET {} HTTP/1.0\r\n", expected_path)));
        assert!(http_str.contains("Active-Remote: 987654321\r\n"));
        assert!(http_str.contains("DACP-ID: 1A2B3C4D5E6F7788\r\n"));
    }
}

#[test]
fn test_sync_drift_tracker_and_stereo_frame_stuff_drop() {
    let mut tracker = SyncDriftTracker::default();
    // 2 ms at 44.1 kHz = 88 stereo frames
    assert_eq!(tracker.window_frames(), 88);

    // 1. Within ±88 frames deadband -> 0 adjustment
    assert_eq!(tracker.compute_frame_adjustment(10_000, 10_000), 0);
    assert_eq!(tracker.compute_frame_adjustment(10_050, 10_000), 0);
    assert_eq!(tracker.compute_frame_adjustment(10_000, 10_050), 0);

    // 2. DAC lagging behind sender by 120 frames (> 88) -> drop 1 frame (-1)
    assert_eq!(tracker.compute_frame_adjustment(10_120, 10_000), -1);
    assert_eq!(tracker.frames_dropped(), 1);

    // 3. DAC ahead of sender by 120 frames (> 88) -> stuff 1 frame (+1)
    assert_eq!(tracker.compute_frame_adjustment(10_000, 10_120), 1);
    assert_eq!(tracker.frames_stuffed(), 1);

    // 4. NTP + RTP Sync anchor playtime and time-based drift check
    let ntp = NtpTimingPacket::new_response(1_000, ms_to_ntp(50_000), ms_to_ntp(50_000));
    assert!(tracker.record_ntp_timing(&ntp, 1_010));
    let sync = RtpSyncPacket::new(
        true,
        0,
        44_100,
        (ms_to_ntp(50_500) >> 32) as u32,
        ms_to_ntp(50_500) as u32,
        88_200,
    );
    assert!(tracker.record_rtp_sync(&sync, 1_500));
    assert!(tracker.is_synced());
    assert_eq!(tracker.expected_playtime_ms(44_100), Some(1_500));
    assert_eq!(tracker.compute_time_adjustment(44_100, 1_501), 0);
    assert_eq!(tracker.compute_time_adjustment(44_100, 1_505), -1);
    assert_eq!(tracker.compute_time_adjustment(44_100, 1_495), 1);

    // 5. Verify stereo PCM adjustment (-1 drops 1 stereo frame, +1 stuffs 1)
    let input_stereo: [i16; 6] = [10, 20, 30, 40, 50, 60]; // 3 stereo frames
    let mut out = Vec::new();

    SyncDriftTracker::apply_stereo_adjustment(0, &input_stereo, &mut out);
    assert_eq!(out, vec![10, 20, 30, 40, 50, 60]);

    SyncDriftTracker::apply_stereo_adjustment(-1, &input_stereo, &mut out);
    assert_eq!(out, vec![10, 20, 30, 40]);

    SyncDriftTracker::apply_stereo_adjustment(1, &input_stereo, &mut out);
    assert_eq!(out, vec![10, 20, 30, 40, 50, 60, 50, 60]);
}

#[test]
fn test_wo253_dacp_crlf_rejection() {
    let client = DacpClient::new("1234", "9876");
    // Action with newline / CRLF injection attempt
    let injected = "playpause\r\nEvil-Header: injected";
    assert_eq!(client.build_raw_action_request(injected), None);

    // Empty action
    assert_eq!(client.build_raw_action_request(""), None);

    // Valid action
    assert!(client.build_raw_action_request("playpause").is_some());
}

#[test]
fn test_wo253_dmap_rtp_wrapping() {
    // 32-bit timestamp wrapping: start is near u32::MAX, current wrapped around
    let progress = TrackProgress::new(u32::MAX - 44_100, 44_100, 88_200);
    // delta = 44_100.wrapping_sub(u32::MAX - 44_100) = 88_201 frames
    // at 44_100 Hz, elapsed = (88_201 * 1000) / 44_100 = 2000 ms
    assert_eq!(progress.elapsed_ms(44_100), 2000);
}

#[test]
fn test_wo253_base64_invalid_length_mod_4() {
    use squeezelite_raop::crypto::CryptoError;
    // Length % 4 == 1 cannot be valid Base64
    let invalid_len_1 = "A";
    assert_eq!(base64_decode(invalid_len_1), Err(CryptoError::InvalidBase64));
    let invalid_len_5 = "ABCDE";
    assert_eq!(base64_decode(invalid_len_5), Err(CryptoError::InvalidBase64));
}

#[test]
fn test_wo253_sync_drift_negative_ntp_rejected() {
    let mut tracker = SyncDriftTracker::default();
    let ntp = NtpTimingPacket::new_response(1_000, ms_to_ntp(50_000), ms_to_ntp(50_000));
    assert!(tracker.record_ntp_timing(&ntp, 1_010));

    // Sync packet with remote NTP timestamp earlier than anchor (40_000 < 50_000): must be rejected
    let stale_sync = RtpSyncPacket::new(
        true,
        0,
        44_100,
        (ms_to_ntp(40_000) >> 32) as u32,
        ms_to_ntp(40_000) as u32,
        88_200,
    );
    assert!(!tracker.record_rtp_sync(&stale_sync, 1_500));
}


