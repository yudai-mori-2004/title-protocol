# タスク1: MockRuntime実装

## 読むべきファイル

1. `docs/SPECS_JA.md` — §6.4「鍵管理」「TEE起動シーケンス」のみ読む
2. `crates/tee/src/runtime/mod.rs` — TeeRuntime trait定義
3. `crates/tee/src/runtime/mock.rs` — 現在のスタブ
4. `crates/tee/src/main.rs` — AppStateとの統合方法を確認
5. `crates/crypto/src/lib.rs` — 利用可能な暗号プリミティブ（実装済み）

## 作業内容

`crates/tee/src/runtime/mock.rs` の MockRuntime を実装する。

### TeeRuntime trait の改修

現在のtraitシグネチャは戻り値がない。以下のように変更する:

```rust
pub trait TeeRuntime: Send + Sync {
    /// Ed25519署名用キーペアを生成し、内部に保持する。
    fn generate_signing_keypair(&self);

    /// X25519暗号化用キーペアを生成し、内部に保持する。
    fn generate_encryption_keypair(&self);

    /// Attestation Documentを取得する。
    fn get_attestation(&self) -> Vec<u8>;

    /// 署名用秘密鍵でデータに署名する。
    fn sign(&self, message: &[u8]) -> Vec<u8>;

    /// 署名用公開鍵を取得する。
    fn signing_pubkey(&self) -> Vec<u8>;

    /// 暗号化用秘密鍵を取得する（ECDH用）。
    fn encryption_secret_key(&self) -> Vec<u8>;

    /// 暗号化用公開鍵を取得する。
    fn encryption_pubkey(&self) -> Vec<u8>;
}
```

### MockRuntime実装の要件

- `generate_signing_keypair()`: `ed25519-dalek` で `OsRng` からキーペア生成。内部の `RwLock<Option<SigningKey>>` に保持
- `generate_encryption_keypair()`: `x25519-dalek` で `StaticSecret` 生成。内部の `RwLock<Option<StaticSecret>>` に保持
- `get_attestation()`; 固定のモックAttestation Document（バイト列）を返す。PCR値は全てゼロ（Nitroのdebug-modeと同等）。実態は `serde_json::to_vec` でJSON化した構造体で可
- `sign()`: 保持している `SigningKey` で署名
- `signing_pubkey()`: `VerifyingKey` をバイト列で返す
- `encryption_secret_key()`: `StaticSecret` のバイト列を返す
- `encryption_pubkey()`: `PublicKey` のバイト列を返す

### 注意

- `main.rs` での `generate_signing_keypair()` / `generate_encryption_keypair()` 呼び出しは既存。traitシグネチャ変更に伴い `NitroRuntime` のスタブも合わせて更新すること（`todo!()` のままでよい）
- `crates/crypto` の関数は直接使ってもよいし、dalek系クレートを直接使ってもよい

## 完了条件

- `cargo check --workspace` が通る
- `cargo test -p title-tee` でMockRuntimeのテストが通る
- テスト: 鍵ペア生成→署名→検証のラウンドトリップ
- テスト: 暗号化用鍵ペアのECDH鍵交換（共通鍵導出の一致確認）
- `docs/COVERAGE.md` の該当箇所を更新
