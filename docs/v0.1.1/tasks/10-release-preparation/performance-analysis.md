# Title Protocol パフォーマンス分析

v0.1.1 リリース時点の E2E レイテンシ計測。
テスト画像: `pixel_photo_ramen.jpg` (2,240 KB, JPEG, C2PA署名済み)

## 計測環境

| 項目 | Local | EC2 |
|------|-------|-----|
| TEE Runtime | MockRuntime | AWS Nitro Enclave (c5.xlarge) |
| TempStorage | localhost:3001 (in-memory) | S3 ap-northeast-1 |
| Gateway | localhost:3000 | EC2:3000 (Docker) |
| Solana RPC | devnet public | devnet public |
| Client 所在地 | macOS (東京) | macOS (東京) → EC2 (ap-northeast-1) |

## E2E レイテンシ内訳（--broadcast, 1コンテンツ = 3 cNFT）

| ステップ | 処理内容 | Local (ms) | EC2 (ms) | 差分 |
|----------|----------|-----------|----------|------|
| STEP 1 | GlobalConfig 取得 + ノード発見 | 800 | 950 | +150 |
| STEP 3a | クライアント暗号化 (ECDH + AES-GCM) | 26 | 30 | +4 |
| STEP 3b | TempStorage アップロード (4MB) | 24 | 700 | **+676** |
| STEP 4 | `/verify` (TEE 処理) | 54 | 444 | **+390** |
| STEP 5 | Irys 初期化 + Arweave アップロード | 2,260 | 2,260 | 0 |
| STEP 6 | `/sign` (TX 構築 + TEE 署名) | 1,449 | 1,266 | -183 |
| STEP 7 | Solana broadcast + confirm | 734 | 692 | -42 |
| | **合計** | **~6,000** | **~7,300** | **+1,300** |

## Verify-only レイテンシ（--skip-sign）

| ステップ | Local (ms) | EC2 (ms) |
|----------|-----------|----------|
| 暗号化 | 22 | 27 |
| アップロード | 19 | 870 |
| `/verify` | 53 | 419 |
| **合計** | **~900** | **~2,300** |

## ボトルネック分析

### 1. Irys (Arweave) アップロード — 2,260ms（全体の31-38%）

E2E の最大ボトルネック。Irys SDK の初期化（balance check 含む）+ アップロードで ~2.3s。
外部サービス依存のため直接の最適化は困難。

**将来の最適化案:**
- Irys アップロードを非同期化（broadcast 後にバックグラウンドで実行し、後から cNFT metadata を更新）
- Irys bulk upload API の活用

### 2. /sign — 1,266-1,449ms（全体の17-24%）

内訳（推定）:
- Solana RPC 並列 fetch（getAccountInfo × 複数）: ~800ms
- TX 構築 + TEE 署名: ~200ms
- ALT resolve + VersionedTransaction 構築: ~200ms

Task 08 で RPC fetch を並列化済み。Task 09 で ALT 導入により TX が 1 本に集約済み。
残りのレイテンシは devnet RPC のレスポンスタイムが支配的。

**将来の最適化案:**
- 専用 RPC（Helius, Triton）で fetch レイテンシ削減
- アカウントデータのキャッシュ（GlobalConfig, Collection は頻繁に変わらない）

### 3. /verify — Local 54ms vs EC2 444ms

| 内訳 | Local | EC2 |
|------|-------|-----|
| TempStorage からのダウンロード | ~5ms (localhost) | ~150ms (S3→Enclave) |
| AES-GCM 復号 | ~3ms | ~3ms |
| C2PA 検証 (c2pa crate) | ~30ms | ~30ms |
| WASM 実行 (phash 等) | ~5ms | ~5ms |
| TEE 署名 + attestation | ~5ms | ~5ms (NSM API) |
| vsock + proxy オーバーヘッド | 0 | ~250ms |
| **合計** | **~54ms** | **~444ms** |

Nitro Enclave の vsock ブリッジ + proxy 経由の通信が ~250ms のオーバーヘッド。
これは Nitro のアーキテクチャ上不可避（ネットワーキングが vsock 経由のみ）。

### 4. S3 アップロード — 700ms

4MB の暗号化ペイロードを S3 ap-northeast-1 に PUT。
ローカル開発では in-memory TempStorage で 24ms だが、本番では S3 レイテンシが加わる。

## パイプライン全体像

```
Client                    Gateway              TEE (Enclave)         Solana
  │                         │                      │                  │
  ├─ encrypt (26ms) ───────►│                      │                  │
  ├─ upload (24-700ms) ────►│                      │                  │
  ├─ /verify ──────────────►├─ download ──────────►│                  │
  │                         │                      ├─ decrypt         │
  │                         │                      ├─ C2PA verify     │
  │                         │                      ├─ WASM exec       │
  │                         │                      ├─ sign+attest     │
  │◄── verify result ───────┤◄─────────────────────┤                  │
  ├─ Irys upload (2.3s) ───►│ (external)           │                  │
  ├─ /sign ────────────────►├─ RPC fetch ─────────────────────────────►│
  │                         │◄─────────────────────────────────────────┤
  │                         ├─ build TX ──────────►│                  │
  │                         │                      ├─ sign TX         │
  │◄── partial TX ──────────┤◄─────────────────────┤                  │
  ├─ broadcast ────────────────────────────────────────────────────────►│
  │◄── confirmed ──────────────────────────────────────────────────────┤
```

## 環境別サマリ

| メトリクス | Local (Mock) | EC2 (Nitro) |
|-----------|-------------|-------------|
| Verify-only | **~0.9s** | **~2.3s** |
| Full (broadcast) | **~6.0s** | **~7.3s** |
| TEE 処理のみ | 54ms | 444ms |
| TX サイズ | 604 bytes (v0 + ALT) | 604 bytes (v0 + ALT) |
| TX 数 | 1 | 1 |

## v0.1.0 → v0.1.1 改善

| 項目 | v0.1.0 | v0.1.1 | 改善 |
|------|--------|--------|------|
| TX 数 | 3 (core + 2 ext) | **1** (ALT) | 3→1 |
| /sign レイテンシ | ~3,600ms | **~1,350ms** | **-63%** |
| TX サイズ合計 | ~2,700 bytes | **604 bytes** | **-78%** |
| broadcast + confirm | ~2,200ms (直列) | **~700ms** (1TX) | **-68%** |

Task 08（並列 RPC fetch）と Task 09（ALT による TX 集約）により、
sign→broadcast 区間で **約 70% のレイテンシ削減** を達成。
