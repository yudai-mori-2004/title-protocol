# K5 Round 2: crates/solana + programs/title-whitelist 縦深掘り

Round 1 (`docs/v0.1.2/audit/k5-solana.md`) で挙げた 24 件 (must:4 / should:13 / nitpick:7) の処理状況を確認し、修正に伴う新規問題を洗い出した。

参照したのは `crates/solana/{Cargo.toml, src/{lib.rs, whitelist.rs, signing_key.rs, cnft.rs, extension.rs}, tests/devnet_whitelist.rs}` および `programs/title-whitelist/{Cargo.toml, src/lib.rs, vk/groth16_vk_v6.2.bin}` 全文。Round 1 と同じ深さ (1 文 1 文) で読んだ。

## Round 1 指摘の処理状況サマリ

- fixed: 13 件
- partially-fixed: 4 件
- unchanged: 7 件
- regressed: 0 件 (ただし新規発見セクションで「修正に伴って混入したバグ」を 3 件報告)

| ID | 重大度 | 状態 | 一行コメント |
|---|---|---|---|
| must-fix-001 | must | partially-fixed | client struct layout は一致したが `AnchorDeserialize` は付かず、本当の意味の mirror にはまだ届いていない |
| must-fix-002 | must | unchanged | 二重定義は残ったまま (コメント追加だけ) |
| must-fix-003 | must | fixed | `UpdateApproved*` に二重防御 (`has_one=admin` + `constraint=ADMIN_AUTHORITY`) が入った |
| must-fix-004 | must | fixed | `proof.len() == 4 + 256` の厳密チェック + 新エラー `InvalidProofLength` |
| should-fix-001 | should | unchanged | `RevokeKey` のエラー文言は Anchor generic のまま |
| should-fix-002 | should | fixed | 確認順序が parse→measurement→bind→verify に再配置 |
| should-fix-003 | should | fixed | `has_public_key` 含む末尾まで parse し `data.len() == offset` で終端チェック |
| should-fix-004 | should | unchanged | `process_extension` は依然 signing key の whitelist 在籍を見ない (ついでに `KeyNotWhitelisted` variant も削除されたので feature 自体が消失) |
| should-fix-005 | should | fixed | `build_and_sign_mint_tx` に CU budget 追加 (collection 有無で 250k/400k) |
| should-fix-006 | should | unchanged | `rent_exempt_minimum` ハードコードのまま |
| should-fix-007 | should | unchanged | bump を捨てる API のまま |
| should-fix-008 | should | fixed | `OffchainData` 削除 |
| should-fix-009 | should | fixed | `WhitelistInstruction` enum 削除 |
| should-fix-010 | should | partially-fixed | テスト分割はされていないが、新しい `InvalidProofLength` で決定論的に同じパスを通るようになった |
| should-fix-011 | should | unchanged | placeholder 鍵に feature gate も運用手順記載もない |
| should-fix-012 | should | partially-fixed | 移行計画コメントは追記された (lib.rs:33-40)、`transfer_admin` ix はまだ無し |
| should-fix-013 | should | fixed | `iter().take(num_signers).position(...)` に書き換わった |
| nitpick-001 | nit | unchanged | `WhitelistEntry::SIZE` テストは手計算等値のまま |
| nitpick-002 | nit | fixed | `pubkey!` マクロで `const WHITELIST_PROGRAM_ID` |
| nitpick-003 | nit | fixed | `strip_prefix("sha256:")` ベース |
| nitpick-004 | nit | fixed | should-fix-008 と統合 |
| nitpick-005 | nit | partially-fixed | コメント拡充はされたが「上流の定数を指す」ところまでは到達せず |
| nitpick-006 | nit | unchanged | `.replace("/crates/solana", "")` のまま |
| nitpick-007 | nit | fixed | `hex` workspace 依存追加 + 自前 `hex_encode` 削除 |

## Round 1 指摘の検証 (詳細)

### must-fix-001 / partially-fixed

- 修正後: `crates/solana/src/whitelist.rs:27-31, 65`
  - `pub struct StoredMeasurement { pub bytes: [u8; 64], pub len: u8 }` (on-chain と同じレイアウト)
  - `WhitelistEntry::measurement: StoredMeasurement` に変更
  - `WhitelistEntry::SIZE` の手計算式も `64 + 1` ベースに差し替え (whitelist.rs:78)
