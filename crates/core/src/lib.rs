//! # Title Protocol Core
//!
//! 仕様書セクション2で定義されるC2PA検証と来歴グラフ構築を実装する。
//!
//! ## 処理フロー
//! 1. C2PA署名チェーンを検証する
//! 2. Active Manifestの署名からcontent_hashを計算する
//! 3. Manifestに含まれる素材情報を再帰的に抽出する
//! 4. 来歴グラフ（ノードとエッジ）を構築する

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

/// C2PA検証の結果。
/// 仕様書 §2.1
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
pub struct ProvenanceGraph {
    /// グラフのノード一覧（content_hashで識別）
    pub nodes: Vec<GraphNode>,
    /// グラフのリンク一覧（素材→派生の関係）
    pub links: Vec<GraphLink>,
}

/// C2PA署名チェーンを検証し、結果を返す。
/// 仕様書 §2.1
///
/// TEEはC2PA署名チェーンの正当性を検証し、以下を確認する:
/// - 署名チェーンの正当性（コンテンツの出自が改ざんされていない）
/// - コンテンツの同一性（Manifestが付与された時点から変更されていない）
pub fn verify_c2pa(
    _content_bytes: &[u8],
    _mime_type: &str,
) -> Result<C2paVerificationResult, CoreError> {
    todo!("C2PA検証の実装 (c2pa-rsを使用)")
}

/// Active Manifestの署名からcontent_hashを抽出する。
/// 仕様書 §2.1: `content_hash = SHA-256(Active Manifestの署名)`
pub fn extract_content_hash(
    _content_bytes: &[u8],
    _mime_type: &str,
) -> Result<[u8; 32], CoreError> {
    todo!("Active Manifestの署名抽出 → SHA-256ハッシュ計算")
}

/// C2PAの素材情報を再帰的に抽出し、来歴グラフ（DAG）を構築する。
/// 仕様書 §2.2
///
/// 各ノードはcontent_hashで識別され、各エッジは
/// 「この素材がこのコンテンツの作成に使われた」という関係を表す。
/// グラフはC2PAデータから客観的・機械的に構築される。
pub fn build_provenance_graph(
    _content_bytes: &[u8],
    _mime_type: &str,
    _max_graph_size: usize,
) -> Result<ProvenanceGraph, CoreError> {
    todo!("C2PA素材情報の再帰的抽出 → DAG構築")
}
