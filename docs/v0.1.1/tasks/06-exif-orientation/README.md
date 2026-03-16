# Task 06: EXIF Orientation 適用（ホスト関数デコーダー）

## 目的

画像デコーダー（`crates/wasm-host/src/decode.rs`）が EXIF orientation タグに基づく回転・反転を適用するようにし、pHash 等の後続処理が表示向きのピクセルデータに基づいて動作するようにする。

## 背景

`image::load_from_memory` は EXIF orientation を適用せず、ファイルに格納されたピクセル向きのまま返す。スマートフォンで撮影した写真は orientation=6（90° CW）等が一般的であり、未適用のままだと pHash が表示向きと異なるピクセル配列に対して計算される。

## 設計

`image_decoder::decode` でデコード後、元バイナリから EXIF orientation タグを読み取り、`DynamicImage` に回転・反転を適用する。

### EXIF Orientation マッピング

| 値 | 変換 |
|----|------|
| 1 | なし |
| 2 | 水平反転 |
| 3 | 180° 回転 |
| 4 | 垂直反転 |
| 5 | 水平反転 + 270° CW 回転 |
| 6 | 90° CW 回転 |
| 7 | 水平反転 + 90° CW 回転 |
| 8 | 270° CW 回転 |

### 依存

`exif` crate を使用して EXIF タグを読み取る。EXIF 読み取り失敗時（PNG 等 EXIF 非対応フォーマット含む）は orientation=1（無変換）として扱う。

## 変更ファイル

| ファイル | 変更内容 |
|---------|---------|
| `Cargo.toml`（ワークスペース） | `exif` 依存追加 |
| `crates/wasm-host/Cargo.toml` | `exif` 依存追加 |
| `crates/wasm-host/src/decode.rs` | `apply_exif_orientation` 関数追加、`image_decoder::decode` で呼び出し |

## テスト

- `cargo check --workspace && cargo test --workspace`
- 既存の pHash 統合テストがパスすること

## 完了条件

- [ ] `image_decoder::decode` がデコード後に EXIF orientation を適用する
- [ ] EXIF 非対応フォーマット（PNG 等）でエラーにならない
- [ ] 全既存テストがパスする
