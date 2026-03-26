// SPDX-License-Identifier: Apache-2.0

//! vPDQ video perceptual hash (Meta ThreatExchange compatible).
//!
//! Computes a per-frame PDQ 256-bit hash sequence for video content. Each
//! frame is sampled at approximately 1-second intervals, hashed with the
//! PDQ algorithm, and annotated with quality and timestamp. Low-quality
//! frames (quality < 50) and consecutive duplicate frames are filtered out.
//!
//! # Algorithm
//!
//! 1. Host decodes video metadata (frame count, fps) via `decode_content`.
//! 2. For each sampled frame (every `floor(fps)` frames ≈ 1 per second):
//!    a. Host extracts the frame via ffmpeg and returns a 64×64 Jarosz-
//!       downsampled luminance buffer (`video_frame_grayscale` op).
//!    b. This module computes the PDQ hash (DCT + Torben median) and quality.
//!    c. Frames with quality < 50 or identical hash to the previous frame
//!       are pruned.
//! 3. Output is a JSON array of per-frame hashes with timestamps.
//!
//! # References
//!
//! - vPDQ reference (C++): <https://github.com/facebook/ThreatExchange/tree/main/vpdq>
//! - PDQ algorithm: <https://github.com/facebook/ThreatExchange/tree/main/pdq>
//!
//! # Target
//!
//! `wasm32-unknown-unknown` (`#![no_std]`)

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

#[global_allocator]
static ALLOC: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// ---------------------------------------------------------------------------
// Host function declarations
// ---------------------------------------------------------------------------

extern "C" {
    fn read_content_chunk(offset: u32, length: u32, buf_ptr: u32) -> u32;
    fn get_content_length() -> u32;
    fn get_extension_input(buf_ptr: u32, buf_len: u32) -> u32;
    fn decode_content(params_ptr: u32, params_len: u32, metadata_ptr: u32) -> i32;
    fn get_decoded_feature(spec_ptr: u32, spec_len: u32, output_ptr: u32) -> i32;
}

// ---------------------------------------------------------------------------
// Memory allocator
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn alloc(size: u32) -> u32 {
    let layout = core::alloc::Layout::from_size_align(size as usize, 1).unwrap();
    unsafe { alloc::alloc::alloc(layout) as u32 }
}

fn write_result(json: &str) -> u32 {
    let json_bytes = json.as_bytes();
    let total = 4 + json_bytes.len();
    let ptr = alloc(total as u32);
    if ptr == 0 {
        return 0;
    }
    let len_bytes = (json_bytes.len() as u32).to_le_bytes();
    unsafe {
        let p = ptr as *mut u8;
        core::ptr::copy_nonoverlapping(len_bytes.as_ptr(), p, 4);
        core::ptr::copy_nonoverlapping(json_bytes.as_ptr(), p.add(4), json_bytes.len());
    }
    ptr
}

// ---------------------------------------------------------------------------
// PDQ constants and core functions (shared with image-pdq)
// ---------------------------------------------------------------------------

const SIZE: usize = 64;
const LOW_FREQ: usize = 16;
const HASH_BITS: usize = 256;
const HASH_BYTES: usize = HASH_BITS / 8;
const QUALITY_THRESHOLD: u32 = 50;

fn compute_quality(buf: &[u8; SIZE * SIZE]) -> u32 {
    let mut gradient_sum = 0i32;
    for i in 0..(SIZE - 1) {
        for j in 0..SIZE {
            let u = buf[i * SIZE + j] as f32;
            let v = buf[(i + 1) * SIZE + j] as f32;
            let d = ((u - v) * 100.0) as i32 / 255;
            gradient_sum += if d < 0 { -d } else { d };
        }
    }
    for i in 0..SIZE {
        for j in 0..(SIZE - 1) {
            let u = buf[i * SIZE + j] as f32;
            let v = buf[i * SIZE + j + 1] as f32;
            let d = ((u - v) * 100.0) as i32 / 255;
            gradient_sum += if d < 0 { -d } else { d };
        }
    }
    let quality = gradient_sum / 90;
    if quality > 100 { 100 } else { quality as u32 }
}

fn torben_median(m: &[f32; LOW_FREQ * LOW_FREQ]) -> f32 {
    let n = m.len();
    let mut min = m[0];
    let mut max = m[0];
    for i in 1..n {
        if m[i] < min { min = m[i]; }
        if m[i] > max { max = m[i]; }
    }
    loop {
        let guess = (min + max) / 2.0;
        let mut less = 0usize;
        let mut greater = 0usize;
        let mut equal = 0usize;
        let mut maxltguess = min;
        let mut mingtguess = max;
        for i in 0..n {
            if m[i] < guess {
                less += 1;
                if m[i] > maxltguess { maxltguess = m[i]; }
            } else if m[i] > guess {
                greater += 1;
                if m[i] < mingtguess { mingtguess = m[i]; }
            } else {
                equal += 1;
            }
        }
        if less <= (n + 1) / 2 && greater <= (n + 1) / 2 {
            return if less >= (n + 1) / 2 {
                maxltguess
            } else if less + equal >= (n + 1) / 2 {
                guess
            } else {
                mingtguess
            };
        } else if less > greater {
            max = maxltguess;
        } else {
            min = mingtguess;
        }
    }
}

