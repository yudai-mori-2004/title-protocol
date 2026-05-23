# タスク07: video-vpdq Processor 実装

## 目的

動画の各フレームに PDQ ハッシュを適用し、フレームハッシュ列を出力する video-vpdq processor を実装する。

## 読むべきファイル

1. `CLAUDE.md`
2. `docs/v0.1.2/SPECS_JA.md` — 特に:
   - §3.2 video-vpdq の入出力定義（frame_hashes の JSON 構造）
   - §3.2 処理内容: 1fps でフレーム抽出、各フレームに PDQ、品質フィルタ
3. `docs/v0.1.2/COVERAGE.md`
4. `crates/core/src/processor.rs` — Processor trait
5. `crates/core/src/processor_outputs.rs` — VideoVpdqOutput, FrameHash 型
6. `legacy/v0.1.0/wasm/video-vpdq/` — **vPDQ の WASM 実装。フレーム抽出 + PDQ 適用のパターンがある。**
7. Task 06 の image-pdq 実装 — PDQ ハッシュ算出ロジックを共有する

## スコープ

### やること

1. **video-vpdq processor 実装**:
   - `Processor` trait の実装体
   - 動画からのフレーム抽出（1fps）
   - 各フレームに PDQ ハッシュ算出（Task 06 のロジックを共有）
   - 品質が低いフレームの除去
   - 前フレームと同一ハッシュのフレームの除去
   - 結果を `VideoVpdqOutput` として構築

2. **テスト**:
   - テスト動画でのフレームハッシュ列算出
   - 品質フィルタの動作
   - 重複フレーム除去の動作
   - 非動画コンテンツへのエラーハンドリング

### やらないこと

- PDQ アルゴリズム自体の実装（Task 06 から共有）
- 動画のトランスコードや再エンコード

## 依存

- Task 02: Processor trait + VideoVpdqOutput
- Task 06: PDQ ハッシュ算出ロジック

## 成功基準

- [ ] `VideoVpdqProcessor` が `Processor` trait を実装
- [ ] MP4 動画から 1fps でフレームハッシュ列を算出できる
- [ ] 品質フィルタ・重複除去が動作する
- [ ] `cargo test` 合格
- [ ] COVERAGE.md 更新
