# H. OSS 成熟度

## 概要

「クローンした人が困らない」「フォークした人が貢献しやすい」「初見の人が概要を 5 分で掴める」を基準に、リポジトリ最上層のコミュニティ・サポート文書、初見導線、CI / リリース体制、各 crate の公開メタデータを精査した。

- 担当範囲: `README.md` / `CHANGELOG.md` / `LICENSE` / `CONTRIBUTING.md` / `SECURITY.md` / `CODE_OF_CONDUCT.md` / `docs/README.md` / `docs/v0.1.2/{SPECS_JA, COVERAGE, OPERATIONS_JA}` / 各 crate の `Cargo.toml` の `description` / `.gitignore` / `.dockerignore` / `docker-compose.yml` / `docker/smoke-test.sh` / `Anchor.toml` / `rust-toolchain.toml` / `deploy/aws/README.md` / `sp1-guests/README.md` / `programs/title-whitelist/Cargo.toml` / `docs/v0.1.2/audit/README.md`
- 監査方針: GitHub に上がっているリポジトリを初見の Rust + Solana エンジニアとして開いた想定で 1 文ずつ読む。チェックリストでなく「次の 5 分で詰まらないか」を判定基準にした。
- 件数サマリ: 20 件（must-fix 4 / should-fix 9 / nitpick 7）

## 重大度別内訳

- must-fix: 4 件
- should-fix: 9 件
- nitpick: 7 件

## 発見

### must-fix-001 README が「アーカイブ済み」と書く `legacy/v0.1.0/` がリポジトリに存在しない

- 場所: `README.md:130`, `.gitignore:34`
- 観察:
  - README:
    ```
    Previous implementation (v0.1.0) is archived in `legacy/v0.1.0/` for reference.
    ```
  - `CONTRIBUTING.md:33`:
    ```
    legacy/v0.1.0/         -- Archived v0.1.0 implementation (reference only)
    ```
  - 一方で `.gitignore` には:
    ```
    # Legacy code (local reference only, history preserved in git)
    legacy/
    ```
- 問題: クローンした人のディレクトリには `legacy/` は存在しない。README / CONTRIBUTING を読んで「v0.1.0 の実装はここにあるはず」と探すと存在しない。GitHub Web UI で見ても表示されない。`history preserved in git` というコメントも、`.gitignore` 直後に追加された場合は誤り（過去のコミットには残るが、`git ls-files` には出ない）。初見の人にとっては明らかな不整合。
- 修正案: 以下のいずれか:
  - (A) `legacy/` を git に含める方針なら `.gitignore` から `legacy/` を削除し、`legacy/v0.1.0/README.md` を 1 つ置いてコンテキストを残す
  - (B) `legacy/` を意図的にリポジトリから除外する方針なら README / CONTRIBUTING の `legacy/v0.1.0/` 記述を削除し、過去実装は `git log --all -- legacy/` で参照する、または別タグ (`v0.1.0`) を見るよう案内する
  - 推奨は (B)。`.gitignore` の `legacy/` コメントも「Excluded from repository — see tag v0.1.0 for the previous implementation.」に書き換える

### must-fix-002 GitHub Actions / CI が存在しない

- 場所: リポジトリ全体（`.github/workflows/` がない）
- 観察: `find` の結果、`.github` ディレクトリは `legacy/v0.1.0/.github` のみ。現行コードに対する CI 設定はゼロ。一方で CHANGELOG の v0.1.0 セクション (`CHANGELOG.md:54`) には「**CI/CD**: GitHub Actions (check, test, audit, WASM build, TypeScript build, npm publish)」と書かれている。
- 問題:
  - OSS として PR が来たときに「自動で test/check が回る」状態でないと、初見コントリビューターは「自分のローカルだけ通ればよいのか」がわからない
  - クローンした人にとって「main は green」の保証がない
  - `cargo test --workspace` の実行が CI で担保されていないため、ある PR で別の crate を壊しても気づけない
  - CHANGELOG が「v0.1.0 では CI があった」と謳っているのに現状ゼロなのは退行に見える
