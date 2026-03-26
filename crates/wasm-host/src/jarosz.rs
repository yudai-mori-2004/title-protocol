// SPDX-License-Identifier: Apache-2.0

//! Jarosz filter downsampling for PDQ perceptual hashing.
//!
//! Rust port of the Jarosz filter from Meta's ThreatExchange PDQ reference
//! implementation (BSD-licensed C++). The algorithm uses two passes of a 1D box
//! filter to approximate a triangle (tent) window, providing anti-aliased
//! downsampling suitable for perceptual hashing.
//!
//! # References
//!
//! - Original C++ source: <https://github.com/facebook/ThreatExchange/blob/main/pdq/cpp/downscaling/downscaling.cpp>
//! - Jarosz, W. (2001). *Fast Image Convolutions*. ACM SIGGRAPH.
//! - PDQ algorithm paper: <https://github.com/facebook/ThreatExchange/blob/main/pdq/pdqhash-2017-10-09.pdf>
//!
//! # License of original work
//!
//! The original C++ implementation is Copyright (c) Meta Platforms, Inc. and
//! affiliates, licensed under the BSD License. This Rust port is licensed under
//! Apache-2.0 as part of the Title Protocol project.

/// Compute the Jarosz filter window size for downscaling from `old_dim` to `new_dim`.
///
/// Matches the C++ `computeJaroszFilterWindowSize`. The window is sized so that
/// two passes of the box filter cover approximately `old_dim / new_dim` pixels,
/// producing a triangle-weighted average over the full block.
fn compute_window_size(old_dim: u32, new_dim: u32) -> usize {
    ((old_dim as usize) + 2 * (new_dim as usize) - 1) / (2 * new_dim as usize)
}

/// 1D box filter with adaptive boundary handling.
///
/// Port of C++ `box1DFloat`. The filter operates in four phases to avoid
/// boundary padding: it gradually widens the window at the start and narrows
/// it at the end, dividing by the actual number of accumulated samples in
/// each phase.
///
/// This approach produces exact results without requiring mirror/clamp padding,
/// and matches the original implementation's output.
fn box_1d_float(
    invec: &[f32],
    outvec: &mut [f32],
    vector_length: usize,
    stride: usize,
    full_window_size: usize,
) {
    let half_window_size = (full_window_size + 2) / 2;

    let phase_1_nreps = half_window_size - 1;
    let phase_2_nreps = full_window_size - half_window_size + 1;
    let phase_3_nreps = if vector_length > full_window_size {
        vector_length - full_window_size
    } else {
        0
    };
    let phase_4_nreps = half_window_size - 1;

    let mut li = 0usize;
    let mut ri = 0usize;
    let mut oi = 0usize;
    let mut sum = 0.0f32;
    let mut current_window_size = 0usize;

    // Phase 1: accumulate initial sum, no output
    for _ in 0..phase_1_nreps {
        sum += invec[ri];
        current_window_size += 1;
        ri += stride;
    }

    // Phase 2: growing window, begin writing output
    for _ in 0..phase_2_nreps {
        sum += invec[ri];
        current_window_size += 1;
        outvec[oi] = sum / current_window_size as f32;
        ri += stride;
        oi += stride;
    }

    // Phase 3: full window, sliding add+subtract
    for _ in 0..phase_3_nreps {
        sum += invec[ri];
        sum -= invec[li];
        outvec[oi] = sum / current_window_size as f32;
        li += stride;
        ri += stride;
        oi += stride;
    }

    // Phase 4: shrinking window, final output
    for _ in 0..phase_4_nreps {
        sum -= invec[li];
        current_window_size -= 1;
        outvec[oi] = sum / current_window_size as f32;
        li += stride;
        oi += stride;
    }
}

/// Apply horizontal box filter to each row. Port of C++ `boxAlongRowsFloat`.
fn box_along_rows(
    input: &[f32],
    output: &mut [f32],
    num_rows: usize,
    num_cols: usize,
    window_size: usize,
) {
    for i in 0..num_rows {
        let offset = i * num_cols;
        box_1d_float(
            &input[offset..],
            &mut output[offset..],
            num_cols,
            1,
            window_size,
        );
    }
}

/// Apply vertical box filter to each column. Port of C++ `boxAlongColsFloat`.
fn box_along_cols(
    input: &[f32],
    output: &mut [f32],
    num_rows: usize,
    num_cols: usize,
    window_size: usize,
) {
    for j in 0..num_cols {
        box_1d_float(&input[j..], &mut output[j..], num_rows, num_cols, window_size);
    }
}

/// Apply the full Jarosz filter: alternating row and column box filter passes.
///
/// Port of C++ `jaroszFilterFloat`. Uses a double-buffer scheme where each
/// pass reads from one buffer and writes to the other. For PDQ, `nreps` is 2
/// (two passes of row+column filtering approximate a 2D triangle kernel).
fn jarosz_filter(
    buffer1: &mut [f32],
    buffer2: &mut [f32],
    num_rows: usize,
    num_cols: usize,
    window_along_rows: usize,
    window_along_cols: usize,
    nreps: usize,
) {
    for _ in 0..nreps {
        box_along_rows(buffer1, buffer2, num_rows, num_cols, window_along_rows);
        box_along_cols(buffer2, buffer1, num_rows, num_cols, window_along_cols);
    }
}

