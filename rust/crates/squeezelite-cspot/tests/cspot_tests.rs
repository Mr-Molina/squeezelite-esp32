//! Comprehensive unit and golden vector tests for pure-Rust Spotify Connect
//! (`cspot`) engine.

use squeezelite_cspot::cdn::AudioKeyDecryptor;
use squeezelite_cspot::cdn::CdnAudioDecryptor;
use squeezelite_cspot::cdn::CdnStreamFeeder;
use squeezelite_cspot::cdn::AUDIO_AES_IV;
use squeezelite_cspot::cdn::SPOTIFY_HEADER_SIZE;
use squeezelite_cspot::crypto::base64_decode;
use squeezelite_cspot::crypto::base64_encode;
use squeezelite_cspot::crypto::big_num_add;
use squeezelite_cspot::crypto::hmac_sha1;
use squeezelite_cspot::crypto::pbkdf2_hmac_sha1;
use squeezelite_cspot::crypto::Aes128Ctr;
use squeezelite_cspot::crypto::Aes192Ecb;
use squeezelite_cspot::crypto::CryptoError;
use squeezelite_cspot::crypto::OakleyGroup1Dh;
use squeezelite_cspot::crypto::Sha1;
use squeezelite_cspot::mercury::AudioKeyPacket;
use squeezelite_cspot::mercury::MercuryError;
use squeezelite_cspot::mercury::MercuryHeader;
use squeezelite_cspot::mercury::MercuryPacket;
use squeezelite_cspot::mercury::MercuryPacketBuilder;
use squeezelite_cspot::mercury::MercuryType;
use squeezelite_cspot::shannon::Shannon;
use squeezelite_cspot::spirc::decode_varint;
use squeezelite_cspot::spirc::encode_varint;
use squeezelite_cspot::spirc::PlayStatus;
use squeezelite_cspot::spirc::SpircEvent;
use squeezelite_cspot::spirc::SpircFrame;
use squeezelite_cspot::spirc::SpircMessageType;
use squeezelite_cspot::spirc::SpircStateMachine;
use squeezelite_cspot::spirc::TrackMetadata;
use squeezelite_cspot::spirc::SPOTIFY_MAX_VOLUME;
use squeezelite_cspot::zeroconf::ReusableCredentials;
use squeezelite_cspot::zeroconf::SpotifyMdnsBuilder;
use squeezelite_cspot::zeroconf::SpotifyMdnsConfig;
use squeezelite_cspot::zeroconf::ZeroConfServer;
use squeezelite_cspot::zeroconf::AP_AUTH_DECLINED_CMD;
use squeezelite_cspot::zeroconf::AP_WELCOME_CMD;
use squeezelite_cspot::zeroconf::AUTHENTICATION_STORED_SPOTIFY_CREDENTIALS;

// --- Shannon Golden Vector Tests ---

const SHANNON_TEST_KEY: [u8; 16] = [
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
    0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10,
];

const SHANNON_TEST_NONCE: [u8; 4] = [0x11, 0x22, 0x33, 0x44];

const SHANNON_PLAINTEXT: &[u8] = b"Hello, Spotify Shannon Stream Cipher!";

const SHANNON_GOLDEN_CIPHERTEXT: [u8; 37] = [
    0x91, 0xC8, 0xCE, 0x69, 0x83, 0xB4, 0xFB, 0x97,
    0xB3, 0xDF, 0xDC, 0x9A, 0x27, 0x9F, 0x2B, 0xF9,
    0x4E, 0x43, 0x49, 0x31, 0x65, 0x22, 0x42, 0x1F,
    0xC8, 0x87, 0xCB, 0xB3, 0xBA, 0x8A, 0xF4, 0xB7,
    0xDC, 0x06, 0xC9, 0x07, 0x88,
];

const SHANNON_GOLDEN_MAC: [u8; 4] = [0x0A, 0x8C, 0x92, 0x37];

