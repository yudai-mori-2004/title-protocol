//! # SP1 Host: Attestation Document 検証の proof 生成・検証
//!
//! 仕様書 §6.2 — Solana Extension 準備
//!
//! 5つのフェーズで検証を実施:
//! 1. フィクスチャ読み込みとパース確認
//! 2. SP1 Execute（proof なし、サイクル数計測）
//! 3. SP1 Proof 生成（core mode）
//! 4. Proof 検証と公開値の確認
//! 5. 改ざん検知テスト

use sha2::{Digest, Sha256};
use sp1_sdk::{include_elf, CpuProver, Elf, ProveRequest, Prover, ProverClient, ProvingKey, SP1Stdin};
use std::collections::BTreeMap;
use std::time::Instant;

/// Guest プログラムの ELF バイナリ（build.rs で自動ビルド）
const ATTESTATION_ELF: Elf = include_elf!("attestation-program");

/// ホスト側で Attestation Document をパースし、期待値を抽出する。
/// Guest の出力と照合するために使用。
#[derive(Debug)]
struct AttestationInfo {
    module_id: String,
    timestamp: u64,
    pcr0: Vec<u8>,
    user_data: Option<Vec<u8>>,
    public_key: Option<Vec<u8>>,
}

/// serde_cbor を使って Attestation Document をホスト側でパースする。
/// COSE_Sign1 の payload 部分を抽出し、CBOR マップから各フィールドを取得。
fn parse_attestation_on_host(doc_bytes: &[u8]) -> Result<AttestationInfo, String> {
    // COSE_Sign1 は CBOR Tag 18 の配列 [protected, unprotected, payload, signature]
    let value: serde_cbor::Value =
        serde_cbor::from_slice(doc_bytes).map_err(|e| format!("CBOR パース失敗: {}", e))?;

    // Tagged value の中身を取得
    let array = match &value {
        serde_cbor::Value::Tag(18, inner) => match inner.as_ref() {
            serde_cbor::Value::Array(arr) => arr,
            _ => return Err("COSE_Sign1 の中身が配列ではありません".into()),
        },
        serde_cbor::Value::Array(arr) => arr,
        _ => return Err("COSE_Sign1 のパースに失敗".into()),
    };

    if array.len() < 4 {
        return Err(format!("COSE_Sign1 配列の要素数不足: {}", array.len()));
    }

    // payload (index 2) を取得
    let payload_bytes = match &array[2] {
        serde_cbor::Value::Bytes(b) => b,
        _ => return Err("payload がバイト列ではありません".into()),
    };

    // payload を CBOR マップとしてパース
    let payload: serde_cbor::Value = serde_cbor::from_slice(payload_bytes)
        .map_err(|e| format!("payload CBOR パース失敗: {}", e))?;

    let map = match &payload {
        serde_cbor::Value::Map(m) => m,
        _ => return Err("payload がマップではありません".into()),
    };

    // 各フィールドを抽出
    let module_id = get_text(map, "module_id")?;
    let timestamp = get_integer(map, "timestamp")?;

    // PCR0 を取得
    let pcrs_val = map
        .get(&serde_cbor::Value::Text("pcrs".into()))
        .ok_or("pcrs フィールドが見つかりません")?;
    let pcr0 = match pcrs_val {
        serde_cbor::Value::Map(pcr_map) => {
            let key = serde_cbor::Value::Integer(0);
            match pcr_map.get(&key) {
                Some(serde_cbor::Value::Bytes(b)) => b.clone(),
                _ => return Err("PCR0 が見つかりません".into()),
            }
        }
        _ => return Err("pcrs がマップではありません".into()),
    };

    let user_data = get_optional_bytes(map, "user_data");
    let public_key = get_optional_bytes(map, "public_key");

    Ok(AttestationInfo {
        module_id,
        timestamp,
        pcr0,
        user_data,
        public_key,
    })
}

fn get_text(
    map: &BTreeMap<serde_cbor::Value, serde_cbor::Value>,
    key: &str,
) -> Result<String, String> {
    match map.get(&serde_cbor::Value::Text(key.into())) {
        Some(serde_cbor::Value::Text(s)) => Ok(s.clone()),
        _ => Err(format!("フィールド '{}' が見つからないかテキストではありません", key)),
    }
}

