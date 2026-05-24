# E. 再現性・ビルド品質 — Round 3

## 概要

担当範囲（Round 1 / Round 2 と同一）: `Cargo.toml` / `Cargo.lock` / `rust-toolchain.toml` / 全 `crates/*/Cargo.toml` / `sp1-guests/**/Cargo.toml` / `programs/title-whitelist/Cargo.toml` / `docker/**/*.Dockerfile` / `docker-compose.yml` / `.dockerignore` / `deploy/aws/**/*.tf` / `deploy/aws/**/*.sh` / `Anchor.toml` / ルート `.gitignore`。

Round 2 で挙げた既存 23 件 + 新規 7 件 = 計 30 件の処理状況と、Round 3 で発見した regression / 新規問題を整理する。
判定基盤: Round 2 末尾の「処理ログ」が `fixed` または `wontfix(...)` と宣言した項目について、コードを実際に読んで宣言通りかを検証。

## Round 2 既存指摘の処理状況

### 既存 must-fix（6 件）

| ID | Round 2 status | Round 3 検証 |
|---|---|---|
| must-fix-001 (proxy `default = ["vendor-aws"]`) | fixed | **fixed-confirmed** |
| must-fix-002 (sha2_sp1 branch 指定) | fixed (round2 ログ) | **fixed-confirmed** |
| must-fix-003 (Dockerfile が proxy を COPY しない) | fixed (round2 ログ) | **partially-fixed** ※`|| true` が title-proxy.Dockerfile に残置 |
| must-fix-004 (AMI most_recent) | wontfix → 実際は fixed | **fixed-confirmed**（処理ログと実装の乖離。良い方向の乖離） |
| must-fix-005 (`.terraform.lock.hcl` gitignore) | fixed | **fixed-confirmed** |
| must-fix-006 (tfstate 共有運用ガイド) | partially-fixed | **unchanged** ※OPERATIONS_JA / deploy/aws/README に backend ガイドの追記なし |

### 既存 should-fix（10 件）

| ID | Round 2 status | Round 3 検証 |
|---|---|---|
| should-fix-001 (SP1 Cargo.lock) | fixed | **fixed-confirmed** |
| should-fix-002 (SP1 / Anchor の rust-toolchain.toml) | wontfix | **unchanged** |
| should-fix-003 (`p256` 指定の不揃い) | wontfix | **unchanged** |
| should-fix-004 (base image digest pin) | wontfix | **unchanged** |
| should-fix-005 (Dockerfile `|| true`) | fixed (round2 ログ) | **partially-fixed** ※title-proxy.Dockerfile:38 に残置 |
| should-fix-006 (`dnf update -y`) | wontfix | **unchanged** |
| should-fix-007 (`Anchor.toml [scripts] test`) | wontfix | **unchanged** |
| should-fix-008 (CI/CD パイプライン) | partially-fixed | **partially-fixed**（変更なし） |
| should-fix-009 (proxy が workspace dep を活用しない) | partially-fixed | **partially-fixed**（変更なし） |
| should-fix-010 (`[profile.release]` reproducibility 指定) | wontfix | **unchanged** |

### 既存 nitpick（7 件）

| ID | Round 2 status | Round 3 検証 |
|---|---|---|
| nitpick-001 (`.dockerignore`) | wontfix | **unchanged** |
| nitpick-002 (workspace member 並び) | wontfix | **unchanged** |
| nitpick-003 (Dockerfile §5.4 コメント) | wontfix | **unchanged** |
| nitpick-004 (title-whitelist の package メタ) | wontfix | **unchanged** |
| nitpick-005 (smoke-test.sh が compose を呼ばない) | wontfix | **unchanged** |
| nitpick-006 (run-stack.sh の TEE 起動待ち) | wontfix → 実際は fixed | **fixed-confirmed**（処理ログ漏れ。良い方向の乖離） |
| nitpick-007 (Anchor wallet パス) | wontfix | **unchanged** |

### Round 2 新規発見（new-）7 件