#[test]
fn test_shannon_golden_vector_encrypt_and_mac() {
    let mut sh = Shannon::new();
    sh.key(&SHANNON_TEST_KEY);
    sh.nonce(&SHANNON_TEST_NONCE);

    let mut data = SHANNON_PLAINTEXT.to_vec();
    sh.encrypt(&mut data);
    assert_eq!(data, SHANNON_GOLDEN_CIPHERTEXT);

    let mut mac = [0u8; 4];
    sh.finish(&mut mac);
    assert_eq!(mac, SHANNON_GOLDEN_MAC);
}

#[test]
fn test_shannon_golden_vector_decrypt_and_mac() {
    let mut sh = Shannon::new();
    sh.key(&SHANNON_TEST_KEY);
    sh.nonce(&SHANNON_TEST_NONCE);

    let mut data = SHANNON_GOLDEN_CIPHERTEXT.to_vec();
    sh.decrypt(&mut data);
    assert_eq!(data, SHANNON_PLAINTEXT);

    let mut mac = [0u8; 4];
    sh.finish(&mut mac);
    assert_eq!(mac, SHANNON_GOLDEN_MAC);
}

#[test]
fn test_shannon_stream_keystream() {
    let mut sh1 = Shannon::new();
    sh1.key(&SHANNON_TEST_KEY);
    sh1.nonce(&SHANNON_TEST_NONCE);

    let mut stream_buf = [0u8; 32];
    sh1.stream(&mut stream_buf);

    let mut sh2 = Shannon::new();
    sh2.key(&SHANNON_TEST_KEY);
    sh2.nonce(&SHANNON_TEST_NONCE);

    let mut zeros = [0u8; 32];
    sh2.stream(&mut zeros);
    assert_eq!(stream_buf, zeros);
}

// --- Crypto Primitives Tests ---

#[test]
fn test_sha1_basic() {
    let digest = Sha1::digest(b"The quick brown fox jumps over the lazy dog");
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12");
}

#[test]
fn test_hmac_sha1_rfc2202() {
    // RFC 2202 Test Case 1
    let key = [0x0b; 20];
    let data = b"Hi There";
    let mac = hmac_sha1(&key, data);
    let hex: String = mac.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, "b617318655057264e28bc0b6fb378c8ef146be00");
}

#[test]
fn test_pbkdf2_hmac_sha1_rfc6070() {
    // RFC 6070 Test 1: P="password", S="salt", c=1, dkLen=20
    let dk = pbkdf2_hmac_sha1(b"password", b"salt", 1, 20);
    let hex: String = dk.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, "0c60c80f961f0e71f3a9b524af6012062fe037a6");
}

#[test]
fn test_oakley_group_1_dh_shared_agreement() {
    let alice_priv = [0x42u8; 96];
    let bob_priv = [0x77u8; 96];

    let alice = OakleyGroup1Dh::from_private_key(alice_priv).expect("alice dh init");
    let bob = OakleyGroup1Dh::from_private_key(bob_priv).expect("bob dh init");

    let alice_shared = alice.calculate_shared(bob.public_key()).expect("alice shared");
    let bob_shared = bob.calculate_shared(alice.public_key()).expect("bob shared");

    assert_eq!(alice_shared, bob_shared);
}

#[test]
fn test_aes128_ctr_roundtrip() {
    let key = [0x55u8; 16];
    let iv = [0x11u8; 16];
    let plaintext = b"Pure Rust AES-128-CTR stream cipher test vector block!";

    let mut cipher = Aes128Ctr::new(&key, iv);
    let mut data = plaintext.to_vec();
    cipher.process(&mut data);
    assert_ne!(data, plaintext);

    let mut decipher = Aes128Ctr::new(&key, iv);
    decipher.process(&mut data);
    assert_eq!(data, plaintext);
}

#[test]
fn test_aes192_ecb_roundtrip() {
    let key = [0x33u8; 24];
    let block = [0xAAu8; 16];

    let ecb = Aes192Ecb::new(&key);
    let mut data = block;
    ecb.decrypt_block(&mut data);
    assert_ne!(data, block);
}

#[test]
fn test_big_num_add() {
    let num = vec![0x00, 0x01, 0xFF];
    let res = big_num_add(&num, 2);
    assert_eq!(res, vec![0x00, 0x02, 0x01]);

    let max = vec![0xFF, 0xFF];
    let carry_res = big_num_add(&max, 1);
    assert_eq!(carry_res, vec![0x01, 0x00, 0x00]);
}

