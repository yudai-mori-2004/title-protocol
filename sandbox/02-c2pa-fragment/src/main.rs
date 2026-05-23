// SPDX-License-Identifier: Apache-2.0
//! Sandbox B: c2pa-rs CMAF フラグメント検証
//!
//! 目的:
//! c2pa-rs の `with_fragment` API で init.mp4 + seg-*.m4s の
//! フラグメント署名・検証ラウンドトリップが正しく動作するか確認する。
//!
//! 検証の信頼性を担保する基準（Sandbox A で確立）:
//! 1. v0.84 ラウンドトリップ（署名→検証）で cert 信頼性以外のエラーがないこと
//! 2. フラグメント改ざん時にハッシュ不整合が検出されること
//! 3. validation_status() の全コードを列挙して判断すること
//!
//! 仕様書参照: §1.3「入力形式 — フラグメント」, §4.3「フラグメント」

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// CMAF セグメント生成（ffmpeg）
// ---------------------------------------------------------------------------

/// ffmpeg で CMAF セグメントを生成する。
/// init.mp4 + seg-*.m4s の形式で出力。
///
/// 重要: ffmpeg DASH muxer は `-init_seg_name` / `-media_seg_name` を
/// MPD ファイルのディレクトリからの相対パスとして解釈する。
/// 絶対パスを渡すと二重パスになるため、ファイル名のみを指定し、
/// MPD の出力先で制御する。
fn generate_cmaf_segments(output_dir: &Path, duration_secs: u32) -> Result<(), String> {
    println!("  ffmpeg で CMAF セグメント生成中...");
    println!("  出力先: {}", output_dir.display());

    // まず通常の MP4 を生成（中間ファイル）
    let temp_mp4 = output_dir.join("temp_source.mp4");
    // Step 1: テストソース → 通常 MP4（video のみ）
    // DASH muxer は複数ストリームでセグメント名が競合するため、video only にする。
    // -g 60: キーフレーム間隔 = 60フレーム = 2秒@30fps。
    //   DASH muxer はキーフレーム境界でしかセグメントを分割できないため、
    //   seg_duration と一致させる必要がある。
    let output1 = Command::new("ffmpeg")
        .args([
            "-y",
            "-f", "lavfi",
            "-i", &format!("testsrc=duration={duration_secs}:size=640x480:rate=30"),
            "-c:v", "libx264",
            "-pix_fmt", "yuv420p",
            "-g", "60",
            "-keyint_min", "60",
            "-movflags", "+faststart",
        ])
        .arg(&temp_mp4)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("ffmpeg 起動失敗: {e}"))?;

    if !output1.status.success() {
        let stderr = String::from_utf8_lossy(&output1.stderr);
        return Err(format!("ffmpeg MP4 生成に失敗しました:\n{stderr}"));
    }

    // Step 2: 通常 MP4 → DASH fragmented MP4（init.mp4 + seg-*.m4s）
    // ファイル名のみを指定（相対パス）— MPD の出力先ディレクトリに生成される
    let mpd_path = output_dir.join("manifest.mpd");

    let output2 = Command::new("ffmpeg")
        .args([
            "-y",
            "-i", temp_mp4.to_str().unwrap(),
            "-c", "copy",
            "-f", "dash",
            "-seg_duration", "2",
            "-init_seg_name", "init.mp4",          // ファイル名のみ（絶対パスにしない）
            "-media_seg_name", "seg-$Number$.m4s",  // ファイル名のみ
            "-use_template", "1",
            "-use_timeline", "0",
            "-single_file", "0",
        ])
        .arg(&mpd_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("ffmpeg fragmented MP4 変換失敗: {e}"))?;

    if !output2.status.success() {
        let stderr = String::from_utf8_lossy(&output2.stderr);
        return Err(format!("ffmpeg fragmented MP4 変換に失敗しました:\n{stderr}"));
    }

    // temp ファイル削除
    let _ = std::fs::remove_file(&temp_mp4);

    // 生成されたファイルを確認
    list_generated_files(output_dir)
}