| ID | Round 2 status | Round 3 検証 |
|---|---|---|
| new-must-fix-001 (CI `--locked` 抜け) | wontfix | **unchanged** |
| new-should-fix-001 (clippy `-D warnings` 抜け) | wontfix | **unchanged** |
| new-should-fix-002 (CI が Anchor/SP1/Docker 未カバー) | wontfix | **unchanged** |
| new-should-fix-003 (`RUSTFLAGS=-D warnings` を全 job に被せる) | wontfix | **unchanged** |
| new-nitpick-001 (`legacy/` 二重宣言) | wontfix | **unchanged** |
| new-nitpick-002 (`CLAUDE.md` gitignore) | wontfix | **unchanged** |
| new-nitpick-003 (proxy `default = []` の mock 用途コメント) | wontfix | **unchanged** |
| new-nitpick-004 (`title-whitelist` の `[profile.release]`) | wontfix | **unchanged** |

集計: 30 件中、新たに完全解決を確認できたのは **6 件**（must-fix-001 / must-fix-002 / must-fix-004 / must-fix-005 / should-fix-001 / nitpick-006）。残りは Round 2 から変化なし、または部分修正のまま。Round 2 → Round 3 の修正活動は実質「コードに反映されていないが処理ログだけ進んだ」項目（後述 r3-regression-001）と「処理ログには出ないが静かに修正された」項目（must-fix-004 / nitpick-006）の双方が混在しており、**処理ログを source of truth として扱えない**点が一番の構造的問題。

### 個別詳細（Round 3 で観察事実が変わったもののみ）

#### must-fix-002 (fixed-confirmed)

- 場所: `crates/attestation-aws-nitro/Cargo.toml:33`
- 観察:
  ```toml
  # Pin to commit hash, not branch — branch tip drifts on every upstream
  # push and would silently change the SP1 vkey if cargo refreshed it.
  sha2_sp1 = { git = "https://github.com/sp1-patches/RustCrypto-hashes", rev = "1f224388fdede7cef649bce0d63876d1a9e3f515", package = "sha2", optional = true }
  ```
  `Cargo.lock:5338` も `source = "git+https://github.com/sp1-patches/RustCrypto-hashes?rev=1f224388fdede7cef649bce0d63876d1a9e3f515#1f224388fdede7cef649bce0d63876d1a9e3f515"` で固定。コメントが「なぜ rev 化したか」を明示している点も良い。
- 評価: 完了。SP1 vkey の silent drift リスクは塞がれた。

#### must-fix-003 (partially-fixed)

- 場所: `docker/gateway.Dockerfile`, `docker/tee-mock.Dockerfile`, `deploy/aws/docker/tee-nitro.Dockerfile`, `deploy/aws/docker/title-proxy.Dockerfile`
- 観察:
  - 4 Dockerfile すべて 8 つの workspace member の Cargo.toml + stub source を COPY するように揃った。`gateway.Dockerfile:11-30`, `tee-mock.Dockerfile:11-30`, `tee-nitro.Dockerfile:17-36`, `title-proxy.Dockerfile:18-34` で完全に同形。
  - `gateway.Dockerfile:32`, `tee-mock.Dockerfile:32`, `tee-nitro.Dockerfile:39-41` の dep-cache 用 cargo build は `|| true` が **削除済み**。
  - **しかし `deploy/aws/docker/title-proxy.Dockerfile:38` だけ `|| true` が残置**:
    ```dockerfile
    RUN cargo build --release --bin title-proxy --features vendor-aws 2>&1 || true
    ```
    Round 2 の処理ログは「`|| true` を 3 Dockerfile から削除」と宣言していたが、4 つあるうちの 3 つを正しく直し、最後の 1 つを見落とした形。
- 残課題: title-proxy.Dockerfile の `|| true` を外す。stub source を COPY しているので外しても fail-fast でビルドが通るはず（他の 3 Dockerfile が既に証明している）。

#### must-fix-004 (fixed-confirmed)

- 場所: `deploy/aws/terraform/main.tf:43-47, 118`
- 観察:
  ```hcl
  variable "al2023_ami_id" {
    description = "Pinned Amazon Linux 2023 AMI id for ap-northeast-1. Bump consciously and re-register the resulting PCR0."
    type        = string
    default     = "ami-0c8698b371227f828"
  }
  ...
  resource "aws_instance" "node" {
    ami = var.al2023_ami_id
  ```
  `data "aws_ami" "al2023"` の宣言は削除され、AMI ID をハードコードした variable のみ参照。コメントに「PCR0 baseline currently registered on Solana devnet」「pick a new AMI consciously, ... re-register the new PCR0 via the SP1 prove flow」と運用フローも明記。
