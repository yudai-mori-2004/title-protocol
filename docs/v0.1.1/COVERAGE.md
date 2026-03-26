# v0.1.1 カバレッジレポート

v0.1.0 を基準とし、v0.1.1 での変更のみを追跡する。

## 変更サマリー

| カテゴリ | 変更内容 |
|---------|---------|
| ドキュメント体系 | Diataxis フレームワークに基づく7ファイル再構成 |
| Solana プログラム | `register_tee_node` / `remove_tee_node` にコレクション権限委譲を統合 |
| TEE エンドポイント | `/register-node` リクエストにコレクションアドレスを追加 |
| CLI | `register-node` / `remove-node` がコレクションアドレスを送信 |
| デプロイスクリプト | `setup-ec2.sh` の環境変数書き込み修正 |
| WASM 実行環境 | ホスト側コンテンツデコード関数 + ResourcePool（統合セマフォ）+ Feature Host Functions |
| WASM Extension | phash-v1 を dHash → pHash (DCT) に移行、ホスト側デコード活用、get_decoded_feature で grayscale_resize をホスト委譲 |
| 仕様書 | §7.1 ホスト関数追加・ResourcePool仕様・Feature Host Functions、§6.4 三層防御更新、§7.4 pHash アルゴリズム更新 |

---

## ドキュメント再構成

旧 `QUICKSTART.md`（682行、全部入り）を Diataxis フレームワークで分解:

| ファイル | Diataxis 種別 | 対象読者 |
|---------|--------------|---------|
| `QUICKSTART.md` | Tutorial | 新規来訪者 |
| `docs/architecture.md` | Explanation | 全員 |
| `docs/reference.md` | Reference | オペレーター / 開発者 |
| `docs/troubleshooting.md` | How-to | オペレーター |
| `programs/title-config/README.md` | How-to | プロトコル管理者 |
| `deploy/local/README.md` | How-to | ノードオペレーター |
| `deploy/aws/README.md` | How-to | ノードオペレーター |

---

## §8 ガバナンス — コレクション権限委譲の原子化

v0.1.0 では `delegate_collection_authority` / `revoke_collection_authority` が独立した Anchor 命令として存在していた。

v0.1.1 でこれらを `register_tee_node` / `remove_tee_node` に統合し、MPL Core CPI として1トランザクション内で不可分に実行する設計に変更。

**不変条件:** `GlobalConfig.trusted_node_keys == コレクションの UpdateDelegate.additional_delegates`

| 変更前（v0.1.0） | 変更後（v0.1.1） |
|-----------------|-----------------|
| `register_tee_node` — ノード登録のみ | `register_tee_node` — ノード登録 + コレクション権限委譲（MPL Core CPI） |
| `remove_tee_node` — ノード削除のみ | `remove_tee_node` — ノード削除 + コレクション権限取消（MPL Core CPI） |
| `delegate_collection_authority` — 独立命令 | 削除（register_tee_node に統合） |
| `revoke_collection_authority` — 独立命令 | 削除（remove_tee_node に統合） |

### 変更ファイル

| ファイル | 変更内容 |
|---------|---------|
| `programs/title-config/src/lib.rs` | CPI ヘルパー追加、RegisterTeeNode / RemoveTeeNode コンテキストにコレクションアカウント追加 |
| `crates/types/src/lib.rs` | `RegisterNodeRequest` に `core_collection_mint` / `ext_collection_mint` 追加 |
| `crates/tee/src/endpoints/register_node.rs` | コレクションアドレスのパース、命令アカウント 5→8 に拡張 |
| `crates/cli/src/commands/register_node.rs` | `network.json` からコレクションアドレスを送信 |
| `crates/cli/src/commands/remove_node.rs` | コレクションアドレスを `build_remove_tee_node_ix` に渡す |
| `crates/cli/src/anchor.rs` | `build_remove_tee_node_ix` にコレクションアカウント追加、独立命令ビルダー削除 |

---

## デプロイスクリプト修正

