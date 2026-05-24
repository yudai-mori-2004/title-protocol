# K1. Attestation crates 縦深監査 — Round 2

## 概要

担当範囲: `crates/attestation/` 全ファイル、`crates/attestation-aws-nitro/` 全ファイル（`tests/fixtures/*.report` バイナリ除外）。

Round 1 指摘 24 件（must-fix: 7 / should-fix: 11 / nitpick: 6）の処理状況を file:line 単位で再精査し、修正で発生した新規問題を追加した。

## Round 1 指摘の処理状況

| Severity | Total | fixed | partially-fixed | unchanged | regressed |
|---|---|---|---|---|---|
| must-fix | 7 | 7 | 0 | 0 | 0 |
| should-fix | 11 | 5 | 4 | 2 | 0 |
| nitpick | 6 | 5 | 1 | 0 | 0 |
| **計** | **24** | **17** | **5** | **2** | **0** |

### must-fix 詳細

| ID | Status | 検証ポイント |
|---|---|---|
| must-fix-001 | **fixed** | `cert.rs:77` の `Cert::verify(&self, issuer: &Self)` から `Option` が消え、`verify_chain` (cert.rs:137-152) は `for i in 1..self.certs.len()` で root の自己署名検証を行わない。root pin (`doc.rs:73-77`) のみで信頼起点を担保する形に整理された。 |
| must-fix-002 | **fixed** | `verify_chain` から `trusted_certs_len` 引数が消え、`authenticate(timestamp)` (doc.rs:53) も `trusted_certs_len` を取らない。`lib.rs:80` 旧呼び出し箇所も整合済み。「全リンク検証スキップ」経路は消滅。 |
| must-fix-003 | **fixed** | `doc.rs:54-61` で `if self.doc.digest != "SHA384"` を冒頭ガードとして追加。docstring (doc.rs:46-52) も "1. Check `digest == \"SHA384\"`" を 1 番目の手順として明文化。 |
| must-fix-004 | **fixed** | `cose.rs:64-74` で protected header の全キーを列挙し、`Integer(1)`（alg）以外は `Err` で reject。`crit`（label 2）含む未知ヘッダの defence-in-depth が成立。 |
| must-fix-005 | **fixed** | `sign.rs:88-128` の `verify_signature_der` が `P256Signature::from_der` / `P384Signature::from_der` を直接呼ぶ形に書き直され、`ec_decode_sig` / `pad_zero_to_length` / `as_biguint` / 独自 DER walker は全削除。COSE_Sign1 用は `verify_signature_raw` (sign.rs:131-158) として `Signature::from_slice` 経由。 |
| must-fix-006 | **fixed** | RSA / RSASSA-PSS パスは `sign.rs` から完全に撤去。`KeyAlgo` enum は `EcdsaP256` / `EcdsaP384` のみ、`SigAlgo` は `EcdsaSHA256` / `EcdsaSHA384` のみ。RSA 由来の `rsa` / `pkcs8` 等の dep 痕跡なし。 |
| must-fix-007 | **fixed** | `attestation-aws-nitro/Cargo.toml:30-31` で `sha2 = "0.10"` / `sha2_sp1` の `features = ["oid"]` が削除済み。`oid` crate 自体への dep も消えた（`x509_parser::der_parser::Oid` に統一）。 |

### should-fix 詳細

