//! DMAP binary metadata (`application/x-dmap-tagged`), cover artwork, and
//! track progress (`text/parameters`) parsers for AirPlay 1 (RAOP).

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::fmt;

/// Maximum recursion depth for nested DMAP container tags (`mlit`, `mdcl`).
const MAX_DMAP_DEPTH: usize = 8;

/// Errors encountered when decoding binary DMAP (`application/x-dmap-tagged`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DmapError {
    /// A TLV header or payload extends past the end of the buffer.
    TruncatedPayload,
    /// Container nesting exceeded the safety recursion bound.
    RecursionLimitExceeded,
}

impl fmt::Display for DmapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedPayload => {
                write!(f, "DMAP TLV field extends beyond buffer boundary")
            }
            Self::RecursionLimitExceeded => {
                write!(f, "DMAP container nesting depth exceeded limit")
            }
        }
    }
}

/// Decoded DMAP track metadata from RTSP `SET_PARAMETER`.
///
/// Extracts standard DAAP/DMAP tags:
/// - `minm` (`dmap.itemname` / `daap.songname`) -> `title`
/// - `asar` (`daap.songartist`) -> `artist`
/// - `asal` (`daap.songalbum`) -> `album`
/// - `astm` (`daap.songtime`) -> `duration_ms`
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub struct DmapMetadata {
    /// Track title (`minm`).
    pub title: Option<String>,
    /// Track artist (`asar`).
    pub artist: Option<String>,
    /// Album name (`asal`).
    pub album: Option<String>,
    /// Track duration in milliseconds (`astm`).
    pub duration_ms: Option<u32>,
}

impl DmapMetadata {
    /// Creates an empty `DmapMetadata` struct.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the track title (`minm`).
    #[must_use]
    pub fn with_title(mut self, title: &str) -> Self {
        self.title = Some(title.to_string());
        self
    }

    /// Sets the track artist (`asar`).
    #[must_use]
    pub fn with_artist(mut self, artist: &str) -> Self {
        self.artist = Some(artist.to_string());
        self
    }

    /// Sets the album name (`asal`).
    #[must_use]
    pub fn with_album(mut self, album: &str) -> Self {
        self.album = Some(album.to_string());
        self
    }

