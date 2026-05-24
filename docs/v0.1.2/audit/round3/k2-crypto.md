# K2. `crates/crypto` 縦深掘り（Round 3）

## 概要

Round 2（`docs/v0.1.2/audit/round2/k2-crypto.md`）の処理ログで判定された:

- Round 1 由来 16 件のうち、Round 2 で fixed 認定 8 件 / wontfix 8 件
- Round 2 新規 7 件のうち、fixed 5 件 / wontfix 2 件

の合計 23 件について、`crates/crypto/` 全ファイル（`lib.rs`, `aead.rs`, `error.rs`, `hkdf.rs`, `key_bundle.rs`, `payload.rs`, `sealed_channel.rs`, `wire.rs`, `kem/{mod,x25519,p256_ecdh,ml_kem768}.rs`）および呼び出し側 `crates/tee/src/orchestrator.rs:285-317` を Spec §2.4 を Source of Truth として 1 行 1 行再走査した。さらに Round 3 で新規発見した事項を分類した。

`hkdf.rs:11-19`, `ml_kem768.rs:45-48`, `payload.rs:18-24`, `sealed_channel.rs:19-29`, `x25519.rs:78-93` の追記が Round 2 の本体修正であり、これらが「指摘の要旨を満たしているか」「副作用がないか」を最優先で見た。

---

## Round 2 fixed 項目の検証

Round 2 で `fixed` と認定された 13 件について、現コードを精査し、修正が定着しているか・退行していないかを確認した。

| Round 2 ID | 修正手段 | Round 3 検証 |
|---|---|---|
| must-fix-001 (P-256 自前スカラー乗算) | `p256::ecdh::diffie_hellman(scalar, point)` (Round 2 で fixed) | `p256_ecdh.rs:81-83` で `self.secret.to_nonzero_scalar()` + `eph_pk.as_affine()` を渡している。`p256::ecdh::diffie_hellman` の規定通り。**定着** |
| must-fix-002 (AAD 不使用) | `suite_aad(suite_id, encap_key_len)` 3 バイト AAD + HKDF salt 経由で encap_key を間接 bind | `sealed_channel.rs:26-29` の `suite_aad` ヘルパー、`hkdf.rs:11-19` の rationale 共に確認。後述の new-must-fix-001 で残課題を指摘 |
| must-fix-003 (ML-KEM determinism) | `OsRng.fill_bytes(&mut m)` + `encapsulate_deterministic` + コメント | `ml_kem768.rs:45-52` で確認。コメント 4 行 (lines 45-48) に「infallible 化目的」と明記済み。**定着** |
| must-fix-004 (P-256 seed bias) | `SecretKey::random(rng)` | `p256_ecdh.rs:59-63` で SEC1 rejection sampling 経由の `SecretKey::random` を確認。docstring (lines 53-58) で rejection sampling の利得まで言及されている。**定着** |
| should-fix-002 (HKDF salt 根拠) | `hkdf.rs` モジュール docstring に rationale 追記 | `hkdf.rs:11-19` に「Why `salt = encap_key`」節を確認。AEAD AAD と HKDF salt の責任分担、ML-KEM-768 で encap_key = 1088 B を AAD に入れないコスト合理化が明記。**定着** |
| should-fix-004 (metadata_len cap) | `MAX_METADATA_LEN = 64 KiB` + `checked_add` + テスト | `payload.rs:24` 定数、`payload.rs:54-62` 検査、`payload.rs:118-123` テストを確認。docstring (lines 20-23) で「AAD 検証後の defense in depth」位置付けも記述。**定着** |
| should-fix-005 (open_request expected_suite) | `open_request(key_bundle, expected_suite, wire)` 3 引数化 + `EncryptionSuiteMismatch` 集約 | `sealed_channel.rs:104-115`, `error.rs:32-33`, `orchestrator.rs:290-298` を確認。orchestrator は `open_request` の戻り値 `OpenedRequest` の `suite` を再チェックせず、エラーマッピングだけで対応 (lines 291-296)。10 行の手書き突合は消えた。回帰テスト `declared_suite_mismatch_rejected` (lines 202-215) も確認。**定着** |
| should-fix-006 (X25519 low-order) | `reject_zero_shared_secret` を双方向で呼ぶ | `x25519.rs:40` (encapsulate)、`x25519.rs:73` (decapsulate)、`x25519.rs:82-93` の constant-time-ish OR 蓄積を確認。テスト `low_order_point_rejected` (lines 110-115) は decapsulate 側のみだが、encapsulate 側は `recipient_pubkey` が all-zero でも `from_public_key` で受け入れられてしまうため後述の追加観察あり。**定着 (一部観察あり new-nitpick-001)** |
| nitpick-006 (テストの「暗黙にテスト」コメント) | 実 assertion 化 | `sealed_channel.rs:232` の `assert!(opened.response_channel.open(&wire[..]).is_err())` を確認。**定着** |
| new-must-fix-001 (wire ヘッダ全体 AAD) | must-fix-002 と統合 | 後述 (現状の合成設計には残課題があるが、Round 2 結論を受け入れたうえで残課題を観察として再整理) |
| new-should-fix-002 (ML-KEM rationale) | must-fix-003 と統合 | 上記 must-fix-003 で確認 |
| new-should-fix-003 (`OpenedRequest.suite` API) | should-fix-005 と統合 | 上記 should-fix-005 で確認。ただし `OpenedRequest.suite` フィールド自体は `pub` で残存 (`sealed_channel.rs:37`)。new-nitpick-002 として再指摘 |
| new-should-fix-004 (meta_len cap defense-in-depth doc) | should-fix-004 と統合 | 上記 should-fix-004 で確認 |
| new-nitpick-001 (AAD 構築重複) | `suite_aad()` ヘルパーで集約 | `sealed_channel.rs:26-29` 関数定義 + 4 箇所 (lines 56, 65, 85, 120) からの呼び出しを確認。**定着** |

