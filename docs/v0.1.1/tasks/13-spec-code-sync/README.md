# Task 13: 仕様書・コード同期

## 概要

v0.1.1 仕様書 (`docs/v0.1.1/SPECS_JA.md`) とコードベースの不一致を解消する。
開発が仕様書に先行した結果生じた差分を双方向で修正。

## 不一致一覧と修正方針

### 仕様書を修正（コードに合わせる）

| # | 不一致 | 仕様書箇所 | 状態 |
|---|--------|-----------|------|
| 1 | `trusted_wasm_modules` → `trusted_wasm_ids` + WasmModuleAccount PDA分離設計 | §5.2 Step 1, §6.7 | DONE |
| 2 | `ResourceLimitsOnChain` フィールドがGlobalConfigAccountに未記載 | §5.2 Step 1 | DONE |
| 3 | RegisterNodeRequest に追加フィールド (`core_collection_mint`, `ext_collection_mint`, `measurements`) | §6.4 /register-node | DONE |
| 4 | SignRequest に `fee_payer` フィールド追加 | §6.2 /sign | DONE |
| 5 | Fuel制限値 100M → 1B | §7.1 | DONE |
| 6 | `get_content_feature` に `c2pa_verify_active_cert_chain` 操作が未記載 | §7.1 | DONE |
| 7 | WASMモジュールのバージョン管理（WasmVersionEntry）が未記載 | §7.5 | DONE |

### コードを修正（仕様書に合わせる）

| # | 不一致 | コード箇所 | 状態 |
|---|--------|-----------|------|
| 8 | `update_authority` 命令の欠如（authority移行不可） | `programs/title-config/src/lib.rs` | DONE |
| 9 | SDK wasm_hash検証が未実装（extension_idチェックのみ） | `sdk/ts/src/client.ts`, `chain.ts`, `types.ts` | DONE |

## 修正内容の詳細

### #1 GlobalConfigAccount PDA分離設計 (§5.2)
- オンチェーン構造: `trusted_wasm_modules: Vec<WasmModuleEntry>` → `trusted_wasm_ids: Vec<[u8; 32]>` に変更
- WasmModuleAccount PDA (seeds=[b"wasm-module", &extension_id]) の構造を追記
- TEEノードとの対称設計の説明を追加
- 論理ビューに `trusted_wasm_hashes` を反映

### #2 ResourceLimitsOnChain (§5.2)
- GlobalConfigAccountに `resource_limits: ResourceLimitsOnChain` フィールドを追記
- Option型でGatewayのデフォルト値とのmin制御を説明
- 論理ビューに `resource_limits` オブジェクトを追加
- フィールド説明テーブルに追記

### #3 RegisterNodeRequest (§6.4)
- `core_collection_mint`, `ext_collection_mint`, `measurements` を追記

### #4 SignRequest fee_payer (§6.2)
- `fee_payer` Optional フィールドを追記、sign-and-mintでの用途を説明

### #5 Fuel制限 (§7.1)
- 100,000,000 → 1,000,000,000

### #6 c2pa_verify_active_cert_chain (§7.1)
- get_content_feature操作テーブルに追記
- エラーコード -5 の説明を拡張（C2PA構造エラー）

### #7 WASMバージョン管理 (§7.5)
- WasmVersionEntryの全フィールド（version, wasm_hash, wasm_source, status, registered_at）を記載
- add_wasm_version, update_wasm_version命令の説明を追加

### #8 update_authority命令 (programs/title-config)
- `update_authority(new_authority: Pubkey)` 命令を追加
- UpdateConfig contextを再利用（has_one = authority制約）
- AuthorityUpdatedイベント（old_authority, new_authority）を追加
- §8.1にupdate_authority命令によるフェーズ間移行の説明を追記

### #9 SDK wasm_hash検証
- `chain.ts`: findWasmModulePDA(), fetchWasmModuleAccount(), fetchWasmHashes() を追加
- `chain.ts`: fetchGlobalConfig()でWasmModuleAccount PDAも並列取得
- `types.ts`: GlobalConfigにtrusted_wasm_hashes?: Map<string, string>を追加
- `client.ts`: validateWasmHashes()でextension_idチェック+wasm_hashチェックの二段階検証
- §6.7の記述をtrusted_wasm_hashesに更新

## ビルド確認結果

- [x] `cargo check --workspace` パス
- [x] `cargo test --workspace` パス
- [x] `cd sdk/ts && npm run build` パス
- [x] `cd programs/title-config && cargo check` パス
