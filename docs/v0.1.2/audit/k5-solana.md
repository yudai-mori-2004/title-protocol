# K5: crates/solana + programs/title-whitelist 縦深掘り

## 概要

担当範囲: `crates/solana/{Cargo.toml, src/*.rs, tests/devnet_whitelist.rs}` および `programs/title-whitelist/{Cargo.toml, src/lib.rs, vk/groth16_vk_v6.2.bin}`。

監査方針: SPECS_JA §6 を Source of Truth とし、(1) Anchor 0.30 慣習準拠 (PDA seeds / constraint / event)、(2) `register_key` の 4 段確認の順序と早期失敗、(3) `revoke_key` の close vs flag 設計、(4) Approved* レジストリの容量、(5) `parse_public_values` の境界チェック網羅、(6) SP1 Groth16 verify の正しい使用、(7) CU 予算、(8) cNFT mint TX 構築の正しさ (mpl-bubblegum 2.1.1)、(9) partial_sign のキー順序、(10) クライアント側 mirror struct と on-chain struct の整合、(11) devnet テストの本質検証性、(12) admin authority のハードコード / 移行手段、を 1 文 1 文確認した。

確認した上流コード: `sp1_solana::verify_proof_raw` (0.1.0), `mpl_bubblegum` v2.1.1 の `mint_v2.rs` / `create_tree_config_v2.rs`, `sp1-guests/attestation-aws-nitro/program/src/main.rs` のコミット順序。

合計件数: 24 件 (必読 4、改善推奨 13、要改善 7)。

## 重大度別内訳

- must-fix: 4 件
- should-fix: 13 件
- nitpick: 7 件

## 発見

### must-fix-001 client-side mirror struct が on-chain struct と Borsh 互換でない

- 場所: `crates/solana/src/whitelist.rs:31-54` と `programs/title-whitelist/src/lib.rs:457-479`
- 観察:
  - on-chain: `pub measurement: StoredMeasurement` (= `[u8; 64] + u8 len`、固定 65 バイト)
  - client mirror: `pub measurement: Vec<u8>` (Borsh の `Vec<u8>` 表現は `u32 len + bytes`)
  - `WhitelistEntry::SIZE` の数字 (123) は両側で一致するが、これは「8+32+8+8+65+1+1=123」と「8+32+8+8+(4+N)+1+1」が偶然同じ計算結果になるだけ
- 問題: クライアントが将来 `AccountInfo::data` を Borsh で読もうとした瞬間に静かに壊れる。`Vec<u8>` の 4 バイト長プレフィックスを `[u8;64]` の先頭 4 バイトとして読み、その後の `bool/u8` フィールドが完全に位相ずれする。コメント (whitelist.rs:23-29) は「Client-side mirror of the Anchor account」と謳っているのに、実態は mirror になっていない
- 修正案: client 側を on-chain と一致させる。`pub struct StoredMeasurement { bytes: [u8; 64], len: u8 }` を `crates/solana/src/whitelist.rs` 内に再宣言し、`WhitelistEntry::measurement` をそれに置換。あわせて `AnchorDeserialize` を derive する (Cargo.toml に `anchor-lang` 依存を no-entrypoint で追加するか、Borsh 直導入で対応)。`SIZE` 等値テスト (whitelist.rs:177-181) は構造体の `try_to_vec().len() + 8` で検証する形に書き直し、無意味な手計算一致を避ける

### must-fix-002 KEY_EXPIRY_SECONDS が二箇所に分散しドリフトしうる