#[test]
fn test_base64_encode_decode() {
    let sample = b"Spotify Connect Pure Rust 2026";
    let encoded = base64_encode(sample);
    let decoded = base64_decode(&encoded).expect("b64 decode");
    assert_eq!(decoded, sample);
}

// --- ZeroConf mDNS & HTTP Tests ---

#[test]
fn test_zeroconf_mdns_builder() {
    let config = SpotifyMdnsConfig {
        player_name: "BASEMENT".to_string(),
        mac_address: [0xC8, 0x2E, 0x18, 0xBE, 0xB8, 0x34],
        ip_address: [172, 22, 1, 15],
        port: 80,
    };

    let packet = SpotifyMdnsBuilder::build_announcement(&config);
    assert!(packet.len() > 100);

    // Verify service name substring exists in binary DNS labels
    let contains_service = packet.windows(16).any(|w| w == b"_spotify-connect");
    assert!(contains_service, "Packet must announce _spotify-connect");

    // Verify TXT properties exist
    let contains_cpath = packet.windows(7).any(|w| w == b"CPath=/");
    let contains_version = packet.windows(11).any(|w| w == b"VERSION=1.0");
    let contains_stack = packet.windows(8).any(|w| w == b"Stack=SP");
    assert!(contains_cpath);
    assert!(contains_version);
    assert!(contains_stack);
}

#[test]
fn test_zeroconf_get_info() {
    let dh = OakleyGroup1Dh::from_private_key([0x01u8; 96]).expect("dh init");
    let device_id = "142137fd329622137a149016c82e18beb834";
    let remote_name = "BASEMENT";

    let mut server = ZeroConfServer::new();
    let resp = server.handle_request(
        "/?action=getInfo",
        "",
        &dh,
        device_id,
        remote_name,
    )
    .expect("handle getInfo");

    assert_eq!(resp.status_code, 200);
    let body_str = String::from_utf8(resp.body).expect("utf8 json");
    assert!(body_str.contains("\"status\":101"));
    assert!(body_str.contains("\"statusString\":\"OK\""));
    assert!(body_str.contains("\"remoteName\":\"BASEMENT\""));
    assert!(body_str.contains("\"deviceID\":\"142137fd329622137a149016c82e18beb834\""));
    assert!(body_str.contains("\"deviceType\":\"AudioDongle\""));
}

// --- Spirc State Machine Tests ---

#[test]
fn test_spirc_varint_codec() {
    let mut buf = Vec::new();
    encode_varint(&mut buf, 0);
    encode_varint(&mut buf, 127);
    encode_varint(&mut buf, 128);
    encode_varint(&mut buf, 65536);

    let mut pos = 0;
    assert_eq!(decode_varint(&buf, &mut pos).unwrap(), 0);
    assert_eq!(decode_varint(&buf, &mut pos).unwrap(), 127);
    assert_eq!(decode_varint(&buf, &mut pos).unwrap(), 128);
    assert_eq!(decode_varint(&buf, &mut pos).unwrap(), 65536);
}

