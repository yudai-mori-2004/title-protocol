# Title Protocol 運用ガイド (v0.1.2)

仕様書 (`SPECS_JA.md`) は「何を作るか」を、本書は「どうやって動かすか」を扱う。クローンしたばかりのリポジトリから TEE を AWS Nitro Enclaves 上で起動し、Solana 上の whitelist に鍵を登録するまでを一通り示す。

> 関連ドキュメント
> - 仕様: [SPECS_JA.md](./SPECS_JA.md)
> - 実装カバレッジ: [COVERAGE.md](./COVERAGE.md)
> - AWS デプロイの README: [../../deploy/aws/README.md](../../deploy/aws/README.md)

---

## 0. 全体像

Title Protocol は 3 つの実行主体で構成される（仕様 §0.6, §5.1）。

```
       ┌──────────┐                ┌───────────┐               ┌─────────┐
Client │ External │  HTTPS         │ AWS Nitro │     vsock     │ Gateway │  HTTPS
──────▶│ Storage  │◀─── fetch ────│ Enclave   │◀────proxy────▶│ (EC2)   │◀──── Client
       │ (R2 等)  │                │  (TEE)    │               │         │
       └──────────┘                └─────────┘               └─────────┘
                                         │
                                         │ 起動時/事前作業:
                                         │   Attestation → SP1 ZK proof
                                         ▼
                                   ┌──────────────┐
                                   │   Solana     │
                                   │ title-       │
                                   │ whitelist    │
                                   └──────────────┘
```

| コンポーネント | 役割 | 仕様参照 |
|---|---|---|
| Gateway | クライアント認証・TEE 情報中継 | §5.3 |
| TEE | C2PA 検証 + 属性抽出 + Attestation 封印 | §5.2 |
| External Storage | クライアントが管理。コンテンツの実体置場 | §0.4 |
| Solana `title-whitelist` | TEE 署名鍵の許可リスト・cNFT 信頼根拠 | §6.2 |

運用者の責任範囲は **Gateway + TEE + Solana コントラクト** の 3 つ。External Storage は利用者の責任。

---

## 1. ライフサイクル全体

Title Protocol が動き出すまでの依存関係を時系列で示す（仕様 §6.2）。
各ステップ右側に対応する `title-cli` サブコマンドを併記。

```
[1] 検証回路の同一性指定                  ── プロトコル運営者(1回 + guest コード更新時)
       SP1 guest をビルド → vkey_hash 取得    cargo run --bin vkey
       admin が許可リストに追加              title-cli add-vkey
                  │
                  ▼
[2] TEE バイナリの同一性指定              ── プロトコル運営者(リリース毎)
       TEE バイナリをビルド (EIF) → PCR0    bash build.sh
       admin が許可リストに追加              title-cli add-measurement
                  │
                  ▼
[3] TEE 署名鍵を whitelist 登録          ── TEE 起動毎(90日有効、仕様 §6.2)
       TEE 起動 → 鍵生成 → attestation      bash run.sh + fetch-registration-bundle.sh
       SP1 proof 生成                       bash prover-run.sh
       register_key 提出                    title-cli register-key
                  │
                  ▼
[4] cNFT 発行                            ── アプリ利用毎
       Client → Gateway → /extension/solana title-cli mint (or SDK)
       TEE が部分署名 → Client が最終署名
```

`[1]` `[2]` はコード変更時のみ。`[3]` は TEE 起動毎（仕様 §0.5 stateless 設計）。
`[4]` がアプリの日常利用。

許可レジストリは `title-cli init-registries` でプログラムデプロイ後に 1 回だけ
初期化する（冪等）。状態確認は `title-cli describe-whitelist`、鍵取り消しは
`title-cli revoke-key`、許可リスト掃除は `title-cli remove-vkey` / `remove-measurement`。

---

## 2. デプロイ手順

### 2.1 前提

ローカル開発マシン（手元の Mac/Linux）に必要なもの:

