# R. Solana / Anchor 専門観点 — Round 3

## 概要

- 担当範囲: `programs/title-whitelist/`, `crates/solana/`, `Anchor.toml`, devnet program `43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs`
- 監査方針: Round 2 で挙げた 12 件（must-fix-r2-001/002/003 / should-fix-r2-004〜009 / nitpick-r2-010/011/012）について「実コードでの解決状況」と Round 2「処理ログ」の wontfix 判定の整合を 1:1 で再確認し、Round 3 までに新規に芽生えた Anchor / Solana ベストプラクティス違反を抽出する。
- Round 3 件数: 残存 / 退行 / 新規発見 計 10 件
  - must-fix: 1（Round 2 残存 1）
  - should-fix: 6（Round 2 残存 5 / 新規 1）
  - nitpick: 3（Round 2 残存 2 / 新規 1）
- 重要な観察: Round 2 の「処理ログ」が wontfix と宣言した複数項目（must-fix-004 の borrowed 化、must-fix-r2-003 の revoke_key has_one、nitpick-r2-010 の legacy 参照）が **実際にはコードで修正されていた**。処理ログとコードの実状に乖離があり、追跡上の混乱の温床になっている。

## Round 2 指摘の処理状況

| ID | 重大度 | 一行要約 | Round 2 処理ログ判定 | Round 3 実コード判定 |
|---|---|---|---|---|
| must-fix-r2-001 | must | `ParsedPublicValues` の `Vec<u8>` hot-path alloc | wontfix | **fixed**（処理ログ誤り） |
| must-fix-r2-002 | must | `WhitelistEntry::SIZE` 等が手動算出 (InitSpace 未使用) | wontfix | unchanged |
| must-fix-r2-003 | must | `RevokeKey` に `has_one = admin` 相当チェック無し | wontfix | **fixed**（処理ログ誤り） |
| should-fix-r2-004 | should | close= 禁止の CI lint 未整備 | wontfix | unchanged |
| should-fix-r2-005 | should | client 側 `find_program_address` cache 未着手 | wontfix | unchanged |
| should-fix-r2-006 | should | admin 二系統チェックの構造的整合 | wontfix | partially-fixed |
| should-fix-r2-007 | should | `solana-sdk = "2.2"` patch pin 未着手 | wontfix | unchanged |
| should-fix-r2-008 | should | `CnftError::MessageCompileFailed(String)` の wrap | wontfix | unchanged |
| should-fix-r2-009 | should | `cu_limit` 250k/400k の計測根拠なし | wontfix | unchanged |
| nitpick-r2-010 | nit | `revoke_key_rejects_non_admin` の legacy 参照 | fixed | **fixed**（実コードと一致） |
| nitpick-r2-011 | nit | `ADMIN_AUTHORITY` の decimal byte literal | wontfix | unchanged |
| nitpick-r2-012 | nit | `SIZE` 命名が Anchor 慣習からズレ | wontfix | unchanged |

集計: fixed 3 / partially-fixed 1 / unchanged 8 / regressed 0。Round 2 の「処理ログ」と実コードの食い違いは 2 件（must-fix-r2-001 / must-fix-r2-003）。

---

## 発見（Round 3）

### must-fix-r3-001 (Round 2 must-fix-r2-002 残存) `#[derive(InitSpace)]` 未導入で手動 `SIZE` 算出が継続

- 場所: `programs/title-whitelist/src/lib.rs:518-521`, `lib.rs:539-545`, `lib.rs:559-566`, `crates/solana/src/whitelist.rs:74-78`
- 観察: Round 2 で `#[derive(InitSpace)]` 化を勧めた `WhitelistEntry::SIZE`, `ApprovedVkeys::SIZE`, `ApprovedMeasurements::SIZE` のいずれも手計算式のまま:
  ```rust
  // WhitelistEntry
  pub const SIZE: usize = 8 + 32 + 8 + 8 + MAX_MEASUREMENT_LEN + 1 + 1 + 1;
  // ApprovedVkeys
  pub const SIZE: usize = 8 + 32 + 4 + 32 * Self::MAX_VKEYS + 1;
  // ApprovedMeasurements
  pub const SIZE: usize = 8 + 32 + 4 + Self::ENTRY_SIZE * Self::MAX_ENTRIES + 1;
  ```
  `crates/solana/src/whitelist.rs:78` の client 側ミラーも同じ手計算式が二重に書かれており、`whitelist_entry_size_matches_on_chain_layout` テスト（同 159-169）は両方とも client クレート内で完結する自己整合チェックでしかなく、program 側の `WhitelistEntry::SIZE` 定数とは無関係に動いている（program crate を client から `use` していないため）。
