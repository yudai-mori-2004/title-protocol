// SPDX-License-Identifier: Apache-2.0

//! # コンテンツデコーダー
//!
//! コンテンツ種別（画像・音声・動画等）に応じたデコード処理を提供する。
//! 仕様書 §7.1
//!
//! ## 設計
//!
//! `decode_content` ホスト関数はコンテンツ種別に依存しない。
//! 種別ごとのロジック（ピーク推定・デコード・メタデータ形式）は
//! サブモジュール（`image_decoder` 等）に閉じ込める。
//!
//! メタデータ形式はデコーダーとWASMモジュール間の暗黙契約であり、
//! ホスト関数は不透明バイト列として中継するのみ。
//!
//! ## 拡張
//!
//! 新しいコンテンツ種別を追加するには:
//! 1. `DecoderKind` にバリアントを追加
//! 2. サブモジュール（例: `audio_decoder`）を実装
//! 3. `detect` / `estimate_peak_bytes` / `decode` に match arm を追加

use std::io::Cursor;

/// サポートするデコーダーの種別。
/// 仕様書 §7.1
#[derive(Debug, Clone, Copy)]
pub enum DecoderKind {
    /// 画像デコーダー（JPEG, PNG, WebP, GIF, BMP, TIFF）
    Image,
    // Audio,  // 将来追加
    // Video,  // 将来追加
}

/// デコード結果。
/// 仕様書 §7.1
pub struct DecodeResult {
    /// デコード済み生データ
    pub data: Vec<u8>,
    /// WASMメモリに書き込むメタデータ（フォーマット依存、不透明バイト列）
    /// 画像: `[width:u32 LE, height:u32 LE, channels:u32 LE]` (12 bytes)
    pub metadata: Vec<u8>,
}

/// コンテンツのマジックバイトからデコーダーを自動選択する。
/// 仕様書 §7.1
pub fn detect(content: &[u8]) -> Option<DecoderKind> {
    if image_decoder::supports(content) {
        return Some(DecoderKind::Image);
    }
    // 将来: audio_decoder::supports(), video_decoder::supports() を追加
    None
}

/// デコード時のピークメモリ使用量（バイト）を推定する。
/// ヘッダのみ読みで算出し、フルデコードは行わない。
/// 戻り値: `Ok(bytes)` = 推定サイズ, `Err(rc)` = `decode_content` の戻り値
/// 仕様書 §7.1
pub fn estimate_peak_bytes(kind: DecoderKind, content: &[u8]) -> Result<usize, i32> {
    match kind {
        DecoderKind::Image => image_decoder::estimate_peak_bytes(content),
    }
}

/// コンテンツをデコードする。
/// 戻り値: `Ok(result)` = デコード結果, `Err(rc)` = `decode_content` の戻り値
/// 仕様書 §7.1
pub fn decode(kind: DecoderKind, content: &[u8]) -> Result<DecodeResult, i32> {
    match kind {
        DecoderKind::Image => image_decoder::decode(content),
    }
}

// ---------------------------------------------------------------------------
// 画像デコーダー
// ---------------------------------------------------------------------------

mod image_decoder {
    use super::*;
    use image::{ColorType, ImageDecoder, ImageReader};

    /// コンテンツが画像としてデコード可能かを判定する。
    pub fn supports(content: &[u8]) -> bool {
        ImageReader::new(Cursor::new(content))
            .with_guessed_format()
            .ok()
            .and_then(|r| r.format())
            .is_some()
    }

    /// 画像デコード時のピークメモリ（バイト）を推定する。
    /// ヘッダのみ読みで dimensions + ColorType を取得し、
    /// bit深度に基づいてネイティブバッファ + fallback変換バッファのピークを計算する。
    /// 仕様書 §7.1 — 圧縮爆弾対策
    pub fn estimate_peak_bytes(content: &[u8]) -> Result<usize, i32> {
        let reader = ImageReader::new(Cursor::new(content))
            .with_guessed_format()
            .map_err(|_| -1i32)?;
        let format = reader.format();
        let (width, height) = reader.into_dimensions().map_err(|_| -1i32)?;

        let peak_bpp = detect_peak_bpp(content, format);

        (width as usize)
            .checked_mul(height as usize)
            .and_then(|v| v.checked_mul(peak_bpp))
            .ok_or(-2i32)
    }

