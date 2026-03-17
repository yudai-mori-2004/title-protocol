# Task 07: WASMモジュール PDA管理 + Arweaveアップロード

## 目的

WASMモジュール管理をGlobalConfig内のインラインリストから、TEEノードと対称的なPDAベースの管理に移行する。各WASMモジュール種別ごとにPDAを持ち、バージョン管理（Arweave URL付き）を行う。

## 設計

### アカウント構造

```
GlobalConfig
├── trusted_wasm_ids: Vec<[u8; 32]>       ← extension_id のフラットリスト
│
├── WasmModuleAccount PDA [seeds: "wasm-module" + extension_id]
│   ├── extension_id: "phash"
│   ├── versions: Vec<WasmVersionEntry>
│   │   ├── { version: 1, wasm_hash, wasm_source: "ar://...", status, registered_at }
│   │   └── { version: 2, wasm_hash, wasm_source: "ar://...", status, registered_at }
│   └── bump
│
├── WasmModuleAccount PDA [seeds: "wasm-module" + extension_id]
│   ├── extension_id: "hardware-google"
│   └── versions: [...]
```

### TEEとの対称性

| | TEE Node | WASM Module |
|---|---|---|
| GlobalConfigリスト | `trusted_node_keys: Vec<[u8; 32]>` | `trusted_wasm_ids: Vec<[u8; 32]>` |
| PDA seeds | `["tee-node", signing_pubkey]` | `["wasm-module", extension_id]` |
| 登録 | `register_tee_node` | `register_wasm_module` |
| 削除 | `remove_tee_node` | `remove_wasm_module` |
| 更新 | `update_tee_node` | `add_wasm_version` / `update_wasm_version` |
| devnet | authority自動署名 | 同左 |
| mainnet | DAO承認TX | 同左 |

### WasmModuleAccount

```rust
#[account]
pub struct WasmModuleAccount {
    pub extension_id: [u8; 32],
    pub versions: Vec<WasmVersionEntry>,
    pub bump: u8,
}

pub struct WasmVersionEntry {
    pub version: u32,
    pub wasm_hash: [u8; 32],
    pub wasm_source: String,     // "ar://..."
    pub status: u8,              // 0=active, 1=deprecated
    pub registered_at: i64,
}
```

PDAサイズは動的。`realloc` でバージョン追加ごとに拡張（最大10MB）。

### 新規命令

| 命令 | 動作 |
|------|------|
| `register_wasm_module` | PDA作成 + GlobalConfig.trusted_wasm_ids に追加 |
| `remove_wasm_module` | PDAクローズ + GlobalConfig.trusted_wasm_ids から除去 |
| `add_wasm_version` | PDAに新バージョン追加（realloc） |
| `update_wasm_version` | 既存バージョンのstatus等を更新 |

### 削除する命令

| 命令 | 理由 |
|------|------|
| `add_wasm_module` (旧) | インラインGlobalConfig方式を廃止 |
| `remove_wasm_module` (旧) | 同上 |

### GlobalConfig変更

```rust
// Before
pub trusted_wasm_modules: Vec<WasmModuleEntry>,

// After
pub trusted_wasm_ids: Vec<[u8; 32]>,
```

### init-global変更

- WASMモジュール登録ステップを削除
- `trusted_wasm_ids` は空Vecで初期化
- WASMは後から `title-cli register-wasm` で個別管理

### CLI新規コマンド

| コマンド | 動作 |
|---------|------|
| `register-wasm` | WASMバイナリをArweaveにアップロード → PDA作成 → バージョン1登録 |
| `add-wasm-version` | WASMバイナリをArweaveにアップロード → 既存PDAに新バージョン追加 |
| `update-wasm-version` | バージョンのstatus変更等 |
| `remove-wasm` | PDA削除 + GlobalConfigから除去 |

## 変更ファイル

### オンチェーンプログラム

| ファイル | 変更内容 |
|---------|---------|
| `programs/title-config/src/lib.rs` | WasmModuleAccount PDA追加、旧WASMインライン方式を削除、新命令4つ追加 |

### CLI

| ファイル | 変更内容 |
|---------|---------|
| `crates/cli/src/main.rs` | 新サブコマンド追加 |
| `crates/cli/src/anchor.rs` | WasmModule PDA導出、新命令ビルダー追加 |
| `crates/cli/src/commands/init_global.rs` | WASM登録ステップ削除 |
| `crates/cli/src/commands/register_wasm.rs` | 新規：Arweaveアップロード + PDA作成 |
| `crates/cli/src/commands/add_wasm_version.rs` | 新規：バージョン追加 |