- 残課題: `AnchorDeserialize` / `BorshDeserialize` は derive されていない。`WhitelistEntry` の doc コメント (whitelist.rs:25-26) は「mirrors `programs/title-whitelist::StoredMeasurement` exactly so a future `AnchorDeserialize` flow walks the same wire bytes」と将来形を約束しているが、現状は宣言だけで実際の wire 読み取り API は存在しない。Round 1 で指摘した「クライアントが `AccountInfo::data` を読もうとした瞬間に静かに壊れる」状態は、レイアウト面では解消されたが、読み取り経路を実装するまでテスト不能。**実用ガード**として `whitelist.rs` に `#[test] fn layout_matches_on_chain()` を追加し、`borsh::to_vec(&StoredMeasurement { bytes: [0;64], len: 0 })` のサイズが 65 になることを assert すべき (現状の `whitelist_entry_size_matches_on_chain_layout` (whitelist.rs:168-178) は同じ式を二度書く nitpick-001 の問題そのままで、本当の wire レイアウト検証になっていない)

### must-fix-002 / unchanged

- 修正後: `programs/title-whitelist/src/lib.rs:27` と `crates/solana/src/whitelist.rs:21` の両方に `pub const KEY_EXPIRY_SECONDS: i64 = 90 * 24 * 60 * 60;` が残存
- 追加されたのは client 側のコメント (whitelist.rs:17-20) のみ:
  > **Authoritative source is on-chain** (`programs/title-whitelist`); this constant is for client-side `is_valid_at` checks and rotates with the program. Anchor's `idl` flow does not expose plain constants, so this duplicates the on-chain definition — update both together.
- 問題: 「update both together」は規律として弱い。Round 1 の修正案 (program crate を `title-whitelist = { workspace = true, features = ["no-entrypoint"] }` で取り込み `pub use title_whitelist::KEY_EXPIRY_SECONDS`) は実装されていない。`crates/solana/Cargo.toml` の `[dependencies]` に `title-whitelist` は無い (Cargo.toml:10-26 確認済み)
- 修正案 (Round 1 と同): `crates/solana/Cargo.toml` に `title-whitelist = { path = "../../programs/title-whitelist", default-features = false, features = ["no-entrypoint"] }` を追加し、`crates/solana/src/whitelist.rs:21` を `pub use title_whitelist::KEY_EXPIRY_SECONDS;` に置き換える。`no-entrypoint` feature は `programs/title-whitelist/Cargo.toml:14` に既にある

### must-fix-003 / fixed

- `programs/title-whitelist/src/lib.rs:594-607` (`UpdateApprovedVkeys`):
  ```
  #[account(... has_one = admin @ WhitelistError::Unauthorized)]
  pub approved_vkeys: Account<'info, ApprovedVkeys>,
  #[account(constraint = admin.key() == ADMIN_AUTHORITY @ WhitelistError::Unauthorized)]
  pub admin: Signer<'info>,
  ```
- `UpdateApprovedMeasurements` も同じく二重防御 (lib.rs:629-642)
- Round 1 で「将来 admin transfer ix を入れたときの二重防御」と書いたガードがそのまま入った
- 副産物: Round 1 時点の `admin_authority()` ヘルパ関数 (旧 640-642 行) は削除され、`ADMIN_AUTHORITY: Pubkey` 定数を直接参照する形に統一されたのも良改善
- 残課題: admin 鍵移管の手段 (transfer_admin / multisig 化) は依然未実装。lib.rs:33-40 にコメントとして migration plan が記載されたのみ (should-fix-012 と同じ話)

### must-fix-004 / fixed

- `programs/title-whitelist/src/lib.rs:291-294`:
  ```
  require!(proof.len() == 4 + 256, WhitelistError::InvalidProofLength);
  ```
- 新エラー variant: lib.rs:745-746 (`InvalidProofLength`, msg: "SP1 proof has unexpected length (expected 4 + 256 bytes)")
- sp1_solana::verify_proof_raw の前提 (256 バイトの Groth16 proof) が呼び出し側で保証されるようになり、配列インデックス panic 経路が閉じた
- ただし副作用: Anchor のエラーコード採番がシフトした (`InvalidProofLength` を index 1 に挿入したため、後続全 variant が +1)。これが devnet テスト側の hard-coded エラーコード assertion を壊している (後述「新規発見 N-1」参照)

### should-fix-001 / unchanged

- `RevokeKey` (lib.rs:677-689) は依然 `Account<'info, WhitelistEntry>`。Anchor は未初期化 PDA に対し `AccountNotInitialized (3012)` で fail する
- 独自エラー文言の追加は無し
- 修正案 (Round 1 と同): `revoke_key` 本体の冒頭で `require!(ctx.accounts.whitelist_entry.signing_pubkey != [0u8; 32], WhitelistError::EntryNotFound)` のような自前ガードを置く案。あるいは Round 1 通り「現状は Anchor の挙動で fail するので運用上問題ない」と判断するならコメントを残す

### should-fix-002 / fixed

