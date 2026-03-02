# Task 38: コード監査 — wasm/

## 対象
`wasm/` — 4つの `#![no_std]` WASMモジュール（Extension Layer サンプル実装）

## ファイル
- `wasm/phash-v1/src/lib.rs` — 知覚ハッシュ（SHA-256ベース簡易実装）
- `wasm/hardware-google/src/lib.rs` — ハードウェア署名マーカー検出
- `wasm/c2pa-training-v1/src/lib.rs` — AI学習許諾フラグ抽出
- `wasm/c2pa-license-v1/src/lib.rs` — ライセンス情報抽出

## 前提: WASMモジュールの位置づけ

これらのモジュールは **AppStore における初期公開アプリ** に相当する。
本番環境では、TEEが外部URL（Arweave等）からDAO監査済みWASMバイナリを取得・実行する。
本質的なセキュリティ境界は **wasm-host（wasmtime）** 側にあり、個々のモジュールの品質は
プロトコルのセキュリティに直接影響しない。

### wasm-host セキュリティ評価（確認済み）
- **データ分離**: 毎実行で新規 `Store` + `InnerHostState`。他ユーザーのコンテンツにアクセス不可
- **リソース制限**: Fuel 1億命令 / Memory 64MB。無限ループ・OOM不可
- **ホスト関数安全性**: 全関数でソース・デスティネーション両方の境界チェック実施
- **パニック隔離**: `catch_unwind` でCore処理への影響を遮断
- **ホワイトリスト**: `TRUSTED_EXTENSIONS` 環境変数で実行許可Extension制御

## 監査で発見した問題

### 安全性
1. **host返値未検証（3モジュール）**: `read_content_chunk` の返値をバッファサイズで
   クランプせずに `from_raw_parts` に使用。ホスト側は正しい値を返すが、
   WASMモジュール側の防御的プログラミングとして修正。
2. **未初期化メモリ読取（c2pa-training-v1）**: `find_pattern_with_context` で
   コンテキストバッファを256バイト固定でスライス化。ファイル末尾付近でパターン検出時に
   実際の読取バイト数 < 256 の場合、未初期化メモリを読む。

### 修正不要と判断した項目
- **offsetアンダーフロー**: `read >= pattern.len()` ガードが機能しており発生しない
- **`use core::fmt::Write`（phash-v1）**: `write!` マクロで暗黙使用中。dead codeではない
- **コード重複（~160行）**: `#![no_std]` + `cdylib` のWASMモジュールでは共通crate化すると
  `#[global_allocator]` / `#[panic_handler]` の重複定義問題が発生。意図的な自己完結設計
- **テスト不在**: WASMモジュール自体はサンプルアプリ。wasm-host側に10テスト、
  tee/extension側にE2Eテストがあり、サンドボックスの正しさは検証済み

## 完了基準
- [x] host返値クランプ（hardware-google, c2pa-training-v1, c2pa-license-v1）
- [x] c2pa-training-v1: コンテキストバッファサイズを実際の読取長に修正
- [x] 全4モジュール `cargo check --target wasm32-unknown-unknown` パス
- [x] `cargo test --workspace` パス（142テスト）
