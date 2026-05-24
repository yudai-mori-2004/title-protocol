# A. コメント・ドキュメント癖

## 概要

担当範囲: リポジトリ全体の `.rs` / `.md` / `.sh` / `Dockerfile` / `*.tf` のコメント・docstring。`legacy/`, `target/`, `keys/`, `docs/v0.1.0/`, `docs/v0.1.1/` は対象外。

監査方針: Opus 4.7 が書いたコメントの癖（「ない」もの列挙、廃止経緯・git 履歴依存の注釈、内輪話、過剰防御 rationale、Spec §X の機械的添付、本体より長い rationale、装飾過多）を 1 文ずつ確認。SPECS_JA.md と実装本体は別観点 (F, D) に委ねる。

サマリ: **must-fix 23 / should-fix 28 / nitpick 14** = **65 件**。

「ない」もの列挙が main.tf / aws/README / Dockerfile に集中、内輪話（task 番号・"legacy/v0.1.0/" 由来引用・"future optimization" 等）がコード本体に散在、`/// 仕様書 §X` の機械的添付が core / gateway / crypto に多い、というのが三大パターン。

## 重大度別内訳

- must-fix: 23 件
- should-fix: 28 件
- nitpick: 14 件

## 発見

### must-fix-001 「ない」もの列挙（main.tf 冒頭）

- 場所: `deploy/aws/terraform/main.tf:1-12`
- 観察:
  ```
  # Title Protocol — AWS infrastructure (v0.1.2)
  #
  # Single EC2 with Nitro Enclaves, no Elastic IP, no S3, no IAM user.
  # A fresh `terraform apply` provisions everything required to run a TEE node.
  # A fresh `terraform destroy` removes everything (the only AWS residue is the
  # legacy `title-signed-json-devnet` S3 bucket, which lives outside this state
  # on purpose).
  #
  # IP address is allocated by AWS at instance launch and changes whenever the
  # instance is stopped/started. Clients reach the gateway via whatever
  # `terraform output -raw public_ip` reports at the time.
  ```
- 問題: タスク 16 README が「典型例」として名指ししている "no Elastic IP, no S3, no IAM user" がそのまま残存。初見の読み手にとって何があるかではなく何が無いかを列挙しても情報価値がない。さらに「the only AWS residue is the legacy `title-signed-json-devnet` S3 bucket」は v0.1.0 の負債への内輪言及で、新規読者には全く意味不明。
- 修正案: 「ない」列挙と legacy 残存物の言及を削除し、何があるかと public_ip 揮発性だけを残す。
  ```
  # Title Protocol — AWS infrastructure (v0.1.2)
  #
  # Single Nitro-Enclaves-capable EC2 + minimal security group + auto-
  # generated SSH key. `terraform apply` provisions the full TEE node;
  # `terraform destroy` removes it.
  #
  # The public IP is reassigned on each stop/start — re-read
  # `terraform output -raw public_ip` after restarting the instance.
  ```

### must-fix-002 「ない」もの列挙（deploy/aws/README.md "Cost note"）

- 場所: `deploy/aws/README.md:62-68`
- 観察:
  ```
  Single `c5.xlarge` ($0.214/hr Tokyo) + 50 GB gp3 EBS. Stop the instance
  when idle and you only pay for EBS (~$5/mo); destroy with `terraform
  destroy` and you pay nothing. There is no Elastic IP, so the public IP
  changes on every stop/start — always re-read `terraform output public_ip`
  after restarting.
  ```
- 問題: "There is no Elastic IP" は同じく「ない」もの列挙。読み手は「IP は再起動で変わる」だけ知れば十分で、その理由（EIP 未割り当て）は実装詳細。
- 修正案: 該当文を「Public IP changes on every stop/start — re-read `terraform output public_ip` after restarting.」に短縮。

### must-fix-003 内輪話（README.md "Status" の "legacy/v0.1.0/" 参照）

- 場所: `README.md:130`
- 観察:
  ```
  Previous implementation (v0.1.0) is archived in `legacy/v0.1.0/` for reference.
  ```
- 問題: OSS の README で「過去版がどこに退避されているか」を強調するのは内輪事情。クローンしたユーザーの大半は最新版を使いたいだけで、`legacy/` の存在は CONTRIBUTING や CHANGELOG で十分。
- 修正案: 該当文を削除。`Status` セクションは「**v0.1.2 — implementation in progress.** See [Technical Spec](docs/v0.1.2/SPECS_JA.md) for the full design.」だけにする。

### must-fix-004 廃止経緯の長文 rationale（CHANGELOG.md "Changed/Removed" を Unreleased に詰め込み）

- 場所: `CHANGELOG.md:11-35`
- 観察: `## [Unreleased] — v0.1.2` の下に "Trust model: Collection-based -> Attestation Document-based", "WASM execution engine (wasmtime)" の削除、`image-phash`/`cert-rootlens` の deprecate など、まだリリースされていない版で「以前のものを取り消した」記述が並ぶ。
- 問題: Keep a Changelog の流儀では Unreleased は「次にリリースされる差分」のはず。v0.1.0 から見た差分を Unreleased に書くと、v0.1.2 を最初に触る人にとっては「廃止された機能」が大量に目に入る。Full rewrite であれば「Initial release of the rewritten protocol. See SPECS_JA.md.」で十分。
- 修正案: Unreleased セクションを次に置き換える:
  ```
  ## [Unreleased] — v0.1.2
  
  Full protocol rewrite. See [Technical Spec](docs/v0.1.2/SPECS_JA.md) for the design.
  
  ### Added
  - Attestation-document-based trust model (Gateway + TEE, two components)
  - Native Rust processors (c2pa-verify mandatory, others optional)
  - Optional E2EE with three KEM suites (X25519, P-256, ML-KEM-768)
  - Fragmented / sidecar input types in addition to single
  - Solana Extension with ZK-proven TEE signing key whitelist
  ```
  v0.1.0 との差分一覧は v0.1.0 の changelog で読めるので削除可能。

### must-fix-005 `crates/tee/src/lib.rs` "Legacy参照" セクション

