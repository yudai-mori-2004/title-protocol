# Task 32: コード監査 — crates/types

## 対象
`crates/types/` — 共有型定義

## ファイル
- `src/lib.rs` — 全型定義（SignedJson, CorePayload, GraphNode, GlobalConfig, API型等）

## 監査で発見された問題

### デッドコード
1. **`SignedJsonExtension` が未使用**: 定義のみ存在し、Rustコードベース全体で一度もimportされていない。Extension signed_jsonは `crates/tee/src/endpoints/verify/extension.rs` で `serde_json::json!()` によるアドホック構築。TS SDKにも対応型なし。→ 削除

### コード品質
2. **`PartialEq` 未derive**: 全30型に `PartialEq` がない。テストで `assert_eq!` 不可、下流クレートでの比較も不可能。全型で derive 可能。`serde_json::Value` を含まない型は `Eq` も追加可能。
3. **テストが一切ない**: 型クレートの最低限として以下が必要:
   - `#[serde(flatten)]` が期待通りのJSON構造を生成するか（`SignedJson.core`, `ExtensionPayload.result`）
   - `#[serde(rename = "type")]` → `"type"` キーになるか（`GraphNode.node_type`）
   - `#[serde(skip_serializing_if = "Option::is_none")]` → `None` 時にフィールド省略されるか
   - シリアライズ/デシリアライズ roundtrip

### 設計メモ（修正不要）
- `GlobalConfig`, `TrustedTeeNode`, `TrustedWasmModule`, `CnftMetadata`: Rustワークスペース内では未使用だが、TS SDK (`sdk/ts/src/client.ts`) とAnchorプログラム (`programs/title-config`) が対応型を定義・使用。APIコントラクトとして正当。
- `serde` / `serde_json`: 型定義内で `serde_json::Value` / `serde_json::Map` を直接使用 → 通常依存で正当。
- Extension signed_jsonのアドホック構築（`serde_json::json!()` vs 構造体）: teeクレート側の問題であり、typesクレートのスコープ外。

## 完了基準
- [x] `SignedJsonExtension` 削除
- [x] 全型に `PartialEq` 追加（`Eq` は `serde_json::Value` 非含有型のみ）
- [x] serde属性のテスト追加（flatten, rename, skip_serializing_if, roundtrip）
- [x] `cargo test -p title-types` パス（12テスト）
- [x] `cargo check --workspace` パス（警告なし）

## 対処内容

### 1. `SignedJsonExtension` 削除
- Rustコードベース全体・TS SDKのいずれからも未使用のデッドコード
- `ExtensionPayload` が実質的な代替として使用されている

### 2. `PartialEq` / `Eq` 追加
- 全29型に `PartialEq` を追加
- `serde_json::Value` を含まない23型には `Eq` も追加
- `Eq` なし（`serde_json::Value` 含有）: `SignedJson`, `ExtensionPayload`, `ClientPayload`, `ProcessorResult`, `VerifyResponse`, `GatewayAuthSignTarget`, `GatewayAuthWrapper`

### 3. テスト追加（0 → 12テスト）
- `test_signed_json_flatten_core_fields` — flatten により core フィールドがトップレベルに展開
- `test_signed_json_flatten_roundtrip` — SignedJson のシリアライズ/デシリアライズ往復
- `test_extension_payload_flatten_result` — flatten により WASM結果がペイロードにマージ
- `test_graph_node_rename_type` — `#[serde(rename = "type")]` で `"type"` キー生成
- `test_graph_node_rename_roundtrip` — GraphNode の往復
- `test_core_payload_skip_none_fields` — `None` 時に TSAフィールド省略
- `test_core_payload_includes_some_fields` — `Some` 時にTSAフィールド出力
- `test_resource_limits_skip_all_none` — 全フィールド `None` → `{}`
- `test_attribute_roundtrip` — Attribute 往復
- `test_graph_link_roundtrip` — GraphLink 往復
- `test_verify_request_roundtrip` — VerifyRequest 往復
- `test_encrypted_payload_roundtrip` — EncryptedPayload 往復