- 修正案: 最低限以下の `.github/workflows/ci.yml` を追加（`rust-toolchain.toml` の 1.93.1 が pin 済みなので fetch されれば再現性は確保される）:
  ```yaml
  name: ci
  on:
    push: { branches: [main] }
    pull_request:
  jobs:
    check:
      runs-on: ubuntu-latest
      steps:
        - uses: actions/checkout@v4
        - uses: dtolnay/rust-toolchain@stable
          with: { components: "rustfmt, clippy" }
        - uses: Swatinem/rust-cache@v2
        - run: cargo fmt --all -- --check
        - run: cargo clippy --workspace --all-targets -- -D warnings
        - run: cargo test --workspace --no-fail-fast
    audit:
      runs-on: ubuntu-latest
      steps:
        - uses: actions/checkout@v4
        - uses: rustsec/audit-check@v1
          with: { token: ${{ secrets.GITHUB_TOKEN }} }
  ```
- 補足: `sp1-guests/` は workspace から exclude されている (`Cargo.toml:12-16`) ので、`cargo test --workspace` だけで SP1 ビルドの 90 分プロセスは走らない。CI で安全に回せる。

### must-fix-003 `CONTRIBUTING.md` の Getting Started が「実装が存在しないこと」を前提に書かれていて、現状と食い違う

- 場所: `CONTRIBUTING.md:14-21`, `CONTRIBUTING.md:36`
- 観察:
  ```bash
  # Build and test (once implementation exists)
  cargo check --workspace
  cargo test --workspace
  ```
  および:
  ```
  Implementation code will be added as v0.1.2 tasks are completed. The crate structure is defined by the specification but not yet created.
  ```
- 問題: `crates/` 配下には 8 個の crate が実装済み（`title-core` / `title-tee` / `title-gateway` / `title-crypto` / `title-attestation` / `title-attestation-aws-nitro` / `title-proxy` / `title-solana`）、`docs/v0.1.2/COVERAGE.md` を見ても多くの項目が `[x]`。「(once implementation exists)」「crate structure is ... not yet created」は明確に古い情報。初見の人は「まだ何もないのか」と判断して離れる可能性がある。
- 修正案: 以下に置き換え:
  ```bash
  git clone https://github.com/yudai-mori-2004/title-protocol.git
  cd title-protocol

  # Run the local mock stack (TEE in mock runtime + Gateway).
  docker compose up --build
  ./docker/smoke-test.sh    # 5 endpoints, ~10s

  # Or, build and test directly:
  cargo test --workspace    # ~150 tests across 8 crates
  cargo clippy --workspace --all-targets -- -D warnings
  ```
  併せて `Implementation code will be added as v0.1.2 tasks are completed.` の段落を削除し、`docs/v0.1.2/COVERAGE.md` への参照に置き換える。

### must-fix-004 `programs/title-whitelist` crate が workspace から除外されているのに、SECURITY.md は "Solana Extension" を in-scope と宣言している

- 場所: `Cargo.toml:12-16`, `SECURITY.md:25`, `Anchor.toml`
- 観察: ルート `Cargo.toml` の `exclude = ["legacy", "programs", "sp1-guests"]` により、Solana プログラム本体 (`programs/title-whitelist/`) は `cargo build --workspace` / `cargo test --workspace` で一切ビルドされない。一方 `SECURITY.md:25` は `Solana Extension | On-chain integration | ZK proof bypass, whitelist manipulation, unauthorized minting` を **scope** に入れている。
- 問題: セキュリティ報告者が「Solana プログラムを試したい」と思って `cargo check --workspace` を打っても、対象ファイルが一切コンパイルされない。`anchor build --no-idl` が必要なことは `OPERATIONS_JA.md:97` にあるが、初見の人は CONTRIBUTING / SECURITY を先に読むのでそこに辿り着けない。
- 修正案: `CONTRIBUTING.md` に明示する:
  ```markdown
  ### Solana program build

  The Anchor program at `programs/title-whitelist/` is **not** part of
  the Cargo workspace (it has a conflicting toolchain). Build it via:

      cd programs/title-whitelist && anchor build --no-idl

  See `docs/v0.1.2/OPERATIONS_JA.md` §2.2 for the full deployment flow.
  ```
  併せて `SECURITY.md` の scope 表に「Solana program build instructions: see CONTRIBUTING.md」を一行加える。

