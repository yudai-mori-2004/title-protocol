//! # WASMバイナリローダー
//!
//! WASM Extensionバイナリの取得を抽象化する。
//! 仕様書 §7.1
//!
//! ## ローダー実装
//! - `FileLoader`: ローカルディレクトリからWASMを読み込む（開発・テスト用）
//! - `HttpLoader`: URL経由でWASMを取得する（本番用、Arweave等）

use std::future::Future;
use std::pin::Pin;

/// WASMバイナリのロード結果。
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

/// ローカルディレクトリからWASMバイナリを読み込むローダー。
/// 開発・テスト環境用。
///
/// ディレクトリ構成: `{dir}/{extension_id}.wasm`
pub struct FileLoader {
    dir: String,
}

impl FileLoader {
    /// 新しいFileLoaderを作成する。
    ///
    /// # 引数
    /// - `dir`: WASMバイナリが格納されているディレクトリパス
    pub fn new(dir: String) -> Self {
        Self { dir }
    }
}

impl WasmLoader for FileLoader {
    fn load<'a>(
        &'a self,
        extension_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<WasmBinary, String>> + Send + 'a>> {
        Box::pin(async move {
            let path = format!("{}/{extension_id}.wasm", self.dir);
            let bytes = std::fs::read(&path)
                .map_err(|e| format!("WASMバイナリの読み込みに失敗 ({path}): {e}"))?;
            Ok(WasmBinary {
                source: format!("file://{path}"),
                bytes,
            })
        })
    }
}

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
            let response = crate::proxy_client::proxy_get(&self.proxy_addr, &url)
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

/// 標準エクスポート関数名。
/// 全WASMモジュールはこの名前で処理関数をエクスポートする。
pub const STANDARD_EXPORT_NAME: &str = "process";
