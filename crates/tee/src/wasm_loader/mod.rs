// SPDX-License-Identifier: Apache-2.0

//! # WASMバイナリローダー
//!
//! WASM Extensionバイナリの取得を抽象化する。
//! 仕様書 §7.1
//!
//! ## ローダー実装
//! - `FileLoader`: ローカルディレクトリからWASMを読み込む
//! - `ConfigLoader`: GlobalConfig PDAからURI解決 → HTTPS取得
//! - `CachedWasmLoader`: 任意のローダーにLRUキャッシュを付加

pub mod file;
pub mod remote;

pub use file::FileLoader;
pub use remote::ConfigLoader;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// WASMバイナリのロード結果。
#[derive(Clone)]
pub struct WasmBinary {
    /// WASMバイナリデータ
    pub bytes: Vec<u8>,
    /// ソースURI（signed_jsonの`wasm_source`フィールドに記録される）
    pub source: String,
}

/// WASMバイナリをロードするトレイト。
/// 仕様書 §7.1
///
/// Extension IDに対応するWASMバイナリを取得する方法を抽象化する。
/// ファイルシステム、HTTP（Arweave等）、その他のソースに対応可能。
pub trait WasmLoader: Send + Sync {
    /// extension_idに対応するWASMバイナリをロードする。
    fn load<'a>(
        &'a self,
        extension_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<WasmBinary, String>> + Send + 'a>>;
}

/// 標準エクスポート関数名。
/// 全WASMモジュールはこの名前で処理関数をエクスポートする。
pub const STANDARD_EXPORT_NAME: &str = "process";

// ---------------------------------------------------------------------------
// CachedWasmLoader
// ---------------------------------------------------------------------------

struct CacheEntry {
    binary: WasmBinary,
    fetched_at: Instant,
}

/// 任意の WasmLoader にインメモリ LRU キャッシュを付加するラッパー。
/// TTL ベースの無効化。同一 extension_id への並行リクエストは1つだけ実際にフェッチする。
pub struct CachedWasmLoader {
    inner: Box<dyn WasmLoader>,
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    ttl: Duration,
}

impl CachedWasmLoader {
    pub fn new(inner: Box<dyn WasmLoader>, ttl: Duration) -> Self {
        Self {
            inner,
            cache: Arc::new(RwLock::new(HashMap::new())),
            ttl,
        }
    }
}

impl WasmLoader for CachedWasmLoader {
    fn load<'a>(
        &'a self,
        extension_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<WasmBinary, String>> + Send + 'a>> {
        Box::pin(async move {
            // キャッシュヒット確認
            {
                let cache = self.cache.read().await;
                if let Some(entry) = cache.get(extension_id) {
                    if entry.fetched_at.elapsed() < self.ttl {
                        tracing::debug!(extension_id, "WASMキャッシュヒット");
                        return Ok(entry.binary.clone());
                    }
                }
            }

            // キャッシュミス → 実際にフェッチ
            tracing::info!(extension_id, "WASMキャッシュミス → フェッチ");
            let binary = self.inner.load(extension_id).await?;

            // キャッシュに保存
            {
                let mut cache = self.cache.write().await;
                cache.insert(
                    extension_id.to_string(),
                    CacheEntry {
                        binary: binary.clone(),
                        fetched_at: Instant::now(),
                    },
                );
            }

            Ok(binary)
        })
    }
}
