# K1. Attestation crates 縦深監査

## 概要

担当範囲: `crates/attestation/` 全ファイル、`crates/attestation-aws-nitro/` 全ファイル（`tests/fixtures/*.report` バイナリ除外）。

監査方針: SPECS_JA §1.2 / §1.6 / §5.2 / §6.2 を基準に、COSE_Sign1 (RFC 8152) パーサー、X.509 cert chain 検証、RustCrypto API の妥当性、ベンダー中立 trait の API 設計、mock feature の境界、AWS Nitro root pin 定数、エラー型粒度を 1 行ずつ精読。重複指摘は許容（K1 は独立判断）。

ファイル単位の精読対象:

- `crates/attestation/Cargo.toml`
- `crates/attestation/src/lib.rs`
- `crates/attestation-aws-nitro/Cargo.toml`
- `crates/attestation-aws-nitro/src/lib.rs`
- `crates/attestation-aws-nitro/src/cert.rs`
- `crates/attestation-aws-nitro/src/cose.rs`
- `crates/attestation-aws-nitro/src/constants.rs`
- `crates/attestation-aws-nitro/src/doc.rs`
- `crates/attestation-aws-nitro/src/sign.rs`

合計 24 件（must-fix: 7 / should-fix: 11 / nitpick: 6）。

## 重大度別内訳

- must-fix: 7 件
- should-fix: 11 件
- nitpick: 6 件

## 発見

### must-fix-001  チェーン未署名問題: 自己署名 root の正当性が「pin SHA-256」のみで担保されている割に、root 自身の自己署名検証が "issuer=self" の弱検証になっている

- 場所: `crates/attestation-aws-nitro/src/cert.rs:79-91` および `crates/attestation-aws-nitro/src/cert.rs:149-160`
- 観察:
  ```rust
  pub fn verify(&self, issuer: Option<&Self>) -> anyhow::Result<bool> {
      let issuer_key = issuer.unwrap_or(self).pubkey();
      ...
  }
  ```
  と `verify_chain` のループは `i == 0` のとき `issuer = None` を渡し、root を「root の公開鍵で root の TBS を検証」する自己署名チェックに落とす。
- 問題: AWS Nitro の root pin SHA-256 が一致していれば root の中身は信頼できるため、自己署名検証はそもそも不要（pin がそれを担保する）。しかし現状コードは「pin 一致」と「自己署名チェック成功」の両方を要求しており、AWS が将来 root の self-signature を入れ替えた場合や、cross-signed root に置き換えた場合に検証全体が落ちる。OSS 監査対象としては、`Cert::verify` の `issuer: Option<&Self>` という API が「Option = self-signed」という暗黙仕様を持っていることが非自明で、誤用しやすい。
- 修正案: `verify_chain` を「root は pin 一致のみで信頼」「i=1 以降を親で検証」に明示変更し、`Cert::verify` から `Option` を削除、引数 `issuer: &Self` に統一する。i=0 の自己署名チェックは削除する。

### must-fix-002  `verify_chain(trusted_certs_len)` の境界条件で全リンクの検証スキップが可能

- 場所: `crates/attestation-aws-nitro/src/cert.rs:143-160`
- 観察:
  ```rust
  if trusted_certs_len > self.certs.len() {
      return Err(...);
  }
  for i in trusted_certs_len..self.certs.len() { ... }
  ```
- 問題: `trusted_certs_len == self.certs.len()` は許容され、その場合ループ本体が 1 度も実行されず `Ok(true)` が返る。`doc.rs:62` の `authenticate(trusted_certs_len, ...)` は呼び出し側が値を渡すパラメータであり、現行コードでは `lib.rs:80` から `0` 固定で渡されているが、API として「全リンク検証スキップ」が成功扱いになる経路は監査上の落とし穴。
- 修正案: `trusted_certs_len` を完全に廃止する（`doc.rs:55` のコメントが既に「production は常に 0」と認めている）。`authenticate` のシグネチャから引数を削除し、`verify_chain` も `trusted_certs_len` を取らない形に書き直す。pin が信頼起点である以上、部分検証の余地を残す API 自体が不要。

### must-fix-003  Nitro Attestation Document の `digest` フィールドが未検証

