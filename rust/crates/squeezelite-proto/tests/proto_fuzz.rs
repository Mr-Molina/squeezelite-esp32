//! Property-based fuzz tests for `squeezelite-proto`
//!
//! Verifies zero-panic guarantees and mathematical parsing invariants across
//! arbitrary adversarial byte sequences and mutated packets.

use proptest::prelude::*;
use squeezelite_proto::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Invariant: `SlimprotoParser::parse_frame` must NEVER panic on arbitrary byte sequences.
    /// If it returns `Ok((_, consumed))`, then `consumed <= buf.len()` and `consumed >= 6`.
    #[test]
    fn fuzz_parse_frame_arbitrary_bytes(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        match SlimprotoParser::parse_frame(&data) {
            Ok((_cmd, consumed)) => {
                prop_assert!(consumed <= data.len(), "Consumed {} > total {}", consumed, data.len());
                prop_assert!(consumed >= 6, "Consumed {} < min header size 6", consumed);
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
        buf[15] = (threshold_kb % 256) as u8;
        buf[16] = spdif;
        buf[17] = trans_dur;
        buf[18] = trans_type;
        buf[19] = flags;
        buf[20] = output_thresh;
        buf[21] = 0; // slaves
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
                prop_assert_eq!(autostart, autostart_byte == b'1' || autostart_byte == b'3');
                prop_assert_eq!(format, AudioFormat::from_byte(format_byte));
                prop_assert_eq!(t_kb, threshold_kb % 256);
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
        let payload_len = 18 + extra_len;
        let total_len = 8 + payload_len;
        let mut buf = vec![0u8; total_len];

        buf[0..4].copy_from_slice(b"audg");
        buf[4..8].copy_from_slice(&(payload_len as u32).to_be_bytes());
        // 8..16 legacy old_gain
        buf[16] = digital_atten;
        buf[17] = preamp;
        buf[18..22].copy_from_slice(&gain_l.to_be_bytes());
        buf[22..26].copy_from_slice(&gain_r.to_be_bytes());

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
        let res = DiscoveryParser::parse_response(&data);
        if data.is_empty() || data[0] != b'E' {
            prop_assert!(res.is_none());
        }
    }

    /// Invariant: `SlimprotoBuilder::build_stat` must always produce a 61-byte packet starting with STAT.
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

        let mut out = [0u8; 64];
        let res = SlimprotoBuilder::build_stat(event, stream_buf, output_buf, frames, ts, &mut out);
        prop_assert!(res.is_ok());
        let len = res.unwrap();
        prop_assert_eq!(len, 61);
        prop_assert_eq!(&out[0..4], b"STAT");
        prop_assert_eq!(&out[4..8], &53u32.to_be_bytes());
        prop_assert_eq!(&out[8..12], &event.opcode_bytes());
        prop_assert_eq!(out[12], 0);
        prop_assert_eq!(out[13], 0);
        prop_assert_eq!(out[14], 0);
        prop_assert_eq!(u32::from_be_bytes([out[15], out[16], out[17], out[18]]), 65536);
        prop_assert_eq!(u32::from_be_bytes([out[19], out[20], out[21], out[22]]), stream_buf);
        prop_assert_eq!(u32::from_be_bytes([out[23], out[24], out[25], out[26]]), 0);
        prop_assert_eq!(u32::from_be_bytes([out[27], out[28], out[29], out[30]]), frames);
        prop_assert_eq!(u16::from_be_bytes([out[31], out[32]]), 0xFFFF);
        prop_assert_eq!(u32::from_be_bytes([out[33], out[34], out[35], out[36]]), ts);
        prop_assert_eq!(u32::from_be_bytes([out[37], out[38], out[39], out[40]]), 65536);
        prop_assert_eq!(u32::from_be_bytes([out[41], out[42], out[43], out[44]]), output_buf);
        prop_assert_eq!(u32::from_be_bytes([out[45], out[46], out[47], out[48]]), ts / 1000);
        prop_assert_eq!(u16::from_be_bytes([out[49], out[50]]), 0);
        prop_assert_eq!(u32::from_be_bytes([out[51], out[52], out[53], out[54]]), ts);
        prop_assert_eq!(u32::from_be_bytes([out[55], out[56], out[57], out[58]]), 0);
        prop_assert_eq!(u16::from_be_bytes([out[59], out[60]]), 0);
    }

    /// Invariant: `sample_rate_from_index` must return a non-zero, standard audio frequency for all u8 inputs.
    #[test]
    fn fuzz_sample_rate_all_indices(index in any::<u8>()) {
        let rate = sample_rate_from_index(index);
        prop_assert!((8000..=192000).contains(&rate), "Sample rate {} out of valid bounds", rate);
    }

    /// Invariant: Official 6-byte server-to-client frames parse correctly.
    #[test]
    fn fuzz_parse_6byte_frames(
        data in proptest::collection::vec(any::<u8>(), 0..512),
    ) {
        let length = (4 + data.len()) as u16;
        let mut buf = vec![0u8; 2 + 4 + data.len()];
        buf[0..2].copy_from_slice(&length.to_be_bytes());
        buf[2..6].copy_from_slice(b"body");
        buf[6..].copy_from_slice(&data);

        let res = SlimprotoParser::parse_frame(&buf);
        prop_assert!(res.is_ok());
        let (cmd, consumed) = res.unwrap();
        prop_assert_eq!(consumed, buf.len());
        prop_assert_eq!(cmd, ServerCommand::Body { data: &data });
    }

    /// Invariant: Discovery parser handles both 1-byte TLV and null-terminated strings for valid ports.
    #[test]
    fn fuzz_discovery_parser_tlv_and_null(
        json_port in 1..=65535u16,
        cli_port in 1..=65535u16,
    ) {
        // Test standard 1-byte TLV format
        let mut json_str = [0u8; 5];
        let json_len = write_port_u16(json_port, &mut json_str);
        let mut cli_str = [0u8; 5];
        let cli_len = write_port_u16(cli_port, &mut cli_str);

        let mut tlv_packet = Vec::new();
        tlv_packet.push(b'E');
        tlv_packet.extend_from_slice(b"JSON");
        tlv_packet.push(json_len as u8);
        tlv_packet.extend_from_slice(&json_str[..json_len]);
        tlv_packet.extend_from_slice(b"CLIP");
        tlv_packet.push(cli_len as u8);
        tlv_packet.extend_from_slice(&cli_str[..cli_len]);

        let parsed_tlv = DiscoveryParser::parse_response(&tlv_packet);
        prop_assert!(parsed_tlv.is_some());
        let server_tlv = parsed_tlv.unwrap();
        prop_assert_eq!(server_tlv.json_port, json_port);
        prop_assert_eq!(server_tlv.cli_port, cli_port);

        // Test null-terminated fallback format
        let mut null_packet = Vec::new();
        null_packet.push(b'E');
        null_packet.extend_from_slice(b"JSON");
        null_packet.extend_from_slice(&json_str[..json_len]);
        null_packet.push(0);
        null_packet.extend_from_slice(b"CLIP");
        null_packet.extend_from_slice(&cli_str[..cli_len]);
        null_packet.push(0);

        let parsed_null = DiscoveryParser::parse_response(&null_packet);
        prop_assert!(parsed_null.is_some());
        let server_null = parsed_null.unwrap();
        prop_assert_eq!(server_null.json_port, json_port);
        prop_assert_eq!(server_null.cli_port, cli_port);
    }

    /// Invariant: setd parses correctly for 1-byte setting query and 4-byte sample rate.
    #[test]
    fn fuzz_parse_setd(
        sample_rate in any::<u32>(),
        setting_byte in any::<u8>(),
    ) {
        // 4-byte sample rate (6-byte frame)
        let mut buf_4b = [0u8; 10];
        buf_4b[0..2].copy_from_slice(&8u16.to_be_bytes());
        buf_4b[2..6].copy_from_slice(b"setd");
        buf_4b[6..10].copy_from_slice(&sample_rate.to_be_bytes());
        let res_4b = SlimprotoParser::parse_frame(&buf_4b);
        prop_assert!(res_4b.is_ok());
        let (cmd, consumed) = res_4b.unwrap();
        prop_assert_eq!(consumed, 10);
        prop_assert_eq!(cmd, ServerCommand::SetDevice { sample_rate });

        // 1-byte setting query (6-byte frame)
        let mut buf_1b = [0u8; 7];
        buf_1b[0..2].copy_from_slice(&5u16.to_be_bytes());
        buf_1b[2..6].copy_from_slice(b"setd");
        buf_1b[6] = setting_byte;
        let res_1b = SlimprotoParser::parse_frame(&buf_1b);
        prop_assert!(res_1b.is_ok());
        let (cmd1, consumed1) = res_1b.unwrap();
        prop_assert_eq!(consumed1, 7);
        prop_assert_eq!(cmd1, ServerCommand::SetDevice { sample_rate: setting_byte as u32 });
    }

    /// Invariant: negative truncation bounds return Err(ParseError::InvalidFormat).
    #[test]
    fn fuzz_negative_truncation_bounds(
        strm_len in 0..24usize,
        audg_len in 0..18usize,
    ) {
        // Truncated strm
        let mut strm_buf = vec![0u8; 8 + strm_len];
        strm_buf[0..4].copy_from_slice(b"strm");
        strm_buf[4..8].copy_from_slice(&(strm_len as u32).to_be_bytes());
        prop_assert_eq!(SlimprotoParser::parse_frame(&strm_buf), Err(ParseError::InvalidFormat));

        // Truncated audg
        let mut audg_buf = vec![0u8; 8 + audg_len];
        audg_buf[0..4].copy_from_slice(b"audg");
        audg_buf[4..8].copy_from_slice(&(audg_len as u32).to_be_bytes());
        prop_assert_eq!(SlimprotoParser::parse_frame(&audg_buf), Err(ParseError::InvalidFormat));

        // Empty setd (0 bytes payload)
        let mut setd_buf = [0u8; 8];
        setd_buf[0..4].copy_from_slice(b"setd");
        setd_buf[4..8].copy_from_slice(&0u32.to_be_bytes());
        prop_assert_eq!(SlimprotoParser::parse_frame(&setd_buf), Err(ParseError::InvalidFormat));
    }
}

fn write_port_u16(mut port: u16, out: &mut [u8; 5]) -> usize {
    if port == 0 {
        out[0] = b'0';
        return 1;
    }
    let mut temp = [0u8; 5];
    let mut i = 0;
    while port > 0 {
        temp[i] = b'0' + (port % 10) as u8;
        port /= 10;
        i += 1;
    }
    for j in 0..i {
        out[j] = temp[i - 1 - j];
    }
    i
}
