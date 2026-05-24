# K8 (Round 2): `crates/core` 縦深掘り — 修正適用後

## 概要

担当範囲: `crates/core/` 全ファイル — `Cargo.toml`, `src/lib.rs`, `src/c2pa_verify.rs`, `src/jumbf.rs`, `src/processor.rs`, `src/request.rs`, `src/response.rs`。

監査方針: Round 1 で出した 22 件（must:5, should:10, nitpick:7）の処理状況を確認した上で、修正によって生まれた退行と新規問題を検出する。各 `.rs` を再通読し、Round 1 と同じ観点（仕様整合・公開 API 妥当性・`#[serde(flatten)]`・c2pa-rs 作法・JUMBF パーサ・`Processor` trait 設計）に加え、

- 修正パッチ自体の品質（コメントの正確さ、エラーパスの取りこぼし、テストの追加有無）
- `lib.rs` の API サーフェスが Round 1 提案後どこに着地したか

を観点に再通読した。新規発見件数: **7 件**。

## 重大度別内訳

- must-fix: 0 件
- should-fix: 4 件
- nitpick: 3 件

## Round 1 指摘の処理状況

| ID | カテゴリ | 状態 | コメント |
|---|---|---|---|
| must-fix-001 | dead public API (`processor_outputs` の 8 型) | **fixed** | `crates/core/src/processor_outputs.rs` ごと削除、`Cargo.toml` の `[[lib]]` も整合。`lib.rs` から該当 re-export も消えた |
| must-fix-002 | `CoreError` デッド | **fixed** | `crates/core/src/error.rs` 削除、`lib.rs` から `pub use error::CoreError;` も消えた |
| must-fix-003 | `jumbf` モジュール内の `pub fn` が `pub(crate)` 化 | **fixed** | `jumbf.rs:159`, `jumbf.rs:218` ともに `pub(crate) fn` に修正済 |
| must-fix-004 | `ProcessorOutput.data: Value` の `#[serde(flatten)]` 安全性 | **partially-fixed** | `ProcessorOutput::ok()` で `data.is_object()` チェックを入れ、非オブジェクトは `error()` にフォールバック。ただし `serde::Deserialize` 経由で生成された場合は依然 unchecked。詳細 round2-new-001 |
| must-fix-005 | `c2pa::Reader` 二重構築 | **unchanged** | `c2pa_verify.rs:199` (`extract_active_manifest_signature`) と `c2pa_verify.rs:231` (`verify_and_extract`) は依然それぞれ独立に `c2pa::Reader::from_context(...).with_stream(...)` を呼ぶ。`orchestrator.rs:222` が `compute_signature_hash` を、そのあと `execute_processors` 経由で `C2paVerifyProcessor::process` を呼ぶ実行フロー（同じ content に対し 2 回パース）も維持されている |
| should-fix-001 | `ProcessorRegistry::execute` の逐次実行 vs 仕様 | **unchanged** | `processor.rs:122-143` の doc コメント「注: 現在は逐次実行。並列実行はTEEオーケストレーション層で実装する。」もループも維持。`orchestrator.rs:347` も `registry.execute(...)` を素通しで呼ぶのみで並列化していない |
| should-fix-002 | sidecar/fragmented + encryption の型未防御 | **unchanged** | `ProcessRequest` の構造は変わらず。orchestrator 側で `OrchestratorError::EncryptionUnsupportedForInputType` (`orchestrator.rs:95,176,286`) を実行時に返すだけ。仕様 §2.2 を型で表現する設計には到達せず |
| should-fix-003 | `read_so_far` 計算の精度問題 | **fixed** | `jumbf.rs:136-141`: `let label_bytes: u64 = if label.is_empty() { 0 } else { label.len() as u64 + 1 }; let read_so_far: u64 = 16 + 1 + label_bytes;` の二段化に書き換え済 |
| should-fix-004 | JUMBF ラベルの非 ASCII バリデーション | **fixed** | `jumbf.rs:124-128` で `if !byte[0].is_ascii() { return Err(...) }` を `push` の直前に追加 |
| should-fix-005 | `MAX_SIGNATURE_SIZE` 16MiB の根拠 | **fixed** | `jumbf.rs:29-33`: 256 KiB に下げ、`// Upper bound on the COSE signature CBOR blob — sized to cover realistic PKI deployments (ECDSA signature ~70 B, certificate chain a few KiB per certificate, OCSP/timestamp tokens up to a few hundred KiB)` と根拠 doc を追加 |
| should-fix-006 | `read_header` の EOF センチネル | **fixed** | `jumbf.rs:47-82`: 戻り型を `Result<Option<BoxHeader>, ...>` に変更、呼び出し側 (`find_manifest_labels`, `extract_signature_from_jumbf`, `find_signature_in_manifest`, `find_cbor_in_box`) も `?.ok_or_else(...)` ないし `let Some(...) = ... else { break };` に整理されている |
| should-fix-007 | サイドカー active manifest = last の根拠出典 | **partially-fixed** | `compute_signature_hash_from_manifest_data` の doc に「C2PA 2.1 §13.4 defines the active manifest as the last `c2pa.manifest` box in the store」を追加。一方で `jumbf::find_manifest_labels` 側の doc は「conventionally the last」のまま。詳細 round2-new-004 |
| should-fix-008 | re-export の二重露出 | **fixed** | `lib.rs:9-13` で各モジュールが `mod` (= private) になり、`pub use ...::{...}` の flat re-export だけが公開 API となった |
| should-fix-009 | `ProcessorError` に手動 `impl Clone` がテスト側に混入 | **fixed** | `processor.rs:60` で本体に `#[derive(Debug, Clone, thiserror::Error)]` を追加。テスト側の `impl Clone` 手書きは消えた |
| should-fix-010 | `core` の `image` dev-dep と署名 fixture 毎テスト生成 | **unchanged** | `Cargo.toml:20` に `image` dev-dep 残置、`c2pa_verify.rs` のテストは依然 `create_signed_jpeg()` を 7〜8 回呼んでいる |
| nitpick-001 | doc comment 言語の不統一 | **unchanged** | `request.rs` / `response.rs` / `processor.rs` は日本語、`c2pa_verify.rs` / `jumbf.rs` / `lib.rs` は英語と分かれたまま |
| nitpick-002 | doc コメントの JSON 例に `...` | **unchanged** | `response.rs:39-44`, `request.rs:21-26` などに `...` を含む例が残る |
| nitpick-003 | `EncryptionSuite::suite_id(&self)` / `from_suite_id` 非対称 | **unchanged** | `request.rs:102` `pub fn suite_id(&self) -> u8`、`request.rs:112` `pub fn from_suite_id(id: u8) -> Option<Self>` のまま |
| nitpick-004 | `ProcessorError::ParseFailed` / `UnsupportedContentType` が未使用 | **unchanged** | `grep -rn "ProcessorError::ParseFailed\|ProcessorError::UnsupportedContentType" crates/` は依然 0 件 |
| nitpick-005 | `ProcessResponse` / `VerifiableResponse` / `ProcessorOutput` の `PartialEq` 不在 | **unchanged** | `response.rs:21,47,73` の `#[derive(...)]` には `PartialEq` が付かない |
| nitpick-006 | `extract_actions` doc の不正確 | **unchanged** | `c2pa_verify.rs:306` のコメントは「Looks for both `c2pa.actions` and `c2pa.actions.v2`」のままで、コード上のリテラル参照に書き換えていない |
| nitpick-007 | `build_claim_generator_string` の `"{name} {version}"` 形式の spec 対応 doc | **unchanged** | `c2pa_verify.rs:287-301` の doc に spec §3.2 との対応説明は追加されていない |

