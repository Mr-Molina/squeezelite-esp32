//! Task 17 integration & quality tests for [`Resampler`].
//!
//! Verifies IEEE-1241 least-squares single-tone SNR for upsampling
//! (`44.1 kHz -> 48 kHz`), anti-aliased downsampling (`96 kHz -> 44.1 kHz`),
//! `ResampleQuality::Basic` floor (`>= 55 dB`), and exact rational frame
//! count preservation over a 10-second continuous stream.

use squeezelite_audio_core::{InterpolationMode, ResampleQuality, Resampler};

/// Computes the single-tone Signal-to-Noise-and-Distortion Ratio (SNR in dB)
/// at `tone_hz` for a resampled signal from `from_hz` to `to_hz`.
fn measure_stereo_sine_snr_db(
    from_hz: u32,
    to_hz: u32,
    tone_hz: f64,
    quality: ResampleQuality,
    interp: InterpolationMode,
) -> (f64, f64) {
    let mut resampler = Resampler::new(from_hz, to_hz, quality, interp);
    let in_frames = (from_hz as usize) / 5; // 200 ms of input
    let amplitude = 1_073_741_824.0_f64; // 2^30 (~0.5 full-scale i32)

    let mut input = Vec::with_capacity(in_frames * 2);
    for n in 0..in_frames {
        let theta = core::f64::consts::TAU * tone_hz * (n as f64) / f64::from(from_hz);
        let l = (amplitude * theta.sin()).round() as i32;
        let r = (amplitude * theta.cos()).round() as i32;
        input.push(l);
        input.push(r);
    }

    let max_out_frames = ((in_frames as u64 * u64::from(to_hz)) / u64::from(from_hz)) as usize + 64;
    let mut output = vec![0_i32; max_out_frames * 2];
    let (consumed, produced) = resampler.process(&input, &mut output);
    assert_eq!(consumed, input.len(), "all input samples must be consumed");

    let produced_frames = produced / 2;
    let settle_frames = 128usize;
    let window_frames = produced_frames.saturating_sub(settle_frames * 2);
    // Align window_frames to an integer number of 1 kHz cycles when possible.
    let samples_per_10_cycles = ((u64::from(to_hz) * 10) / (tone_hz as u64)) as usize;
    let aligned_frames = (window_frames / samples_per_10_cycles) * samples_per_10_cycles;
    assert!(aligned_frames >= samples_per_10_cycles, "window too short");

    let eval_channel = |ch: usize| -> f64 {
        let mut sum_sin = 0.0_f64;
        let mut sum_cos = 0.0_f64;
        for m in 0..aligned_frames {
            let idx = (settle_frames + m) * 2 + ch;
            let y = f64::from(output[idx]);
            let phi = core::f64::consts::TAU * tone_hz * (m as f64) / f64::from(to_hz);
            sum_sin += y * phi.sin();
            sum_cos += y * phi.cos();
        }
        let n_f = aligned_frames as f64;
        let c_sin = (2.0 / n_f) * sum_sin;
        let c_cos = (2.0 / n_f) * sum_cos;
        let sig_power = 0.5 * (c_sin * c_sin + c_cos * c_cos);

        let mut noise_energy = 0.0_f64;
        for m in 0..aligned_frames {
            let idx = (settle_frames + m) * 2 + ch;
            let y = f64::from(output[idx]);
            let phi = core::f64::consts::TAU * tone_hz * (m as f64) / f64::from(to_hz);
            let fitted = c_sin * phi.sin() + c_cos * phi.cos();
            let err = y - fitted;
            noise_energy += err * err;
        }
        let noise_power = (noise_energy / n_f).max(1e-12);
        10.0 * (sig_power / noise_power).log10()
    };

    (eval_channel(0), eval_channel(1))
}

#[test]
fn test_sine_snr_medium_44k1_to_48k() {
    let (snr_l, snr_r) = measure_stereo_sine_snr_db(
        44_100,
        48_000,
        1_000.0,
        ResampleQuality::Medium,
        InterpolationMode::LinearPhase,
    );
    assert!(
        snr_l >= 90.0 && snr_r >= 90.0,
        "expected Medium 44.1k->48k SNR >= 90 dB, got L={snr_l:.2} dB, R={snr_r:.2} dB"
    );
}

#[test]
fn test_sine_snr_downsample_96k_to_44k1() {
    let (snr_l, snr_r) = measure_stereo_sine_snr_db(
        96_000,
        44_100,
        1_000.0,
        ResampleQuality::Medium,
        InterpolationMode::LinearPhase,
    );
    assert!(
        snr_l >= 90.0 && snr_r >= 90.0,
        "expected Medium 96k->44.1k SNR >= 90 dB, got L={snr_l:.2} dB, R={snr_r:.2} dB"
    );
}

#[test]
fn test_basic_quality_floor() {
    let (snr_l, snr_r) = measure_stereo_sine_snr_db(
        44_100,
        48_000,
        1_000.0,
        ResampleQuality::Basic,
        InterpolationMode::QuantizedPhase,
    );
    assert!(
        snr_l >= 55.0 && snr_r >= 55.0,
        "expected Basic 44.1k->48k SNR >= 55 dB, got L={snr_l:.2} dB, R={snr_r:.2} dB"
    );
}

#[test]
fn test_length_ratio_exact_over_10s() {
    let from_hz = 44_100_u32;
    let to_hz = 48_000_u32;
    let mut resampler = Resampler::new(
        from_hz,
        to_hz,
        ResampleQuality::Basic,
        InterpolationMode::QuantizedPhase,
    );

    let total_in_frames = (from_hz as usize) * 10; // 441,000 stereo frames (10s)
    let expected_out_frames = (to_hz as usize) * 10; // 480,000 stereo frames (10s)

    let chunk_in = [0_i32; 1024 * 2];
    let mut chunk_out = [0_i32; 2048 * 2];

    let mut rem_in_frames = total_in_frames;
    let mut total_consumed_samples = 0usize;
    let mut total_produced_samples = 0usize;

    while rem_in_frames > 0 {
        let step_frames = rem_in_frames.min(1024);
        let (c, p) = resampler.process(&chunk_in[..step_frames * 2], &mut chunk_out);
        assert_eq!(c, step_frames * 2, "chunk must be fully consumed");
        total_consumed_samples += c;
        total_produced_samples += p;
        rem_in_frames -= step_frames;
    }

    let consumed_frames = total_consumed_samples / 2;
    let produced_frames = total_produced_samples / 2;
    assert_eq!(consumed_frames, total_in_frames);
    let diff = produced_frames.abs_diff(expected_out_frames);
    assert!(
        diff <= 1,
        "expected {expected_out_frames} +/- 1 output frames over 10s, got {produced_frames}"
    );
}
