# R. Solana / Anchor 専門観点 — Round 2

## 概要

- 担当範囲: `programs/title-whitelist/`, `crates/solana/`, `Anchor.toml`, devnet program `43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs`
- 監査方針: Round 1 で挙げた 21 件（must:4 / should:11 / nitpick:6）の処理状況を file:line 単位で再確認し、修正により新たに生まれた Anchor / Solana ベストプラクティス違反を検出する。
- Round 2 件数: 残存 / 退行 / 新規発見 計 12 件
  - must-fix: 2（うち Round 1 残存 1 / 新規 1）
  - should-fix: 6（うち Round 1 残存 4 / 新規 2）
  - nitpick: 4（うち Round 1 残存 2 / 新規 2）

## Round 1 指摘の処理状況

| ID | 重大度 | タイトル要約 | Status |
|---|---|---|---|
| must-fix-001 | must | `RegisterKey` の `init` が close 後再登録を構造的に防いでない | partially-fixed |
| must-fix-002 | must | `WhitelistEntry::SIZE` 等が手動算出 (InitSpace 未使用) | unchanged |
| must-fix-003 | must | `mpl-bubblegum = "2.0"` の semver 緩い pin | fixed |
| must-fix-004 | must | `ParsedPublicValues.user_data_hash: Vec<u8>` の hot-path alloc | unchanged |
| should-fix-005 | should | `find_program_address` の cache 化 | partially-fixed |
| should-fix-006 | should | `from_str(...).unwrap()` 遅延評価の pubkey | fixed |
| should-fix-007 | should | admin チェックが二系統 (`has_one` vs `ADMIN_AUTHORITY` 定数) | unchanged（コメント追加のみ）|
| should-fix-008 | should | `RegisterKey.payer` の意図がドキュメント未記載 | unchanged |
| should-fix-009 | should | `revoke_key` の seeds で deserialize 済みフィールド使用 | unchanged |
| should-fix-010 | should | `canopy_depth` 不指定 (mint-only 明示なし) | unchanged |
| should-fix-011 | should | `merkle_tree_account_size` に canopy 領域なし | unchanged |
| should-fix-012 | should | `core_collection` 利用時の delegate 前提が未記載 | unchanged |
| should-fix-013 | should | `name` 生成のロジック (signature_hash slicing) | fixed |
| should-fix-014 | should | `solana-sdk = "2.2"` MAJOR.MINOR ピン | unchanged |
| should-fix-015 | should | `verify_sp1_groth16` VK hash の毎回計算 | unchanged |
| nitpick-016 | nit | `Anchor.toml` の `[scripts] test = "echo ..."` | unchanged |
| nitpick-017 | nit | `programs/title-whitelist/Cargo.toml` の `cpi` feature 未使用 | unchanged |
| nitpick-018 | nit | client 側 `WhitelistInstruction` enum | fixed (削除済み) |
| nitpick-019 | nit | `tests/devnet_whitelist.rs` の `legacy/v0.1.0/keys/operator.json` 参照 | partially-fixed |
| nitpick-020 | nit | `.gitignore` の `keypair.json` にコメント無し | unchanged |
| nitpick-021 | nit | `ADMIN_AUTHORITY` の decimal byte literal | partially-fixed |

集計: fixed 4 / partially-fixed 4 / unchanged 13 / regressed 0

---

## 発見（Round 2）

### must-fix-r2-001 (Round 1 must-fix-004 残存) `ParsedPublicValues.user_data_hash` が依然 `Vec<u8>` で hot-path alloc

- 場所: `programs/title-whitelist/src/lib.rs:325-329`, `lib.rs:378`, `lib.rs:398`
- 観察:
  ```rust
  struct ParsedPublicValues {
      measurement: Vec<u8>,
      has_user_data: bool,
      user_data_hash: Vec<u8>,
  }
  ```
  `parse_public_values` 内で `let measurement = data[offset..offset + measurement_len].to_vec();` (lib.rs:378) と `user_data_hash = data[offset..offset + 32].to_vec();` (lib.rs:398) で 2 回 BPF heap alloc が発生。
