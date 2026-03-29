// SPDX-License-Identifier: Apache-2.0

//! exiftool CLIサブプロセスによるRAW画像プレビュー抽出。
//!
//! 仕様書 §7.1 — カメラRAWファイル（ARW, DNG, NEF, CR3等）から
//! exiftoolを使用して埋め込みJPEGプレビューを抽出する。
//! 抽出されたプレビューは通常の画像パイプラインで知覚ハッシュ用にデコードされる。
//!
//! # セキュリティモデル
//!
//! `video.rs` と同様: exiftoolバイナリはTEE内で実行される
//! （アテステーション対象Dockerイメージに含まれる）。
//! コンテンツは `/dev/shm`（RAMバック型tmpfs）に書き込まれ、
//! プレビュー抽出後に即座に削除される。

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

/// `/dev/shm` 上の一時ファイル（temp_dirフォールバック）。Drop時に削除。
struct TempRawFile {
    path: PathBuf,
}

impl TempRawFile {
    fn new(content: &[u8]) -> Result<Self, String> {
        let dir = if std::path::Path::new("/dev/shm").exists() {
            PathBuf::from("/dev/shm")
        } else {
            std::env::temp_dir()
        };

        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let pid = std::process::id();
        let path = dir.join(format!("title-tee-raw-{pid}-{id}"));

        let mut file = std::fs::File::create(&path)
            .map_err(|e| format!("Failed to create temp RAW file: {e}"))?;
        file.write_all(content)
            .map_err(|e| format!("Failed to write temp RAW file: {e}"))?;

        Ok(TempRawFile { path })
    }
}

impl Drop for TempRawFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// exiftoolでRAWファイルから埋め込みJPEGプレビューを抽出する。
///
/// 仕様書 §7.1 — `-b -JpgFromRaw`（フルサイズプレビュー）を優先し、
/// 失敗時は `-b -PreviewImage`（中サイズ）にフォールバックする。
/// 生のJPEGバイト列を返す。
pub fn extract_preview_jpeg(content: &[u8]) -> Result<Vec<u8>, String> {
    let tmp = TempRawFile::new(content)?;

    // Try JpgFromRaw first (full-size embedded JPEG)
    let output = Command::new("exiftool")
        .args(["-b", "-JpgFromRaw"])
        .arg(&tmp.path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("exiftool execution failed: {e}"))?;

    if output.status.success() && output.stdout.len() > 100 {
        return Ok(output.stdout);
    }

    // Fallback: PreviewImage (medium-size)
    let output = Command::new("exiftool")
        .args(["-b", "-PreviewImage"])
        .arg(&tmp.path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("exiftool execution failed: {e}"))?;

    if output.status.success() && output.stdout.len() > 100 {
        return Ok(output.stdout);
    }

    Err("No embedded JPEG preview found in RAW file".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_non_raw_fails() {
        // JPEG is not a RAW file — exiftool won't find JpgFromRaw/PreviewImage
        let jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        let result = extract_preview_jpeg(&jpeg);
        assert!(result.is_err());
    }
}
