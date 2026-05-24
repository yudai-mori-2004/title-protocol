# E. 再現性・ビルド品質

## 概要

担当範囲: `Cargo.toml` / `Cargo.lock` / `rust-toolchain.toml` / 全 `crates/*/Cargo.toml` / `sp1-guests/**/Cargo.toml` / `programs/title-whitelist/Cargo.toml` / `docker/**/*.Dockerfile` / `docker-compose.yml` / `.dockerignore` / `deploy/aws/**/*.tf` / `deploy/aws/**/*.sh` / `Anchor.toml` / ルート `.gitignore`。

監査方針: 仕様書 §5.4「リプロデューシブルビルド」の要件（ソースコード公開、ビルド手順公開、依存ライブラリのバージョン固定、Rust コンパイラとターゲットアーキの指定）を Source of Truth とし、検証者が clone → build → ハッシュ照合できる状態かを 1 文ずつ精査した。

件数: **23 件**

## 重大度別内訳

- must-fix: 6 件
- should-fix: 10 件
- nitpick: 7 件

## 発見

### must-fix-001 `crates/proxy/Cargo.toml` の `vendor-aws` デフォルト feature が非 Linux で破綻する

- 場所: `crates/proxy/Cargo.toml:14-18, 28-29`
- 観察:
  ```toml
  [features]
  default = ["vendor-aws"]
  vendor-aws = ["dep:vsock"]
  ...
  [target.'cfg(target_os = "linux")'.dependencies]
  vsock = { version = "0.5", optional = true }
  ```
- 問題: `dep:vsock` 参照は target 条件を考慮せずに評価されるため、Mac/Windows の開発者が `cargo build --workspace` を叩いた瞬間「feature `vendor-aws` includes `dep:vsock`, but `vsock` is not an optional dependency」相当のエラーで止まる。仕様書 §5.4 の「同一手順で誰でも再現」が成立しない。実機（Linux/amd64）でしか検証されていないことの傍証でもある。
- 修正案: `default = []` に落とし、`title-proxy.Dockerfile:38` で `--features vendor-aws` を明示的に渡す（`tee` クレートが同じ方針を取っているのと整合）。あるいは `vendor-aws` を「リスト無し」にして、コード側だけ `cfg(target_os = "linux")` で分岐する。

### must-fix-002 SP1 guest 用 `sha2_sp1` が git branch 指定で再現性が崩れる

- 場所: `crates/attestation-aws-nitro/Cargo.toml:34`
- 観察:
  ```toml
  sha2_sp1 = { git = "https://github.com/sp1-patches/RustCrypto-hashes", branch = "patch-sha2-v0.10.8", package = "sha2", features = ["oid"], optional = true }
  ```
- 問題: 現在は `Cargo.lock` がコミット時点のハッシュ `1f224388...` を保持しているが、誰かが `cargo update -p sha2_sp1` を実行すれば branch HEAD が黙って新しい commit に進む。仕様書 §5.4 の「依存ライブラリのバージョン固定」を最弱の鎖が破る。同ファイル `p256_sp1` は `rev = "patch-p256-13.2-sp1-5.0.0"` を使えているので同じ方式に揃えるべき。
- 修正案: 該当 branch の最新タグまたは commit に固定。例:
  ```toml
  sha2_sp1 = { git = "https://github.com/sp1-patches/RustCrypto-hashes", rev = "1f224388fdede7cef649bce0d63876d1a9e3f515", package = "sha2", features = ["oid"], optional = true }
  ```

### must-fix-003 開発用 Dockerfile が workspace member `proxy` の manifest をコピーしない

