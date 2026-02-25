# GlobalConfig 運用ガイド (v1)

## 概要

GlobalConfig PDA は Title Protocol の**信頼の原点**である。
全ての TEE ノード、WASM モジュール、TSA 鍵、コレクション情報がここに集約され、
SDK はこの PDA を参照してノードの信頼性を検証する。

- **ネットワークごとに唯一**: devnet に 1 つ、mainnet に 1 つ
- **全更新に authority 署名が必要**: 不正な変更は不可能
- **v1 では単一 keypair 管理**: 将来的に Squads 等の multi-sig へ移行予定

---

## オンチェーン情報

| 項目 | 値 |
|------|-----|
| Program ID | `C2HryYkBKeoc4KE2RJ6au1oXc1jtKeKw3zrknQ455JQN` |
| PDA seed | `b"global-config"` |
| Space | 8 + 32 + 32 + 32 + 4 + 4 + 4 + 1024 = 1140 bytes |

### Devnet 現在値

| 項目 | 値 |
|------|-----|
| Authority | `wrVwsTuRzbsDutybqqpf9tBE7JUqRPYzJ3iPUgcFmna` |
| GlobalConfig PDA | `W4AYqC9sFpuHz8LbeuB8jtxNReUkGkjzrWr3EUftsWZ` |
| Core Collection | `3z7mdLX7TsMkuCtX34cnDVZVFo7y9zrtha48vmm7dd6K` |
| Extension Collection | `Ey7DmfeX5wiWGEVfzpaPbJSVSZkjrLxy86zt9Wsn7Fk` |

---

## フィールド一覧

| フィールド | 型 | 更新命令 | 説明 |
|-----------|---|---------|------|
| `authority` | Pubkey | 初期化時に固定 | 全更新操作の署名者。変更不可 |
| `core_collection_mint` | Pubkey | `update_collections` | Core cNFT コレクション |
| `ext_collection_mint` | Pubkey | `update_collections` | Extension cNFT コレクション |
| `trusted_tee_nodes` | Vec | `update_tee_nodes` | 信頼された TEE ノードリスト |
| `trusted_wasm_modules` | Vec | `update_wasm_modules` | 信頼された WASM モジュールリスト |
| `trusted_tsa_keys` | Vec | `update_tsa_keys` | 信頼する TSA 鍵ハッシュリスト |

### TrustedTeeNodeAccount (98 bytes/node)

```
signing_pubkey:    [u8; 32]  — TEE の Ed25519 公開鍵（cNFT 署名用）
encryption_pubkey: [u8; 32]  — TEE の X25519 公開鍵（E2EE 用）
gateway_pubkey:    [u8; 32]  — Gateway の Ed25519 公開鍵（認証用）
status:            u8        — 0=Inactive, 1=Active
tee_type:          u8        — 0=aws_nitro, 1=amd_sev_snp, 2=intel_tdx
```

### TrustedWasmModuleAccount (64 bytes/module)

```
extension_id: [u8; 32]  — UTF-8 識別子（右ゼロ埋め、例: "phash-v1")
wasm_hash:    [u8; 32]  — WASM バイナリの SHA-256 ハッシュ
```

---

## 命令一覧

| 命令 | discriminator | 用途 |
|------|--------------|------|
| `initialize` | `sha256("global:initialize")[..8]` | PDA 作成（1 回限り） |
| `update_collections` | `sha256("global:update_collections")[..8]` | コレクション Mint 更新 |
| `update_tee_nodes` | `sha256("global:update_tee_nodes")[..8]` | TEE ノードリスト全置換 |
| `update_wasm_modules` | `sha256("global:update_wasm_modules")[..8]` | WASM モジュールリスト全置換 |
| `update_tsa_keys` | `sha256("global:update_tsa_keys")[..8]` | TSA 鍵リスト全置換 |
| `delegate_collection_authority` | `sha256("global:delegate_collection_authority")[..8]` | TEE への Collection Authority 委譲 |
| `revoke_collection_authority` | `sha256("global:revoke_collection_authority")[..8]` | Collection Authority 取り消し |

**重要**: `update_tee_nodes` / `update_wasm_modules` / `update_tsa_keys` はリスト**全体を置き換える**。
追加時は既存エントリも含めた完全なリストを渡すこと。

---

## 運用スクリプト

`scripts/init-devnet.mjs` が GlobalConfig の初期化と更新を一括で行う（冪等）。