- 場所: `crates/tee/src/lib.rs:17-21, 59-63`
- 観察:
  ```rust
  //! ## Legacy参照
  //!
  //! `legacy/v0.1.0/crates/tee/src/runtime/` — 前バージョンのTeeRuntime実装。
  //! v0.1.0ではcrypto固有メソッド（signer, decapsulator等）がTeeRuntimeに含まれていたが、
  //! v0.1.2ではTEEハードウェア抽象化に専念し、暗号操作は別層で扱う。
  ```
  及び `TeeRuntime` トレイトの doc comment 内に重ねて:
  ```rust
  /// # v0.1.0からの変更点
  ///
  /// v0.1.0では暗号操作（署名、KEM復号等）がTeeRuntimeに含まれていたが、
  /// v0.1.2ではTEEハードウェア抽象化に専念する。
  ```
- 問題: 過去 git 履歴で済む情報。さらに同じ説明が 2 箇所に重複。初見の読み手は「v0.1.0 では〜」と言われても判断材料がない。
- 修正案: 両方の "Legacy参照" / "v0.1.0からの変更点" セクションを削除。`TeeRuntime` の責務は「TEE ハードウェア抽象化（Attestation 取得 + 乱数生成）」だけで自然に表現できる。

### must-fix-006 `crates/gateway/src/lib.rs` "## Legacy" セクション

- 場所: `crates/gateway/src/lib.rs:21-23`
- 観察:
  ```rust
  //! ## Legacy
  //!
  //! `legacy/v0.1.0/crates/gateway/` -- Previous Gateway implementation (Axum).
  ```
- 問題: 同上。過去版の場所を本番コードのトップ docstring に書く必要はない。
- 修正案: 削除。

### must-fix-007 ポート由来コメント（resource_pool.rs "Design notes (from legacy v0.1.0)"）

- 場所: `crates/tee/src/resource_pool.rs:35-39`
- 観察:
  ```rust
  //! ## Design notes (from legacy v0.1.0)
  //!
  //! The CAS-loop pattern in `extend()` is carried forward from
  //! `legacy/v0.1.0/crates/wasm-host/src/resource_pool.rs`.
  //! It provides lock-free, non-blocking reservation under contention.
  ```
- 問題: "from legacy v0.1.0" / "carried forward from `legacy/.../wasm-host/`" は内輪話。`wasm-host` は v0.1.2 にはもう存在せず、混乱を招く。CAS-loop の利点だけ残せばよい。
- 修正案:
  ```rust
  //! ## Concurrency
  //!
  //! `extend()` uses a CAS loop on `used`, so reservation is lock-free and
  //! non-blocking under contention.
  ```

### must-fix-008 ポート由来コメント（jumbf.rs）

- 場所: `crates/core/src/jumbf.rs:12-13`
- 観察:
  ```rust
  //! Ported from `legacy/v0.1.0/crates/core/src/jumbf.rs` with error type
  //! adapted for v0.1.2 processor framework.
  ```
- 問題: 同様の内輪話。
- 修正案: 削除。

### must-fix-009 ポート由来コメント（cnft.rs）

- 場所: `crates/solana/src/cnft.rs:7`
- 観察:
  ```rust
  //! Ported from `legacy/v0.1.0/crates/tee/src/blockchain/solana_tx.rs`.
  ```
- 問題: 同上。
- 修正案: 削除。

### must-fix-010 タスク番号への内輪言及（c2pa_verify.rs）

- 場所: `crates/core/src/c2pa_verify.rs:25-27, 100-101`
- 観察:
  ```rust
  //! The utility is public because the TEE orchestration layer (Task 04)
  //! also needs it when assembling the final response.
  ```
  ```rust
  /// - The TEE orchestration layer (Task 04) to populate `ProcessResponse.signature_hash`
  ```
- 問題: "(Task 04)" は開発タスク番号の内輪言及。最新の docs/v0.1.2/tasks/04-* は実装後の読み手にとって何の手がかりにもならず、リネームすれば即座にリンク切れ。
- 修正案: 「TEE orchestration layer」だけ残し `(Task 04)` を削除。

### must-fix-011 タスク番号への内輪言及（orchestrator.rs テスト doc）

- 場所: `crates/tee/src/orchestrator.rs:477` (`"task04-test"` シグナ名)
- 観察: `c2pa::EphemeralSigner::new("task04-test")` 及び `"task04-orchestrator-test"`（487 行付近）。
- 問題: テスト内に "task04" が固有名として埋め込まれる。Signer 名にタスク番号を載せる必然性がない。
- 修正案: `"title-orchestrator-test"` 等の中立的な名前に変更。

### must-fix-012 「現時点では / 将来は」表現が API doc に混入（content_fetch.rs）

- 場所: `crates/tee/src/content_fetch.rs:142-148`
- 観察:
  ```rust
  /// Per-fetch wall-clock budget. Spec §4.4 specifies a 60-second chunk
  /// timeout enforced by `ResourcePool::Ticket` between successive
  /// data-arrival callbacks. Here we apply a single overall timeout on the
  /// blocking client as the floor protection: a non-responsive origin
  /// cannot stall a fetch beyond this duration even if `Ticket::extend`
  /// is never reached. Large legitimate fetches must use the Range Request
  /// path (a future addition) rather than a single multi-minute bulk GET.
  ```
- 問題: rationale が長文（6 行）で本体 `FETCH_TIMEOUT: Duration = Duration::from_secs(60)` より大きい。"a future addition" は時間軸表現でロジックを暗黙に上書きする（実装は今は別ではない）。Range Request は §4.3 のメモリパターン項で扱えば十分。
- 修正案:
  ```rust
  /// Per-fetch wall-clock budget. Caps how long a single GET can block,
  /// independent of `Ticket` chunk timeouts.
  pub const FETCH_TIMEOUT: Duration = Duration::from_secs(60);
  ```

### must-fix-013 「将来の最適化」コメントが本体ロジックを上書き（content_fetch.rs ETag 節）

- 場所: `crates/tee/src/content_fetch.rs:25-29`
- 観察:
  ```rust
  //! ## ETag consistency (SS5.2)
  //!
  //! For Range Request scenarios (future optimization), the initial ETag is
  //! recorded and sent in subsequent If-Match headers. A 412 response means
  //! the file changed during transfer, and the request is aborted.
  ```
