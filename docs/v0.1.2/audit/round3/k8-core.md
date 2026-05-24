# K8 (Round 3): `crates/core` 縦深掘り — Round 2 修正後 + 処理ログ反映の再点検

## 概要

担当範囲: `crates/core/` 全ファイル — `Cargo.toml`, `src/lib.rs`, `src/c2pa_verify.rs`, `src/jumbf.rs`, `src/processor.rs`, `src/request.rs`, `src/response.rs`。

監査方針: Round 2 で出した 7 件（must:0, should:4, nitpick:3）+ Round 1 由来の継承 9 件の処理状況（fixed / partially-fixed / unchanged / wontfix）を round2 README 末尾の処理ログと突き合わせて再確認し、(a) `wontfix` 化された項目が「妥当な acceptable risk」かを批判的に再評価、(b) round2 → round3 の修正によって生まれた退行、(c) 新規発見、の 3 軸で再通読した。

各 `.rs` を 1 行ずつ読み直した上で、新規発見の網を以下に広げている:

- `c2pa::Reader` の `with_stream` 後の cursor 状態（`extract_active_manifest_signature` が同じ `content` を `Reader` と `load_jumbf_from_memory` の 2 経路でパースしている）
- `extract_signature_from_jumbf` / `find_signature_in_manifest` / `find_cbor_in_box` の `top_end` / `manifest_end` / `scan_end` バウンドが「`reader.position()` 基準」と「`child_start + size` 基準」の混在で取られている点
- `ProcessorRegistry::execute` がエラー時に `e.to_string()` でフラット化していて `ProcessorError` 型情報を失っている点
- `EncryptionSuite` の `from_suite_id` を呼ぶ場所が core 内に存在しない（unused public API の疑い）

新規発見件数: **8 件**（should-fix 2 / nitpick 6）。Round 2 の `wontfix` 判定群と内容が重なる項目は重複指摘を避け、追加観点・追加根拠が出たもののみ独立 ID で立てる。

## 重大度別内訳

- must-fix: 0 件
- should-fix: 2 件
- nitpick: 6 件

## Round 2 指摘の処理状況

`docs/v0.1.2/audit/round2/k8-core.md` 末尾の処理ログを Round 3 視点で再評価する。