#[test]
fn test_spirc_state_machine_transitions() {
    let mut sm = SpircStateMachine::new();
    assert_eq!(sm.status(), PlayStatus::Stop);
    assert_eq!(sm.volume(), 32768);

    // Play command frame
    let frame_play = SpircFrame {
        seq_nr: 1,
        message_type: SpircMessageType::Play,
        volume: None,
        position_ms: None,
        track_uri: None,
    };
    let ev = sm.handle_frame(&frame_play);
    assert_eq!(ev, Some(SpircEvent::Play));
    assert_eq!(sm.status(), PlayStatus::Play);

    // Pause command frame
    let frame_pause = SpircFrame {
        seq_nr: 2,
        message_type: SpircMessageType::Pause,
        volume: None,
        position_ms: None,
        track_uri: None,
    };
    let ev = sm.handle_frame(&frame_pause);
    assert_eq!(ev, Some(SpircEvent::Pause));
    assert_eq!(sm.status(), PlayStatus::Pause);

    // Set volume frame
    let frame_vol = SpircFrame {
        seq_nr: 3,
        message_type: SpircMessageType::Volume,
        volume: Some(50000),
        position_ms: None,
        track_uri: None,
    };
    let ev = sm.handle_frame(&frame_vol);
    assert_eq!(ev, Some(SpircEvent::SetVolume(50000)));
    assert_eq!(sm.volume(), 50000);

    // Load track frame
    let frame_load = SpircFrame {
        seq_nr: 4,
        message_type: SpircMessageType::Load,
        volume: None,
        position_ms: Some(1500),
        track_uri: Some("spotify:track:4cOdK2wGLETKBW3PvgPWqT".to_string()),
    };
    let ev = sm.handle_frame(&frame_load);
    assert_eq!(
        ev,
        Some(SpircEvent::LoadTrack {
            uri: "spotify:track:4cOdK2wGLETKBW3PvgPWqT".to_string(),
            position_ms: 1500,
        })
    );
    assert_eq!(sm.status(), PlayStatus::Play);
    assert_eq!(sm.position_ms(), 1500);
}

#[test]
fn test_spirc_notify_frame_build() {
    let mut sm = SpircStateMachine::new();
    sm.set_status(PlayStatus::Play);
    sm.update_position_ms(12000);
    sm.set_volume(40000);

    let frame_bytes = sm.build_notify_frame(42, "cspot-player");
    let parsed = SpircFrame::parse(&frame_bytes).expect("parse notify frame");
    assert_eq!(parsed.seq_nr, 42);
    assert_eq!(parsed.message_type, SpircMessageType::Notify);
    assert_eq!(parsed.volume, Some(40000));
    assert_eq!(parsed.position_ms, Some(12000));
}

// --- Mercury Protocol Tests ---

#[test]
fn test_mercury_request_builder() {
    let payload = b"{\"context\":\"spotify:album:123\"}";
    let packet = MercuryPacketBuilder::build_request(
        1001,
        "SUB",
        "hm://remote/user/spotify",
        &[payload],
    );

    assert!(packet.len() > 30);
    // Sequence length is 8 bytes
    assert_eq!(&packet[0..2], &8u16.to_be_bytes());
    // Sequence ID 1001
    assert_eq!(&packet[2..10], &1001u64.to_be_bytes());
    // Flags 0x01
    assert_eq!(packet[10], 0x01);
}

#[test]
fn test_mercury_audio_key_request_and_response() {
    let file_id = [0xAAu8; 20];
    let track_id = [0xBBu8; 16];
    let seq_id = 777u32;

    let req = MercuryPacketBuilder::build_audio_key_request(&file_id, &track_id, seq_id);
    assert_eq!(req.len(), 42);
    assert_eq!(&req[0..20], &file_id);
    assert_eq!(&req[20..36], &track_id);
    assert_eq!(&req[36..40], &seq_id.to_be_bytes());
    assert_eq!(&req[40..42], &[0x00, 0x00]);

    // Success response
    let golden_key = [0x55u8; 16];
    let mut resp_payload = Vec::new();
    resp_payload.extend_from_slice(&seq_id.to_be_bytes());
    resp_payload.extend_from_slice(&golden_key);

    let (parsed_seq, parsed_key) = MercuryPacketBuilder::parse_audio_key_response(
        MercuryType::AudioKeySuccess as u8,
        &resp_payload,
    )
    .expect("parse key success");

    assert_eq!(parsed_seq, seq_id);
    assert_eq!(parsed_key, golden_key);
}

// --- CDN Audio Decryptor & Feeder Tests ---

#[test]
fn test_cdn_audio_decryptor_iv_calculation() {
    let offset = 0;
    let iv0 = CdnAudioDecryptor::calculate_iv(offset);
    assert_eq!(iv0, AUDIO_AES_IV);

    let offset16 = 16;
    let iv1 = CdnAudioDecryptor::calculate_iv(offset16);
    let expected_iv1 = big_num_add(&AUDIO_AES_IV, 1);
    assert_eq!(&iv1[..], &expected_iv1[..]);
}