| ファイル | 変更内容 |
|---------|---------|
| `deploy/aws/setup-ec2.sh` | `ensure_env "CORE_COLLECTION_MINT"` / `ensure_env "EXT_COLLECTION_MINT"` を追加。`network.json` の値を `.env` に書き込む |

---

## §7.1 / §6.4 — ResourcePool統合（セマフォアーキテクチャ統一）

v0.1.1 Task 02 で導入した `MemoryPool`（セマフォB）と既存の `tokio::Semaphore`（セマフォA）を、CASベースの単一 `ResourcePool` に統合。

### 解決した問題

| 問題 | 旧設計 | 新設計 |
|------|--------|--------|
| デコード中ヒープピーク過小見積もり | `w×h×1`（grayscale出力）で予約、実ピーク4倍 | ビット深度考慮のピーク推定（8bit=native_bpp, 16/32bit=native_bpp+3） |
| ホスト側不要変換 | `to_luma8()` で中間バッファ発生 | ネイティブフォーマットでデコード、grayscale変換はWASM側 |
| A/B統合不在 | `tokio::Semaphore` + `MemoryPool` が独立 | 単一 `ResourcePool` + `Ticket`（Drop自動解放） |
| ダウンロード無制限占有 | チャンクタイムアウトのみ（積算で長時間占有可能） | ダウンロード全体にグローバルタイムアウト適用 |
| handler.rsメモリピーク | 中間表現が全スコープ存続 | 使用後即drop（proxy_response, ciphertext, plaintext, content文字列） |
| デコーダー密結合 | lib.rs内に画像固有ロジック | decode.rsにコンテンツ種別非依存の抽象化（DecoderKind + サブモジュール） |

### 変更ファイル

| ファイル | 変更内容 |
|---------|---------|
| `crates/wasm-host/src/resource_pool.rs` | 新規作成。`ResourcePool` + `Ticket`（CASベース統合セマフォ） |
| `crates/wasm-host/src/memory_pool.rs` | 削除（`resource_pool.rs` に統合） |
| `crates/wasm-host/src/decode.rs` | 新規作成。コンテンツ種別非依存のデコーダー抽象化（DecoderKind、ビット深度考慮ピーク推定） |
| `crates/wasm-host/src/lib.rs` | `decode_content` API変更（ネイティブデコード、decode.rsに委譲）、`execute_inner`パラメータ所有権移動（clone削除） |
| `crates/tee/src/config.rs` | `memory_semaphore` + `wasm_memory_pool` → `resource_pool` |
| `crates/tee/src/infra/security.rs` | `SemaphoreGuard` 削除、Ticket返却パターンに変更 |
| `crates/tee/src/main.rs` | ResourcePool初期化に統一 |
| `crates/tee/src/endpoints/verify/handler.rs` | download Ticket保持パターン、ダウンロードグローバルタイムアウト、中間変数早期drop |
| `crates/tee/src/endpoints/verify/extension.rs` | `with_resource_pool` に変更 |
| `crates/tee/src/endpoints/sign/handler.rs` | download Ticket保持パターン、ダウンロードグローバルタイムアウト |
| `wasm/phash-v1/src/lib.rs` | `decode_content` 3引数化、WASM側grayscale変換追加、channelsバリデーション追加 |
| `docs/v0.1.1/SPECS_JA.md` | §6.4 三層防御・漸進的予約、§7.1 decode_content・ResourcePool・ABIテーブル更新 |

---

## §7.1 — Feature Host Functions（特徴量計算ホスト関数）

`hash_content` を汎用的な `get_content_feature`（JSON spec指定）に置き換え、新規 `get_decoded_feature` を追加。pHash計算のfuel消費を劇的に削減。

### 変更内容

