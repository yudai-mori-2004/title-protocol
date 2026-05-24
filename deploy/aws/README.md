# Title Protocol — AWS Nitro Enclaves deployment

End-to-end recipe for provisioning a Title Protocol TEE node on AWS from a
clean clone. Every step assumes you're in the repository root unless noted.

> **Status**: experimental — first cloud deployment on the v0.1.2 rewrite.
> Treat as a working harness for measurement / proof generation, not a
> hardened production blueprint.

---

## What this provisions

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
| title-tee | Nitro Enclave | vsock only — `socat` for inbound, `title-proxy` for outbound |

The Enclave has **no network interface**. Every outbound HTTPS call goes
through the proxy; every inbound request comes from the socat bridge.

---

## Prerequisites (local machine)

| Tool | Notes |
|---|---|
| `aws` CLI | `aws configure` against an account that can create EC2 + IAM-free networking |
| Terraform 1.5+ | `brew install terraform` etc. |
| Docker with buildx | Used to cross-build linux/amd64 images on Mac |
| `jq` | For parsing JSON in scripts |
| Solana CLI (optional) | Only needed for the on-chain `register_key` follow-up |

You will **not** need: Solana CLI on EC2, Rust on EC2, the AWS SDK locally —
all of that is handled by the scripts.

---

## Cost note

Single `c5.xlarge` ($0.214/hr Tokyo) + 50 GB gp3 EBS. Stop the instance
when idle and you only pay for EBS (~$5/mo); destroy with `terraform
destroy` and you pay nothing. Public IP changes on every stop/start —
re-read `terraform output public_ip` after restarting.

---

## End-to-end setup

### 1. Provision infrastructure

```bash
cd deploy/aws/terraform
terraform init
terraform apply
cd -
```

`terraform apply` provisions an EC2 instance, a security group (SSH + 3000),
and generates a fresh ed25519 SSH key under `deploy/aws/keys/`. First-boot
provisioning (Docker, nitro-cli, hugepage allocation) runs automatically
via cloud-init; subsequent scripts wait for it to finish.

### 2. Build the three images locally

```bash
bash deploy/aws/scripts/build-images.sh
```

Produces three `linux/amd64` Docker images:
- `title-protocol-tee-nitro:latest` — base for the EIF (vendor-aws build, no mock)
- `title-protocol-proxy:latest` — host-side vsock <-> internet bridge
- `title-protocol-gateway:latest` — public HTTP entrypoint

### 3. Ship images to EC2 and build the EIF

```bash
bash deploy/aws/scripts/ship-images.sh
```

`docker save | ssh docker load` for all three images, then
`nitro-cli build-enclave` on the host. The script prints **PCR0 / PCR1 /
PCR2** at the end — record PCR0; you'll register it on Solana via
`add_approved_measurement`.

### 4. Start the stack

```bash
bash deploy/aws/scripts/run-stack.sh
```

Brings up `title-proxy`, the Enclave, the socat bridge, and `title-gateway`.
Stops anything that was already running first, so it's safe to re-run after
edits.

To gate the Gateway behind a Bearer token:
```bash
API_KEYS="my-secret-key,another-key" bash deploy/aws/scripts/run-stack.sh
```

### 5. Sanity check

```bash
PUBLIC_IP=$(cd deploy/aws/terraform && terraform output -raw public_ip)
curl http://$PUBLIC_IP:3000/health
# {"status":"ok","tee_type":"aws-nitro"}
curl http://$PUBLIC_IP:3000/keys | jq
curl http://$PUBLIC_IP:3000/solana-keys | jq
```

If anything looks off, tail the Enclave console:
```bash
bash deploy/aws/scripts/tee-console.sh
```

---

## On-chain follow-up (one-time per measurement)

The TEE's signing key is fresh on every restart and is not trusted by the
`title-whitelist` program until you register it. See
[docs/v0.1.2/OPERATIONS_JA.md §4](../../docs/v0.1.2/OPERATIONS_JA.md) for the
full proof-generation + `register_key` sequence. Quick outline:

1. Note the PCR0 emitted by `ship-images.sh`.
2. Register it: see the `add_placeholder_measurement_devnet` test for the
   exact instruction encoding — swap the placeholder bytes for your real
   PCR0 and submit with the admin keypair.
3. SSH in (`bash deploy/aws/scripts/ssh.sh`) and dump the boot attestation
   from `/dev/nsm` (`nitro-cli console` shows the captured measurement in
   the startup log; the matching `signing_pubkey` is also logged).
4. Generate the SP1 Groth16 proof locally (`cargo run --release -p
   title-sp1-attestation-aws-nitro-host --bin prove -- <attestation.bin>`).
   This takes ~90 minutes on CPU.
5. Submit `register_key` with the resulting proof, public_values, and the
   freshly generated signing_pubkey.

The Gateway will then accept `POST /extension/solana` requests that mint
cNFTs signed by the TEE.

---

## Stop / restart

```bash
# Stop containers + enclave (leaves EC2 running, IP unchanged):
bash deploy/aws/scripts/stop-stack.sh

# Bring everything back without redeploying images:
bash deploy/aws/scripts/run-stack.sh
```

## Update code

After changes in `crates/`:

```bash
bash deploy/aws/scripts/build-images.sh   # rebuilds the three images
bash deploy/aws/scripts/ship-images.sh    # re-uploads and re-EIFs
bash deploy/aws/scripts/run-stack.sh      # restarts the stack
```

If TEE source changed, PCR0 will change. Register the new PCR0 with
`add_approved_measurement` and run a fresh `register_key`.

## Teardown

```bash
cd deploy/aws/terraform
terraform destroy
```

Removes the instance, security group, and SSH key from AWS. The local
key file under `deploy/aws/keys/` is left intact.

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| `nitro-cli build-enclave` fails with "Insufficient memory" | hugepage allocation race after instance reboot | `sudo systemctl restart nitro-enclaves-allocator` then retry |
| `curl /health` hangs | socat bridge or Enclave still booting | `bash deploy/aws/scripts/tee-console.sh` and confirm "TEE server starting" appeared |
| TEE process exits with "Self-attestation failed" | NSM device permission or runtime mismatch | confirm `TEE_RUNTIME=nitro` in the EIF (set by the Dockerfile) and `/dev/nsm` is accessible |
| `/extension/solana` returns 400 with "measurement mismatch" | TEE's runtime PCR0 doesn't match what was registered on Solana | rebuild + re-ship the EIF, register the new PCR0 |
| Gateway returns 502 / "TEE unavailable" | TEE container crashed; socat bridge orphaned | re-run `run-stack.sh` (it stops & restarts the whole stack) |
| Proxy logs "DNS lookup failed" for an upstream | upstream not reachable from EC2 | confirm AWS security group + VPC route table allow egress |
