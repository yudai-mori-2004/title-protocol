# Task 12: バイナリ暗号化ペイロード パフォーマンス分析

JSON + Base64 を全廃し、暗号化ペイロードを完全バイナリ化した後の E2E レイテンシ計測。

テスト画像: `pixel_photo_ramen.jpg` (2,240 KB, JPEG, C2PA署名済み, Google Pixel)

## 計測環境

| 項目 | Local | EC2 |
|------|-------|-----|
| TEE Runtime | MockRuntime | AWS Nitro Enclave (c5.xlarge) |
| TempStorage | localhost:3001 (in-memory) | S3 ap-northeast-1 |
| Gateway | localhost:3000 | EC2:3000 (Docker) |
| Solana RPC | devnet public | devnet public |
| Client 所在地 | macOS (東京) | macOS (東京) → EC2 (ap-northeast-1) |
| SDK | @title-protocol/sdk 0.1.9 | 同左 |

## E2E レイテンシ内訳（--broadcast, 3回計測の平均）

| ステップ | 処理内容 | Local (ms) | EC2 (ms) | 差分 |
|----------|----------|-----------|----------|------|
| STEP 1 | GlobalConfig 取得 + ノード発見 | 755 | 860 | +105 |
| STEP 3a | バイナリ暗号化 (ECDH + AES-GCM) | 16 | 11 | -5 |
| STEP 3b | TempStorage アップロード (2.2MB) | 11 | 600 | **+589** |
| STEP 4 | `/verify` (TEE 処理) | **36** | **375** | **+339** |
| STEP 5 | Irys 初期化 + Arweave アップロード | 2,742 | 1,982 | -760 |
| STEP 6 | `/sign` (TX 構築 + TEE 署名) | 1,474 | 1,206 | -268 |
| STEP 7 | Solana broadcast + confirm | 724 | 625 | -99 |
| | **合計** | **~5.9s** | **~5.8s** | **-0.1s** |

## Verify-only レイテンシ（--skip-sign, 3回計測の平均）

| ステップ | Local (ms) | EC2 (ms) |
|----------|-----------|----------|
| バイナリ暗号化 | 21 | 13 |
| アップロード | 10 | 553 |
| `/verify` | **40** | **395** |
| **合計 (STEP 1除く)** | **~71** | **~961** |

## ペイロードサイズ比較

| | 旧 (JSON + Base64) | 新 (バイナリ) | 削減率 |
|---|---|---|---|
| 暗号化前の平文 | 3,051 KB (content Base64 + metadata JSON) | **2,240 KB** (4B header + metadata JSON + raw content) | **-27%** |
| 暗号化後ペイロード | ~4,070 KB (JSON{Base64(eph_pk), Base64(nonce), Base64(ciphertext)}) | **2,240 KB** (32B + 12B + ciphertext) | **-45%** |
| S3 アップロードサイズ | ~4.1 MB | **2.2 MB** | **-45%** |
| Base64 変換回数 (SDK) | 3回 (content→B64, ciphertext→B64, payload→JSON) | **0回** | **-100%** |
| Base64 デコード回数 (TEE) | 4回 (eph_pk, nonce, ciphertext, content) | **0回** | **-100%** |

注: 2,240 KB のテスト画像の場合。5MB ファイルでは旧: ~17MB → 新: ~5MB（**-70%**）。

## v0.1.1 (JSON) → v0.1.1 (バイナリ) 改善

v0.1.1 task 10 時点（JSON + Base64）の計測値との比較:

| メトリクス | JSON + Base64 (Local) | バイナリ (Local) | 改善 |
|-----------|----------------------|-----------------|------|
| ペイロードサイズ | ~4.1 MB | **2.2 MB** | **-45%** |
| SDK 暗号化時間 | 26ms | **16ms** | **-38%** |
| アップロード時間 | 24ms | **11ms** | **-54%** |
| `/verify` レイテンシ | 54ms | **36ms** | **-33%** |
| Verify-only 合計 (STEP 1除く) | ~100ms | **~71ms** | **-29%** |
| Full-flow 合計 | ~6.0s | **~5.9s** | -2% |

| メトリクス | JSON + Base64 (EC2) | バイナリ (EC2) | 改善 |
|-----------|---------------------|---------------|------|
| S3 アップロード時間 | 700ms | **600ms** | **-14%** |
| `/verify` レイテンシ | 444ms | **375ms** | **-16%** |
| Verify-only 合計 (STEP 1除く) | ~2,300ms | **~961ms** | **-58%** |
| Full-flow 合計 | ~7.3s | **~5.8s** | **-21%** |

EC2 での改善が大きい理由: S3 経由のアップロードサイズが 4.1MB → 2.2MB に半減し、S3 PUT + TEE ダウンロードの両方でネットワーク時間が削減されるため。

## /verify 内訳推定

| 処理 | Local | EC2 |
|------|-------|-----|
| TempStorage/S3 からダウンロード | ~3ms | ~120ms |
| バイナリヘッダパース (eph_pk + nonce + ciphertext) | <0.1ms | <0.1ms |
| ECDH + HKDF + AES-GCM 復号 | ~3ms | ~3ms |
| 平文パース (metadata_len + JSON + content 分離) | <0.1ms | <0.1ms |
| C2PA 検証 (c2pa crate v0.78) | ~25ms | ~25ms |
| TEE 署名 + attestation | ~3ms | ~5ms (NSM API) |
| レスポンス暗号化 | ~1ms | ~1ms |
| vsock + proxy オーバーヘッド | 0 | ~220ms |
| **合計** | **~36ms** | **~375ms** |