- 評価: Round 2 処理ログでは「wontfix(CI 整備フェーズで対応)」と書かれていたが、実装はすでに pin 化されている。**処理ログのリグレッション**（後述 r3-regression-001）。

#### nitpick-006 (fixed-confirmed)

- 場所: `deploy/aws/scripts/run-stack.sh:87-101`
- 観察:
  ```bash
  TEE_READY=0
  for i in {1..60}; do
    if curl -sf http://127.0.0.1:4000/health > /dev/null 2>&1; then
      echo "    TEE ready (${i}s)"
      TEE_READY=1
      break
    fi
    sleep 1
  done
  if [[ "$TEE_READY" != "1" ]]; then
    echo "ERROR: TEE /health did not respond within 60s; aborting before gateway start." >&2
    echo "  Check 'sudo nitro-cli console --enclave-id <id>' and $REMOTE_DIR/socat.log." >&2
    exit 1
  fi
  ```
- 評価: 完了。TEE が 60s 以内に来ない場合 Gateway 起動前に `exit 1` する。Round 2 処理ログでは「wontfix(... 整理)」扱いだったが、実装は既に整っている。

## Round 3 新規発見

### r3-regression-001 Round 2 処理ログとコードの乖離が複数箇所

- 場所: `docs/v0.1.2/audit/round2/e-reproducibility.md:528-541`
- 観察:
  - Round 2 処理ログは must-fix-004（AMI 固定）と nitpick-006（TEE ready チェック）を `wontfix` と記載するが、実装ではどちらも fix 完了。
  - 逆に must-fix-003 / should-fix-005 を `fixed` と宣言するが、`deploy/aws/docker/title-proxy.Dockerfile:38` に `|| true` が残置。
  - should-fix-008 を `partially-fixed` と書きつつ「CI/workspace dep は 17g で部分対応済み」と未来形タスク参照（読み手には意味不明）。
- 問題: 「処理ログ = 修正主の self-report」として機能していない。Round 4 以降の監査者が処理ログを信じて検証を省略すると、`|| true` 残置のような半端な状態が静かに通過する。
- 修正案: Round 3 の処理ログは「コードを実際に読んで判定する」運用に固定する（このファイルの末尾でその通りに記載）。Round 2 ログの誤記載は別途修正コミットを 1 本立てるか、Round 3 / 4 の総括で訂正する。

### r3-must-fix-001 `title-proxy.Dockerfile` の `|| true` 残置

- 場所: `deploy/aws/docker/title-proxy.Dockerfile:38`
- 観察:
  ```dockerfile
  RUN cargo build --release --bin title-proxy --features vendor-aws 2>&1 || true
  ```
  他 3 Dockerfile（gateway / tee-mock / tee-nitro）は同じ位置の `|| true` を削除済み。title-proxy だけ残っている。
- 問題: must-fix-003 で「proxy の `Cargo.toml` を COPY していない」ことを `|| true` が握り潰した経緯を踏まえると、最も `|| true` を外すべき Dockerfile が title-proxy 自身（被害者）であり同時に加害者（残置）。この 1 行が残っているために、proxy の dep-cache 段が壊れても CI で気付けない。
- 修正案:
  ```dockerfile
  RUN cargo build --release --bin title-proxy --features vendor-aws
  ```

### r3-should-fix-001 `Cargo.toml` の `exclude = ["legacy", "programs", "sp1-guests"]` と `.gitignore` の `legacy/` ignore がずれた挙動を生む

- 場所: ルート `Cargo.toml:12-16`, ルート `.gitignore:33-34`
- 観察:
  - `Cargo.toml:12-16` で `exclude = ["legacy", "programs", "sp1-guests"]`（cargo の workspace から除外）。
  - `.gitignore:33-34` で `legacy/` を ignore（コメントは「local reference only, history preserved in git」）。
  - `programs/` と `sp1-guests/` は `.gitignore` の対象ではないので git tracked。`legacy/` だけ「Cargo は無視するし、Git も新規ファイルは無視する」非対称。
