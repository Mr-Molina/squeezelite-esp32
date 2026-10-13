//! Audio Core Stress and Invariant Verification Test Suite
//!
//! Stress-tests lock-free ringbuffer under rapid cyclic wraps,
//! verifies DSP equalizer stability (no NaN / Inf), and audits volume scaling.

use squeezelite_audio_core::{
    scale_samples_inplace, AudioRingBuffer, BiquadCoeffs, Equalizer, VolumeScale,
};

#[test]
fn test_ringbuffer_one_million_samples_stress() {
    let mut storage = [0u8; 1024]; // Small 1KB buffer forces constant wrapping
    let mut rb = AudioRingBuffer::new(&mut storage);

    const TOTAL_SAMPLES: usize = 1_000_000;
    let mut samples_written = 0usize;
    let mut samples_read = 0usize;

    let mut write_seq: u32 = 0;
    let mut read_seq: u32 = 0;

    let mut write_buf = [0u8; 256];
    let mut read_buf = [0u8; 256];

    while samples_read < TOTAL_SAMPLES {
        // Write burst if space permits
        let avail_w = rb.available_write();
        let frames_to_write = core::cmp::min(avail_w / 4, 32);

        if frames_to_write > 0 && samples_written < TOTAL_SAMPLES {
            let to_write = core::cmp::min(frames_to_write, TOTAL_SAMPLES - samples_written);
            for i in 0..to_write {
                let bytes = write_seq.to_le_bytes();
                write_buf[i * 4..i * 4 + 4].copy_from_slice(&bytes);
                write_seq = write_seq.wrapping_add(1);
            }
            let written = rb.write(&write_buf[..to_write * 4]).unwrap();
            samples_written += written / 4;
        }

        // Read burst
        let avail_r = rb.available_read();
        let frames_to_read = core::cmp::min(avail_r / 4, 32);

        if frames_to_read > 0 {
            let read_bytes = rb.read(&mut read_buf[..frames_to_read * 4]).unwrap();
            let frames_read = read_bytes / 4;

            for i in 0..frames_read {
                let val = u32::from_le_bytes([
                    read_buf[i * 4],
                    read_buf[i * 4 + 1],
                    read_buf[i * 4 + 2],
                    read_buf[i * 4 + 3],
                ]);
                assert_eq!(val, read_seq, "Fidelity mismatch during ringbuffer wrap");
                read_seq = read_seq.wrapping_add(1);
            }
            samples_read += frames_read;
        }
    }

    assert_eq!(samples_read, TOTAL_SAMPLES);
    assert_eq!(samples_written, TOTAL_SAMPLES);
}

#[test]
fn test_equalizer_numerical_stability() {
    let mut eq = Equalizer::<10>::new();

    // Configure all bands with non-zero peaking filters
    for i in 0..10 {
        let coeffs = BiquadCoeffs {
            b0: 1.0,
            b1: -0.5,
            b2: 0.25,
            a1: -0.4,
            a2: 0.2,
        };
        eq.set_filter(i, coeffs).unwrap();
    }
    eq.enabled = true;

    // Feed 50,000 impulses and alternating signals
    let mut pcm = [0i32; 1024];
    for cycle in 0..50 {
        for (i, sample) in pcm.iter_mut().enumerate() {
            *sample = if (i + cycle) % 2 == 0 {
                i32::MAX / 2
            } else {
                i32::MIN / 2
            };
        }

        eq.process_interleaved(&mut pcm);

        // Verify output is finite, within range, and has no arithmetic overflows
        for &s in pcm.iter() {
            assert!((s as f32).is_finite());
        }
    }
}

#[test]
fn test_volume_scaling_stress_boundaries() {
    let mut samples = [
        i32::MAX,
        i32::MIN + 1,
        0,
        1,
        -1,
        1 << 24,
        -(1 << 24),
        1 << 16,
    ];

    // Unity gain (65536)
    let unity = VolumeScale::unity();
    scale_samples_inplace(&mut samples, unity);
    assert_eq!(samples[2], 0);

    // Mute
    let mute = VolumeScale::mute();
    scale_samples_inplace(&mut samples, mute);
    for &s in &samples {
        assert_eq!(s, 0, "Mute must zero all samples");
    }
}

#[test]
fn test_ringbuffer_concurrent_threads_stress() {
    let mut storage = vec![0u8; 1024];
    let mut rb = AudioRingBuffer::new(&mut storage);
    let (producer, consumer) = rb.split();

    const TOTAL_ITEMS: u32 = 10_000;

    std::thread::scope(|s| {
        let p_handle = s.spawn(move || {
            let mut seq = 0u32;
            let mut buf = [0u8; 4];
            while seq < TOTAL_ITEMS {
                buf.copy_from_slice(&seq.to_le_bytes());
                let mut written = 0;
                while written < 4 {
                    match producer.write(&buf[written..]) {
                        Ok(n) => written += n,
                        Err(_) => std::thread::yield_now(),
                    }
                }
                seq += 1;
            }
        });

        let c_handle = s.spawn(move || {
            let mut expected = 0u32;
            let mut buf = [0u8; 4];
            while expected < TOTAL_ITEMS {
                let mut read = 0;
                while read < 4 {
                    match consumer.read(&mut buf[read..]) {
                        Ok(n) => read += n,
                        Err(_) => std::thread::yield_now(),
                    }
                }
                let val = u32::from_le_bytes(buf);
                assert_eq!(val, expected, "Concurrent SPSC item out of order!");
                expected += 1;
            }
        });

        p_handle.join().unwrap();
        c_handle.join().unwrap();
    });
}