| 項目 | 用途 | インストール |
|---|---|---|
| Rust toolchain | リポジトリ全体のビルド | `rust-toolchain.toml` が自動選択 |
| Anchor CLI 0.30.1 | Solana プログラムのビルド | `cargo install --git https://github.com/coral-xyz/anchor anchor-cli --tag v0.30.1` |
| Solana CLI 3.x | プログラムデプロイ・admin 鍵作成 | `sh -c "$(curl -sSfL https://release.anza.xyz/stable/install)"` |
| AWS CLI v2 | EC2 / Terraform 操作 | https://docs.aws.amazon.com/cli/v2/ |
| Terraform 1.5+ | EC2 + SG + 鍵をプロビジョン | https://developer.hashicorp.com/terraform/install |

`title-cli` 自身も同じリポジトリでビルドする。後続のコマンドはすべて
`cargo run --release -p title-cli -- <subcommand>` または `./target/release/title-cli <subcommand>`
で呼べる。本書では短く **`title-cli <subcommand>`** と表記する。

```bash
cargo build --release -p title-cli
alias title-cli="$PWD/target/release/title-cli"   # 一時的に PATH 通すなら
```

admin 鍵は `keys/admin.json` に置く。プログラムの `ADMIN_AUTHORITY`
(`wrVwsTuRzbsDutybqqpf9tBE7JUqRPYzJ3iPUgcFmna`) と一致している必要がある。
チーム内では同じ鍵を共有する（変更したい場合は仕様 §6.2 に従いプログラム再デプロイ）。

### 2.2 Solana コントラクトのデプロイ

```bash
anchor build --no-idl
solana program deploy \
  --url <devnet|mainnet-beta> \
  --keypair keys/admin.json \
  --upgrade-authority keys/admin.json \
  --program-id programs/title-whitelist/keypair.json \
  programs/title-whitelist/target/deploy/title_whitelist.so
```

`--no-idl` は anchor 0.30.1 と最新 proc-macro2 の非互換を回避するため。
アップグレードも同じコマンドで実行できる。

> 既にデプロイ済みの devnet (`43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs`) を使う場合は本ステップ不要。

### 2.3 許可レジストリの初期化

仕様 §6.2 の「許容 verifying_key_hash 集合」と「許容 measurement 集合」両方の
PDA を作る。プログラムデプロイ後に **1 回だけ** 実行。冪等（既に初期化済みなら
スキップ）。

```bash
title-cli init-registries --admin keys/admin.json
```

### 2.4 SP1 guest ビルドと vkey_hash の登録

仕様 §6.2 「確認 1: 検証回路の正規性」用の指紋を取得・登録する。検証回路の
Rust コード (`sp1-guests/attestation-aws-nitro/`) を変更しない限り変わらない。

```bash
# SP1 toolchain を入れる（初回のみ）
curl -L https://sp1.succinct.xyz | bash
~/.sp1/bin/sp1up --version v6.2.0

# vkey_hash を取得
cd sp1-guests/attestation-aws-nitro/host
cargo run --release --locked --bin vkey
cd -
# 出力: 0x00d742a0c7af54b880c0bc27eaff7f8f481cd75d9cd7b2516fea02e9ded29754
```

得られた hex を許可リストに追加:

```bash
title-cli add-vkey \
  --admin keys/admin.json \
  --vkey-hex 0x00d742a0c7af54b880c0bc27eaff7f8f481cd75d9cd7b2516fea02e9ded29754
```

> guest 更新時は新しい vkey を追加してから旧 vkey を `title-cli remove-vkey`
> で外す。新旧併存期間中はどちらの proof も受理される。

### 2.5 EC2 インフラ準備

`deploy/aws/terraform` で 1 台の `c5.xlarge` (Nitro Enclaves 対応) を立てる。
SSH 鍵 (`deploy/aws/keys/title-protocol-devnet.pem`) は Terraform が
自動生成し、初回起動時に `user-data.sh` が Docker・nitro-cli・hugepage を
セットアップする。

```bash
cd deploy/aws/terraform
terraform init
terraform apply
cd -

# SSH で入る
PUBLIC_IP=$(terraform -chdir=deploy/aws/terraform output -raw public_ip)
ssh -i deploy/aws/keys/title-protocol-devnet.pem ec2-user@$PUBLIC_IP
```