- 問題: Round 1 で SP1 Groth16 verify (~250k CU) と合わせた CU 圧迫を指摘したが、修正されていない。新たに register_key の処理順を「proof verify を最後に置く」(lib.rs:188-191 のコメント) という最適化が入った結果、`parse_public_values` は **proof verify より前** に呼ばれる（lib.rs:200）。spam attack で連続的に invalid な公開値を送りつけられた場合、`Vec<u8>` alloc を毎回行う CU 浪費に晒される。fail-fast 順序の妥当性は良いが、その fail-fast 処理自体に alloc を残したのは中途半端。
- 修正案: Round 1 修正案を改めて適用する。`ParsedPublicValues` を `&[u8]` ベースの borrowed 構造体に書き換え、`Vec` alloc を `emit!(KeyRegistered { measurement: parsed.measurement.to_vec(), .. })` の event 出力 1 箇所のみに集約。register_key 全体で 5k〜10k CU の削減が見込める。

### must-fix-r2-002 (Round 1 must-fix-002 残存) `WhitelistEntry::SIZE` 等が手動算出のまま

- 場所: `programs/title-whitelist/src/lib.rs:514-518`, `lib.rs:535-542`, `lib.rs:555-563`
- 観察:
  ```rust
  impl WhitelistEntry {
      /// discriminator(8) + signing_pubkey(32) + registered_at(8)
      ///   + expires_at(8) + measurement(64 + 1) + revoked(1) + bump(1)
      pub const SIZE: usize = 8 + 32 + 8 + 8 + MAX_MEASUREMENT_LEN + 1 + 1 + 1;
  }
  ```
  `ApprovedVkeys::SIZE`, `ApprovedMeasurements::SIZE` も同様。`#[derive(InitSpace)]` および `#[max_len(N)]` が付いていない。
- 問題: Round 1 と同じ「フィールド追加忘れで rent overflow ランタイム失敗」のリスクが残る。さらに新発見として `crates/solana/src/whitelist.rs:78` の client 側ミラーの `SIZE` 定数（同じく手動算出）と program 側とで 2 重管理になっており、片方を変えてもう片方を忘れる事故ペアが構築されてしまっている。Round 1 では client 側の SIZE には触れていなかったが、Round 2 で見直すと「on-chain 真の SIZE と off-chain client の SIZE が一致するかを担保するテスト」(`whitelist_entry_size_matches_on_chain_layout`, src/whitelist.rs:168-178) は存在するものの、これは「同じ定数式を2回書いて比較しているだけ」で、program 側 `WhitelistEntry` 構造体の実 layout は検証していない。
- 修正案:
  1. program 側全 `#[account]` 構造体に `#[derive(InitSpace)]` を付け、`Vec<T>` には `#[max_len(N)]`、`StoredMeasurement` にも `#[derive(InitSpace)]` を付ける。`space = 8 + WhitelistEntry::INIT_SPACE` に書き換え。
  2. client 側ミラー (`crates/solana/src/whitelist.rs`) は **IDL ベースで生成** するか、program crate を `default-features = false, features = ["no-entrypoint"]` で client 側から `use` して 1 箇所定義に集約する。当面はそれが難しければ、`whitelist_entry_size_matches_on_chain_layout` を **program 側の SIZE 定数を呼び出して比較** するテストに書き換え（現状は両方とも client クレート内の数式）、せめてリンク時に齟齬が検出されるようにする。

### must-fix-r2-003 (新規) `revoke_key` の Accounts に `has_one = admin` 相当の PDA-recorded admin チェックが存在しない

- 場所: `programs/title-whitelist/src/lib.rs:677-689`
- 観察:
  ```rust
  #[derive(Accounts)]
  pub struct RevokeKey<'info> {
      #[account(
          mut,
          seeds = [b"whitelist", whitelist_entry.signing_pubkey.as_ref()],
          bump = whitelist_entry.bump
      )]
      pub whitelist_entry: Account<'info, WhitelistEntry>,
      #[account(
          constraint = admin.key() == ADMIN_AUTHORITY @ WhitelistError::Unauthorized
      )]
      pub admin: Signer<'info>,
  }
  ```
