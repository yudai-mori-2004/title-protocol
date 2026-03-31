// SPDX-License-Identifier: Apache-2.0

//! ffmpeg CLIサブプロセスによる動画フレーム抽出。
//!
//! 仕様書 §7.1 — ffmpegをサブプロセスとして呼び出し、動画コンテンツから
//! 個別フレームを抽出する。コンテンツはRAMバック型tmpfs (`/dev/shm`) に書き込み、
//! ディスクI/Oを回避しつつffmpegが必要とするシーク可能なファイルインターフェースを提供する
//! （MP4コンテナのmoovアトムパースに必要）。
//!
//! # セキュリティモデル
//!
//! ffmpegバイナリはTEE内で実行される（アテステーション対象イメージに含まれる）。
//! `/dev/shm` に書き込まれたコンテンツはTEEのメモリ空間内にあり、
//! ホストオペレータからはアクセス不可。フレーム抽出後に即座に削除される。
//!
//! # フォールバック
//!
//! `/dev/shm` が利用不可の場合（macOSでのローカル開発等）、
//! `std::env::temp_dir()` にフォールバックする。

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

/// ffprobeで動画ファイルヘッダから抽出されたメタデータ。
/// 仕様書 §7.1
#[derive(Debug, Clone)]
pub struct VideoMeta {
    pub width: u32,
    pub height: u32,
    pub frame_count: u32,
    /// フレームレート（例: 29.97, 30.0, 60.0）
    pub fps: f64,
    /// 再生時間（ミリ秒）
    pub duration_ms: u64,
}

/// Drop時に自動削除される一時ファイルハンドル。
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

/// ffprobeで動画メタデータを抽出する。
/// 仕様書 §7.1
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

/// 指定タイムスタンプのフレームをRGB24ピクセルとして抽出する。
///
/// 仕様書 §7.1 — `width * height * 3` バイトのRGBデータを返す。
pub fn extract_frame_rgb(content: &[u8], timestamp_secs: f64, width: u32, height: u32) -> Result<Vec<u8>, String> {
    let tmp = TempVideoFile::new(content)?;

    let ts = format!("{:.4}", timestamp_secs);

    let output = Command::new("ffmpeg")
        .args([
            // 決定論的なフレーム抽出のため、デコーダ依存の処理を無効化:
            // - ignore_editlist: edit list の解釈差異を排除し、生PTSで参照
            // - noautorotate: 回転メタデータの適用を無効化し、センサー生データで取得
            "-ignore_editlist", "1",
            "-noautorotate",
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
        let path = format!("{manifest_dir}/../../tests/fixtures/video/mp4/sample.mp4");
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