### should-fix-005 README の `## Status` が「実装中」のままで現状と齟齬がある

- 場所: `README.md:124-130`
- 観察:
  ```
  ## Status

  **v0.1.2 — Implementation in progress.**

  The protocol has been redesigned from the ground up. See [Technical Specification (Japanese)](docs/v0.1.2/SPECS_JA.md) for the full design.
  ```
- 問題:
  - 「Implementation in progress」だけでは何が動いて何が動いていないかわからない。`docs/v0.1.2/COVERAGE.md` を見ると Gateway / TEE / crypto / attestation / solana ext がほぼ揃い、§3 の追加 processor 群 (provenance-graph / image-pdq / video-vpdq / cert-*) と一部の memory pattern が未実装、という具体的な状況がある
  - 「クローンした人が困らない」基準では「mock runtime と AWS Nitro 上の実機検証は終わっており、追加 processor が未実装」程度の粒度が欲しい
- 修正案:
  ```markdown
  ## Status

  **v0.1.2 — alpha, single-node deployments verified on AWS Nitro Enclaves.**

  - Core flow (Gateway + TEE + `c2pa-verify` + Attestation + Solana whitelist) works end-to-end on devnet.
  - Local mock stack: `docker compose up --build && ./docker/smoke-test.sh`.
  - Not yet implemented: `provenance-graph`, `image-pdq`, `video-vpdq`, `cert-google/sony/leica` processors; a TypeScript client SDK; mainnet deployment.

  See [`docs/v0.1.2/COVERAGE.md`](docs/v0.1.2/COVERAGE.md) for the full spec-to-implementation matrix.
  ```

### should-fix-006 README に "Quickstart" / "Try it locally" 節がない

- 場所: `README.md` 全体
- 観察: 現在の README は「What It Does → The Problem → How It Works → Architecture → Processors → Input Types → Encryption → Extension Layer → Trust Model → Design Principles → Status → Documentation → Contributing → Security → License」。動かす導線がどこにも書かれていない。
- 問題: 競合 OSS（同種の Rust + Solana プロジェクト）はほぼ全て README 上部に「30 秒で動かせる例」を載せている。Title Protocol は `docker compose up` で本当に 30 秒で立ち上がるのに、それが README から見えない。初見の人は「面白そうだが手を動かせない」と感じる可能性が高い。
- 修正案: `## How It Works` の直後（README:34 付近）に挿入:
  ```markdown
  ## Quickstart

  ```bash
  git clone https://github.com/yudai-mori-2004/title-protocol.git
  cd title-protocol
  docker compose up --build       # boots TEE (mock runtime) + Gateway
  ./docker/smoke-test.sh          # hits 5 endpoints, prints PASS/FAIL
  ```

  This runs the full request path against a mock TEE — useful for client
  development and for inspecting the API shape. For a real AWS Nitro
  deployment, see [`deploy/aws/README.md`](deploy/aws/README.md).
  ```

### should-fix-007 README にバッジが一つもない

