// SPDX-License-Identifier: Apache-2.0

//! # Processor トレイト定義
//!
//! 仕様書 §3.1 — Processorの規約
//!
//! Processorはコンテンツのデータから属性を抽出する独立モジュール。
//! Rustで実装され、TEEバイナリに直接コンパイルされる（WASM不使用）。
//!
//! ## 規約
//! - コンテンツを `Read + Seek` ストリームとして受け取り、属性をJSON構造で出力する
//! - 他のprocessorの実行結果に依存しない（実行順序の制約なし）
//! - 処理失敗時はエラーを返すが、他processorの実行には影響しない
//!
//! ## ストリーミング入力 (仕様 §4.3)
//!
//! Processor は `&[u8]` ではなく `&mut dyn ContentStream` を受け取る。
//! これにより、50 GB 級の動画でも HTTP Range Request 経由で必要な部分だけ
//! を読み込み、ピークメモリを「マニフェスト + チャンク 1 個分」に抑えられる。
//! 既存の c2pa-rs API (`Reader::with_stream`) が `Read + Seek` を要求する
//! ため、Range Request reader を直接 c2pa-rs に流せる。

use crate::content_stream::{ContentSource, ContentStream};
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
    /// 仕様書 §3.1, §4.3
    ///
    /// # Arguments
    /// * `content` — コンテンツの読み込みストリーム (`Read + Seek + Send`)。
    ///   processor は必要に応じて先頭に seek し直してから読む。orchestrator は
    ///   processor ごとに独立した reader を渡す。
    /// * `content_type` — コンテンツのMIMEタイプ
    ///
    /// # Returns
    /// 成功時: processor固有のJSON構造（`status` フィールドなし、
    ///   オーケストレーション層が `ProcessorOutput::ok()` でラップ）
    /// 失敗時: `ProcessorError`（オーケストレーション層が
    ///   `ProcessorOutput::error()` に変換）
    fn process(
        &self,
        content: &mut dyn ContentStream,
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

    /// コンテンツの読み込み (I/O) に失敗。Range Request での network エラー等。
    /// 仕様 §4.3 — ストリーミング読み込み中に発生しうる。
    #[error("Content read failed: {0}")]
    ReadFailed(String),

    /// 処理中の内部エラー。
    #[error("Internal processor error: {0}")]
    Internal(String),
}

/// Processorのレジストリ。
/// 仕様書 §2.5 GET /processors
///
/// TEEが対応しているprocessorの一覧を管理し、
/// リクエストの `processor_ids` に応じてprocessorをディスパッチする。
pub struct ProcessorRegistry {
    processors: Vec<Box<dyn Processor>>,
}

