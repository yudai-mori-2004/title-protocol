# K8: `crates/core` 縦深掘り

## 概要

担当範囲: `crates/core/` 全ファイル — `Cargo.toml`, `src/lib.rs`, `src/error.rs`, `src/request.rs`, `src/response.rs`, `src/processor.rs`, `src/processor_outputs.rs`, `src/c2pa_verify.rs`, `src/jumbf.rs`。

監査方針: `docs/v0.1.2/SPECS_JA.md` §1〜3 を頭に入れた上で、各 `.rs` を 1 文 1 文読み、

1. 仕様（§1.3, §2.2, §2.3, §3.1, §3.2）との JSON 表現整合性
2. 公開 API の妥当性（`pub` の過多、未使用シンボル、ダブルエクスポート）
3. `#[serde(flatten)]` 周りの未定義挙動
4. `c2pa-rs 0.84` の使用作法（重複パース、エラー伝播）
5. `jumbf` パーサの I/O・サイズ制限・ASCII 仮定
6. `Processor` trait と `ProcessorRegistry` の API 設計

を観点に通読した。件数: **22 件**。

## 重大度別内訳

- must-fix: 5 件
- should-fix: 10 件
- nitpick: 7 件

## 発見

### must-fix-001 `processor_outputs` 内の DAG/PDQ/Cert 出力型は丸ごとデッドコード

- 場所: `crates/core/src/processor_outputs.rs:78-231`
- 観察: `ProvenanceGraphOutput`, `GraphNode`, `GraphEdge`, `ImagePdqOutput`, `VideoVpdqOutput`, `FrameHash`, `CertVerifyOutput`, `CertChainEntry` の 8 型が `pub` で定義されている。
- 検証: `grep -rn "ProvenanceGraphOutput\|ImagePdqOutput\|VideoVpdqOutput\|CertVerifyOutput\|GraphNode\|GraphEdge\|FrameHash\|CertChainEntry" crates/` の結果、これらは `processor_outputs.rs` 自身以外では一度も参照されていない（テスト含む `c2pa_verify` / `orchestrator` / `tee` のどこにも来ていない）。
- 問題: v0.1.2 では c2pa-verify 以外の processor は実装されておらず（spec §3.3 で「処理は再ビルドで追加」と書かれている）、これらは「将来こうなる」の予示型でしかない。クローンした人は「これに対応する processor 実装がどこ？」と探して空振る。`pub` で API 面積に出ているため OSS 公開時のセマンティック・バージョニングの足枷にもなる。
- 修正案: **削除**。または `processor_outputs` モジュール自体を削除して `c2pa-verify` 出力型（`C2paVerifyOutput`, `SignerInfo`, `C2paAction`）だけを `c2pa_verify.rs` 内に同居させる。将来 processor が増えた時に、その processor を実装するクレートが対応する出力型を持てばよい（core で集約する理由がない）。

### must-fix-002 `CoreError` が未使用のまま公開 API になっている

- 場所: `crates/core/src/error.rs:10-29`, `crates/core/src/lib.rs:32`
- 観察: `pub enum CoreError { InvalidRequest, UnknownProcessor, Json, Internal }` が定義され `lib.rs` で re-export されている。
- 検証: `grep -n "CoreError" crates/` の結果、`error.rs` の定義と `lib.rs` の re-export 以外では使われていない。`c2pa_verify`, `orchestrator`, `tee` のどこも `Result<_, CoreError>` を返していない。
- 問題: 「title-core のトップレベルエラー型」と doc comment に書かれているが、その実体としての関数が存在しない。型は存在するのに produce する場所がないため、利用者から見ると「これはいつ返ってくるのか？」がわからない。
- 修正案: **削除**（`error.rs` ごと、`lib.rs` の `pub mod error;` と `pub use error::CoreError;` も削除）。`ProcessorError` で十分足りている。将来 core 内で別種のエラーが必要になった時点で追加すればよい。

### must-fix-003 `jumbf` モジュールは private なのに内部関数が `pub`

