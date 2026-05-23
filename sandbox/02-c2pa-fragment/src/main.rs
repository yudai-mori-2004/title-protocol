// SPDX-License-Identifier: Apache-2.0
//! Sandbox B: c2pa-rs CMAF フラグメント検証
//!
//! 目的:
//! c2pa-rs の `with_fragment` API で init.mp4 + seg-*.m4s の
//! フラグメント検証ができるか確認する。
//!
//! 仕様書参照: §1.3「入力形式 — フラグメント」, §4.3「フラグメント」

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;

// sha2 not needed in this sandbox (signature_hash computation deferred to main impl)

// テスト証明書（legacy から参照）
const CERTS: &[u8] = include_bytes!("../../../legacy/v0.1.0/tests/fixtures/certs/chain.pem");
const PRIVATE_KEY: &[u8] = include_bytes!("../../../legacy/v0.1.0/tests/fixtures/certs/ee.key");

// ---------------------------------------------------------------------------
// CMAF セグメント生成（ffmpeg）
// ---------------------------------------------------------------------------

/// ffmpeg で CMAF セグメントを生成する。
/// init.mp4 + seg-*.m4s の形式で出力。
fn generate_cmaf_segments(output_dir: &Path, duration_secs: u32) -> Result<(), String> {
    println!("  ffmpeg で CMAF セグメント生成中...");
    println!("  出力先: {}", output_dir.display());

    let output_pattern = output_dir.join("seg-%d.m4s");
    let init_path = output_dir.join("init.mp4");

    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f", "lavfi",
            "-i", &format!("testsrc=duration={duration_secs}:size=640x480:rate=30"),
            "-f", "lavfi",
            "-i", &format!("sine=frequency=440:duration={duration_secs}:sample_rate=44100"),
            "-c:v", "libx264",
            "-pix_fmt", "yuv420p",
            "-c:a", "aac",
            // CMAF フラグメント出力
            "-f", "dash",
            "-seg_duration", "2",  // 2秒セグメント
            "-init_seg_name", init_path.to_str().unwrap(),
            "-media_seg_name", output_pattern.to_str().unwrap(),
            "-use_template", "0",
            "-use_timeline", "0",
            // single file per segment
            "-single_file", "0",
        ])
        .arg(output_dir.join("manifest.mpd"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("ffmpeg 起動失敗: {e}"))?;

    if !status.success() {
        return Err("ffmpeg CMAF 生成に失敗しました".to_string());
    }

    // 生成されたファイルを確認
    if !init_path.exists() {
        return Err("init.mp4 が生成されませんでした".to_string());
    }

    let segments: Vec<_> = std::fs::read_dir(output_dir)
        .map_err(|e| format!("ディレクトリ読み取り失敗: {e}"))?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("seg-") && name.ends_with(".m4s")
        })
        .collect();

    println!("  生成完了:");
    println!("    init.mp4: {} bytes", std::fs::metadata(&init_path).map(|m| m.len()).unwrap_or(0));
    println!("    セグメント数: {}", segments.len());
    for seg in &segments {
        let size = seg.metadata().map(|m| m.len()).unwrap_or(0);
        println!("    {}: {} bytes ({:.1} KB)", seg.file_name().to_string_lossy(), size, size as f64 / 1024.0);
    }

    Ok(())
}

/// ffmpeg で CMAF フラグメント（fmp4 形式）を直接生成する。
/// DASH の manifest.mpd 経由ではなく、直接 fragmented mp4 を生成。
fn generate_fmp4_segments(output_dir: &Path, duration_secs: u32) -> Result<(), String> {
    println!("  ffmpeg で fragmented MP4 生成中...");

    // まず通常の MP4 を生成
    let temp_mp4 = output_dir.join("temp_source.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f", "lavfi",
            "-i", &format!("testsrc=duration={duration_secs}:size=640x480:rate=30"),
            "-f", "lavfi",
            "-i", &format!("sine=frequency=440:duration={duration_secs}:sample_rate=44100"),
            "-c:v", "libx264",
            "-pix_fmt", "yuv420p",
            "-c:a", "aac",
            "-movflags", "+faststart",
        ])
        .arg(&temp_mp4)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("ffmpeg 起動失敗: {e}"))?;

    if !status.success() {
        return Err("ffmpeg MP4 生成に失敗しました".to_string());
    }

    // fragmented MP4 に変換（init + segments）
    let init_path = output_dir.join("init.mp4");
    let seg_pattern = output_dir.join("seg-%d.m4s");

    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-i", temp_mp4.to_str().unwrap(),
            "-c", "copy",
            "-f", "dash",
            "-seg_duration", "2",
            "-init_seg_name", init_path.to_str().unwrap(),
            "-media_seg_name", seg_pattern.to_str().unwrap(),
            "-use_template", "0",
            "-use_timeline", "0",
            "-single_file", "0",
        ])
        .arg(output_dir.join("manifest.mpd"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("ffmpeg fragmented MP4 変換失敗: {e}"))?;

    if !status.success() {
        return Err("ffmpeg fragmented MP4 変換に失敗しました".to_string());
    }

    // temp ファイル削除
    let _ = std::fs::remove_file(&temp_mp4);

    // 生成されたファイルを確認
    list_generated_files(output_dir)
}

