# タスク 16: v0.1.2 全体大監査

## 目的

v0.1.2 の本番化フェーズ最終工程として、リポジトリ全体を1文単位で精査し、OSS として公開できる品質まで引き上げる。Nitro 実機での疎通は終わっている（タスク 15）。次フェーズに進む前の検収。

## なぜやるか

- 主開発を担っていた AI（Opus 4.7）には作業時の意図をコメントに残す癖がある。「ない」ものの説明、「廃止した経緯」の注釈、過剰な防御コード、その瞬間の rationale の埋め込み — これらは初見の読み手にとってノイズになる
- セキュリティ監査は複数回実施したが、コード品質・ドキュメント整合性・ディレクトリ構造・OSS としての成熟度は未監査
- このタスクを通過することで「v0.1.2 はクローンした人が困らない状態である」を担保する

## やり方

10 観点に分けて独立した監査エージェントを並列起動する。すべて Opus 4.6（主開発を担った 4.7 を意図的に避け、視点を相対化する）。

各エージェントの共通要件:

1. `docs/v0.1.2/SPECS_JA.md` を全文最初に読む（仕様との乖離を判断するため）
2. 担当範囲のファイルを「1 文 1 文」読む（速読・要約禁止）
3. 発見は file:line で正確に
4. 重大度を `must-fix / should-fix / nitpick` で分類
5. 修正案は「削除 / 書き直し（新文案）/ ファイル分割」のいずれかを明示
6. 成果物は `docs/v0.1.2/audit/<topic>.md` に Write

## 担当割当

| エージェント | 観点 | 担当範囲 | 成果物 |
|---|---|---|---|
| A | コメント・ドキュメント癖 | 全 .rs / .md / .sh / Dockerfile / Cargo.toml のコメント・docstring | [a-comment-hygiene.md](../audit/a-comment-hygiene.md) |
| B | 死んでいるコード | 未使用 fn/struct/mod、到達不能パス、移植漏れ | [b-dead-code.md](../audit/b-dead-code.md) |
| C | エラーハンドリング | unwrap/expect/panic、握りつぶし、recovery 戦略 | [c-error-handling.md](../audit/c-error-handling.md) |
| D | アーキテクチャ・ディレクトリ | crate 境界・責務、循環依存、ファイル配置 | [d-architecture.md](../audit/d-architecture.md) |
| E | 再現性・ビルド品質 | Cargo の依存固定、Dockerfile、Terraform、scripts | [e-reproducibility.md](../audit/e-reproducibility.md) |
| F | ドキュメント整合性 | SPECS_JA ↔ 実装、README ↔ 動作、COVERAGE ↔ コード | [f-docs-consistency.md](../audit/f-docs-consistency.md) |
| G | セキュリティ最終確認 | 過去監査の残存、新規発見 | [g-security-wrapup.md](../audit/g-security-wrapup.md) |
| H | OSS 成熟度 | CONTRIBUTING / SECURITY / LICENSE / quickstart / 初見導線 | [h-oss-maturity.md](../audit/h-oss-maturity.md) |
| I | テスト品質 | カバレッジ、テストが本質を検証しているか、flaky 要素 | [i-test-quality.md](../audit/i-test-quality.md) |
| J | 実機検証 | 稼働中の EC2 stack を実際に叩いて挙動確認 | [j-runtime-verification.md](../audit/j-runtime-verification.md) |

エージェント間の重複は許容する（独立判断のため）。最後に主開発者（人間 + Opus 4.7）が突合して修正計画を作る。

## 4.7 の癖の例

監査者が特に重点的に検出すべきパターン:

```rust
// AWS infrastructure (v0.1.2)
// Single EC2 with Nitro Enclaves, no Elastic IP, no S3, no IAM user.
```
→「ない」ものを列挙している。初見の読み手にとっては何の情報にもならない。削除推奨。

```rust
/// Updated in commit X to handle Y because previously Z.
```
→ 過去の git 履歴で済む情報をコードに焼き付けている。

```rust
/// Spec §6.2 — see also Spec §5.2
fn foo() {}
```
→ 全関数に貼ると価値が薄れる。本当に対応関係が読み手に必要な場所のみ残す。

```rust
// We deliberately do NOT cache this because cache invalidation would …
```
→ 「やらなかった理由」の長文 rationale が本体ロジックより長い。

```rust
let _ = ignore_this_error;  // ignored intentionally
```
→ 本当に無視していいのか不明。コメントが「監査済み」を主張するだけになっている。

これら全てを `must-fix / should-fix / nitpick` で分類し、置換文案を添える。

## 期限

エージェント全員が成果物を Write したら完了。集約は主開発者が後段で実施。

## 成功基準

- [ ] 10 観点すべてに `docs/v0.1.2/audit/*.md` が存在する
- [ ] 各成果物が「件数 / 重大度別内訳 / file:line でのリスト / 修正案」を含む
- [ ] 監査終了後の修正計画は別タスク（17）として定義する