- 場所: `README.md:1-5`
- 観察: 冒頭 5 行はタイトル + サブタイトル + 区切り線のみ。CI バッジ・license バッジ・version バッジ・rustc バッジが一切ない。
- 問題: GitHub の OSS リポジトリでバッジは「健康診断書」の役割を持つ。バッジが 0 個だと「メンテされているか」「テストが通っているか」が一見でわからず、第一印象でマイナス。
- 修正案: must-fix-002 の CI を追加した上で、タイトル直後に以下:
  ```markdown
  # Title Protocol

  [![CI](https://github.com/yudai-mori-2004/title-protocol/actions/workflows/ci.yml/badge.svg)](https://github.com/yudai-mori-2004/title-protocol/actions/workflows/ci.yml)
  [![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
  [![Rust: 1.93.1](https://img.shields.io/badge/rust-1.93.1-orange.svg)](rust-toolchain.toml)

  **Attribute Extraction Layer for C2PA-signed Content**
  ```
  Codecov 等は精度を犠牲にしてまで出す必要はないが、上記 3 つは無コストで掲示できる。

### should-fix-008 `CHANGELOG.md` の `[Unreleased] — v0.1.2` がリンク先を持たない / 比較リンクがない

- 場所: `CHANGELOG.md:7`, `CHANGELOG.md:37`
- 観察: Keep a Changelog 準拠を謳っている (`CHANGELOG.md:5`) が、末尾に `[Unreleased]: https://github.com/.../compare/v0.1.0...HEAD` のような比較リンクがない。`## [0.1.0] — 2026-03-02` にもタグへのリンクがない。
- 問題: Keep a Changelog の規約上、各セクション見出しはリンク化されることが期待される。なくても読めるが、「準拠を謳っているのに準拠していない」のは品質感に響く。
- 修正案: ファイル末尾に追加:
  ```markdown

  [Unreleased]: https://github.com/yudai-mori-2004/title-protocol/compare/v0.1.0...HEAD
  [0.1.0]: https://github.com/yudai-mori-2004/title-protocol/releases/tag/v0.1.0
  ```
  さらに「`Initial open-source release.`」(`CHANGELOG.md:39`) も v0.1.0 の `git tag` を切ってあるか確認し、無ければタグを打つ。

### should-fix-009 `SECURITY.md` の連絡先メールアドレスがプロジェクト owner と一致するか初見では判断できない

- 場所: `SECURITY.md:9`, `CODE_OF_CONDUCT.md:20`
- 観察: 両ファイルとも `contact@titleprotocol.org` を指している。一方 `Cargo.toml:23` の `repository = "https://github.com/yudai-mori-2004/title-protocol"` および `authors = ["Title Protocol Contributors"]` からは、`titleprotocol.org` が誰の所有か（GitHub アカウント `yudai-mori-2004` との同一性）が読めない。
- 問題: セキュリティ報告者は「このメールアドレスは本当に届くのか」「届いた先がリポジトリ運営者か」を確認できない。OSS において Security ポリシーで「届かないメールアドレス」を案内するのは大きな信頼問題になる。
- 修正案:
  - (A) `titleprotocol.org` が運営ドメインであることを README に明記する。例えば README の Status 節か Footer に「Maintained by ... — issues to GitHub, security reports to contact@titleprotocol.org」を加える
  - (B) もし `titleprotocol.org` がまだ取得されていない / DNS / MX が設定されていないなら、GitHub Security Advisories のみを残してメール案内を削る。「Alternative: Email ...」と書くからには動いていることが必須

### should-fix-010 OPERATIONS_JA.md §2.4 / §2.5 / §2.6 がプレースホルダーバイト列 (`[0xAA; 32]`, `[0xBB; 48]`) を運用前提にしている

- 場所: `docs/v0.1.2/OPERATIONS_JA.md:141`, `docs/v0.1.2/OPERATIONS_JA.md:178`
- 観察:
  - 「開発中はテスト用の placeholder（`[0xAA; 32]`）が登録されている。**本番ローンチ前に必ず本物の vkey_hash に差し替える**。」
  - 「開発中は placeholder（`[0xBB; 48]`）が登録されている。**本番ローンチ前に本物の PCR0 に差し替える**。」