#[test]
fn test_cdn_audio_decryptor_roundtrip() {
    let key = [0x99u8; 16];
    let decryptor = CdnAudioDecryptor::new(key);

    let original = b"Vorbis audio frame test bitstream chunk data payload 123456";
    let mut encrypted = original.to_vec();

    // Decryptor in CTR mode is symmetric (process XORs keystream)
    decryptor.decrypt_chunk(&mut encrypted, 0);
    assert_ne!(encrypted, original);

    decryptor.decrypt_chunk(&mut encrypted, 0);
    assert_eq!(encrypted, original);
}

#[test]
fn test_cdn_stream_feeder_initialization() {
    let key = [0x12u8; 16];
    let mut feeder = CdnStreamFeeder::new(key);
    assert_eq!(feeder.sample_rate(), 44100);
    assert_eq!(feeder.channels(), 2);

    let mut pcm_out = [0i32; 64];
    // Feeding empty chunk yields 0 consumed, 0 samples
    let res = feeder.feed_and_decode(&[], 0, &mut pcm_out);
    assert!(res.is_ok());

    feeder.flush();
    assert_eq!(feeder.sample_rate(), 44100);
}

// --- Phase 6 Completion Tests (WO-227) ---

#[test]
fn test_reusable_credentials_ap_welcome_and_encrypted_persistence() {
    let creds = ReusableCredentials::stored_spotify(
        "spotify_user_canonical",
        vec![0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04],
    );
    assert_eq!(
        creds.auth_type,
        AUTHENTICATION_STORED_SPOTIFY_CREDENTIALS
    );

    // Encode APWelcome (0xAC) and parse back
    let welcome_bytes = creds.encode_ap_welcome();
    let parsed_welcome =
        ReusableCredentials::from_ap_welcome_packet(AP_WELCOME_CMD, &welcome_bytes)
            .expect("parse APWelcome 0xAC");
    assert_eq!(parsed_welcome, creds);

    // Reject non-0xAC command (e.g., 0xAD declined)
    let declined = ReusableCredentials::from_ap_welcome_packet(
        AP_AUTH_DECLINED_CMD,
        &welcome_bytes,
    );
    assert_eq!(declined, Err(CryptoError::DecryptionFailed));

    // Encrypt for FlashJournal with AES-128-CTR + HMAC-SHA1
    let secret = b"louder-esp32-device-unique-secret";
    let iv = [0x3Cu8; 16];
    let encrypted = creds.encrypt(secret, iv);
    assert!(encrypted.len() > 36);

    let decrypted = ReusableCredentials::decrypt(&encrypted, secret)
        .expect("decrypt valid credentials");
    assert_eq!(decrypted, creds);

    // Base64 FlashJournal roundtrip
    let b64 = creds.encrypt_base64(secret, iv);
    let from_b64 = ReusableCredentials::decrypt_base64(&b64, secret)
        .expect("decrypt base64 credentials");
    assert_eq!(from_b64, creds);

    // Tamper with ciphertext byte -> must fail HMAC-SHA1 check
    let mut tampered_ct = encrypted.clone();
    tampered_ct[20] ^= 0x01;
    assert_eq!(
        ReusableCredentials::decrypt(&tampered_ct, secret),
        Err(CryptoError::DecryptionFailed)
    );

    // Tamper with HMAC tag byte -> must fail HMAC-SHA1 check
    let mut tampered_mac = encrypted.clone();
    let last = tampered_mac.len() - 1;
    tampered_mac[last] ^= 0x80;
    assert_eq!(
        ReusableCredentials::decrypt(&tampered_mac, secret),
        Err(CryptoError::DecryptionFailed)
    );

    // Wrong secret key -> must fail HMAC-SHA1 check
    assert_eq!(
        ReusableCredentials::decrypt(&encrypted, b"wrong-secret"),
        Err(CryptoError::DecryptionFailed)
    );
}