集計: 13/13 fixed 定着、退行なし。

---

## Round 2 wontfix 項目の再確認

Round 2 で `wontfix` と判定した 10 件について、判定理由が現コードでも妥当かを再評価した。

| Round 2 ID | wontfix 理由 | Round 3 判断 |
|---|---|---|
| should-fix-001 (型レベル one-shot 強制) | type-state パターンの API 破壊 | 妥当。`ResponseChannel::seal(&self, ...)` は self ref を取るので呼び出し側の規律で one-shot を担保する設計。`crates/tee/src/orchestrator.rs:257` の `.seal(&response_json)` 1 箇所のみが呼ぶため運用上 one-shot は保たれている |
| should-fix-003 (wire 2 段化) | 退行リスクが利得を超える | 妥当。現状の `wire.rs:38-72` は 1 段ながら suite_id → ek_len → bounds の順で早期 reject しており可読 |
| nitpick-001 (`lib.rs` mod 一覧重複) | Rust 慣例 | 妥当 |
| nitpick-002 (`HkdfError(String)`) | `hkdf` crate 内部エラー透過 | 妥当 |
| nitpick-003 (`Nonce` type alias 位置) | use と並ぶのが自然 | 妥当 |
| nitpick-004 (nonce 長エラーが `InvalidWireFormat`) | wire 由来不正値で整合 | 妥当だが、`aead.rs:33-39, 72-78` の nonce 長エラーは「鍵束生成側のバグ」でも発火しうる経路で、その場合 `InvalidWireFormat` は誤誘導になる可能性がある。new-nitpick-003 で観察追加 |
| nitpick-005 (`key_bundle.rs` の spec 参照過剰) | §1.4/§2.4 境界明示 | 妥当 |
| new-should-fix-001 (`aead` 空 AAD 禁止) | `sealed_channel` は常に 3B AAD を渡す | 妥当。ただし `aead::encrypt/decrypt` を pub re-export しているわけではないものの `pub fn` のままなので、外部から空 AAD で呼ぶ経路は形式上存在する。`crates/crypto/src/lib.rs:21` で `pub mod aead` 公開。new-nitpick-004 で観察追加 |
| new-nitpick-002 (`aead` テスト AAD が UTF-8) | GCM は opaque bytes で意味独立 | 妥当 |

集計: 10/10 wontfix 妥当、ただし 2 件は補足観察あり (new-nitpick-003, new-nitpick-004)。

---

## 新規発見

