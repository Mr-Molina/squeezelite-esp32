//! Property-based fuzz tests for `squeezelite-proto`
//!
//! Verifies zero-panic guarantees and mathematical parsing invariants across
//! arbitrary adversarial byte sequences and mutated packets.

use proptest::prelude::*;
use squeezelite_proto::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Invariant: `SlimprotoParser::parse_frame` must NEVER panic on arbitrary byte sequences.
    /// If it returns `Ok((_, consumed))`, then `consumed <= buf.len()` and `consumed >= 8`.
    #[test]
    fn fuzz_parse_frame_arbitrary_bytes(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        match SlimprotoParser::parse_frame(&data) {
            Ok((_cmd, consumed)) => {
                prop_assert!(consumed <= data.len(), "Consumed {} > total {}", consumed, data.len());
                prop_assert!(consumed >= 8, "Consumed {} < header size 8", consumed);
            }
            Err(e) => {
                // Must be one of the bounded parse errors
                prop_assert!(matches!(
                    e,
                    ParseError::BufferTooShort | ParseError::PayloadTooLarge | ParseError::InvalidFormat
                ));
            }
        }
    }

    /// Invariant: `SlimprotoParser::parse_frame` on mutated `strm` packets must never panic.
    #[test]
    fn fuzz_parse_mutated_strm_frame(
        cmd_byte in any::<u8>(),
        autostart_byte in any::<u8>(),
        format_byte in any::<u8>(),
        sample_size in any::<u8>(),
        sample_rate_idx in any::<u8>(),
        channels in any::<u8>(),
        endian_byte in any::<u8>(),
        threshold_kb in any::<u16>(),
        spdif in any::<u8>(),
        trans_dur in any::<u8>(),
        trans_type in any::<u8>(),
        flags in any::<u8>(),
        output_thresh in any::<u8>(),
        replay_gain in any::<u32>(),
        server_port in any::<u16>(),
        server_ip in any::<[u8; 4]>(),
        extra_headers in proptest::collection::vec(any::<u8>(), 0..256),
    ) {
        let payload_len = 24 + extra_headers.len();
        let total_len = 8 + payload_len;
        let mut buf = vec![0u8; total_len];

        // Opcode + length
        buf[0..4].copy_from_slice(b"strm");
        buf[4..8].copy_from_slice(&(payload_len as u32).to_be_bytes());

        // Payload
        buf[8] = cmd_byte;
        buf[9] = autostart_byte;
        buf[10] = format_byte;
        buf[11] = sample_size;
        buf[12] = sample_rate_idx;
        buf[13] = channels;
        buf[14] = endian_byte;
        buf[15..17].copy_from_slice(&threshold_kb.to_be_bytes());
        buf[17] = spdif;
        buf[18] = trans_dur;
        buf[19] = trans_type;
        buf[20] = flags;
        buf[21] = output_thresh;
        buf[22..26].copy_from_slice(&replay_gain.to_be_bytes());
        buf[26..28].copy_from_slice(&server_port.to_be_bytes());
        buf[28..32].copy_from_slice(&server_ip);
        if !extra_headers.is_empty() {
            buf[32..total_len].copy_from_slice(&extra_headers);
        }

        let res = SlimprotoParser::parse_frame(&buf);
        prop_assert!(res.is_ok(), "Validly framed strm packet should parse without error");
        let (cmd, consumed) = res.unwrap();
        prop_assert_eq!(consumed, total_len);

        match cmd {
            ServerCommand::Stream {
                command,
                autostart,
                format,
                threshold_kb: t_kb,
                server_port: port,
                server_ip: ip,
                http_headers,
                ..
            } => {
                prop_assert_eq!(command, StreamCommandType::from_byte(cmd_byte));
                prop_assert_eq!(autostart, autostart_byte == b'1');
                prop_assert_eq!(format, AudioFormat::from_byte(format_byte));
                prop_assert_eq!(t_kb, threshold_kb);
                prop_assert_eq!(port, server_port);
                prop_assert_eq!(ip, server_ip);
                prop_assert_eq!(http_headers, &extra_headers[..]);
            }
            _ => prop_assert!(false, "Expected ServerCommand::Stream"),
        }
    }

    /// Invariant: `SlimprotoParser::parse_frame` on mutated `audg` packets must never panic.
    #[test]
    fn fuzz_parse_mutated_audg_frame(
        gain_l in any::<u32>(),
        gain_r in any::<u32>(),
        digital_atten in any::<u8>(),
        preamp in any::<u8>(),
        extra_len in 0..10usize,
    ) {
        let payload_len = 10 + extra_len;
        let total_len = 8 + payload_len;
        let mut buf = vec![0u8; total_len];

        buf[0..4].copy_from_slice(b"audg");
        buf[4..8].copy_from_slice(&(payload_len as u32).to_be_bytes());
        buf[8..12].copy_from_slice(&gain_l.to_be_bytes());
        buf[12..16].copy_from_slice(&gain_r.to_be_bytes());
        buf[16] = digital_atten;
        buf[17] = preamp;

        let res = SlimprotoParser::parse_frame(&buf);
        prop_assert!(res.is_ok());
        let (cmd, consumed) = res.unwrap();
        prop_assert_eq!(consumed, total_len);
        match cmd {
            ServerCommand::AudioGain {
                gain_left,
                gain_right,
                digital_atten: atten,
                preamp: pre,
            } => {
                prop_assert_eq!(gain_left, gain_l);
                prop_assert_eq!(gain_right, gain_r);
                prop_assert_eq!(atten, digital_atten);
                prop_assert_eq!(pre, preamp);
            }
            _ => prop_assert!(false, "Expected ServerCommand::AudioGain"),
        }
    }

    /// Invariant: `DiscoveryParser::parse_response` must never panic on arbitrary byte sequences.
    #[test]
    fn fuzz_discovery_parser_arbitrary_bytes(data in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let _ = DiscoveryParser::parse_response(&data);
    }

    /// Invariant: `SlimprotoBuilder::build_stat` must always produce a 28-byte packet starting with STAT.
    #[test]
    fn fuzz_stat_builder_soundness(
        event_code in 0..9u8,
        stream_buf in any::<u32>(),
        output_buf in any::<u32>(),
        frames in any::<u32>(),
        ts in any::<u32>(),
    ) {
        let event = match event_code {
            0 => StatEvent::Heartbeat,
            1 => StatEvent::TrackStarted,
            2 => StatEvent::TrackPaused,
            3 => StatEvent::TrackResumed,
            4 => StatEvent::TrackStopped,
            5 => StatEvent::StreamUnderrun,
            6 => StatEvent::OutputUnderrun,
            7 => StatEvent::StreamFlushed,
            _ => StatEvent::Connected,
        };

        let mut out = [0u8; 32];
        let res = SlimprotoBuilder::build_stat(event, stream_buf, output_buf, frames, ts, &mut out);
        prop_assert!(res.is_ok());
        let len = res.unwrap();
        prop_assert_eq!(len, 28);
        prop_assert_eq!(&out[0..4], b"STAT");
        prop_assert_eq!(&out[8..12], &event.opcode_bytes());

        let parsed_stream = u32::from_be_bytes([out[12], out[13], out[14], out[15]]);
        let parsed_output = u32::from_be_bytes([out[16], out[17], out[18], out[19]]);
        let parsed_frames = u32::from_be_bytes([out[20], out[21], out[22], out[23]]);
        let parsed_ts = u32::from_be_bytes([out[24], out[25], out[26], out[27]]);

        prop_assert_eq!(parsed_stream, stream_buf);
        prop_assert_eq!(parsed_output, output_buf);
        prop_assert_eq!(parsed_frames, frames);
        prop_assert_eq!(parsed_ts, ts);
    }

    /// Invariant: `sample_rate_from_index` must return a non-zero, standard audio frequency for all u8 inputs.
    #[test]
    fn fuzz_sample_rate_all_indices(index in any::<u8>()) {
        let rate = sample_rate_from_index(index);
        prop_assert!((8000..=192000).contains(&rate), "Sample rate {} out of valid bounds", rate);
    }
}
