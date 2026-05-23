// SPDX-License-Identifier: Apache-2.0

//! # リクエスト型定義
//!
//! 仕様書 §2.2 — リクエスト形式
//!
//! クライアントがTEEに送信する属性抽出リクエストの型。
//! 3種の入力形式（single / fragmented / sidecar）に対応し、
//! 暗号化オプションを任意で指定できる。

use serde::{Deserialize, Serialize};

/// 属性抽出リクエスト。
/// 仕様書 §2.2
///
/// クライアントがコンテンツの所在と実行する処理を指定する。
/// `input` フィールドは `#[serde(flatten)]` によりトップレベルに展開される。
///
/// # JSON例（単一ファイル）
/// ```json
/// {
///   "input_type": "single",
///   "content_url": "https://r2.example.com/photo.jpg",
///   "processor_ids": ["c2pa-verify", "image-pdq"]
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessRequest {
    /// 入力データの形式とURL情報。
    /// `input_type` タグでディスパッチされ、トップレベルにフラット展開される。
    #[serde(flatten)]
    pub input: InputData,

    /// 実行するprocessorのID一覧。
    /// `c2pa-verify` は暗黙的に必須だが、クライアントが明示しなくてもTEE側で追加する。
    /// 仕様書 §1.3
    pub processor_ids: Vec<String>,

    /// 使用する暗号化スイート。省略時はコンテンツを平文として扱う。
    /// 仕様書 §1.4, §2.4
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption: Option<EncryptionSuite>,
}

/// 入力データの形式。
/// 仕様書 §1.3, §2.2
///
/// コンテンツの保存形式に応じて3つのバリアントを持つ。
/// `input_type` フィールドでタグ付けされる。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "input_type", rename_all = "lowercase")]
pub enum InputData {
    /// 単一ファイル（JPEG, PNG, MP4等）。
    /// 仕様書 §1.3 — 大容量ファイルはHTTP Range Requestで部分取得可能。
    Single {
        /// コンテンツファイルのURL。
        content_url: String,
    },

    /// フラグメント（CMAF分割動画）。
    /// 仕様書 §1.3 — 初期化セグメント + メディアセグメント群。
    Fragmented {
        /// 初期化セグメント（init.mp4）のURL。
        init_url: String,
        /// メディアセグメント（seg-*.m4s）のURL一覧。
        fragment_urls: Vec<String>,
    },

    /// サイドカー（マニフェスト分離形式）。
    /// 仕様書 §1.3 — C2PAマニフェストとコンテンツが別ファイル。
    Sidecar {
        /// C2PAマニフェストファイル（.c2pa）のURL。
        manifest_url: String,
        /// コンテンツファイルのURL。
        content_url: String,
    },
}

/// 暗号化スイート。
/// 仕様書 §2.4
///
/// TEEが起動時に各スイートの鍵ペアを生成する。
/// クライアントはコンテンツを暗号化する際に使用したスイートを申告する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EncryptionSuite {
    /// X25519 ECDH + HKDF-SHA256 + AES-256-GCM (suite_id: 0x01)
    #[serde(rename = "x25519")]
    X25519,

    /// ECDH P-256 + HKDF-SHA256 + AES-256-GCM (suite_id: 0x02)
    #[serde(rename = "p256")]
    P256,

    /// ML-KEM-768 (FIPS 203) + HKDF-SHA256 + AES-256-GCM (suite_id: 0x03)
    #[serde(rename = "ml-kem-768")]
    MlKem768,
}

impl EncryptionSuite {
    /// ワイヤーフォーマット上のsuite_id（1バイト）を返す。
    /// 仕様書 §2.4
    pub fn suite_id(&self) -> u8 {
        match self {
            Self::X25519 => 0x01,
            Self::P256 => 0x02,
            Self::MlKem768 => 0x03,
        }
    }

    /// suite_idからEncryptionSuiteを復元する。
    /// 仕様書 §2.4
    pub fn from_suite_id(id: u8) -> Option<Self> {
        match id {
            0x01 => Some(Self::X25519),
            0x02 => Some(Self::P256),
            0x03 => Some(Self::MlKem768),
            _ => None,
        }
    }
}