| ID | Status | 検証ポイント |
|---|---|---|
| should-fix-001 | **fixed** | `lib.rs:36-42` の crate doc を "The chain root is pinned to `constants::AWS_NITRO_ROOT_CA_SHA256`; a cabundle whose first certificate does not match the fingerprint is rejected." に書き換え済み。「呼び出し側に委ねろ」誤導文は削除。 |
| should-fix-002 | **fixed** | production パス (`lib.rs:55-65`) は `now_unix_secs` をそのまま `authenticate()` へ渡し、`min(now, doc.ts/1000)` の折り畳みは削除。test (lib.rs:108-109) のみ `report.doc().timestamp / 1000` を `check_ts` に使う形に局所化。"smaller of (now, doc.timestamp)" 旧コメントも消滅。 |
| should-fix-003 | **partially-fixed** | `AttestationError::Expired` variant は削除されたが、新たに `CertChainInvalid(String)` (lib.rs:51-52) が追加されているにもかかわらず実装内で一度も使われていない（`attestation-aws-nitro/src/lib.rs:61-72` は `ParseFailed` / `SignatureInvalid` / `MissingField` の 3 種類のみ生成）。`grep` 結果でも `CertChainInvalid` の唯一の出現箇所は定義のみ。新規 dead variant が発生。下記 new-001 で再掲。 |
| should-fix-004 | **unchanged** | `MockAttestationVerifier::PREFIX` は `attestation/src/lib.rs:101` で依然 `pub const`。`crates/solana/src/extension.rs:203` が `MockAttestationVerifier::PREFIX.to_vec()` を直接参照する状態のまま。`pub(crate)` 化や `build_mock_attestation(user_data)` helper への置き換えは未実施。 |
| should-fix-005 | **partially-fixed** | `MEASUREMENT = [0u8; 48]` (lib.rs:104) はゼロのまま残存。ただし `crates/solana/src/extension.rs:249-257` に `verify_attestation_binding_measurement_mismatch` が追加され、`wrong = [0xAA; 48]` で `MeasurementMismatch` 経路をカバーしている。テスト網羅性の懸念は解消、定数の意味性は未解消。 |
| should-fix-006 | **partially-fixed** | `attestation/src/lib.rs:70-76` の docstring は「`now_unix_secs`」と「documents whose internal timestamp is in the future relative to `now_unix_secs`」「certificates that expire before `now_unix_secs`」まで言及するように改善されたが、(a) 単位（秒 vs ミリ秒）、(b) UNIX 元期、(c) zkVM guest で実時計が無い場合の渡し方、の 3 点は未記載。さらに「doc 内 timestamp の未来チェック」は trait 仕様としては明記されたが、AWS Nitro 実装側 (`doc.rs:53-93`) では一切実装されていない（new-002 で再掲）。 |
| should-fix-007 | **fixed** | `pad_zero_to_length` 関数自体が `sign.rs` から削除（must-fix-005 修正と一括）。`sig_slice.len() != expected_len` 比較も撤去。 |
| should-fix-008 | **fixed** | `cose.rs:25-31` の `sig_algo_val` 関数自体は `match` のため typo 経路無し。Round 1 で指摘の typo 文言（"unsupport"）は現行版で確認不能（該当 `anyhow!` が削除されている）。crate 内に "unsupport" の文字列残存なし。 |
| should-fix-009 | **unchanged** | `CoseSign1` は `cose.rs:34` で `pub struct` のまま、その手書き `Deserialize` impl (cose.rs:107-162) も `pub`。`lib.rs` には `pub use cert::CertChain;` / `pub use doc::{AttestationDocument, AttestationReport};` はあるが `pub use cose::CoseSign1;` は無いため crate 外には漏れていない。ただし `pub` 修飾子のまま module を `pub` にすれば即露出する状態は変わらず（API hardening 観点で未対応）。 |
| should-fix-010 | **partially-fixed** | `cose.rs:60-84` で protected header の未知キーは `Err` で reject（must-fix-004 経由で厳格化）。一方、`alg` 値の不一致は依然 `Ok(false)` を返す（cose.rs:76-84）。「失敗系は全部 `Ok(false)` で統一」の API 一貫性方針は未採択。但し新規 reject 経路は protected header 改竄検出として理に適っており悪化ではない。 |
| should-fix-011 | **fixed** | `Cargo.toml` から `oid = "0.2"` dep が削除、`constants.rs:5` は `use x509_parser::der_parser::{oid, Oid};` の単一系統に整理。`sign.rs:9-21` でも `ObjectIdentifier::try_from` 経路は消え、`x509_parser::der_parser::Oid` と `AlgorithmIdentifier` のみ使用。 |