    /// 画像をネイティブフォーマットでデコードする。
    /// 仕様書 §7.1
    pub fn decode(content: &[u8]) -> Result<DecodeResult, i32> {
        let img = image::load_from_memory(content).map_err(|_| -3i32)?;

        let (width, height) = (img.width(), img.height());

        use image::DynamicImage;
        let (data, channels) = match img {
            DynamicImage::ImageLuma8(buf) => (buf.into_raw(), 1u32),
            DynamicImage::ImageRgb8(buf) => (buf.into_raw(), 3u32),
            DynamicImage::ImageRgba8(buf) => (buf.into_raw(), 4u32),
            other => (other.to_rgb8().into_raw(), 3u32), // fallback
        };

        // 画像メタデータ: [width:u32 LE, height:u32 LE, channels:u32 LE]
        let mut metadata = Vec::with_capacity(12);
        metadata.extend_from_slice(&width.to_le_bytes());
        metadata.extend_from_slice(&height.to_le_bytes());
        metadata.extend_from_slice(&channels.to_le_bytes());

        Ok(DecodeResult { data, metadata })
    }

    /// フォーマット別デコーダでカラータイプを判定し、ピークbytes/pixelを返す。
    /// 8-bit直接マッチ型: ピーク = native_bpp（変換バッファなし）
    /// その他（16-bit, 32-bit等）: ピーク = native_bpp + 3（to_rgb8変換バッファが一時共存）
    fn detect_peak_bpp(content: &[u8], format: Option<image::ImageFormat>) -> usize {
        let color_type = detect_color_type(content, format);

        match color_type {
            Some(ct) => {
                let native_bpp = color_type_bpp(ct);
                match ct {
                    // 8-bit variants: decode match で直接 into_raw()。変換バッファなし
                    ColorType::L8 | ColorType::Rgb8 | ColorType::Rgba8 => native_bpp,
                    // その他: to_rgb8() fallback で変換バッファ (w*h*3) が一時共存
                    _ => native_bpp + 3,
                }
            }
            // カラータイプ不明: 最悪ケース (RGBA32F=16bpp + to_rgb8=3bpp)
            None => 16 + 3,
        }
    }

    /// カラータイプごとのbytes/pixelを返す。
    fn color_type_bpp(ct: ColorType) -> usize {
        match ct {
            ColorType::L8 => 1,
            ColorType::La8 => 2,
            ColorType::Rgb8 => 3,
            ColorType::Rgba8 => 4,
            ColorType::L16 => 2,
            ColorType::La16 => 4,
            ColorType::Rgb16 => 6,
            ColorType::Rgba16 => 8,
            ColorType::Rgb32F => 12,
            ColorType::Rgba32F => 16,
            _ => 16, // 未知の型: 最悪ケース
        }
    }

    /// フォーマット別デコーダのヘッダ読みでカラータイプを判定する。
    fn detect_color_type(
        content: &[u8],
        format: Option<image::ImageFormat>,
    ) -> Option<ColorType> {
        match format? {
            image::ImageFormat::Jpeg => image::codecs::jpeg::JpegDecoder::new(Cursor::new(content))
                .ok()
                .map(|d| d.color_type()),
            image::ImageFormat::Png => image::codecs::png::PngDecoder::new(Cursor::new(content))
                .ok()
                .map(|d| d.color_type()),
            image::ImageFormat::Tiff => image::codecs::tiff::TiffDecoder::new(Cursor::new(content))
                .ok()
                .map(|d| d.color_type()),
            // 以下は常に8-bit（保守的にRGBA8=4bpp）
            image::ImageFormat::Gif => Some(ColorType::Rgba8),
            image::ImageFormat::WebP => Some(ColorType::Rgba8),
            image::ImageFormat::Bmp => Some(ColorType::Rgba8),
            _ => None,
        }
    }
}
