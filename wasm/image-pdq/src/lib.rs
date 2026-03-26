// SPDX-License-Identifier: Apache-2.0

//! PDQ 256-bit perceptual image hash (Meta ThreatExchange compatible).
//!
//! Computes a 256-bit perceptual hash using the PDQ algorithm developed by
//! Meta (formerly Facebook) for content matching. The hash is robust to
//! re-encoding, resizing, and minor edits, while different images produce
//! hashes with high Hamming distance.
//!
//! # Algorithm
//!
//! 1. Host decodes the image and produces a 64×64 luminance buffer via
//!    Jarosz filter downsampling (see `wasm-host/src/jarosz.rs`).
//! 2. This module computes a 2D DCT on the 64×64 buffer, extracting a
//!    16×16 block of low-frequency coefficients (skipping DC).
//! 3. The 256 coefficients are quantized against their median (Torben
//!    algorithm) to produce a 256-bit binary hash.
//! 4. A quality metric (0–100) is computed from the gradient energy of
//!    the downsampled image.
//!
//! # Compatibility
//!
//! The DCT matrix, Torben median, quality metric, and bit layout are ported
//! from the C++ reference implementation in Meta's ThreatExchange repository
//! (BSD-licensed). The hex output uses the same byte order as the `pdqhash`
//! Python package (`pip install pdqhash`).
//!
//! # References
//!
//! - PDQ reference (C++): <https://github.com/facebook/ThreatExchange/tree/main/pdq>
//! - PDQ algorithm paper: <https://github.com/facebook/ThreatExchange/blob/main/pdq/pdqhash-2017-10-09.pdf>
//! - Torben median: Torben Mogensen algorithm, N. Devillard implementation (public domain)
//!
//! # Target
//!
//! `wasm32-unknown-unknown` (`#![no_std]`)

#![no_std]

extern crate alloc;

use alloc::string::String;
use core::fmt::Write;

#[global_allocator]
static ALLOC: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// ---------------------------------------------------------------------------
// Host function declarations (provided by TEE wasm-host runtime)
// ---------------------------------------------------------------------------

extern "C" {
    fn read_content_chunk(offset: u32, length: u32, buf_ptr: u32) -> u32;
    fn get_content_length() -> u32;
    fn get_extension_input(buf_ptr: u32, buf_len: u32) -> u32;
    fn decode_content(params_ptr: u32, params_len: u32, metadata_ptr: u32) -> i32;
    fn get_decoded_feature(spec_ptr: u32, spec_len: u32, output_ptr: u32) -> i32;
}

// ---------------------------------------------------------------------------
// WASM memory allocator (exported for host interaction)
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn alloc(size: u32) -> u32 {
    let layout = core::alloc::Layout::from_size_align(size as usize, 1).unwrap();
    unsafe { alloc::alloc::alloc(layout) as u32 }
}

/// Write a JSON result string into a length-prefixed buffer and return its pointer.
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
// Constants
// ---------------------------------------------------------------------------

const SIZE: usize = 64;
const LOW_FREQ: usize = 16;
const HASH_BITS: usize = 256;
const HASH_BYTES: usize = HASH_BITS / 8;

// ---------------------------------------------------------------------------
// Quality metric — port of C++ pdqImageDomainQualityMetric
// ---------------------------------------------------------------------------

/// Compute image quality as a measure of gradient energy in the 64×64 buffer.
///
/// Each adjacent-pixel difference is integer-quantized via `((u-v)*100)/255`,
/// which discards gradients smaller than ~2.5 luminance units. The sum is
/// divided by an empirical constant (90) and capped at 100.
///
/// Ported from `pdqhashing.cpp` `pdqImageDomainQualityMetric`.
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

// ---------------------------------------------------------------------------
// Torben median — port of torben.cpp (public domain)
// ---------------------------------------------------------------------------

/// Find the median of a float array without sorting.
///
/// Implementation of the Torben Mogensen algorithm (N. Devillard, public
/// domain). Used by PDQ to threshold DCT coefficients. The behavior for
/// even-length arrays matches the C++ reference exactly.
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

// ---------------------------------------------------------------------------
// DCT + quantization — port of pdqhashing.cpp dct64To16 + pdqBuffer16x16ToBits
// ---------------------------------------------------------------------------

