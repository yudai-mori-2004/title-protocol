// SPDX-License-Identifier: Apache-2.0

//! # Processor トレイト定義
//!
//! 仕様書 §3.1 — Processorの規約
//!
//! Processorはコンテンツのデータから属性を抽出する独立モジュール。
//! Rustで実装され、TEEバイナリに直接コンパイルされる（WASM不使用）。
//!
//! ## 規約
//! - コンテンツのデータを入力として受け取り、属性をJSON構造で出力する
//! - 他のprocessorの実行結果に依存しない（実行順序の制約なし）
//! - 処理失敗時はエラーを返すが、他processorの実行には影響しない

use crate::response::ProcessorOutput;

/// Processorトレイト。
/// 仕様書 §3.1
///
/// 全processorはこのトレイトを実装する。
/// TEEのオーケストレーション層が各processorを並列に実行し、
/// 結果を `ProcessResponse` にまとめる。
///
/// # エラーハンドリング
///
/// `process()` が `Err` を返した場合、オーケストレーション層は
/// `ProcessorOutput::error()` に変換してレスポンスに含める。
/// 他のprocessorの実行は継続される。
pub trait Processor: Send + Sync {
    /// Processor ID。リクエストの `processor_ids` と照合される。
    /// 仕様書 §3.2
    ///
    /// 例: `"c2pa-verify"`, `"image-pdq"`, `"provenance-graph"`
    fn id(&self) -> &str;

    /// コンテンツから属性を抽出する。
    /// 仕様書 §3.1
    ///
    /// # Arguments
    /// * `content` — コンテンツのバイト列（TEEが取得済み）
    /// * `content_type` — コンテンツのMIMEタイプ
    ///
    /// # Returns
    /// 成功時: processor固有のJSON構造（`status` フィールドなし、
    ///   オーケストレーション層が `ProcessorOutput::ok()` でラップ）
    /// 失敗時: `ProcessorError`（オーケストレーション層が
    ///   `ProcessorOutput::error()` に変換）
    fn process(
        &self,
        content: &[u8],
        content_type: &str,
    ) -> Result<serde_json::Value, ProcessorError>;
}

/// Processorの処理エラー。
/// 仕様書 §3.1
///
/// processorが処理に失敗した場合に返す。
/// このエラーは他processorの実行に影響しない。
#[derive(Debug, Clone, thiserror::Error)]
pub enum ProcessorError {
    /// コンテンツ形式が非対応。
    #[error("Unsupported content type: {content_type}")]
    UnsupportedContentType {
        /// 受信したMIMEタイプ。
        content_type: String,
    },

    /// コンテンツデータのパースに失敗。
    #[error("Failed to parse content: {0}")]
    ParseFailed(String),

    /// C2PA署名の検証に失敗。
    #[error("C2PA verification failed: {0}")]
    C2paVerificationFailed(String),

    /// 処理中の内部エラー。
    #[error("Internal processor error: {0}")]
    Internal(String),
}

/// Processorのレジストリ。
/// 仕様書 §2.5 GET /processors
///
/// TEEが対応しているprocessorの一覧を管理し、
/// リクエストの `processor_ids` に応じてprocessorをディスパッチする。
#[derive(Default)]
pub struct ProcessorRegistry {
    processors: Vec<Box<dyn Processor>>,
}

impl ProcessorRegistry {
    /// 空のレジストリを作成する。
    pub fn new() -> Self {
        Self {
            processors: Vec::new(),
        }
    }

    /// Processorを登録する。
    pub fn register(&mut self, processor: Box<dyn Processor>) {
        self.processors.push(processor);
    }

    /// 登録済みprocessorのID一覧を返す。
    /// 仕様書 §2.5 GET /processors
    pub fn processor_ids(&self) -> Vec<&str> {
        self.processors.iter().map(|p| p.id()).collect()
    }

    /// 指定されたIDのprocessorを検索する。
    pub fn get(&self, id: &str) -> Option<&dyn Processor> {
        self.processors
            .iter()
            .find(|p| p.id() == id)
            .map(|p| p.as_ref())
    }