- 場所: `crates/attestation-aws-nitro/src/doc.rs:103-114`
- 観察: `AttestationDocument.digest: String` をデコードはするが、`authenticate()` 内で値の検証を行わない。AWS Nitro 仕様では `digest` は "SHA384" 固定値で、これが他値であれば PCR の解釈アルゴリズムが異なる。
- 問題: 攻撃者が `digest: "SHA256"` の偽 doc を作って、PCR0 を SHA-256 計算値で偽装したものを通せる可能性が概念上残る（実際は cabundle/leaf の署名で守られるが、defence-in-depth として欠落）。仕様 §1.2「PCR0 は 48 バイトの SHA-384」を実装側でも検査すべき。
- 修正案: `authenticate()` の冒頭で `if self.doc.digest != "SHA384" { return Err(...); }` を追加する。

### must-fix-004  COSE_Sign1 の `protected` ヘッダ検証が "alg" のみ。`crit` / 未知ヘッダの検出なし

- 場所: `crates/attestation-aws-nitro/src/cose.rs:56-83`
- 観察: protected header から `CborValue::Integer(1)`（alg）のみを取得し、他のキー（特に RFC 8152 §3.1 の `crit` ヘッダ）は無視する。
- 問題: RFC 8152 §3.1 では `crit` を実装が理解できない場合は処理を失敗させる必要がある。AWS Nitro は現状 `crit` を使っていないが、将来仕様拡張時にこのパーサーは静かに無視する。検証 crate としての契約違反。
- 修正案: `protected.0` 内のキーを列挙し、`alg` 以外のキーが存在する場合（特に `crit = 2`）、エラーで失敗させる。あるいは、AWS Nitro 用の固定 protected header bytes と完全一致を要求する。

### must-fix-005  ECDSA 署名の SEQUENCE 内 INTEGER が `expected_len` を超える場合に黙ってトランケートする経路がない代わり、エラーは出るが「負数 INTEGER」を許す

- 場所: `crates/attestation-aws-nitro/src/sign.rs:117-155`
- 観察: `as_biguint()` を使っているため負の INTEGER は弾かれるが、それ以外の不正フォーマット（leading-zero 過剰、indefinite-length）を `as_biguint()` の挙動に丸ごと委ねている。さらに、`big.to_bytes_be()` が `expected_len` を超える場合 `pad_zero_to_length` は no-op を返し、その後の長さチェック `if sig_slice.len() != expected_len` で初めて気付くが、エラー文言が「want / got」だけで攻撃検知としては弱い。
- 問題: ECDSA DER 形式に対する厳密性が不十分。`p384` / `p256` crate が DER 直接 verify をサポートしている（`Signature::from_der`）ため、独自パースを廃止し crate 標準に委譲したほうが攻撃面が小さい。
- 修正案: `ec_decode_sig` を撤去し、`p256::ecdsa::Signature::from_der(sig)` / `p384::ecdsa::Signature::from_der(sig)` を `verify_signature` 内で直接呼ぶ。`cert.rs:85-87` の `ec_decode_sig` 経路も同様に置き換える。

### must-fix-006  RSA-PSS の MGF1 hash / saltLength が AlgorithmIdentifier parameters から読まれていない

- 場所: `crates/attestation-aws-nitro/src/sign.rs:194-201`
- 観察:
  ```rust
  let verifying_key: PSSVerifyingKey<Sha384> = PSSVerifyingKey::new(pub_key);
  ```
  Sha384 でハードコードされており、RSASSA-PSS の AlgorithmIdentifier parameters（RFC 4055 §3.1 で MGF / hashAlgorithm / saltLength を明示する）を一切参照していない。
- 問題: 実装が「RSASSA-PSS = SHA-384 固定」を強制している。仕様準拠の PSS では parameters なしの場合は SHA-1 がデフォルトであり、明示された場合はその hash を使わなければならない。AWS Nitro のチェーンには RSA は出ないため実害は今のところゼロだが、コード上は「RSA-PSS を受け入れる」と公言しているのに誤実装。OSS 公開時の監査リスク。
- 修正案: (a) `KeyAlgo::RSA` 経路と RSASSA-PSS 経路を完全に削除する（Nitro のチェーンには登場しないため）。あるいは (b) AlgorithmIdentifier.parameters から正しく MGF1/hash/saltLen を読み、それぞれに対応する `PSSVerifyingKey<H>` を構築する。OSS としての方針が決まっていなければ (a) を推奨。

