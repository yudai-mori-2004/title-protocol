# S. v0.1.0 → v0.1.2 移行で失われた / 後退したもの — Round 2

## 概要

Round 1 で 14 件（must-fix:3 / should-fix:7 / nitpick:4）を指摘。Round 2 では `docs/v0.1.2/tasks/17-audit-fixup/README.md` の進捗表示が 17a〜17g すべて `done` になっている状態を前提に、S 観点の指摘がどう処理されたかを確認した。

Round 2 監査方針:

1. 17-audit-fixup README の 17f セクション「CHANGELOG/移行（S）」に列挙された S-mf001..003 / S-sf001..007 の宣言文と、実ファイル（`CHANGELOG.md`, `docs/v0.1.2/SPECS_JA.md`, `docs/v0.1.2/COVERAGE.md`, `docs/v0.1.2/OPERATIONS_JA.md`）の現状を 1 つずつ突合
2. 17f は 17e と異なり「完了内訳」のサブセクションが README に書かれておらず、`done` のマーカーだけが付いている。実態を一次資料で確認する
3. 17a〜17e の修正で新たに v0.1.0 由来の retreat が発生していないか（特に `crates/` 配下に v0.1.0 への参照が増えていないか）を確認

件数サマリ: 7 件（must-fix 2 / should-fix 3 / nitpick 2）

## 重大度別内訳

- must-fix: 2 件
- should-fix: 3 件
- nitpick: 2 件

## Round 1 指摘の処理状況