- 場所: `crates/solana/src/whitelist.rs:17` と `programs/title-whitelist/src/lib.rs:27`
- 観察: どちらも `90 * 24 * 60 * 60` を独立に定義
- 問題: 片方を変更しても他方は変わらない。on-chain が真実なので、client がそれを知らずに古い値で `is_valid_at` を返すと「on-chain では valid だが client では invalid」/逆 のスプリットが起きる。今後ガバナンスで 60 日や 120 日に変える際に必ず事故る
- 修正案: client crate から program crate への定数参照を確立する。最小コストの解は `title-whitelist` の `no-entrypoint` feature を `title-solana` の dev/通常依存として有効化し、`pub use title_whitelist::KEY_EXPIRY_SECONDS` を `crates/solana/src/whitelist.rs` 冒頭に置く。少なくとも client 側を `#[doc(hidden)]` にして「使うな、on-chain の値を信じろ」と宣言する

### must-fix-003 `add_approved_vkey`/`remove_approved_vkey` に管理者照合がない

- 場所: `programs/title-whitelist/src/lib.rs:62-94` と `551-560` (UpdateApprovedVkeys)
- 観察: `UpdateApprovedVkeys` には `has_one = admin @ WhitelistError::Unauthorized` がある (556 行)。`Initialize*` 側 (543 行, 575 行) には `constraint = admin.key() == admin_authority() ...` がある
- 問題: `has_one = admin` は「`approved_vkeys.admin` フィールド = signer の admin」しか保証しない。仮に何らかの経路で `approved_vkeys.admin` が ADMIN_AUTHORITY 以外に書き換えられた場合 (現状は init 時の 1 回しか書かれないが、将来的に admin transfer 命令が追加されたとき)、`add_approved_vkey` は ADMIN_AUTHORITY 検証を素通りで通る
- もう一点、admin 鍵流出時のリカバリ手段が全く設計されていない (ADMIN_AUTHORITY ハードコード、admin transfer ix なし、multisig 化の経路なし)
- 修正案:
  1. `UpdateApprovedVkeys` / `UpdateApprovedMeasurements` にも `constraint = admin.key() == admin_authority() @ WhitelistError::Unauthorized` を併記し二重防御にする
  2. 将来用に `transfer_admin(new_admin: Pubkey)` ix を追加する設計タスクを別途切る (現フェーズで実装不要だが、`ADMIN_AUTHORITY` を変更不能な定数として宣言している現状は「OSS として再利用する第三者が常に Title Protocol 運営の鍵を信じる」ことを強制する点で問題。CLAUDE.md 的にも単一鍵 hardcode は OSS 公開時に説明が必要)。最低限 `ADMIN_AUTHORITY` の上に「Phase 1 only — see TASK-NN for multi-sig migration」とリンクされた TODO を残す

### must-fix-004 `verify_sp1_groth16` の `proof.len() > 4` で 5 バイト未満 proof を弾かない

- 場所: `programs/title-whitelist/src/lib.rs:279`
- 観察: `require!(proof.len() > 4, WhitelistError::EmptyProof);` — これは「len > 4」つまり 5 バイト以上で通る。続く `proof[..4]` は OK、しかし `verify_proof_raw(&proof[4..], ...)` に渡る後段の `load_proof_from_bytes` は 256 バイト ちょうどを期待する (`pi_a [64] + pi_b [128] + pi_c [64]`)
- 問題: 5〜259 バイトの proof は length チェックを抜けて sp1-solana 内部で配列インデックス out-of-range を起こす可能性がある (sp1-solana 0.1.0 の `load_proof_from_bytes` は `buffer[64..192]` 等を `try_into()` で固定長変換するため、足りない場合 panic ではなく `try_into().unwrap()` で panic = compute aborted。意図したエラーではない)
- 修正案: 厳密に `require!(proof.len() == 4 + 256, WhitelistError::EmptyProof);` (新しいエラー variant `InvalidProofLength` を作るとなお良し)。これで sp1-solana の前提を満たすことを自分で保証する

### should-fix-001 `revoke_key` がレジストリ未登録の鍵を区別できない

