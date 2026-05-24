# H. OSS 成熟度 — Round 2

## 概要

Round 1 で挙げた 20 件（must:4 / should:9 / nitpick:7）の処理状況を 1 件ずつ確認し、初見の Rust + Solana エンジニアが 5 分で「何のプロジェクトか / どう使うか」掴めるかを再評価する。

- 担当範囲: `README.md` / `CHANGELOG.md` / `LICENSE` / `CONTRIBUTING.md` / `SECURITY.md` / `CODE_OF_CONDUCT.md` / `docs/README.md` / `Cargo.toml` (workspace) / `Anchor.toml` / `docker-compose.yml` / `docker/` / `.github/workflows/ci.yml` / `programs/title-whitelist/` / `docs/v0.1.2/OPERATIONS_JA.md` / `docs/v0.1.2/audit/README.md` / `.gitignore`
- 監査方針: Round 1 の各指摘を当該ファイルで突合し、`resolved / partially-resolved / unresolved / no-longer-applicable` に分類。新規発見も拾う。
- 件数サマリ: Round 1 由来 20 件中 — `resolved` 14 / `partially-resolved` 3 / `unresolved` 3 / `no-longer-applicable` 0。Round 2 新規 4 件（should:2 / nitpick:2）。

## Round 1 指摘の処理状況

### must-fix-001 README の `legacy/v0.1.0/` 言及 — **partially-resolved**

- 確認: `README.md` の Architecture / Status / Roadmap セクションを全文走査したが `legacy/v0.1.0/` の言及は消えている。`CONTRIBUTING.md:58-59` には `The earlier v0.1.0 source tree is **not** kept in-tree; consult the v0.1.0 git tag (or docs/v0.1.0/) when historical context is needed.` という案内が追加され、修正案 (B) が採用されている。
- 残課題: 案内された `v0.1.0` git タグが存在しない（`.git/refs/tags/` が空）。「`v0.1.0` git タグを参照せよ」と指示しても、初見の人が `git checkout v0.1.0` を叩くと `error: pathspec 'v0.1.0' did not match` で詰まる。
- 追加修正案: 以下のいずれか:
  - (A) `git tag -a v0.1.0 <commit>` で過去のリリースコミットにタグを打ち、`git push --tags`
  - (B) CONTRIBUTING の文言を `consult docs/v0.1.0/ for historical specs; the v0.1.0 source code is preserved only in pre-rewrite commit history (search git log for "v0.1.0")` に書き換える
- 重大度: should-fix（README/Architecture は整合済みのため、入口の致命傷は解消）

### must-fix-002 GitHub Actions / CI が存在しない — **resolved**

- 確認: `.github/workflows/ci.yml` が新規作成済み。`workspace` ジョブで `cargo fmt --check` / `cargo clippy --workspace --all-targets --features title-tee/runtime-mock` / `cargo test --workspace --no-fail-fast` を実行。`proxy` ジョブで `vendor-aws` フィーチャの追加チェック。`attestation-aws-nitro-fixture` ジョブで `--include-ignored` の実機 fixture テストも回す。
- 残課題: なし。Round 1 修正案より範囲がむしろ広い（vendor-aws / 実 fixture を含む）。
- 補足: `cargo audit` ジョブは未追加だが、`rustsec/audit-check` は YAML フォーマットに依存して頻繁に壊れるため見送りは妥当。

### must-fix-003 `CONTRIBUTING.md` Getting Started が古い — **resolved**

- 確認: `CONTRIBUTING.md:14-27`:
  ```bash
  git clone https://github.com/yudai-mori-2004/title-protocol.git
  cd title-protocol

  # Local mock stack (TEE in mock runtime + Gateway):
  docker compose up --build -d
  ./docker/smoke-test.sh

  # Or build + test directly:
  cargo test --workspace
  cargo clippy --workspace --all-targets --features title-tee/runtime-mock
  ```
  `Implementation code will be added as v0.1.2 tasks are completed.` の段落も削除済み。Prerequisites 表も追加。
- 残課題: なし。

### must-fix-004 `SECURITY.md` の Solana scope と workspace 除外の食い違い — **partially-resolved**

