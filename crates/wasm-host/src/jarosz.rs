// SPDX-License-Identifier: Apache-2.0

//! PDQ知覚ハッシュ用Jaroszフィルタダウンサンプリング。
//!
//! 仕様書 §7.1 — Meta ThreatExchange PDQリファレンス実装（BSD C++）の
//! Jaroszフィルタを Rust に移植。1Dボックスフィルタ2パスで三角（テント）
//! ウィンドウを近似し、知覚ハッシュに適したアンチエイリアスダウンサンプリングを行う。
//!
//! # 参考文献
//!
//! - 元C++ソース: <https://github.com/facebook/ThreatExchange/blob/main/pdq/cpp/downscaling/downscaling.cpp>
//! - Jarosz, W. (2001). *Fast Image Convolutions*. ACM SIGGRAPH.
//! - PDQアルゴリズム論文: <https://github.com/facebook/ThreatExchange/blob/main/pdq/pdqhash-2017-10-09.pdf>
//!
//! # 元実装のライセンス
//!
//! 元C++実装は Copyright (c) Meta Platforms, Inc. and affiliates、
//! BSDライセンス。本Rust移植はTitle Protocolの一部としてApache-2.0。

/// `old_dim` → `new_dim` ダウンスケール時のJaroszフィルタウィンドウサイズを計算する。
///
/// C++ `computeJaroszFilterWindowSize` と同一。ボックスフィルタ2パスが
/// 約 `old_dim / new_dim` ピクセルをカバーし、ブロック全体の三角加重平均を生成する。
fn compute_window_size(old_dim: u32, new_dim: u32) -> usize {
    ((old_dim as usize) + 2 * (new_dim as usize) - 1) / (2 * new_dim as usize)
}

/// 適応的境界処理を持つ1Dボックスフィルタ。
///
/// C++ `box1DFloat` の移植。4フェーズで動作し、境界パディングを回避する:
/// 開始時にウィンドウを漸次拡大、終了時に漸次縮小し、各フェーズで
/// 実際の累積サンプル数で除算する。ミラー/クランプパディング不要で
/// 元実装と同一の結果を生成する。
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

/// 各行に水平ボックスフィルタを適用する。C++ `boxAlongRowsFloat` の移植。
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

/// 各列に垂直ボックスフィルタを適用する。C++ `boxAlongColsFloat` の移植。
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

/// Jaroszフィルタ全体を適用: 行・列ボックスフィルタパスの交互実行。
///
/// C++ `jaroszFilterFloat` の移植。ダブルバッファ方式で各パスは一方から
/// 読み取り他方に書き込む。PDQでは `nreps=2`（行+列フィルタ2パスで
/// 2D三角カーネルを近似）。
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

/// フィルタ済みバッファを中心ピクセルサンプリングで目標サイズにサブサンプルする。
///
/// C++ `decimateFloat` の移植。各出力ピクセルは対応領域の中心を
/// サンプリング: `ini = (outi + 0.5) * in_dim / out_dim`。
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

/// デコード済みピクセルデータをPDQ Jaroszフィルタで目標サイズにダウンサンプルする。
///
/// 仕様書 §7.1 — RGB/RGBA/グレースケールピクセルをf32輝度（ITU-R BT.601）に変換し、
/// フル解像度でJaroszフィルタを適用後、目標サイズにデシメートする。
/// f32パイプラインにより最終出力までu8量子化を回避し、C++リファレンス実装と
/// 同等の精度を維持する。
///
/// # 引数
///
/// * `data` - デコード済みピクセルデータ（インターリーブRGB, RGBA, またはグレースケール）
/// * `width`, `height` - 元画像の寸法
/// * `channels` - ピクセル当たりのチャネル数 (1, 3, or 4)
/// * `target_w`, `target_h` - 出力目標サイズ
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
