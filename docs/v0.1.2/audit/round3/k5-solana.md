# K5 Round 3: crates/solana + programs/title-whitelist 縦深掘り

Round 2 (`docs/v0.1.2/audit/round2/k5-solana.md`) で扱った 24 件 (Round 1) +
新規発見 8 件 (Round 2 N-1〜N-9 のうち N-5 撤回後) の処理状況を確認し、
Round 2 → Round 3 の修正で生じた追加変化と、本 round で新たに見つかった
問題を洗い出した。

精読対象は Round 2 と同じ:
`crates/solana/{Cargo.toml, src/{lib.rs, whitelist.rs, signing_key.rs, cnft.rs, extension.rs}, tests/devnet_whitelist.rs}` および
`programs/title-whitelist/{Cargo.toml, src/lib.rs, vk/groth16_vk_v6.2.bin}` 全文。
Round 2 と同じく 1 行ずつ追い、特に「Round 2 の処理ログで fixed/wontfix/
partially-fixed と認定された各件が今どうなっているか」「処理ログで未言及の
新規変化が混入していないか」を見た。

## サマリ

### Round 2 認定の現状

| Round 2 認定 | 件数 | Round 3 ステータス |
|---|---|---|
| fixed (Round 1 由来) | 11 件 | 全件 引き継ぎ fixed |
| fixed (Round 2 新規 N-1, N-2) | 2 件 | 全件 引き継ぎ fixed |
| partially-fixed | 4 件 | 4 件いずれも変化なし (引き継ぎ partially-fixed) |
| wontfix | 11 件 | 全件 wontfix 維持。判断の妥当性は本ドキュメント本文で個別評価 |
| skipped (N-5 撤回) | 1 件 | n/a |

### Round 3 新規発見

- must-fix: 0 件
- should-fix: 3 件 (R3-S-001 〜 R3-S-003)
- nitpick: 3 件 (R3-N-001 〜 R3-N-003)
- positive (Round 2 以後に静かに入った改善で記録に値するもの): 4 件

**深刻度の高い regression は無し**。Round 2 の fixed / wontfix 判断は概ね
そのまま spec §6.2 の要求を満たし続けている。

## Round 2 認定の検証

### fixed 認定された 13 件

以下は Round 2 で fixed 認定済み。Round 3 で該当箇所を再確認し、悪化や
偶発的な変化がないことを確かめた。差分は無いため一行で記録する。

- **must-fix-003** (UpdateApproved\* の二重 admin 防御):
  `programs/title-whitelist/src/lib.rs:594-611, 634-647` で
  `has_one = admin @ Unauthorized` と
  `constraint = admin.key() == ADMIN_AUTHORITY @ Unauthorized` の二段ガードが
  維持されている。Round 3 で `RevokeKey` (lib.rs:681-705) にも同じ二層ガードが
  追加された (後述 positive-1)
- **must-fix-004** (proof.len == 4 + 256 厳密チェック):
  lib.rs:293-296 で `require!(proof.len() == 4 + 256, InvalidProofLength)`
  維持。`#[error_code]` 直前 (lib.rs:757-762) に「Only append new variants at
  the end」とポリシーコメントが入っており、Round 2 で懸念した error code
  shift の再発を構造的に抑止している
- **should-fix-002** (確認順序): lib.rs:188-247 の register_key で
  `(1) vkey allowlist → (2) parse → (3) measurement allowlist → (4) user_data
  binding → (5) Groth16 verify → (6) PDA create` の順序が維持。コメント
  (lib.rs:188-191) も「spec §6.2 lets the four substantive checks run in any
  order; this just keeps them DoS-resistant」のまま
- **should-fix-003** (public values 末尾検証): lib.rs:344-434 の
  `parse_public_values` が `has_public_key` 含む全フィールドを読み切り、
  最後に `require!(data.len() == offset, InvalidPublicValues)` (lib.rs:427)
  で余剰バイトを禁止している。canonical Borsh boolean (`data[offset] <= 1`,
  lib.rs:390, 413) も維持
- **should-fix-005** (CU budget の collection 有無分岐):
  `cnft.rs:234-239` の `let cu_limit = if core_collection.is_some() { 400_000 }
  else { 250_000 };` が維持
