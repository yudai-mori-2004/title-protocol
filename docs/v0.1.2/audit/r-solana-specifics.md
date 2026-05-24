# R. Solana / Anchor 専門観点

## 概要

- 担当範囲: `programs/title-whitelist/`, `crates/solana/`, `Anchor.toml`, devnet にデプロイ済みプログラム `43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs`
- 監査方針: Anchor 0.30 / Solana 2.x / mpl-bubblegum 2.x のベストプラクティスと、SP1 Groth16 verify を on-chain で動かす際の固有の落とし穴を1文単位で確認。セキュリティ観点 (G) や crate 深掘り (K5) と重複する事項は意図的に許容し、Solana 視点で改めて記述する。
- 件数: must-fix 4 / should-fix 11 / nitpick 6 = 計 21 件

## 重大度別内訳

- must-fix: 4 件
- should-fix: 11 件
- nitpick: 6 件

## 発見

### must-fix-001 `RegisterKey` の `init` は revoke 後の再登録を防げない (Anchor account close ルールとの不整合)

- 場所: `programs/title-whitelist/src/lib.rs:597-619`, `lib.rs:244-252`
- 観察:
  ```rust
  #[account(
      init,
      payer = payer,
      space = WhitelistEntry::SIZE,
      seeds = [b"whitelist", signing_pubkey.as_ref()],
      bump
  )]
  pub whitelist_entry: Account<'info, WhitelistEntry>,
  ```
  `revoke_key` は PDA を close せず `revoked = true` フラグだけ立てる仕様。
- 問題: 仕様 (§6.2 「PDA削除…再投入で再登録できてしまう」) は守られているが、Anchor 0.30 の `init` は **「lamport が 0 かつ data が 0 のアカウント」も新規扱いにする** という挙動を持つ。`revoke_key` が将来「close する」変更を受けた瞬間にこの不変条件が破れる。コード単体では rationale (lib.rs:240-243 のコメント) でしか守られておらず、構造的に防護されていない。`#[account(init, ...)]` だけでは「同一 PDA への二回目の `register_key` を必ず失敗させる」保証がないため、`init` + `constraint = !whitelist_entry.revoked` のような防御は不可能（init 時には未初期化なので constraint は無意味）。
- 修正案: 仕様の意図を構造で固定するため、`WhitelistRegistryHead` という追加 PDA（`[b"registered", signing_pubkey]`）に「過去に register された事実」だけ残し、ここを `init` の対象にする。本体 `WhitelistEntry` は `init_if_needed` ではなく **常に新規 PDA に作る**。あるいは「PDA close を物理的に不可能にするための `bump = entry.bump` を `RevokeKey` でも明示し、`#[account(close = ..)]` を絶対書かないこと」を Rust の `compile_error!` レベルで担保するモジュール構造に変更する。最低限、`RegisterKey` の docstring と `revoke_key` の docstring に「`init` がガードしているのは『未割当 PDA』だけであり、`#[account(close = ..)]` の追加は禁忌」と明記し、CI で `close =` の grep を回す。

### must-fix-002 `WhitelistEntry::SIZE` が Anchor の `InitSpace` / `#[derive(InitSpace)]` ではなく手動算出で、64+1+1 のアラインメントが暗黙

- 場所: `programs/title-whitelist/src/lib.rs:475-479`
- 観察:
  ```rust
  impl WhitelistEntry {
      /// discriminator(8) + signing_pubkey(32) + registered_at(8)
      ///   + expires_at(8) + measurement(64 + 1) + revoked(1) + bump(1)
      pub const SIZE: usize = 8 + 32 + 8 + 8 + MAX_MEASUREMENT_LEN + 1 + 1 + 1;
  }
  ```
