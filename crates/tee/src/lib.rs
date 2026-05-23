// SPDX-License-Identifier: Apache-2.0

//! # Title Protocol TEE Runtime
//!
//! TEE（Trusted Execution Environment）のハードウェア抽象化レイヤー。
//!
//! 仕様書 §5.2
//!
//! ## 設計
//!
//! `TeeRuntime` トレイトでTEEハードウェア固有の機能を抽象化し、
//! feature flagでベンダー実装を切り替える。
//!
//! - `vendor-aws` — AWS Nitro Enclaves実装（`vendor::aws::NitroRuntime`）
//! - （デフォルト）— トレイト定義のみ、ベンダー実装なし
//!
//! ## Legacy参照
//!
//! `legacy/v0.1.0/crates/tee/src/runtime/` — 前バージョンのTeeRuntime実装。
//! v0.1.0ではcrypto固有メソッド（signer, decapsulator等）がTeeRuntimeに含まれていたが、
//! v0.1.2ではTEEハードウェア抽象化に専念し、暗号操作は別層で扱う。

pub mod vendor;

/// TEEランタイムのエラー型。
/// 仕様書 §5.2
#[derive(Debug, thiserror::Error)]
pub enum TeeError {
    /// Attestation Document取得の失敗。
    #[error("Failed to get attestation document: {0}")]
    AttestationFailed(String),

    /// 乱数生成の失敗。
    #[error("Failed to generate random bytes: {0}")]
    RandomFailed(String),

    /// TEE環境の初期化エラー。
    #[error("TEE initialization failed: {0}")]
    InitializationFailed(String),

    /// ベンダー固有のエラー。
    #[error("Vendor-specific error: {0}")]
    VendorError(String),
}

/// TEEランタイム抽象化トレイト。
/// 仕様書 §5.2
///
/// TEEハードウェア固有の機能（Attestation取得、乱数生成）を抽象化する。
/// ベンダー実装は feature flag で切り替える（`vendor-aws` 等）。
///
/// # v0.1.0からの変更点
///
/// v0.1.0では暗号操作（署名、KEM復号等）がTeeRuntimeに含まれていたが、
/// v0.1.2ではTEEハードウェア抽象化に専念する。
/// 暗号鍵の生成・管理は別途暗号化タスクで実装する。
///
/// # 設計原則
///
/// - ステートレス: リクエスト間で状態を持たない（仕様書 §0.5）
/// - 鍵はメモリ内のみ: 秘密鍵はTEEメモリ上にのみ存在する（仕様書 §5.2）
pub trait TeeRuntime: Send + Sync {
    /// TEEベンダー種別を返す。
    /// 仕様書 §5.2
    ///
    /// 例: `"aws_nitro"`, `"amd_sev_snp"`, `"intel_tdx"`, `"mock"`
    fn tee_type(&self) -> &str;

    /// Attestation Documentを取得する。
    /// 仕様書 §1.2, §5.2
    ///
    /// `user_data` にはレスポンスのJCS正規化ハッシュ（SHA-256）が渡される。
    /// TEEのハイパーバイザーが `user_data` を含むAttestation Documentを生成し、
    /// ベンダーの秘密鍵で署名する。
    ///
    /// # Returns
    /// Attestation Documentのバイナリ表現（ベンダー固有のフォーマット）。
    /// AWS Nitro: COSE_Sign1エンコードされたCBOR
    fn get_attestation_document(&self, user_data: &[u8]) -> Result<Vec<u8>, TeeError>;

    /// 暗号学的に安全な乱数バイト列を生成する。
    /// 仕様書 §5.2
    ///
    /// TEE内部のエントロピーソースを使用する。
    /// 鍵生成やnonce生成に使用される。
    ///
    /// # Arguments
    /// * `len` — 生成するバイト数
    fn random_bytes(&self, len: usize) -> Result<Vec<u8>, TeeError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用のモックTEEランタイム。
    struct MockRuntime;

    impl TeeRuntime for MockRuntime {
        fn tee_type(&self) -> &str {
            "mock"
        }

        fn get_attestation_document(&self, user_data: &[u8]) -> Result<Vec<u8>, TeeError> {
            // モック: user_dataをそのまま返す
            Ok(user_data.to_vec())
        }

        fn random_bytes(&self, len: usize) -> Result<Vec<u8>, TeeError> {
            // モック: ゼロ埋め
            Ok(vec![0u8; len])
        }
    }

    #[test]
    fn trait_object_safety() {
        // TeeRuntime がオブジェクトセーフであることを確認
        let rt: Box<dyn TeeRuntime> = Box::new(MockRuntime);
        assert_eq!(rt.tee_type(), "mock");
    }

    #[test]
    fn mock_attestation() {
        let rt = MockRuntime;
        let user_data = b"sha256:test_hash";
        let doc = rt.get_attestation_document(user_data).unwrap();
        assert_eq!(doc, user_data);
    }

    #[test]
    fn mock_random_bytes() {
        let rt = MockRuntime;
        let bytes = rt.random_bytes(32).unwrap();
        assert_eq!(bytes.len(), 32);
    }
}
