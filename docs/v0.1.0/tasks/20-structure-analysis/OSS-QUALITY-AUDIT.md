# OSS品質監査レポート

**実施日**: 2026-02-22
**対象**: title-protocol リポジトリ全体
**総合評価**: B+ → A- への道

技術的な基盤は優秀だが、プロのOSSとして世に出すには構造的な改善が必要。

---

## 1. 命名の不統一

### 1.1 TEEの状態オブジェクト名

| コンポーネント | 状態構造体 | 定義場所 | 問題 |
|---|---|---|---|
| Gateway | `GatewayState` | `config.rs`（専用ファイル） | 自己文書化的 |
| TEE | `AppState` | `main.rs:37`（初期化と混在） | 汎用名、場所も不統一 |

**修正**: `AppState` → `TeeAppState` にリネーム + `crates/tee/src/config.rs` に移動

### 1.2 エラー型の非対称性

| クレート | エラー型 | IntoResponse | 状態 |
|---|---|---|---|
| gateway | `GatewayError`（専用error.rs） | 実装済み | 模範的 |
| crypto | `CryptoError` | — | 良好 |
| core | `CoreError` | — | 良好 |
| wasm-host | `WasmError` | — | 良好 |
| tee | なし（`(StatusCode, String)` タプル直書き） | なし | **問題** |
| proxy | なし | — | 許容範囲 |

TEEクレートは最大・最複雑なのに統一エラー型がない。全エンドポイントで `(StatusCode, String)` を手動構築している。

**修正**: `crates/tee/src/error.rs` に `TeeError` enum を作成し、`GatewayError` と同じパターンに統一

### 1.3 Core関数の動詞選択

```
verify_c2pa()              // verify_*
extract_content_hash()     // extract_*
build_provenance_graph()   // build_*
resolve_duplicate()        // resolve_*  ← 他と粒度が異なる
```

crypto crateは `algorithm_operation()` で統一されているが、coreは動詞がバラバラ。

---

## 2. 抽象化の粒度不統一

### 2.1 Trait実装のファイル分離

```
TeeRuntime (模範的)          TempStorage (問題)           WasmLoader (問題)
├── mod.rs (trait定義のみ)    storage.rs (trait + S3実装    wasm_loader.rs (trait +
├── mock.rs (Mock実装)         が同一ファイル、162行)         FileLoader + HttpLoader
└── nitro.rs (Nitro実装)                                     が同一ファイル、125行)
```

`TeeRuntime` は「trait定義 = mod.rs / 実装 = 個別ファイル」で完璧に分離されているのに、`TempStorage` と `WasmLoader` は混在。新しいストレージバックエンド（GCS, R2）やWASMローダー（IPFS, Arweave）を追加する際にスケールしない。

**修正**:
```
crates/gateway/src/storage/     crates/tee/src/wasm_loader/
├── mod.rs (trait定義のみ)       ├── mod.rs (trait定義のみ)
└── s3.rs  (S3TempStorage)      ├── file.rs (FileLoader)
                                 └── http.rs (HttpLoader)
```

### 2.2 verify.rs の肥大化（1,150行）

TEEの `/verify` エンドポイントが1ファイルに全て詰まっている:
- MIME検出ユーティリティ
- メインハンドラ（300行）
- Core処理（C2PA検証）
- Extension処理（WASM実行）
- テスト（570行、ファイルの48%）

**修正**:
```
endpoints/verify/
├── mod.rs          (ハンドラディスパッチ)
├── handler.rs      (メインハンドラ)
├── core.rs         (process_core)
├── extension.rs    (process_extension)
└── tests.rs        (テスト)
```

---

## 3. ディレクトリ構造の情報量不足

### 3.1 TEEクレートのフラット構造

```
crates/tee/src/
├── endpoints/          ← ✓ 構造化済み
├── runtime/            ← ✓ 構造化済み
├── gateway_auth.rs     ← ✗ フラット（認証）
├── proxy_client.rs     ← ✗ フラット（インフラ）
├── security.rs         ← ✗ フラット（横断的関心事）
├── solana_tx.rs        ← ✗ フラット（ブロックチェーン）
└── wasm_loader.rs      ← ✗ フラット（WASM管理）
```

