# K1. Attestation crates 縦深監査 — Round 3

## 概要

担当範囲: `crates/attestation/` 全ファイル、`crates/attestation-aws-nitro/` 全ファイル（`tests/fixtures/*.report` バイナリ除外）。

Round 2 で記録された判定（必要に応じ wontfix 含む）が現コードベースに正しく反映されているかを file:line 単位で再確認し、Round 2 の修正で混入した可能性のある regression と Round 3 で初めて気付いた問題を追加した。

## Round 2 指摘の処理状況

Round 2 の処理ログを基準とし、現在のソース状態をクロスチェックする。

### Round 1 由来（Round 2 で fixed / partially-fixed / wontfix と判定済）

| ID | Round 2 判定 | Round 3 検証 | 検証ポイント |
|---|---|---|---|
| must-fix-001..007 | fixed | **confirmed** | `cert.rs:77` `Cert::verify(&self, issuer: &Self) -> anyhow::Result<bool>`（Option 無し）。`cert.rs:142-157` `verify_chain` は `for i in 1..self.certs.len()` で root の自己署名検証をしない。`doc.rs:85-89` で root pin を SHA-256 比較。`doc.rs:55` `authenticate(timestamp)` に `trusted_certs_len` 引数なし。`doc.rs:58-63` で `digest == "SHA384"` 早期ガード。`cose.rs:64-74` で protected header の未知キー reject。`sign.rs:105-140` `verify_signature_der` は `P256Signature::from_der` / `P384Signature::from_der` 直呼び、`pad_zero_to_length` / `ec_decode_sig` / 独自 DER walker は痕跡なし。`KeyAlgo` / `SigAlgo` 双方 `EcdsaP256` + `EcdsaP384` / `EcdsaSHA256` + `EcdsaSHA384` のみ（`sign.rs:30-33`, `sign.rs:61-64`）。RSA dep は `Cargo.toml` に皆無。`attestation-aws-nitro/Cargo.toml` から `sha2_sp1` の `features = ["oid"]` も除去済。 |
| should-fix-001 | fixed | **confirmed** | `lib.rs:36-40` で root pin の責務記述に書き直し済。 |
| should-fix-002 | fixed | **confirmed** | `lib.rs:55-65` で `now_unix_secs` をそのまま `authenticate()` に流す（`min()` 折り畳みなし）。 |
| should-fix-003 | Round 2 で再 fixed（`CertChainInvalid` を削除） | **confirmed** | `attestation/src/lib.rs:46-56` で `AttestationError` は `ParseFailed` / `SignatureInvalid` / `MissingField` の 3 variant のみ。`CertChainInvalid` の文字列は workspace 全体で出現せず（`grep CertChainInvalid` 結果 0 件）。 |
| should-fix-004 | wontfix（`MockAttestationVerifier::PREFIX` を `pub` のまま残置） | **acknowledged** | `attestation/src/lib.rs:108` で `pub const PREFIX: &'static [u8] = b"mock-attestation:";`。`crates/solana/src/extension.rs:201` で `MockAttestationVerifier::PREFIX.to_vec()` として直接使用。Round 2 の判断は妥当（mock helper の重複実装回避）だが、`mock` feature は `crates/tee/src/main.rs:29` / `server.rs:417` / `gateway/tests/e2e.rs:76` / `solana/src/extension.rs:181` の 4 箇所で参照される横断 API なので、API surface の一部として SemVer 上の扱いをコメント等で明示しておくと将来の混乱を防げる（new-nitpick-002 で再掲）。 |
| should-fix-005 | Round 2 で再 fixed（`MEASUREMENT` を ASCII バナーに変更） | **confirmed** | `attestation/src/lib.rs:113` で `pub const MEASUREMENT: [u8; 48] = *b"TITLE-PROTOCOL-MOCK-MEASUREMENT-DO-NOT-APPROVE!!"`。バイト数を実測すると `TITLE-PROTOCOL-MOCK-MEASUREMENT-DO-NOT-APPROVE!!` は 48 文字（コンパイル時に長さチェックされる array literal なので不整合は build error になる）。debug-mode AWS Nitro の all-zero PCR0 との衝突は構造的に排除済。 |
| should-fix-006 | Round 2 で再 fixed（trait docstring に「秒 / UNIX 元期 / zkVM 環境」を追加） | **confirmed** | `attestation/src/lib.rs:67-76` で:<br>「`now_unix_secs` is the reference time in **seconds since the UNIX epoch (1970-01-01 UTC)** used for certificate validity checks. Pass a `SystemTime::now()` reading on hosts; for zkVM guests that have no real clock, pass the parsed document's own timestamp …」<br>と 3 要素全てに明示的言及あり。「future timestamp は reject」「expired cert は reject」も同段で明文化。 |
| should-fix-007/008/011 | fixed | **confirmed** | `pad_zero_to_length` / "unsupport" typo / 外部 `oid` crate dep いずれも痕跡なし。`constants.rs:5` で `use x509_parser::der_parser::{oid, Oid};` の単一系統。 |
| should-fix-009 | wontfix（`CoseSign1` の `pub` は `mod cose` 内のみで外部公開 0） | **acknowledged** | `attestation-aws-nitro/src/lib.rs:22` で `mod cose;`（pub なし、crate-private）。`lib.rs:26-28` の `pub use` リストに `cose::CoseSign1` は含まれず、外部 surface には現れない。判断妥当。 |
| should-fix-010 | wontfix（alg 不一致時 `Err` 返却は意図的な fail-close 設計） | **acknowledged** | `sign.rs:91-96` / `sign.rs:164-168` 双方とも `Err` 返却。`cose.rs:79` の alg 値不一致のみ `Ok(false)` で逃がす形が残るが、これは「protected header の alg 値が攻撃者の改竄」に対する素直な「署名検証失敗」扱いで合理的。 |
| nitpick-001..006 | fixed / partially-fixed / wontfix | **confirmed** | Round 2 認定済。`attestation/Cargo.toml:12` 1 行コメント、`lib.rs:10` 1 行 derivation note、`cose.rs:4` の系譜 1 行、`lib.rs:91-96` の `assert!(matches!)` 形式、`cert.rs:91-92` の `///` doc コメント、いずれも現状維持。 |