| 変更 | 旧 | 新 |
|------|-----|-----|
| コンテンツハッシュ | `hash_content(algorithm, offset, length, out_ptr) -> u32` | `get_content_feature(spec_ptr, spec_len, output_ptr) -> i32` — JSON specで op/offset/length を指定 |
| デコード済み特徴量 | なし（WASM側で全ピクセル転送+処理） | `get_decoded_feature(spec_ptr, spec_len, output_ptr) -> i32` — grayscale_resize等をホスト側で実行 |
| pHash処理フロー | decode → read全ピクセル → WASM側grayscale → WASM側resize → DCT | decode → get_decoded_feature(grayscale_resize 32x32) → DCTのみ |
| DecodedContent | `data` のみ | `data` + `width` + `height` + `channels` |

### 変更ファイル

| ファイル | 変更内容 |
|---------|---------|
| `crates/wasm-host/src/lib.rs` | `DecodedContent` に w/h/ch 追加、`hash_content` → `get_content_feature`、`get_decoded_feature` 追加、全WATテスト更新 |
| `wasm/phash-v1/src/lib.rs` | `get_decoded_feature` 使用、`read_all_decoded`/`rgb_to_grayscale`/`resize_bilinear` 削除、`compute_phash_dct` 簡素化 |
| `wasm/hardware-google/src/lib.rs` | extern宣言: `hash_content` → `get_content_feature` |
| `wasm/c2pa-training-v1/src/lib.rs` | 同上 |
| `wasm/c2pa-license-v1/src/lib.rs` | 同上 |
| `crates/tee/src/endpoints/verify/tests.rs` | WATテスト: `hash_content` → `get_content_feature` |
| `docs/v0.1.1/SPECS_JA.md` | §7.1 ホスト関数ABIテーブル・特徴量計算セクション更新 |

---

## §5.1 / §6.4 — Address Lookup Table (ALT) によるTX圧縮

全トランザクションを VersionedTransaction (v0) に統一し、MintV2 TX に ALT を適用。

### 効果

| | 旧（Legacy TX） | 新（VersionedTransaction + ALT） |
|---|---|---|
| 2 cNFT TX サイズ | ~1,024 bytes | ~750 bytes |
| 最大 cNFT/TX | 2 | 4 |
| TX フォーマット | Legacy Transaction | VersionedTransaction (v0) |

### 変更ファイル

| ファイル | 変更内容 |
|---------|---------|
| `crates/tee/src/blockchain/solana_tx.rs` | 全TX を VersionedTransaction (v0) に統一、ALT ビンパッキング |
| `crates/tee/src/config.rs` | `alt_address`, `alt_addresses` 追加 |
| `crates/tee/src/endpoints/sign/handler.rs` | ALT 参照、VersionedTransaction 返却 |
| `crates/tee/src/endpoints/set_alt.rs` | `/set-alt` エンドポイント新設 |
| `crates/tee/src/endpoints/create_tree.rs` | VersionedTransaction 対応 |
| `crates/tee/src/endpoints/register_node.rs` | VersionedTransaction 対応 |
| `crates/gateway/src/endpoints/sign_and_mint.rs` | VersionedTransaction 対応 |
| `crates/cli/src/commands/create_alt.rs` | `title-cli create-alt` サブコマンド新設 |
| `crates/cli/src/commands/register_node.rs` | VersionedTransaction 対応 |
| `integration-tests/register-photo.ts` | VersionedTransaction deserialize/sign |
| `deploy/local/setup.sh` | ALT 作成ステップ追加 |
| `deploy/aws/setup-ec2.sh` | ALT 作成ステップ追加 |

---

## §6.7 — SDK ノード選択改善 + CryptoProvider 抽象化

### selectNode() 並列レース化

| | 旧 | 新 |
|---|---|---|
| アルゴリズム | ランダム逐次（1ノードずつ） | `Promise.any` 並列レース（バッチ単位） |
| 死んだノードに当たった場合 | ~10秒待ち（fetch タイムアウト） | 影響なし（生きたノードが即勝つ） |
| 同時リクエスト上限 | 1 | `HEALTH_CHECK_BATCH_SIZE = 8` |
| 個別タイムアウト | なし | `HEALTH_CHECK_TIMEOUT_MS = 5000` |

### CryptoProvider インターフェース

