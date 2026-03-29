# Troubleshooting

Organized as: Common → Local-specific → AWS-specific.

---

## Common Issues

### Port already in use

```
Error: Address already in use (os error 48)
```

A previous session's process is still running. Stop everything and retry:

```bash
# Local
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

> **Airdrops from EC2 are often rate-limited.** Sending SOL from a local machine is more reliable:
> ```bash
> solana transfer <EC2_WALLET_PUBKEY> 2 --url devnet
> ```

### AES-GCM decryption failure on `/verify`

```
Payload decryption failed: AES-GCM decryption failed
```

The SDK encrypted the payload with a **stale TEE node's key**. TEE nodes regenerate keys on every restart, but old node entries remain on-chain. The SDK (`selectNode()`) deduplicates by gateway endpoint and uses the most recently registered entry.

**Fix:** Restart the node to force re-registration:

```bash
# Local
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
ERROR: network.json not found.
```

Phase 1 is not complete. Run `title-cli init-global` first:

```bash
cargo build --release -p title-cli
./target/release/title-cli init-global --cluster devnet
```

See [`programs/title-config/README.md`](../programs/title-config/README.md) for the full Phase 1 guide.

### `CORE_COLLECTION_MINT` / `EXT_COLLECTION_MINT` not set

If the TEE cannot mint cNFTs, missing collection addresses are the most common cause.

**How auto-configuration works:**

`setup.sh` / `setup-ec2.sh` reads `core_collection_mint` / `ext_collection_mint` from `network.json` and passes them as environment variables to the TEE process. If explicitly set in `.env`, those values take precedence.

**How to verify:**

```bash
# Check network.json values
python3 -c "import json; d=json.load(open('network.json')); print('Core:', d['core_collection_mint']); print('Ext:', d['ext_collection_mint'])"

# Check TEE process environment (local)
ps aux | grep title-tee
cat /proc/<PID>/environ | tr '\0' '\n' | grep COLLECTION_MINT

# Check Docker container environment (AWS)
docker inspect $(docker ps -q --filter name=gateway) | python3 -c "
import sys, json
env = json.load(sys.stdin)[0]['Config']['Env']
for e in env:
    if 'COLLECTION' in e: print(e)
"
```

**To set manually:**

```bash
# Add to .env
CORE_COLLECTION_MINT=<address from network.json>
EXT_COLLECTION_MINT=<address from network.json>
```

---

## Local-Specific Issues

### `setup.sh` fails midway

`setup.sh` is idempotent — safe to run multiple times. Already-running services are skipped. If it fails partway through, just re-run:

```bash
./deploy/local/setup.sh
```

For a full reset:

```bash
./deploy/local/teardown.sh
./deploy/local/setup.sh
```

### Services not responding after `setup.sh` completes

Check the logs:

```bash
tail -20 /tmp/title-tee.log
tail -20 /tmp/title-gateway.log
tail -20 /tmp/title-temp-storage.log
tail -20 /tmp/title-indexer.log
```

---

## AWS-Specific Issues

### `docker: permission denied`

The docker group may not be applied immediately after SSH:

```bash
# Option 1: reconnect
exit
ssh -i deploy/aws/keys/title-protocol-devnet.pem ec2-user@NODE_IP

# Option 2: sg command
sg docker bash
```

> `setup-ec2.sh` automatically retries with `sg docker`.

### C compiler not found during `cargo build`

```bash
sudo dnf install -y gcc gcc-c++
```

### Enclave startup failure

`enclave_memory_mib` may exceed available instance memory. Adjust `ENCLAVE_MEMORY_MIB` in `setup-ec2.sh`:

```bash
ENCLAVE_MEMORY_MIB=512 ./deploy/aws/setup-ec2.sh
```

### S3 presigned URL returns 403

Re-check S3 credentials from Terraform output:

```bash
cd deploy/aws/terraform
terraform output -raw s3_access_key_id
terraform output -raw s3_secret_access_key
terraform output -raw s3_bucket_name
```

Verify they match `S3_ACCESS_KEY` / `S3_SECRET_KEY` / `S3_BUCKET` in `.env`.

### Proxy log permission error

If `title-proxy` crashes because it cannot write to `/var/log/`:

```bash
# Redirect logs to home directory
nohup ./target/release/title-proxy > ~/title-proxy.log 2>&1 &
```

> This is already fixed in `setup-ec2.sh` (uses `~/title-proxy.log`).

### `solana: command not found`

PATH is not set in the SSH session:

```bash
source ~/.bashrc
# or open a new SSH session
```
