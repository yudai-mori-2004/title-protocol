# タスク06: image-pdq Processor 実装

## 目的

画像の知覚ハッシュを PDQ アルゴリズムで算出する image-pdq processor を実装する。256ビットの知覚ハッシュと品質スコアを出力する。

## 読むべきファイル

1. `CLAUDE.md`
2. `docs/v0.1.2/SPECS_JA.md` — 特に:
   - §3.2 image-pdq の入出力定義（pdqhash + quality の JSON 構造）
   - §3.2 処理内容: グレースケール化 → 64×64ダウンサンプル → DCT → 256ビットハッシュ
3. `docs/v0.1.2/COVERAGE.md`
4. `crates/core/src/processor.rs` — Processor trait
5. `crates/core/src/processor_outputs.rs` — ImagePdqOutput 型
6. `legacy/v0.1.0/wasm/image-pdq/` — **PDQ アルゴリズムの実装。v0.1.0 では `#![no_std]` + dlmalloc の WASM モジュールだが、v0.1.2 では標準 Rust にポートする。アルゴリズム自体（DCT, ハッシュ算出）はそのまま流用可能。**

## スコープ

### やること

1. **image-pdq processor 実装**:
   - `Processor` trait の実装体
   - 画像デコード（JPEG, PNG 等 → ピクセルデータ）
   - グレースケール変換
   - 64×64 ダウンサンプリング
   - DCT（離散コサイン変換）ベースのハッシュ算出
   - 品質スコア（quality）の算出
   - 結果を `ImagePdqOutput` として構築

2. **テスト**:
   - テスト画像での PDQ ハッシュ算出
   - 同一画像の決定論性（同じ画像 → 同じハッシュ）
   - リサイズ/再圧縮耐性の基本テスト
   - 非画像コンテンツへのエラーハンドリング

### やらないこと

- 動画フレームの PDQ（それは video-vpdq — Task 07）
- ハッシュの類似度比較ロジック（アプリケーション層の責任）

## 依存

- Task 02: Processor trait + ImagePdqOutput

## 成功基準

- [ ] `ImagePdqProcessor` が `Processor` trait を実装
- [ ] JPEG/PNG 画像から 256 ビット PDQ ハッシュを算出できる
- [ ] 品質スコアが算出される
- [ ] `cargo test` 合格
- [ ] COVERAGE.md 更新
