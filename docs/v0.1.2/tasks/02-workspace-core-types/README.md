# タスク02: ワークスペース構築 + コア型 + トレイト定義

## 目的

Title Protocol v0.1.2 本実装の第一歩。全後続タスクの基盤となるワークスペース構造、型定義、トレイトを確立する。

Task 01（サンドボックス技術検証）で3領域の技術的実現性を確認済み:

- Sandbox A: c2pa-rs HTTP Range Request（§5.2 単一ファイル取得）→ PASS
- Sandbox B: c2pa-rs CMAF Fragment（§5.2 フラグメント取得）→ PASS
- Sandbox C: SP1 zkVM Attestation Document 検証（§6.2 ZK proof）→ PASS

## 読むべきファイル

1. `CLAUDE.md` — プロジェクト規約（Key Design Decisions にクレート構成方針あり）
2. `docs/v0.1.2/SPECS_JA.md` — 全文。特に:
   - §1.3 Processor実行フレームワーク
   - §2.2 リクエスト形式（3種の入力タイプ）
   - §2.3 レスポンス形式（signature_hash + results + attestation）
   - §3.1 Processor規約（trait定義の根拠）
   - §5.2 TEE起動シーケンス + リクエスト処理フロー
3. `docs/v0.1.2/COVERAGE.md` — 現在の実装状況
4. `legacy/v0.1.0/crates/` — **設計で迷ったらまずここを読め。** 同じプロトコルの前バージョン:
   - `types/` — 型定義・データモデルの設計判断
   - `tee/` — TeeRuntime trait の前身、vendor 分離パターン
   - `core/` — c2pa-rs 統合、processor 実行パターン
   - `gateway/` — Axum サーバー構成
   - `crypto/` — 暗号プリミティブ（AES-GCM, HKDF, X25519）
   - ワークスペース構成: `legacy/v0.1.0/Cargo.toml`

## スコープ

### やること

1. **Cargo ワークスペース構築** — `Cargo.toml`（ワークスペースルート）+ クレート構造:
   - `crates/core/` — Processor trait、リクエスト/レスポンス型、入力タイプ enum、エラー型
   - `crates/tee/` — TeeRuntime trait、Attestation 抽象化、`vendor-aws` feature flag
   - `crates/gateway/` — スケルトン（型定義のみ、実装は後続タスク）

2. **コア型定義（§2.2, §2.3 に完全準拠）**:
   - `ProcessRequest` — `input_type`, `content_url`, `processor_ids`, `encryption`（Option）等
   - `ProcessResponse` — `signature_hash`, `results` (HashMap), `attestation`
   - `InputType` enum — `Single`, `Fragmented`, `Sidecar`
   - 各 processor の出力型（§3.2 の JSON 構造に対応する struct）

3. **Processor trait 定義（§3.1）**:
   - `trait Processor` — `id()`, `process()` メソッド
   - 入力: コンテンツデータへのアクセス手段
   - 出力: `ProcessorOutput`（status + processor固有データ）
   - エラー時も他 processor に影響しない設計

4. **TeeRuntime trait 定義（§5.2）**:
   - `trait TeeRuntime` — `get_attestation_document(user_data)`, `random_bytes()` 等
   - `#[cfg(feature = "vendor-aws")]` で AWS Nitro 実装のスケルトン

5. **テスト** — 型の serialize/deserialize が仕様の JSON 例と一致することの検証

### やらないこと

- Processor の実装（c2pa-verify 等は Task 03 以降）
- Gateway の HTTP サーバー実装
- メモリ管理（ResourcePool/Ticket は後続タスク）
- 暗号化関連の実装（encryption フィールドの型だけ定義、ロジックはしない）
- Solana Extension

## 成功基準

- [ ] `cargo check --workspace` が通る
- [ ] `cargo test --workspace` が通る（型の serde テスト）
- [ ] Processor trait が §3.1 の規約を満たす設計になっている
- [ ] リクエスト/レスポンス型が §2.2, §2.3 の JSON 例と互換
- [ ] TeeRuntime trait が vendor 分離されている（feature flag）
- [ ] COVERAGE.md 更新