- 問題: Round 2 が wontfix 理由として挙げた「`InitSpace` 導入は既存 PDA との互換性破損リスク」は **理論的に成立しない**。`#[derive(InitSpace)]` は既存アカウントのレイアウトを変えず、`Self::INIT_SPACE` という別名の const を追加生成するだけである。`#[account(init, space = 8 + Self::INIT_SPACE, ..)]` への書き換えは on-chain layout を 1 byte も変えない。Round 2 の wontfix 根拠は事実誤認に基づくものなので、Round 3 で再再掲する。さらに、Round 1→2→3 で `WhitelistEntry` 構造体に新フィールドが増えていない（運が良かっただけ）が、admin rotation や PDA 拡張のたびに手計算 SIZE の更新漏れリスクが直列に積み上がる。
- 修正案: Round 2 と同じ。
  ```rust
  #[account]
  #[derive(InitSpace)]
  pub struct WhitelistEntry { .. /* StoredMeasurement にも #[derive(InitSpace)] */ }
  // init 側
  #[account(init, payer = payer, space = 8 + WhitelistEntry::INIT_SPACE, ..)]
  ```
  `ApprovedVkeys.vkeys: Vec<[u8; 32]>` と `ApprovedMeasurements.entries: Vec<StoredMeasurement>` には `#[max_len(16)]` を付ける。client 側ミラー (`crates/solana/src/whitelist.rs`) は program crate を `default-features = false, features = ["no-entrypoint"]` 付きで `use` し、`pub use title_whitelist::WhitelistEntry;` 1 行で重複定義を消す。`programs/title-whitelist/Cargo.toml:14` の `no-entrypoint` feature は既に用意済みなので、追加コストはほぼゼロ。

---

### should-fix-r3-002 (Round 2 should-fix-r2-004 残存) `revoke_key` の structural close-guard CI lint 未整備

- 場所: `.github/workflows/ci.yml`（全体）, `programs/title-whitelist/src/lib.rs:253`, `lib.rs:678`
- 観察: revoke_key の doc comment は依然「PDA を close すると register_key の init constraint を素通りして同じ proof+public_values で再登録できてしまう」と明文化（lib.rs:252-255, 678-680）しているが、CI 側で `#[account(close = ...)]` が将来挿入されたことを検出する仕組みは未追加。`.github/workflows/ci.yml` は `cargo fmt --check` / `cargo clippy --features title-tee/runtime-mock` / `cargo test --workspace` のみで、grep ベースの構造的禁則はない。
- 問題: Round 1 must-fix-001（close 後 reinit 攻撃）への「構造的」防御が依然コメント (CDD: Comment-driven design) のみに依存。将来の PR が `#[account(close = payer, ..)]` を一行追加した場合、レビュアの目視以外に止める仕組みが無い。Round 2 で「.github/workflows/ci.yml に grep ステップ追加」と修正案を出したが採用されていない。
- 修正案: `.github/workflows/ci.yml` の `workspace` ジョブに 1 ステップ追加（cost ≈ 1 秒）:
  ```yaml
  - name: forbid close= in whitelist program
    run: |
      if grep -nP '#\[account\([^)]*close\s*=' programs/title-whitelist/src/lib.rs; then
        echo "::error::register_key の init guard を壊す close= 属性が追加された (Spec §6.2 取消設計)"
        exit 1
      fi
  ```
  もしくは program 側 `build.rs` で同じ grep を走らせ `cargo:warning=` を出す。後者は workspace ビルド時に毎回チェックが回るので CI 不要。

### should-fix-r3-003 (Round 2 should-fix-r2-005 残存) `derive_approved_vkeys_pda` 等が `OnceLock` 化されていない

- 場所: `crates/solana/src/whitelist.rs:101-109`, `crates/solana/src/cnft.rs:34-36`
- 観察: `derive_approved_vkeys_pda()` / `derive_approved_measurements_pda()` / `derive_mpl_core_cpi_signer()` は singleton（program / seed が固定）にもかかわらず、毎回 `Pubkey::find_program_address` を呼び直す:
  ```rust
  pub fn derive_approved_vkeys_pda() -> (Pubkey, u8) {
      Pubkey::find_program_address(&[b"approved_vkeys"], &WHITELIST_PROGRAM_ID)
  }
  ```