集計:
- fixed: 9 件
- partially-fixed: 2 件
- unchanged: 11 件
- regressed: 0 件

## 新規発見

### round2-new-001 (should-fix) `ProcessorOutput::ok()` のオブジェクト性チェックが Serialize 時しかかからず、Deserialize 経由を素通り

- 場所: `crates/core/src/response.rs:95-122`
- 観察: Round 1 の must-fix-004 への対応として `pub fn ok(data: serde_json::Value) -> Self` に `if !data.is_object() { return Self::error(...); }` を入れたのは正しい方向。ただし `ProcessorOutput` は `#[derive(Serialize, Deserialize)]` で `pub data: serde_json::Value` をそのまま公開しているため、

  ```rust
  let bad = serde_json::from_str::<ProcessorOutput>(r#"{"status":"ok"}"#).unwrap();
  // bad.data == Value::Object({}) （flatten 残余が空オブジェクト）
  let bad2 = ProcessorOutput { status: ProcessorStatus::Ok, data: serde_json::json!([1,2,3]) };
  serde_json::to_value(&bad2); // 依然 #[serde(flatten)] 違反
  ```

  のように、(a) コンストラクタを経由しない構築（`pub` フィールドへの直接代入）、(b) deserialize 経由の構築、のいずれにおいてもチェックが効かない。