| Round 1 ID | 種別 | 状態 | 備考 |
|---|---|---|---|
| must-fix-001 (§3.2 嘘の processor 一覧) | must-fix | **unchanged** | SPECS §3.2 冒頭「以下は初期実装で提供するprocessorの一覧である」（`docs/v0.1.2/SPECS_JA.md:726`）が現在形のまま。Gateway テスト fixture も `["c2pa-verify", "image-pdq", "provenance-graph"]` のハードコード（`crates/gateway/src/lib.rs:204`）が残る |
| must-fix-002 (TSA タイムスタンプ消失) | must-fix | **unchanged** | `grep -i "tsa\|RFC.3161\|sigTst" docs/v0.1.2/` 結果は 0 件、`crates/core/src/c2pa_verify.rs` にも `timestamp_source` 等の区別フィールドなし。CHANGELOG の Removed にも記載なし |
| must-fix-003 (CHANGELOG Removed セクション網羅性) | must-fix | **unchanged** | `CHANGELOG.md:29-35` は 6 項目のまま。Round 1 で指摘した 12 項目（TypeScript SDK、cNFT Indexer、Rust CLI、`/sign-and-mint`、`/create-tree`、`/register-node`、TSA Trust List、`signed_json` モデル、Gateway storage backends、DAO ガバナンス、コスト章、`hardware-google` / `c2pa-training-v1` / `c2pa-license-v1` WASM 等）は一切追記されていない |
| should-fix-001 (legacy processor 移植) | should-fix | partially-fixed | 17-audit-fixup README で「S-sf001 レガシー processor の移植 → 個別の実装タスク」として明示的に先送り。COVERAGE.md 上は `[ ] Not started` のままだが OPERATIONS §9 ロードマップ（`OPERATIONS_JA.md:448`）に「未実装」として明記された点は前進。ただし `(legacy: legacy/v0.1.0/wasm/image-pdq/...)` のような port-candidate 注記は付いていない |
| should-fix-002 (攻撃モデル rationale 削減) | should-fix | **unchanged** | SPECS §4.4（`SPECS_JA.md:956-986`）は「データサイズの上限」「チャンクタイムアウト」「グローバルタイムアウト」「デコード時のメモリ保護」の 4 小節があるが、Zip Bomb / Reservation DoS / Slow Write DoS の攻撃シナリオと防御手段の対応表は追加されていない |
| should-fix-003 (コスト章消失) | should-fix | **unchanged（受け入れ済み）** | 17-audit-fixup で「S-sf003 OPERATIONS にコスト/料金セクション追加 → ビジネス文書であり、コード修正ではない」として先送り宣言。`grep -i "コスト\|cost\|料金\|credit"` を `OPERATIONS_JA.md` に対して実行した結果 0 件。明示的に accepted-as-is として扱う |
| should-fix-004 (Reproducible Build 公開手段) | should-fix | **unchanged** | SPECS §5.4（`SPECS_JA.md:1117-1128`）は 12 行のまま。クライアント側検証フローの擬似コードは未追加。OPERATIONS §2.5 / §2.4 で vkey_hash 取得手順はあるが、ApprovedMeasurements PDA を読んで Attestation Document の PCR0 と照合するクライアント側手順は未文書化 |
| should-fix-005 (Core/Extension 移行ガイド) | should-fix | **unchanged** | `docs/v0.1.2/MIGRATION_FROM_010.md` は存在しない（`ls docs/v0.1.2/` 結果: `COVERAGE.md / OPERATIONS_JA.md / SPECS_JA.md / audit / tasks` のみ）。SPECS 冒頭にも用語の対応表（v0.1.0 Core/Extension → v0.1.2 用語）の追加はない |
| should-fix-006 (`troubleshooting.md` 知見の引き継ぎ) | should-fix | **unchanged** | OPERATIONS §8（`OPERATIONS_JA.md:403-439`）は 4 件のみ（anchor build、VkeyNotApproved、MeasurementNotApproved、Self-attestation failed、TEE unavailable、SP1 proof OOM の 6 件に微増したが、AES-GCM 復号失敗 / SOL 残高チェック等の v0.1.0 ナレッジは未統合。`grep "AES-GCM\|復号失敗\|SOL残高"` 結果 0 件） |
| should-fix-007 (COVERAGE 行 3 「No carryover」誤記) | should-fix | **unchanged** | `COVERAGE.md:3` が「v0.1.2 is a full rewrite. No carryover from v0.1.0/v0.1.1.」のままで、`crates/tee/src/resource_pool.rs` / `crates/core/src/jumbf.rs` での legacy 由来コメント（17e で実は削除された — 後述 regression-001 を参照）との整合性が逆方向で崩れた |
| nitpick-001 (architecture.md 縮約) | nitpick | unchanged | `docs/v0.1.2/ARCHITECTURE.md` は未作成。OPERATIONS §0 の ASCII 図のまま |
| nitpick-002 (統合テスト数の減少) | nitpick | **partially-fixed** | `crates/attestation-aws-nitro/tests/` が追加され、Gateway も `tests/e2e.rs` で 8 E2E テストに拡充された。ただし TEE 単独の `crates/tee/tests/` ディレクトリ統合テストは未追加 |
| nitpick-003 (Tree Depth 選定知見) | nitpick | unchanged | OPERATIONS に Tree Depth 選定ガイドは未追加 |
| nitpick-004 (examples/ ディレクトリ消失) | nitpick | **unchanged** | `crates/core/examples/` は依然として存在しない（`ls crates/core/examples` → No such file or directory） |

集計: fixed 0 / partially-fixed 2 / unchanged 11 / regressed 0（既存指摘のうち回帰は無し）/ accepted-as-is 1。Round 1 で出した 14 件のうち実体修正が確認できたものは 0 件、部分前進が 2 件、その他は宣言レベルで先送りまたは未対応のまま。

## 新規発見

### regression-001 17e で legacy 由来コメント（"Ported from legacy/v0.1.0/..."）を削除した結果、`COVERAGE.md:3` の「No carryover」宣言と doc コメントの整合性は逆方向に「崩れた」状態が固定された

- 場所:
  - `docs/v0.1.2/COVERAGE.md:3`: `> v0.1.2 is a full rewrite. No carryover from v0.1.0/v0.1.1.`
  - `docs/v0.1.2/tasks/17-audit-fixup/README.md:467`: 「A-mf-005/006/007/008/009 「Legacy 参照」「legacy/v0.1.0 から ported」言及を全削除(tee/lib.rs、gateway/lib.rs、resource_pool.rs、jumbf.rs、cnft.rs)」
