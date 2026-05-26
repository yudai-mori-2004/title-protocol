# attestation-aws-nitro — ZKP Proof 生成ガイド

AWS Nitro Attestation Document に対する Groth16 proof を生成する。
TEE が起動時に確保した Solana 署名鍵を、on-chain `register_key`
(`title-cli register-key`) でホワイトリスト登録するために必要な
proof bundle を出力する。

> **独立性**: ZKP 生成はコアフロー（TEE / Gateway / Solana コントラクト）
> から完全に独立したパイプラインで、`attestation.bin` を入力に proof を
> 出力するだけの単機能。prover 実行ホストに信頼は不要（仕様 §6.2）。

## ワンライナー（推奨）

`fetch-registration-bundle.sh` で `attestation.bin` を取得した直後に、
ローカル（AWS CLI が使えるマシン）から:

```bash
bash deploy/aws/scripts/prover-run.sh
```

これだけで以下が自動実行される（合計 30 分 / ~$1）:

1. c5.12xlarge prover EC2 を起動
2. toolchain (Rust + SP1 v6.2.0) + 16 GiB swap をインストール
3. `deploy/aws/build/registration/attestation.bin` を prover に転送
4. リポジトリを git clone し、`prove` バイナリをビルド
5. Groth16 proof を生成（~18 分）
6. proof artifact 3 ファイルをローカルの `deploy/aws/build/registration/` へ取得
7. prover EC2 を terminate

オプション:
- `bash deploy/aws/scripts/prover-run.sh <path/to/attestation.bin>` で入力を指定
- `bash deploy/aws/scripts/prover-run.sh --keep-alive` で終了後も prover を残す（デバッグ・リトライ用）

完了すると `deploy/aws/build/registration/` に以下が揃う:

| ファイル | 内容 | サイズ |
|---|---|---|
| `attestation.bin` | TEE ノードから取得した Attestation Document | ~4.5 KB |
| `attestation.bin.proof.bin` | Groth16 proof (4-byte VK selector + 256-byte proof) | 260 B |
| `attestation.bin.public_values.bin` | ZKP が commit した公開値 (instance_id, PCR0, user_data_hash, …) | ~140 B |
| `attestation.bin.vkey_hash.hex` | verifying_key_hash (`ApprovedVkeys` との照合用) | 67 B |
| `measurements.json` | PCR0/PCR1/PCR2 (`nitro-cli describe-enclaves` の出力) | ~370 B |
| `solana_pubkey.txt` | TEE が生成した Ed25519 公開鍵 (Base58) | ~45 B |

このディレクトリ全体をそのまま `title-cli register-key --bundle` に渡せる。

## 前提

| 項目 | 値 |
|---|---|
| ローカル (`prover-run.sh` を実行する側) | AWS CLI v2、bash、scp、`deploy/aws/keys/title-protocol-devnet.pem` |
| SSH エージェント | `ssh-add ~/.ssh/id_ed25519` — github.com の deploy-key として登録済み |
| `attestation.bin` | TEE ノードの `fetch-registration-bundle.sh` で取得済み |

ローカル SSH エージェントに github 鍵が読み込まれていないと、prover 上での
`git clone` でこける。`ssh-add -l` で `id_ed25519` が出ることを確認する。

## ステップごとに実行したい場合

`prover-run.sh` は以下 4 つのスクリプトを順に実行しているだけ。デバッグや
個別のリトライをしたいときは直接呼ぶ:

```bash
# 1. EC2 を起動（標準出力に "INSTANCE_ID\nPUBLIC_IP" を吐く）
bash deploy/aws/scripts/prover-launch.sh

# 2. prover に setup スクリプトと attestation を転送
scp -i deploy/aws/keys/title-protocol-devnet.pem \
    deploy/aws/scripts/prover-setup.sh \
    deploy/aws/scripts/prover-prove.sh \
    deploy/aws/build/registration/attestation.bin \
    ec2-user@<PUBLIC_IP>:/tmp/

# 3. prover 上で toolchain + swap を入れる
ssh -i deploy/aws/keys/title-protocol-devnet.pem ec2-user@<PUBLIC_IP> \
    'bash /tmp/prover-setup.sh'

# 4. prover 上で git clone + build + prove (SSH agent 転送が必要)
ssh -A -i deploy/aws/keys/title-protocol-devnet.pem ec2-user@<PUBLIC_IP> \
    'bash /tmp/prover-prove.sh /tmp/attestation.bin'

# 5. artifact を取得
scp -i deploy/aws/keys/title-protocol-devnet.pem \
    'ec2-user@<PUBLIC_IP>:~/title-protocol/sp1-guests/attestation-aws-nitro/host/attestation.bin.*' \
    deploy/aws/build/registration/

# 6. 完了したら terminate
aws ec2 terminate-instances --instance-ids <INSTANCE_ID>
```

## proof の中身

`public_values.bin` のレイアウト（SP1 guest が commit する順）:

```
instance_id_len   : u32 LE
instance_id       : instance_id_len bytes (UTF-8, AWS の module_id)
timestamp_ms      : u64 LE
measurement_len   : u32 LE
measurement       : measurement_len bytes (PCR0 = 48 bytes)
has_user_data     : u8 (0 or 1)
user_data_hash    : 32 bytes (SHA-256(doc.user_data); has_user_data==1 のとき)
has_public_key    : u8 (0 or 1)
public_key_hash   : 32 bytes (SHA-256(doc.public_key); has_public_key==1 のとき)
```

on-chain `register_key` の四段検証 (仕様 §6.2):

1. `vkey_hash` が `ApprovedVkeys` に含まれる
2. `measurement` (PCR0) が `ApprovedMeasurements` に含まれる
3. `user_data_hash == SHA-256(SHA-256(b"title:solana-key" || signing_pubkey))`
4. Groth16 proof の数学的検証 (`sp1_solana::verify_proof`、~280K CU)

すべて通過すると `WhitelistEntry` PDA が作成され、90 日間有効な署名鍵として
登録される。CLI からは `title-cli register-key --bundle <dir>` 一発。

## 再現性

proof 生成は **冪等**: 同じ `attestation.bin` から何度実行しても同じ proof と
同じ `public_values` が出る。SP1 guest プログラム本体も
`build.rs` の `--remap-path-prefix` でホスト独立にビルドされるため、ソース・
`Cargo.lock`・SP1 toolchain バージョン (`v6.2.0`) が一致するどの環境でも
同じ `vkey_hash` (`0x00d742a0c7af54b880c0bc27eaff7f8f481cd75d9cd7b2516fea02e9ded29754`) が得られる。

vkey_hash が想定値と一致しないときに確認すること:
- `git pull` で `Cargo.lock` が最新か
- SP1 toolchain が `v6.2.0` か (`~/.sp1/bin/sp1up --version v6.2.0`)
- `cargo build --locked` を使っているか (`prover-prove.sh` はそうしている)

## メモリ・所要時間

Groth16 wrapping のピーク RSS は **~95 GiB**。

| インスタンス | 物理 RAM | swap | 所要時間 | コスト目安 |
|---|---|---|---|---|
| `c5.12xlarge` (推奨) | 96 GiB | 16 GiB | ~18 分 | $2/hr |
| ローカル Mac (16 GiB) | 16 GiB | SSD 圧縮 swap | ~90 分 | 0 |

c5.12xlarge は物理 RAM ぎりぎりなので、16 GiB swap が無いと OOM kill される
（`prover-setup.sh` が自動で `/swapfile` を確保する）。

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