> **以降 §2.6〜§2.8 のコマンドは EC2 上で実行する。** §2.9 以降は手元に
> 戻る。

### 2.6 TEE バイナリのビルド（EC2 上）

EC2 にリポジトリを clone し、3 つの Docker image (`tee-nitro` / `title-proxy`
/ `title-gateway`) をビルドして `tee-nitro` から EIF を生成する。

```bash
# === EC2 上 ===
git clone <repo-url> ~/title-protocol
cd ~/title-protocol
bash deploy/aws/scripts/build.sh
```

末尾に PCR0/PCR1/PCR2 が表示される。**PCR0 をメモする** (例:
`bab9ec51dcefb562...`)。

> リプロデューシブルビルド (仕様 §5.4): 同じコミット + 同じ DockerHub の
> ベースイメージから別マシンでビルドしても同じ PCR0 が得られる。確認方法は
> §6.2 (`bash deploy/aws/scripts/build.sh --verify`)。

### 2.7 スタック起動（EC2 上）

```bash
# === EC2 上 ===
bash deploy/aws/scripts/run.sh
```

`title-proxy` → Nitro Enclave → socat ブリッジ → `title-gateway` の順で起動。
デフォルトは **release-mode** （PCR が実値）。`ENCLAVE_DEBUG=1` を設定すると
debug-mode で起動するが、NSM が発行する Attestation Document の PCR が全て
ゼロになり on-chain 登録に使えないため、本番では絶対に設定しない。

起動確認:

```bash
# 外部から
curl -sf http://<EC2_PUBLIC_IP>:3000/health
# {"status":"ok","tee_type":"aws-nitro"}
```

### 2.8 PCR0 と registration attestation の取得（EC2 上）

```bash
# === EC2 上 ===
bash deploy/aws/scripts/fetch-registration-bundle.sh
```

`deploy/aws/build/registration/` に以下が出力される:

| ファイル | 内容 |
|---|---|
| `measurements.json` | PCR0 / PCR1 / PCR2 (`nitro-cli describe-enclaves` 由来) |
| `pcr0.hex` | PCR0 単体 (hex 96 文字) |
| `solana_pubkey.txt` | TEE が起動時にメモリ内生成した Ed25519 公開鍵 (Base58) |
| `attestation.bin` | NSM が発行した Attestation Document (CBOR バイト列、`user_data = SHA-256(b"title:solana-key" \|\| solana_pubkey)` で binding) |

debug-mode で起動した Enclave だと本スクリプトは fail-fast する。

> **この 4 ファイルは同一 TEE 起動から取得したセット**。バラバラに差し替えると
> §4 で `UserDataMismatch` になる。

### 2.9 PCR0 の登録 (`add-measurement`)

`measurements.json` の `PCR0` 値を手元 (Solana CLI / AWS CLI が使える側) で
許可リストに追加する。EC2 上の `registration/` ディレクトリをそのままローカルに
コピーする想定（§4 で同じディレクトリを `register-key --bundle` の入力にする）:

```bash
# === ローカル ===
mkdir -p deploy/aws/build/registration
scp -i deploy/aws/keys/title-protocol-devnet.pem \
    'ec2-user@<EC2_PUBLIC_IP>:~/title-protocol/deploy/aws/build/registration/*' \
    deploy/aws/build/registration/

# PCR0 を許可リストに追加
PCR0=$(jq -r '.PCR0' deploy/aws/build/registration/measurements.json)
title-cli add-measurement --admin keys/admin.json --pcr0-hex 0x$PCR0
```

> TEE バイナリ更新時は新 PCR0 を追加してから旧 PCR0 を
> `title-cli remove-measurement` で外す。

### 2.10 Gateway 設定

`API_KEYS` 環境変数が空のとき Gateway は認証を無効化する。本番は外側 (ALB / WAF / Cloudflare 等) で認証する設計でない限り、API キーを設定する:

```bash
API_KEYS="<key1>,<key2>" bash deploy/aws/scripts/run.sh
```