/// ディレクトリ内の init.mp4 + seg-*.m4s を一覧表示する。
fn list_generated_files(dir: &Path) -> Result<(), String> {
    let init_path = dir.join("init.mp4");
    if !init_path.exists() {
        // init.mp4 が無い場合、ディレクトリ内の全ファイルを表示してデバッグ
        println!("  !! init.mp4 が見つかりません。ディレクトリ内容:");
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                println!("    - {}", e.file_name().to_string_lossy());
            }
        }
        return Err("init.mp4 が生成されませんでした".to_string());
    }

    println!("  生成完了:");
    println!(
        "    init.mp4: {} bytes ({:.1} KB)",
        std::fs::metadata(&init_path).map(|m| m.len()).unwrap_or(0),
        std::fs::metadata(&init_path).map(|m| m.len()).unwrap_or(0) as f64 / 1024.0
    );

    let segments = collect_segments(dir);
    println!("    セグメント数: {}", segments.len());
    for seg in &segments {
        let size = seg.metadata().map(|m| m.len()).unwrap_or(0);
        println!(
            "    {}: {} bytes ({:.1} KB)",
            seg.file_name().unwrap_or_default().to_string_lossy(),
            size,
            size as f64 / 1024.0
        );
    }

    Ok(())
}

/// ディレクトリ内の seg-*.m4s をソート済みで返す。
fn collect_segments(dir: &Path) -> Vec<PathBuf> {
    let mut segments: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("seg-") && name.ends_with(".m4s")
        })
        .map(|e| e.path())
        .collect();

    // 数値順にソート（seg-1.m4s, seg-2.m4s, ...）
    segments.sort_by(|a, b| {
        let extract_num = |p: &Path| -> u32 {
            p.file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix("seg-"))
                .and_then(|n| n.strip_suffix(".m4s"))
                .and_then(|n| n.parse().ok())
                .unwrap_or(0)
        };
        extract_num(a).cmp(&extract_num(b))
    });

    segments
}

// ---------------------------------------------------------------------------
// 検証結果の詳細レポート（Sandbox A と同じ厳密さ）
// ---------------------------------------------------------------------------

/// 検証結果を詳細に出力し、ハードバインディング検証が実施されたか確認する。
/// 返り値: (validation_state, validation_status のコード一覧)
fn report_validation_details(reader: &c2pa::Reader, label: &str) -> (String, Vec<String>) {
    let state = format!("{:?}", reader.validation_state());
    println!("  [{label}] 検証状態: {state}");

    // validation_status: エラー一覧
    let mut status_codes = Vec::new();
    if let Some(statuses) = reader.validation_status() {
        println!("  [{label}] validation_status ({} 件):", statuses.len());
        for s in statuses {
            let code = s.code().to_string();
            println!("    [{}] url={}", code, s.url().unwrap_or("(none)"));
            status_codes.push(code);
        }
    } else {
        println!("  [{label}] validation_status: なし（エラーなし）");
    }

    // validation_results: 成功・情報・失敗の詳細
    if let Some(results) = reader.validation_results() {
        if let Some(active) = results.active_manifest() {
            let success = active.success().len();
            let info = active.informational().len();
            let failure = active.failure().len();
            println!("  [{label}] validation_results: success={success}, info={info}, failure={failure}");

            if failure > 0 {
                println!("  [{label}] !! 失敗項目:");
                for f in active.failure() {
                    println!("    - [{}] url={}", f.code(), f.url().unwrap_or("(none)"));
                }
            }
        }
    }

    // マニフェスト情報
    if let Some(manifest) = reader.active_manifest() {
        println!(
            "  [{label}] Manifest: {}, タイトル: {}",
            reader.active_label().unwrap_or("unknown"),
            manifest.title().unwrap_or("unknown")
        );
    }

    // JSON の SHA-256（結果の同一性比較用）
    let json = reader.json();
    let hash = Sha256::digest(json.as_bytes());
    println!("  [{label}] Manifest JSON hash: sha256:{}", hex::encode(hash));

    (state, status_codes)
}

/// validation_status で cert 信頼性以外のエラーがあるか判定する。
fn has_non_cert_errors(codes: &[String]) -> bool {
    codes.iter().any(|c| {
        c != "signing_credential.untrusted" && c != "signingCredential.untrusted"
    })
}

/// cert 信頼性以外のエラーコードを抽出する。
fn non_cert_error_codes(codes: &[String]) -> Vec<&String> {
    codes
        .iter()
        .filter(|c| {
            c.as_str() != "signing_credential.untrusted"
                && c.as_str() != "signingCredential.untrusted"
        })
        .collect()
}

// ---------------------------------------------------------------------------
// C2PA フラグメント署名（EphemeralSigner）
// ---------------------------------------------------------------------------