- 問題: 現状の `fetch()` 実装は ETag を取得するが If-Match 送信もしていないし複数呼び出しもしていない。"future optimization" 前提のドキュメントが現状の挙動を上回って書かれている。
- 修正案: 該当セクションを削除（or 412 ハンドリングが実際にある事実だけ短く残す）。Range Request 設計は SPECS_JA に書けばよい。

### must-fix-014 「現時点では / 将来の」表現が処理パターン doc に混入（fetch_fragmented）

- 場所: `crates/tee/src/content_fetch.rs:398-403`
- 観察:
  ```rust
  /// ## Memory pattern (SS4.3)
  ///
  /// Currently accumulates all fragments into a single buffer, tracking total
  /// memory via Ticket. The spec's ideal pattern (extend per fragment → process
  /// → shrink) requires a streaming C2PA reader, which is a future optimization.
  /// Peak memory = init + all fragments.
  ```
- 問題: "Currently / future optimization" の時間軸表現が本体ロジックの説明を覆っている。COVERAGE.md の §4.3 Note と完全に重複。読み手はコードの doc を読みたいだけで、未来計画は不要。
- 修正案:
  ```rust
  /// ## Memory
  ///
  /// All fragments are accumulated into a single buffer; peak memory is
  /// `init + Σ fragments`, tracked via `Ticket::extend`.
  ```

### must-fix-015 「やらなかった理由」rationale が本体より長い（auth.rs `contains`）

- 場所: `crates/gateway/src/auth.rs:86-98`
- 観察:
  ```rust
  /// API key validation with constant-time per-entry comparison.
  ///
  /// Walks every configured key (never short-circuits on a match) and uses
  /// a XOR-accumulator inner comparison. Length-mismatched entries still
  /// consume a constant number of comparisons against a fixed zero buffer
  /// so the total time depends on the configured set size and the longest
  /// stored key length, not on which (if any) entry matched.
  ///
  /// Note: candidates whose length doesn't appear in the configured set
  /// will leak that fact via overall execution time differences (no entry
  /// performs a same-length compare). API keys are typically high-entropy
  /// fixed-length tokens, making this leak negligible in practice.
  pub fn contains(&self, candidate: &str) -> bool {
  ```
- 問題: 12 行の rationale + さらにコメントで実装と乖離（「length-mismatched entries still consume a constant number of comparisons against a fixed zero buffer」と書いてあるが、`if stored_bytes.len() != candidate_bytes.len() { continue; }` で実際は dummy compare していない）。コメントが実装に嘘をついている可能性が高い must-fix。
- 修正案: コメントを 2 行にしてリスクだけ書く:
  ```rust
  /// Constant-time per-entry comparison; full set is always walked so the
  /// total time does not reveal which entry matched. Note: candidates whose
  /// length is absent from the set leak that fact via timing.
  ```
  さらに実装と整合させるか、`continue;` を dummy 比較に変えるかは C 観点担当に委ねる。

### must-fix-016 過剰防御 rationale（whitelist program lib.rs `from_slice`）

- 場所: `programs/title-whitelist/src/lib.rs:421-432`
- 観察:
  ```rust
  /// Build from a slice. The caller is responsible for length validation
  /// (instructions enforce `1..=MAX_MEASUREMENT_LEN`); excessive input is
  /// truncated here as a defensive measure to keep equality well-defined.
  pub fn from_slice(input: &[u8]) -> Self {
      // Length should already be validated by the caller
      // (`parse_public_values` rejects > MAX_MEASUREMENT_LEN). The
      // `debug_assert` makes the invariant explicit while the runtime
      // `min` keeps us in-bounds even if a future caller forgets.
      debug_assert!(
          input.len() <= MAX_MEASUREMENT_LEN,
  ```
- 問題: 同じ事実（「caller が validate するはずだが defensive truncate もしている」）を doc / 関数内コメント / debug_assert メッセージで 3 回繰り返している。
- 修正案: doc comment 1 行 + debug_assert だけ残し、関数内コメントを削除。
  ```rust
  /// Build from a slice. Excess bytes are truncated; callers should still
  /// pre-validate with `1..=MAX_MEASUREMENT_LEN`.
  pub fn from_slice(input: &[u8]) -> Self {
      debug_assert!(input.len() <= MAX_MEASUREMENT_LEN, ...);
  ```

### must-fix-017 内輪話（whitelist program `ADMIN_AUTHORITY` "Phase 1: single wallet"）

- 場所: `programs/title-whitelist/src/lib.rs:33-34`
- 観察:
  ```rust
  /// Admin authority pubkey: wrVwsTuRzbsDutybqqpf9tBE7JUqRPYzJ3iPUgcFmna
  /// Phase 1: single wallet. Future: multi-sig / DAO migration.
  ```
- 問題: "Phase 1 / Future" のロードマップを on-chain 定数の doc に書くと、その後マルチシグ化していなくても「将来計画」が永続する。 OPERATIONS_JA.md §9 のロードマップで管理すれば十分。
- 修正案: 「`Phase 1` / `Future: multi-sig / DAO migration.`」を削除し、Base58 表記の説明だけ残す。

### must-fix-018 内輪話（attestation-aws-nitro lib.rs "## Origin" セクション）

- 場所: `crates/attestation-aws-nitro/src/lib.rs:14-19`
- 観察:
  ```rust
  //! ## Origin
  //!
  //! Verification logic is derived from Automata Network's
  //! `aws-nitro-enclave-attestation` crate (Apache-2.0), itself based on
  //! Amazon's `aws-nitro-enclaves-cose` (Apache-2.0). The crypto primitives
  //! were ported from OpenSSL to RustCrypto by Automata. Title Protocol
  //! internalised the code and removed unrelated dependencies.
  ```
- 問題: 由来情報は LICENSE / NOTICE / CHANGELOG で十分。本体 docstring の冒頭にあると毎ファイル読むたびに目に入る。さらに `cose.rs:4` / `sign.rs:4-5` / `cert.rs:4` / `constants.rs:32-34` にも `// Origin: Automata Network — aws-nitro-enclave-attestation (Apache-2.0).` が反復され、合計 5 箇所に同じ由来表記がある。
- 修正案: ファイルレベルの `// Origin: ...` をすべて削除し、 `crates/attestation-aws-nitro/NOTICE` に集約する。コードの冒頭は責務だけで十分。

