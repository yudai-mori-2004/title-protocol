// SPDX-License-Identifier: Apache-2.0

//! Content decoder with unified format detection.
//!
//! Uses the `file-format` crate for format detection across all content types
//! (images, video, camera RAW, audio). Delegates decoding to format-specific
//! backends:
//!
//! - **Image** (JPEG, PNG, WebP, TIFF, etc.): `image` crate
//! - **Video** (MP4, WebM, MOV, AVI): ffmpeg CLI via `video.rs`
//! - **RAW image** (ARW, NEF, CR3, etc.): exiftool CLI via `raw.rs` → embedded JPEG → `image` crate
//! - **Audio** (MP3, WAV, FLAC, M4A): recognized but decoding not yet implemented
//!
//! DNG is TIFF-based and detected as TIFF by `file-format`. If the `image`
//! crate fails to decode it (unsupported TIFF variant), it falls back to
//! RAW preview extraction.

use std::io::Cursor;

use file_format::FileFormat;

/// Decoder kind, determined by format detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecoderKind {
    /// Standard image (JPEG, PNG, WebP, GIF, BMP, TIFF, AVIF, HEIC/HEIF).
    /// Decoded by the `image` crate.
    Image,
    /// Video (MP4, WebM, MOV, AVI).
    /// Metadata via ffprobe; frames extracted on demand via ffmpeg.
    Video,
    /// Camera RAW (ARW, NEF, CR3, DNG, RAF, ORF, RW2, etc.).
    /// Embedded JPEG preview extracted via exiftool, then decoded as JPEG.
    RawImage,
    /// Audio (MP3, WAV, FLAC, M4A, OGG, AAC).
    /// Recognized for C2PA manifest extraction, but pixel decoding is not
    /// applicable. Future: waveform/spectrogram extraction or audio
    /// fingerprinting (e.g., Chromaprint).
    Audio,
}

/// Decode result returned to the WASM host function.
pub struct DecodeResult {
    /// Content type that produced this result.
    pub kind: DecoderKind,
    /// Decoded pixel data (empty for Video/Audio — extracted on demand or N/A).
    pub data: Vec<u8>,
    /// Format-dependent metadata written to WASM linear memory.
    /// - Image/RawImage: `[width:u32 LE, height:u32 LE, channels:u32 LE]` (12 bytes)
    /// - Video: `[frame_count:u32, fps_x100:u32, width:u32, height:u32, duration_ms:u32]` (20 bytes)
    /// - Audio: `[sample_rate:u32, channels:u32, duration_ms:u32]` (12 bytes) — reserved, not yet populated
    pub metadata: Vec<u8>,
}

/// Detect content format and return the appropriate decoder kind.
///
/// Uses `file-format` crate for unified magic-byte detection. For TIFF-based
/// content (including DNG), first attempts `image` crate decoding; falls back
/// to RAW if it fails.
pub fn detect(content: &[u8]) -> Option<DecoderKind> {
    use FileFormat::*;

    let fmt = FileFormat::from_bytes(content);

    match fmt {
        // -----------------------------------------------------------------
        // Image — decoded by `image` crate
        // -----------------------------------------------------------------
        JointPhotographicExpertsGroup
        | PortableNetworkGraphics
        | GraphicsInterchangeFormat
        | Webp
        | WindowsBitmap
        | HighEfficiencyImageCoding
        | HighEfficiencyImageCodingSequence
        | HighEfficiencyImageFileFormat
        | HighEfficiencyImageFileFormatSequence => Some(DecoderKind::Image),

        // TIFF-based: could be plain TIFF, DNG, NEF, or other TIFF-variant RAW.
        // All TIFF-magic files go through RawImage path first (exiftool tries
        // to extract a full-size preview). If no preview is found, decode()
        // falls back to image crate as a standard TIFF.
        TagImageFileFormat => Some(DecoderKind::RawImage),

        // -----------------------------------------------------------------
        // Video — ffmpeg CLI
        // -----------------------------------------------------------------
        Mpeg4Part14
        | MatroskaVideo
        | AppleQuicktime
        | AudioVideoInterleave
        | FlashVideo => Some(DecoderKind::Video),

        // -----------------------------------------------------------------
        // Camera RAW — exiftool preview extraction
        // -----------------------------------------------------------------
        SonyAlphaRaw
        | NikonElectronicFile
        | CanonRaw              // CRW (older Canon RAW)
        | CanonRaw2             // CR2
        | CanonRaw3             // CR3
        | FujifilmRaw
        | OlympusRawFormat
        | PanasonicRaw => Some(DecoderKind::RawImage),

        // -----------------------------------------------------------------
        // Audio — recognized, decoding not yet implemented
        // -----------------------------------------------------------------
        Mpeg12AudioLayer3           // MP3 (without ID3 tag)
        | Id3v2                     // MP3 with ID3v2 metadata tag
        | WaveformAudio             // WAV
        | FreeLosslessAudioCodec    // FLAC
        | AppleItunesAudio          // M4A
        | AdvancedAudioCoding       // AAC
        | OggVorbis                 // OGG Vorbis
        | OggOpus                   // OGG Opus
        | OggFlac                   // OGG FLAC
        | AudioInterchangeFileFormat // AIFF
        | MatroskaAudio             // MKA
        => Some(DecoderKind::Audio),

        // -----------------------------------------------------------------
        // Fallback: check for ISO BMFF image formats (AVIF, HEIC) that
        // file-format may not recognize by variant name.
        // -----------------------------------------------------------------
        _ => {
            if is_iso_bmff_image(content) {
                Some(DecoderKind::Image)
            } else {
                None
            }
        }
    }
}