- 問題: (1) Anchor 0.30 では `#[derive(InitSpace)]` で同等の計算をコンパイル時に行えるが、これを使わず手動で書いている。フィールド追加時に `SIZE` 更新を忘れると Anchor の `init` が rent overflow で実行時失敗する。 (2) `StoredMeasurement` は `[u8; 64] + u8` の構造で、Anchor が AnchorSerialize で詰めるサイズ (65 bytes) は計算と一致するが、`#[repr(C)]` ではないため Rust 側のメモリ表現と Borsh 表現がずれる可能性がある (現状は問題ないが、将来 `bytemuck::Pod` などの zero-copy 化を行うと壊れる)。 (3) `ApprovedVkeys::SIZE` (lib.rs:496-503), `ApprovedMeasurements::SIZE` (lib.rs:516-524) も同じく手動算出。
- 修正案: `WhitelistEntry`, `ApprovedVkeys`, `ApprovedMeasurements`, `StoredMeasurement` 全てに `#[derive(InitSpace)]` を付け、`space = 8 + WhitelistEntry::INIT_SPACE` に書き換える。`Vec<T>` には `#[max_len(N)]` 属性を付ける。これで `MAX_VKEYS` / `MAX_ENTRIES` 変更時のサイズ伝播が自動になる。

### must-fix-003 client (`crates/solana`) と program (`programs/title-whitelist`) で `mpl-bubblegum` のバージョンが不整合

- 場所: `crates/solana/Cargo.toml:18` (`mpl-bubblegum = "2.0"`), workspace `Cargo.lock` で実体は `2.1.1` (program 側は Bubblegum を直接依存していないので影響なし)
- 観察:
  - `crates/solana/Cargo.toml`: `mpl-bubblegum = "2.0"` (=2.1.1 が解決される)
  - `crates/solana/src/cnft.rs:9`: `use mpl_bubblegum::instructions::{CreateTreeConfigV2Builder, MintV2Builder};`
- 問題: 2.0 系と 2.1 系では `MetadataArgsV2` の field（特に `token_standard`）の `Option` 扱いが微妙に変わり、IDL Bug fix を含む。`"2.0"` 表記は cargo semver 上「2.0.0 ≤ x < 3.0.0」を意味するため、たまたま 2.1.1 が選ばれているだけで、`cargo update` で `2.x.y` の挙動差が出るリスクがある。再現性 (E 観点) と Solana 観点の両面で問題。
- 修正案: `mpl-bubblegum = "=2.1.1"` の厳密 pin、もしくは `mpl-bubblegum = "~2.1"` で 2.1.x に固定。`Cargo.lock` を必ず checked-in する（既に在る）。program 側が Bubblegum へ CPI する設計に変えた場合は、program 側にも同じ pin を入れる。

### must-fix-004 `ParsedPublicValues.user_data_hash` が `Vec<u8>` で zero-copy 比較ではなく、`require!` の左辺で alloc が発生する hot path

- 場所: `programs/title-whitelist/src/lib.rs:310-314`, `lib.rs:208-213`
- 観察:
  ```rust
  struct ParsedPublicValues {
      measurement: Vec<u8>,
      has_user_data: bool,
      user_data_hash: Vec<u8>,
  }
  ...
  let user_data = Sha256::digest(signing_pubkey);
  let expected_hash = Sha256::digest(user_data);
  require!(
      parsed.user_data_hash == expected_hash.as_slice(),
      WhitelistError::UserDataMismatch
  );
  ```
- 問題: SP1 Groth16 verify は ~250k CU 消費する非常に重い処理で、その後段にあるこの `register_key` は CU budget 上極めてタイトなはず。`user_data_hash: Vec<u8>` を `parse_public_values` 内で `to_vec()` (lib.rs:383) するのは BPF heap alloc を 1 回挟むため数千 CU を消費する。Solana program の hot path で `Vec<u8>` を作るのはアンチパターン。`user_data_hash` は常に 32 bytes 固定なので `[u8; 32]` または `&[u8]` で持つべき。同じ問題が `parsed.measurement: Vec<u8>` にもある (lib.rs:363, lib.rs:230)。
- 修正案: `ParsedPublicValues` を以下に書き換える:
  ```rust
  struct ParsedPublicValues<'a> {
      measurement: &'a [u8],  // 元データの slice
      has_user_data: bool,
      user_data_hash: Option<&'a [u8; 32]>,
  }
  ```
  `parse_public_values(data: &[u8]) -> Result<ParsedPublicValues<'_>>` に。`emit!(KeyRegistered { measurement: parsed.measurement.to_vec(), .. })` で alloc は event 1 箇所に集約。これで register_key の CU を 5k〜10k 削減できる見込み。

