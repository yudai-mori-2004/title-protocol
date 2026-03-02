# タスク6: Gateway全ハンドラ + Gateway認証

## 前提タスク

- タスク5（/sign + /create-tree）が完了していること

## 読むべきファイル

1. `docs/SPECS_JA.md` — §6.2 全体（Gateway認証、各API仕様、ノード情報公開）
2. `crates/gateway/src/main.rs` — 現在のスタブ（ルーティングのみ）
3. `crates/types/src/lib.rs` — GatewayAuthWrapper, ResourceLimits, VerifyRequest, SignRequest, NodeInfo等
4. `crates/crypto/src/lib.rs` — `ed25519_sign()`, `ed25519_verify()`（実装済み）

## 作業内容

### GatewayStateの完成

現在の `GatewayState` に以下を追加:
- `signing_key`: Ed25519秘密鍵（Gateway認証用）
- `signing_pubkey`: 対応する公開鍵
- `storage_client`: S3/MinIOクライアント（署名付きURL発行用）
- 環境変数から読み込み: `TEE_ENDPOINT`, `GATEWAY_SIGNING_KEY`, `MINIO_ENDPOINT`, `MINIO_ACCESS_KEY`, `MINIO_SECRET_KEY`

### 5つのハンドラ実装

#### POST /upload-url
- MinIO/S3への署名付きアップロードURLを生成
- `content-length-range` 条件を設定（EDoS対策）
- `aws-sdk-s3` または `rust-s3` クレートを使用

#### POST /verify
- クライアントのリクエストを受け取る
- GatewayAuthWrapper を構築（method, path, body, resource_limits）
- Gateway秘密鍵で署名を付与
- TEEエンドポイントにリレー
- TEEからのレスポンスをそのままクライアントに返す

#### POST /sign
- /verify と同様のGateway認証ラップ+リレー

#### POST /sign-and-mint
- /sign と同じだが、返却されたpartial_txにGatewayのウォレットで最終署名+ブロードキャスト
- Solana RPCへの接続が必要（環境変数: `SOLANA_RPC_URL`）

#### GET /.well-known/title-node-info
- NodeInfo（signing_pubkey, supported_extensions, limits）をJSON返却

### Gateway認証の実装

§6.2「Gateway認証」に従い:
1. Gatewayがリクエスト内容 + resource_limits を含む構造体を構築
2. JSON正規化（`serde_json::to_string` でキーソート保証、または `serde_json::to_vec`）
3. Ed25519で署名
4. `gateway_signature` フィールドを追加してTEEに転送

TEE側（`crates/tee`）にもGateway署名検証を追加:
- タスク4で「TODOコメントを残す」としたGateway署名検証ステップを実装
- Global Configの `gateway_pubkey` で検証（今はハードコードまたは環境変数でも可）

## 完了条件

- 全5エンドポイントが動作する
- Gateway認証: 正しいGateway署名を持つリクエストがTEEで受理される
- Gateway認証: 署名なし/不正署名のリクエストがTEEで拒否される
- /upload-url: MinIO（docker-compose上）に対して署名付きURLを生成できる
- /verify + /sign のE2Eテスト: Gateway経由でTEEにリクエストを送り、レスポンスが返る
- `cargo check --workspace && cargo test --workspace` が通る
- `docs/COVERAGE.md` の該当箇所を更新