- 場所: `programs/title-whitelist/src/lib.rs:244-252`, `626-638`
- 観察: `RevokeKey` の `whitelist_entry` は `#[account(mut, seeds = [b"whitelist", whitelist_entry.signing_pubkey.as_ref()], bump = whitelist_entry.bump)]`。Anchor の挙動上、Account として要求しているので未初期化 PDA を渡すと AccountNotInitialized で失敗する
- 問題: `revoke_key_rejects_nonexistent_pda` テスト (devnet_whitelist.rs:235-251) は「未登録 PDA で失敗する」ことを期待しているが、エラー型は Anchor 内部の generic な `AccountNotInitialized (3012)` であり、本プログラムの `WhitelistError` ではない。運用者がエラーメッセージを見て何が起きたか即座に分からない。さらに、もし将来 RevokeKey のアカウント宣言を `UncheckedAccount` に変えた場合、未登録鍵を revoke した瞬間に空データ書き込みが走る可能性がある
- 修正案: 現行 Anchor `Account<'info, WhitelistEntry>` のままなら問題ないが、`#[msg("Whitelist entry does not exist for that signing key")]` の独自 variant を `RevokeKey` の前段で `require!(ctx.accounts.whitelist_entry.signing_pubkey != [0u8; 32], ...)` のようにチェックして、ユーザー向け文言を改善する

### should-fix-002 `register_key` の 4 段確認順序は spec と微妙にずれている

- 場所: `programs/title-whitelist/src/lib.rs:181-235`
- 観察: 実装順は (1) vkey allowlist → (2) Groth16 verify → (3) parse_public_values → (4) measurement allowlist → (5) user_data binding → (6) PDA 作成
- 問題: Spec §6.2 (1180 行) は「1. 検証回路が正規のものか (verifying_key_hash 照合)、2. TEE 実体が正規のものか (measurement 照合)、両方を通過し、かつ ZK proof の数学的検証に成功した場合のみ、署名鍵をホワイトリスト PDA に登録」と書いている。コード上では Groth16 verify (重い計算; alt_bn128 syscall x 3) が早期に走るため、たとえば不正な measurement での register_key リクエストでも Groth16 検証分の CU を消費してから弾かれる。攻撃者は DOS 用に「vkey は正しいが measurement は偽」のリクエストを連射できる
- 修正案: 早期失敗最大化のため `(1) vkey allowlist → (2) parse_public_values → (3) measurement allowlist → (4) user_data binding (signing_pubkey とのバインド) → (5) Groth16 verify → (6) PDA 作成` の順に並べ替える。Groth16 verify を最後にすれば、bind/measurement/parse のいずれかが先に失敗した場合に重い計算を回避できる。なお `RegisterKey` の `init` constraint は実行が成功した場合のみ PDA を作るので順序入れ替えは安全

### should-fix-003 `parse_public_values` が `has_public_key` 以降を一切検証しない

- 場所: `programs/title-whitelist/src/lib.rs:327-391`
- 観察: parser は `user_data_hash` までで切り上げ、`has_public_key (u8)` と `public_key_hash (32 bytes)` を読まない
- 問題: 仕様 (`sp1-guests/attestation-aws-nitro/program/src/main.rs:70-75`) では guest が必ず `has_public_key` と (true なら) `public_key_hash` をコミットする。on-chain parser は最後尾が「未確認の余剰バイト」を含んでいても素通す。これにより、guest 側で公開値レイアウトが変わって最後 1 バイトが消失するような変更が入った場合、エラー検出されない。Groth16 verify は `committed_values_digest = SHA-256(public_values)` でリンクされるため、データ整合性自体は保証されるが、parser のセマンティック上の境界は曖昧
- もう一点、`has_public_key` も同様に「u8 が 0 か 1 か」のチェックがあるべき (`has_user_data` には 369-373 行で `data[offset] <= 1` のチェックがある)
- 修正案: parser を最後まで読み切る形にし、終端で `require!(data.len() == offset, WhitelistError::InvalidPublicValues);` を入れる。`has_public_key` も canonical Borsh boolean validate する

### should-fix-004 `process_extension` が signing_key 自身の whitelist 在籍を確認しない