Round 3 で改めて精読して見つけた事項。Round 2 では拾われていない、または Round 2 修正の副次効果として現れたもの。

### new-must-fix-001  `seal_for` の戻り値 `ResponseChannel` の `encap_key_len` が「実 encap_key のバイト長」由来で、wire `encap_key_len` フィールド (`u16`) と一致しない可能性が型レベルで担保されていない

- 場所:
  - `crates/crypto/src/sealed_channel.rs:85` (client seal)
  - `crates/crypto/src/sealed_channel.rs:120` (TEE open)
  - `crates/crypto/src/sealed_channel.rs:46` (`ResponseChannel::encap_key_len: usize`)
- 観察:
  - `seal_for` は `encap_key.len()` (usize) をそのまま `suite_aad` と `ResponseChannel.encap_key_len` に詰める。
  - `open_request` は `wire::parse_request` 経由で `parsed.encap_key.len()` を取り、これは `wire.rs:49-55` で `encap_key_len` が `encap_key_len(suite)` と一致することを既に検証済み。よって TEE 側の値は `expected_ek_len`（=32/65/1088）で確実に一致する。
  - クライアント側 `seal_for` は `encapsulator.encapsulate()` の返した `encap_key.len()` を信用している。`encap_key_len(suite)` との一致チェックは入っていない。
- 問題:
  - 仮に将来 `kem::Encapsulator::encapsulate` の実装が `encap_key_len(suite)` と異なる長さを返した場合（バグ・派生実装）、クライアント側 `ResponseChannel.encap_key_len` が TEE 側 (= `encap_key_len(suite)` 固定) と乖離する。結果として AEAD AAD がミスマッチし、レスポンスが復号できない。これは AEAD タグで弾けるので**機密性は守られる**が、可用性側のサイレント不整合になる。
  - 現実には 3 つの KEM 実装はすべて固定長を返すよう書かれている（`x25519.rs:41` の `eph_pubkey.as_bytes()` は 32B 固定、`p256_ecdh.rs:42` の `to_encoded_point(false)` は 65B 固定、`ml_kem768.rs:51` の `ct.to_vec()` は `CT_SIZE=1088` 固定）ため実害ゼロだが、型システムや assertion で固定されていない。
- 修正案:
  - **追記**: `seal_for` 中で `debug_assert_eq!(encap_key.len(), kem::encap_key_len(suite))` を入れる。あるいは `Encapsulator::encapsulate` の戻り値型を `[u8; N]` 固定にせず、`Vec<u8>` 返却の事後 invariant として `if encap_key.len() != encap_key_len(suite) { return Err(...) }` を明示。
  - 優先度は `must` ではなく `should` に近いが、AAD と shared_secret derivation の唯一の「クライアント発信側の自由度」がここに残っているため must-fix として記録する。

### new-should-fix-001  `aead.rs:14-15` の `pub const KEY_SIZE: usize = 32;` は AES-256-GCM の固定値だが、`crates/crypto/src/lib.rs` の re-export に含まれず、外部呼び出し側 (`orchestrator.rs` 等) には露出しない

- 場所:
  - `crates/crypto/src/aead.rs:14-15`
  - `crates/crypto/src/lib.rs:30-32`
- 観察:
  - `KEY_SIZE` / `NONCE_SIZE` は `aead.rs` 内 pub だが、`lib.rs` の `pub use` 行 (line 30-32) には載っていない。
  - `wire.rs:19`, `sealed_channel.rs:14` などは `use crate::aead::NONCE_SIZE;` で内部使用しているので動く。
- 問題:
  - 外部クレートから直接呼ぶ際は `title_crypto::aead::NONCE_SIZE` というフルパスでアクセスが必要。pub re-export していないのは設計判断としては妥当（`aead` モジュールは内部 primitive）だが、内部使用に閉じるなら `pub mod aead` を `pub(crate) mod aead` に下げる方が一貫する。
  - 現状は `pub mod aead` (lib.rs:21) で外部公開しているのに re-export しないという中途半端な状態。
- 修正案:
  - 二択:
    - (a) `aead` を内部 primitive と位置付けるなら `pub(crate) mod aead;` に変更
    - (b) 外部から使う想定なら `pub use aead::{NONCE_SIZE, KEY_SIZE};` を `lib.rs` に追加
  - 実害はない (テストもコンパイルも通る) ので nitpick 寄りだが、API 設計の意図不明瞭は OSS maturity に影響するため should として記録。