### nitpick 詳細

| ID | Status | 検証ポイント |
|---|---|---|
| nitpick-001 | **fixed** | `attestation/Cargo.toml:12` 「`# Enables MockAttestationVerifier. Test-only.`」の 1 行に圧縮済み。 |
| nitpick-002 | **partially-fixed** | `attestation/src/lib.rs:5` 依然 `Spec §1.2, §5.2, §6.2` のリスト羅列。`§5.2` / `§6.2` の関連性は本 crate からは依然読み取れない。 |
| nitpick-003 | **fixed** | `attestation-aws-nitro/src/lib.rs:10` `// Derived from Automata Network's aws-nitro-enclave-attestation (Apache-2.0).` の 1 行に圧縮。`cose.rs:4` にも同種の系譜 1 行（Amazon → Automata）が残るが過度ではない。 |
| nitpick-004 | **fixed** | `attestation-aws-nitro/src/lib.rs:91-96` の `rejects_invalid_bytes` は `assert!(matches!(err, AttestationError::ParseFailed(_)));` 形式に修正。死テスト形式は解消。`vendor_tag_consistent` は削除済み。あわせて `verifies_real_aws_nitro_attestation`（lib.rs:101-117）が実 fixture (`tests/fixtures/attestation_1.report`) ベースで追加され、テスト本質性は大きく向上。 |
| nitpick-005 | **fixed** | `cert.rs:91-92` に `/// Certificates ordered root → leaf. The root must be authenticated out of band (the AWS Nitro verifier pins its SHA-256 fingerprint).` を `///` 形式で配置。rustdoc に出る。 |
| nitpick-006 | **fixed** | must-fix-002 修正に連動して `doc.rs` の `trusted_certs_len` 解説コメントは消滅。 |

## 新規発見（Round 2）

合計 6 件（must-fix: 1 / should-fix: 3 / nitpick: 2）。

### must-fix 新規

#### new-must-001  trait docstring の「未来 timestamp は reject せよ」契約が AWS Nitro 実装で守られていない

- 場所: `crates/attestation/src/lib.rs:72-75` （契約）vs `crates/attestation-aws-nitro/src/doc.rs:53-93` （実装）
- 観察: trait docstring は
  > Implementations should reject documents whose internal timestamp is in the future relative to `now_unix_secs`, and certificates that expire before `now_unix_secs`.
  と書く。一方、`authenticate(timestamp)` 内では `cert_chain.check_valid(timestamp)` を呼ぶのみで、`self.doc.timestamp` と引数 `timestamp` の前後関係を比較する箇所が一切ない。`lib.rs:55-65` の `verify` も `now_unix_secs` をそのまま `authenticate` へ流すだけで、doc.timestamp の未来チェックは未実装。
- 問題: Round 1 should-fix-002 の修正で「`min()` 折り畳み」を消す代わりに「呼び出し側が `now_unix_secs` を厳密に渡す」前提に切り替えたが、その厳密性を支える「doc.timestamp が未来でないこと」の検証を trait 側で約束しておきながら実装で守っていない。replay 防御や clock-skew 攻撃の defence-in-depth に穴が空いた状態。
- 修正案: `doc.rs:53` の `authenticate` 冒頭で:
  ```rust
  let doc_secs = self.doc.timestamp.saturating_div(1000);
  if doc_secs > timestamp {
      return Err(anyhow!(
          "attestation document timestamp {} is in the future relative to verifier clock {}",
          doc_secs, timestamp
      ));
  }
  ```
  を追加。あるいは trait docstring 側を「未来チェックは呼び出し側責務」に弱める。前者推奨。

### should-fix 新規

