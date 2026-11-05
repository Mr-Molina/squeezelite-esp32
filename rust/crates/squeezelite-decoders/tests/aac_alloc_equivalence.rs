//! WO-199: AAC (`syom`) host tests shared by the `std` and `no_std + alloc`
//! builds of the vendored decoder.
//!
//! Run both:
//! - `cargo test -p squeezelite-decoders --test aac_alloc_equivalence`
//!   (syom with `std`: platform libm, runtime x86 SIMD probe)
//! - `cargo test -p squeezelite-decoders --no-default-features \`
//!   `--features alloc --test aac_alloc_equivalence`
//!   (syom `no_std`: `libm` crate, compile-time SIMD selection)
//!
//! Both assert the same golden PCM digest, so passing both proves the two
//! builds emit bit-identical PCM for the fixture.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Barrier;

use squeezelite_decoders::{AacDecoder, AudioDecoder, DecoderError};

const FIXTURE: &[u8] = include_bytes!("fixtures/sine_440_1000_stereo_44k1_lc.aac");

/// FNV-1a 64 over the little-endian bytes of every decoded i32 sample.
const GOLDEN_PCM_FNV1A64: u64 = 0xec34_c84f_47bb_d5ed;
const GOLDEN_FRAMES: usize = 23552;

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
    let mut dec = AacDecoder::new();
    let mut out = [0i32; 1024];
    let mut pcm = Vec::new();
    let mut pos = 0usize;
    let mut stalls = 0usize;
    while stalls < 8 {
        let end = core::cmp::min(pos + chunk, FIXTURE.len());
        match dec.decode(&FIXTURE[pos..end], &mut out) {
            Ok((consumed, frames)) => {
                pos += consumed;
                pcm.extend_from_slice(&out[..frames * 2]);
                stalls = if consumed == 0 && frames == 0 {
                    stalls + 1
                } else {
                    0
                };
            }
            Err(DecoderError::NeedMoreData) | Err(DecoderError::CorruptedFrame) => {
                stalls += 1;
            }
            Err(e) => panic!("unexpected decoder error: {e:?}"),
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
    // Documents which syom flavor this test binary linked.
    let expect_std = cfg!(feature = "std");
    assert_eq!(syom::STD_ENABLED, expect_std);
}

#[test]
fn filterbank_and_stream_decoder_sizes_are_compact() {
    let (dec_size, fb_size) = syom::stream_decoder_and_filterbank_sizes();
    let aac_size = core::mem::size_of::<AacDecoder>();
    println!(
        "StreamDecoder size={dec_size}, Filterbank size={fb_size}, AacDecoder size={aac_size}"
    );
    assert!(fb_size < 64, "Filterbank size {fb_size} >= 64");
    assert!(dec_size < 1024, "StreamDecoder size {dec_size} >= 1024");
}

#[test]
fn test_stereo_15s_44k1_decodes_cleanly() {
    if let Ok(data) = std::fs::read("/tmp/test_stereo_15s_44k1.aac") {
        let mut dec = AacDecoder::new();
        let mut out = [0i32; 1024];
        let mut pos = 0;
        let mut total_frames = 0;
        while pos < data.len() {
            let chunk = core::cmp::min(2048, data.len() - pos);
            match dec.decode(&data[pos..pos + chunk], &mut out) {
                Ok((consumed, frames)) => {
                    pos += consumed;
                    total_frames += frames;
                    if consumed == 0 && frames == 0 {
                        break;
                    }
                }
                Err(DecoderError::NeedMoreData) => {
                    break;
                }
                Err(e) => {
                    panic!("decode error at pos {pos}: {e:?}");
                }
            }
        }
        println!(
            "Total decoded frames: {total_frames}, pos: {pos}/{}",
            data.len()
        );
        assert!(total_frames > 0);
    }
}

#[test]
fn adts_lc_fixture_decodes_to_golden_pcm() {
    let d = decode_fixture(2048);
    assert_eq!(d.sample_rate, 44_100);
    assert_eq!(d.channels, 2);
    assert_eq!(d.pcm.len() % 2, 0);
    let frames = d.pcm.len() / 2;
    let digest = fnv1a64(&d.pcm);
    println!(
        "std={} frames={frames} fnv1a64={digest:#018x}",
        syom::STD_ENABLED
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

static INITS: AtomicUsize = AtomicUsize::new(0);
static DROPS: AtomicUsize = AtomicUsize::new(0);

struct Table([u32; 64]);

impl Drop for Table {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

fn build_table() -> Table {
    INITS.fetch_add(1, Ordering::SeqCst);
    let mut t = [0u32; 64];
    for (i, v) in t.iter_mut().enumerate() {
        *v = (i as u32).wrapping_mul(2_654_435_761).rotate_left(7);
    }
    Table(t)
}

/// WO-199 condition 2: two racing initializers observe one deterministic
/// value, and every losing allocation is dropped (freed).
#[test]
fn lazybox_two_thread_race_is_deterministic_and_frees_loser() {
    let expected = build_table().0;
    let before_inits = INITS.load(Ordering::SeqCst);
    let before_drops = DROPS.load(Ordering::SeqCst);
    const ROUNDS: usize = 200;
    for _ in 0..ROUNDS {
        let cell = syom::lazy::LazyBox::new(build_table);
        let barrier = Barrier::new(2);
        let (a, b) = std::thread::scope(|s| {
            let ha = s.spawn(|| {
                barrier.wait();
                cell.0
            });
            let hb = s.spawn(|| {
                barrier.wait();
                cell.0
            });
            (ha.join().unwrap_or([0; 64]), hb.join().unwrap_or([1; 64]))
        });
        assert_eq!(a, b);
        assert_eq!(a, expected);
        drop(cell);
    }
    let inits = INITS.load(Ordering::SeqCst) - before_inits;
    let drops = DROPS.load(Ordering::SeqCst) - before_drops;
    // One winner per round is dropped with its cell; every loser is dropped
    // inside OnceBox. No leak means drops == inits.
    assert!(inits >= ROUNDS);
    assert_eq!(drops, inits, "inits={inits} drops={drops}");
}