/// EphemeralSigner + sign_fragmented_files で init + セグメントに C2PA 署名する。
/// 返り値: 署名済みファイルが実際に生成されたディレクトリのパス
///
/// 注意: sign_fragmented_files は入力パスのディレクトリ構造を出力先に保持する。
/// 例: input_dir="work/raw", output_dir="work/signed" → "work/signed/raw/" に出力される。
fn sign_fragments(input_dir: &Path, output_dir: &Path) -> Result<PathBuf, String> {
    println!("\n  C2PA 署名中（EphemeralSigner + sign_fragmented_files）...");

    let init_path = input_dir.join("init.mp4");
    let fragment_glob = input_dir.join("seg-*.m4s");

    // 出力先を準備
    std::fs::create_dir_all(output_dir)
        .map_err(|e| format!("出力ディレクトリ作成失敗: {e}"))?;

    let manifest_json = serde_json::json!({
        "claim_generator_info": [{
            "name": "sandbox-b-fragment-test",
            "version": "0.1.0"
        }],
        "assertions": [{
            "label": "c2pa.actions.v2",
            "data": {
                "actions": [{
                    "action": "c2pa.created",
                    "digitalSourceType": "http://cv.iptc.org/newscodes/digitalsourcetype/trainedAlgorithmicMedia"
                }]
            }
        }]
    })
    .to_string();

    let context = c2pa::Context::default();
    let mut builder = c2pa::Builder::from_context(context)
        .with_definition(&manifest_json)
        .map_err(|e| format!("Builder 作成失敗: {e}"))?;

    // EphemeralSigner: c2pa-rs 組み込みのテスト用 Ed25519 signer
    // 自己署名 CA + EE 証明書を自動生成する（legacy cert 不要）
    let signer = c2pa::EphemeralSigner::new("sandbox-b-test")
        .map_err(|e| format!("EphemeralSigner 構築失敗: {e}"))?;

    builder
        .sign_fragmented_files(
            &signer,
            &init_path,
            &fragment_glob,
            &output_dir.to_path_buf(),
        )
        .map_err(|e| format!("フラグメント署名失敗: {e}"))?;

    // sign_fragmented_files は入力のディレクトリ構造を保持する。
    // 実際の出力先を探す（init.mp4 を再帰的に検索）。
    let actual_signed_dir = find_signed_dir(output_dir)?;
    println!(
        "  署名完了。実際の出力先: {}",
        actual_signed_dir.display()
    );
    list_generated_files(&actual_signed_dir)?;

    Ok(actual_signed_dir)
}

/// sign_fragmented_files が実際にファイルを出力したディレクトリを探す。
fn find_signed_dir(base: &Path) -> Result<PathBuf, String> {
    // base 直下に init.mp4 があればそこ
    if base.join("init.mp4").exists() {
        return Ok(base.to_path_buf());
    }

    // 再帰的に init.mp4 を探す
    fn find_init_recursive(dir: &Path) -> Option<PathBuf> {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(found) = find_init_recursive(&path) {
                        return Some(found);
                    }
                } else if entry.file_name() == "init.mp4" {
                    return Some(dir.to_path_buf());
                }
            }
        }
        None
    }

    find_init_recursive(base)
        .ok_or_else(|| format!("署名済み init.mp4 が見つかりません: {}", base.display()))
}

// ---------------------------------------------------------------------------
// フラグメント検証: ラウンドトリップ（正常系）
// ---------------------------------------------------------------------------

