# PQC Migration Guide

Task 21で構築したPQC-ready暗号抽象化レイヤーの上で、実際にPQCアルゴリズムを導入する手順。

## 前提

現在のアーキテクチャ（Task 21完了時点）:

| ペア | Trait | Phase 1実装 | PQC実装 |
|------|-------|------------|---------|
| 署名 | `Signer`/`Verifier` | Ed25519 | ML-DSA-65 (FIPS 204) |
| KEM | `Encapsulator`/`Decapsulator` | X25519-HKDF-SHA256 | ML-KEM-768 (FIPS 203) |
| AEAD | `Aead` | AES-256-GCM | AES-256-GCM（変更なし） |

変更不要な箇所:
- `open_request()` / `seal_for()` 合成関数: KEM+KDF+AEADの組み合わせは内部で完結
- `/verify`, `/sign` ハンドラ: `&dyn Signer`, `&dyn Decapsulator` 経由なのでアルゴリズム非依存
- `domain_tagged()`: アルゴリズムに依存しない
- AAD, ワイヤーフォーマット: `CryptoSuite` の `suite_id` で切り替え

## Step 1: Rust暗号実装の追加

### 1a. ML-DSA-65 署名 (`crates/crypto/src/impls/ml_dsa.rs`)

```rust
// pqcrypto-dilithium crate or ml-dsa crate
pub struct MlDsa65Signer { /* ML-DSA-65 signing key */ }
pub struct MlDsa65Verifier { /* ML-DSA-65 verification key */ }

impl Signer for MlDsa65Signer { ... }  // algorithm() -> SigningAlgorithm::MlDsa65
impl Verifier for MlDsa65Verifier { ... }
```

### 1b. ML-KEM-768 KEM (`crates/crypto/src/impls/ml_kem.rs`)

```rust
pub struct MlKem768Encapsulator { /* ML-KEM-768 public key */ }
pub struct MlKem768Decapsulator { /* ML-KEM-768 secret key */ }

impl Encapsulator for MlKem768Encapsulator { ... }
impl Decapsulator for MlKem768Decapsulator { ... }
```

### 1c. アルゴリズム識別子の追加 (`algorithm.rs`)

```rust
pub enum SigningAlgorithm {
    Ed25519,
    MlDsa65,        // 追加
}

pub enum KemAlgorithm {
    X25519HkdfSha256,
    MlKem768HkdfSha256,  // 追加
}

pub enum CryptoSuite {
    X25519Aes256Gcm = 0x01,
    MlKem768Aes256Gcm = 0x02,  // 追加
}
```

### 1d. Factory関数の拡張 (`factory.rs`)

```rust
pub fn create_signer(algorithm: SigningAlgorithm, seed: &[u8]) -> Result<...> {
    match algorithm {
        SigningAlgorithm::Ed25519 => /* 既存 */,
        SigningAlgorithm::MlDsa65 => Ok(Box::new(MlDsa65Signer::from_seed(seed))),
    }
}
// create_verifier, create_encapsulator, create_decapsulator も同様
```

## Step 2: TeeRuntime の鍵分離

Phase 1では `protocol_signer()` と `solana_signer()` が同一鍵を返す。
PQCでは分離が必要:

```rust
pub struct MockRuntime {
    protocol_signing: OnceLock<MlDsa65Signer>,    // PQC署名鍵（新規）
    solana_signing: OnceLock<Ed25519Signer>,       // Solana TX署名（Ed25519固定）
    decap: OnceLock<MlKem768Decapsulator>,         // PQC KEM鍵（新規）
    tree: OnceLock<Ed25519Signer>,                 // Solana TX用（Ed25519固定）
    ext_tree: OnceLock<Ed25519Signer>,             // Solana TX用（Ed25519固定）
}
```

注意: `solana_signer()`, `tree_signer()`, `ext_tree_signer()` は **Ed25519固定のまま**。
Solana TX署名はSolanaランタイムの制約（Ed25519のみ、TX最大1232B）により変更不可。

## Step 3: register_node.rs のアルゴリズムマッピング更新

```rust
fn signing_algorithm_to_u8(algo: &str) -> Result<u8, TeeError> {
    match algo {
        "ed25519" => Ok(0),
        "ml-dsa-65" => Ok(1),       // 追加
        _ => Err(TeeError::Internal(...)),
    }
}

fn kem_algorithm_to_u8(algo: &str) -> Result<u8, TeeError> {
    match algo {
        "x25519-hkdf-sha256" => Ok(0),
        "ml-kem-768-hkdf-sha256" => Ok(1),  // 追加
        _ => Err(TeeError::Internal(...)),
    }
}
```