### 影響を受ける既存コード

| ファイル | 変更内容 |
|---------|---------|
| `crates/gateway/src/onchain.rs` | GlobalConfig パース更新 |
| `crates/tee/src/endpoints/verify/extension.rs` | WASM検証ロジック更新（必要に応じ） |

## TEE側: OnChainLoader（オンチェーンPDAからWASMを動的取得）

### 方針

TEEは起動時にローカルファイルからWASMを読むのではなく、リクエスト時にオンチェーンPDAからwasm_source URLを解決し、Arweaveから動的に取得する。将来的にキャッシングレイヤーを追加する。

### 通信経路

既存の `proxy_client::proxy_request` を使用。GET/POST両対応済み。

```
TEE → proxy_request("POST", rpc_url, body) → socat TCP:8000 → vsock → Host Proxy → Solana RPC
TEE → proxy_request("GET", arweave_url, []) → socat TCP:8000 → vsock → Host Proxy → Arweave
```

`PROXY_ADDR="direct"` の場合は reqwest 直接（開発用）。既存のvsockアーキテクチャに追加変更なし。

### OnChainLoader フロー

1. `proxy_post(rpc_url, getAccountInfo(wasm_module_pda))` → PDAバイナリ取得
2. PDAデータをパース → 最新activeバージョンの `wasm_source` URL を抽出
3. `proxy_get(wasm_source)` → WASMバイナリ取得
4. `WasmBinary { bytes, source }` を返す

### 変更ファイル（TEE側）

| ファイル | 変更内容 |
|---------|---------|
| `crates/tee/src/infra/proxy_client.rs` | `proxy_post` を公開関数として追加 |
| `crates/tee/src/wasm_loader/onchain.rs` | OnChainLoader 実装（proxy_post/proxy_get 経由） |
| `crates/tee/src/wasm_loader/mod.rs` | `onchain` モジュール追加 |
| `crates/tee/src/main.rs` | デフォルトローダーを OnChainLoader に変更（`WASM_DIR` 設定時のみ FileLoader にフォールバック） |
| `deploy/aws/docker/entrypoint.sh` | `WASM_DIR` 強制上書きを削除（OnChainLoaderがデフォルト） |
| `deploy/aws/setup-ec2.sh` | `PROGRAM_ID` を .env にベイク、MockRuntime時の `WASM_DIR` 強制を削除 |
| `deploy/local/setup.sh` | 同上（`WASM_DIR` 強制削除 + `GATEWAY_SIGNING_KEY` 永続化 + `PROGRAM_ID` 渡し） |

### ローダー選択ロジック

```
WASM_DIR が設定されている → FileLoader（開発用フォールバック）
それ以外               → OnChainLoader（PDA → Arweave取得）
```

### キャッシング（将来）

OnChainLoaderの上にキャッシングレイヤーを追加する形で拡張可能:
- extension_id + version → WASMバイナリ をメモリまたはディスクにキャッシュ
- PDA上のバージョン番号が変わらなければキャッシュヒット
- 本タスクではキャッシュなし（毎回取得）

## テスト

- プログラムビルド: `cd programs/title-config && cargo-build-sbf`
- `cargo check --workspace && cargo test --workspace`
- ローカルノード: `deploy/local/setup.sh` → 写真検証
- EC2ノード: `deploy/aws/setup-ec2.sh` → 写真検証

## 完了条件

- [x] WasmModuleAccount PDA がバージョン管理付きで動作する
- [x] register/remove/add-version/update-version 4命令が動作する
- [x] GlobalConfig から旧 trusted_wasm_modules が削除されている
- [x] init-global が WASM 登録なしで動作する
- [x] CLI で Arweave アップロード + PDA 登録ができる
- [x] devnet/mainnet の authority 署名パターンが TEE と対称
- [x] SDK が新レイアウト (trusted_wasm_ids) に対応している
- [x] Extension ID 命名更新 (image-phash, c2pa-training, c2pa-license)
- [x] TEE OnChainLoader が proxy_post/proxy_get 経由でPDA読み取り + Arweave取得
- [x] WASM_DIR 未設定時に OnChainLoader がデフォルトで使用される
- [x] ローカルで動作確認済み（image-phash を Arweave から動的取得、pHash計算成功）
- [x] setup.sh / setup-ec2.sh / entrypoint.sh から WASM_DIR 強制設定を削除
- [x] PROGRAM_ID を .env にベイクする処理を追加（setup-ec2.sh）
- [x] 全既存テストがパスする
- [ ] EC2 Enclave で OnChainLoader 動作確認（プログラム再デプロイ後）
