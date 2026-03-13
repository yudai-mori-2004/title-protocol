# Troubleshooting

全環境共通 → ローカル固有 → AWS 固有の順で構成。

---

## Common Issues

### Port already in use

```
Error: Address already in use (os error 48)
```

A previous session's process is still running. Stop everything and retry:

```bash
# ローカル
./deploy/local/teardown.sh
./deploy/local/setup.sh

# AWS
docker compose -f deploy/aws/docker-compose.production.yml down
sudo nitro-cli terminate-enclave --all
pkill title-proxy || true
./deploy/aws/setup-ec2.sh
```

If a process still clings to a port, kill it directly:

```bash
lsof -ti :3000 | xargs kill   # replace 3000 with the blocked port
```

### SOL balance insufficient

`setup.sh` fails at node registration or Merkle Tree creation — both steps require ~0.6 SOL in your operator wallet.

```bash
# Check balance
solana balance $(solana-keygen pubkey keys/operator.json) --url devnet

# Request more (devnet)
solana airdrop 2 $(solana-keygen pubkey keys/operator.json) --url devnet
```

Then re-run `setup.sh` — it skips already-running services and retries the failed steps.

> **EC2 からのエアドロップはレート制限されやすい。** ローカルから送金する方が確実:
> ```bash
> solana transfer <EC2_WALLET_PUBKEY> 2 --url devnet
> ```

### AES-GCM decryption failure on `/verify`

```
ペイロードの復号に失敗: AES-GCM復号に失敗しました
```

The SDK encrypted the payload with a **stale TEE node's key**. TEE nodes regenerate keys on every restart, but old node entries remain on-chain. The SDK (`selectNode()`) deduplicates by gateway endpoint and uses the most recently registered entry.

**Fix:** Restart the node to force re-registration:

```bash
# ローカル
./deploy/local/teardown.sh
./deploy/local/setup.sh

# AWS
./deploy/aws/setup-ec2.sh
```

### Docker / PostgreSQL won't start

Make sure Docker Desktop (or the Docker daemon) is running:

```bash
docker info
```

Port 5432 may conflict with a local PostgreSQL installation. Stop it or change the port in `deploy/local/docker-compose.yml`.

### `network.json` not found

```
ERROR: network.json が見つかりません。
```

Phase 1 が未完了。先に `title-cli init-global` を実行する:

```bash
cargo build --release -p title-cli
./target/release/title-cli init-global --cluster devnet
```

See [`programs/title-config/README.md`](../programs/title-config/README.md) for the full Phase 1 guide.

### `CORE_COLLECTION_MINT` / `EXT_COLLECTION_MINT` not set

TEE が cNFT をミントできない場合、コレクションアドレスの設定漏れが原因であることが多い。

**自動設定の仕組み:**

`setup.sh` / `setup-ec2.sh` は `network.json` から `core_collection_mint` / `ext_collection_mint` を読み取り、環境変数としてTEEプロセスに渡す（`setup.sh:120-133`, `setup-ec2.sh:92-105`）。`.env` で明示設定した場合はそちらが優先される。

**確認方法:**

```bash
# network.json の値を確認
python3 -c "import json; d=json.load(open('network.json')); print('Core:', d['core_collection_mint']); print('Ext:', d['ext_collection_mint'])"

# TEE プロセスの環境変数を確認（ローカル）
ps aux | grep title-tee
cat /proc/<PID>/environ | tr '\0' '\n' | grep COLLECTION_MINT

# Docker コンテナの環境変数を確認（AWS）
docker inspect $(docker ps -q --filter name=gateway) | python3 -c "
import sys, json
env = json.load(sys.stdin)[0]['Config']['Env']
for e in env:
    if 'COLLECTION' in e: print(e)
"
```

**手動で設定する場合:**

```bash
# .env に追加
CORE_COLLECTION_MINT=<address from network.json>
EXT_COLLECTION_MINT=<address from network.json>
```

---

## Local-Specific Issues

### `setup.sh` fails midway

`setup.sh` は冪等（何度実行しても安全）。既に稼働中のサービスはスキップされる。途中で失敗した場合はそのまま再実行:

```bash
./deploy/local/setup.sh
```

完全にリセットしたい場合:

```bash
./deploy/local/teardown.sh
./deploy/local/setup.sh
```

### Services not responding after `setup.sh` completes

ログを確認:

```bash
tail -20 /tmp/title-tee.log
tail -20 /tmp/title-gateway.log
tail -20 /tmp/title-temp-storage.log
tail -20 /tmp/title-indexer.log
```

---

## AWS-Specific Issues

### `docker: permission denied`

EC2 に SSH した直後は docker グループが反映されていないことがある:

```bash
# 方法1: 再接続
exit
ssh -i deploy/aws/keys/title-protocol-devnet.pem ec2-user@NODE_IP

# 方法2: sg コマンド
sg docker bash
```

> `setup-ec2.sh` は自動で `sg docker` による再実行を試みる。

### C compiler not found during `cargo build`

```bash
sudo dnf install -y gcc gcc-c++
```

### Enclave startup failure

`enclave_memory_mib` がインスタンスの利用可能メモリを超えている可能性がある。`setup-ec2.sh` 内の `ENCLAVE_MEMORY_MIB` を調整:

```bash
ENCLAVE_MEMORY_MIB=512 ./deploy/aws/setup-ec2.sh
```

### S3 presigned URL returns 403

Terraform output で S3 認証情報を再確認:

```bash
cd deploy/aws/terraform
terraform output -raw s3_access_key_id
terraform output -raw s3_secret_access_key
terraform output -raw s3_bucket_name
```

`.env` の `S3_ACCESS_KEY` / `S3_SECRET_KEY` / `S3_BUCKET` と一致していることを確認。

### Proxy log permission error

`title-proxy` が `/var/log/` への書き込み権限がなくクラッシュする場合:

```bash
# ログファイルをホームディレクトリに変更
nohup ./target/release/title-proxy > ~/title-proxy.log 2>&1 &
```

> この問題は `setup-ec2.sh` では修正済み（`~/title-proxy.log` を使用）。

### `solana: command not found`

SSH セッションで PATH が未設定:

```bash
source ~/.bashrc
# または新しい SSH セッションを開く
```