fn compute_pdq_hash(gray: &[u8; SIZE * SIZE]) -> [u8; HASH_BYTES] {
    let mut a = [[0.0f32; SIZE]; SIZE];
    for y in 0..SIZE {
        for x in 0..SIZE {
            a[y][x] = gray[y * SIZE + x] as f32;
        }
    }

    let matrix_scale = libm::sqrtf(2.0 / SIZE as f32);
    let mut d = [[0.0f32; SIZE]; LOW_FREQ];
    for i in 0..LOW_FREQ {
        for j in 0..SIZE {
            d[i][j] = matrix_scale * libm::cosf(
                core::f32::consts::PI / (2.0 * SIZE as f32)
                    * (i + 1) as f32
                    * (2 * j + 1) as f32,
            );
        }
    }

    let mut t = [[0.0f32; SIZE]; LOW_FREQ];
    for i in 0..LOW_FREQ {
        for j in 0..SIZE {
            let mut sum = 0.0f32;
            for k in 0..SIZE { sum += d[i][k] * a[k][j]; }
            t[i][j] = sum;
        }
    }

    let mut dct_values = [0.0f32; LOW_FREQ * LOW_FREQ];
    for i in 0..LOW_FREQ {
        for j in 0..LOW_FREQ {
            let mut sum = 0.0f32;
            for k in 0..SIZE { sum += t[i][k] * d[j][k]; }
            dct_values[i * LOW_FREQ + j] = sum;
        }
    }

    let median = torben_median(&dct_values);
    let mut hash = [0u8; HASH_BYTES];
    for i in 0..HASH_BITS {
        if dct_values[i] > median {
            hash[i / 8] |= 1u8 << (i % 8);
        }
    }
    hash
}

fn hash_to_hex(hash: &[u8; HASH_BYTES]) -> String {
    let mut hex = String::with_capacity(64);
    for i in (0..HASH_BYTES).rev() {
        let _ = write!(&mut hex, "{:02x}", hash[i]);
    }
    hex
}

fn hashes_equal(a: &[u8; HASH_BYTES], b: &[u8; HASH_BYTES]) -> bool {
    a == b
}

// ---------------------------------------------------------------------------
// WASM export
// ---------------------------------------------------------------------------

/// Compute vPDQ frame hash sequence for a video.
///
/// Output JSON:
/// ```json
/// {
///   "frames": [
///     {"pdqhash": "<64 hex>", "quality": 85, "timestamp": 0.0},
///     ...
///   ],
///   "frame_count": 42,
///   "algorithm": "vpdq",
///   "sampling_fps": 1
/// }
/// ```
#[no_mangle]
pub extern "C" fn process() -> u32 {
    let _ = (get_extension_input, read_content_chunk, get_content_length);

    // 1. Decode video header → metadata
    //    Video metadata: [frame_count:u32, fps_x100:u32, width:u32, height:u32, duration_ms:u32]
    let mut metadata = [0u8; 20];
    let rc = unsafe { decode_content(0, 0, metadata.as_mut_ptr() as u32) };
    if rc != 0 {
        return write_result("{\"error\":\"video decode failed\"}");
    }

    let total_frames = u32::from_le_bytes([metadata[0], metadata[1], metadata[2], metadata[3]]);
    let fps_x100 = u32::from_le_bytes([metadata[4], metadata[5], metadata[6], metadata[7]]);
    let fps = fps_x100 as f64 / 100.0;

    if total_frames == 0 || fps <= 0.0 {
        return write_result("{\"error\":\"invalid video metadata\"}");
    }

    // 2. Sampling interval: 1 frame per second
    let sampling_mod = if fps >= 1.0 { fps as u32 } else { 1 };

    // 3. Iterate frames, compute PDQ for each sampled frame
    let mut frames_json = String::with_capacity(4096);
    frames_json.push('[');
    let mut prev_hash = [0u8; HASH_BYTES];
    let mut has_prev = false;
    let mut output_count = 0u32;

    let mut frame = 0u32;
    while frame < total_frames {
        // Build spec JSON for video_frame_grayscale
        let mut spec = String::with_capacity(80);
        let _ = write!(
            &mut spec,
            "{{\"op\":\"video_frame_grayscale\",\"frame\":{},\"width\":64,\"height\":64}}",
            frame
        );
        let spec_bytes = spec.as_bytes();
        let spec_ptr = alloc(spec_bytes.len() as u32);
        if spec_ptr == 0 {
            break;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(
                spec_bytes.as_ptr(),
                spec_ptr as *mut u8,
                spec_bytes.len(),
            );
        }

        let mut gray = [0u8; SIZE * SIZE];
        let rc = unsafe {
            get_decoded_feature(
                spec_ptr,
                spec_bytes.len() as u32,
                gray.as_mut_ptr() as u32,
            )
        };

        if rc != (SIZE * SIZE) as i32 {
            // Frame extraction failed — skip
            frame += sampling_mod;
            continue;
        }

        let quality = compute_quality(&gray);

        // Quality filter (Meta default: 50)
        if quality < QUALITY_THRESHOLD {
            frame += sampling_mod;
            continue;
        }

        let hash = compute_pdq_hash(&gray);

        // Dedupe: skip if identical to previous hash
        if has_prev && hashes_equal(&hash, &prev_hash) {
            frame += sampling_mod;
            continue;
        }

        // Append to output
        let timestamp = frame as f64 / fps;
        let hex = hash_to_hex(&hash);

        if output_count > 0 {
            frames_json.push(',');
        }
        let _ = write!(
            &mut frames_json,
            "{{\"pdqhash\":\"{hex}\",\"quality\":{quality},\"timestamp\":{timestamp:.3}}}"
        );

        prev_hash = hash;
        has_prev = true;
        output_count += 1;
        frame += sampling_mod;
    }

    frames_json.push(']');

    // 4. Build final result
    let mut json = String::with_capacity(frames_json.len() + 100);
    json.push_str("{\"frames\":");
    json.push_str(&frames_json);
    let _ = write!(&mut json, ",\"frame_count\":{output_count},\"algorithm\":\"vpdq\",\"sampling_fps\":1}}");

    write_result(&json)
}
