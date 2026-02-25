# タスク29: GlobalConfig完全初期化 + devnet完成

## 概要

devnet上でTitle Protocolの「信頼の連鎖」を完成させる。
現状の `init-config.mjs` はダミーのコレクションMint・ゼロ埋めの公開鍵を使用しており、
ミントされたcNFTはコレクション認証されていない。

本タスクで以下を実現する:
1. MPL Core コレクション作成（Core + Extension）
2. GlobalConfig の全フィールドを正しい値で初期化
3. Collection Authority を TEE signing_pubkey に委譲
4. Authority keypair を永続保管
5. コレクション認証付き cNFT ミントの E2E 確認

## 仕様書セクション

- §5.2 Step 1: Global Config
- §8.2: TEEノードの追加・Collection Authority委譲
- §1.2: 信頼の連鎖（Global Config → Collection → cNFT → Off-chain Data）

## 前提タスク

- タスク17（Devnetデプロイ基盤）完了
- タスク16（Collection Authority Delegate）完了
- EC2 + Nitro Enclave が稼働中（またはモックTEE）

## 成果物

| ファイル | 内容 |
|---------|------|
| `scripts/init-devnet.mjs` | devnet完全初期化スクリプト（冪等） |
| `deploy/aws/keys/devnet-authority.json` | Authority keypair（.gitignore済み） |
| `programs/title-config/src/lib.rs` | `update_collections` 命令を追加 |

## 実施記録（2026-02-23）

### 1. Anchorプログラム修正

`programs/title-config/src/lib.rs` に `update_collections` 命令を追加。
GlobalConfigが既にinitialize済みでもコレクションMintを後から更新可能にした。

```rust
pub fn update_collections(
    ctx: Context<UpdateConfig>,
    core_collection_mint: Pubkey,
    ext_collection_mint: Pubkey,
) -> Result<()>
```

### 2. プログラムのリビルド＆アップグレードデプロイ

```bash
cd programs/title-config
rm -f Cargo.lock && cargo generate-lockfile
~/.local/share/solana/install/active_release/bin/cargo-build-sbf \
  --manifest-path Cargo.toml --tools-version v1.52

# .so が既存のプログラムデータ領域より大きかったため拡張
solana program extend C2HryYkBKeoc4KE2RJ6au1oXc1jtKeKw3zrknQ455JQN 30000 \
  --url https://api.devnet.solana.com

solana program deploy target/deploy/title_config.so \
  --program-id C2HryYkBKeoc4KE2RJ6au1oXc1jtKeKw3zrknQ455JQN \
  --url https://api.devnet.solana.com
```

**罠**: `anchor build` は .so を `<project_root>/target/deploy/` に出すが、
`cargo-build-sbf` は `programs/title-config/target/deploy/` に出す。デプロイ時にパスを間違えると古い .so が上がる。

**罠**: 新しい .so が旧プログラムデータ領域より大きい場合、
`solana program extend <PROGRAM_ID> <BYTES>` で事前に拡張が必要。

### 3. Authority keypair

既存のデフォルトキーペア（`~/.config/solana/id.json`）をそのまま使用。
GlobalConfig PDAのauthorityとプログラムのupgrade authorityが同一ウォレットのため。

```bash
cp ~/.config/solana/id.json deploy/aws/keys/devnet-authority.json
```

### 4. init-devnet.mjs 実行結果

```
Authority:            wrVwsTuRzbsDutybqqpf9tBE7JUqRPYzJ3iPUgcFmna
GlobalConfig PDA:     W4AYqC9sFpuHz8LbeuB8jtxNReUkGkjzrWr3EUftsWZ
Core Collection:      3z7mdLX7TsMkuCtX34cnDVZVFo7y9zrtha48vmm7dd6K
Extension Collection: Ey7DmfeX5wiWGEVfzpaPbJSVSZkjrLxy86zt9Wsn7Fk
Program ID:           C2HryYkBKeoc4KE2RJ6au1oXc1jtKeKw3zrknQ455JQN
```

| ステップ | 結果 |
|---------|------|
| Authority keypair ロード | OK |
| MPL Core Collection × 2 作成 | OK |
| update_collections | OK |
| TEEノード登録 (update_tee_nodes) | OK |
| WASMモジュール登録 × 4 (update_wasm_modules) | OK |
| Collection Authority 委譲 | 未実施（EC2接続要） |
| Merkle Tree 作成 | 未実施（EC2接続要） |

### 5. 遭遇した問題と解決

| 問題 | 原因 | 解決 |
|------|------|------|
| devnet airdropが失敗 | レート制限 | 手動でSOL送金 |
| GlobalConfig再初期化不可 | Anchor `init` は1回限り | `update_collections` 命令を追加 |
| update_collections が InstructionFallbackNotFound | `anchor build` が正しい .so を生成していなかった | `cargo-build-sbf --tools-version v1.52` でリビルド |
| `solana program deploy` が account data too small | 新 .so が旧領域より大きい | `solana program extend` で30KB拡張 |
| update_tee_nodes が custom program error | authority不一致（新keypair ≠ PDA内のauthority） | 既存keypairを使用 |

## 完了条件

- [x] `deploy/aws/keys/devnet-authority.json` が生成・保存される
- [x] devnet上に Core / Extension の MPL Core コレクションが存在する
- [x] GlobalConfig PDA に全フィールドが正しく登録される（コレクション・TEEノード・WASMモジュール）
- [ ] TEE signing_pubkey が両コレクションの delegate authority を持つ（EC2接続後）
- [ ] コレクション認証付き cNFT が正常にミントされる（E2E、EC2接続後）
- [ ] `COLLECTION_MINT` が .env に反映されている（EC2接続後）

## 残作業（EC2接続時）

```bash
# EC2上で COLLECTION_MINT を設定 → Enclave再起動
ssh ec2-user@<IP>
echo 'COLLECTION_MINT=3z7mdLX7TsMkuCtX34cnDVZVFo7y9zrtha48vmm7dd6K' >> ~/title-protocol/.env

# Collection Authority委譲 + Merkle Tree作成
cd scripts
node init-devnet.mjs --rpc https://api.devnet.solana.com --gateway http://<EC2_IP>:3000

# E2Eテスト
node test-devnet.mjs --gateway http://<EC2_IP>:3000
```
