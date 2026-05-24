# Title Protocol 運用ガイド (v0.1.2)

このドキュメントは Title Protocol の本番運用に必要な手順を一通り示す。仕様書 (`SPECS_JA.md`) は「何を作るか」を、本書は「どうやって動かすか」を扱う。

> 関連ドキュメント
> - 仕様: [SPECS_JA.md](./SPECS_JA.md)
> - 実装カバレッジ: [COVERAGE.md](./COVERAGE.md)

---

## 0. 全体像

Title Protocol は 3 つの実行主体で構成される。

```
       ┌──────────┐                ┌───────────┐               ┌─────────┐
Client │ External │  HTTPS         │ AWS Nitro │     vsock     │ Gateway │  HTTPS
──────▶│ Storage  │◀─── fetch ────│ Enclave   │◀────proxy────▶│ (EC2)   │◀──── Client
       │ (R2 等)  │                │  (TEE)    │               │         │
       └──────────┘                └─────────┘               └─────────┘
                                         │
                                         │ 起動時/事前作業:
                                         │   Attestation → SP1 ZK proof
                                         ▼
                                   ┌──────────────┐
                                   │   Solana     │
                                   │ title-       │
                                   │ whitelist    │
                                   └──────────────┘
```

| コンポーネント | 役割 | 仕様参照 |
|---|---|---|
| Gateway | クライアント認証・TEE 情報中継 | §5.3 |
| TEE | C2PA 検証 + 属性抽出 + Attestation 封印 | §5.2 |
| External Storage | クライアントが管理。コンテンツの実体置場 | §0.4 |
| Solana `title-whitelist` | TEE 署名鍵の許可リスト・cNFT 信頼根拠 | §6.2 |

運用者の責任範囲は **Gateway + TEE + Solana コントラクト** の 3 つ。External Storage はユーザー（プロトコル利用者）の責任。

---

## 1. ライフサイクル全体

Title Protocol が動き出すまでの依存関係を時系列で示す。

```
[1] 検証回路の同一性指定                  ── プロトコル運営者(1回)
       SP1 guest をビルド → vkey_hash 取得
                  │
                  ▼
[2] TEE バイナリの同一性指定              ── プロトコル運営者(リリース毎)
       TEE バイナリをビルド (EIF)
       Nitro 上で起動 → 自己 Attestation 取得 → measurement 抽出
                  │
                  ▼
[3] Solana 許可リスト登録                ── プロトコル運営者(リリース毎)
       add_approved_vkey(vkey_hash)
       add_approved_measurement(measurement)
                  │
                  ▼
[4] TEE 署名鍵を whitelist 登録          ── TEE 起動毎(90日)
       TEE 起動 → Solana 署名鍵生成
       Attestation 取得 → SP1 proof 生成
       register_key 提出 → WhitelistEntry PDA 作成
                  │
                  ▼
[5] cNFT 発行                            ── アプリ利用毎
       Client → Gateway → TEE → 部分署名 TX
       Client が最終署名してブロードキャスト
```

このうち `[1]–[3]` は不変（コードを変えるときだけ再実行）、`[4]` は TEE 起動毎、`[5]` がアプリの日常利用。

---

## 2. デプロイ手順

### 2.1 前提

| 項目 | バージョン/値 |
|---|---|
| Rust toolchain | `rust-toolchain.toml` で固定 (1.93.1) |
| Anchor CLI | 0.30.1 |
| Solana CLI | 3.x |
| SP1 toolchain | v6.2 (`cargo install --git https://github.com/succinctlabs/sp1 sp1up` → `sp1up`) |
| Docker | 24+ (ローカル開発のみ) |
| AWS CLI + Nitro CLI | EC2 デプロイ時 |

### 2.2 Solana コントラクトのデプロイ

> 既存 program ID: `43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs`（devnet にデプロイ済み、admin authority = `wrVwsTuRzbsDutybqqpf9tBE7JUqRPYzJ3iPUgcFmna`）

**初回デプロイ**:

```bash
anchor build --no-idl    # IDL ビルドは anchor 0.30.1 と新 proc-macro2 の非互換で失敗するので skip
                          # 参考: docs/v0.1.0/tasks/12-e2e-local-dev/solana-build-notes.md

solana program deploy \
  --url <devnet|mainnet-beta> \
  --keypair <admin keypair path> \
  --upgrade-authority <admin keypair path> \
  --program-id programs/title-whitelist/keypair.json \
  programs/title-whitelist/target/deploy/title_whitelist.so
```

**アップグレード** も同じコマンドで実行可能（既存 program ID への upgrade として処理される）。

### 2.3 許可レジストリの初期化

両方の PDA（`ApprovedVkeys`, `ApprovedMeasurements`）を作る。プログラムデプロイ後に **1 回だけ** 実行。

```bash
cargo test -p title-solana --test devnet_whitelist initialize_registries_devnet -- --ignored --nocapture
```

成功すると 2 つの PDA が作成される:
- seeds `[b"approved_vkeys"]` → `ApprovedVkeys` (vkey_hash の集合)
- seeds `[b"approved_measurements"]` → `ApprovedMeasurements` (TEE measurement の集合)

冪等: 既に初期化済みなら "already in use" を検知してスキップする。

### 2.4 SP1 guest ビルドと vkey_hash 取得

検証回路（Attestation を検証する Rust プログラム）の指紋を取得する。これを変えると検証回路が変わる。

```bash
cd sp1-guests/attestation-aws-nitro/host
cargo run --bin vkey
# stdout: 0x<32 byte hex>
```

得られた hex を `add_approved_vkey` で Solana に登録する（admin only）:

```rust
// crates/solana/tests/devnet_whitelist.rs の add_placeholder_vkey_devnet を参考に、
// placeholder バイト列を本物の vkey_hash に置換して実行
```

> 開発中はテスト用の placeholder（`[0xAA; 32]`）が登録されている。**本番ローンチ前に必ず本物の vkey_hash に差し替える**。

### 2.5 TEE バイナリのビルドと measurement 取得

> ⚠️ **本番運用では `ENCLAVE_DEBUG=1` を絶対に設定しないこと。**
> debug-mode で起動した enclave は NSM が PCR0/PCR1/PCR2 をすべて 0
> で返すため、本物の measurement との照合が不可能になる。誤って
> `[0u8; 48]` を `add_approved_measurement` 経由で登録した場合、誰でも
> 自前 AWS アカウントで debug-mode enclave を立てて on-chain 承認を
> 取れる状態になる（mock runtime と同値の信頼レベルに転落する）。
> `deploy/aws/scripts/run-stack.sh` は `ENCLAVE_DEBUG=1` のとき
> stderr に `WARNING: ENCLAVE_DEBUG=1 — Attestation Documents from this
> enclave will have zeroed PCRs.` を出すが、見落とし防止のため
> 本番ホストでは環境変数自体を残さない運用を推奨する。

> ⚠️ **この章は AWS Nitro EC2 上での実機検証後に内容を追記する**（プレースホルダー）。
>
> 現時点で確定している段取りは以下:
>
> 1. ローカルで Docker ビルド（リプロデューシブルビルドの担保、§5.4）
>    ```bash
>    docker compose build tee
>    ```
> 2. EIF（Enclave Image File）にパッケージング
>    ```bash
>    nitro-cli build-enclave --docker-uri title-protocol-tee:latest \
>                            --output-file title-tee.eif
>    # 出力に PCR0/PCR1/PCR2 が含まれる
>    ```
> 3. EC2 起動オプションで Nitro Enclave 有効化、ホストに vsock proxy を構成
> 4. EIF を起動
>    ```bash
>    nitro-cli run-enclave --eif-path title-tee.eif \
>                          --memory 2048 --cpu-count 2
>    ```
> 5. Enclave 内で TEE が起動 → 自己 Attestation 取得 → ログに measurement (PCR0) が出力される
>
> 詳細な手順・トラブルシューティングは実機検証後に追記。

### 2.6 measurement の登録

EIF ビルドで得た PCR0 を Solana に登録する（admin only）:

```rust
// crates/solana/tests/devnet_whitelist.rs の add_placeholder_measurement_devnet を参考に、
// placeholder バイト列を本物の PCR0 (48 バイト) に置換して実行
```

> 開発中は placeholder（`[0xBB; 48]`）が登録されている。**本番ローンチ前に本物の PCR0 に差し替える**。

### 2.7 Gateway のデプロイ

```bash
docker compose up gateway -d
# または独立 EC2 上で直接バイナリ起動
TEE_ENDPOINT=http://<tee endpoint>:4000 \
  API_KEYS=<comma-separated keys> \
  GATEWAY_BIND_ADDR=0.0.0.0:3000 \
  ./title-gateway
```

| 環境変数 | デフォルト | 説明 |
|---|---|---|
| `TEE_ENDPOINT` | `http://localhost:4000` | TEE 内部 HTTP URL |
| `API_KEYS` | (空 = 認証無効) | カンマ区切りの API キー |
| `RATE_LIMIT_MAX` | 100 | ウィンドウあたりリクエスト上限 |
| `RATE_LIMIT_WINDOW_SECS` | 60 | レート制限ウィンドウ秒 |
| `HEALTH_CHECK_INTERVAL_SECS` | 10 | TEE health 監視間隔 |
| `GATEWAY_BIND_ADDR` | `0.0.0.0:3000` | バインドアドレス |

`API_KEYS` を空のままにすると認証が無効化される。本番では外側（ALB / WAF / VPC）で認証する設計であれば未設定でよい。

---

## 3. TEE 起動時の自動処理

TEE バイナリ（`title-tee`）は起動時に以下を自動実行する（実装: `crates/tee/src/main.rs`）。

```
1. ランタイム選択 (TEE_RUNTIME=mock|nitro)
        │
        ▼
2. 暗号化用鍵束 (KeyBundle) 生成 — X25519 + P-256 + ML-KEM-768
        │
        ▼
3. Solana Extension 用 Ed25519 署名鍵生成
        │
        ▼
4. Processor 登録 (c2pa-verify)
        │
        ▼
5. ResourcePool 初期化
        │
        ▼
6. ★ 自己 Attestation Document 取得 → measurement 抽出 → メモリ保持
   失敗時は起動中止 (Spec §5.2)
        │
        ▼
7. HTTP サーバー起動 (0.0.0.0:4000)
```

以降、`/extension/solana` のリクエスト処理時に「相手の Attestation の measurement が自分の measurement と一致するか」を確認する。

---

## 4. SP1 proof の生成（TEE 起動毎）

TEE が起動すると新しい Solana 署名鍵が生成される。これを Solana の whitelist に登録するため、Attestation Document から SP1 proof を生成する。

```bash
# Step 1: TEE 内の Attestation Document をダンプ
#   実装方法は §2.5 と同じ手順で取得する（EIF 内のスクリプトで /dev/nsm 経由で取得）
#   出力: <tee>.attestation.bin

# Step 2: ホスト側（普通の Linux サーバー）で SP1 proof を生成
cd sp1-guests/attestation-aws-nitro/host
cargo run --release --bin prove -- <tee>.attestation.bin
# 約 90 分。出力:
#   <tee>.attestation.proof.bin
#   <tee>.attestation.public_values.bin
#   <tee>.attestation.vkey_hash.hex
```

> proof 生成は **TEE の外** で動く。SP1 prover は zkVM 上で「正規の検証回路を実行した」ことを zk で証明するため、ホスト機の信頼は不要。

### Step 3: Solana へ register_key 提出

`register_key` 命令を構築して提出する（実装は `crates/solana/tests/devnet_whitelist.rs::build_register_key_ix` を参照）。

オンチェーンで以下が順に確認される（Spec §6.2）:
1. `sp1_vkey_hash` が `ApprovedVkeys` に含まれる
2. SP1 Groth16 proof が数学的に有効
3. `measurement` が `ApprovedMeasurements` に含まれる
4. `user_data_hash == SHA-256(SHA-256(signing_pubkey))`

