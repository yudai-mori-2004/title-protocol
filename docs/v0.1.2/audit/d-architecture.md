# D. アーキテクチャ・ディレクトリ構造

## 概要

担当範囲: workspace 構造（`Cargo.toml`）、`crates/`・`programs/`・`sp1-guests/`・`deploy/`・`docker/`・`docs/`・`keys/`・`legacy/` の配置、各 crate の責務・境界・公開 API、ファイルサイズ・多責務、命名、依存方向。

監査方針: 仕様書 §5.1（構成）と実装を照合し、(1) crate 境界の責務漏れ、(2) ベンダー中立性、(3) 循環/逆方向依存、(4) 多責務ファイル、(5) 命名揺れ を 1 ファイルずつ確認した。具体的に「もし AMD SEV-SNP / Intel TDX を追加するとき何ファイルを触るか」「もし新しい processor を 1 つ追加するとき何 crate に手を入れるか」をシミュレートして judgement の根拠とした。

リポジトリ全景（`target` / `node_modules` / `.git` を除外）：

```
title-protocol/
├── Cargo.toml                # workspace = 8 crates + 3 exclude
├── Anchor.toml               # Anchor (programs/)
├── rust-toolchain.toml
├── docker-compose.yml
├── crates/
│   ├── core/                 # Processor trait, request/response, c2pa_verify, jumbf
│   ├── crypto/               # KEM/AEAD/HKDF/wire/payload/sealed_channel/key_bundle
│   ├── attestation/          # vendor-agnostic AttestationVerifier trait
│   ├── attestation-aws-nitro/# AWS Nitro 実装（cose/cert/sign/doc）
│   ├── tee/                  # TeeRuntime trait + axum server + orchestrator
│   │   ├── runtime/mock.rs
│   │   └── vendor/aws.rs     # vendor-aws feature gated
│   ├── solana/               # signing_key + whitelist + cnft + extension
│   ├── gateway/              # axum gateway + auth + rate_limit + tee_client
│   └── proxy/                # vsock/tcp HTTP forwarder
├── programs/title-whitelist/ # Anchor program (single 728-line lib.rs)
├── sp1-guests/attestation-aws-nitro/{host,program}
├── deploy/aws/{terraform,docker,scripts,keys}
├── docker/                   # gateway.Dockerfile + tee-mock.Dockerfile
├── docs/v0.1.{0,1,2}/
├── keys/admin.json           # 64-byte ed25519 keypair, in repo
└── legacy/v0.1.0/            # 旧コード一式
```

## 重大度別内訳

- must-fix: 5 件
- should-fix: 12 件
- nitpick: 6 件

合計 23 件。

## 発見

### must-fix-001 `keys/admin.json` がリポジトリにコミットされた秘密鍵

- 場所: `keys/admin.json` (1 行 64 整数 = Ed25519 シード+公開鍵)
- 観察: ファイルの末尾 32 バイト
  `14,13,85,28,133,146,12,228,...,103,125,184,3`
  が `programs/title-whitelist/src/lib.rs:35-38` の `ADMIN_AUTHORITY` 定数と完全一致する。前半 32 バイトはこの公開鍵に対応する Ed25519 秘密鍵シード。
- 問題: アーキテクチャ的に `keys/` は「リポジトリの一部としてある必要のないもの」が置かれている。`ADMIN_AUTHORITY` は whitelist program の `add_approved_vkey` / `add_approved_measurement` / `revoke_key` などすべての admin 操作の唯一の認可主体（programs/title-whitelist/src/lib.rs:543, 575, 635）。これを clone できる者は誰でも whitelist を改竄できる。OSS 公開直前にこの状態のままだと、後段の鍵ローテーション無しには即座に program ownership を失う。
- 修正案: (a) `keys/admin.json` をリポジトリから削除し `git filter-repo` で履歴も消す、(b) `.gitignore` に `keys/` `*.json` の admin/operator 系を追加、(c) `keys/README.md` に「ここには鍵を置かない。デプロイ手順は deploy/aws/scripts/ を参照」だけを残す、(d) admin pubkey は別途 `programs/title-whitelist/Anchor.toml` か env で注入する設計に変更（現状 hard-coded `[u8; 32]` も spec §6.2 の「人手による管理は介在しない」と齟齬がある旨を別途 G 担当で確認）。