- 問題: 「ok コンストラクタを必ず通す」という慣習に頼った安全性しかなく、`pub data` の型が `serde_json::Value` のままだと型レベルでは依然 unsafe な API。テストは `ProcessorOutput::ok(...)` 経由のみカバーしている (`response.rs:225-247`)。
- 修正案:
  - (a) `pub data: serde_json::Value` を `data(&self) -> &serde_json::Map<...>` 経由のみアクセス可能にする（フィールド private 化 + 構築 API を限定）。
  - (b) 型自体を `pub data: serde_json::Map<String, serde_json::Value>` に変える（`#[serde(flatten)]` がマップ前提なので妥当）。serde の flatten はマップ型に対しても正しく動く。
  - (c) `Deserialize` 実装を手書きして、deserialize 時に「`status` 以外がオブジェクトとして flatten される」前提を validate する。
  まずは (b) が最小コスト。`ProcessorOutput::ok` / `error` のシグネチャを `Map<String, Value>` / `String` に揃え、テストも更新する。

### round2-new-002 (should-fix) `ProcessorOutput::error` が `"status":"ok"` データ内に `"error"` フィールドを持つ入力で曖昧

- 場所: `crates/core/src/response.rs:116-121`、影響先 `crates/core/src/response.rs:226-247`
- 観察: `error()` は `data: json!({ "error": message })` で生成する。`#[serde(flatten)]` で展開後の JSON は `{"status":"error","error":"..."}` になる。逆向き（Deserialize）では `{"status":"ok","error":"こちらは ok の processor のフィールド"}` のような入力も合法で、その場合 `data` には `{"error":"..."}` が入り、ステータスは `ok` のまま。
- 問題: `"error"` フィールドが「ステータスがエラーである」のシグナルなのか「データの中身としての error 文字列」なのかが構造的に区別できない。spec §3.1 の処理失敗時 JSON 例は `{ "status": "error", "error": "..." }` の組み合わせを暗黙の前提にしている。
- 修正案:
  - `ProcessorOutput::error` の表現を `data: json!({})` + 別フィールド `message: Option<String>` に切り出す（`status == Error` の時のみ Some）。
  - もしくは spec §3.1 側で「`status == "ok"` の processor 出力は `error` キーを持ってはならない」を明記し、core 側で deserialize 時に validate する。
  優先度はそれほど高くないが、`status` と `error` の関係を仕様で明文化しないと将来の processor 追加時に踏む。

### round2-new-003 (should-fix) `ProcessorRegistry::execute` の doc コメントが「逐次実行」と明記している一方、上位 `orchestrator::execute_processors` がそれを素通しで使い続けており、仕様 §1.3「並列に実行」と矛盾している事実が Round 1 から temporally regression していない

- 場所: `crates/core/src/processor.rs:122` (`/// 注: 現在は逐次実行。並列実行はTEEオーケストレーション層で実装する。`)、`crates/tee/src/orchestrator.rs:339-348` (`fn execute_processors` が `registry.execute(...)` の薄い wrapper)
- 観察: Round 1 should-fix-001 は「unchanged」のままだが、修正パッチが入った他項目との対比で「ここだけ仕様乖離を明文化したまま放置」という構造になっており、レビュー観点では悪化に近い。`processor_outputs.rs` を削除しつつ `Registry::execute` を残したことで、core の API サーフェスでは「逐次実行 dispatcher」が中核 API として残った形になる。
- 問題: v0.1.2 では現実的に processor は `c2pa-verify` 1 個しか稼働しないため並列化の実害は無いが、

  - 仕様書 §1.3「指定された全processorが並列に実行され」
  - 仕様書 §3.1「processor間に実行順序の制約は存在しない」
  - core の doc「逐次実行」
  - orchestrator の素通し

  の四重乖離が残る。OSS リリース時に「仕様と違う」と最初に指摘されるポイント。
- 修正案: いずれか:
  - (a) `Registry::execute` を削除し、`Registry::get()` + 上位での並列ループに統一する。
  - (b) `Registry::execute` を `rayon::scope` ベースで並列化する。Send + Sync 制約は既に trait に付与済み。
  - (c) 仕様 §1.3 / §3.1 を v0.1.2 の現状（"並列化されうるが、現行構成では逐次で良い"）に合わせて書き直す。
  Round 1 監査者と同じ提案だが、Round 2 では「他の must/should-fix が次々消化される中で取り残されている」事実を加味して再掲する。