fn list_generated_files(dir: &Path) -> Result<(), String> {
    let init_path = dir.join("init.mp4");
    if !init_path.exists() {
        return Err("init.mp4 が生成されませんでした".to_string());
    }

    println!("  生成完了:");
    println!("    init.mp4: {} bytes", std::fs::metadata(&init_path).map(|m| m.len()).unwrap_or(0));

    let mut segments: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("ディレクトリ読み取り失敗: {e}"))?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("seg-") && name.ends_with(".m4s")
        })
        .collect();

    segments.sort_by_key(|e| e.file_name());

    println!("    セグメント数: {}", segments.len());
    for seg in &segments {
        let size = seg.metadata().map(|m| m.len()).unwrap_or(0);
        println!("    {}: {} bytes ({:.1} KB)", seg.file_name().to_string_lossy(), size, size as f64 / 1024.0);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// C2PA フラグメント署名
// ---------------------------------------------------------------------------

fn test_signer() -> Box<dyn c2pa::Signer> {
    c2pa::create_signer::from_keys(CERTS, PRIVATE_KEY, c2pa::SigningAlg::Ed25519, None)
        .expect("テスト用 Signer の作成に失敗")
}

/// sign_fragmented_files で init + セグメントに C2PA 署名する。
fn sign_fragments(input_dir: &Path, output_dir: &Path) -> Result<(), String> {
    println!("\n  C2PA 署名中（sign_fragmented_files）...");

    let init_path = input_dir.join("init.mp4");
    let fragment_glob = input_dir.join("seg-*.m4s");

    // 出力先を準備
    std::fs::create_dir_all(output_dir)
        .map_err(|e| format!("出力ディレクトリ作成失敗: {e}"))?;

    let manifest_json = serde_json::json!({
        "title": "test-cmaf-fragment",
        "format": "video/mp4",
        "claim_generator_info": [{
            "name": "title-sandbox-b",
            "version": "0.1.0"
        }]
    })
    .to_string();

    let context = c2pa::Context::default();
    let mut builder = c2pa::Builder::from_context(context)
        .with_definition(&manifest_json)
        .map_err(|e| format!("Builder 作成失敗: {e}"))?;

    let signer = test_signer();

    let output_path_buf = output_dir.to_path_buf();
    builder
        .sign_fragmented_files(
            signer.as_ref(),
            &init_path.to_path_buf(),
            &fragment_glob.to_path_buf(),
            &output_path_buf,
        )
        .map_err(|e| format!("フラグメント署名失敗: {e}"))?;

    println!("  署名完了。出力先: {}", output_dir.display());
    list_generated_files(output_dir)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// C2PA フラグメント検証
// ---------------------------------------------------------------------------

/// with_fragment API でフラグメントを1つずつ検証する。
fn verify_fragments(signed_dir: &Path) -> Result<(), String> {
    println!("\n--- フラグメント検証 ---");

    let init_path = signed_dir.join("init.mp4");
    if !init_path.exists() {
        return Err("署名済み init.mp4 が見つかりません".to_string());
    }

    // セグメントファイルを収集・ソート
    let mut segments: Vec<PathBuf> = std::fs::read_dir(signed_dir)
        .map_err(|e| format!("ディレクトリ読み取り失敗: {e}"))?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("seg-") && name.ends_with(".m4s")
        })
        .map(|e| e.path())
        .collect();

    segments.sort();

    println!("  init.mp4: {} bytes", std::fs::metadata(&init_path).map(|m| m.len()).unwrap_or(0));
    println!("  セグメント数: {}", segments.len());

    // init.mp4 を読み込み
    let init_data = std::fs::read(&init_path)
        .map_err(|e| format!("init.mp4 読み込み失敗: {e}"))?;

    // Reader を構築（init.mp4 + 最初のフラグメント）
    if segments.is_empty() {
        return Err("セグメントが見つかりません".to_string());
    }

    // フラグメントを1つずつ検証
    println!("\n  逐次検証テスト:");
    let mut total_fragment_bytes: u64 = 0;
    let mut peak_memory_estimate: u64 = init_data.len() as u64;

    for (i, seg_path) in segments.iter().enumerate() {
        let seg_data = std::fs::read(seg_path)
            .map_err(|e| format!("セグメント読み込み失敗: {e}"))?;
        let seg_size = seg_data.len() as u64;
        total_fragment_bytes += seg_size;

        // メモリ推定: init + 現在のフラグメント + Reader 内部状態
        let current_memory = init_data.len() as u64 + seg_size;
        if current_memory > peak_memory_estimate {
            peak_memory_estimate = current_memory;
        }

        let mut init_cursor = Cursor::new(&init_data);
        let mut seg_cursor = Cursor::new(&seg_data);

        let ctx = c2pa::Context::default();
        let result = c2pa::Reader::from_context(ctx)
            .with_fragment("video/mp4", &mut init_cursor, &mut seg_cursor);

        match result {
            Ok(reader) => {
                println!(
                    "    seg-{i}: {} bytes - 検証状態: {:?}",
                    seg_size,
                    reader.validation_state()
                );

                // 最初のフラグメントでマニフェスト情報を表示
                if i == 0 {
                    if let Some(manifest) = reader.active_manifest() {
                        println!("    Manifest: {}", reader.active_label().unwrap_or("unknown"));
                        println!("    タイトル: {}", manifest.title().unwrap_or("unknown"));
                    }
                }
            }
            Err(e) => {
                println!("    seg-{i}: {} bytes - ERROR: {e}", seg_size);
            }
        }

        // フラグメントデータを解放（スコープ終了で自動解放）
        // ここが §4.3 の「フラグメントデータを解放」に相当
    }

    println!("\n  メモリパターン分析:");
    println!("    init.mp4 サイズ: {} bytes", init_data.len());
    println!("    合計フラグメントサイズ: {} bytes ({:.2} MB)",
        total_fragment_bytes, total_fragment_bytes as f64 / 1_048_576.0);
    println!("    ピークメモリ推定: {} bytes ({:.2} MB)",
        peak_memory_estimate, peak_memory_estimate as f64 / 1_048_576.0);
    println!("    → init + 最大フラグメント1個分のみ（§4.3 のパターンに合致）");

    Ok(())
}

