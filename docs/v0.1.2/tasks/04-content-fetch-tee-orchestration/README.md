# タスク04: コンテンツ取得 + TEE オーケストレーション

## 目的

TEE のリクエスト処理フロー（§5.2）を実装する。コンテンツの取得（3入力形式対応）、processor の並列実行、結果の組み立て、Attestation Document の取得までの一連のパイプラインを構築する。

Task 03 の c2pa-verify processor と Task 02 の型・trait をつなぐ接着層。

## 読むべきファイル

1. `CLAUDE.md` — プロジェクト規約
2. `docs/v0.1.2/SPECS_JA.md` — 特に:
   - §5.2 TEE起動シーケンス + リクエスト処理フロー
   - §5.2 コンテンツ取得の詳細（single / fragmented / sidecar）
   - §1.2 Attestation Documentの役割（user_data = ハッシュ）
   - §1.5 検証モデル（JCS + hash comparison）
   - §2.3 レスポンス形式（signature_hash の位置づけ）
3. `docs/v0.1.2/COVERAGE.md`
4. `crates/core/` — ProcessRequest, ProcessResponse, ProcessorRegistry（Task 02）
5. `crates/tee/` — TeeRuntime trait（Task 02）
6. `sandbox/01-c2pa-range-request/` — **HTTP Range Request の実装パターン。http-range-client クレートの使い方。**
7. `sandbox/02-c2pa-fragment/` — **CMAF フラグメント処理パターン。**
8. `legacy/v0.1.0/crates/tee/src/endpoints/verify/` — **前バージョンのリクエスト処理フロー。**

## スコープ

### やること

1. **コンテンツ取得層**:
   - `single`: HTTP GET（小ファイル）/ HTTP Range Request（大ファイル、Read+Seek アダプタ）
   - `fragmented`: init.mp4 + seg-*.m4s の順次取得
   - `sidecar`: マニフェスト + コンテンツの個別取得
   - ETag による一貫性保証（Range Request 時の If-Match ヘッダ）

2. **TEE リクエスト処理フロー**:
   - `ProcessRequest` を受信
   - 入力形式に応じたコンテンツ取得
   - `c2pa-verify` の暗黙追加（processor_ids に未指定でも実行）
   - ProcessorRegistry 経由で processor を実行（将来的に並列化）
   - signature_hash をレスポンスのトップレベルに配置
   - 全 processor の結果を `VerifiableResponse` にまとめる

3. **Attestation Document 統合**:
   - `VerifiableResponse` を JCS 正規化
   - SHA-256 ハッシュ算出
   - `TeeRuntime::get_attestation_document(hash)` で Attestation 取得
   - `ProcessResponse` を構築して返却

4. **テスト**:
   - モック HTTP サーバー + モック TeeRuntime でのエンドツーエンドテスト
   - 各入力形式のコンテンツ取得テスト
   - JCS ハッシュの決定論性テスト
   - c2pa-verify 未指定時の暗黙追加テスト

### やらないこと

- 暗号化ペイロードの復号（Task 11）
- メモリ管理（ResourcePool / Ticket — Task 09）
- Gateway の HTTP サーバー（Task 10）
- processor の並列実行の最適化（初期は逐次実行で可）

## 依存

- Task 02: 型・trait
- Task 03: c2pa-verify processor

## 成功基準

- [ ] 3入力形式でコンテンツを取得できる
- [ ] ProcessRequest → ProcessResponse のパイプラインが動作する
- [ ] VerifiableResponse の JCS ハッシュが Attestation Document の user_data にバインドされる
- [ ] `cargo test` で全テスト合格
- [ ] COVERAGE.md 更新