- 場所: `crates/core/src/lib.rs:21` (`mod jumbf;`)、`crates/core/src/jumbf.rs:209` (`pub fn extract_signature_from_jumbf`)
- 観察: `lib.rs` で `mod jumbf;`（非 `pub mod`）として閉じているにもかかわらず、`extract_signature_from_jumbf` だけが `pub fn`、`find_manifest_labels` は `pub(crate)`。
- 問題: 可視性が混乱している。private モジュール内の `pub fn` は実質 `pub(crate)` と同じだが、読み手は「外部に公開するつもりがあるのか？」を判断できない。`c2pa_verify.rs:140` から呼ばれているだけなのに `pub` を付ける意味がない。
- 修正案: `extract_signature_from_jumbf` を `pub(crate) fn` に統一する。`MAX_SIGNATURE_SIZE` 等の定数も含めて、外部に出る予定がないなら `pub(crate)` で固定する。

### must-fix-004 `ProcessorOutput` の `#[serde(flatten)]` で `data` が非オブジェクトの場合の挙動が未定義

- 場所: `crates/core/src/response.rs:73-82`
- 観察:
  ```rust
  pub struct ProcessorOutput {
      pub status: ProcessorStatus,
      #[serde(flatten)]
      pub data: serde_json::Value,
  }
  ```
- 問題: `#[serde(flatten)]` は serde の仕様上、内側がマップ型である場合のみ正しく機能する。`data` を `serde_json::json!(42)` や `serde_json::json!([1,2,3])` で構築した場合、シリアライズは成功するが結果 JSON は壊れる（または serde 内部のパニックすれすれの挙動になる）。`ProcessorOutput::ok()` / `error()` のコンストラクタは型シグネチャ上 `serde_json::Value` を受け取り、非オブジェクトを弾かない。
- 修正案: (a) `data` を `serde_json::Map<String, Value>` に変える（呼び出し側で `as_object()` の検査が必要）、または (b) `ProcessorOutput::ok` のシグネチャを `impl Serialize` にして内部で `to_value` → `as_object()` チェックを行い、非オブジェクトなら `error()` に振り替える。`#[serde(flatten)]` の前提を型レベルで担保する。

### must-fix-005 `c2pa::Reader::from_context(...).with_stream(...)` が同一コンテンツに対し 2 回呼ばれる

- 場所: `crates/core/src/c2pa_verify.rs:159-164` および `c2pa_verify.rs:191-196`
- 観察: `extract_active_manifest_signature` と `verify_and_extract` がそれぞれ `Reader::from_context(...).with_stream(content_type, &mut cursor)` を独立に呼んでいる。両者は `process()` → `verify_and_extract()` と、`compute_signature_hash()` → `extract_active_manifest_signature()` から別ルートで呼ばれるが、TEE オーケストレータ（`tee/orchestrator.rs:202`）は **同じ content に対し両方を呼ぶ**ため、c2pa パースが事実上 2 回走る。
- 問題: c2pa-rs の `Reader` 構築は JUMBF 全走査・COSE 検証を伴う比較的高コストな処理。MP4 のような大容量ファイルでは Range Request の往復含めて顕著な遅延要因になりうる。
- 修正案: `Reader` 構築結果（または raw JUMBF + active_label）をキャッシュ可能な構造体にして、`process()` と `compute_signature_hash()` の両方が引数として受け取れる API にする。例:
  ```rust
  pub struct C2paReadContext { /* reader, jumbf_bytes, active_label */ }
  pub fn read_c2pa(content: &[u8], content_type: &str) -> Result<C2paReadContext, ProcessorError>;
  ```
  そのうえで `C2paVerifyProcessor::process(&ctx)` と `compute_signature_hash(&ctx)` が共有する。これは API 破壊変更だが、現在 v0.1.2 で `core` を外部利用するのは workspace 内のみなので影響範囲は orchestrator 1 箇所に閉じる。

### should-fix-001 `ProcessorRegistry::execute` が逐次実行で「並列」の仕様と矛盾

