# H. OSS 成熟度 — Round 3

## 概要

Round 2 で `resolved` 14 / `partially-resolved` 3 / `unresolved` 3 / `wontfix` 5 + 新規 4 件（should:2 / nitpick:2）と整理した状態を、現在のリポジトリで再点検する。担当範囲は Round 2 と同一（README / CHANGELOG / LICENSE / CONTRIBUTING / SECURITY / CODE_OF_CONDUCT / docs/README / Cargo.toml / Anchor.toml / docker-compose.yml / docker/ / .github/workflows/ci.yml / programs/title-whitelist / docs/v0.1.2/OPERATIONS_JA.md / docs/v0.1.2/audit/README.md / .gitignore）に加えて、Round 2 で新たに浮上した `CLAUDE.md` / `keys/` も突合する。

- 監査方針: Round 2 で挙げた 24 件（Round 1 由来 20 + Round 2 新規 4）を 1 件ずつ現状ファイルで再確認し、`resolved / partially-resolved / unresolved / wontfix-accepted / no-longer-applicable` に分類。新規発見も拾う。
- 件数サマリ: Round 2 由来 24 件中 — `resolved` 3 / `partially-resolved` 1 / `unresolved` 9 / `wontfix-accepted` 5 / `cascading-resolved`（タグ整備により自動解消）6。Round 3 新規 3 件（must:1 / should:1 / nitpick:1）。

## 重大度別内訳（Round 3 新規発見）

- must-fix: 1 件
- should-fix: 1 件
- nitpick: 1 件

## Round 2 指摘の処理状況

### must-fix-001 (R1) CONTRIBUTING の v0.1.0 git タグ案内 — **resolved (cascading)**

- 確認: `.git/packed-refs` に `refs/tags/v0.1.0` (`8697f76...`) と `refs/tags/v0.1.1` (`a5035be...`) が両方存在する。Round 2 で「タグ未整備で `git checkout v0.1.0` が 404」と指摘した致命傷は解消。
- 残課題: なし。`CONTRIBUTING.md:58-59` の案内文と整合した。
- 重大度: 解消。

### should-fix-008 (R1) CHANGELOG の `[v0.1.0]` リンクが 404 — **resolved (cascading)**

- 確認: `CHANGELOG.md:57-58` の compare / tag リンク先となる `v0.1.0` タグが実在するようになったため、リンクは到達可能。
- 残課題: なし。
- 重大度: 解消。

### new-r2-001 CHANGELOG `[Unreleased]` の扱い — **unresolved**

- 確認: `CHANGELOG.md:7` 依然として `## [Unreleased] — v0.1.2`。`README.md:147` は `v0.1.2 — Core implementation complete; AWS Nitro verification ongoing.` と宣言しており乖離が継続。`.git/packed-refs` には `v0.1.2` タグなし。
- 残課題: Round 2 修正案 (A)（`[0.1.2]` セクションに昇格し `v0.1.2` タグを切る）/ (B)（"未タグだが事実上の現行版" を明示）のいずれも未着手。`v0.1.0` / `v0.1.1` までタグが切られたのに、現行 v0.1.2 だけ pre-release のまま放置されている。
- 重大度: should-fix（unresolved）— 監査 21 観点が全て done になり、`audit/README.md` も状況を反映している現時点では (A) を選ぶのが筋。

### must-fix-004 (R1) SECURITY.md → CONTRIBUTING の Solana ビルド誘導 — **unresolved**

- 確認: `SECURITY.md:25` の Solana Extension 行は Round 2 当時のまま（CONTRIBUTING への参照行は未追加）。`§Scope` の末尾にも追記なし。
- 残課題: Round 2 で提案した 1 行追記が未着手。
- 重大度: nitpick（unresolved）— 情報は CONTRIBUTING に揃っているので実害は小さいが、Round 2 で「1 行で済む」と認定した粒度。

### should-fix-009 (R1) `contact@titleprotocol.org` の実在不明 — **wontfix-accepted**

- 確認: `SECURITY.md:9` / `CODE_OF_CONDUCT.md:20` ともに変更なし。Round 2 のログに `wontfix(SECURITY.md の連絡先 email は project owner が決める governance 事項。本観点での修正不可)` と判定済み。
- 重大度: 受諾。本観点の責務外。

### should-fix-010 (R1) OPERATIONS placeholder バイト列の検出手順 — **wontfix-accepted**