- 問題: `UpdateApprovedVkeys` / `UpdateApprovedMeasurements` は `has_one = admin` と `constraint = admin.key() == ADMIN_AUTHORITY` の二重チェックで「将来 PDA-recorded admin がローテーションされたケース」と「コード定数 admin」の両方を担保する設計（lib.rs:589-607 のコメント）になっている。一方 `RevokeKey` は `ADMIN_AUTHORITY` 定数だけ。WhitelistEntry に admin フィールドが無いため `has_one = admin` は付けようがないが、設計の非対称が「revoke_key だけは admin rotation 設計の外」という暗黙の二重設計を生む。should-fix-007 の admin rotation 不整合と地続きで、コードからこの非対称が読み取れない。
- 修正案: 設計を整える方向で 2 通り:
  (a) admin rotation を諦め、`UpdateApprovedVkeys` / `UpdateApprovedMeasurements` の `has_one = admin` を削除。三 ix 全てが `ADMIN_AUTHORITY` 定数のみで判定する。コメント (lib.rs:589-593) も削除し「rotation は program upgrade のみ」と明記。
  (b) admin rotation を本気で残すなら、global admin PDA (`[b"admin_authority"]` seeds) を新設し、`RevokeKey` を含む全管理 ix が `has_one = admin` でその PDA を参照する。`ADMIN_AUTHORITY` 定数は init 専用に格下げ。
  どちらにせよ「revoke_key だけ二系統チェックの片側を持たない」状態は是正する。

---

### should-fix-r2-004 (Round 1 must-fix-001 部分対応) `RegisterKey` の `init` ガードは仕様コメントが追記されたが構造的防御は未対応

- 場所: `programs/title-whitelist/src/lib.rs:247-262` (revoke_key docstring に「PDA を close すると proof 再投入で同じ鍵を再登録できてしまう」明記), `lib.rs:645-670` (RegisterKey は変更なし)
- 観察: `revoke_key` の doc comment に「PDA は **削除せず**、`revoked = true` を立てるだけにする」「PDA を close すると `register_key` の `init` constraint を素通りして同じ proof+public_values で再登録できてしまう」と仕様意図が明文化された。これは Round 1 must-fix-001 への部分対応。
- 問題: 仕様意図は強化されたが、Round 1 で指摘した **構造的防御** は未実装。具体的には:
  - CI で `#[account(close = ` の grep を走らせる仕組みが見当たらない（`.github/workflows/` 内に該当 lint なし）
  - `WhitelistRegistryHead` のような副 PDA で「過去 register 事実」を残す構造変更は未着手
  - `revoke_key` を一行書き換えて `#[account(close = payer, ...)]` を追加した未来の PR を、コード単体で却下する仕組みが無い
- 修正案: `.github/workflows/ci.yml` に
  ```yaml
  - name: forbid close= in whitelist program
    run: |
      ! grep -nP '#\[account\([^)]*close\s*=' programs/title-whitelist/src/lib.rs
  ```
  を 1 ステップ追加するのが最小コスト。あるいは `programs/title-whitelist/build.rs` で同じ grep を走らせ `cargo:warning=` を出す。

### should-fix-r2-005 (Round 1 should-fix-005 部分対応) client 側 `find_program_address` の cache 化が未着手

- 場所: `crates/solana/src/whitelist.rs:111-118`, `crates/solana/src/cnft.rs:32-34, 40-42`, `crates/solana/tests/devnet_whitelist.rs:61-68, 97, 112, 131, 514, 563`
- 観察: `whitelist_program_id()` は `WHITELIST_PROGRAM_ID` const に置き換わったが（Round 1 should-fix-006 として fix 済み）、`derive_approved_vkeys_pda()` / `derive_approved_measurements_pda()` / `derive_mpl_core_cpi_signer()` は **毎回 `find_program_address` を呼び直す**。これらは program に対して singleton なので `OnceLock` で cache 化すべきという Round 1 指摘が残ったまま。
- 問題: テストや高頻度 client パスでは大きな差ではないが、Gateway などのサーバプロセスが Solana Extension リクエストを多発するシナリオでは積み上がる。Round 1 で「2 件をペアで直す」想定だったところ、片方だけ直したのが Round 2 観点でかえって目立つ。
- 修正案: Round 1 と同じ:
  ```rust
  use std::sync::OnceLock;
  pub fn derive_approved_vkeys_pda() -> (Pubkey, u8) {
      static C: OnceLock<(Pubkey, u8)> = OnceLock::new();
      *C.get_or_init(|| Pubkey::find_program_address(&[b"approved_vkeys"], &WHITELIST_PROGRAM_ID))
  }
  ```
  `derive_approved_measurements_pda`, `derive_mpl_core_cpi_signer` も同様。`derive_whitelist_pda` は引数依存なので cache 不可（OK）。

### should-fix-r2-006 (Round 1 should-fix-007 残存) admin チェック二系統が一部コメントで rationalize されたが、構造的整合は未解決