Gatewayは `endpoints/` + `config.rs` + `auth.rs` + `storage.rs` + `error.rs` で整理されているが、TEEは5つの無関係なファイルがルートに散在。

**推奨構造**:
```
crates/tee/src/
├── config.rs                  (TeeAppState定義)
├── error.rs                   (TeeError enum)
├── endpoints/                 (API層)
│   ├── verify/
│   ├── sign/
│   └── create_tree/
├── runtime/                   (TEE抽象化)
├── infra/                     (インフラ層)
│   ├── proxy_client.rs
│   ├── security.rs
│   └── gateway_auth.rs
└── blockchain/                (Solana層)
    └── solana_tx.rs
```

---

## 4. プロトコル vs ベンダー実装の分離

### 方針決定

**モノレポを維持する。** レポ分散を避けるために選んだモノレポ構成を崩さない。
TEEを扱う上でRustの堅牢なメモリハンドリングは不可欠であり、言語依存（Rust）は選択的に受け入れている。

リポジトリ分割は行わず、以下の戦略で「プロトコルOSS」と「ベンダー実装込みフォーク」を実現する:

```
title-protocol (OSS)         = モノレポからベンダー固有コードを除去したもの
title-protocol-aws (フォーク) = モノレポそのまま（すぐ動くリファレンス実装）
```

### 分離の仕組み: Cargo feature flags + ディレクトリ規約

2つの仕組みを組み合わせる:

1. **Rustクレート内のベンダーコード** → Cargo feature flags で条件コンパイル
2. **インフラ・デプロイ関連** → `deploy/aws/` ディレクトリに集約

#### A. Cargo feature flags（Rustクレート）

ベンダー固有の実装ファイルは現在の場所に残したまま、feature flag でゲートする。
trait抽象化が既に機能しているため、ファイル移動は不要。

```toml
# crates/tee/Cargo.toml
[features]
default = ["vendor-aws"]
vendor-aws = ["aws-nitro-enclaves-nsm-api"]
```

```rust
// crates/tee/src/runtime/mod.rs
pub mod mock;

#[cfg(feature = "vendor-aws")]
pub mod nitro;
```

```toml
# crates/crypto/Cargo.toml
[features]
default = ["vendor-aws"]
vendor-aws = []  # nitro.rs の条件コンパイルに使用
```

```rust
// crates/crypto/src/attestation/mod.rs
#[cfg(feature = "vendor-aws")]
pub mod nitro;

pub fn verify_attestation(...) -> Result<...> {
    match tee_type {
        #[cfg(feature = "vendor-aws")]
        "aws_nitro" => { ... }
        other => Err(AttestationError::UnsupportedTeeType(other.into())),
    }
}
```

```toml
# crates/gateway/Cargo.toml
[features]
default = ["vendor-aws"]
vendor-aws = ["rust-s3"]  # S3TempStorage の条件コンパイル
```

```rust
// crates/gateway/src/storage/mod.rs  (trait定義のみ)
#[cfg(feature = "vendor-aws")]
pub mod s3;  // S3TempStorage実装
```

```toml
# crates/proxy/Cargo.toml
[features]
default = ["vendor-aws"]
vendor-aws = []  # vsockリスナーの条件コンパイル（Linux + vendor-aws）
```

ビルド結果:
- `cargo build` → NitroRuntime + S3TempStorage + vsock 込み（フォーク版 = すぐ動く）
- `cargo build --no-default-features` → MockRuntime + TempStorage trait のみ（プロトコルOSS版）

#### B. インフラ・デプロイのディレクトリ移動

AWS固有のインフラコードを `deploy/aws/` に集約する。

**現状:**
```
deploy/
├── setup-ec2.sh                    ← AWS固有
├── docker-compose.production.yml   ← AWS想定
├── keys/
└── terraform/                      ← 100% AWS
    ├── main.tf
    ├── variables.tf
    ├── outputs.tf
    └── user-data.sh
docker/
├── tee.Dockerfile                  ← AWS Nitro（EIF変換前提）
├── tee-mock.Dockerfile             ← 汎用
├── gateway.Dockerfile              ← 汎用
├── proxy.Dockerfile                ← 汎用
└── indexer.Dockerfile              ← 汎用
scripts/
├── build-enclave.sh                ← AWS nitro-cli
├── setup-local.sh                  ← 汎用
├── init-config.mjs                 ← 汎用
├── register-content.mjs            ← 汎用
└── test-devnet.mjs                 ← 汎用
```