    /// 指定された `processor_ids` の各processorでコンテンツを処理し、
    /// 結果をHashMapで返す。
    /// 仕様書 §3.1 — 各processorが独立に実行され、一つの失敗が他に影響しない。
    ///
    /// 注: 現在は逐次実行。並列実行はTEEオーケストレーション層で実装する。
    pub fn execute(
        &self,
        processor_ids: &[String],
        content: &[u8],
        content_type: &str,
    ) -> std::collections::HashMap<String, ProcessorOutput> {
        let mut results = std::collections::HashMap::new();

        for id in processor_ids {
            let output = match self.get(id) {
                Some(proc) => match proc.process(content, content_type) {
                    Ok(data) => ProcessorOutput::ok(data),
                    Err(e) => ProcessorOutput::error(e.to_string()),
                },
                None => ProcessorOutput::error(format!("Unknown processor: {id}")),
            };
            results.insert(id.clone(), output);
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用のモックprocessor。
    struct MockProcessor {
        id: String,
        result: Result<serde_json::Value, ProcessorError>,
    }

    impl Processor for MockProcessor {
        fn id(&self) -> &str {
            &self.id
        }

        fn process(
            &self,
            _content: &[u8],
            _content_type: &str,
        ) -> Result<serde_json::Value, ProcessorError> {
            self.result.clone()
        }
    }

    #[test]
    fn processor_trait_object_safety() {
        // Processor trait がオブジェクトセーフであることを確認
        let proc: Box<dyn Processor> = Box::new(MockProcessor {
            id: "test".into(),
            result: Ok(serde_json::json!({"key": "value"})),
        });
        assert_eq!(proc.id(), "test");
    }

    #[test]
    fn registry_register_and_list() {
        let mut registry = ProcessorRegistry::new();
        registry.register(Box::new(MockProcessor {
            id: "c2pa-verify".into(),
            result: Ok(serde_json::json!({})),
        }));
        registry.register(Box::new(MockProcessor {
            id: "image-pdq".into(),
            result: Ok(serde_json::json!({})),
        }));

        let ids = registry.processor_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"c2pa-verify"));
        assert!(ids.contains(&"image-pdq"));
    }

    #[test]
    fn registry_get_existing() {
        let mut registry = ProcessorRegistry::new();
        registry.register(Box::new(MockProcessor {
            id: "c2pa-verify".into(),
            result: Ok(serde_json::json!({})),
        }));

        assert!(registry.get("c2pa-verify").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn registry_execute_success() {
        let mut registry = ProcessorRegistry::new();
        registry.register(Box::new(MockProcessor {
            id: "c2pa-verify".into(),
            result: Ok(serde_json::json!({"validation": "valid"})),
        }));

        let results = registry.execute(&["c2pa-verify".into()], b"content", "image/jpeg");

        assert_eq!(results.len(), 1);
        let output = &results["c2pa-verify"];
        assert_eq!(output.status, crate::response::ProcessorStatus::Ok);
    }

    #[test]
    fn registry_execute_error_does_not_affect_others() {
        let mut registry = ProcessorRegistry::new();
        registry.register(Box::new(MockProcessor {
            id: "c2pa-verify".into(),
            result: Ok(serde_json::json!({"validation": "valid"})),
        }));
        registry.register(Box::new(MockProcessor {
            id: "failing-proc".into(),
            result: Err(ProcessorError::Internal("test failure".into())),
        }));

        let results = registry.execute(
            &["c2pa-verify".into(), "failing-proc".into()],
            b"content",
            "image/jpeg",
        );

        assert_eq!(results.len(), 2);
        assert_eq!(
            results["c2pa-verify"].status,
            crate::response::ProcessorStatus::Ok
        );
        assert_eq!(
            results["failing-proc"].status,
            crate::response::ProcessorStatus::Error
        );
    }

    #[test]
    fn registry_execute_unknown_processor() {
        let registry = ProcessorRegistry::new();
        let results = registry.execute(&["nonexistent".into()], b"content", "image/jpeg");

        assert_eq!(results.len(), 1);
        assert_eq!(
            results["nonexistent"].status,
            crate::response::ProcessorStatus::Error
        );
    }
}