- 場所: `docker/gateway.Dockerfile:11-17`, `docker/tee-mock.Dockerfile:11-17`
- 観察: ルート `Cargo.toml:2-11` は `crates/proxy` を `[workspace] members` に含めるが、両 Dockerfile はその `Cargo.toml` を COPY せず、stub stage では `crates/proxy/` 自体が存在しない。`RUN cargo build ... 2>&1 || true` (gateway.Dockerfile:28) が失敗を握りつぶしているため気付きにくいだけで、依存キャッシュ層は実質無効化されている（後段の `COPY crates/ crates/` でようやく解決する）。
- 問題: 「Cargo の依存キャッシュ最適化」の意図が壊れている上、`|| true` がエラーを隠すので将来 workspace 構成が変わった時にも検出できない。`docker compose up` ごとに依存解決が重複してビルド時間が膨らみ、再現ビルド時の I/O も無駄に増える。
- 修正案: 両 Dockerfile に以下を追加:
  ```dockerfile
  COPY crates/proxy/Cargo.toml crates/proxy/Cargo.toml
  ```
  および stub source 行に `&& mkdir -p crates/proxy/src && echo "fn main() {}" > crates/proxy/src/main.rs` を追加。あわせて `|| true` を外し、stub build の失敗を露見させる。

### must-fix-004 Terraform AMI 解決が `most_recent = true` で時間によって変動

- 場所: `deploy/aws/terraform/main.tf:39-57`
- 観察:
  ```hcl
  data "aws_ami" "al2023" {
    most_recent = true
    owners      = ["amazon"]
    filter { name = "name" values = ["al2023-ami-*-x86_64"] }
    ...
  }
  ```
- 問題: 同じ `terraform apply` を別日に走らせると別の AMI ID が返る。EC2 ホスト側に焼き付く kernel / Docker / nitro-cli のバージョンが日替わりで変わり、user-data.sh の `dnf update -y` (user-data.sh:20) と組み合わさって host 環境が再現不能になる。Enclave 内の PCR0 は影響を受けないが、build-enclave 時の `nitro-cli` の挙動差異（PCR8 を含む将来拡張）に当たる可能性がある。
- 修正案:
  ```hcl
  variable "al2023_ami_id" {
    description = "Pinned Amazon Linux 2023 AMI for the deployment region. Update consciously."
    type        = string
    default     = "ami-XXXXXXXXXXXXXXXXX"  # 現在 production にデプロイしているもの
  }
  resource "aws_instance" "node" {
    ami = var.al2023_ami_id
    ...
  }
  ```
  data ブロックは「現行 production AMI を一覧で確認したいとき」用に残しても良いが、`aws_instance.node.ami` からは参照しない。

### must-fix-005 `deploy/aws/terraform/.gitignore` が `.terraform.lock.hcl` を除外している

- 場所: `deploy/aws/terraform/.gitignore:5`
- 観察:
  ```
  *.tfstate
  *.tfstate.*
  .terraform/
  .terraform.lock.hcl
  tfplan
  ```
- 問題: HashiCorp の公式ガイダンスは `.terraform.lock.hcl` を**コミットせよ**である。lock 無しだと clone した別開発者が `terraform init` するたびに provider バージョンが新しい patch にスライドし得る（`~> 5.0` 制約は 5.x 全部許容）。現在ローカルでは aws 5.100.0 / tls 4.3.0 / local 2.9.0 が選ばれているが、これを共有できない。仕様書 §5.4 の精神（環境固定）に反する。
- 修正案: `.terraform.lock.hcl` の行を削除し、現在のファイル（`deploy/aws/terraform/.terraform.lock.hcl`）をコミット。`tfplan` も実は機密を含むため `*.tfplan` のほうが安全（出力ファイル名拡張子可変のため）。

### must-fix-006 `terraform.tfstate` / `terraform.tfstate.backup` が working tree に存在する

