// SPDX-License-Identifier: Apache-2.0

//! コンテンツデコーダ（統一フォーマット判定）。
//!
//! 仕様書 §7.1 — `file-format` クレートによるマジックバイト判定で
//! 全コンテンツ種別（画像・動画・カメラRAW・音声）を自動識別し、
//! 種別ごとのバックエンドにデコードを委譲する:
//!
//! - **画像** (JPEG, PNG, WebP, TIFF等): `image` クレート
//! - **動画** (MP4, WebM, MOV, AVI): ffmpeg CLI (`video.rs`)
//! - **カメラRAW** (ARW, NEF, CR3等): exiftool CLI (`raw.rs`) → 埋め込みJPEG → `image` クレート
//! - **音声** (MP3, WAV, FLAC, M4A): 認識のみ（デコード未実装）
//!
//! DNGはTIFFベースであり `file-format` はTIFFとして検出する。
//! `image` クレートがデコードに失敗した場合（非対応TIFFバリアント）、
//! RAWプレビュー抽出にフォールバックする。

use std::io::Cursor;

use file_format::FileFormat;

/// フォーマット判定結果に基づくデコーダ種別。
/// 仕様書 §7.1
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecoderKind {
    /// 標準画像 (JPEG, PNG, WebP, GIF, BMP, TIFF, AVIF, HEIC/HEIF)。
    /// `image` クレートでデコード。
    Image,
    /// 動画 (MP4, WebM, MOV, AVI)。
    /// ffprobeでメタデータ取得、ffmpegでフレームをオンデマンド抽出。
    Video,
    /// カメラRAW (ARW, NEF, CR3, DNG, RAF, ORF, RW2等)。
    /// exiftoolで埋め込みJPEGプレビューを抽出し、JPEGとしてデコード。
    RawImage,
    /// 音声 (MP3, WAV, FLAC, M4A, OGG, AAC)。
    /// C2PAマニフェスト抽出用に認識するが、ピクセルデコードは不要。
    Audio,
}

/// WASMホスト関数に返すデコード結果。
/// 仕様書 §7.1
pub struct DecodeResult {
    /// この結果を生成したコンテンツ種別。
    pub kind: DecoderKind,
    /// デコード済みピクセルデータ（Video/Audioは空 — オンデマンド抽出または不要）。
    pub data: Vec<u8>,
    /// フォーマット依存のメタデータ（WASMリニアメモリに書き込まれる）。
    /// - Image/RawImage: `[width:u32 LE, height:u32 LE, channels:u32 LE]` (12バイト)
    /// - Video: `[frame_count:u32, fps_x100:u32, width:u32, height:u32, duration_ms:u32]` (20バイト)
    /// - Audio: `[sample_rate:u32, channels:u32, duration_ms:u32]` (12バイト) — 予約、未実装
    pub metadata: Vec<u8>,
}

/// コンテンツフォーマットを判定し、適切なデコーダ種別を返す。
///
/// 仕様書 §7.1 — `file-format` クレートによるマジックバイト判定。
/// TIFFベースのコンテンツ（DNG含む）はRawImageパスを優先し、
/// プレビューが見つからない場合はimageクレートにフォールバックする。
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

/// 画像デコード時のピークメモリを推定する（ヘッダのみ読み取り）。
///
/// 仕様書 §7.1 — `image::load_from_memory` が使用するピークメモリの推定値。
/// 中間展開バッファを含む保守的推定: デコード後ピクセルサイズの2倍
/// （出力バッファ + 中間バッファ）。
/// 画像以外の種別またはヘッダ読み取り失敗時は0を返す。
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

/// 種別に応じてコンテンツをデコードする。
///
/// 仕様書 §7.1
/// - Image: `image` クレートでフルピクセルデコード。
/// - Video: メタデータのみ（フレームは `video_frame_grayscale` でオンデマンド抽出）。
/// - RawImage: exiftoolで埋め込みJPEGを抽出後、JPEGとしてデコード。
/// - Audio: 未実装（エラーコード -7）。
pub fn decode(kind: DecoderKind, content: &[u8]) -> Result<DecodeResult, i32> {
    match kind {
        DecoderKind::Image => image_decoder::decode(kind, content),
        DecoderKind::Video => {
            let meta = crate::video::probe(content).map_err(|_| -3i32)?;
            // Metadata (WASM向け 24バイト):
            //   [0-3]  frame_count, [4-7] fps_x100, [8-11] width,
            //   [12-15] height, [16-19] duration_ms, [20-23] keyframe_count
            let mut metadata = Vec::with_capacity(24);
            metadata.extend_from_slice(&meta.frame_count.to_le_bytes());
            metadata.extend_from_slice(&((meta.fps * 100.0) as u32).to_le_bytes());
            metadata.extend_from_slice(&meta.width.to_le_bytes());
            metadata.extend_from_slice(&meta.height.to_le_bytes());
            metadata.extend_from_slice(&(meta.duration_ms as u32).to_le_bytes());
            metadata.extend_from_slice(&(meta.keyframe_pts.len() as u32).to_le_bytes());
            // Data (Rust内部用): キーフレームPTS を f64 LE でパック
            let mut data = Vec::with_capacity(meta.keyframe_pts.len() * 8);
            for &pts in &meta.keyframe_pts {
                data.extend_from_slice(&pts.to_le_bytes());
            }
            Ok(DecodeResult { kind, data, metadata })
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

/// ISO BMFFイメージ（AVIF, HEIC）をftypボックスで判定する。
/// `file-format` がバリアント名で認識できない形式に対応。
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

    /// 画像をピクセルにデコードし、EXIF回転を適用する。
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