- 問題: 3 ディレクトリすべてが「workspace 外、しかし repo tracked」のはずなのに、`legacy/` だけ「workspace 外、`.gitignore` で新規ファイルが入らない」設定。Round 2 new-nitpick-001 で半分指摘していた件だが、根本は `exclude` と `.gitignore` の整合性。
- 修正案: 
  - 意図が「legacy には新規ファイルを追加させない」なら `.gitignore` の `legacy/` を残し、コメントに「new files under `legacy/` are intentionally hidden from git; bump existing files directly if you must」と明記。
  - 意図が「legacy も普通に track する」なら `.gitignore:34` を削除し、`Cargo.toml` の `exclude` だけで対応。

### r3-should-fix-002 SP1 host crate の `Cargo.toml` が独自 workspace なのに `version` をハードコード

- 場所: `sp1-guests/attestation-aws-nitro/host/Cargo.toml:4`
- 観察:
  ```toml
  [package]
  name = "title-sp1-attestation-aws-nitro-host"
  version = "0.1.2"
  edition = "2021"
  license = "Apache-2.0"
  ...
  [workspace]
  ```
  ルート `Cargo.toml [workspace.package].version = "0.1.2"` と一致しているが、別管理。`[workspace]` を独自に切っているのでルート workspace の `version.workspace = true` は使えない。
- 問題: バージョン bump 時にここを直し忘れると、SP1 host の version が遅れる。同 program 側 (`sp1-guests/attestation-aws-nitro/program/Cargo.toml`) と `programs/title-whitelist/Cargo.toml` も同じ理由でハードコード。3 箇所のバージョンを手で揃える運用は脆い。
- 修正案: 
  - 短期: `scripts/check-versions.sh` を追加し、4 箇所（ルート workspace.package, SP1 host, SP1 program, title-whitelist）の version を一致させる check を CI に追加。
  - 中期: SP1 host も `[workspace.package]` の継承を使う構造を検討（ただし独自 workspace を切る制約があるので、いまの形のままで version check で吸収するのが現実的）。

### r3-nitpick-001 Round 2 new-nitpick-002 の判断が論争的

- 場所: `.gitignore:20-21`
- 観察:
  ```
  # AI assistant config (not part of the project)
  CLAUDE.md
  ```
  Round 2 新規発見 new-nitpick-002 では「OSS が CLAUDE.md を tracked にする方針が増えている」と書いた。しかしリポジトリ owner の方針として「個人ごとに違う CLAUDE.md を書きたい」もありうる。
- 問題: 単独の nitpick として残すよりも、OSS 公開時点で「rules-as-code を repo 公開するか個人運用するか」の方針判断と一括で扱う方が筋が良い。
- 修正案: 観点 H（OSS maturity）と統合判断を仰ぐ。E 観点としては「コードベース contributor が CLAUDE.md を読まないと protocol 規約に従えない」状況になる前にコミット運用へ寄せる方が再現性的に safe、という意見を表明するに留める。

### r3-nitpick-002 `docker-compose.yml` に `Cargo.lock` の volume mount が無く CI 検証と本番 build が同期しない

- 場所: `docker-compose.yml:9-22`, `docker/tee-mock.Dockerfile:10`
- 観察: compose は `dockerfile: docker/tee-mock.Dockerfile` をビルド context `.` で呼ぶ。Dockerfile 側は `COPY Cargo.lock` で固定するので一見問題ないが、`.dockerignore` が `Cargo.lock` を除外していないことが暗黙の前提（実際 `.dockerignore` は `target/ / legacy/ / programs/ / docs/ / .git/` のみ）。
- 問題: 仕様 §5.4 「依存ライブラリのバージョン固定（Cargo.lock）」を docker build 経路で破る経路として `.dockerignore` の編集が一発で済む点が脆い。
- 修正案: `.dockerignore` の冒頭にコメント
  ```
  # Cargo.lock and rust-toolchain.toml are NOT in this file — they are
  # the reproducibility anchor for Spec §5.4. Removing them here would
  # silently allow drift in the image's dependency versions.
  ```
  を追加し、運用者の意識化を図る。

## 全体所感

