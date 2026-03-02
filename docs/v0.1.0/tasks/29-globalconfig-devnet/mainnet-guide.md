# GlobalConfig + Collection ゼロベース構築ガイド（Mainnet対応）

devnetでの実施経験をもとに、ゼロからGlobalConfigとCollectionを構築する手順をまとめる。
メインネット・新規devnet環境どちらにも適用可能。

---

## 前提

- Solana CLI 2.1+ (`solana --version`)
- Anchor CLI 0.30.1 (`anchor --version`)
- `cargo-build-sbf` (Solana Platform Tools内)
- Node.js 18+ (`node --version`)
- 十分なSOL残高（メインネット: ~5 SOL、devnet: airdrop可）

---

## Step 1: Authority Keypair の準備

GlobalConfigの管理者キーペア。**この鍵を失うと全設定の更新が不可能になる**。

```bash
# 新規生成
solana-keygen new -o deploy/aws/keys/<env>-authority.json --no-bip39-passphrase

# または既存のウォレットを使用
cp ~/.config/solana/id.json deploy/aws/keys/<env>-authority.json

# アドレス確認
solana address -k deploy/aws/keys/<env>-authority.json
```

**重要**:
- `deploy/aws/keys/` は `.gitignore` 済み。秘密鍵はgitに入らない
- メインネットでは multi-sig (Squads等) の使用を推奨。単一keypairはリスクが高い
- バックアップを必ず取ること

---

## Step 2: Anchorプログラムのビルド

```bash
cd programs/title-config

# Cargo.lock を再生成（Platform Tools互換性のため）
rm -f Cargo.lock && cargo generate-lockfile

# ビルド（Platform Tools v1.52 指定が安定）
~/.local/share/solana/install/active_release/bin/cargo-build-sbf \
  --manifest-path Cargo.toml --tools-version v1.52
```

**罠まとめ** (詳細: `docs/v0.1.0/tasks/12-e2e-local-dev/solana-build-notes.md`):
- `anchor build` でなく `cargo-build-sbf` を直接使う（tools-version指定のため）
- Platform Tools は **v1.52** が最も安定。v1.53はsbpfv3移行中で問題あり
- Rust Edition 2024 に対応するには v1.52 以上が必須
- .so の出力先は `programs/title-config/target/deploy/title_config.so`

---

## Step 3: プログラムのデプロイ

### 初回デプロイ

```bash
# プログラムキーペア生成（初回のみ）
mkdir -p programs/title-config/target/deploy
solana-keygen new -o programs/title-config/target/deploy/title_config-keypair.json \
  --no-bip39-passphrase

# Program ID を確認 → lib.rs の declare_id! と Anchor.toml を更新
solana address -k programs/title-config/target/deploy/title_config-keypair.json

# デプロイ
solana program deploy programs/title-config/target/deploy/title_config.so \
  --program-id programs/title-config/target/deploy/title_config-keypair.json \
  --url <RPC_URL>
```

### アップグレードデプロイ（既存プログラムの更新）

```bash
# 新しい .so が旧領域より大きい場合、先に拡張
solana program show <PROGRAM_ID> --url <RPC_URL>
# Data Length を確認 → 新 .so のサイズと比較
ls -la programs/title-config/target/deploy/title_config.so

# 必要なら拡張（差分 + 余裕をもって）
solana program extend <PROGRAM_ID> 30000 --url <RPC_URL>

# デプロイ
solana program deploy programs/title-config/target/deploy/title_config.so \
  --program-id <PROGRAM_ID> --url <RPC_URL>
```

**罠**:
- `account data too small` → `solana program extend` で領域拡張が必要
- deploy に失敗すると ephemeral keypair の seed phrase が表示される。これで `solana program close` してlamportsを回収可能

---

## Step 4: コレクション作成 + GlobalConfig初期化

`scripts/init-devnet.mjs` を使用する。スクリプトは冪等（何度実行しても安全）。

### 依存パッケージのインストール

```bash
cd scripts && npm install
```

必要なパッケージ（`package.json`に定義済み）:
- `@solana/web3.js` — Solana SDK
- `@metaplex-foundation/umi` — Metaplex Umi フレームワーク
- `@metaplex-foundation/umi-bundle-defaults` — Umi のデフォルト実装
- `@metaplex-foundation/mpl-core` — MPL Core コレクション操作

### 実行

```bash
# GlobalConfig初期化 + コレクション作成 + TEEノード登録 + WASMモジュール登録
node init-devnet.mjs --rpc <RPC_URL> --gateway http://<GATEWAY_IP>:3000
```

スクリプトの動作:

1. **Authority keypair** — `deploy/aws/keys/devnet-authority.json` をロード（なければ生成）
2. **MPL Core Collection × 2** — "Title Protocol Core" と "Title Protocol Extension"
3. **GlobalConfig** — 未初期化なら `initialize`、コレクションが無効なら `update_collections`
4. **TEEノード** — Gateway `/.well-known/title-node-info` からgateway_pubkey取得、`tee-info.json` からsigning/encryption_pubkey取得
5. **WASMモジュール** — ローカルの .wasm バイナリからSHA-256ハッシュを計算して登録
6. **Collection Authority委譲** — Anchor命令 + MPL Core UpdateDelegateプラグイン追加
7. **Merkle Tree** — TEE `/create-tree` → signed_tx ブロードキャスト
8. **ガイダンス** — `COLLECTION_MINT` の .env 設定方法を表示

