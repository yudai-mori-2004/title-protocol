# タスク11: 暗号化（E2EE）

## 目的

クライアントから TEE へのコンテンツ暗号化オプション（§2.4）を実装する。鍵束の生成、ワイヤーフォーマット、HKDF による方向別鍵導出、3つの暗号スイートを実装する。

## 読むべきファイル

1. `CLAUDE.md`
2. `docs/v0.1.2/SPECS_JA.md` — **§2.4 全体を精読**:
   - 鍵束（起動時に各スイートの鍵ペア生成）
   - 暗号化フロー（12ステップ）
   - 方向別鍵導出（HKDF-SHA256, info="title-request-key" / "title-response-key", salt=encap_key）
   - ワイヤーフォーマット（リクエスト: suite_id + encap_key_len + encap_key + nonce + ciphertext）
   - ワイヤーフォーマット（レスポンス: nonce + ciphertext のみ）
   - 暗号化ペイロード内部構造（metadata_len + JSON + raw binary）
   - 対応スイート: x25519(0x01), p256(0x02), ml-kem-768(0x03)
   - §1.4 暗号化はオプション（encryption 省略 = 平文）
3. `docs/v0.1.2/COVERAGE.md`
4. `crates/core/src/request.rs` — EncryptionSuite, EncryptedPayloadMetadata（Task 02 成果物）
5. `legacy/v0.1.0/crates/crypto/` — **前バージョンの暗号プリミティブ。AES-GCM, HKDF, X25519 の実装がある。wire.rs にワイヤーフォーマットの実装がある。sealed_channel.rs に方向別鍵導出がある。**

## スコープ

### やること

1. **鍵束管理**:
   - TEE 起動時に各スイートの鍵ペアを生成
   - 秘密鍵は TEE メモリ内のみ（TeeRuntime::random_bytes で seed 生成）
   - 公開鍵の Gateway への通知用構造体

2. **暗号スイート実装**:
   - `x25519`: X25519 ECDH + HKDF-SHA256 + AES-256-GCM
   - `p256`: ECDH P-256 + HKDF-SHA256 + AES-256-GCM
   - `ml-kem-768`: ML-KEM-768 (FIPS 203) + HKDF-SHA256 + AES-256-GCM

3. **方向別鍵導出**:
   - `request_key = HKDF-SHA256(shared_secret, info="title-request-key", salt=encap_key)`
   - `response_key = HKDF-SHA256(shared_secret, info="title-response-key", salt=encap_key)`

4. **ワイヤーフォーマット**:
   - リクエスト: `[suite_id(1B)][encap_key_len(2B BE)][encap_key][nonce(12B)][ciphertext]`
   - レスポンス: `[nonce(12B)][ciphertext]`
   - パース + 構築ユーティリティ

5. **ペイロード内部構造**:
   - 平文: `[4B: metadata_len BE u32][metadata JSON][raw content]`
   - メタデータ: `{"signature_hash": "sha256:..."}`
   - パース + 構築ユーティリティ

6. **テスト**:
   - 各スイートの暗号化→復号ラウンドトリップ
   - 方向別鍵導出（request_key ≠ response_key）
   - ワイヤーフォーマットのパース・構築
   - ペイロード内部構造のパース・構築
   - 不正スイートID / 切り詰めデータへのエラー

### やらないこと

- signature_hash のクライアント側照合ロジック（アプリケーション層）
- Gateway 経由のペイロード改ざん検知（§1.7 の議論はドキュメントのみ）

## 依存

- Task 02: EncryptionSuite, EncryptedPayloadMetadata
- Task 04: TEE オーケストレーションへの統合

## 成功基準

- [ ] 3スイート全てで暗号化→復号が動作する
- [ ] 方向別鍵導出が正しく分離される
- [ ] ワイヤーフォーマットが仕様と一致する
- [ ] `cargo test` 合格
- [ ] COVERAGE.md 更新
