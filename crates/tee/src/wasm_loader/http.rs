//! # HTTP WASMローダー
//!
//! 仕様書 §7.1
//!
//! URL経由でWASMバイナリを取得する。
//! 本番環境用（Arweave等のオフチェーンストレージ）。

use std::future::Future;
use std::pin::Pin;

use super::WasmBinary;
use super::WasmLoader;

/// URL経由でWASMバイナリを取得するローダー。
/// 本番環境用（Arweave等のオフチェーンストレージ）。
///
/// TEEはネットワークアクセスを持たないため、vsockプロキシ経由で取得する。
/// URL形式: `{base_url}/{extension_id}.wasm`
pub struct HttpLoader {
    /// vsockプロキシのアドレス
    proxy_addr: String,
    /// WASMバイナリのベースURL
    base_url: String,
}

impl HttpLoader {
    /// 新しいHttpLoaderを作成する。
    ///
    /// # 引数
    /// - `proxy_addr`: vsockプロキシのアドレス（例: "127.0.0.1:8000"）
    /// - `base_url`: WASMバイナリのベースURL（例: "https://arweave.net/wasm"）
    pub fn new(proxy_addr: String, base_url: String) -> Self {
        Self {
            proxy_addr,
            base_url,
        }
    }
}

impl WasmLoader for HttpLoader {
    fn load<'a>(
        &'a self,
        extension_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<WasmBinary, String>> + Send + 'a>> {
        Box::pin(async move {
            let url = format!("{}/{extension_id}.wasm", self.base_url);
            let response = crate::infra::proxy_client::proxy_get(&self.proxy_addr, &url)
                .await
                .map_err(|e| format!("WASM取得に失敗 ({url}): {e}"))?;
            if response.status != 200 {
                return Err(format!(
                    "WASM取得でHTTPエラー: ステータス {} ({url})",
                    response.status
                ));
            }
            if response.body.is_empty() {
                return Err(format!("WASM取得: 空のレスポンス ({url})"));
            }
            Ok(WasmBinary {
                source: url,
                bytes: response.body,
            })
        })
    }
}
