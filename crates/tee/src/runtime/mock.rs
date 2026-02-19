//! # ローカル開発用モックランタイム
//!
//! 仕様書 §6.4
//!
//! TEEハードウェアが利用できない開発環境で使用するモック実装。
//! メモリ内で鍵を生成し、固定のAttestation Documentを返す。

use super::TeeRuntime;

/// モックTEEランタイム。ローカル開発・テスト用。
pub struct MockRuntime {
    // TODO: Ed25519署名用キーペア（メモリ内生成）
    // TODO: X25519暗号化用キーペア（メモリ内生成）
    // TODO: Tree用Ed25519キーペア（メモリ内生成）
}

impl MockRuntime {
    /// MockRuntimeを初期化する。
    pub fn new() -> Self {
        Self {}
    }
}

impl TeeRuntime for MockRuntime {
    /// メモリ内でEd25519署名用キーペアを生成する。
    fn generate_signing_keypair(&self) {
        todo!("メモリ内Ed25519キーペア生成（モック）")
    }

    /// メモリ内でX25519暗号化用キーペアを生成する。
    fn generate_encryption_keypair(&self) {
        todo!("メモリ内X25519キーペア生成（モック）")
    }

    /// 固定のAttestation Documentを返す（モック）。
    fn get_attestation(&self) -> Vec<u8> {
        todo!("固定Attestation Document返却（モック）")
    }
}
