//! Task 12 Review Focus: the gapless track boundary (C `output.track_start`) under arbitrary
//! interleavings of decoding, partial sink writes, rewinds, skips, `strm f` and new streams.
//!
//! The pipeline keeps the boundary as a distance from the output read position
//! ([`TrackMarker::bytes_ahead`]). Whatever moves the read position (output, skip, rewind) or
//! the write position (decode, `strm f` truncation) must keep that distance on the first frame
//! of the track it marks: never behind the read position (a "negative" distance), never past
//! the written data, never off a frame boundary.
#![cfg(not(target_arch = "xtensa"))]

use louder_esp32_app::pipeline::{ActiveDecoder, StreamPipeline, TrackMarker};
use louder_esp32_app::AudioOutputSink;
use proptest::prelude::*;
use squeezelite_audio_core::OutputError;
use squeezelite_decoders::PcmDecoder;

/// Frames per track are numbered from 0; a frame's left sample is `track * 1000 + index`.
const TRACK_STRIDE: i16 = 1000;
const MAX_TRACK_FRAMES: usize = 900;

/// Sink that accepts at most `limit` frames per write (0: a full DMA queue).
struct LimitedSink {
    samples: Vec<i32>,
    limit: usize,
}

impl AudioOutputSink for LimitedSink {
    type Error = OutputError;

    fn write_frames(&mut self, frames: &[i32]) -> Result<usize, Self::Error> {
        let n = (frames.len() / 2).min(self.limit);
        self.samples.extend_from_slice(&frames[..n * 2]);
        Ok(n)
    }

    fn set_sample_rate(&mut self, _rate_hz: u32) -> Result<(), Self::Error> {
        Ok(())
    }

    fn set_mute(&mut self, _muted: bool) {}
}

#[derive(Debug, Clone)]
enum Op {
    /// Stream this many more frames of the current track and decode them.
    Feed(usize),
    /// `strm s` for the next track (stream ring flushed, codec opened, output kept).
    NewTrack,
    /// One output step with a sink accepting at most this many frames.
    Output(usize),
    /// Rewind the output read position by this many frames.
    Rewind(usize),
    /// `strm a`: skip this many frames.
    Skip(u64),
    /// `strm f`: drop the next track's unplayed data.
    FlushStreaming,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (1usize..48).prop_map(Op::Feed),
        1 => Just(Op::NewTrack),
        4 => (0usize..80).prop_map(Op::Output),
        2 => (0usize..120).prop_map(Op::Rewind),
        1 => (0u64..120).prop_map(Op::Skip),
        1 => Just(Op::FlushStreaming),
    ]
}

/// Left sample of an output-ring frame (native-endian i32 pair), as (track, index).
fn frame_id(bytes: &[u8]) -> (i32, i32) {
    let left = i32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) >> 16;
    (left / i32::from(TRACK_STRIDE), left % i32::from(TRACK_STRIDE))
}

fn pcm(track: i16, from: usize, frames: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(frames * 4);
    for i in from..from + frames {
        let v = track * TRACK_STRIDE + i as i16;
        out.extend_from_slice(&v.to_be_bytes());
        out.extend_from_slice(&v.to_be_bytes());
    }
    out
}

/// The boundary sits on whole frames inside the unread output, on the first frame of a track
/// that is not newer than the current one.
fn check_marker(p: &StreamPipeline<'_, LimitedSink>, current_track: i16) -> Result<(), TestCaseError> {
    let Some(marker) = p.track_marker().copied() else { return Ok(()) };
    let unread = p.output_buffer_available() as u64;
    prop_assert_eq!(marker.bytes_ahead % 8, 0, "boundary off a frame: {:?}", marker);
    prop_assert!(marker.bytes_ahead <= unread, "boundary past the written data: {:?} unread {}", marker, unread);
    if marker.bytes_ahead < unread {
        let at = marker.bytes_ahead as usize;
        let mut buf = vec![0u8; at + 8];
        prop_assert_eq!(p.output_buffer.peek(&mut buf), at + 8);
        let (track, index) = frame_id(&buf[at..]);
        prop_assert_eq!(index, 0, "boundary is not on a track start: {:?}", marker);
        prop_assert!(track >= 1 && track <= i32::from(current_track));
        if at > 0 {
            // The frame just before the boundary belongs to an older track.
            let (before, _) = frame_id(&buf[at - 8..at]);
            prop_assert!(before < track, "frame before the boundary is from track {}", before);
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn marker_distances_never_negative_after_rewind(ops in prop::collection::vec(op(), 1..120)) {
        let mut stream_storage = [0u8; 1024];
        // Small odd-sized output ring: positions wrap and rewinds can be clamped mid-frame.
        let mut output_storage = [0u8; 517];
        let mut p = StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Pcm(PcmDecoder::new(16, 2, true)),
            LimitedSink { samples: Vec::new(), limit: 0 },
            44_100,
        );
        let mut track: i16 = 1;
        let mut fed = 0usize;
        let mut stream_open = true;

        for op in ops {
            match op {
                Op::Feed(frames) => {
                    let frames = frames
                        .min(MAX_TRACK_FRAMES - fed)
                        .min(p.stream_buffer.available_write() / 4);
                    if stream_open && frames > 0 {
                        p.ingest_stream_data(&pcm(track, fed, frames)).unwrap();
                        fed += frames;
                    }
                    for _ in 0..64 {
                        let progressed = matches!(p.process_decode_step(), Ok(n) if n > 0);
                        if !progressed {
                            break;
                        }
                    }
                }
                Op::NewTrack => {
                    if track < 30 {
                        track += 1;
                        fed = 0;
                        stream_open = true;
                        p.begin_stream(44_100, 0, 0);
                        p.set_decoder(ActiveDecoder::Pcm(PcmDecoder::new(16, 2, true)));
                        p.open_stream();
                    }
                }
                Op::Output(limit) => {
                    p.sink.limit = limit;
                    let before = p.sink.samples.len();
                    let written = p.process_output_step().unwrap();
                    prop_assert_eq!(p.sink.samples.len() - before, written * 2);
                    let crossed: Option<TrackMarker> = p.active_marker_crossed();
                    if crossed.is_some() && written > 0 {
                        // Playback crossed exactly on a track start.
                        let left = p.sink.samples[before] >> 16;
                        prop_assert_eq!(left % i32::from(TRACK_STRIDE), 0, "crossed mid-track");
                    }
                }
                Op::Rewind(frames) => {
                    let before = p.track_marker().copied();
                    let rewound = p.rewind_output_frames(frames);
                    prop_assert!(rewound <= frames);
                    if let Some(before) = before {
                        let after = p.track_marker().copied().unwrap();
                        prop_assert_eq!(after.bytes_ahead, before.bytes_ahead + rewound as u64 * 8);
                    }
                }
                Op::Skip(frames) => {
                    p.skip_output_frames(frames);
                }
                Op::FlushStreaming => {
                    let had = p.track_marker().is_some();
                    prop_assert_eq!(p.flush_streaming(), had);
                    p.discard_decoder_backlog();
                    p.stream_buffer.flush();
                    stream_open = false;
                    prop_assert!(p.track_marker().is_none());
                }
            }
            check_marker(&p, track)?;
        }
    }
}