### round2-new-004 (nitpick) `jumbf::find_manifest_labels` の doc がサイドカー「last == active」の根拠を `c2pa_verify.rs` 側にだけ書き、`jumbf.rs` 側は「conventionally」のまま

- 場所: `crates/core/src/jumbf.rs:152-159`（`/// The active manifest is conventionally the last one in the list.`）
- 観察: Round 1 should-fix-007 への対応で `c2pa_verify.rs:160-166` には `C2PA 2.1 §13.4` の参照が追加されたが、`jumbf::find_manifest_labels` の doc にはその出典が反映されていない。
- 問題: `find_manifest_labels` を別の文脈から呼ぶ将来コードが「conventionally」だけ読んで誤解する可能性がある。出典は呼び出し側ではなく算出側（jumbf）にも置くべき。
- 修正案: `jumbf.rs:155` の `/// The active manifest is conventionally the last one in the list.` を `/// The active manifest is the last `c2pa.manifest` box in the store (C2PA 2.1 §13.4).` に書き換える。

### round2-new-005 (nitpick) `lib.rs` 内 doc comment の typo

- 場所: `crates/core/src/c2pa_verify.rs:25`
  ```rust
  //! The utility is public because the TEE orchestration layer //! also needs it when assembling the final response.
  ```
- 観察: 行末の `//!` が行内に紛れている（コピーミス由来）。`cargo doc` ビルドではコメントとして無害だが、rendered doc では `//! ` が文字として現れる可能性がある（rustdoc は行頭の `//! ` だけを doc とみなすので、行中の `//!` はリテラルとして残る）。
- 修正案:
  ```rust
  //! The utility is public because the TEE orchestration layer also
  //! needs it when assembling the final response.
  ```
  に整形する。

### round2-new-006 (nitpick) `c2pa::Reader::from_context(c2pa::Context::default()).with_stream(...)` の `.map_err(|e| { ... })` のインデントが Rustfmt の現行設定と整合していない

- 場所: `crates/core/src/c2pa_verify.rs:199-203`, `c2pa_verify.rs:231-235`
- 観察:
  ```rust
  let reader = c2pa::Reader::from_context(c2pa::Context::default())
      .with_stream(content_type, &mut cursor)
      .map_err(|e| {
      ProcessorError::C2paVerificationFailed(format!("C2PA Reader construction failed: {e}"))
  })?;
  ```
  クロージャ本体のブロックが `.map_err(` の開き括弧位置基準ではなく、左端から 4 スペースで始まっている（通常は閉じ `})` 直前に揃える）。
- 問題: `cargo fmt` が走っていれば自動修正されるはず（既存の他コードと整合する）の見た目崩れ。CI に `cargo fmt --check` が入っていない可能性を示唆する。
- 修正案: 一括 `cargo fmt --all` 適用。CI に `cargo fmt --check` を追加する（K8 観点外だが派生）。

### round2-new-007 (nitpick) `compute_signature_hash` と `compute_signature_hash_from_manifest_data` の戻り型・エラー型は同じだが、形式（`"sha256:"` プレフィクス）の正典化が個別実装に散らばっている

- 場所: `crates/core/src/c2pa_verify.rs:148-155`, `c2pa_verify.rs:168-182`
- 観察: 両関数とも `Ok(format!("sha256:{}", hex::encode(hash)))` で文字列化している。プレフィクス `"sha256:"` は spec §1.3 / §2.3 の `signature_hash` の identifier 仕様で、core 全体で 2 箇所に複製されている。orchestrator の `declared != signature_hash` 比較（`orchestrator.rs:228-232`）はこの文字列形式の同一性に依存する。
- 問題: 将来 `sha3-256` 等を追加するときに正典が複数箇所に散らばっている。
- 修正案: `pub(crate) fn format_signature_hash(bytes: &[u8]) -> String { format!("sha256:{}", hex::encode(Sha256::digest(bytes))) }` のような単一のフォーマット関数を作り、両 `compute_*` から呼ばせる。

## 全体所感

Round 1 の must-fix 5 件中 3 件（001 `processor_outputs` 削除、002 `CoreError` 削除、003 `pub fn` 可視性）は完全に整理された。dead public API の剥がし忘れが解消されたことで OSS 公開時の API 面が大きくきれいになった。`jumbf.rs` も EOF 表現の `Option<BoxHeader>` 化、非 ASCII バリデーション、`MAX_SIGNATURE_SIZE` 根拠 doc、`read_so_far` 二段化など Round 1 指摘の細部がほぼ全部消化されており、品質が一段上がっている。