| ID | round2 自判定 | round3 再評価 | コメント |
|---|---|---|---|
| must-fix-001/002/003 | fixed | **fixed (verified)** | `crates/core/src/` 配下に `processor_outputs.rs` / `error.rs` 不在、`jumbf.rs:179, 235` の `pub(crate) fn`、`lib.rs:9-13` の `mod` (private) を確認。再退行なし |
| must-fix-004 | partially-fixed → wontfix | **partially-fixed (still open)** | `response.rs:74-82` の `pub data: serde_json::Value` + `#[serde(flatten)]` 構造は据え置き。`ok()` のオブジェクト性チェック (`response.rs:102-105`) は serialize 入口のみ。deserialize は `Value` を素通し可能で、テスト `processor_output_error_roundtrip` (`response.rs:256-262`) は `status` だけ確認し `data` の中身を validate していない。`wontfix` を round3 で再開封する根拠を round3-new-001 で詳述 |
| must-fix-005 | wontfix | **wontfix (acceptable, ただし re-instrument 推奨)** | `c2pa_verify.rs:209, 239` の `c2pa::Reader::from_context(...).with_stream(...)` 2 回構築は健在。`extract_active_manifest_signature` 内では同じ `content` を Reader でパース → `load_jumbf_from_memory` で**再パース**、すなわち 1 リクエストあたり 3 回 JUMBF を歩く実装になっており、c2pa-rs 内部の `load_jumbf_from_memory` が JPEG/PNG/MP4 など形式ごとに XMP/APP11/box scan を走らせるコストを考えると acceptable risk としては微妙。詳細 round3-new-002 |
| should-fix-001 | wontfix(v0.1.3 で決着) | **wontfix (受容)** | spec §3.1 §1.3 「並列に実行」と core doc 「逐次実行」の乖離。v0.1.2 で processor が 1 個しか無い現実を踏まえれば一旦 OK。ただし spec §3.1 末尾の `some-proc` JSON 例（`SPECS_JA.md:713`）が他 processor 前提を残しているので spec 側の整合は round3-new-005 で別途指摘 |
| should-fix-002 | wontfix | **wontfix (受容)** | `orchestrator.rs:175-177` の `EncryptionRequiresSingleInput` で実行時に弾けており、core の型レベル拒否は v0.1.3 SDK と一緒で OK |
| should-fix-003/004/005/006 | fixed | **fixed (verified)** | `jumbf.rs:155-160` の `read_so_far` 二段化、`:140-147` の `is_ascii()` ガード、`:28-33` の 256 KiB + 根拠 doc、`:69-101` の `Result<Option<BoxHeader>, _>` + 呼び出し側の `else { break }` パターンを再確認 |
| should-fix-007 | fixed | **fixed (verified)** | `jumbf.rs:171-178`「The active manifest is the last `c2pa.manifest` box in the store (C2PA 2.1 §13.4).」+ `c2pa_verify.rs:166-169` の同一出典への refer が両方入った |
| should-fix-008/009 | fixed | **fixed (verified)** | `lib.rs:9-13` の private mod + flat re-export、`processor.rs:60` の `#[derive(Debug, Clone, thiserror::Error)]` を再確認 |
| should-fix-010 | wontfix | **wontfix (受容、ただし計測不足)** | `Cargo.toml:20` の `image` dev-dep + `c2pa_verify.rs:357-423` の `create_signed_jpeg` / `create_signed_jpeg_with_actions` の毎テスト署名は据え置き。OnceLock キャッシュは round3-new-007 で別途 nitpick |
| nitpick-001..007 | wontfix | **wontfix (受容)** | doc 言語不統一・`...` JSON 例・`EncryptionSuite` 非対称・dead variant・PartialEq 不在 etc は OSS 公開前の整理に押し出すで OK |
| round2-new-001 | wontfix | **partially open** | must-fix-004 と同根。深堀り結果を round3-new-001 で再掲（特に Deserialize 経路で `data.is_object() == false` が**通る**ことの検証） |
| round2-new-002 | wontfix | **wontfix (受容)** | `status == "ok"` で `data` 側に `"error"` キーが居る曖昧性は spec 改訂で決着すべきで、core 単独修正は筋が悪い。受容 |
| round2-new-003 | wontfix | **wontfix (受容)** | should-fix-001 と同根 |
| round2-new-004 | fixed | **fixed (verified)** | should-fix-007 と統合 |
| round2-new-005 | fixed | **fixed (verified)** | `c2pa_verify.rs:25-26` が `//! The utility is public because the TEE orchestration layer also` / `//! needs it when assembling the final response.` の 2 行構成になっている |
| round2-new-006 | fixed | **fixed (verified)** | `c2pa_verify.rs:209-213` および `:239-243` の `.map_err` クロージャがインデント揃え |
| round2-new-007 | fixed | **fixed (verified)** | `c2pa_verify.rs:189-192` `fn format_signature_hash(...)` 新設、`compute_signature_hash` (`:159`) と `compute_signature_hash_from_manifest_data` (`:184`) の両方が呼ぶ構造 |

集計（Round 2 提起 16 項目の round3 時点判定）:
- fixed (verified): 11 件
- wontfix (受容): 4 件
- partially-fixed (still open): 1 件（must-fix-004 / round2-new-001）

退行: 0 件。

## 新規発見

### round3-new-001 (should-fix) `ProcessorOutput::data` の `#[serde(flatten)]` 不変条件が deserialize 経路で本当に壊せるか — 実証コード付き

- 場所: `crates/core/src/response.rs:73-82`, テスト群 `response.rs:122-289`
- 観察: Round 2 で「Deserialize 経由は素通し」「公開 `pub data: Value` への直接代入で壊せる」と指摘した点を、現実装で再現させる JSON サンプルを立ててみた。
  - 直代入:
    ```rust
    let bad = ProcessorOutput {
        status: ProcessorStatus::Ok,
        data: serde_json::json!([1, 2, 3]),
    };
    serde_json::to_value(&bad).unwrap(); // panics inside serde-json
    //   thread '...' panicked at: 'can only flatten structs and maps (got an array)'
    ```
    `#[serde(flatten)]` は `serde_json::Value::Array` を許容しない（v0.1 系の serde_json でも v1 でも同様。Value::Number, Value::Bool, Value::String を flatten しても同じ）。`ok()` のガードを通さず直接 struct literal で組むと **serialize 時に panic**（`Result` ではなく panic）。
  - Deserialize 経由:
    ```rust
    let unsafe_json = r#"{"status":"ok"}"#;
    let recovered: ProcessorOutput = serde_json::from_str(unsafe_json).unwrap();
    // recovered.data == Value::Object({}) (空オブジェクト)
    ```
    こちらは empty object に落ちるので panic はしないが、`{"status":"ok"}` を deserialize した後で `recovered.data.get("validation")` のような spec §3.2 必須フィールドのアクセスが暗黙に `None` になる。spec §3.2 の `c2pa-verify` 出力 schema 違反を deserialize 側が**検知できない**。