- 場所: `deploy/aws/terraform/terraform.tfstate`, `deploy/aws/terraform/terraform.tfstate.backup`
- 観察: `.gitignore` は除外しているが、ファイル実体は repo 内に置かれており、過去に誤コミットしていれば履歴に残る可能性がある。state には `tls_private_key.ssh.private_key_openssh`（生のSSH秘密鍵 PEM）と AWS リソースの内部 ARN が平文で入る。
- 問題: ローカル backend で state を扱う設計だと多人数オペレーションで state lock が無く、`terraform apply` の競合で state 破壊のリスク。OSS 公開を見据えるなら remote backend + state locking が事実上必須。
- 修正案: `git log --all -- deploy/aws/terraform/terraform.tfstate` で履歴混入を確認し、混入していれば即座に SSH 鍵ローテーション。仕様書とは別文書 (`deploy/aws/README.md` に追記) で「state を共有したい場合の手順」として s3 backend + DynamoDB lock を例示:
  ```hcl
  terraform {
    backend "s3" {
      bucket         = "title-protocol-tfstate"
      key            = "v0.1.2/devnet.tfstate"
      region         = "ap-northeast-1"
      dynamodb_table = "title-protocol-tfstate-lock"
    }
  }
  ```
  当面の dev 用途で local backend のままなら、最低限 README に「同時実行禁止」を明記。

### should-fix-001 SP1 guest / host が独立 workspace なのに `Cargo.lock` をコミットしていない

- 場所: `sp1-guests/attestation-aws-nitro/host/Cargo.toml:10` (`[workspace]`), `sp1-guests/attestation-aws-nitro/program/Cargo.toml:9` (`[workspace]`)
- 観察: 両クレートとも独自 workspace なのに、ディレクトリには `Cargo.lock` が無い。
- 問題: 仕様書 §6.2 で SP1 vkey_hash を Solana プログラムに埋め込む設計のため、guest binary は bit-for-bit 再現できないと chain 上の whitelist と整合しなくなる。lock が無いと検証者が `cargo run --bin vkey` を実行する度に異なる依存解決になり、vkey_hash が変わる可能性がある。host 側 (`prove.rs`) も同様で、SP1 SDK のマイナー更新で証明形式が変わると downstream の Solana 検証が破綻する。
- 修正案: 両ディレクトリで `cargo generate-lockfile` 後、`Cargo.lock` をコミット。`.gitignore` 直下に `!sp1-guests/**/Cargo.lock` を明示しておくと将来の事故を防げる。

### should-fix-002 `sp1-guests/` と `programs/title-whitelist/` に `rust-toolchain.toml` が無い

- 場所: ルート `rust-toolchain.toml`, `sp1-guests/attestation-aws-nitro/{host,program}/`, `programs/title-whitelist/`
- 観察: ルート workspace は `channel = "1.93.1"` で固定されているが、`[workspace]` を切っている SP1 guest/host と Anchor program は対象外（rustup は最も近い親の `rust-toolchain.toml` を見つける挙動なので一見継承されるが、ディレクトリを単独で配布した時に効かなくなる）。
- 問題: 仕様書 §5.4 の「Rust コンパイラのバージョン指定」を満たすために、ビルド単位ごとに toolchain を独立宣言しておく方が安全。特に SP1 host は `sp1-build` が裏で `cargo prove` を呼ぶため、guest 側 toolchain は別途 SP1 が管理するが host 側の channel は明示しておくべき。
- 修正案: SP1 host に
  ```toml
  [toolchain]
  channel = "1.93.1"
  ```
  Anchor プログラムは Solana 系の理由で別 channel になりがちなので、必要な channel を明記（例: `channel = "1.79.0"` 等、現状ビルドが通っているもの）。

### should-fix-003 `crates/attestation-aws-nitro/Cargo.toml` の `p256` 指定が workspace ポリシーから外れている