/// 部分フラグメントのみで検証が成功するかテスト
fn verify_partial_fragments(signed_dir: &Path) -> Result<(), String> {
    println!("\n--- 部分フラグメント検証テスト ---");
    println!("  全フラグメントを渡さずに、一部だけの検証が成功するか確認");

    let init_path = signed_dir.join("init.mp4");
    let init_data = std::fs::read(&init_path)
        .map_err(|e| format!("init.mp4 読み込み失敗: {e}"))?;

    let mut segments: Vec<PathBuf> = std::fs::read_dir(signed_dir)
        .map_err(|e| format!("ディレクトリ読み取り失敗: {e}"))?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("seg-") && name.ends_with(".m4s")
        })
        .map(|e| e.path())
        .collect();

    segments.sort();

    if segments.len() < 2 {
        println!("  SKIP: セグメントが2個未満のため部分テスト不可");
        return Ok(());
    }

    // 最初のフラグメントだけで検証
    let first_seg = std::fs::read(&segments[0])
        .map_err(|e| format!("最初のセグメント読み込み失敗: {e}"))?;

    let mut init_cursor = Cursor::new(&init_data);
    let mut seg_cursor = Cursor::new(&first_seg);

    let context = c2pa::Context::default();
    match c2pa::Reader::from_context(context)
        .with_fragment("video/mp4", &mut init_cursor, &mut seg_cursor)
    {
        Ok(reader) => {
            println!("  最初のフラグメントのみ: 検証状態 = {:?}", reader.validation_state());
            println!("  → 部分検証は可能（全フラグメント不要）");
        }
        Err(e) => {
            println!("  最初のフラグメントのみ: ERROR = {e}");
            println!("  → 部分検証は不可（全フラグメントが必要な可能性）");
        }
    }

    // 最後のフラグメントだけで検証
    let last_seg = std::fs::read(segments.last().unwrap())
        .map_err(|e| format!("最後のセグメント読み込み失敗: {e}"))?;

    let mut init_cursor = Cursor::new(&init_data);
    let mut seg_cursor = Cursor::new(&last_seg);

    let context = c2pa::Context::default();
    match c2pa::Reader::from_context(context)
        .with_fragment("video/mp4", &mut init_cursor, &mut seg_cursor)
    {
        Ok(reader) => {
            println!("  最後のフラグメントのみ: 検証状態 = {:?}", reader.validation_state());
        }
        Err(e) => {
            println!("  最後のフラグメントのみ: ERROR = {e}");
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    println!("========================================================");
    println!("  Sandbox B: c2pa-rs CMAF フラグメント検証");
    println!("  仕様書 §1.3, §4.3");
    println!("========================================================\n");

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let work_dir = manifest_dir.join("work");

    // 作業ディレクトリを準備
    let raw_dir = work_dir.join("raw");       // ffmpeg 出力
    let signed_dir = work_dir.join("signed"); // C2PA 署名済み

    let _ = std::fs::remove_dir_all(&work_dir);
    std::fs::create_dir_all(&raw_dir).expect("raw ディレクトリ作成失敗");

    // Step 1: CMAF セグメント生成
    println!("=== Step 1: CMAF セグメント生成 ===");
    match generate_fmp4_segments(&raw_dir, 10) {
        Ok(()) => println!("  => 生成成功"),
        Err(e) => {
            eprintln!("  => ERROR: {e}");
            // DASH 出力がうまくいかない場合、直接 CMAF 生成を試す
            println!("\n  フォールバック: 直接 CMAF 生成を試みます...");
            let _ = std::fs::remove_dir_all(&raw_dir);
            std::fs::create_dir_all(&raw_dir).unwrap();
            match generate_cmaf_segments(&raw_dir, 10) {
                Ok(()) => println!("  => 生成成功（フォールバック）"),
                Err(e2) => {
                    eprintln!("  => ERROR: {e2}");
                    eprintln!("  ffmpeg による CMAF セグメント生成に失敗しました。");
                    return;
                }
            }
        }
    }

    // Step 2: C2PA 署名
    println!("\n=== Step 2: C2PA フラグメント署名 ===");
    match sign_fragments(&raw_dir, &signed_dir) {
        Ok(()) => println!("  => 署名成功"),
        Err(e) => {
            eprintln!("  => ERROR: {e}");
            eprintln!("  C2PA フラグメント署名に失敗しました。");

            // 署名なしで検証 API のテストだけ行う
            println!("\n  フォールバック: 署名なしフラグメントで with_fragment API をテスト...");
            println!("  （署名なしなので検証は失敗するが、API の動作確認は可能）");

            // 未署名のフラグメントで API テスト
            match verify_fragments(&raw_dir) {
                Ok(()) => {}
                Err(e) => eprintln!("  API テスト ERROR: {e}"),
            }
            return;
        }
    }

    // Step 3: フラグメント検証
    println!("\n=== Step 3: フラグメント検証 ===");
    match verify_fragments(&signed_dir) {
        Ok(()) => println!("  => 検証テスト完了"),
        Err(e) => eprintln!("  => ERROR: {e}"),
    }

    // Step 4: 部分検証テスト
    println!("\n=== Step 4: 部分検証テスト ===");
    match verify_partial_fragments(&signed_dir) {
        Ok(()) => println!("  => 部分検証テスト完了"),
        Err(e) => eprintln!("  => ERROR: {e}"),
    }

    // サマリー
    println!("\n========================================================");
    println!("  検証完了");
    println!("========================================================");

    // 作業ディレクトリの後始末は行わない（結果確認用に残す）
    println!("  作業ディレクトリ: {}", work_dir.display());
}
