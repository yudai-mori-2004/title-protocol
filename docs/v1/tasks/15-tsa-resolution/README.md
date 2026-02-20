# タスク15: TSAタイムスタンプ重複解決

## 仕様書

§2.4 — 重複の解決（先に作成した者が優先）

## 背景

同一content_hashに複数の権利トークンが存在する場合、「先に作成した者」を正当な権利者とする。
C2PA TSAタイムスタンプ（RFC 3161）があればそれを使い、なければSolana block timeで代用する。

## 読むべきファイル

1. `docs/v1/SPECS_JA.md` §2.4（重複の解決ロジック）
2. `crates/core/src/lib.rs` — `verify_c2pa()` の現在の実装（tsa_* フィールドがNone返却）
3. `crates/types/src/lib.rs` — `CorePayload` の `tsa_timestamp`, `tsa_pubkey_hash`, `tsa_token_data` フィールド
4. `programs/title-config/src/lib.rs` — `update_tsa_keys()`, `trusted_tsa_keys: Vec<[u8; 32]>`
5. `crates/tee/src/endpoints/verify.rs` — `process_core()` でのTSAフィールド利用箇所

## 要件

### Phase 1: TSA抽出（crates/core）

- `verify_c2pa()` でC2PAマニフェストからTSAタイムスタンプを抽出
- RFC 3161トークンからタイムスタンプ値（Unix epoch秒）を取得
- TSA署名者の公開鍵ハッシュ（SHA-256）を抽出
- RFC 3161トークンデータをBase64で保存
- TSA情報がない場合は引き続きNoneを返す

### Phase 2: 重複解決ロジック（crates/core または新モジュール）

- `resolve_duplicate(tokens: &[TokenRecord]) -> &TokenRecord` 関数
  - 各トークンの作成時刻を決定: TSAあり→TSA時刻、TSAなし→Solana block time
  - 最古の作成時刻を持つトークンを選択
  - 同一時刻の場合、登録時刻（Solana block time）が最古のものを選択
  - Burnされたトークンは除外
- `trusted_tsa_keys` との照合: TSA公開鍵ハッシュが信頼リストに含まれるか検証

## 完了条件

- `verify_c2pa()` がTSA付きC2PAマニフェストからタイムスタンプを正しく抽出
- 重複解決ロジックのユニットテスト（TSAあり/なし混合、同一時刻、Burn済み除外）
- `cargo check --workspace && cargo test --workspace` 通過