- 場所: `crates/solana/src/extension.rs:179-202`
- 観察: TEE 内で attestation を verify した後、即座に cNFT mint TX を作って `signing_key` で部分署名する。signing_key 自身がオンチェーンの whitelist に登録済みかどうか、有効期限内 (`is_valid_at`) かどうかを一切確認しない
- 問題: 仕様上、TEE 起動時に register_key を済ませる前提だが、register が失敗してリトライ未完の状態で extension リクエストを処理してしまうと、検証者から見て「ホワイトリスト外鍵で署名された cNFT」が発生する。クライアントは cNFT のサブミットには成功するが、検証段階で信頼されない。TEE 側で予防可能な事故
- 修正案: TEE 起動時に `register_key` の成否を内部状態として持ち、`process_extension` の入口で `if !self.whitelist_registered { return Err(KeyNotWhitelisted) }` のようにガードする。`ExtensionError::KeyNotWhitelisted` variant は既に extension.rs:58 で定義済みだが使われていない (dead code)

### should-fix-005 mint TX に compute budget instruction がない

- 場所: `crates/solana/src/cnft.rs:209-235` (`build_and_sign_mint_tx`)
- 観察: `build_create_tree_tx` (88 行) は `ComputeBudgetInstruction::set_compute_unit_limit(400_000)` を入れているが、mint 側にはない
- 問題: Bubblegum V2 の MintV2 は Merkle tree への append 操作と (collection 指定時は) MPL Core への CPI を伴う。Solana のデフォルト CU 上限は 200,000 で、collection 込みの mint は実測 300k 程度になりうる。devnet で `cnft_full_flow_devnet` が collection=None で通っているのは運が良いだけ。本番で collection 指定したリクエストが CU 超過で落ちる可能性
- 修正案: `build_and_sign_mint_tx` の先頭で `ComputeBudgetInstruction::set_compute_unit_limit(400_000)` を ix 配列に追加する (collection の有無で 250_000 / 400_000 と分岐すると尚良し)。priority fee (`set_compute_unit_price`) も合わせて 0 でセットしておくと運用時に上書きしやすい

### should-fix-006 `rent_exempt_minimum` がハードコード値で RPC に問い合わせない

- 場所: `crates/solana/src/cnft.rs:64-66`
- 観察: `(128 + data_len) * 6960` で計算
- 問題: rent_exemption_threshold は Solana のチェーン定数で滅多に変わらないが、変わった場合に Merkle tree create_account の lamports 不足で TX が失敗する。TEE 側で計算するなら正しいが、ここは TEE ではなくクライアント (devnet テスト) 経路。`client.get_minimum_balance_for_rent_exemption(space)` を呼ぶべき
- 修正案: `build_create_tree_tx` の引数に `lamports: u64` を追加し、呼び出し側 (テスト/上位レイヤ) が RPC で取得して渡す形に変える。`rent_exempt_minimum` ヘルパは fallback / unit test 用に残す

### should-fix-007 `derive_tree_config` 等のヘルパが bump を捨てる API

- 場所: `crates/solana/src/cnft.rs:25-33`, `whitelist.rs:83-100`
- 観察: `(Pubkey, u8)` を返すが、呼び出し側 (cnft.rs:98, 159 など) は常に `(tree_config, _)` で bump を捨てる
- 問題: bump を再計算するための `find_program_address` は CU と時間を食う。CPI でなくとも、`create_program_address(&[seeds, &[bump]], program)` で再構築できる場合に効率が落ちる。Anchor program 側では `ctx.bumps.tree_config` で取得して保存している
- 修正案: client では bump を Whitelist mirror struct に保持する (`WhitelistEntry::bump` フィールドは既にある)。`derive_*_pda` を呼ぶ場所はバッチ的に bump も使う前提でリファクタする (今回はクリティカルではない)

### should-fix-008 `OffchainData` 型が宣言されているが一切使われていない