- 致命級（must-fix）6 件のうち、Round 3 検証で「完全に閉じた」と判定できるのは **must-fix-001 / 002 / 004 / 005 の 4 件**。must-fix-003 は title-proxy.Dockerfile の `|| true` 残置で **partially**、must-fix-006 は backend 運用ガイド未追記で **unchanged**。
- should-fix 10 件のうち閉じたのは should-fix-001 のみ。CI 観点（should-fix-008 + new-must-fix-001 + new-should-fix-001-003）の薄さは未解決のまま積まれている。
- nitpick は nitpick-006 が静かに閉じた。それ以外は変化なし。
- Round 2 → Round 3 で観察された一番の構造的問題は「処理ログとコードが乖離している」こと。良い方向の乖離（実装が処理ログを上回って進んでいる）と悪い方向の乖離（処理ログが fixed と宣言したものが実装では残っている）の双方が混在。**処理ログ単体を監査根拠にしてはいけない**。
- 再現性の達成度: Round 2 末尾で「半歩前進」と評したが、Round 3 では AMI pin（must-fix-004）が静かに入ったことで「Enclave PCR0 の再現性」に直接効く 4 系統（Cargo.lock / rust-toolchain.toml / Dockerfile dep-cache / AMI）のうち 3 が揃った。残るは base image digest（should-fix-004）と `[profile.release]` 強化（should-fix-010）。SP1 vkey_hash 側は sha2_sp1 rev 化（must-fix-002）と Cargo.lock 同梱（should-fix-001）で原理上の再現性が成立する状態に。

