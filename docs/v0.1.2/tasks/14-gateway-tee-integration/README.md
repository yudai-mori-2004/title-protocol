# タスク14: Gateway ↔ TEE 統合

## 目的

Gateway と TEE を実際に HTTP で接続し、クライアントから Gateway 経由で TEE にリクエストを送って処理結果を受け取る E2E フローを完成させる。

Task 10 で構築した Gateway HTTP サーバーの TeeClient trait に対して、実際の HTTP 通信を行う HttpTeeClient を実装し、Gateway の main.rs バイナリを作成する。

## 読むべきファイル

1. `CLAUDE.md`
2. `docs/v0.1.2/SPECS_JA.md` — 特に:
   - §2.1 リクエストの流れ（Client → Gateway → TEE → External Storage）
   - §1.7 Gateway の位置づけ（信頼モデルに関与しない薄い管理層）
   - §5.3 Gateway の役割（クライアント認証、TEE 情報提供、リクエスト中継）
   - §5.3 TEE 再起動時の挙動（鍵の再取得）
3. `docs/v0.1.2/COVERAGE.md`
4. `crates/gateway/src/tee_client.rs` — TeeClient trait + HttpTeeClient スタブ（Task 10 成果物）
5. `crates/gateway/src/state.rs` — TeeInfoCache, refresh_tee_info（Task 10 成果物）
6. `crates/gateway/src/server.rs` — router 構成、ミドルウェア（Task 10 成果物）
7. `docs/v0.1.2/tasks/13-tee-binary/README.md` — TEE バイナリ仕様（Task 13）
8. `legacy/v0.1.0/crates/gateway/src/main.rs` — **前バージョンの Gateway 起動。TEE_ENDPOINT 環境変数。**
9. `legacy/v0.1.0/crates/gateway/src/auth.rs` — **GatewayAuthWrapper: Gateway → TEE リクエスト署名。**

## スコープ

### やること

1. **HttpTeeClient 実装**:
   - reqwest ベースの HTTP クライアント
   - TEE エンドポイント URL を設定で受け取る
   - `/health`, `/keys`, `/processors`, `/solana-keys` への GET リクエスト
   - `/process`, `/extension/solana` への POST リクエスト中継
   - エラーハンドリング（接続失敗、タイムアウト、TEE 側エラー）

2. **GatewayAuth（Gateway → TEE リクエスト認証）**:
   - Gateway が Ed25519 鍵ペアを保持
   - TEE への中継時にリクエストを署名で wrap
   - TEE 側でリクエストの署名を検証
   - dev モードでは署名スキップ可能

3. **Gateway main.rs（起動シーケンス）**:
   - 環境変数から設定読み込み（TEE_ENDPOINT, API_KEYS 等）
   - HttpTeeClient 初期化
   - GatewayState 構築
   - TEE 情報の初回取得（refresh_tee_info）
   - Axum サーバー起動（0.0.0.0:3000）

4. **E2E テスト**:
   - Gateway + TEE を同時起動
   - Client → Gateway → TEE → External Storage → レスポンスの全フロー
   - TEE 再起動シナリオ（Gateway が鍵を再取得するか）
   - API キー認証の検証

### やらないこと

- Proxy（Nitro Enclave 固有の vsock ブリッジ）
- Nitro runtime（mock で動作確認）
- クライアント SDK

## 依存

- Task 10: Gateway HTTP サーバー
- Task 13: TEE バイナリ

## 成功基準

- [ ] `cargo run -p title-gateway` で Gateway が起動する
- [ ] Gateway が TEE に HTTP で接続し、/health で稼働確認できる
- [ ] Client → Gateway `/process` → TEE → ProcessResponse が返る
- [ ] Client → Gateway `/extension/solana` → TEE → partial_tx が返る
- [ ] TEE 再起動後に Gateway が鍵を再取得する
- [ ] `cargo test` 合格
- [ ] COVERAGE.md 更新