| 環境変数 | デフォルト | 説明 |
|---|---|---|
| `API_KEYS` | (空 = 認証無効) | カンマ区切りの API キー |
| `ENCLAVE_DEBUG` | `0` | `1` で debug-mode 起動（PCR が 0 になる、本番禁止） |
| `ENCLAVE_MEM_MIB` | `2048` | Enclave に割り当てるメモリ（allocator.yaml と整合させる） |
| `ENCLAVE_CPU_COUNT` | `2` | Enclave に割り当てる vCPU 数 |

---

## 3. TEE 起動時の自動処理

TEE バイナリ（`title-tee`）は起動時に以下を自動実行する（実装: `crates/tee/src/main.rs`、仕様 §5.2）。

```
1. ランタイム選択 (TEE_RUNTIME=nitro|mock)
        │
        ▼
2. 暗号化用鍵束 (KeyBundle) 生成 — X25519 + P-256 + ML-KEM-768
        │
        ▼
3. Solana Extension 用 Ed25519 署名鍵生成
        │
        ▼
4. 自己 Attestation Document 取得 → measurement 抽出 → メモリ保持
   失敗時は起動中止
        │
        ▼
5. registration Attestation 取得 (user_data = SHA-256(b"title:solana-key" || solana_pubkey))
   失敗時は起動中止
        │
        ▼
6. Processor 登録 (c2pa-verify)
        │
        ▼
7. ResourcePool 初期化
        │
        ▼
8. HTTP サーバー起動 (vsock 4000、host へは socat ブリッジ経由)
```

ステップ 4 の measurement は `/extension/solana` 受付時に「相手の Attestation の measurement が自分の measurement と一致するか」を比較するために使う。ステップ 5 の attestation は `GET /solana-keys` の `registration_attestation_b64` フィールドで外部に公開され、SP1 prover の入力となる。

---

## 4. ZKP proof 生成と register-key 提出

仕様 §6.2 の四段検証を満たす proof を生成し、TEE 署名鍵を on-chain に登録する。
**TEE 起動毎に 1 回実行** が必要（仕様 §0.5 stateless 設計のため）。

### 4.1 Groth16 proof の生成

§2.9 で `deploy/aws/build/registration/` に `attestation.bin` を含む bundle を
揃えた状態で、ローカルから:

```bash
bash deploy/aws/scripts/prover-run.sh
```

これだけで c5.12xlarge prover EC2 を起動し → toolchain インストール →
proof 生成 → artifact 回収 → EC2 terminate まで自動で済む（合計 30 分 / ~$1）。
詳細・分割実行方法は
[sp1-guests/attestation-aws-nitro/README.md](../../sp1-guests/attestation-aws-nitro/README.md) 参照。

完了後 `deploy/aws/build/registration/` に追加で 3 ファイルが揃う:
- `attestation.bin.proof.bin` (260 B)
- `attestation.bin.public_values.bin` (~140 B)
- `attestation.bin.vkey_hash.hex` (67 B)

> proof 生成は **TEE の外** で動く。SP1 prover は zkVM 内で正規の検証回路を
> 実行したことを zk で証明するため、prover 実行ホストの信頼は不要
> （仕様 §6.2 「verifying_key_hash で proof 生成元を固定する」）。

### 4.2 register-key 提出

bundle ディレクトリをそのまま渡す。payer は admin である必要はない（誰でも
払える）が、admin で揃えるのが分かりやすい:

```bash
title-cli register-key \
  --payer keys/admin.json \
  --bundle deploy/aws/build/registration
```

オンチェーンで以下の四段検証が走る（仕様 §6.2、`programs/title-whitelist/src/lib.rs::register_key`）:

1. `sp1_vkey_hash` が `ApprovedVkeys` に含まれる (§2.4 で追加済み)
2. `measurement` が `ApprovedMeasurements` に含まれる (§2.9 で追加済み)
3. `user_data_hash == SHA-256(SHA-256(b"title:solana-key" || signing_pubkey))`
4. SP1 Groth16 proof の数学的検証（`sp1_solana::verify_proof`、~280K CU。
   CLI が compute-unit limit を 400K に設定する）