- 場所: `crates/attestation-aws-nitro/Cargo.toml:24` (`p256 = "0.13"`), `crates/crypto/Cargo.toml:14` (`p256 = { version = "0.13.2", features = ["ecdh"] }`)
- 観察: 同じ crate を 2 クレートが別書式で指定。lock 上は同 0.13.2 に解決されているが、片方は `"0.13"` (≥ 0.13.0, < 0.14)、片方は `"0.13.2"` (≥ 0.13.2, < 0.14) と semver 範囲が異なる。
- 問題: 将来 0.13.3 が出た時に振る舞いが crate ごとに微妙に分かれ得る。workspace dep 化していないため版数ドリフトの検出も難しい。
- 修正案: ルート `Cargo.toml [workspace.dependencies]` に `p256 = "0.13.2"` を追加し、両 crate から `p256 = { workspace = true }` ないし feature 指定込みで参照する。同様に `sha2` も既に workspace dep だが attestation-aws-nitro:33 だけ別書式で `features = ["oid"]` 指定なので、workspace dep 側に oid を入れて統一する。

### should-fix-004 Dockerfile base image の `bookworm` / `rust:1.93-bookworm` がパッチ更新で揺れる

- 場所: `docker/gateway.Dockerfile:5,36`, `docker/tee-mock.Dockerfile:5,36`, `deploy/aws/docker/tee-nitro.Dockerfile:11,47`, `deploy/aws/docker/title-proxy.Dockerfile:12,45`
- 観察: `rust:1.93-bookworm` は Debian Bookworm パッチ更新のたびに base layer が変わる。`debian:bookworm-slim` も同様。`rust-toolchain.toml` が rustc を 1.93.1 に固定するので Rust 側のドリフトは止まるが、glibc / OpenSSL / ca-certificates のバージョンが build 日によって変わる。
- 問題: 仕様書 §5.4 は「同一バイナリ」を要求。bookworm のセキュリティパッチでバイナリの埋め込みパスや動的依存が変われば、`title-tee` の SHA-256 ≒ Enclave PCR0 が変動する可能性がある。
- 修正案: digest ピン留め。例:
  ```dockerfile
  FROM rust:1.93.1-bookworm@sha256:<digest> AS builder
  ...
  FROM debian:bookworm-slim@sha256:<digest>
  ```
  digest 更新は `docs/v0.1.2/OPERATIONS_JA.md` に運用手順として追記。

### should-fix-005 Dockerfile の `RUN cargo build ... 2>&1 || true` がエラーを握りつぶす

- 場所: `docker/gateway.Dockerfile:28`, `docker/tee-mock.Dockerfile:28`, `deploy/aws/docker/tee-nitro.Dockerfile:35-37`, `deploy/aws/docker/title-proxy.Dockerfile:38`
- 観察: stub build 段で `|| true` で常に成功扱い。コメント「Warm dep cache (no-op if all deps are unchanged across builds)」(tee-nitro.Dockerfile:34) の前向き効果はあるが、副作用として workspace の構成ミスや真のビルド不能を隠す。
- 問題: must-fix-003 の `proxy` 漏れがまさにこの `|| true` のせいで実害として表面化していなかった。stub build がコケた瞬間 docker build が止まる方が再現性監査の精度は高い。
- 修正案: `|| true` を外す。stub stage で全 manifest と空ソースを揃えてあれば cargo build は成功する。失敗するなら manifest 列挙が間違っているサインなので、人間に気づかせるべき。

### should-fix-006 `user-data.sh` が初回ブートで `dnf update -y` を打つ

- 場所: `deploy/aws/terraform/user-data.sh:20`
- 観察: `dnf update -y` をテンプレート無しで実行。Amazon Linux 2023 のリポジトリは更新が頻繁で、kernel・glibc・OpenSSL・nitro-cli の patch level が日替わりで変わる。
- 問題: ホスト側パッケージは Enclave PCR には入らないが、`nitro-cli build-enclave` の出力差（メタデータ・cmdline）に影響し得る。実機 PCR0 を再現する手順を OPERATIONS にしている以上、ホスト側もスナップショット化すべき。
- 修正案: 短期対処として更新を最小限に（`dnf install -y aws-nitro-enclaves-cli aws-nitro-enclaves-cli-devel docker socat jq tmux` のみで `dnf update -y` を削除）。中期で AMI を「Title Protocol が動く既知良品」として独自に作り、`packer` ビルドのハッシュごと記録する運用が望ましい。