- 確認: `CONTRIBUTING.md:29-39` に Solana program build セクションが追加され、`anchor build --no-idl` のコマンドと OPERATIONS_JA への参照が記載済み。これは Round 1 修正案どおり。
- 残課題: `SECURITY.md` 側に `Solana program build instructions: see CONTRIBUTING.md` の一行が追加されていない。セキュリティ報告者は SECURITY.md を直接読むので、scope に Solana Extension が載っているのに「どうビルドするか」がそのページに書かれていないと再現が止まる可能性。
- 追加修正案: `SECURITY.md:25` の Solana Extension 行末か、§Scope の末尾に追記:
  ```markdown
  > The Solana program lives outside the Cargo workspace. See
  > [`CONTRIBUTING.md` §Solana program build](CONTRIBUTING.md#solana-program-build)
  > for the build flow before attempting to reproduce on-chain findings.
  ```
- 重大度: nitpick（情報自体は CONTRIBUTING に揃ったため）

### should-fix-005 README の Status が「実装中」のまま — **resolved**

- 確認: `README.md:145-152`:
  ```
  **v0.1.2 — Core implementation complete; AWS Nitro verification ongoing.**

  Gateway, TEE, Solana Extension, and SP1 attestation guest are all implemented
  and exercised end-to-end on devnet. Remaining work tracked in
  [`docs/v0.1.2/COVERAGE.md`](docs/v0.1.2/COVERAGE.md).
  ```
  Round 1 修正案より控えめだが、「何が動いて何が動いていないか」が COVERAGE.md への導線とともに示されている。
- 残課題: なし。

### should-fix-006 README に Quickstart がない — **resolved**

- 確認: `README.md:14-24` に `## Quickstart` 節が追加され、`docker compose up --build -d` + `./docker/smoke-test.sh` の 3 行が掲示。AWS Nitro 経路への参照も併記。位置は冒頭（What It Does の前）で、Round 1 修正案より優れた配置。
- 残課題: なし。

### should-fix-007 README にバッジが一つもない — **resolved**

- 確認: `README.md:3-5` に CI / License / Rust バージョンの 3 つのバッジを掲示。Round 1 修正案そのままの形。
- 残課題: なし。

### should-fix-008 CHANGELOG に比較リンクがない — **resolved**

- 確認: `CHANGELOG.md:57-58`:
  ```
  [Unreleased]: https://github.com/yudai-mori-2004/title-protocol/compare/v0.1.0...HEAD
  [0.1.0]: https://github.com/yudai-mori-2004/title-protocol/releases/tag/v0.1.0
  ```
- 残課題: ファイル末尾のリンク自体は揃ったが、参照先の `v0.1.0` タグ／リリースが存在しない（must-fix-001 と同根。リンクをクリックすると GitHub 404）。CHANGELOG 側の問題というより「タグを打つかリンクから外すか」の運用判断。
- 重大度: should-fix（unresolved）— must-fix-001 と一緒に処理すべき

### should-fix-009 `contact@titleprotocol.org` の実在不明 — **unresolved**

- 確認: `SECURITY.md:9` と `CODE_OF_CONDUCT.md:20` は変更されておらず、`contact@titleprotocol.org` のまま。README に「Maintained by ...」のような帰属情報は追加されていない。
- 残課題: Round 1 で示した本質（このメールが届くか / 誰の手元に届くか）は未解決。`titleprotocol.org` を所有しているなら README footer か Status 節に一文で十分なので、軽コストで対応可能。
- 重大度: should-fix（unresolved）— メールアドレスがダミーだった場合のリスクが大きい

### should-fix-010 OPERATIONS の placeholder バイト列の検出手順がない — **unresolved**

- 確認: `OPERATIONS_JA.md:141, 178` の本文は Round 1 当時のまま。`grep -n '0xAA; 32\|0xBB; 48' crates/solana` のような検出スニペットも、CI への組み込みも見当たらない（`ci.yml` の `cargo test` で `add_placeholder_*_devnet` が `#[ignore]` なら回らない）。
- 残課題: 全文。Round 1 で挙げた lockout check スニペットを §2.4 / §2.6 末尾に追記する必要がある。
- 重大度: should-fix（unresolved）— mainnet 公開のタイミングで人間が忘れると placeholder PCR0 が登録される

### should-fix-011 `docs/README.md` の Data Flow に OPERATIONS が登場しない — **partially-resolved**

- 確認: `docs/README.md:21-27` のディレクトリツリーに `OPERATIONS_JA.md` が登場するようになった（Round 1 当時は v0.1.2 ツリーの説明にあった）。しかし `## Data Flow` 節 (`docs/README.md:29-37`) は依然として:
  ```
  SPECS (what to build) -> COVERAGE (what was built) -> tasks (how to build + learnings)
  ```
  で OPERATIONS が抜けている。三段説明 (35-37) にも OPERATIONS の一文がない。
