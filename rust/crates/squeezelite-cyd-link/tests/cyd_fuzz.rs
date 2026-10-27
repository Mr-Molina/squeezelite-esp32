//! Property-based fuzz tests for `squeezelite-cyd-link`
//!
//! Verifies zero-panic guarantees and protocol roundtrip invariants for the
//! Cheap Yellow Display (CYD) UART communication protocol.

use heapless::{String, Vec};
use proptest::prelude::*;
use squeezelite_cyd_link::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Invariant: `parse_line` must NEVER panic on arbitrary byte sequences.
    #[test]
    fn fuzz_parse_line_arbitrary_bytes(data in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let _ = CydLinkProtocol::parse_line(&data);
    }

    /// Invariant: `parse_command` must NEVER panic on arbitrary byte sequences.
    #[test]
    fn fuzz_parse_command_arbitrary_bytes(data in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let _ = CydLinkProtocol::parse_command(&data);
    }

    /// Invariant: `parse_command` must NEVER panic on arbitrary UTF-8 strings including control chars.
    #[test]
    fn fuzz_parse_command_arbitrary_strings(s in ".*") {
        let _ = CydLinkProtocol::parse_command(s.as_bytes());
    }

    /// Invariant: Roundtrip encoding & decoding of volume levels (0..=255).
    #[test]
    fn fuzz_volume_roundtrip(vol in any::<u8>()) {
        let mut buf = Vec::new();
        let enc_res = CydLinkProtocol::encode_message(&CydMessage::Volume(vol), &mut buf);
        prop_assert!(enc_res.is_ok());

        let dec_res = CydLinkProtocol::parse_line(&buf);
        prop_assert!(dec_res.is_ok());
        prop_assert_eq!(dec_res.unwrap(), CydMessage::Volume(vol));
    }

    /// Invariant: Roundtrip encoding & decoding of CydCommand::SetVolume (0..=100).
    #[test]
    fn fuzz_command_set_volume_roundtrip(vol in 0..=100u8) {
        let mut buf = Vec::new();
        let enc_res = CydLinkProtocol::encode_command(&CydCommand::SetVolume(vol), &mut buf);
        prop_assert!(enc_res.is_ok());

        let dec_res = CydLinkProtocol::parse_command(&buf);
        prop_assert!(dec_res.is_ok());
        prop_assert_eq!(dec_res.unwrap(), CydCommand::SetVolume(vol));
    }

    /// Invariant: Out-of-range volume levels (>100) are rejected with ValueOutOfRange.
    #[test]
    fn fuzz_command_set_volume_out_of_range(vol in 101..=255u8) {
        let mut buf = Vec::new();
        let enc_res = CydLinkProtocol::encode_command(&CydCommand::SetVolume(vol), &mut buf);
        prop_assert_eq!(enc_res, Err(LinkError::ValueOutOfRange));

        // Format as ASCII CMD=VOL:<vol> and assert parse_command rejects
        let mut ascii_cmd = [0u8; 16];
        ascii_cmd[..8].copy_from_slice(b"CMD=VOL:");
        let mut n = 8;
        let mut temp = [0u8; 3];
        let mut v = vol;
        let mut digits = 0;
        while v > 0 {
            temp[digits] = b'0' + (v % 10);
            v /= 10;
            digits += 1;
        }
        for d in 0..digits {
            ascii_cmd[n] = temp[digits - 1 - d];
            n += 1;
        }
        ascii_cmd[n] = b'\n';
        n += 1;

        let dec_res = CydLinkProtocol::parse_command(&ascii_cmd[..n]);
        prop_assert_eq!(dec_res, Err(LinkError::ValueOutOfRange));
    }

    /// Invariant: Roundtrip encoding & decoding of playback states.
    #[test]
    fn fuzz_playback_state_roundtrip(state_idx in 0..3u8) {
        let state = match state_idx {
            0 => PlayState::Stopped,
            1 => PlayState::Playing,
            _ => PlayState::Paused,
        };

        let mut buf = Vec::new();
        let enc_res = CydLinkProtocol::encode_message(&CydMessage::PlaybackState(state), &mut buf);
        prop_assert!(enc_res.is_ok());

        let dec_res = CydLinkProtocol::parse_line(&buf);
        prop_assert!(dec_res.is_ok());
        prop_assert_eq!(dec_res.unwrap(), CydMessage::PlaybackState(state));
    }

    /// Invariant: Roundtrip encoding & decoding of touch interaction commands with CRLF tolerance.
    #[test]
    fn fuzz_touch_commands_roundtrip(cmd_idx in 0..6u8, crlf in any::<bool>()) {
        let cmd = match cmd_idx {
            0 => CydCommand::Play,
            1 => CydCommand::Pause,
            2 => CydCommand::Toggle,
            3 => CydCommand::Next,
            4 => CydCommand::Previous,
            _ => CydCommand::QueryStatus,
        };

        let mut buf = Vec::new();
        let enc_res = CydLinkProtocol::encode_command(&cmd, &mut buf);
        prop_assert!(enc_res.is_ok());

        if crlf {
            // Replace \n with \r\n
            if buf.last() == Some(&b'\n') {
                buf.pop();
                let _ = buf.push(b'\r');
                let _ = buf.push(b'\n');
            }
        }

        let dec_res = CydLinkProtocol::parse_command(&buf);
        prop_assert!(dec_res.is_ok());
        prop_assert_eq!(dec_res.unwrap(), cmd);
    }

    /// Invariant: Progress messages roundtrip correctly.
    #[test]
    fn fuzz_progress_roundtrip(elapsed in any::<u32>(), duration in any::<u32>()) {
        let mut buf = Vec::new();
        let msg = CydMessage::Progress { elapsed_secs: elapsed, duration_secs: duration };
        let res = CydLinkProtocol::encode_message(&msg, &mut buf);
        prop_assert!(res.is_ok());

        let parsed = CydLinkProtocol::parse_line(&buf);
        prop_assert!(parsed.is_ok());
        prop_assert_eq!(parsed.unwrap(), msg);
    }

    /// Invariant: WiFi status messages roundtrip correctly.
    #[test]
    fn fuzz_wifi_status_roundtrip(connected in any::<bool>(), rssi in any::<i8>()) {
        let mut buf = Vec::new();
        let msg = CydMessage::WifiStatus { connected, rssi };
        let res = CydLinkProtocol::encode_message(&msg, &mut buf);
        prop_assert!(res.is_ok());

        let parsed = CydLinkProtocol::parse_line(&buf);
        prop_assert!(parsed.is_ok());
        prop_assert_eq!(parsed.unwrap(), msg);
    }

    /// Invariant: Track metadata never crashes on arbitrary strings and strips forbidden delimiters.
    /// Preserves semicolons in title (LOGIC-404).
    #[test]
    fn fuzz_metadata_sanitization(
        artist_str in "[a-zA-Z0-9;\\r\\n ]{0,60}",
        title_str in "[a-zA-Z0-9;\\r\\n ]{0,60}",
    ) {
        let mut artist = String::new();
        let _ = artist.push_str(&artist_str);
        let mut title = String::new();
        let _ = title.push_str(&title_str);

        let msg = CydMessage::TrackMetadata { artist, title };
        let mut buf = Vec::new();
        let res = CydLinkProtocol::encode_message(&msg, &mut buf);
        prop_assert!(res.is_ok());

        // Must parse back successfully without corruption
        let parsed = CydLinkProtocol::parse_line(&buf);
        prop_assert!(parsed.is_ok());

        if let Ok(CydMessage::TrackMetadata { artist: parsed_artist, title: parsed_title }) = parsed {
            // Semicolons in title must be preserved
            let expected_semicolons = title_str.chars().filter(|&c| c == ';').count();
            let actual_semicolons = parsed_title.chars().filter(|&c| c == ';').count();
            prop_assert_eq!(actual_semicolons, expected_semicolons);

            // Artist must have all semicolons stripped/sanitized
            prop_assert!(!parsed_artist.contains(';'));
        }
    }
}