- `register_key` の確認順序が見事に Round 1 修正案通りに並んだ (lib.rs:193-244)。`(1) vkey allowlist → (2) parse_public_values → (3) measurement allowlist → (4) user_data binding → (5) Groth16 verify → (6) Create PDA`
- 重い alt_bn128 syscall が最後に回ったので、不正な measurement や user_data binding 違反の連射に対する DOS 耐性が改善
- コメント (lib.rs:188-191) で「spec §6.2 lets the four substantive checks run in any order; this just keeps them DoS-resistant」と意図を明示しているのも好印象。これは Round 1 で危惧した「spec のリスト順を機械的に踏襲して順序が固まる」事態を避けている

### should-fix-003 / fixed

- `parse_public_values` が末尾まで読み切るようになった (lib.rs:404-423):
  - `has_public_key: u8` を `data[offset] <= 1` で canonical Borsh boolean 検証
  - `if has_public_key { ... offset += 32; }` で 32 バイト消費
  - 最後に `require!(data.len() == offset, WhitelistError::InvalidPublicValues)` で「余剰バイトなし」を強制
- これで SP1 guest 側 (`sp1-guests/attestation-aws-nitro/program/src/main.rs:70-75`) のコミット完全形が on-chain parser とビット単位で一致することが保証される
- `has_user_data` の `<= 1` チェック (lib.rs:388) と一貫した処理になった

### should-fix-004 / unchanged (悪化方向)

- Round 1 で `ExtensionError::KeyNotWhitelisted` が dead code として変数だけあると指摘したが、現在の `extension.rs:29-48` を確認すると `KeyNotWhitelisted` variant 自体が削除されている
- `process_extension` は依然 signing_key.pubkey() が on-chain whitelist にあるかを照会しない (extension.rs:154-177)
- 結果: 「TEE 内 register_key が未完了/失敗の状態で extension request を処理する」事故シナリオに対するガードは消えた (dead code の variant を消したことで feature 化への足がかりも失った)
- 修正案 (Round 1 と同 + 補足): `KeyNotWhitelisted` を error enum に復活させ、`process_extension` の冒頭で `TEE 起動時 register_key の成否` をフラグで持って参照する。あるいは extension request の処理経路に on-chain RPC 経由で `WhitelistEntry::is_valid_at(now)` を確認する step を入れる (RPC 依存になる代わりに TEE 内状態管理が不要)

### should-fix-005 / fixed

- `cnft.rs:236-247`:
  ```
  let cu_limit = if core_collection.is_some() { 400_000 } else { 250_000 };
  let cu_ix = ComputeBudgetInstruction::set_compute_unit_limit(cu_limit);
  let mut tx = build_v0_tx(&[cu_ix, ix], payer, recent_blockhash, &[])?;
  ```
- collection 有無で CU を分岐する作りで、Round 1 修正案 (「250_000 / 400_000 と分岐すると尚良し」) が完全に反映されている
- `set_compute_unit_price` (priority fee) は未対応だが、ユースケース上必須ではない (CLAUDE.md レベルで「TEE が運用上 priority fee を制御する」要件は無いはず)

### should-fix-006 / unchanged

- `cnft.rs:67-69` の `rent_exempt_minimum` は依然 `(128 + data_len as u64) * 6960` のハードコード
- devnet テスト (`cnft_full_flow_devnet`) は通っているが、Solana の rent_exemption_threshold が将来変更された場合に静かに lamports 不足で fail する経路は残ったまま
- 修正案: `build_create_tree_tx` の引数に `lamports: u64` を追加し、テスト側から `client.get_minimum_balance_for_rent_exemption(space)` の結果を渡す。`rent_exempt_minimum` は fallback / unit test 用に `#[deprecated]` を付けて残す

### should-fix-007 / unchanged

- `derive_*` は依然 `(Pubkey, u8)` を返し、呼び出し側 (`cnft.rs:101, 162, 199` 等) で `_bump` として捨てる
- `WhitelistEntry::bump` フィールドは on-chain にあるが、client 側のヘルパは PDA derive 結果の bump を活用していない
- 影響度は微少 (`find_program_address` 自体は数百 CU レベル)、Round 1 通り後回しで OK

### should-fix-008 / fixed

- `OffchainData` 構造体は `crates/solana/src/extension.rs` から削除された (現ファイル全文 grep で出現なし)
- `process_extension` は `&ProcessResponse` を直接受ける形のまま (extension.rs:154-177)
- 結果として「extension request の orchestration がどこにあるのか」という問いは未解決だが、少なくとも誤解を招く dead struct は消えた

### should-fix-009 / fixed