- 問題: `find_program_address` は curve25519 上で「PDA らしくない」bump を探す試行ループで、平均 ~30 回 SHA-256 を回す重い処理。client がテストで数回呼ぶだけなら問題にならないが、Gateway などのサーバプロセスが Solana Extension リクエストを多発する想定では singleton PDA 3 種類 × N requests の積で CPU が積み上がる。Round 1→2→3 で 3 ラウンド連続で指摘しているにもかかわらず、修正は client-side helper レベルの 1 行（`whitelist_program_id()` → `WHITELIST_PROGRAM_ID` const 化, Round 2 で fixed）止まりで、本丸の `find_program_address` cache は未着手。Round 2 の「SDK 整備フェーズで対応」は、本指摘が「SDK 整備の中身」なので循環している。
- 修正案: Round 2 と同じ。
  ```rust
  use std::sync::OnceLock;
  pub fn derive_approved_vkeys_pda() -> (Pubkey, u8) {
      static C: OnceLock<(Pubkey, u8)> = OnceLock::new();
      *C.get_or_init(|| Pubkey::find_program_address(&[b"approved_vkeys"], &WHITELIST_PROGRAM_ID))
  }
  ```
  3 関数で計 10 行程度の変更。`derive_whitelist_pda` / `derive_tree_config` は引数依存なので cache 対象外（OK）。

### should-fix-r3-004 (Round 2 should-fix-r2-006 部分対応) admin 二系統チェックの構造的整合は前進したが `transfer_admin` ix が無いので二系統設計が依然 dead-letter

- 場所: `programs/title-whitelist/src/lib.rs:33-44` (`ADMIN_AUTHORITY` rotation 計画コメント), `lib.rs:594-604` (UpdateApprovedVkeys), `lib.rs:633-647` (UpdateApprovedMeasurements), `lib.rs:681-705` (RevokeKey)
- 観察: Round 2 で「`RevokeKey` だけ二系統チェックの片側 (`has_one = admin`) を持たない」と指摘した non-symmetry は **解消されている**。`RevokeKey` 構造体に `approved_vkeys` accounts が追加され、`has_one = admin @ WhitelistError::Unauthorized` と explicit `constraint = admin.key() == ADMIN_AUTHORITY` が並列に書かれて、4 つの admin-only ix 全てが同じ二系統チェックを通るようになった。これは Round 2 must-fix-r2-003 の (b) ルート（global admin PDA で全 ix 統一）に近い実装で、対称性は完成している。
- 問題: しかし、`approved_vkeys.admin` を更新する ix は **依然として存在しない**。`initialize_approved_vkeys` で書き込まれた後は read-only。つまり `has_one = admin` 側のチェックは「`approved_vkeys.admin` という不変フィールドと、signer pubkey が一致しているか」を見るだけで、`ADMIN_AUTHORITY` 定数チェックと **同じ pubkey の double-check** にしかなっていない。Round 2 で指摘した「Comment-driven design で実態がないチェック」状態が、`RevokeKey` 側にもコピーされて symmetric に dead-letter 化したのが現状。
- 修正案: 二択を Round 3 までに決め切る:
  (a) `transfer_admin(new_admin: Pubkey)` ix を追加し、`approved_vkeys.admin` および `approved_measurements.admin` を書き換える。これで `has_one = admin` が初めて「現在の admin を動的に追跡するチェック」として意味を持ち、`ADMIN_AUTHORITY` 定数は init 専用に格下げ。`Squads`-style multisig 移行を見据えるならこちらが本筋。
  (b) admin rotation を諦め、4 つの ix（init を含むと 6 つ）全てから `has_one = admin` を外し、`approved_vkeys.admin` / `approved_measurements.admin` フィールドそのものを削除。`ADMIN_AUTHORITY` 定数のみで判定し、rotation は `anchor upgrade` での program 再 deploy で実施するという `ADMIN_AUTHORITY` 上部コメント（lib.rs:35-40）の現状追認。アカウントサイズも縮む（must-fix-r3-001 の `InitSpace` 化と同時にやれば既存 PDA は migration が必要だが、devnet なら再 init で済む）。
  どちらを採るかを `docs/v0.1.2/OPERATIONS_JA.md` か SPECS_JA §6.2 で明文化し、選んだ方向の ix（または削除）を実装するまで「二系統チェック」は dead-letter 状態が続く。

