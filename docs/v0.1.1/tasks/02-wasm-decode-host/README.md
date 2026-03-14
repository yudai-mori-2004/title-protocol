# Task 02: WASM ホスト側コンテンツデコード + メモリプール

## 目的

WASM Extension がコンテンツの生バイナリだけでなく、デコード済みデータ（画像ピクセル等）にもアクセスできるよう、ホスト側に汎用デコード関数とメモリプールを追加する。phash-v1 を dHash から pHash (DCT) に移行し、デコードホスト関数を活用する最初の実装例とする。

## 背景

### 問題 1: WASM 内デコードの限界

現在 `wasm/phash-v1` は `zune-jpeg` / `zune-png` を WASM 内でリンクし `#![no_std]` 環境で画像デコードを行っている。この方式の問題:

- **対応フォーマットの追加が困難** — 新フォーマットごとに no_std 対応の Rust デコーダを探して WASM バイナリに含める必要がある
- **WASM バイナリサイズの肥大化** — デコーダのコード自体が WASM に入る
- **Fuel 消費** — デコード処理の計算コストが WASM の Fuel 制限を圧迫する
- **メモリ管理の不透明性** — WASM 内でのメモリ確保はホスト側から監視できない

### 問題 2: メモリ安全性

デコード処理は圧縮爆弾のリスクを伴う（例: 10KB の PNG → 30000×30000 RGBA = 3.6GB）。ホスト側でデコードすることで、デコード前にヘッダからサイズを読み取り、メモリ予約を事前検証できる。

### 問題 3: pHash への移行

現在の dHash（difference hash）は実装が簡易だが、pHash（DCT ベース）の方が:

- 画像変換（リサイズ、圧縮、色調補正）に対してロバスト
- 学術的にも広く使われている標準アルゴリズム
- ハミング距離による類似度判定の精度が高い

## 設計

### メモリプール（セマフォ方式）

```
┌───────────────────────────────────────────────────┐
│                  MemoryPool                        │
│  total_limit: 設定可能（例: 1GB）                  │
│                                                    │
│  ┌──────────────┐  ┌───────────────┐  ┌─────┐    │
│  │ Semaphore A  │  │ Semaphore B   │  │ C…  │    │
│  │ (raw binary) │  │ (decoded)     │  │将来 │    │
│  │ used: 10MB   │  │ used: 100MB   │  │     │    │
│  └──────────────┘  └───────────────┘  └─────┘    │
│                                                    │
│  判定: A.used + B.used + … + 新規要求 ≤ total     │
└───────────────────────────────────────────────────┘
```

- **Semaphore A（raw）**: バイナリ取得時に確保、リクエスト完了時に解放
- **Semaphore B（decoded）**: `decode_content` 呼び出し時に確保、リクエスト完了時に解放
- A と B は論理的に独立（ライフサイクルが異なる）、物理的には同一プールを共有
- 将来 Semaphore C（別種の変換）を追加しても同じ構造で拡張可能

### 圧縮爆弾対策

```
decode_content() 呼び出し
  → ヘッダだけ読む（数十〜数百バイト、デコードなし）
  → width × height × channels で decoded_size を計算
  → Semaphore B: try_acquire(decoded_size)
    → A.used + B.used + decoded_size > total_limit なら即座に拒否
    → 実際のデコード処理には一切入らない
  → 成功した場合のみフルデコード実行
```

### ホスト関数追加

| 関数 | シグネチャ | 対象 | 新規/既存 |
|------|-----------|------|----------|
| `read_content_chunk` | `(offset, length, buf_ptr) -> u32` | 元バイナリ | 既存（変更なし） |
| `get_content_length` | `() -> u32` | 元バイナリ | 既存（変更なし） |
| `hash_content` | `(algorithm, offset, length, out_ptr) -> u32` | 元バイナリ | 既存（変更なし） |
| `hmac_content` | `(algorithm, key_ptr, key_len, offset, length, out_ptr) -> u32` | 元バイナリ | 既存（変更なし） |
| `get_extension_input` | `(buf_ptr, buf_len) -> u32` | 補助入力 | 既存（変更なし） |
| `decode_content` | `(target_format, params_ptr, params_len, metadata_ptr) -> i32` | 生→decoded | **新規** |
| `read_decoded_chunk` | `(offset, length, buf_ptr) -> u32` | decoded | **新規** |
| `get_decoded_length` | `() -> u32` | decoded | **新規** |

### target_format enum

| 値 | 名称 | 出力形式 | metadata (12 bytes) |
|----|------|---------|---------------------|
| 0 | `GRAYSCALE_U8` | 1 byte/px, row-major | `[width:u32 LE, height:u32 LE, channels=1:u32 LE]` |
| 1 | `RGB_U8` | 3 bytes/px, row-major | `[width:u32 LE, height:u32 LE, channels=3:u32 LE]` |
| 2 | `RGBA_U8` | 4 bytes/px, row-major | `[width:u32 LE, height:u32 LE, channels=4:u32 LE]` |
| 16+ | 将来拡張（音声等） | 未定 | 未定 |