- 問題: (a) panic-safety が崩れている（`pub` フィールドへの直接代入で serialize 時 panic）、(b) deserialize で spec 違反を検知できない。Round 2 で「acceptable risk」とした wontfix 判定は (a) の panic を踏まえていない可能性が高い。
- 修正案:
  1. `data` を `pub(crate)` に下げて `pub fn new_ok(map: serde_json::Map<String, Value>) -> Self` / `pub fn data(&self) -> &Map<String, Value>` のみを公開する。`pub` フィールドの直接代入経路を物理的に塞ぐ。
  2. もしくは `pub data: serde_json::Map<String, serde_json::Value>` に型変更。`#[serde(flatten)]` が `Map` 限定なので型システムで panic を排除できる。
  3. ついでに deserialize 時に `validate_object_shape` をかける `#[serde(deserialize_with = "...")]` を入れる。
  優先度: should-fix。round2 で `wontfix` の根拠が「下流呼び出し全箇所への波及」だったが、orchestrator 側の `ProcessorOutput::ok(json!({...}))` 呼び出し（`orchestrator.rs:588` ほか）はマップ生成 helper を介すれば波及は限定的。

### round3-new-002 (should-fix) `extract_active_manifest_signature` 内部で同じ content を 2 経路でパース — orchestrator 含めると 1 リクエスト = 3 回 JUMBF パース

- 場所: `crates/core/src/c2pa_verify.rs:203-230`, orchestrator 側 `crates/tee/src/orchestrator.rs:218-240`
- 観察: must-fix-005 (`c2pa::Reader` の 2 回構築) は orchestrator フローレベルでの問題として round2 で wontfix されたが、`extract_active_manifest_signature` 内**だけ**を見ても:
  1. `c2pa::Reader::from_context(...).with_stream(content_type, &mut cursor)` で content をパース → `active_label()` だけ取り出す
  2. `c2pa::jumbf_io::load_jumbf_from_memory(content_type, content)` で **同じ** content を再パースして raw JUMBF を取り出す
  3. 取り出した JUMBF を `jumbf::extract_signature_from_jumbf` でさらに parse

  という三段構成になっている。orchestrator が `compute_signature_hash` → `execute_processors` → `C2paVerifyProcessor::process` → `verify_and_extract` で**さらに** Reader を構築するので、1 リクエストあたり JUMBF 抽出 4 回 + Reader 構築 2 回。
- 問題: パース回数 ≠ 安全性。c2pa-rs の `Reader` は既に `active_label()` と raw manifest 取得 API（`Reader::manifest_store_bytes()` 系）を備えているので、Reader を 1 回構築 → そこから (a) `active_label()` (b) raw JUMBF 取得 (c) 検証結果取得をまとめて済ますことができる。現実装はインタフェース設計上の冗長性。
- 修正案: `c2pa_verify.rs` 内で `Reader` 構築を 1 回にまとめる thin wrapper を作り、`extract_active_manifest_signature` と `verify_and_extract` の両方からそれを呼ばせる。`pub fn verify_with_signature_hash(content, content_type) -> (signature_hash, C2paVerifyOutput)` のように 1 entry を公開して orchestrator 側もそれ 1 回で済ませる、が筋。
  優先度は should-fix。実害は CPU 数 × JUMBF パース時間 × トラフィック量で線形に効くので、v0.1.3 までに整理したい。

### round3-new-003 (nitpick) `find_signature_in_manifest` / `find_cbor_in_box` の `scan_end` バウンドが `manifest_end` と整合的だが、`extract_signature_from_jumbf` の `top_end` だけ `top_header.size` 直値（== 「ファイル先頭からのオフセット」ではなく「先頭が 0 のときたまたま一致する値」）