### should-fix-005 `find_program_address` (canonical bump 探索) が client hot path で多用されている

- 場所: `crates/solana/src/cnft.rs:26`, `cnft.rs:32`, `crates/solana/src/whitelist.rs:84`, `whitelist.rs:93`, `whitelist.rs:99`, `crates/solana/tests/devnet_whitelist.rs:61, 66, 68, 97, 112, 131, 514, 563`
- 観察: client 側のすべての PDA 導出が `Pubkey::find_program_address` を呼んでいる。
- 問題: `find_program_address` は 0xFF から 0x00 まで降順に試行する iterative 関数で、最悪 256 回 SHA-256 を回す。on-chain では `Pubkey::create_program_address` + 保存済み bump で済むが、off-chain でも同じ PDA を **同一プロセス内で複数回** 計算する場合は cache すべき。`approved_vkeys_pda` / `approved_measurements_pda` はプログラムに 1 つしか存在しない singleton なので、初回計算後に `OnceLock<Pubkey>` に入れるのが理想。
- 修正案: `crates/solana/src/whitelist.rs` に
  ```rust
  use std::sync::OnceLock;
  static APPROVED_VKEYS_PDA: OnceLock<(Pubkey, u8)> = OnceLock::new();
  pub fn derive_approved_vkeys_pda() -> (Pubkey, u8) {
      *APPROVED_VKEYS_PDA.get_or_init(|| {
          Pubkey::find_program_address(&[b"approved_vkeys"], &whitelist_program_id())
      })
  }
  ```
  と同様に `APPROVED_MEASUREMENTS_PDA`, `MPL_CORE_CPI_SIGNER`, `WHITELIST_PROGRAM_ID` を cache。

### should-fix-006 `spl_account_compression_v2_id()` と `whitelist_program_id()` が `from_str(...).unwrap()` の遅延評価

- 場所: `crates/solana/src/cnft.rs:37-39`, `crates/solana/src/whitelist.rs:77-79`
- 観察:
  ```rust
  pub fn spl_account_compression_v2_id() -> Pubkey {
      Pubkey::from_str("mcmt6YrQEMKw8Mw43FmpRLmf7BqRnFMKmAcbxE3xkAW").unwrap()
  }
  ```
- 問題: 関数呼び出しの度に Base58 decode が走る。Anchor の `declare_id!` 相当の **コンパイル時に `[u8; 32]` を生成する** API (`solana_program::pubkey!`) が使えるはず。実行時 `unwrap()` も panic 源として弱い。
- 修正案:
  ```rust
  use solana_sdk::pubkey;
  pub const SPL_ACCOUNT_COMPRESSION_V2_ID: Pubkey =
      pubkey!("mcmt6YrQEMKw8Mw43FmpRLmf7BqRnFMKmAcbxE3xkAW");
  pub const WHITELIST_PROGRAM_ID: Pubkey =
      pubkey!("43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs");
  ```
  関数版は thin wrapper として残す。`crates/solana/tests/devnet_whitelist.rs:25-29` の `WHITELIST_PROGRAM_ID: &str` も同じ Pubkey 定数に置換可能。

### should-fix-007 Anchor の `has_one = admin` 制約が `UpdateApprovedVkeys` / `UpdateApprovedMeasurements` にだけ付いていて、`InitializeApprovedVkeys` / `InitializeApprovedMeasurements` には `constraint = admin.key() == admin_authority()` の手書きチェックが残る