/// 署名済みフラグメントを1つずつ with_fragment で検証する。
/// 返り値: 各フラグメントの (segment_name, validation_state, status_codes, json_hash)
fn verify_fragments_roundtrip(
    signed_dir: &Path,
) -> Result<Vec<(String, String, Vec<String>, String)>, String> {
    println!("\n--- フラグメント検証（ラウンドトリップ） ---");

    let init_path = signed_dir.join("init.mp4");
    if !init_path.exists() {
        return Err("署名済み init.mp4 が見つかりません".to_string());
    }

    let segments = collect_segments(signed_dir);
    if segments.is_empty() {
        return Err("署名済みセグメントが見つかりません".to_string());
    }

    let init_data = std::fs::read(&init_path)
        .map_err(|e| format!("init.mp4 読み込み失敗: {e}"))?;

    println!(
        "  init.mp4: {} bytes ({:.1} KB)",
        init_data.len(),
        init_data.len() as f64 / 1024.0
    );
    println!("  セグメント数: {}", segments.len());

    let mut results = Vec::new();
    let mut total_fragment_bytes: u64 = 0;
    let mut peak_memory_estimate: u64 = init_data.len() as u64;

    for seg_path in &segments {
        let seg_name = seg_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let seg_data = std::fs::read(seg_path)
            .map_err(|e| format!("{seg_name} 読み込み失敗: {e}"))?;
        let seg_size = seg_data.len() as u64;
        total_fragment_bytes += seg_size;

        // メモリ推定: init + 現在のフラグメント
        let current_memory = init_data.len() as u64 + seg_size;
        if current_memory > peak_memory_estimate {
            peak_memory_estimate = current_memory;
        }

        let mut init_cursor = Cursor::new(&init_data);
        let mut seg_cursor = Cursor::new(&seg_data);

        let ctx = c2pa::Context::default();
        match c2pa::Reader::from_context(ctx)
            .with_fragment("video/mp4", &mut init_cursor, &mut seg_cursor)
        {
            Ok(reader) => {
                let label = format!("{seg_name} ({seg_size}B)");
                let (state, codes) = report_validation_details(&reader, &label);
                let json_hash = hex::encode(Sha256::digest(reader.json().as_bytes()));
                results.push((seg_name, state, codes, json_hash));
            }
            Err(e) => {
                println!("  [{seg_name}] ERROR: {e}");
                results.push((seg_name, "Error".to_string(), vec![format!("error: {e}")], String::new()));
            }
        }

        // フラグメントデータはスコープ終了で解放 — §4.3 のメモリパターン
    }

    println!("\n  メモリパターン分析:");
    println!("    init.mp4 サイズ: {} bytes", init_data.len());
    println!(
        "    合計フラグメントサイズ: {} bytes ({:.2} MB)",
        total_fragment_bytes,
        total_fragment_bytes as f64 / 1_048_576.0
    );
    println!(
        "    ピークメモリ推定: {} bytes ({:.2} MB)",
        peak_memory_estimate,
        peak_memory_estimate as f64 / 1_048_576.0
    );
    println!("    → init + 最大フラグメント1個分のみ（§4.3 のパターンに合致）");

    Ok(results)
}

// ---------------------------------------------------------------------------
// フラグメント改ざん検知テスト
// ---------------------------------------------------------------------------

/// フラグメントの内容を 1byte 改ざんして、検出されるか確認する。
/// これが検出されなければ「ハードバインディングが機能する」とは言えない。
fn verify_fragment_tampered(signed_dir: &Path) -> Result<(String, Vec<String>), String> {
    println!("\n--- フラグメント改ざん検知テスト ---");

    let init_path = signed_dir.join("init.mp4");
    let init_data = std::fs::read(&init_path)
        .map_err(|e| format!("init.mp4 読み込み失敗: {e}"))?;

    let segments = collect_segments(signed_dir);
    if segments.is_empty() {
        return Err("セグメントが見つかりません".to_string());
    }

    // 最初のセグメントを改ざんする
    let target_seg = &segments[0];
    let seg_name = target_seg
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let mut seg_data = std::fs::read(target_seg)
        .map_err(|e| format!("{seg_name} 読み込み失敗: {e}"))?;

    let seg_size = seg_data.len();
    if seg_size < 10 {
        return Err(format!("{seg_name} が小さすぎて改ざんテスト不可 ({seg_size} bytes)"));
    }

    // コンテンツ部分を改ざん（中間地点の1バイトを反転）
    let tamper_pos = seg_size / 2;
    let original_byte = seg_data[tamper_pos];
    seg_data[tamper_pos] ^= 0xFF;
    println!(
        "  改ざん対象: {seg_name} (offset={tamper_pos}, 0x{:02x} → 0x{:02x})",
        original_byte, seg_data[tamper_pos]
    );

    let mut init_cursor = Cursor::new(&init_data);
    let mut seg_cursor = Cursor::new(&seg_data);

    let ctx = c2pa::Context::default();
    match c2pa::Reader::from_context(ctx)
        .with_fragment("video/mp4", &mut init_cursor, &mut seg_cursor)
    {
        Ok(reader) => {
            let label = format!("{seg_name} (改ざん済み)");
            let (state, codes) = report_validation_details(&reader, &label);
            Ok((state, codes))
        }
        Err(e) => {
            println!("  Reader 構築自体が失敗: {e}");
            println!("  → 改ざんにより C2PA データ構造が壊れた（検知成功）");
            Ok(("Error".to_string(), vec![format!("reader_error: {e}")]))
        }
    }
}