- `WhitelistInstruction` enum および関連の `whitelist_instruction_serialize` テストは `crates/solana/src/whitelist.rs` から削除された (現ファイル全文確認、出現なし)
- devnet_whitelist.rs:42-46 の `anchor_discriminator` ヘルパ + 手書きの命令データ構築が唯一のパスになり、設計が一本化された

### should-fix-010 / partially-fixed

- `register_key_rejects_invalid_proof` (devnet_whitelist.rs:193-231) は 9 バイトの fake proof を使い続けている
- 救い: must-fix-004 の fix で `proof.len() == 4 + 256` の厳密 length チェックが入ったので、このテストは決定論的に `InvalidProofLength (0x1771)` で fail する。Round 1 で危惧した「どのパスで fail しているか分からない」状態は実質的に解消
- 残課題: 「テストの意図 = 偽 proof が ProofVerificationFailed で弾かれる」は依然テストされていない。Round 1 修正案の 3 分割 (`vkey_not_approved`, `proof_wrong_vk_hash_prefix`, `proof_correct_prefix_invalid_body`) は未実施
- 軽い改善案: 既存テストで `err_msg.contains("0x1771")` を assert に格上げするだけでも、「length check が効いている」ことの retrocession test として有意義

### should-fix-011 / unchanged

- `add_placeholder_vkey_devnet` (devnet_whitelist.rs:508-553) と `add_placeholder_measurement_devnet` (556-601) は引き続き `[0xAA; 32]` と `[0xBB; 48]` を on-chain 登録
- `#[cfg(feature = "devnet-placeholders")]` などの gate は未付与
- `OPERATIONS_JA.md` / 運用ドキュメントへの「mainnet promote 前に必ず placeholder を消す」記載は未確認 (本 round の担当範囲外なのでドキュメント実体までは追っていない)
- なお、これら placeholder の登録命令自体に新規バグが混入している → 「新規発見 N-2」参照

### should-fix-012 / partially-fixed

- `programs/title-whitelist/src/lib.rs:33-40` に移行計画コメントが追記された:
  ```
  /// Phase 1: single wallet. Future: multi-sig / DAO migration plan:
  ///   A) replace this with an on-chain `admin_authority` PDA owned by a
  ///      Squads-style multisig program, and
  ///   B) add `transfer_admin(new_admin)` ix gated by the current admin
  ///      signature so rotation no longer requires a program upgrade.
  /// Until that lands, rotation requires `anchor upgrade` by the deploy key.
  ```
- Round 1 修正案の「短期: TODO コメント」は満たされた
- 「中期: `ctx.accounts.approved_vkeys.admin` 直接参照に切り替え」「`transfer_admin` ix 実装」は未着手 — 別タスクとして切り出すべき

### should-fix-013 / fixed

- `signing_key.rs:81-85`:
  ```
  let index = static_keys
      .iter()
      .take(num_signers)
      .position(|k| k == &pubkey)
      .ok_or_else(|| SigningKeyError::PubkeyNotInSigners(pubkey.to_string()))?;
  ```
- `take(num_signers)` で「signer 領域のみ」を見るようになっており、`num_signers > static_keys.len()` のような病的ケースでも安全 (`take` が自然に saturate する)
- `SigningKeyError::PubkeyNotInSigners` で具体的なエラー文言を返すようにもなった (Round 1 では `Ok(())` を返してしまう sentinel パスがあったが、現在は明確に `Err`)

### nitpick-001 / unchanged

- `whitelist.rs:168-178` は依然「discriminator + 各フィールドサイズを足し算した値」と `WhitelistEntry::SIZE` を比較するだけ。本物の wire encode 検証になっていない
- must-fix-001 と合わせて、`AnchorDeserialize` が derive された暁に `let s = entry.try_to_vec().unwrap(); assert_eq!(s.len(), WhitelistEntry::SIZE - 8)` 形式に書き換えるべき

### nitpick-002 / fixed

- `whitelist.rs:91-92`:
  ```
  pub const WHITELIST_PROGRAM_ID: Pubkey =
      pubkey!("43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs");
  ```
- `whitelist_program_id()` 関数 (98-100) は `#[inline]` で `WHITELIST_PROGRAM_ID` を返す薄いラッパとして残った (互換目的)。doc コメント「Prefer the WHITELIST_PROGRAM_ID constant directly when possible」も明示的

### nitpick-003 / fixed

- `cnft.rs:165-171`:
  ```
  let hex = signature_hash.strip_prefix("sha256:").unwrap_or(signature_hash);
  let short = &hex[..hex.len().min(8)];
  let name = format!("Title #{short}");
  ```
- `sha256:` prefix が無いケースでも安全にフォールバックする、明示的な実装

### nitpick-004 / fixed (should-fix-008 と統合)