    /// Sets the track duration in milliseconds (`astm`).
    #[must_use]
    pub fn with_duration_ms(mut self, duration_ms: u32) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }

    /// Returns `true` if no metadata fields were populated.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.artist.is_none()
            && self.album.is_none()
            && self.duration_ms.is_none()
    }

    /// Parses binary DMAP (`application/x-dmap-tagged`) TLV trees.
    ///
    /// Traverses container tags (`mlit`, `mdcl`, `mlcl`, `mcon`, `cmst`,
    /// `adbs`, `apso`) and extracts leaf values (`minm`, `asar`, `asal`,
    /// `astm`).
    pub fn parse(buf: &[u8]) -> Result<Self, DmapError> {
        let mut meta = Self::default();
        Self::parse_slice(buf, &mut meta, 0)?;
        Ok(meta)
    }

    /// Encodes this metadata into a standard `mlit`-wrapped DMAP binary blob.
    #[must_use]
    pub fn to_dmap_bytes(&self) -> Vec<u8> {
        let mut inner = Vec::new();
        if let Some(ref title) = self.title {
            Self::append_tlv(&mut inner, b"minm", title.as_bytes());
        }
        if let Some(ref artist) = self.artist {
            Self::append_tlv(&mut inner, b"asar", artist.as_bytes());
        }
        if let Some(ref album) = self.album {
            Self::append_tlv(&mut inner, b"asal", album.as_bytes());
        }
        if let Some(ms) = self.duration_ms {
            Self::append_tlv(&mut inner, b"astm", &ms.to_be_bytes());
        }
        let mut out = Vec::with_capacity(8 + inner.len());
        Self::append_tlv(&mut out, b"mlit", &inner);
        out
    }

    fn append_tlv(out: &mut Vec<u8>, tag: &[u8; 4], payload: &[u8]) {
        out.extend_from_slice(tag);
        let len = payload.len() as u32;
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(payload);
    }

    fn is_container_tag(tag: &[u8; 4]) -> bool {
        matches!(
            tag,
            b"mlit" | b"mdcl" | b"mlcl" | b"mcon" | b"cmst" | b"adbs" | b"apso"
        )
    }

    fn parse_uint_be(val: &[u8]) -> Option<u32> {
        match val.len() {
            1 => Some(u32::from(val[0])),
            2 => Some(u32::from(u16::from_be_bytes([val[0], val[1]]))),
            4 => Some(u32::from_be_bytes([val[0], val[1], val[2], val[3]])),
            8 => {
                let hi = u64::from_be_bytes([
                    val[0], val[1], val[2], val[3], val[4], val[5], val[6], val[7],
                ]);
                Some(hi as u32)
            }
            _ => None,
        }
    }

    fn parse_slice(buf: &[u8], meta: &mut Self, depth: usize) -> Result<(), DmapError> {
        if depth > MAX_DMAP_DEPTH {
            return Err(DmapError::RecursionLimitExceeded);
        }
        let mut offset = 0usize;
        while offset < buf.len() {
            if buf.len() - offset < 8 {
                return Err(DmapError::TruncatedPayload);
            }
            let tag = [
                buf[offset],
                buf[offset + 1],
                buf[offset + 2],
                buf[offset + 3],
            ];
            let field_len = u32::from_be_bytes([
                buf[offset + 4],
                buf[offset + 5],
                buf[offset + 6],
                buf[offset + 7],
            ]) as usize;
            offset += 8;

            if field_len > buf.len() - offset {
                return Err(DmapError::TruncatedPayload);
            }
            let value = &buf[offset..offset + field_len];
            offset += field_len;

            if Self::is_container_tag(&tag) {
                Self::parse_slice(value, meta, depth + 1)?;
            } else if &tag == b"minm" || &tag == b"cann" {
                let capped = &value[..value.len().min(512)];
                if let Ok(s) = core::str::from_utf8(capped) {
                    meta.title = Some(s.to_string());
                }
            } else if &tag == b"asar" || &tag == b"cana" {
                let capped = &value[..value.len().min(512)];
                if let Ok(s) = core::str::from_utf8(capped) {
                    meta.artist = Some(s.to_string());
                }
            } else if &tag == b"asal" || &tag == b"canl" {
                let capped = &value[..value.len().min(512)];
                if let Ok(s) = core::str::from_utf8(capped) {
                    meta.album = Some(s.to_string());
                }
            } else if &tag == b"astm" || &tag == b"cast" {
                if let Some(ms) = Self::parse_uint_be(value) {
                    meta.duration_ms = Some(ms);
                }
            }
        }
        Ok(())
    }
}

/// Supported cover artwork image formats delivered over RTSP `SET_PARAMETER`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ArtworkMimeType {
    /// `image/jpeg` payload.
    Jpeg,
    /// `image/png` payload.
    Png,
}

impl ArtworkMimeType {
    /// Parses a `Content-Type` header string into an `ArtworkMimeType`.
    #[must_use]
    pub fn from_content_type(content_type: &str) -> Option<Self> {
        let trimmed = content_type.trim();
        let media_type = trimmed.split(';').next().unwrap_or(trimmed).trim();
        if media_type.eq_ignore_ascii_case("image/jpeg")
            || media_type.eq_ignore_ascii_case("image/jpg")
        {
            Some(Self::Jpeg)
        } else if media_type.eq_ignore_ascii_case("image/png") {
            Some(Self::Png)
        } else {
            None
        }
    }

    /// Returns the canonical MIME type string.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
        }
    }
}

/// Cover artwork payload extracted from RTSP `SET_PARAMETER`.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct CoverArtwork {
    /// Image MIME format (`image/jpeg` or `image/png`).
    pub mime_type: ArtworkMimeType,
    /// Raw encoded image bytes.
    pub data: Vec<u8>,
    /// Associated RTP timestamp from `RTP-Info` header, if provided.
    pub rtp_timestamp: Option<u32>,
}