### must-fix-002 `title-tee` crate が `title-solana` に静的依存している

- 場所: `crates/tee/Cargo.toml:38` `title-solana = { workspace = true }`、`crates/tee/src/server.rs:29-30, 51, 79-80, 193-267`、`crates/tee/src/main.rs:22, 99, 177`
- 観察: TEE crate がコアの処理ループに `title_solana::extension::process_extension`・`SolanaSigningKey`・`/solana-keys` `/extension/solana` ルートを直接組み込んでいる。`title_solana` は cargo の workspace 必須 dep として `title-tee` から無条件に取り込まれる。
- 問題: 仕様書 §6.1 は「Extension はコアとは別のレイヤー」「Solana Extension は §6.2、コア処理は §1–§5」と明確に分離しているのに、コード上は TEE バイナリに必ず Solana が同梱される構造になっており、Extension の差し替え（将来の Ethereum 等）や Extension 無効化ビルドができない。「TEE = コアハードウェア抽象 + コアパイプライン」「Extension = 別 crate がフックする」という仕様の責務境界が壊れている。さらに `crates/gateway/src/endpoints.rs:138-179` で「Solana Extension が無効の場合は 404」と書いてあるのに、TEE 側では常に有効なため Gateway の `cache.solana_keys.is_none()` 分岐が事実上 dead branch になっている。
- 修正案: (a) `title-tee` から `title-solana` 依存を外し、Solana ルートを `cargo feature = "extension-solana"` で gating する、もしくは (b) `crates/tee-server` を新設して `tee-core + extension-solana` を組み合わせるエントリ専用 crate にし、`title-tee` 自体は trait + pipeline までに限定する。仕様 §6.1 の「コアと Extension を別リクエストにする」を crate 境界にも反映させる。

### must-fix-003 「aws-nitro」識別子の三重定義と表記揺れ

- 場所:
  - `crates/attestation-aws-nitro/src/lib.rs:43` `pub const VENDOR: &str = "aws-nitro";`
  - `crates/tee/src/vendor/aws.rs:133-134` `fn tee_type() { title_attestation_aws_nitro::VENDOR }`（参照、OK）
  - `crates/tee/src/lib.rs:73` doc コメント `// 例: "aws_nitro", "amd_sev_snp", "intel_tdx", "mock"`（**アンダースコア表記**）
  - `crates/gateway/src/lib.rs:212` テスト `tee_type: Some("aws_nitro".into())`（**アンダースコア表記**）
  - `crates/tee/src/main.rs:42-82` 起動時の env キー `"nitro"` / supported.push(`"nitro"`)（**3 つ目の表記**）
- 問題: 同じ「AWS Nitro」を指す識別子が `"aws-nitro"`（VENDOR 定数）/ `"aws_nitro"`（doc + テスト）/ `"nitro"`（env key）の 3 系統で混在しており、`TeeRuntime::tee_type` の値と `TEE_RUNTIME` env の値が一致しない。ベンダー追加時に毎回どこを直すべきか正解がない。
- 修正案: (a) `crates/attestation/src/lib.rs` に `pub mod vendor_tags { pub const AWS_NITRO: &str = "aws-nitro"; pub const AMD_SEV_SNP: &str = "amd-sev-snp"; pub const INTEL_TDX: &str = "intel-tdx"; pub const MOCK: &str = "mock"; }` を置く、(b) `main.rs` の env パースを `match s { "aws-nitro" | "nitro" => ..., }` に統一（後方互換は alias で）、(c) `crates/tee/src/lib.rs:73` の例コメントを更新、(d) `crates/gateway/src/lib.rs:212` のテストを `vendor_tags::AWS_NITRO` 参照に置き換える。