全部通過すると `WhitelistEntry` PDA が作成され、90 日間有効な署名鍵として
登録される。

### 4.3 登録結果の確認

```bash
SIGNING_PUBKEY=$(cat deploy/aws/build/registration/solana_pubkey.txt)
title-cli describe-whitelist --signing-pubkey "$SIGNING_PUBKEY"
```

`WhitelistEntry` セクションに `revoked: false`、`expires_at` 90 日先が出れば成功。

### 4.4 動作検証 — cNFT 発行

`POST /extension/solana` を叩いて cNFT 部分署名が返ることを確認する:

```bash
curl -X POST http://<EC2_PUBLIC_IP>:3000/extension/solana \
  -H 'Content-Type: application/json' \
  -d '{
    "offchain_data_url": "<URL to core response JSON>",
    "payer": "<base58 payer pubkey>",
    "merkle_tree": "<base58 merkle tree address>",
    "recent_blockhash": "<base58 blockhash>"
  }'
# {"partial_tx": "<base64 partially signed VersionedTransaction>"}
```

CLI から一気通貫で叩く場合（Merkle tree 未作成時は最初に `create-tree`）:

```bash
# 1) Merkle tree を作成（初回 / cNFT 容量を増やしたい時のみ）
title-cli create-tree --payer keys/admin.json

# 2) コンテンツを Gateway に POST し、レスポンス JSON をオフチェーンストレージへアップロード
title-cli process --url https://your-storage/big-video.mp4

# 3) cNFT を発行
title-cli mint \
  --offchain-data-url https://your-storage/<step2 で得た JSON URL> \
  --merkle-tree <step1 で得た tree pubkey> \
  --payer keys/admin.json
```

---

## 5. クライアント quickstart

### 5.1 平文リクエスト

```typescript
// 1. Gateway から TEE 情報を取得
const keys = await fetch(`${GATEWAY}/keys`).then(r => r.json());
const { solana_pubkey } = await fetch(`${GATEWAY}/solana-keys`).then(r => r.json());

// 2. コア処理リクエスト
const coreResp = await fetch(`${GATEWAY}/process`, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({
    input_type: "single",
    content_url: "https://your-storage/photo.jpg",
    processor_ids: ["c2pa-verify"],
  }),
}).then(r => r.json());

// coreResp = { signature_hash, results, attestation }
// オフチェーンストレージに保存し URL を得る
const offchainUrl = await uploadToYourStorage(JSON.stringify(coreResp));

// 3. Solana Extension で cNFT を発行
const extResp = await fetch(`${GATEWAY}/extension/solana`, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({
    offchain_data_url: offchainUrl,
    payer: yourWallet.publicKey.toBase58(),
    merkle_tree: yourTreeAddress.toBase58(),
    recent_blockhash: (await connection.getLatestBlockhash()).blockhash,
    collection: yourCollectionAddress?.toBase58(),
  }),
}).then(r => r.json());

// 4. partial_tx を base64 decode → 自ウォレットで最終署名 → broadcast
const partialTx = VersionedTransaction.deserialize(
  Buffer.from(extResp.partial_tx, "base64")
);
partialTx.sign([yourWallet]);
await connection.sendRawTransaction(partialTx.serialize());
```

### 5.2 暗号化リクエスト（input_type=single のみ）

仕様 §2.4 に従う。`crates/crypto/src/sealed_channel.rs` の `seal_for` / `ResponseChannel::open` を参照（80 行程度で実装可能）。

```text
1. クライアント: コンテンツから signature_hash をローカル算出
2. クライアント: GET /keys で TEE 公開鍵取得
3. クライアント: ペイロード組立 ([4B len][JSON metadata][raw content])
4. クライアント: スイート選択 → KEM 暗号化 → wire format で自社ストレージにアップロード
5. クライアント: POST /process に encryption: "x25519" を付けて送信
6-10. TEE 内処理（復号 → C2PA 検証 → signature_hash 突合 → 処理 → response 暗号化）
11. クライアント: nonce+ciphertext で返ってきたレスポンスを response_key で復号
12. クライアント: レスポンスの signature_hash がローカル値と一致するか確認
```

