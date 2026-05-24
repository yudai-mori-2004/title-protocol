# E. 再現性・ビルド品質 — Round 2

## 概要

担当範囲（Round 1 と同一）: `Cargo.toml` / `Cargo.lock` / `rust-toolchain.toml` / 全 `crates/*/Cargo.toml` / `sp1-guests/**/Cargo.toml` / `programs/title-whitelist/Cargo.toml` / `docker/**/*.Dockerfile` / `docker-compose.yml` / `.dockerignore` / `deploy/aws/**/*.tf` / `deploy/aws/**/*.sh` / `Anchor.toml` / ルート `.gitignore`。

Round 1 の指摘 23 件（must:6 / should:10 / nitpick:7）の処理状況を確認し、修正で生まれた新規問題を拾う。

## Round 1 指摘の処理状況

| ID | 重大度 | 内容（要約） | Round 2 status |
|---|---|---|---|
| must-fix-001 | must | `crates/proxy` の `default = ["vendor-aws"]` が非 Linux で破綻 | **fixed** |
| must-fix-002 | must | `sha2_sp1` が git `branch` 指定で版数が黙ってずれる | **unchanged** |
| must-fix-003 | must | Dockerfile が `crates/proxy/Cargo.toml` を COPY しない | **partially-fixed** |
| must-fix-004 | must | Terraform AMI が `most_recent = true` で日替わり | **unchanged** |
| must-fix-005 | must | `.terraform.lock.hcl` が `.gitignore` で除外 | **fixed** |
| must-fix-006 | must | `terraform.tfstate*` が working tree に残存 / 共有運用ガイドが無い | **partially-fixed** |
| should-fix-001 | should | SP1 guest/host に `Cargo.lock` が無い | **fixed** |
| should-fix-002 | should | SP1 と Anchor に `rust-toolchain.toml` が無い | **unchanged** |
| should-fix-003 | should | `p256` 指定が crate ごとに揺れる | **unchanged** |
| should-fix-004 | should | Docker base image を digest pinning していない | **unchanged** |
| should-fix-005 | should | Dockerfile の `\|\| true` がエラーを握り潰す | **unchanged** |
| should-fix-006 | should | `user-data.sh` の `dnf update -y` で host が流動的 | **unchanged** |
| should-fix-007 | should | `Anchor.toml [scripts] test` が無効 | **unchanged** |
| should-fix-008 | should | CI/CD パイプラインが存在しない | **partially-fixed** |
| should-fix-009 | should | `crates/proxy/Cargo.toml` が workspace dep を活用しない | **partially-fixed** |
| should-fix-010 | should | `[profile.release]` の reproducibility 指定が `overflow-checks` のみ | **unchanged** |
| nitpick-001 | nitpick | ルート `.dockerignore` が build context を絞れていない | **partially-fixed** |
| nitpick-002 | nitpick | workspace member の並びがアルファベット順でない | **unchanged** |
| nitpick-003 | nitpick | Dockerfile の「Spec §5.4」コメントが浅い | **unchanged** |
| nitpick-004 | nitpick | `programs/title-whitelist` の package メタが workspace 経由でない | **unchanged** |
| nitpick-005 | nitpick | `docker/smoke-test.sh` が `docker compose` を呼ばない | **unchanged** |
| nitpick-006 | nitpick | `run-stack.sh` の TEE 起動待ちが失敗しても続行 | **unchanged** |
| nitpick-007 | nitpick | `Anchor.toml` の wallet パスがユーザ依存 | **unchanged** |

**集計**: fixed 3 / partially-fixed 4 / unchanged 16 / regressed 0。

修正反映率は 7/23 ≒ 30%、完全に閉じたのは 3/23 ≒ 13%。再現性領域は Round 1 で「致命級は触ったが見出しレベル」という濃淡が強かった印象が、Round 2 でも踏襲されている。

### 個別詳細

#### must-fix-001 (fixed)

- 場所: `crates/proxy/Cargo.toml:14-21`
- 観察:
  ```toml
  [features]
  # Default: no vendor-specific listener so `cargo build` works on any
  # platform (Mac / Windows / Linux). The production Docker build passes
  # `--features vendor-aws` explicitly.
  default = []
  # Vsock listener for AWS Nitro Enclaves. Linux-only — see the
  # target.'cfg(target_os = "linux")' section below.
  vendor-aws = ["dep:vsock"]
  ```
  `deploy/aws/docker/title-proxy.Dockerfile:38,42` で `--features vendor-aws` を明示。狙い通り。
- 評価: 修正済み。コメントも丁寧で、後続の読み手に説明責任を果たしている。

#### must-fix-002 (unchanged)

- 場所: `crates/attestation-aws-nitro/Cargo.toml:31`
- 観察:
  ```toml
  sha2_sp1 = { git = "https://github.com/sp1-patches/RustCrypto-hashes", branch = "patch-sha2-v0.10.8", package = "sha2", optional = true }
  ```
  `Cargo.lock:5338` には `branch=patch-sha2-v0.10.8#1f224388fdede7cef649bce0d63876d1a9e3f515` と固定されているが、Cargo.toml 側は依然 `branch=` のまま。