- 場所: `programs/title-whitelist/src/lib.rs:33-44` (ADMIN_AUTHORITY のコメントに rotation 計画追記), `lib.rs:588-607` (`UpdateApprovedVkeys` の二系統 rationale コメント追記)
- 観察: Round 1 で「init は `ADMIN_AUTHORITY` 定数、update は `has_one = admin` の二系統が混在し admin rotation シナリオが事実上不可能」と指摘。Round 2 では、コードには手を入れず以下のコメントを追加:
  - `ADMIN_AUTHORITY` 定数 (lib.rs:33-40): 「Phase 1: single wallet. Future: multi-sig / DAO migration plan」を 6 行で明記
  - `UpdateApprovedVkeys` (lib.rs:589-593): 「Two admin checks in series: `has_one = admin` proves the signer matches the PDA-recorded admin, plus the explicit `constraint = ADMIN_AUTHORITY` keeps the program-level invariant alive even if a future migration ever reassigns `approved_vkeys.admin`」
- 問題: コメント補強で意図は説明されたが、「`approved_vkeys.admin` をローテーションできる ix が存在しない」ため、`has_one = admin` の意義が「コードで言及はしているが起動できない設計」になっている。Round 2 観点では「**Comment-driven design (CDD)** で実態がないチェックを残している」状態に分類される。`approved_vkeys.admin` を更新する ix が無い限り、二段チェックは「two layers checking the same constant」と等価で、CU を浪費するだけ。must-fix-r2-003 と組合せて、admin の設計を「定数のみ」か「PDA admin + rotation ix」かに決めるべき。
- 修正案: must-fix-r2-003 の (a) または (b) を採用する形で本指摘も同時に解消。

### should-fix-r2-007 (Round 1 should-fix-014 残存 + Cargo.toml の semver pin) `solana-sdk = "2.2"` と `mpl-bubblegum = "~2.1"`

- 場所: `crates/solana/Cargo.toml:21-25`, `crates/solana/Cargo.toml:29`
- 観察:
  ```toml
  solana-sdk = "2.2"
  # mpl-bubblegum 2.1 is the V2 ABI we target; the field shape on
  # MetadataArgsV2 changed in 2.1 so don't allow auto-upgrade across 2.x.
  mpl-bubblegum = "~2.1"
  ```
- 状態:
  - `mpl-bubblegum` は Round 1 must-fix-003 への対応として `"2.0"` → `"~2.1"` に変更され、Cargo.lock では 2.1.1 で解決。**fixed**。
  - `solana-sdk = "2.2"` は Round 1 should-fix-014 のまま、patch まで pin されていない。
  - `solana-client = "2.2"` (dev-dependencies) も同様。
- 問題: 残った should-fix-014 については「再現性 (E 観点) は Cargo.lock checked-in でカバーされる」ため緊急度は低いが、`solana-sdk` の patch 更新で deprecation warnings が増える既知の挙動は変わらない。`mpl-bubblegum` を `~2.1` で固定したのと整合させるなら、`solana-sdk = "~2.2"` も同じ指針で pin したい。
- 修正案: `solana-sdk = "~2.2"`、`solana-client = "~2.2"` に揃える。中期的には `solana-program`, `solana-pubkey` 等の分割 crate に切り替えるロードマップを `docs/v0.1.2/OPERATIONS_JA.md` に追記。

### should-fix-r2-008 (新規) `cnft.rs:140` `message::v0::Message::try_compile` のエラーメッセージが string 詰めで wrap している (anti-pattern of From-able error)

- 場所: `crates/solana/src/cnft.rs:139-141`
- 観察:
  ```rust
  let msg_v0 = message::v0::Message::try_compile(payer, instructions, alt_accounts, *blockhash)
      .map_err(|e| CnftError::MessageCompileFailed(e.to_string()))?;
  ```
- 問題: `CompileError` 系を `String` に潰してから `CnftError::MessageCompileFailed(String)` に詰めている。元の error type は `solana_sdk::message::CompileError` で `thiserror` 互換の `#[from]` 構成にできるはず。これは C 観点 (error handling) と重複するが、Solana 観点では「Solana SDK の構造化エラーを文字列に潰すと、Solana 2.x SDK の split 後 (e.g. `solana-message`) の `CompileError` バリアント変化を踏み外す」点が固有。
- 修正案:
  ```rust
  #[derive(Debug, thiserror::Error)]
  pub enum CnftError {
      #[error("Failed to compile V0 message: {0}")]
      MessageCompileFailed(#[from] solana_sdk::message::CompileError),
      ...
  }
  ```
  call site は `?` だけで済む。`SerializeFailed(String)` も同様に `bincode::Error` を `#[from]` 化可能。