### must-fix-007  `Cargo.toml` の `sha2 = { version = "0.10", features = ["oid"] }` の `oid` 機能はコード内で未使用

- 場所: `crates/attestation-aws-nitro/Cargo.toml:33`
- 観察: `oid` feature は `sha2` の OID associated const を有効化する機能だが、`sign.rs` / `cert.rs` 内で sha2 の OID 機能（`Sha256::OID` 等）は参照されていない（OID 比較は `x509-parser` 側で行われる）。`sha2_sp1` も同じ `oid` feature を付けているが同じく未使用。
- 問題: 機能の有効/無効が成果バイナリの依存解決とビルド再現性に影響する。「使っていない feature を有効化している」は監査対象としてのノイズであり、本当に必要なのか不明。
- 修正案: `features = ["oid"]` を両 dep から削除する。万一 `x509-parser` 等の transitive 経由で必要であれば、明示的にコメントを残す。

---

### should-fix-001  `lib.rs` のドキュメントが「root pinning は呼び出し側に委ねろ」と書きつつ、実装は内部で pinning を行う

- 場所: `crates/attestation-aws-nitro/src/lib.rs:47-51`
- 観察:
  > Uses the certificate chain shipped inside each Attestation Document (`cabundle`) and trusts it implicitly — AWS rotates the root externally and includes the full chain in every document. Verifiers that require a pinned root should re-check `cert_chain.certs[0]` against their own trusted copy of the AWS Nitro root.
- 問題: 実際は `doc.rs:72-80` で `AWS_NITRO_ROOT_CA_SHA256` との照合を強制している。ドキュメントが古いか、実装が後から防御強化されたかのどちらかで、現状の状態を読者に伝えていない。これは CLAUDE.md が指摘する「過去 rationale の埋め込み」典型例。
- 修正案: 当該段落を削除し、代わりに「Pins the chain root to `AWS_NITRO_ROOT_CA_SHA256` (constants.rs). The cabundle's first certificate must match this fingerprint or verification fails.」へ書き換える。

### should-fix-002  `lib.rs:74-78` のコメント「Use the smaller of (now, doc.timestamp/1000) for cert validity」は意図が不明瞭で危険

- 場所: `crates/attestation-aws-nitro/src/lib.rs:74-78`
- 観察:
  ```rust
  // Use the smaller of (now, doc.timestamp/1000) for cert validity, so
  // documents from a TEE whose clock is slightly ahead of ours still
  // verify while genuinely expired certificates are still caught.
  let check_ts = report.doc().timestamp.saturating_div(1000).min(now_unix_secs);
  ```
- 問題: cert validity は `(not_before, not_after)` の範囲チェック。`min(now, doc.ts)` を使うと、`doc.ts > not_after` でも `now < not_after` なら通ってしまう。つまり「過去に作られた doc を 100 年後に検証しても通る」が成立する。仕様 §5.2 の attestation 再生（replay）防御は別レイヤだが、cert 期限と doc 期限の二重チェックが崩れる点は中レベルの defence-in-depth 後退。trait の `now_unix_secs` の意味も「現在時刻」とドキュメントに書かれており、内部で意図的に小さい値に丸めるのは契約違反。
- 修正案: `check_ts = now_unix_secs` のみとし、コメントを削除する。TEE 側のクロックずれは Gateway/オペレーター責務として別途記載。最低限、`min()` ではなく `max()` の意味であった可能性を疑い意図を再確認する。

### should-fix-003  `AttestationError::Expired` variant が定義されているが、実装側で一度も使われていない

- 場所: `crates/attestation/src/lib.rs:56-58` / `crates/attestation-aws-nitro/src/lib.rs:80-81`
- 観察: `AttestationError` に `Expired(String)` があるが、AWS Nitro 実装では cert 期限切れも doc timestamp 異常も全て `SignatureInvalid(format!("{e:?}"))` に丸めている。
- 問題: 横断 trait のエラー粒度が呼び出し側で使えない。Solana Extension（`solana/src/extension.rs:50`）は `Verifier(#[from] AttestationError)` で受け取っているが、`Expired` と `SignatureInvalid` を区別する経路を持てない。
- 修正案: `cert.rs:39-54` の `check_valid` を専用エラーで返し、`doc.rs:authenticate()` 内で down-cast して `AttestationError::Expired` にマップする。あるいは `Expired` variant を削除し、`SignatureInvalid` 一本化する。「定義したが使わない」状態を解消する。

