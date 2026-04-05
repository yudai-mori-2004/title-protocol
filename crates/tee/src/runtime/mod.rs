// SPDX-License-Identifier: Apache-2.0

//! # TEEランタイム抽象化
//!
//! 仕様書 §6.4
//!
//! TEEの鍵生成・署名・復号・Attestation取得を抽象化するトレイト。
//! 暗号処理は `title_crypto` の3ペアtrait（Signer/Verifier, Encapsulator/Decapsulator, Aead）に基づく。
//!
//! ランタイム実装:
//! - `mock` — ローカル開発・テスト用（メモリ内鍵生成）
//! - （ベンダー実装はfeature flagで追加される）

pub mod mock;
#[cfg(feature = "vendor-aws")]
pub mod nitro;

use title_crypto::signing::Signer;
use title_crypto::kem::Decapsulator;
use title_crypto::{SigningAlgorithm, KemAlgorithm};

/// TEEランタイムのトレイト。
/// 仕様書 §6.4
///
/// 暗号鍵は `OnceLock` で保持し、`generate_keypairs()` で一度だけ生成する。
/// `protocol_signer()` / `solana_signer()` / `decapsulator()` は `&dyn Trait` を返す。
pub trait TeeRuntime: Send + Sync {
    /// TEE種別を返す（signed_jsonの`tee_type`フィールドに使用）。
    /// 仕様書 §5.1 Step 4
    fn tee_type(&self) -> &str;

    /// 全キーペアを生成し、内部に保持する。
    /// 仕様書 §6.4 Step 1
    fn generate_keypairs(&self);

    /// Attestation Documentを取得する。
    /// 仕様書 §5.2 Step 4.1
    fn get_attestation(&self) -> Vec<u8>;

    /// Protocol署名用Signer。signed_jsonの署名に使用。
    /// PQC対応時にアルゴリズムが変更される。
    /// 仕様書 §5.1 Step 4
    fn protocol_signer(&self) -> &dyn Signer;

    /// Solana TX署名用Signer。Ed25519固定（Solana制約）。
    /// 仕様書 §6.4
    fn solana_signer(&self) -> &dyn Signer;

    /// Core Merkle Tree署名用Signer。Ed25519固定（Solana制約）。
    /// 仕様書 §6.4 Step 2, §6.5
    fn tree_signer(&self) -> &dyn Signer;

    /// Extension Merkle Tree署名用Signer。Ed25519固定（Solana制約）。
    /// 仕様書 §6.4 Step 2, §6.5
    fn ext_tree_signer(&self) -> &dyn Signer;

    /// KEM Decapsulator。E2EEペイロードの復号に使用。
    /// PQC対応時にアルゴリズムが変更される。
    /// 仕様書 §6.4
    fn decapsulator(&self) -> &dyn Decapsulator;

    /// Protocol署名アルゴリズム。
    fn protocol_signing_algorithm(&self) -> SigningAlgorithm;

    /// KEMアルゴリズム。
    fn kem_algorithm(&self) -> KemAlgorithm;
}