/// init.mp4 を改ざんして検出されるか確認する。
/// 複数の改ざん位置を試すことで、どの領域がハッシュ保護されているか調べる。
///
/// init.mp4 の構造（署名後）:
/// - 先頭 ~835B: 元の BMFF boxes（ftyp + moov）
/// - 残り ~13,594B: C2PA JUMBF マニフェスト box
fn verify_init_tampered(signed_dir: &Path) -> Result<Vec<(String, String, Vec<String>)>, String> {
    println!("\n--- init.mp4 改ざん検知テスト ---");

    let init_path = signed_dir.join("init.mp4");
    let original_init = std::fs::read(&init_path)
        .map_err(|e| format!("init.mp4 読み込み失敗: {e}"))?;

    let segments = collect_segments(signed_dir);
    if segments.is_empty() {
        return Err("セグメントが見つかりません".to_string());
    }

    let seg_data = std::fs::read(&segments[0])
        .map_err(|e| format!("セグメント読み込み失敗: {e}"))?;

    let init_size = original_init.len();
    println!("  init.mp4 サイズ: {init_size} bytes");

    // 複数の位置で改ざんを試す
    let tamper_positions = vec![
        (100, "先頭付近 (ftyp/moov 領域)"),
        (400, "moov box 内部"),
        (init_size / 2, "中間地点 (JUMBF 領域内の可能性)"),
        (init_size - 100, "末尾付近"),
    ];

    let mut results = Vec::new();

    for (pos, description) in tamper_positions {
        if pos >= init_size {
            println!("  SKIP: offset {pos} はファイルサイズ外");
            continue;
        }

        let mut tampered_init = original_init.clone();
        let original_byte = tampered_init[pos];
        tampered_init[pos] ^= 0xFF;
        println!(
            "\n  改ざん位置: offset={pos} ({description}), 0x{:02x} → 0x{:02x}",
            original_byte, tampered_init[pos]
        );

        let mut init_cursor = Cursor::new(&tampered_init);
        let mut seg_cursor = Cursor::new(&seg_data);

        let ctx = c2pa::Context::default();
        match c2pa::Reader::from_context(ctx)
            .with_fragment("video/mp4", &mut init_cursor, &mut seg_cursor)
        {
            Ok(reader) => {
                let label = format!("init.mp4 改ざん@{pos}");
                let (state, codes) = report_validation_details(&reader, &label);
                let detected = has_non_cert_errors(&codes) || state == "Invalid";
                println!(
                    "  → {description}: {}",
                    if detected { "改ざん検出 ✓" } else { "改ざん未検出 ⚠" }
                );
                results.push((description.to_string(), state, codes));
            }
            Err(e) => {
                println!("  → {description}: Reader 構築失敗 = {e}（検知成功）");
                results.push((
                    description.to_string(),
                    "Error".to_string(),
                    vec![format!("reader_error: {e}")],
                ));
            }
        }
    }

    Ok(results)
}

// ---------------------------------------------------------------------------
// 部分フラグメント検証
// ---------------------------------------------------------------------------

