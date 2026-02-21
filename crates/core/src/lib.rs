//! # Title Protocol Core
//!
//! 仕様書セクション2で定義されるC2PA検証と来歴グラフ構築を実装する。
//!
//! ## 処理フロー
//! 1. C2PA署名チェーンを検証する
//! 2. Active Manifestの署名からcontent_hashを計算する
//! 3. Manifestに含まれる素材情報を再帰的に抽出する
//! 4. 来歴グラフ（ノードとエッジ）を構築する

mod jumbf;

use std::io::Cursor;

use c2pa::validation_results::ValidationState;
use sha2::{Digest, Sha256};
use title_types::{GraphLink, GraphNode};

/// Coreモジュールのエラー型
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// C2PA検証エラー
    #[error("C2PA検証に失敗しました: {0}")]
    C2paVerificationFailed(String),
    /// コンテンツハッシュ抽出エラー
    #[error("コンテンツハッシュの抽出に失敗しました: {0}")]
    ContentHashExtractionFailed(String),
    /// 来歴グラフ構築エラー
    #[error("来歴グラフの構築に失敗しました: {0}")]
    GraphBuildFailed(String),
    /// グラフサイズ超過エラー
    #[error("来歴グラフのサイズが上限を超えました: {nodes_and_links} > {max}")]
    GraphSizeExceeded {
        /// 実際のノード+エッジ数
        nodes_and_links: usize,
        /// 上限値
        max: usize,
    },
}

/// JUMBF署名データの最大サイズ（16 MiB）。
/// これを超えるCBORデータは不正とみなす。
const MAX_SIGNATURE_SIZE: u64 = 16 * 1024 * 1024;

/// ingredient再帰処理の最大深度。
/// スタックオーバーフロー防止のため制限する。
const MAX_INGREDIENT_DEPTH: usize = 32;

/// C2PA検証の結果。
/// 仕様書 §2.1
#[derive(Debug)]
pub struct C2paVerificationResult {
    /// 検証が成功したか
    pub is_valid: bool,
    /// Active Manifestの署名バイト列
    pub active_manifest_signature: Vec<u8>,
    /// コンテンツのMIMEタイプ
    pub content_type: String,
    /// TSAタイムスタンプ（存在する場合）
    pub tsa_timestamp: Option<u64>,
    /// TSA公開鍵のSHA-256ハッシュ（存在する場合）
    pub tsa_pubkey_hash: Option<String>,
    /// TSAトークンデータ（存在する場合）
    pub tsa_token_data: Option<Vec<u8>>,
}

/// 来歴グラフ（有向非巡回グラフ）。
/// 仕様書 §2.2
#[derive(Debug)]
pub struct ProvenanceGraph {
    /// グラフのノード一覧（content_hashで識別）
    pub nodes: Vec<GraphNode>,
    /// グラフのリンク一覧（素材→派生の関係）
    pub links: Vec<GraphLink>,
}

/// content_hashを「0x」プレフィックス付きhex文字列に変換する。
fn format_content_hash(hash: &[u8; 32]) -> String {
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    format!("0x{hex}")
}

/// C2PAマニフェストからTSAタイムスタンプ情報を抽出する。
/// 仕様書 §2.4 重複の解決
///
/// マニフェストの `time()` メソッドでRFC 3339形式のタイムスタンプを取得し、
/// Unix epoch秒に変換する。`signature_info().issuer()` のSHA-256ハッシュを
/// TSA公開鍵ハッシュの代替として使用する。
fn extract_tsa_info(manifest: &c2pa::Manifest) -> (Option<u64>, Option<String>) {
    let time_str = match manifest.time() {
        Some(t) => t,
        None => return (None, None),
    };

    // RFC 3339 → Unix epoch秒
    let timestamp = parse_rfc3339_to_epoch(&time_str);
    if timestamp.is_none() {
        return (None, None);
    }

    // issuerのSHA-256ハッシュ
    let pubkey_hash = manifest
        .signature_info()
        .and_then(|si| si.issuer.as_ref())
        .map(|issuer: &String| {
            let hash = Sha256::digest(issuer.as_bytes());
            hex::encode(hash)
        });

    (timestamp, pubkey_hash)
}