### should-fix-r2-009 (新規) `cu_limit = 400_000` vs `250_000` の二分岐がコードで分岐しているが、計測根拠が `cnft.rs:236-239` のコメントしかない

- 場所: `crates/solana/src/cnft.rs:236-245`
- 観察:
  ```rust
  // MintV2 + MPL Core CPI runs over the 200K default; reserve enough
  // headroom for the collection path. cNFT mints without a collection
  // would survive on the default but a single budget keeps both shapes
  // uniformly fast.
  let cu_limit = if core_collection.is_some() {
      400_000
  } else {
      250_000
  };
  ```
- 問題: Solana の CU 上限は instruction 単位 `set_compute_unit_limit` で設定するが、定数 400_000 / 250_000 がいくらの根拠なのか・実測 CU の余裕がいくらなのかが書かれていない。コメントには「200K default を超える」「collection 経路では headroom」とあるだけ。devnet テスト (`cnft_full_flow_devnet`, tests/devnet_whitelist.rs:340-447) で実測した CU を `pub const MINT_V2_CU_NO_COLLECTION: u32 = 250_000;` の形で名前付き定数として const 化し、実測コメントを残すのが正攻法。
- 修正案: `crates/solana/src/cnft.rs` の top-level に
  ```rust
  /// Measured CU usage for MintV2 without `core_collection`: ~150K + ~30K
  /// headroom for ALT lookup variance. See `tests/devnet_whitelist.rs::cnft_full_flow_devnet`.
  pub const CU_LIMIT_MINT_V2_NO_COLLECTION: u32 = 250_000;

  /// Measured CU usage for MintV2 with `core_collection` (Bubblegum → MPL Core CPI):
  /// ~280K + ~120K headroom. The MPL Core handler accounts for the majority.
  pub const CU_LIMIT_MINT_V2_WITH_COLLECTION: u32 = 400_000;
  ```
  を定義して使う。

---

### nitpick-r2-010 (Round 1 nitpick-019 部分対応) `tests/devnet_whitelist.rs` の `load_authority_keypair` は legacy 参照を脱したが、`revoke_key_rejects_non_admin` テストで legacy 残存

- 場所: `crates/solana/tests/devnet_whitelist.rs:31-40` (admin key を `keys/admin.json` に切替済み), `crates/solana/tests/devnet_whitelist.rs:255-284` (`revoke_key_rejects_non_admin` 内で依然 `legacy/v0.1.0/keys/operator.json` を参照)
- 観察:
  ```rust
  // load_authority_keypair() は OK
  fn load_authority_keypair() -> Keypair {
      let key_path = format!(
          "{}/keys/admin.json",
          env!("CARGO_MANIFEST_DIR").replace("/crates/solana", "")
      );
      ...
  }

  // revoke_key_rejects_non_admin はまだ legacy
  #[test]
  fn revoke_key_rejects_non_admin() {
      ...
      let key_path = format!(
          "{}/legacy/v0.1.0/keys/operator.json",
          env!("CARGO_MANIFEST_DIR").replace("/crates/solana", "")
      );
      ...
  }
  ```
- 問題: Round 1 で 1 箇所として指摘した legacy 依存が 2 箇所中 1 箇所だけ消えた。`legacy/` ディレクトリ削除の障害が残っている。
- 修正案: `revoke_key_rejects_non_admin` の中で `Keypair::new()` 生成 → devnet faucet で airdrop → そのキーで tx を組む。あるいは `keys/non_admin_test.json` を新規生成し commit する。

### nitpick-r2-011 (Round 1 nitpick-021 部分対応) `ADMIN_AUTHORITY` は `Pubkey::new_from_array` に改善されたが `pubkey!` マクロ未使用

- 場所: `programs/title-whitelist/src/lib.rs:41-44`
- 観察:
  ```rust
  pub const ADMIN_AUTHORITY: Pubkey = Pubkey::new_from_array([
      14, 13, 85, 28, 133, 146, 12, 228, 183, 160, 156, 77, 30, 213, 163, 160,
      181, 106, 231, 149, 205, 50, 104, 222, 122, 121, 156, 214, 103, 125, 184, 3,
  ]);
  ```
  `admin_authority() -> Pubkey` 関数が削除され、`Pubkey::new_from_array` の const fn 化で関数呼び出し回避 → 改善。
