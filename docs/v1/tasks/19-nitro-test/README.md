# タスク19: Nitro Enclave 実環境テスト

## 概要

MockRuntime で動作確認済みの全フローを、AWS Nitro Enclaves の実TEE環境で実行し、
Mock → Nitro の切り替えが設計通り透過的に動作することを検証する。

現状: タスク18まで MockRuntime (`TEE_RUNTIME=mock`) で全フロー完走済み。
本タスクでは `TEE_RUNTIME=nitro` + vsockプロキシ構成で同一フローを再現する。

## 仕様書セクション

- §6.4 TEE（Nitro Enclaves、vsockプロキシ、Attestation）
- §6.1 コンポーネント構成（Enclave + Proxy + Gateway）

## 前提タスク

- タスク01〜18全完了
- タスク11（NitroRuntime実装）で `NitroRuntime` + `RealNsm` は実装済み
- タスク17（Devnetデプロイ）で Terraform / setup-ec2.sh は整備済み

## アーキテクチャ（Nitro構成）

```
Client (local) ──HTTP──→ Gateway (:3000)
                              │
                              │ HTTP
                              ▼
                         Proxy (host)
                              │
                              │ vsock (CID:16, port:4000)
                              ▼
                     ┌─── Enclave ───┐
                     │  title-tee    │
                     │  NitroRuntime │
                     │  /dev/nsm     │
                     │  WASM modules │
                     └───────────────┘
```

**Mock構成との差分:**

| 項目 | Mock | Nitro |
|------|------|-------|
| `TEE_RUNTIME` | `mock` | `nitro` |
| `PROXY_ADDR` | `direct` | `vsock:8000` |
| TEE起動方式 | `nohup title-tee` | `nitro-cli run-enclave` |
| ネットワーク | localhost直接 | vsock経由（Proxy中継） |
| 鍵生成エントロピー | `OsRng` | NSMデバイス (`/dev/nsm`) |
| Attestation | ゼロPCR値のモック | 実PCR測定値 (PCR0/1/2) |
| Proxy | 不要 | 必須（`title-proxy`） |

## 読むべきファイル

| ファイル | 理由 |
|---------|------|
| `crates/tee/src/runtime/nitro.rs` | NitroRuntime + RealNsm 実装 |
| `crates/tee/src/runtime/mod.rs` | TeeRuntime trait、Mock/Nitro 分岐 |
| `crates/tee/src/main.rs` | `TEE_RUNTIME` 環境変数による分岐 |
| `crates/proxy/src/main.rs` | vsock/TCP リスナー |
| `crates/proxy/src/protocol.rs` | Length-prefixed プロトコル |
| `docker/tee.Dockerfile` | Enclave用Dockerイメージ |
| `deploy/setup-ec2.sh` | EIFビルド + Enclave起動（Step 2-4） |
| `docs/v1/tasks/18-vendor-neutrality/NODE-SETUP-AND-REGISTRATION.md` | Mock構成の手順書（比較用） |

## 作業内容

### 1. Nitro Enclaves の有効化

EC2インスタンスで Nitro Enclaves を有効にする。
`c5.xlarge` は Enclave 対応済み。

```bash
# Nitro Enclaves CLI のインストール
sudo amazon-linux-extras install aws-nitro-enclaves-cli -y
sudo usermod -aG ne ec2-user
# allocator の設定（CPU 2, Memory 1024MB を Enclave に割り当て）
sudo vim /etc/nitro_enclaves/allocator.yaml
sudo systemctl enable --now nitro-enclaves-allocator
```

### 2. EIF ビルド

```bash
# tee.Dockerfile から Docker イメージ → EIF 変換
docker build -t title-tee-enclave -f docker/tee.Dockerfile .
nitro-cli build-enclave \
  --docker-uri title-tee-enclave:latest \
  --output-file title-tee.eif

# PCR測定値を記録（Global Config に登録する値）
nitro-cli describe-eif --eif-path title-tee.eif
```

### 3. Proxy + Enclave 起動

```bash
# Proxy 起動（ホスト側、vsock↔HTTP変換）
nohup ./target/release/title-proxy > /tmp/title-proxy.log 2>&1 &

# Enclave 起動
nitro-cli run-enclave \
  --eif-path title-tee.eif \
  --cpu-count 2 \
  --memory 1024
```

### 4. Gateway の接続先変更

Gateway の `TEE_ENDPOINT` はそのまま `http://localhost:4000`。
Proxy がホストの `:4000` で listen し、vsock 経由で Enclave に中継する。

### 5. E2E フロー検証

タスク18と同一の `register-content.mjs` を使用:

```bash
GATEWAY_URL=http://<EC2_IP>:3000 \
SOLANA_RPC_URL=<RPC_URL> \
  node scripts/register-content.mjs <image.jpg> --processor core-c2pa,phash-v1
```

### 6. Attestation Document の検証

Nitro構成では `/verify` レスポンスの `tee_attestation` に実PCR値が含まれる。
signed_json の `tee_attestation` をデコードして PCR0/1/2 を確認:

```bash
# signed_json から tee_attestation を抽出し、CBOR/COSEデコード
# PCR値が EIF ビルド時に記録した値と一致することを確認
```

## 想定される課題

### 1. Enclave 内のネットワーク

Enclave は外部ネットワークに直接アクセスできない。
全ての外部通信（S3、Solana RPC）は vsock Proxy 経由で行う。
`crates/tee/src/proxy_client.rs` の `ProxyHttpClient` がこれを担当。

Proxy 側で Enclave からのリクエストを受け取り、通常の HTTP で外部に転送する。

### 2. WASMモジュールの配置

Mock構成ではホスト上のファイルパス（`WASM_DIR`）からWASMを読み込むが、
Enclave 内ではDockerイメージに含まれたパスを使用する。
`tee.Dockerfile` に WASM モジュールの COPY を追加する必要がある可能性あり。

### 3. メモリ制限

`c5.xlarge` (8GB RAM) のうち Enclave に 1024MB を割り当てると、
ホスト側に残るメモリが限られる。
大きな画像（>5MB）の C2PA 検証 + WASM 実行でメモリ不足になる可能性。
`ENCLAVE_MEMORY_MIB` を調整する。

### 4. デバッグの困難さ

Enclave 内のログは `nitro-cli console --enclave-id <id>` でしか見えない。
クラッシュ時の情報が限られるため、まず小さなテスト画像で確認してから
大きな画像に移行する。

## 完了条件

- [ ] `nitro-cli` がインストールされ、Enclave が起動する
- [ ] EIF がビルドされ、PCR測定値が記録される
- [ ] `title-proxy` が vsock 経由で Enclave と通信できる
- [ ] Gateway → Proxy → Enclave(TEE) の疎通確認（`/health`）
- [ ] `register-content.mjs` で core-c2pa + phash-v1 が Confirmed される
- [ ] signed_json の `tee_attestation` に実PCR値が含まれることを確認
- [ ] `tee_type` が `"aws_nitro"` であることを確認（Mockでは `"mock"`）
