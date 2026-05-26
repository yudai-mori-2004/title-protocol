# Stress test — 大容量 white-noise mp4 を TEE に流す

タスク20 の一環として、実 TEE (c5.xlarge / Nitro Enclave 2 GiB) に
C2PA-signed white-noise mp4 を投げて挙動を測った記録。

## セットアップ

- TEE: AWS Nitro Enclave, c5.xlarge host, 2048 MiB enclave RAM,
  PCR0 `bc3cbddb...` (trust-off ポリシー入り)
- 署名: 自前 ED25519 cert chain (CA → EE, `/tmp/c2pa-test-cert/chain.pem`)
- 入力生成: `ffmpeg -f lavfi nullsrc + geq=random(1)*255` で白ノイズ動画
- ホスト: ローカル Mac から `title-cli process --url <S3 URL>` で投げる
- ストレージ: `s3://title-signed-json-devnet/test-fixtures/stress-*.mp4`

## 発見 1: `ProxyContentFetcher` の 100 MiB 制限

| signed.mp4 サイズ | TEE c2pa-verify status | 備考 |
|---|---|---|
| 4 MiB | ok | small 確認 |
| 40 MiB | ok | |
| 60 MiB | ok | |
| 81 MiB | ok | |
| 101 MiB | ok | 境界手前 |
| 121 MiB | **error (Invalid)** | 境界超え |
| 141 MiB | error (Invalid) | |
| 161 MiB | error (Invalid) | |
| 606 MiB | error (Invalid) | |

c2pa-rs 自体は手元 (同じバイナリ) で全サイズ Valid と判定するので、
TEE 側で content が途中で切られている (truncate されている) ことが原因。

コード追跡:
```
crates/tee/src/proxy_fetcher.rs:81
pub const DEFAULT_MAX_BODY_BYTES: usize = 100 * 1024 * 1024;
```

これは仕様 §4.4 「Attack defense parameters」のデフォルト値だが、Title
Protocol の現実的なユースケース (full-length / high-resolution video の cNFT 化)
では小さすぎる。

## 対応: `MAX_CONTENT_BYTES` env var 追加

`crates/tee/src/main.rs` で `MAX_CONTENT_BYTES` env var を読み、未設定なら
仕様デフォルト 100 MiB。AWS Nitro 用 Dockerfile (`deploy/aws/docker/tee-nitro.Dockerfile`)
では `ENV MAX_CONTENT_BYTES=2147483648` で 2 GiB をデフォルトに引き上げ。

```rust
let max_content_bytes: usize = std::env::var("MAX_CONTENT_BYTES")
    .ok()
    .and_then(|s| s.parse().ok())
    .unwrap_or(ProxyContentFetcher::DEFAULT_MAX_BODY_BYTES);
```

Dockerfile 変更で PCR0 が変わるので allowlist と register-key 再実行が必要。
それ以外のフローは無変更。

## 発見 2: signature_hash の局所性

ローカル `c2pa::Reader::from_stream(..).active_manifest().signature().expect()`
の sha256 と、TEE の `compute_signature_hash` (jumbf::extract_signature_from_jumbf
経由) で値が異なる。両者とも決定的なので、Title Protocol 内で完結する用途
(`/process` レスポンスの `signature_hash`, attestation の `user_data`,
JCS canonical hashing) は問題なく動く。c2pa-rs 公式ツールの hash と直接比較
したいユースケースが将来出てきたら別途揃える必要あり。

## レイテンシ — 単発リクエスト

`MAX_CONTENT_BYTES=2 GiB` + `MAX_RESPONSE_BYTES=2 GiB` 拡張後 (PCR0
`49daf071...`、enclave 2048 MiB RAM、c5.xlarge ホスト)。
ローカル Mac → 東京リージョン EC2、S3 download 込みの end-to-end latency:

| 入力 | TEE c2pa-verify | elapsed |
|---|---|---|
| `stress-big-200.mp4` (214 MiB) | ok | 7s |
| `stress-big-500.mp4` (536 MiB) | ok | 12s |
| `stress-big-1000.mp4` (1071 MiB) | ok | 23s |

スループット: おおよそ **40 MiB/s** 安定 (S3 → proxy → enclave → c2pa-rs
パース込み)。c5.xlarge の network 帯域に支配される。

## 並列リクエスト

