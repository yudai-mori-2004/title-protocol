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

## 残作業 (本セッションで実施予定)

- [ ] MAX_CONTENT_BYTES 2 GiB でデプロイ後の TEE で同じ計測表を再取得
- [ ] 1 GiB / 2 GiB レベルでの latency と enclave メモリ消費の測定
- [ ] 並列 N=2/4/8 でレスポンス時間の劣化を確認 (前提: `ResourcePool` の
  admission limit / total limit)
- [ ] 端まで通る最大サイズの確認 (`MAX_CONTENT_BYTES` 超え or RAM 圧迫で失敗するか)

## レイテンシ (現状 = 100 MiB cap、cache hit 後)

| asset | サイズ | TEE 処理時間 (CLI elapsed) | 備考 |
|---|---|---|---|
| `c2pa-properly-signed.jpg` | 273 KiB | < 1s | cache 関係なく即時 |
| `stress-small.mp4` (4 MiB) | 4 MiB | ~1s | |
| `stress-101.mp4` | 101 MiB | ~2-3s | S3 → TEE proxy fetch 込み |
| `stress-606mb.mp4` (truncated に終わる) | 606 MiB DL → 100 MiB 処理 | ~1s | early fail |

実 throughput 測定は MAX_CONTENT_BYTES 拡張デプロイ後に追加予定。

## 既知の周辺問題 (本タスクと独立)

- `c2pa::Reader` の MP4 parser が大きい BMFF atom テーブルを抱える点。
  数 GiB レベルだと heap 使用量が無視できない可能性。
- title-proxy 側の `MAX_TOTAL_BYTES` が別途存在する場合は要確認
  (本セッションで grep した範囲では proxy 側に独立した cap は見つからず)。
