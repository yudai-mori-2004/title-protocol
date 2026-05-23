# タスク12: Solana Extension

## 目的

コア処理の成果物を Solana ブロックチェーン上に cNFT として記録する Solana Extension を実装する。ZK proof による TEE 署名鍵のホワイトリスト登録と、ホワイトリスト済み鍵での cNFT 部分署名を行う。

Task 01（Sandbox C: SP1 zkVM Attestation Document 検証）で技術的実現性を確認済み。

## 読むべきファイル

1. `CLAUDE.md`
2. `docs/v0.1.2/SPECS_JA.md` — **§6 全体を精読**:
   - §6.1 Extension の汎用定義（コア結果 → Extension リクエスト）
   - §6.2 準備: Ed25519署名鍵生成 → Attestation取得 → ZK proof生成 → ホワイトリスト登録
   - §6.2 利用: オフチェーンデータ fetch → Attestation検証 → cNFT mint トランザクション部分署名
   - §6.2 検証: ホワイトリスト済み署名の確認
   - §6.2 運用: 署名鍵有効期限（90日）、鍵ローテーション、ホワイトリスト削除
3. `docs/v0.1.2/COVERAGE.md`
4. `crates/gateway/src/lib.rs` — SolanaExtensionRequest/Response（Task 02 成果物）
5. `sandbox/03-sp1-attestation/` — **SP1 zkVM での Attestation Document 検証。96M cycles, Groth16 ~479B fits Solana 1,232B。program/ と attestation-verify/ のコードを本実装に統合する。**
6. `legacy/v0.1.0/crates/tee/src/blockchain/` — **前バージョンの Solana トランザクション構築。**

## スコープ

### やること

1. **TEE 側: Ed25519 署名鍵の生成と管理**:
   - Solana 用 Ed25519 署名鍵ペアの生成（TEE 起動時）
   - 秘密鍵は TEE メモリ内のみ

2. **ホワイトリスト登録（準備フェーズ）**:
   - Attestation Document 取得（user_data = SHA-256(Solana公開鍵)）
   - SP1 zkVM での ZK proof 生成（sandbox/03 のコードを統合）
   - Solana プログラムへの ZK proof 提出
   - ホワイトリスト PDA への署名鍵登録

3. **cNFT 発行（利用フェーズ）**:
   - POST /extension/solana リクエスト処理
   - オフチェーンデータの fetch + Attestation Document 検証
   - PCR 値の照合（自分のコードと一致するか）
   - user_data ハッシュの照合（処理結果と一致するか）
   - cNFT 発行トランザクション構築 + TEE 署名鍵で部分署名
   - 部分署名済みトランザクションの返却

4. **Solana プログラム（オンチェーン）**:
   - ホワイトリスト PDA の管理
   - ZK proof 検証（sp1_solana crate）
   - 署名鍵の有効期限管理（90日）
   - 緊急時の鍵削除

5. **テスト**:
   - ZK proof 生成→検証のラウンドトリップ
   - ホワイトリスト登録フロー
   - cNFT 部分署名のテスト
   - Attestation Document 検証のテスト

### やらないこと

- 開発者のコレクション作成 UI（開発者の責任）
- cNFT のメタデータ設計（開発者の責任）
- Merkle Tree の作成・管理 UI

## 依存

- Task 02: Gateway API 型定義
- Task 04: TEE オーケストレーション
- sandbox/03-sp1-attestation: ZK proof の実装基盤

## 成功基準

- [ ] Solana 用署名鍵の生成が TEE 内で動作する
- [ ] Attestation Document から ZK proof を生成できる
- [ ] ZK proof でホワイトリスト PDA に署名鍵を登録できる
- [ ] ホワイトリスト済み鍵で cNFT 部分署名ができる
- [ ] `cargo test` 合格
- [ ] COVERAGE.md 更新