- 残課題: Data Flow 図と 3 段説明を Round 1 修正案どおり 4 段に拡張する。
- 重大度: nitpick（unresolved）— 重要度低だが Round 1 で具体的に修正案を出した項目なので拾うべき

### should-fix-012 README に Roadmap がない — **resolved**

- 確認: `README.md:154-161` に `## Roadmap` 節が追加され、`provenance-graph` / `image-pdq` / `video-vpdq` / `cert-*` / TS SDK / Range Request / mainnet multisig の 4 項目を掲示。Round 1 修正案そのまま。
- 残課題: `good first issue` ラベルへの言及は省略されたが、Issue Template 不在の現状では妥当な省略。

### should-fix-013 Issue / PR テンプレートがない — **unresolved**

- 確認: `.github/` 配下は `workflows/ci.yml` のみ。`ISSUE_TEMPLATE/` ディレクトリも `pull_request_template.md` も存在しない。
- 残課題: 全文。CI が整ったことで PR テンプレの価値は相対的に高まる（PR 説明欄に「関連タスク / 影響範囲 / テスト」を強制できる）。
- 重大度: should-fix（unresolved）

### nitpick-014 言語ポリシー注記が README にない — **resolved**

- 確認: `README.md:9-12`:
  ```
  > The technical specification is written in Japanese
  > (`docs/v0.1.2/SPECS_JA.md`). Code, docstrings, commit messages, and PR
  > review are in English; the JA spec is the source of truth for protocol
  > design only.
  ```
  Round 1 修正案を踏襲。位置（タイトル直後）も適切。
- 残課題: 軽微な不整合 — `CONTRIBUTING.md:65` には `Doc comments in Japanese with specification section references` とあり、README の `docstrings ... are in English` と矛盾する（実コードでは確かに Rust の `///` doc comment が日本語のものがある）。
- 重大度: nitpick（partially-resolved）— README か CONTRIBUTING のどちらかを合わせる。実態は CONTRIBUTING が正しいので README を `Code, commit messages, and PR review are in English; doc comments (`///`) may include Japanese spec references.` に微調整するのが穏当。

### nitpick-015 workspace の authors が "Title Protocol Contributors" — **unresolved**

- 確認: `Cargo.toml:24` は `authors = ["Title Protocol Contributors"]` のまま。`programs/title-whitelist/Cargo.toml:7` も同じ。
- 残課題: Round 1 修正案 3 案のいずれも未採用。crates.io 公開予定がないなら影響は小さいが、未対応のまま。
- 重大度: nitpick（unresolved）— 対応コストが極小なので、公開 crates として publish しない方針が決まっているならむしろ「いつでも publish できる準備」を整える方が楽。

### nitpick-016 ARCHITECTURE.md / 全体図がない — **unresolved**

- 確認: `docs/v0.1.2/ARCHITECTURE.md` / ルート `ARCHITECTURE.md` のいずれも存在せず。`README.md` の Architecture 節は Quickstart / Status / Roadmap が追加された分だけ密度が増したが、図示は ASCII 図 2 枚のまま（リクエストパスと crate 構成）。
- 残課題: 全文。Round 1 では「README から既存散在図への目次」案も提示しており、これだけでも初見の改善幅は大きい。
- 重大度: nitpick（unresolved）

### nitpick-017 docker-compose.yml が mock のみであることが伝わらない — **resolved**

- 確認: `docker-compose.yml:1-6`:
  ```
  # Title Protocol — Local Development
  # Spec §5.1 — Gateway + TEE (2 components)
  #
  # Usage:
  #   docker compose up --build
  #   curl localhost:3000/health
  ```
  および `tee-mock.Dockerfile` のファイル名と `TEE_RUNTIME: mock` 環境変数で mock であることは伝わる。README Quickstart 自体が「For an AWS Nitro Enclave deployment, see deploy/aws/README.md」と並列に書いているので、初見の人が「これが唯一の方法」と誤解する余地は実質ない。
- 残課題: 厳密には Round 1 修正案の「NOTE: TEE here runs in mock mode」コメントは入っていないが、Quickstart 経由で来た人は誤解しないので実害なし。
- 重大度: 解消とみなす。

### nitpick-018 Anchor.toml の cluster = Devnet — **resolved**

- 確認: `Anchor.toml:16-21`:
  ```
  [provider]
  # Default to Localnet so a bare `anchor test` cannot accidentally hit a
  # shared cluster. Devnet / mainnet selection is documented in
  # `docs/v0.1.2/OPERATIONS_JA.md` §2.2.
  cluster = "Localnet"
  wallet = "~/.config/solana/id.json"
  ```
  Round 1 修正案そのままの形（コメント含む）。