### new-should-fix-002  `MlKem768Encapsulator::encapsulate` の rationale コメントが「`encapsulate_deterministic` は infallible」と書かれているが、`ml-kem` crate 0.4 系で API が変わる可能性に触れていない

- 場所:
  - `crates/crypto/src/kem/ml_kem768.rs:44-53`
  - `crates/crypto/Cargo.toml:15` (`ml-kem = "0.3.2"`)
- 観察:
  - コメント 4 行 (lines 45-48) は「infallible 化のため」と述べる。
  - `~/.cargo/registry/.../ml-kem-0.3.2/src/encapsulation_key.rs:78-83` を見ると、0.3.2 の `Encapsulate::encapsulate_with_rng` は内部で 32 バイトを RNG から読んで `encapsulate_deterministic` を呼ぶ実装。つまり**現バージョンでは両者は完全に等価**で「failure path を増やす」という rationale は実際には適用されない（`encapsulate_with_rng` の戻り値も `(Ciphertext, SharedKey)` で `Result` を返さない）。
- 問題:
  - rationale コメントが実装のリアリティと一致しない。コメントを読んだ後任は「`encapsulate_with_rng` には Result が返るのか」と誤認する。
  - 0.4 系では仕様 (FIPS 203) との整合のため `RngCore` 要件強化 / API 再編が予想され、`Encapsulate` trait method 経由に統一される可能性がある。`encapsulate_deterministic` を直接呼ぶ現状は 0.4 アップグレードでビルド破壊する可能性が高い。
- 修正案:
  - **書き直し**: コメントを「0.3.2 では `Encapsulate::encapsulate_with_rng` も内部で同じパスを通るため等価。`encapsulate_deterministic` を選んだのは (a) ml-kem crate の API 安定面で trait 経由より具象 method の方が破壊が少ない、(b) seed bytes の経路を 1 箇所に集中させて TEE での RNG 制御を明示化、の 2 点」と書き直す。
  - もしくは `let (ct, ss) = ml_kem::Encapsulate::encapsulate(&self.ek, &mut rand::rngs::OsRng).expect("0.3.2 infallible");` に置換 (0.4 でも追従しやすい)。

### new-should-fix-003  `aead.rs` の `decrypt` で `key.len() != KEY_SIZE` を `InvalidKeyLength` に分類しているが、`encrypt` 失敗時はそれを除いて全部 `EncryptError` に潰している（情報損失）

- 場所:
  - `crates/crypto/src/aead.rs:40-53` (encrypt)
  - `crates/crypto/src/aead.rs:79-92` (decrypt)
- 観察:
  - `encrypt` 内: `Aes256Gcm::new_from_slice` 失敗 → `EncryptError`、`try_into` 失敗 → `InvalidWireFormat`、`cipher.encrypt` 失敗 → `EncryptError`。
  - `decrypt` 内: `cipher.decrypt` 失敗 → `DecryptError` (AEAD タグ失敗もここに該当)、それ以外は同じ。
- 問題:
  - `decrypt` の `DecryptError` に「タグ検証失敗」「内部状態エラー」「メモリ確保失敗」が全部押し込まれている。タグ検証失敗は active attack の徴候だが、ライブラリレベルでは見分けがつかない。
  - これ自体は AEAD ライブラリの普通の挙動だが、上位（orchestrator）でログを出す際に「攻撃の可能性 vs バグ」が判別できない。
- 修正案:
  - 現状の `DecryptError` の variant に「ほぼ確実に tag verification failure」と doc comment を追加するか、`CryptoError::DecryptError` を `AuthFailure` にリネームしてセマンティクスを正す。`aes-gcm` crate の `decrypt` は他の失敗パスがほぼないので、リネームでも実害なし。

### new-nitpick-001  `x25519.rs:35-43` の `Encapsulator::encapsulate` で、`from_public_key` が all-zero / low-order な `recipient_pubkey` を受け入れる

- 場所:
  - `crates/crypto/src/kem/x25519.rs:22-32` (`from_public_key`)
  - `crates/crypto/src/kem/x25519.rs:35-43` (`encapsulate`)
