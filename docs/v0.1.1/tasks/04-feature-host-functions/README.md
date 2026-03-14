# Task 04: Feature Host Functions — get_content_feature / get_decoded_feature

## 目的

ホスト関数APIを拡張し、WASMモジュールがデコード済みデータをWASMメモリに転送せずにホスト側で特徴量計算を行えるようにする。これによりfuel消費・メモリ転送量を劇的に削減する。

## 背景

### 問題: WASM側でのピクセル処理がボトルネック

pHash計算の現状フロー:
1. ホスト: ネイティブデコード (RGB/RGBA) — 数百万ピクセル
2. WASM: `read_decoded_chunk` で全ピクセル転送 — 数MB
3. WASM: RGB→グレースケール変換 — 数百万回のピクセルループ（fuel大量消費）
4. WASM: 32×32リサイズ — バイリニア補間
5. WASM: DCT → pHash — 1024要素の行列演算

ステップ2〜4がfuel消費の大部分を占める。2MB画像でデコード後数百万ピクセル、WASM fuel制限付きで回すのが根本原因。

### 設計方針: 「データをWASMに渡す」から「計算をホストに委譲」へ

`compute_hash` (hash_content) のようにホスト側で計算して結果だけ返すパターンを一般化する。デコード済みデータはホスト側に保持したまま、WASMがJSON specで「何を計算してほしいか」を指定し、ホストがネイティブ速度で実行、小さな結果だけWASMメモリに返す。

## 設計

### 新ホスト関数

```
get_content_feature(spec_ptr: u32, spec_len: u32, output_ptr: u32) -> i32
get_decoded_feature(spec_ptr: u32, spec_len: u32, output_ptr: u32) -> i32
```

対称的なシグネチャ。specはJSON:

```json
// get_content_feature
{"op": "sha256"}
{"op": "sha256", "offset": 0, "length": 1024}
{"op": "sha384"}
{"op": "sha512"}

// get_decoded_feature
{"op": "grayscale_resize", "width": 32, "height": 32}
```

戻り値: 出力バイト数（正値）またはエラーコード（負値）

### エラーコード

| コード | 意味 |
|--------|------|
| -1 | specパースエラー / 未知のop |
| -2 | コンテンツ範囲外 |
| -3 | 出力バッファ境界外 |
| -4 | デコード未実行（get_decoded_feature のみ） |
| -5 | チャネル数不正 / データサイズ不一致 |

### pHash処理フロー変化

```
変更前:
  decode_content → read全ピクセル(数MB) → WASM: grayscale(数百万回ループ) → WASM: resize → DCT
  Fuel: 数千万〜1億

変更後:
  decode_content → get_decoded_feature(grayscale_resize 32x32) → 1024バイト → WASM: DCTのみ
  Fuel: 数万で済む
```

### 廃止

- `hash_content`: `get_content_feature` に吸収（全WASMモジュールで未使用のため破壊的変更なし）

### 維持

- `read_content_chunk` / `get_content_length`: I/Oプリミティブ（C2PA系モジュールがチャンク読みで使用）
- `decode_content` / `read_decoded_chunk` / `get_decoded_length`: 全ピクセルがWASM側で必要なケース用
- `get_extension_input`: Extension入力パラメータ
- `hmac_content`: HMAC計算（鍵パラメータが異なるため別途維持）

### メモリ設計

- デコード済みデータはホスト側に保持（decode_ticketで追跡済み）
- `get_decoded_feature` の出力は小さい（32×32×1ch = 1024バイト）
- 中間バッファ（grayscale変換）はホスト側一時割当、関数終了時に解放
- ResourcePoolの追加予約不要

## 変更ファイル

| ファイル | 変更内容 |
|---------|---------|
| `crates/wasm-host/src/lib.rs` | DecodedContentにwidth/height/channels追加、`get_content_feature`追加、`get_decoded_feature`追加、`hash_content`削除、全WATテスト更新 |
| `wasm/phash-v1/src/lib.rs` | `get_decoded_feature`使用に書き換え、`read_all_decoded`/`rgb_to_grayscale`/`resize_bilinear`削除、`compute_phash_dct`簡素化 |
| `wasm/hardware-google/src/lib.rs` | extern宣言: `hash_content` → `get_content_feature` |
| `wasm/c2pa-training-v1/src/lib.rs` | 同上 |
| `wasm/c2pa-license-v1/src/lib.rs` | 同上 |
| `docs/v0.1.1/SPECS_JA.md` | §7.1 ホスト関数ABIテーブル更新 |
| `docs/v0.1.1/COVERAGE.md` | Task 04 追加 |

## 完了条件

1. `cargo check --workspace && cargo test --workspace` パス
2. `cd wasm/phash-v1 && cargo build --target wasm32-unknown-unknown --release` 成功
3. phash統合テスト4件パス（hamming距離の閾値維持）
4. `get_content_feature` SHA-256テストパス
5. `get_decoded_feature` grayscale_resizeテストパス
6. 仕様書・COVERAGE更新