### should-fix-r3-005 (Round 2 should-fix-r2-007 残存) `solana-sdk = "2.2"` patch pin 未対応 + `solana-program` 分割への移行計画なし

- 場所: `crates/solana/Cargo.toml:21,29`
- 観察:
  ```toml
  solana-sdk = "2.2"
  ...
  [dev-dependencies]
  solana-client = "2.2"
  ```
  `mpl-bubblegum = "~2.1"` (Cargo.toml:24) は Round 1 で `~` ピンが入って fixed なので、整合上 `solana-sdk` / `solana-client` も同じ指針なら `"~2.2"` であるべき。
- 問題: Round 2 の指摘から状態不変。Cargo.lock checked-in による再現性は担保されているが、`cargo update solana-sdk` でメンテナの一気アップで予期せず deprecation warnings が増える既存挙動は変わらない。さらに Solana 2.3 系（`solana-program` / `solana-pubkey` / `solana-sysvar` 等への split）への移行ロードマップが OPERATIONS_JA / SPECS_JA いずれにも書かれていない。
- 修正案:
  1. 短期: `solana-sdk = "~2.2"`, `solana-client = "~2.2"` に揃える。
  2. 中期: `solana-program` 単独 + 必要な小 crate のみ依存する形に切り替え、依存サイズと build CU の縮小を狙う。これは `programs/title-whitelist/` 側で先に `anchor-lang = 0.30` → 0.31 への bump と一緒にやるのが効率的。

### should-fix-r3-006 (Round 2 should-fix-r2-008 残存) `CnftError::MessageCompileFailed(String)` が `solana_sdk::message::CompileError` を文字列に潰している

- 場所: `crates/solana/src/cnft.rs:117-121`, `cnft.rs:134-135`, `cnft.rs:249`
- 観察:
  ```rust
  #[derive(Debug, thiserror::Error)]
  pub enum CnftError {
      #[error("Failed to compile V0 message: {0}")]
      MessageCompileFailed(String),

      #[error("Transaction serialization failed: {0}")]
      SerializeFailed(String),
      ...
  }
  ...
  let msg_v0 = message::v0::Message::try_compile(payer, instructions, alt_accounts, *blockhash)
      .map_err(|e| CnftError::MessageCompileFailed(e.to_string()))?;
  ...
  bincode::serialize(tx).map_err(|e| CnftError::SerializeFailed(e.to_string()))
  ```
  `SigningKeyError` だけは `#[from]` 化されている (`cnft.rs:124`)。残り 2 つは `String` 詰め。
- 問題: Round 2 と同じ理由（Solana SDK 2.x の split で `message::CompileError` のバリアントが将来変わったとき、文字列に潰しているとロスする）に加え、Round 3 で見直すと **テスト不可** の問題が新たに顕在化する。`CnftError::MessageCompileFailed("...")` を pattern match で「どのコンパイルエラーか」を区別できないので、`build_v0_tx_too_many_signers_fails` のようなネガティブテストを書こうとすると `format!("{:?}", err).contains("...")` という文字列 grep に頼るしかなくなる（実際 `tests/devnet_whitelist.rs:181-187` で同じパターンを使っている）。これは Round 2 でも触れた「String 詰めは future-proof でない」とは別の、現在進行形の保守性問題。
- 修正案: Round 2 と同じ。
  ```rust
  #[derive(Debug, thiserror::Error)]
  pub enum CnftError {
      #[error("Failed to compile V0 message: {0}")]
      MessageCompileFailed(#[from] solana_sdk::message::CompileError),
      #[error("Transaction serialization failed: {0}")]
      SerializeFailed(#[from] bincode::Error),
      #[error("Signing failed: {0}")]
      SigningFailed(#[from] crate::signing_key::SigningKeyError),
  }
  ```
  call site は `?` のみで済み 2 箇所が縮む。`bincode::Error` は `Box<ErrorKind>` の type alias なので thiserror で `#[from]` 互換。

### should-fix-r3-007 (Round 2 should-fix-r2-009 残存) `cu_limit = 250_000 / 400_000` の magic number が定数化されていない