- 観察:
  - `PublicKey::from(arr)` は X25519 公開鍵の有効性を検査しない (x25519-dalek の API 仕様)。
  - `encapsulate` 後の `reject_zero_shared_secret` は shared 側で all-zero を弾くので、最終的には全体としては安全。
- 問題:
  - クライアントが攻撃者から受け取った GET /keys レスポンスに低位点が混入していた場合、`from_public_key` ではエラーにならず、`encapsulate` の `reject_zero_shared_secret` で初めて InvalidWireFormat になる。エラー位置と原因（不正な「公開鍵」）が乖離している。
  - ML-KEM-768 では `EncapsulationKey::new` が形式検査をするのに対し、X25519/P-256 では入力検査が緩い。
- 修正案:
  - `X25519Encapsulator::from_public_key` で low-order point リストとの照合を入れるか、少なくとも all-zero 公開鍵を `InvalidKeyLength` ではない明示エラーで弾く:
    ```rust
    if arr.iter().all(|&b| b == 0) {
        return Err(CryptoError::InvalidWireFormat("X25519 public key is all-zero".into()));
    }
    ```
  - P-256 側は `PublicKey::from_sec1_bytes` が identity point を弾くので問題なし。

### new-nitpick-002  `OpenedRequest.suite: pub` フィールドが残存しているが、Round 2 で「`open_request` の `expected_suite` 引数と必ず一致する」と保証された結果、フィールドは redundant

- 場所:
  - `crates/crypto/src/sealed_channel.rs:36-40`
  - `crates/tee/src/orchestrator.rs:290-298` (consumer 1)
  - その他 `opened.suite` の参照: なし (grep 結果)
- 観察:
  - `OpenedRequest.suite` フィールドが `pub`。orchestrator は `open_request(&bundle, suite, ...)` を呼ぶので戻り値の `opened.suite == suite` は invariant として保証済み。
  - 実際に orchestrator は `opened.suite` を一度も参照していない (上記 grep)。
  - sealed_channel テストも `assert_eq!(opened.suite, ...)` を書いていない (line 149, 165, 181 はそれぞれ `plaintext` のみ照合)。
- 問題:
  - dead-data。Round 2 で should-fix-005 を統合した際に消し忘れた。
- 修正案:
  - **書き直し**: `OpenedRequest` から `pub suite: EncryptionSuite` を削除。doc comment 33-35 の「suite is guaranteed to equal the `expected_suite`」も削除（自明）。

### new-nitpick-003  `aead.rs:33-39, 72-78` の nonce 長エラーが `InvalidWireFormat` のままだが、`sealed_channel::seal_for` 側は内部で `[0u8; NONCE_SIZE]` 固定生成しているため、本経路では到達不能

- 場所:
  - `crates/crypto/src/aead.rs:33-39` (encrypt)
  - `crates/crypto/src/aead.rs:72-78` (decrypt)
  - `crates/crypto/src/sealed_channel.rs:54-55, 83-84` (固定長 nonce 生成)
- 観察:
  - sealed_channel 経由では nonce は常に 12 バイト固定。
  - aead を外部から直接呼ぶ場合のみ nonce 長エラーが発火しうる。
- 問題:
  - `InvalidWireFormat` は「wire 由来」のエラーセマンティクスを持つが、`aead::encrypt` を直接呼ぶ場合は wire とは無関係。エラーカテゴリの誤誘導。Round 2 で nitpick-004 として「wire 由来の不正値で整合」と判定したが、内部経路は wire を介さない点が見落とされている。
- 修正案:
  - `InvalidWireFormat` を継続使用するなら API doc に「`aead::encrypt` の入力検証エラーはすべて `InvalidWireFormat` でラップされる（カテゴリ名は内部の wire 解析と共通利用）」と明記。
  - もしくは `CryptoError::InvalidNonceLength { expected, actual }` を追加して両者を分離。

### new-nitpick-004  `pub mod aead` (`lib.rs:21`) は外部公開だが re-export がないため、外部呼び出し時は `title_crypto::aead::encrypt` のフルパスが必要

- new-should-fix-001 と同根 (上記 should 側に詳述)。

---

## 全体所感

