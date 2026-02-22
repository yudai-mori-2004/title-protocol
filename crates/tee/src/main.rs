//! # Title Protocol TEEサーバー
//!
//! 仕様書セクション6.4で定義されるTEEサーバーのエントリポイント。
//!
//! ## 起動シーケンス (仕様書 §6.4)
//! 1. 鍵生成（署名用Ed25519、暗号化用X25519、Tree用Ed25519）
//! 2. /create-tree エンドポイント公開（inactive状態）
//! 3. /create-tree 呼び出し後、active状態に遷移
//! 4. /verify, /sign エンドポイントの受付開始

pub mod config;
pub mod error;
mod runtime;
mod endpoints;
pub mod infra;
mod blockchain;
pub mod wasm_loader;

use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::{RwLock, Semaphore};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

use config::{TeeAppState, TeeState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    // ランタイム選択: TEE_RUNTIME 環境変数で切り替え（デフォルト: mock）
    let runtime_name = std::env::var("TEE_RUNTIME").unwrap_or_else(|_| "mock".to_string());
    let runtime: Box<dyn runtime::TeeRuntime + Send + Sync> = match runtime_name.as_str() {
        "mock" => {
            tracing::info!("MockRuntimeで起動します");
            Box::new(runtime::mock::MockRuntime::new())
        }
        #[cfg(feature = "vendor-aws")]
        "nitro" => {
            tracing::info!("NitroRuntimeで起動します");
            Box::new(runtime::nitro::NitroRuntime::new())
        }
        other => {
            anyhow::bail!("未対応のTEEランタイム: {other} (対応: mock, nitro)");
        }
    };

    let proxy_addr =
        std::env::var("PROXY_ADDR").unwrap_or_else(|_| "127.0.0.1:8000".to_string());

    // MPL-Coreコレクションアドレス（仕様書 §5.2）
    let collection_mint = std::env::var("COLLECTION_MINT")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| Pubkey::from_str(&s).expect("COLLECTION_MINTが不正なBase58です"));

    if let Some(ref mint) = collection_mint {
        tracing::info!(collection_mint = %mint, "コレクションミント設定済み");
    } else {
        tracing::warn!("COLLECTION_MINTが未設定です。コレクションなしでミントします");
    }

    // Gateway認証用公開鍵（仕様書 §6.2）
    let gateway_pubkey = std::env::var("GATEWAY_PUBKEY").ok().filter(|s| !s.is_empty()).map(|s| {
        use base58::FromBase58;
        let bytes = s.from_base58().expect("GATEWAY_PUBKEYが不正なBase58です");
        let arr: [u8; 32] = bytes
            .try_into()
            .expect("GATEWAY_PUBKEYは32バイトである必要があります");
        title_crypto::Ed25519VerifyingKey::from_bytes(&arr)
            .expect("GATEWAY_PUBKEYが不正なEd25519公開鍵です")
    });
    if gateway_pubkey.is_some() {
        tracing::info!("Gateway認証が有効です");
    } else {
        tracing::warn!("GATEWAY_PUBKEYが未設定です。Gateway認証をスキップします（開発環境用）");
    }

    // WASMローダー構築（仕様書 §7.1）
    // WASM_BASE_URL が設定されている場合はHTTPローダー、それ以外はファイルローダー
    let wasm_loader: Option<Box<dyn wasm_loader::WasmLoader>> =
        if let Ok(base_url) = std::env::var("WASM_BASE_URL") {
            tracing::info!(base_url = %base_url, "HTTP WASMローダーを使用します");
            Some(Box::new(wasm_loader::HttpLoader::new(
                proxy_addr.clone(),
                base_url,
            )))
        } else {
            let wasm_dir = std::env::var("WASM_DIR").unwrap_or_else(|_| "./wasm-modules".to_string());
            tracing::info!(wasm_dir = %wasm_dir, "ファイル WASMローダーを使用します");
            Some(Box::new(wasm_loader::FileLoader::new(wasm_dir)))
        };

    // メモリ管理セマフォ（仕様書 §6.4 漸進的重み付きセマフォ予約）
    let max_concurrent_bytes: usize = std::env::var("MAX_CONCURRENT_BYTES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(infra::security::DEFAULT_MAX_CONCURRENT_BYTES as usize);
    // Semaphoreのパーミット上限はusize::MAXだが、実用上はu32::MAX以下に抑える
    let semaphore_permits = max_concurrent_bytes.min(u32::MAX as usize);
    let memory_semaphore = Arc::new(Semaphore::new(semaphore_permits));
    tracing::info!(max_concurrent_bytes = semaphore_permits, "メモリセマフォ初期化");

    // 信頼されたExtension ID（仕様書 §6.4 不正WASMインジェクション防御）
    // TRUSTED_EXTENSIONS=phash-v1,hardware-google,c2pa-training-v1,c2pa-license-v1
    let trusted_extension_ids = std::env::var("TRUSTED_EXTENSIONS").ok().map(|s| {
        let ids: HashSet<String> = s.split(',').map(|id| id.trim().to_string()).filter(|id| !id.is_empty()).collect();
        tracing::info!(extensions = ?ids, "信頼されたExtension一覧を設定しました");
        ids
    });
    if trusted_extension_ids.is_none() {
        tracing::warn!("TRUSTED_EXTENSIONSが未設定です。全Extension実行を許可します（開発環境用）");
    }

    let shared_state = Arc::new(TeeAppState {
        runtime,
        state: RwLock::new(TeeState::Inactive),
        proxy_addr,
        tree_address: RwLock::new(None),
        collection_mint,
        gateway_pubkey,
        wasm_loader,
        memory_semaphore,
        trusted_extension_ids,
    });

    // Step 1: 鍵生成 (仕様書 §6.4)
    tracing::info!("鍵を生成中...");
    {
        let rt = &shared_state.runtime;
        rt.generate_signing_keypair();
        rt.generate_encryption_keypair();
        rt.generate_tree_keypair();
    }
    tracing::info!("鍵生成完了");

    // axumルーターの構築
    let app = axum::Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .route("/create-tree", axum::routing::post(endpoints::handle_create_tree))
        .route("/verify", axum::routing::post(endpoints::handle_verify))
        .route("/sign", axum::routing::post(endpoints::handle_sign))
        .with_state(shared_state);

    let addr = "0.0.0.0:4000";
    tracing::info!("TEEサーバーを {} で起動します (inactive状態)", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