### decode_content 戻り値

| 値 | 意味 |
|----|------|
| 0 | 成功 |
| -1 | 非対応フォーマット |
| -2 | メモリ予算超過（圧縮爆弾検出を含む） |
| -3 | デコードエラー |

### メモリフロー

```
1. リクエスト到着
2. Semaphore A: try_acquire(content_size)      [A: content_size]
3. バイナリ保持 → InnerHostState.content
4. WASM 実行開始
5. WASM: read_content_chunk() で元バイナリ読み取り可能
6. WASM: decode_content(0, ...) 呼び出し
7. ホスト:
   a. ヘッダ読み → width, height 判明
   b. decoded_size = width * height * channels
   c. Semaphore B: try_acquire(decoded_size)    [A + B ≤ total?]
   d. 成功 → デコード実行 → InnerHostState.decoded に格納
8. WASM: read_decoded_chunk() でピクセルデータ読み取り
9. WASM: hash_content() で元バイナリのハッシュも取得可能
10. WASM 実行完了
11. Semaphore B: release(decoded_size)
12. Semaphore A: release(content_size)
```

### pHash (DCT) アルゴリズム

phash-v1 を以下のアルゴリズムに書き換える:

1. `decode_content(0, ...)` → グレースケールピクセル取得
2. 32×32 にバイリニア補間リサイズ
3. 2D DCT（分離型: 行方向 → 列方向、O(N³)）
4. 左上 8×8 ブロック（低周波成分）を抽出
5. 64 値の平均を計算（DC 成分を除く 63 値）
6. 各値を平均と比較 → 64-bit ハッシュ生成

出力 JSON: `{"phash":"<16桁hex>","algorithm":"phash-dct","bits":64}`

## 変更ファイル

### 新規作成

| ファイル | 内容 |
|---------|------|
| `crates/wasm-host/src/memory_pool.rs` | `MemoryPool` 構造体（セマフォA/B/…管理） |
| `crates/wasm-host/tests/phash_integration.rs` | phash-v1 統合テスト（WASM 実行による pHash 品質検証） |
| `tests/fixtures/test_4x4.jpg` | `image` crate でデコード可能な 4×4 JPEG テスト画像 |

### 仕様書更新

| ファイル | 変更内容 |
|---------|---------|
| `docs/v0.1.1/SPECS_JA.md` §7.1 | ホスト関数一覧に `decode_content` / `read_decoded_chunk` / `get_decoded_length` を追加。メモリプール（セマフォ方式）の仕様を追記 |
| `docs/v0.1.1/SPECS_JA.md` §7.4 | phash-v1 のアルゴリズムを dHash → pHash (DCT) に更新。WASM 内デコードからホスト側デコードへの移行を反映 |

### 変更

| ファイル | 変更内容 |
|---------|---------|
| `Cargo.toml`（ワークスペース） | `image` をワークスペース依存に追加 |
| `crates/wasm-host/Cargo.toml` | `image` 依存追加、`c2pa` / `serde_json` を dev-dependencies に追加 |
| `crates/wasm-host/src/lib.rs` | `DecodedContent` 構造体、`InnerHostState` に `decoded` フィールド追加、`decode_content` / `read_decoded_chunk` / `get_decoded_length` ホスト関数登録、`WasmRunner` に `Arc<MemoryPool>` 保持、C2PA デコードテスト追加 |
| `crates/tee/src/config.rs` | `TeeAppState` に `wasm_memory_pool` フィールド追加 |
| `crates/tee/src/main.rs` | `MemoryPool` 初期化、`TeeAppState` 構築に追加 |
| `crates/tee/src/endpoints/verify/extension.rs` | `WasmRunner::with_memory_pool()` に変更 |
| `crates/tee/src/endpoints/*/tests.rs` 等（12箇所） | `TeeAppState` 構築に `wasm_memory_pool` 追加 |
| `wasm/phash-v1/Cargo.toml` | `zune-jpeg` / `zune-png` / `zune-core` 削除、`libm` 追加 |
| `wasm/phash-v1/src/lib.rs` | 画像デコードコード削除、`decode_content` / `read_decoded_chunk` / `get_decoded_length` extern 宣言追加、pHash (DCT) 実装 |

### テスト画像フィクスチャ統合

テスト画像・証明書を `tests/fixtures/` に集約。旧ロケーション（`crates/core/tests/fixtures/`、`crates/wasm-host/tests/`）は削除。