**移動後:**
```
deploy/
├── aws/                            ← ベンダー固有（フォークのみ）
│   ├── terraform/
│   │   ├── main.tf
│   │   ├── variables.tf
│   │   ├── outputs.tf
│   │   └── user-data.sh
│   ├── setup-ec2.sh
│   ├── build-enclave.sh            ← scripts/ から移動
│   ├── docker-compose.production.yml
│   └── docker/
│       └── tee.Dockerfile          ← docker/ から移動
├── keys/
└── ...
docker/                             ← 汎用のみ残す
├── tee-mock.Dockerfile
├── gateway.Dockerfile
├── proxy.Dockerfile
└── indexer.Dockerfile
scripts/                            ← 汎用のみ残す
├── setup-local.sh
├── init-config.mjs
├── register-content.mjs
└── test-devnet.mjs
```

### 全ベンダー固有コードの棚卸し

| コード | ベンダー | 分離方法 | 現在の抽象化 |
|---|---|---|---|
| `crates/tee/src/runtime/nitro.rs` | AWS Nitro | feature flag `vendor-aws` | ✓ `TeeRuntime` trait |
| `crates/tee/src/main.rs` L78-81 | AWS Nitro | feature flag（match arm） | ✓ ランタイム選択 |
| `crates/crypto/src/attestation/nitro.rs` | AWS Nitro | feature flag `vendor-aws` | ✓ `AttestationResult` 汎化 |
| `crates/crypto/src/attestation/mod.rs` L92-94 | AWS Nitro | feature flag（match arm） | ✓ `verify_attestation()` 分岐 |
| `crates/gateway/src/storage.rs` S3TempStorage | S3互換 | feature flag `vendor-aws` | ✓ `TempStorage` trait |
| `crates/proxy/src/main.rs` vsock部分 | Linux/Nitro | 既存の `cfg(target_os)` + feature flag | ✓ TCP fallback |
| `deploy/terraform/` | AWS | ディレクトリ移動 → `deploy/aws/` | ✗ |
| `deploy/setup-ec2.sh` | AWS | ディレクトリ移動 → `deploy/aws/` | ✗ |
| `deploy/docker-compose.production.yml` | AWS想定 | ディレクトリ移動 → `deploy/aws/` | ✗ |
| `docker/tee.Dockerfile` | AWS Nitro | ディレクトリ移動 → `deploy/aws/docker/` | ✗ |
| `scripts/build-enclave.sh` | AWS nitro-cli | ディレクトリ移動 → `deploy/aws/` | ✗ |

### プロトコルとして残るもの（ベンダー中立）

| コード | 性質 |
|---|---|
| `crates/types` | プロトコル型定義 |
| `crates/crypto`（attestation trait + 汎用暗号） | プロトコル暗号 |
| `crates/core` | C2PA検証アルゴリズム |
| `crates/wasm-host` | WASM実行環境 |
| `crates/tee`（MockRuntime + エンドポイント + trait定義） | TEEサーバーフレームワーク |
| `crates/gateway`（TempStorage trait + エンドポイント） | Gatewayフレームワーク |
| `crates/proxy`（TCP モード） | プロキシフレームワーク |
| `programs/title-config` | オンチェーンプログラム |
| `sdk/ts` | クライアントSDK |
| `wasm/*` | Extension モジュール |
| `indexer/` | cNFTインデクサ（DAS APIは標準仕様） |
| `docker/`（tee-mock, gateway, proxy, indexer） | 汎用Dockerfile |
| `docker-compose.yml` | ローカル開発環境（MockRuntime） |
| `scripts/`（setup-local, init-config, register-content, test-devnet） | 汎用スクリプト |
| `docs/` | ドキュメント |

### OSS公開フロー