### must-fix-004 巨大な単一ファイル: `crates/tee/src/orchestrator.rs` 1185 行

- 場所: `crates/tee/src/orchestrator.rs`（本体 372 行 + テスト 813 行）
- 観察: 一ファイルに (1) `OrchestratorError`、(2) `ProcessOutcome` enum、(3) `process_request` のステップ 1–11、(4) `decrypt_single_payload` の暗号サブパイプライン、(5) `ensure_c2pa_verify` / `execute_processors` / `compute_jcs_hash` / `build_attested_response` のヘルパー、(6) 4 つの mock 実装（MockFetcher, MockRuntime, JPEG 生成, 署名）、(7) 21 個のテスト関数 が同居する。
- 問題: 「1 ファイル 1 責務」原則からも、`v0.1.2` の本番化フェーズで読み手が最初に開くだろうコア crate のコアファイルが 1185 行という規模は読みにくく、修正の衝突も起こりやすい。さらにテスト用 `create_signed_jpeg` のような fixture が `crates/tee/src/orchestrator.rs:474` と `crates/tee/src/server.rs:520` と `crates/gateway/tests/e2e.rs` の 3 箇所で複製されている可能性が高い（要確認）。
- 修正案: 以下に分割:
  - `crates/tee/src/orchestrator/mod.rs` — `process_request` + `ProcessOutcome`
  - `crates/tee/src/orchestrator/error.rs` — `OrchestratorError`
  - `crates/tee/src/orchestrator/encryption.rs` — `decrypt_single_payload`
  - `crates/tee/src/orchestrator/attest.rs` — `compute_jcs_hash` + `build_attested_response`
  - `crates/tee/tests/fixtures.rs` 共通モジュール — `create_signed_jpeg`、`MockFetcher`、`MockRuntime` を集約

### must-fix-005 巨大な単一ファイル: `programs/title-whitelist/src/lib.rs` 728 行

- 場所: `programs/title-whitelist/src/lib.rs`
- 観察: Anchor の `#[program] mod` + `verify_sp1_groth16` 関数 + `parse_public_values` パーサ + `StoredMeasurement` 型 + 5 つの `#[account]` 構造体 + 5 つの `#[derive(Accounts)]` Context + 8 つの `#[event]` + `WhitelistError` 全てが同一ファイル。
- 問題: Anchor では単一クレート制約のため `mod`/`lib.rs` 分割でも 1 ファイルに集約しがちだが、Anchor 自身は `pub mod state; pub mod instructions::register_key;` 等の分割を推奨している。読みにくさは spec §6.2 を実装と並べて読むときに顕著（vkey / measurement / register / revoke が縦に長く並ぶ）。
- 修正案: 以下に分割（Anchor 標準パターン）:
  - `src/lib.rs` — `declare_id!`, `mod`, `#[program]` の各 instruction エントリだけ
  - `src/state.rs` — `WhitelistEntry`, `ApprovedVkeys`, `ApprovedMeasurements`, `StoredMeasurement`
  - `src/errors.rs` — `WhitelistError`
  - `src/events.rs` — 8 つの event
  - `src/instructions/register_key.rs` — `RegisterKey` Context + `verify_sp1_groth16` + `parse_public_values`
  - `src/instructions/admin.rs` — vkey / measurement / revoke 系

### should-fix-001 `crates/attestation/Cargo.toml` の `mock` feature 設計が attestation-aws-nitro と一貫しない

- 場所: `crates/attestation/Cargo.toml:10-15`、`crates/tee/Cargo.toml:23` (`runtime-mock = ["title-attestation/mock"]`)
- 問題: `title-attestation/mock` は production ビルドに混入しないよう default off だが、`crates/tee` 側では `runtime-mock` を有効にすると `MockAttestationVerifier` も常にコンパイルされる。一方 `vendor-aws` 側は同様の `vendor-aws = [... "dep:title-attestation-aws-nitro" ...]` で attestation crate 依存をコントロールするが、`title-attestation/mock` 経由のみで mock verifier が触れるという経路が分かりにくい。命名規則（`runtime-mock` ↔ `title-attestation/mock`）の対称性も崩れている。
- 修正案: `title-attestation` の `mock` feature を `mock-verifier` に rename、`title-tee` の `runtime-mock` を `["title-attestation/mock-verifier"]` に揃える。doc string で「production には feature 無効を必須にする」旨を明示。

