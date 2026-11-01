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
use squeezelite_raop::mdns::MdnsBuilder;
use squeezelite_raop::mdns::MdnsConfig;
use squeezelite_raop::rtp::RtpAudioReceiver;
use squeezelite_raop::rtp::RtpHeader;
use squeezelite_raop::rtp::RtpReceiverError;
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