| 構成 | 結果 | 備考 |
|---|---|---|
| N=2 × 200 MiB | 2/2 ok, 8s | enclave 健在 |
| N=2 × 500 MiB | 2/2 ok, 14s | enclave 健在 |
| N=4 × 200 MiB | 3/4 ok (1 c2pa-verify error), 10s | enclave 健在 |
| N=2 × 1 GiB | 0/2, **HTTP 503**, enclave クラッシュ | 2 GiB 合計 = enclave RAM 上限 |
| N=4 × 500 MiB | 0/4, **HTTP 503**, enclave クラッシュ | 2 GiB 合計 = enclave RAM 上限 |

**観測**: `enclave RAM (2048 MiB) ≒ in-flight 合計 content size` を超えると
enclave が落ちて Gateway が 503 を返す。`run.sh` で再起動すれば復活。

実運用上の指針:
- 単発 1 GiB までは安定
- 並列度を上げる場合は **N × size ≦ enclave RAM × 0.8 程度** を目安に
- 大容量を捌くなら `ENCLAVE_MEM_MIB` を 4096〜8192 に上げる
  (`/etc/nitro_enclaves/allocator.yaml` も同じ値に揃える)

## 修正コミット履歴

| commit | 内容 |
|---|---|
| `58abf3d` | `MAX_CONTENT_BYTES` env var 追加、AWS Nitro image で 2 GiB をデフォルトに |
| `f42cf88` | `title-proxy` の `MAX_RESPONSE_BYTES` を 100 MiB → 2 GiB |
| `15c308a` | **真の streaming 修正**: `SafeRangeReader::read` を 4 MiB clamp + c2pa-rs fork で `MAX_HASH_BUF` を 256 MiB → 4 MiB |

## 真の streaming 修正 (commit `15c308a`) 後の再測

PCR0 `fd46afe9b07369d8ff71306d376cadad2abc7c26160deb0b87ee7be4aec6fbf6988733cd632eae9c60511643ea474d95`、
enclave 2048 MiB RAM。

### 単発

| 入力 | 結果 | elapsed |
|---|---|---|
| 200 MiB | ok | 9 s |
| 500 MiB | ok | 20 s |
| 1 GiB | ok | 38 s |

### 並列（**合計 in-flight が enclave RAM を超える領域**）

| 構成 | 合計 | 結果 | elapsed |
|---|---|---|---|
| N=4 × 500 MiB | 2 GiB | **4/4 ok**, enclave 健在 | 20 s |
| N=8 × 500 MiB | **4 GiB** | **8/8 ok**, enclave 健在 | 30 s |
| N=4 × 1 GiB | **4 GiB** | **4/4 ok**, enclave 健在 | 39 s |

修正前は N=2 × 1 GiB (2 GiB 合計) で OOM クラッシュしていた。修正後は
合計 4 GiB の in-flight でも安定 → **per-reader peak メモリが content size
非依存になった証拠**。streaming verification の本来の利点が回復。

## どうしてこれが効いたか

1. `c2pa-rs hash_stream_by_alg_with_progress` (`utils/hash_utils.rs:449,472`)
   が要求する `read_exact(256 MiB buf)` を、`SafeRangeReader::read` が
   per-call 4 MiB に clamp。`Read::read` 規約上「要求より少なく返してよい」
   のを利用して、c2pa-rs の `read_exact` ループに残りを取り直させる。
   底層 Range Request は常に 4 MiB 上限。
2. c2pa-rs fork の `MAX_HASH_BUF = 256 MiB → 4 MiB`。chunk 用 alloc
   (current + next_chunk のダブルバッファ) が 512 MiB → 8 MiB に圧縮。
   per-reader peak メモリは `4 MiB (clamp) + 8 MiB (hash chunks) + 4 MiB
   (http buffer) ≈ 16 MiB`。

100 並列でも 1.6 GiB なので 2 GiB enclave に余裕で収まる試算。実 throughput は
ネットワーク帯域・vsock 帯域・c5.xlarge の 4 vCPU が律速。

## 残作業

- enclave memory 4 GiB / 8 GiB プロビジョン時の最大 N × size の再測定
- HTTP server / ResourcePool / enclave 内 OOM 時の自己回復 (現状は外部から
  `run.sh` を叩き直す必要あり)
- `c2pa::Reader` の MP4 BMFF parser memory profile (大きい moov atom が
  どれだけ RAM を食うか実測)

## 既知の周辺問題 (本タスクと独立)

- `c2pa::Reader` の MP4 parser が大きい BMFF atom テーブルを抱える点。
  数 GiB レベルだと heap 使用量が無視できない可能性。
- title-proxy 側の `MAX_TOTAL_BYTES` が別途存在する場合は要確認
  (本セッションで grep した範囲では proxy 側に独立した cap は見つからず)。