- 場所: `crates/solana/src/extension.rs:30-35`
- 観察: `#[derive(Deserialize)] pub struct OffchainData { pub response: ProcessResponse }` が定義されているが、コード内で `OffchainData` を参照している箇所は一切なし (grep 確認済み)。`process_extension` は `ProcessResponse` を直接受け取る
- 問題: dead code。読み手は「これがオフチェーンから fetch した JSON のスキーマだろう」と期待するが、実際の fetch 経路 (上位の gateway や TEE オーケストレータ) でこの struct は使われていない。または将来使う予定の「未完成 API」のシグナル
- 修正案: 削除する。あるいは extension request flow を `OffchainData::from_url(url, http_client)` の形で集約し、fetch まで含めた orchestration の入口にする (現状 fetch は extension.rs の責務外)

### should-fix-009 `WhitelistInstruction` enum が dead code 寄り

- 場所: `crates/solana/src/whitelist.rs:104-142`
- 観察: serde derive されており、テスト (`whitelist_instruction_serialize`) で JSON 往復が確認されているのみ。実際に on-chain プログラムを呼ぶときには使われない (devnet_whitelist.rs:43-46 で手書きの Anchor discriminator を使う)
- 問題: 「JSON 経由で命令を表現するレイヤがどこかにある」という錯誤を読み手に与える。本来 Anchor のオフチェーン側は IDL から自動生成された client か、`anchor_client` crate を使うのが標準。serde 派は Gateway API でリクエストを受ける形を想定していると思しいが、それなら命令の Vec<u8> proof/public_values を JSON で受けるための base64 化が要る。設計が中途半端
- 修正案: いずれかに振る。(A) 削除して devnet_whitelist.rs と同じ手書き discriminator パターンに統一、(B) anchor-client を導入して IDL ベースに刷新、(C) JSON Gateway API として完成させる場合は base64 エンコード/デコードと proof サイズ上限を追加。今は (A) が低コスト

### should-fix-010 devnet テストの `register_key_rejects_invalid_proof` は本質を検証していない

- 場所: `crates/solana/tests/devnet_whitelist.rs:193-231`
- 観察: 9 バイトの fake proof + 最小限の fake public_values で register_key を呼び、「失敗すれば OK」としている
- 問題: 9 バイトは `proof.len() > 4` (= 5 以上) を通る境界の値だが、これが弾かれるパスは「sp1-solana の load_proof_from_bytes 内 panic」「proof[..4] の vk_hash 不一致」のどちらか。前者が起きるなら CU 全消費で abort、後者なら `ProofVerificationFailed`。テストは err_msg を print するだけでどちらか確認していない。「失敗した」だけでは vkey allowlist 未登録による `VkeyNotApproved` でも通ってしまい (sp1_vkey_hash = [0u8;32] は approved set に入っていない可能性が高い)、想定しているエラーパスを実際にはテストしていない
- 修正案: テストを 3 つに分割: (a) `vkey_not_approved` (sp1_vkey_hash 偽 → 期待 0x1772)、(b) `proof_wrong_vk_hash_prefix` (proof[..4] 偽 → 期待 ProofVerificationFailed)、(c) `proof_correct_prefix_invalid_body` (4 + 256 バイトの全ゼロ → 期待 ProofVerificationFailed)。それぞれエラーコードを `msg.contains("0xNNNN")` で確認する

### should-fix-011 devnet テストの placeholder 鍵がそのまま本番に流れるリスク