### should-fix-002 `crates/proxy/Cargo.toml` の `vendor-aws` feature と `title-tee` の `vendor-aws` feature が同名だが意味が違う

- 場所: `crates/proxy/Cargo.toml:14-18` と `crates/tee/Cargo.toml:24-31`
- 問題: 両方 `vendor-aws` だが、proxy は「vsock を有効にする」、tee は「NitroRuntime と NSM API をリンクする」を意味する。組み合わせビルドを誤ると「proxy だけ TCP、tee だけ Nitro」のような不整合状態を許す。
- 修正案: proxy 側を `vsock-listener` / `nitro-vsock` 等に rename、もしくは workspace 全体で `vendor-aws` を必ず一緒に切り替える共通 feature とする旨を `Cargo.toml` の `[workspace.metadata]` に明文化。

### should-fix-003 `crates/tee/src/orchestrator.rs` と `crates/solana/src/extension.rs` で JCS+SHA-256 ロジックが二重実装

- 場所:
  - `crates/tee/src/orchestrator.rs:335-341` `fn compute_jcs_hash`
  - `crates/solana/src/extension.rs:116-124` `fn compute_verifiable_hash`
- 観察: 後者のコメント自体に「same as orchestrator.rs but standalone」と書かれている（extension.rs:115）。
- 問題: 「VerifiableResponse の JCS-SHA256 ハッシュ」は仕様書 §1.5/§2.3 の根幹であり、二箇所に実装があると 1 箇所だけ更新したときに Attestation のバインディングが静かに壊れる。Extension はそれを「自分が computed する」のではなく「コアの計算結果と同一になる」ことを検証するレイヤーなので、定義箇所がコア側に 1 つだけある必要がある。
- 修正案: `title-core` に `pub fn jcs_sha256(v: &VerifiableResponse) -> [u8; 32]` を追加し、tee/solana 両方から呼ぶ。orchestrator.rs と extension.rs の重複を削除。

### should-fix-004 `crates/solana/src/extension.rs::OffchainData` が完全に未使用

- 場所: `crates/solana/src/extension.rs:30-35`
- 観察: `pub struct OffchainData { #[serde(flatten)] pub response: ProcessResponse }` が定義されているが、`grep -rn 'OffchainData'` の結果ヒットは定義 1 箇所のみ。`process_extension` も `&ProcessResponse` を直接受け取る。
- 修正案: 削除する。あるいは将来「offchain wrapper」を入れるなら `crates/core/src/response.rs` に一緒に置き、`ProcessResponse` の serialize と完全に対称になることを保証する。

### should-fix-005 `crates/tee/src/main.rs::hex_short` と `crates/solana/src/extension.rs::hex_encode` の重複

- 場所: `crates/tee/src/main.rs:231-242`、`crates/solana/src/extension.rs:166-173`
- 問題: どちらも `for b in bytes { write!(s, "{:02x}", b) }` の手書きエンコーダ。`hex` クレートは既に workspace dep にあり、`hex::encode` / `&hex::encode(bytes)[..16]` で代替できる。
- 修正案: 両関数を削除し `hex::encode` を直接使う。

### should-fix-006 `crates/tee/src/server.rs::SolanaExtensionBody` と `crates/gateway/src/lib.rs::SolanaExtensionRequest` が別型

