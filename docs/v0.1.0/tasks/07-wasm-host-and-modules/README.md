# タスク7: WASMホスト実行エンジン + 4モジュール実装

## 前提タスク

- タスク4（/verify）が完了していること（WASMを呼び出す側のフレームワーク）

## 読むべきファイル

1. `docs/SPECS_JA.md` — §7 全体（安全性確保、ホスト関数、公式WASMセット）
2. `crates/wasm-host/src/lib.rs` — 現在のスタブ
3. `wasm/phash-v1/src/lib.rs` — WASMモジュールのスタブ（4つとも同じパターン）
4. `crates/types/src/lib.rs` — ExtensionPayload
5. `crates/tee/src/endpoints/verify.rs` — WASMを呼び出す箇所（タスク4でTODOにした部分）

## 作業内容

### Part 1: WASMホスト（crates/wasm-host）

`execute_inner()` を wasmtime で実装する。

#### wasmtime エンジン設定
- `Config::new().consume_fuel(true)` でFuel制限を有効化
- Store にfuel_limitを設定
- Memory制限を `StoreLimitsBuilder` で設定

#### ホスト関数の登録（§7.1）

| 関数 | シグネチャ | 実装 |
|------|-----------|------|
| `read_content_chunk` | `(offset: u32, length: u32, buf_ptr: u32) -> u32` | HostStateのcontentからチャンクを読み取り、WASMメモリにコピー。実際にコピーしたバイト数を返す |
| `hash_content` | `(algorithm: u32, offset: u32, length: u32, out_ptr: u32) -> u32` | HostStateのcontent[offset..offset+length]に対してハッシュ計算。結果をWASMメモリにコピー。0=sha256, 1=sha384, 2=sha512, 3=keccak256 |
| `get_extension_input` | `(buf_ptr: u32, buf_len: u32) -> u32` | HostStateのextension_inputをWASMメモリにコピー。実際のサイズを返す。なければ0 |

#### 実行フロー
1. wasmtime Engineを作成
2. HostStateを含むStoreを作成
3. ホスト関数をLinkerに登録
4. WASMバイナリをコンパイル・インスタンス化
5. `alloc()` でWASM側にメモリ確保させる
6. エクスポートされた計算関数を呼び出す
7. 結果をWASMメモリから読み取り、`ExtensionResult` として返す

### Part 2: WASMモジュール4種

#### phash-v1（知覚ハッシュ）
- `compute_phash()` をエクスポート
- `read_content_chunk` で画像データをチャンク読み取り
- 簡易pHash実装: 画像をグレースケール8x8に縮小 → DCT → 中央値比較 → 64bitハッシュ
- 画像デコードには `no_std` 対応のライブラリが必要。最小限の実装でもよい（JPEG SOFヘッダからサイズ取得+ピクセルサンプリング等）
- 最初は `hash_content("sha256", 0, 全長)` で代替し、後から本格実装でもよい

#### hardware-google（ハードウェア撮影証明）
- `verify_hardware()` をエクスポート
- `read_content_chunk` でC2PAマニフェスト部分を読み取り
- Google Titan M2の署名チェーンをパース・検証
- 初期実装: マニフェスト内のハードウェア署名アサーションの有無を判定するだけでもよい

#### c2pa-training-v1（AI学習許可フラグ）
- `extract_training_flag()` をエクスポート
- C2PA `c2pa.training-mining` アサーションを探してフラグを返す
- `read_content_chunk` でマニフェストを読み、JSONパースしてアサーションを抽出

#### c2pa-license-v1（ライセンス情報）
- `extract_license()` をエクスポート
- C2PA Creative Work アサーションからライセンス種別・条件を抽出

### Part 3: TEE /verify への統合

タスク4で「Extension（WASM）は今はスキップ」としたTODO部分を実装:
- `processor_ids` に `core-c2pa` 以外のIDが含まれる場合、`WasmRunner` で対応するWASMを実行
- WASMバイナリの取得: 初期実装ではファイルパスまたは埋め込みで可。将来はArweaveから取得
- Extension用 `signed_json` の構築（§5.1 Step 5の構造）

## 完了条件

- `cargo test -p title-wasm-host` でホスト関数+実行のテストが通る
  - テスト: 簡単なWASMバイナリをビルドして実行、ホスト関数経由でデータ受け渡し
  - テスト: Fuel制限超過でエラー
  - テスト: パニックがcatch_unwindで捕捉される
- 4つのWASMモジュールが `cargo build --target wasm32-unknown-unknown --release` で通る
- TEE /verify で `processor_ids: ["core-c2pa", "phash-v1"]` を指定して両方のsigned_jsonが返る
- `cargo check --workspace && cargo test --workspace` が通る
- `docs/COVERAGE.md` の該当箇所を更新
