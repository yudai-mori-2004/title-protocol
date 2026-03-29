# Task 12: 暗号化ペイロードのバイナリプロトコル化

## 背景

SDK → TempStorage → TEE 間の暗号化ペイロードが JSON + Base64 で転送されており、モバイル環境でBase64変換がボトルネックになっている。

### 現行フォーマット（JSON + Base64）

```
Client → S3: Content-Type: application/json
{
  "ephemeral_pubkey": "base64(32B)",
  "nonce": "base64(12B)",
  "ciphertext": "base64(AES-GCM(base64(content) + metadata_json))"
}
```

5MBのファイルが3.4倍に膨張する:
- content: 5MB → Base64: 7MB → JSON embedding: 13MB → encrypt: 13MB → Base64: 17MB

### 提案フォーマット（バイナリ）

```
Client → S3: Content-Type: application/octet-stream

[32B: ephemeral_pubkey][12B: nonce][remaining: AES-GCM ciphertext + 16B tag]
```

暗号化前の平文フォーマット:
```
[4B: metadata_len (big-endian u32)]
[metadata_len bytes: JSON {"owner_wallet":"...","extension_inputs":{...}}]
[remaining: raw content bytes]
```

5MBのファイル → 5MB + 60B（固定ヘッダ）。膨張なし。

### セキュリティ検証

- AES-256-GCM の認証付き暗号化はBase64の有無に無関係。改竄検知は暗号アルゴリズムが保証
- ephemeral_pubkey と nonce は元々平文で公開されていた（JSONでもBase64で送信）
- E2EE の保護対象は ciphertext の中身。外側のエンコーディングは暗号学的に無関係
- TempStorage（S3）へのアクセスは presigned URL で制御済み
- ECDH鍵交換、HKDF鍵導出、AES-GCMアルゴリズムは一切変更なし

## 作業内容

### SDK (`sdk/ts/`)

| ファイル | 変更内容 |
|---------|---------|
| `src/types.ts` | `EncryptedPayload` を内部型に。新型 `BinaryEncryptedPayload = Uint8Array` |
| `src/crypto.ts` | `encryptPayload()` がバイナリを返すように変更。`CryptoProvider.toBase64/fromBase64` は小データ用に残す |
| `src/client.ts` | `upload()` が `application/octet-stream` で送信。`register()` 内の content Base64 変換を廃止 |

### TEE (`crates/tee/`)

| ファイル | 変更内容 |
|---------|---------|
| `src/endpoints/verify/handler.rs` | ダウンロード後のパース: `serde_json::from_slice` → バイナリヘッダ読み取り。content の Base64 デコード廃止 |

### Types (`crates/types/`)

| ファイル | 変更内容 |
|---------|---------|
| `src/lib.rs` | `ClientPayload.content` を `String`(Base64) → バイナリ前提に変更。`EncryptedPayload` にバイナリパース関数追加 |

### Gateway (`crates/gateway/`)

変更なし。presigned URL を生成するだけで暗号化ペイロードの中身には触れない。

### Proxy (`crates/proxy/`)

変更なし。バイト列をそのまま中継する。

### レスポンス（TEE → Client）

/verify レスポンスは小さい（数KB）ので JSON + Base64 のまま据え置き。

## 後方互換性

- SDK 0.1.8 以前のクライアントは JSON + Base64 で送信する
- TEE は Content-Type または先頭バイトで JSON/バイナリを判別して両方受け付ける
- 段階的移行: SDK更新 → しばらく両対応 → 旧フォーマット廃止

## 効果

| | JSON + Base64 | バイナリ |
|---|---|---|
| 5MB content のペイロードサイズ | ~17MB | ~5MB |
| SDK側 Base64 変換回数 | 3回（content + ciphertext + payload JSON） | 0回 |
| TEE側 Base64 デコード回数 | 3回（ephemeral_pubkey + nonce + ciphertext + content） | 0回 |
| モバイル暗号化時間（推定） | 8.4s（大半がBase64変換） | <500ms |
| アップロード時間（推定） | 6.2s（17MB over WiFi） | ~2s（5MB over WiFi） |

## 読むべきファイル

| ファイル | 理由 |
|---------|------|
| `sdk/ts/src/crypto.ts` | `encryptPayload()`, `CryptoProvider` |
| `sdk/ts/src/client.ts` | `register()`, `upload()` |
| `sdk/ts/src/types.ts` | `EncryptedPayload`, `ClientPayload` |
| `crates/tee/src/endpoints/verify/handler.rs` | ダウンロード→復号→パースの全フロー |
| `crates/types/src/lib.rs` | `EncryptedPayload`, `ClientPayload` 型定義 |
| `crates/crypto/src/lib.rs` | `aes_gcm_decrypt`, `ecdh_derive_shared_secret` |
| `crates/tee/src/infra/security.rs` | `proxy_get_secured()` |
| `integration-tests/register-photo.ts` | 手動テストスクリプト（低レベルAPI使用、バイナリ対応が必要） |
| `tests/e2e/` | E2Eテスト（あれば確認） |

## 完了条件

- [ ] バイナリ暗号化ペイロードフォーマット定義（仕様書更新）
- [ ] SDK `encryptPayload()` がバイナリを返す
- [ ] SDK `upload()` が `application/octet-stream` で送信
- [ ] SDK `register()` 内の content Base64 変換を廃止
- [ ] TEE がバイナリフォーマットをパース・復号
- [ ] TEE が JSON/バイナリ両方を受け付ける（後方互換）
- [ ] `CryptoProvider` インターフェース更新
- [ ] `cargo check --workspace && cargo test --workspace` 通過
- [ ] `npm test` 通過
- [ ] devnet ノード更新・動作確認
- [ ] SDK npm publish