fn get_integer(
    map: &BTreeMap<serde_cbor::Value, serde_cbor::Value>,
    key: &str,
) -> Result<u64, String> {
    match map.get(&serde_cbor::Value::Text(key.into())) {
        Some(serde_cbor::Value::Integer(i)) => Ok(*i as u64),
        _ => Err(format!(
            "フィールド '{}' が見つからないか整数ではありません",
            key
        )),
    }
}

fn get_optional_bytes(
    map: &BTreeMap<serde_cbor::Value, serde_cbor::Value>,
    key: &str,
) -> Option<Vec<u8>> {
    match map.get(&serde_cbor::Value::Text(key.into())) {
        Some(serde_cbor::Value::Bytes(b)) => Some(b.clone()),
        _ => None,
    }
}

/// Guest の公開値を読み取り、期待値と照合する。
fn verify_public_values(
    public_values: &mut sp1_sdk::SP1PublicValues,
    expected: &AttestationInfo,
) -> Result<(), String> {
    // module_id
    let module_id: String = public_values.read();
    if module_id != expected.module_id {
        return Err(format!(
            "module_id 不一致: got={}, expected={}",
            module_id, expected.module_id
        ));
    }
    println!("  module_id: {} ✓", module_id);

    // timestamp
    let timestamp: u64 = public_values.read();
    if timestamp != expected.timestamp {
        return Err(format!(
            "timestamp 不一致: got={}, expected={}",
            timestamp, expected.timestamp
        ));
    }
    println!("  timestamp: {} ✓", timestamp);

    // PCR0 (48 bytes)
    let mut pcr0 = vec![0u8; 48];
    public_values.read_slice(&mut pcr0);
    if pcr0 != expected.pcr0 {
        return Err("PCR0 不一致".into());
    }
    println!("  PCR0: {}... ✓", hex::encode(&pcr0[..8]));

    // user_data
    let has_user_data: u8 = public_values.read();
    if has_user_data == 1 {
        let mut ud_hash = vec![0u8; 32];
        public_values.read_slice(&mut ud_hash);
        if let Some(ref expected_ud) = expected.user_data {
            let expected_hash = Sha256::digest(expected_ud);
            if ud_hash != &expected_hash[..] {
                return Err("user_data ハッシュ不一致".into());
            }
            println!("  user_data hash: {}... ✓", hex::encode(&ud_hash[..8]));
        } else {
            return Err("Guest は user_data ありだがホスト側ではなし".into());
        }
    } else {
        if expected.user_data.is_some() {
            return Err("Guest は user_data なしだがホスト側ではあり".into());
        }
        println!("  user_data: なし ✓");
    }

    // public_key
    let has_public_key: u8 = public_values.read();
    if has_public_key == 1 {
        let mut pk_hash = vec![0u8; 32];
        public_values.read_slice(&mut pk_hash);
        if let Some(ref expected_pk) = expected.public_key {
            let expected_hash = Sha256::digest(expected_pk);
            if pk_hash != &expected_hash[..] {
                return Err("public_key ハッシュ不一致".into());
            }
            println!("  public_key hash: {}... ✓", hex::encode(&pk_hash[..8]));
        } else {
            return Err("Guest は public_key ありだがホスト側ではなし".into());
        }
    } else {
        if expected.public_key.is_some() {
            return Err("Guest は public_key なしだがホスト側ではあり".into());
        }
        println!("  public_key: なし ✓");
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║  Sandbox C: SP1 zkVM Attestation Document 検証         ║");
    println!("╚══════════════════════════════════════════════════════════╝");
    println!();

    // ── Phase 1: フィクスチャ読み込み ──
    println!("━━━ Phase 1: フィクスチャ読み込みとパース確認 ━━━");

    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("attestation_1.report");

    let doc_bytes = std::fs::read(&fixture_path).unwrap_or_else(|e| {
        panic!(
            "フィクスチャファイルの読み込みに失敗: {} ({})",
            fixture_path.display(),
            e
        );
    });
    println!("  フィクスチャ: {}", fixture_path.display());
    println!("  サイズ: {} bytes", doc_bytes.len());

    // ホスト側でパースして期待値を取得
    let expected = parse_attestation_on_host(&doc_bytes).expect("ホスト側パースに失敗");
    println!("  module_id: {}", expected.module_id);
    println!("  timestamp: {}", expected.timestamp);
    println!("  PCR0: {}...", hex::encode(&expected.pcr0[..8]));
    println!(
        "  user_data: {} bytes",
        expected
            .user_data
            .as_ref()
            .map(|d| d.len())
            .unwrap_or(0)
    );
    println!(
        "  public_key: {} bytes",
        expected
            .public_key
            .as_ref()
            .map(|d| d.len())
            .unwrap_or(0)
    );
    println!("  Phase 1: PASS ✓");
    println!();

    // ── Phase 2: SP1 Execute（サイクル数計測） ──
    println!("━━━ Phase 2: SP1 Execute（proof なし、サイクル数計測） ━━━");

    let client: CpuProver = ProverClient::builder().cpu().build().await;

    let mut stdin = SP1Stdin::new();
    stdin.write_vec(doc_bytes.clone());
    stdin.write(&0u32); // trusted_certs_prefix_len = 0（全チェーン検証）

    let execute_start = Instant::now();
    let (mut public_values, report) = client
        .execute(ATTESTATION_ELF, stdin.clone())
        .await
        .expect("SP1 Execute に失敗");
    let execute_duration = execute_start.elapsed();

    let total_cycles = report.total_instruction_count();
    println!("  実行時間: {:.2}s", execute_duration.as_secs_f64());
    println!("  総サイクル数: {} ({:.1}M)", total_cycles, total_cycles as f64 / 1_000_000.0);

    // 公開値の検証
    println!("  公開値の検証:");
    verify_public_values(&mut public_values, &expected).expect("公開値の検証に失敗");
    println!("  Phase 2: PASS ✓");
    println!();

    // ── Phase 3: SP1 Proof 生成（core mode） ──
    println!("━━━ Phase 3: SP1 Proof 生成（core mode） ━━━");

    let pk = client.setup(ATTESTATION_ELF).await.unwrap();

    let prove_start = Instant::now();
    let proof = client
        .prove(&pk, stdin.clone())
        .core()
        .await
        .expect("Proof 生成に失敗");
    let prove_duration = prove_start.elapsed();

    // Proof サイズ: public_values + proof 構造全体の概算
    let proof_pv_size = proof.public_values.as_slice().len();
    println!("  Proof 生成時間: {:.2}s", prove_duration.as_secs_f64());
    println!(
        "  公開値サイズ: {} bytes",
        proof_pv_size
    );
    println!("  Phase 3: PASS ✓");
    println!();

    // ── Phase 4: Proof 検証と公開値の再確認 ──
    println!("━━━ Phase 4: Proof 検証と公開値の再確認 ━━━");

    let verify_start = Instant::now();
    client
        .verify(&proof, pk.verifying_key(), None)
        .expect("Proof 検証に失敗");
    let verify_duration = verify_start.elapsed();
    println!("  Proof 検証: PASS ✓");
    println!("  検証時間: {:.4}s", verify_duration.as_secs_f64());

    // Proof から公開値を再読み取り
    let mut proof_public_values = proof.public_values.clone();
    println!("  公開値の再検証:");
    verify_public_values(&mut proof_public_values, &expected)
        .expect("Proof 公開値の検証に失敗");
    println!("  Phase 4: PASS ✓");
    println!();

    // ── Phase 5: 改ざん検知テスト ──
    println!("━━━ Phase 5: 改ざん検知テスト ━━━");

    // テスト 5a: ペイロード改ざん（中間バイト反転）
    println!("  テスト 5a: ペイロード中間バイト改ざん");
    {
        let mut tampered = doc_bytes.clone();
        let mid = tampered.len() / 2;
        tampered[mid] ^= 0xFF;
        println!("    改ざん位置: offset={} ({}バイト中)", mid, tampered.len());

        let mut tampered_stdin = SP1Stdin::new();
        tampered_stdin.write_vec(tampered);
        tampered_stdin.write(&0u32);

        let result = client
            .execute(ATTESTATION_ELF, tampered_stdin)
            .await;

        match result {
            Err(e) => {
                let err_str = format!("{}", e);
                // エラーメッセージの先頭部分のみ表示（長すぎる場合がある）
                let display_err = if err_str.len() > 200 {
                    format!("{}...", &err_str[..200])
                } else {
                    err_str
                };
                println!("    結果: Guest パニック（期待通り）✓");
                println!("    エラー: {}", display_err);
            }
            Ok(_) => {
                println!("    結果: 改ざんが検出されなかった ✗");
                println!("    ※ 改ざん位置がパディング領域だった可能性あり");
            }
        }
    }

    // テスト 5b: 署名改ざん（末尾バイト反転）
    println!("  テスト 5b: 署名領域改ざん（末尾バイト反転）");
    {
        let mut tampered = doc_bytes.clone();
        let pos = tampered.len() - 10;
        tampered[pos] ^= 0xFF;
        println!("    改ざん位置: offset={} ({}バイト中)", pos, tampered.len());

        let mut tampered_stdin = SP1Stdin::new();
        tampered_stdin.write_vec(tampered);
        tampered_stdin.write(&0u32);

        let result = client
            .execute(ATTESTATION_ELF, tampered_stdin)
            .await;

        match result {
            Err(e) => {
                let err_str = format!("{}", e);
                let display_err = if err_str.len() > 200 {
                    format!("{}...", &err_str[..200])
                } else {
                    err_str
                };
                println!("    結果: Guest パニック（期待通り）✓");
                println!("    エラー: {}", display_err);
            }
            Ok(_) => {
                println!("    結果: 改ざんが検出されなかった ✗");
            }
        }
    }

    // テスト 5c: 先頭バイト改ざん（COSE 構造破壊）
    println!("  テスト 5c: 先頭バイト改ざん（COSE 構造破壊）");
    {
        let mut tampered = doc_bytes.clone();
        tampered[0] ^= 0xFF;
        println!("    改ざん位置: offset=0 ({}バイト中)", tampered.len());

        let mut tampered_stdin = SP1Stdin::new();
        tampered_stdin.write_vec(tampered);
        tampered_stdin.write(&0u32);

        let result = client
            .execute(ATTESTATION_ELF, tampered_stdin)
            .await;

        match result {
            Err(e) => {
                let err_str = format!("{}", e);
                let display_err = if err_str.len() > 200 {
                    format!("{}...", &err_str[..200])
                } else {
                    err_str
                };
                println!("    結果: Guest パニック（期待通り）✓");
                println!("    エラー: {}", display_err);
            }
            Ok(_) => {
                println!("    結果: 改ざんが検出されなかった ✗");
            }
        }
    }

    println!("  Phase 5: 完了");
    println!();

    // ── 総合結果 ──
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║  総合結果                                              ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  Phase 1: フィクスチャパース        PASS ✓             ║");
    println!("║  Phase 2: SP1 Execute              PASS ✓             ║");
    println!("║  Phase 3: Proof 生成               PASS ✓             ║");
    println!("║  Phase 4: Proof 検証               PASS ✓             ║");
    println!("║  Phase 5: 改ざん検知               完了               ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  メトリクス                                            ║");
    println!(
        "║  総サイクル数:   {:>12} ({:.1}M)            ║",
        total_cycles,
        total_cycles as f64 / 1_000_000.0
    );
    println!(
        "║  Execute 時間:   {:>8.2}s                        ║",
        execute_duration.as_secs_f64()
    );
    println!(
        "║  Proof 生成時間: {:>8.2}s                        ║",
        prove_duration.as_secs_f64()
    );
    println!(
        "║  公開値サイズ:   {:>8} bytes                   ║",
        proof_pv_size
    );
    println!(
        "║  Proof 検証時間: {:>8.4}s                      ║",
        verify_duration.as_secs_f64()
    );
    println!("╚══════════════════════════════════════════════════════════╝");
}