### should-fix-004  `MockAttestationVerifier::PREFIX` がパブリック定数として公開されている

- 場所: `crates/attestation/src/lib.rs:107-114`
- 観察:
  ```rust
  pub const PREFIX: &'static [u8] = b"mock-attestation:";
  ```
  使用例: `crates/solana/src/extension.rs:228` が `MockAttestationVerifier::PREFIX.to_vec()` で攻撃シミュレーションテストを書いている。
- 問題: 「mock の入力フォーマット」を crate API として固めると、production 経路から `MockAttestationVerifier::PREFIX` を参照したコードが書ける。`mock` feature gate と組み合わさるが、`title-tee` のように両方を build 時に有効化するクレートでは、結局 production binary でも mock prefix が分かる文字列として焼き込まれる。
- 修正案: `PREFIX` を `pub(crate)` に下げ、`build_mock_attestation(user_data: &[u8]) -> Vec<u8>` のような helper を公開する。テストはその helper 経由で書く。

### should-fix-005  `MockAttestationVerifier::MEASUREMENT` が `[0u8; 48]` で hard-coded

- 場所: `crates/attestation/src/lib.rs:113-114` および `crates/gateway/tests/e2e.rs:79` / `crates/tee/src/server.rs:332` での使用
- 観察: 「mock の measurement は 48 zeros」が public const として固定されており、`expected_measurement` の比較に直接渡されている。
- 問題: production の measurement 照合ロジック（仕様 §1.2、§1.6）の意味的妥当性が、mock テストでは「ゼロベクトル vs ゼロベクトル」になり、`if expected != measurement` の分岐が一度もテストされない可能性が高い。
- 修正案: mock の measurement をランダム or 認識しやすい値（例: `[0xAA; 48]`）に変更し、テスト側で「期待値 = mock measurement」「期待値 ≠ mock measurement」両方のケースを書く。

### should-fix-006  `AttestationVerifier::verify` の `now_unix_secs: u64` の semantics がドキュメント不足

- 場所: `crates/attestation/src/lib.rs:80-86`
- 観察: docstring は「reference time used for certificate validity checks」と書くが、TEE 内 doc の timestamp との関係、未来日付に対する挙動、`u64` の単位（秒）の暗黙性、UNIX 元期からの計算が前提など、契約が明確でない。
- 問題: ベンダー追加時の trait 利用者が、`now_unix_secs` に何を渡すべきか迷う。SP1 guest 内では時計が無いはずで、その場合の渡し方の指針もない。
- 修正案: docstring に「(1) Unix seconds since 1970-01-01 UTC, (2) implementations MUST reject documents whose internal timestamp is more than X seconds in the future, (3) when no real wall clock is available (zkVM guest), pass the doc's own timestamp converted to seconds」を明記する。

### should-fix-007  `pad_zero_to_length` は `Vec<u8>` を消費して新規 alloc する。`expected_length` を超える入力時の挙動も曖昧

- 場所: `crates/attestation-aws-nitro/src/sign.rs:213-222`
- 観察:
  ```rust
  if input.len() < expected_length {
      let padding = expected_length - input.len();
      let mut padded = vec![0; padding];
      padded.extend(input);
      padded
  } else {
      input
  }
  ```
  `>` の場合は input をそのまま返すので、呼び出し側（line 141-147）の `if sig_slice.len() != expected_len` で初めてエラーになる。
- 問題: 関数名が「pad to length」と言っているのに `len > expected` の場合は何もしない。must-fix-005 と合わせて削除候補。残すなら `Result<Vec<u8>, _>` でオーバーフローをエラーで返すべき。
- 修正案: must-fix-005 の修正で `ec_decode_sig` ごと削除されれば本関数も不要。残す方針なら `if input.len() > expected_length { return Err(...) }` を冒頭に追加。

### should-fix-008  `cose.rs:25-31` の `sig_algo_val` が `EcdsaSHA256` と `EcdsaSHA384` 以外をエラーにしているが、エラー文言が "unsupport sigAlgo" とタイポ