- 場所: `crates/core/src/jumbf.rs:259, 201`
- 観察:
  - `find_manifest_labels` (`:179`) と `extract_signature_from_jumbf` (`:235`) はどちらも `let top_end = top_header.size;` としている。これは「JUMBF データの先頭から `top_header.size` バイトまで」を意味し、外側の `reader.position() < top_end` ループは `Cursor<&[u8]>` のオフセットが先頭 0 で始まることに依存する。
  - 一方 `find_signature_in_manifest` は呼び出し側から `manifest_end = box_end(child_start, child_header.size)?` を渡される（`:273-274`）。こちらは `child_start` ベース。
- 問題: 同じ「box の終端」概念が「絶対 0 ベース」と「box_start + box_size ベース」に分かれていて、`find_manifest_labels` を別 cursor（オフセット 0 でない部分 slice）から呼ぶと壊れる。現状の呼び出し元は常に raw JUMBF 全体を `Cursor::new(jumbf_data)` で渡しているので動いている。
- 修正案: `top_end = box_end(0, top_header.size)?` に書き換え、box_start を `let top_start = reader.position(); /* == 0 */` で陽に取り出して一貫性を保つ。defensive にしておくと将来 `find_manifest_labels` をネスト box 走査で再利用できる。

### round3-new-004 (nitpick) `ProcessorRegistry::execute` がエラー時に `e.to_string()` で型情報を捨てている

- 場所: `crates/core/src/processor.rs:132-144`
- 観察:
  ```rust
  Err(e) => ProcessorOutput::error(e.to_string()),
  ```
  `ProcessorError::UnsupportedContentType { content_type }` などのバリアント情報が文字列化されて潰れる。下流の orchestrator / API gateway 側で「`UnsupportedContentType` だけ別 HTTP ステータスにしたい」ような将来要求に対し、文字列パターンマッチか e2e 修正のいずれかが必要になる。
- 問題: spec §3.1 の `{"status":"error","error":"unsupported format"}` は文字列前提で書かれているので spec 整合は OK。ただし API 安定性の観点で、エラー型情報を保持して `ProcessorOutput::error_with_kind(error_kind, message)` の形にしておく将来余地がある。
- 修正案: 短期では現状維持で OK。中期では `ProcessorOutput` に `#[serde(skip_serializing_if = "Option::is_none")] pub error_kind: Option<String>` を追加して `e` のバリアント discriminator を一緒に持たせる、を spec §3.1 の改訂と合わせて。

### round3-new-005 (nitpick) spec §3.1 末尾の JSON 例 `some-proc` が「現存しない processor」名を残したまま — core 実装が `c2pa-verify` 1 個に整理されたのと不整合

- 場所: `docs/v0.1.2/SPECS_JA.md:709-714`
  ```json
  "results": {
    "c2pa-verify": { "status": "ok", ... },
    "image-pdq":   { "status": "ok", ... },
    "some-proc":   { "status": "error", "error": "unsupported format" }
  }
  ```
- 観察: K8 観点は core だが、core が「逐次実行で processor は c2pa-verify のみ」に整理された結果、spec §3.1 の `image-pdq` / `some-proc` は v0.1.2 では実在しない processor 名を仕様例として残している。round2-new-003 で指摘した「逐次 vs 並列の四重乖離」と表裏。Q (SPECS_JA self audit) の領分でもあるが、core 起点で気付くのでここに残す。
- 修正案: spec §3.1 の例を v0.1.2 現実に合わせて `image-pdq` / `some-proc` を「将来追加され得る processor の例」と明記するか、もしくは `c2pa-verify` のみの単一エントリ JSON にする。または v0.1.3 で複数 processor が実装されるまで example を保留する旨を 1 行入れる。

### round3-new-006 (nitpick) `EncryptionSuite::from_suite_id` を core 内 + 依存 crate から呼んでいる箇所が見当たらない — dead public API の疑い

- 場所: `crates/core/src/request.rs:112-119`
- 観察: `suite_id(&self) -> u8` は decrypt のワイヤーフォーマット組立側で呼ばれる想定（spec §2.4 のバイナリ先頭 1 バイト）が、対称となる `from_suite_id` を core / tee / sdk のどこから呼ぶ設計だったかが現コードからは追えない。Round 1 nitpick-003 で「`&self` / `&Self` 非対称」が指摘済みだが、これは「dead public API かも」という別の問題。
- 問題: round2 で wontfix された API 整理対象（nitpick-001..007 群）の中でも、`from_suite_id` だけは「実装は正しいが呼び出し元が不在」の dead public API 疑いがある。`suite_id` だけならクライアント側のシリアライズで使う論理が立つが、`from_suite_id` は decrypt 側のワイヤーフォーマット parse で必要なはずで、それが orchestrator 側にも見えない。
- 修正案: `crates/tee/src/orchestrator.rs:207` 周辺の `decrypt_single_payload` で `EncryptionSuite::from_suite_id` を実際に呼んでいるかを再確認し、未使用なら削除（または `pub(crate)` 化）。使われているのに grep に出ないなら、それは別ファイルに移動済みか re-export 経由なので doc を整える。OSS 公開前の API trim に含めて良い。