### should-fix-007 `Anchor.toml` の `[scripts] test` が無効

- 場所: `Anchor.toml:20-21`
- 観察:
  ```toml
  [scripts]
  test = "echo 'Use: cargo test --workspace'"
  ```
- 問題: `anchor test` の標準フローを完全に潰している。CI で `anchor build && anchor test` を走らせる運用が一切できない。OSS 利用者が IDL を再生成しようとすると `idl-build` feature の workaround も含めて指示が無い。
- 修正案: `Anchor.toml` の `[scripts]` を実体に整える（`anchor test` を Solana program のテスト、`cargo test --workspace` は別途 root で実行）。あるいは `Anchor.toml` から `[scripts]` 自体を消し、`deploy/aws/README.md` か新規 `programs/title-whitelist/README.md` に
  ```bash
  anchor build --no-idl     # IDL 生成は別途
  anchor build --features idl-build  # IDL 必要時
  ```
  と明記する。

### should-fix-008 CI/CD パイプラインがリポジトリに存在しない

- 場所: 該当無し（`/Users/forest/WebCreations/title-protocol/.github/` が存在しない）
- 観察: workflow ファイル一切無し、Jenkinsfile も無し、`.circleci/` も無し。`docker/smoke-test.sh` というローカル smoke はあるが、PR ごとに走る保証が無い。
- 問題: 仕様書 §5.4 が要求する「同じソース → 同じバイナリ」を CI で継続検証していないと、依存更新や手元 cargo update で再現性が壊れた瞬間に気付けない。Cargo.lock 更新だけでも PR が腐る。
- 修正案: 最低限 `.github/workflows/ci.yml` で `cargo fmt --check`、`cargo clippy --workspace -- -D warnings`、`cargo test --workspace`、`docker compose up --build -d && bash docker/smoke-test.sh` を走らせる。reproducible build 検証として `docker build` 後にバイナリ SHA を artifact で残し、`main` の毎 push に対して比較する仕組みも追加できる。

### should-fix-009 `crates/proxy/Cargo.toml` が proxy 用の `[workspace.dependencies]` を活用していない

- 場所: `crates/proxy/Cargo.toml:21-26`
- 観察: `reqwest`, `tokio`, `tracing`, `tracing-subscriber`, `anyhow` を crate ローカルで個別宣言。`reqwest` だけは workspace dep が存在する (`Cargo.toml:34`) のに参照していない。
- 問題: workspace dep を活用しないと、他 crate (`gateway` / `tee` 等) と reqwest の feature 合成が分かれ、cargo がビルドユニットを別物として扱う可能性がある。再現性的にも整合性的にも見通しが悪い。
- 修正案: workspace dep `reqwest` に `stream` feature を追加し、proxy 側は `reqwest = { workspace = true, features = ["stream"] }`。tokio / tracing も workspace dep 化を検討。

### should-fix-010 `[profile.release]` に `overflow-checks` 以外の reproducibility 指定が無い

- 場所: `Cargo.toml:48-49`, `programs/title-whitelist/Cargo.toml:24-25`
- 観察: `overflow-checks = true` のみ。`debug`、`lto`、`codegen-units`、`strip`、`panic` 指定無し。
- 問題: rustc は `codegen-units` 未指定だと default の 16 で並列分割し、スレッド数や入力順により出力バイナリが微妙に変わるケースがある（特にデバッグ情報のセクション順）。再現性を仕様で謳う以上、`codegen-units = 1` と `lto` を明示するのが定石。
- 修正案:
  ```toml
  [profile.release]
  overflow-checks = true
  codegen-units = 1
  lto = "fat"
  strip = "symbols"
  panic = "abort"   # TEE は panic 即落としで良いはず（要確認）
  ```
  特に panic = "abort" は §5.3 の attestation policy と整合する必要があるため要レビュー。