impl ProcessorRegistry {
    /// 空のレジストリを作成する。
    ///
    /// `.default()` 呼出ゼロの dead surface を再追加しないため
    /// `clippy::new_without_default` を抑制する。
    #[allow(clippy::new_without_default)]
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
    /// 仕様書 §3.1, §4.3 — 各processorが独立に実行され、一つの失敗が他に影響しない。
    ///
    /// `source` は reader factory。各 processor 呼び出しの直前に `source.open()`
    /// で新しい `Read + Seek` stream を取得し、processor に渡す。これにより
    /// processor 間で stream position が干渉せず、Range Request 経由でも
    /// それぞれが必要な部分だけを読み込める。
    ///
    /// 注: 現在は逐次実行。並列実行はTEEオーケストレーション層で実装する。
    pub fn execute(
        &self,
        processor_ids: &[String],
        source: &dyn ContentSource,
        content_type: &str,
    ) -> std::collections::HashMap<String, ProcessorOutput> {
        let mut results = std::collections::HashMap::new();

        for id in processor_ids {
            let output = match self.get(id) {
                Some(proc) => match source.open() {
                    Ok(mut reader) => match proc.process(reader.as_mut(), content_type) {
                        Ok(data) => ProcessorOutput::from_value_object(data),
                        Err(e) => ProcessorOutput::error(e.to_string()),
                    },
                    Err(e) => ProcessorOutput::error(format!(
                        "Failed to open content stream for {id}: {e}"
                    )),
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
    use crate::content_stream::InMemorySource;

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
            _content: &mut dyn ContentStream,
            _content_type: &str,
        ) -> Result<serde_json::Value, ProcessorError> {
            self.result.clone()
        }
    }

    fn mem(bytes: &[u8]) -> InMemorySource {
        InMemorySource::new(bytes.to_vec())
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

        let source = mem(b"content");
        let results = registry.execute(&["c2pa-verify".into()], &source, "image/jpeg");

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

        let source = mem(b"content");
        let results = registry.execute(
            &["c2pa-verify".into(), "failing-proc".into()],
            &source,
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
        let source = mem(b"content");
        let results = registry.execute(&["nonexistent".into()], &source, "image/jpeg");

        assert_eq!(results.len(), 1);
        assert_eq!(
            results["nonexistent"].status,
            crate::response::ProcessorStatus::Error
        );
    }

    /// 仕様 §3.1 — 各 processor は独立した reader を持つべき。`ProcessorRegistry`
    /// が `source.open()` を processor ごとに呼ぶ factory パターン契約を検証。
    ///
    /// 2 つの processor がそれぞれ `read_to_end` でストリーム全体を消費するが、
    /// それぞれ独立した reader なので両方とも完全なバイト列を見る。共有 reader
    /// なら 2 番目は空を見るはず。本テストは「共有していない」事実を assertion する。
    #[test]
    fn registry_execute_gives_each_processor_independent_reader() {
        /// ストリーム全長を読んでバイト数を返す processor。
        struct ReadAllProcessor {
            id: String,
        }
        impl Processor for ReadAllProcessor {
            fn id(&self) -> &str {
                &self.id
            }
            fn process(
                &self,
                content: &mut dyn ContentStream,
                _content_type: &str,
            ) -> Result<serde_json::Value, ProcessorError> {
                let mut buf = Vec::new();
                content
                    .read_to_end(&mut buf)
                    .map_err(|e| ProcessorError::ReadFailed(format!("{e}")))?;
                Ok(serde_json::json!({ "byte_count": buf.len() }))
            }
        }

        let mut registry = ProcessorRegistry::new();
        registry.register(Box::new(ReadAllProcessor { id: "a".into() }));
        registry.register(Box::new(ReadAllProcessor { id: "b".into() }));

        let payload = b"hello-world-this-is-a-test-payload";
        let source = mem(payload);
        let results = registry.execute(&["a".into(), "b".into()], &source, "text/plain");

        assert_eq!(results.len(), 2);
        for id in ["a", "b"] {
            let output = &results[id];
            assert_eq!(
                output.status,
                crate::response::ProcessorStatus::Ok,
                "processor {id} failed: {output:?}"
            );
            let byte_count = output
                .data
                .get("byte_count")
                .and_then(|v| v.as_u64())
                .expect("byte_count field");
            assert_eq!(
                byte_count as usize,
                payload.len(),
                "processor {id} should see the full payload via its own reader"
            );
        }
    }

    /// 各 processor の reader 状態が他の processor に漏れないことを更に保証する:
    /// 1 つ目の processor が stream を中途半端な位置 (3 byte 読み) で離脱した
    /// あと、2 つ目の processor が独立 reader で先頭から読めることを確認。
    #[test]
    fn registry_execute_isolated_state_across_processors() {
        struct PartialReadProcessor {
            id: String,
            read_n: usize,
        }
        impl Processor for PartialReadProcessor {
            fn id(&self) -> &str {
                &self.id
            }
            fn process(
                &self,
                content: &mut dyn ContentStream,
                _: &str,
            ) -> Result<serde_json::Value, ProcessorError> {
                let mut buf = vec![0u8; self.read_n];
                content
                    .read_exact(&mut buf)
                    .map_err(|e| ProcessorError::ReadFailed(format!("{e}")))?;
                Ok(serde_json::json!({ "first": buf[0], "last": buf[buf.len() - 1] }))
            }
        }

        let mut registry = ProcessorRegistry::new();
        registry.register(Box::new(PartialReadProcessor {
            id: "first3".into(),
            read_n: 3,
        }));
        registry.register(Box::new(PartialReadProcessor {
            id: "first5".into(),
            read_n: 5,
        }));

        // payload[0]=0xAA, payload[2]=0xCC, payload[4]=0xEE
        let payload = vec![0xAAu8, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let source = mem(&payload);
        let results = registry.execute(
            &["first3".into(), "first5".into()],
            &source,
            "application/octet-stream",
        );

        // first3: payload[0..3] → first=0xAA, last=0xCC
        let f3 = &results["first3"].data;
        assert_eq!(f3["first"].as_u64(), Some(0xAA));
        assert_eq!(f3["last"].as_u64(), Some(0xCC));

        // first5: payload[0..5] → first=0xAA, last=0xEE。先頭から読めている = reader 独立。
        let f5 = &results["first5"].data;
        assert_eq!(f5["first"].as_u64(), Some(0xAA));
        assert_eq!(f5["last"].as_u64(), Some(0xEE));
    }
}
