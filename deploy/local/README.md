# Title Protocol — Local development deployment

Run the full stack locally with mock TEE runtime. Two startup methods:

---

## Method A: Native processes (fast iteration)

Builds and runs binaries directly — no Docker rebuild on code changes.

```bash
# Start
bash deploy/local/start.sh

# Start without rebuilding
bash deploy/local/start.sh --skip-build

# Stop
bash deploy/local/stop.sh
```

| Component | Address | Runtime |
|---|---|---|
| Proxy | 127.0.0.1:8000 | TCP (no vsock) |
| TEE | 127.0.0.1:4000 | mock |
| Gateway | 0.0.0.0:3000 | — |

Logs: `.local-stack/logs/`

---

## Method B: Docker Compose

Same architecture as AWS but containerized with mock runtime.

```bash
# Start
docker compose -f deploy/local/docker-compose.yml up --build -d

# Smoke test
bash deploy/local/docker/smoke-test.sh

# Stop
docker compose -f deploy/local/docker-compose.yml down
```

All services share a network namespace (same as AWS `--network host`),
communicating over 127.0.0.1.

---

## Verify

```bash
curl http://localhost:3000/health
curl http://localhost:3000/keys | jq
curl http://localhost:3000/processors | jq
curl http://localhost:3000/solana-keys | jq
```