### nitpick-001 ルート `.dockerignore` が build context を十分に絞れていない

- 場所: `.dockerignore` (5 行)
- 観察: `target/`, `legacy/`, `programs/`, `docs/`, `.git/` のみ。`deploy/`, `keys/`, `node_modules/`, `*.eif`, `Cargo.lock` 以外の `.lock`、IDE 系 (`.idea/`, `.vscode/`) を除外していない。
- 問題: docker build が無駄に大きいコンテキストを送る。`keys/` が context に乗ると CI ログにファイル名が出る可能性。
- 修正案:
  ```
  target/
  legacy/
  programs/
  docs/
  .git/
  deploy/
  keys/
  node_modules/
  *.eif
  .idea/
  .vscode/
  *.swp
  .env*
  ```

### nitpick-002 workspace member の並びがアルファベット順でない

- 場所: `Cargo.toml:2-11`
- 観察: `attestation`, `attestation-aws-nitro`, `core`, `crypto`, `tee`, `gateway`, `proxy`, `solana`。`tee` が `gateway` より前に来ている。
- 問題: 探しにくい、レビューで diff が読みにくい。
- 修正案: アルファベット順に並び替え (`attestation`, `attestation-aws-nitro`, `core`, `crypto`, `gateway`, `proxy`, `solana`, `tee`)。`[workspace.dependencies]` の crate 参照 (line 39-46) も同じ並びに揃える。

### nitpick-003 Dockerfile 内コメントの「Spec §5.4」リファレンスが浅い

- 場所: `docker/gateway.Dockerfile:2`, `docker/tee-mock.Dockerfile:2`, `deploy/aws/docker/tee-nitro.Dockerfile:3`
- 観察: 全 Dockerfile が `# Spec §5.4 — Reproducible build via Cargo.lock + rust-toolchain.toml` という同文を冒頭に持つ。
- 問題: 同じ文を 3 ファイルに貼ると「形だけ」感が出る上、上記 should-fix-004 で base image digest を固定していない実態と齟齬がある（Cargo.lock + rust-toolchain だけでは再現性は完成しない）。
- 修正案: いずれか:
  - digest 固定まで踏み込んでから残す。
  - 当面は「Cargo.lock + rust-toolchain.toml を尊重する」程度のニュアンスに弱める: `# Reproducible build inputs: Cargo.lock + rust-toolchain.toml. Base image digest pinning is TODO.`
  - 重複コメントを 1 箇所 (`docs/v0.1.2/OPERATIONS_JA.md`) に集約し、Dockerfile はリンクのみ。

### nitpick-004 `programs/title-whitelist/Cargo.toml` が `authors` を workspace 経由で取らない

- 場所: `programs/title-whitelist/Cargo.toml:1-8`
- 観察: workspace dep のメリットを享受せず、`version`, `edition`, `license`, `repository`, `authors`, `description` を全部ベタ書き。`[workspace]` が無いため `[workspace.package]` を継承できない事情はある（programs は exclude 対象）。
- 問題: ルート `[workspace.package]` のバージョンを 0.1.3 に上げる時、ここを忘れる。
- 修正案: programs ディレクトリにも `[workspace]` を切って独自 workspace 化し、その `[workspace.package]` を共有する。あるいはルート CI に「version 番号一致チェック」スクリプトを追加。

### nitpick-005 `docker/smoke-test.sh` が `docker compose` を呼んでいない

- 場所: `docker/smoke-test.sh:1-10`
- 観察: ヘッダコメント (line 4-7) に `docker compose up --build -d` を別途実行する前提と書かれているが、スクリプト自体は up/down を一切呼ばず curl チェックだけ行う。利用者は 3 コマンドを順に打つ必要がある。
- 問題: 「smoke test」を名乗る以上ワンショットが期待される。
- 修正案: スクリプト冒頭に
  ```bash
  trap 'docker compose down' EXIT
  docker compose up --build -d
  ```
  を加え、`./docker/smoke-test.sh` 単独で完結させる。

