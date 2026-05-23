# タスク05: provenance-graph Processor 実装

## 目的

C2PA マニフェストから素材情報を再帰的に抽出し、来歴グラフ（DAG）として出力する provenance-graph processor を実装する。

## 読むべきファイル

1. `CLAUDE.md`
2. `docs/v0.1.2/SPECS_JA.md` — 特に:
   - §3.2 provenance-graph の入出力定義（nodes + edges の JSON 構造）
   - §4.4 来歴グラフの最大サイズ（10,000 ノード+エッジ）
3. `docs/v0.1.2/COVERAGE.md`
4. `crates/core/src/processor.rs` — Processor trait
5. `crates/core/src/processor_outputs.rs` — ProvenanceGraphOutput 型
6. `legacy/v0.1.0/crates/core/src/lib.rs` — **`build_provenance_graph()` がそのまま移植元。ingredient 再帰処理、グラフサイズチェック、深度制限（MAX_INGREDIENT_DEPTH=32）が実装済み。**

## スコープ

### やること

1. **provenance-graph processor 実装**:
   - `Processor` trait の実装体
   - Active Manifest の ingredient 情報を再帰的に抽出
   - 各 ingredient の signature_hash を算出してノード ID とする
   - source → target のエッジを構築（role = コンテンツ種別）
   - グラフサイズ上限チェック（10,000）
   - 再帰深度制限（スタックオーバーフロー防止）

2. **テスト**:
   - ingredient なしコンテンツ → ルートノードのみ
   - ingredient 付きコンテンツ → ノード + エッジ
   - グラフサイズ超過時のエラー
   - `ProvenanceGraphOutput` の serde 互換

### やらないこと

- 他の processor の実装
- グラフの可視化

## 依存

- Task 02: Processor trait + ProvenanceGraphOutput
- Task 03: signature_hash 算出ユーティリティ

## 成功基準

- [ ] `ProvenanceGraphProcessor` が `Processor` trait を実装
- [ ] ingredient の再帰抽出が動作する
- [ ] グラフサイズ上限を超えた場合にエラーを返す
- [ ] `cargo test` 合格
- [ ] COVERAGE.md 更新