### Round 2 新規（Round 2 で fixed / wontfix と判定済）

| ID | Round 2 判定 | Round 3 検証 | 検証ポイント |
|---|---|---|---|
| new-must-001 | fixed | **confirmed** | `doc.rs:65-73` で:<br>```rust\nlet doc_secs = self.doc.timestamp / 1000;\nif doc_secs > timestamp {\n    return Err(anyhow!(\n        \"attestation document timestamp {doc_secs}s is ahead of verifier clock {timestamp}s\"\n    ));\n}\n```<br>と未来 timestamp チェックを `digest == \"SHA384\"` ガード直後に挿入。回帰テスト `rejects_doc_timestamp_in_future`（`lib.rs:124-137`）が `doc_ts_secs - 60` で `AttestationError::SignatureInvalid` を期待する形で配置。`doc.rs:47-52` の手順番号付き docstring 第 2 項目にも明文化済。trait 契約（`attestation/src/lib.rs:74-76`）と実装が完全に整合した。 |
| new-should-001 | fixed（`CertChainInvalid` 削除） | **confirmed** | should-fix-003 と同様。`AttestationError` は 3 variant に縮約。 |
| new-should-002 | wontfix（`CoseSign1::from_bytes` の HeaderMap 早期 decode は早期失敗用途、二重 decode コストは無視可能） | **acknowledged with caveat** | `cose.rs:50-53` で:<br>```rust\nlet protected = cosesign1.value.protected.as_slice();\nlet _: HeaderMap = serde_cbor::from_slice(protected)\n    .map_err(|err| anyhow!(\"deserialization failed: {:?}\", err))?;\n```<br>`cose.rs:57` で同じ bytes を再度 decode。Round 2 のコスト判断（数十マイクロ秒）は妥当だが、「`from_bytes` は decode 可否のみ確認・未知キー検査は `verify_signature` 側」という責務分割が直接コメントされていないため、新規読者は「冗長 / バグ」と誤認しやすい。コメント 1 行で「intentional early-fail for parse soundness; key validation happens in verify_signature」と添えると review コストが下がる（new-nitpick-001 で再掲）。 |
| new-should-003 | fixed（`check_compatible_with` を `check_compatible_with_der` にリネーム、適用範囲を DER 経路に限定） | **confirmed** | `sign.rs:86` `pub fn check_compatible_with_der(self, key_algo: KeyAlgo) -> anyhow::Result<()>`。`sign.rs:80-85` の docstring に「Compatibility table for DER-encoded X.509 signatures only … COSE_Sign1 raw signatures use a stricter table … the corresponding check lives inside `verify_signature_raw` itself」と raw 経路の挙動を明記。`cert.rs:80` 呼び出し側も `sig_algo.check_compatible_with_der(issuer_key.algo)?` に追従。`verify_signature_raw`（`sign.rs:143-170`）は `(P256, SHA256)` と `(P384, SHA384)` の 2 組のみで他は `Err`、API 契約と実装が一致。 |
| new-nitpick-001 | wontfix（`cert.rs:check_valid` の Display + UNIX 秒の二重表記は debug 用途） | **acknowledged** | `cert.rs:39-47` 維持。ログ可読性より時計確認の双方表示優先という判断、運用観点で妥当。 |
| new-nitpick-002 | wontfix（`authenticate` の `CertChain` 戻り値は将来の `into_doc()` 連携・追加属性露出を見据えて残置） | **acknowledged** | `doc.rs:55` 戻り値 `anyhow::Result<CertChain<'_>>` のまま。`lib.rs:26` の `pub use cert::CertChain;` も維持。「将来の API surface 拡張のための placeholder」という設計意図のコメントが `doc.rs` / `lib.rs` どちらにも無いため、`cargo +stable doc --no-deps` 出力では「使われていない CertChain」という印象になる。意図を `lib.rs:26` 横に 1 行注記しておく方が API consumer の混乱が減る（new-nitpick-005 で再掲）。 |

