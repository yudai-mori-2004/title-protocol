//! # Attestation Document 検証
//!
//! 仕様書 §5.2 Step 4.1
//!
//! TEE種別に応じたAttestation Documentの検証を提供する。
//! 各TEE実装はサブモジュールとして配置される。
//!
//! ## 対応TEE種別
//!
//! | `tee_type` | サブモジュール | Attestation形式 | 測定値 |
//! |------------|--------------|----------------|--------|
//! | `aws_nitro` | [`nitro`] | COSE Sign1 + CBOR | PCR0, PCR1, PCR2 |
//! | `amd_sev_snp` | （将来実装） | AMD SEV-SNP Report | MEASUREMENT |
//! | `intel_tdx` | （将来実装） | Intel TDX Quote | MRTD, RTMR0〜RTMR3 |

pub mod nitro;

use std::collections::BTreeMap;

/// Attestation Document検証のエラー型。
/// 全TEE種別で共通。
#[derive(Debug, thiserror::Error)]
pub enum AttestationError {
    /// 未対応のTEE種別
    #[error("未対応のTEE種別: {0}")]
    UnsupportedTeeType(String),
    /// COSE Sign1のパースに失敗
    #[error("COSE Sign1のパースに失敗: {0}")]
    CoseParseError(String),
    /// CBORペイロードのパースに失敗
    #[error("CBORペイロードのパースに失敗: {0}")]
    CborParseError(String),
    /// 証明書チェーンの検証に失敗
    #[error("証明書チェーンの検証に失敗: {0}")]
    CertChainError(String),
    /// 署名検証に失敗
    #[error("署名検証に失敗")]
    SignatureVerificationFailed,
    /// 必須フィールドが見つからない
    #[error("必須フィールドが見つかりません: {0}")]
    MissingField(String),
    /// 証明書のパースに失敗
    #[error("証明書のパースに失敗: {0}")]
    CertParseError(String),
    /// Base64デコードに失敗
    #[error("Base64デコードに失敗: {0}")]
    Base64Error(String),
    /// レポートのパースに失敗（SEV-SNP, TDX向け）
    #[error("Attestation Reportのパースに失敗: {0}")]
    ReportParseError(String),
}

/// TEE種別に依存しないAttestation検証結果。
/// 仕様書 §5.2 Step 4.1
///
/// 各TEE固有の詳細情報が必要な場合は、サブモジュールの個別結果型
/// （例: [`nitro::NitroAttestationResult`]）を直接使用する。
#[derive(Debug, Clone)]
pub struct AttestationResult {
    /// TEE種別（`"aws_nitro"`, `"amd_sev_snp"`, `"intel_tdx"`）
    pub tee_type: String,
    /// 測定値マップ（TEE種別ごとにキー名が異なる）
    ///
    /// - AWS Nitro: `"PCR0"`, `"PCR1"`, `"PCR2"` (各48バイト)
    /// - AMD SEV-SNP: `"MEASUREMENT"` (48バイト)
    /// - Intel TDX: `"MRTD"`, `"RTMR0"` 〜 `"RTMR3"` (各48バイト)
    pub measurements: BTreeMap<String, Vec<u8>>,
    /// Attestation Documentに含まれる公開鍵（TEE署名用公開鍵）
    pub public_key: Option<Vec<u8>>,
    /// Attestation Documentに含まれるユーザーデータ
    pub user_data: Option<Vec<u8>>,
    /// Attestation Documentに含まれるノンス
    pub nonce: Option<Vec<u8>>,
    /// Attestation生成時のタイムスタンプ（Unix ms、取得可能な場合のみ）
    pub timestamp: Option<u64>,
}

/// `tee_type` に応じてAttestation Documentを検証し、共通結果を返す。
/// 仕様書 §5.2 Step 4.1
///
/// ```text
/// tee_type に応じた証明書チェーンを検証:
///   - aws_nitro:   AWS Nitro Attestation PKI ルート証明書
///   - amd_sev_snp: AMD ARK → ASK → VCEK 証明書チェーン
///   - intel_tdx:   Intel SGX PCK 証明書チェーン
/// ```
pub fn verify_attestation(
    tee_type: &str,
    document: &[u8],
) -> Result<AttestationResult, AttestationError> {
    match tee_type {
        "aws_nitro" => {
            let nitro_result = nitro::verify_nitro_attestation(document)?;
            Ok(nitro_result.into())
        }
        // 将来の TEE 種別はここに追加:
        // "amd_sev_snp" => { ... }
        // "intel_tdx" => { ... }
        other => Err(AttestationError::UnsupportedTeeType(other.into())),
    }
}

/// 測定値が期待値と一致するか確認する。
/// 仕様書 §5.2 Step 4.1 — Global Config の expected_measurements と照合
///
/// `expected_measurements` のキー名はTEE種別に対応:
/// - AWS Nitro: `"PCR0"`, `"PCR1"`, `"PCR2"`
/// - AMD SEV-SNP: `"MEASUREMENT"`
/// - Intel TDX: `"MRTD"`, `"RTMR0"` 〜 `"RTMR3"`
pub fn verify_measurements(
    result: &AttestationResult,
    expected_measurements: &BTreeMap<String, Vec<u8>>,
) -> bool {
    expected_measurements.iter().all(|(key, expected)| {
        result
            .measurements
            .get(key)
            .map_or(false, |actual| actual == expected)
    })
}

/// 公開鍵が期待値と一致するか確認する。
/// 仕様書 §5.2 Step 4.1 — tee_pubkey との一致確認
pub fn verify_public_key(result: &AttestationResult, expected_pubkey: &[u8]) -> bool {
    result
        .public_key
        .as_ref()
        .map_or(false, |pk| pk == expected_pubkey)
}

impl From<nitro::NitroAttestationResult> for AttestationResult {
    fn from(nitro: nitro::NitroAttestationResult) -> Self {
        let mut measurements = BTreeMap::new();
        for (idx, value) in &nitro.pcrs {
            measurements.insert(format!("PCR{}", idx), value.clone());
        }
        Self {
            tee_type: "aws_nitro".to_string(),
            measurements,
            public_key: nitro.public_key,
            user_data: nitro.user_data,
            nonce: nitro.nonce,
            timestamp: Some(nitro.timestamp),
        }
    }
}