```
[開発]  title-protocol (モノレポ、全コード含む)
          │
          ├── cargo build                  → フォーク版バイナリ（AWS Nitro対応）
          └── cargo build --no-default-features → プロトコル版バイナリ（Mockのみ）
          │
          ▼
[公開]  title-protocol (OSS)
          │   deploy/aws/ を除去
          │   Cargo.toml の default features を空に
          │   = プロトコル仕様 + MockRuntime で動作するリファレンス
          │
          └── title-protocol-aws (フォーク)
              │   OSS版を git fork
              │   deploy/aws/ を追加
              │   Cargo.toml の default features に vendor-aws を追加
              │   = AWS Nitro で本番稼働可能なノード実装
```

### 将来の拡張

新しいTEEベンダー（AMD SEV-SNP, Intel TDX）やクラウド（GCP, Azure）に対応する場合:

```toml
# crates/tee/Cargo.toml
[features]
default = []
vendor-aws = ["aws-nitro-enclaves-nsm-api"]
vendor-gcp = ["gcp-confidential-computing"]  # 将来
vendor-azure = ["azure-attestation"]          # 将来
```

```
deploy/
├── aws/        ← title-protocol-aws フォーク
├── gcp/        ← title-protocol-gcp フォーク（将来）
└── azure/      ← title-protocol-azure フォーク（将来）
```

各フォークは独立してメンテナンスでき、プロトコルOSSの変更は通常の git merge で取り込める。
trait抽象化が正しく機能していれば、新ベンダーの追加は「trait実装ファイル + feature flag + deploy/ディレクトリ」の3点のみ。

---

## 5. OSS公開に必要なファイル（致命的な欠落）

| ファイル | 状態 | 重要度 |
|---|---|---|
| `LICENSE` | なし | **致命的**（法的に使用不可） |
| `CONTRIBUTING.md` | なし | **致命的** |
| `CODE_OF_CONDUCT.md` | なし | 高 |
| `SECURITY.md` | なし | 高 |
| `CHANGELOG.md` | なし | 中 |
| `.github/ISSUE_TEMPLATE/` | なし | 中 |
| `.github/PULL_REQUEST_TEMPLATE/` | なし | 中 |
| `cargo-audit` in CI | なし | 中 |

README.md は 9/10 で優秀（アーキテクチャ図、クイックスタート、デザイン原則が揃っている）。

---

## 6. 良い点（変更不要）

- **クレート命名パターン** `title-*` — 全7クレートで完全統一
- **型名パターン** `*Request` / `*Response` / `*Payload` — 一貫性あり
- **エンドポイントハンドラ名** `handle_*()` — TEE/Gateway両方で統一
- **定数名** — マジックナンバーなし、全て名前付き定数
- **TeeRuntime trait分離** — `mod.rs` + `mock.rs` + `nitro.rs` の理想的な構造
- **日本語docコメント + 仕様書参照** `§5.1 Step 4` — 全公開関数に記載
- **セキュリティ定数** — `security.rs` に名前付きで集約
- **E2Eテスト** — 7スイート、Docker Compose統合、CI組み込み

---

## 7. 優先度順アクションリスト

### P0: OSS公開ブロッカー（法的・社会的）

1. LICENSE ファイル追加（Apache 2.0推奨）
2. CONTRIBUTING.md 作成
3. SECURITY.md 作成

### P1: 構造的不統一の修正

4. `TeeError` enum 作成（`GatewayError` と同パターン）
5. `AppState` → `TeeAppState` + `config.rs` 移動
6. `storage.rs` → `storage/mod.rs` + `s3.rs` 分離
7. `wasm_loader.rs` → `wasm_loader/mod.rs` + `file.rs` + `http.rs` 分離
8. TEE `endpoints/mod.rs` に barrel export追加

### P2: ファイル肥大化の解消

9. `verify.rs`（1,150行）をサブディレクトリに分割
10. TEEのフラットファイルを `infra/` + `blockchain/` に整理

### P3: プロトコル/ベンダー分離（モノレポ維持）

11. 全Rustクレートに `vendor-aws` feature flag を導入（§4 セクション参照）
12. AWS固有インフラを `deploy/aws/` に集約（ディレクトリ移動）
13. プロトコルOSS公開スクリプト作成（vendor除去 + default-features 書き換え）
14. フォーク用テンプレート（title-protocol-aws）の初回作成