| ファイル | 変更内容 |
|---------|---------|
| `crates/core/src/lib.rs` | `include_bytes!` パスを `tests/fixtures/` に変更 |
| `crates/core/examples/gen_fixture.rs` | 同上 |
| `crates/tee/src/endpoints/verify/tests.rs` | 同上 |

### Semaphore A 統合（スコープ外）

Semaphore A の統合は既存のバイナリメモリ管理フロー（`crates/tee` / `crates/gateway`）に依存するため、本タスクでは `MemoryPool` の構造体と Semaphore B の動作を完成させる。Semaphore A の呼び出し元統合は、上流のメモリ管理を精査した上で別タスクとする。

## テスト

### ユニットテスト（`crates/wasm-host`）

- `MemoryPool` の `try_acquire` / `release` の基本動作
- `try_acquire` が `total_limit` を超える場合に `false` を返すこと
- 複数セマフォの合算が `total_limit` と正しく比較されること
- `decode_content` が PNG を正しくデコードすること（WAT テスト）
- `decode_content` が C2PA 署名済み JPEG を正しくデコードすること（WAT テスト + c2pa crate で動的生成）
- `decode_content` が非対応フォーマットで `-1` を返すこと
- `decode_content` がメモリ予算超過で `-2` を返すこと
- デコード前に `read_decoded_chunk` を呼んだ場合に 0 を返すこと
- `MemoryPool` の Drop 時に Semaphore B が解放されること
- 既存テスト（10件）が引き続きパスすること

### 統合テスト（`crates/wasm-host/tests/phash_integration.rs`）

phash-v1 は `#![no_std]` WASM でホスト関数に依存するため、単体テストは不可。
コンパイル済み `phash_v1.wasm` を `WasmRunner` で実行する統合テストとして検証する。

**前提:** `cd wasm/phash-v1 && cargo build --target wasm32-unknown-unknown --release`

- 同一画像の JPEG / PNG エンコードが同じ pHash を返すこと（ハミング距離 ≤ 5）
- リサイズ後の画像（256×256 vs 64×64）が近い pHash を返すこと（ハミング距離 ≤ 5）
- 異なる画像（グラデーション vs チェッカーボード）が異なる pHash を返すこと（ハミング距離 ≥ 20）
- pHash 計算が決定的であること（同一入力 → 同一ハッシュ、退化なし）

テスト画像は `image` crate でプログラム的に生成する。WASM 未ビルド時はスキップされる。

### ビルド確認

- `cargo check --workspace && cargo test --workspace`
- `cd wasm/phash-v1 && cargo build --target wasm32-unknown-unknown --release`
- `cargo test --package title-wasm-host --test phash_integration`

## 完了条件

- [x] `MemoryPool` がセマフォ方式でメモリ予算を管理し、合計が `total_limit` 以下であることを保証する
- [x] `decode_content` がヘッダ先読みで圧縮爆弾を検出し、メモリ予算超過時に `-2` を返す
- [x] `decode_content` が画像を指定 `target_format` にデコードし、`InnerHostState.decoded` に格納する
- [x] `read_decoded_chunk` / `get_decoded_length` でデコード済みデータにチャンクアクセスできる
- [x] 元バイナリへのアクセス（`read_content_chunk` / `hash_content` 等）がデコード後も維持される
- [x] phash-v1 が pHash (DCT) アルゴリズムを実装し、`decode_content` ホスト関数を使用する
- [x] phash-v1 から `zune-jpeg` / `zune-png` / `zune-core` 依存が削除されている
- [x] 対応画像フォーマット: JPEG, PNG, WebP, GIF, BMP, TIFF（`image` crate 経由）
- [x] 全既存テスト + 新規テストがパスする（ユニットテスト 20件 + 統合テスト 4件）
- [x] `wasm32-unknown-unknown` ターゲットで phash-v1 がビルドできる
- [x] `target_format` enum が将来の音声・動画拡張に対応可能な構造である（16+ 予約）
- [x] C2PA 署名済み JPEG のデコードテストが含まれている
- [x] pHash の品質テスト: JPEG/PNG 同一性、リサイズ耐性、異画像分離、決定性
- [x] `docs/v0.1.1/SPECS_JA.md` §7.1 にホスト関数追加・メモリプール仕様が反映されている
- [x] `docs/v0.1.1/SPECS_JA.md` §7.4 に pHash (DCT) アルゴリズムとホスト側デコードが反映されている
- [x] テスト画像フィクスチャが `tests/fixtures/` に集約されている

## 参照

- `crates/wasm-host/src/lib.rs` — 現在のホスト関数実装（5関数）
- `wasm/phash-v1/src/lib.rs` — 現在の dHash 実装
- `docs/v0.1.0/SPECS_JA.md` §7.1 — WASM 実行環境仕様
- `docs/v0.1.0/SPECS_JA.md` §7.4 — Extension モジュール仕様
