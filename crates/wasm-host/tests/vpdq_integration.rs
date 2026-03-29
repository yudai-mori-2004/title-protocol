// SPDX-License-Identifier: Apache-2.0

//! vPDQ integration test: compute per-frame PDQ hashes for a video file.
//!
//! Requires:
//! - `ffmpeg` and `ffprobe` on PATH
//! - `wasm/video-vpdq` built for wasm32-unknown-unknown
//! - `tests/fixtures/test_video.mp4`

use title_wasm_host::WasmRunner;

const WASM_RELATIVE: &str =
    "../../wasm/video-vpdq/target/wasm32-unknown-unknown/release/video_vpdq.wasm";

const VIDEO_RELATIVE: &str = "../../tests/fixtures/video/mp4/sample.mp4";

fn load_wasm() -> Option<Vec<u8>> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    std::fs::read(format!("{manifest_dir}/{WASM_RELATIVE}")).ok()
}

fn load_video() -> Option<Vec<u8>> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    std::fs::read(format!("{manifest_dir}/{VIDEO_RELATIVE}")).ok()
}

#[test]
fn test_vpdq_basic() {
    let wasm = match load_wasm() {
        Some(w) => w,
        None => {
            eprintln!("SKIP: video-vpdq.wasm not found");
            return;
        }
    };
    let video = match load_video() {
        Some(v) => v,
        None => {
            eprintln!("SKIP: test_video.mp4 not found");
            return;
        }
    };

    // Video processing needs more fuel than images
    let runner = WasmRunner::new(10_000_000_000, 256 * 1024 * 1024);
    let result = runner
        .execute(&wasm, &video, "video/mp4", None, "process")
        .expect("vPDQ WASM execution failed");

    eprintln!("vPDQ result: {}", result.output);

    assert_eq!(result.output["algorithm"].as_str(), Some("vpdq"));

    let frames = result.output["frames"].as_array().expect("frames should be array");
    let frame_count = result.output["frame_count"].as_u64().unwrap();

    eprintln!("Frame hashes: {} (reported: {})", frames.len(), frame_count);
    assert_eq!(frames.len(), frame_count as usize);
    assert!(frame_count > 0, "Should have at least one frame hash");

    // Each frame should have pdqhash, quality, timestamp
    for (i, frame) in frames.iter().enumerate() {
        let hash = frame["pdqhash"].as_str().unwrap();
        let quality = frame["quality"].as_u64().unwrap();
        let timestamp = frame["timestamp"].as_f64().unwrap();

        assert_eq!(hash.len(), 64, "PDQ hash should be 64 hex chars");
        assert!(quality >= 50, "Quality should be >= 50 (filtered)");
        assert!(timestamp >= 0.0, "Timestamp should be non-negative");

        if i < 3 {
            eprintln!("  Frame {i}: hash={hash} quality={quality} ts={timestamp:.3}");
        }
    }
}

#[test]
fn test_vpdq_deterministic() {
    let wasm = match load_wasm() {
        Some(w) => w,
        None => {
            eprintln!("SKIP: video-vpdq.wasm not found");
            return;
        }
    };
    let video = match load_video() {
        Some(v) => v,
        None => {
            eprintln!("SKIP: test_video.mp4 not found");
            return;
        }
    };

    let runner = WasmRunner::new(10_000_000_000, 256 * 1024 * 1024);
    let r1 = runner
        .execute(&wasm, &video, "video/mp4", None, "process")
        .expect("run 1 failed");
    let r2 = runner
        .execute(&wasm, &video, "video/mp4", None, "process")
        .expect("run 2 failed");

    assert_eq!(
        r1.output["frames"], r2.output["frames"],
        "Same video should produce identical frame hashes"
    );
}