- 場所: `programs/title-whitelist/src/lib.rs:531-547`, `lib.rs:551-560`, `lib.rs:562-579`, `lib.rs:581-592`
- 観察: Init では `admin_authority()` 関数 (lib.rs:640-642) との照合、Update では `has_one = admin` (登録済み admin との照合)。二系統の admin チェックロジックが共存している。
- 問題: 2 種類の admin チェックがあると、`ADMIN_AUTHORITY` をローテートするには (a) `admin_authority()` 定数を変える (recompile + redeploy) か、(b) `ApprovedVkeys.admin` フィールドだけ書き換える、の二経路を整合させる必要がある。`has_one = admin` で「PDA に書かれた admin と signer が一致」を見ているのに、init は「コード定数の admin と signer が一致」で初期化する設計は、admin rotation のシナリオが事実上不可能。
- 修正案: 仕様 §6.2 で admin rotation 想定があるならば PDA のフィールド admin を Source of Truth にし、`set_admin(new_admin: Pubkey)` 命令を追加 (`has_one = admin` で現 admin が signer)。Init 時のチェックも `ADMIN_AUTHORITY` 定数依存をやめ、`payer = admin` だけで運用 (誰でも一度は init できるが、`init` 制約で 1 回だけになる)。あるいは仕様で「rotation しない」と決め、コード定数のみに統一する (両方持つのが最悪)。

### should-fix-008 `RegisterKey` の `payer` が任意の Signer で、誰でもガス代を払えば register できる設計の意図がコメントから読み取れない

- 場所: `programs/title-whitelist/src/lib.rs:616-617`
- 観察:
  ```rust
  #[account(mut)]
  pub payer: Signer<'info>,
  ```
- 問題: ZK proof と public_values で済むので「誰が tx を投げてもよい」設計は理にかなっているが、これは **書かれていない仕様** であり、初見の Solana 開発者は「admin だけが register できる」を期待する。実機 (devnet) テストでは `load_authority_keypair()` が payer を兼ねている (tests/devnet_whitelist.rs:165) ため誤解しやすい。
- 修正案: `RegisterKey` の docstring に「payer は任意の Signer で構わない。proof + public_values の検証で TEE の正規性が保証されるため」と1行追加。逆に「admin 限定にする」設計を採用するなら `constraint = payer.key() == admin_authority()` を追加。

### should-fix-009 `revoke_key` の Account context が `whitelist_entry.signing_pubkey` をシードに使っていて、PDA 再導出のコストが高い

- 場所: `programs/title-whitelist/src/lib.rs:626-638`
- 観察:
  ```rust
  #[account(
      mut,
      seeds = [b"whitelist", whitelist_entry.signing_pubkey.as_ref()],
      bump = whitelist_entry.bump
  )]
  pub whitelist_entry: Account<'info, WhitelistEntry>,
  ```
- 問題: Anchor は `seeds + bump` を `create_program_address` で検証するが、その前提として **PDA 自体を deserialize** する必要があり、deserialize した結果のフィールドをシードに使う循環が発生している。Anchor 0.30 は対応しているが、CU 消費は大きめ (~3k)。canonical bump (entry.bump) を使っているのは正しい。
- 修正案: `signing_pubkey: [u8; 32]` を `#[instruction]` で渡し、`seeds = [b"whitelist", signing_pubkey.as_ref()]` にすれば deserialize 前に PDA 検証が完了して数百 CU 削減。同時に「rev対象を tx の見た目で明示」できる利点もある:
  ```rust
  #[derive(Accounts)]
  #[instruction(signing_pubkey: [u8; 32])]
  pub struct RevokeKey<'info> {
      #[account(
          mut,
          seeds = [b"whitelist", signing_pubkey.as_ref()],
          bump = whitelist_entry.bump
      )]
      pub whitelist_entry: Account<'info, WhitelistEntry>,
      ...
  }
  pub fn revoke_key(ctx: Context<RevokeKey>, _signing_pubkey: [u8; 32]) -> Result<()> { ... }
  ```