---

## 6. 鍵・許可リストのライフサイクル

### 6.1 TEE 再起動

TEE を再起動するたびに以下が新しくなる（仕様 §0.5 stateless 設計、§6.2）:
- 暗号化用鍵束（KeyBundle）→ Gateway が health check で自動再取得
- Solana 署名鍵 → 新規 `register_key` 提出が必要

Gateway は health check で鍵変更を検知し、`/keys` と `/solana-keys` のキャッシュを自動更新する（`crates/gateway/src/state.rs::check_and_refresh`）。

新しい署名鍵の whitelist 登録は §4 のフローを再実行（fetch-registration-bundle → proof → register_key 提出）。

ホワイトリストには鍵が増えていく一方で問題ない（仕様 §6.2 末尾）。

### 6.2 TEE バイナリ更新 + PCR0 再現性検証

1. 新 EIF をビルド → 新 PCR0 を取得（EC2 上で `bash deploy/aws/scripts/build.sh`）
2. admin が許可リストに追加:
   ```bash
   title-cli add-measurement --admin keys/admin.json --pcr0-hex 0x<new_pcr0>
   ```
3. 新 TEE を起動 → §4 のフローで新しい署名鍵を `register-key` 提出
4. 古い TEE を停止
5. （任意）`title-cli remove-measurement --admin keys/admin.json --pcr0-hex 0x<old_pcr0>` で旧バイナリを deprecate

過渡期は新旧 measurement の両方を許容できる。

**再現性検証** (`build.sh --verify`):

```bash
# === EC2 上 ===
bash deploy/aws/scripts/build.sh --verify
```

`--no-cache` で再ビルドして、`deploy/aws/build/registration/measurements.json` の
PCR0 と一致するか比較する。一致しなければ exit 1。仕様 §5.4 の要件「同じソースから
誰でも同じ PCR0 を再現できる」を満たしているかの単体検証として使う。所要 ~13 分。

### 6.3 SP1 guest 更新

検証回路の Rust コードを変更した時:

1. `cd sp1-guests/attestation-aws-nitro/host && cargo run --release --locked --bin vkey` で新 vkey_hash を取得
2. `title-cli add-vkey --admin keys/admin.json --vkey-hex 0x<new>` で許可リストに追加
3. 以降、新 vkey で生成された proof を受理
4. （任意）`title-cli remove-vkey --admin keys/admin.json --vkey-hex 0x<old>` で旧 guest を deprecate

### 6.4 緊急時の鍵取り消し (revoke)

侵害が疑われる TEE 署名鍵を緊急取り消し:

```bash
title-cli revoke-key \
  --admin keys/admin.json \
  --signing-pubkey <to_revoke_base58>
```

admin のみ実行可能。仕様 §6.2 「ホワイトリスト鍵の取り消し」通り、
`WhitelistEntry` PDA は **close せず** `revoked = true` フラグを立てる。
これは取り消し巻き戻し攻撃を防ぐためで、PDA を close すると同じ proof を
再投入して鍵を復活させられてしまう。

取り消し前に発行された cNFT はチェーン上に残るが、アプリ側で「現在
`revoked == false` か」を判定する設計なら以降は信頼されなくなる。
状態確認は `title-cli describe-whitelist --signing-pubkey <pk>`。

---

## 7. 開発者ローカル環境

実 TEE がなくても動作確認できる mock モード:

```bash
docker compose up --build
```

| 起動内容 | デフォルトポート |
|---|---|
| TEE (mock runtime) | 4000 |
| Gateway | 3000 |

起動確認:

```bash
curl -sf http://localhost:3000/health
# {"status":"ok","tee_type":"mock"}
```

mock runtime は `MockAttestationVerifier` とペアで動き、`"mock-attestation:"` プレフィックス付きのバイト列を Attestation として扱う。実 PCR や AWS 証明書チェーン検証は走らないので、本番経路の構造確認用。`add_approved_measurement` には mock の measurement (`[0u8; 48]`) を登録すれば devnet で疎通確認できるが、これは攻撃面が自明なので **devnet 限定**。

