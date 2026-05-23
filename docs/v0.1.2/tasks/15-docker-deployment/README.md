# タスク15: Docker デプロイメント

## 目的

Gateway + TEE (mock) を `docker compose up` 一発で起動できるようにし、ローカル開発環境とリプロデューシブルビルドの基盤を構築する。

## 読むべきファイル

1. `CLAUDE.md`
2. `docs/v0.1.2/SPECS_JA.md` — 特に:
   - §5.1 構成（Gateway + TEE の 2 コンポーネント）
   - §5.4 リプロデューシブルビルド（Dockerfile, Cargo.lock, toolchain pinning）
3. `docs/v0.1.2/COVERAGE.md`
4. `docs/v0.1.2/tasks/13-tee-binary/README.md` — TEE バイナリ仕様
5. `docs/v0.1.2/tasks/14-gateway-tee-integration/README.md` — Gateway ↔ TEE 接続仕様
6. `legacy/v0.1.0/docker/` — **前バージョンの Dockerfile 群。tee-mock, gateway, proxy の multi-stage ビルド。**
7. `legacy/v0.1.0/deploy/local/docker-compose.yml` — **ローカル開発用 compose。**
8. `legacy/v0.1.0/deploy/aws/docker-compose.production.yml` — **本番構成の参考。**

## スコープ

### やること

1. **Dockerfile（TEE mock）**:
   - Multi-stage ビルド（builder + runtime）
   - Builder: Rust toolchain + 依存ビルド
   - Runtime: 最小ベースイメージ + libssl + ca-certificates
   - Entrypoint: `title-tee`
   - Port: 4000
   - 環境変数: `TEE_RUNTIME=mock`

2. **Dockerfile（Gateway）**:
   - Multi-stage ビルド
   - Entrypoint: `title-gateway`
   - Port: 3000
   - 環境変数: `TEE_ENDPOINT`, `API_KEYS`

3. **docker-compose.yml（ローカル開発）**:
   - `tee` サービス（mock runtime、port 4000）
   - `gateway` サービス（port 3000、TEE_ENDPOINT=http://tee:4000）
   - ヘルスチェック設定
   - 起動順序（TEE → Gateway）

4. **リプロデューシブルビルド対応**:
   - Cargo.lock をコンテナにコピー（依存バージョン固定）
   - rust-toolchain.toml で Rust バージョン固定
   - ビルドキャッシュの最適化（依存を先にビルド）

5. **スモークテスト**:
   - `docker compose up` で Gateway + TEE が起動する
   - `curl localhost:3000/health` でステータスが返る
   - `curl localhost:3000/keys` で暗号化用公開鍵が返る
   - CI 用スモークテストスクリプト

### やらないこと

- Nitro Enclave 用ビルド（EIF 生成）
- Proxy コンテナ（Nitro 固有）
- 本番 AWS デプロイメント
- CD パイプライン

## 依存

- Task 13: TEE バイナリ
- Task 14: Gateway ↔ TEE 統合

## 成功基準

- [ ] `docker compose up` で Gateway + TEE が起動する
- [ ] Gateway → TEE の HTTP 通信が動作する
- [ ] `docker compose down && docker compose up` で再起動しても動作する
- [ ] Dockerfile が multi-stage で最適化されている
- [ ] Cargo.lock + rust-toolchain.toml でビルドが再現可能
- [ ] COVERAGE.md 更新
