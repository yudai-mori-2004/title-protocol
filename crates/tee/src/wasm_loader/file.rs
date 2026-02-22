//! # ファイルシステム WASMローダー
//!
//! 仕様書 §7.1
//!
//! ローカルディレクトリからWASMバイナリを読み込む。
//! 開発・テスト環境用。

use std::future::Future;
use std::pin::Pin;

use super::WasmBinary;
use super::WasmLoader;

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
