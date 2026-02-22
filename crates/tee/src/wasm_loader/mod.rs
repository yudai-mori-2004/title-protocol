//! # WASMバイナリローダー
//!
//! WASM Extensionバイナリの取得を抽象化する。
//! 仕様書 §7.1
//!
//! ## ローダー実装
//! - `FileLoader`: ローカルディレクトリからWASMを読み込む（開発・テスト用）
//! - `HttpLoader`: URL経由でWASMを取得する（本番用、Arweave等）

pub mod file;
pub mod http;

pub use file::FileLoader;
pub use http::HttpLoader;

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

/// 標準エクスポート関数名。
/// 全WASMモジュールはこの名前で処理関数をエクスポートする。
pub const STANDARD_EXPORT_NAME: &str = "process";