- 問題:
  - 「本番ローンチ前に差し替える」だけでは、レビュー時のチェック手順が文書化されていない（誰がいつ確認するか、CI で検査できるか）
  - これを忘れた状態で誰かが mainnet にデプロイすると「placeholder PCR0 で登録された TEE 鍵」が whitelist に乗ってしまう
  - OSS としては「誰でも同じ手順で正しい状態にできる」がほしいので、検出手段を文書化したい
- 修正案: §2.4 末尾に追加:
  ```markdown
  > **Placeholder lockout check**: before merging to a release branch,
  > grep for the placeholder bytes and confirm none appear in
  > `crates/solana/tests/devnet_whitelist.rs` outside of `#[ignore]` fixtures:
  >
  >     grep -n '0xAA; 32\|0xBB; 48' crates/solana
  >
  > CI should fail if these patterns are found in non-test code.
  ```

### should-fix-011 `docs/README.md` のディレクトリ構造説明が古い（OPERATIONS_JA を欠落）

- 場所: `docs/README.md:11-26`
- 観察:
  ```
  ├── v0.1.2/                <- Full rewrite (current)
  │   ├── SPECS_JA.md
  │   ├── COVERAGE.md
  │   ├── OPERATIONS_JA.md   <- Deploy + lifecycle + troubleshooting guide
  │   └── tasks/
  ```
  これは OK。だが続く「Data Flow」節:
  ```
  SPECS (what to build) -> COVERAGE (what was built) -> tasks (how to build + learnings)
  ```
  に OPERATIONS が登場しない。さらに本文の三段説明 (`docs/README.md:35-37`) も OPERATIONS に触れない。
- 問題: 初見の人にとって OPERATIONS_JA がどこに位置するか（誰向け / いつ読むのか）がわからない。
- 修正案: 「Data Flow」節を以下に拡張:
  ```markdown
  SPECS (what to build) -> COVERAGE (what was built) -> OPERATIONS (how to run it) -> tasks (how to build + learnings)
  ```
  および:
  ```markdown
  - **OPERATIONS**: Deploy steps, lifecycle, troubleshooting. Read this when bringing a real TEE up; SPECS describes the design, OPERATIONS describes the procedures.
  ```

### should-fix-012 README に "Roadmap" / 当面の未実装項目への導線がない

- 場所: `README.md:124-130`
- 観察: ロードマップは `docs/v0.1.2/OPERATIONS_JA.md:443-449` にあるが、README からそこへのリンクがない。新規コントリビューターが「何をやれば貢献できるか」を見つけられない。
- 問題: 一般的に「Roadmap」「Good first issue」「Help wanted」セクションが OSS の貢献者獲得には決定的。
- 修正案: README に追加:
  ```markdown
  ## Roadmap

  See [`docs/v0.1.2/OPERATIONS_JA.md` §9](docs/v0.1.2/OPERATIONS_JA.md#9-ロードマップ) for current priorities. Headline items:

  - Additional processors: `provenance-graph`, `image-pdq`, `video-vpdq`, `cert-google/sony/leica`
  - TypeScript client SDK
  - Range Request streaming for large content fetch
  - mainnet contract deployment + multisig admin

  GitHub Issues labeled `good first issue` are welcoming places to start.
  ```

### should-fix-013 Issue / PR テンプレートが存在しない

- 場所: `.github/` 全体（不在）
- 観察: `.github/ISSUE_TEMPLATE/` / `.github/pull_request_template.md` がない。
- 問題: バグ報告 / 機能リクエストの粒度がコントロールできない。Security 案件が普通の Issue として投稿されるリスクもある（SECURITY.md は読まれない前提で設計する）。
- 修正案: 最低限 3 ファイル追加:
  - `.github/ISSUE_TEMPLATE/bug.yml` — runtime / repro / 環境 / 期待結果 / 実結果
  - `.github/ISSUE_TEMPLATE/feature.yml` — モチベーション / 想定インターフェース / 既存仕様との整合性
  - `.github/ISSUE_TEMPLATE/config.yml` で `blank_issues_enabled: false`、`contact_links` に security advisories 経由を明示
  - `.github/pull_request_template.md` — 関連 task / 関連 spec §番号 / テスト / breaking change チェック

### nitpick-014 ルート `README.md` と `docs/README.md` の冒頭言語ポリシーが揃っていない

- 場所: `README.md`, `docs/README.md:3`
- 観察: ルート README は完全英語。`docs/README.md:3` は `> Note: Technical specifications (`SPECS_JA.md`) are written in Japanese.` と注釈付き。一方ルート README には「日本語仕様書がある」が `README.md:128, 136` の `Japanese` 表記でしか伝わらず、なぜ日本語かの説明がない。
- 問題: 国際的なコントリビューター候補にとって「日本語の仕様 = 中心言語が日本語のプロジェクト」と誤解される可能性。
- 修正案: README の冒頭 (`## What It Does` の直前 or `## Documentation` 節) に一文:
  ```markdown
  > The technical specification is written in Japanese (`docs/v0.1.2/SPECS_JA.md`). All code, docstrings, and PR review are in English; the JA spec is the source-of-truth for protocol design only.
  ```

### nitpick-015 `Cargo.toml` workspace の `authors = ["Title Protocol Contributors"]` がメンテナの実体を示さない

- 場所: `Cargo.toml:24`
- 観察: GitHub URL は `yudai-mori-2004` 個人アカウント、`SECURITY.md` の連絡先は `contact@titleprotocol.org`。`Title Protocol Contributors` という総称は実体が薄い。
- 問題: crates.io に公開するときに「メンテナ不明 crate」と見える。ライセンス上は問題ないが信頼感に影響。
- 修正案: 以下のいずれか:
  - `authors = ["Yudai Mori <contact@titleprotocol.org>"]`
  - `authors = ["Title Protocol Contributors <contact@titleprotocol.org>"]` （メールが届く前提で）
  - 個人名を出さない方針なら `authors = ["The Title Protocol Authors"]` のように Linux カーネル風に表現

### nitpick-016 ルート最上層に高レベル `ARCHITECTURE.md` / 図がない

- 場所: リポジトリ最上層
- 観察: アーキテクチャ図は `README.md:36-53`, `README.md:63`, `deploy/aws/README.md:14-33`, `docs/v0.1.2/OPERATIONS_JA.md:14-30` に散在。初見で「全体図を 1 枚で見たい」とき、どこを見ればいいか即答できない。
- 問題: 競合 OSS の多くは `ARCHITECTURE.md` を 1 つ置いて crate 境界 / データフロー / 信頼境界を一覧する。Title Protocol は概念が多い（TEE / Attestation / Gateway / Proxy / SP1 / Solana whitelist / cNFT）ので、なおさら欲しい。
- 修正案: `docs/v0.1.2/` 配下に `ARCHITECTURE.md` を新設し、`README.md` の `## Architecture` から短いリンクのみ残す:
  - 信頼境界（vsock / HTTPS / 暗号境界）
  - crate 依存グラフ
  - リクエストパスの sequence diagram
  - Solana 側コンポーネント図（`title-whitelist` PDA 群 + SP1 verifier 配置）
  もしくは README 内に短い `## Architecture at a Glance` 節を作って既存散在図への目次にしてもよい。

### nitpick-017 `docker-compose.yml` がカバーするのは mock のみで、実 Nitro 経路の図がない

- 場所: `docker-compose.yml`, `docker/`
- 観察: `tee-mock.Dockerfile` のみ workspace 標準として置いてあり、production 経路（`vendor-aws` build / `title-proxy` / EIF）は `deploy/aws/` 配下にしかない。
- 問題: README/`docker-compose.yml` に来た人は「これが Title Protocol を動かす唯一の方法」と誤解しがち。「これは mock であり、実 TEE は別経路」と冒頭に書きたい。
- 修正案: `docker-compose.yml` 冒頭コメントに 1 行追加:
  ```yaml
  # NOTE: TEE here runs in *mock* mode for client / API-shape development.
  # For a real AWS Nitro Enclave deployment, see deploy/aws/README.md.
  ```
  および `docker/tee-mock.Dockerfile` のファイル名で mock であることは伝わるが、`docker/` 配下に `README.md` を置いて「mock vs Nitro 経路の差分」を一文書けると親切。

### nitpick-018 `Anchor.toml` の `cluster = "Devnet"` がリポジトリのデフォルトとして固定されている

- 場所: `Anchor.toml:16-17`
- 観察:
  ```toml
  [provider]
  cluster = "Devnet"
  wallet = "~/.config/solana/id.json"
  ```
- 問題: 初見の人が `anchor test` などを叩くと意図せず devnet に接続する可能性。`Localnet` が安全側のデフォルトで、必要なら明示的に切り替える方が安全。
- 修正案:
  ```toml
  [provider]
  cluster = "Localnet"
  wallet = "~/.config/solana/id.json"
  # Devnet / mainnet selection is documented in
  # docs/v0.1.2/OPERATIONS_JA.md §2.2.
  ```

### nitpick-019 `audit/README.md` のステータス表が静的で運用しにくい

- 場所: `docs/v0.1.2/audit/README.md:53-67`
- 観察: 「エージェント完了時にこの表を更新する」とあるが、10 エージェントすべて pending のまま。完了時の自動化手段がない。
- 問題: 監査自体の運用がスケールしない（OSS 観点では nitpick）。
- 修正案: 監査終了タスク（17）で `done` に更新する手順を `audit/README.md` 末尾に明記、または完了時に commit message のテンプレ (`audit(h): complete`) を決めるだけでよい。

### nitpick-020 `programs/title-whitelist/keypair.json` がローカルに残置している

- 場所: `programs/title-whitelist/keypair.json`
- 観察: `.gitignore:31` で `keypair.json` を除外しているのでリポジトリには上がっていないが、ローカルクローン時に「これは何か」が初見ではわからない。
- 問題: クローンしてビルドした人にとって「鍵ペアが必要 → 自分で生成するのか、リポジトリにあるべきものなのか」が曖昧。
- 修正案: `programs/title-whitelist/README.md` を 1 つ追加し、`anchor keys list` や `solana-keygen new -o programs/title-whitelist/keypair.json` でのローカル生成方法を 5 行で書く。Anchor の慣習だが OSS としては不親切。

## 全体所感

主要なサポート文書（README / CHANGELOG / LICENSE / CONTRIBUTING / SECURITY / CODE_OF_CONDUCT / SPDX ヘッダ 58/58 / 各 crate `description`）は揃っており、平均的な Rust OSS と比べてもむしろ整っている。特に SPDX 100% カバレッジ、`rust-toolchain.toml` の pinning コメント、`deploy/aws/README.md` の詳しさは賞賛できる。

一方で「初見が 5 分で詰まる」要素は明確に 4 つある:
1. `legacy/` が消えている（README が嘘をついている）
2. CI がない（PR を出す側も受ける側も保証が薄い）
3. CONTRIBUTING が「実装はまだ存在しない」と書いてある（明らかに古い）
4. Solana プログラムが `cargo build --workspace` に含まれない説明がない

この 4 つを潰すだけで、リポジトリの第一印象は大きく改善する。バッジ / Quickstart / Issue Template は次の階層（OSS としての「丁寧さ」）の話で、必須ではないが「フォークされやすさ」に効く。

監査者として、Title Protocol は「設計と実装の質は高いが、入口の整備がいま一歩」という印象を受けた。must-fix 4 件は数日で対応可能で、それで OSS としての見栄えは劇的に変わる。