- 場所: `crates/core/src/processor.rs:118-143`
- 観察:
  ```rust
  /// 注: 現在は逐次実行。並列実行はTEEオーケストレーション層で実装する。
  pub fn execute(...) -> HashMap<String, ProcessorOutput> {
      for id in processor_ids { ... }
  }
  ```
- 問題: 仕様書 §1.3「指定された全processorが並列に実行され」、§3.1「processor間に実行順序の制約は存在しない」と明記されているのに、`Registry` 直接利用者は知らずに逐次実行する。doc comment で「オーケストレーション層で並列化」と書いているが、現状 orchestrator 側も `Registry::execute` を呼んで終わり（並列化していない）。仕様と実装の二重の乖離。
- 修正案: 真の並列化が必要なら `rayon::par_iter` を core に持ち込むか、`Registry::execute` を削除して trait の呼び出しを orchestrator に寄せる。または「現行 c2pa-verify 単一構成では並列化の必要なし」と仕様側で書き直す（spec §3.1 を v0.1.2 の現実に合わせる）。どちらにせよ、doc comment と実装と仕様の三者が現在矛盾しているため整理が必要。

### should-fix-002 sidecar / fragmented の暗号化未対応がリクエスト型レベルで防がれていない

- 場所: `crates/core/src/request.rs:27-43`
- 観察: `ProcessRequest { input: InputData, encryption: Option<EncryptionSuite>, ... }` の構造上、`InputData::Fragmented` や `InputData::Sidecar` と `Some(encryption)` を組み合わせたリクエストが serde レベルでは合法。
- 仕様: §2.2「本仕様では `input_type: "single"` に限り暗号化に対応する」と明記。
- 問題: 型システムで弾けるのに弾いていない。orchestrator 側で実行時チェックしているのなら、それは仕様の意図を型に落とせていない設計。
- 修正案: `ProcessRequest::validate(&self) -> Result<(), CoreError>` を生やして「Sidecar + encryption は spec §2.2 違反」を返す。あるいは `enum ProcessRequest { Plain(InputData), EncryptedSingle { content_url, encryption } }` のように型自体で表現する（こちらは API 破壊だがより堅牢）。

### should-fix-003 `read_desc_info` の `read_so_far` 計算が読みづらく、型と演算子優先順位の罠

- 場所: `crates/core/src/jumbf.rs:133-134`
- 観察:
  ```rust
  let read_so_far =
      16 + 1 + if label.is_empty() { 0 } else { label.len() + 1 } as u64;
  ```
- 問題: `as u64` は `if` 式全体ではなく `label.len() + 1` だけにかかる（Rust の precedence では `as` は項単位）。実際にはコンパイラが両辺の型推論で帳尻を合わせるが、人が読んで「どこが u64 になるか」がぱっとわからない。仕様読解の前段で躓く。
- 修正案:
  ```rust
  let label_bytes: u64 = if label.is_empty() { 0 } else { label.len() as u64 + 1 };
  let read_so_far: u64 = 16 + 1 + label_bytes;
  ```
  に書き直す。ついでに 16・1 にも `// uuid + toggles` のコメントを付ける。

### should-fix-004 JUMBF ラベルを `u8 as char` で読むため非 ASCII で silently 化け

- 場所: `crates/core/src/jumbf.rs:128`
- 観察: `label.push(byte[0] as char);` 直前のコメントは「C2PA labels are ASCII-only, so byte-by-byte reading suffices」。
- 問題: 「ASCII 前提」を validation していない。`u8 as char` は Latin-1 として解釈するので、攻撃者が制御文字や非 ASCII を含むラベルを送ると黙って「化けたラベル」になり、後段の `desc.label == manifest_label` 比較は決して一致しなくなる。`C2paVerificationFailed("Manifest not found")` で落ちるので致命的ではないが、デバッグ困難。
- 修正案:
  ```rust
  if !byte[0].is_ascii() {
      return Err(ProcessorError::C2paVerificationFailed(
          "Non-ASCII byte in JUMBF label".into()));
  }
  ```
  を `push` の直前に挟む。あるいはラベル全体を `Vec<u8>` で読んでから `std::str::from_utf8` で検証する。