- 場所: `crates/tee/src/server.rs:181-189` (`SolanaExtensionBody`)、`crates/gateway/src/lib.rs:131-149` (`SolanaExtensionRequest`)
- 観察: フィールドはほぼ同一（`offchain_data_url, payer, merkle_tree, recent_blockhash, collection`）だが crate ごとに別の型として再宣言されている。gateway は relay 専用なので JSON 値をそのまま流す Body にしか使わないが、TEE 側で `SolanaExtensionBody` を独自定義しているため、片方を変更したらもう片方を忘れる risk が高い。
- 修正案: `title-core` か新設の `title-api`/`title-protocol-types` クレートに 1 つ定義し、gateway と tee の両方が同じ型を import する。`ProcessRequest`/`ProcessResponse` と同じ扱い。

### should-fix-007 `crates/tee/src/lib.rs` doc コメントが「v0.1.0 からの変更点」を述べている

- 場所: `crates/tee/src/lib.rs:17-22, 59-65`
- 観察: `## Legacy参照` セクションで `legacy/v0.1.0/crates/tee/src/runtime/` を参照し、変更点を Rust doc comment 内で説明している。`# v0.1.0からの変更点` も同様。
- 問題: 初見の読み手にとって「過去にどうだったか」は無関係であり、本来 git log / docs/CHANGELOG に置くべき情報がコード内に焼き付いている（タスク 16 で挙がっている「4.7 の癖」の典型例）。
- 修正案: doc comment の `## Legacy参照` と `# v0.1.0からの変更点` を削除する。CHANGELOG.md に「v0.1.2: TeeRuntime trait から crypto operations を分離」と一文だけ書く。

### should-fix-008 `programs/title-whitelist/keypair.json` がコミットされている

- 場所: `programs/title-whitelist/keypair.json`（program ID `43y8E...` の deploy 鍵と推定）
- 問題: program upgrade authority を握る鍵がリポジトリに入っていると、devnet ですら誰でも program を上書きできる。`Anchor.toml` で参照される deploy 鍵は通常 `.gitignore` 対象。
- 修正案: コミットから除外し、`Anchor.toml` の `[provider]` `wallet` を環境変数経由にする。OSS 公開時は「devnet 用の program ID は固定だが、それを deploy した鍵は持っていない」状態が正常。

### should-fix-009 `programs/title-whitelist/Cargo.toml` の `repository`/`license` が `[workspace.package]` 継承を使っていない

- 場所: `programs/title-whitelist/Cargo.toml:3-8`
- 観察: `version = "0.1.2"`, `edition = "2021"`, `license = "Apache-2.0"`, `repository = "https://github.com/yudai-mori-2004/title-protocol"` が hard-code されている（`programs/` は `Cargo.toml` で exclude されているため `workspace.package` 継承不可）。
- 問題: 同じ理由で `sp1-guests/{host,program}/Cargo.toml` も同じ値を 3 重に保持している。v0.1.3 に bump するときに 4 箇所を手で揃える必要がある。
- 修正案: 簡易には CI で `grep -c '"0.1.2"' Cargo.toml` を全クレートで実行し一致確認するスクリプトを追加する。本格的には `cargo workspaces version` / `release-plz` 等を導入。

### should-fix-010 `sp1-guests/attestation-aws-nitro/program` は AWS Nitro 専用なのに crate 名は将来分の拡張余地が見えない

- 場所: `sp1-guests/attestation-aws-nitro/{host,program}/`
- 観察: パス命名 `attestation-aws-nitro` は妥当だが、ホスト crate 名は `title-sp1-attestation-aws-nitro-host`、guest 名は `title-sp1-attestation-aws-nitro-program`。一方 SP1 verifier 側 `programs/title-whitelist/src/lib.rs:177-181` の `register_key` は「ベンダー中立な public values レイアウト」（measurement_len 可変）を採用しており、複数ベンダー guest を許容する設計。
- 問題: 設計が中立を目指しているのに guest 名前空間が AWS 固定のままだと、AMD SEV-SNP guest を追加するとき `sp1-guests/attestation-amd-sev-snp/` を作って同じ host CLI（`vkey`, `prove`）を再実装することになる。
- 修正案: `sp1-guests/attestation-host/` 一つに統合し、`--vendor aws-nitro|amd-sev-snp|...` で guest ELF を切り替える設計に変える。guest crate は per-vendor のままで OK。