旧フォーマットでの差分:
- `serde_json::from_slice(&body)` (EncryptedPayload パース) → 削除、バイナリスライス参照に置換
- `b64().decode()` × 4回 → 削除（eph_pk, nonce, ciphertext, content の全 Base64 デコード不要）
- `serde_json::from_slice(&plaintext)` (ClientPayload パース) → `parse_plaintext_payload()` でメタデータのみパース、content はゼロコピー
- S3 ダウンロードサイズ 4.1MB → 2.2MB（Enclave 内のネットワーク通信量半減）

## パイプライン全体像

```
Client (東京)             Gateway (EC2)        TEE (Nitro Enclave)   Solana
  │                         │                    │                     │
  ├─ buildPlaintext         │                    │                     │
  │  [4B meta_len]          │                    │                     │
  │  [metadata JSON]        │                    │                     │
  │  [raw content]          │                    │                     │
  ├─ encrypt (11ms) ──────►│                    │                     │
  │  [32B eph_pk]           │                    │                     │
  │  [12B nonce]            │                    │                     │
  │  [ciphertext]           │                    │                     │
  ├─ S3 PUT (600ms) ──────►│                    │                     │
  │  2.2MB octet-stream     │                    │                     │
  ├─ /verify ──────────────►├─ S3 GET ──────────►│                     │
  │                         │  (via vsock+proxy) ├─ parse binary header │
  │                         │                    ├─ ECDH + HKDF        │
  │                         │                    ├─ AES-GCM decrypt    │
  │                         │                    ├─ parse plaintext    │
  │                         │                    ├─ C2PA verify        │
  │                         │                    ├─ NSM sign+attest    │
  │◄── encrypted response ──┤◄───────────────────┤ (375ms)             │
  ├─ Irys upload (2.0s) ───►│ (external)         │                     │
  ├─ /sign ────────────────►├─ RPC fetch ────────────────────────────►│
  │                         │◄────────────────────────────────────────┤
  │                         ├─ build TX ────────►│                     │
  │                         │                    ├─ sign TX (NSM)      │
  │◄── partial TX ──────────┤◄───────────────────┤ (1.2s)              │
  ├─ broadcast ───────────────────────────────────────────────────────►│
  │◄── confirmed ─────────────────────────────────────────────────────┤
  │                                                (625ms)             │
```

## 環境別サマリ

| メトリクス | Local (Mock) | EC2 (Nitro) |
|-----------|-------------|-------------|
| Verify-only (STEP 1除く) | **~71ms** | **~961ms** |
| Full (broadcast) | **~5.9s** | **~5.8s** |
| TEE 処理のみ | 36ms | 375ms |
| TX サイズ | 604 bytes (v0 + ALT) | 604 bytes (v0 + ALT) |
| TX 数 | 1 | 1 |

## devnet TX 証跡

### Local (MockRuntime)

| Run | TX Signature | Explorer |
|-----|-------------|---------|
| 1 | `2aUncr...cShje` | [explorer](https://explorer.solana.com/tx/2aUncrAUg8bvyKJkNsEiGqUiz2FzvQiNWQ6MwHxRfjpsK3oD4XEUzFTjTeWfYsdPJZtWCpR6jozTqbBLHwvcShje?cluster=devnet) |
| 2 | `2qxU9v...MYetj` | [explorer](https://explorer.solana.com/tx/2qxU9v9euWYJARHkjTZBSTPixRk8YSZ7CBWBxhJdMdEvNJ3PKfk8n3bKvVbYLLwCcYYRd6TKEMV4rMmdkWbMYetj?cluster=devnet) |
| 3 | `4VLnJc...gBTg` | [explorer](https://explorer.solana.com/tx/4VLnJcFrCksdgP4tnXWG4NTRvWKfg6GSAXHR5dP1yXirEEd3CWvJPi2ZfYTzdZAzGp81CwJyxemGKm6ZvLwygBTg?cluster=devnet) |

### EC2 (AWS Nitro Enclave, c5.xlarge, ap-northeast-1)

| Run | TX Signature | Explorer |
|-----|-------------|---------|
| 1 | `3JntG2...TmSke` | [explorer](https://explorer.solana.com/tx/3JntG2smXgVggo1QmLsyPzrxbDoLyhQTQPvzNh3ZcyKrkEP89XPwtx6Ag9gteSjpieWmDhqqXEoH3QStyr4TmSke?cluster=devnet) |
| 2 | `4Y4THF...zCxr` | [explorer](https://explorer.solana.com/tx/4Y4THFE7iEs7XRsv3AmyJUt1b7DfXZq6kGpj1QGYzf3GGXr463C3bTv9c23DmiSC11ghWZWiCTBjDXTM3uvWzCxr?cluster=devnet) |
| 3 | `3CirZF...NgX8` | [explorer](https://explorer.solana.com/tx/3CirZFMPDhh57Kn76Qfh4tvBSsLXeDg3gxUQREUJSMtGeyRB1YHwFkCbZ7h9V82QLsL2t1r7HLipfZMg1d1gNgX8?cluster=devnet) |

全 TX: 604 bytes, VersionedTransaction v0, ALT 使用, 1 cNFT/TX。
