# Task 03: ResourcePool統合 — セマフォアーキテクチャ統一

## 目的

Task 02 で導入した `MemoryPool`（セマフォB）と既存の `tokio::Semaphore`（セマフォA）を、CASベースの単一 `ResourcePool` に統合し、3つの問題を解決する。

## 背景

### 問題 1: デコード中ヒープピーク過小見積もり

セマフォ予約が `w×h×1`（grayscale出力）だが、`image` crate の `to_luma8()` は内部で `w×h×3`（JPEG→RGB中間バッファ）を確保する。実ピークは予約の4倍。

### 問題 2: ホスト側の不要な変換

`to_luma8()` で grayscale 変換を行うと中間バッファが発生する。デコードはネイティブフォーマット（JPEG→RGB, PNG→RGBA）のまま返し、grayscale 変換は各 WASM Extension 側で行うべき。

### 問題 3: A/B セマフォ統合の不在

`tokio::Semaphore`（セマフォA: raw binary ダウンロード）と `MemoryPool`（セマフォB: デコード済みデータ）が別オブジェクトで、合計管理ができていない。両方 `try_acquire_many`（非ブロッキング）なので CAS ベースの単一 ResourcePool で統一可能。

### 追加発見

`proxy_get_secured` は body 返却前にガードを drop。`proxy_get_secured_direct` は acquire 後即 drop。どちらも WASM 実行中の raw binary メモリが未追跡だった。

## 設計

### ResourcePool + Ticket

```
┌──────────────────────────────────────────────┐
│             ResourcePool                      │
│  total_limit: 設定可能（例: 1GB）             │
│  used: AtomicUsize（CAS判定の唯一の値）       │
│                                               │
│  Ticket A          Ticket B        Ticket C   │
│  (download)        (decode)        (将来)     │
│  reserved: 10MB    reserved: 100MB            │
│                                               │
│  不変条件: Σ Ticket.reserved ≤ total_limit    │
│  Drop で自動解放（パニック時も安全）           │
└──────────────────────────────────────────────┘
```

- **単一 AtomicUsize**: `used` 1つで全予約の合計を CAS 管理。A/B の区別不要
- **Ticket の Drop**: パニック時も確実に解放。手動 release 不要
- **非ブロッキング統一**: CAS ベースの `extend` は非ブロッキング。async 依存なし

### ネイティブフォーマットデコード

```
旧: decode_content(target_format=GRAYSCALE, ...) → to_luma8() → w×h×1 出力
    ピーク = w×h×3 (内部RGB) + w×h×1 (grayscale) ≈ 予約の4倍

新: decode_content(...) → ネイティブ形式で出力 (JPEG→RGB, PNG→RGBA)
    ピーク = 出力 = w×h×native_channels = 予約と一致
```

フォーマット別ネイティブチャネル数:

| フォーマット | native_channels | 理由 |
|-------------|----------------|------|
| JPEG | 3 (RGB) | JPEG は常に YCbCr→RGB |
| BMP | 3 (RGB) | 大半が RGB |
| PNG | 4 (RGBA) | 保守的（アルファチャネルの可能性） |
| GIF / WebP / TIFF | 4 (RGBA) | 保守的 |

### Ticket 保持パターン

```rust
// handler.rs — download Ticket をスコープ末尾まで保持
let (proxy_response, _download_ticket) = security::proxy_get_secured(
    ..., &state.resource_pool
).await?;
// _download_ticket は handler return 時に Drop → 予約解放
```

## 変更ファイル

### 新規作成

| ファイル | 内容 |
|---------|------|
| `crates/wasm-host/src/resource_pool.rs` | `ResourcePool` + `Ticket`（CASベース統合セマフォ、6ユニットテスト） |

### 削除

| ファイル | 理由 |
|---------|------|
| `crates/wasm-host/src/memory_pool.rs` | `resource_pool.rs` に統合 |

### 変更（Rust）