/// Estimate peak memory during image decode (header-only read).
///
/// Returns the estimated peak memory that `image::load_from_memory` will use
/// including intermediate decompression buffers. Conservative estimate: 2x the
/// decoded pixel size (output + intermediate buffer).
/// Returns 0 for non-image kinds or if header read fails.
pub fn estimate_decode_peak(kind: DecoderKind, content: &[u8]) -> usize {
    match kind {
        DecoderKind::Image | DecoderKind::RawImage => {
            use std::io::Cursor;
            let reader = match image::ImageReader::new(Cursor::new(content)).with_guessed_format() {
                Ok(r) => r,
                Err(_) => return 0,
            };
            let (w, h) = match reader.into_dimensions() {
                Ok(dims) => dims,
                Err(_) => return 0,
            };
            // Conservative: 4 bytes per pixel (RGBA) × 2 (output + intermediate)
            (w as usize).saturating_mul(h as usize).saturating_mul(8)
        }
        _ => 0,
    }
}

/// Decode content according to its kind.
///
/// - Image: full pixel decode via `image` crate.
/// - Video: metadata only (frames extracted on demand via `video_frame_grayscale`).
/// - RawImage: extract embedded JPEG via exiftool, then decode as JPEG.
/// - Audio: not yet implemented (returns error code -7).
pub fn decode(kind: DecoderKind, content: &[u8]) -> Result<DecodeResult, i32> {
    match kind {
        DecoderKind::Image => image_decoder::decode(kind, content),
        DecoderKind::Video => {
            let meta = crate::video::probe(content).map_err(|_| -3i32)?;
            let mut metadata = Vec::with_capacity(20);
            metadata.extend_from_slice(&meta.frame_count.to_le_bytes());
            metadata.extend_from_slice(&((meta.fps * 100.0) as u32).to_le_bytes());
            metadata.extend_from_slice(&meta.width.to_le_bytes());
            metadata.extend_from_slice(&meta.height.to_le_bytes());
            metadata.extend_from_slice(&(meta.duration_ms as u32).to_le_bytes());
            Ok(DecodeResult { kind, data: vec![], metadata })
        }
        DecoderKind::RawImage => {
            // Try exiftool preview extraction first. If no preview is found
            // (e.g., plain TIFF files), fall back to image crate decoding.
            match crate::raw::extract_preview_jpeg(content) {
                Ok(jpeg) => image_decoder::decode(kind, &jpeg),
                Err(_) => image_decoder::decode(kind, content),
            }
        }
        DecoderKind::Audio => {
            // Audio decoding not yet implemented.
            // C2PA manifest extraction works without decoding (operates on raw bytes).
            // Future: ffmpeg CLI for PCM extraction, spectrogram, or Chromaprint.
            Err(-7)
        }
    }
}

/// Check if content is an ISO BMFF image (AVIF, HEIC) by ftyp box.
/// Handles formats that file-format may not recognize by variant name.
fn is_iso_bmff_image(content: &[u8]) -> bool {
    if content.len() < 12 {
        return false;
    }
    // ISO BMFF: ftyp box at offset 4
    if &content[4..8] != b"ftyp" {
        return false;
    }
    let brand = &content[8..12];
    matches!(
        brand,
        b"avif" | b"avis" | b"heic" | b"heix" | b"hevc" | b"mif1"
    )
}

// ---------------------------------------------------------------------------
// Image decoder (image crate)
// ---------------------------------------------------------------------------

mod image_decoder {
    use super::*;

    /// Decode image to pixels, applying EXIF orientation.
    pub fn decode(kind: DecoderKind, content: &[u8]) -> Result<DecodeResult, i32> {
        let img = image::load_from_memory(content).map_err(|_| -3i32)?;
        let img = apply_exif_orientation(img, content);
        let (width, height) = (img.width(), img.height());

        use image::DynamicImage;
        let (data, channels) = match img {
            DynamicImage::ImageLuma8(buf) => (buf.into_raw(), 1u32),
            DynamicImage::ImageRgb8(buf) => (buf.into_raw(), 3u32),
            DynamicImage::ImageRgba8(buf) => (buf.into_raw(), 4u32),
            other => (other.to_rgb8().into_raw(), 3u32),
        };

        let mut metadata = Vec::with_capacity(12);
        metadata.extend_from_slice(&width.to_le_bytes());
        metadata.extend_from_slice(&height.to_le_bytes());
        metadata.extend_from_slice(&channels.to_le_bytes());
        Ok(DecodeResult { kind, data, metadata })
    }

    fn apply_exif_orientation(
        img: image::DynamicImage,
        content: &[u8],
    ) -> image::DynamicImage {
        let orientation = (|| -> Option<u32> {
            let exif_reader = exif::Reader::new();
            let exif_data = exif_reader
                .read_from_container(&mut Cursor::new(content))
                .ok()?;
            let field = exif_data.get_field(exif::Tag::Orientation, exif::In::PRIMARY)?;
            field.value.get_uint(0)
        })();

        match orientation.unwrap_or(1) {
            2 => img.fliph(),
            3 => img.rotate180(),
            4 => img.flipv(),
            5 => img.fliph().rotate270(),
            6 => img.rotate90(),
            7 => img.fliph().rotate90(),
            8 => img.rotate270(),
            _ => img,
        }
    }

}