### round3-new-007 (nitpick) `c2pa_verify.rs` テストの `create_signed_jpeg` 呼び出しが 7 回 — `OnceLock` キャッシュで CI 時間が線形に縮む余地

- 場所: `crates/core/src/c2pa_verify.rs:434, 472, 484, 491, 500, 536, 549, 562, 581, 597, 622, 637`
- 観察: round2 should-fix-010 で「wontfix（OnceLock キャッシュは coverage 改善案）」と判定されたが、独立試験 12 回（`create_signed_jpeg()` 7 回 + `create_signed_jpeg_with_actions(...)` 1 回 + `create_test_jpeg()` 直接 4 回）の実体を改めて数えると、`c2pa::Builder::sign` が test 1 件あたり数十 ms かかる前提では cumulative で測定可能な CI 時間ロスになる。
- 問題: 機能的問題ではなく CI コスト。実害は低いが、`once_cell` ベースの `static SIGNED_JPEG: OnceLock<Vec<u8>> = OnceLock::new();` を作れば 7 回 → 1 回に減らせる。
- 修正案: round2 受容を維持してもよいが、OSS 公開前の cleanup 候補として明記。

### round3-new-008 (nitpick) `process_unsigned_content_returns_error` / `process_garbage_content_returns_error` は `ProcessorError::C2paVerificationFailed` を期待するが、c2pa-rs が将来 error variant を分割した場合のテスト脆弱性

- 場所: `crates/core/src/c2pa_verify.rs:471-495`
- 観察:
  ```rust
  match result {
      Err(ProcessorError::C2paVerificationFailed(_)) => {} // expected
      other => panic!("Expected C2paVerificationFailed, got: {other:?}"),
  }
  ```
  c2pa-rs の `Reader::with_stream` がエラーを返す経路は現状すべて `extract_active_manifest_signature` / `verify_and_extract` 内で `ProcessorError::C2paVerificationFailed(format!("..."))` にラップされるので OK。しかし `Reader` 構築失敗 vs 検証失敗 vs JUMBF 抽出失敗 の区別が文字列内 prefix だけに依存しているので、c2pa-rs 0.85+ で error variant が増えたときに「`ParseFailed` 側に分岐させたい」というニーズが出ても、現テストが variant 区別を要求しない以上、本物の分岐ロジックを退行検出できない。
- 問題: テスト網が「とにかく Err」を確認するレベルに留まっている。`ProcessorError::ParseFailed` が `lib.rs` で `pub` 公開され dead variant 化（round1 nitpick-004）している事実と組み合わさると、「variant を増やしても誰も気付かない」状態。
- 修正案: 短期は現状維持で OK。中期で `ProcessorError::ParseFailed` を実際に使い分けたいなら、エラー variant 別の `assert!(matches!(...))` を追加する。Round 2 nitpick-004 と統合対応する候補。

## 全体所感

Round 2 で `fixed` 判定された 9 件は round3 でもすべて verify でき、退行はゼロ。Round 2 で `wontfix` にした 7 件（must-fix-004, must-fix-005, should-fix-001/002/010, nitpick-001..007）も、その大半は「v0.1.3 / OSS 公開前 cleanup」のラベル付きで送ったものとして筋が通っている。

ただし以下 2 点は round3 で再開封する価値がある:

1. **`ProcessorOutput.data` の型レベル不変条件 (round3-new-001)**: round2 で `acceptable risk` とされたが、`pub data: Value` への直接代入で **serialize 時 panic** する経路が型から塞がれていないことは API 安定性として明確な傷。`Map<String, Value>` 化（または `pub(crate)` + ctor）で型レベルに昇格させるのが望ましい。
2. **`extract_active_manifest_signature` の三段パース (round3-new-002)**: round2 では orchestrator 側の 2 回構築としてだけ把握されていたが、core 単独で見ても Reader 構築 + `load_jumbf_from_memory` + `extract_signature_from_jumbf` の三段構成で、これは「Reader 1 回 → そこから raw JUMBF + active_label + 検証結果をまとめ取り」する API 整理で削れる。orchestrator 全体の refactor を待たずに core 内だけで前進できる。