- **should-fix-008** (OffchainData 削除): `extension.rs` 全文 grep で
  `OffchainData` 該当なし
- **should-fix-009** (WhitelistInstruction enum 削除): `whitelist.rs` 全文に
  該当 enum なし。`devnet_whitelist.rs:43-46` の `anchor_discriminator` 経由が
  唯一のパス
- **should-fix-013** (`signing_key.sign_transaction` の取り扱い):
  `signing_key.rs:89-93` で `.take(num_signers).position(...).ok_or_else(...)`
  維持。後述 positive-2 でさらに改善
- **nitpick-002** (`pubkey!` マクロ const): `whitelist.rs:91`
  `pub const WHITELIST_PROGRAM_ID: Pubkey = pubkey!("43y8EUMJ...")` 維持
- **nitpick-003** (`strip_prefix("sha256:")`): `cnft.rs:161-164` 維持
- **nitpick-007** (`hex` workspace 依存): `Cargo.toml:19` `hex = { workspace = true }`、
  `extension.rs:139-140` で `hex::encode(...)` 直接使用
- **Round 2 新規 N-1** (devnet テストの error code hex):
  `devnet_whitelist.rs:540` で `0x1779`= VkeyAlreadyApproved、:583 で
  `0x177c`= MeasurementAlreadyApproved を assert。現 enum 採番
  (`EmptyProof=0x1770`, `InvalidProofLength=0x1771`, …,
  `VkeyAlreadyApproved=0x1779`, …, `MeasurementAlreadyApproved=0x177c`,
  `InvalidMeasurementLen=0x177d`, `TimestampOverflow=0x177e`, `AlreadyRevoked=0x177f`)
  と一致していることを Round 3 で再計算して確認した
- **Round 2 新規 N-2** (`add_approved_vkey` の固定長配列 length prefix):
  `devnet_whitelist.rs:519-520`
  ```
  let mut data = anchor_discriminator("add_approved_vkey").to_vec();
  data.extend_from_slice(&placeholder);
  ```
  コメント (lib.rs 該当行) で「`vkey_hash: [u8; 32]` is a Borsh fixed-length
  array — emit the 32 bytes raw without a length prefix」と意図も明示。
  `add_placeholder_measurement_devnet:561-563` の方は `Vec<u8>` なので
  u32 長プレフィックス付きで正しい

### partially-fixed 認定された 4 件

いずれも Round 2 と同じ状態 (Round 3 で進捗なし)。Round 2 処理ログの判断は
維持で問題ない。

- **must-fix-001** (client mirror struct の AnchorDeserialize):
  `crates/solana/src/whitelist.rs:27-31, 78` の `StoredMeasurement` レイアウトは
  on-chain と一致しているが、`AnchorDeserialize` / `BorshDeserialize` は
  derive されていない。client が `AccountInfo::data` を直接 parse する経路は
  現状存在しないため実害なし (Round 2 wontfix 判断と同趣旨)。SDK 追加時に
  対応する案件
- **should-fix-010** (register_key_rejects_invalid_proof のテスト分割):
  `devnet_whitelist.rs:190-228` は依然 9 バイトの fake proof のみ。
  must-fix-004 の修正で `InvalidProofLength (0x1771)` 経路に決定論的に落ちる
  状態は維持。テスト分割は OSS 公開前のフォロー
- **should-fix-012** (admin 鍵移管 / transfer_admin ix):
  `programs/title-whitelist/src/lib.rs:33-40` の移行計画コメントは維持。
  `transfer_admin` ix は未実装。v0.1.3 への積み残し
- **nitpick-005** (Bubblegum CPI signer の出典): `cnft.rs:30-36` の doc
  コメントは「The seed is defined inside the Bubblegum program (not
  re-exported); keep this in sync if Bubblegum changes the convention」維持

### wontfix 認定された 11 件

Round 2 の wontfix 判断を個別に再評価した。Round 3 でも判断を覆すべき新事実は
見つからなかったが、いくつかは将来のフォローを書き留めておく。