#### new-should-001  `AttestationError::CertChainInvalid` が定義されたが実装側で一度も生成されない（Round 1 should-fix-003 の dead variant 問題が再発）

- 場所: `crates/attestation/src/lib.rs:51-52`
- 観察: Round 1 で指摘した未使用 variant `Expired` は削除されたが、代わりに新規追加された `CertChainInvalid(String)` が `attestation-aws-nitro` 実装内で一度も生成されない（`grep AttestationError::` の出力に該当無し）。cert chain 検証失敗も timestamp 期限切れも全て `AttestationError::SignatureInvalid` (lib.rs:65) に集約されている。
- 問題: trait の error variant 粒度が「定義したが使わない」状態に逆戻りしている。下流 (`crates/solana/src/extension.rs:22` で `AttestationVerifier` を import している) は variant を区別できないため、追加した意味が無い。
- 修正案: (a) `attestation-aws-nitro/src/lib.rs:55-65` で `report.authenticate` の `Err` を down-cast または error context で識別して `CertChainInvalid` / `SignatureInvalid` / `MissingField` に振り分ける。`anyhow::Error` を持ち回っているため真の down-cast は難しく、代替策として `doc.rs` を `thiserror` ベースの enum 戻り値に書き直す。(b) 振り分けする予定が無いなら `CertChainInvalid` を再削除し variant を 3 種類に戻す。

#### new-should-002  `cose.rs:51-52` の自己費消パスで `HeaderMap` を 1 回 decode しているのに結果を `_` で捨てる

- 場所: `crates/attestation-aws-nitro/src/cose.rs:50-53`
- 観察:
  ```rust
  let protected = cosesign1.value.protected.as_slice();
  let _: HeaderMap = serde_cbor::from_slice(protected)
      .map_err(|err| anyhow!("deserialization failed: {:?}", err))?;
  ```
  `from_bytes` 内では「protected が valid CBOR HeaderMap として decode できる」ことだけ確認し、値は捨てる。後で `verify_signature` (cose.rs:56-58) が再度 `serde_cbor::from_slice` で同じ bytes を decode する。
- 問題: 同じ CBOR を 2 回 decode するのは無駄でもあり、`from_bytes` だけ通って `verify_signature` 直前で失敗する経路が論理的に存在する（攻撃面ではないが冗長な double-parse）。Round 1 must-fix-004 修正で `verify_signature` 側に header key 厳格化が入ったため、`from_bytes` 側の「decode できれば良し」とは検査基準がズレている。
- 修正案: `from_bytes` で decode した `HeaderMap` を `CoseSign1` 内に保存し、`verify_signature` ではそれを使う。`protected: ByteBuf` を `protected_bytes: ByteBuf` + `protected_map: HeaderMap` に分離する。あるいは `from_bytes` 側の事前 decode を削除し、`verify_signature` の検査に一本化する。

#### new-should-003  `SigAlgo::check_compatible_with` が `(SHA256, P384)` を許容するが、`verify_signature_raw` は `(P384, SHA256)` を rejected として `Err` 返す非対称

- 場所: `crates/attestation-aws-nitro/src/sign.rs:74-86` （許容表）vs `sign.rs:137-156` （raw 経路）
- 観察: `check_compatible_with` は `(EcdsaSHA256, EcdsaP256)` / `(EcdsaSHA256, EcdsaP384)` / `(EcdsaSHA384, EcdsaP384)` の 3 組を `Ok` とする。一方 `verify_signature_raw` は `(P256, SHA256)` / `(P384, SHA384)` の 2 組のみ実装し、それ以外は "incompatible key/signature algorithms for raw ECDSA" で `Err`。
- 問題: `cert.rs:80` で `sig_algo.check_compatible_with(issuer_key.algo)?` を通り、`verify_signature_der` に進むパス（DER 形式）は 3 組すべて実装されていて整合する。だが raw 形式の `verify_signature_raw` は COSE 専用で、`(P384, SHA256)` 組み合わせは AWS Nitro 仕様上現れないため `verify_signature_raw` 側が 2 組だけになっている。`check_compatible_with` は両者共用なので、API として「check 通過した組み合わせが必ず verify できる」前提が崩れる（中で `Err` になる）。検証成功/失敗の区別が `Ok(false)` ではなく `Err` に化けるため、呼び出し側 (`doc.rs:88`) では `?` で即 propagate される。
- 修正案: (a) `check_compatible_with` を「DER 用 / raw 用」の 2 種類に分け、raw 用は 2 組のみ Ok とする。(b) もしくは `verify_signature_raw` 側で `(P384, SHA256)` も実装（COSE では使わないが対称性のため）。AWS Nitro 専用前提を残すなら (a) で十分。