/// 全フラグメントを渡さずに、一部だけの検証が成功するかテスト。
/// ストリーミング処理では、全フラグメントが揃う前に個別検証できることが重要。
fn verify_partial_fragments(signed_dir: &Path) -> Result<(), String> {
    println!("\n--- 部分フラグメント検証テスト ---");
    println!("  全フラグメントを渡さずに、一部だけの検証が成功するか確認");

    let init_path = signed_dir.join("init.mp4");
    let init_data = std::fs::read(&init_path)
        .map_err(|e| format!("init.mp4 読み込み失敗: {e}"))?;

    let segments = collect_segments(signed_dir);
    if segments.len() < 2 {
        println!("  SKIP: セグメントが2個未満のため部分テスト不可");
        return Ok(());
    }

    // 最初のフラグメントだけで検証
    let first_seg_data = std::fs::read(&segments[0])
        .map_err(|e| format!("最初のセグメント読み込み失敗: {e}"))?;
    {
        let mut init_cursor = Cursor::new(&init_data);
        let mut seg_cursor = Cursor::new(&first_seg_data);

        let ctx = c2pa::Context::default();
        match c2pa::Reader::from_context(ctx)
            .with_fragment("video/mp4", &mut init_cursor, &mut seg_cursor)
        {
            Ok(reader) => {
                let (state, codes) = report_validation_details(&reader, "最初のフラグメントのみ");
                let has_errors = has_non_cert_errors(&codes);
                if has_errors {
                    println!("  → 部分検証: 検証エラーあり（cert以外）");
                } else {
                    println!("  → 部分検証は可能（全フラグメント不要）: state={state}");
                }
            }
            Err(e) => {
                println!("  最初のフラグメントのみ: ERROR = {e}");
                println!("  → 部分検証は不可（全フラグメントが必要な可能性）");
            }
        }
    }

    // 最後のフラグメントだけで検証
    let last_seg_data = std::fs::read(segments.last().unwrap())
        .map_err(|e| format!("最後のセグメント読み込み失敗: {e}"))?;
    {
        let mut init_cursor = Cursor::new(&init_data);
        let mut seg_cursor = Cursor::new(&last_seg_data);

        let ctx = c2pa::Context::default();
        match c2pa::Reader::from_context(ctx)
            .with_fragment("video/mp4", &mut init_cursor, &mut seg_cursor)
        {
            Ok(reader) => {
                let (state, codes) = report_validation_details(&reader, "最後のフラグメントのみ");
                let has_errors = has_non_cert_errors(&codes);
                if has_errors {
                    println!("  → 部分検証: 検証エラーあり（cert以外）");
                } else {
                    println!("  → 最後のフラグメントも単独で検証可能: state={state}");
                }
            }
            Err(e) => {
                println!("  最後のフラグメントのみ: ERROR = {e}");
            }
        }
    }

    // 中間のフラグメントだけで検証（3個以上ある場合）
    if segments.len() >= 3 {
        let mid_idx = segments.len() / 2;
        let mid_seg_data = std::fs::read(&segments[mid_idx])
            .map_err(|e| format!("中間セグメント読み込み失敗: {e}"))?;

        let mut init_cursor = Cursor::new(&init_data);
        let mut seg_cursor = Cursor::new(&mid_seg_data);

        let ctx = c2pa::Context::default();
        match c2pa::Reader::from_context(ctx)
            .with_fragment("video/mp4", &mut init_cursor, &mut seg_cursor)
        {
            Ok(reader) => {
                let seg_name = segments[mid_idx]
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                let label = format!("中間フラグメント ({seg_name})");
                let (state, _codes) = report_validation_details(&reader, &label);
                println!("  → 中間フラグメントも単独で検証可能: state={state}");
            }
            Err(e) => {
                println!("  中間フラグメント: ERROR = {e}");
            }
        }
    }

    // 順序逆転テスト: 最後のフラグメントを先に検証し、次に最初を検証
    if segments.len() >= 2 {
        println!("\n  順序逆転テスト（最後→最初の順で検証）:");
        let mut reverse_ok = true;

        for idx in [segments.len() - 1, 0] {
            let seg_data = std::fs::read(&segments[idx])
                .map_err(|e| format!("セグメント読み込み失敗: {e}"))?;
            let seg_name = segments[idx]
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            let mut init_cursor = Cursor::new(&init_data);
            let mut seg_cursor = Cursor::new(&seg_data);

            let ctx = c2pa::Context::default();
            match c2pa::Reader::from_context(ctx)
                .with_fragment("video/mp4", &mut init_cursor, &mut seg_cursor)
            {
                Ok(reader) => {
                    let label = format!("逆順:{seg_name}");
                    let (state, codes) = report_validation_details(&reader, &label);
                    if has_non_cert_errors(&codes) {
                        println!("    ✗ {seg_name}: cert以外のエラーあり");
                        reverse_ok = false;
                    } else {
                        println!("    ✓ {seg_name}: state={state}, エラーなし");
                    }
                }
                Err(e) => {
                    println!("    ✗ {seg_name}: ERROR = {e}");
                    reverse_ok = false;
                }
            }
        }

        if reverse_ok {
            println!("  → 順序逆転でも検証成功 ✓（フラグメント間に順序依存なし）");
        } else {
            println!("  → 順序逆転で問題発生 ⚠");
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
    let raw_dir = work_dir.join("raw");
    let signed_base = work_dir.join("signed");

    let _ = std::fs::remove_dir_all(&work_dir);
    std::fs::create_dir_all(&raw_dir).expect("raw ディレクトリ作成失敗");

    // ======================================================================
    // Phase 1: CMAF セグメント生成
    // ======================================================================

    println!("=== Phase 1: CMAF セグメント生成 ===\n");
    if let Err(e) = generate_cmaf_segments(&raw_dir, 20) {
        eprintln!("  FATAL: {e}");
        eprintln!("  ffmpeg による CMAF セグメント生成に失敗しました。");
        std::process::exit(1);
    }
    println!("  => Phase 1 完了\n");

    // ======================================================================
    // Phase 2: C2PA フラグメント署名（EphemeralSigner ラウンドトリップ）
    // ======================================================================

    println!("=== Phase 2: C2PA フラグメント署名 ===\n");
    let signed_dir = match sign_fragments(&raw_dir, &signed_base) {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("  FATAL: {e}");
            eprintln!("  C2PA フラグメント署名に失敗しました。");
            std::process::exit(1);
        }
    };
    println!("  => Phase 2 完了\n");

    // ======================================================================
    // Phase 3: ラウンドトリップ検証（署名→検証、全フラグメント）
    // ======================================================================

    println!("=== Phase 3: ラウンドトリップ検証（全フラグメント） ===");
    let roundtrip_results = match verify_fragments_roundtrip(&signed_dir) {
        Ok(results) => results,
        Err(e) => {
            eprintln!("  FATAL: {e}");
            std::process::exit(1);
        }
    };

    // ラウンドトリップ結果の判定: cert 信頼性以外のエラーがないこと
    let mut roundtrip_clean = true;
    for (seg_name, state, codes, _hash) in &roundtrip_results {
        let non_cert = non_cert_error_codes(codes);
        if non_cert.is_empty() {
            println!("  ✓ {seg_name}: state={state}, cert信頼性以外のエラーなし");
        } else {
            println!("  ✗ {seg_name}: state={state}, cert以外のエラー: {non_cert:?}");
            roundtrip_clean = false;
        }
    }

    // Manifest JSON hash の全フラグメント一致を検証
    let hashes: Vec<&String> = roundtrip_results
        .iter()
        .filter(|(_, _, _, h)| !h.is_empty())
        .map(|(_, _, _, h)| h)
        .collect();
    let json_hash_consistent = if hashes.len() >= 2 {
        let first = hashes[0];
        let all_match = hashes.iter().all(|h| *h == first);
        if all_match {
            println!(
                "  ✓ Manifest JSON hash: 全 {} フラグメントで一致 (sha256:{}...)",
                hashes.len(),
                &first[..16]
            );
        } else {
            println!("  ✗ Manifest JSON hash: フラグメント間で不一致!");
            for (name, _, _, hash) in &roundtrip_results {
                println!("    {name}: {hash}");
            }
        }
        all_match
    } else {
        println!("  ⚠ Manifest JSON hash: 比較対象不足 ({} フラグメント)", hashes.len());
        true // 1個以下なら比較不可、pass 扱い
    };

    roundtrip_clean = roundtrip_clean && json_hash_consistent;

    if roundtrip_clean {
        println!("\n  ✓ Phase 3 PASS: 全フラグメントで cert 信頼性以外のエラーなし + JSON hash 一致");
        println!("    → EphemeralSigner (v0.84) ラウンドトリップが正しく動作");
    } else {
        println!("\n  ✗ Phase 3 FAIL");
        if !json_hash_consistent {
            println!("    → Manifest JSON hash がフラグメント間で不一致");
        }
    }
    println!();

    // ======================================================================
    // Phase 4: フラグメント改ざん検知テスト
    // ======================================================================

    println!("=== Phase 4: フラグメント改ざん検知テスト ===");

    // 4a: セグメント改ざん
    let tamper_seg_ok = match verify_fragment_tampered(&signed_dir) {
        Ok((tamper_state, tamper_codes)) => {
            let tamper_non_cert = non_cert_error_codes(&tamper_codes);

            // 正常時のコードと比較（roundtrip_results の tuple は 4 要素）
            let empty = vec![];
            let baseline_codes = if !roundtrip_results.is_empty() {
                &roundtrip_results[0].2
            } else {
                &empty
            };
            let baseline_non_cert = non_cert_error_codes(baseline_codes);

            println!("\n  改ざん検知判定（セグメント）:");
            println!("    正常時の non-cert エラー数: {}", baseline_non_cert.len());
            println!("    改ざん後の non-cert エラー数: {}", tamper_non_cert.len());

            if tamper_non_cert.len() > baseline_non_cert.len() {
                // 改ざんにより新しいエラーコードが増えた
                let new_errors: Vec<&&String> = tamper_non_cert
                    .iter()
                    .filter(|c| !baseline_non_cert.contains(c))
                    .collect();
                println!("    新しいエラーコード: {:?}", new_errors);
                println!("    → 改ざんが正しく検出された ✓");
                true
            } else if tamper_state == "Error" {
                println!("    → Reader 構築失敗（データ構造破壊で検知） ✓");
                true
            } else {
                println!("    ⚠ 改ざんが検出されていない可能性");
                println!("    tamper_state={tamper_state}, tamper_codes={tamper_codes:?}");
                false
            }
        }
        Err(e) => {
            eprintln!("  改ざんテストエラー: {e}");
            false
        }
    };

    // 4b: init.mp4 改ざん（複数位置テスト）
    let mut init_tamper_detected = 0u32;
    let mut init_tamper_total = 0u32;
    match verify_init_tampered(&signed_dir) {
        Ok(init_results) => {
            println!("\n  init.mp4 改ざん検知サマリー:");
            for (desc, state, codes) in &init_results {
                init_tamper_total += 1;
                let detected = has_non_cert_errors(codes) || state == "Error";
                if detected {
                    init_tamper_detected += 1;
                }
                println!(
                    "    {}: {}",
                    desc,
                    if detected { "検出 ✓" } else { "未検出 ⚠" }
                );
            }
        }
        Err(e) => {
            eprintln!("  init.mp4 改ざんテストエラー: {e}");
        }
    }

    let tamper_init_ok = init_tamper_detected > 0;

    if tamper_seg_ok && tamper_init_ok {
        println!("\n  ✓ Phase 4 PASS: フラグメント改ざん検出、init.mp4 改ざんも部分検出");
    } else if tamper_seg_ok {
        println!("\n  △ Phase 4 PARTIAL: セグメント改ざんは検出、init.mp4 改ざんは全位置で未検出");
        println!("    → init.mp4 の JUMBF 領域は BMFF ハッシュの対象外の可能性");
        println!("    → セグメントのコンテンツ改ざん検知は正常に機能");
    } else {
        println!("\n  ✗ Phase 4 FAIL: 改ざん検出に失敗");
    }
    println!();

    // ======================================================================
    // Phase 5: 部分フラグメント検証テスト
    // ======================================================================

    println!("=== Phase 5: 部分フラグメント検証テスト ===");
    if let Err(e) = verify_partial_fragments(&signed_dir) {
        eprintln!("  ERROR: {e}");
    }
    println!("  => Phase 5 完了\n");

    // ======================================================================
    // 総合判定
    // ======================================================================

    println!("========================================================");
    println!("  総合判定");
    println!("========================================================\n");

    let seg_count = roundtrip_results.len();
    println!("  Phase 1: CMAF セグメント生成     — PASS ({seg_count} segments)");
    println!("  Phase 2: C2PA フラグメント署名    — PASS (EphemeralSigner)");
    println!(
        "  Phase 3: ラウンドトリップ検証      — {}",
        if roundtrip_clean { "PASS" } else { "FAIL" }
    );
    println!(
        "  Phase 4a: セグメント改ざん検知     — {}",
        if tamper_seg_ok { "PASS" } else { "FAIL" }
    );
    println!(
        "  Phase 4b: init.mp4 改ざん検知      — {} ({}/{}位置で検出)",
        if tamper_init_ok { "PARTIAL" } else { "未検出" },
        init_tamper_detected,
        init_tamper_total,
    );
    println!("  Phase 5: 部分フラグメント検証     — 情報収集（Pass/Fail なし）");

    let overall = roundtrip_clean && tamper_seg_ok;
    println!();
    if overall {
        println!("  ★ OVERALL: PASS");
        println!("    c2pa-rs v0.84 の with_fragment API はフラグメント署名・検証に");
        println!("    正しく対応している。改ざん検知も機能する。");
    } else {
        println!("  ★ OVERALL: FAIL");
        if !roundtrip_clean {
            println!("    → ラウンドトリップで cert 以外のエラーあり（要調査）");
        }
        if !tamper_seg_ok {
            println!("    → フラグメント改ざんが検出されない（重大な問題）");
        }
    }
    println!();
    println!("  作業ディレクトリ: {}", work_dir.display());
}
