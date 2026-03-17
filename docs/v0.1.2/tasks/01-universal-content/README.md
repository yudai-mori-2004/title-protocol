# Task 01: ユニバーサルコンテンツ対応

## 目的

Title Protocol の core フローを、c2pa crate が対応する全コンテンツ形式で通るようにする。画像・動画・音声・文書の全てを1つのプロトコルで扱い、コンテンツの出自を証明する。

## 対応形式

c2pa crate v0.75 が対応する全形式を Title Protocol で通す：

| カテゴリ | 形式 | MIME | 用途 |
|----------|------|------|------|
| 画像 | JPEG | image/jpeg | カメラ撮影、Web |
| 画像 | PNG | image/png | スクリーンショット、グラフィック |
| 画像 | WebP | image/webp | Web |
| 画像 | AVIF | image/avif | 次世代Web画像 |
| 画像 | HEIC/HEIF | image/heic | iPhone撮影 |
| 画像 | TIFF | image/tiff | 印刷、DNG RAW |
| 画像 | DNG | image/x-adobe-dng | RAW写真 |
| 画像 | GIF | image/gif | アニメーション |
| 動画 | MP4 | video/mp4 | 汎用動画 |
| 動画 | MOV | video/quicktime | iPhone動画、ProRes |
| 音声 | WAV | audio/wav | 非圧縮音声 |
| 音声 | MP3 | audio/mpeg | 圧縮音声 |
| 文書 | PDF | application/pdf | 文書全般 |

## 現状分析

### 既にコンテンツ形式非依存のレイヤー（変更不要）

| レイヤー | 理由 |
|----------|------|
| `content_hash` 計算 | Active Manifest 署名の SHA-256。コンテンツ自体を見ない |
| Provenance graph | C2PA マニフェストの DAG。コンテナ形式に依存しない |
| c2pa crate の Reader | 上記全形式の C2PA 読み取りに対応済み |
| hardware-google, c2pa-training, c2pa-license WASM | バイトパターン検索のみ |
| SDK / Gateway / Crypto / Proxy | 全てバイト透過 |
| Solana cNFT / Arweave 保存 | content_hash と signed_json のみ。形式無関係 |

### 変更が必要な箇所（3箇所のみ）

#### 1. MIME 検出（`crates/tee/src/endpoints/verify/mod.rs`）

現在 JPEG/PNG/WebP の3形式のみ。他は `application/octet-stream` になり c2pa::Reader が失敗する。

**修正:** 全形式の magic bytes を追加。

#### 2. pHash extension の graceful skip

pHash は画像ピクセルの DCT ベースのアルゴリズムで、動画・音声・PDF には適用不可。

**修正:** 非画像コンテンツでは pHash をスキップ。processor_ids に `image-phash` が含まれていても、非画像なら結果から除外して正常終了。

将来的に `video-phash`, `audio-fingerprint` 等の知覚ハッシュモジュールを別途追加可能。

#### 3. JUMBF 抽出の汎用化（`crates/wasm-host/src/c2pa_cert.rs`）

`extract_jumbf_from_jpeg()` が JPEG APP11 セグメント専用。MP4 では BMFF ボックス、PDF では xref ストリームに JUMBF が格納される。

**修正:** `c2pa::jumbf_io::load_jumbf_from_memory(mime_type, data)` に委譲し、コンテナ形式別の抽出を c2pa crate に任せる。

## 変更ファイル

| ファイル | 変更内容 |
|---------|---------|
| `crates/tee/src/endpoints/verify/mod.rs` | `detect_mime_type` に全13形式の magic bytes 追加 |
| `crates/tee/src/endpoints/verify/handler.rs` | content_type を extension 実行に渡す |
| `crates/tee/src/endpoints/verify/extension.rs` | 非対応 content_type の extension を graceful skip |
| `crates/wasm-host/src/c2pa_cert.rs` | JUMBF 抽出を c2pa crate に委譲 |
| `wasm/phash-v1/src/lib.rs` | 非画像で明示的なスキップ応答を返す |

## テスト

テスト用ファイルは c2pa crate の `c2patool` で C2PA 署名を付与して作成：

- C2PA 署名付き MP4 で core-c2pa が content_hash + provenance graph を返す
- C2PA 署名付き HEIC で同上
- C2PA 署名付き PDF で同上
- C2PA 署名付き WAV で同上
- 非画像 + image-phash 指定 → pHash がスキップされ core-c2pa のみ返る
- 全既存画像テストがパスする

## 完了条件

- [ ] `detect_mime_type` が c2pa 対応全13形式を認識する
- [ ] C2PA 署名付き動画 (MP4) で core-c2pa が content_hash を返す
- [ ] C2PA 署名付き HEIC で core-c2pa が content_hash を返す
- [ ] C2PA 署名付き PDF で core-c2pa が content_hash を返す
- [ ] 非画像コンテンツで image-phash が graceful skip する
- [ ] JUMBF 抽出が全コンテナ形式で動作する
- [ ] 全既存テストがパスする

## スコープ外

- 動画/音声の知覚ハッシュ（`video-phash`, `audio-fingerprint`）→ 別タスク
- `decode_content` の動画/音声対応（キーフレーム抽出等）→ 別タスク
- 新しい WASM extension の追加 → 別タスク
