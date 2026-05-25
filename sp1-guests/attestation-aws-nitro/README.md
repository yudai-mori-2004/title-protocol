# attestation-aws-nitro — ZKP Proof 生成ガイド

AWS Nitro Attestation Document に対する Groth16 proof を生成する。
TEE が起動時に確保した Solana 署名鍵を、on-chain `register_key` で
ホワイトリスト登録するために必要な proof bundle を出力する。

> **独立性**: ZKP 生成はコアフロー（TEE / Gateway / Solana コントラクト）
> から完全に独立したパイプラインであり、attestation.bin を入力に proof を
> 出力するだけの単機能。prover の実行ホストに信頼は不要（仕様 §6.2）。

## 前提

| 項目 | 値 |
|---|---|
| SP1 toolchain | v6.2.0 (`sp1up --version v6.2.0`) |
| Rust toolchain | `rust-toolchain.toml` で自動選択 |
| attestation.bin | TEE ノードから `fetch-registration-bundle.sh` で取得済み |

## EC2 での proof 生成（推奨）

ローカル Mac (16 GiB) でも動作するが swap 依存で 90 分かかる。
EC2 `c5.12xlarge` (48 vCPU / 96 GiB) なら 18 分で完了する。

### Step 0: EC2 インスタンス起動

TEE ノードとは別に proof 専用インスタンスを立てる。
spot でもよい（途中で落ちても attestation.bin がある限りリトライできる）。

```bash
# c5.12xlarge (48 vCPU / 96 GiB, ~$2/hr Tokyo)
# セキュリティグループは SSH (22) のみ開放
aws ec2 run-instances \
  --image-id resolve:ssm:/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-x86_64 \
  --instance-type c5.12xlarge \
  --key-name title-protocol-devnet \
  --security-group-ids <your-sg-id> \
  --subnet-id <your-subnet-id> \
  --tag-specifications 'ResourceType=instance,Tags=[{Key=Name,Value=title-protocol-prover}]' \
  --block-device-mappings '[{"DeviceName":"/dev/xvda","Ebs":{"VolumeSize":50}}]'
```

### Step 1: 環境構築

SSH して toolchain をインストールする。

```bash
ssh -i deploy/aws/keys/title-protocol-devnet.pem ec2-user@<PROVER_IP>
```

```bash
# ビルドツール
sudo dnf -y install gcc gcc-c++ openssl-devel git pkgconfig tar tmux protobuf-devel

# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# SP1 toolchain
curl -L https://sp1.succinct.xyz | bash
~/.sp1/bin/sp1up --version v6.2.0
```

### Step 2: swap 有効化

Groth16 wrapping のピークメモリは ~95 GiB。96 GiB インスタンスでは
物理 RAM だけでは OOM kill されるため、16 GiB の swap を追加する。

```bash
sudo dd if=/dev/zero of=/swapfile bs=1G count=16
sudo chmod 600 /swapfile
sudo mkswap /swapfile
sudo swapon /swapfile
free -g   # Swap: 15 0 15 を確認
```

### Step 3: ソースと attestation を配置

```bash
git clone <repo-url> ~/title-protocol
cd ~/title-protocol
```

attestation.bin を TEE ノードから取得する。TEE ノード EC2 上で
`fetch-registration-bundle.sh` を実行済みであれば、TEE ノード側から
prover へ push する:

```bash
# TEE ノード EC2 上で実行 — prover EC2 へ転送
scp -i ~/.ssh/title-protocol-devnet.pem \
  ~/title-protocol/deploy/aws/build/registration/attestation.bin \
  ec2-user@<PROVER_IP>:~/title-protocol/sp1-guests/attestation-aws-nitro/host/
```

### Step 4: ビルド

```bash
cd ~/title-protocol/sp1-guests/attestation-aws-nitro/host
cargo build --release --bin prove --bin vkey --locked
```

初回ビルドは SP1 guest の RISC-V クロスコンパイルを含むため 10-15 分かかる。

### Step 5: vkey_hash 確認

```bash
./target/release/vkey
# 0x00d742a0...  ← on-chain の ApprovedVkeys に登録済みの値と一致すること
```

### Step 6: proof 生成

tmux で実行する（SSH 切断に耐えるため）。