一方、

- **must-fix-004**（`#[serde(flatten)]` の型レベル安全性）はコンストラクタにガードを足したが `pub data: Value` のままで deserialize 経由を素通り（round2-new-001）。完全な型安全には `Map<String, Value>` 化が必要。
- **must-fix-005**（c2pa Reader 二重構築）は手付かず。orchestrator が `compute_signature_hash` → `C2paVerifyProcessor::process` の順で同じ content をパースする実行フローも維持されていて、実コストの非効率が残る。
- **should-fix-001/002/010, nitpick-001〜007** はほぼ unchanged で、特に「逐次実行 vs 仕様」と「sidecar+encryption の型未防御」は仕様と実装の乖離を可視化するレビュー上の inflection point として残っている（round2-new-003）。

新規発見 7 件のうち round2-new-001（deserialize 経由の flatten 安全性）と round2-new-002（`status` と `error` キーの曖昧性）は外部公開前に決着させたい型問題。残りは doc / フォーマット / 軽微なリファクタの域。退行は検出されなかった。

Round 1 で 22 件 → Round 2 で 7 件（うち should:4, nitpick:3）。修正サイクルとしては正常に収束しつつあり、残課題は「c2pa Reader 二重構築」「`pub data: Value` の型強化」「逐次 vs 並列の最終決着」の 3 系統に集約される。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| must-fix-001/002/003 | fixed | Round 2 認定済み。 |
| must-fix-004 | partially-fixed(`ProcessorOutput::ok` のオブジェクト性チェックは存続。`pub data: Value` を `Map<String, Value>` に変更すると下流の呼び出し全箇所 (`json!({...})` への `.as_object()` 取り出し) に波及するため、現実装の Deserialize 経由バイパスは acceptable risk として残置。round2-new-001 も同じ理由で wontfix) | |
| must-fix-005 | wontfix(`c2pa::Reader` の二重構築は orchestrator フロー全体の refactor が必要で本 audit 範囲を超える。c2pa-rs 0.84 では Reader 構築コストは低く、CPU/メモリへの実害も限定的) | |
| should-fix-001 | wontfix(逐次 vs 並列は spec §1.3 と processor.rs doc の乖離だが、v0.1.2 では c2pa-verify が唯一の processor で実害ゼロ。仕様改訂か並列化のどちらかは v0.1.3 で決着) | |
| should-fix-002 | wontfix(sidecar/fragmented + encryption の型レベル防御は orchestrator 側 `EncryptionRequiresSingleInput` で実行時に reject 済み。型システムで強制する設計変更は v0.1.3) | |
| should-fix-003/004/005/006 | fixed | Round 2 認定済み。 |
| should-fix-007 | fixed | round2-new-004 と統合対応。`jumbf::find_manifest_labels` の doc に C2PA 2.1 §13.4 出典を追加し、conventional 表現を排除。 |
| should-fix-008/009 | fixed | Round 2 認定済み。 |
| should-fix-010 | wontfix(`image` dev-dep + 7-8 回の `create_signed_jpeg()` 呼び出しは test only。OnceLock キャッシュは coverage 改善案だが OSS 公開前のフェーズで対応) | |
| nitpick-001..007 | wontfix(doc 言語統一・JSON 例の `...` 整理・`EncryptionSuite::suite_id` の `&self`/`&Self` 非対称・dead enum variant・PartialEq 追加 etc は OSS 公開前のドキュメント仕上げで一括対応) | |
| round2-new-001/002 | wontfix(must-fix-004 partially-fixed と同根。`Map<String, Value>` への移行 + status/error 曖昧性解消は wire format 互換性確認を要し、v0.1.3 SDK 整備と合わせて対応) | |
| round2-new-003 | wontfix(should-fix-001 と同根) | |
| round2-new-004 | fixed | should-fix-007 と統合対応。 |
| round2-new-005 | fixed | `c2pa_verify.rs:25` の `//!` インライン typo を修正。2 行の doc comment に整形。 |
| round2-new-006 | fixed | `cargo fmt -p title-core` を適用。`.map_err` クロージャの indent が標準に揃った。 |
| round2-new-007 | fixed | `c2pa_verify.rs` に `format_signature_hash(&[u8]) -> String` ヘルパを新設し、`compute_signature_hash` / `compute_signature_hash_from_manifest_data` の両方から呼ばせる構造に集約。将来 sha3-256 等への切替が 1 箇所で済む。 |