- 観察: Round 1 should-fix-007 では「『No carryover』は事実と異なる、実装コメントには移植が明記されている」と指摘した。17e ではこの矛盾を「実装コメント側を削除する」方向で解決した。結果として COVERAGE の宣言と doc コメントは形式上一致するが、実体（ResourcePool の CAS-loop パターンや JUMBF パーサが v0.1.0 から移植されたこと）は変わっていない。これは「監査の指摘を逆方向に解消した」状態であり、OSS の読み手は「v0.1.2 はゼロから書かれた」と誤って受け取る
- 問題: Round 1 should-fix-007 で求めていたのは「COVERAGE の宣言を実態に合わせて緩める」ことだった。実装コメントの削除は「同じ嘘を 2 箇所から 1 箇所に減らした」だけで、誤情報の固定化に他ならない。OSS 公開時、他者が `legacy/v0.1.0/crates/wasm-host/src/resource_pool.rs` と現 `crates/tee/src/resource_pool.rs` を独立に diff した場合、「No carryover」の宣言と矛盾することを発見してしまう
- 重大度: should-fix
- 修正案: COVERAGE.md:3 を「`v0.1.2` is a full architectural rewrite. Selected algorithmic details (ResourcePool CAS-loop / JUMBF parser etc.) draw on v0.1.0; see `legacy/v0.1.0/` for the prior reference.」のように事実に合わせて緩める。あわせて削除した doc コメントの少なくとも 1 行（ResourcePool / JUMBF 等の中核実装）には軽い「Inspired by `legacy/v0.1.0/crates/...`」程度の出典を復活させる

### regression-002 17e の dead code 一掃で `processor_outputs.rs` を削除した結果、SPECS §3.2 で「将来提供する」と明示すらされていない processor 群の Rust 型が拠り所を失い、移植の足場まで消えた

- 場所:
  - 削除: `docs/v0.1.2/tasks/17-audit-fixup/README.md:373`: 「K8-mf001 dead public API: `processor_outputs.rs` を削除(`ProvenanceGraphOutput`/`GraphNode`/`GraphEdge`/`ImagePdqOutput`/`VideoVpdqOutput`/`FrameHash`/`CertVerifyOutput`/`CertChainEntry` は使われていない予示型)」
  - SPECS §3.2: `docs/v0.1.2/SPECS_JA.md:756-839` で `provenance-graph` / `image-pdq` / `video-vpdq` / `cert-*` の出力 JSON 構造を「現行のprocessor一覧」として記載
- 観察: 17e は K8-mf001 で「予示型」を dead code として削除した。一方で SPECS §3.2 は同じ processor 群の出力 JSON 例を「現行」として残している。仕様書側は未修正、実装側は型まで消滅。両者の乖離が拡大した
- 問題: 仕様書を Source of Truth として読みに来た開発者が「`provenance-graph` の出力をパースする Rust 型はどこ？」と探した時、v0.1.0 では `wasm-host` 側に類型があり、Round 1 時点では `crates/core/src/processor_outputs.rs` に予示型があったが、今は両方無い。「仕様で約束された出力構造をパースする型がリポジトリのどこにもない」という最悪の組み合わせになった
- 重大度: must-fix
- 修正案:
  - SPECS §3.2 の冒頭を Round 1 must-fix-001 の修正案どおり「v0.1.2 で稼働するのは `c2pa-verify` のみ。以下に挙げる processor は v0.2.x の予定」に書き換える。これで型を消した整合性が取れる
  - もしくは、`processor_outputs.rs` を「将来の processor 用予示型」として復活させ、`#[allow(dead_code)]` + crate-doc コメントで存続意図を明記する

### new-finding-001 OPERATIONS §8 トラブルシューティングが `docs/v0.1.0/tasks/12-e2e-local-dev/solana-build-notes.md` を「参考」リンクで指している

