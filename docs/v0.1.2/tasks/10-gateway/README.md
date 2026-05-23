# タスク10: Gateway HTTP サーバー

## 目的

Gateway の HTTP サーバーを Axum で実装する。クライアント認証、TEE 情報の中継、リクエストの転送を行う。

## 読むべきファイル

1. `CLAUDE.md`
2. `docs/v0.1.2/SPECS_JA.md` — 特に:
   - §1.7 Gatewayの位置づけ（信頼モデルに関与しない薄い管理層）
   - §2.5 Gateway API（6エンドポイントの定義）
   - §5.3 Gatewayの役割（クライアント認証、TEE情報提供、リクエスト中継）
   - §5.3 TEE再起動時の挙動（鍵の再取得）
3. `docs/v0.1.2/COVERAGE.md`
4. `crates/gateway/src/lib.rs` — API型定義（Task 02 成果物）
5. `legacy/v0.1.0/crates/gateway/` — **前バージョンの Axum Gateway。エンドポイント構成、エラーハンドリング、ストレージ抽象化のパターンがある。**

## スコープ

### やること

1. **Axum HTTP サーバー**:
   - `GET /keys` — TEE の暗号化用公開鍵一覧を返す
   - `GET /processors` — 対応 processor 一覧を返す
   - `POST /process` — 属性抽出リクエストを TEE に中継
   - `GET /health` — TEE の稼働状態を返す
   - `GET /solana-keys` — Solana Extension 用公開鍵（Extension有効時のみ）
   - `POST /extension/solana` — Solana Extension リクエストの中継

2. **クライアント認証**:
   - API キーベースの認証ミドルウェア
   - レート制限

3. **TEE との通信**:
   - TEE への HTTP 内部通信
   - TEE 再起動検知 + 公開鍵の再取得
   - エラーハンドリング（TEE が停止している場合の 503 等）

4. **テスト**:
   - 各エンドポイントの integration テスト
   - 認証ミドルウェアのテスト
   - TEE 停止時のエラーハンドリングテスト

### やらないこと

- TEE 内部のリクエスト処理ロジック（Task 04）
- 暗号化ペイロードの処理（Task 11）
- Solana Extension のロジック（Task 12）

## 依存

- Task 02: Gateway API 型定義
- Task 04: TEE オーケストレーション（中継先）

## 成功基準

- [ ] 6つのエンドポイントが動作する
- [ ] クライアント認証が機能する
- [ ] TEE へのリクエスト中継が動作する
- [ ] TEE 再起動時に鍵が更新される
- [ ] `cargo test` 合格
- [ ] COVERAGE.md 更新