/// RFC 3339形式のタイムスタンプをUnix epoch秒に変換する。
/// 仕様書 §2.4
///
/// 形式例: "2024-01-15T10:30:45Z", "2024-01-15T10:30:45.123456Z"
fn parse_rfc3339_to_epoch(s: &str) -> Option<u64> {
    // 基本的なRFC 3339パーサ: YYYY-MM-DDThh:mm:ss[.frac]Z
    let s = s.trim();
    let date_time = s.strip_suffix('Z').or_else(|| {
        // +00:00 形式のオフセット（UTCのみサポート）
        s.strip_suffix("+00:00")
    })?;

    let (date_part, time_part) = date_time.split_once('T')?;
    let parts: Vec<&str> = date_part.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let year: i64 = parts[0].parse().ok()?;
    let month: i64 = parts[1].parse().ok()?;
    let day: i64 = parts[2].parse().ok()?;

    let time_no_frac = time_part.split('.').next()?;
    let tparts: Vec<&str> = time_no_frac.split(':').collect();
    if tparts.len() != 3 {
        return None;
    }
    let hour: i64 = tparts[0].parse().ok()?;
    let min: i64 = tparts[1].parse().ok()?;
    let sec: i64 = tparts[2].parse().ok()?;

    // 簡易Unix epoch計算（うるう秒は無視）
    // days_from_epoch using a simplified algorithm
    let m = if month <= 2 { month + 9 } else { month - 3 };
    let y = if month <= 2 { year - 1 } else { year };
    let days = 365 * y + y / 4 - y / 100 + y / 400 + (m * 306 + 5) / 10 + day - 1 - 719468;
    let epoch = days * 86400 + hour * 3600 + min * 60 + sec;

    if epoch >= 0 {
        Some(epoch as u64)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// 重複解決 (仕様書 §2.4)
// ---------------------------------------------------------------------------

/// 重複解決用のトークンレコード。
/// 仕様書 §2.4
#[derive(Debug, Clone)]
pub struct TokenRecord {
    /// トークンの一意識別子
    pub id: String,
    /// TSAタイムスタンプ（存在する場合、Unix epoch秒）
    pub tsa_timestamp: Option<u64>,
    /// TSA公開鍵ハッシュ（hex文字列）
    pub tsa_pubkey_hash: Option<String>,
    /// Solana block time（登録時刻、Unix epoch秒）
    pub solana_block_time: u64,
    /// トークンがBurn済みかどうか
    pub is_burned: bool,
}

/// 同一content_hashに対する複数の権利トークンから正当な所有者を決定する。
/// 仕様書 §2.4 重複の解決
///
/// 判定ロジック:
/// 1. Burn済みトークンを除外する
/// 2. 各トークンの「作成時刻」を決定する
///    - 信頼できるTSAタイムスタンプを持つ → TSAタイムスタンプを使用
///    - 持たない → Solana block time（登録時刻）を使用
/// 3. 作成時刻が最古のトークンを選択する
/// 4. 同一作成時刻の場合、登録時刻（Solana block time）が最古のものを選択する
///
/// `trusted_tsa_keys` が指定された場合、TSA公開鍵ハッシュがリストに含まれるもののみ
/// TSAタイムスタンプを信頼する。リストが空の場合、全てのTSAを信頼する。
pub fn resolve_duplicate<'a>(
    tokens: &'a [TokenRecord],
    trusted_tsa_keys: &[String],
) -> Option<&'a TokenRecord> {
    let active: Vec<&TokenRecord> = tokens.iter().filter(|t| !t.is_burned).collect();

    if active.is_empty() {
        return None;
    }

    active.into_iter().min_by(|a, b| {
        let a_time = effective_creation_time(a, trusted_tsa_keys);
        let b_time = effective_creation_time(b, trusted_tsa_keys);

        a_time
            .cmp(&b_time)
            .then(a.solana_block_time.cmp(&b.solana_block_time))
    })
}