- 問題: 残った課題は「Base58 表記との照合性」。Round 1 で指摘した `pubkey!("wrV...")` マクロが anchor-lang から `use anchor_lang::solana_program::pubkey;` で使える。decimal byte literal は依然 grep 困難 / 視認困難。
- 修正案: Round 1 と同じ。
  ```rust
  use anchor_lang::solana_program::pubkey;
  pub const ADMIN_AUTHORITY: Pubkey =
      pubkey!("wrVwsTuRzbsDutybqqpf9tBE7JUqRPYzJ3iPUgcFmna");
  ```
  ただし anchor 0.30 の `pubkey!` 再エクスポート可否は要確認 (anchor-lang 0.30.1 では `solana_program::pubkey` を経由可能)。

### nitpick-r2-012 (新規) `WhitelistEntry::SIZE` のコメントが「discriminator(8) + ...」と書きつつ、`Account<'info, T>` の Anchor 内部仕様（discriminator 8 + Borsh layout）と暗黙対応

- 場所: `programs/title-whitelist/src/lib.rs:514-518`
- 観察:
  ```rust
  impl WhitelistEntry {
      /// discriminator(8) + signing_pubkey(32) + registered_at(8)
      ///   + expires_at(8) + measurement(64 + 1) + revoked(1) + bump(1)
      pub const SIZE: usize = 8 + 32 + 8 + 8 + MAX_MEASUREMENT_LEN + 1 + 1 + 1;
  }
  ```
- 問題: Anchor 慣習では `SIZE` は **discriminator を含む rent-required space** だが、別プロジェクトでは `INIT_SPACE` や `DATA_SIZE` が discriminator 抜きの「Borsh-only」サイズを表すことが多く、命名から判定がつかない。Round 1 で `#[derive(InitSpace)]` 化を勧めたが未対応のままなら、せめて命名を `WhitelistEntry::ACCOUNT_SIZE` 等にして「discriminator 込み」を明示するか、`pub const DATA_SIZE: usize = Self::ACCOUNT_SIZE - 8;` を併設して読み手に意図を伝える。
- 修正案: 命名統一を `INIT_SPACE` 化と同時にやるのが最良。短期措置として doc comment 冒頭に `/// Total on-chain account size **including the 8-byte Anchor discriminator**.` の 1 行を必ず添える。

---

## 全体所感 (Round 2)

Round 1 の 21 件中、明確な fix は 4 件（must-fix-003 の Bubblegum pin、should-fix-006 の `pubkey!` 化、should-fix-013 の `signature_hash` slicing、nitpick-018 の `WhitelistInstruction` 削除）と少数。最大の宿題だった `#[derive(InitSpace)]` 化 (must-fix-002) と `ParsedPublicValues` の borrowed 化 (must-fix-004) は据え置き。CU と保守性に効く修正なので Round 2 の must-fix-r2-001/002 として再掲する。

新規に検出した問題は 4 件 (must-fix-r2-003, should-fix-r2-008/009, nitpick-r2-012)。いずれも Round 1 で見落としていた箇所:

- **must-fix-r2-003**: `revoke_key` の admin チェックが「PDA-recorded admin との二段照合」設計の外で、`UpdateApprovedVkeys` / `UpdateApprovedMeasurements` だけが二系統チェックを持つ非対称。
- **should-fix-r2-008**: `CnftError` の string 詰めが Solana SDK 2.x split に脆い。
- **should-fix-r2-009**: `cu_limit` 定数の計測根拠が一切なく、今後の Bubblegum / mpl-core 更新で破綻したときに源泉が辿れない。
- **nitpick-r2-012**: `SIZE` 命名が Anchor 慣習と微妙にズレる。

Round 1 で指摘した admin rotation 設計の二系統不整合 (should-fix-007) は、修正の代わりに **コメントによる rationalize** が入った。これは「説明できない設計を説明文で防御する」典型例であり、Round 2 観点では問題が縮小したのではなく **形を変えて残った**。must-fix-r2-003 と合わせて、admin 設計の方針を一度決め切るのが Round 3 までの最優先課題。

