// SPDX-License-Identifier: Apache-2.0

//! RAW image preview extraction via exiftool CLI subprocess.
//!
//! Extracts the embedded JPEG preview from camera RAW files (ARW, DNG, NEF,
//! CR3, etc.) using exiftool. The preview is then decoded through the normal
//! image pipeline for perceptual hashing.
//!
//! # Security model
//!
//! Same as `video.rs`: the exiftool binary runs inside the TEE (included in
//! the attestable Docker image). Content is written to `/dev/shm` (RAM-backed
//! tmpfs) and deleted immediately after preview extraction.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

/// Temporary file on `/dev/shm` (or temp_dir fallback). Deleted on drop.
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

/// Extract the embedded JPEG preview from a RAW file using exiftool.
///
/// Tries `-b -JpgFromRaw` first (full-size preview), then falls back to
/// `-b -PreviewImage` (medium-size). Returns the raw JPEG bytes.
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
