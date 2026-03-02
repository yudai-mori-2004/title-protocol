# Task 39: コード監査 — programs/title-config/

## 対象
`programs/title-config/` — Anchor Solanaプログラム（GlobalConfig PDA管理）

## ファイル
- `programs/title-config/src/lib.rs` — プログラム本体（唯一のソースファイル）
- `programs/title-config/Cargo.toml` — 依存定義
- `scripts/init-config.mjs` — ローカル開発用GlobalConfig初期化スクリプト
- `scripts/init-devnet.mjs` — Devnet完全初期化スクリプト

## 監査で発見した問題

### 設計レベルの問題（全て修正済み）

#### 1. PDA space不足 + realloc未対応
旧設計では GlobalConfig 1アカウントに全データを格納:
```
space = 8 + 32 + 32 + 32 + 4 + 4 + 4 + 1024 = 1140 bytes
```
各TEEノード=98B, WASMモジュール=64B, TSA鍵=32B。
TEEノード10件+WASMモジュール5件+TSA鍵5件=1460B → 1024Bを超えてトランザクション失敗。
さらに `UpdateConfig` に `realloc` がなく、初期割当を超える拡張が不可能だった。

#### 2. 仕様書フィールドの欠落
仕様書 §5.2 Step 1 で定義された以下のフィールドがオンチェーンに存在しなかった:
- `gateway_endpoint`: クライアントがノードを発見する唯一の手段。これがないとSDKの `discoverNodes` が機能しない
- `expected_measurements`: 第三者がTEEの正当性を検証する基準（§5.2 Step 4）。これがないと「TEEが正しいコードを実行していたか」を確認できない
- `wasm_source`: 第三者がExtension結果を独立検証するためのWASMバイナリ取得先（§7.2）

#### 3. 更新粒度の問題
`update_tee_nodes` は Vec 全体を上書きする設計。ノード50個の状態で1つだけ更新するにも全データを再送信する必要があり、Solanaトランザクションの1232Bデータ制限に容易に抵触する。

## 修正内容: per-node PDAアーキテクチャへの再設計

### アカウント構造（新設計）

```
GlobalConfigAccount (PDA: seeds=[b"global-config"])
├── authority: Pubkey
├── core_collection_mint: Pubkey
├── ext_collection_mint: Pubkey
├── trusted_node_keys: Vec<[u8; 32]>       ← signing_pubkeyのフラットリスト
├── trusted_tsa_keys: Vec<[u8; 32]>
└── trusted_wasm_modules: Vec<WasmModuleEntry>
    ├── extension_id: [u8; 32]
    ├── wasm_hash: [u8; 32]
    └── wasm_source: String                 ← 新規追加（可変長URL）

TeeNodeAccount (PDA: seeds=[b"tee-node", &signing_pubkey])
├── signing_pubkey: [u8; 32]
├── encryption_pubkey: [u8; 32]
├── gateway_pubkey: [u8; 32]
├── gateway_endpoint: String                ← 新規追加
├── status: u8
├── tee_type: u8
├── measurements: Vec<MeasurementEntry>     ← 新規追加
│   ├── key: [u8; 16]    // "PCR0", "MRTD" 等
│   └── value: [u8; 48]  // SHA-384ハッシュ
└── bump: u8
```

### 命令セット（旧→新）

| 旧 | 新 | 変更理由 |
|----|-----|---------|
| `update_tee_nodes(Vec<...>)` | `register_tee_node(...)` | per-node PDA作成 + フラットリスト追加 |
| — | `update_tee_node(...)` | 個別フィールド更新（Optional引数） |
| — | `deactivate_tee_node()` | status=Inactive。アカウント維持（過去のcNFT検証用） |
| `update_wasm_modules(Vec<...>)` | `add_wasm_module(...)` / `remove_wasm_module(...)` | 個別追加/削除。TX size制限回避 |
| `update_tsa_keys(Vec<...>)` | `add_tsa_key(...)` / `remove_tsa_key(...)` | 同上 |
| `delegate_collection_authority` | 同名（引数変更） | TeeNodeAccount PDAで検証。Vec走査不要に |
| `revoke_collection_authority` | 同名（引数変更） | 同上 |
| `initialize` | 変更なし | — |
| `update_collections` | 変更なし | — |

### Space設計

- **GlobalConfig**: `BASE_SIZE(116B) + 10240B = 10356B`。可変領域の内訳: ノードID=32B, TSA鍵=32B, WASMモジュール≈98B（wasm_source平均30文字時）。現実的な上限目安: ノード×100 + TSA鍵×30 + WASMモジュール×30 ≈ 6,140B。将来的にrealloc命令追加で拡張可能
- **TeeNodeAccount**: `MAX_SPACE = BASE_SIZE(115B) + 256(endpoint) + 512(measurements×8) = 883B`

### deactivate vs close

仕様書 §5.2 Step 4 では、検証者がオフチェーンデータの `tee_pubkey` を GlobalConfig で照合する。
古い鍵で署名された既存cNFTの検証にはTeeNodeAccountが必要なため、通常は deactivate（status変更のみ）で対応し、アカウントは維持する。close命令は現時点では未実装。

### スクリプト更新

- `scripts/init-config.mjs`: `update_tee_nodes` → `register_tee_node`（PDAアカウント追加、gateway_endpoint/measurements引数追加）
- `scripts/init-devnet.mjs`: 同上 + `update_wasm_modules` → `add_wasm_module`（per-module呼び出し）+ `delegate_collection_authority`（TeeNodeAccount PDA追加、tee_signing_pubkeyをデータから除去）

## 修正不要と判断した項目

- **`encryption_algorithm` がオンチェーンにない**: プロトコル定数 `x25519-hkdf-sha256-aes256gcm`。全ノード共通でノードごとに保存する必要なし
- **`delegate/revoke` がMPL Core CPIを行わない**: コメントで文書化済み。クライアントサイドで同一トランザクション内にMPL Core命令を合成する設計。アトミック性は保証される
- **テスト不在**: Anchorテストには solana-test-validator + デプロイ環境が必要。`init-config.mjs` / `init-devnet.mjs` がE2Eテストの役割を果たしている
- **close_tee_node 未実装**: 現時点ではdeactivateで十分。将来必要になった場合に追加

## イベント（新規追加）

- `TeeNodeRegistered { signing_pubkey }` — ノード登録時
- `TeeNodeDeactivated { signing_pubkey }` — ノード無効化時
- `CollectionAuthorityDelegated` / `CollectionAuthorityRevoked` — 既存（変更なし）

## エラーコード（新規追加）

- `DuplicateWasmModule` — 同一extension_idの重複登録防止
- `WasmModuleNotFound` — 削除対象が存在しない
- `DuplicateTsaKey` / `TsaKeyNotFound` — TSA鍵の重複/不在
- `GatewayEndpointTooLong` — 256文字制限
- `TooManyMeasurements` — 8エントリ制限
- `WasmSourceTooLong` — 256文字制限

## 完了基準
- [x] per-node PDAアーキテクチャ実装
- [x] gateway_endpoint, measurements, wasm_source フィールド追加
- [x] 個別add/remove命令（WASM, TSA鍵）
- [x] `cargo check` パス（programs/title-config）
- [x] スクリプト更新（init-config.mjs, init-devnet.mjs）
- [x] `cargo test --workspace` パス（142テスト）
