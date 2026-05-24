# v0.1.2 大監査 — 成果物インデックス

監査の趣旨と進め方は [タスク 16](../tasks/16-audit/README.md) を参照。

各観点は独立した監査エージェント（Opus 4.6）が担当する。重複指摘は許容。最終的な修正計画は別タスク（17）で集約する。

## 成果物一覧

| 観点 | ファイル | 主担当 |
|---|---|---|
| A. コメント・ドキュメント癖 | [a-comment-hygiene.md](./a-comment-hygiene.md) | コメント / docstring の冗長性・偏り |
| B. 死んでいるコード | [b-dead-code.md](./b-dead-code.md) | 未使用 / 到達不能 / 移植漏れ |
| C. エラーハンドリング | [c-error-handling.md](./c-error-handling.md) | unwrap・握りつぶし・panic |
| D. アーキテクチャ・ディレクトリ | [d-architecture.md](./d-architecture.md) | crate 境界・配置・循環依存 |
| E. 再現性・ビルド品質 | [e-reproducibility.md](./e-reproducibility.md) | Cargo / Docker / Terraform / scripts |
| F. ドキュメント整合性 | [f-docs-consistency.md](./f-docs-consistency.md) | SPECS_JA ↔ 実装 ↔ README |
| G. セキュリティ最終確認 | [g-security-wrapup.md](./g-security-wrapup.md) | 過去監査残存 + 新規 |
| H. OSS 成熟度 | [h-oss-maturity.md](./h-oss-maturity.md) | 初見導線・サポート文書 |
| I. テスト品質 | [i-test-quality.md](./i-test-quality.md) | カバレッジ・本質的か |
| J. 実機検証 | [j-runtime-verification.md](./j-runtime-verification.md) | 稼働中スタックの挙動 |

## 各成果物の必須セクション

監査者は以下のテンプレを使う。

```markdown
# <観点名>

## 概要
担当範囲 / 監査方針 / 件数サマリ。

## 重大度別内訳
- must-fix: N 件
- should-fix: M 件
- nitpick: K 件

## 発見

### must-fix-001 <短い件名>
- 場所: `path/to/file.rs:42`
- 観察: <コードの引用と現状>
- 問題: <なぜ問題か>
- 修正案: <具体的な書き直し or 削除>

### must-fix-002 ...
（以下同様）

## 全体所感
<監査者からの一文>
```

## 合計件数

**21 エージェント / 459 件**（G を含めた重大度別分類含む）。

| 重大度 | 件数 |
|---|---|
| must-fix (G の Critical / High 含む) | **131** |
| should-fix (G の Medium 含む) | **203** |
| nitpick (G の Low 含む) | **125** |

最大ボリュームは A コメント癖 65 件 — 4.7 の癖が定量化された。次いで K3 tee 31 件、B dead code / C error handling が各 30 件。

## ステータス

エージェント完了時にこの表を更新する（最初は全て pending）。

| 観点 | ステータス |
|---|---|
| A コメント癖 | done (must:23, should:28, nitpick:14) |
| B dead code | done (must:10, should:15, nitpick:7) |
| C error handling | done (must:11, should:12, nitpick:7) |
| D architecture | done (must:5, should:12, nitpick:6) |
| E reproducibility | done (must:6, should:10, nitpick:7) |
| F docs consistency | done (must:7, should:11, nitpick:6) |
| G security wrapup | done (C:2, H:2, M:5, L:4 — Verdict No) |
| H OSS maturity | done (must:4, should:9, nitpick:7) |
| I test quality | done (must:9, should:10, nitpick:5) |
| J runtime verification | done (must:1, should:1, nitpick:2; 6 pass / 4 partial / 4 skipped) |
| K1 attestation | done (must:7, should:11, nitpick:6) |
| K2 crypto | done (must:4, should:6, nitpick:6) |
| K3 tee | done (must:6, should:14, nitpick:11) |
| K4 gateway | done (must:5, should:9, nitpick:6) |
| K5 solana+program | done (must:4, should:13, nitpick:7) |
| K6 proxy | done (must:5, should:7, nitpick:4) |
| K7 sp1-guests | done (must:3, should:6, nitpick:4) |
| K8 core | done (must:5, should:10, nitpick:7) |
| Q SPECS_JA self | done (must:9, should:13, nitpick:7) |
| R Solana/Anchor specifics | done (must:4, should:11, nitpick:6) |
| S v0.1.0→v0.1.2 regression | done (must:3, should:7, nitpick:4) |
| I | pending |
| J | pending |