- 場所:
  - `docs/v0.1.2/OPERATIONS_JA.md:98`: `# 参考: docs/v0.1.0/tasks/12-e2e-local-dev/solana-build-notes.md`
  - `docs/v0.1.2/OPERATIONS_JA.md:413`: `参考: [docs/v0.1.0/tasks/12-e2e-local-dev/solana-build-notes.md](../v0.1.0/tasks/12-e2e-local-dev/solana-build-notes.md)`
- 観察: 17e で「legacy/v0.1.0 への参照をコード内コメントから削除（CHANGELOG にのみ残す）」というルールを採用したが、OPERATIONS_JA.md（運用文書 = OSS 利用者の主要な読み物）はこのルールから外れて v0.1.0 docs への参照が 2 箇所残っている
- 問題: 「v0.1.2 はゼロから書かれた」と COVERAGE.md:3 で宣言しつつ、運用者は v0.1.0 docs を読まないと anchor build の落とし穴を解決できない。OSS 公開時に v0.1.0 docs ディレクトリが付いてくることは確定なのか、別途扱いなのかも未定義
- 重大度: should-fix
- 修正案: v0.1.0 tasks への直リンクをやめ、`docs/v0.1.0/tasks/12-e2e-local-dev/solana-build-notes.md` の anchor build 解決法を `docs/v0.1.2/OPERATIONS_JA.md` 内部に転記する（数行で済む）。`legacy/v0.1.0/` を OSS 配布から外す方針なら必須

### new-finding-002 `crates/solana/tests/devnet_whitelist.rs` がテスト依存として `legacy/v0.1.0/keys/operator.json` を hardcode しており、17-audit-fixup の R-nitpick019「legacy/v0.1.0 へのテスト依存を除去」が未完了

- 場所:
  - `crates/solana/tests/devnet_whitelist.rs:9`: `//! - Authority key at legacy/v0.1.0/keys/authority.json with SOL balance`
  - `crates/solana/tests/devnet_whitelist.rs:260`: `"{}/legacy/v0.1.0/keys/operator.json"`
  - 17-audit-fixup README:132: `R-nitpick019 legacy/v0.1.0 へのテスト依存を除去` と記載されているが 17d 完了内訳の「先送り」リストには含まれず、対応した記述もない
- 観察: 17-audit-fixup README の R-nitpick019 は宣言だけで対応状況の記載がない（17d「完了内訳」に該当エントリなし、「先送り」リストにもなし）。実態として devnet テストは `legacy/v0.1.0/keys/` 内のキーペアに依存している
- 問題: legacy ディレクトリを OSS 公開時に削除すると devnet テストが壊れる。逆に保持すると COVERAGE.md:3 の「No carryover from v0.1.0/v0.1.1」宣言と矛盾するうえ、v0.1.2 リポジトリのテスト基盤が v0.1.0 のディレクトリ構造に縛られる
- 重大度: should-fix
- 修正案: テスト用キーペアを `crates/solana/tests/keys/` 以下に移動するか、CI で `KEYPAIR_PATH` env を要求する形に変更する

### new-finding-003 CHANGELOG の `[Unreleased]` 比較リンクの target が `compare/v0.1.0...HEAD` のまま固定で、v0.1.1 への言及が消滅

- 場所:
  - `CHANGELOG.md:57`: `[Unreleased]: https://github.com/yudai-mori-2004/title-protocol/compare/v0.1.0...HEAD`
  - `CHANGELOG.md` 全体: `[0.1.1]` セクション / リンクの追加なし
- 観察: 17f は CHANGELOG を見直したはずだが、`legacy/v0.1.1/` の存在（少なくとも CLAUDE.md 等で言及されている可能性のある中間バージョン）が CHANGELOG から省かれている。Round 1 でこの点は明示的に拾えていなかった
- 問題: ユーザーが Git tag を見て「v0.1.1 はリリースされていない」と誤解する可能性、または「v0.1.0 → v0.1.2 で 0.1.1 を経由していない」かどうかの説明責任が宙に浮く
- 重大度: nitpick
- 修正案: v0.1.1 がリリースされていない場合は CHANGELOG 冒頭に 1 行「v0.1.1 was an internal experimental phase and was not publicly released; v0.1.2 is the next public version.」と書く。リリース履歴がある場合は `[0.1.1]` セクションを追加する

