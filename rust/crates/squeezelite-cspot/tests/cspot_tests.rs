//! Comprehensive unit and golden vector tests for pure-Rust Spotify Connect (cspot) engine.

use squeezelite_cspot::cdn::CdnAudioDecryptor;
use squeezelite_cspot::cdn::CdnStreamFeeder;
use squeezelite_cspot::cdn::AUDIO_AES_IV;
use squeezelite_cspot::crypto::base64_decode;
use squeezelite_cspot::crypto::base64_encode;
use squeezelite_cspot::crypto::big_num_add;
use squeezelite_cspot::crypto::hmac_sha1;
use squeezelite_cspot::crypto::pbkdf2_hmac_sha1;
use squeezelite_cspot::crypto::Aes128Ctr;
use squeezelite_cspot::crypto::Aes192Ecb;
use squeezelite_cspot::crypto::OakleyGroup1Dh;
use squeezelite_cspot::crypto::Sha1;
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
use squeezelite_cspot::zeroconf::SpotifyMdnsBuilder;
use squeezelite_cspot::zeroconf::SpotifyMdnsConfig;
use squeezelite_cspot::zeroconf::ZeroConfServer;

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

    let resp = ZeroConfServer::handle_request(
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
