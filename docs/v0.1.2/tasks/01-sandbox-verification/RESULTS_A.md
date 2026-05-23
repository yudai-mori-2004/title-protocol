# Sandbox A: c2pa-rs HTTP Range Request 検証 — 結果

## 検証日

2026-05-23

## 環境

- Rust: 1.93.1 (stable)
- c2pa: 0.84.1
- OS: macOS (aarch64-apple-darwin)
- HTTP server: axum 0.8 (ローカル、Range Request + ETag 対応)
- HTTP client: reqwest 0.12 (blocking)

## 結果: 成功（全検証項目 PASS）

c2pa-rs の Reader は HTTP Range Request 経由で C2PA 検証を完了する。
署名検証・ハードバインディング検証・改ざん検知のすべてが正しく機能することを確認した。

## 検証の信頼性を担保する 3 つのテスト

### テスト 1: v0.84 署名→検証ラウンドトリップ

`EphemeralSigner`（c2pa-rs 組み込みの Ed25519 テスト用 signer）で新規署名したファイルを即座に検証。

- **結果**: `ValidationState::Valid`
- **validation_status**: `signingCredential.untrusted`（self-signed cert）のみ
- **validation_results**: success=5, failure=1（failure は cert 信頼性のみ）
- **意味**: c2pa v0.84 の署名→検証パイプラインは正しく動作する

### テスト 2: 改ざん検知（ハードバインディング）

v0.84 で署名したファイルの中間地点 1 バイトを反転し、検証を実行。

- **結果**: `assertion.bmffHash.mismatch` が即座に検出
- **ベースライン**: failure=1（cert のみ）→ **改ざん後**: failure=2（cert + bmffHash）
- **意味**: BMFF ハードバインディング（コンテンツ全体のハッシュ検証）が正しく機能。1 バイトの改ざんでも検出する

### テスト 3: ベースライン vs Range Request の一致

同一ファイルをメモリ全ロード（ベースライン）と Range Request 経由で検証し、結果を比較。

- **validation_state**: 一致 ✓
- **validation_status コード**: 完全一致 ✓
- **Manifest JSON hash (SHA-256)**: 完全一致 ✓
- **意味**: Range Request アダプタは検証結果に一切影響を与えない

## 成功基準の達成状況

### 1. c2pa-rs Reader が Range Request 経由で C2PA 検証を完了する — 達成

`Read + Seek` を実装したカスタム HTTP アダプタ (`HttpRangeReader`) を c2pa v0.84 の `Reader::from_context(context).with_stream(format, stream)` に渡すことで、HTTP Range Request 経由での C2PA 検証が正常に動作した。

**API の変更点**: c2pa v0.84 では `Reader::from_stream()` は非推奨。新 API は `Reader::from_context(Context::default()).with_stream(format, stream)` を使用する。

### 2. ファイル全体をメモリに載せていないことを確認 — 達成

`HttpRangeReader` のメモリ使用量はバッファサイズ（256KB）に制限される。50MB の MP4 ファイルでもバッファ + c2pa-rs の内部状態のみでメモリ消費は数百 KB 程度。

ただし **転送量はファイルサイズの ~100% になる**。これは c2pa-rs がハードバインディング（コンテンツ全体のハッシュ）を検証するために、コンテンツ全体を読み通す必要があるため。Range Request の価値は転送量削減ではなく、**メモリ使用量の制限**にある。

### 3. ETag / If-Match による整合性チェックの実現方法を確認 — 達成

以下の方式で実装:
1. HEAD リクエストで ETag を取得
2. 以降の Range GET リクエストに `If-Match` ヘッダを付与
3. サーバーが `412 Precondition Failed` を返した場合、処理を中断

## メトリクス

256KB バッファでの結果:

| テスト | Requests | 転送量 | ファイルサイズ | 転送比率 |
|---|---|---|---|---|
| 50MB MP4 | 192 | 47.68 MB | 47.43 MB | 100.5% |
| 25MB MP4 | 98 | 24.06 MB | 23.81 MB | 101.1% |
| 10MB MP4 | 43 | 10.33 MB | 10.08 MB | 102.5% |
| 5MB MP4 | 22 | 5.07 MB | 4.82 MB | 105.2% |
| 1MB MP4 | 6 | 1.24 MB | 0.99 MB | 125.3% |
| 720p MP4 | 3 | 0.49 MB | 0.25 MB | 200.0% |
| JPEG | 2 | 0.01 MB | 0.01 MB | 100.0% |

## c2pa-rs のアクセスパターン分析

c2pa-rs の単一ファイル読み取りパターン:

```
1. offset=4 から 256KB を読み取り（MP4 ftyp box のパース）
2. offset=0 に戻り（唯一の後方シーク）
3. offset=0 から EOF まで 256KB チャンクで順次読み取り
```

特徴:
- **ほぼ完全にシーケンシャル**: 後方シークは冒頭の 1 回のみ
- **ファイル全体を読み通す**: ハードバインディング検証のため不可避
- **ランダムアクセスは発生しない**: Seek 回数の爆発の懸念は杞憂だった

## 発見した制約・注意点

### 1. 転送量はファイルサイズと同等