/// トークンの有効な作成時刻を決定する。
/// 仕様書 §2.4
fn effective_creation_time(token: &TokenRecord, trusted_tsa_keys: &[String]) -> u64 {
    if let Some(tsa_ts) = token.tsa_timestamp {
        // TSA公開鍵が信頼リストに含まれるか確認
        let is_trusted = if trusted_tsa_keys.is_empty() {
            // 信頼リストが空の場合は全てのTSAを信頼
            true
        } else if let Some(ref hash) = token.tsa_pubkey_hash {
            trusted_tsa_keys.contains(hash)
        } else {
            false
        };

        if is_trusted {
            return tsa_ts;
        }
    }

    // TSAなし or 信頼できないTSA → Solana block timeで代用
    token.solana_block_time
}

/// JUMBFデータからマニフェストの署名バイト列を取得する。
/// 仕様書 §2.1
fn extract_manifest_signature(
    content_bytes: &[u8],
    mime_type: &str,
    manifest_label: &str,
) -> Result<Vec<u8>, CoreError> {
    let jumbf_data = c2pa::jumbf_io::load_jumbf_from_memory(mime_type, content_bytes)
        .map_err(|e| CoreError::ContentHashExtractionFailed(format!("JUMBF抽出エラー: {e}")))?;
    jumbf::extract_signature_from_jumbf(&jumbf_data, manifest_label)
}

/// C2PA署名チェーンを検証し、結果を返す。
/// 仕様書 §2.1 コンテンツの識別子
///
/// TEEはC2PA署名チェーンの正当性を検証し、以下を確認する:
/// - 署名チェーンの正当性（コンテンツの出自が改ざんされていない）
/// - コンテンツの同一性（Manifestが付与された時点から変更されていない）
pub fn verify_c2pa(
    content_bytes: &[u8],
    mime_type: &str,
) -> Result<C2paVerificationResult, CoreError> {
    // c2pa::Readerでコンテンツを読み込み・検証する
    let reader = c2pa::Reader::from_stream(mime_type, Cursor::new(content_bytes))
        .map_err(|e| CoreError::C2paVerificationFailed(format!("C2PAデータ読み込みエラー: {e}")))?;

    // 検証状態を確認
    let validation_state = reader.validation_state();
    let is_valid = matches!(
        validation_state,
        ValidationState::Valid | ValidationState::Trusted
    );

    // Active Manifestを取得
    let active_label = reader
        .active_label()
        .ok_or_else(|| {
            CoreError::C2paVerificationFailed("Active Manifestが見つかりません".to_string())
        })?
        .to_string();

    let manifest = reader.active_manifest().ok_or_else(|| {
        CoreError::C2paVerificationFailed("Active Manifestが見つかりません".to_string())
    })?;

    // MIMEタイプを取得
    let content_type = manifest
        .format()
        .unwrap_or(mime_type)
        .to_string();

    // JUMBFから署名バイト列を抽出
    let signature = extract_manifest_signature(content_bytes, mime_type, &active_label)?;

    // TSAタイムスタンプ抽出（仕様書 §2.4）
    // C2PAマニフェストにTSAタイムスタンプが含まれる場合、RFC 3339形式の文字列を
    // Unix epoch秒に変換する。issuerのSHA-256ハッシュをtsa_pubkey_hashとして使用する。
    let (tsa_timestamp, tsa_pubkey_hash) = extract_tsa_info(manifest);

    Ok(C2paVerificationResult {
        is_valid,
        active_manifest_signature: signature,
        content_type,
        tsa_timestamp,
        tsa_pubkey_hash,
        tsa_token_data: None,
    })
}

/// Active Manifestの署名からcontent_hashを抽出する。
/// 仕様書 §2.1 コンテンツの識別子: `content_hash = SHA-256(Active Manifestの署名)`
pub fn extract_content_hash(
    content_bytes: &[u8],
    mime_type: &str,
) -> Result<[u8; 32], CoreError> {
    let result = verify_c2pa(content_bytes, mime_type)?;
    Ok(title_crypto::content_hash_from_manifest_signature(
        &result.active_manifest_signature,
    ))
}