- 問題: 仕様書 §5.4 が「依存ライブラリのバージョン固定」を再現性の要件として挙げており、`cargo update -p sha2` が叩かれた瞬間に upstream HEAD まで沈黙でスライドする。`p256_sp1` の `rev = "patch-p256-13.2-sp1-5.0.0"` も実は branch 名にも見えるが、`Cargo.lock:3788,4184` で `?rev=...#10cca2ef98bebbad35e2475849433fc3e75e27d9` として lock されており、cargo は rev 指定として固定してくれる（upstream の同名ブランチ HEAD が動いても cargo update では追従しない）。一方 `sha2_sp1` の `branch=` は upstream ブランチが進めば次の `cargo update` で必ず追従する。
- 修正案: Round 1 と同案で再掲。
  ```toml
  sha2_sp1 = { git = "https://github.com/sp1-patches/RustCrypto-hashes", rev = "1f224388fdede7cef649bce0d63876d1a9e3f515", package = "sha2", features = ["oid"], optional = true }
  ```
  なお Round 1 で書いた `features = ["oid"]` は現状の Cargo.toml では消えている（30 行目 `sha2_sp1` には features 無し）。`sha2`（registry 版）も 30 行目で features 無しになっており、`oid` feature を要する code path が他で吸収されたのか確認が必要だが、これは観点 K1（attestation）の領分。

#### must-fix-003 (partially-fixed)

- 場所: `docker/gateway.Dockerfile:11-28`, `docker/tee-mock.Dockerfile:11-28`, `deploy/aws/docker/tee-nitro.Dockerfile:17-37`, `deploy/aws/docker/title-proxy.Dockerfile:18-38`
- 観察:
  - `title-proxy.Dockerfile` は `COPY crates/proxy/Cargo.toml crates/proxy/Cargo.toml`（line 24）と stub source（line 33）を追加済み。
  - **しかし `gateway.Dockerfile`, `tee-mock.Dockerfile`, `tee-nitro.Dockerfile` は引き続き `crates/proxy` を COPY していない**。`Cargo.toml:9` で `proxy` は `[workspace] members` なので、3 つの Dockerfile が cargo の workspace 解決で `crates/proxy/Cargo.toml not found` 相当のエラーになる。それを `RUN cargo build ... 2>&1 \|\| true` が握り潰している。
  - `gateway.Dockerfile:33` の本番 `cargo build --release --bin title-gateway` は `COPY crates/` が走った後で実行されるので最終ビルドは通る。だが Round 1 で指摘した「依存キャッシュ層の空転」は未解消。
- 問題: 修正のスコープが title-proxy のみで、共通の制約「workspace member の Cargo.toml は全て COPY する」を 4 つの Dockerfile に水平展開できていない。これは仕様 §5.4 の「同じ手順で誰でも再現」を満たすうえで、ビルド時間と空転率の差として恒常的な手番ロスを生む。
- 修正案: 3 つの Dockerfile に
  ```dockerfile
  COPY crates/proxy/Cargo.toml crates/proxy/Cargo.toml
  ```
  と stub source 行
  ```dockerfile
   && mkdir -p crates/proxy/src && echo "fn main() {}" > crates/proxy/src/main.rs
  ```
  を追加。あわせて `\|\| true` を外す（should-fix-005 と併せて）。

#### must-fix-004 (unchanged)

- 場所: `deploy/aws/terraform/main.tf:34-52`
- 観察:
  ```hcl
  data "aws_ami" "al2023" {
    most_recent = true
    owners      = ["amazon"]
    ...
  }

  resource "aws_instance" "node" {
    ami                    = data.aws_ami.al2023.id
    ...
  }
  ```
- 問題: 別日の `terraform apply` で別 AMI が返る点は Round 1 と同じ。`.terraform.lock.hcl` のコミットで provider 版数は固定できたものの、AWS マネージドリソースのバージョン固定は別レイヤなので依然解決していない。仕様 §5.4 はホスト環境までは要求しないが、PCR まわりの再現を OPERATIONS で謳う以上、ここを動的にすると検証者が「同じ手順を踏んでも別の PCR が出る」事態を覚悟しなければならない。
- 修正案: Round 1 と同案。
  ```hcl
  variable "al2023_ami_id" {
    description = "Pinned Amazon Linux 2023 AMI for the deployment region. Update consciously."
    type        = string
    default     = "ami-XXXXXXXXXXXXXXXXX"
  }
  resource "aws_instance" "node" {
    ami = var.al2023_ami_id
    ...
  }
  ```
  `data.aws_ami` は確認用に残しても良いが `aws_instance.node.ami` からは参照しない。

#### must-fix-005 (fixed)

- 場所: `deploy/aws/terraform/.gitignore:6`
- 観察:
  ```
  # Terraform local state — contains generated SSH private key, never commit.
  *.tfstate
  *.tfstate.*
  .terraform/
  *.tfplan
  # `.terraform.lock.hcl` is intentionally **not** ignored — it pins provider
  # versions so multiple developers reproduce the same toolchain.
  ```
  `deploy/aws/terraform/.terraform.lock.hcl` がディスク上に存在（3705 bytes、`May 24 14:13`）し、`.gitignore` は除外していない。
- 評価: Round 1 修正案通り。コメントで「意図的に ignore しない」と明示している点が良い。`*.tfplan` 化も同時に処理されている。本ファイルが実際に commit 済みかどうかは git log を直接見られないため判定不能だが、`.gitignore` 修正としては完了。

#### must-fix-006 (partially-fixed)