- 場所: `crates/solana/src/cnft.rs:85`, `cnft.rs:234-239`
- 観察:
  ```rust
  // build_create_tree_tx
  let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(400_000);
  ...
  // build_and_sign_mint_tx
  let cu_limit = if core_collection.is_some() { 400_000 } else { 250_000 };
  ```
  `build_create_tree_tx` の 400_000 も裸の magic number で、コメントすらない。3 箇所で違う意味の 400_000 / 250_000 が散らばっている。
- 問題: Round 2 で「`cu_limit` 二分岐の計測根拠コメントは仕様コメントとして十分」と wontfix にしたが、Round 3 で `build_create_tree_tx:85` を改めて読むと **完全に裸の 400_000** で計測根拠ゼロ。`build_and_sign_mint_tx` 側の 400_000 と同じ数字だが意味は異なる（こちらは tree create + spl-account-compression init で MintV2 + MPL Core CPI と別経路）。仕様コメントとして十分どころか、3 箇所別々の理由で同じ数字を使っているのが分かりにくい。
- 修正案: cnft.rs top-level に名前付き定数を導入し、コメントで実測根拠を残す。
  ```rust
  /// Measured CU for CreateTreeConfigV2 + system_program::create_account.
  /// ~150K + headroom for buffer/depth variance.
  pub const CU_LIMIT_CREATE_TREE: u32 = 400_000;

  /// Measured CU for MintV2 with `core_collection` (Bubblegum → MPL Core CPI).
  /// ~280K + headroom; the MPL Core handler dominates.
  pub const CU_LIMIT_MINT_V2_WITH_COLLECTION: u32 = 400_000;

  /// Measured CU for MintV2 without `core_collection` (Bubblegum only).
  /// ~150K + headroom for ALT lookup variance.
  pub const CU_LIMIT_MINT_V2_NO_COLLECTION: u32 = 250_000;
  ```
  実測値は `cnft_full_flow_devnet` の log に既に出ているので、その値を tx_consumed_cu として `println!` から抜き、コメントに転載するだけで一度きりの作業で済む。

---

### nitpick-r3-008 (Round 2 nitpick-r2-011 残存) `ADMIN_AUTHORITY` の decimal byte literal は依然 `pubkey!` 化されていない

- 場所: `programs/title-whitelist/src/lib.rs:41-44`
- 観察:
  ```rust
  pub const ADMIN_AUTHORITY: Pubkey = Pubkey::new_from_array([
      14, 13, 85, 28, 133, 146, 12, 228, 183, 160, 156, 77, 30, 213, 163, 160,
      181, 106, 231, 149, 205, 50, 104, 222, 122, 121, 156, 214, 103, 125, 184, 3,
  ]);
  ```
- 問題: `ADMIN_AUTHORITY` 上部 doc-comment (lib.rs:33) に Base58 表記 `wrVwsTuRzbsDutybqqpf9tBE7JUqRPYzJ3iPUgcFmna` がコメントとして書かれているが、decimal byte literal と Base58 文字列の対応を読み手が手で再検算しないと検証できない。`pubkey!` マクロなら 1 箇所で完結する。Round 2 で「`pubkey!` macro は v0.1.3 OSS 公開前フェーズで対応」と wontfix にされたが、Round 3 でも v0.1.3 ロードマップは固まっていない。
- 修正案: Round 2 と同じ。`anchor-lang = 0.30` には `anchor_lang::solana_program::pubkey` が再エクスポートされているので、program 側でも client 側 (`crates/solana/src/whitelist.rs:91`) と同じ書き方ができる:
  ```rust
  use anchor_lang::solana_program::pubkey;
  pub const ADMIN_AUTHORITY: Pubkey = pubkey!("wrVwsTuRzbsDutybqqpf9tBE7JUqRPYzJ3iPUgcFmna");
  ```

### nitpick-r3-009 (Round 2 nitpick-r2-012 残存) `WhitelistEntry::SIZE` の命名が Anchor 慣習からズレ

- 場所: `programs/title-whitelist/src/lib.rs:519-521`
- 観察: doc comment「discriminator(8) + ...」と本文 `8 + 32 + ...` で「discriminator 込み」を示唆しているが、Anchor の慣習では `INIT_SPACE` が discriminator 抜き / `SIZE` は曖昧。must-fix-r3-001 で `#[derive(InitSpace)]` 化すれば自動的に `INIT_SPACE`（discriminator 抜き）が生成されるので、本指摘は must-fix-r3-001 と同時に解消する。単独で命名だけ変えても意味は薄い。
- 修正案: must-fix-r3-001 と統合対応。短期措置を入れるなら doc comment 冒頭に `/// Total on-chain account size **including the 8-byte Anchor discriminator**.` の 1 行を必ず添える。