/// Compute the 256-bit PDQ hash from a 64×64 luminance buffer.
///
/// The DCT matrix is defined as:
///
///     D[i][j] = sqrt(2/64) * cos(π/128 * (i+1) * (2j+1))
///
/// where `i` ranges over 0..15 (output rows) and `j` over 0..63 (input
/// columns). The `(i+1)` term skips the DC component, so the hash captures
/// frequencies 1 through 16 in each dimension.
///
/// The 16×16 output is computed as `B = D * A * Dᵀ`, then binarized against
/// the Torben median of all 256 values.
fn compute_pdq_hash(gray: &[u8; SIZE * SIZE]) -> [u8; HASH_BYTES] {
    let mut a = [[0.0f32; SIZE]; SIZE];
    for y in 0..SIZE {
        for x in 0..SIZE {
            a[y][x] = gray[y * SIZE + x] as f32;
        }
    }

    // Pre-compute DCT matrix D[16][64]
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

    // T = D * A  (16×64 = 16×64 · 64×64)
    let mut t = [[0.0f32; SIZE]; LOW_FREQ];
    for i in 0..LOW_FREQ {
        for j in 0..SIZE {
            let mut sum = 0.0f32;
            for k in 0..SIZE {
                sum += d[i][k] * a[k][j];
            }
            t[i][j] = sum;
        }
    }

    // B = T * Dᵀ  (16×16 = 16×64 · 64×16)
    let mut dct_values = [0.0f32; LOW_FREQ * LOW_FREQ];
    for i in 0..LOW_FREQ {
        for j in 0..LOW_FREQ {
            let mut sum = 0.0f32;
            for k in 0..SIZE {
                sum += t[i][k] * d[j][k];
            }
            dct_values[i * LOW_FREQ + j] = sum;
        }
    }

    // Quantize against median
    let median = torben_median(&dct_values);
    let mut hash = [0u8; HASH_BYTES];
    for i in 0..HASH_BITS {
        if dct_values[i] > median {
            hash[i / 8] |= 1u8 << (i % 8);
        }
    }

    hash
}

// ---------------------------------------------------------------------------
// WASM export
// ---------------------------------------------------------------------------

/// Compute a PDQ 256-bit perceptual hash for the input image.
///
/// Output JSON: `{"pdqhash":"<64 hex chars>","quality":<0-100>,"algorithm":"pdq","bits":256}`
#[no_mangle]
pub extern "C" fn process() -> u32 {
    let _ = (get_extension_input, read_content_chunk, get_content_length);

    let mut metadata = [0u8; 12];
    let rc = unsafe { decode_content(0, 0, metadata.as_mut_ptr() as u32) };
    match rc {
        0 => {}
        -1 => return write_result("{\"error\":\"unsupported format\"}"),
        -2 => return write_result("{\"error\":\"memory budget exceeded\"}"),
        _ => return write_result("{\"error\":\"decode error\"}"),
    }

    let spec = b"{\"op\":\"grayscale_resize\",\"width\":64,\"height\":64}";
    let mut gray = [0u8; SIZE * SIZE];
    let rc = unsafe {
        get_decoded_feature(
            spec.as_ptr() as u32,
            spec.len() as u32,
            gray.as_mut_ptr() as u32,
        )
    };
    if rc != (SIZE * SIZE) as i32 {
        return write_result("{\"error\":\"grayscale_resize failed\"}");
    }

    let quality = compute_quality(&gray);
    let hash = compute_pdq_hash(&gray);

    // Emit hash in Meta-compatible byte order (MSB-first, matching the
    // pdqhash Python package's output convention).
    let mut hex = String::with_capacity(64);
    for i in (0..HASH_BYTES).rev() {
        let _ = write!(&mut hex, "{:02x}", hash[i]);
    }

    let mut json = String::with_capacity(128);
    json.push_str("{\"pdqhash\":\"");
    json.push_str(&hex);
    json.push_str("\",\"quality\":");
    let mut qbuf = String::with_capacity(4);
    let _ = write!(&mut qbuf, "{}", quality);
    json.push_str(&qbuf);
    json.push_str(",\"algorithm\":\"pdq\",\"bits\":256}");

    write_result(&json)
}
