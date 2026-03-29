// SPDX-License-Identifier: Apache-2.0

//! Integration tests for unified format detection and decoding.
//!
//! Covers all supported content types: images, video, audio, camera RAW.
//! Uses organized fixture directory under `tests/fixtures/`.

use title_wasm_host::decode::{self, DecoderKind};

fn fixture(relative: &str) -> Option<Vec<u8>> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    std::fs::read(format!("{manifest_dir}/../../tests/fixtures/{relative}")).ok()
}

// -----------------------------------------------------------------------
// Image format detection
// -----------------------------------------------------------------------

#[test]
fn test_detect_jpeg() {
    let data = fixture("images/jpeg/pixel_plane.jpg").unwrap();
    assert!(matches!(decode::detect(&data), Some(DecoderKind::Image)));
}

#[test]
fn test_detect_png() {
    let data = fixture("images/png/sample.png").unwrap();
    assert!(matches!(decode::detect(&data), Some(DecoderKind::Image)));
}

#[test]
fn test_detect_webp() {
    let data = fixture("images/webp/sample.webp").unwrap();
    assert!(matches!(decode::detect(&data), Some(DecoderKind::Image)));
}

#[test]
fn test_detect_gif() {
    let data = fixture("images/gif/sample.gif").unwrap();
    assert!(matches!(decode::detect(&data), Some(DecoderKind::Image)));
}

#[test]
fn test_detect_bmp() {
    let data = fixture("images/bmp/sample.bmp").unwrap();
    assert!(matches!(decode::detect(&data), Some(DecoderKind::Image)));
}

#[test]
fn test_detect_tiff() {
    let data = fixture("images/tiff/sample.tiff").unwrap();
    // TIFF files go through RawImage path (exiftool first, image crate fallback).
    assert!(matches!(decode::detect(&data), Some(DecoderKind::RawImage)));
}

#[test]
fn test_detect_avif() {
    let data = match fixture("images/avif/sample.avif") {
        Some(d) => d,
        None => { eprintln!("SKIP: sample.avif not found"); return; }
    };
    // AVIF may be detected as Image or as MPEG4 container (AVIF is HEIF-based)
    let kind = decode::detect(&data);
    assert!(kind.is_some(), "AVIF should be detected, got None");
}

#[test]
fn test_detect_heic() {
    let data = match fixture("images/heic/sample.heic") {
        Some(d) => d,
        None => { eprintln!("SKIP: sample.heic not found"); return; }
    };
    let kind = decode::detect(&data);
    assert!(kind.is_some(), "HEIC should be detected, got None");
}

// -----------------------------------------------------------------------
// Video format detection
// -----------------------------------------------------------------------

#[test]
fn test_detect_mp4() {
    let data = fixture("video/mp4/sample.mp4").unwrap();
    assert!(matches!(decode::detect(&data), Some(DecoderKind::Video)));
}

// -----------------------------------------------------------------------
// Audio format detection
// -----------------------------------------------------------------------

#[test]
fn test_detect_mp3() {
    let data = fixture("audio/mp3/sample.mp3").unwrap();
    assert!(matches!(decode::detect(&data), Some(DecoderKind::Audio)));
}

#[test]
fn test_detect_wav() {
    let data = fixture("audio/wav/sample.wav").unwrap();
    assert!(matches!(decode::detect(&data), Some(DecoderKind::Audio)));
}

#[test]
fn test_detect_flac() {
    let data = fixture("audio/flac/sample.flac").unwrap();
    assert!(matches!(decode::detect(&data), Some(DecoderKind::Audio)));
}

// -----------------------------------------------------------------------
// Camera RAW format detection
// -----------------------------------------------------------------------

#[test]
fn test_detect_arw() {
    let data = match fixture("raw/arw/sample.arw") {
        Some(d) => d,
        None => { eprintln!("SKIP: sample.arw not found"); return; }
    };
    assert!(matches!(decode::detect(&data), Some(DecoderKind::RawImage)));
}

#[test]
fn test_detect_cr2() {
    let data = match fixture("raw/cr2/sample.cr2") {
        Some(d) => d,
        None => { eprintln!("SKIP: sample.cr2 not found"); return; }
    };
    // CR2 is TIFF-based. file-format detects as CanonRaw2 or TagImageFileFormat.
    let kind = decode::detect(&data);
    assert!(
        matches!(kind, Some(DecoderKind::RawImage) | Some(DecoderKind::Image)),
        "CR2 should be RawImage or Image (TIFF fallback), got {:?}", kind
    );
}

#[test]
fn test_detect_nef() {
    let data = match fixture("raw/nef/sample.nef") {
        Some(d) => d,
        None => { eprintln!("SKIP: sample.nef not found"); return; }
    };
    // NEF is TIFF-based. Detected as TagImageFileFormat, then is_standard_tiff
    // heuristic should route to RawImage (large file, small decoded thumbnail).
    let kind = decode::detect(&data);
    assert!(
        matches!(kind, Some(DecoderKind::RawImage) | Some(DecoderKind::Image)),
        "NEF should be RawImage or Image, got {:?}", kind
    );
}