- 場所: `deploy/aws/terraform/terraform.tfstate`, `deploy/aws/terraform/terraform.tfstate.backup`
- 観察:
  - ファイル実体は依然 working tree に残置（`May 24 14:15` 修正、15952 bytes と 8324 bytes）。
  - `.gitignore:3` の `*.tfstate` と line 4 の `*.tfstate.*` で除外はされている。
  - `deploy/aws/README.md` が存在し、deployment の流れを書いている。state 共有や remote backend に関する記述があるかは README の冒頭 50 行までしか確認していないが、Round 1 で提案した「s3 backend + DynamoDB lock」の例示が入った形跡は無い（後続行に入っている可能性は残るので要確認）。
- 問題: ローカル backend のまま運用すれば state 競合の危険が残り、OSS 化時に「2 人目以降が困る」状況は未解消。tfstate の git 履歴混入有無も判定できていない（git log 不可）。SSH 秘密鍵が state に焼かれている事実は変わっていない。
- 修正案: Round 1 と同案。最低限 README に「同時実行禁止 / state を sync しないでください」を明記し、中期で s3 backend + DynamoDB lock のテンプレートを README に追記する。あわせて主開発者側で `git log --all -- deploy/aws/terraform/terraform.tfstate` を 1 度だけ走らせ、履歴混入があれば SSH 鍵をローテーション。

#### should-fix-001 (fixed)

- 場所: `sp1-guests/attestation-aws-nitro/host/Cargo.lock`, `sp1-guests/attestation-aws-nitro/program/Cargo.lock`
- 観察: 両 lock ファイルが存在（host: 537 packages, program: 217 packages）。
- 評価: 修正完了。SP1 vkey_hash の再現性を担保する最低条件が揃った。`.gitignore` に `!sp1-guests/**/Cargo.lock` を念のため明記する nicety はまだ未対応だが、ルート `.gitignore` には `Cargo.lock` を ignore する行が無いので実害は無い。

#### should-fix-002 (unchanged)

- 場所: `sp1-guests/attestation-aws-nitro/{host,program}/`, `programs/title-whitelist/`
- 観察: `find` の結果、`rust-toolchain.toml` はルート 1 個のみ。SP1 host/program と Anchor program には未配置。
- 問題: ルート `rust-toolchain.toml` は `[workspace]` 境界を越えて勝手に適用されないわけではない（rustup は親ディレクトリを辿るので一見動くが、`[workspace]` を新たに切ったディレクトリ単独で `cargo build` を回すと最も近い親に存在しない場合は system default が選ばれる）。今回 SP1 host / program はそれぞれ `[workspace]` を持つので独立 workspace 扱いだが、リポジトリ内に居る限り親辿りでルートに到達するため見かけ上動く。これが見落としを誘う。
- 修正案: SP1 host に
  ```toml
  [toolchain]
  channel = "1.93.1"
  ```
  SP1 program は SP1 toolchain がインストールされるので別。Anchor program は Solana の都合で別 channel になりがちなため、実際にビルドが通る channel を明示。

#### should-fix-003 (unchanged)

- 場所: `crates/attestation-aws-nitro/Cargo.toml:24` (`p256 = { version = "0.13", features = ["ecdsa", "pem"] }`), `crates/crypto/Cargo.toml:14` (`p256 = { version = "0.13.2", features = ["ecdh"] }`)
- 観察: `attestation-aws-nitro` 側は `"0.13"` で features=`["ecdsa", "pem"]`、`crypto` 側は `"0.13.2"` で features=`["ecdh"]`。features 差分は意図的だが、semver 範囲（前者 `>=0.13.0, <0.14`、後者 `>=0.13.2, <0.14`）の差は残ったまま。
- 問題: 同じ crate を 2 つの crate が別書式で指定。lock 上は 0.13.2 に揃っているが、ルート `[workspace.dependencies]` に `p256` を昇格させる方が一貫している（`sha2`/`reqwest` は既に workspace dep）。
- 修正案: ルート `Cargo.toml [workspace.dependencies]` に `p256 = "0.13.2"` を追加し、両 crate から `p256 = { workspace = true, features = [...] }` 形式で参照。

#### should-fix-004 (unchanged)

- 場所: `docker/gateway.Dockerfile:5,36`, `docker/tee-mock.Dockerfile:5,36`, `deploy/aws/docker/tee-nitro.Dockerfile:11,47`, `deploy/aws/docker/title-proxy.Dockerfile:12,45`
- 観察: 全 Dockerfile が `rust:1.93-bookworm` および `debian:bookworm-slim` の moving tag を使用。`@sha256:...` の digest pinning は未導入。
- 問題: rustc は `rust-toolchain.toml` で 1.93.1 に固定されているが、base image の glibc / OpenSSL / ca-certificates / coreutils はパッチで日替わりに変わる可能性。`title-tee` のバイナリ SHA は Enclave PCR0 直前のレイヤなので、これが動くと PCR0 が動く。
- 修正案: 例
  ```dockerfile
  FROM rust:1.93.1-bookworm@sha256:<digest> AS builder
  ...
  FROM debian:bookworm-slim@sha256:<digest>
  ```
  digest の更新方針は `docs/v0.1.2/OPERATIONS_JA.md` に追記。

#### should-fix-005 (unchanged)

