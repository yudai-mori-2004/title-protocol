# Task 14: SDK WasmModule / TeeNode 対称性修正

## 背景

SDK の `fetchGlobalConfig` が返す `GlobalConfig` 型において、TeeNode と WasmModule の扱いが非対称だった。TeeNode はフルオブジェクト配列（`TrustedTeeNode[]`）で返すのに対し、WasmModule は ID リスト（`string[]`）とハッシュ Map（`Map<string, string>`）に分割されており、`wasm_source` 等の情報が欠落していた。

RootLens 等の外部コンシューマが WasmModule の詳細（`wasm_hash`, `wasm_source`）を表示するには、SDK とは別に PDA を直接叩く必要があり、SDK の関数が全部存在するのに使えない状態だった。

### 修正前の非対称性

| 観点 | TeeNode | WasmModule |
|------|---------|------------|
| GlobalConfig フィールド | `trusted_tee_nodes: TrustedTeeNode[]` | `trusted_wasm_ids: string[]` + `trusted_wasm_hashes?: Map<string, string>` |
| fetchGlobalConfig 内部 | 並列 fetch → filter nulls → フルオブジェクト | `fetchWasmHashes()` → Map（情報欠落） |
| 変換関数 | `rawToTrustedTeeNode`（名前付き） | インライン変換 |
| TitleClient accessor | `getTrustedTeeNodes(): TrustedTeeNode[]` | `getTrustedWasmIds(): string[]`（ID のみ） |
| テスト | PDA 4件 + デシリアライズ 3件 | なし |

## 作業内容

### 1. GlobalConfig 型の統一 (`types.ts`)

`trusted_wasm_ids: string[]` + `trusted_wasm_hashes?: Map<string, string>` を削除し、`trusted_wasm_modules: TrustedWasmModule[]` に統一。`TrustedWasmModule` 型は既に定義済みだったが未使用だった。

### 2. fetchGlobalConfig の対称化 (`chain.ts`)

- `wasmModuleInfoToTrusted()` 名前付き関数を追加（`rawToTrustedTeeNode` と対称）
- WasmModule の fetch を TeeNode と同パターン（並列 fetch → filter nulls → フルオブジェクト配列）に変更
- `fetchWasmHashes()` を削除（情報を潰すだけの中間関数）

### 3. TitleClient の更新 (`client.ts`)

- `getTrustedWasmIds(): string[]` → `getTrustedWasmModules(): TrustedWasmModule[]`
- `validateWasmHashes` を `trusted_wasm_modules` から統一的に参照するよう変更

### 4. テストの対称化 (`chain.test.ts`)

- `findWasmModulePDA` テスト 3件追加（TeeNode PDA テストと対称）
- `WasmModuleAccount deserialization` テスト 4件追加（TeeNode テストと対称）

## 変更ファイル

| ファイル | 変更内容 |
|---------|---------|
| `sdk/ts/src/types.ts` | `GlobalConfig` から `trusted_wasm_ids` + `trusted_wasm_hashes?` を削除、`trusted_wasm_modules: TrustedWasmModule[]` に統一 |
| `sdk/ts/src/chain.ts` | `wasmModuleInfoToTrusted` 追加、`fetchWasmHashes` 削除、`fetchGlobalConfig` 対称化 |
| `sdk/ts/src/client.ts` | `getTrustedWasmModules()` accessor、`validateWasmHashes` 更新 |
| `sdk/ts/src/__tests__/chain.test.ts` | WasmModule PDA 3件 + デシリアライズ 4件 追加 |
| `sdk/ts/README.md` | API リファレンス更新 |
| `integration-tests/register-photo.ts` | フィールド名更新 |
| `integration-tests/stress-test.ts` | フィールド名更新 |
| `sdk/ts/package.json` | `0.1.9` → `0.1.10` |

## 検証

- 全 33 ユニットテスト通過（新規 7件含む）
- devnet `fetchGlobalConfig` で `trusted_wasm_modules` が 4件取得成功（image-phash, hardware-google, c2pa-training, c2pa-license）
- 各モジュールの `extension_id`, `wasm_source`, `wasm_hash` 全フィールド取得確認

## 完了条件

- [x] `GlobalConfig.trusted_wasm_modules: TrustedWasmModule[]` に統一
- [x] `fetchGlobalConfig` が TeeNode と同パターンで WasmModule を並列 fetch
- [x] `fetchWasmHashes` 削除
- [x] `getTrustedWasmModules()` accessor
- [x] WasmModule PDA テスト 3件追加
- [x] WasmModule デシリアライズテスト 4件追加
- [x] 全テスト通過
- [x] devnet 実データ検証
- [x] npm publish (`@title-protocol/sdk@0.1.10`)