## 全体所感（Round 2 → Round 3 差分）

Round 2 の修正は機械的にも論理的にも綺麗で、追加された 4 件の修正（`CertChainInvalid` 削除、`MEASUREMENT` バナー化、trait docstring 拡張、`check_compatible_with` の DER 専用化）は全て狙い通り入っている。回帰テスト `rejects_doc_timestamp_in_future` も実 fixture ベースの認証成功テスト（`verifies_real_aws_nitro_attestation`）と組になっており、テスト面の信頼性が著しく向上した。

逆に言えば、コア部分の must-fix / should-fix は出尽くしている。以下に挙げる Round 3 新規発見は、いずれも `nitpick` 〜 軽量 `should-fix` レベル（API ergonomics / docstring 整備 / dead surface 整理）であり、リリース可否を左右しない。

---

## 新規発見（Round 3）

合計 7 件（must-fix: 0 / should-fix: 3 / nitpick: 4）。

### should-fix 新規

#### r3-should-001  `verify_chain` / `Cert::verify` の戻り型 `anyhow::Result<bool>` が、呼び出し側で `Ok(false)` を Err と等価に扱われている — API 設計と用法の乖離

- 場所: `crates/attestation-aws-nitro/src/cert.rs:77`（`Cert::verify -> anyhow::Result<bool>`）, `cert.rs:142-157`（`CertChain::verify_chain -> anyhow::Result<bool>`）, `crates/attestation-aws-nitro/src/doc.rs:91-95`（唯一の `verify_chain` 呼び出し）, `cert.rs:149-153`（`subject.verify(issuer)` 唯一の呼び出し）
- 観察:
  ```rust
  // doc.rs:91-95
  match cert_chain.verify_chain() {
      Ok(true) => {}
      Ok(false) => return Err(anyhow!("failed to verify x509 chain")),
      Err(err) => return Err(anyhow!("failed to verify x509 chain: {err:?}")),
  };
  ```
  ```rust
  // cert.rs:149-153
  if !subject
      .verify(issuer)
      .with_context(|| format!("verify cert sig failed at {i}"))?
  {
      return Ok(false);
  }
  ```
  全呼び出しで `Ok(false)` は即 `Err` に変換される。`bool` を返す意味（「成功 / 失敗を変数として取り回したい」）が立っていない。
- 問題: `Result<bool>` は「3 値ロジック（成功 / 業務的失敗 / システム的失敗）」を期待させるが、現実は `Ok(false)` と `Err` が同義。読み手の認知負荷が高く、`?` で済むコードが冗長な `match` になっている（`doc.rs:91-95`）。一貫した `Result<()>` API なら `cert_chain.verify_chain()?;` 1 行で済む。
- 修正案: `Cert::verify` / `CertChain::verify_chain` の戻り型を `anyhow::Result<()>` に変更し、署名 verify 失敗は `Err(anyhow!("signature verification failed"))` で返す。`verify_signature_der` / `verify_signature_raw` も同様に `bool` を撤去すると、`cose.rs:56` `verify_signature` の戻り型 `anyhow::Result<bool>` も `Result<()>` に揃えられる。`cose.rs:76-84` の「alg 値不一致を `Ok(false)`」も `Err` に統一する（既に should-fix-010 で fail-close 方針が選択されているので整合する）。