### should-fix-010 Bubblegum V2 の `mint_v2` 呼び出しで `canopy_depth` の選択が無い

- 場所: `crates/solana/src/cnft.rs:77-115` (CreateTree), `crates/solana/tests/devnet_whitelist.rs:356-357` (depth=3, buffer=8)
- 観察: `CreateTreeConfigV2Builder` に `canopy_depth` を渡していない (デフォルト 0)。
- 問題: canopy_depth = 0 は **transfer/burn/update_metadata のような post-mint 命令で、Merkle proof を全 depth 分 (depth=14 で 14 個) AccountMeta に並べる必要がある**。Solana の TX 上限 1232 bytes / accounts 64 個に当たって死ぬのが定番。mint だけなら canopy 0 で問題ないが、Title Protocol は cNFT の transfer や update を想定していないとはどこにも書かれていないので、「mint only」を明示するか、canopy_depth を depth-5 程度に設定して post-mint 操作も可能にしておくべき。
- 修正案: (a) `build_create_tree_tx` に `canopy_depth: u32` パラメータを追加し、SPECS_JA §6.2 「cNFT は mint only」と明記する。 (b) `merkle_tree_account_size` の式に canopy 領域 `((1 << (canopy_depth + 1)) - 2) * 32` を加算する (現状の式では canopy 領域が account size に入っていないため、canopy>0 にすると create_account が rent 不足で失敗する)。

### should-fix-011 `merkle_tree_account_size` が canopy 領域を含まないため depth/buffer の組合せによっては off-by-N

- 場所: `crates/solana/src/cnft.rs:43-61`
- 観察:
  ```rust
  pub fn merkle_tree_account_size(max_depth: u32, max_buffer_size: u32) -> usize {
      // ...
      header_size + tree_header + b * change_log_size + path_size
  }
  ```
- 問題: spl-account-compression V2 のレイアウトには canopy 領域が含まれる (`((1 << (canopy + 1)) - 2) * 32` bytes)。現状 canopy=0 想定なので 0 bytes で正しいが、関数名が `merkle_tree_account_size` でカノピー対応している印象を与える。
- 修正案: 関数名を `merkle_tree_account_size_no_canopy` にするか、`canopy_depth` 引数を追加して `+ canopy_size(canopy_depth)` を加算。

### should-fix-012 `MintV2Builder` で `core_collection` を指定したときに `collection_authority` を `tee_signing_pubkey` にしているが、Anchor IDL の `collection_authority` は **collection の権限者** であって TEE 署名鍵ではない可能性

- 場所: `crates/solana/src/cnft.rs:193-199`
- 観察:
  ```rust
  builder
      .core_collection(Some(*collection))
      .collection_authority(Some(*tee_signing_pubkey))
      .mpl_core_cpi_signer(Some(mpl_core_cpi_signer));
  ```
- 問題: 仕様 §6.2「コレクションの発行権限を TEE の署名鍵に delegate する」に従えば、開発者が事前に `mpl-core` の `approve_collection_delegate` を呼んで TEE pubkey を delegate に設定しておく必要がある。コメントに「事前に delegate されている前提」と一切書かれていないため、運用者が「TEE 鍵を tree_creator_or_delegate にすれば自動で動く」と誤読する。
- 修正案: `build_mint_v2_ix` の docstring (line 148-149) に「caller must have previously delegated `tee_signing_pubkey` as collection delegate via `mpl_core::approve_collection_delegate` (Spec §6.2 コレクションの準備)」と明記。あるいは整合性チェックを行う helper を追加。

### should-fix-013 `name` 生成のロジックが `signature_hash[7..15]` の固定スライスで panic 安全だが意図不明

