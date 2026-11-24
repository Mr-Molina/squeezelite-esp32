//! `squeezelite-decoders`
//!
//! Pure-Rust audio decoding pipeline for squeezelite-esp32:
//! - Defines unified `AudioDecoder` trait
//! - Zero-copy `PcmDecoder` with mono channel duplication and multi-depth PCM
//! - Heap-isolated `Mp3Decoder` using nanomp3-core (zero task stack explosion)
//! - Stateful streaming `FlacDecoder` using claxon
//! - ADTS/AAC decoder using syom with exact byte consumption
//! - Ogg/Vorbis decoder using lewton with header/audio packet differentiation

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;

#[cfg(all(feature = "alloc", not(feature = "std")))]
extern crate alloc;

mod decoders;

#[cfg(any(feature = "std", test))]
pub use decoders::FlacByteReader;
#[cfg(any(feature = "alloc", feature = "std"))]
pub use decoders::FlacDecoder;
pub use decoders::{
    has_complete_ogg_page, ogg_crc32, parse_esds_audio_specific_config, parse_xing_lame_header,
    synthesize_adts_header, AacAudioSpecificConfig, AudioDecoder, DecodeError, DecoderError,
    PcmDecoder, StaticMp3Decoder, XingLameInfo, AAC_SAMPLE_RATES, MAX_FLAC_BLOCK_SAMPLES,
    MAX_MP4_META_BUF, MAX_MP4_SAMPLES, MAX_PACKET_QUEUE, MAX_SAMPLE_RATE, MAX_STREAM_BUF,
    MIN_SAMPLE_RATE, OGG_CRC32_TABLE,
};
#[cfg(feature = "alloc")]
pub use decoders::{
    prewarm_opus, warm_up_decoders, AacDecoder, AlacDecoder, Mp3Decoder, OpusDecoder, VorbisDecoder,
};
