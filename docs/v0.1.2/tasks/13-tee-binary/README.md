# タスク13: TEE バイナリ + Mock Runtime

## 目的

TEE を単独で起動可能なバイナリとして実装する。mock runtime で鍵生成・Attestation Document 取得を行い、Axum HTTP サーバーでリクエストを受け付け、既存の orchestrator パイプラインを呼び出して処理結果を返す。

Task 02 で定義した TeeRuntime trait の mock 実装と、Task 04 で構築した orchestrator パイプラインを、実際に動く HTTP サーバーとして結合する。

## 読むべきファイル

1. `CLAUDE.md`
2. `docs/v0.1.2/SPECS_JA.md` — 特に:
   - §5.2 TEE 起動シーケンス（鍵生成 → Gateway 通知 → リクエスト受付）
   - §5.2 リクエスト処理フロー（コンテンツ取得 → Processor → Attestation → 返却）
   - §2.5 Gateway API（TEE が実装すべき内部エンドポイント群）
   - §6.2 Solana Extension 起動時処理（Ed25519 署名鍵生成）
3. `docs/v0.1.2/COVERAGE.md`
4. `crates/tee/src/lib.rs` — TeeRuntime trait（Task 02 成果物、現在 trait 定義のみ）
5. `crates/tee/src/orchestrator.rs` — process_request パイプライン（Task 04 成果物）
6. `crates/tee/src/content_fetch.rs` — コンテンツ取得層（Task 04 成果物）
7. `crates/tee/src/resource_pool.rs` — メモリ管理（Task 09 成果物）
8. `crates/crypto/src/key_bundle.rs` — 暗号化用鍵束の生成（Task 11 成果物）
9. `crates/solana/src/signing_key.rs` — Solana 署名鍵の生成（Task 12 成果物）
10. `legacy/v0.1.0/crates/tee/src/main.rs` — **前バージョンの TEE 起動シーケンス。State 初期化、Axum サーバー構成、ルーティングの参考。**
11. `legacy/v0.1.0/crates/tee/src/runtime/` — **TeeRuntime の mock / nitro 実装パターン。**

## スコープ

### やること

1. **Mock TeeRuntime 実装**:
   - `generate_random_bytes()` — OsRng ベースの乱数生成
   - `get_attestation_document(user_data)` — `"mock-attestation:" + user_data` 形式の疑似 Attestation
   - テスト用の PCR 値、モジュール ID を返す

2. **TEE Application State**:
   - TeeRuntime インスタンス
   - KeyBundle（暗号化用鍵束、§2.4）
   - SolanaSigningKey（Solana Extension 有効時、§6.2）
   - ProcessorRegistry（c2pa-verify 登録済み）
   - ResourcePool（メモリ管理）
   - ContentFetcher（HTTP クライアント）

3. **Axum HTTP サーバー（TEE 内部エンドポイント）**:
   - `GET /health` — 稼働状態（tee_type, uptime 等）
   - `GET /keys` — 暗号化用公開鍵一覧
   - `GET /processors` — 対応 processor 一覧
   - `POST /process` — コア処理リクエスト → orchestrator → レスポンス
   - `GET /solana-keys` — Solana 公開鍵（Extension 有効時）
   - `POST /extension/solana` — Solana Extension リクエスト

4. **TEE main.rs（起動シーケンス）**:
   - Runtime 選択（mock / 将来 nitro）
   - 鍵生成（KeyBundle + SolanaSigningKey）
   - State 構築
   - Axum サーバー起動（0.0.0.0:4000）

5. **テスト**:
   - Mock runtime の単体テスト
   - TEE サーバーの integration テスト（reqwest でリクエスト → レスポンス検証）
   - /process の E2E テスト（C2PA 署名付きコンテンツ → ProcessResponse）

### やらないこと

- Nitro runtime 実装（将来タスク）
- Proxy（vsock / TCP ブリッジ — Nitro Enclave 固有、将来タスク）
- GatewayAuth 検証ミドルウェア（Task 14 で Gateway 側と合わせて実装）

## 依存

- Task 02: TeeRuntime trait、コア型
- Task 04: orchestrator パイプライン
- Task 09: ResourcePool
- Task 11: KeyBundle（暗号化用鍵束）
- Task 12: SolanaSigningKey

## 成功基準

- [ ] `cargo run -p title-tee` で TEE サーバーが起動する
- [ ] `GET /health` がステータスを返す
- [ ] `GET /keys` が暗号化用公開鍵を返す
- [ ] `POST /process` で C2PA コンテンツの属性抽出が動作する
- [ ] レスポンスに mock Attestation Document が含まれる
- [ ] `cargo test` 合格
- [ ] COVERAGE.md 更新