- 場所: `crates/solana/src/cnft.rs:161-166`
- 観察:
  ```rust
  let hash_suffix = if signature_hash.len() > 7 {
      &signature_hash[7..signature_hash.len().min(15)]
  } else {
      signature_hash
  };
  let name = format!("Title #{hash_suffix}");
  ```
- 問題: signature_hash の形式 (`sha256:...`) の prefix `sha256:` (7 bytes) を skip して 8 桁取る意図と思われるが、コメントが無い。`signature_hash` が UTF-8 マルチバイト文字の場合 `&str[..]` でパニックする。仕様で `signature_hash` が常に ASCII (`sha256:hex`) であることが保証されているなら OK だが、入力検証が無い。
- 修正案:
  ```rust
  // Strip the "sha256:" prefix and take the first 8 hex chars for a stable
  // short id. signature_hash is always ASCII per Spec §1.5.
  let hex = signature_hash
      .strip_prefix("sha256:")
      .unwrap_or(signature_hash);
  let short = &hex[..hex.len().min(8)];
  let name = format!("Title #{short}");
  ```

### should-fix-014 Solana 2.x SDK の `solana-sdk = "2.2"` 直依存があり、近い将来分割される `solana-program` / `solana-pubkey` に追従できない

- 場所: `crates/solana/Cargo.toml:17`, `crates/solana/Cargo.toml:dev-dependencies` (`solana-client = "2.2"`)
- 観察: Solana 2.x 系では SDK の monolithic crate がモジュール単位 (`solana-pubkey`, `solana-instruction`, `solana-program`, `solana-sdk-ids` 等) に分割されつつあり、Agave 2.1+ では `solana-sdk` の再エクスポートが deprecation 警告を出すモジュールがある。
- 問題: `2.2` という MAJOR.MINOR ピンは 2.x の任意の patch を受け入れるので、deprecation 警告だけは増えていく。長期的には `solana-program` / `solana-pubkey` を直接使う方が依存ツリーが軽くなる。
- 修正案: 最低限 `solana-sdk = "=2.2.x"` の patch まで固定。中期的には `solana-program`, `solana-pubkey`, `solana-instruction`, `solana-system-interface`, `solana-compute-budget-interface` に分解した依存に切り替える。

### should-fix-015 `verify_sp1_groth16` の VK hash 計算が毎回 SHA-256 を回しており、`OnceLock` cache 化できる

- 場所: `programs/title-whitelist/src/lib.rs:280-282`
- 観察:
  ```rust
  let groth16_vk_hash: [u8; 4] = Sha256::digest(GROTH16_VK_BYTES)[..4]
      .try_into()
      .unwrap();
  ```
- 問題: `GROTH16_VK_BYTES` は `include_bytes!` の compile-time 定数だが、その SHA-256 prefix は毎回計算される。SBF runtime に `OnceLock` は使えない (no_std + no global mutable state) ため、**ビルド時に const fn で計算したい** が、SHA-256 const は不可。代替案として build.rs で precompute して `pub const GROTH16_VK_HASH_PREFIX: [u8; 4] = [..];` を `OUT_DIR` に出すと CU 削減できる (SHA-256 1 回 = ~1500 CU)。
- 修正案: `programs/title-whitelist/build.rs` を追加:
  ```rust
  use sha2::{Digest, Sha256};
  fn main() {
      let vk_bytes = include_bytes!("vk/groth16_vk_v6.2.bin");
      let hash = Sha256::digest(vk_bytes);
      let prefix: [u8; 4] = hash[..4].try_into().unwrap();
      let out = std::env::var("OUT_DIR").unwrap();
      std::fs::write(
          format!("{out}/groth16_vk_hash_prefix.rs"),
          format!("pub const GROTH16_VK_HASH_PREFIX: [u8; 4] = {prefix:?};"),
      ).unwrap();
      println!("cargo:rerun-if-changed=vk/groth16_vk_v6.2.bin");
  }
  ```