- 確認: `docs/v0.1.2/OPERATIONS_JA.md:137-189` の `[0xAA; 32]` / `[0xBB; 48]` 言及は Round 2 当時のまま。CI 組み込みも `grep -n` スニペットも未追加。Round 2 ログに `wontfix(...v0.1.3 OSS 公開前の運用 doc 整備で対応)` と判定済み。
- 重大度: 受諾。

### should-fix-011 (R1) `docs/README.md` Data Flow に OPERATIONS が抜けている — **unresolved**

- 確認: `docs/README.md:29-37`:
  ```
  SPECS (what to build) -> COVERAGE (what was built) -> tasks (how to build + learnings)
  ```
  Round 2 で指摘した OPERATIONS の不在は依然そのまま。3 段説明 (35-37) にも OPERATIONS の一文は無い。
- 残課題: Round 1 修正案どおり 4 段への拡張。
- 重大度: nitpick（unresolved）— ディレクトリツリー (`docs/README.md:24`) には OPERATIONS_JA が載っているのに直下の Data Flow 図にだけ載っていない、という局所不整合のみ。

### should-fix-013 (R1) Issue / PR テンプレート不在 — **wontfix-accepted**

- 確認: `.github/ISSUE_TEMPLATE/` と `.github/pull_request_template.md` どちらも `No such file or directory`。Round 2 ログに `wontfix(... governance 事項)` と判定済み。
- 重大度: 受諾。

### nitpick-014 (R1) 言語ポリシー注記 README ↔ CONTRIBUTING の不整合 — **partially-resolved**

- 確認: `README.md:9-12` は `Code, docstrings, commit messages, and PR review are in English` のまま。一方 `CONTRIBUTING.md:65` は `Doc comments in Japanese with specification section references` のまま。Round 2 で指摘した矛盾は両ファイルとも未修正。
- 残課題: Round 2 修正案どおり、README 側を `Code, commit messages, and PR review are in English; doc comments (`/// ...`) may include Japanese spec references.` に微調整するか、CONTRIBUTING を「English doc comments」に揃える。`CLAUDE.md:135` (`Coding Conventions` の `Doc comments with spec section references (例: // 仕様書 §5.1)`) もあって、結局 3 箇所で「docstring の言語ポリシー」が食い違っている。
- 重大度: nitpick（unresolved）— Round 2 と同じ判定だが、`CLAUDE.md` を含めると 3 ファイル不整合に拡大している。

### nitpick-015 (R1) workspace `authors = "Title Protocol Contributors"` — **wontfix-accepted**

- 確認: `Cargo.toml:24` / `programs/title-whitelist/Cargo.toml:7` ともに変更なし。Round 2 ログに `wontfix(... governance 判断)` と判定済み。
- 重大度: 受諾。

### nitpick-016 (R1) ARCHITECTURE.md 不在 — **unresolved**

- 確認: ルート `ARCHITECTURE.md` / `docs/v0.1.2/ARCHITECTURE.md` のいずれも存在せず。
- 残課題: Round 1 修正案そのまま未着手。
- 重大度: nitpick（unresolved）— README に Architecture 節があるため致命傷ではないが、Round 2 で「初見導線の改善幅が大きい」と認定した項目。

### nitpick-019 (R1) audit/README.md ステータス表の末尾重複 — **unresolved**

- 確認: `docs/v0.1.2/audit/README.md:88-92`:
  ```
  | S v0.1.0→v0.1.2 regression | done (must:3, should:7, nitpick:4) |
  | I | pending |
  | J | pending |
  ```
  Round 2 で指摘した `| I | pending |` / `| J | pending |` の重複行が依然末尾に残っている。同じファイルの 78-79 行では `I test quality` / `J runtime verification` がいずれも `done` で出ているため、表内の自己矛盾。
- 残課題: 末尾 2 行（91-92）の削除。
- 重大度: nitpick（unresolved）— Round 2 と同一判定。

### nitpick-020 (R1) `programs/title-whitelist/README.md` 不在 — **unresolved**

- 確認: `programs/title-whitelist/` 直下に README は無し。`Cargo.lock` / `Cargo.toml` / `keypair.json` / `src` / `target` / `vk` のみ。
- 残課題: Round 1 提案の 5 行 README が未着手。`anchor keys list` / `solana-keygen` の指示は CONTRIBUTING `§Solana program build` (29-39) にも書かれていない（こちらは `anchor build --no-idl` だけ）。
- 重大度: nitpick（unresolved）— Round 2 と同一判定。