### nitpick 新規

#### new-nitpick-001  `cert.rs:42-47` の `check_valid` エラー文言が timestamp を 2 形式で 3 重に出力する

- 場所: `crates/attestation-aws-nitro/src/cert.rs:39-47`
- 観察:
  ```rust
  Err(anyhow!(
      "certificate is not valid at time: {}({}), range: {}({}) - {}({})",
      time, time.timestamp(),
      validity.not_before, validity.not_before.timestamp(),
      validity.not_after, validity.not_after.timestamp(),
  ))
  ```
- 問題: 1 つの時刻を「ASN1Time の Display」と「UNIX 秒」の両方で並べて出すため、ログが非常に読みにくい。3 つ × 2 形式 = 6 数値。debug 用途としても過剰。
- 修正案: UNIX 秒のみ、もしくは ASN1Time Display のみに統一。例: `"certificate not valid at {} (UTC); range: {} - {}"` 形式。

#### new-nitpick-002  `doc.rs:46-52` の手順番号付きコメントは正論だが、「`authenticate` の return 型は `CertChain` を返すのに、それを呼び出し側が一度も使っていない」

- 場所: `crates/attestation-aws-nitro/src/doc.rs:53` （signature）, `crates/attestation-aws-nitro/src/lib.rs:63-65` （唯一の呼び出し）
- 観察: `pub fn authenticate(&self, timestamp: u64) -> anyhow::Result<CertChain<'_>>`。`lib.rs:63-65` は
  ```rust
  report.authenticate(now_unix_secs)
      .map_err(|e| AttestationError::SignatureInvalid(format!("{e:?}")))?;
  ```
  と戻り値 `CertChain` を即座に捨てる。
- 問題: API として `CertChain` を返却する意味が現状無い。`pub use cert::CertChain;` も `lib.rs:26` で公開しているが、これも外部から使われていない（`grep CertChain` の結果は本 crate 内のみ）。OSS 公開時の "余分な surface" 候補。
- 修正案: `authenticate` の戻り値を `anyhow::Result<()>` に変更し、`pub use cert::CertChain;` を撤回する。`CertChain` を crate-private にして API surface を縮小。

## 全体所感

Round 1 指摘 24 件中 17 件 fixed / 5 件 partially-fixed / 2 件 unchanged で、must-fix は全完了。`ec_decode_sig` 撤去・`trusted_certs_len` 撤去・root pin の API 整理は当初の主目的だった「Automata 由来コードの RustCrypto 標準依拠化」が綺麗に進んでいる。

一方、修正で生まれた 6 件は (1) trait docstring と実装の乖離（new-must-001: 未来 timestamp チェック）と (2) error variant の dead-on-arrival 再発（new-should-001: `CertChainInvalid`）が中心。前者は Round 1 should-fix-002 の `min()` 削除を契機に「呼び出し側責務」と「実装責務」の境界が動いたが、その境界を trait/実装の両側で揃え忘れた典型例。後者は Round 1 で 1 度解消した「定義したが使わない variant」の問題が、同じ enum に新規 variant を足す形で復活している。`mock` 周りの should-fix-004 / 005 は意図的に保留した可能性もあるが、`PREFIX` が依然 `pub` で `crates/solana/src/extension.rs:203` から参照されている状態は OSS 公開時の警戒ポイントとして残置。

