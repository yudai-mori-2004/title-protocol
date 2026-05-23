// SPDX-License-Identifier: Apache-2.0

//! # AWS Nitro Enclaves ランタイム
//!
//! 仕様書 §5.2
//!
//! AWS Nitro Enclaves上で動作するTeeRuntime実装。
//! NSM (Nitro Security Module) APIでAttestation Document取得と乱数生成を行う。
//!
//! ## ステータス
//!
//! 現在はスケルトン実装。NSM API統合は後続タスクで実装する。
//!
//! ## Legacy参照
//!
//! `legacy/v0.1.0/crates/tee/src/runtime/nitro.rs` —
//! 前バージョンのNitro実装。NsmOpsトレイトでNSMデバイス操作を抽象化し、
//! テスト時にモック注入するパターンを採用していた。

use crate::{TeeError, TeeRuntime};

/// AWS Nitro Enclaves ランタイム。
/// 仕様書 §5.2
///
/// NSM APIを使用してAttestation取得と乱数生成を行う。
/// 全ての秘密鍵はEnclave内メモリにのみ保持される。
///
/// # 設計（Legacy参照）
///
/// `legacy/v0.1.0/crates/tee/src/runtime/nitro.rs` のNsmOpsトレイトパターンを
/// 踏襲する予定。テスト用モックNSMデバイスの注入を可能にする。
pub struct NitroRuntime {
    // NSMデバイスハンドルやモック注入用フィールドは後続タスクで追加
    _private: (),
}

impl NitroRuntime {
    /// NitroRuntimeを初期化する。
    ///
    /// # Panics
    /// Linux以外の環境で呼び出された場合（Nitro Enclavesが利用不可）。
    pub fn new() -> Result<Self, TeeError> {
        // TODO: NSMデバイスの初期化
        // legacy参照: legacy/v0.1.0/crates/tee/src/runtime/nitro.rs
        //   let fd = driver::nsm_init();
        Err(TeeError::InitializationFailed(
            "NitroRuntime is not yet implemented. See legacy/v0.1.0/crates/tee/src/runtime/nitro.rs"
                .into(),
        ))
    }
}

impl TeeRuntime for NitroRuntime {
    fn tee_type(&self) -> &str {
        "aws_nitro"
    }

    fn get_attestation_document(&self, _user_data: &[u8]) -> Result<Vec<u8>, TeeError> {
        // TODO: NSM Attestation API呼び出し
        // legacy参照: legacy/v0.1.0/crates/tee/src/runtime/nitro.rs
        //   self.nsm.get_attestation_doc(
        //       Some(&signing_pk),
        //       Some(&encryption_pk),
        //       None,
        //   )
        Err(TeeError::AttestationFailed(
            "NitroRuntime attestation not yet implemented".into(),
        ))
    }

    fn random_bytes(&self, _len: usize) -> Result<Vec<u8>, TeeError> {
        // TODO: NSM GetRandom API呼び出し
        // legacy参照: legacy/v0.1.0/crates/tee/src/runtime/nitro.rs
        //   self.nsm.get_random(len)
        Err(TeeError::RandomFailed(
            "NitroRuntime random not yet implemented".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tee_type_is_aws_nitro() {
        // NitroRuntimeの直接構築（テスト用）
        let rt = NitroRuntime { _private: () };
        assert_eq!(rt.tee_type(), "aws_nitro");
    }

    #[test]
    fn new_returns_not_implemented() {
        let result = NitroRuntime::new();
        assert!(result.is_err());
    }
}