## nitpick

### nitpick-016 `Anchor.toml` の `[scripts] test` が cargo test を echo するだけ

- 場所: `Anchor.toml:14-15`
- 観察:
  ```toml
  [scripts]
  test = "echo 'Use: cargo test --workspace'"
  ```
- 問題: `anchor test` は通常 BPF deploy → ts/mocha test の起動だが、`echo` だけだとユーザが「テストが通った」と誤解する可能性。
- 修正案: `test = "cargo test --workspace --all-features"` に置換するか、`[scripts]` セクション自体を削除してデフォルトに任せる。

### nitpick-017 `programs/title-whitelist/Cargo.toml` の `cpi` feature が `no-entrypoint` だけを enable していて使用箇所が無い

- 場所: `programs/title-whitelist/Cargo.toml:13-16`
- 観察:
  ```toml
  [features]
  no-entrypoint = []
  cpi = ["no-entrypoint"]
  default = []
  idl-build = ["anchor-lang/idl-build"]
  ```
- 問題: 他クレートが `title-whitelist = { features = ["cpi"] }` 形で参照していない (workspace exclude されている)。Anchor 標準テンプレが残っているだけの可能性。
- 修正案: 当面 CPI 提供予定が無いなら `cpi` feature を削除。提供予定なら `cpi = ["no-entrypoint", "anchor-lang/idl-build"]` 等の対応を追加。

### nitpick-018 `WhitelistInstruction` enum (client side) が事実上の dead code

- 場所: `crates/solana/src/whitelist.rs:104-142`
- 観察: `WhitelistInstruction` は serde serialize されるだけのデータ型で、Anchor の wire format (discriminator + Borsh) とは互換性が無い。実機テスト (tests/devnet_whitelist.rs) は `anchor_discriminator()` で直接 Borsh を組み立てている。
- 問題: B 観点 (dead code) と重複するが、Solana 観点では「client が enum で命令を表現してから wire 化するのが Solana 慣習」ではなく、Anchor IDL ベースの client crate (anchor-client) を使うのが本来。中途半端な抽象が残っている。
- 修正案: `WhitelistInstruction` 削除。代わりに `pub fn register_key_ix(...) -> solana_sdk::instruction::Instruction` のような builder 関数を提供 (tests/devnet_whitelist.rs:51 の `build_register_key_ix` をそのまま `crates/solana/src/whitelist.rs` に移動)。

### nitpick-019 `tests/devnet_whitelist.rs` の load_authority_keypair が legacy/v0.1.0 配下の operator.json を参照

- 場所: `crates/solana/tests/devnet_whitelist.rs:260-263`
- 観察:
  ```rust
  let key_path = format!(
      "{}/legacy/v0.1.0/keys/operator.json",
      env!("CARGO_MANIFEST_DIR").replace("/crates/solana", "")
  );
  ```
- 問題: `legacy/` ディレクトリへの test 依存が残っている。`legacy/` を将来削除する障害になる。v0.1.2 では `keys/admin.json` のみが正規 (lib.rs:34) で、operator key の概念が消えているはず。
- 修正案: 「non-admin」テスト用の鍵は `Keypair::new()` で生成し、faucet で airdrop してから使う。あるいは `keys/non_admin_test.json` を生成しておく。

### nitpick-020 `programs/title-whitelist/keypair.json` が `.gitignore` に登録されているが、コメントが無い

- 場所: `.gitignore:31`, `programs/title-whitelist/keypair.json` (存在するが untracked)
- 観察: program ID keypair (デプロイ済み program の upgrade authority と program key) が local 専用。
- 問題: 新規 clone した開発者は `anchor build` 後に keypair.json を生成する必要があるが、`.gitignore` のエントリにコメントが無いので「なぜ ignore されているか」を理解する手段が無い。program ID は `Anchor.toml` に固定されているので、誰かが `anchor build` するたびに新規 program key が生成され、deploy しても program ID が変わり混乱する。
- 修正案: `.gitignore` に
  ```
  # Solana program keypair: program ID is pinned in Anchor.toml; this local
  # file is only used by `anchor deploy` (admin-only) and must not be committed.
  programs/*/keypair.json
  ```
  と記載。CONTRIBUTING.md にも「再デプロイは admin のみ。`anchor build` 後の keypair.json の扱い」を一段追加。