/// C2PAの素材情報を再帰的に抽出し、来歴グラフ（DAG）を構築する。
/// 仕様書 §2.2 来歴グラフの導出
///
/// 各ノードはcontent_hashで識別され、各エッジは
/// 「この素材がこのコンテンツの作成に使われた」という関係を表す。
/// グラフはC2PAデータから客観的・機械的に構築される。
pub fn build_provenance_graph(
    content_bytes: &[u8],
    mime_type: &str,
    max_graph_size: usize,
) -> Result<ProvenanceGraph, CoreError> {
    // Readerでコンテンツを読み込む
    let reader = c2pa::Reader::from_stream(mime_type, Cursor::new(content_bytes))
        .map_err(|e| CoreError::GraphBuildFailed(format!("C2PAデータ読み込みエラー: {e}")))?;

    let active_label = reader
        .active_label()
        .ok_or_else(|| {
            CoreError::GraphBuildFailed("Active Manifestが見つかりません".to_string())
        })?
        .to_string();

    let manifest = reader.active_manifest().ok_or_else(|| {
        CoreError::GraphBuildFailed("Active Manifestが見つかりません".to_string())
    })?;

    // JUMBFデータを読み込む
    let jumbf_data = c2pa::jumbf_io::load_jumbf_from_memory(mime_type, content_bytes)
        .map_err(|e| CoreError::GraphBuildFailed(format!("JUMBF抽出エラー: {e}")))?;

    // ルートノードのcontent_hashを算出
    let root_sig = jumbf::extract_signature_from_jumbf(&jumbf_data, &active_label)?;
    let root_hash = title_crypto::content_hash_from_manifest_signature(&root_sig);
    let root_hash_str = format_content_hash(&root_hash);

    let mut nodes = Vec::new();
    let mut links = Vec::new();

    // ルートノードを追加
    nodes.push(GraphNode {
        id: root_hash_str.clone(),
        node_type: "final".to_string(),
    });

    // ingredientsを再帰的に処理する（深度0から開始）
    process_ingredients(
        &reader,
        manifest,
        &jumbf_data,
        &root_hash_str,
        &mut nodes,
        &mut links,
        0,
    )?;

    // グラフサイズチェック
    let total = nodes.len() + links.len();
    if total > max_graph_size {
        return Err(CoreError::GraphSizeExceeded {
            nodes_and_links: total,
            max: max_graph_size,
        });
    }

    Ok(ProvenanceGraph { nodes, links })
}

/// ingredientのMIMEタイプをroleとして返す。
/// 仕様書 §2.2, §5.1 Step 4: roleはコンテンツ種別（例: "audio", "image/jpeg"）
fn ingredient_role(ingredient: &c2pa::Ingredient) -> String {
    ingredient.format().unwrap_or("unknown").to_string()
}

