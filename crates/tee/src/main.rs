//! # Title Protocol TEEサーバー
//!
//! 仕様書セクション6.4で定義されるTEEサーバーのエントリポイント。
//!
//! ## 起動シーケンス (仕様書 §6.4)
//! 1. 鍵生成（署名用Ed25519、暗号化用X25519、Tree用Ed25519）
//! 2. /create-tree エンドポイント公開（inactive状態）
//! 3. /create-tree 呼び出し後、active状態に遷移
//! 4. /verify, /sign エンドポイントの受付開始

mod runtime;
mod endpoints;
mod proxy_client;

use std::sync::Arc;
use tokio::sync::RwLock;

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
pub struct AppState {
    /// TEEランタイム実装
    pub runtime: Box<dyn runtime::TeeRuntime + Send + Sync>,
    /// サーバーの現在の状態
    pub state: RwLock<TeeState>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    // ランタイム選択: MOCK_MODE=true ならモック、それ以外はNitro
    let runtime: Box<dyn runtime::TeeRuntime + Send + Sync> =
        if std::env::var("MOCK_MODE").unwrap_or_default() == "true" {
            tracing::info!("MockRuntimeで起動します");
            Box::new(runtime::mock::MockRuntime::new())
        } else {
            tracing::info!("NitroRuntimeで起動します");
            Box::new(runtime::nitro::NitroRuntime::new())
        };

    let shared_state = Arc::new(AppState {
        runtime,
        state: RwLock::new(TeeState::Inactive),
    });

    // Step 1: 鍵生成 (仕様書 §6.4)
    tracing::info!("鍵を生成中...");
    {
        let rt = &shared_state.runtime;
        rt.generate_signing_keypair();
        rt.generate_encryption_keypair();
    }
    tracing::info!("鍵生成完了");

    // axumルーターの構築
    let app = axum::Router::new()
        .route("/create-tree", axum::routing::post(endpoints::create_tree::handle_create_tree))
        .route("/verify", axum::routing::post(endpoints::verify::handle_verify))
        .route("/sign", axum::routing::post(endpoints::sign::handle_sign))
        .with_state(shared_state);

    let addr = "0.0.0.0:4000";
    tracing::info!("TEEサーバーを {} で起動します (inactive状態)", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