- 場所: `crates/attestation-aws-nitro/src/cose.rs:25-31`
- 観察:
  ```rust
  alg => return Err(anyhow!("unsupport sigAlgo: {:?}", alg)),
  ```
- 問題: "unsupport" は "unsupported" の typo。OSS として公開するエラー文言の品質。
- 修正案: `"unsupported sigAlgo: {:?}"` に修正。同 crate 内には他にも `"unsupport"` / `"sigAlgo"` 等の表現があれば合わせて修正する。

### should-fix-009  `cose.rs:99-153` の `Deserialize` 手書き実装で、`visit_seq` が tag 18（COSE_Sign1）チェックを行わない

- 場所: `crates/attestation-aws-nitro/src/cose.rs:99-153`
- 観察: tag チェックは `CoseSign1::from_bytes` で `serde_cbor::tags::Tagged<Self>` を経由して行う設計だが、`Deserialize` の手書き実装は tag を一切意識しない。`from_bytes` を経由しない経路（serde 直接呼び出し）が将来できると tag 検証が外れる。
- 問題: API の安全性が `from_bytes` の利用に依存している。`Deserialize` を `impl` した時点で外部から `serde_cbor::from_slice::<CoseSign1>` 直接呼び出しが可能。
- 修正案: `Deserialize` impl を `pub(crate)` ではなく完全に private にする、または `CoseSign1` 自体を crate-private にして `from_bytes` のみ公開する。

### should-fix-010  `cose.rs:73-76` のエラーで「protected header に algorithm が無い」場合 `anyhow!` を返すが、`sig_algo` 引数が無視されている

- 場所: `crates/attestation-aws-nitro/src/cose.rs:60-82`
- 観察: `verify_signature(&self, sig_algo: SigAlgo, ...)` の `sig_algo` は (a) protected.alg との比較用、(b) `sig_algo_val` への引数、として使われるが、protected.alg が無い場合のエラーは「呼び出し側が指定した sig_algo を無視してエラー」になる。
- 問題: 攻撃者が protected を空にすると、`verify_signature` は `false` ではなく `Err` を返す。これは結果的に「失敗」だが、呼び出し側 `doc.rs:92-97` は `Err` のときと `Ok(false)` のときの扱いを違える（`?` で前者は即 propagate）。signature 失敗は全部 `Ok(false)` のほうが API 一貫性が高い。
- 修正案: protected.alg 欠落を `Ok(false)` で返す。protected が CBOR としてパース不能な場合のみ `Err`。

### should-fix-011  `constants.rs:5` の `oid` macro import 経由で内部依存している `x509_parser::der_parser` が `x509-parser` の private re-export 経路

- 場所: `crates/attestation-aws-nitro/src/constants.rs:5` / `Cargo.toml:37`
- 観察: `oid = "0.2"` を Cargo.toml に dep 宣言しているが、`constants.rs` で使われているのは `x509_parser::der_parser::{oid, Oid}`（macro と type）であり、独立の `oid` crate は `sign.rs:9` の `ObjectIdentifier::try_from` でしか使われない。
- 問題: 依存が重複しており、`x509_parser::der_parser::Oid` と `oid::ObjectIdentifier` の二系統が同一目的で混在している。
- 修正案: 一方に統一する。AWS Nitro 用には `x509-parser` 経由の `Oid` のみで足りるはずなので、`oid` crate dep を削除し、`sign.rs:55-58` の OID パースを `x509-parser` 内の機能に書き換える。

---

### nitpick-001  `attestation/Cargo.toml:12-15` の mock feature コメントが冗長

- 場所: `crates/attestation/Cargo.toml:11-15`
- 観察:
  ```
  # Enables `MockAttestationVerifier`. Dev / test only — production builds for a
  # real TEE should leave this disabled so the mock can never be selected at
  # runtime by mistake.
  mock = []
  ```
- 問題: コメントは正しいが「production では off」「by mistake」は CLAUDE.md 例にあるような「ない/しない理由の埋め込み」気味。短くてよい。
- 修正案: `# Enables MockAttestationVerifier. Test-only.` の 1 行に圧縮。

### nitpick-002  `attestation/src/lib.rs:5-7` の Spec 参照が「§1.2, §5.2, §6.2」のリスト羅列

