# タスク5: TEE /sign + /create-tree + Bubblegum連携

## 前提タスク

- タスク4（/verify）が完了していること

## 読むべきファイル

1. `docs/SPECS_JA.md` — §1.1「Phase 2: Sign」§6.4「/sign フェーズの内部処理」「/sign フェーズでの防御（Verify on Sign）」「TEE起動シーケンス Step 2」
2. `crates/tee/src/endpoints/sign.rs` — 現在のスタブ
3. `crates/tee/src/endpoints/create_tree.rs` — 現在のスタブ（状態遷移ロジックのみ実装済み）
4. `crates/tee/src/main.rs` — AppState、ルーティング
5. `crates/types/src/lib.rs` — SignRequest, SignResponse, CreateTreeRequest, CreateTreeResponse

## 作業内容

### /sign エンドポイント

1. `signed_json_uri` からプロキシ経由でJSONを取得（サイズ制限付き: 1MB上限）
2. JSON内の `tee_signature` を自身の公開鍵で検証（自身が生成したsigned_jsonであることの確認）
3. cNFT発行トランザクション構築（Bubblegum `mint_v1` 命令）
4. TEE秘密鍵で部分署名
5. 部分署名済みトランザクションを返却

Bubblegum連携には `mpl-bubblegum` クレートを使用する。Cargo.tomlへの依存追加が必要。

### /create-tree エンドポイント

現在の状態遷移ロジック（inactive→active）は実装済み。以下を追加:

1. Tree用Ed25519キーペアの生成（TeeRuntime traitに `generate_tree_keypair()` を追加）
2. `spl-account-compression` の `create_tree` 命令でMerkle Tree作成トランザクション構築
3. 署名用 + Tree用の両キーペアで部分署名
4. レスポンスに `tree_address`, `signing_pubkey`, `encryption_pubkey` を含める

### AppStateの拡張

現在のAppStateにはMerkle Treeアドレス等の情報がない。以下を追加:
- `tree_address: RwLock<Option<Pubkey>>` — Merkle Treeのアドレス
- 必要に応じて他のフィールド

## 完了条件

- /sign: タスク4で生成したsigned_jsonのURIを渡し、部分署名済みトランザクションが返る
- /sign: TEE再起動（鍵ローテーション）後に旧signed_jsonが拒否されるテスト
- /sign: サイズ制限を超えるURIが拒否されるテスト
- /create-tree: inactive状態で呼び出しが成功し、active状態に遷移する
- /create-tree: active状態での二度目の呼び出しがエラーになる
- `cargo check --workspace && cargo test --workspace` が通る
- `docs/COVERAGE.md` の該当箇所を更新