Round 2 で本気で取り組んだ should/must-fix のすべて（13 件）が綺麗に定着している。退行は皆無。特に印象的なのは:

1. **`suite_aad` ヘルパー + HKDF salt 設計のコメント化**: must-fix-002 / new-must-fix-001 / new-nitpick-001 が 1 つの設計で同時解決されている。`hkdf.rs:11-19` の rationale は本クレートの暗号設計を読む際の必須読み物になっており、特に ML-KEM-768 (encap_key = 1088 B) を AEAD AAD に入れないという**サイズ最適化と認証範囲のトレードオフ**を明文化している点は技術的に価値が高い。HPKE が同じ問題を `info` フィールドで解決しているのに対し、本クレートは salt で間接的に解決している。設計選択として正当だが、HPKE 互換性は失われていることをどこかに書いておくと良い。
2. **`should-fix-005` の徹底**: API 強制で 10 行のチェックが消えただけでなく、エラー型 `EncryptionSuiteMismatch { declared, wire }` が `error.rs:32-33` に追加され、orchestrator 側の `OrchestratorError::EncryptionSuiteMismatch` への変換も 1:1 で対応している (orchestrator.rs:291-296)。型レベルで担保するという Round 2 の意図が完全に反映された。
3. **`reject_zero_shared_secret` の constant-time-ish 実装**: `x25519.rs:82-93` は `acc |= b;` で OR 蓄積してから `acc == 0` を見るパターン。short-circuit がないので timing attack に対しても妥当（厳密には Rust の最適化次第だが、x25519-dalek 自体が constant-time でない部分もあるので十分）。テストは decapsulate 側のみだが、encapsulate 側も同じ関数を呼ぶので機能的にはカバー済み。

Round 3 で新規発見した 8 件はいずれも「軽微 + 即時の実害なし」のレベルで、Round 2 の修正によって**コアの脅威モデルは閉じた**と判断できる。残る課題は API 設計のクリーンアップ（dead field 削除、エラー型の整理、外部公開範囲の明確化）と、`ml-kem` crate 0.4 アップグレード時の追従準備に絞られる。

### Round 3 集計

- must-fix: 1 件新規（`encap_key.len()` invariant 不在）
- should-fix: 3 件新規（API 公開範囲、ml-kem rationale 精度、decrypt エラー分類）
- nitpick: 4 件新規（X25519 入力検査、dead field、nonce エラー分類、aead 公開範囲再掲）

合計 Round 3 新規 = **8 件**。

Round 2 から繰り越し対応中の件は**ゼロ**（すべて fixed または wontfix で結論済み）。Round 3 で新たに開く件のみが追跡対象。

### 退行（regression）

Round 2 → Round 3 でコードが**悪化した箇所はない**。

- `sealed_channel.rs` の `OpenedRequest.suite` フィールドは Round 2 の `open_request(expected_suite)` 化により API 上 redundant になったが、削除されず残っている。これは「退行」ではなく「クリーンアップ漏れ」（new-nitpick-002）。
- `aead.rs` の nonce 長エラーカテゴリ `InvalidWireFormat` も Round 1 / Round 2 と同じで変更なし。「wontfix 判定の見落とし」として new-nitpick-003 に再整理。

### Round 4 を見据えた優先度