### オプション

| フラグ | 効果 |
|-------|------|
| `--rpc <url>` | Solana RPC URL |
| `--gateway <url>` | Gateway URL |
| `--skip-tree` | Merkle Tree作成をスキップ |
| `--skip-delegate` | Collection Authority委譲をスキップ |

---

## Step 5: TEE環境への反映

GlobalConfig初期化後、TEE (Enclave) に `COLLECTION_MINT` を設定する必要がある。

```bash
ssh ec2-user@<EC2_IP>

# .env に COLLECTION_MINT を追加
echo 'COLLECTION_MINT=<Core Collection Mint Address>' >> ~/title-protocol/.env

# Enclave再起動
sudo systemctl restart nitro-enclave  # または手動でstop→start
```

TEEは再起動すると鍵が再生成される（ステートレス設計）。
再起動後は `init-devnet.mjs` を再実行してTEEノード情報を更新する。

---

## Step 6: E2Eテスト

```bash
cd scripts
node test-devnet.mjs --gateway http://<GATEWAY_IP>:3000 --rpc <RPC_URL>
```

成功すれば:
- C2PA画像の検証 (`/verify`) → signed_json生成
- cNFTミント (`/sign`) → 部分署名付きトランザクション
- クライアント署名 + ブロードキャスト → devnet上でcNFT確認

---

## Anchor命令一覧とdiscriminator

| 命令 | discriminator計算 | 用途 |
|------|-------------------|------|
| `initialize` | `sha256("global:initialize")[..8]` | GlobalConfig PDA作成（1回限り） |
| `update_collections` | `sha256("global:update_collections")[..8]` | コレクションMint更新 |
| `update_tee_nodes` | `sha256("global:update_tee_nodes")[..8]` | TEEノードリスト更新 |
| `update_wasm_modules` | `sha256("global:update_wasm_modules")[..8]` | WASMモジュールリスト更新 |
| `update_tsa_keys` | `sha256("global:update_tsa_keys")[..8]` | TSA鍵リスト更新 |
| `delegate_collection_authority` | `sha256("global:delegate_collection_authority")[..8]` | TEEへのCollection Authority委譲（イベント発行） |
| `revoke_collection_authority` | `sha256("global:revoke_collection_authority")[..8]` | Collection Authority取り消し |

---

## GlobalConfig PDAのレイアウト

```
seeds = [b"global-config"]
program = <PROGRAM_ID>
space = 8 + 32 + 32 + 32 + 4 + 4 + 4 + 1024

オフセット:
  [0..8]     Anchor discriminator
  [8..40]    authority: Pubkey
  [40..72]   core_collection_mint: Pubkey
  [72..104]  ext_collection_mint: Pubkey
  [104..]    trusted_tee_nodes: Vec<TrustedTeeNodeAccount>  (4-byte len prefix)
             trusted_tsa_keys: Vec<[u8; 32]>                (4-byte len prefix)
             trusted_wasm_modules: Vec<TrustedWasmModuleAccount> (4-byte len prefix)
```

---

## メインネット固有の注意点

1. **SOLコスト**: プログラムデプロイ ~1.5 SOL、コレクション作成 ~0.01 SOL × 2、GlobalConfig PDA ~0.01 SOL
2. **Authority管理**: 単一keypairではなく multi-sig (Squads Protocol等) を強く推奨
3. **プログラムのupgrade authority**: メインネットでは最終的に `solana program set-upgrade-authority <PROGRAM_ID> --final` でイミュータブルにすることを検討
4. **airdropは使えない**: メインネットのSOLは取引所等から調達
5. **RPC**: 公開RPCは本番用途に不適切。Helius、Triton等の専用RPCを使用
6. **コレクション・GlobalConfigは唯一**: ネットワークごとに1つ。devnetとmainnetは完全に独立

---

## トラブルシューティング

| 症状 | 原因 | 対処 |
|------|------|------|
| `InstructionFallbackNotFound` | デプロイされたプログラムが古い | `cargo-build-sbf` でリビルド → 再デプロイ |
| `account data too small` | 新 .so > 既存領域 | `solana program extend` で拡張 |
| `has_one = authority` 失敗 | 使用中のkeypairがGlobalConfigのauthorityと不一致 | GlobalConfig PDA [8..40] からauthorityを読み取り、正しいkeypairを使用 |
| `edition2024` ビルドエラー | Platform Tools が古い | `--tools-version v1.52` を指定 |
| `custom program error: 0x0` | Anchor `init` で既にアカウントが存在 | `update_collections` を使用（再初期化は不可） |
| airdrop失敗 | devnetレート制限 | https://faucet.solana.com/ または手動送金 |
| コレクション作成失敗 | SOL不足 | `solana balance` で確認、必要額を送金 |
