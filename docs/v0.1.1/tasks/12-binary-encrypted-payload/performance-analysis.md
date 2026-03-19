# Task 12: バイナリ暗号化ペイロード パフォーマンス分析

JSON + Base64 を全廃し、暗号化ペイロードを完全バイナリ化した後の E2E レイテンシ計測。

テスト画像: `pixel_photo_ramen.jpg` (2,240 KB, JPEG, C2PA署名済み, Google Pixel)

## 計測環境

| 項目 | 値 |
|------|-----|
| TEE Runtime | MockRuntime (localhost:4000) |
| TempStorage | localhost:3001 (in-memory) |
| Gateway | localhost:3000 |
| Solana RPC | devnet public |
| Client | macOS (東京), Node.js v24.11.1 |
| SDK | @title-protocol/sdk 0.1.9 |

## Verify-only レイテンシ（--skip-sign, 3回計測）

| ステップ | 処理内容 | Run 1 | Run 2 | Run 3 | 平均 |
|----------|----------|-------|-------|-------|------|
| STEP 1 | GlobalConfig 取得 + ノード発見 | 797ms | 800ms | 766ms | 788ms |
| STEP 3a | バイナリ暗号化 (ECDH + AES-GCM) | 15ms | 20ms | 28ms | 21ms |
| STEP 3b | TempStorage アップロード (2.2MB) | 8ms | 13ms | 10ms | 10ms |
| STEP 4 | `/verify` (TEE 処理) | **36ms** | **43ms** | **42ms** | **40ms** |
| | **合計 (STEP 1 除く)** | **59ms** | **76ms** | **80ms** | **71ms** |

## Full-flow レイテンシ（--broadcast, 3回計測）

| ステップ | 処理内容 | Run 1 | Run 2 | Run 3 | 平均 |
|----------|----------|-------|-------|-------|------|
| STEP 1 | GlobalConfig 取得 + ノード発見 | 743ms | 772ms | 749ms |  |
| STEP 3a | バイナリ暗号化 | 9ms | 14ms | 24ms | 16ms |
| STEP 3b | TempStorage アップロード (2.2MB) | 6ms | 15ms | 11ms | 11ms |
| STEP 4 | `/verify` | **31ms** | **34ms** | **44ms** | **36ms** |
| STEP 5 | Irys 初期化 + Arweave アップロード | 3,086ms | 2,759ms | 2,380ms | 2,742ms |
| STEP 6 | `/sign` (TX 構築 + TEE 署名) | 1,399ms | 1,511ms | 1,511ms | 1,474ms |
| STEP 7 | Solana broadcast + confirm | 751ms | 571ms | 850ms | 724ms |
| | **合計** | **~6.1s** | **~5.8s** | **~5.7s** | **~5.9s** |

## ペイロードサイズ比較

| | 旧 (JSON + Base64) | 新 (バイナリ) | 削減率 |
|---|---|---|---|
| 暗号化前の平文 | 3,051 KB (content Base64 + metadata JSON) | **2,240 KB** (4B header + metadata JSON + raw content) | **-27%** |
| 暗号化後ペイロード | ~4,070 KB (JSON{Base64(eph_pk), Base64(nonce), Base64(ciphertext)}) | **2,240 KB** (32B + 12B + ciphertext) | **-45%** |
| Base64 変換回数 (SDK) | 3回 (content→B64, ciphertext→B64, payload→JSON) | **0回** | **-100%** |
| Base64 デコード回数 (TEE) | 4回 (eph_pk, nonce, ciphertext, content) | **0回** | **-100%** |

注: 2,240 KB のテスト画像の場合。5MB ファイルでは旧: ~17MB → 新: ~5MB（**-70%**）。

## v0.1.1 (JSON) → v0.1.1 (バイナリ) 改善

v0.1.1 task 10 時点（JSON + Base64）の計測値との比較:

| メトリクス | JSON + Base64 | バイナリ | 改善 |
|-----------|---------------|---------|------|
| ペイロードサイズ (2.2MB画像) | ~4.1 MB | **2.2 MB** | **-45%** |
| SDK 暗号化時間 | 26ms | **16ms** | **-38%** |
| アップロード時間 (local) | 24ms | **11ms** | **-54%** |
| `/verify` レイテンシ | 54ms | **36ms** | **-33%** |
| Verify-only 合計 (STEP 1除く) | ~100ms | **~71ms** | **-29%** |
| Full-flow 合計 | ~6.0s | **~5.9s** | -2% |