### must-fix-019 内輪話（aws.rs `tee_type_matches_attestation_vendor_tag` テスト）

- 場所: `crates/tee/src/vendor/aws.rs:191-195`
- 観察:
  ```rust
  #[test]
  fn tee_type_matches_attestation_vendor_tag() {
      let rt = fake_runtime(vec![]);
      // Single source of truth for the "aws-nitro" identifier.
      assert_eq!(rt.tee_type(), title_attestation_aws_nitro::VENDOR);
  }
  ```
- 問題: 「Single source of truth for the "aws-nitro" identifier」は監査者向けの自己弁護コメント。テスト名と assert が同じことを言っているので不要。
- 修正案: コメント行を削除。

### must-fix-020 内輪話（attestation lib.rs `AwsNitroVerifier` "Verifiers that require a pinned root..."）

- 場所: `crates/attestation-aws-nitro/src/lib.rs:47-51`
- 観察:
  ```rust
  /// Uses the certificate chain shipped inside each Attestation Document
  /// (`cabundle`) and trusts it implicitly — AWS rotates the root externally
  /// and includes the full chain in every document. Verifiers that require a
  /// pinned root should re-check `cert_chain.certs[0]` against their own
  /// trusted copy of the AWS Nitro root.
  ```
- 問題: 実装本体 (`doc.rs::authenticate`) ではちゃんと `AWS_NITRO_ROOT_CA_SHA256` で pin している。この doc は「pin していない」かのように読める。実装と矛盾する古い記述。
- 修正案: 削除し、以下に置き換える:
  ```rust
  /// Verifies the COSE_Sign1 signature, the certificate chain, and pins
  /// the chain root to the AWS Nitro Enclaves Root-G1 fingerprint embedded
  /// in `constants::AWS_NITRO_ROOT_CA_SHA256`.
  ```

### must-fix-021 内輪話（attestation lib.rs `MockAttestationVerifier` 「Pairs with the `MockRuntime` in `title-tee`」）

- 場所: `crates/attestation/src/lib.rs:99-105`
- 観察:
  ```rust
  /// Pairs with the `MockRuntime` in `title-tee`: accepts attestations of the
  /// form `"mock-attestation:" || user_data`, returns a zero-measurement
  /// `VerifiedAttestation` with the trailing bytes as `user_data`.
  ///
  /// Performs no cryptographic verification. Exists so the orchestration
  /// pipeline can be exercised without a real Attestation Document; never
  /// compiled into TEE binaries built for real hardware.
  ```
- 問題: 「never compiled into TEE binaries built for real hardware」は誤り。`title-attestation` の `mock` feature は `title-tee/runtime-mock` から有効化されるが、コード自体は通常ビルドに含まれうる。さらに「Pairs with the `MockRuntime` in `title-tee`」は親 crate への循環的参照で、`title-attestation` 単体の読み手には謎。
- 修正案:
  ```rust
  /// Accepts attestations of the form `"mock-attestation:" || user_data`,
  /// returns a zero-measurement `VerifiedAttestation` whose `user_data`
  /// is the trailing bytes. Gated behind the `mock` feature.
  ```

### must-fix-022 過剰防御 rationale（lib.rs `tee_seeded_rng` 「Using the host kernel's `OsRng` directly would defeat the point」）

- 場所: `crates/tee/src/main.rs:84-89`
- 観察:
  ```rust
  // Spec §2.4, §5.2 — per-suite key pairs, lost on restart. Entropy comes
  // from the TEE hardware via `TeeRuntime::random_bytes` (NSM GetRandom on
  // Nitro). Using the host kernel's `OsRng` directly would defeat the
  // point: enclave-internal entropy must be vendor-attestable, and Nitro's
  // /dev/urandom has no guaranteed seed source other than NSM.
  ```
- 問題: 5 行の rationale が「OsRng を直接使わない理由」を熱弁。fn の doc にあれば十分（実際 `tee_seeded_rng` の doc にも同じ説明がある）。重複。
- 修正案: コード内コメントを 1 行に:
  ```rust
  // Entropy must come from the TEE (NSM on Nitro), not host OsRng. See
  // `tee_seeded_rng` for why.
  ```

### must-fix-023 ASCII 装飾過多（orchestrator.rs / content_fetch.rs / resource_pool.rs / 各 gateway/tee）

- 場所: `crates/tee/src/orchestrator.rs:53-55, 126-128, 302-304, 373-375` と類似の `// --- ... ---` 区切りが crate 全体で 80 箇所超
- 観察:
  ```rust
  // ---------------------------------------------------------------------------
  // Error type
  // ---------------------------------------------------------------------------
  ```
- 問題: 75 文字幅 のダッシュ枠で「Error type」と書くだけのセクション見出しが多用される。Rust では普通 `// region: ...` か `mod` で十分。コード量を増やし、diff を読みづらくする。
- 修正案: 全 crate で `// ---- (75 chars) ---- \n // <title> \n // ---- ---- \n` パターンを `// <title>` の一行に統一。少なくとも 1 関数 1 セクションの区切りは削除。

## should-fix

### should-fix-001 `///` の Spec §X 機械的添付（core / gateway / crypto 全般）

- 場所: 
  - `crates/core/src/processor.rs:18-22, 31-33, 56-59` (全 trait method)
  - `crates/core/src/request.rs:14-15, 27-31, 46-47, 50-52, 60-61, 70-71, 79-82, 99-101, 110-111, 122-125, 134-137`
  - `crates/core/src/response.rs:14-19, 32-46, 58-72, 84-85, 95-97, 104-106`
  - `crates/gateway/src/lib.rs:41-56, 67-77, 87-90, 105-116, 126-130, 152-153`
  - `crates/gateway/src/state.rs:25-27, 50-51, 75-76, 94-98, 133-134`
