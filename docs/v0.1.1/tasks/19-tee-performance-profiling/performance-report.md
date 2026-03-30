# TEE Performance Report

## Test Environment

| Item | Value |
|------|-------|
| EC2 Instance | c5.2xlarge (8 vCPU, 16 GB RAM) |
| Enclave vCPU | 2 |
| Enclave Memory | 12288 MB |
| Proxy | Streaming (chunk-by-chunk, no response buffering) |
| Region | ap-northeast-1 |
| Date | 2026-03-30 |

All measurements use the streaming proxy and the same instance. The only variable is `POOL_TOTAL_LIMIT`.

---

## Results

Values are total wall-clock time (ms). "n/m FAIL" = n succeeded out of m requests.

### POOL_TOTAL_LIMIT = 1500 MB

| Content | ×1 | ×2 | ×4 | ×8 | ×16 |
|---------|-----|-----|-----|-----|------|
| **10MB** | 308 | 458 | 808 | 1,414 | 14/16 FAIL |
| **25MB** | 646 | 952 | 1,604 | 2,859 | 5,507 |
| **50MB** | 1,183 | 1,684 | 3,150 | 5,807 | 10,925 |
| **100MB** | 2,391 | 3,330 | 5,981 | 11,242 | 21,947 |
| **200MB** | 4,612 | 6,670 | 11,791 | 6/8 FAIL | 5/16 FAIL |
| **500MB** | 10,902 | 1/2 FAIL | 1/4 FAIL | — | — |

### POOL_TOTAL_LIMIT = 2500 MB

| Content | ×1 | ×2 | ×4 | ×8 | ×16 |
|---------|-----|-----|-----|-----|------|
| **10MB** | 328 | 634 | 803 | 1,423 | 2,596 |
| **25MB** | 622 | 943 | 1,733 | 3,147 | 5,441 |
| **50MB** | 1,143 | 1,738 | 2,874 | 5,459 | 10,762 |
| **100MB** | 2,332 | 3,432 | 6,101 | 11,128 | 21,686 |
| **200MB** | 4,607 | 6,725 | 12,095 | 22,237 | 15/16 FAIL |
| **500MB** | 10,889 | 16,927 | 3/4 FAIL | — | — |

### POOL_TOTAL_LIMIT = 10000 MB

| Content | ×1 | ×2 | ×4 | ×8 | ×16 |
|---------|-----|-----|-----|-----|------|
| **10MB** | 315 | 461 | 857 | 1,444 | 2,698 |
| **25MB** | 678 | 988 | 1,636 | 2,979 | 5,671 |
| **50MB** | 1,256 | 1,742 | 2,961 | 5,608 | 10,809 |
| **100MB** | 2,295 | 3,573 | 5,951 | 10,947 | 21,627 |
| **200MB** | 4,544 | 6,632 | 11,547 | 22,351 | 41,501 |
| **500MB** | 10,987 | 16,010 | 28,154 | S3 timeout | S3 timeout |

500MB×8/×16: client-side S3 upload timeout (not a TEE limitation).

---

## Comparison: Maximum Concurrent Capacity

| Content | 1500 MB | 2500 MB | 10000 MB |
|---------|---------|---------|----------|
| 10MB | ×8 | ×16 | ×16 |
| 25MB | ×16 | ×16 | ×16 |
| 50MB | ×16 | ×16 | ×16 |
| 100MB | ×16 | ×16 | ×16 |
| 200MB | ×4 | ×8 | **×16** |
| 500MB | ×1 | ×2 | **×4** |

## Latency Comparison (same workload, different pool)

| Workload | 1500 MB | 2500 MB | 10000 MB |
|----------|---------|---------|----------|
| 10MB × 16 | 14/16 FAIL | 2,596ms | 2,698ms |
| 50MB × 8 | 5,807ms | 5,459ms | 5,608ms |
| 100MB × 8 | 11,242ms | 11,128ms | 10,947ms |
| 200MB × 8 | 6/8 FAIL | 22,237ms | 22,351ms |
| 200MB × 16 | 5/16 FAIL | 15/16 FAIL | **41,501ms** |
| 500MB × 1 | 10,902ms | 10,889ms | 10,987ms |
| 500MB × 4 | 1/4 FAIL | 3/4 FAIL | **28,154ms** |

**Latency is constant regardless of pool size.** Pool only determines admission — whether a request is accepted or rejected.

---

## Analysis

### Pool = admission control

`POOL_TOTAL_LIMIT` is a simple admission gate. A request is admitted if `current_used + content_size ≤ POOL_TOTAL_LIMIT × 0.75`. If exceeded, HTTP 503 is returned immediately. Pool size has zero effect on processing speed.

### Latency per content size (single request average)

| Content | Verify latency |
|---------|---------------|
| 10MB | ~310ms |
| 25MB | ~650ms |
| 50MB | ~1,190ms |
| 100MB | ~2,340ms |
| 200MB | ~4,570ms |
| 500MB | ~10,930ms |

Linear at **~22ms per MB** (TEE download from S3 + decrypt + C2PA parse).

### Concurrency scaling

At ×8, total wall-clock is ~4-5× single (not 8×). The 2 Enclave vCPUs limit true parallelism but async I/O enables overlap.

### Failure modes

| Error | Cause | TEE-side? |
|-------|-------|-----------|
| HTTP 503 | ResourcePool exhausted | Yes — increase POOL_TOTAL_LIMIT |
| S3 Headers Timeout | Client upload bandwidth saturated | No — client-side network |
| S3 socket closed | Parallel upload contention | No — client-side network |

### Recommendation

Set `POOL_TOTAL_LIMIT` based on expected workload:

| Use case | POOL_TOTAL_LIMIT | Rationale |
|----------|-----------------|-----------|
| Photos only (<50MB) | 1500 MB | 16 concurrent photos |
| Mixed photo + video (<200MB) | 2500 MB | 8 concurrent 200MB videos |
| Large video (500MB+) | 5000-10000 MB | 4 concurrent 500MB, requires large instance |

Pool memory is not pre-allocated — unused capacity has no cost. Setting a high limit on a large instance has no downside.