#[test]
fn test_mercury_header_and_multipart_packet_framing() {
    let header = MercuryHeader::new("hm://remote/user/spotify/v1", "SUB")
        .with_status_code(200)
        .with_content_type("application/x-protobuf");

    let encoded_hdr = header.encode();
    let decoded_hdr =
        MercuryHeader::decode(&encoded_hdr).expect("decode mercury header");
    assert_eq!(decoded_hdr, header);

    // Full MercuryPacket roundtrip for 0xB2..0xB5
    for (method, expected_cmd) in [
        ("SEND", MercuryType::MercuryReq),
        ("GET", MercuryType::MercuryReq),
        ("SUB", MercuryType::MercurySub),
        ("UNSUB", MercuryType::MercuryUnsub),
    ] {
        let pkt = MercuryPacket::request(
            4242,
            method,
            "hm://metadata/4/track/abcdef",
            vec![b"part1".to_vec(), b"part2".to_vec()],
        );
        assert_eq!(pkt.cmd, expected_cmd);
        let (cmd_byte, body) = pkt.encode_ap_frame();
        let decoded =
            MercuryPacket::decode(cmd_byte, &body).expect("decode mercury pkt");
        assert_eq!(decoded.cmd, expected_cmd);
        assert_eq!(decoded.seq_id, 4242);
        assert_eq!(decoded.header.uri, "hm://metadata/4/track/abcdef");
        assert_eq!(decoded.header.method, method);
        assert_eq!(decoded.payload_parts.len(), 2);
        assert_eq!(decoded.payload_parts[0], b"part1");
        assert_eq!(decoded.payload_parts[1], b"part2");
    }

    // Multipart assembly across two fragments (0xB5 MercuryEvent)
    let event_hdr = MercuryHeader::new("hm://remote/user/spotify/", "SEND")
        .with_status_code(200);
    let hdr_bytes = event_hdr.encode();

    let mut frag1 = Vec::new();
    frag1.extend_from_slice(&8u16.to_be_bytes());
    frag1.extend_from_slice(&99u64.to_be_bytes());
    frag1.push(0x00); // partial fragment
    frag1.extend_from_slice(&1u16.to_be_bytes());
    frag1.extend_from_slice(&(hdr_bytes.len() as u16).to_be_bytes());
    frag1.extend_from_slice(&hdr_bytes);

    let mut frag2 = Vec::new();
    frag2.extend_from_slice(&8u16.to_be_bytes());
    frag2.extend_from_slice(&99u64.to_be_bytes());
    frag2.push(0x01); // final fragment
    frag2.extend_from_slice(&2u16.to_be_bytes());
    frag2.extend_from_slice(&6u16.to_be_bytes());
    frag2.extend_from_slice(b"chunkA");
    frag2.extend_from_slice(&6u16.to_be_bytes());
    frag2.extend_from_slice(b"chunkB");

    let assembled = MercuryPacket::assemble_multipart(
        MercuryType::MercuryEvent as u8,
        &[&frag1, &frag2],
    )
    .expect("assemble multipart");
    assert_eq!(assembled.cmd, MercuryType::MercuryEvent);
    assert_eq!(assembled.seq_id, 99);
    assert_eq!(assembled.flags, 0x01);
    assert_eq!(assembled.header, event_hdr);
    assert_eq!(
        assembled.payload_parts,
        vec![b"chunkA".to_vec(), b"chunkB".to_vec()]
    );
}

#[test]
fn test_audio_key_packet_codec_roundtrip() {
    let req = AudioKeyPacket::RequestKey {
        file_id: [0x11; 20],
        track_gid: [0x22; 16],
        seq: 12345,
    };
    let (cmd_req, bytes_req) = req.encode();
    assert_eq!(cmd_req, 0x0C);
    assert_eq!(
        AudioKeyPacket::decode(cmd_req, &bytes_req).expect("decode 0x0C"),
        req
    );

    let key_resp = AudioKeyPacket::AesKey {
        seq: 12345,
        key: [0x7F; 16],
    };
    let (cmd_key, bytes_key) = key_resp.encode();
    assert_eq!(cmd_key, 0x0D);
    assert_eq!(
        AudioKeyPacket::decode(cmd_key, &bytes_key).expect("decode 0x0D"),
        key_resp
    );

    let err_resp = AudioKeyPacket::AesKeyError {
        seq: 12345,
        error_code: 0x0102,
    };
    let (cmd_err, bytes_err) = err_resp.encode();
    assert_eq!(cmd_err, 0x0E);
    assert_eq!(
        AudioKeyPacket::decode(cmd_err, &bytes_err).expect("decode 0x0E"),
        err_resp
    );
    assert_eq!(
        MercuryPacketBuilder::parse_audio_key_response(cmd_err, &bytes_err),
        Err(MercuryError::AudioKeyFailed(0x0102))
    );
}