#### r3-should-002  SP1 feature 有効時に標準 `sha2 = "0.10"` が同時にコンパイルされる — zkVM guest の cycle / バイナリサイズ無駄

- 場所: `crates/attestation-aws-nitro/Cargo.toml:30`（`sha2 = "0.10"` 無条件 dep）, `Cargo.toml:33`（`sha2_sp1` optional dep）, `crates/attestation-aws-nitro/src/lib.rs:14-15`（`#[cfg(feature = "sp1")] extern crate sha2_sp1 as sha2;`）
- 観察: `sha2 = "0.10"` は features を持たない無条件 dep。`sp1` feature を on にすると、`extern crate sha2_sp1 as sha2;` がクレート内の `sha2` 識別子を `sha2_sp1` にすり替える。型・関数の呼び出し先は `sha2_sp1` 経由になり、標準 `sha2` クレートのアイテムは参照されなくなる。ただし `Cargo.toml` 上は依然として依存しているため、`cargo build --features sp1` でも標準 `sha2` がビルドされ、最終バイナリのデッドコード除去で削られる可能性はあるが、guest コンパイラ（`riscv32im-succinct-zkvm-elf`）でのリンク段デッドコード除去は完全とは限らない。同じく `p256` / `p256_sp1` 関係でも同型の構造（`lib.rs:17-18`）。
- 問題: SP1 zkVM では guest crate の総コンパイルサイズと展開後 cycle 数がコストに直結する。RustCrypto の `sha2` は intrinsics 切り替えのために `cfg!` 経路が複雑で、未使用でもコンパイル時間は無視できない。Cargo features の正しい使い方は「ある dep を gate する」もしくは「`default-features = false` + 排他 features」。
- 修正案: `Cargo.toml` で:
  ```toml
  sha2 = { version = "0.10", optional = true }
  ```
  とした上で、
  ```toml
  [features]
  default = ["host"]
  host = ["dep:sha2", "dep:p256"]
  sp1 = ["dep:sha2_sp1", "dep:p256_sp1"]
  ```
  のような排他 feature にする。あるいは `Cargo.toml` を据え置きで `#[cfg(not(feature = "sp1"))] use sha2 as sha2;` / `#[cfg(feature = "sp1")] use sha2_sp1 as sha2;` をクレート Root に置く案もあるが、optional dep 化のほうが Cargo の依存解決上クリーン。`p256` も同様（既に `optional` の `p256_sp1` と非対称）。

#### r3-should-003  `p256` 依存の `"pem"` feature が未使用 — dependency feature の dead surface

- 場所: `crates/attestation-aws-nitro/Cargo.toml:24`
- 観察:
  ```toml
  p256 = { version = "0.13", features = ["ecdsa", "pem"] }
  ```
  `pem` feature は `p256::pkcs8` 経由の PEM 解析を有効化するが、当 crate 内では `from_sec1_bytes`（`sign.rs:113` / `sign.rs:120` / `sign.rs:128` / `sign.rs:151`）のみ使用。`grep -rn 'pem\|pkcs8\|from_public_key_pem' crates/attestation-aws-nitro/src/` の結果 0 件。
- 問題: `pem` feature は `pem-rfc7468`, `base64ct`, `pkcs8` などを引き込むため、依存ツリーとコンパイル時間の純損失。SP1 guest でも同様に dead。
- 修正案: `features = ["ecdsa"]` のみに削減。

### nitpick 新規

#### r3-nitpick-001  `cose.rs:50-53` の「decode 結果を `_` で捨てる」意図を 1 行コメントで明示

- 場所: `crates/attestation-aws-nitro/src/cose.rs:50-53`
- 観察: Round 2 で wontfix 判定された new-should-002 と同じコード。コスト判断は妥当だが、コードのみを読むと冗長に見える。
- 修正案: 直前に `// Smoke-test that protected is a well-formed CBOR map; full key validation happens in verify_signature.` を 1 行追加。

#### r3-nitpick-002  `MockAttestationVerifier::PREFIX` / `MEASUREMENT` の `mock` feature が公開 API surface であることを明記