- 観察: 全フィールド・全関数に `/// 仕様書 §X.Y` がほぼ機械的に貼られている。`KeysResponse.keys` のような自明なフィールドにも個別の `/// スイート名 → Base64エンコードされた公開鍵のマップ。` の上に struct レベルの `/// 仕様書 §2.5` がある等。
- 問題: タスク 16 が指摘する「全関数に貼ると価値が薄れる」の典型。Spec §2.5 のすべてが gateway/lib.rs にまとめて書かれているなら、struct 単位の §X.Y で十分。
- 修正案: ファイル先頭の `//! 仕様書 §X.Y` を残し、struct/fn レベルの `/// 仕様書 §X.Y` は本当に節間を跨ぐ参照のみに整理する。fields 単位の §X.Y は削除。

### should-fix-002 重複 docstring（gateway lib.rs `KeysResponse` / `HealthResponse` / `SolanaKeysResponse`）

- 場所: `crates/gateway/src/lib.rs:41-61, 88-100, 106-121`
- 観察: 3 つの `*Response` がそれぞれ `/// 仕様書 §2.5` + 日本語 1 行 + `# JSON例` + JSON サンプル + struct 内に再び `///` フィールド説明、という構造が並ぶ。
- 問題: JSON 例は SPECS_JA §2.5 にすでにあるので、ここに再掲する必要はない（むしろズレるリスクが増える）。
- 修正案: `# JSON例` 全体を削除し `/// GET /keys response (§2.5).` 等の 1 行に。

### should-fix-003 重複 docstring（whitelist program `WhitelistEntry` ↔ `title-solana` の `WhitelistEntry`）

- 場所: `programs/title-whitelist/src/lib.rs:447-479` と `crates/solana/src/whitelist.rs:23-73`
- 観察: 同じフィールドに対して両側で日本語/英語 doc が重複。`measurement: StoredMeasurement` の説明が両側でほぼ同じ文章を別言語で書いている。
- 問題: 「client-side mirror」だと doc に書きながら実態は重複してメンテ負担を生む。
- 修正案: client 側 (`crates/solana/src/whitelist.rs`) を `/// Mirror of `title_whitelist::WhitelistEntry` (see on-chain program for field docs).` に短縮。フィールド doc は on-chain 側のみに集約。

### should-fix-004 重複 docstring（gateway endpoints.rs `handle_keys` 等）

- 場所: `crates/gateway/src/endpoints.rs:48-50, 67-68, 84-89, 112-115, 134-137, 152-154`
- 観察: 全 6 ハンドラに `/// GET /keys -- Return cached TEE encryption public keys.\n/// Spec §2.5` 形式の重複。ルーター定義 (`server.rs:71-87`) のメソッド + パスとほぼ同じ情報。
- 修正案: `/// Spec §2.5` を `crates/gateway/src/lib.rs` のモジュール doc にまとめ、各 handler の doc を 1 行に。

### should-fix-005 long rationale（lib.rs `handle_solana_extension` "System clock failure..."）

- 場所: `crates/tee/src/server.rs:230-235`
- 観察:
  ```rust
  // Process extension (verify attestation + build & sign TX).
  // System clock failure here is fatal for this request: attestation
  // verifiers use `now_unix_secs` as the upper bound for cert validity,
  // so a silent 0 fallback would either accept everything or reject
  // everything depending on chain timing. Return 500 to surface it.
  ```
- 問題: 4 行の rationale。500 を返している事実だけ伝われば良く、なぜそうしないかの長文説明は不要。
- 修正案:
  ```rust
  // System clock unavailable -> 500. Cert validity depends on `now`.
  ```

### should-fix-006 rationale（orchestrator.rs `decrypt_single_payload` の "Reject mismatches ..."）

- 場所: `crates/tee/src/orchestrator.rs:272-275`
- 観察:
  ```rust
  // Reject mismatches between the suite the client declared on the JSON
  // request and the suite embedded in the wire payload header. Without
  // this check the declared field would be ignored, leaving the API
  // semantics confusingly loose.
  ```
- 問題: 「Without this check...」の防御的説明が `if opened.suite != suite { return Err(... EncryptionSuiteMismatch ...) }` という自明な 4 行コードに対して 4 行。`EncryptionSuiteMismatch` のエラーメッセージで十分自己説明的。
- 修正案: コメントを削除（または 1 行に: `// Declared suite must equal wire suite_id.`）。

### should-fix-007 rationale（content_fetch.rs `HttpContentFetcher` 本体 doc）

- 場所: `crates/tee/src/content_fetch.rs:119-127`
- 観察:
  ```rust
  /// HTTP-based content fetcher using `reqwest::blocking::Client`.
  /// Spec §5.2, §4.4
  ///
  /// Enforces the size and timeout limits that Spec §4.4 specifies for the
  /// fetch layer: every connection has a chunk-level read timeout, an overall
  /// wall-clock deadline, and a hard body-size ceiling. These prevent a
  /// malicious or misbehaving origin from stalling the TEE or exhausting its
  /// memory.
  ```
- 問題: 「These prevent a malicious or misbehaving origin from stalling...」は読者でも自明な攻撃モデル説明。
- 修正案: 5 行を「Enforces chunk timeout, fetch timeout, and a body-size cap (§4.4).」に短縮。

### should-fix-008 「敢えて〜しない」rationale（attestation guest main.rs `trusted_certs_prefix_len`）

- 場所: `sp1-guests/attestation-aws-nitro/program/src/main.rs:27-30`
- 観察:
  ```rust
  //! `trusted_certs_prefix_len` is intentionally NOT a guest input — it is
  //! hard-coded to 0 (verify the full cabundle chain). Allowing the prover
  //! to skip leading certs would let an attacker bypass chain verification
  //! by claiming the entire chain is "already trusted".
  ```
  同様の文が `sp1-guests/attestation-aws-nitro/host/src/lib.rs:55-60`、`crates/attestation-aws-nitro/src/doc.rs:55-58` にも重複。
- 問題: 同じ「やらなかった理由」rationale が 3 ファイルに分散。
- 修正案: guest 側に 1 箇所だけ残し、host / doc.rs 側は `// trusted prefix = 0; see guest for rationale.` 程度に短縮。

### should-fix-009 rationale（attestation lib.rs `AttestationVerifier::verify` doc 「Implementations should reject documents whose internal timestamp ...」）