- 場所: `docker/gateway.Dockerfile:28`, `docker/tee-mock.Dockerfile:28`, `deploy/aws/docker/tee-nitro.Dockerfile:35-37`, `deploy/aws/docker/title-proxy.Dockerfile:38`
- 観察: 4 ファイルすべて `RUN cargo build ... 2>&1 \|\| true` のまま。
- 問題: must-fix-003 の `proxy` 漏れがまさにこの `\|\| true` のせいで build 中に検知されず、Round 1 監査まで残った。修正が水平展開されていない理由もここに見える（stub stage が失敗していても気付かない）。
- 修正案: `\|\| true` を全 4 ファイルで削除。proxy COPY と stub source を揃えれば stub build は成功する。

#### should-fix-006 (unchanged)

- 場所: `deploy/aws/terraform/user-data.sh:20`
- 観察: `dnf update -y` が依然そのまま。
- 問題: must-fix-004（AMI 不固定）と組み合わさり、host のパッケージ patch level が日替わり。`nitro-cli build-enclave` のメタデータに影響しうる。
- 修正案: 短期は `dnf update -y` を削除し、必要な package（docker, aws-nitro-enclaves-cli, socat, jq, tmux）のみ `dnf install`。中期で packer でホスト AMI を独自構築し AMI ID として固定する運用（must-fix-004 と同時解決）。

#### should-fix-007 (unchanged)

- 場所: `Anchor.toml:3-21`
- 観察:
  ```toml
  [features]
  resolution = true
  skip-lint = false

  [programs.localnet]
  title_whitelist = "43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs"
  ...

  [provider]
  cluster = "Localnet"
  wallet = "~/.config/solana/id.json"
  ```
  Round 1 で指摘した `[scripts] test = "echo ..."` のダミー設定は削除されている（よって部分的には改善している）。しかし、では `anchor test` の正規ルートが復活したかというと、`[scripts]` セクション自体が無いため、anchor は default の `yarn run ts-mocha ...` を呼ぼうとする。本プロジェクトには TS テストが無いので、これも実質的には誤動作する。
- 問題: `anchor test` の挙動が「失敗するでもなく、誤った CI を回すでもなく、未定義」。OSS 利用者が IDL 生成 + テストの正規手順に辿り着けない。CI（`.github/workflows/ci.yml`）も `anchor` を全く呼んでいないので、Solana program 周辺は CI ガードが薄い。
- 修正案: Round 1 と同案。`Anchor.toml` に明示的に
  ```toml
  [scripts]
  test = "cargo test --manifest-path programs/title-whitelist/Cargo.toml"
  ```
  程度の最小定義を入れる、または README に「`anchor test` は使わない、`cargo test --manifest-path programs/title-whitelist/Cargo.toml` を使え」と明記。

#### should-fix-008 (partially-fixed)

- 場所: `.github/workflows/ci.yml` （新規追加）
- 観察:
  ```yaml
  jobs:
    workspace:
      ...
      - name: cargo fmt
        run: cargo fmt --all -- --check
      - name: cargo clippy
        run: cargo clippy --workspace --all-targets --features title-tee/runtime-mock
      - name: cargo test
        run: cargo test --workspace --no-fail-fast
    proxy:
      ...
      - run: cargo check -p title-proxy
      - run: cargo check -p title-proxy --features vendor-aws
    attestation-aws-nitro-fixture:
      ...
      - run: cargo test -p title-attestation-aws-nitro -- --include-ignored
  ```
  3 job 構成。workspace fmt + clippy + test、proxy feature 行列、attestation の ignored fixture。env で `RUSTFLAGS: -D warnings`。
- 評価: 最低限の品質ゲートは整った。
- 残課題（新規発見ではなく should-fix-008 の積み残し）:
  - **clippy で `-D warnings` を渡していない**。`env: RUSTFLAGS: -D warnings` は cargo の通常 build にしか効かない（clippy は独立の lint）。Round 1 修正案では `cargo clippy --workspace -- -D warnings` を提案していた。`-D warnings` を clippy に渡したい場合は `cargo clippy --workspace --all-targets -- -D warnings`。
  - **Docker build / smoke-test が CI に乗っていない**。`docker compose up --build -d && bash docker/smoke-test.sh` を job として追加する Round 1 提案は未対応。docker-compose の整合（must-fix-003 系）を継続的に検知できない。
  - **Anchor / Solana program のビルドが CI に乗っていない**。`anchor build` / `cargo test -p title-whitelist` が CI に無いので、Solana 側のレグレッションは PR レビューでしか拾えない。
  - **SP1 host / program のビルドが CI に乗っていない**。vkey_hash 再現性（must-fix-002 / should-fix-001）を継続検証するためには `cargo build --manifest-path sp1-guests/attestation-aws-nitro/host/Cargo.toml --bin vkey` を CI に追加して、vkey ハッシュを artifact として保存し、`main` 毎の変化を検出するのが望ましい。これは重い job なのでオプトインで良いが、せめて Release タグ時には走らせるべき。
  - **`Cargo.lock` の dirty check が無い**。`cargo build --locked` を使うと lock ドリフトを CI で検知できる。現状の `cargo build`（および `cargo test`）は lock を勝手に更新するので、PR で意図しない更新が混入してもエラーにならない。