優先順位（Round 4 に向けて）:
1. r3-must-fix-001（title-proxy.Dockerfile の `|| true` 削除） — 5 分作業
2. new-must-fix-001（CI に `--locked` 追加） — 10 分作業
3. new-should-fix-001（clippy に `-- -D warnings` 追加） — 5 分作業
4. should-fix-010（`[profile.release]` 強化） — K3 観点と合議
5. should-fix-004（base image digest pin） — operations ドキュメント更新と同時
6. must-fix-006（tfstate backend ガイド） — OPERATIONS_JA に追記
7. r3-should-fix-002（version 同期 check）
8. should-fix-008 残（Anchor / SP1 / Docker を CI に追加）

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| must-fix-001 | fixed-confirmed | `crates/proxy/Cargo.toml:18` で `default = []`、`vendor-aws` は本番 Dockerfile で明示。 |
| must-fix-002 | fixed-confirmed | `crates/attestation-aws-nitro/Cargo.toml:33` で `rev = "1f224388fdede7cef649bce0d63876d1a9e3f515"`、`Cargo.lock:5338` も同 rev で固定。 |
| must-fix-003 | partially-fixed | 4 Dockerfile すべて proxy COPY + stub 完了。ただし `deploy/aws/docker/title-proxy.Dockerfile:38` に `|| true` 残置（r3-must-fix-001 で再掲）。 |
| must-fix-004 | fixed-confirmed | `deploy/aws/terraform/main.tf:43-47` で `variable "al2023_ami_id"` をハードコード、`aws_instance.node.ami` から参照。Round 2 処理ログの wontfix 判定は誤り。 |
| must-fix-005 | fixed-confirmed | `.gitignore` 除外なし、コメントで意図明示。 |
| must-fix-006 | unchanged | `deploy/aws/README.md` / `docs/v0.1.2/OPERATIONS_JA.md` ともに tfstate backend / state 共有運用の記述なし。 |
| should-fix-001 | fixed-confirmed | `sp1-guests/attestation-aws-nitro/{host,program}/Cargo.lock` 両方存在。 |
| should-fix-002 | unchanged | SP1 host / program / Anchor program に `rust-toolchain.toml` なし。 |
| should-fix-003 | unchanged | `crates/attestation-aws-nitro/Cargo.toml:24` の `p256 = "0.13"` と `crates/crypto/Cargo.toml:14` の `p256 = "0.13.2"` の不揃いそのまま。 |
| should-fix-004 | unchanged | 4 Dockerfile すべて `rust:1.93-bookworm` / `debian:bookworm-slim` の moving tag。 |
| should-fix-005 | partially-fixed | gateway / tee-mock / tee-nitro の `|| true` 削除済み。title-proxy のみ残置（r3-must-fix-001）。 |
| should-fix-006 | unchanged | `user-data.sh:20` で `dnf update -y` 維持。 |
| should-fix-007 | unchanged | `Anchor.toml` に `[scripts]` セクション無し。 |
| should-fix-008 | partially-fixed | `.github/workflows/ci.yml` 存在。`--locked` / clippy `-D warnings` / Anchor / SP1 / Docker は未カバー（new-must-fix-001 / new-should-fix-001..003 で再掲）。 |
| should-fix-009 | partially-fixed | `crates/proxy/Cargo.toml` の `reqwest` は inline 宣言のまま。 |
| should-fix-010 | unchanged | ルート `Cargo.toml:48-49`, `programs/title-whitelist/Cargo.toml:24-25` ともに `overflow-checks = true` のみ。 |
| nitpick-001 | unchanged | `.dockerignore` 5 行のまま。 |
| nitpick-002 | unchanged | workspace member 順序、`title-tee` が `title-gateway` より前。 |
| nitpick-003 | unchanged | Dockerfile §5.4 コメント変化なし。 |
| nitpick-004 | unchanged | `programs/title-whitelist/Cargo.toml` の package メタが直書き。 |
| nitpick-005 | unchanged | `docker/smoke-test.sh` が `docker compose` を呼ばないまま。 |
| nitpick-006 | fixed-confirmed | `deploy/aws/scripts/run-stack.sh:87-101` で `TEE_READY=0` フラグ + `exit 1` 実装済み。Round 2 処理ログの wontfix 判定は誤り。 |
| nitpick-007 | unchanged | `Anchor.toml:21` の `wallet = "~/.config/solana/id.json"` のまま。 |
| new-must-fix-001 | unchanged | `.github/workflows/ci.yml` の cargo 呼び出しに `--locked` 無し。 |
| new-should-fix-001 | unchanged | `.github/workflows/ci.yml:24` の clippy に `-- -D warnings` 無し。 |
| new-should-fix-002 | unchanged | CI が Anchor / SP1 / Docker をビルドしない構成のまま。 |
| new-should-fix-003 | unchanged | `RUSTFLAGS: -D warnings` が file-level env で全 job に被さる。 |
| new-nitpick-001 | unchanged | `.gitignore:34` `legacy/` 二重宣言問題そのまま。 |
| new-nitpick-002 | unchanged | `.gitignore:20-21` `CLAUDE.md` ignore そのまま（r3-nitpick-001 で論点整理）。 |
| new-nitpick-003 | unchanged | `crates/proxy/Cargo.toml:14-21` の mock 用途未言及。 |
| new-nitpick-004 | unchanged | `programs/title-whitelist/Cargo.toml:24-25` の `[profile.release]` そのまま。 |
| r3-regression-001 | fixed | Round 2 audit ファイルの処理ログを訂正。`docs/v0.1.2/audit/round2/e-reproducibility.md:528-541` で must-fix-004 と nitpick-006 を `fixed` に、must-fix-003 と should-fix-005 を `partially-fixed` (Round 3 で削除済みの注記付き) に書き換え。 |
| r3-must-fix-001 | fixed | `deploy/aws/docker/title-proxy.Dockerfile:38` の `|| true` を削除。他 3 Dockerfile と対称化、stub source COPY 済みなので fail-fast でビルドは通る。 |
| r3-should-fix-001 | fixed | `.gitignore:33-38` の `legacy/` 直前コメントを拡張、運用ルール (新規ファイル追加禁止) と `programs/` `sp1-guests/` との非対称の意図を明示。 |
| r3-should-fix-002 | wontfix | バージョン同期 check は CI 整備フェーズで should-fix-008 と統合対応する。専用スクリプト追加は CI workflow 改修と同時に行うのが筋。 |
| r3-nitpick-001 | wontfix(H観点) | `CLAUDE.md` ignore 方針は OSS maturity の話で、H 観点で扱う。E 観点では再現性に直接影響しない。 |
| r3-nitpick-002 | fixed | `.dockerignore` 冒頭に「Cargo.lock / rust-toolchain.toml は §5.4 再現性アンカー、除外するな」コメント追加。 |