- 場所: `crates/attestation/src/lib.rs:79-86`
- 観察: trait method `verify` の doc に「what implementations should do」を細かく規定している。実際の `AwsNitroVerifier::verify` では `min(now, doc.timestamp/1000)` で逆の挙動。trait コントラクトと実装が衝突。
- 問題: 仕様矛盾の must-fix としても扱えるが、コメント観点では「規約だが実装が守っていない」例。整合性は F 観点に委ねるが、現状の文言は削除すべき。
- 修正案: trait doc から「Implementations should reject documents whose internal timestamp is in the future relative to `now_unix_secs`」を削除し、「`now_unix_secs` is the reference time used for certificate validity checks.」だけ残す。

### should-fix-010 rationale（attestation aws-nitro lib.rs `verify` impl の "Use the smaller of (now, doc.timestamp/1000)..."）

- 場所: `crates/attestation-aws-nitro/src/lib.rs:74-77`
- 観察:
  ```rust
  // Use the smaller of (now, doc.timestamp/1000) for cert validity, so
  // documents from a TEE whose clock is slightly ahead of ours still
  // verify while genuinely expired certificates are still caught.
  ```
- 問題: 攻撃面の評価が浅い。TEE 側時計を信用すると cert 有効期限の clock skew 攻撃が成立する可能性があるが、コメントはそれに触れない。F/G 観点が拾うが、コメントとしても「why this is safe」を書いていない（事実だけ）。
- 修正案: 削除か、`// Allow doc-side clock skew; cap by `now` so expired certs are still rejected.` に短縮。

### should-fix-011 重複（lib.rs CHANGELOG-style コメント "## Trust model / Module system" 等）