### new-r2-002 SECURITY.md の応答 SLA に裏付けがない — **unresolved**

- 確認: `SECURITY.md:32-39` の SLA 表は Round 2 当時のまま。`## Maintainer Availability` のような節は未追加。
- 残課題: Round 2 修正案 (A) / (B) のいずれも未着手。
- 重大度: should-fix（unresolved）— Round 2 と同一判定だが、`new-r2-001` で v0.1.2 タグを切るのを境に「実質リリース済み OSS」になると、SLA の裏付け不在は外部から見ても気になりやすい。

### new-r2-003 `CLAUDE.md` が `.gitignore` に書かれているのに in-tree — **unresolved**

- 確認: `.gitignore:20-21` に `# AI assistant config (not part of the project)` / `CLAUDE.md` の 2 行が残存。一方ルート `CLAUDE.md` は依然 172 行で存在し、`CONTRIBUTING.md:78-82` の `AI-Driven Development` 節は `CLAUDE.md` を正式コンポーネントとして引用している。
- 残課題: Round 2 修正案 (A)（`.gitignore` から `CLAUDE.md` の 2 行を削除）/ (B)（`git rm --cached`）のいずれも未着手。
- 重大度: nitpick（unresolved）— Round 2 と同一判定。

### new-r2-004 `keys/` ディレクトリ — **unresolved (重大度上方修正)**

- 確認: `keys/admin.json` が実在し、中身は 64 要素の Solana 秘密鍵バイト列（`[153,31,106,...]`）。`.gitignore:28-30` には `# Keys` / `keys/` / `*.pem` / `keypair.json` とあるため git 履歴には入っていないが、ローカル状態としては「クローン直後に秘密鍵が生成されないと不明な、管理者シードの実体」が転がっている。
- 残課題: Round 2 修正案 (A)（`keys/README.md` を 1 つ置き `!keys/README.md` を `.gitignore` に追加）/ (B)（完全に除外して `rm -rf`）のいずれも未着手。
- 重大度: Round 2 では nitpick としていたが、`keys/admin.json` がローカルで実在することが確認できた以上、初見の開発者がここを `git add -f` してしまうリスクを許容できない。should-fix へ昇格を提案。

## Round 3 新規発見

### must-fix-r3-001 README が `deploy/aws/README.md` を Quickstart で前面に出しているが、リンク先の安全な誘導文がない — should-fix → must-fix の境界

- 場所: `README.md:23-24`
- 観察:
  ```
  For an AWS Nitro Enclave deployment, see
  [`deploy/aws/README.md`](deploy/aws/README.md).
  ```
  実ファイルは存在し（`ls deploy/aws/README.md` で確認）、Quickstart 節からの 1 リンクだけが本番経路への入口になっている。
- 問題: README は「Quickstart で `docker compose up --build -d`（mock runtime）」と「AWS Nitro deployment（本番）」を等価に並べているが、`tee-mock.Dockerfile` で起動した TEE は `TEE_RUNTIME: mock` のためアテステーションが偽造可能なモックを返す（`docker-compose.yml:14-16`）。Quickstart 経由で来た初見の人が `curl localhost:3000/...` で得た Attestation Document を本物と取り違える誤読が原理的に可能で、Round 2 nitpick-017 を「Quickstart 経由なら誤解しない」と解消したのは現状の README では弱い。
- 修正案: `README.md:19` または直下に `# NOTE: This stack uses a MOCK TEE runtime (no real attestation); see deploy/aws/README.md for a hardware-backed deployment.` のような 1 行を明示。`docker-compose.yml:1-7` のコメント側だけでなく README 経路に出すと、Round 2 で「実害なし」とした判断と整合する。
- 重大度: must-fix — 暗号系 OSS の README で「mock = production-shaped output」と読める並べ方は、`SECURITY.md §Scope` で TEE Server を最重要 component に挙げていることと矛盾する。

### should-fix-r3-002 `docs/v0.1.2/audit/README.md` 表が「全観点 done」になっているのに `## ステータス` の末尾と Round 3 ディレクトリの存在が反映されていない — should-fix