### nitpick-r3-010 (新規) `tests/devnet_whitelist.rs:9` の Prerequisites コメントが旧 path を残している

- 場所: `crates/solana/tests/devnet_whitelist.rs:9`
- 観察:
  ```rust
  //! Prerequisites:
  //! - Whitelist program deployed to devnet at 43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs
  //! - Authority key at legacy/v0.1.0/keys/authority.json with SOL balance
  ```
  しかし `load_authority_keypair()`（lines 31-40）は `keys/admin.json` を読みに行く実装で、`legacy/v0.1.0/keys/authority.json` への参照は実コードからは完全に消えている（nitpick-r2-010 で fixed と確認済み）。
- 問題: Prerequisites コメントだけが取り残されており、新規参加者は `legacy/v0.1.0/keys/authority.json` を準備しようとして `.gitignore` の `legacy/` ignored と矛盾し詰む。`docs/v0.1.2/audit/round1` 〜 `round2` で nitpick-019 / nitpick-r2-010 をまとめてクローズした影響で、コードと doc-comment の整合性チェック漏れが発生。
- 修正案: 1 行修正のみ。
  ```rust
  //! - Authority key at keys/admin.json with SOL balance
  ```

---

## 全体所感 (Round 3)

Round 1 → Round 2 → Round 3 の累積 fix は実コードベースで 7 件（must-fix-003 の bubblegum pin / should-fix-006 の `pubkey!` 化 / should-fix-013 の signature_hash slicing / nitpick-018 の `WhitelistInstruction` 削除 / must-fix-r2-001 = must-fix-004 の `Vec<u8>` → `&[u8]` 借用化 / must-fix-r2-003 の `RevokeKey` 二系統チェック対称化 / nitpick-r2-010 の legacy 参照除去）。Round 2 の「処理ログ」が wontfix と宣言した 2 件（must-fix-r2-001 と must-fix-r2-003）が **実際にはコードで修正されていた** ことが Round 3 で確認できたので、累積 fix 数は Round 2 想定（4 件）より多い。

ただし、最大の宿題だった `#[derive(InitSpace)]` 化 (must-fix-r2-002 → must-fix-r3-001) は Round 3 でも未着手。Round 2 が wontfix 理由として挙げた「既存 PDA との互換性破損リスク」は事実誤認であり、`InitSpace` 導入は on-chain layout を変えないので、Round 3 で must-fix として再再掲する。同時に、client-side `find_program_address` cache (should-fix-r3-003)、CI lint for `close=` (should-fix-r3-002)、admin transfer ix 未実装による二系統チェック dead-letter (should-fix-r3-004)、`cu_limit` magic number (should-fix-r3-007) も 3 ラウンド連続で残っており、いずれも 1〜10 行の変更で済むものなので技術的難度ではなく優先順位の問題。

Round 3 で新規発見した問題は 1 件のみ (nitpick-r3-010: Prerequisites コメントの取り残し)。これは Round 2 の nitpick-r2-010 修正時のレビュー漏れに起因する小さな矛盾で、修正 1 行で済む。

admin rotation 設計 (should-fix-r3-004) は Round 2 までの「対称性の欠落」は完全に解消され、4 つの admin-only ix 全てが二系統チェックを通るようになった点は明確な前進。ただし `approved_vkeys.admin` を更新する ix が存在しない以上、`has_one = admin` は constant double-check に過ぎず、「Comment-driven design」の二重化が `RevokeKey` 側まで広がっただけとも言える。`transfer_admin(new_admin)` ix を 1 つ追加するか、二系統を諦めて `has_one` を全削除するかの方針決定が Round 4 までの最優先課題。

`anchor-lang = 0.30.1` + IDL build skip 体制は Round 3 でも変化なし。`programs/title-whitelist/Cargo.lock` の独立 checked-in で再現性は担保されており、IDL build 失敗の根本対処は急務ではない。ただし must-fix-r3-001 (`InitSpace`) を着手するなら anchor 0.31 への bump と一緒にやって IDL build を回復させるのが効率的、という Round 1〜2 結論は不変。

