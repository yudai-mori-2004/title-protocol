# Task 31: コード監査 — crates/core

## 対象
`crates/core/` — C2PA検証・来歴グラフ構築

## ファイル
- `src/lib.rs` — verify_c2pa, extract_content_hash, build_provenance_graph, resolve_duplicate
- `src/tsa.rs` — RFC 3161 TSAタイムスタンプ抽出（COSE → CMS → TstInfo）
- `src/jumbf.rs` — JUMBF最小パーサ（COSE署名バイト列抽出）
- `examples/gen_fixture.rs` — E2Eテスト用フィクスチャ生成ツール

## 初期監査で発見済みの問題

### 致命的
1. **TSAが実際にはTSAではない**: `extract_tsa_info()` が `manifest.time()`（作成者の自己申告時刻）を「TSAタイムスタンプ」として使用し、`signature_info().issuer` を「TSA公開鍵ハッシュ」として使用。RFC 3161タイムスタンプ検証は一切行われていない。`resolve_duplicate()` がこれを信頼して重複解決している。

### コード品質
2. **自作RFC 3339パーサ** (`parse_rfc3339_to_epoch`): UTCのみ対応、手書きepoch計算。`chrono` / `time` crateで置換すべき。
3. **`format_content_hash` が `hex` crateを使っていない**: `hex` は既に依存に入っているのに手動フォーマット。
4. **`serde`/`serde_json` が通常依存**: テストでのみ使用。`[dev-dependencies]` に移動すべき。
5. **不要なexample**: `test_pixel.rs`, `test_verify.rs` はデバッグ痕跡。

## 完了基準
- [x] TSA問題の対処（実TSA実装 or 明示的な「claim time」リネーム）
- [x] 自作パーサを外部crate置換
- [x] `format_content_hash` を `hex::encode` 使用に変更
- [x] `serde`/`serde_json` を dev-dependencies に移動
- [x] デバッグexample削除
- [x] `cargo test -p title-core` パス

## 対処内容

### 1. TSA: 実RFC 3161タイムスタンプ抽出に全面書き直し
- 旧: `manifest.time()`（自己申告時刻）を「TSA」と偽装
- 新: `src/tsa.rs` を新設。COSE_Sign1 → unprotected headers → sigTst2/sigTst →
  TstContainer CBOR → RFC 3161 DERトークン → CMS ContentInfo → SignedData →
  EncapsulatedContentInfo → TstInfo → `gen_time` (GeneralizedTime) を正しく抽出
- `der` クレート (v0.7) を使用したDER解析。手書きDERパーサを全廃
- TSA証明書ハッシュ: SignedData.certificates[0]からSHA-256を計算（旧: `None` TODO放置）
- BERエンコードの小数秒への堅牢なフォールバック処理
- `C2paVerificationResult` を `tsa_info: Option<TsaInfo>` 構造に変更
- `crates/tee` の参照も追従修正

### 2. 自作パーサ → 外部crate
- `parse_rfc3339_to_epoch`（手書きepoch計算） → 廃止
- `DerReader`（手書きASN.1パーサ） → `der::SliceReader` + `der::asn1::GeneralizedTime`
- `parse_generalized_time`（手書きepoch計算） → `GeneralizedTime::into::<SystemTime>()`
- 小数秒フォールバックも`GeneralizedTime`で再デコード（手動epoch計算を完全排除）

### 3. `format_content_hash`
- 旧: `format!("0x{:02x}", ...)` ループ → 新: `format!("0x{}", hex::encode(hash))`

### 4. `serde`/`serde_json` 依存整理
- `serde` / `serde_bytes`: TSA CBORデシリアライズ（`TstContainer`/`TstToken`）で本番使用 → 通常依存のまま（正当）
- `serde_json`: テストのみ使用 → `[dev-dependencies]` に配置済み

### 5. デバッグexample削除
- `test_pixel.rs`, `test_verify.rs` を削除
- `gen_fixture.rs` のみ残存（E2Eテスト用、正当）

### 6. テスト: 17 → 30テスト（+13）
新規テスト:
- `test_extract_tsa_from_cose_invalid_bytes` — 不正COSEバイト
- `test_extract_tsa_sigTst2_preferred_over_sigTst` — sigTst2がsigTstより優先される（仕様の核心動作）
- `test_parse_tst_info_empty_input` — 空入力
- `test_parse_tst_info_truncated` — 切り詰め入力
- `test_parse_full_tst_token_with_cert` — 証明書付きTSTトークン + cert_hash検証
- `test_parse_tst_token_empty_input` — 空DER入力
- `test_parse_tst_token_garbage` — 不正バイト列
- `test_find_header_by_text_found` — ヘッダ検索ヒット
- `test_find_header_by_text_not_found` — ヘッダ検索ミス
- `test_find_header_by_text_prefers_exact_match` — 完全一致検索
- `test_resolve_duplicate_empty_input` — 空トークンリスト
- `test_resolve_duplicate_empty_trusted_list_trusts_all` — 空信頼リスト=全TSA信頼
- `test_resolve_duplicate_tsa_without_cert_hash_ignored_when_trusted_list_set` — cert_hash=None + 非空信頼リスト → TSA無視
- `test_verify_c2pa_valid` にtsa_info=Noneのアサーション追加

### 7. その他OSS品質向上
- sigTst2/sigTst検索の重複ロジック → `find_header_by_text()` ヘルパーに抽出
- `core::str` → `std::str`（non-no_std crateとの一貫性）
- 移行完了メモのコメント削除（`lib.rs` "旧extract_tsa_info..."）
- `jumbf.rs`: ラベル読み取りのASCII前提にコメント追記
- DERフラット方式の設計意図をdocコメントに明記
- `generalized_time_to_epoch()` ヘルパーで変換ロジックを共通化
- テストヘルパー `build_tst_token()`, `wrap_sequence()` で構築ロジックを整理