- 場所: `crates/attestation/src/lib.rs:108`, `:113`
- 観察: Round 2 should-fix-004 wontfix の理由「extension テストが mock helper を再実装する必要が生じる」は妥当だが、`pub const PREFIX` / `pub const MEASUREMENT` は `cargo doc --features mock` の出力に現れ、OSS 公開時に「外部利用者が `mock` feature を有効化して prod パスで mock を使ってしまう」リスクが残る。
- 修正案: `attestation/src/lib.rs:97` の `MockAttestationVerifier` docstring に「`#[cfg(feature = \"mock\")]` でのみ exposed. Do not enable in production.」を追加。`Cargo.toml:12` の `# Enables MockAttestationVerifier. Test-only.` 1 行で意図は示されているが、`Cargo.toml` を読まずに rustdoc だけ見るユーザに対する保険として `pub struct MockAttestationVerifier` 側にも明示が望ましい。

#### r3-nitpick-003  `AttestationDocument::nonce` フィールドが parse されるが `VerifiedAttestation` に到達せず未利用

- 場所: `crates/attestation-aws-nitro/src/doc.rs:118` (`pub nonce: Option<ByteBuf>`), `crates/attestation-aws-nitro/src/lib.rs:76-83` (`VerifiedAttestation { … }` 構築箇所)
- 観察: AWS Nitro Attestation Document の `nonce` は仕様上 client-supplied 1 回限り使用フィールドで、Title Protocol では `signature_hash` バインドに `user_data` を使う設計（SPECS_JA §1.2 / §5.2）のため不要。だが parse はしている。
- 問題: 「読むが使わない」フィールドは「将来使う想定 vs 単に消し忘れ」の判別が読み手にできない。`VerifiedAttestation` にも `nonce` フィールドが無いので、外部から取得する経路もない。
- 修正案: (a) `AttestationDocument::nonce` を `pub(crate)` に下げる、もしくは `pub` のまま `/// AWS Nitro nonce (client-supplied freshness token). Not currently surfaced by `VerifiedAttestation`; kept for completeness with the Nitro wire format.` の rustdoc を追加。(b) Title Protocol で nonce 経路を使う計画がなければ `serde(skip)` で parse 自体を省く。

#### r3-nitpick-004  `cose.rs:124-150` の手書き `visit_seq` が COSE_Sign1 配列長 4 を構造的に強制していない

- 場所: `crates/attestation-aws-nitro/src/cose.rs:124-150`
- 観察: `visit_seq` は 4 つの `next_element()` を呼ぶ。RFC 8152 §4.2 は COSE_Sign1 が「正確に 4 要素の CBOR array」と定義しているが、現コードは 5 つ目以降の要素が存在した場合に silently 無視する（serde の `SeqAccess` 既定挙動）。
- 問題: 攻撃面は限定的（追加要素は signed by COSE 鍵ではないのでバインドされない）だが、防御深度として配列長検査があると安全側に倒れる。`maps_duplicate_key_is_error` を protected header に適用したのと同じ理屈。
- 修正案: 4 つの `next_element()` の後に:
  ```rust
  if seq.next_element::<serde::de::IgnoredAny>()?.is_some() {
      return Err(A::Error::invalid_length(5, &"COSE_Sign1 must have exactly 4 elements"));
  }
  ```
  を追加。

#### r3-nitpick-005  `pub use cert::CertChain;` の意図（将来 surface 拡張）が `lib.rs` から読み取れない

- 場所: `crates/attestation-aws-nitro/src/lib.rs:26`（再 export）, `crates/attestation-aws-nitro/src/doc.rs:55`（戻り値で使用）
- 観察: Round 2 new-nitpick-002 wontfix の理由「将来の `into_doc()` 連携や追加属性露出を見据えて残置」は妥当だが、コード上は単純に `authenticate()` の戻り値を即座に `drop` する `lib.rs:63-65` のみで、現時点で `CertChain` を crate 外から触る経路は存在しない。
- 問題: rustdoc 上に「使い道のない `pub use`」として現れる。
- 修正案: `pub use cert::CertChain;` の直前に 1 行コメント:
  ```rust
  // Re-exported for downstream code that wants to inspect the leaf cert / chain
  // post-authentication. Currently used internally only.
  pub use cert::CertChain;
  ```

---

## 全体所感

Round 2 までで attestation crates は機能・セキュリティ・API クリーンネス共に「リリース可能」水準に達した。`ec_decode_sig` 撤去・root pin 一本化・raw / DER 経路の責務分割・mock の構造的安全化など、Round 1 で挙げられた主要な懸念は全て解消されている。

Round 3 で挙げた 7 件は API 整理（`Result<bool>` → `Result<()>`、dead feature flag、未使用フィールド）と rustdoc 整備が中心で、いずれもユーザに見える挙動は変えない。「Round 2 wontfix の判断意図をコードに残す」系のメンテナンス改善が多く、コア実装の堅牢性に対する懸念は無い。