全て通過すると `WhitelistEntry` PDA が作成され、90 日間有効な署名鍵として登録される。

---

## 5. クライアント quickstart

cNFT 発行までの最小フロー（クライアント実装観点）。

### 5.1 平文リクエスト（暗号化なし）

```typescript
// 1. Gateway から TEE 情報を取得
const keys = await fetch(`${GATEWAY}/keys`).then(r => r.json());
const solanaPubkey = (await fetch(`${GATEWAY}/solana-keys`).then(r => r.json())).solana_pubkey;

// 2. コア処理リクエスト
const coreResp = await fetch(`${GATEWAY}/process`, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({
    input_type: "single",
    content_url: "https://your-storage/photo.jpg",
    processor_ids: ["c2pa-verify"],
  }),
}).then(r => r.json());

// coreResp = { signature_hash, results, attestation }
// オフチェーンストレージに保存し URL を得る
const offchainUrl = await uploadToYourStorage(JSON.stringify(coreResp));

// 3. Solana Extension で cNFT を発行
const extResp = await fetch(`${GATEWAY}/extension/solana`, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({
    offchain_data_url: offchainUrl,
    payer: yourWallet.publicKey.toBase58(),
    merkle_tree: yourTreeAddress.toBase58(),
    recent_blockhash: (await connection.getLatestBlockhash()).blockhash,
    collection: yourCollectionAddress?.toBase58(),
  }),
}).then(r => r.json());

// 4. partial_tx を base64 decode → 自ウォレットで最終署名 → broadcast
const partialTx = VersionedTransaction.deserialize(
  Buffer.from(extResp.partial_tx, "base64")
);
partialTx.sign([yourWallet]);
await connection.sendRawTransaction(partialTx.serialize());
```

### 5.2 暗号化リクエスト（input_type=single のみ）

仕様 §2.4 に従う。本書では概略のみ。

```text
1. クライアント: コンテンツから signature_hash をローカル算出
2. クライアント: GET /keys で TEE 公開鍵取得
3. クライアント: ペイロード組立 ([4B len][JSON metadata][raw content])
4. クライアント: スイート選択 → KEM 暗号化 → wire format で自社ストレージにアップロード
5. クライアント: POST /process に encryption: "x25519" を付けて送信
6-10. TEE 内処理（復号 → C2PA 検証 → signature_hash 突合 → 処理 → response 暗号化）
11. クライアント: nonce+ciphertext で返ってきたレスポンスを response_key で復号
12. クライアント: レスポンスの signature_hash がローカル値と一致するか確認
```

> 現状クライアント SDK は提供していない。`crates/crypto/src/sealed_channel.rs` を読めば 80 行程度で実装できる。SDK 化はロードマップ。

---

## 6. 鍵・許可リストのライフサイクル

### 6.1 TEE 再起動

TEE を再起動するたびに以下が新しくなる:
- 暗号化用鍵束（KeyBundle）→ Gateway が自動再取得（health check）
- Solana 署名鍵 → 新規 `register_key` 提出が必要

Gateway は health check で TEE の鍵変更を検知し、`/keys` のキャッシュを自動更新する（`crates/gateway/src/state.rs::check_and_refresh`）。

新しい Solana 署名鍵の whitelist 登録は手動オペレーション:
1. TEE 内で新しい Attestation を取得
2. SP1 proof を生成（90 分）
3. `register_key` 提出

ホワイトリストには鍵が増えていく一方で問題ない（Spec §6.2 末尾）。

### 6.2 TEE バイナリ更新

新しい TEE バイナリをリリースする時:
1. 新 EIF をビルド → 新 PCR0 を取得
2. `add_approved_measurement(new_pcr0)` を admin が実行
3. 新 TEE を起動 → 新しい署名鍵で `register_key` 提出（上記 6.1 と同じ）
4. 古い TEE を停止
5. （任意）`remove_approved_measurement(old_pcr0)` で旧バイナリを deprecate

過渡期は新旧 measurement の両方を許容できる。

### 6.3 SP1 guest 更新