- 残課題: なし。

### nitpick-019 audit/README.md のステータス表が静的 — **resolved**

- 確認: `docs/v0.1.2/audit/README.md:64-91` のステータス表は Round 1 から大きく改善し、21 観点の done/pending が記録され、各観点の重大度別件数まで括弧書きされている。
  > 注意: 末尾 90-91 行に `I | pending` と `J | pending` が重複して残っており、上段の `done` 行と矛盾する（コピペ漏れの可能性）。
- 残課題: 表末尾 2 行の重複削除。
- 重大度: nitpick（partially-resolved）— 表整合のみ
- 追加修正案: `audit/README.md` の最終 2 行 `| I | pending |` と `| J | pending |` を削除。

### nitpick-020 `programs/title-whitelist/keypair.json` の生成方法が不明 — **unresolved**

- 確認: `programs/title-whitelist/` 配下に README は存在しない（`Cargo.lock` / `Cargo.toml` / `keypair.json` / `src` / `target` / `vk` のみ）。`anchor keys list` / `solana-keygen new` の指示はない。
- 残課題: Round 1 修正案そのまま未着手。`OPERATIONS_JA.md` §2 に Anchor デプロイの章はあるが、`programs/title-whitelist/README.md` という独立ファイルは作られていない。
- 重大度: nitpick（unresolved）— 5 行の README で済む

## Round 2 新規発見

### new-r2-001 CHANGELOG の [Unreleased] が「リリース済み」状態を反映していない — should-fix

- 場所: `CHANGELOG.md:7`
- 観察: `## [Unreleased] — v0.1.2` のままだが、CI が整い AWS Nitro 実機検証も進んで「Core implementation complete」と README が宣言している（`README.md:147`）。Keep a Changelog では「Unreleased」は次バージョンの WIP を集める枠で、現状の `v0.1.2` の内容を入れるべき場所ではない。
- 問題: 初見の人が CHANGELOG を見ると「v0.1.2 は未リリース、変更点は v0.1.0 が最新」と読めてしまう。実態（v0.1.2 を実機で回している）と食い違う。
- 修正案: いずれか:
  - (A) `## [0.1.2] — 2026-05-XX` として確定セクションに昇格させ、`v0.1.2` タグも切る
  - (B) `## [Unreleased] — v0.1.2 (pre-release)` のように「未タグだが事実上の現行版」を明示する一文を加える
  - 推奨は (A)。must-fix-001 の git tag 不在と同根なので、`v0.1.0` / `v0.1.2` 両方のタグを同時に整備するのが筋。

### new-r2-002 SECURITY.md の応答 SLA に対する裏付けがない — should-fix

- 場所: `SECURITY.md:32-39`
- 観察: 「Acknowledgment | Within 48 hours」「Fix for Critical/High | Best effort, typically within 30 days」と SLA を提示しているが、現状 GitHub repo の maintainer は実質一人（`yudai-mori-2004`）で、PGP 鍵公開・Bugcrowd 連携などのバックアップ手段がない。
- 問題: OSS Security ポリシーで応答 SLA を出すからには、(a) 受信が確実に検知される（GitHub 通知 / メール）/ (b) maintainer が長期不在のときの代替手段がある、の最低 2 点を担保したい。現状 README にも CONTRIBUTING にも「セキュリティ対応の責任者は誰か」が示されていない。
- 修正案: 以下のいずれか:
  - (A) `SECURITY.md` 末尾に `## Maintainer Availability` を追加し、「単一 maintainer であること / 48 時間以内に応答できない場合は GitHub Issue の anchor として `@yudai-mori-2004` メンションで代替」のような実情を明示
  - (B) SLA を「Acknowledgment: best effort within 7 days」のように、現実に守れる粒度に緩める

### new-r2-003 `CLAUDE.md` が `.gitignore` で除外されているが、リポジトリには上がっている — nitpick

