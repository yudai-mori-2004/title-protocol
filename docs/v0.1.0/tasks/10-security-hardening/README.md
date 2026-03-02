# タスク10: セキュリティ強化 + DoS対策

## 前提タスク

- タスク6（Gateway認証）が完了していること
- タスク7（WASMホスト）が完了していること

## 読むべきファイル

1. `docs/SPECS_JA.md` — §6.4「メモリ管理」「漸進的重み付きセマフォ予約」「動的グローバルタイムアウト」「処理上限の管理」「不正WASMインジェクションに対する防御モデル」「/sign フェーズでの防御（Verify on Sign）」
2. `crates/tee/src/endpoints/verify.rs` — タスク4で実装済み
3. `crates/tee/src/endpoints/sign.rs` — タスク5で実装済み
4. `crates/types/src/lib.rs` — ResourceLimits

## 作業内容

### TEE側のDoS対策

#### 漸進的重み付きセマフォ予約（§6.4）
- `tokio::sync::Semaphore` でグローバルメモリ予約を管理
- ペイロードダウンロード時に64KBチャンクごとにセマフォ予約
- 予約失敗（メモリ上限到達）→ 即座に接続切断
- `max_concurrent_bytes` のデフォルト値: 8GB

#### Zip Bomb対策
- `tokio::io::take` で宣言サイズを超えるデータの読み取りを遮断
- Content-Lengthヘッダーの事前検証

#### Slowloris対策
- チャンク単位のRead Timeout（デフォルト30秒）
- `chunk_read_timeout_sec` をresource_limitsから適用

#### 動的グローバルタイムアウト
- `Timeout = min(MaxLimit, BaseTime + ContentSize / MinSpeed)`
- resource_limitsのパラメータから動的に算出
- `tokio::time::timeout` で各リクエストに適用

#### Verify on Sign 防御（/signフェーズ）
- signed_json取得時にもサイズ制限（1MB上限）
- チャンク単位のRead Timeout
- Content-Lengthヘッダーの事前検証

### resource_limits の完全適用

現在のTEEは resource_limits をデフォルト値で使用している。Gateway認証済みリクエストに含まれる resource_limits を実際に適用する:

- `max_single_content_bytes`: ペイロードのサイズ上限
- `max_concurrent_bytes`: セマフォの総容量
- `min_upload_speed_bytes`: 動的タイムアウト計算
- `base_processing_time_sec`: 動的タイムアウト計算
- `max_global_timeout_sec`: 絶対的な最大タイムアウト
- `chunk_read_timeout_sec`: チャンク読み取りタイムアウト
- `c2pa_max_graph_size`: 来歴グラフのノード+エッジ上限

### 不正WASMインジェクション防御

- /verifyレスポンスは既にE2EE暗号化されている（第1層）
- SDK側のwasm_hash検証はタスク8で実装済み（第2層）
- TEE側: Global Configの `trusted_wasm_modules` にextension_idが存在するか確認してから実行

## 完了条件

- テスト: max_single_content_bytes を超えるペイロードが拒否される
- テスト: Slowloris攻撃（チャンク単位で30秒超無応答）が切断される
- テスト: セマフォ枯渇時に新規リクエストが適切にエラーになる
- テスト: 信頼されていないextension_idのWASM実行が拒否される
- `cargo check --workspace && cargo test --workspace` が通る
- `docs/COVERAGE.md` の該当箇所を更新
