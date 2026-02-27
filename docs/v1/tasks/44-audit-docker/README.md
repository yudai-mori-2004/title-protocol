# Task 44: コード監査 — Docker

## 対象
`docker/` + `deploy/aws/docker/` + `deploy/aws/docker-compose.production.yml`

## ファイル
- `docker/gateway.Dockerfile` — Gateway (axum)
- `docker/tee-mock.Dockerfile` — TEE MockRuntime
- `docker/proxy.Dockerfile` — vsock HTTPプロキシ
- `docker/indexer.Dockerfile` — cNFTインデクサ (Node.js)
- `docker/README.md` — ドキュメント
- `deploy/aws/docker/tee.Dockerfile` — 本番Enclave用TEE (amazonlinux)
- `deploy/aws/docker/entrypoint.sh` — Enclave内socatブリッジ
- `deploy/aws/docker-compose.production.yml` — 本番オーケストレーション

## 監査で発見された問題

### バグ
1. **docker-compose.production.yml: gatewayがpostgresに依存している**:
   `gateway` サービスに `depends_on: postgres: condition: service_healthy` があるが、GatewayはPostgreSQLを使用しない。Indexerのみがpostgresを必要とする。
   → gateway から `depends_on` を削除。

### コード品質
2. **`.dockerignore` が存在しない**:
   docker-composeのcontextがrepoルート(`../..`)のため、`target/`、`node_modules/`、`.git/`等がビルドコンテキストに含まれる。
   → repoルートに`.dockerignore`を追加。

### 設計メモ（修正不要）
- Rust Dockerfiles 3つが同一パターン（bin名のみ異なる）: 十分小規模で、テンプレート化する必要なし。
- tee.Dockerfile が `.env` をイメージにベイク: Enclaveはランタイムでファイルマウント不可のため意図的。
- proxy.Dockerfile がdocker-composeにない: 本番ではホスト側で直接実行（vsockアクセスが必要）のため正しい。
- entrypoint.sh の ifconfig/ip フォールバック: 環境差異への堅実な対応。

## 完了基準
- [x] docker-compose.production.yml: gatewayのdepends_on削除
- [x] `.dockerignore` 追加

## 対処内容

### 1. gatewayのdepends_on削除
`deploy/aws/docker-compose.production.yml` の gateway サービスから `depends_on: postgres` を削除。
GatewayはPostgreSQLに依存しない。Indexerのみが正しくdepends_onを持つ。

### 2. .dockerignore追加
repoルートに `.dockerignore` を作成。`target/`、`node_modules/`、`.git/`、`docs/`、`tests/`、`*.eif`、`.env`等を除外。
ビルドコンテキストの大幅な縮小によりDockerビルドが高速化される。