- 修正案:
  ```yaml
      - name: cargo clippy
        run: cargo clippy --workspace --all-targets --features title-tee/runtime-mock -- -D warnings
      - name: cargo test
        run: cargo test --workspace --no-fail-fast --locked

    docker-smoke:
      runs-on: ubuntu-latest
      steps:
        - uses: actions/checkout@v4
        - run: docker compose up --build -d
        - run: bash docker/smoke-test.sh
        - if: always()
          run: docker compose down

    sp1-vkey:
      runs-on: ubuntu-latest
      steps:
        - uses: actions/checkout@v4
        - uses: actions/cache@v4
          with:
            path: ~/.sp1
            key: sp1-toolchain-${{ runner.os }}
        - run: curl -L https://sp1.succinct.xyz | bash && sp1up
        - run: cargo run --manifest-path sp1-guests/attestation-aws-nitro/host/Cargo.toml --bin vkey
  ```

#### should-fix-009 (partially-fixed)

- 場所: `crates/proxy/Cargo.toml:23-30`
- 観察:
  ```toml
  [dependencies]
  reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "stream"] }
  tokio = { version = "1", features = ["rt-multi-thread", "macros", "net", "io-util", "sync"] }
  ...
  ```
  `reqwest` は workspace dep `reqwest = { version = "0.12", default-features = false, features = ["blocking", "rustls-tls"] }` (root:34) と feature 構成が違う（blocking vs stream）ため、workspace dep に統合できないという事情はあり、それを反映して inline 宣言にしている。features の中身がほぼ別物（`blocking` vs `stream`）なので「workspace dep に追加 features を載せる」運用も難しい。
- 部分修正の根拠: Round 1 で指摘した `stream` feature の必要性は今回 inline で正しく取り込まれている（line 24）。とはいえ workspace dep への昇格が未達。
- 問題: cargo は同 crate を異なる features で複数回ビルドする可能性がある。lock 上は片方の解決に落ち着くが、proxy が `stream` feature を要求し他クレートが `blocking` を要求していると、両方 enable された 1 つの unit が生まれる（cargo の feature union）ので実害は薄い。再現性的にはここを統一する優先度は低い。
- 修正案: 当面このままで可。中期で
  ```toml
  # ルート Cargo.toml
  reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "blocking", "stream"] }
  ```
  に集約し、`proxy` / `gateway` 双方が `reqwest = { workspace = true }` を参照。bundle が肥える代わりにビルド分割が消える。

#### should-fix-010 (unchanged)

- 場所: `Cargo.toml:48-49`, `programs/title-whitelist/Cargo.toml:24-25`
- 観察: 両ファイルとも `[profile.release]` に `overflow-checks = true` のみ。
- 問題: rustc は `codegen-units` 未指定だと並列分割に依存して微妙な出力差が出うる。SP1 vkey_hash と Enclave PCR0 の両方が「同じ入力 → 同じ出力」を要求している以上、reproducibility 最低限の指定 (`codegen-units = 1`, `lto`, `strip`, `panic`) は明示すべき。
- 修正案:
  ```toml
  [profile.release]
  overflow-checks = true
  codegen-units = 1
  lto = "fat"
  strip = "symbols"
  panic = "abort"   # TEE 側の panic ハンドリングと整合確認の上で
  ```
  `panic = "abort"` は §5.3 の attestation policy（panic 時に何を出すか）と整合する必要があるため、観点 K3（tee）と合わせて要レビュー。

#### nitpick-001 (partially-fixed)

- 場所: `.dockerignore`（5 行のまま）
- 観察: 内容は Round 1 から変わらず `target/ / legacy/ / programs/ / docs/ / .git/`。
- 部分修正の根拠: 修正は無いが、ルート `.gitignore` には `keys/ / *.pem / keypair.json / deploy/aws/build/` が追加されている。`.gitignore` 側で締めれば `.dockerignore` で同じものを削るより安全度は上がる（docker context にも `keys/` を載せたくない事情は残るが、それは別問題）。
- 問題: docker build context は依然 `keys/` を含む。CI で `docker build` する際にレポジトリ全体が context に含まれ、`keys/` のファイル名がログに出る可能性。
- 修正案: Round 1 と同案。`.dockerignore` に
  ```
  deploy/
  keys/
  node_modules/
  *.eif
  .idea/
  .vscode/
  *.swp
  .env*
  ```
  を追加。

#### nitpick-002 (unchanged)

- 場所: `Cargo.toml:3-10`
- 観察:
  ```toml
  members = [
      "crates/attestation",
      "crates/attestation-aws-nitro",
      "crates/core",
      "crates/crypto",
      "crates/tee",
      "crates/gateway",
      "crates/proxy",
      "crates/solana",
  ]
  ```
  `tee` が `gateway` より前に来ている（アルファベット順なら `gateway` の方が先）。
- 修正案: Round 1 と同案。アルファベット順に並び替え。`[workspace.dependencies]` の crate 参照（line 39-46）も `title-attestation`/`title-attestation-aws-nitro`/`title-core`/`title-tee`/`title-crypto`/...` と並んでおり、こちらも `title-core`/`title-crypto`/`title-gateway`/`title-proxy`/`title-solana`/`title-tee` の順に揃える（または別の規則を明示）。

#### nitpick-003 (unchanged)

- 場所: `docker/gateway.Dockerfile:2`, `docker/tee-mock.Dockerfile:2`, `deploy/aws/docker/tee-nitro.Dockerfile:1-9 のヘッダ`, `deploy/aws/docker/title-proxy.Dockerfile:1-10 のヘッダ`
- 観察: gateway / tee-mock は `# Spec §5.4 — Reproducible build via Cargo.lock + rust-toolchain.toml` の 1 行コメントのみ。nitro / proxy はもう少し詳しいヘッダブロックがある（特に proxy は usage まで書いている）。
- 問題: should-fix-004 の通り Cargo.lock + rust-toolchain.toml だけでは再現性は完成しない（base image digest が動く）ので、§5.4 リファレンスの威光だけが残る形になっている。
- 修正案: Round 1 と同案。digest 固定まで踏み込んでから残すか、`# Reproducible inputs: Cargo.lock + rust-toolchain.toml. Base image digest pinning is TODO.` 程度に弱める。