---

## 8. トラブルシューティング

### `anchor build` が `source_file` エラーで失敗する

```
error[E0599]: no method named `source_file` found for struct `proc_macro2::Span`
```

anchor 0.30.1 と最新 proc-macro2 の非互換。`anchor build --no-idl` で IDL ビルドをスキップする。

### `register_key` が `VkeyNotApproved (6007)` で reject される

`ApprovedVkeys` PDA に該当の `sp1_vkey_hash` が登録されていない（仕様 §6.2 確認 1）。`add_approved_vkey` で追加する（admin only）。

### `register_key` が `MeasurementNotApproved (6010)` で reject される

`ApprovedMeasurements` PDA に該当 measurement が登録されていない（仕様 §6.2 確認 2）。`add_approved_measurement` で追加する。

### `register_key` が `UserDataMismatch (6009)` で reject される

`user_data_hash != SHA-256(b"title:solana-key" || signing_pubkey)`（仕様 §6.2 確認 3）。typical な原因:
- proof 生成に使った attestation.bin と register_key に渡している signing_pubkey が別の TEE 起動由来
- → §4 を最初からやり直し（fetch-registration-bundle で取った 3 つのファイル全部を 1 セットで使う）

### TEE 起動時に `Self-attestation failed` で停止する

自己 Attestation の取得に失敗している。仕様 §5.2 の通り、measurement を保持できない状態でリクエスト受付を始めると信頼モデルが崩壊するため fail-fast している:
- `TEE_RUNTIME=nitro` でローカル実行している（Nitro Enclave 外では NSM デバイスがない）→ ローカルでは `TEE_RUNTIME=mock` を使う
- 実 Nitro 上で発生 → `/dev/nsm` が利用可能か確認 (`nitro-cli describe-enclaves`)

### TEE 起動時に `Failed to obtain registration attestation` で停止する

ステップ 5 で NSM が `user_data = SHA-256(b"title:solana-key" || pubkey)` を含む attestation を発行できなかった。NSM の容量制限（user_data ≤ 1024 bytes）を超えていることはまずないので、`/dev/nsm` のエラーログを確認する。

### Gateway が `TEE unavailable` を返し続ける

`TEE_ENDPOINT` が誤っているか、TEE 側で health check が失敗している:
- `curl ${TEE_ENDPOINT}/health` を直接叩いて応答確認
- `bash deploy/aws/scripts/status.sh` で stack 全体の稼働状態を確認

### SP1 proof 生成が OOM で死ぬ

Groth16 wrapping のピークメモリは ~95 GiB。swap なしの環境では OOM kill される。EC2 `c5.12xlarge` (96 GiB) + 16 GiB swap を推奨。詳細は [sp1-guests/attestation-aws-nitro/README.md](../../sp1-guests/attestation-aws-nitro/README.md) の Step 2 を参照。

### `fetch-registration-bundle.sh` が `Enclave is running in DEBUG_MODE` で fail する

`ENCLAVE_DEBUG=1` で起動した Enclave は NSM が PCR をすべて 0 で返すため、その attestation を on-chain 登録に使うと信頼モデルが空洞化する（誰でも自前 AWS アカウントで debug Enclave を立てて承認を取れる）。`bash deploy/aws/scripts/stop.sh` で停止し、`bash deploy/aws/scripts/run.sh`（環境変数なし）で release-mode で起動し直す。

### `nitro-cli run-enclave` が `Insufficient memory` で fail する

allocator (`/etc/nitro_enclaves/allocator.yaml`) の `memory_mib` が `ENCLAVE_MEM_MIB` より小さい。`run.sh` のデフォルトは 2048 MiB で、allocator も同値に合わせる:

```bash
sudo sed -i 's/^memory_mib:.*$/memory_mib: 2048/' /etc/nitro_enclaves/allocator.yaml
sudo systemctl restart nitro-enclaves-allocator
```
