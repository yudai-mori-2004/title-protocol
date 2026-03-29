// SPDX-License-Identifier: Apache-2.0

//! Video frame extraction via ffmpeg CLI subprocess.
//!
//! Extracts individual frames from video content by invoking ffmpeg as a
//! subprocess. Content is written to a RAM-backed tmpfs (`/dev/shm`) to
//! avoid disk I/O while providing the seekable file interface that ffmpeg
//! requires for MP4 container parsing (moov atom).
//!
//! # Security model
//!
//! The ffmpeg binary runs inside the TEE (included in the attestable image).
//! Content written to `/dev/shm` resides in the TEE's memory space and is
//! not accessible to the host operator. Files are deleted immediately after
//! frame extraction.
//!
//! # Fallback
//!
//! When `/dev/shm` is not available (e.g., local development on macOS),
//! falls back to `std::env::temp_dir()`.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

/// Metadata extracted from a video file header via ffprobe.
#[derive(Debug, Clone)]
pub struct VideoMeta {
    pub width: u32,
    pub height: u32,
    pub frame_count: u32,
    /// Frames per second (e.g., 29.97, 30.0, 60.0)
    pub fps: f64,
    /// Duration in milliseconds
    pub duration_ms: u64,
}

/// Temporary file handle that deletes on drop.
struct TempVideoFile {
    path: PathBuf,
}

impl TempVideoFile {
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
        let path = dir.join(format!("title-tee-video-{pid}-{id}"));

        let mut file = std::fs::File::create(&path)
            .map_err(|e| format!("Failed to create temp video file: {e}"))?;
        file.write_all(content)
            .map_err(|e| format!("Failed to write temp video file: {e}"))?;

        Ok(TempVideoFile { path })
    }
}

impl Drop for TempVideoFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Extract video metadata using ffprobe.
pub fn probe(content: &[u8]) -> Result<VideoMeta, String> {
    let tmp = TempVideoFile::new(content)?;

    let output = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-select_streams", "v:0",
            "-show_entries", "stream=width,height,r_frame_rate,nb_frames,duration",
            "-show_entries", "format=duration",
            "-of", "json",
        ])
        .arg(&tmp.path)
        .output()
        .map_err(|e| format!("ffprobe execution failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ffprobe failed: {stderr}"));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("ffprobe JSON parse error: {e}"))?;

    let stream = json["streams"].as_array()
        .and_then(|s| s.first())
        .ok_or("No video stream found")?;

    let width = stream["width"].as_u64().unwrap_or(0) as u32;
    let height = stream["height"].as_u64().unwrap_or(0) as u32;

    // Parse r_frame_rate (e.g., "30000/1001" or "30/1")
    let fps = parse_frame_rate(stream["r_frame_rate"].as_str().unwrap_or("30/1"));

    // Duration: prefer stream duration, fall back to format duration
    let duration_secs = stream["duration"].as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| {
            json["format"]["duration"].as_str()
                .and_then(|s| s.parse::<f64>().ok())
        })
        .unwrap_or(0.0);
    let duration_ms = (duration_secs * 1000.0) as u64;

    // Frame count: prefer nb_frames, fall back to fps * duration
    let frame_count = stream["nb_frames"].as_str()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or_else(|| (fps * duration_secs).ceil() as u32);

    Ok(VideoMeta { width, height, frame_count, fps, duration_ms })
}

/// Extract a single frame as raw RGB24 pixels at the specified timestamp.
///
/// Returns `width * height * 3` bytes of RGB data.
pub fn extract_frame_rgb(content: &[u8], timestamp_secs: f64, width: u32, height: u32) -> Result<Vec<u8>, String> {
    let tmp = TempVideoFile::new(content)?;

    let ts = format!("{:.4}", timestamp_secs);

    let output = Command::new("ffmpeg")
        .args([
            "-ss", &ts,
            "-i",
        ])
        .arg(&tmp.path)
        .args([
            "-frames:v", "1",
            "-s", &format!("{width}x{height}"),
            "-pix_fmt", "rgb24",
            "-f", "rawvideo",
            "pipe:1",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("ffmpeg execution failed: {e}"))?;

    if !output.status.success() {
        return Err("ffmpeg frame extraction failed".to_string());
    }

    let expected = (width as usize) * (height as usize) * 3;
    if output.stdout.len() != expected {
        return Err(format!(
            "Unexpected frame size: got {} bytes, expected {expected}",
            output.stdout.len()
        ));
    }

    Ok(output.stdout)
}

fn parse_frame_rate(s: &str) -> f64 {
    if let Some((num, den)) = s.split_once('/') {
        let n: f64 = num.parse().unwrap_or(30.0);
        let d: f64 = den.parse().unwrap_or(1.0);
        if d > 0.0 { n / d } else { 30.0 }
    } else {
        s.parse().unwrap_or(30.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frame_rate() {
        assert!((parse_frame_rate("30/1") - 30.0).abs() < 0.01);
        assert!((parse_frame_rate("30000/1001") - 29.97).abs() < 0.01);
        assert!((parse_frame_rate("60") - 60.0).abs() < 0.01);
    }

    #[test]
    fn test_probe_and_extract_real_video() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let path = format!("{manifest_dir}/../../integration-tests/fixtures/video/mp4/sample.mp4");
        let content = match std::fs::read(&path) {
            Ok(c) => c,
            Err(_) => {
                eprintln!("SKIP: test_video.mp4 not found");
                return;
            }
        };

        let meta = probe(&content).expect("ffprobe should succeed");
        eprintln!("Video: {}x{}, {:.2}fps, {}ms, {} frames",
            meta.width, meta.height, meta.fps, meta.duration_ms, meta.frame_count);
        assert!(meta.width > 0);
        assert!(meta.height > 0);
        assert!(meta.fps > 0.0);

        // Extract first frame
        let frame = extract_frame_rgb(&content, 0.0, meta.width, meta.height)
            .expect("Frame extraction should succeed");
        assert_eq!(frame.len(), (meta.width as usize) * (meta.height as usize) * 3);
    }
}