### nitpick-005 / partially-fixed

- `cnft.rs:38-39`:
  ```
  /// Derive the MPL Core CPI Signer PDA used by Bubblegum V2 when minting
  /// into an MPL Core collection. Seeds: `[b"mpl_core_cpi_signer"]`,
  /// program = Bubblegum. The seed is defined inside the Bubblegum program
  /// (not re-exported); keep this in sync if Bubblegum changes the convention.
  ```
- 「再 export されていない、Bubblegum がこの慣習を変えたら追従が必要」という出典の弱さを正直に書いた点は good
- 上流 (`mpl_bubblegum::constants::*` または `mpl_bubblegum::accounts::*`) に該当 const が無いことは Round 1 で確認済み。これ以上踏み込むのは難しいので partially-fixed で妥当

### nitpick-006 / unchanged

- devnet_whitelist.rs:34 と:261 の `env!("CARGO_MANIFEST_DIR").replace("/crates/solana", "")` は変わらず
- 上記コードは「crate のディレクトリ名が `crates/solana` に格納されている」ことに依存しており、リポジトリ構造変更時に静かに壊れる
- 修正案 (Round 1 と同): `std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join("keys/admin.json")`

### nitpick-007 / fixed

- `crates/solana/Cargo.toml:19` に `hex = { workspace = true }` 追加
- `extension.rs:141, 143` で `hex::encode(...)` 直接使用
- 自前 `hex_encode` 関数は削除

## 新規発見 (Round 1 では拾えなかった/修正で生まれた問題)

### N-1 (must-fix): devnet テストの error-code assertion が must-fix-004 のシフトで全部ずれている

- 場所:
  - `crates/solana/tests/devnet_whitelist.rs:547` — `msg.contains("0x1775")` (期待: `VkeyAlreadyApproved`)
  - `crates/solana/tests/devnet_whitelist.rs:595` — `msg.contains("0x1778")` (期待: `MeasurementAlreadyApproved`)
- 観察: must-fix-004 の修正で `WhitelistError` enum の先頭に `InvalidProofLength` が挿入された (lib.rs:744-746)。Anchor の error code 採番は宣言順なので、後続 variant のコードが全て +1 シフトしている
  - 採番表 (現状):
    - `EmptyProof = 6000 = 0x1770`
    - `InvalidProofLength = 6001 = 0x1771`
    - `EmptyPublicValues = 6002 = 0x1772`
    - `ProofVerificationFailed = 6003 = 0x1773`
    - `InvalidPublicValues = 6004 = 0x1774`
    - `MissingUserData = 6005 = 0x1775`
    - `UserDataMismatch = 6006 = 0x1776`
    - `Unauthorized = 6007 = 0x1777`
    - `VkeyNotApproved = 6008 = 0x1778`
    - `VkeyAlreadyApproved = 6009 = 0x1779`
    - `VkeyRegistryFull = 6010 = 0x177A`
    - `MeasurementNotApproved = 6011 = 0x177B`
    - `MeasurementAlreadyApproved = 6012 = 0x177C`
  - すなわちテストの assertion:
    - `add_placeholder_vkey_devnet:547` が探している `0x1775` は実は `MissingUserData`。`VkeyAlreadyApproved` を期待するなら `0x1779`
    - `add_placeholder_measurement_devnet:595` が探している `0x1778` は実は `VkeyNotApproved`。`MeasurementAlreadyApproved` を期待するなら `0x177C`
- 問題: テストの「すでに登録済みなら冪等 skip」分岐が、実際は文字列 `0x1775` を含まない別エラーで panic することになる。冪等性の前提が崩れている。`||` の右辺で `msg.contains("VkeyAlreadyApproved")` を併記しているので、Anchor のエラーログに variant 名が含まれていれば救われるが、Solana RPC のエラー文字列が常にそうとは限らない (program log を含めるかは `RpcSendTransactionConfig` 次第)
- 修正案:
  1. 即座: assertion を `0x1779` (VkeyAlreadyApproved) / `0x177C` (MeasurementAlreadyApproved) に修正
  2. 構造的: hard-coded hex の代わりに `WhitelistError::VkeyAlreadyApproved as u32 + 6000` を計算するか、`title-whitelist` を dev-dep として取り込み `WhitelistError::VkeyAlreadyApproved.code()` を参照する (must-fix-002 で title-whitelist を dep に取り込めば同じ仕組みで解決)
  3. 防御: enum 順序が意味を持つ事実を `programs/title-whitelist/src/lib.rs` の `#[error_code]` 上に「DO NOT INSERT new variants except at the end — error codes are external ABI consumed by tests at `crates/solana/tests/devnet_whitelist.rs`」とコメントで明記