ベンダー追加（AMD SEV-SNP / Intel TDX）に向けては `AttestationError` 3 variant 設計と `now_unix_secs` 契約が安定したため、新規実装は trait に合わせて並べるだけで済む見通し。`Result<bool>` → `Result<()>` への移行（r3-should-001）を行うなら、ベンダー追加前に済ませた方が拡散コストが少ない。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| must-fix-001..007 | confirmed | Round 2 fixed 維持。 |
| should-fix-001/002/007/008/011 | confirmed | Round 2 fixed 維持。 |
| should-fix-003 | confirmed | Round 2 で `CertChainInvalid` を削除した状態を維持。 |
| should-fix-004 | acknowledged | `MockAttestationVerifier::PREFIX` `pub` 維持（Round 2 wontfix の理由は今も成立）。 |
| should-fix-005 | confirmed | `MEASUREMENT` ASCII バナー（48 文字）維持。 |
| should-fix-006 | confirmed | trait docstring の単位 / 元期 / zkVM 言及維持。 |
| should-fix-009 | acknowledged | `CoseSign1` `pub` が module-private で外部露出 0、Round 2 wontfix 妥当。 |
| should-fix-010 | acknowledged | alg 不一致 `Err` の fail-close 設計維持。 |
| nitpick-001..006 | confirmed | Round 2 認定維持。 |
| new-must-001 | confirmed | `doc.rs:65-73` で未来 timestamp チェック実装、回帰テストあり。 |
| new-should-001 | confirmed | `CertChainInvalid` 削除済。 |
| new-should-002 | acknowledged | `cose.rs:50-53` の二重 decode 維持。コメント追加は r3-nitpick-001 で別途提案。 |
| new-should-003 | confirmed | `check_compatible_with_der` リネーム済。 |
| new-nitpick-001 | acknowledged | `cert.rs:check_valid` の二重表記維持。 |
| new-nitpick-002 | acknowledged | `pub use cert::CertChain;` 維持。意図の rustdoc 追記は r3-nitpick-005 で別途提案。 |
| r3-should-001 | wontfix | `Result<bool>` → `Result<()>` 移行は cert/cose/sign 全体に渡る API 整理で、実害ゼロの ergonomics 改善。修正範囲の広さに対する利得が小さい。ベンダー追加 (SEV-SNP / TDX) で trait を再整備するタイミングでまとめて行う方が拡散コストが少ない。 |
| r3-should-002 | wontfix | SP1 feature と sha2/p256 の排他化。SP1 prove は別 binary で動いており現状動作している。性能影響は推測ベースで実測なし。Cargo feature 排他化は破壊面が広く、得るものが不確実な状況で壊すリスクが上回る。実測が必要なら別タスク。 |
| r3-should-003 | fixed | `p256` の `"pem"` feature を削除。`Cargo.toml` で `features = ["ecdsa"]` のみに。grep で `pkcs8` / `from_public_key_pem` の利用ゼロを確認済。依存ツリーとビルド時間の純損失を解消、副作用なし。 |
| r3-nitpick-001 | fixed | `cose.rs:50-53` の二重 decode に「protected が well-formed CBOR map であることの早期確認。key validity は verify_signature 側」コメントを追加。 |
| r3-nitpick-002 | wontfix | `MockAttestationVerifier` docstring に「production で mock を有効化するな」追記。`attestation/Cargo.toml:12` で既に `Enables MockAttestationVerifier. Test-only.` と意図表明済み。rustdoc 同レイヤでの重複は冗長。 |
| r3-nitpick-003 | wontfix | `AttestationDocument::nonce` は AWS Nitro wire の正規フィールド。Title Protocol では未使用だが、別ベンダー実装で再利用する可能性 + 仕様完全性のため parse を維持。`serde(skip)` は将来の wire 互換性を損なうリスクがある。 |
| r3-nitpick-004 | fixed | `cose.rs` の `visit_seq` 末尾に `IgnoredAny` チェックを追加。RFC 8152 §4.2 の「正確に 4 要素」を構造的に強制。攻撃面はゼロだが defense-in-depth として有効、副作用なし。 |
| r3-nitpick-005 | fixed | `lib.rs:26` の `pub use cert::CertChain;` 直前に「downstream で leaf cert / チェーン情報を inspect する用途を想定した API surface、現状は内部利用のみ」コメントを追加。 |