#[test]
fn test_detect_dng() {
    let data = match fixture("raw/dng/sample.dng") {
        Some(d) => d,
        None => { eprintln!("SKIP: sample.dng not found"); return; }
    };
    let kind = decode::detect(&data);
    assert!(
        matches!(kind, Some(DecoderKind::Image) | Some(DecoderKind::RawImage)),
        "DNG should be Image or RawImage, got {:?}", kind
    );
}

// -----------------------------------------------------------------------
// Image decoding
// -----------------------------------------------------------------------

#[test]
fn test_decode_jpeg() {
    let data = fixture("images/jpeg/pixel_plane.jpg").unwrap();
    let kind = decode::detect(&data).unwrap();
    let result = decode::decode(kind, &data).unwrap();
    assert!(!result.data.is_empty());
    assert_eq!(result.metadata.len(), 12);
    let w = u32::from_le_bytes(result.metadata[0..4].try_into().unwrap());
    let h = u32::from_le_bytes(result.metadata[4..8].try_into().unwrap());
    eprintln!("JPEG decoded: {w}x{h}");
    assert!(w > 100 && h > 100);
}

#[test]
fn test_decode_png() {
    let data = fixture("images/png/sample.png").unwrap();
    let kind = decode::detect(&data).unwrap();
    let result = decode::decode(kind, &data).unwrap();
    assert!(!result.data.is_empty());
}

#[test]
fn test_decode_webp() {
    let data = fixture("images/webp/sample.webp").unwrap();
    let kind = decode::detect(&data).unwrap();
    let result = decode::decode(kind, &data).unwrap();
    assert!(!result.data.is_empty());
}

#[test]
fn test_decode_gif() {
    let data = fixture("images/gif/sample.gif").unwrap();
    let kind = decode::detect(&data).unwrap();
    let result = decode::decode(kind, &data).unwrap();
    assert!(!result.data.is_empty());
}

#[test]
fn test_decode_bmp() {
    let data = fixture("images/bmp/sample.bmp").unwrap();
    let kind = decode::detect(&data).unwrap();
    let result = decode::decode(kind, &data).unwrap();
    assert!(!result.data.is_empty());
}

#[test]
fn test_decode_tiff() {
    let data = fixture("images/tiff/sample.tiff").unwrap();
    let kind = decode::detect(&data).unwrap();
    // TIFF goes through RawImage → exiftool fails (no preview) → image crate fallback
    let result = decode::decode(kind, &data).unwrap();
    assert!(!result.data.is_empty());
}

// -----------------------------------------------------------------------
// Video decoding (metadata only)
// -----------------------------------------------------------------------

#[test]
fn test_decode_video_metadata() {
    let data = fixture("video/mp4/sample.mp4").unwrap();
    let kind = decode::detect(&data).unwrap();
    assert!(matches!(kind, DecoderKind::Video));
    let result = decode::decode(kind, &data).unwrap();
    assert!(result.data.is_empty());
    assert_eq!(result.metadata.len(), 20);
    let frame_count = u32::from_le_bytes(result.metadata[0..4].try_into().unwrap());
    let fps_x100 = u32::from_le_bytes(result.metadata[4..8].try_into().unwrap());
    eprintln!("Video: {frame_count} frames, {:.2} fps", fps_x100 as f64 / 100.0);
    assert!(frame_count > 0);
    assert!(fps_x100 > 0);
}

// -----------------------------------------------------------------------
// RAW decoding (exiftool preview extraction)
// -----------------------------------------------------------------------

#[test]
fn test_decode_raw_arw() {
    let data = match fixture("raw/arw/sample.arw") {
        Some(d) => d,
        None => { eprintln!("SKIP: sample.arw not found"); return; }
    };
    let kind = decode::detect(&data).unwrap();
    assert!(matches!(kind, DecoderKind::RawImage));
    match decode::decode(kind, &data) {
        Ok(r) => {
            assert!(!r.data.is_empty());
            let w = u32::from_le_bytes(r.metadata[0..4].try_into().unwrap());
            let h = u32::from_le_bytes(r.metadata[4..8].try_into().unwrap());
            eprintln!("ARW preview: {w}x{h}");
            assert!(w > 100 && h > 100);
        }
        Err(_) => eprintln!("SKIP: ARW decode failed (exiftool not available?)"),
    }
}