### N-2 (must-fix): `add_placeholder_vkey_devnet` の instruction data 構築が壊れている

- 場所: `crates/solana/tests/devnet_whitelist.rs:521-523`
  ```
  let mut data = anchor_discriminator("add_approved_vkey").to_vec();
  data.extend_from_slice(&(placeholder.len() as u32).to_le_bytes());
  data.extend_from_slice(&placeholder);
  ```
- 観察: `add_approved_vkey` の引数は `vkey_hash: [u8; 32]` (lib.rs:69-71)。Borsh の固定長配列 `[u8; 32]` は **長さプレフィックスを持たない** (32 バイトをそのまま読む)
- 問題:
  - 構築データ = `[disc(8), len_prefix(4), placeholder(32)]` = 44 バイト
  - Anchor 側のデシリアライズ = disc 後の 32 バイトを `vkey_hash` として読む → `[0x20, 0x00, 0x00, 0x00, 0xAA, 0xAA, ... (28 個の 0xAA)]` を vkey_hash として登録
  - 残り 4 バイトの 0xAA を Borsh `try_from_slice` が「Not all bytes read」で reject する可能性が高い (Anchor は `BorshDeserialize::try_from_slice` を使う厳格 mode)。仮に reject されなくても、登録される vkey_hash は `[0xAA; 32]` ではなく先頭 4 バイトが length prefix の `[0x20, 0x00, 0x00, 0x00, 0xAA × 28]`
- 結果: (A) test が `try_from_slice` の strict mode で fail し続けている、または (B) 意図と異なる vkey_hash が on-chain に書かれている。どちらにせよ「placeholder vkey が approved set に入る」テストの目的が達成されていない。devnet 上の現状の `ApprovedVkeys` PDA を `getAccountInfo` で確認すれば真実が分かる
- 対比: `add_placeholder_measurement_devnet:569-571` は同じ構築をしているが、こちらは `measurement: Vec<u8>` (lib.rs:123) なので `u32 len + bytes` が正しい Borsh エンコード。両者で書き方が同じなのに片方だけ正しい
- 修正案: devnet_whitelist.rs:521-523 を以下に置換 (length prefix を消す):
  ```
  let mut data = anchor_discriminator("add_approved_vkey").to_vec();
  data.extend_from_slice(&placeholder);
  ```
- 検証手順: 修正後、devnet 上の `ApprovedVkeys` PDA を `client.get_account()` し、`vkeys: Vec<[u8;32]>` の中身が `[0xAA; 32]` ちょうどになっているかを確認する。should-fix-011 の「placeholder を消す手順」と合わせ、誤登録された値も remove_approved_vkey で除去する必要がある (現運用次第)

### N-3 (should-fix): `EmptyProof` error variant が dead code 化

- 場所: `programs/title-whitelist/src/lib.rs:743-744`
  ```
  #[msg("SP1 proof is empty")]
  EmptyProof,
  ```
- 観察: must-fix-004 の修正で `proof.len() == 4 + 256` の equality check に置き換わったため、`proof.len() == 0` だけを弾くパスは消滅。`EmptyProof` を発火させるコードパスは grep 結果でゼロ
- 問題: dead variant。`InvalidProofLength` が「length != 260」を一括カバーするのに、`EmptyProof` が残っているのは紛らわしい (運用者が「Empty なら EmptyProof、4-259 byte なら InvalidProofLength」と勘違いする余地)
- 修正案: `EmptyProof` を削除。ただし削除すると enum 採番が前にシフトし N-1 と同じ問題を引き起こす (devnet テストや外部の error code 参照者に影響)。安全策は「`EmptyProof` を `#[deprecated]` 化して残し、新たに variant を追加する場合は必ず末尾に追加する」というポリシーを `#[error_code]` 直前にコメント明文化する (N-1 修正案 3 と統合)

### N-4 (nitpick): `EmptyPublicValues` の事前チェックが parse_public_values と二重

- 場所:
  - `programs/title-whitelist/src/lib.rs:282-285`: `verify_sp1_groth16` 冒頭で `require!(!public_values.is_empty(), WhitelistError::EmptyPublicValues);`
  - `programs/title-whitelist/src/lib.rs:346-348`: `parse_public_values` 冒頭で `require!(data.len() >= offset + 4, WhitelistError::InvalidPublicValues);` (offset=0 なので空 input は必ずここで fail)