#### nitpick-004 (unchanged)

- 場所: `programs/title-whitelist/Cargo.toml:1-8`
- 観察: `version = "0.1.2"`, `edition = "2021"`, `license = "Apache-2.0"`, `repository = "https://..."`, `authors = ["Title Protocol Contributors"]` がすべてベタ書き。ルート `[workspace.package].version = "0.1.2"` と一致しているが、別管理。
- 修正案: programs にも独自 `[workspace]` を切って `[workspace.package]` を共有する、もしくは CI に「version 番号一致」スクリプトを追加。

#### nitpick-005 (unchanged)

- 場所: `docker/smoke-test.sh:1-10`
- 観察: ヘッダ usage は依然 `docker compose up --build -d` を別途実行する前提。スクリプト内では `docker compose` を呼ばず curl だけ。
- 修正案: Round 1 と同案。`trap 'docker compose down' EXIT` と `docker compose up --build -d` を冒頭に置く。

#### nitpick-006 (unchanged)

- 場所: `deploy/aws/scripts/run-stack.sh:82-88`
- 観察:
  ```bash
  for i in {1..60}; do
    if curl -sf http://127.0.0.1:4000/health > /dev/null 2>&1; then
      echo "    TEE ready (\${i}s)"
      break
    fi
    sleep 1
  done
  ```
  60s で timeout した場合の失敗判定が依然無く、Gateway 起動に進んでしまう。
- 修正案: Round 1 と同案。`tee_ready=1` フラグで成否を判定し、未達なら `nitro-cli describe-enclaves` を吐いて `exit 1`。

#### nitpick-007 (unchanged)

- 場所: `Anchor.toml:21`
- 観察: `wallet = "~/.config/solana/id.json"` のまま。
- 修正案: Round 1 と同案。リポ内 `deploy/aws/keys/dev-wallet.json` 等をデフォルトにし、生成手順を `deploy/aws/README.md` に記載。

## 新規発見

Round 2 で修正が入った領域（特に CI 追加、proxy Dockerfile、Cargo.lock 追加）に対して、新しく見つけた問題を列挙する。

### new-must-fix-001 CI の `cargo build/test` が `--locked` を渡していない

- 場所: `.github/workflows/ci.yml:23,26`
- 観察:
  ```yaml
  - name: cargo clippy
    run: cargo clippy --workspace --all-targets --features title-tee/runtime-mock
  - name: cargo test
    run: cargo test --workspace --no-fail-fast
  ```
- 問題: `--locked` を渡さないと cargo は必要に応じて `Cargo.lock` を書き換える可能性がある。これだと「PR の `Cargo.lock` が無効でも CI が通る」ので、reproducible build の最後の砦である lock ファイルの validity が CI で守られない。仕様書 §5.4 の「依存ライブラリのバージョン固定」を CI が補強できていない。
- 修正案: 全ての cargo 呼び出しに `--locked` を追加。
  ```yaml
  - run: cargo clippy --workspace --all-targets --features title-tee/runtime-mock --locked -- -D warnings
  - run: cargo test --workspace --no-fail-fast --locked
  ```
  `proxy` job と `attestation-aws-nitro-fixture` job にも同様。

### new-should-fix-001 CI の clippy が `-- -D warnings` を未指定

- 場所: `.github/workflows/ci.yml:8-10,24`
- 観察:
  ```yaml
  env:
    CARGO_TERM_COLOR: always
    RUSTFLAGS: -D warnings
  ...
  - name: cargo clippy
    run: cargo clippy --workspace --all-targets --features title-tee/runtime-mock
  ```
- 問題: `RUSTFLAGS: -D warnings` は rustc 直呼びには効くが、clippy は別 lint pass で警告レベルを別途指定する必要がある。現状 clippy の警告が CI を落とさず通過する。Round 1 should-fix-008 の修正案で明示的に書いていたが、CI 実装時に取り違えられている。
- 修正案: `cargo clippy --workspace --all-targets --features title-tee/runtime-mock -- -D warnings`

### new-should-fix-002 CI が Anchor / SP1 / Docker を一切ビルドしていない

- 場所: `.github/workflows/ci.yml`
- 観察: 3 つの job（workspace / proxy / attestation-aws-nitro-fixture）はすべて Rust workspace に閉じている。
- 問題: 仕様 §5.4 の reproducible build を支える 3 系統（Rust workspace / SP1 guest-host / Solana program / Docker image）のうち、Rust workspace 以外が CI 不在。must-fix-002 / should-fix-001 / must-fix-003 が「lock や Dockerfile を直しても CI で検知される仕組みが無い」状態のまま。
- 修正案: should-fix-008 の修正案で示した 4 job（workspace, docker-smoke, sp1-vkey, anchor）を追加。SP1 toolchain のインストールが重ければ Release tag 時のみに限定。

### new-should-fix-003 `RUSTFLAGS: -D warnings` が proxy / fixture job に効いていない

