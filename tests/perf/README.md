# Performance Tests

TEE node performance characterization and resilience testing.

## Setup

```bash
cd tests/perf
npm install
```

Set `TEST_GATEWAY_URL` for remote testing (default: `http://localhost:3000`).

## Tests

| Script | Purpose | Duration |
|--------|---------|----------|
| `npm run baseline` | Single-request latency × content size × processor count | ~5 min |
| `npm run throughput` | Find saturation point (1→64 parallel requests) | ~5 min |
| `npm run sustained` | Constant-rate load for 2 min, detect latency drift | ~3 min |
| `npm run content-size` | Upload vs verify time breakdown by content size | ~3 min |
| `npm run resilience` | Adversarial inputs: oversized, rapid-fire, tampered, invalid | ~2 min |

```bash
# Run all
npm run all

# Run against EC2
TEST_GATEWAY_URL=http://52.69.105.233:3000 npm run baseline
```

## What to look for

**Baseline** — Establishes reference latency per content type and processor combination.

**Throughput** — Shows requests/second at each concurrency level. When p95 latency spikes or success rate drops below 50%, that's the saturation point.

**Sustained** — If the "Latency drift" percentage exceeds 50%, the node has a resource leak (memory, ResourcePool, file handles).

**Content size** — If verify time is constant regardless of content size, the bottleneck is C2PA parsing (constant-time). If it scales linearly, the bottleneck is transfer/decryption.

**Resilience** — Verifies the node rejects bad input without crashing and recovers after adversarial load.

## Comparing TEE specs

Run baseline and throughput on different EC2 instance types to identify:
- **Proportional costs** (scale with vCPU/memory): WASM execution, image decoding
- **Constant costs** (same regardless of spec): crypto operations, network round-trip