### should-fix-011 `crates/gateway/src/lib.rs` が「API 型定義」と「mod 宣言」を兼任

- 場所: `crates/gateway/src/lib.rs:25-31, 33-157`
- 観察: lib.rs に `pub mod auth; pub mod endpoints; ...` と並んで `KeysResponse` / `ProcessorsResponse` / `HealthResponse` / `SolanaKeysResponse` / `SolanaExtensionRequest` / `SolanaExtensionResponse` の DTO がインラインで定義されている。
- 問題: 仕様書 §2.5 で定義されるレスポンス型はクライアント側からも参照したいデータ型であり、Gateway crate の trait/server とは独立した「線」である。さらに `should-fix-006` の通り TEE 側にも同じ型が必要。
- 修正案: 別 crate `title-api` または既存の `title-core` に DTO を移し、`title-gateway` は薄い実装側に専念。

### should-fix-012 `crates/tee/src/vendor/mod.rs` と `crates/tee/src/runtime/mod.rs` の分割粒度が中途半端

- 場所: `crates/tee/src/vendor/mod.rs` (18 行、`#[cfg(feature = "vendor-aws")] pub mod aws;` だけ)、`crates/tee/src/runtime/mod.rs` (12 行、`pub mod mock;` だけ)
- 観察: 「runtime = mock 実装の置き場」「vendor = 実 TEE の置き場」と分けているが、両方とも `TeeRuntime` 実装であり、内訳は `runtime::mock::MockRuntime` と `vendor::aws::NitroRuntime` の 1 ファイル zutsu。
- 問題: 階層が深く、また「実 TEE」「mock」を別ディレクトリにしている明確な意味がない。
- 修正案: `crates/tee/src/runtime/{mod.rs, mock.rs, aws_nitro.rs}` に統合し、`vendor/` ディレクトリは廃止する。doc 上の区別は cfg-feature と doc コメントで十分。

### nitpick-001 `crates/tee/src/orchestrator.rs` 内の `Spec SS5.2`/`SS1.3` 表記

- 場所: `crates/tee/src/orchestrator.rs:6-32` 等
- 観察: `SS` = `§` をエスケープしたつもりの表記が残っている。他ファイルは `§5.2` を直接使っている。
- 修正案: `SS` を全部 `§` に置換する（`crates/tee/src/orchestrator.rs`, `crates/tee/src/content_fetch.rs`, `crates/tee/src/limits.rs`, `crates/tee/src/resource_pool.rs`, `crates/gateway/src/state.rs`, `crates/gateway/src/endpoints.rs`, `crates/core/src/c2pa_verify.rs` 等）。

### nitpick-002 `crates/gateway/src/lib.rs` の docstring が日本語と英語混在

- 場所: `crates/gateway/src/lib.rs:41-56`（KeysResponse は日本語）, `:1-23`（クレートヘッダは英語）
- 修正案: クレート内で言語を統一する（プロジェクト全体が日英混在状態のため、最低限「同一 crate 内では揃える」を方針として明示する）。

### nitpick-003 `crates/proxy/src/protocol.rs` の async/sync 関数の `cfg` 順序

- 場所: `crates/proxy/src/protocol.rs:32-85`
- 観察: async ヘルパー（汎用）→ sync ヘルパー（vsock only）の順だが、それぞれ複数の `#[cfg(all(target_os = "linux", feature = "vendor-aws"))]` を繰り返している。
- 修正案: `#[cfg(all(target_os = "linux", feature = "vendor-aws"))] mod sync_io { ... }` でモジュールごと cfg してインデント圧縮。

### nitpick-004 `Cargo.toml` の workspace `members` と `exclude` の順序揺れ

- 場所: `Cargo.toml:2-16`
- 観察: members は `attestation, attestation-aws-nitro, core, crypto, tee, gateway, proxy, solana`（attestation 系まとめ、core/crypto、tee/gateway/proxy/solana）。exclude は `legacy, programs, sp1-guests`。
- 修正案: アルファベット順に揃える（自動 lint しやすい）。