impl CoverArtwork {
    /// Creates a new `CoverArtwork` container.
    #[must_use]
    pub fn new(mime_type: ArtworkMimeType, data: Vec<u8>, rtp_timestamp: Option<u32>) -> Self {
        Self {
            mime_type,
            data,
            rtp_timestamp,
        }
    }
}

/// Track playback progress parsed from `text/parameters`
/// (`progress: <start>/<curr>/<end>`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct TrackProgress {
    /// RTP timestamp corresponding to the beginning of the track.
    pub rtp_start: u32,
    /// RTP timestamp corresponding to current playback position.
    pub rtp_current: u32,
    /// RTP timestamp corresponding to the end of the track.
    pub rtp_end: u32,
}

impl TrackProgress {
    /// Creates a new `TrackProgress` descriptor.
    #[must_use]
    pub fn new(rtp_start: u32, rtp_current: u32, rtp_end: u32) -> Self {
        Self {
            rtp_start,
            rtp_current,
            rtp_end,
        }
    }

    /// Parses a `progress: start/curr/end` parameter line or multi-line body.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        for line in text.lines() {
            let trimmed = line.trim();
            let val_part = if let Some(colon) = trimmed.find(':') {
                let key = trimmed[..colon].trim();
                if !key.eq_ignore_ascii_case("progress") {
                    continue;
                }
                trimmed[colon + 1..].trim()
            } else {
                trimmed
            };

            let mut parts = val_part.split('/');
            let (Some(start_s), Some(curr_s), Some(end_s)) =
                (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            if parts.next().is_some() {
                continue;
            }
            let Ok(rtp_start) = start_s.trim().parse::<u32>() else {
                continue;
            };
            let Ok(rtp_current) = curr_s.trim().parse::<u32>() else {
                continue;
            };
            let Ok(rtp_end) = end_s.trim().parse::<u32>() else {
                continue;
            };
            return Some(Self {
                rtp_start,
                rtp_current,
                rtp_end,
            });
        }
        None
    }

    /// Computes elapsed playback time in milliseconds at the given sample rate.
    #[must_use]
    pub fn elapsed_ms(&self, sample_rate_hz: u32) -> u32 {
        let rate = u64::from(sample_rate_hz.max(1));
        let raw_diff = self.rtp_current.wrapping_sub(self.rtp_start);
        let delta = if raw_diff > 0x8000_0000 {
            0
        } else {
            u64::from(raw_diff)
        };
        ((delta * 1000) / rate) as u32
    }

    /// Computes total track duration in milliseconds at the given sample rate.
    #[must_use]
    pub fn duration_ms(&self, sample_rate_hz: u32) -> u32 {
        let rate = u64::from(sample_rate_hz.max(1));
        let delta = u64::from(self.rtp_end.wrapping_sub(self.rtp_start));
        ((delta * 1000) / rate) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_track_progress_multi_line_with_leading_headers() {
        let text = concat!(
            "volume: -15.000000\r\n",
            "client-instance: 0123456789ABCDEF\r\n",
            "progress: 1000/5000/10000\r\n"
        );
        let progress = TrackProgress::parse(text).expect("progress parse succeeds");
        assert_eq!(progress.rtp_start, 1000);
        assert_eq!(progress.rtp_current, 5000);
        assert_eq!(progress.rtp_end, 10000);
    }

    #[test]
    fn test_track_progress_elapsed_ms_negative_jitter_clamped() {
        // Current is before start due to jitter: wrapping_sub produces
        // a value > 0x8000_0000.
        let progress = TrackProgress::new(10_000, 9_900, 20_000);
        assert_eq!(progress.elapsed_ms(44_100), 0);
    }

    #[test]
    fn test_dmap_metadata_cap_512_bytes() {
        let mut buf = Vec::new();
        let long_title = [b'A'; 600];
        DmapMetadata::append_tlv(&mut buf, b"minm", &long_title);
        let mut wrapper = Vec::new();
        DmapMetadata::append_tlv(&mut wrapper, b"mlit", &buf);

        let parsed = DmapMetadata::parse(&wrapper).expect("metadata parses");
        let title = parsed.title.expect("title present");
        assert_eq!(title.len(), 512);
    }
}