#[test]
fn test_audio_key_decryptor_arbitrary_offset_and_header_strip() {
    let key = [0x42u8; 16];
    let decryptor = AudioKeyDecryptor::new(key);

    // Construct a 400-byte simulated Spotify CDN file:
    // - 167 bytes (0xA7) proprietary Spotify header
    // - 233 bytes Ogg Vorbis payload starting with "OggS"
    let mut plaintext = vec![0xAAu8; SPOTIFY_HEADER_SIZE];
    plaintext.extend_from_slice(b"OggS-Vorbis-Stream-Payload-");
    while plaintext.len() < 400 {
        plaintext.push((plaintext.len() & 0xFF) as u8);
    }

    // Encrypt full file at offset 0
    let mut ciphertext = plaintext.clone();
    decryptor.decrypt_in_place(&mut ciphertext, 0);
    assert_ne!(ciphertext, plaintext);

    // Decrypt from offset 0 via decrypt_chunk -> must strip 0xA7 (167-byte) header
    let mut chunk0 = ciphertext.clone();
    let ogg_slice = decryptor.decrypt_chunk(&mut chunk0, 0);
    assert_eq!(ogg_slice.len(), 400 - SPOTIFY_HEADER_SIZE);
    assert!(ogg_slice.starts_with(b"OggS-Vorbis-Stream-Payload-"));
    assert_eq!(ogg_slice, &plaintext[SPOTIFY_HEADER_SIZE..]);

    // Decrypt at unaligned byte offsets (e.g. offset 7, offset 173)
    for offset in [7usize, 16, 31, 167, 173, 255] {
        let mut sub_cipher = ciphertext[offset..offset + 64].to_vec();
        decryptor.decrypt_in_place(&mut sub_cipher, offset);
        assert_eq!(
            &sub_cipher[..],
            &plaintext[offset..offset + 64],
            "mismatch at byte_offset={offset}"
        );
    }

    // Stateful streaming decryptor with seek(0)
    let mut stateful = AudioKeyDecryptor::from_slice(&key).expect("valid key");
    stateful.seek(0);
    let mut first_half = ciphertext[..200].to_vec();
    let ogg_part1 = stateful.decrypt_next(&mut first_half);
    assert_eq!(ogg_part1, &plaintext[SPOTIFY_HEADER_SIZE..200]);
    assert_eq!(stateful.byte_offset(), 200);
}

