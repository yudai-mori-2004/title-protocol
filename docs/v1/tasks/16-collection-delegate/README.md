# タスク16: Collection Authority Delegate

## 仕様書

§8.2 — TEEノードの追加・削除時のCollection Authority委譲

## 背景

TEE側のミント処理（`build_mint_v2_tx` でcollection_authorityを設定）は実装済み。
足りないのは、DAOがMPL CoreコレクションのAuthority権限をTEEの`signing_pubkey`に委譲するオンチェーン命令。

信頼の連鎖: DAO → Collection Authority Delegate → TEE signing_pubkey → cNFTミント

## 読むべきファイル

1. `docs/v1/SPECS_JA.md` §8.2（TEEノードの追加・削除）
2. `programs/title-config/src/lib.rs` — 既存の4命令、GlobalConfig構造
3. `crates/tee/src/solana_tx.rs` — `build_mint_v2_tx()`, `derive_mpl_core_cpi_signer()`
4. `Cargo.toml`（workspace root）— mpl-core関連依存

## 要件

### programs/title-config に命令追加

- `delegate_collection_authority(ctx, tee_signing_pubkey)` — Collection AuthorityをTEE署名鍵に委譲
  - authority（DAO管理者）のみ実行可能
  - MPL CoreのCPIでプラグイン権限を委譲
- `revoke_collection_authority(ctx, tee_signing_pubkey)` — 委譲の取り消し（不正TEE発覚時）
  - authority（DAO管理者）のみ実行可能

### GlobalConfig への追加（必要に応じて）

- `core_collection_mint` と `ext_collection_mint` がGlobalConfigに定義済みか確認
  - 未定義なら追加

## 完了条件

- `delegate_collection_authority` / `revoke_collection_authority` 命令が定義される
- テスト追加
- `cargo check --workspace` 警告ゼロ