Full-flow の差が小さいのは、ボトルネックが Irys (2.7s) と /sign (1.5s) にあるため。
暗号化〜verify 区間（SDK→TempStorage→TEE）に限れば **約 30% の改善**。

## /verify 内訳推定

| 処理 | 時間 (推定) |
|------|-----------|
| TempStorage からダウンロード | ~3ms |
| バイナリヘッダパース (eph_pk + nonce + ciphertext) | <0.1ms |
| ECDH + HKDF + AES-GCM 復号 | ~3ms |
| 平文パース (metadata_len + JSON + content 分離) | <0.1ms |
| C2PA 検証 (c2pa crate v0.78) | ~25ms |
| TEE 署名 + attestation | ~3ms |
| レスポンス暗号化 | ~1ms |
| **合計** | **~36ms** |

旧フォーマットでの差分:
- `serde_json::from_slice(&body)` (EncryptedPayload パース) → 削除、バイナリスライス参照に置換
- `b64().decode()` × 4回 → 削除（eph_pk, nonce, ciphertext, content の全 Base64 デコード不要）
- `serde_json::from_slice(&plaintext)` (ClientPayload パース) → `parse_plaintext_payload()` でメタデータのみパース、content はゼロコピー

## パイプライン全体像

```
Client                    Gateway              TEE                   Solana
  │                         │                    │                     │
  ├─ buildPlaintext         │                    │                     │
  │  [4B meta_len]          │                    │                     │
  │  [metadata JSON]        │                    │                     │
  │  [raw content]          │                    │                     │
  ├─ encrypt (16ms) ──────►│                    │                     │
  │  [32B eph_pk]           │                    │                     │
  │  [12B nonce]            │                    │                     │
  │  [ciphertext]           │                    │                     │
  ├─ upload octet-stream ──►│                    │                     │
  │  (11ms, 2.2MB)          │                    │                     │
  ├─ /verify ──────────────►├─ download ────────►│                     │
  │                         │                    ├─ parse binary header │
  │                         │                    ├─ ECDH + HKDF        │
  │                         │                    ├─ AES-GCM decrypt    │
  │                         │                    ├─ parse plaintext    │
  │                         │                    ├─ C2PA verify        │
  │                         │                    ├─ sign + attest      │
  │◄── encrypted response ──┤◄───────────────────┤ (36ms)              │
  ├─ Irys upload (2.7s) ───►│ (external)         │                     │
  ├─ /sign ────────────────►├─ RPC fetch ────────────────────────────►│
  │                         │◄────────────────────────────────────────┤
  │                         ├─ build TX ────────►│                     │
  │                         │                    ├─ sign TX            │
  │◄── partial TX ──────────┤◄───────────────────┤ (1.5s)              │
  ├─ broadcast ───────────────────────────────────────────────────────►│
  │◄── confirmed ─────────────────────────────────────────────────────┤
  │                                                (724ms)             │
```

## devnet TX 証跡

| Run | TX Signature | Explorer |
|-----|-------------|---------|
| 1 | `2aUncr...cShje` | [explorer](https://explorer.solana.com/tx/2aUncrAUg8bvyKJkNsEiGqUiz2FzvQiNWQ6MwHxRfjpsK3oD4XEUzFTjTeWfYsdPJZtWCpR6jozTqbBLHwvcShje?cluster=devnet) |
| 2 | `2qxU9v...MYetj` | [explorer](https://explorer.solana.com/tx/2qxU9v9euWYJARHkjTZBSTPixRk8YSZ7CBWBxhJdMdEvNJ3PKfk8n3bKvVbYLLwCcYYRd6TKEMV4rMmdkWbMYetj?cluster=devnet) |
| 3 | `4VLnJc...gBTg` | [explorer](https://explorer.solana.com/tx/4VLnJcFrCksdgP4tnXWG4NTRvWKfg6GSAXHR5dP1yXirEEd3CWvJPi2ZfYTzdZAzGp81CwJyxemGKm6ZvLwygBTg?cluster=devnet) |

全 TX: 604 bytes, VersionedTransaction v0, ALT 使用, 1 cNFT/TX。