### new-finding-004 17-audit-fixup README:272 が「17f ドキュメント+仕様 | done」と表示しているが、17f の完了内訳サブセクションが存在せず、S 観点の修正実態が追跡不能

- 場所:
  - `docs/v0.1.2/tasks/17-audit-fixup/README.md:272`: `| 17f ドキュメント+仕様 | done | 約 67 |`
  - `docs/v0.1.2/tasks/17-audit-fixup/README.md` は 500 行で終了。17a〜17e は「完了内訳」サブセクションがあるが、17f / 17g は無い
- 観察: 17f は S-mf001..003、S-sf001..007 を含む 67 件を対象としていた。done マークだけで内訳記録がないため、実際にどの S 指摘が修正されたのかが第三者から検証できない（実態として、本ラウンドの突合では「何も修正されていない」と判定した）
- 問題: 監査 → 修正 → 再監査のサイクルにおいて「done と書かれているが対応事実が確認できない」項目が大量に発生する。プロジェクト全体のガバナンス上の問題
- 重大度: nitpick（プロセス問題、コード品質には直接影響しない）
- 修正案: 17f / 17g のサブセクションを 17a〜17e と同じフォーマット（「完了内訳」+「先送り」+「検証」）で記述する。実際は「先送り」だけが大量にあるならそのとおりに記録する

## 全体所感

Round 1 で出した S 観点 14 件のうち、Round 2 で「修正された」と認められるものは 0 件。partially-fixed が 2 件（legacy processor: OPERATIONS ロードマップに記載 / 統合テスト: Gateway e2e と attestation-aws-nitro テストの追加で部分前進）、accepted-as-is が 1 件（コスト章: 「ビジネス文書」として正式に先送り宣言）、残り 11 件は unchanged。

特に問題が大きいのは:

1. **must-fix-001 / must-fix-002 / must-fix-003 が 1 件も解消されていない**: 17f の「done」マークと現物の乖離。17f は実質的に未着手だったと判断する
2. **regression-002（新規 must-fix）**: 17e の dead code 削除が SPECS §3.2 と整合性を取らずに先行した結果、「仕様で約束された出力をパースする型がリポジトリにない」という最悪の組み合わせが発生
3. **regression-001**: should-fix-007 を「実装コメント側を消す」方向で誤って解決した
4. **new-finding-002**: 17-audit-fixup README が宣言した R-nitpick019 が処理状況不明のまま放置

Round 1 で挙げた CHANGELOG Removed セクションの 12 項目欠落（must-fix-003）と TSA タイムスタンプの silent removal（must-fix-002）は引き続き ship 前に解決すべき。プロセス面では 17f / 17g に完了内訳のサブセクションを追加し、何が done で何が defer されたかの追跡可能性を回復する必要がある。

---

## 処理ログ

| ID | 判定 |
|---|---|
| regression-001 | wontfix(`COVERAGE.md` の "No carryover" 宣言と doc コメントの整合は legacy ディレクトリ削除のスコープと一括対応。本ラウンドの code-level 退行ではない) |
| regression-002 | wontfix(`processor_outputs.rs` 削除は B-2 で意図的な dead code 一掃。将来の image-pdq/video-vpdq 実装時に再追加) |
| new-finding-001 | wontfix(`OPERATIONS §8` の v0.1.0 task notes 参照は外部歴史リファレンスとして妥当。本リポジトリ内で `docs/v0.1.0/tasks/12-e2e-local-dev/solana-build-notes.md` が依然存在し動作する) |
| new-finding-002 | fixed (R ラウンドで `revoke_key_rejects_non_admin` から legacy operator.json 参照を除去) |
| new-finding-003/004 | wontfix(CHANGELOG の `[Unreleased]` 比較リンク・17-audit-fixup README の完了内訳サブセクション補完は OSS 公開前の doc メンテナンスフェーズで対応) |