- 観察: `register_key` の現順序では `parse_public_values` (step 2) が `verify_sp1_groth16` (step 5) より先に走る。`public_values.is_empty()` の場合、parser が先に `InvalidPublicValues` で reject する
- 問題: `EmptyPublicValues` を返すパスは事実上ない (`verify_sp1_groth16` を `register_key` 以外から呼ぶ場所はなく、register_key からは必ず parse 後に呼ばれる)。エラー分類として `InvalidPublicValues` に統合した方が運用者は理解しやすい
- 修正案: `verify_sp1_groth16` 内の `EmptyPublicValues` 早期 return を削除し、`EmptyPublicValues` variant を `#[deprecated]` で残す (N-3 と同じ理由で削除はしない)。または `EmptyPublicValues` を残すなら `parse_public_values` の冒頭の `>= offset + 4` を `EmptyPublicValues` ではなく既存の `InvalidPublicValues` で fail させ続け、`verify_sp1_groth16` の早期チェックも `EmptyPublicValues` のままにする (現状) — どちらでも整合するが、変な「2 経路あるけど片方しか到達しない」状態だけは解消したい

### N-5 (should-fix): `cnft.rs` の `pubkey` インポートが未使用

- 場所: `crates/solana/src/cnft.rs:9-18` の `use solana_sdk::{..., pubkey, ...}`
- 観察: `pubkey!` マクロは `SPL_ACCOUNT_COMPRESSION_V2_ID` (cnft.rs:22-23) で使用。`pubkey::Pubkey` モジュール経路も `pubkey::Pubkey` (cnft.rs:14) で使用。問題なし — false positive
- 撤回: 改めて読み直したところ正常に使用されている。記録上 N-5 はスキップとする

### N-5'(nitpick): `extension.rs` の `verifier` ヘルパが test 内で重複定義可能

- 場所: `crates/solana/src/extension.rs:213-215`
  ```
  fn verifier() -> MockAttestationVerifier {
      MockAttestationVerifier::new()
  }
  ```
- 観察: 単一の `MockAttestationVerifier::new()` を呼ぶだけのヘルパ。`mock_process_response` と異なり実装が trivial で、callsite ごとに `MockAttestationVerifier::new()` を直接書いた方が短いケースが多い (7 箇所中 6 箇所が `&verifier()` 1 行)
- 影響度: ゼロ (テストの読みやすさ寄りの好み)
- 修正案: 維持で良い。なお `MockAttestationVerifier::new()` が今後 default 以外のセットアップに変わる可能性があるなら、ヘルパ集約はむしろ望ましい

### N-6 (should-fix): `signing_key::sign_transaction` の Pubkey not in signers エラーが運用上扱いにくい

- 場所: `crates/solana/src/signing_key.rs:81-86`
  ```
  let index = static_keys
      .iter()
      .take(num_signers)
      .position(|k| k == &pubkey)
      .ok_or_else(|| SigningKeyError::PubkeyNotInSigners(pubkey.to_string()))?;
  tx.signatures[index] = solana_sdk::signature::Signature::from(sig_bytes);
  ```
- 観察: TEE 公開鍵が signer 群に居なければ `Err` を返すように整備された (should-fix-013 fix)。これは正しい
- 問題: 呼び出し側 (`cnft.rs:248` の `signing_key.sign_transaction(&mut tx)?` を `process_extension` 経由で発火) は `CnftError::SigningFailed(SigningKeyError)` 経由で `ExtensionError::TxFailed(CnftError)` にラップされる。`ExtensionError` の thiserror メッセージ chain は `"Transaction construction failed: Signing failed: Public key XYZ not found in transaction signers"` になる。意味は通るがやや迂遠
- 副次的懸念: build_and_sign_mint_tx (cnft.rs:225-251) は `MintV2Builder` で TEE pubkey を必ず `tree_creator_or_delegate(Some(*tee_signing_pubkey))` に入れているので、`tree_creator_or_delegate` は writable signer (mpl-bubblegum の `MintV2` アカウントレイアウト依存) になるはず → 通常パスでは `PubkeyNotInSigners` は起きない。dead code 寄り
- 修正案 (どちらか):
  - (A) `PubkeyNotInSigners` 発火を本物のバグとして `panic!` に格上げ ("invariant violation: TEE pubkey must be a signer of the built mint tx")。これにより呼び出し側のエラーラッピングが軽くなる
  - (B) このエラーを `cnft::build_and_sign_mint_tx` 内で `debug_assert!` ベースの早期確認に置き換え、`SigningKeyError` には残さない

### N-7 (nitpick): `ApprovedVkeys::MAX_VKEYS` / `ApprovedMeasurements::MAX_ENTRIES` の境界テストが無い