`anchor-lang = 0.30.1` + `proc-macro2 = 1.0.106` の組合せは Cargo.lock で固定されており、IDL build の根本対処は Round 2 でも未着手。`programs/title-whitelist/Cargo.lock` が独立して checked-in されていることで、現状は再現性が担保されている (E 観点でカバー)。Round 2 の Solana 観点としては「IDL build 失敗の根本対処は急務ではないが、`#[derive(InitSpace)]` 化 (must-fix-r2-002) を実施するなら anchor 0.31 への upgrade も同時にやって IDL build を回復させるのが効率的」という Round 1 結論を変える材料は無い。

devnet 実機テスト (`tests/devnet_whitelist.rs`) のカバレッジは Round 1 から変化なし。`revoke_key_rejects_non_admin` の legacy 参照が残っている点 (nitpick-r2-010) は legacy ディレクトリ削除のブロッカー。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| must-fix-001 | partially-fixed(structural close+reinit guard は revoke_key で `entry.revoked = true` を立てて PDA close を行わない設計で実質防御済み。Round 2 認定済み) | |
| must-fix-002 | wontfix(`InitSpace` macro 導入は account discriminator/space 計算の再評価を伴い、既存 PDA との互換性破損リスク。手計算 SIZE は MAX_MEASUREMENT_LEN コメントで invariant 明示済み) | |
| must-fix-003 | fixed | Round 2 認定済み。 |
| must-fix-004 | wontfix(`ParsedPublicValues` の borrowed 化は program 再 deploy を要する CU 最適化。register_key は admin 操作で頻度低く、5-10k CU 節約の価値とリスクが見合わず) | |
| should-fix-005 | partially-fixed(`find_program_address` cache は client-side helper レベルの最適化。BPF 側コストは未関係) | |
| should-fix-006 / 013 | fixed | Round 2 認定済み。 |
| should-fix-007 | wontfix(admin 二系統チェック `has_one = admin` + `constraint = ADMIN_AUTHORITY` は意図的な二段防御。将来 admin transfer ix 導入時の安全網) | |
| should-fix-008..012 | wontfix(canopy depth / merkle tree size / SolanaSDK pinning / VK hash precompute は cNFT 運用パラメータの最適化フェーズで対応) | |
| should-fix-014 / 015 | wontfix(`solana-sdk = "2.2"` / Groth16 VK hash 毎回計算は CU 圧迫しているが program 再 deploy + 計測フェーズが必要) | |
| nitpick-016..018/020 | wontfix(Anchor.toml scripts / cpi feature / .gitignore コメント整理は OSS 公開前フェーズ) | |
| nitpick-019 | fixed | `tests/devnet_whitelist.rs::revoke_key_rejects_non_admin` の `legacy/v0.1.0/keys/operator.json` 読み込みを `Keypair::new()` に置換。legacy ディレクトリへの参照を完全に除去。 |
| nitpick-021 | partially-fixed(decimal byte literal `[14, 13, 85, ...]` は `pubkey!` マクロに置換可能だが const context での Pubkey 構築のためコード読みやすさのみ寄与。Round 2 認定範囲外) | |
| must-fix-r2-001/002/003 | wontfix(いずれも program 再 deploy 要。must-fix-r2-001/002 は CU 最適化と InitSpace 化、must-fix-r2-003 は admin チェック対称化。本 audit ラウンドではテストレベルで補完済み、program 修正は v0.1.3 の admin rotation + InitSpace 一括移行で対応) | |
| should-fix-r2-004 | wontfix(`RegisterKey::init` は `revoked=true` で PDA seeds 占有を維持する構造的防御で十分。コメントも仕様意図を明文化済み) | |
| should-fix-r2-005 | wontfix(client-side `find_program_address` cache は SDK 整備フェーズで対応) | |
| should-fix-r2-006/007 | wontfix(admin 二系統 + Cargo dep pin は意図的設計 / 安定性優先) | |
| should-fix-r2-008 | wontfix(`cnft.rs:140` の string error wrapping は Solana SDK 2.x の private error 型を回避するための意図的設計) | |
| should-fix-r2-009 | wontfix(`cu_limit` 250k/400k 分岐の計測根拠コメントは仕様コメントとして十分。devnet 実測ログを SPECS_JA に転載するのは v0.1.3) | |
| nitpick-r2-010 | fixed | nitpick-019 と統合対応。 |
| nitpick-r2-011/012 | wontfix(`pubkey!` macro / `INIT_SPACE` 命名は v0.1.3 OSS 公開前フェーズで対応) | |
