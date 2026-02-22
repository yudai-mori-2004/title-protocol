//! # TEE設定・共有状態
//!
//! 仕様書 §6.4
//!
//! TEEサーバーの共有状態の定義。
//! `GatewayState`（`crates/gateway/src/config.rs`）と同パターン。

use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::{RwLock, Semaphore};
use solana_sdk::pubkey::Pubkey;

use crate::runtime::TeeRuntime;
use crate::wasm_loader::WasmLoader;

/// TEEサーバーの状態。
/// 仕様書 §6.4
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeeState {
    /// 起動直後。/create-tree のみ受付。
    Inactive,
    /// /create-tree 完了後。/verify, /sign 受付中。
    Active,
}

/// TEEサーバーの共有状態。
/// 仕様書 §6.4
pub struct TeeAppState {
    /// TEEランタイム実装
    pub runtime: Box<dyn TeeRuntime + Send + Sync>,
    /// サーバーの現在の状態
    pub state: RwLock<TeeState>,
    /// vsockプロキシの接続先アドレス（macOS: "127.0.0.1:8000"）
    pub proxy_addr: String,
    /// Merkle Treeアドレス（/create-tree後に設定される）
    pub tree_address: RwLock<Option<[u8; 32]>>,
    /// MPL-Coreコレクションアドレス（環境変数 COLLECTION_MINT で設定）
    /// 仕様書 §5.2 Step 1 — Global Configのcore_collection_mintに対応
    pub collection_mint: Option<Pubkey>,
    /// Gateway認証用Ed25519公開鍵（環境変数 GATEWAY_PUBKEY で設定）
    /// 仕様書 §6.2: Global Configのgateway_pubkeyで署名を検証
    /// Noneの場合はGateway認証をスキップ（開発環境用）
    pub gateway_pubkey: Option<title_crypto::Ed25519VerifyingKey>,
    /// WASMバイナリローダー（Extension実行時に使用）
    /// 仕様書 §7.1: Extension WASMバイナリの取得を抽象化
    /// Noneの場合、Extension実行は不可（core-c2paのみ対応）
    pub wasm_loader: Option<Box<dyn WasmLoader>>,
    /// グローバルメモリ予約セマフォ。
    /// 仕様書 §6.4 漸進的重み付きセマフォ予約
    /// max_concurrent_bytes分のパーミットを持ち、チャンク単位で予約する。
    pub memory_semaphore: Arc<Semaphore>,
    /// 信頼されたExtension IDの一覧。
    /// 仕様書 §6.4 不正WASMインジェクション防御
    /// Noneの場合は全Extension許可（開発環境用）、Someの場合は一覧にあるIDのみ許可。
    pub trusted_extension_ids: Option<HashSet<String>>,
}