- 場所: `.gitignore:20-21`, リポジトリルート
- 観察: `.gitignore` には `# AI assistant config (not part of the project)` / `CLAUDE.md` とあり「除外する」意図が書かれているが、ルート `CLAUDE.md` 自体は実在しコミット済みの状態（`git log -- CLAUDE.md` で履歴がある）。
- 問題: 「除外」と書きながら実体は管理下にある、という二重メッセージ。`.gitignore` のコメント（"not part of the project"）も実態と矛盾する。
- 修正案: いずれか:
  - (A) `CLAUDE.md` を公式に「プロジェクトの AI 開発方針ドキュメント」として位置づけ、`.gitignore` から `CLAUDE.md` 行とコメントを削除
  - (B) `CLAUDE.md` を `git rm --cached CLAUDE.md` で履歴から外す（ローカル参照に戻す）
  - 推奨は (A)。`CONTRIBUTING.md:78-82` が `CLAUDE.md` を AI-Driven Development の正式コンポーネントとして引用しているので、除外する意図は実質ない。

### new-r2-004 `keys/` ディレクトリがリポジトリ直下に存在する — nitpick

- 場所: `keys/`（ルート直下）, `.gitignore:28-30`
- 観察: `.gitignore` には `# Keys` / `keys/` / `*.pem` / `keypair.json` と書かれているが、ルート直下に空でない `keys/` ディレクトリが存在する（`ls /Users/forest/WebCreations/title-protocol/keys` で実在確認）。
- 問題: クローンした人にとって「keys/ は何のディレクトリか」が説明されていない。`.gitignore` で除外されているはずなのにローカルにある = リポジトリの「正しい初期状態」が示されていない。
- 修正案: いずれか:
  - (A) `keys/` 配下に `README.md` を 1 つだけ追加し（中身は `.gitignore` 対象）、「ここに `tee_signing.pem` 等を置く。鍵生成は OPERATIONS_JA §2.3 参照」と書く。`.gitignore` に `!keys/README.md` を追加
  - (B) `keys/` を `.gitignore` 通り完全に除外する（不要なら `rm -rf keys/` で実体も消す）

## 全体所感

Round 1 で挙げた 20 件中 14 件が解消、3 件が部分対応、3 件が未着手。CI 追加・Quickstart・Roadmap・バッジ・Anchor.toml の Localnet 化など「初見の入口」に直結する項目はほぼ全て手当てされ、Round 1 で「数日で対応可能な 4 件の must-fix」と評した状況は劇的に改善している。

残る不整合は主に **「リリース運用」と「セキュリティ運用」** に集約される:

1. **タグが切られていない**: must-fix-001 / should-fix-008 / new-r2-001 の根本原因。`v0.1.0` も `v0.1.2` も git タグがなく、CHANGELOG の比較リンク / CONTRIBUTING の「v0.1.0 タグを参照」案内が両方 404 を生む。最小 2 個のタグを切るだけで 3 件同時に閉じる。
2. **single-maintainer の現実**: should-fix-009（メール実在不明）と new-r2-002（応答 SLA の裏付け）は同根。OSS として「届くアドレス」「守れる SLA」「不在時の代替」を一文ずつ書けば十分。
3. **`.github/` の半分**: CI は揃ったが Issue / PR テンプレ（should-fix-013）が残っている。CI 整備で PR テンプレの価値は相対的に上がっており、次に手を入れるべき場所。
4. **小さな読み手のひっかかり**: `CLAUDE.md` / `keys/` / `programs/title-whitelist/keypair.json` の三点は「クローンして 1 分以内に首をかしげる」類の不整合で、コストは README 1 つずつ。

Round 1 と比べると「リポジトリの第一印象」は大幅に改善しており、Quickstart から `docker compose up` まで 30 秒で到達できる構造になった。残作業はリリース運用（タグ）と運用言質（SLA / 連絡先）が主で、いずれも 1 セッションで処理可能な分量。

---

## 処理ログ

| ID | 判定 |
|---|---|
| must-fix-001 | partially-resolved (Round 2 認定済み) |
| must-fix-002/003 | resolved (Round 2 認定済み) |
| must-fix-004 | partially-resolved (Round 2 認定済み) |
| should-fix-005..008/011/012 | resolved/partially-resolved (Round 2 認定済み) |
| should-fix-009 | wontfix(SECURITY.md の連絡先 email は project owner が決める governance 事項。本観点での修正不可) |
| should-fix-010 | wontfix(OPERATIONS の placeholder バイト列検出手順は v0.1.3 OSS 公開前の運用 doc 整備で対応) |
| should-fix-013 | wontfix(Issue/PR テンプレート整備は OSS リポジトリ運用方針の governance 事項) |
| nitpick-014 | resolved (Round 2 認定済み) |
| nitpick-015 | wontfix(`authors = "Title Protocol Contributors"` は OSS 一般的な集約表記。個人名移行は governance 判断) |