/// 暗号化ペイロード内のメタデータ。
/// 仕様書 §2.4
///
/// 暗号化ペイロードの先頭に埋め込まれるJSON。
/// TEEが復号後にsignature_hashの照合に使用する。
///
/// # バイナリ形式
/// ```text
/// [4B: metadata_len (big-endian u32)]
/// [metadata_len bytes: JSON EncryptedPayloadMetadata]
/// [remaining: raw content bytes]
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedPayloadMetadata {
    /// コンテンツのsignature_hash。
    /// クライアントがローカルで算出し、TEEが検証結果と照合する。
    /// 仕様書 §2.4 ステップ1, ステップ8
    pub signature_hash: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── §2.2 単一ファイルリクエスト ──

    #[test]
    fn single_request_serialize() {
        let req = ProcessRequest {
            input: InputData::Single {
                content_url: "https://r2.example.com/photo.jpg".into(),
            },
            processor_ids: vec!["c2pa-verify".into(), "image-pdq".into()],
            encryption: None,
        };
        let json = serde_json::to_value(&req).unwrap();

        assert_eq!(json["input_type"], "single");
        assert_eq!(json["content_url"], "https://r2.example.com/photo.jpg");
        assert_eq!(json["processor_ids"][0], "c2pa-verify");
        assert_eq!(json["processor_ids"][1], "image-pdq");
        assert!(json.get("encryption").is_none());
    }

    #[test]
    fn single_request_deserialize_from_spec() {
        let json = r#"{
            "input_type": "single",
            "content_url": "https://r2.example.com/photo.jpg",
            "processor_ids": ["c2pa-verify", "image-pdq"]
        }"#;
        let req: ProcessRequest = serde_json::from_str(json).unwrap();

        assert!(matches!(req.input, InputData::Single { .. }));
        if let InputData::Single { content_url } = &req.input {
            assert_eq!(content_url, "https://r2.example.com/photo.jpg");
        }
        assert_eq!(req.processor_ids, vec!["c2pa-verify", "image-pdq"]);
        assert!(req.encryption.is_none());
    }

    #[test]
    fn single_request_roundtrip() {
        let req = ProcessRequest {
            input: InputData::Single {
                content_url: "https://example.com/test.jpg".into(),
            },
            processor_ids: vec!["c2pa-verify".into()],
            encryption: None,
        };
        let json_str = serde_json::to_string(&req).unwrap();
        let restored: ProcessRequest = serde_json::from_str(&json_str).unwrap();
        assert_eq!(req, restored);
    }

    // ── §2.2 フラグメントリクエスト ──

    #[test]
    fn fragmented_request_deserialize_from_spec() {
        let json = r#"{
            "input_type": "fragmented",
            "init_url": "https://r2.example.com/video/init.mp4",
            "fragment_urls": [
                "https://r2.example.com/video/seg-0.m4s",
                "https://r2.example.com/video/seg-1.m4s",
                "https://r2.example.com/video/seg-2.m4s"
            ],
            "processor_ids": ["c2pa-verify"]
        }"#;
        let req: ProcessRequest = serde_json::from_str(json).unwrap();

        if let InputData::Fragmented {
            init_url,
            fragment_urls,
        } = &req.input
        {
            assert_eq!(init_url, "https://r2.example.com/video/init.mp4");
            assert_eq!(fragment_urls.len(), 3);
        } else {
            panic!("Expected Fragmented variant");
        }
    }

    #[test]
    fn fragmented_request_roundtrip() {
        let req = ProcessRequest {
            input: InputData::Fragmented {
                init_url: "https://example.com/init.mp4".into(),
                fragment_urls: vec!["https://example.com/seg-0.m4s".into()],
            },
            processor_ids: vec!["c2pa-verify".into()],
            encryption: None,
        };
        let json_str = serde_json::to_string(&req).unwrap();
        let restored: ProcessRequest = serde_json::from_str(&json_str).unwrap();
        assert_eq!(req, restored);
    }

    // ── §2.2 サイドカーリクエスト ──

    #[test]
    fn sidecar_request_deserialize_from_spec() {
        let json = r#"{
            "input_type": "sidecar",
            "manifest_url": "https://r2.example.com/photo.c2pa",
            "content_url": "https://r2.example.com/photo.jpg",
            "processor_ids": ["c2pa-verify"]
        }"#;
        let req: ProcessRequest = serde_json::from_str(json).unwrap();

        if let InputData::Sidecar {
            manifest_url,
            content_url,
        } = &req.input
        {
            assert_eq!(manifest_url, "https://r2.example.com/photo.c2pa");
            assert_eq!(content_url, "https://r2.example.com/photo.jpg");
        } else {
            panic!("Expected Sidecar variant");
        }
    }

    #[test]
    fn sidecar_request_roundtrip() {
        let req = ProcessRequest {
            input: InputData::Sidecar {
                manifest_url: "https://example.com/photo.c2pa".into(),
                content_url: "https://example.com/photo.jpg".into(),
            },
            processor_ids: vec!["c2pa-verify".into()],
            encryption: None,
        };
        let json_str = serde_json::to_string(&req).unwrap();
        let restored: ProcessRequest = serde_json::from_str(&json_str).unwrap();
        assert_eq!(req, restored);
    }

    // ── §2.2 暗号化ありリクエスト ──

    #[test]
    fn encrypted_request_deserialize_from_spec() {
        let json = r#"{
            "input_type": "single",
            "content_url": "https://r2.example.com/encrypted.bin",
            "encryption": "x25519",
            "processor_ids": ["c2pa-verify"]
        }"#;
        let req: ProcessRequest = serde_json::from_str(json).unwrap();

        assert_eq!(req.encryption, Some(EncryptionSuite::X25519));
    }

    #[test]
    fn encryption_suite_all_variants() {
        // x25519
        let json = r#""x25519""#;
        let suite: EncryptionSuite = serde_json::from_str(json).unwrap();
        assert_eq!(suite, EncryptionSuite::X25519);
        assert_eq!(suite.suite_id(), 0x01);

        // p256
        let json = r#""p256""#;
        let suite: EncryptionSuite = serde_json::from_str(json).unwrap();
        assert_eq!(suite, EncryptionSuite::P256);
        assert_eq!(suite.suite_id(), 0x02);

        // ml-kem-768
        let json = r#""ml-kem-768""#;
        let suite: EncryptionSuite = serde_json::from_str(json).unwrap();
        assert_eq!(suite, EncryptionSuite::MlKem768);
        assert_eq!(suite.suite_id(), 0x03);
    }

    #[test]
    fn encryption_suite_from_suite_id() {
        assert_eq!(
            EncryptionSuite::from_suite_id(0x01),
            Some(EncryptionSuite::X25519)
        );
        assert_eq!(
            EncryptionSuite::from_suite_id(0x02),
            Some(EncryptionSuite::P256)
        );
        assert_eq!(
            EncryptionSuite::from_suite_id(0x03),
            Some(EncryptionSuite::MlKem768)
        );
        assert_eq!(EncryptionSuite::from_suite_id(0x04), None);
        assert_eq!(EncryptionSuite::from_suite_id(0x00), None);
    }

    // ── §2.4 暗号化ペイロードメタデータ ──

    #[test]
    fn encrypted_payload_metadata_roundtrip() {
        let meta = EncryptedPayloadMetadata {
            signature_hash: "sha256:abcdef1234".into(),
        };
        let json_str = serde_json::to_string(&meta).unwrap();
        let restored: EncryptedPayloadMetadata = serde_json::from_str(&json_str).unwrap();
        assert_eq!(meta, restored);
    }

    // ── encryption フィールド省略時 ──

    #[test]
    fn encryption_none_omitted_in_json() {
        let req = ProcessRequest {
            input: InputData::Single {
                content_url: "https://example.com/test.jpg".into(),
            },
            processor_ids: vec!["c2pa-verify".into()],
            encryption: None,
        };
        let json_str = serde_json::to_string(&req).unwrap();
        assert!(!json_str.contains("encryption"));
    }
}