| ファイル | 変更内容 |
|---------|---------|
| `crates/wasm-host/src/lib.rs` | `mod resource_pool` + `pub use`、`decode_content` API（`target_format`削除、ネイティブデコード）、`InnerHostState` にTicket格納、`WasmRunner::with_resource_pool()` |
| `crates/tee/src/config.rs` | `memory_semaphore` + `wasm_memory_pool` → `resource_pool` |
| `crates/tee/src/infra/security.rs` | `SemaphoreGuard` 削除、`proxy_get_secured` / `proxy_get_secured_direct` がTicket返却 |
| `crates/tee/src/main.rs` | ResourcePool初期化に統一 |
| `crates/tee/src/endpoints/verify/handler.rs` | download Ticket保持パターン |
| `crates/tee/src/endpoints/verify/extension.rs` | `with_resource_pool` に変更 |
| `crates/tee/src/endpoints/sign/handler.rs` | download Ticket保持パターン |
| `crates/tee/src/endpoints/*/tests.rs` 等（12箇所） | `TeeAppState` 構築を `resource_pool` に変更 |

### 変更（WASM）

| ファイル | 変更内容 |
|---------|---------|
| `wasm/phash-v1/src/lib.rs` | `decode_content` 3引数化、`rgb_to_grayscale` 追加（ITU-R BT.601）、WASM側grayscale変換 |

### 変更（仕様書）

| ファイル | 変更内容 |
|---------|---------|
| `docs/v0.1.1/SPECS_JA.md` §6.4 | 三層防御テーブル・漸進的予約をResourcePool + Ticketに更新 |
| `docs/v0.1.1/SPECS_JA.md` §7.1 | `decode_content` API（`target_format`削除）、ABIテーブル、ResourcePool記述、圧縮爆弾対策 |
| `docs/v0.1.1/COVERAGE.md` | Task 03 エントリ追加 |

## テスト

### ResourcePool ユニットテスト（6件）

- basic acquire/release
- acquire exceeds limit → None
- ticket + extend パターン
- extend exceeds limit → false（既存予約保持）
- 複数 Ticket が同一プールを共有
- Drop で確実に解放（スコープ離脱）

### 既存テスト互換

- `cargo test --workspace` — 175件全パス
- `WasmRunner::new()` は pool なしで動作（既存 WAT テスト変更不要）
- phash-v1 統合テスト 4件が引き続きパス（RGB経由のgrayscale変換でもハッシュ品質維持）

### ビルド確認

- `cargo check --workspace && cargo test --workspace`
- `cd wasm/phash-v1 && cargo build --target wasm32-unknown-unknown --release`

## 完了条件

- [x] `ResourcePool` + `Ticket` がCASベースで単一メモリ予算を管理する
- [x] `tokio::Semaphore` と `MemoryPool` が完全に削除されている
- [x] `decode_content` が `target_format` なしでネイティブフォーマットデコードする
- [x] ピーク = 出力 = セマフォ予約（中間バッファなし）
- [x] grayscale 変換が WASM 側（phash-v1）で行われている
- [x] `proxy_get_secured` が Ticket を返し、handler がスコープ末尾まで保持する
- [x] Ticket の Drop で予約が確実に解放される（パニック安全）
- [x] 全既存テスト + 新規テスト6件がパスする
- [x] phash-v1 WASM ビルドが成功する
- [x] 仕様書（§6.4, §7.1）が更新されている
- [x] COVERAGE.md が更新されている

## 参照

- Task 02 `docs/v0.1.1/tasks/02-wasm-decode-host/README.md` — 前提タスク
- `crates/wasm-host/src/resource_pool.rs` — ResourcePool 実装
- `crates/wasm-host/src/lib.rs` — ホスト関数実装
- `crates/tee/src/infra/security.rs` — Ticket 返却パターン
- `docs/v0.1.1/SPECS_JA.md` §7.1 — WASM 実行環境仕様