### nitpick-021 `ADMIN_AUTHORITY: [u8; 32]` が hex ではなく decimal byte literal で、検算しにくい

- 場所: `programs/title-whitelist/src/lib.rs:35-38`
- 観察:
  ```rust
  pub const ADMIN_AUTHORITY: [u8; 32] = [
      14, 13, 85, 28, 133, 146, 12, 228, 183, 160, 156, 77, 30, 213, 163, 160,
      181, 106, 231, 149, 205, 50, 104, 222, 122, 121, 156, 214, 103, 125, 184, 3,
  ];
  ```
- 問題: コメントには Base58 `wrVwsTuRzbsDutybqqpf9tBE7JUqRPYzJ3iPUgcFmna` と書かれているので照合は可能だが、`Pubkey::new_from_array` の解釈と Base58 表記をひと目で照合できない。`solana_program::pubkey!("wrV...")` という const マクロを使えば 1 行で済む。
- 修正案:
  ```rust
  use anchor_lang::solana_program::pubkey;
  pub const ADMIN_AUTHORITY: Pubkey = pubkey!("wrVwsTuRzbsDutybqqpf9tBE7JUqRPYzJ3iPUgcFmna");
  // `admin_authority()` 関数も不要になり、`admin.key() == ADMIN_AUTHORITY` で済む
  ```
  これにより `fn admin_authority() -> Pubkey` (lib.rs:640-642) も削除可能。

## 全体所感

`title-whitelist` プログラム本体は仕様 §6.2 をかなり丁寧に反映しており、特に「revoke で PDA を close しない」「二段の同一性確認 (vkey + measurement)」「Groth16 vk_hash の prefix チェック」など、SP1 + Solana on-chain verify の落とし穴を一通り回避している。

その上で気になったのは:

1. **Anchor 0.30 慣習との微妙なズレ** — `#[derive(InitSpace)]`, `pubkey!` マクロ, `OnceLock` を活用すれば、保守性と CU 効率の両面で改善余地が大きい。特に SP1 Groth16 verify が CU を大きく食う設計なので、register_key の周辺で `Vec<u8>` を回避する価値は高い (must-fix-004)。
2. **client / program 間の Bubblegum バージョン不整合** (must-fix-003) と **`mpl-bubblegum` の API 利用が「mint only」前提** (should-fix-010, 011, 012) なのは、本番投入前に明示か境界明確化が必要。
3. **admin rotation の運用設計が二系統** (should-fix-007) なのは Phase 2 の multi-sig / DAO 移行を見越して、今のうちに片方に統一しておくと将来痛みが少ない。

devnet 上の挙動テスト (tests/devnet_whitelist.rs) はカバレッジが「empty proof / invalid proof / nonexistent PDA / non-admin / full e2e cNFT mint」と必要十分に揃っており、Solana 観点で見て不足は感じなかった。`init_if_needed` を使わず `init` で固定している (must-fix-001 の議論を除けば) のも安全側で良い設計。

Anchor build の IDL 失敗 (proc-macro2 互換) の根本対処については、現状 `proc-macro2 = 1.0.106` (Cargo.lock) + `anchor-lang = 0.30.1` の組合せで stable に動いているなら、`anchor-lang = 0.31` への upgrade は急務ではない。0.31 は (a) `#[derive(InitSpace)]` のバグ修正と (b) Solana 2.x 系との互換性向上が主で、SP1 verify 周りの破壊的変更は無い。upgrade するなら must-fix-002 (InitSpace 化) と同時に実施するのが効率的。