/// Subsample a filtered buffer to the target size using center-pixel sampling.
///
/// Port of C++ `decimateFloat`. Each output pixel samples the input at the
/// center of its corresponding region: `ini = (outi + 0.5) * in_dim / out_dim`.
fn decimate(
    input: &[f32],
    in_rows: usize,
    in_cols: usize,
    output: &mut [f32],
    out_rows: usize,
    out_cols: usize,
) {
    for outi in 0..out_rows {
        let ini = ((outi as f64 + 0.5) * in_rows as f64 / out_rows as f64) as usize;
        for outj in 0..out_cols {
            let inj = ((outj as f64 + 0.5) * in_cols as f64 / out_cols as f64) as usize;
            output[outi * out_cols + outj] = input[ini * in_cols + inj];
        }
    }
}

/// Downsample decoded pixel data to a target size using the PDQ Jarosz filter.
///
/// Converts RGB/RGBA/grayscale pixels to f32 luminance (ITU-R BT.601), applies
/// the Jarosz filter at full resolution, then decimates to the target size.
/// The f32 pipeline avoids u8 quantization until the final output, matching
/// the precision of the C++ reference implementation.
///
/// # Arguments
///
/// * `data` - Raw decoded pixel data (interleaved RGB, RGBA, or grayscale)
/// * `width`, `height` - Source image dimensions
/// * `channels` - Number of channels per pixel (1, 3, or 4)
/// * `target_w`, `target_h` - Target output dimensions
pub fn downsample_from_decoded(
    data: &[u8],
    width: u32,
    height: u32,
    channels: u32,
    target_w: u32,
    target_h: u32,
) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let tw = target_w as usize;
    let th = target_h as usize;
    let ch = channels as usize;

    // Convert to f32 luminance (BT.601: Y = 0.299R + 0.587G + 0.114B)
    let mut buffer1 = vec![0.0f32; w * h];
    match ch {
        1 => {
            for i in 0..w * h {
                buffer1[i] = data[i] as f32;
            }
        }
        3 => {
            for i in 0..w * h {
                let r = data[i * 3] as f32;
                let g = data[i * 3 + 1] as f32;
                let b = data[i * 3 + 2] as f32;
                buffer1[i] = 0.299 * r + 0.587 * g + 0.114 * b;
            }
        }
        4 => {
            for i in 0..w * h {
                let r = data[i * 4] as f32;
                let g = data[i * 4 + 1] as f32;
                let b = data[i * 4 + 2] as f32;
                buffer1[i] = 0.299 * r + 0.587 * g + 0.114 * b;
            }
        }
        _ => {
            for i in 0..w * h {
                buffer1[i] = data[i * ch] as f32;
            }
        }
    }

    // If already at target size, return luminance directly (matches C++
    // reference fast path for pre-downsampled video frames).
    if w == tw && h == th {
        return buffer1
            .iter()
            .map(|&v| v.round().max(0.0).min(255.0) as u8)
            .collect();
    }

    // Apply Jarosz filter at full resolution, then decimate
    let mut buffer2 = vec![0.0f32; w * h];
    let window_along_rows = compute_window_size(width, target_w);
    let window_along_cols = compute_window_size(height, target_h);
    const NUM_JAROSZ_XY_PASSES: usize = 2;

    jarosz_filter(
        &mut buffer1,
        &mut buffer2,
        h,
        w,
        window_along_rows,
        window_along_cols,
        NUM_JAROSZ_XY_PASSES,
    );

    let mut output_f32 = vec![0.0f32; tw * th];
    decimate(&buffer1, h, w, &mut output_f32, th, tw);

    output_f32
        .iter()
        .map(|&v| v.round().max(0.0).min(255.0) as u8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_window_size() {
        assert_eq!(compute_window_size(1024, 64), 8);
        assert_eq!(compute_window_size(512, 64), 4);
        assert_eq!(compute_window_size(128, 64), 1);
        assert_eq!(compute_window_size(64, 64), 1);
        assert_eq!(compute_window_size(3840, 64), 30);
    }

    #[test]
    fn test_downsample_solid_preserves_value() {
        let data: Vec<u8> = vec![128; 256 * 256];
        let result = downsample_from_decoded(&data, 256, 256, 1, 64, 64);
        assert_eq!(result.len(), 64 * 64);
        for &v in &result {
            assert!((v as i32 - 128).unsigned_abs() <= 1, "Expected ~128, got {v}");
        }
    }

    #[test]
    fn test_downsample_output_size() {
        let data: Vec<u8> = (0..512 * 384 * 3).map(|i| (i % 256) as u8).collect();
        let result = downsample_from_decoded(&data, 512, 384, 3, 64, 64);
        assert_eq!(result.len(), 64 * 64);
    }

    #[test]
    fn test_downsample_deterministic() {
        let data: Vec<u8> = (0..256 * 256).map(|i| (i % 256) as u8).collect();
        let r1 = downsample_from_decoded(&data, 256, 256, 1, 64, 64);
        let r2 = downsample_from_decoded(&data, 256, 256, 1, 64, 64);
        assert_eq!(r1, r2);
    }
}