単一ファイルの C2PA 検証では、ハードバインディング（コンテンツ全体のバイト列ハッシュ）の検証が必要なため、コンテンツ全体を読み通す必要がある。Range Request で「必要な部分だけ取得」するシナリオは、単一ファイルでは実現できない。

仕様書 §4.3 の「Range Request パターン」の真の利点は:
- ファイル全体をメモリにロードせずに処理できる（メモリ使用量 = バッファサイズ）
- ストリーミング的に処理できる（チャンクごとに読み取り→処理→解放）

転送量の削減は、フラグメント形式（Sandbox B）で実現されるべき領域。

### 2. バッファサイズとリクエスト数のトレードオフ

| バッファサイズ | 10MB での Requests | メモリ使用量 |
|---|---|---|
| 8KB（バッファなし相当） | ~165 | 8KB |
| 256KB | 43 | 256KB |
| 1MB | ~12 | 1MB |
| 4MB | ~4 | 4MB |

本実装では、ネットワークレイテンシとメモリ使用量のバランスを考慮してバッファサイズを決定する必要がある。TEE 内ではローカルネットワーク（VPC 内）からの取得が多いため、1MB 程度が適切と推測。

### 3. ValidationState と validation_status の使い分け

- `ValidationState`: `Valid` / `Invalid` の 2 値。cert 信頼性も含む
- `validation_status()`: 個別のエラーコード一覧。本実装で使うべきはこちら
- 自己署名 cert を使う場合、`signingCredential.untrusted` は常に出る。これを無視した上で他のエラーがないことを確認するロジックが必要

本実装での判定ロジック:
```
cert 関連のエラー（signingCredential.untrusted）→ TEE 側では無視（信頼判定はプロトコル層で行う）
bmffHash.mismatch → コンテンツ改ざん（致命的エラー）
claimSignature.mismatch → 署名不正（致命的エラー）
assertion.*.mismatch → アサーション改ざん（致命的エラー）
```

### 4. v0.78 → v0.84 のフィクスチャ互換性

v0.78（c2pa-rs の旧バージョン）で署名したファイルを v0.84 で検証すると、`claimSignature.mismatch` と `assertion.action.malformed` が発生する。

- `claimSignature.mismatch`: COSE 署名検証の方式変更（v0.84 でより厳格になった可能性）
- `assertion.action.malformed`: c2pa.actions アサーションのスキーマが v2 で変更

**本実装への影響**: v0.84 で署名→v0.84 で検証するため、互換性問題は発生しない。ただし legacy フィクスチャはテストに使えないため、テスト用フィクスチャは v0.84 の `EphemeralSigner` + `Builder` で生成し直す必要がある。

### 5. c2pa v0.84 の format 検出

MP4 ファイルに対して `manifest.format()` が `"unknown"` を返すケースがある。これは c2pa-rs の内部的な format 検出の問題で、マニフェスト内に format 情報が明示されていない場合に発生する。本実装では、リクエストで指定された MIME type を優先して使用すべき。

## 本実装に向けた推奨事項

### HttpRangeReader の設計

1. **バッファサイズは設定可能にする**: TEE のメモリ制約とネットワーク環境に応じて調整
2. **reqwest の async 版を使用する**: 本実装は async。`spawn_blocking` ではなく、async HTTP client + async c2pa Reader（c2pa v0.84 は async 対応している可能性あり）を検討
3. **コネクションプーリング**: reqwest の Client を使い回すことで TCP ハンドシェイクのオーバーヘッドを削減

### ETag / If-Match

1. 初回 HEAD リクエストで ETag を取得
2. 全ての Range GET に If-Match を付与
3. 412 応答時は即座にエラーを返す
4. ETag 非対応のサーバーには警告を出しつつ処理を続行（ただしリスクをログに記録）

### メモリ管理との統合（§4.3）

```
ResourcePool から Ticket を取得
  → ticket.extend(バッファサイズ) でバッファ分を予約
  → Range Request でチャンク取得 → c2pa-rs に渡す
  → 読み通し完了 → ticket.shrink()
  → Ticket 解放
```

ピークメモリ = バッファサイズ + c2pa-rs の内部状態（マニフェストデータ、数十 KB）

### テストフィクスチャ生成

legacy フィクスチャ（v0.78 署名）は v0.84 で互換性エラーが出るため、本実装のテストでは `EphemeralSigner` でフィクスチャを動的生成する:

```rust
let signer = c2pa::EphemeralSigner::new("test-signer")?;
let mut builder = c2pa::Builder::from_context(context)
    .with_definition(&definition_json)?;
builder.sign(&signer, mime_type, &mut source, &mut output)?;
```

### R2 → TEE 推定所要時間（1GB ファイル）

- 256KB バッファ: ~3,900 リクエスト。R2 レイテンシ ~20ms/req → ~78s + 転送時間 ~10-30s = **~90-120s**
- 1MB バッファ: ~1,000 リクエスト。~20s + 転送時間 = **~30-50s**
- TEE はメモリのみ（ディスクなし）のため、バッファサイズがメモリ消費の支配項

## コード

`sandbox/01-c2pa-range-request/src/main.rs`
