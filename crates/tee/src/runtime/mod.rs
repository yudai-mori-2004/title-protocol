//! # TEEランタイム抽象化
//!
//! 仕様書 §6.4
//!
//! TEEの鍵生成・Attestation取得を抽象化するトレイト。
//! 環境変数 `TEE_RUNTIME` で実装を切り替える。
//!
//! 現在のランタイム実装:
//! - `mock` — ローカル開発・テスト用（メモリ内鍵生成）
//! - `nitro` — AWS Nitro Enclaves（NSM API経由）

pub mod mock;
pub mod nitro;

/// TEEランタイムのトレイト。
/// 仕様書 §6.4
pub trait TeeRuntime: Send + Sync {
    /// TEE種別を返す（signed_jsonの`tee_type`フィールドに使用）。
    /// 仕様書 §5.1 Step 4
    fn tee_type(&self) -> &str;

    /// Ed25519署名用キーペアを生成し、内部に保持する。
    /// 仕様書 §6.4 Step 1
    fn generate_signing_keypair(&self);

    /// X25519暗号化用キーペアを生成し、内部に保持する。
    /// 仕様書 §6.4 Step 1
    fn generate_encryption_keypair(&self);

    /// Attestation Documentを取得する。
    /// 仕様書 §5.2 Step 4.1
    fn get_attestation(&self) -> Vec<u8>;

    /// 署名用秘密鍵でデータに署名する。
    /// 仕様書 §5.1 Step 4
    fn sign(&self, message: &[u8]) -> Vec<u8>;

    /// 署名用公開鍵を取得する。
    /// 仕様書 §6.4
    fn signing_pubkey(&self) -> Vec<u8>;

    /// 暗号化用秘密鍵を取得する（ECDH用）。
    /// 仕様書 §6.4
    fn encryption_secret_key(&self) -> Vec<u8>;

    /// 暗号化用公開鍵を取得する。
    /// 仕様書 §6.4
    fn encryption_pubkey(&self) -> Vec<u8>;

    /// Tree用Ed25519キーペアを生成し、内部に保持する。
    /// 仕様書 §6.4 Step 2
    fn generate_tree_keypair(&self);

    /// Tree用公開鍵を取得する。
    /// 仕様書 §6.4 Step 2
    fn tree_pubkey(&self) -> Vec<u8>;

    /// Tree用秘密鍵でデータに署名する。
    /// 仕様書 §6.4 Step 2
    fn tree_sign(&self, message: &[u8]) -> Vec<u8>;
}