## Step 4: Solana Program の TeeNodeAccount realloc

Phase 1の `MAX_SPACE` はEd25519/X25519の32B鍵を想定。
PQC鍵はサイズが大きい:

| アルゴリズム | 公開鍵サイズ |
|------------|-------------|
| Ed25519 | 32 B |
| ML-DSA-65 | 1,952 B |
| X25519 | 32 B |
| ML-KEM-768 | 1,184 B |

### 方法: `update_tee_node` に realloc を追加

```rust
#[derive(Accounts)]
pub struct UpdateTeeNode<'info> {
    #[account(
        mut,
        realloc = TeeNodeAccount::compute_space(
            &tee_node.protocol_signing_pubkey,
            &tee_node.encryption_pubkey,
            &tee_node.gateway_pubkey,
            &tee_node.gateway_endpoint,
            tee_node.measurements.len(),
        ),
        realloc::payer = authority,
        realloc::zero = false,
        seeds = [b"tee-node", tee_node.solana_pubkey.as_ref()],
        bump = tee_node.bump
    )]
    pub tee_node: Account<'info, TeeNodeAccount>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}
```

### 代替方法: remove + register（新しいスペースでPDA再作成）

既存TeeNodeAccountを `remove_tee_node` で削除し、新しい `register_tee_node` で再登録。
これが最もシンプルで、reallocの複雑さを避けられる。

## Step 5: TypeScript SDK

### 5a. crypto.ts: 新しいCryptoSuiteのサポート

```typescript
// Suite ID constants
const SUITE_X25519_AES256GCM = 0x01;
const SUITE_MLKEM768_AES256GCM = 0x02;  // 追加

// encryptPayload() の suite_id 決定ロジック
// TrustedTeeNode.encryption_algorithm を見て分岐
```

ただし、SDK側にML-KEM-768のJavaScript実装が必要。
候補: `@noble/post-quantum` (crystals-kyber/ml-kem)

### 5b. chain.ts: アルゴリズムマッピング追加

```typescript
const SIGNING_ALGORITHM_MAP: Record<number, string> = {
  0: "ed25519",
  1: "ml-dsa-65",     // 追加
};

const ENCRYPTION_ALGORITHM_MAP: Record<number, string> = {
  0: "x25519-hkdf-sha256",
  1: "ml-kem-768-hkdf-sha256",  // 追加
};
```

### 5c. wire format パース

SDKが受信レスポンスのワイヤーフォーマットをパースする場合、
`suite_id` を見てnonce_sizeを決定する必要がある。
AES-256-GCMを継続使用する限り、nonce_size=12で変更なし。

## Step 6: TEE main.rs の GATEWAY_SIGNING_ALGORITHM

`GATEWAY_SIGNING_ALGORITHM` 環境変数でGateway鍵のアルゴリズムを制御済み（Task 21で対応）。
PQC Gateway鍵を使う場合は `GATEWAY_SIGNING_ALGORITHM=ml-dsa-65` を設定。

## Step 7: デプロイ手順

1. Solanaプログラムをアップグレードデプロイ（TeeNodeAccount schema変更なし、realloc対応追加のみ）
2. 既存TeeNodeAccountを `remove_tee_node` で削除
3. PQC鍵で `register_tee_node` を再実行（新しいMAX_SPACE）
4. SDK更新: npm publish
5. EC2ノード: TEEバイナリ更新、環境変数にPQCアルゴリズム設定

## 変更不要な箇所（確認済み）

| コンポーネント | 理由 |
|-------------|------|
| `open_request()` / `seal_for()` | KEM traitを通すため、アルゴリズム非依存 |
| `verify/handler.rs` | `state.runtime.decapsulator()` で抽象化済み |
| `verify/core.rs`, `extension.rs` | `state.runtime.protocol_signer()` で抽象化済み |
| `sign/handler.rs` | `create_verifier(runtime.protocol_signing_algorithm(), ...)` で動的 |
| `domain_tagged()` | アルゴリズムに依存しない |
| `GlobalConfigAccount.trusted_node_keys` | `solana_pubkey` (Ed25519固定) のリスト |
| HKDF鍵導出 | salt/info構造はアルゴリズム非依存 |
| AAD | エンドポイントパスベース、アルゴリズム非依存 |

## Solana PQC 将来対応

Solana自体がPQC対応を発表した場合:
- `solana_pubkey` フィールドを可変長に拡張（新フィールド `solana_pqc_pubkey: Option<Vec<u8>>` を追加）
- PDA seedは引き続き `[b"tee-node", &solana_pubkey_32bytes]` を使用（互換性維持）
- Solana TX署名は Solana SDK の更新に追従