- 場所: `crates/attestation/src/lib.rs:5-7`
- 観察: 「Spec §1.2, §5.2, §6.2」を crate doc に付けているが、§5.2 と §6.2 がこの crate と直接対応しているかは曖昧。
- 問題: CLAUDE.md 例の「Spec §6.2 — see also Spec §5.2」型の節参照癖。読み手が SPECS を引いた時の費用対効果が薄い。
- 修正案: §1.2（Attestation Document の役割）に限定して、他は削除。あるいは「verifier interface — see Spec §1.2 for the document model」に書き換え。

### nitpick-003  `attestation-aws-nitro/src/lib.rs:14-19` の Origin コメントが大きすぎる

- 場所: `crates/attestation-aws-nitro/src/lib.rs:14-19`
- 観察: 「Automata Network → Amazon → RustCrypto port」の系譜を 6 行記載。
- 問題: ライセンス・由来は重要だが、`NOTICE` / `THIRD-PARTY-NOTICES` 等に集約してもよい。crate 冒頭にここまでの説明は冗長。
- 修正案: 1 行 `// Derived from Automata Network's aws-nitro-enclave-attestation (Apache-2.0).` に圧縮、詳細は `NOTICE` ファイルへ。

### nitpick-004  `lib.rs:103-119` のテスト `vendor_tag_consistent` と `rejects_invalid_bytes` が浅い

- 場所: `crates/attestation-aws-nitro/src/lib.rs:107-119`
- 観察: 1 つ目は `assert_eq!("aws-nitro", "aws-nitro")` 同然、2 つ目は `matches!` を `assert!` で囲んでおらず常に成功する死テスト。
- 問題: `matches!` 単独は副作用なく評価して捨てるだけ。`assert!(matches!(err, AttestationError::ParseFailed(_)));` が正しい。テスト品質低下。
- 修正案: `assert!(matches!(err, AttestationError::ParseFailed(_)));` に修正し、`vendor_tag_consistent` は削除する。

### nitpick-005  `cert.rs:100` のコメント `// cert order: root -> leaf` がフィールド doc コメントになっていない

- 場所: `crates/attestation-aws-nitro/src/cert.rs:99-102`
- 観察:
  ```rust
  pub struct CertChain<'a> {
      // cert order: root -> leaf
      pub certs: Vec<Cert<'a>>,
  }
  ```
- 問題: `//` ではなく `///` にすれば rustdoc に出る。`pub` フィールドの順序仕様は API 契約。
- 修正案: `/// Certificates are ordered root → leaf.` に変更し、`pub certs` の前に置く。

### nitpick-006  `doc.rs:53-57` の `trusted_certs_len` ドキュメントは「production で 0」と書きながら API 上の引数を維持

- 場所: `crates/attestation-aws-nitro/src/doc.rs:53-57`
- 観察:
  > The `trusted_certs_len` parameter is retained for parity with the underlying CertChain API but should be set to 0 in production
- 問題: must-fix-002 と重複するが、コメント自身が「使うな」と書いている API を残しているのは CLAUDE.md の「不要な rationale 埋め込み」典型。
- 修正案: must-fix-002 で API を消す。コメントも自然消滅。

## 全体所感

ロジックは Automata Network 由来で枯れており、cabundle の root pinning（doc.rs:72-80）と RustCrypto 移植は妥当な作りになっている。一方、(1) cert chain の root 自己署名検証と pin の二重防御が API 表面に染み出していること、(2) `trusted_certs_len` という「使うな」と書いた引数を残していること、(3) 起源不明な RSA/RSA-PSS 経路がハードコード hash で生きていること、(4) `now_unix_secs` の `min()` 折り畳みなど時刻まわりの暗黙仕様、の 4 点が「コードを読む第三者の理解コスト」を押し上げている。

mock feature 周りは `PREFIX` が pub になっており、production 経路から参照可能な点だけ要注意（should-fix-004）。trait の API surface はベンダー追加時には Solana Extension 側を含めて全置換になりそうで、`AttestationError::Expired` のような未使用 variant や曖昧な `now_unix_secs` 契約を整理しておくと将来のベンダー追加が楽になる。

OSS 公開を見据えるなら、`sign.rs` の独自 DER パーサーを RustCrypto crate 標準の `Signature::from_der` に置き換える（must-fix-005）のが最も投資対効果が高い。