### should-fix-005 `MAX_SIGNATURE_SIZE = 16 MiB` の根拠不明

- 場所: `crates/core/src/jumbf.rs:36-37`
- 観察: `const MAX_SIGNATURE_SIZE: u64 = 16 * 1024 * 1024;`
- 問題: COSE_Sign1 の署名サイズは現実には数 KB（ECDSA: ~70 B、証明書チェーン込みでも < 32 KB）。16 MiB は「上限なし」と実質変わらず、OOM 防御の意義が薄い。
- 修正案: 仕様調査の上で、現実的な上限（例: 256 KiB）に下げる。`MAX_SIGNATURE_SIZE` の値根拠を doc comment に書く（「証明書チェーン最大 N 段 × 1 枚 ~4 KiB = …」のような）。

### should-fix-006 `read_header` が EOF を `(box_type=0, size=0)` のセンチネルで返している

- 場所: `crates/core/src/jumbf.rs:53-87`
- 観察:
  ```rust
  if reader.read(&mut buf)? < 8 {
      return Ok(BoxHeader { box_type: 0, size: 0 });
  }
  ```
- 問題: 「EOF を成功扱いで返し、呼び出し側が `size == 0` で break する」というセンチネル値パターン。box_type / size のどちらも実値として 0 を取らない保証はなく（拡張型サイズで size=0 が「end of file marker」を意味する仕様分岐もありうる）、コードの意図が読み取りづらい。
- 修正案: `fn read_header(...) -> Result<Option<BoxHeader>, ProcessorError>` に変更し、EOF を `Ok(None)` で表現する。呼び出し側は `while let Some(h) = read_header(&mut r)?` でループする。

### should-fix-007 `find_manifest_labels` が「last == active」と仮定している

- 場所: `crates/core/src/c2pa_verify.rs:132-137`
- 観察:
  ```rust
  let labels = jumbf::find_manifest_labels(manifest_data)?;
  let active_label = labels.last().ok_or_else(|| ...);
  ```
- 問題: doc comment にも「The active manifest is conventionally the last one in the list」と書かれているが、これは慣例であって規格上の保証ではない。サイドカー .c2pa では C2PA Reader を経由できないため代替手段がないのは事実だが、`last()` を真とする根拠の出典（C2PA spec の章節）を doc に書くべき。
- 修正案: C2PA spec の該当箇所（おそらく "active manifest" の定義部）を doc comment に明記する。または compute_signature_hash_from_manifest_data 自体を廃止し、サイドカー入力でも `c2pa::Reader::from_manifest_data_and_stream` 経由で active_label を取得する（c2pa-rs 0.84 にこの API があるかは要確認）。

### should-fix-008 `lib.rs` の re-export が public 面で二重露出している

- 場所: `crates/core/src/lib.rs:19-35`
- 観察:
  ```rust
  pub mod c2pa_verify;
  pub mod error;
  mod jumbf;
  pub mod processor;
  pub mod processor_outputs;
  pub mod request;
  pub mod response;

  pub use c2pa_verify::{...};
  pub use error::CoreError;
  pub use processor::{...};
  pub use request::{...};
  pub use response::{...};
  ```
- 問題: モジュール自体も `pub`、その中の主要型も `pub use` で flat 再公開。利用者は `title_core::C2paVerifyProcessor` でも `title_core::c2pa_verify::C2paVerifyProcessor` でも参照できてしまい、どっちが正典か不明。OSS の API 設計としては片方を選ぶべき（rustdoc 上も重複表示になる）。
- 修正案: モジュールを `pub(crate)` に閉じ、flat な `pub use` だけを公開 API として扱う。または逆に `pub use` を削除して module path での参照に統一する。`processor_outputs` 等は前者を採用し、`c2pa_verify::compute_signature_hash_from_manifest_data` のような細かい関数も flat re-export している現状はノイズ。

### should-fix-009 `ProcessorError` 派生に `Clone` がなく、テスト用に `impl Clone` が混入している

