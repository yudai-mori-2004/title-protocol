# タスク03: c2pa-verify Processor 実装

## 目的

全リクエストで必須の c2pa-verify processor を実装する。C2PA署名チェーンの検証、signature_hash の算出、マニフェスト情報の抽出を行う。Title Protocol の信頼モデルの中核。

Task 02 で定義した `Processor` trait の最初の実装体。

## 読むべきファイル

1. `CLAUDE.md` — プロジェクト規約
2. `docs/v0.1.2/SPECS_JA.md` — 特に:
   - §1.3 c2pa-verifyの必須化とsignature_hash
   - §3.1 Processorの規約
   - §3.2 c2pa-verify の入出力定義
   - §2.3 レスポンス形式（signature_hashの位置）
3. `docs/v0.1.2/COVERAGE.md` — 現在の実装状況
4. `crates/core/src/processor.rs` — Processor trait（Task 02 成果物）
5. `crates/core/src/processor_outputs.rs` — C2paVerifyOutput 型（Task 02 成果物）
6. `legacy/v0.1.0/crates/core/src/lib.rs` — **前バージョンの c2pa-verify 実装。verify_c2pa(), extract_content_hash(), extract_manifest_signature() がある。ロジックの大部分をここから移植できる。**
7. `legacy/v0.1.0/crates/core/src/jumbf.rs` — JUMBF署名抽出ロジック
8. `sandbox/01-c2pa-range-request/` — c2pa-rs の使い方（Reader API, validation_state）

## スコープ

### やること

1. **c2pa-verify processor 実装**:
   - `Processor` trait の実装体 `C2paVerifyProcessor`
   - `c2pa::Reader` でコンテンツを読み込み・検証
   - `ValidationState` の判定（Valid / Trusted / Invalid）
   - Active Manifest の署名抽出 → SHA-256 → signature_hash 算出
   - 署名者情報（issuer, cert_serial）の抽出
   - タイムスタンプの抽出
   - claim_generator の抽出
   - アクション履歴（actions）の抽出
   - 結果を `C2paVerifyOutput` として構築

2. **signature_hash 算出ユーティリティ**:
   - Active Manifest の署名バイト列の取得（JUMBF パース）
   - `SHA-256(署名バイト列)` → `"sha256:hex..."` 形式の文字列化
   - このユーティリティは TEE オーケストレーション層（Task 04）からも呼ばれる

3. **テスト**:
   - テスト用 C2PA 署名済み画像の生成（c2pa::Builder）
   - 署名検証の正常系・異常系
   - signature_hash の決定論性（同一コンテンツ → 同一ハッシュ）
   - `C2paVerifyOutput` が §3.2 の JSON 例と互換
   - C2PA署名なしコンテンツへのエラーハンドリング

### やらないこと

- HTTP Range Request 経由の大容量ファイル処理（Task 04）
- フラグメント / サイドカー形式の処理（Task 04）
- 他の processor（provenance-graph 等）の実装
- TEE オーケストレーション（結果のまとめ、Attestation取得）
- メモリ管理（ResourcePool / Ticket）

## 成功基準

- [ ] `C2paVerifyProcessor` が `Processor` trait を実装している
- [ ] C2PA署名済みコンテンツから signature_hash を算出できる
- [ ] 署名者情報、タイムスタンプ、actions を抽出できる
- [ ] C2PA署名なしコンテンツで適切にエラーを返す
- [ ] `cargo test` で全テスト合格
- [ ] COVERAGE.md 更新