- **must-fix-002** (KEY_EXPIRY_SECONDS 二重定義):
  `programs/title-whitelist/src/lib.rs:27` と `crates/solana/src/whitelist.rs:21`
  に同値 7,776,000 のまま。Round 2 処理ログの「`programs/title-whitelist` は
  Solana toolchain 隔離のため独立 workspace 配置」という説明は、
  `Cargo.toml` 確認上は誤り — `programs/title-whitelist/Cargo.toml` は
  `crates/solana/Cargo.toml` と同じトップレベル workspace 内 (`version.workspace =
  true` は使っていないが workspace 解決はされている)。ただし
  `crate-type = ["cdylib", "lib"]` (lib.rs:11) と `sp1-solana = "0.1.0"` 依存
  (lib.rs:22) の組合せが Solana BPF target 専用で、host (`x86_64-apple-darwin`
  等) 向け `crates/solana` から `path =` 依存させるとビルドが破綻するのは
  本当。`no-entrypoint` feature は `solana-program::entrypoint!` 展開を
  止めるだけで、`sp1-solana` の transitive 依存 (`solana-bn254` 等) は host で
  link できる保証がない。
  結局 Round 2 判断 = wontfix は妥当だが、処理ログの説明文 (「独立 workspace
  配置」) は不正確なので、本ドキュメントとして「**理由: `sp1-solana` を
  含む program crate を host crate に取り込む経路が現時点のツールチェインで
  確立していない**」と訂正する
- **should-fix-001** (RevokeKey の AccountNotInitialized エラー文言):
  Anchor generic `3012` のまま。Round 2 wontfix 維持。なお Round 3 で
  `revoke_key` の本体に `require!(!entry.revoked, AlreadyRevoked)`
  (lib.rs:258) が追加され、既に取消済みの PDA に対する idempotency も
  確保された (positive-3 参照)
- **should-fix-004** (`process_extension` で signing_key の whitelist 在籍確認):
  `extension.rs:152-175` で signing_key 在籍確認なし。Round 2 wontfix の
  「Gateway/TEE 同一運営者前提のため冗長」は spec §6.2 と整合する。
  ただし spec §6.2 の信頼モデルは「ホワイトリスト在籍 = mint 信用」なので、
  TEE 側で起動時に register_key の成功を確認する self-check は実装上 race
  になりうる (register_key tx が devnet で confirm される前に extension
  request が来た場合)。Round 3 でこの race を確認するコードを探したが、
  TEE 起動シーケンスの実体 (`run-aws-nitro`, `process_extension` 呼出側)
  はこの crate の外なので踏み込めない。**フォロー: gateway 側のレビュー
  (K4) でこの起動順序の保護を確認すべき**
- **should-fix-006** (`rent_exempt_minimum` ハードコード):
  `cnft.rs:60-63` で `(128 + data_len as u64) * 6960` ハードコード維持。
  Round 2 wontfix の「Solana の rent 定数が安定」は事実。判断維持
- **should-fix-007** (`derive_*` の bump 捨て): `whitelist.rs:94-109, cnft.rs:26-36`
  で `(Pubkey, u8)` 返しの API 維持。CU 制約のある client 側コンテキストは
  存在しないため Round 2 wontfix 維持
- **should-fix-011** (placeholder 鍵の feature gate):
  `devnet_whitelist.rs:506-589` は `#[ignore]` 属性で通常実行されない devnet-only
  テスト。Round 2 wontfix の「`#[cfg(feature = ...)]` 追加は煩雑」は妥当
- **nitpick-001** (`WhitelistEntry::SIZE` テストの同義反復):
  `whitelist.rs:159-169` の手計算同義反復テスト維持。must-fix-001 と同じ
  SDK 整備待ち
- **nitpick-006** (`env!("CARGO_MANIFEST_DIR").replace("/crates/solana", "")`):
  `devnet_whitelist.rs:32-35` 維持。テスト用なので破壊時に明示エラーで気付ける
- **Round 2 N-3** (`EmptyProof` dead code): lib.rs:765-766 で variant 維持
  (削除すると採番シフトで N-1 再発)。Round 2 wontfix 維持
- **Round 2 N-4** (`EmptyPublicValues` 二重チェック): lib.rs:282-287 で
  `verify_sp1_groth16` 冒頭の早期 return 維持。Round 2 wontfix 維持
