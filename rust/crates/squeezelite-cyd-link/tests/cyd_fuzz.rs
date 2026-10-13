//! Property-based fuzz tests for `squeezelite-cyd-link`
//!
//! Verifies zero-panic guarantees and protocol roundtrip invariants for the
//! Cheap Yellow Display (CYD) UART communication protocol.

use heapless::Vec;
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

    /// Invariant: `parse_command` must NEVER panic on arbitrary UTF-8 strings.
    #[test]
    fn fuzz_parse_command_arbitrary_strings(s in "\\PC*") {
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

    /// Invariant: Roundtrip encoding & decoding of CydCommand::SetVolume (0..=255).
    #[test]
    fn fuzz_command_set_volume_roundtrip(vol in any::<u8>()) {
        let mut buf = Vec::new();
        let enc_res = CydLinkProtocol::encode_command(&CydCommand::SetVolume(vol), &mut buf);
        prop_assert!(enc_res.is_ok());

        let dec_res = CydLinkProtocol::parse_command(&buf);
        prop_assert!(dec_res.is_ok());
        prop_assert_eq!(dec_res.unwrap(), CydCommand::SetVolume(vol));
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

    /// Invariant: Roundtrip encoding & decoding of touch interaction commands.
    #[test]
    fn fuzz_touch_commands_roundtrip(cmd_idx in 0..6u8) {
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

        let dec_res = CydLinkProtocol::parse_command(&buf);
        prop_assert!(dec_res.is_ok());
        prop_assert_eq!(dec_res.unwrap(), cmd);
    }

    /// Invariant: Progress messages must encode without overflow and terminate with newline.
    #[test]
    fn fuzz_progress_encoding(elapsed in any::<u32>(), duration in any::<u32>()) {
        let mut buf = Vec::new();
        let res = CydLinkProtocol::encode_message(
            &CydMessage::Progress { elapsed_secs: elapsed, duration_secs: duration },
            &mut buf,
        );
        prop_assert!(res.is_ok());
        prop_assert_eq!(buf.last(), Some(&b'\n'));
        prop_assert!(buf.contains(&b'/'));
    }

    /// Invariant: WiFi status messages must encode without overflow and terminate with newline.
    #[test]
    fn fuzz_wifi_status_encoding(connected in any::<bool>(), rssi in any::<i8>()) {
        let mut buf = Vec::new();
        let res = CydLinkProtocol::encode_message(
            &CydMessage::WifiStatus { connected, rssi },
            &mut buf,
        );
        prop_assert!(res.is_ok());
        prop_assert_eq!(buf.last(), Some(&b'\n'));
        prop_assert!(buf.contains(&b';'));
    }
}
