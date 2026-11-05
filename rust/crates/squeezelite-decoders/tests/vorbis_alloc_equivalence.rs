//! WO-203: Ogg Vorbis (`lewton`) host tests shared by `std` and
//! `no_std + alloc` builds of the vendored decoder.
//!
//! Run both:
//! - `cargo test -p squeezelite-decoders --test vorbis_alloc_equivalence`
//!   (lewton with `std`: platform libm)
//! - `cargo test -p squeezelite-decoders --no-default-features \`
//!   `--features alloc --test vorbis_alloc_equivalence`
//!   (lewton `no_std`: `libm` crate)
//!
//! Both assert the same golden PCM digest, so passing both proves the two
//! builds emit bit-identical PCM for the fixture.

use squeezelite_decoders::AudioDecoder;
use squeezelite_decoders::DecoderError;
use squeezelite_decoders::VorbisDecoder;

const FIXTURE: &[u8] = include_bytes!("fixtures/sine_440_1000_stereo_44k1.ogg");

/// FNV-1a 64 over the little-endian bytes of every decoded i32 sample.
const GOLDEN_PCM_FNV1A64: u64 = 0xfe93_46f1_654b_ca6d;
const GOLDEN_FRAMES: usize = 22080;

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
    let mut dec = VorbisDecoder::new();
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
    println!("Running vorbis_alloc_equivalence with std={expect_std}");
}

#[test]
fn ogg_vorbis_fixture_decodes_to_golden_pcm() {
    let d = decode_fixture(2048);
    assert_eq!(d.sample_rate, 44_100);
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
fn chunking_does_not_change_pcm() {
    let a = decode_fixture(2048);
    let b = decode_fixture(777);
    assert_eq!(a.pcm.len(), b.pcm.len());
    assert!(a.pcm == b.pcm);
}

#[test]
fn decoded_signal_is_not_silent() {
    let d = decode_fixture(2048);
    let peak = d.pcm.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
    // A -inf..0 dBFS sine pair: peak must clear 1/16 full scale.
    assert!(peak > (i32::MAX as u32) / 16, "peak {peak}");
}

#[test]
fn decoder_struct_size_is_compact() {
    let sz = core::mem::size_of::<VorbisDecoder>();
    println!("VorbisDecoder struct size={sz} bytes");
    // Assert struct size is compact on stack (< 256 bytes) to protect
    // FreeRTOS task stacks.
    assert!(sz < 256, "VorbisDecoder size {sz} >= 256 bytes");
}