AES-256-GCM + Base64 をプラットフォーム非依存に抽象化。デフォルトは `crypto.subtle` + `Buffer`（Web標準）。

| メソッド | 役割 |
|----------|------|
| `encrypt(key, plaintext)` | AES-256-GCM 暗号化（12バイトnonce自動生成） |
| `decrypt(key, nonce, ciphertext)` | AES-256-GCM 復号 |
| `toBase64(bytes)` | バイト列→Base64文字列 |
| `fromBase64(str)` | Base64文字列→バイト列 |

### 変更ファイル

| ファイル | 変更内容 |
|---------|---------|
| `sdk/ts/src/crypto.ts` | `CryptoProvider` IF + `defaultCryptoProvider` + 全関数にprovider引数 |
| `sdk/ts/src/client.ts` | `TitleClientOptions` + `selectNode()` 並列化 + `register()` でprovider使用 |
| `sdk/ts/package.json` | `0.1.6` → `0.1.7` |

---

## タスク一覧

| タスク | 内容 | 状態 |
|-------|------|------|
| [01-node-operator-docs](tasks/01-node-operator-docs/README.md) | ドキュメント体系再設計 + コレクション権限委譲統合 + 環境変数修正 | 完了 |
| [02-wasm-decode-host](tasks/02-wasm-decode-host/README.md) | WASM ホスト側デコード + メモリプール + pHash (DCT) | 完了 |
| [03-resource-pool-unification](tasks/03-resource-pool-unification/README.md) | ResourcePool統合 — セマフォアーキテクチャ統一 | 完了 |
| [04-feature-host-functions](tasks/04-feature-host-functions/README.md) | Feature Host Functions — get_content_feature / get_decoded_feature | 完了 |
| [05-signed-json-storage](tasks/05-signed-json-storage/README.md) | Gateway signed_json ストレージ委譲（sign-and-mint） | 完了 |
| [06-exif-orientation](tasks/06-exif-orientation/README.md) | EXIF Orientation 正規化 | 完了 |
| [07-wasm-pda-management](tasks/07-wasm-pda-management/README.md) | WASM モジュール PDA 管理 + OnChainLoader | 完了 |
| [08-performance-optimization](tasks/08-performance-optimization/README.md) | パフォーマンス最適化（並列化・キャッシュ・ビンパッキング） | 完了 |
| [09-address-lookup-table](tasks/09-address-lookup-table/README.md) | ALT による TX 圧縮 + VersionedTransaction 統一 | 完了 |
| [10-release-preparation](tasks/10-release-preparation/README.md) | v0.1.1 リリース準備 — ドキュメント精査 + ゼロベース検証 | 進行中 |
| [11-sdk-node-selection-crypto-provider](tasks/11-sdk-node-selection-crypto-provider/README.md) | SDK ノード選択改善 + CryptoProvider 抽象化 | 完了 |
| [12-binary-encrypted-payload](tasks/12-binary-encrypted-payload/README.md) | 暗号化ペイロードのバイナリプロトコル化（Base64膨張排除） | 完了 |
| [13-spec-code-sync](tasks/13-spec-code-sync/README.md) | 仕様書・コード同期（update_authority、wasm_hash検証） | 完了 |
| [14-sdk-wasm-module-symmetry](tasks/14-sdk-wasm-module-symmetry/README.md) | SDK WasmModule / TeeNode 対称性修正 | 完了 |
| [15-tsa-key-management](tasks/15-tsa-key-management/README.md) | TSA鍵管理 — CLI + ドキュメント整備 | 未着手 |

---

## §5.1 — 暗号化ペイロードのバイナリプロトコル

JSON + Base64 を全廃し、暗号化ペイロードを完全バイナリ化。

### ワイヤーフォーマット（S3上）

```
[32B: ephemeral_pubkey (X25519)]
[12B: nonce (AES-GCM)]
[remaining: AES-GCM ciphertext + 16B auth tag]
```

Content-Type: `application/octet-stream`

### 平文フォーマット（復号後）

