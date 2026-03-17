# Task 08: 共通アーキテクチャレベルのパフォーマンス最適化

## 目的

TEE → SDK のリクエストフロー全体を精査し、共通構造（ベンダー非依存）のボトルネックを解消する。Arweaveやベンダー固有の最適化はスコープ外とし、並列化・キャッシング・重複排除で処理レイテンシを削減する。

## 現状ベースライン

EC2 (aws_nitro) での `core-c2pa + image-phash` フル登録:

| ステップ | 時間 |
|---------|------|
| S3アップロード | ~700ms |
| /verify (C2PA + pHash) | ~1100ms |
| Arweave署名保存 ×2 | ~3200ms |
| /sign (cNFTミントTX構築) | ~2300ms |
| Solana TX送信+確認 ×2 | ~1600ms |
| **合計** | **~11秒** |

## ボトルネック一覧

### P1: プロセッサの逐次実行（TEE /verify）

**箇所:** `crates/tee/src/endpoints/verify/handler.rs:162` — processor_ids の forループ

**現状:** core-c2pa と各 extension が逐次実行。各プロセッサは独立処理。

**修正:** `tokio::spawn` / `futures::join_all` で並列実行。

**効果:** 2プロセッサで最大50%削減（例: 200ms + 150ms → max(200ms, 150ms) = 200ms）

### P2: WASMバイナリのキャッシング（TEE OnChainLoader）

**箇所:** `crates/tee/src/wasm_loader/onchain.rs` — 毎リクエストで RPC + Arweave ダウンロード

**現状:** 同じ extension_id の WASM を毎回2回のHTTPで取得（RPC POST + Arweave GET）。

**修正:** `CachedWasmLoader` ラッパー。extension_id → (wasm_binary, version, fetched_at) の LRU キャッシュ。TTL ベースの無効化。

```
WasmLoader trait
  └── CachedWasmLoader (LRU + TTL)
        └── OnChainLoader (PDA → Arweave)
```

**効果:** 2回目以降のリクエストで ~200ms/extension 削減。

### P3: C2PA検証の重複実行（TEE Extension処理）

**箇所:** `crates/tee/src/endpoints/verify/extension.rs:73` — `title_core::verify_c2pa` を Extension ごとに再実行

**現状:** content_hash を得るために C2PA フル検証を Extension ごとに行う。Core で既に検証済みの同じ結果。

**修正:** Core 処理結果（content_hash, c2pa_result）をリクエストコンテキストに保持し Extension から参照。

**効果:** Extension あたり ~100ms 削減。

### P4: Arweave署名保存の並列化（SDK / integration-tests）

**箇所:** `integration-tests/register-photo.ts` および `sdk/ts/src/client.ts` — signed_json の逐次アップロード

**現状:** 各 processor の signed_json を Irys に順番にアップロード。

**修正:** `Promise.all` で並列アップロード。

**効果:** N 個で (N-1) × アップロード時間分の削減。

### P5: cNFT TX の並列ブロードキャスト（SDK / integration-tests）

**箇所:** `integration-tests/register-photo.ts` — TX を逐次送信+確認

**現状:** Core cNFT TX と Extension cNFT TX を逐次ブロードキャスト。TEE の /sign は部分署名済みTXを返すだけで、ブロードキャストはクライアント側。

**修正:** `Promise.all` で並列送信。Merkle Tree の `max_buffer_size=64` が並行書き込みを吸収する。失敗時はリトライ。

**効果:** 2 TX で確認待ち ~800ms → ~400ms。

### P6: /sign 内の signed_json 取得の並列化（TEE）

**箇所:** `crates/tee/src/endpoints/sign/handler.rs:77` — items の forループ

**現状:** 各 item の signed_json_uri ダウンロード → 署名検証 → TX 構築が逐次。

**修正:** items を `futures::join_all` で並列処理。各 item は独立（異なる Tree/Collection）。

**効果:** 2 items で ~50% 削減。

### P7: SDK リクエストパイプライニング

**箇所:** `sdk/ts/src/client.ts` — upload → verify → store → sign が完全逐次

**現状:** 独立した前準備（upload と blockhash 取得）も逐次。

**修正:** `Promise.all` で独立ステップを並行実行。

**効果:** ~100-200ms 削減。

### P8: プロキシ接続の再利用（TEE）

**箇所:** `crates/tee/src/infra/proxy_client.rs:102` — 毎回 `TcpStream::connect`

**現状:** プロキシへの TCP 接続を毎回新規作成。

**修正:** 接続プールで再利用。

**効果:** リクエストあたり ~30-50ms 削減。

## スコープ外

- Arweave アップロード速度（Irys バッチ API 等）
- Solana RPC レイテンシ（dedicated RPC ノード等）
- ベンダー固有の最適化（vsock チューニング、Nitro CPU 割当等）
- WASM モジュール自体の最適化（アルゴリズム改善等）

## 実装順序（ROI順）

| 順序 | ボトルネック | 効果 | 複雑度 |
|------|------------|------|--------|
| 1 | P2: WASMキャッシング | 200ms/ext/req | 低 |
| 2 | P1: プロセッサ並列化 | 50%削減 | 中 |
| 3 | P3: C2PA重複排除 | 100ms/ext | 中 |
| 4 | P4: Arweave並列アップロード | (N-1)×30ms | 低 |
| 5 | P5: TX並列ブロードキャスト | ~400ms | 低 |
| 6 | P6: /sign並列処理 | ~50%削減 | 中 |
| 7 | P7: SDKパイプライニング | 100-200ms | 低 |
| 8 | P8: プロキシ接続プール | 30-50ms | 中 |

## 変更ファイル

### TEE (P1, P2, P3, P6, P8)

| ファイル | 変更内容 |
|---------|---------|
| `crates/tee/src/endpoints/verify/handler.rs` | プロセッサ並列実行 |
| `crates/tee/src/endpoints/verify/extension.rs` | C2PA結果のコンテキスト参照 |
| `crates/tee/src/endpoints/sign/handler.rs` | items並列処理 |
| `crates/tee/src/wasm_loader/mod.rs` | `CachedWasmLoader` 追加 |
| `crates/tee/src/infra/proxy_client.rs` | 接続プーリング |

### SDK (P4, P5, P7)

| ファイル | 変更内容 |
|---------|---------|
| `sdk/ts/src/client.ts` | 並列アップロード、パイプライニング |
| `integration-tests/register-photo.ts` | 並列ブロードキャスト、リトライ |

## テスト

- `cargo check --workspace && cargo test --workspace`
- ローカル: setup.sh → register-photo.ts → 前後の時間比較
- EC2: 同上

## 完了条件

- [x] WASMバイナリの LRU キャッシュが TEE に実装されている（CachedWasmLoader, 1h TTL）
- [x] /verify でプロセッサが並列実行される（tokio::spawn）
- [x] SDK で signed_json の Arweave アップロードが並列化されている（Promise.all）
- [x] SDK で TX ブロードキャストが並列化されている（Promise.all）
- [x] MintV2 instruction のビンパッキング（2 cNFT → 1 TX, 1024 bytes）
- [x] フル登録フローのレイテンシが 30% 改善（11秒 → 7.8秒）
- [x] 全既存テストがパスする
- P3 (C2PA重複排除): 取り下げ — 各プロセッサはステートレスに独立動作すべき
- P6 (/sign並列処理): ビンパッキングで吸収
- 残: /sign 2.7秒、Irys init 1.3秒 → Task 09 (ALT) + Irys再利用で追加改善可能