#[test]
fn test_spirc_extended_seek_volume_queue_metadata_and_track_ended() {
    let mut sm = SpircStateMachine::new();
    sm.set_track_queue(
        vec![
            "spotify:track:track_1".to_string(),
            "spotify:track:track_2".to_string(),
            "spotify:track:track_3".to_string(),
        ],
        0,
    );
    assert_eq!(sm.current_track(), Some("spotify:track:track_1"));
    assert_eq!(sm.playing_track_index(), 0);

    // Track metadata with 20-byte artwork image ID -> https://i.scdn.co/image/<hex_id>
    let meta = TrackMetadata::new(
        "Money for Nothing",
        "Dire Straits",
        "Brothers in Arms",
        245_000,
        Some("ab67616d0000b2730123456789abcdef01234567".to_string()),
    );
    let meta_bytes = meta.encode_protobuf();
    let decoded_meta =
        TrackMetadata::decode_protobuf(&meta_bytes).expect("decode track proto");
    assert_eq!(decoded_meta, meta);
    assert_eq!(
        decoded_meta.artwork_url().as_deref(),
        Some("https://i.scdn.co/image/ab67616d0000b2730123456789abcdef01234567")
    );

    let meta_ev = sm.set_metadata(decoded_meta.clone());
    assert_eq!(meta_ev, SpircEvent::MetadataUpdated(decoded_meta.clone()));
    assert_eq!(
        sm.artwork_url().as_deref(),
        Some("https://i.scdn.co/image/ab67616d0000b2730123456789abcdef01234567")
    );

    // Seek frame roundtrip & state update
    let seek_frame = SpircFrame::seek(10, 90_000);
    let seek_parsed =
        SpircFrame::parse(&seek_frame.encode()).expect("parse seek frame");
    assert_eq!(seek_parsed, seek_frame);
    assert_eq!(sm.handle_frame(&seek_parsed), Some(SpircEvent::Seek(90_000)));
    assert_eq!(sm.position_ms(), 90_000);

    // Volume clamping to 0..=65535
    let vol_frame = SpircFrame::volume(11, 99_999);
    assert_eq!(
        sm.handle_frame(&vol_frame),
        Some(SpircEvent::SetVolume(SPOTIFY_MAX_VOLUME))
    );
    assert_eq!(sm.volume(), 65535);

    // Next / Prev transitions
    let next_frame = SpircFrame::next(12);
    assert_eq!(sm.handle_frame(&next_frame), Some(SpircEvent::Next));
    assert_eq!(sm.current_track(), Some("spotify:track:track_2"));
    assert_eq!(sm.position_ms(), 0);

    let prev_frame = SpircFrame::prev(13);
    assert_eq!(sm.handle_frame(&prev_frame), Some(SpircEvent::Prev));
    assert_eq!(sm.current_track(), Some("spotify:track:track_1"));

    // Start playing and advance position until TrackEnded
    sm.set_metadata(decoded_meta);
    sm.set_status(PlayStatus::Play);
    assert_eq!(sm.advance_position_ms(200_000), None);
    assert_eq!(sm.position_ms(), 200_000);
    assert_eq!(sm.advance_position_ms(50_000), Some(SpircEvent::TrackEnded));
    assert_eq!(sm.current_track(), Some("spotify:track:track_2"));
    assert_eq!(sm.position_ms(), 0);
}

#[test]
fn test_wo253_spirc_empty_queue_clears_current_track() {
    let mut sm = SpircStateMachine::new();
    sm.set_track_queue(vec!["spotify:track:abc".to_string()], 0);
    assert_eq!(sm.current_track(), Some("spotify:track:abc"));
    sm.set_track_queue(Vec::new(), 0);
    assert_eq!(sm.current_track(), None);
}

#[test]
fn test_wo253_zeroconf_url_decode_utf8() {
    use squeezelite_cspot::zeroconf::get_param;
    let query = "userName=Caf%C3%A9+User&device=ESP32";
    let user = get_param(query, "userName").expect("username");
    assert_eq!(user, "Café User");
}

#[test]
fn test_wo253_mercury_truncated_part_allocation() {
    // seq_len = 2, seq_id = 0x0102, flags = 0x01, part_count = 10 (within MAX_MERCURY_PARTS bound)
    // but data ends right after part_count
    let corrupt = vec![0x00, 0x02, 0x01, 0x02, 0x01, 0x00, 0x0A];
    let err = MercuryPacket::decode(0xB2, &corrupt);
    assert_eq!(err, Err(MercuryError::TruncatedPacket));
}

#[test]
fn test_wo253_spirc_protobuf_wire_types_1_and_5() {
    // Construct a protobuf Track submessage with unknown wire type 1 and 5 fields
    let mut data = Vec::new();
    // Field 99 (wire type 1 = 64-bit fixed)
    data.push((99 << 3) | 1);
    data.extend_from_slice(&0x1122334455667788u64.to_le_bytes());
    // Field 98 (wire type 5 = 32-bit fixed)
    data.push((98 << 3) | 5);
    data.extend_from_slice(&0xAABBCCDDu32.to_le_bytes());
    // Field 2 (string, wire type 2): title = "Test Song"
    data.push((2 << 3) | 2);
    data.push(9);
    data.extend_from_slice(b"Test Song");

    let meta = TrackMetadata::decode_protobuf(&data).expect("decode with fixed32/fixed64");
    assert_eq!(meta.title, "Test Song");
}


