//! WO-207: Opus (`opus-rs 0.1.40`) host tests shared by `std` and
//! `no_std + alloc` builds of the vendored decoder.
//!
//! Run both:
//! - `cargo test -p squeezelite-decoders --test opus_alloc_equivalence`
//! - `cargo test -p squeezelite-decoders --no-default-features \`
//!   `--features alloc --test opus_alloc_equivalence`

use squeezelite_decoders::AudioDecoder;
use squeezelite_decoders::DecoderError;
use squeezelite_decoders::OpusDecoder;

const FIXTURE: &[u8] = include_bytes!("fixtures/test_stereo_05s_48k.opus");

/// FNV-1a 64 over the little-endian bytes of every decoded i32 sample.
const GOLDEN_PCM_FNV1A64: u64 = 0x626fe7b25b0baae2;
const GOLDEN_FRAMES: usize = 24648;

struct Decoded {
    pcm: Vec<i32>,
    sample_rate: u32,
    channels: u32,
}

fn fnv1a64(samples: &[i32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for s in samples {
        for b in s.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// Feed the fixture the way the firmware does: bounded input windows,
/// honoring the consumed byte count, then drain pending PCM.
fn decode_fixture(chunk: usize) -> Decoded {
    let mut dec = OpusDecoder::new();
    let mut out = [0i32; 2048];
    let mut pcm = Vec::new();
    let mut pos = 0usize;
    let mut stalls = 0usize;
    while stalls < 16 {
        let end = core::cmp::min(pos + chunk, FIXTURE.len());
        let slice = if pos < FIXTURE.len() {
            &FIXTURE[pos..end]
        } else {
            &[]
        };

        match dec.decode(slice, &mut out) {
            Ok((consumed, frames)) => {
                pos += consumed;
                pcm.extend_from_slice(&out[..frames * 2]);
                if consumed == 0 && frames == 0 {
                    if pos >= FIXTURE.len() && !dec.has_pending_pcm() {
                        break;
                    }
                    stalls += 1;
                } else {
                    stalls = 0;
                }
            }
            Err(DecoderError::NeedMoreData) => {
                if pos >= FIXTURE.len() && !dec.has_pending_pcm() {
                    break;
                }
                stalls += 1;
            }
            Err(e) => panic!("unexpected decoder error at pos {pos}: {e:?}"),
        }
    }
    Decoded {
        pcm,
        sample_rate: dec.sample_rate,
        channels: dec.channels,
    }
}

#[test]
fn reports_build_flavor() {
    let expect_std = cfg!(feature = "std");
    println!("Running opus_alloc_equivalence with std={expect_std}");
}

#[test]
fn opus_fixture_decodes_successfully() {
    let d = decode_fixture(2048);
    assert_eq!(d.sample_rate, 48_000);
    assert_eq!(d.channels, 2);
    assert_eq!(d.pcm.len() % 2, 0);
    let frames = d.pcm.len() / 2;
    let digest = fnv1a64(&d.pcm);
    println!(
        "std={} frames={frames} fnv1a64={digest:#018x}",
        cfg!(feature = "std")
    );
    assert_eq!(frames, GOLDEN_FRAMES);
    assert_eq!(digest, GOLDEN_PCM_FNV1A64);
}

#[test]
fn chunk_sizes_yield_identical_pcm() {
    let baseline = decode_fixture(2048);
    for chunk in [256, 512, 1024, 4096] {
        let trial = decode_fixture(chunk);
        assert_eq!(
            trial.pcm.len(),
            baseline.pcm.len(),
            "chunk {chunk} length mismatch"
        );
        assert_eq!(
            fnv1a64(&trial.pcm),
            fnv1a64(&baseline.pcm),
            "chunk {chunk} PCM mismatch"
        );
    }
}

#[test]
fn decoder_flush_resets_pending_state() {
    let mut dec = OpusDecoder::new();
    let mut out = [0i32; 512];
    let _ = dec.decode(&FIXTURE[..core::cmp::min(2048, FIXTURE.len())], &mut out);
    dec.flush();
    assert!(!dec.has_pending_pcm());
}

#[test]
fn opus_decoder_struct_size_guard() {
    let sz = core::mem::size_of::<OpusDecoder>();
    println!("OpusDecoder struct size={sz} bytes");
    println!(
        "opus_rs::OpusDecoder: {}",
        core::mem::size_of::<opus_rs::OpusDecoder>()
    );
    println!(
        "CeltDecoder: {}",
        core::mem::size_of::<opus_rs::celt::CeltDecoder>()
    );
    println!(
        "SilkDecoder: {}",
        core::mem::size_of::<opus_rs::silk::dec_api::SilkDecoder>()
    );
    // Hard budget: must be well under 1024 B to prevent task stack overflows
    assert!(sz < 1024, "OpusDecoder size {sz} >= 1024 bytes");
}

#[test]
fn test_15s_fixture_decodes() {
    let data = include_bytes!("fixtures/test_stereo_15s_48k.opus");
    let mut dec = OpusDecoder::new();
    let mut out = [0i32; 11520];
    let mut offset = 0;
    let mut total_frames = 0;
    while offset < data.len() {
        let chunk = 2048.min(data.len() - offset);
        let slice = &data[offset..offset + chunk];
        match dec.decode(slice, &mut out) {
            Ok((consumed, frames)) => {
                offset += consumed;
                total_frames += frames;
            }
            Err(e) => panic!("Decode error at offset {offset}: {e:?}"),
        }
    }
    while dec.has_pending_pcm() {
        match dec.decode(&[], &mut out) {
            Ok((_, frames)) => {
                if frames == 0 {
                    break;
                }
                total_frames += frames;
            }
            Err(e) => panic!("Drain error: {e:?}"),
        }
    }
    println!("15s fixture total frames: {total_frames}");
    assert_eq!(total_frames, 720648);
}
