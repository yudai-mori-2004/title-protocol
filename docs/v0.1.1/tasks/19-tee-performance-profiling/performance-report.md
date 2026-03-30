# TEE Performance Report

## Test Environment

| Item | Value |
|------|-------|
| EC2 Instance | c5.2xlarge (8 vCPU, 16 GB RAM) |
| Enclave vCPU | 2 |
| Enclave Memory | 8192 MB |
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

---

## Comparison

### Maximum concurrent capacity by POOL size

| Content | 1500 MB | 2500 MB |
|---------|---------|---------|
| 10MB | ×8 | ×16 |
| 25MB | ×16 | ×16 |
| 50MB | ×16 | ×16 |
| 100MB | ×16 | ×16 |
| 200MB | ×4 | ×8 |
| 500MB | ×1 | ×2 |

### Latency comparison (same size × concurrency)

| Content × Concurrency | 1500 MB | 2500 MB |
|-----------------------|---------|---------|
| 100MB × 8 | 11,242ms | 11,128ms |
| 200MB × 4 | 11,791ms | 12,095ms |
| 500MB × 1 | 10,902ms | 10,889ms |
| 50MB × 16 | 10,925ms | 10,762ms |

**Latency is identical regardless of POOL size.** The only difference is whether the request is admitted or rejected.

---

## Analysis

### POOL_TOTAL_LIMIT = maximum concurrent content in memory

The pool acts as a simple admission control. A request is admitted if `current_used + content_size ≤ POOL_TOTAL_LIMIT × 0.75` (admission threshold is 75% of total). If exceeded, the TEE returns HTTP 503 immediately.

- **1500MB pool**: admits up to ~1125MB concurrent → 500MB×1 OK, 500MB×2 FAIL
- **2500MB pool**: admits up to ~1875MB concurrent → 500MB×2 OK (1000MB), 500MB×4 FAIL (2000MB)

### Latency is determined by content size and vCPU, not pool size

Single-request latency per content size (averaged across both pool sizes):

| Content | Verify latency |
|---------|---------------|
| 10MB | ~300ms |
| 25MB | ~640ms |
| 50MB | ~1,160ms |
| 100MB | ~2,360ms |
| 200MB | ~4,610ms |
| 500MB | ~10,900ms |

Scales linearly at ~22ms per MB. This is the TEE's download + decrypt + C2PA parse time.

### Concurrent request latency scales sub-linearly

At ×8 concurrency, total wall-clock is ~4-5× single (not 8×), indicating parallel processing within the TEE. The 2 vCPU allocation limits true parallelism.

### Failure modes

| Error | Cause |
|-------|-------|
| HTTP 503 (ResourcePool exhausted) | Concurrent content exceeds POOL_TOTAL_LIMIT |
| S3 socket closed | Client-side upload bandwidth saturated (500MB × 4+ parallel uploads) |
| Headers timeout | S3 presigned URL timeout during large parallel uploads |

### Recommendation

| Use case | POOL_TOTAL_LIMIT |
|----------|-----------------|
| Small content only (<50MB) | 700 MB |
| Mixed content up to 200MB | 1500 MB |
| Large content (500MB) + concurrency | 2500 MB |

The pool size should be set based on expected max content size × expected concurrent requests. Increasing beyond the actual concurrent load has no benefit and no cost (unused pool memory is not allocated).