ベンダー追加（AMD SEV-SNP / Intel TDX）を見据えるなら、`AttestationError` variant 設計と「`now_unix_secs` の正確な契約（単位 / zkVM 環境）」だけ Round 3 前に固めておくと、追加実装が trait に合わせやすくなる。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| must-fix-001..007 | fixed | Round 2 認定済み。 |
| should-fix-001/002/007/008/011 | fixed | Round 2 認定済み。 |
| should-fix-003 | fixed | `AttestationError::CertChainInvalid` variant を削除（dead variant）。Cert chain 検証失敗は `SignatureInvalid` に集約済みのため、不要 variant を残す意義がない。 |
| should-fix-004 | wontfix(`MockAttestationVerifier::PREFIX` は test fixture / extension テストで意図的に共有。`pub` のままにすることで mock 生成式が呼び出し側で 1 行で書ける。`pub(crate)` 化すると extension テストが mock helper を再実装する必要が生じてコスト高) | |
| should-fix-005 | fixed | G ラウンドで `MEASUREMENT` を ASCII バナーに変更済み (`TITLE-PROTOCOL-MOCK-MEASUREMENT-DO-NOT-APPROVE!!`)。debug-mode の all-zero PCR0 との衝突は構造的に解消。 |
| should-fix-006 | fixed | `attestation/src/lib.rs` の trait docstring に「秒 / UNIX 元期 / zkVM 環境では doc.timestamp を渡せ」を 7 行で追加。 |
| should-fix-009 | wontfix(`CoseSign1` の手書き Deserialize は `cose` モジュール内 `pub` だが `lib.rs:pub use` で外部公開していないため API surface には現れない。`pub(crate)` 化はクレート内部のみで意味が薄い) | |
| should-fix-010 | wontfix(alg 不一致を `Ok(false)` ではなく `Err` で返す API 設計は意図的。`?` で即時 fail-close できる一貫性のほうが優先) | |
| nitpick-001/003/004/005/006 | fixed | Round 2 認定済み。 |
| nitpick-002 | wontfix(`Spec §1.2 §5.2 §6.2` のリストは attestation 横断利用個所を示すインデックスで、削減すると参照が辿りにくい) | |
| new-must-001 | fixed | `attestation-aws-nitro/src/doc.rs::authenticate` 冒頭に `self.doc.timestamp / 1000 > timestamp` チェックを追加。trait docstring と実装の契約を一致。回帰テスト `rejects_doc_timestamp_in_future` を追加。 |
| new-should-001 | fixed | should-fix-003 と統合対応。`CertChainInvalid` variant を削除。 |
| new-should-002 | wontfix(`CoseSign1::from_bytes` 内の HeaderMap 早期 decode はパース成功保証の早期失敗用途。`verify_signature` 側との二重 decode は CPU 数十マイクロ秒のオーダーで、attestation 処理全体に対し無視できる) | |
| new-should-003 | fixed | `SigAlgo::check_compatible_with` を `check_compatible_with_der` にリネームし、適用範囲を DER 経路のみに明示。raw 経路 (`verify_signature_raw`) は内部 match で厳格化済みの旨を docstring に明記。`cert.rs` の呼び出しも更新。 |
| new-nitpick-001 | wontfix(`cert.rs:check_valid` のエラー文言は debug 用途。Display + UNIX 秒の二重表記は時計確認のため意図的) | |
| new-nitpick-002 | wontfix(`authenticate` の `CertChain` 戻り値は将来の `into_doc()` 連携や追加属性露出を見据えて残置。`pub use CertChain` も型として現在 crate 内のみだが trait 実装と一緒に公開する妥当性あり) | |