```bash
tmux new-session -s prove

RUST_LOG=info ./target/release/prove ./attestation.bin
```

進捗ログ:

```
Loaded 4542 bytes from ./attestation.bin
INFO initializing cpu prover
INFO starting proof generation mode=Groth16
WARN Memory usage is high: 98.68%    ← Groth16 wrap phase（数分で通過）
INFO prove shrink: close time.busy=2.10s
INFO prove wrap: close time.busy=22.1s
INFO Running prove in docker          ← Groth16 circuit (5-7 分)
INFO Running verify in docker
Proof generated in 1058.9s
Wrote:
  ./attestation.bin.proof.bin
  ./attestation.bin.public_values.bin
  ./attestation.bin.vkey_hash.hex
```

c5.12xlarge で約 18 分。`Memory usage is high` の WARN は正常動作。

### Step 7: artifact を registration ディレクトリへ移動

```bash
cp ~/title-protocol/sp1-guests/attestation-aws-nitro/host/attestation.bin.* \
   ~/title-protocol/deploy/aws/build/registration/
```

取得される 3 ファイル:

| ファイル | 内容 | サイズ目安 |
|---|---|---|
| `attestation.bin.proof.bin` | Groth16 proof (4-byte VK selector + proof) | ~360 B |
| `attestation.bin.public_values.bin` | ZKP が commit した公開値 | ~140 B |
| `attestation.bin.vkey_hash.hex` | verifying_key_hash (sanity check) | 67 B |

### Step 8: インスタンス破棄

```bash
aws ec2 terminate-instances --instance-ids <PROVER_INSTANCE_ID>
```

proof 生成は冪等。同じ attestation.bin に対して何度実行しても同じ proof が得られる。

## proof の検証

proof が正しく attestation の中身を証明しているか確認する。
`public_values.bin` を以下の順にパースする（guest program の commit 順序）:

```
instance_id_len   : u32 LE
instance_id       : instance_id_len bytes (UTF-8, AWS の module_id)
timestamp_ms      : u64 LE
measurement_len   : u32 LE
measurement       : measurement_len bytes (PCR0, 48 bytes)
has_user_data     : u8 (0 or 1)
user_data_hash    : 32 bytes (SHA256(doc.user_data), has_user_data==1 のとき)
has_public_key    : u8 (0 or 1)
public_key_hash   : 32 bytes (SHA256(doc.public_key), has_public_key==1 のとき)
```

on-chain `register_key` の四段検証 (仕様 §6.2):

1. `vkey_hash.hex` の値が `ApprovedVkeys` に含まれる
2. `measurement` (PCR0) が `ApprovedMeasurements` に含まれる
3. `user_data_hash == SHA256(SHA256(b"title:solana-key" || signing_pubkey))`
4. Groth16 proof の数学的検証 (`sp1_solana::verify_proof`)

## Reproducible Build (vkey の再現性)

`build.rs` が `--remap-path-prefix` を使って DWARF debug info からホスト固有の
パス文字列を除去する。これにより、ソース・`Cargo.lock`・SP1 toolchain バージョンが
同一であれば、どのホストでビルドしても同じ ELF → 同じ vkey_hash が得られる。

```
--remap-path-prefix=<repo_root>=/repo
--remap-path-prefix=<home>/.cargo=/cargo
--remap-path-prefix=<home>/.rustup=/rustup
```

vkey_hash が on-chain 登録値と一致しない場合、以下を確認:
- `Cargo.lock` が最新か (`git pull`)
- SP1 toolchain バージョンが一致するか (`sp1up --version v6.2.0`)
- `cargo build --locked` を使っているか

## ディレクトリ構成

```
attestation-aws-nitro/
├── README.md        ← 本ファイル
├── program/         SP1 guest: Attestation Document の証明書チェーン検証を
│                    zkVM 内で実行し、公開値を commit する
└── host/            Host harness:
     ├── src/
     │   ├── lib.rs      CpuProver setup + Groth16 proof 生成
     │   └── bin/
     │       ├── prove.rs   CLI: attestation.bin → proof + public_values
     │       └── vkey.rs    CLI: vkey_hash 表示
     └── build.rs        Reproducible build (--remap-path-prefix)
```
