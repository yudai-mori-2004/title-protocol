# Task 34: コード監査 — crates/wasm-host

## 対象
`crates/wasm-host/` — WASM実行エンジン (wasmtime)

## ファイル
- `src/lib.rs` — WasmRunner, ホスト関数5種, ExtensionResult

## 監査で発見された問題

### デッドコード
1. **`HostState` が未使用**: `pub struct HostState` が定義されているが、`execute()` は `content: &[u8]` と `extension_input: Option<&[u8]>` を直接受け取る。内部では `InnerHostState` のみ使用。`HostState` は外部クレートからも一切importされていない。→ 削除

### コード品質
2. **`serde` が不要な通常依存**: ソースコード内に `use serde` / `#[derive(Serialize, Deserialize)]` が一切ない。`serde_json` はJSON結果パースで使用（正当）。`serde` のみ削除可能。
3. **HMAC無効キーフォールバックが不適切**: `Hmac::new_from_slice(key).unwrap_or_else(|_| Hmac::new_from_slice(&[0]).unwrap())` — HMACは任意長キーを受け入れるため `new_from_slice` は実質的にfailしないが、万一の場合 `[0]` キーにサイレントフォールバックするのはセキュリティ上不適切。→ `unwrap_or_else` を削除し、エラー時は `return 0` で呼び出し元に通知。
4. **テストカバレッジ不足**: 6テスト完備だがエラーパス未網羅:
   - 不正WASMバイナリ（CompileError）
   - 存在しないエクスポート関数名
   - 結果ポインタ=0（WASM側エラー）
   - 結果バッファ不正（境界外アクセス）

### 設計メモ（修正不要）
- wasmtime Engine/Store/Linker のライフサイクル管理は堅実。リクエスト毎にEngine新規作成で完全分離。
- Fuel/Memory制限 + catch_unwind の3重安全策は仕様書 §7.1 準拠。
- ホスト関数のメモリ境界チェックは全関数で実装済み（buf_ptr+len > mem_data.len() → return 0）。
- 結果バッファ形式 `[4B LE: json_len][json_bytes...]` の読み取りは境界チェック付き。

## 完了基準
- [x] `HostState` 削除
- [x] `serde` を依存から削除
- [x] HMACフォールバック修正
- [x] エラーパステスト追加
- [x] `cargo test -p title-wasm-host` パス（10テスト: 既存6 + エラーパス4）
- [x] `cargo check --workspace` パス（警告なし）

## 対処内容

### 1. `HostState` 削除
- `pub struct HostState` はどこからもimportされていないデッドコード。内部では `InnerHostState` のみ使用。

### 2. `serde` 依存削除
- `Cargo.toml` から `serde = { workspace = true }` を削除。ソース内で一切使用なし。`serde_json` は結果JSONパースで正当に使用。

### 3. HMACフォールバック修正
- 旧: `Hmac::new_from_slice(key).unwrap_or_else(|_| Hmac::new_from_slice(&[0]).unwrap())` — 失敗時に `[0]` キーでサイレント続行
- 新: `let Ok(mut mac) = Hmac::new_from_slice(key) else { return 0; }` — 失敗時はホスト関数が0を返しWASM側にエラー通知

### 4. エラーパステスト追加（+4テスト）
- `test_invalid_wasm_binary` — 不正バイナリでCompileError
- `test_missing_export_function` — 存在しない関数名でExecutionError
- `test_result_ptr_zero` — WASM関数がptr=0を返した場合のエラー
- `test_result_buffer_zero_length` — json_len=0のバッファでエラー