新規発見 8 件のうち should-fix は上記 2 件のみで、残り 6 件は nitpick（spec 例の現実乖離、dead public API 疑い、テスト脆弱性、CI コスト、jumbf bounds 一貫性、エラー型情報の保持）。退行はゼロ、構造的悪化もない。

Round 1: 22 件 → Round 2: 7 件 → Round 3: 8 件（うち should-fix 2 + 既存 partially-fixed の継続 1）。修正サイクルは収束しているが、`ProcessorOutput` の型安全性と c2pa-verify の Reader/JUMBF 経路重複の 2 系統は v0.1.3 で決着させたい構造的負債として残る。

---

## 処理ログ（Round 3 自判定）

| ID | 判定 | 内容 |
|---|---|---|
| must-fix-001/002/003 (継承) | fixed (verified) | round2 認定を再確認。退行なし |
| must-fix-004 / round2-new-001 / round3-new-001 | open(should-fix 相当) | `pub data: Value` の直接代入による serialize panic 経路が型から塞がれていない。`Map<String, Value>` 化または `pub(crate)` + ctor を v0.1.3 で適用すべき |
| must-fix-005 / round3-new-002 | open(should-fix 相当) | core 単独で見ても `extract_active_manifest_signature` の三段パースが残る。Reader 1 回 + raw JUMBF + verify をまとめる thin wrapper の導入で前進可能 |
| should-fix-001/002/010 (継承) | wontfix (round2 受容を維持) | v0.1.3 SDK / 並列実装 / 仕様改訂と合わせて対応 |
| should-fix-003/004/005/006/007/008/009 (継承) | fixed (verified) | round2 認定を再確認 |
| round2-new-002/003 | wontfix (round2 受容を維持) | spec 改訂 / 並列実装側の決着待ち |
| round2-new-004/005/006/007 | fixed (verified) | round2 認定を再確認 |
| round3-new-001 | fixed | `ProcessorOutput::data` の型を `serde_json::Value` から `serde_json::Map<String, Value>` に変更。`#[serde(flatten)]` が要求する「object 形」を型レベルで強制し、配列やスカラーを直接代入して serialize 時 panic する経路を物理的に塞いだ。既存呼び出し側の `ProcessorOutput::ok(json!({...}))` パターンは `from_value_object` ヘルパ経由に整理 (`crates/core/src/processor.rs`, `crates/tee/src/orchestrator.rs`, `crates/solana/src/extension.rs`)。 |
| round3-new-002 | wontfix | `extract_active_manifest_signature` の 3 段パース整理は c2pa-rs API 変更含め範囲が広く、v0.1.3 で本格 refactor。当面は機能優先。 |
| round3-new-003 | fixed | `jumbf.rs` の `find_manifest_labels` (`:201`) と `extract_signature_from_jumbf` (`:259`) の `top_end` を `box_end(0, top_header.size)` に書き換え。box_start ベースで一貫させ、将来 nested box 走査で再利用しても壊れない形に。 |
| round3-new-004 | wontfix | `e.to_string()` で variant 情報破棄は spec §3.1 が文字列前提なので整合。将来要求が出た時点で `error_kind` を追加する余地は残る。 |
| round3-new-005 | wontfix(Q観点) | SPECS §3.1 の `some-proc` 例は SPECS 自体の問題。Q 監査の判定 (v0.1.3 SPECS リライト時に一括整理) と整合。 |
| round3-new-006 | wontfix(誤指摘) | `EncryptionSuite::from_suite_id` を grep し直したところ、`crates/crypto/src/wire.rs:47` (wire parser) と `crates/tee/src/orchestrator.rs:294` (suite mismatch エラー組み立て) で実際に使われていた。dead API ではない。 |
| round3-new-007 | wontfix | テスト fixture の `create_signed_jpeg` 重複呼びは CI 最適化の話。OSS 公開前のクリーンアップ候補。 |
| round3-new-008 | wontfix | `ProcessorError` variant 別の `matches!` テスト不在は c2pa-rs 0.84 系が安定している現状では実害なし。c2pa-rs 側 API 拡張時に再評価。 |