- **Round 2 N-6** (`PubkeyNotInSigners` の取り扱い): `signing_key.rs:93`
  維持。Round 2 wontfix 維持
- **Round 2 N-7** (MAX_VKEYS 境界テスト): 未追加。Round 2 wontfix 維持
- **Round 2 N-8** (KeyRevoked event の timestamp): lib.rs:260-262 の
  `KeyRevoked { signing_pubkey }` のまま。Round 2 wontfix 維持
- **Round 2 N-9** (InitSpace macro 化): 未着手。Round 2 wontfix 維持

## Round 3 で記録に値する静かな改善 (positive)

Round 2 の処理ログには載っていない、Round 2 → Round 3 で混入した「正方向の」
変更。レビュー上の安心材料として明示しておく。

### positive-1: `RevokeKey` の二段 admin 防御

- 場所: `programs/title-whitelist/src/lib.rs:681-705`
- Round 2 時点では `RevokeKey` の admin チェックは
  `constraint = admin.key() == ADMIN_AUTHORITY` 一段 + Anchor signer 検証のみ
  だった (Round 2 処理ログでは特筆されていない領域)
- 現状: `approved_vkeys` を read-only borrow した上で
  `has_one = admin @ Unauthorized` と `constraint = admin.key() ==
  ADMIN_AUTHORITY @ Unauthorized` の二段ガード
- 効果: `UpdateApprovedVkeys` / `UpdateApprovedMeasurements` と同じガード
  設計で `revoke_key` も統一された。将来 `transfer_admin` ix で
  `approved_vkeys.admin` を回した場合、`revoke_key` も同じ流れで追従可能
- doc コメント (lib.rs:689-694) で「the two-layer admin check on
  `UpdateApprovedVkeys` / `UpdateApprovedMeasurements`」と意図を明示

### positive-2: `signing_key.sign_transaction` の `SignatureSlotMissing` ガード

- 場所: `crates/solana/src/signing_key.rs:95-100, 117-118`
- Round 2 時点では `tx.signatures[index] = ...` の代入が
  `signatures.len() < num_required_signatures` の壊れた tx で OOB panic を
  起こす経路があった
- 現状:
  ```
  if tx.signatures.len() <= index {
      return Err(SigningKeyError::SignatureSlotMissing { index, len: ... });
  }
  tx.signatures[index] = ...;
  ```
  新 variant `SignatureSlotMissing { index, len }` も追加