- 場所: `crates/solana/tests/devnet_whitelist.rs:519` (`[0xAA; 32]` vkey), `567` (`[0xBB; 48]` measurement)
- 観察: コメントで「Replace the placeholder before production」とあるが、replace 漏れを検出する仕組みがない
- 問題: 本番デプロイ前にこれら placeholder を remove_approved_* で消す手順が運用ドキュメントに明示されていない。残ったままだと攻撃者は SP1 guest の正規 vkey/measurement を知る必要さえなく、自前で `[0xAA; 32]` を vkey として持つ proof を生成できれば通ってしまう (現実には Groth16 proof を任意の vkey_hash に紐付けて作れるかは別問題だが、placeholder は明確な seam)
- 修正案: (A) test に `#[cfg(feature = "devnet-placeholders")]` のような feature gate をかけて、デフォルトでは compile されないようにする。(B) `OPERATIONS_JA.md` に「mainnet promote 前に必ず placeholder を remove_approved_vkey / remove_approved_measurement で消す」チェックリストを追加。(C) `add_placeholder_*_devnet` の値を運用者が明示的に環境変数で渡す形にし、誤ってデフォルト値を使わせない

### should-fix-012 admin authority がコードに hardcode されており移行手段がない

- 場所: `programs/title-whitelist/src/lib.rs:33-38`
- 観察: `pub const ADMIN_AUTHORITY: [u8; 32] = [..];` で固定。`admin_authority()` (640-642) で常にこの定数を返す
- 問題: 鍵漏洩、運営移管、multi-sig 化のいずれも program redeploy + Anchor `upgrade` 権限が必要。OSS として公開した場合、「フォーク先プロジェクトはここを書き換えて再デプロイ」が前提となり、CLAUDE.md の「OSS として再利用できる」志向に対する初心者の障壁になる。移行設計が一切ない (TODO コメントすらない)
- 修正案: 短期: 33-34 行のコメントを拡張して「Migration plan: A) Replace with on-chain `admin_authority` PDA owned by multisig program, B) Add `transfer_admin` ix gated by current admin signature」と明記。中期: ApprovedVkeys/ApprovedMeasurements の `admin` フィールドが既にあるので、それを真実とし、`admin_authority()` 関数を廃して `ctx.accounts.approved_vkeys.admin` 直接参照に切り替える。`ADMIN_AUTHORITY` 定数は `init` 命令の初期値のみに使う

### should-fix-013 `sign_transaction` がループで線形探索する

- 場所: `crates/solana/src/signing_key.rs:70-89`
- 観察: TEE pubkey が静的アカウントキーの何番目にあるかを探すために `for i in 0..num_signers` でループ
- 問題: 機能的に正しいが、`tx.message.static_account_keys().iter().position(|k| k == &pubkey)` の方が意図が読みやすい。また「最初に見つかった signer 位置」しか使わないが、Solana のアカウントキーリストは重複を許さないので問題はない
- 修正案: `iter().position()` ベースに書き直す。`if i >= static_keys.len() { break; }` 相当の境界チェックも `position` だと暗黙に処理される (`num_signers > static_keys.len()` というあり得ない状況の防御は別途 `require!` で表現する)

### nitpick-001 `WhitelistEntry::SIZE` テストが手計算の等値確認になっている

- 場所: `crates/solana/src/whitelist.rs:177-181`
- 観察: `assert_eq!(WhitelistEntry::SIZE, 8 + 32 + 8 + 8 + MAX_MEASUREMENT_LEN + 1 + 1 + 1);`
- 問題: 同じ式を二度書いて等値確認しているだけ。リファクタ耐性 ゼロ
- 修正案: must-fix-001 で構造を真の mirror にした上で、`bincode::serialize(&entry).unwrap().len() == WhitelistEntry::SIZE - 8` のような実測検証に変える

### nitpick-002 `whitelist_program_id` が文字列 parse + unwrap

- 場所: `crates/solana/src/whitelist.rs:77-79`
- 観察: `Pubkey::from_str("43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs").unwrap()`
- 問題: 起動時に確実に成功するが、`const fn` ではないため毎回呼ばれる。`solana_sdk::pubkey!` マクロを使えば `const Pubkey` にできる
- 修正案: `pub const WHITELIST_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs");` に書き換え、`whitelist_program_id()` を `const` リターンする薄いラッパに

### nitpick-003 `hash_suffix` 切り出しが「sha256:」プレフィックス前提でハードコード

