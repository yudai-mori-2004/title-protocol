//! # AWS Nitro Enclaves ランタイム実装
//!
//! 仕様書 §6.4
//!
//! AWS Nitro Enclaves上で動作するTEEランタイム。
//! NSM (Nitro Security Module) APIを使用して鍵生成とAttestation取得を行う。

use super::TeeRuntime;

/// AWS Nitro Enclaves ランタイム。
pub struct NitroRuntime {
    // TODO: NSMデバイスファイルディスクリプタ
    // TODO: Ed25519署名用キーペア
    // TODO: X25519暗号化用キーペア
    // TODO: Tree用Ed25519キーペア
}

impl NitroRuntime {
    /// NitroRuntimeを初期化する。
    pub fn new() -> Self {
        Self {}
    }
}

impl TeeRuntime for NitroRuntime {
    /// NSM API経由でEd25519署名用キーペアを生成する。
    /// 仕様書 §6.4 Step 1
    fn generate_signing_keypair(&self) {
        todo!("NSM APIを使用したEd25519キーペア生成")
    }

    /// NSM API経由でX25519暗号化用キーペアを生成する。
    /// 仕様書 §6.4 Step 1
    fn generate_encryption_keypair(&self) {
        todo!("NSM APIを使用したX25519キーペア生成")
    }

    /// NSM APIからAttestation Documentを取得する。
    /// 仕様書 §5.2 Step 4.1
    fn get_attestation(&self) -> Vec<u8> {
        todo!("NSM APIからAttestation Document取得")
    }
}