### nitpick-005 `crates/gateway/Cargo.toml` の `[[bin]]` `path = "src/main.rs"` 冗長指定

- 場所: `crates/gateway/Cargo.toml:10-12`、`crates/tee/Cargo.toml:10-12`、`crates/proxy/Cargo.toml:10-12`
- 観察: `path = "src/main.rs"` は cargo の default と同じ。`name` だけで足りる。
- 修正案: `path` 行を削除（または `name` も削除して `[package].name` 推論に任せる）。

### nitpick-006 `crates/tee/src/server.rs::handle_keys` の Json 直接構築

- 場所: `crates/tee/src/server.rs:101-105`、`:109-113`、`:173-177`
- 観察: `Json(serde_json::json!({ "keys": state.key_bundle.public_keys() }))` のように DTO を使わず手書き JSON を返している。`crates/gateway/src/lib.rs:58, 79, 118` の `KeysResponse`/`ProcessorsResponse`/`SolanaKeysResponse` をそのまま import して使うと型整合性が取れる。
- 修正案: TEE 側 server.rs でも `title_gateway::{KeysResponse, ProcessorsResponse, SolanaKeysResponse}` を使う、もしくは `should-fix-011` の通り型を `title-core`/`title-api` に移してから両側から import する。

## 提案する new layout

```
title-protocol/
├── Cargo.toml                       # workspace
├── crates/
│   ├── core/                        # request/response, Processor trait (現状維持)
│   ├── api/                         # 新設: 全 HTTP DTO（仕様 §2.2/§2.3/§2.5/§6.2）
│   ├── crypto/                      # 暗号原語（現状維持）
│   ├── attestation/                 # AttestationVerifier trait + vendor_tags
│   ├── attestation-aws-nitro/       # AWS Nitro 検証実装
│   ├── tee-core/                    # TeeRuntime trait + ResourcePool + content_fetch + orchestrator
│   │   └── src/runtime/{mock,aws_nitro}.rs
│   ├── tee-server/                  # axum server, main.rs（extension 組み立て、ベンダー選択）
│   ├── extension-solana/            # 旧 title-solana を改名（コア依存のみ、tee 依存なし）
│   ├── gateway/                     # 薄い relay
│   └── proxy/                       # HTTP forwarder
├── programs/title-whitelist/        # state.rs / events.rs / errors.rs / instructions/ に分割
├── sp1-guests/
│   ├── host-cli/                    # ベンダー切替 CLI
│   └── attestation-aws-nitro/       # guest 専用
├── deploy/aws/                      # 現状維持
└── docs/                            # 現状維持
```

依存方向（上→下のみ許可）:

```
tee-server, gateway
    │
    ├── extension-solana ── attestation* ── core
    │                                          ▲
    ├── tee-core ── attestation* ──────────────┤
    │                                          │
    └── api ───────────────────────────────────┘
                  crypto ─────────────────────►
```

これにより:
- `extension-solana` を外したビルドが可能（must-fix-002 解消）
- 新ベンダー（AMD SEV-SNP）追加時に触る crate は `attestation-amd-sev-snp` 追加と `tee-core` の vendor 切替 1 行のみ
- ベンダー識別子は `attestation` の `vendor_tags` 1 箇所（must-fix-003 解消）

## 全体所感

仕様書 §5.1 の二層構成（Gateway / TEE）と §6.1 の「コアと Extension の分離」は明確だが、実装上は (a) `title-tee` が `title-solana` に静的依存している、(b) HTTP DTO が gateway crate と tee crate の両方で再宣言されている、(c) ベンダー識別子が 3 表記で揺れている、という 3 つの境界違反が「中立性を謳う設計」に逆らっている。とくに `must-fix-002` は Extension を増やす（Ethereum, EAS など）将来計画が事実上できない状態で、本番化フェーズに入る前に必ず整理する価値がある。`keys/admin.json` と `programs/title-whitelist/keypair.json` のコミットは OSS 公開タスクの一段前で必ず除去すべき。