- 場所: `crates/solana/src/cnft.rs:161-165`
- 観察: `signature_hash[7..signature_hash.len().min(15)]` — 7 は `len("sha256:")` 前提
- 問題: 将来 spec 上のハッシュ形式が `blake3:` 等になった場合 silently 別文字列を切り出す。format validation なし
- 修正案: `signature_hash.strip_prefix("sha256:").map(|h| &h[..h.len().min(8)]).unwrap_or(signature_hash)` のような明示的な strip。spec §1.3 で `signature_hash` 形式を厳密化する話と連動

### nitpick-004 `process_extension` が `OffchainData` の存在意義を奪っている

- 場所: `crates/solana/src/extension.rs:179-186`
- 観察: `offchain_data: &ProcessResponse` を直接受け取り、URL fetch も外部
- 問題: spec §6.2 の「TEE が URL から fetch する」フローと API の責務が一致していない。fetch + verify + sign の orchestration がどこにあるのか extension.rs を読んでも分からない
- 修正案: should-fix-008 と統合し、`fn fetch_and_process_extension(http: &dyn HttpFetcher, ...)` のような high-level wrapper を tee crate 側に置く。extension.rs の `process_extension` は internal の純粋関数として残す

### nitpick-005 `cnft.rs::derive_mpl_core_cpi_signer` の seeds 出典が不明

- 場所: `crates/solana/src/cnft.rs:30-33`
- 観察: `Pubkey::find_program_address(&[b"mpl_core_cpi_signer"], &mpl_bubblegum::ID)`
- 問題: この seed が mpl-bubblegum 内部のどこで定義されているかコメントにない。バージョン更新時に変更が入ったら静かに壊れる
- 修正案: コメントに `mpl_bubblegum::constants::MPL_CORE_CPI_SIGNER_SEED` 等の参照を残す (上流 crate に該当する const があるかを確認した上で、なければハードコード理由を一文添える)

### nitpick-006 devnet テストの key path 算出が脆い

- 場所: `crates/solana/tests/devnet_whitelist.rs:31-34`, `259-262`
- 観察: `env!("CARGO_MANIFEST_DIR").replace("/crates/solana", "")` でリポジトリルートを推定
- 問題: ディレクトリ移動 (例えば crates/ を src/crates/ に変える) で即崩れる。`Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap()` のほうがリファクタ耐性がある
- 修正案: `PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../keys/admin.json")` で構築する。同じ修正を operator.json 側にも

### nitpick-007 `extension.rs::hex_encode` を自前実装している

- 場所: `crates/solana/src/extension.rs:166-173`
- 観察: ループで `write!(s, "{:02x}", b)` で 16 進文字列を作る
- 問題: 依存にすでに `hex` workspace crate (Cargo.toml:27, dev-dependencies) があるが本体 crate には入れていない。本体に hex 依存を加えれば `hex::encode(bytes)` 一行で済む
- 修正案: `Cargo.toml` の `[dependencies]` に `hex = { workspace = true }` を追加し `hex_encode` 関数を削除

## 全体所感

`programs/title-whitelist` 本体は仕様意図 (4 段確認, revoke flag, 容量制限) に概ね忠実で、Anchor 0.30 の慣習にもよく従っている。SP1 Groth16 verifier の使い方も `verify_proof_raw` を直接呼ぶ判断は正当 (ヘキサ変換を省ける)。一方、client mirror struct の Borsh 互換性欠如 (must-fix-001) と KEY_EXPIRY 二重定義 (must-fix-002) は「クライアント側を信じて on-chain と齟齬が出る」典型的な事故源で、優先度高い。`register_key` の確認順序は安全だが性能上は再配置の余地あり (should-fix-002)。devnet テストの placeholder 鍵 (should-fix-011) と admin authority のハードコード (should-fix-012) は OSS 公開時に最も突かれる点で、運用ドキュメント側との連携が必要。