- 場所: `CHANGELOG.md:11-35` の構造説明 + README.md:5-9 + README.md:104-113 の "Trust Model" + SPECS_JA §0.3 の表
- 観察: "C2PA alone vs Title Protocol" の表が README と SPECS_JA に並列。
- 問題: 同期忘れリスク。
- 修正案: README は 1 段落の抜粋にとどめ、表は SPECS_JA だけに置き「See [SPECS](docs/v0.1.2/SPECS_JA.md#0.3) for the full comparison.」へリンク。

### should-fix-012 重複説明（main.tf inline コメント vs deploy/aws/README.md）

- 場所: `deploy/aws/terraform/main.tf:147-150` と `deploy/aws/README.md:84-86`
- 観察:
  ```tf
  # First-boot provisioning: install Docker, nitro-cli, allocate hugepages.
  # See user-data.sh for the script. `user_data_replace_on_change = true`
  # would rebuild the instance on every script edit; we keep it false so
  # iterations don't churn the box. Re-running provisioning manually is
  # `bash deploy/aws/scripts/provision.sh` (idempotent).
  ```
- 問題: `deploy/aws/scripts/provision.sh` は **存在しない**。コメントが嘘の操作手順を案内している（B 観点で拾うが、コメントとしても must-fix 級）。
- 修正案: 「Re-running provisioning manually is `bash deploy/aws/scripts/provision.sh`」を削除。`user_data_replace_on_change` の理由は「Iteration without rebuilding the instance.」に短縮。

### should-fix-013 trace のような rationale（gateway server.rs `router` の "Layer order"）

- 場所: `crates/gateway/src/server.rs:88-89`
- 観察:
  ```rust
  // Layer order: outermost runs first. We want rate limiting to gate
  // even unauthenticated requests, so it sits *outside* the auth layer.
  ```
- 問題: Axum の layer 順序は型レベルで明示されており、`rate_limit_middleware` の doc にも同じ説明がある (`rate_limit.rs:11-13`)。重複。
- 修正案: コメントを削除（fn-level doc に集約済み）。

### should-fix-014 rationale（main.rs "Built outside the async runtime..."）

- 場所: `crates/tee/src/main.rs:132-133`
- 観察:
  ```rust
  // Built outside the async runtime because reqwest::blocking::Client spawns
  // its own tokio runtime internally; doing so inside an async context panics.
  ```
- 問題: `tokio::task::spawn_blocking(HttpContentFetcher::new)` が次の行で書かれている。コメントは実装と一致しているが、`spawn_blocking` で呼ぶ理由は API として明らかなので不要。
- 修正案: 1 行に: `// reqwest::blocking::Client must be built off the async runtime.`

### should-fix-015 rationale（attestation-aws-nitro lib.rs sp1 feature 切替コメント）

- 場所: `crates/attestation-aws-nitro/src/lib.rs:21-27`
- 観察:
  ```rust
  // When built for SP1, shadow the standard `sha2` and `p256` crates with
  // SP1-precompile-accelerated forks. The rest of the source is untouched.
  #[cfg(feature = "sp1")]
  extern crate sha2_sp1 as sha2;
  ```
- 問題: コメント自体は妥当だが「The rest of the source is untouched.」は読み手への安心感の表明であって情報ではない。
- 修正案: 「The rest of the source is untouched.」を削除。

### should-fix-016 rationale（whitelist program lib.rs `parse_public_values` の `has_user_data` チェック）

- 場所: `programs/title-whitelist/src/lib.rs:366-373`
- 観察:
  ```rust
  // has_user_data: u8 — must be canonical 0/1. Treating any non-1 value
  // as `false` would let a SP1 guest with a subtly wrong commit layout
  // pass on-chain, so we reject anything that isn't a Borsh boolean.
  ```
- 問題: 3 行 rationale。1 行で十分。
- 修正案: `// has_user_data: canonical Borsh bool (0 or 1).`

### should-fix-017 重複（COVERAGE.md の "Note: ..." 行と content_fetch.rs の "Note:" 行）

- 場所: `docs/v0.1.2/COVERAGE.md:69-71`、`crates/tee/src/content_fetch.rs:142-148, 398-403`
- 観察: 同じ "future optimization / accumulates fragments" Note が両方にある。
- 修正案: COVERAGE.md の Note は SPECS との差分だけ書き、コード側は self-contained に。あるいは逆。must-fix-014 / must-fix-013 の修正に伴って COVERAGE 側も追従。

### should-fix-018 内輪話（attestation aws-nitro `verifies_real_aws_nitro_attestation` のコメント "Document captured from a live Nitro Enclave; stored alongside this crate so tests don't depend on anything outside the crate tree."）

- 場所: `crates/attestation-aws-nitro/src/lib.rs:120-124`
- 観察:
  ```rust
  /// End-to-end verification against a real AWS Nitro Attestation Document.
  /// Document captured from a live Nitro Enclave; stored alongside this crate
  /// so tests don't depend on anything outside the crate tree.
  #[test]
  fn verifies_real_aws_nitro_attestation() {
  ```
- 問題: 「tests don't depend on anything outside the crate tree」は監査者向け自己弁護。テスト名 + fixture path で十分。
- 修正案:
  ```rust
  /// E2E verification against a captured Nitro Attestation fixture.
  ```

### should-fix-019 rationale（CLAUDE.md の「[Phase 1: single wallet]」「[Future: multi-sig]」が複数箇所に反復）

- 場所: README.md "Status" + CHANGELOG.md + whitelist/lib.rs + OPERATIONS_JA.md §9 ロードマップ
- 観察: "single wallet now / multi-sig later" のロードマップが少なくとも 4 箇所で言及されている。
- 修正案: OPERATIONS_JA.md §9 だけに置く。

### should-fix-020 rationale（rate_limit.rs `rate_limit_middleware` doc の「Runs independently of API-key validation so that a deployment with no `API_KEYS` configured still rejects runaway traffic.」）

- 場所: `crates/gateway/src/rate_limit.rs:85-89`
- 観察: should-fix-013 と同じ事実を別の場所で再説明。
- 修正案: doc を 1 行に短縮し、API_KEYS 連動の説明はモジュール doc (10-13 行目) のみに残す。

### should-fix-021 rationale（content_fetch.rs `fetch` ストリーミング部分 "Bail early if the server advertised a Content-Length..."）

- 場所: `crates/tee/src/content_fetch.rs:198-209`
- 観察:
  ```rust
  // Bail early if the server advertised a Content-Length that already
  // exceeds the cap. Avoids streaming gigabytes only to drop them.
  ```
  と `// Stream the body with an explicit size cap so a server that lies about (or omits) Content-Length still can't OOM the TEE.`
- 問題: 2 つのコメントが同じことを 2 段階説明。
- 修正案: 1 つにまとめる:
  ```rust
  // Cap response body before AND during streaming: Content-Length is a
  // hint only; a lying server must not OOM the TEE.
  ```

### should-fix-022 rationale（resource_pool.rs `Ticket` doc の "It is `Send` but not `Sync` due to `Cell<Instant>`"）

- 場所: `crates/tee/src/resource_pool.rs:165-167`
- 観察:
  ```rust
  /// A Ticket belongs to a single request thread and must not be shared
  /// across threads (it is `Send` but not `Sync` due to `Cell<Instant>`).
  ```
- 問題: 良い doc ではあるが、`Cell<Instant>` の話は実装詳細。コンパイラが Send+!Sync を強制してくれる。
- 修正案: 「(it is `Send` but not `Sync` due to `Cell<Instant>`)」を削除。

### should-fix-023 rationale（gateway state.rs `check_and_refresh` の「Restart is detected by comparing cached keys with live keys.」）

- 場所: `crates/gateway/src/state.rs:97-98`
- 観察: doc 行で説明された後、関数本体の `let keys_changed = ...` 周辺にも同じ内容がコメント付きで書かれている可能性が高い（実コード確認: 関数本体は無コメントだが doc は説明済）。十分単独で機能。
- 修正案: そのまま維持で OK（false positive 寄り、確認のため記載）。

### should-fix-024 nitpick 寄り should-fix（lib.rs `tee_seeded_rng` の「`purpose` is included only in error messages for debuggability.」）

- 場所: `crates/tee/src/main.rs:213-214`
- 観察:
  ```rust
  /// `purpose` is included only in error messages for debuggability.
  ```
- 問題: 関数本体を見れば自明。
- 修正案: 削除。

### should-fix-025 重複説明（KEY_EXPIRY_SECONDS の doc が programs と crates 両方に「90日」と日本語で記述）

- 場所: `programs/title-whitelist/src/lib.rs:26-27`, `crates/solana/src/whitelist.rs:15-17`
- 観察: 同じ定数の説明が両側で重複。
- 修正案: program 側のみ詳細 doc を保ち、client 側は `/// Mirror of `title_whitelist::KEY_EXPIRY_SECONDS`.` に。

### should-fix-026 内輪話 / 時間軸表現（OPERATIONS_JA.md §2.5 のプレースホルダ "この章は AWS Nitro EC2 上での実機検証後に内容を追記する（プレースホルダー）"）

- 場所: `docs/v0.1.2/OPERATIONS_JA.md:144-167`
- 観察: 「⚠️ **この章は AWS Nitro EC2 上での実機検証後に内容を追記する**（プレースホルダー）」と書いてあるが、`deploy/aws/README.md` は既に同等の手順を実機検証込みで網羅している。重複かつ「あとで書く」状態のドキュメントが残っている。
- 修正案: §2.5 全体を `deploy/aws/README.md` への参照に置き換える:
  ```
  ### 2.5 TEE バイナリのビルドと measurement 取得
  
  実機手順は [deploy/aws/README.md](../../deploy/aws/README.md) を参照。
  ```

### should-fix-027 「現時点では」表現（OPERATIONS_JA.md §5.2「現状クライアント SDK は提供していない」）

- 場所: `docs/v0.1.2/OPERATIONS_JA.md:331`
- 観察: 「現状クライアント SDK は提供していない。`crates/crypto/src/sealed_channel.rs` を読めば 80 行程度で実装できる。SDK 化はロードマップ。」
- 問題: 「現状」「ロードマップ」の時間軸表現が運用ガイド内に散在し、SDK ができても更新忘れる。
- 修正案: 「SDK は未提供。実装は `crates/crypto/src/sealed_channel.rs` を参照。」に短縮し、SDK 化計画は §9 ロードマップだけに記載。

### should-fix-028 rationale（gateway server.rs `rate_limit_skips_health` テストの「`GET /health` must never be rate limited — used by Gateway's own health checker and load balancer probes.」）

- 場所: `crates/gateway/src/server.rs:659-660`
- 観察: テスト doc としてはむしろ良いが、`rate_limit.rs` モジュール doc にも同じことが書いてある。
- 修正案: そのまま残しても可。優先度低い。

## nitpick

### nitpick-001 日英混在（content_fetch.rs / orchestrator.rs / resource_pool.rs / limits.rs の Spec 表記が混在: "Spec §X" / "Spec SS X" / "仕様書 §X"）

- 場所: 例 `crates/tee/src/orchestrator.rs:5-17` で `Spec §5.2`, 同 `:58` で `Spec SS5.2`, `crates/tee/src/content_fetch.rs:5-11` で `Spec SS5.2`, `crates/core/src/processor.rs:4` で `仕様書 §3.1`
- 問題: `§` と `SS` と `仕様書 §` の三表記が混在。`SS` は明らかに `§` の打ち間違い／文字化け修正の名残。
- 修正案: 全 crate で `§X.Y` 統一（あるいは ASCII の `Sec X.Y`）。`SS` を全置換。

### nitpick-002 typo（attestation lib.rs comment "AttestationDocument verifier" mixed casing）

- 場所: 観察した範囲では複数の crate の docstring で `JCS(signature_hash + results) SHA-256` 等、表記揺れがある。
- 修正案: スタイルガイド整備の一環で対応。

### nitpick-003 typo（content_fetch.rs `// Spec §5.2 -- 412 Precondition Failed` のダッシュ "--"）

- 場所: 多数のファイルで `-- ` セパレータと `— `（em dash）が混在
- 修正案: en dash / em dash / `--` を 1 種類に統一。

### nitpick-004 ASCII グラフ過多（OPERATIONS_JA.md §0 全体図 / README.md "How It Works" / deploy/aws/README.md）

- 場所: `OPERATIONS_JA.md:14-30`, `README.md:36-53`, `deploy/aws/README.md:14-33`
- 観察: ASCII 図が 3 箇所で類似の情報を表現。
- 修正案: 1 箇所に正準図を置き、他は参照リンク。

### nitpick-005 emoji 装飾（OPERATIONS_JA.md `⚠️`）

- 場所: `docs/v0.1.2/OPERATIONS_JA.md:144`
- 観察: should-fix-026 で削除予定のセクション内に `⚠️`。プロジェクトガイドラインで `Only use emojis if the user explicitly requests it` と CLAUDE.md にあるのでスタイル違反。
- 修正案: 当該セクション削除時に同時除去。

### nitpick-006 typo（rate_limit.rs "Spec §5.3 -- " の `--` 後にスペースなし箇所）

- 場所: `crates/gateway/src/rate_limit.rs:30`
- 修正案: スタイル統一。

### nitpick-007 表記揺れ（COVERAGE.md `[x]` / `[~]` / `[ ]` と本文中のチェックマーク表記）

- 場所: `docs/v0.1.2/COVERAGE.md` 全体
- 修正案: 凡例は冒頭にあるので OK。優先度低。

### nitpick-008 typo（attestation lib.rs `MockAttestationVerifier::MEASUREMENT` の doc "Measurement reported by the mock — always 48 zero bytes so the shape matches AWS Nitro's PCR0 size."）

- 場所: `crates/attestation/src/lib.rs:113-115`
- 観察: en dash と `48 zero bytes` の説明。問題なし。実装と整合。
- 修正案: なし（記載のみ）。

### nitpick-009 全角・半角混在（OPERATIONS_JA.md 数字に全角・半角混在）

- 場所: 散在
- 修正案: 半角統一。

### nitpick-010 マークダウン段組（README.md の `| | C2PA alone | Via Title Protocol |` の左カラムが空）

- 場所: `README.md:28-32`
- 観察: ヘッダーセル先頭が空欄で見栄えが悪い。
- 修正案: `| Aspect | C2PA alone | Via Title Protocol |` 等にラベル付与。

### nitpick-011 タイポ（orchestrator.rs `// Step 2: Fetch content from URL(s) with memory tracking` 直後のコメントに `// Step 3: Decrypt if the request declares...` とあるが、Step 1 が `pool.try_admit` で表記されない場所もあり、ステップ番号がコード内で 1〜11 までついている）

- 場所: `crates/tee/src/orchestrator.rs:169-237`
- 観察: コードのインラインに「Step 1」〜「Step 11」のラベル。SPECS §1.1 / §2.4 のフローと番号が完全には一致せず、メンテで番号がずれそう。
- 修正案: 番号を削除し、各ステップを `// Admit / Fetch / Decrypt / signature_hash / ...` の意味的見出しに。

### nitpick-012 タイポ（content_fetch.rs `fetch_fragmented` の "BMFF/ISO-14496-12 fragmented MP4 is a sequence of boxes"）

- 場所: `crates/tee/src/content_fetch.rs:394-397`
- 観察: doc は正確。issue なし（記載のみ）。

### nitpick-013 docstring 重複（c2pa_verify.rs `process` の `# Returns` で "If the C2PA signature is invalid, `validation` is `\"invalid\"` but the processor does NOT return an error" + `Returns ProcessorError only when the content cannot be parsed at all"`）

- 場所: `crates/core/src/c2pa_verify.rs:75-83`
- 観察: 文章としては妥当。
- 修正案: そのままで可。

### nitpick-014 表記揺れ（CONTRIBUTING.md `docs/` 構造説明と docs/README.md の構造説明の表現が微妙にズレ）

- 場所: `CONTRIBUTING.md:23-34` と `docs/README.md:10-27`
- 観察: 似た図が 2 箇所。
- 修正案: docs/README.md だけに置き、CONTRIBUTING からはリンク。

## 全体所感

4.7 の癖が最も濃いのは「現状/将来」と「やらなかった理由」と「ない列挙」の三つで、本文ロジックに比して説明過多な箇所が多い反面、`crypto/` と `attestation-aws-nitro/` のコア処理コードは比較的乾いており、削除すべきコメントの大半は文書化されたコード本体に対する説明（self-describing なコードへの過剰補足）に集中している。