```
[4B: metadata_len (big-endian u32)]
[metadata_len bytes: JSON {"owner_wallet":"...","extension_inputs":{...}}]
[remaining: raw content bytes]
```

### 効果

| | 旧（JSON + Base64） | 新（バイナリ） |
|---|---|---|
| 5MB content のペイロードサイズ | ~17MB | ~5MB |
| SDK側 Base64 変換回数 | 3回 | 0回 |
| TEE側 Base64 デコード回数 | 4回 | 0回 |

### 型の変更

| 旧 | 新 | 備考 |
|---|---|---|
| `EncryptedPayload` (struct/interface) | 削除 | バイナリは `parse_encrypted_payload()` でパース |
| `ClientPayload` (Rust) | `ClientMetadata` | `content: String` を除去、メタデータのみ |
| `ClientPayload` (TS) | `ClientMetadata` | 同上 |
| `encryptPayload()` → `EncryptedPayload` | → `Uint8Array` | バイナリblob返却 |
| `upload(EncryptedPayload)` | `upload(Uint8Array)` | `application/octet-stream` |

### 変更ファイル

| ファイル | 変更内容 |
|---------|---------|
| `crates/types/src/lib.rs` | `EncryptedPayload` 削除、`ClientPayload` → `ClientMetadata`、`parse_encrypted_payload()` / `parse_plaintext_payload()` 追加 |
| `crates/tee/src/endpoints/verify/handler.rs` | バイナリヘッダパース + 平文パース、Base64デコード全廃 |
| `crates/tee/src/endpoints/verify/tests.rs` | バイナリ形式でペイロード構築 |
| `sdk/ts/src/types.ts` | `EncryptedPayload` → 削除、`ClientPayload` → `ClientMetadata` |
| `sdk/ts/src/crypto.ts` | `encryptPayload()` → `Uint8Array` 返却、`buildPlaintext()` 追加 |
| `sdk/ts/src/client.ts` | `register()` バイナリ平文構築、`upload()` → `application/octet-stream` |
| `sdk/ts/src/__tests__/crypto.test.ts` | バイナリE2Eテスト + `buildPlaintext` テスト |
| `sdk/ts/package.json` | `0.1.8` → `0.1.9` |
| `integration-tests/register-photo.ts` | バイナリ形式に対応 |
| `integration-tests/stress-test.ts` | 全暗号テストをバイナリ形式に対応 |

---

## §5.2 — SDK WasmModule / TeeNode 対称性修正

`fetchGlobalConfig` が返す `GlobalConfig` で、TeeNode はフルオブジェクト配列（`TrustedTeeNode[]`）を返していたのに対し、WasmModule は ID リスト + ハッシュ Map に分割されていた非対称性を修正。

### 変更内容

| | 旧（非対称） | 新（対称） |
|---|---|---|
| GlobalConfig フィールド | `trusted_wasm_ids: string[]` + `trusted_wasm_hashes?: Map` | `trusted_wasm_modules: TrustedWasmModule[]` |
| fetchGlobalConfig | `fetchWasmHashes()` で情報欠落 | TeeNode と同パターンの並列 fetch |
| TitleClient accessor | `getTrustedWasmIds(): string[]` | `getTrustedWasmModules(): TrustedWasmModule[]` |

### 変更ファイル

| ファイル | 変更内容 |
|---------|---------|
| `sdk/ts/src/types.ts` | `GlobalConfig` フィールド統一 |
| `sdk/ts/src/chain.ts` | `wasmModuleInfoToTrusted` 追加、`fetchWasmHashes` 削除、`fetchGlobalConfig` 対称化 |
| `sdk/ts/src/client.ts` | accessor + validation 更新 |
| `sdk/ts/src/__tests__/chain.test.ts` | WasmModule PDA + デシリアライズテスト 7件追加 |
| `sdk/ts/README.md` | API リファレンス更新 |
| `integration-tests/register-photo.ts` | フィールド名更新 |
| `integration-tests/stress-test.ts` | フィールド名更新 |
| `sdk/ts/package.json` | `0.1.9` → `0.1.10` |