- 場所: `crates/core/src/processor.rs:60-80`（定義）、`crates/core/src/processor.rs:176-189`（テスト内の `impl Clone`）
- 観察: 本体は `#[derive(Debug, thiserror::Error)]` のみ。テストモジュールに `impl Clone for ProcessorError { ... }` が手書きされている。
- 問題: テスト用の `impl` を本体側の同じ型に被せると、本体に `Clone` が必要になった時に二重定義になる。テスト固有の事情が本体に染み出している。
- 修正案: 本体側で `#[derive(Debug, Clone, thiserror::Error)]` を宣言する（現状のバリアントは全て `String` / プリミティブで Clone 可能）。テスト側の `impl Clone` ブロックは削除する。

### should-fix-010 `core` クレートが `image` を dev-dep として持ち、テストが C2PA 署名付き JPEG を毎テスト生成

- 場所: `crates/core/Cargo.toml:20`、`crates/core/src/c2pa_verify.rs:294-377`
- 観察: テスト fixture を `image::ImageBuffer::from_fn` で 4x4 JPEG を作り、`c2pa::EphemeralSigner` で毎テスト署名している。`create_signed_jpeg` は `process_signed_content`, `signature_hash_deterministic`, `signature_hash_differs_for_different_content`, `output_matches_spec_json_structure`, `output_roundtrip_through_c2pa_verify_output`, `execute_via_registry` から呼ばれる（少なくとも 6 テスト）。
- 問題: c2pa 署名は ECDSA + 証明書チェーン構築を伴うため 1 回数百ミリ秒〜秒。テスト実行が遅くなる + flaky の温床（時刻依存）。`image` 依存も `core` の本体ロジックには不要で、テストのためだけに dev-dep を持っている。
- 修正案: 署名済み fixture を `tests/fixtures/signed.jpg` として 1 枚コミットし、`include_bytes!` で読む。`EphemeralSigner` 周りのテストは「signature_hash の決定性」を担保する 1 つだけに絞り、それ以外は fixture 利用に切り替える。`image` dev-dep は削除。

### nitpick-001 `c2pa_verify.rs` だけ doc comment が英語、他は日本語

- 場所: `crates/core/src/c2pa_verify.rs` 全体、対する `crates/core/src/request.rs`, `response.rs`, `processor.rs`, `processor_outputs.rs`, `error.rs`, `jumbf.rs` は日本語
- 問題: `jumbf.rs` は英語、`c2pa_verify.rs` も英語、他は日本語と統一感がない。CLAUDE.md には「Doc comments with spec section references」とあるが言語は規定なし。OSS 公開を志向するなら英語統一、内部開発フェーズに留まるなら日本語統一、いずれかに揃える。
- 修正案: プロジェクト全体方針として doc comment 言語を統一する（README / CONTRIBUTING に明記）。
- 重大度: nitpick（機能には無関係、ただし読者体験を損なう）

### nitpick-002 doc comment 中の JSON 例が "..." で truncate されている

- 場所: 例 `crates/core/src/response.rs:39-44`
  ```json
  "signature_hash": "sha256:abcdef1234...",
  "c2pa-verify": { "status": "ok", "validation": "valid", ... }
  ```
- 問題: 「`...`」を含む文字列はそのままパースすると不正な JSON。doc コピペで動かしてみる読者がいる前提だと混乱の元。
- 修正案: `"abcdef1234567890..."` のように先頭 hex で省略を表現するか、`...` を `// truncated` のような行コメントに分離（JSON5 想定）するか、または短い完結例にする。

### nitpick-003 `EncryptionSuite::suite_id` と `from_suite_id` が `&self` / `u8` の非対称

- 場所: `crates/core/src/request.rs:99-119`
- 観察:
  ```rust
  pub fn suite_id(&self) -> u8 { ... }
  pub fn from_suite_id(id: u8) -> Option<Self> { ... }
  ```
- 問題: `suite_id` は `&self` を取るが `EncryptionSuite` は `Copy`。`fn suite_id(self) -> u8` で十分。
- 修正案: `pub fn suite_id(self) -> u8`。`TryFrom<u8>` を実装して `from_suite_id` を消すのも一案。