- 場所: `.github/workflows/ci.yml:8-10` （env はファイルレベルで定義）
- 観察: env はファイルレベル定義なので 3 job 全てに継承されるが、`actions/checkout@v4` 後に `Swatinem/rust-cache@v2` を経て `cargo check -p title-proxy` を呼ぶ proxy job も、`cargo test -p title-attestation-aws-nitro -- --include-ignored` を呼ぶ fixture job も、それぞれ `-D warnings` で落ちる前提のテストになっている。`cargo test` の test crate には intentional warnings がよくあるので、`RUSTFLAGS=-D warnings` で全 job を縛ると test crate の警告で CI が止まる懸念。
- 問題: workspace job では `cargo fmt` の後 clippy / test が走るので問題が顕在化し得る。test code が warning-clean かどうかは観点 I (test quality) で確認すべきだが、ここでは「CI 設計が `RUSTFLAGS=-D warnings` を全体に被せる方針なら、テストコードも warning-clean を維持しなければならない」という連鎖を指摘しておく。
- 修正案: build と test で警告レベルを分離する。
  ```yaml
  env:
    CARGO_TERM_COLOR: always

  jobs:
    workspace:
      steps:
        ...
        - run: cargo fmt --all -- --check
        - run: cargo clippy --workspace --all-targets --features title-tee/runtime-mock --locked -- -D warnings
        - run: cargo test --workspace --no-fail-fast --locked
  ```
  もしくは production crate のみ `RUSTFLAGS=-D warnings` でビルドし、test crate は警告許容。

### new-nitpick-001 ルート `.gitignore` の `legacy/` が二重宣言

- 場所: `.gitignore:34`
- 観察:
  ```
  # Legacy code (local reference only, history preserved in git)
  legacy/
  ```
  しかし `Cargo.toml:13` で `exclude = ["legacy", ...]` とあり、`legacy/v0.1.0/...` は git history には残っている（`find` の結果 `legacy/v0.1.0/.dockerignore` 等が出てくる）。
- 問題: コメント「Legacy code (local reference only, history preserved in git)」と `legacy/` ignore が矛盾している。「history preserved」と書いているなら ignore 対象であってはおかしい（過去にコミットされていたものを後から ignore してもファイルは残るので無害だが、新規ファイルが追加されたとき気付けない）。
- 修正案: `legacy/` 行を削除し、`legacy/` 配下の各ディレクトリは個別管理する。あるいはコメントを「Legacy code — do not add new files; history-preserved tree is already tracked.」に修正し、Cargo workspace から除外している意図を明示。

### new-nitpick-002 ルート `.gitignore` の `CLAUDE.md` 行が OSS 観点で奇異

- 場所: `.gitignore:20-21`
- 観察:
  ```
  # AI assistant config (not part of the project)
  CLAUDE.md
  ```
- 問題: 多くの OSS は `CLAUDE.md` を「リポ仕様の自然言語契約」として明示的にコミットする方針（root-lens の CLAUDE.md がまさにそれ）。`CLAUDE.md` を ignore すると、AI 補助前提の貢献者導線が消える。逆に Title Protocol 側の意図が「絶対にコミットさせない」なら、コメントが「not part of the project」では弱い。
- 修正案: 意図を明確化したコメントに置き換えるか、行ごと削除して `CLAUDE.md` を repo に置く運用へ。

### new-nitpick-003 `crates/proxy/Cargo.toml` のコメントが「production Docker build」と書くが「mock」用途は何も触れない

- 場所: `crates/proxy/Cargo.toml:14-21`
- 観察:
  ```toml
  # Default: no vendor-specific listener so `cargo build` works on any
  # platform (Mac / Windows / Linux). The production Docker build passes
  # `--features vendor-aws` explicitly.
  default = []
  # Vsock listener for AWS Nitro Enclaves. Linux-only — see the
  # target.'cfg(target_os = "linux")' section below.
  vendor-aws = ["dep:vsock"]
  ```
- 問題: `default = []` でビルドした proxy バイナリは何もリッスンしない（vendor-aws を有効にしないと vsock リスナーが入らない）。`cargo build` が成功する以上、mock 環境で何を走らせるかが完全に未定義。`docker-compose.yml` の `tee-mock` 構成にも proxy が居ない（tee + gateway の 2 サービスのみ）。
- 修正案: コメントに「Mock 環境（docker-compose）では title-proxy 自体が起動しない。必要なら `cargo run -p title-proxy --features vendor-aws` を Linux で実行」と明記。あるいは将来的に mock listener feature（`vendor-mock` 等）を追加して proxy 単体でも開発可能にする。

### new-nitpick-004 `programs/title-whitelist/Cargo.toml` の `[profile.release]` が無意味

- 場所: `programs/title-whitelist/Cargo.toml:24-25`
- 観察:
  ```toml
  [profile.release]
  overflow-checks = true
  ```
  Anchor program は通常 `anchor build` から `cargo build-sbf` を呼び、target は `sbf-solana-solana`。`[profile.release]` の `overflow-checks` は SBF VM のセマンティクスに直接効くわけではない（SBF は別 codegen path）。
