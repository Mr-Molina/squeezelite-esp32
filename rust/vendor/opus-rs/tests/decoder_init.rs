use core::mem::MaybeUninit;
use opus_rs::{Application, OpusDecoder, OpusEncoder};

fn packets(sampling_rate: i32, channels: usize, application: Application) -> Vec<Vec<u8>> {
    let frame_size = sampling_rate as usize / 50;
    let mut encoder = OpusEncoder::new(sampling_rate, channels, application).unwrap();
    let mut packets = Vec::new();
    for frame in 0..50 {
        let input: Vec<f32> = (0..frame_size * channels)
            .map(|i| {
                let t = (frame * frame_size + i / channels) as f32 / sampling_rate as f32;
                0.3 * (2.0 * core::f32::consts::PI * (220.0 + 110.0 * (i % channels) as f32) * t)
                    .sin()
            })
            .collect();
        let mut output = vec![0u8; 1276];
        let length = encoder.encode(&input, frame_size, &mut output).unwrap();
        packets.push(output[..length].to_vec());
    }
    packets
}

#[test]
fn init_decodes_like_new() {
    for &(sampling_rate, channels, application) in &[
        (8000, 1, Application::Voip),
        (16000, 2, Application::Voip),
        (48000, 1, Application::Audio),
        (48000, 2, Application::Audio),
    ] {
        let frame_size = sampling_rate as usize / 50;
        let mut expected = OpusDecoder::new(sampling_rate, channels).unwrap();
        let mut slot = Box::new(MaybeUninit::<OpusDecoder>::uninit());
        let actual = OpusDecoder::init(&mut slot, sampling_rate, channels).unwrap();

        for packet in packets(sampling_rate, channels, application) {
            let mut a = vec![0.0f32; frame_size * channels];
            let mut b = vec![0.0f32; frame_size * channels];
            let n = expected.decode(&packet, frame_size, &mut a).unwrap();
            let m = actual.decode(&packet, frame_size, &mut b).unwrap();
            assert_eq!(n, m);
            assert_eq!(a, b);
        }
    }
}

#[test]
fn init_rejects_invalid_settings() {
    let mut slot = Box::new(MaybeUninit::<OpusDecoder>::uninit());
    assert!(OpusDecoder::init(&mut slot, 44100, 2).is_err());
    assert!(OpusDecoder::init(&mut slot, 48000, 3).is_err());
}