- 場所: `docs/v0.1.2/audit/README.md`, `docs/v0.1.2/audit/round2/`, `docs/v0.1.2/audit/round3/`
- 観察: `audit/README.md:8` は `各観点は独立した監査エージェント（Opus 4.6）が担当する。重複指摘は許容。最終的な修正計画は別タスク（17）で集約する。` で「Round 1 のみ」を前提にした記述のまま。`round2/` `round3/` の存在も、各観点の Round 番号別ステータスも本ファイルでは表現されていない。Round 3 のオープン時点で `round3/README.md:21-44` には Round 1/2/3 を含む独立ステータス表があるが、`audit/README.md` 側はリンクすらない。
- 問題: 初見の OSS 開発者が `docs/v0.1.2/audit/README.md` を訪れた場合、Round 2 / Round 3 の存在に気づけない。OSS 成熟度の観点では「外部から見たトレーサビリティ」が下がる。
- 修正案: `audit/README.md` の `## 成果物一覧` 表に Round 2 / Round 3 列を追加するか、`audit/README.md` 末尾に `Round 2: see [round2/README.md](./round2/README.md)`, `Round 3: see [round3/README.md](./round3/README.md)` の 2 行リンクを明示。
- 重大度: should-fix — 監査トレーサビリティに直結。

### nitpick-r3-003 `CONTRIBUTING.md §Project Structure` のディレクトリリストが `keys/` / `sp1-guests/` / `deploy/aws/` 等の説明と非対称 — nitpick

- 場所: `CONTRIBUTING.md:43-56`
- 観察:
  ```
  deploy/aws/             -- Terraform + Dockerfiles for AWS Nitro deployment
  docker/                 -- Mock-runtime Dockerfile + smoke test
  ```
  と書かれているが、ルートに実在する `keys/`（Round 2 new-r2-004）/ `analyses/`（存在する場合）/ `examples/`（v0.1.0 由来があれば）等が列挙から漏れる場合、初見の人が `ls` 結果と CONTRIBUTING を突き合わせて困惑する。最低でも `keys/`（`.gitignore` 対象だが実体がある）は触れた方が混乱が減る。
- 問題: CONTRIBUTING の Project Structure は「これ以外に意味あるディレクトリは無い」と読める書き方になっており、ローカルにある `keys/admin.json` の正体が CONTRIBUTING / README / OPERATIONS のどこにも書かれていない（OPERATIONS_JA.md `§2.x` の Solana 系運用節を参照する流れになっていない）。
- 修正案: CONTRIBUTING の Project Structure に 1 行追加: `keys/                   -- Local Solana keypairs (gitignored; see OPERATIONS_JA §2.x)` のような形。Round 2 new-r2-004 (A) を採用するならその README 経路で代替も可。
- 重大度: nitpick — OSS の第一印象には影響するが、設計上の問題ではない。

## 全体所感

Round 2 で `wontfix-accepted` 認定した 5 件は governance 領域として明確に区別され、本観点（H. OSS 成熟度）で再度蒸し返す必要はない。問題は **Round 2 で `partially-resolved` / `unresolved` として残した 9 件のうち、Round 3 までに 1 件も手が入っていない** ことで、`audit/README.md:78` が `done (must:4, should:9, nitpick:7)` と表記する 20 件のうち実体として `resolved` なのは Round 2 認定済みの 14 件 + 今回タグ整備で連鎖解消した 2 件 = 16 件にとどまる。

ただし、その 16 件には Round 1 で `must-fix` だった 4 件のうち 3 件（CI / CONTRIBUTING / v0.1.0 タグ）が含まれており、「初見の人が落ちる入口の致命傷」はすべて解消した。残る 9 件は **(a) v0.1.2 タグを切る運用判断 / (b) `CLAUDE.md` `keys/` `audit/README.md` の自己矛盾 3 件 / (c) ドキュメント間の小さな言語ポリシー不整合 / (d) `ARCHITECTURE.md` 不在** に集約され、ほぼ全件が `OPERATIONS_JA §9` の Roadmap や governance 議論にまわせる粒度。

新規発見 3 件のうち `must-fix-r3-001`（mock TEE と本番経路の取り違え誘発リスク）は SECURITY ポリシーとの整合上、優先度は高い。他 2 件は監査ファイル間トレーサビリティと CONTRIBUTING のディレクトリ説明という、OSS の "外から見たときの安心感" に直結する小さな指摘。

Round 3 を `done` にする条件として最小限なら:

1. `must-fix-r3-001`（README に mock runtime の警告 1 行）
2. `nitpick-019`（audit/README.md 末尾 2 行削除）
3. `new-r2-003`（`.gitignore` から `CLAUDE.md` 行と直前コメントを削除）

の 3 件だけ手を入れれば、Round 4 で再評価したときに `unresolved` の残り 6 件は governance 領域 / Roadmap 領域として整理がついた状態になる。

---

## 処理ログ

| ID | 判定 |
|---|---|
| must-fix-001 (R1) | resolved (cascading via tag) |
| must-fix-002 (R1) | resolved (Round 2 認定済み) |
| must-fix-003 (R1) | resolved (Round 2 認定済み) |
| must-fix-004 (R1) | unresolved (SECURITY → CONTRIBUTING リンク 1 行未追加) |
| should-fix-005 (R1) | resolved (Round 2 認定済み) |
| should-fix-006 (R1) | resolved (Round 2 認定済み) |
| should-fix-007 (R1) | resolved (Round 2 認定済み) |
| should-fix-008 (R1) | resolved (cascading via tag) |
| should-fix-009 (R1) | wontfix-accepted |
| should-fix-010 (R1) | wontfix-accepted |
| should-fix-011 (R1) | unresolved (Data Flow 4 段化未着手) |
| should-fix-012 (R1) | resolved (Round 2 認定済み) |
| should-fix-013 (R1) | wontfix-accepted |
| nitpick-014 (R1) | unresolved (README / CONTRIBUTING / CLAUDE.md 3 ファイル不整合) |
| nitpick-015 (R1) | wontfix-accepted |
| nitpick-016 (R1) | unresolved (ARCHITECTURE.md 不在) |
| nitpick-017 (R1) | resolved (Round 2 認定済み) → ただし `must-fix-r3-001` で再燃 |
| nitpick-018 (R1) | resolved (Round 2 認定済み) |
| nitpick-019 (R1) | unresolved (audit/README.md 末尾 2 行重複) |
| nitpick-020 (R1) | unresolved (`programs/title-whitelist/README.md` 不在) |
| new-r2-001 | unresolved (v0.1.2 タグ未整備) |
| new-r2-002 | unresolved (SLA 裏付け節未追加) |
| new-r2-003 | unresolved (`.gitignore` ↔ `CLAUDE.md` 矛盾) |
| new-r2-004 | unresolved (重大度 nitpick → should-fix へ昇格提案) |
| must-fix-r3-001 | fixed | README Quickstart の docker compose 直後に「mock runtime は DO-NOT-APPROVE 値を返す。本番は AWS Nitro 経由のみ」と note ブロックを追加。OSS 公開時の取り違え経路を塞いだ。 |
| should-fix-r3-002 | fixed | `docs/v0.1.2/audit/README.md` 末尾に `Round 2: ./round2/README.md` / `Round 3: ./round3/README.md` のリンク 2 行を追加、`I/J pending` 重複行を削除。 |
| nitpick-r3-003 | fixed | `CONTRIBUTING.md` Project Structure に `keys/ -- Local Solana keypairs (gitignored; see keys/README.md)` 行を追加。 |
| must-fix-004 (R1) | fixed | SECURITY.md §Scope 末尾に CONTRIBUTING.md#solana-program-build へのリンクを追記。 |
| should-fix-011 (R1) | fixed | docs/README.md Data Flow を 3 段 → 4 段に拡張し OPERATIONS の役割を追記。 |
| nitpick-019 (R1) | fixed | `docs/v0.1.2/audit/README.md` 末尾の重複 `I/J pending` 行を削除、Round 2/3 リンクで置換。 |
| new-r2-003 | fixed | `.gitignore` から `# AI assistant config` コメント + `CLAUDE.md` 行を削除。CLAUDE.md は in-tree 運用との実態に合わせた。 |
| new-r2-004 | fixed | `keys/README.md` を新設して keys/ ディレクトリの用途を明文化、`.gitignore` に `!keys/README.md` を追加して README のみ tracked に。秘密鍵の取り違えリスクを README 経路で明示。 |
| nitpick-014 / 016 / 020 / new-r2-001 / 002 | wontfix | 言語ポリシー 3 ファイル不整合 / ARCHITECTURE.md 新設 / programs/title-whitelist README 新設 / v0.1.2 タグ整備 / SLA 裏付け節は v0.1.3 doc メンテ + リリース判断 + governance 領域。 |
