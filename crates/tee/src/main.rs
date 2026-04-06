// SPDX-License-Identifier: Apache-2.0

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
use tokio::sync::RwLock;
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
            #[allow(unused_mut)]
            let mut supported = vec!["mock"];
            #[cfg(feature = "vendor-aws")]
            supported.push("nitro");
            anyhow::bail!("未対応のTEEランタイム: {other} (対応: {})", supported.join(", "));
        }
    };

    let proxy_addr =
        std::env::var("PROXY_ADDR").unwrap_or_else(|_| "127.0.0.1:8000".to_string());

    // MPL-Coreコレクションアドレス（仕様書 §5.2, §6.5）
    // Core cNFT用とExtension cNFT用で別コレクションを使用
    let core_collection_mint = std::env::var("CORE_COLLECTION_MINT")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| Pubkey::from_str(&s).expect("CORE_COLLECTION_MINTが不正なBase58です"));
    let ext_collection_mint = std::env::var("EXT_COLLECTION_MINT")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| Pubkey::from_str(&s).expect("EXT_COLLECTION_MINTが不正なBase58です"));

    if let Some(ref mint) = core_collection_mint {
        tracing::info!(core_collection_mint = %mint, "Coreコレクションミント設定済み");
    } else {
        tracing::warn!("CORE_COLLECTION_MINTが未設定です。コレクションなしでミントします");
    }
    if let Some(ref mint) = ext_collection_mint {
        tracing::info!(ext_collection_mint = %mint, "Extensionコレクションミント設定済み");
    } else {
        tracing::warn!("EXT_COLLECTION_MINTが未設定です。コレクションなしでミントします");
    }

    // Gateway認証用公開鍵（仕様書 §6.2）
    let gateway_pubkey: Option<Box<dyn title_crypto::signing::Verifier>> = std::env::var("GATEWAY_PUBKEY").ok().filter(|s| !s.is_empty()).map(|s| {
        use base58::FromBase58;
        let algo_str = std::env::var("GATEWAY_SIGNING_ALGORITHM").unwrap_or_else(|_| "ed25519".to_string());
        let algo = title_crypto::SigningAlgorithm::from_str(&algo_str)
            .unwrap_or_else(|| panic!("GATEWAY_SIGNING_ALGORITHMが不正です: {algo_str}"));
        let bytes = s.from_base58().expect("GATEWAY_PUBKEYが不正なBase58です");
        title_crypto::create_verifier(algo, &bytes)
            .unwrap_or_else(|e| panic!("GATEWAY_PUBKEYが不正な公開鍵です: {e}"))
    });
    if gateway_pubkey.is_some() {
        tracing::info!("Gateway認証が有効です");
    } else {
        tracing::warn!("GATEWAY_PUBKEYが未設定です。Gateway認証をスキップします（開発環境用）");
    }

    // WASMローダー構築（仕様書 §7.1）
    // WASM_DIR設定時: FileLoader（ローカルファイル読み込み）
    // 未設定時: RemoteLoader（PDAからURI解決 → HTTPS取得、キャッシュ付き）
    let wasm_loader: Option<Box<dyn wasm_loader::WasmLoader>> =
        if let Ok(wasm_dir) = std::env::var("WASM_DIR") {
            tracing::info!(wasm_dir = %wasm_dir, "ローカル WASMローダーを使用します");
            Some(Box::new(wasm_loader::FileLoader::new(wasm_dir)))
        } else {
            let solana_rpc = std::env::var("SOLANA_RPC_URL")
                .unwrap_or_else(|_| "https://api.devnet.solana.com".to_string());
            let program_id_str = std::env::var("PROGRAM_ID")
                .unwrap_or_else(|_| "HLdrA1s96z9rTsWMP9H8HrZXcV4guFkCyFrdKGBdAMyC".to_string());
            let program_id_pubkey: solana_sdk::pubkey::Pubkey = program_id_str.parse()
                .expect("PROGRAM_ID のパースに失敗");
            let program_id_bytes = program_id_pubkey.to_bytes();
            tracing::info!(program_id = %program_id_str, rpc = %solana_rpc, "GlobalConfig WASMローダーを使用します（キャッシュ付き）");
            let inner = Box::new(wasm_loader::ConfigLoader::new(
                solana_rpc,
                program_id_bytes,
                proxy_addr.clone(),
            ));
            let ttl = std::time::Duration::from_secs(3600);
            Some(Box::new(wasm_loader::CachedWasmLoader::new(inner, ttl)))
        };

    // ResourcePool — two-tier admission control (§6.4, §7.1)
    // ADMISSION_LIMIT: new requests accepted when used < this value
    // TOTAL_LIMIT: absolute ceiling for in-progress extend() calls
    let total_limit: usize = std::env::var("POOL_TOTAL_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(infra::security::DEFAULT_MAX_CONCURRENT_BYTES as usize);
    let admission_limit: usize = std::env::var("POOL_ADMISSION_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(total_limit * 3 / 4); // default: 75% of total
    let resource_pool = Arc::new(title_wasm_host::ResourcePool::new(admission_limit, total_limit));
    tracing::info!(admission_limit, total_limit, "ResourcePool初期化");

    // 信頼されたExtension ID（仕様書 §6.4 不正WASMインジェクション防御）
    // TRUSTED_EXTENSIONS=image-phash,image-pdq,video-vpdq,cert-google,cert-sony,cert-leica,cert-rootlens
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
        core_tree_address: RwLock::new(None),
        ext_tree_address: RwLock::new(None),
        core_collection_mint,
        ext_collection_mint,
        gateway_pubkey,
        wasm_loader,
        resource_pool,
        trusted_extension_ids,
        alt_address: RwLock::new(None),
        alt_addresses: RwLock::new(Vec::new()),
    });

    // Step 1: 鍵生成 (仕様書 §6.4)
    tracing::info!("鍵を生成中...");
    {
        let rt = &shared_state.runtime;
        rt.generate_keypairs();
    }
    tracing::info!("鍵生成完了");

    // axumルーターの構築
    let app = axum::Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .route("/create-tree", axum::routing::post(endpoints::handle_create_tree))
        .route("/register-node", axum::routing::post(endpoints::handle_register_node))
        .route("/verify", axum::routing::post(endpoints::handle_verify))
        .route("/sign", axum::routing::post(endpoints::handle_sign))
        .route("/set-alt", axum::routing::post(endpoints::handle_set_alt))
        .with_state(shared_state);

    let addr = "0.0.0.0:4000";
    tracing::info!("TEEサーバーを {} で起動します (inactive状態)", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