### nitpick-006 `run-stack.sh` の `for i in {1..60}` が冪等性を主張するが timeout 失敗時の戻り値が曖昧

- 場所: `deploy/aws/scripts/run-stack.sh:64-70`
- 観察: TEE HTTP の起動待ちで 60s 経過しても、エラーで止めずに次の Gateway 起動に進む（`break` しないだけで loop は終わる）。
- 問題: コメントは「Idempotent: stops any running stack before starting a fresh one」(line 8) と謳うが、起動失敗の検出が抜けているため「Gateway は上がったが TEE は死んでいる」状態で正常終了することがある。run の戻り値で運用判断ができない。
- 修正案:
  ```bash
  for i in {1..60}; do
    if curl -sf http://127.0.0.1:4000/health > /dev/null 2>&1; then
      echo "    TEE ready (${i}s)"
      tee_ready=1
      break
    fi
    sleep 1
  done
  if [[ -z "${tee_ready:-}" ]]; then
    echo "FAIL: TEE did not become ready within 60s"
    sudo nitro-cli describe-enclaves
    exit 1
  fi
  ```

### nitpick-007 `Anchor.toml` の `wallet = "~/.config/solana/id.json"` がユーザ依存

- 場所: `Anchor.toml:18`
- 観察: ホームディレクトリ直書きでクローン者ごとに違うパスを期待。
- 問題: OSS 観点で初見ハードルが上がる（持っていない人は anchor を実行できない）。
- 修正案: `deploy/aws/keys/dev-wallet.json` のような repo 内パスをデフォルトにし、無ければ「生成方法を `deploy/aws/README.md` に書く」運用に。あるいは `Anchor.toml` を `.example` 化して `cp Anchor.toml.example Anchor.toml` 案内する。

## 全体所感

仕様書 §5.4 は要件として「ソース公開」「ビルド手順公開」「依存固定」「環境指定」の 4 点を挙げているが、現状は名目上はすべて満たしている一方で、深い穴が随所にある：

1. **依存固定**: registry crate は OK、workspace dep の活用は OK、しかし `sha2_sp1` の git branch 指定と SP1 guest/host の `Cargo.lock` 未コミットが致命的に再現性を損なう（must-fix-002 / should-fix-001）。
2. **環境指定**: rust-toolchain.toml はルートのみ。Dockerfile の base image は moving tag。Terraform AMI は `most_recent = true`。`dnf update -y` で host も流動的。Enclave PCR0 まで含めると現状の運用は「同じ日に同じマシンでビルドすれば多分一致する」レベル（must-fix-004 / should-fix-004 / should-fix-006）。
3. **ビルド手順公開**: `deploy/aws/README.md` の流れは良い。だが Dockerfile が workspace の `proxy` member を取り違えており、cache 層が空転している事実が長らく検出されなかった（must-fix-003）。CI が無いことが質保証の最大の穴（should-fix-008）。
4. **OSS 観点の即時 onboarding**: terraform.tfstate のローカル管理、Anchor.toml の wallet パス、`.terraform.lock.hcl` の gitignore など、クローンしてすぐ動かそうとする人を躓かせる地雷が多い（must-fix-005, must-fix-006, nitpick-007）。

修正の優先順位は: must-fix-001（macOS 開発者が今すぐ詰む）→ must-fix-002 / should-fix-001（vkey_hash 再現性の根幹）→ must-fix-003（Dockerfile 整合）→ must-fix-005 / -006（OSS 化前に必須）→ 残り should-fix → CI 構築（should-fix-008）。CI を組めば残りの nitpick も自然に検出されるようになる。