### nitpick-004 `ProcessorError::ParseFailed` と `UnsupportedContentType` が未使用

- 場所: `crates/core/src/processor.rs:62-71`
- 観察: `C2paVerifyProcessor` は `C2paVerificationFailed` と `Internal` しか produce しない（`grep "ProcessorError::" crates/core/src/c2pa_verify.rs` で確認）。他に producer は無い。
- 問題: 将来用に置いている割に doc が「将来用」と書かれていない。読み手は「どこから飛んでくるのか」を探す。
- 修正案: 当面使わないバリアントを削除する。新 processor を追加する時にその processor が必要なバリアントだけ追加すればよい。

### nitpick-005 `ProcessResponse` / `VerifiableResponse` / `ProcessorOutput` に `PartialEq` がない

- 場所: `crates/core/src/response.rs:21,47,73`
- 観察: `ProcessRequest` 系には `#[derive(... PartialEq, Eq, ...)]` があるのに、response 系には付いていない。
- 問題: 統合テストで「期待 response と実 response の比較」を書くときに `assert_eq!` できない。型システム的に不揃い。
- 修正案: `ProcessorOutput.data: serde_json::Value` は `PartialEq` を実装しているので、`#[derive(... PartialEq)]` を追加可能。`VerifiableResponse` / `ProcessResponse` も同様。`Eq` は `serde_json::Value` が浮動小数を含むため不可だが `PartialEq` は付けられる。

### nitpick-006 `extract_actions` が `c2pa.actions.v2` と `c2pa.actions` の両方を試すが doc が不正確

- 場所: `crates/core/src/c2pa_verify.rs:268-273`
- 観察: doc comment「Looks for both `c2pa.actions` and `c2pa.actions.v2` assertion labels」、実装は `Actions::LABEL_VERSIONED` (=v2) を先に試して、無ければ `Actions::LABEL` (=無印) にフォールバック。
- 問題: c2pa-rs 提供の定数を使っているのは正しいが、コード上のリテラルが存在しないので、`grep` で「c2pa.actions」を探すと doc コメントしか hit しない。コード読解の手がかりが弱い。
- 修正案: doc に `Actions::LABEL_VERSIONED` (= "c2pa.actions.v2") と書く。

### nitpick-007 `build_claim_generator_string` が `"{name} {ver}"` 形式に整形するが spec 例に明記なし

- 場所: `crates/core/src/c2pa_verify.rs:248-262`
- 観察: spec §3.2 例は `"claim_generator": "Google Pixel 10 Camera"`。実装は `claim_generator_info[0].name + " " + version` で連結。
- 問題: spec が「name + space + version」連結を要求しているわけではないが、ヒトの読みやすさで連結している。spec と実装の対応が doc に書かれていない。
- 修正案: doc comment に「c2pa-rs の `claim_generator_info` は構造体だが、spec §3.2 の出力は string 1 本なので `{name} {version}` 形式に flatten する」と明示する。あるいは spec §3.2 に逃げ道（「文字列に丸める」）を書き込む。

## 全体所感

`crates/core` は protocol の型定義レイヤとして役割は明確で、テストカバレッジは（request / response の serde 往復、c2pa-verify の主要パスを含めて）まずまず取れている。一方で、

- 仕様策定者の意図でフライング配置された型（`processor_outputs` の 8 型、`CoreError`）が **dead public API** として残っている
- `#[serde(flatten)]` の前提が型レベルで担保されていない（`ProcessorOutput.data: Value`）
- `c2pa-rs Reader` の二重構築のような実コスト面の非効率
- public/private の境界が混乱している（`pub fn` in `mod jumbf;`、re-export の二重露出）

の 4 系統で「フェーズ移行に伴う剥がし忘れ」が散見される。dead code（must-fix-001, 002）と #[serde(flatten)] の罠（must-fix-004）は外部利用者が増える前に決着させるべき。それ以外は実害より「読み手の負担」寄りで、優先度を見ながら 17 タスクで順次処理して問題ない。