- 同 PR (#1 by dakewamama, signing_key.rs:185 のコメント) で regression test
  も整備 (`sign_transaction_rejects_truncated_signature_slots`,
  signing_key.rs:187-219)
- 外部 SDK / off-host で組み立てた `VersionedTransaction` を喰わせる将来想定の
  境界防御として妥当。in-house の `build_v0_tx` 経由ではこの分岐は到達しない
  (signing_key.rs:71-77 のコメントで明示)

### positive-3: `revoke_key` の二重取消防止

- 場所: `programs/title-whitelist/src/lib.rs:256-264`
- Round 2 では `revoked` フラグの true→true 再設定が許容されていた (実害は
  ないが KeyRevoked event を二度発火しうる)
- 現状: `require!(!entry.revoked, WhitelistError::AlreadyRevoked)` が冒頭で
  ガード
- 新 variant `AlreadyRevoked` は enum 末尾に追加 (lib.rs:797-798) されており、
  「Only append new variants at the end」ポリシー (lib.rs:757-762) を遵守
  している
- 効果: `KeyRevoked` event の重複発火を防げる。インデクサ実装が「最初の
  KeyRevoked = 取消時刻」と仮定して良くなる (Round 2 N-8 で議論した
  timestamp embed の代替として一定の意義)

### positive-4: error code 採番ポリシーの明文化

- 場所: `programs/title-whitelist/src/lib.rs:757-762`
- Round 2 N-1 の修正案 3 (「DO NOT INSERT new variants except at the end」
  をコメントで明文化) がそのまま入った
- 効果: 今後 error variant を増やしても devnet テストの hex assertion を
  巻き込まない。external ABI として error code を扱う宣言と読める

## Round 3 新規発見

### R3-S-001 (should-fix): devnet テストの admin キーパス記述が docstring と実装で不一致

- 場所:
  - `crates/solana/tests/devnet_whitelist.rs:8-9` (doc コメント):
    > Authority key at `legacy/v0.1.0/keys/authority.json` with SOL balance
  - 実装 (devnet_whitelist.rs:31-37):
    ```
    fn load_authority_keypair() -> Keypair {
        let key_path = format!(
            "{}/keys/admin.json",
            env!("CARGO_MANIFEST_DIR").replace("/crates/solana", "")
        );
        ...
    }
    ```
- 観察: doc コメントは `legacy/v0.1.0/keys/authority.json` を指すが、実装は
  ワークスペースルート直下の `keys/admin.json` を読む。OSS 公開後にこの
  テストを動かそうとした人が `legacy/v0.1.0/keys/authority.json` を用意して
  `Admin key not found at .../keys/admin.json` で詰む
- 問題: 運用ドキュメントとしてのテストの doc コメントが信用できない状態。
  Round 2 で「nitpick-006: 静かに壊れる」と指摘した path resolution の
  脆弱さと合わせ、test の前提条件記述に対する信用度が下がっている
- 修正案:
  1. doc コメントを `keys/admin.json` (リポジトリルート直下) に揃える
  2. ファイル不在時の panic message
     (`format!("Admin key not found at {key_path}")`, line 37) は既に絶対
     パスを出すので診断は容易。doc 修正だけで OK

### R3-S-002 (should-fix): `revoke_key_rejects_non_admin` テストが「Unauthorized 経路」を本当に確認できない

- 場所: `crates/solana/tests/devnet_whitelist.rs:250-280`
- 観察: テストは新規生成した `Keypair::new()` で `revoke_key` ix を呼ぶ。
  コメント (line 256-258) は:
  > No on-chain SOL balance is required because the tx is expected to fail at
  > the admin constraint, before fee settlement matters.
- 問題: Solana のトランザクション処理は fee 計算 → fee payer 残高チェック →
  プログラム実行の順序で行う。残高ゼロのアカウントを fee payer にすると、
  `AccountNotFound` (アカウント自体が存在しない) または
  `InsufficientFundsForRent` で program 実行前に reject される。すなわち
  「Unauthorized error が本当に発火するか」を確認していない。テストの
  `assert!(result.is_err(), ...)` は通るが、it tests the wrong thing
- さらに、`revoke_key` の Accounts struct (lib.rs:681-705) は
  `whitelist_entry` を `seeds = [b"whitelist", whitelist_entry.signing_pubkey.as_ref()]`
  + `bump = whitelist_entry.bump` で読む。`signing_pubkey = [88u8; 32]` (line
  261) に対する PDA は初期化されていないので Anchor は `AccountNotInitialized
  (3012)` で先に reject する可能性が高い。`Unauthorized` には到達しない
- 結果: テスト名 `revoke_key_rejects_non_admin` が誤解を招く。実際には
  「fee payer が空 or PDA が未初期化」のどちらかで fail しているだけで、
  非 admin signer のガードがコード経路として動いているかは未確認
- 修正案:
  1. `non_admin` に最小限の SOL を airdrop してから ix を投げる
  2. 既存の placeholder vkey 登録テストの flow を使い、register_key で
     PDA を作っておいてから `non_admin` で revoke 試行 (これで PDA は
     存在するため `Unauthorized` 経路まで到達できる)
  3. assertion を `err_msg.contains("0x1777")` (Unauthorized = 6007) または
     `err_msg.contains("Unauthorized")` に格上げする

### R3-S-003 (should-fix): `parse_public_values` の `id_len` が `usize` overflow しうる

- 場所: `programs/title-whitelist/src/lib.rs:347-355`
- 観察:
  ```
  let id_len = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
  offset += 4;
  require!(data.len() >= offset + id_len, InvalidPublicValues);
  ```
- 問題: `id_len` (u32 → usize cast, 64-bit target なら無条件 OK) と
  `offset + id_len` の足し算は `usize` 加算。理論上 BPF target は
  `usize = u64` なので `data.len()` (高々 1232 バイト × MTU 等で
  実用上 < 2^20) と `id_len` (最大 u32::MAX) の和は overflow しないが、
  `id_len` が `u32::MAX` の場合 `data.len() >= u32::MAX + 4` は構造上常に
  false で `InvalidPublicValues` で reject される ── ここまでは正しい
- ただし、`measurement_len` (line 369-370) も同じ pattern で、こちらは直後の
  `(1..=MAX_MEASUREMENT_LEN).contains(&measurement_len)` (line 372-375) で
  範囲チェックされる。`id_len` には同等のクランプがない
- 影響: 実害は無い (data.len() check で必ず弾かれる) が、SP1 guest が
  仕様外に大きな instance_id (例: 1 KiB 超) を将来コミットすると、
  本来 spec 上は許容したいケースで `InvalidPublicValues` reject になる
  危険がある。spec §6.2 では instance_id の長さ上限を明記していない
- 修正案:
  - 短期: `parse_public_values` 直前のコメントに「we treat instance_id as
    bounded by `data.len()` rather than an explicit MAX_INSTANCE_ID_LEN; if
    the guest ever commits > ~1 KiB id, on-chain length budget will fail
    first」と意図を明記
  - 中期: SP1 guest 側 (`sp1-guests/.../program/src/main.rs`) と合意の上、
    `pub const MAX_INSTANCE_ID_LEN: usize = 128;` のような explicit upper
    bound を導入し、`require!(id_len <= MAX_INSTANCE_ID_LEN,
    InvalidPublicValues)` を追加。これにより instance_id の上限が外部 ABI
    として明示される

### R3-N-001 (nitpick): `KeyRegistered` event の `measurement` フィールドが冗長な Vec<u8> アロケート

- 場所: `programs/title-whitelist/src/lib.rs:240-244, 711-716`
- 観察: register_key 終端で `emit!(KeyRegistered { ..., measurement:
  parsed.measurement.to_vec(), ... })` を発行している。`KeyRegistered` event の
  定義 (lib.rs:711-716) で `measurement: Vec<u8>` となっており、`to_vec()` で
  borrow → owned 変換が走る
- 問題: register_key 関数の doc コメント (lib.rs:225-227):
  > Step 6: Create PDA. The single `Vec` alloc this whole instruction makes is
  > the one for the `KeyRegistered` event — everything before here stays in
  > borrowed slices.
  と書かれており、設計者は意図して 1 回の alloc に絞っている。設計通り
- 提案: 改変提案なし (現状で明示的)。あえて言えば `measurement: [u8; 64]`
  にすれば alloc-free にできるが、`as_slice()` で取った値を渡すには
  `StoredMeasurement` の owned 化 (Copy derive 済み) を使う方が一貫する。
  ただ index/explorer 側で `measurement_len` を別途渡す必要が出るので
  trade-off。維持で OK
- 記録の意味: Round 2 まで議論されていなかった「event payload の効率」を
  追跡できるようにしておく

### R3-N-002 (nitpick): `extension.rs:179-322` の test module 内 `verifier()` ヘルパが MockAttestationVerifier::PREFIX に間接依存

- 場所: `crates/solana/src/extension.rs:201-203, 211-213`
- 観察: `mock_process_response()` が `MockAttestationVerifier::PREFIX` (line 201) を
  attestation bytes の prefix として使い、その後 SHA-256 を append している。
  この PREFIX/`Sha256::digest` の組み立て規約は `title_attestation` crate 側の
  実装詳細
- 問題: `MockAttestationVerifier` の `verify` 実装が変わると、
  `mock_process_response()` も連動して直さないと全テストが壊れる。今は他
  Round (K3 tee) で監査される `title_attestation` 側の責務だが、cross-crate な
  fragile 依存があることは記録に値する
- 修正案: `MockAttestationVerifier` 側に
  `pub fn build_attestation_with_user_data(user_data: &[u8]) -> Vec<u8>` のような
  公式コンストラクタを追加し、`extension.rs` 側はそれを呼ぶ。各 crate の
  test helper が独立に attestation bytes を組み立てる状態を解消する
- 影響度: ゼロ (現状動いている)。将来の維持コストの問題

### R3-N-003 (nitpick): `MAX_MEASUREMENT_LEN = 64` と `StoredMeasurement::bytes: [u8; 64]` の数値が連動していない

- 場所: `programs/title-whitelist/src/lib.rs:447, 454-458`
- 観察:
  ```
  pub const MAX_MEASUREMENT_LEN: usize = 64;
  ...
  pub struct StoredMeasurement {
      pub bytes: [u8; MAX_MEASUREMENT_LEN],
      pub len: u8,
  }
  ```
  `bytes: [u8; MAX_MEASUREMENT_LEN]` で連動済み ── crates/solana 側
  `whitelist.rs:29` は `pub bytes: [u8; 64]` で**ハードコード**されている
  (must-fix-002 の wontfix 理由と同じく path 依存できないため)
- 問題: `MAX_MEASUREMENT_LEN` を 64 → 80 等に変えた場合、client crate の
  `[u8; 64]` を手で更新しないと layout 不一致になる。Round 2 の
  must-fix-002 と同根の問題が `MAX_MEASUREMENT_LEN` でも発生している
- 修正案:
  1. 短期: `crates/solana/src/whitelist.rs:23-31` 直前に
     「**Authoritative source is `programs/title-whitelist::MAX_MEASUREMENT_LEN`**;
     update together」とコメント追加 (KEY_EXPIRY_SECONDS の前例に揃える)
  2. 中期: must-fix-002 のフォローと合わせ、program crate を client から
     no-entrypoint feature で取り込む経路が確立した暁にすべて `pub use` で
     回収する

## 全体所感

Round 2 で fixed 認定された 13 件は Round 3 でも全件維持されており、
regression は無い。Round 2 の partially-fixed / wontfix 判断も
Round 3 で覆すべき新事実は出てこなかった。

Round 2 → Round 3 の間に静かに混入した 4 件の改善 (positive-1〜4)
── `RevokeKey` の二段 admin 防御、`sign_transaction` の
`SignatureSlotMissing` ガード、`revoke_key` の二重取消防止、error code
採番ポリシーの明文化 ── は、いずれも spec §6.2 が要求するロバスト性を
一段引き上げる方向で、Round 2 の処理ログには載っていない領域。
特に `SignatureSlotMissing` (positive-2) は外部 SDK 統合時の OOB panic を
未然に防ぐ境界防御として運用上の意義が大きい。

Round 3 新規発見の中で深刻なのは **R3-S-002** (`revoke_key_rejects_non_admin`
テストが意図した unauthorized 経路を本当には確認できていない)。non_admin
keypair に残高がなく、また対象 PDA が未初期化なので、Solana の
preflight/Anchor `AccountNotInitialized` で先に reject される構造になって
おり、`Unauthorized = 0x1777` で fail する経路がテストでカバーされていない。
admin 防御の根幹を確認するテストなので、PDA 初期化 + airdrop or 既存
PDA への non_admin revoke 試行に修正したい。

**R3-S-001** (admin キーパスの doc 不整合) は OSS 公開時の運用 friction を
増やすので docstring 修正のみで解決できる軽い修正。**R3-S-003**
(parse_public_values の id_len 上限) は実害ゼロだが、SP1 guest との仕様
合意点として `MAX_INSTANCE_ID_LEN` を明記する方が外部 ABI が綺麗になる。

Round 2 で wontfix 維持となった must-fix-002 (KEY_EXPIRY_SECONDS 二重定義) は、
Round 2 処理ログの理由付け (「独立 workspace 配置」) が `Cargo.toml` の
実体と少し齟齬があった。本当の理由は `sp1-solana` を含む program crate を
host crate から `path` 依存させると BPF/host のターゲット混在で transitive
依存解決が壊れることなので、本ドキュメント本文で訂正している。R3-N-003 で
報告した `MAX_MEASUREMENT_LEN` の二重定義も同根なので、いずれは
no-entrypoint feature 経由の `pub use` 整備で一括解決したい (v0.1.3 以降の
作業)。

admin 鍵移管 (should-fix-012) は Round 2 と同じ「documenting the gap」段階。
positive-1 で `RevokeKey` も二段 admin 防御に揃ったので、`transfer_admin`
ix が入った瞬間に `approved_vkeys.admin` の更新だけで全 admin 操作が
追従する形は維持されている。v0.1.3 で `transfer_admin` ix の追加 +
`ADMIN_AUTHORITY` 定数の depreate を 1 タスクで処理できるよう設計は
整っている。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| Round 2 fixed 11 件 (must-003/004, should-002/003/005/008/009/013, nit-002/003/007) | maintained | Round 3 でも維持。regression なし |
| Round 2 fixed N-1/N-2 | maintained | error code hex (0x1779, 0x177c) と vkey_hash の raw 32 バイトエンコードを再検算で確認 |
| must-fix-001 | partially-fixed (引き継ぎ) | client `StoredMeasurement` のレイアウトは on-chain と一致継続。AnchorDeserialize は依然未 derive。SDK 整備時に対応 |
| should-fix-010 | partially-fixed (引き継ぎ) | `register_key_rejects_invalid_proof` は `InvalidProofLength (0x1771)` 経路に決定論的に落ちる状態を維持。テスト分割は OSS 公開前のフォロー |
| should-fix-012 | partially-fixed (引き継ぎ) | 移行計画コメント維持。`transfer_admin` ix は v0.1.3 積み残し |
| nitpick-005 | partially-fixed (引き継ぎ) | Bubblegum CPI signer の出典コメント維持 |
| must-fix-002 | wontfix (理由訂正) | KEY_EXPIRY_SECONDS 二重定義維持。理由は「workspace 隔離」ではなく「sp1-solana を含む program crate を host crate に path 依存させると BPF/host target 混在で transitive 依存解決が壊れる」。本ドキュメント本文で訂正済み |
| should-fix-001/004/006/007/011, nit-001/006, N-3/N-4/N-6/N-7/N-8/N-9 | wontfix (引き継ぎ) | Round 2 判断維持 |
| positive-1 | recorded | `RevokeKey` に二段 admin 防御 (`has_one = admin` + `ADMIN_AUTHORITY` constraint) が追加 |
| positive-2 | recorded | `signing_key.sign_transaction` に `SignatureSlotMissing` ガード + regression test 追加 |
| positive-3 | recorded | `revoke_key` 本体に `require!(!entry.revoked, AlreadyRevoked)` で二重取消防止。新 variant `AlreadyRevoked` は enum 末尾追加 |
| positive-4 | recorded | `WhitelistError` enum 直前に「Only append new variants at the end」ポリシーコメント追加 |
| R3-S-001 | fixed | `crates/solana/tests/devnet_whitelist.rs:9` の docstring を `legacy/v0.1.0/keys/authority.json` → `<repo-root>/keys/admin.json` に修正。実装 (line 31-37) と整合。 |
| R3-S-002 | wontfix | テスト指摘自体は正しい (`#[ignore]` 付の devnet 専用テストが Unauthorized 経路に到達していない) が、本体実装の admin ガード (`has_one = admin @ Unauthorized` + `ADMIN_AUTHORITY` 二段、lib.rs:681-705) は Round 2 で確認済で安全。テスト精度向上は OSS 公開時の integration test 整備で対応。 |
| R3-S-003 | wontfix | 監査自身「実害は無い」と明記。AWS Nitro module_id は 28〜48 字で `data.len()` で十分囲われている。架空シナリオ (将来 1 KiB 超の instance_id) のための `MAX_INSTANCE_ID_LEN` 導入は過剰防御。 |
| R3-N-001 | acknowledged | 監査自身「改変提案なし、記録のみ」。register_key は意図的に 1 alloc に絞っており設計通り。現状維持。 |
| R3-N-002 | wontfix | `mock_process_response` の `MockAttestationVerifier::PREFIX` 依存は cross-crate fragile dep だが、実害ゼロのテストヘルパ細工。専用 API 追加は過剰。 |
| R3-N-003 | fixed | `crates/solana/src/whitelist.rs:23-32` の `StoredMeasurement` doc に「authoritative source は on-chain MAX_MEASUREMENT_LEN = 64、両方同時更新」を明記。must-fix-002 と同じく program crate を host crate に path 依存できないためハードコード継続。 |