- 場所: `programs/title-whitelist/src/lib.rs:539, 558`
- 観察: 容量上限 16 だが、`add_approved_*` を 16 回呼んだ後 17 回目で `VkeyRegistryFull` / `MeasurementRegistryFull` が出ることを確認するテストが (devnet にも unit にも) 無い
- 影響度: 機能的には `require!(registry.vkeys.len() < ApprovedVkeys::MAX_VKEYS, ...)` (lib.rs:77-80) で守られているので壊れない。テストカバレッジの問題のみ
- 修正案: program crate に program-test ベースの local validator テストを追加し、`MAX_VKEYS + 1` 回 add を試して最後の 1 回が `VkeyRegistryFull` で fail することを確認 (現プロジェクトに anchor-test/program-test の整備があるかは未確認)

### N-8 (nitpick): `KeyRevoked` イベントの payload が `signing_pubkey` だけ

- 場所: `programs/title-whitelist/src/lib.rs:258-260, 703-705`
- 観察: 取消時に発行されるイベントは `KeyRevoked { signing_pubkey }` のみで、`revoked_at` (タイムスタンプ) を含まない
- 問題: 監査ログ的にはイベントだけで「いつ取消したか」を再現できない。Solana RPC の `getSignaturesForAddress` から slot を取れば近似可能だが、event payload に embed する方が自己完結する
- 修正案: `KeyRevoked { signing_pubkey, revoked_at: clock.unix_timestamp }` に拡張。`revoke_key` 関数本体で `Clock::get()?` を呼び timestamp を取得

### N-9 (nitpick): `RegisterKey` の `WhitelistEntry::SIZE` 計算が新レイアウトに依存

- 場所: `programs/title-whitelist/src/lib.rs:514-518`
- 観察: `SIZE` は手計算式 (`8 + 32 + 8 + 8 + 64 + 1 + 1 + 1 = 123`)。Round 1 から `MAX_MEASUREMENT_LEN` が 65 になる/`StoredMeasurement` が `[u8; N] + u8` 以外のレイアウトに変わると即破綻
- 影響度: `MAX_MEASUREMENT_LEN` は今のところ将来変える計画は無さそう (lib.rs:436-443 のコメントで「新しい vendor が来たら bump して PDA migration」と明記)
- 修正案: `pub const SIZE: usize = 8 + std::mem::size_of::<WhitelistEntry>();` に置き換えたい所だが、Anchor の account macro 上 `WhitelistEntry` 自体は内部表現が padded する可能性があり一筋縄ではいかない。`borsh::BorshSchema` か anchor の `Space` trait (Anchor 0.30 で `InitSpace` macro が利用可能) で機械化するのが最も安全:
  ```
  #[account]
  #[derive(InitSpace)]
  pub struct WhitelistEntry { ... }
  ```
  と書けば `WhitelistEntry::INIT_SPACE` が自動計算される。`init` の `space = 8 + WhitelistEntry::INIT_SPACE` で使え、手計算が不要になる。ApprovedVkeys / ApprovedMeasurements も同様

## 全体所感

Round 1 の must-fix 4 件のうち 3 件は綺麗に解決した。残った must-fix-002 (KEY_EXPIRY 二重定義) は「コメントだけで防ぐ」という弱い対処に止まり、`title-whitelist` を client crate の dependency として組み込めば一行で根絶できる類の作業が先延ばしになっている。must-fix-001 (client mirror struct) は構造体レイアウトの整合は取れたが、実際の wire 読み取り経路が未実装で「動くかどうか確認できない」状態。

新規発見の中で深刻なのは **N-1 (error code shift によるテストの assertion 破壊)** と **N-2 (`add_placeholder_vkey_devnet` の instruction data 構築が固定長配列に length prefix を付ける誤り)**。前者は must-fix-004 の修正で `WhitelistError` enum 先頭に新 variant を挿入したことの副作用で、Anchor の error code 採番の暗黙 ABI を破壊している。後者は Round 1 では Borsh エンコードの正しさまで踏み込まなかったため拾えなかったが、devnet 上の `ApprovedVkeys` PDA に意図と異なる vkey_hash が登録されている (または冪等 skip 分岐が常に取られて何も登録されていない) 状態が継続している可能性が高い。

`register_key` の確認順序 (should-fix-002) と `parse_public_values` の末尾検証 (should-fix-003) は Round 1 修正案を完全に取り込んでおり、設計品質が一段上がった。CU budget の collection 有無分岐 (should-fix-005) も「実測値に基づいて 250k/400k を出し分ける」という細やかな実装で、本番運用での CU 超過事故リスクが下がっている。

admin 鍵移管 (should-fix-012) は依然「documenting the gap」段階。OSS 公開時の最重要懸念点として変わらず残るので、別タスクとしてフェーズ切り (e.g. v0.1.3 で `transfer_admin` ix + ApprovedVkeys.admin を真実とする refactor) を切ることを強く推奨する。