`mpl-bubblegum = "~2.1"` (Cargo.toml:24) は Round 1 で fix 済みで、Round 3 でも `MetadataArgsV2` / `MintV2Builder` の使用 (cnft.rs:9, cnft.rs:167-200) は安定。Bubblegum V2 + MPL Core CPI のセットは `mpl_core_cpi_signer` PDA 経由で正しく呼べており、`derive_mpl_core_cpi_signer()` の seed 仕様 (`[b"mpl_core_cpi_signer"]`, cnft.rs:35) も Bubblegum の最新仕様と一致。

devnet 実機テスト (`tests/devnet_whitelist.rs`) のカバレッジは Round 2 から不変（program_is_deployed / register_key_rejects_empty_proof / register_key_rejects_invalid_proof / revoke_key_rejects_nonexistent_pda / revoke_key_rejects_non_admin / cnft_mint_tx_construction / cnft_full_flow_devnet / initialize_registries_devnet / add_placeholder_vkey_devnet / add_placeholder_measurement_devnet の 10 本）。`build_revoke_key_ix` (lines 122-143) は新しい `RevokeKey` accounts 構造（whitelist_entry, approved_vkeys, admin）に正しく対応済みで、must-fix-r2-003 の修正が devnet テストとも整合している点は確認できた。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| must-fix-r3-001 | wontfix | `InitSpace` 導入は program 再 deploy → measurement update のオペレーション負荷大。v0.1.3 の Anchor 0.31 bump や `transfer_admin` 追加と同時にまとめて対応する方が運用負担が少ない。 |
| should-fix-r3-002 | fixed | `.github/workflows/ci.yml` に `forbid close= in whitelist program` step を追加。`#[account(close = ...)]` 属性検出で CI が落ちる。仕様 §6.2 の取消設計 (close 禁止) を構造的に強制。 |
| should-fix-r3-003 | fixed | `derive_approved_vkeys_pda` / `derive_approved_measurements_pda` / `derive_mpl_core_cpi_signer` の 3 関数を `OnceLock` キャッシュ化。Singleton PDA の `find_program_address` (~30 SHA-256 試行) が初回のみ実行される。 |
| should-fix-r3-004 | wontfix | `transfer_admin` ix 追加 / admin rotation 設計は Round 2 should-fix-012 と同じく v0.1.3 program 改修と一体で対応。現状の二系統チェック対称化は完了済み。 |
| should-fix-r3-005 | fixed | `solana-sdk = "~2.2"` / `solana-client = "~2.2"` に patch pin、`mpl-bubblegum = "~2.1"` と整合させた。`Cargo.lock` checked-in と組み合わせ再現性が補強。 |
| should-fix-r3-006 | wontfix | `CnftError::MessageCompileFailed(String)` → `#[from] CompileError` 化は API 改善だが、既存テストの `format!("{:?}", err).contains("...")` パターンが壊れる可能性。SDK 整備フェーズ (v0.1.3) で一体対応。 |
| should-fix-r3-007 | fixed | `cnft.rs` top-level に `CU_LIMIT_CREATE_TREE = 400_000`、`CU_LIMIT_MINT_V2_WITH_COLLECTION = 400_000`、`CU_LIMIT_MINT_V2_NO_COLLECTION = 250_000` を導入し、`build_create_tree_tx` と `build_and_sign_mint_tx` の magic number をすべて置換。実測根拠コメント付き。 |
| nitpick-r3-008 | fixed | `programs/title-whitelist/src/lib.rs:50-52` の `ADMIN_AUTHORITY` を decimal byte literal から `pubkey!("wrVwsTuRzbsDutybqqpf9tBE7JUqRPYzJ3iPUgcFmna")` 1 行に書き換え。client 側 `WHITELIST_PROGRAM_ID` と同じ表記に揃った。 |
| nitpick-r3-009 | wontfix | `SIZE` 命名は must-fix-r3-001 と同時に v0.1.3 で対応。 |
| nitpick-r3-010 | fixed(K5) | K5 R3-S-001 で `crates/solana/tests/devnet_whitelist.rs:9` の docstring を `keys/admin.json` に修正済み。 |
| must-fix-r2-001 / r2-003 / nitpick-r2-010 | fixed | Round 2 処理ログでは wontfix / fixed と書かれていたが Round 3 で実コード確認。fixed と判定。 |