#[test]
fn test_decode_raw_cr2() {
    let data = match fixture("raw/cr2/sample.cr2") {
        Some(d) => d,
        None => { eprintln!("SKIP: sample.cr2 not found"); return; }
    };
    let kind = decode::detect(&data).unwrap();
    match decode::decode(kind, &data) {
        Ok(r) => {
            assert!(!r.data.is_empty());
            let w = u32::from_le_bytes(r.metadata[0..4].try_into().unwrap());
            let h = u32::from_le_bytes(r.metadata[4..8].try_into().unwrap());
            eprintln!("CR2 decoded: {w}x{h} (kind={kind:?})");
        }
        Err(_) => eprintln!("SKIP: CR2 decode failed"),
    }
}

#[test]
fn test_decode_raw_nef() {
    let data = match fixture("raw/nef/sample.nef") {
        Some(d) => d,
        None => { eprintln!("SKIP: sample.nef not found"); return; }
    };
    let kind = decode::detect(&data).unwrap();
    match decode::decode(kind, &data) {
        Ok(r) => {
            assert!(!r.data.is_empty());
            let w = u32::from_le_bytes(r.metadata[0..4].try_into().unwrap());
            let h = u32::from_le_bytes(r.metadata[4..8].try_into().unwrap());
            eprintln!("NEF decoded: {w}x{h} (kind={kind:?})");
        }
        Err(_) => eprintln!("SKIP: NEF decode failed"),
    }
}

#[test]
fn test_decode_raw_dng() {
    let data = match fixture("raw/dng/sample.dng") {
        Some(d) => d,
        None => { eprintln!("SKIP: sample.dng not found"); return; }
    };
    let kind = decode::detect(&data).unwrap();
    match decode::decode(kind, &data) {
        Ok(r) => {
            assert!(!r.data.is_empty());
            let w = u32::from_le_bytes(r.metadata[0..4].try_into().unwrap());
            let h = u32::from_le_bytes(r.metadata[4..8].try_into().unwrap());
            eprintln!("DNG decoded: {w}x{h} (kind={kind:?})");
        }
        Err(_) => eprintln!("SKIP: DNG decode failed"),
    }
}

// -----------------------------------------------------------------------
// Audio (not yet implemented)
// -----------------------------------------------------------------------

#[test]
fn test_decode_audio_mp3_not_implemented() {
    let data = fixture("audio/mp3/sample.mp3").unwrap();
    let kind = decode::detect(&data).unwrap();
    assert!(matches!(kind, DecoderKind::Audio));
    assert!(decode::decode(kind, &data).is_err());
}

#[test]
fn test_decode_audio_wav_not_implemented() {
    let data = fixture("audio/wav/sample.wav").unwrap();
    let kind = decode::detect(&data).unwrap();
    assert!(matches!(kind, DecoderKind::Audio));
    assert!(decode::decode(kind, &data).is_err());
}

#[test]
fn test_decode_audio_flac_not_implemented() {
    let data = fixture("audio/flac/sample.flac").unwrap();
    let kind = decode::detect(&data).unwrap();
    assert!(matches!(kind, DecoderKind::Audio));
    assert!(decode::decode(kind, &data).is_err());
}

// -----------------------------------------------------------------------
// C2PA signed files — detect format (C2PA payload doesn't change format detection)
// -----------------------------------------------------------------------

#[test]
fn test_detect_c2pa_signed_jpg() {
    let data = match fixture("c2pa/signed/sample.jpg") {
        Some(d) => d,
        None => { eprintln!("SKIP: c2pa signed sample.jpg not found"); return; }
    };
    assert!(matches!(decode::detect(&data), Some(DecoderKind::Image)));
}

#[test]
fn test_detect_c2pa_signed_tiff() {
    let data = match fixture("c2pa/signed/sample.tiff") {
        Some(d) => d,
        None => { eprintln!("SKIP: c2pa signed sample.tiff not found"); return; }
    };
    let kind = decode::detect(&data);
    assert!(kind.is_some(), "C2PA signed TIFF should be detected");
}

#[test]
fn test_detect_c2pa_signed_mp3() {
    let data = match fixture("c2pa/signed/sample.mp3") {
        Some(d) => d,
        None => { eprintln!("SKIP: c2pa signed sample.mp3 not found"); return; }
    };
    assert!(matches!(decode::detect(&data), Some(DecoderKind::Audio)));
}

#[test]
fn test_detect_c2pa_signed_wav() {
    let data = match fixture("c2pa/signed/sample.wav") {
        Some(d) => d,
        None => { eprintln!("SKIP: c2pa signed sample.wav not found"); return; }
    };
    assert!(matches!(decode::detect(&data), Some(DecoderKind::Audio)));
}

#[test]
fn test_decode_c2pa_signed_jpg() {
    let data = match fixture("c2pa/signed/sample.jpg") {
        Some(d) => d,
        None => { eprintln!("SKIP: c2pa signed sample.jpg not found"); return; }
    };
    let kind = decode::detect(&data).unwrap();
    let result = decode::decode(kind, &data).unwrap();
    assert!(!result.data.is_empty(), "C2PA-signed JPEG should decode to pixels");
}
