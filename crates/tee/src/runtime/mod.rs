//! # TEEランタイム抽象化
//!
//! 仕様書 §6.4
//!
//! TEEの鍵生成・Attestation取得を抽象化するトレイト。
//! AWS Nitro実装とローカル開発用モック実装を提供する。

pub mod mock;
pub mod nitro;

/// TEEランタイムのトレイト。
/// 仕様書 §6.4
pub trait TeeRuntime: Send + Sync {
    /// Ed25519署名用キーペアを生成する。
    /// 仕様書 §6.4 Step 1
    fn generate_signing_keypair(&self);

    /// X25519暗号化用キーペアを生成する。
    /// 仕様書 §6.4 Step 1
    fn generate_encryption_keypair(&self);

    /// Attestation Documentを取得する。
    /// 仕様書 §5.2 Step 4.1
    fn get_attestation(&self) -> Vec<u8>;
}