/// マニフェストのingredientsを処理し、ノードとリンクをグラフに追加する。
/// 仕様書 §2.2: ingredient情報を再帰的に抽出
///
/// C2PAマニフェストを持つingredientのみグラフに含める。
/// マニフェストを持たない or 署名を抽出できないingredientは
/// フォールバックIDを使わず、スキップする（安全性優先）。
fn process_ingredients(
    reader: &c2pa::Reader,
    manifest: &c2pa::Manifest,
    jumbf_data: &[u8],
    parent_hash_str: &str,
    nodes: &mut Vec<GraphNode>,
    links: &mut Vec<GraphLink>,
    depth: usize,
) -> Result<(), CoreError> {
    if depth > MAX_INGREDIENT_DEPTH {
        return Err(CoreError::GraphBuildFailed(format!(
            "ingredient再帰の深さが上限({MAX_INGREDIENT_DEPTH})を超えました"
        )));
    }

    for ingredient in manifest.ingredients() {
        let role = ingredient_role(ingredient);

        // C2PAマニフェストを持つingredientのみ処理する。
        // マニフェストがないingredientは来歴グラフには含めない
        // （content_hashを算出できないため、フォールバックIDは使用しない）。
        let ingredient_label = match ingredient.active_manifest() {
            Some(label) => label,
            None => continue,
        };

        // ingredientのマニフェストの署名からcontent_hashを算出。
        // 署名が抽出できない場合もスキップする（不正データにフォールバックIDを与えない）。
        let sig = match jumbf::extract_signature_from_jumbf(jumbf_data, ingredient_label) {
            Ok(sig) => sig,
            Err(_) => continue,
        };

        let hash = title_crypto::content_hash_from_manifest_signature(&sig);
        let hash_str = format_content_hash(&hash);

        // 重複ノードを防ぐ
        if !nodes.iter().any(|n| n.id == hash_str) {
            nodes.push(GraphNode {
                id: hash_str.clone(),
                node_type: "ingredient".to_string(),
            });
        }

        links.push(GraphLink {
            source: hash_str.clone(),
            target: parent_hash_str.to_string(),
            role,
        });

        // ingredientのマニフェストが存在する場合、再帰的に処理
        if let Some(nested_manifest) = reader.get_manifest(ingredient_label) {
            process_ingredients(
                reader,
                nested_manifest,
                jumbf_data,
                &hash_str,
                nodes,
                links,
                depth + 1,
            )?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CERTS: &[u8] = include_bytes!("../tests/fixtures/certs/chain.pem");
    const PRIVATE_KEY: &[u8] = include_bytes!("../tests/fixtures/certs/ee.key");
    const TEST_IMAGE: &[u8] = include_bytes!("../tests/fixtures/test.jpg");

    /// テスト用のsignerを作成する
    fn test_signer() -> Box<dyn c2pa::Signer> {
        c2pa::create_signer::from_keys(CERTS, PRIVATE_KEY, c2pa::SigningAlg::Ed25519, None)
            .unwrap()
    }

    /// テスト用のC2PA署名済みコンテンツを作成する
    fn create_signed_content(title: &str) -> Vec<u8> {
        use c2pa::Builder;
        use serde_json::json;

        let manifest_json = json!({
            "title": title,
            "format": "image/jpeg",
            "claim_generator_info": [{
                "name": "title-core-test",
                "version": "0.1.0"
            }]
        })
        .to_string();

        let mut builder = Builder::from_json(&manifest_json).unwrap();
        let signer = test_signer();

        let mut source = Cursor::new(TEST_IMAGE);
        let mut dest = Cursor::new(Vec::new());
        builder
            .sign(signer.as_ref(), "image/jpeg", &mut source, &mut dest)
            .unwrap();
        dest.into_inner()
    }

    /// テスト用のingredient付きC2PA署名済みコンテンツを作成する
    fn create_signed_content_with_ingredient(
        title: &str,
        ingredient_bytes: &[u8],
    ) -> Vec<u8> {
        use c2pa::Builder;
        use serde_json::json;

        let manifest_json = json!({
            "title": title,
            "format": "image/jpeg",
            "claim_generator_info": [{
                "name": "title-core-test",
                "version": "0.1.0"
            }]
        })
        .to_string();

        let mut builder = Builder::from_json(&manifest_json).unwrap();

        // ingredientとしてC2PA付きコンテンツを追加
        let ingredient_json = serde_json::json!({
            "title": "ingredient.jpg",
            "relationship": "inputTo"
        })
        .to_string();
        builder
            .add_ingredient_from_stream(
                &ingredient_json,
                "image/jpeg",
                &mut Cursor::new(ingredient_bytes),
            )
            .unwrap();

        let signer = test_signer();

        let mut source = Cursor::new(TEST_IMAGE);
        let mut dest = Cursor::new(Vec::new());
        builder
            .sign(signer.as_ref(), "image/jpeg", &mut source, &mut dest)
            .unwrap();
        dest.into_inner()
    }

    #[test]
    fn test_verify_c2pa_valid() {
        let signed = create_signed_content("test-valid.jpg");
        let result = verify_c2pa(&signed, "image/jpeg").unwrap();

        // 自己署名証明書なのでTrustedではないが、構造的に有効
        assert!(!result.active_manifest_signature.is_empty());
        assert_eq!(result.content_type, "image/jpeg");
    }

    #[test]
    fn test_verify_c2pa_no_c2pa() {
        // C2PAデータなしの生画像
        let result = verify_c2pa(TEST_IMAGE, "image/jpeg");
        assert!(result.is_err());
        match result {
            Err(CoreError::C2paVerificationFailed(_)) => {} // 期待通り
            other => panic!("予期しない結果: {other:?}"),
        }
    }

    #[test]
    fn test_extract_content_hash() {
        let signed = create_signed_content("test-hash.jpg");
        let hash = extract_content_hash(&signed, "image/jpeg").unwrap();

        // ハッシュは32バイト
        assert_eq!(hash.len(), 32);
        // 全てゼロではない
        assert!(hash.iter().any(|&b| b != 0));

        // 同じコンテンツからは同じcontent_hashが得られる（決定論的）
        let hash2 = extract_content_hash(&signed, "image/jpeg").unwrap();
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_extract_content_hash_no_c2pa() {
        let result = extract_content_hash(TEST_IMAGE, "image/jpeg");
        assert!(result.is_err());
    }

    #[test]
    fn test_build_provenance_graph_simple() {
        let signed = create_signed_content("test-graph.jpg");
        let graph = build_provenance_graph(&signed, "image/jpeg", 1000).unwrap();

        // ルートノードのみ（ingredientなし）
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].node_type, "final");
        assert!(graph.nodes[0].id.starts_with("0x"));
        assert_eq!(graph.links.len(), 0);
    }

    #[test]
    fn test_build_provenance_graph_with_ingredient() {
        // まずingredient用のC2PA付きコンテンツを作成
        let ingredient = create_signed_content("ingredient.jpg");
        // それをingredientとして含むコンテンツを作成
        let final_content =
            create_signed_content_with_ingredient("final.jpg", &ingredient);

        let graph =
            build_provenance_graph(&final_content, "image/jpeg", 1000).unwrap();

        // ルートノード + ingredientノード
        assert!(graph.nodes.len() >= 2);
        assert!(graph.nodes.iter().any(|n| n.node_type == "final"));
        assert!(graph.nodes.iter().any(|n| n.node_type == "ingredient"));

        // リンクが存在する
        assert!(!graph.links.is_empty());

        // リンクのtargetがルートノードを指している
        let root = graph.nodes.iter().find(|n| n.node_type == "final").unwrap();
        assert!(graph.links.iter().any(|l| l.target == root.id));
    }

    #[test]
    fn test_build_provenance_graph_size_exceeded() {
        let signed = create_signed_content("test-limit.jpg");
        // max_graph_size=0で必ず超過する
        let result = build_provenance_graph(&signed, "image/jpeg", 0);
        assert!(result.is_err());
        match result {
            Err(CoreError::GraphSizeExceeded { .. }) => {} // 期待通り
            other => panic!("予期しない結果: {other:?}"),
        }
    }

    // ----- TSA / 重複解決テスト -----

    #[test]
    fn test_parse_rfc3339_to_epoch() {
        // 2024-01-01T00:00:00Z = 1704067200
        assert_eq!(parse_rfc3339_to_epoch("2024-01-01T00:00:00Z"), Some(1704067200));
        // フラクション付き
        assert_eq!(parse_rfc3339_to_epoch("2024-01-01T00:00:00.123Z"), Some(1704067200));
        // +00:00オフセット
        assert_eq!(parse_rfc3339_to_epoch("2024-01-01T00:00:00+00:00"), Some(1704067200));
        // 不正な入力
        assert_eq!(parse_rfc3339_to_epoch("not-a-date"), None);
        assert_eq!(parse_rfc3339_to_epoch(""), None);
    }

    #[test]
    fn test_resolve_duplicate_tsa_wins() {
        // TSAタイムスタンプを持つトークンが、Solana block timeで先に登録されたトークンに勝つ
        let tokens = vec![
            TokenRecord {
                id: "later_register_but_earlier_create".into(),
                tsa_timestamp: Some(1000), // 作成時刻: 1000
                tsa_pubkey_hash: Some("trusted_key".into()),
                solana_block_time: 2000,   // 登録時刻: 2000（後）
                is_burned: false,
            },
            TokenRecord {
                id: "earlier_register_but_later_create".into(),
                tsa_timestamp: None,
                tsa_pubkey_hash: None,
                solana_block_time: 1500,   // 登録時刻: 1500（先）、作成時刻もこれ
                is_burned: false,
            },
        ];

        let trusted = vec!["trusted_key".to_string()];
        let winner = resolve_duplicate(&tokens, &trusted).unwrap();
        assert_eq!(winner.id, "later_register_but_earlier_create");
    }

    #[test]
    fn test_resolve_duplicate_untrusted_tsa_ignored() {
        // 信頼リストに含まれないTSAは無視される
        let tokens = vec![
            TokenRecord {
                id: "untrusted_tsa".into(),
                tsa_timestamp: Some(500),
                tsa_pubkey_hash: Some("unknown_key".into()),
                solana_block_time: 2000,
                is_burned: false,
            },
            TokenRecord {
                id: "no_tsa_but_earlier".into(),
                tsa_timestamp: None,
                tsa_pubkey_hash: None,
                solana_block_time: 1000,
                is_burned: false,
            },
        ];

        let trusted = vec!["other_key".to_string()];
        let winner = resolve_duplicate(&tokens, &trusted).unwrap();
        // untrusted TSAは無視されるので、solana_block_time 2000 vs 1000 → 1000が勝つ
        assert_eq!(winner.id, "no_tsa_but_earlier");
    }

    #[test]
    fn test_resolve_duplicate_burn_excluded() {
        let tokens = vec![
            TokenRecord {
                id: "burned".into(),
                tsa_timestamp: Some(100), // 最古だがBurn済み
                tsa_pubkey_hash: None,
                solana_block_time: 100,
                is_burned: true,
            },
            TokenRecord {
                id: "active".into(),
                tsa_timestamp: None,
                tsa_pubkey_hash: None,
                solana_block_time: 500,
                is_burned: false,
            },
        ];

        let winner = resolve_duplicate(&tokens, &[]).unwrap();
        assert_eq!(winner.id, "active");
    }

    #[test]
    fn test_resolve_duplicate_same_time_uses_registration() {
        // 同一作成時刻の場合、Solana block timeが最古のものを選択
        let tokens = vec![
            TokenRecord {
                id: "later_registered".into(),
                tsa_timestamp: None,
                tsa_pubkey_hash: None,
                solana_block_time: 2000,
                is_burned: false,
            },
            TokenRecord {
                id: "earlier_registered".into(),
                tsa_timestamp: None,
                tsa_pubkey_hash: None,
                solana_block_time: 1000,
                is_burned: false,
            },
        ];

        let winner = resolve_duplicate(&tokens, &[]).unwrap();
        assert_eq!(winner.id, "earlier_registered");
    }

    #[test]
    fn test_resolve_duplicate_all_burned_returns_none() {
        let tokens = vec![TokenRecord {
            id: "burned".into(),
            tsa_timestamp: None,
            tsa_pubkey_hash: None,
            solana_block_time: 100,
            is_burned: true,
        }];

        assert!(resolve_duplicate(&tokens, &[]).is_none());
    }

    #[test]
    fn test_format_content_hash() {
        let hash = [0u8; 32];
        assert_eq!(
            format_content_hash(&hash),
            "0x0000000000000000000000000000000000000000000000000000000000000000"
        );

        let mut hash = [0u8; 32];
        hash[0] = 0xab;
        hash[31] = 0xcd;
        let s = format_content_hash(&hash);
        assert!(s.starts_with("0xab"));
        assert!(s.ends_with("cd"));
    }
}
