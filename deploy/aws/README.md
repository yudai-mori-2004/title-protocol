# Title Protocol — AWS Nitro Enclaves deployment

Run a Title Protocol TEE node on AWS Nitro Enclaves. All operations run
ON the EC2 instance after `git clone` — no local cross-build, no terraform
dependency for runtime scripts.

---

## Architecture

```
  EC2 host (c5.xlarge, Amazon Linux 2023)
  ┌──────────────────────────────────────────────────────────────────┐
  │  Docker container: title-gateway        (host net, :3000)        │
  │           │                                                       │
  │           ▼  HTTP                                                 │
  │  socat bridge   127.0.0.1:4000  ───────────►  vsock:CID:4000     │
  │                                                       ▲          │
  │  Docker container: title-proxy   (host net, vsock:8000)          │
  │           ▲                                                       │
  │           │  length-prefixed proto                                │
  │           │                                                       │
  │  ┌────────┴───────────────────────────────────────────────────┐  │
  │  │  Nitro Enclave (isolated VM, no network interface)         │  │
  │  │    title-tee  (HTTP :4000 over vsock)                      │  │
  │  │       │                                                     │  │
  │  │       └─ ContentFetcher ─► vsock:3:8000 (title-proxy)      │  │
  │  └─────────────────────────────────────────────────────────────┘  │
  └──────────────────────────────────────────────────────────────────┘
```

| Component | Process model | Reachable from |
|---|---|---|
| Gateway | Docker container, `--network host` | Public internet, port 3000 |
| socat bridge | Plain process | localhost only |
| title-proxy | Docker container, `--network host` | Enclave only (vsock:8000) |
| title-tee | Nitro Enclave | vsock only |

The Enclave has **no network interface**. Outbound HTTPS goes through
the proxy; inbound requests come from the socat bridge.

---

## Prerequisites

- EC2 instance with Nitro Enclave support (c5.xlarge or larger)
- Amazon Linux 2023 AMI
- Security group: SSH (22) + Gateway port (3000)
- SSH access to the instance

---

## End-to-end setup

### 1. Provision and prepare the host

SSH into a fresh Amazon Linux 2023 EC2 instance:

```bash
sudo bash deploy/aws/scripts/setup-host.sh
```

Log out and back in (for docker/ne group membership), then clone:

```bash
git clone <repo-url>
cd title-protocol
```

### 2. Build images + EIF

```bash
bash deploy/aws/scripts/build.sh
```

First build takes ~25 min on c5.xlarge. Incremental rebuilds use Docker
layer cache (1-3 min). PCR0 / PCR1 / PCR2 are printed at the end —
record PCR0 for on-chain registration.

### 3. Start the stack

```bash
bash deploy/aws/scripts/run.sh
```

Starts: title-proxy → Nitro Enclave → socat bridge → title-gateway.
Idempotent — stops any running stack first.

Optional Bearer token authentication:
```bash
API_KEYS="secret-1,secret-2" bash deploy/aws/scripts/run.sh
```

### 4. Verify

```bash
curl http://localhost:3000/health
curl http://localhost:3000/keys | jq
curl http://localhost:3000/solana-keys | jq
```

---

## On-chain key registration

After the stack is running in release mode (not debug):

```bash
# === on EC2 ===
bash deploy/aws/scripts/fetch-registration-bundle.sh
```

This captures PCR0, solana_pubkey, and the registration attestation into
`deploy/aws/build/registration/`. Then on your local workstation (`title-cli` +
admin keypair):

```bash
# === local ===
# Copy the bundle down
mkdir -p deploy/aws/build/registration
scp -i deploy/aws/keys/title-protocol-devnet.pem \
    'ec2-user@<EC2_IP>:~/title-protocol/deploy/aws/build/registration/*' \
    deploy/aws/build/registration/

# Register the PCR0 (once per TEE binary version)
PCR0=$(jq -r '.PCR0' deploy/aws/build/registration/measurements.json)
title-cli add-measurement --admin keys/admin.json --pcr0-hex 0x$PCR0

# Generate ZKP proof (~30 min, ~$1 on a c5.12xlarge prover EC2)
bash deploy/aws/scripts/prover-run.sh

# Submit register_key on devnet
title-cli register-key \
  --payer keys/admin.json \
  --bundle deploy/aws/build/registration

# Confirm the WhitelistEntry PDA is valid for 90 days
SIGNING_PUBKEY=$(cat deploy/aws/build/registration/solana_pubkey.txt)
title-cli describe-whitelist --signing-pubkey "$SIGNING_PUBKEY"
```

Full operational reference: [docs/v0.1.2/OPERATIONS_JA.md](../../docs/v0.1.2/OPERATIONS_JA.md).

---

## Operations

```bash
# Status check
bash deploy/aws/scripts/status.sh

# Stop stack (leaves EC2 running)
bash deploy/aws/scripts/stop.sh

# Restart after code changes
bash deploy/aws/scripts/build.sh
bash deploy/aws/scripts/run.sh

# Debug mode (zeroed PCRs, enables nitro-cli console)
ENCLAVE_DEBUG=1 bash deploy/aws/scripts/run.sh
sudo nitro-cli console --enclave-id $(sudo nitro-cli describe-enclaves | jq -r '.[0].EnclaveID')
```

If TEE source changed, PCR0 changes. Register the new PCR0 with
`add_approved_measurement` and run a fresh `register_key`.

---

## Configuration

| Variable | Default | Description |
|---|---|---|
| `ENCLAVE_MEM_MIB` | 4096 | Enclave memory allocation |
| `ENCLAVE_CPU_COUNT` | 2 | Enclave vCPU count |
| `ENCLAVE_DEBUG` | 0 | Set to 1 for debug mode |
| `API_KEYS` | (none) | Comma-separated Bearer tokens |
| `EIF_NAME` | title-protocol-tee.eif | EIF filename |

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| `nitro-cli build-enclave` fails "Insufficient memory" | hugepage allocation race after reboot | `sudo systemctl restart nitro-enclaves-allocator` then retry |
| `curl /health` hangs | socat bridge or Enclave still booting | check console: `sudo nitro-cli console --enclave-id <id>` |
| TEE exits with "Self-attestation failed" | NSM device or runtime mismatch | confirm `TEE_RUNTIME=nitro` in EIF, `/dev/nsm` accessible |
| `/extension/solana` returns 400 "measurement mismatch" | PCR0 changed after rebuild | register new PCR0 on-chain |
| Gateway returns 502 "TEE unavailable" | TEE crashed | `bash deploy/aws/scripts/run.sh` |
| Proxy logs "DNS lookup failed" | egress blocked | check AWS security group + VPC route table |