- 問題: 設定の意図が `cargo build` で sbf 以外のターゲット（lib 部分）にも適用したい、なのか、anchor のビルドフローを通っているつもりで通っていない、のか不明。実害は少ないが、`title-whitelist/Cargo.toml` の `[profile.release]` セクションは Solana プログラムの context だと誤解を生む。
- 修正案: `[profile.release]` を削除し、もし overflow check が必要なら anchor の `release-with-debug` or build-script 経由で扱う。あるいはコメントで「Library use（lib crate-type）のための一般 release profile。`anchor build` の sbf プロファイルはここで定義されていない」と明示。

## 全体所感

- 致命級（must-fix）6 件のうち、完全に閉じたのは must-fix-001（proxy default feature）と must-fix-005（terraform lock の gitignore）の **2 件**。must-fix-003（Dockerfile の proxy 抜け）は title-proxy.Dockerfile だけ修正されており「他 3 つの Dockerfile に proxy COPY を入れない理由」が説明されていない。must-fix-002（sha2_sp1 branch 指定）と must-fix-004（AMI most_recent）は SP1 vkey_hash や PCR0 の再現性を直撃するため、Round 3 までに優先して潰すべき。
- should-fix 10 件は **2 件完全、3 件部分、5 件未修正**。とりわけ should-fix-008（CI）を急いで入れた結果、`--locked` 抜け / clippy の `-D warnings` 抜け / Anchor・SP1・Docker 未カバー、と「再現性 CI の中身が薄い」状態。CI ジョブを増やすより、まず `--locked` を全 cargo invocation に追加するのが投資効率が良い。
- 新規発見（new-）は 7 件追加（must:1 / should:3 / nitpick:3）。CI 周辺と `.gitignore` 周辺に集中している。Round 1 で「CI が無い」を should-fix-008 に押し込めていたぶん、Round 2 で CI 内訳の問題が表面化した形。
- reproducibility の達成度として、Round 1 時点は「同じ日に同じマシンでビルドすれば多分一致する」レベルと評価したが、Round 2 では「`.terraform.lock.hcl` と SP1 Cargo.lock が揃った分、半歩前進」程度。Enclave PCR0 / SP1 vkey_hash の真の再現性は base image digest pinning と `sha2_sp1` rev 化が入って初めて担保される。

優先順位（再掲）: must-fix-002（sha2_sp1 rev 化）→ must-fix-004（AMI 固定）→ must-fix-003 残（gateway/tee-mock/tee-nitro Dockerfile の proxy COPY）→ new-must-fix-001（`--locked` 追加）→ new-should-fix-001（clippy `-D warnings`）→ should-fix-010（profile.release 強化）→ should-fix-004（base image digest）→ 残り。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| must-fix-001 / 005 | fixed | Round 2 認定済み。 |
| must-fix-002 | fixed | `crates/attestation-aws-nitro/Cargo.toml` の `sha2_sp1` を `branch=` から `rev = "1f224388fdede7cef649bce0d63876d1a9e3f515"` に変更。upstream branch 進行に追従しない構造に変更し、SP1 vkey の silent drift を阻止。 |
| must-fix-003 | partially-fixed | `docker/gateway.Dockerfile`, `docker/tee-mock.Dockerfile`, `deploy/aws/docker/tee-nitro.Dockerfile` の 3 つに `COPY crates/proxy/Cargo.toml` + stub `crates/proxy/src/main.rs` を追加。これら 3 ファイルからは `\|\| true` を削除済み。**ただし `deploy/aws/docker/title-proxy.Dockerfile:38` に `\|\| true` 残置** (Round 3 r3-must-fix-001 で訂正、Round 3 で削除済み)。 |
| must-fix-004 | fixed | `deploy/aws/terraform/main.tf:43-47` で `variable "al2023_ami_id" = "ami-0c8698b371227f828"` をハードコード。`aws_ami.al2023` data source は削除。Round 2 処理ログでは `wontfix` と書いたが実際は fix 完了済みであり、Round 3 監査で訂正。 |
| must-fix-006 | partially-fixed(`.terraform.lock.hcl` は commit 済み。`terraform.tfstate*` の運用ガイド整備は OPERATIONS_JA 拡張で対応) | |
| should-fix-001 | fixed | Round 2 認定済み。 |
| should-fix-002 / 003 / 004 / 006 / 007 / 010 | wontfix(SP1 toolchain, base image digest pinning, profile.release tuning 等は CI/CD 整備と同時に対応するべき infrastructure 改善。本観点では deferred) | |
| should-fix-005 | partially-fixed | must-fix-003 と統合対応で 3 Dockerfile から `\|\| true` を削除。`deploy/aws/docker/title-proxy.Dockerfile:38` に残置していたが Round 3 r3-must-fix-001 で削除済み。 |
| should-fix-008 / 009 | partially-fixed(CI/workspace dep は 17g で部分対応済み。網羅的整備は v0.1.3) | |
| nitpick-001..005, 007 | wontfix(`.dockerignore` 絞り込み・workspace order・Anchor scripts・wallet path 等は v0.1.3 OSS 公開前整理) | |
| nitpick-006 | fixed | `deploy/aws/scripts/run-stack.sh:87-101` で `TEE_READY=0/1` フラグ + 60s timeout 後 `exit 1` を実装済み。Round 2 処理ログでは `wontfix` と書いたが実際は fix 完了済みであり、Round 3 監査で訂正。 |
| new-must-fix-001 / new-should-fix-001..003 / new-nitpick-001..004 | wontfix(CI 詳細化 (`--locked` / clippy `-D warnings` / 各 toolchain カバー) は CI 整備フェーズで一括対応) | |