```bash
cd scripts && npm install

# 全ステップ実行
node init-devnet.mjs --rpc https://api.devnet.solana.com --gateway http://<IP>:3000

# ノード情報のみ更新（Tree 作成スキップ）
node init-devnet.mjs --rpc https://api.devnet.solana.com --gateway http://<IP>:3000 --skip-tree

# Collection Authority 委譲をスキップ
node init-devnet.mjs --rpc https://api.devnet.solana.com --gateway http://<IP>:3000 --skip-delegate
```

スクリプトの処理フロー:
1. Authority keypair をロード（`deploy/aws/keys/devnet-authority.json`）
2. MPL Core コレクション 2 つを作成（既存なら skip）
3. GlobalConfig 初期化 or `update_collections`
4. Gateway から `gateway_pubkey`、`tee-info.json` から `signing_pubkey` / `encryption_pubkey` 取得
5. `update_tee_nodes` でノード登録
6. ローカル WASM バイナリから SHA-256 計算 → `update_wasm_modules`
7. `delegate_collection_authority`（Core + Extension）
8. TEE `/create-tree` で Merkle Tree 作成

---

## ノード追加

### 手順

1. **ノードを起動する**
   - `deploy/aws/setup-ec2.sh` を実行（TEE + Gateway + Indexer が立ち上がる）

2. **ノード情報を取得する**
   ```bash
   # Gateway の公開鍵
   curl http://<IP>:3000/.well-known/title-node-info
   # => { "signing_pubkey": "<gateway_pubkey>", ... }

   # TEE の公開鍵（tee-info.json に保存される）
   # signing_pubkey, encryption_pubkey
   ```

3. **GlobalConfig を更新する**
   ```bash
   node init-devnet.mjs --rpc <RPC_URL> --gateway http://<IP>:3000
   ```
   これにより `update_tee_nodes` が既存ノード + 新ノードで呼ばれる。

4. **Collection Authority を委譲する**
   - 上記スクリプトが自動で `delegate_collection_authority` を実行
   - 加えて MPL Core の `addCollectionPlugin(UpdateDelegate)` をクライアントサイドで実行

5. **Merkle Tree を作成する**
   - スクリプトが TEE の `/create-tree` を呼び出し、トランザクションをブロードキャスト

---

## ノード削除

### 正常停止

1. ノードを停止する（TEE の秘密鍵はメモリから消滅）
2. `update_tee_nodes` を実行し、該当ノードを除外したリストを渡す
3. 既存の cNFT はそのまま有効（burn されない）

### 不正検知時

1. `revoke_collection_authority` を実行（collection_type: 0 と 1 の両方）
2. 必要に応じて、当該ノードがミントした cNFT に対して `unverify` を実行
3. `update_tee_nodes` でノードを除外

---

## TEE 再起動時の対応

TEE はステートレスなので、再起動すると signing_pubkey / encryption_pubkey が再生成される。

1. 新しい鍵情報を取得（Gateway の `/.well-known/title-node-info` + TEE 起動ログ）
2. `init-devnet.mjs` を再実行 → `update_tee_nodes` で GlobalConfig を更新
3. `delegate_collection_authority` を再実行
4. `/create-tree` で新しい Merkle Tree を作成

**v1 の制限**: 再起動のたびに authority による手動更新が必要。
ノード数が少ないうちはこの運用で十分だが、
将来的にはノード自己登録 + TEE Attestation 自動承認のスマートコントラクトを検討する。

---

## WASM モジュール更新

1. WASM モジュールを再ビルド
   ```bash
   cd wasm/<module>
   cargo build --target wasm32-unknown-unknown --release
   ```
2. `init-devnet.mjs` を再実行 → SHA-256 を自動計算して `update_wasm_modules` を呼ぶ

既存の cNFT は古い WASM で処理されたまま有効。新規登録のみ新 WASM が適用される。

---

## Authority 鍵の管理

- Authority keypair は `deploy/aws/keys/<env>-authority.json` に保存（`.gitignore` 済み）
- **この鍵を失うと GlobalConfig の更新が永久に不可能になる**
- バックアップを必ず取ること
- Mainnet では Squads Protocol 等の multi-sig を強く推奨

---

## トラブルシューティング

| 症状 | 原因 | 対処 |
|------|------|------|
| `has_one = authority` 失敗 | keypair が GlobalConfig の authority と不一致 | 正しい keypair を使用 |
| `custom program error: 0x0` | `initialize` で既にアカウント存在 | `update_collections` を使用 |
| `account data too small` | プログラム更新時に .so サイズ増加 | `solana program extend <ID> 30000` |
| `InstructionFallbackNotFound` | デプロイされたプログラムが古い | `cargo-build-sbf --tools-version v1.52` でリビルド → 再デプロイ |
| airdrop 失敗 | devnet レート制限 | https://faucet.solana.com/ または手動送金 |
