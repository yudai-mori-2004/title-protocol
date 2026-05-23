# タスク09: メモリ管理（ResourcePool + Ticket）

## 目的

TEE の限られたメモリ上で OOM を防ぐメモリ管理機構を実装する。ResourcePool によるグローバル使用量管理と、Ticket による漸進的予約・解放メカニズムを構築する。

## 読むべきファイル

1. `CLAUDE.md`
2. `docs/v0.1.2/SPECS_JA.md` — **§4 全体を精読**:
   - §4.1 ResourcePool（admission_limit / total_limit の2閾値モデル）
   - §4.2 Ticket（漸進的予約: extend / shrink / 自動解放）
   - §4.3 入力形式ごとのメモリパターン（single / fragmented / sidecar）
   - §4.4 攻撃への防御（サイズ上限、チャンクタイムアウト60s、グローバルタイムアウト30min、デコードメモリ保護）
3. `docs/v0.1.2/COVERAGE.md`
4. `legacy/v0.1.0/crates/wasm-host/src/resource_pool.rs` — **前バージョンの ResourcePool 実装。設計パターンの参考。**

## スコープ

### やること

1. **ResourcePool**:
   - `admission_limit` / `total_limit` の2閾値管理
   - 新規リクエスト受付判定（admission_limit 超過時は 503 拒否）
   - 合計使用量の追跡（atomic counter）
   - スレッドセーフ（複数リクエストの同時処理）

2. **Ticket**:
   - ResourcePool からの発行
   - `extend(bytes)` — データ到着時にメモリを漸進的に予約
   - `shrink(bytes)` — 不要になったメモリの部分解放
   - Drop 時の自動全解放
   - total_limit 超過時の予約拒否

3. **入力形式ごとのメモリパターン**:
   - single: Range Request パターン（チャンク単位の extend → shrink）
   - fragmented: フラグメント単位の extend → 処理 → shrink
   - sidecar: マニフェスト + コンテンツの2段階

4. **攻撃防御**:
   - データサイズ上限（フラグメント最大数: 100,000、1個最大: 100MB）
   - チャンクタイムアウト（60秒）
   - グローバルタイムアウト（最大30分、サイズ適応）
   - デコードメモリ保護（ヘッダベースの展開後サイズ推定）

5. **テスト**:
   - ResourcePool の admission_limit / total_limit テスト
   - Ticket の extend / shrink / 自動解放テスト
   - 並行アクセステスト
   - タイムアウトテスト

### やらないこと

- 実際のHTTP接続管理（それはコンテンツ取得層 — Task 04）
- デコードライブラリ固有の推定ロジック

## 依存

- Task 04: コンテンツ取得層との統合点

## 成功基準

- [ ] ResourcePool が admission_limit / total_limit を正しく管理する
- [ ] Ticket が漸進的予約・解放を行う
- [ ] Ticket の Drop で自動解放される
- [ ] 並行アクセスでデータ競合が発生しない
- [ ] `cargo test` 合格
- [ ] COVERAGE.md 更新