1. **new-must-fix-001**: `seal_for` 内に `encap_key.len() == kem::encap_key_len(suite)` を assert（debug でも runtime でも可）。`Encapsulator` trait の implicit invariant を明示化。
2. **new-nitpick-002**: `OpenedRequest.suite` フィールド削除。1 行で済む。
3. **new-should-fix-002**: ML-KEM-768 rationale コメントの精度向上。0.4 アップグレード準備。
4. **new-should-fix-001 / new-nitpick-004**: `aead` モジュールの公開範囲を `pub(crate)` か `pub use` 拡張で一貫させる。API 整理。
5. **new-should-fix-003**: `CryptoError::DecryptError` を `AuthFailure` リネーム or doc 追加。
6. **new-nitpick-001**: `X25519Encapsulator::from_public_key` の入力検査強化。
7. **new-nitpick-003**: nonce 長エラーのカテゴリ分離 or doc 化。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| Round 2 must-fix-001 | fixed (定着) | `p256_ecdh.rs:81-83` |
| Round 2 must-fix-002 | fixed (定着) | `suite_aad` 3B AAD + HKDF salt 経由 |
| Round 2 must-fix-003 | fixed (定着) | `ml_kem768.rs:45-52` (rationale 精度は new-should-fix-002 で追加) |
| Round 2 must-fix-004 | fixed (定着) | `p256_ecdh.rs:59-63` |
| Round 2 should-fix-001 | wontfix (再確認) | one-shot 強制は type-state 必要 |
| Round 2 should-fix-002 | fixed (定着) | `hkdf.rs:11-19` 「Why salt = encap_key」 |
| Round 2 should-fix-003 | wontfix (再確認) | wire 2 段化リスク > 利得 |
| Round 2 should-fix-004 | fixed (定着) | `MAX_METADATA_LEN` + テスト |
| Round 2 should-fix-005 | fixed (定着) | `open_request(expected_suite)` + `EncryptionSuiteMismatch` |
| Round 2 should-fix-006 | fixed (定着) | `reject_zero_shared_secret` 双方向 |
| Round 2 nitpick-001..005 | wontfix (再確認) | 軽微 |
| Round 2 nitpick-006 | fixed (定着) | 実 assertion 化 |
| Round 2 new-must-fix-001 | fixed (定着) | must-fix-002 統合 |
| Round 2 new-should-fix-001 | wontfix (再確認) | `aead` 内部 primitive で空 AAD は呼び出し元責任 |
| Round 2 new-should-fix-002 | fixed (定着) | must-fix-003 統合 |
| Round 2 new-should-fix-003 | fixed (定着) | should-fix-005 統合 |
| Round 2 new-should-fix-004 | fixed (定着) | should-fix-004 統合 |
| Round 2 new-nitpick-001 | fixed (定着) | `suite_aad()` ヘルパー |
| Round 2 new-nitpick-002 | wontfix (再確認) | `aead` テスト独立 |
| Round 3 new-must-fix-001 | fixed | `seal_for` に `debug_assert_eq!(encap_key.len(), kem::encap_key_len(suite))` を追加。将来 KEM 実装を差し替えた時に encap_key 長の不整合をその場で検出する。3 つの KEM が常に固定長を返す現状では機能上の差はないが、副作用ゼロで防御層を増やせる。|
| Round 3 new-should-fix-001 | wontfix | `aead` モジュールの pub 範囲整理は API 美学の指摘。`aead` を外部から直接呼ぶ実例はゼロで、TP の機能性に影響しない。OSS maturity 観点では指摘可能だが、TP の threat model / protocol 公約とは無関係なため見送り。 |
| Round 3 new-should-fix-002 | fixed | `ml_kem768.rs:45-50` の rationale コメントを書き直し。実装と整合しない「infallible 化のため」の説明を、(a) trait dispatch 回避で API 破壊面を狭める、(b) seed bytes の経路を 1 箇所に集約、の 2 点に修正。 |
| Round 3 new-should-fix-003 | wontfix | `DecryptError` リネーム。aes-gcm の `decrypt` 失敗は実質タグ失敗以外ありえず、運用上区別しなくても困らない。enum リネームは消費側の書き換えコストが大きく、得るものとの釣り合いが悪い。 |
| Round 3 new-nitpick-001 | wontfix | X25519 公開鍵入力検査。後段 `reject_zero_shared_secret` が全 KEM 共通で安全側に倒している。早期 reject はエラー位置の美学問題で、攻撃成立は元から無い。 |
| Round 3 new-nitpick-002 | fixed | `OpenedRequest.suite: pub` フィールドを削除。Round 2 で `open_request(expected_suite)` 引数化したことで invariant が呼び出し側で既知になり、戻り値で repeat する意味が消えた。orchestrator は `opened.suite` を参照していない。 |
| Round 3 new-nitpick-003 | wontfix | `aead::encrypt` を直接呼ぶ経路は sealed_channel 経由でなく TP では使われていない。`InvalidWireFormat` のセマンティクス揺れは内部 primitive レベルの議論で、実害ゼロ。 |
| Round 3 new-nitpick-004 | wontfix | new-should-fix-001 と同根、同上の理由。 |