検証回路の Rust コードを変更した時:
1. `cargo run --bin vkey` で新 vkey_hash を取得
2. `add_approved_vkey(new_vkey_hash)` を admin が実行
3. 以降、新 vkey で生成された proof を受理
4. （任意）`remove_approved_vkey(old_vkey_hash)` で旧 guest を deprecate

### 6.4 緊急時の鍵取り消し（revoke）

侵害が疑われる TEE 署名鍵を緊急取り消し:

```rust
WhitelistInstruction::RevokeKey { signing_pubkey: <to_revoke> }
```

admin のみ実行可能。取り消し操作は WhitelistEntry PDA を **close せず** `revoked = true` フラグを立てるだけ。これは取り消し巻き戻し攻撃を防ぐためで、PDA を close すると同じ proof を再投入して鍵を復活させられてしまう（Spec §6.2 「ホワイトリスト鍵の取り消し」参照）。

取り消し前に発行された cNFT はチェーン上に残るが、アプリ側で「現在 `revoked == false` か」を判定する設計なら以降は信頼されなくなる。

---

## 7. 開発者ローカル環境

実 TEE がなくても動作確認できる mock モード:

```bash
docker compose up --build
./docker/smoke-test.sh
```

| 起動内容 | デフォルトポート |
|---|---|
| TEE (mock runtime) | 4000 |
| Gateway | 3000 |

mock runtime は `MockAttestationVerifier` とペアで動き、`"mock-attestation:"` プレフィックス付きのバイト列を Attestation として扱う。実 PCR や AWS 証明書チェーン検証は走らないので、本番経路の構造確認用。

---

## 8. トラブルシューティング

### `anchor build` が `source_file` エラーで失敗する

```
error[E0599]: no method named `source_file` found for struct `proc_macro2::Span`
```

anchor 0.30.1 と最新 proc-macro2 の非互換。`anchor build --no-idl` で IDL ビルドをスキップする。

参考: [docs/v0.1.0/tasks/12-e2e-local-dev/solana-build-notes.md](../v0.1.0/tasks/12-e2e-local-dev/solana-build-notes.md)

### `register_key` が `VkeyNotApproved (6007)` で reject される

`ApprovedVkeys` PDA に該当の `sp1_vkey_hash` が登録されていない。`add_approved_vkey` で追加する（admin only）。

### `register_key` が `MeasurementNotApproved (6010)` で reject される

`ApprovedMeasurements` PDA に該当 measurement が登録されていない。`add_approved_measurement` で追加する。

### TEE 起動時に "Self-attestation failed" で停止する

自己 Attestation の取得に失敗している:
- `TEE_RUNTIME=nitro` でローカル実行している（Nitro Enclave 外では NSM デバイスがない）→ ローカルでは `TEE_RUNTIME=mock` を使う
- 実 Nitro 上で発生 → `/dev/nsm` が利用可能か確認 (`nitro-cli describe-enclaves`)

このエラーで停止するのは仕様（Spec §5.2）。自己 measurement なしで起動すると Solana Extension の measurement 一致確認が無効化されるため、fail-fast している。

### Gateway が "TEE unavailable" を返し続ける

`TEE_ENDPOINT` が誤っているか、TEE 側で health check が失敗している。
- `curl ${TEE_ENDPOINT}/health` を直接叩いて応答確認
- TEE のログで起動完了しているか確認

### SP1 proof 生成が OOM で死ぬ

prover は Groth16 wrap でピーク約 30 GiB を要する。RAM 64 GiB 以上のホスト (EC2 `r5.4xlarge` 以上) を推奨。詳細と理由は `sp1-guests/README.md` を参照。

---

## 9. ロードマップ

- [ ] AWS Nitro Enclave 上での実機検証（§2.5, §2.6 の実例埋め込み）
- [ ] クライアント SDK (TypeScript)
- [ ] Range Request 対応の大容量コンテンツ fetch（Spec §4.3 ピーク最適化）
- [ ] 追加 processor (provenance-graph, image-pdq, video-vpdq, cert-google/sony/leica)
- [ ] mainnet-beta へのコントラクトデプロイ + admin 多重署名化
