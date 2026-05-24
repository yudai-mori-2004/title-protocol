# K2. `crates/crypto` 縦深掘り（Round 2）

## 概要

Round 1（`docs/v0.1.2/audit/k2-crypto.md`）で挙げた must:4 / should:6 / nitpick:6 = 16 件について、修正後の `crates/crypto/` 全ファイル（`aead.rs`, `error.rs`, `hkdf.rs`, `key_bundle.rs`, `lib.rs`, `payload.rs`, `sealed_channel.rs`, `wire.rs`, `kem/{mod,x25519,p256_ecdh,ml_kem768}.rs`）および呼び出し側 `crates/tee/src/orchestrator.rs:285-300` を、Spec §2.4 を Source of Truth として 1 行 1 行再走査した。

Round 2 ではコードの修正が「指摘の要旨を満たしているか」「修正によって新たな破綻を生んでいないか」を観点に判定する。

## Round 1 指摘の処理状況

| ID | 件名 | 状態 | 補足 |
|---|---|---|---|
| must-fix-001 | P-256 ECDH を自前スカラー乗算で実装 | **fixed** | `p256::ecdh::diffie_hellman(scalar, point)` 直呼びに置換 (`p256_ecdh.rs:84-88`) |
| must-fix-002 | AES-256-GCM が AAD を一切バインドしない | **partially-fixed** | AEAD API に `aad` 引数追加、suite_id を AAD に bind したが wire ヘッダ全体（encap_key_len + encap_key）は未バインド |
| must-fix-003 | ML-KEM-768 で `rand::random()` 経由 | **partially-fixed** | `OsRng.fill_bytes(&mut m)` に置換され RNG 統一は達成、ただし `encapsulate_deterministic` のままで `encapsulate(rng)` への切り替え提案は採用されず |
| must-fix-004 | P-256 secret scalar を NSM seed 直流し | **fixed** | `SecretKey::random(rng)` 経由に変更 (`p256_ecdh.rs:61-65`)、bias 排除 |
| should-fix-001 | per-key one-shot 前提の暗黙化 | **unchanged** | `ResponseChannel::seal(&self, ...)` のまま、型レベル one-shot 強制なし、コメントも追加されず |
| should-fix-002 | HKDF salt=encap_key 設計の根拠不明 | **unchanged** | `hkdf.rs` のコメントは未拡張、Spec §2.4 も無記載 |
| should-fix-003 | wire 境界チェックの 2 段化 | **unchanged** | `wire.rs:49-64` は Round 1 と同じ構造 |
| should-fix-004 | `metadata_len` u32 が cap なし + checked_add なし | **unchanged** | `payload.rs:46-55` は Round 1 と完全同一 |
| should-fix-005 | `OpenedRequest.suite` のチェック義務がコメントのみ | **unchanged** | コメント文言が若干書き直された (`sealed_channel.rs:21-25`) ものの `open_request` に `expected_suite` 引数は追加されず、orchestrator 側 (`orchestrator.rs:296`) で照合し続けている |
| should-fix-006 | X25519 low-order point 検査なし | **unchanged** | `x25519.rs:64-75` は Round 1 と同一、all-zero shared_secret 検査なし |
| nitpick-001 | `lib.rs` モジュール一覧の二重管理 | **unchanged** | `lib.rs:10-19` ほぼ同一 |
| nitpick-002 | `HkdfError(String)` 非対称 | **unchanged** | `error.rs:11-12` String のまま |
| nitpick-003 | `Nonce` type alias の位置 | **unchanged** | `aead.rs:10` のまま |
| nitpick-004 | nonce 長エラーが `InvalidWireFormat` | **unchanged** | `aead.rs:28-34, 56-62` のまま |
| nitpick-005 | `key_bundle.rs:33` の spec 参照過剰 | **unchanged** | `key_bundle.rs:33-35` のまま |
| nitpick-006 | テストの「暗黙にテストされてる」コメント | **fixed** | コメントを実 assertion に置換 (`sealed_channel.rs:192-193` の `assert!(opened.response_channel.open(&wire[..]).is_err())`) |

集計: **fixed 3 / partially-fixed 2 / unchanged 11**

---

## 新規発見

Round 2 で改めて読んで気付いた、Round 1 で拾えなかった / 修正で生まれた問題。

### new-must-fix-001  AAD = `[suite_id]` のみで、wire ヘッダ全体は依然として未認証（must-fix-002 の修正が不完全）

- 場所:
  - `crates/crypto/src/sealed_channel.rs:43, 52, 72, 97`
  - 参照: Spec §2.4 lines 563-584（ワイヤーフォーマット定義）
- 観察:
  - 修正後の AEAD コール: `aead::encrypt(&request_key, &nonce, plaintext, &[suite.suite_id()])`
  - AAD は `[u8; 1]` — suite_id だけ。
- 問題:
  - Round 1 の must-fix-002 修正案は「`suite_id` + `encap_key_len.to_be_bytes()` + `encap_key`」を AAD として bind することだった。現状は `suite_id` のみで、wire ヘッダの **encap_key 自体**が GCM タグで保護されない。
  - 攻撃面:
    - `encap_key` を別の有効鍵に flipping した場合、TEE は誤った shared_secret を計算 → HKDF → request_key が変わる → GCM タグが弾く、という流れで結局検知される。よって**実害は防げる**が、HKDF の入力が `salt=encap_key` で encap_key にも依存しているため、現状でも事実上 encap_key も鍵スケジュールに含まれている形にはなっている。
    - 一方で `encap_key_len`（2B）の値を「同じ長さの別整数」に書き換える攻撃は、`wire.rs:51` の `if ek_len != expected_ek_len` で弾かれる。これは AAD ではなく構文チェックで止まる。
  - 設計の意図は読み取れる（rotate しない HKDF salt を encap_key に置くことで疑似的に AAD として作用させている）が、コードと spec のどこにもその意図が書かれていない。Round 1 で指摘した「rationale がない」問題と同根。
- 修正案:
  - **追記 + 書き直し**: `aead::encrypt/decrypt` を「per-suite に対し透過的」のままにしたいなら、せめて `sealed_channel.rs` 内で `[suite_id, ek_len_be[0], ek_len_be[1]]` (3 バイト) を AAD として bind し、HKDF の salt に encap_key を入れている意図を `hkdf.rs:23-26` の doc comment に書く。
  - もしくは Round 1 案通り wire ヘッダの prefix（suite_id + encap_key_len + encap_key）を構築する helper を `wire.rs` に追加し、`sealed_channel.rs` から流用する:
    ```rust
    // wire.rs
    pub fn request_header(suite: EncryptionSuite, encap_key: &[u8]) -> Vec<u8> {
        let mut h = Vec::with_capacity(3 + encap_key.len());
        h.push(suite.suite_id());
        h.extend_from_slice(&(encap_key.len() as u16).to_be_bytes());
        h.extend_from_slice(encap_key);
        h
    }
    ```
    これを AAD に渡す。

### new-should-fix-001  `aead.rs` の `decrypt` で `aad` 引数の length 検査が無く、空 AAD と「忘れた AAD」が同値

- 場所: `crates/crypto/src/aead.rs:49-71`
- 観察:
  - `decrypt(key, nonce, ciphertext, aad: &[u8])` は `aad: b""` でも `aad: b"request"` でも、暗号化時に同じ AAD を渡していれば成功する。
- 問題:
  - これは GCM API として正しい挙動だが、本クレートのユースケースでは「AAD なし呼び出し」を**禁止したい**（Round 1 must-fix-002 の本質）。`encrypt` / `decrypt` をライブラリレベルで `aad: NonEmptyBytes` のような新型でラップし、空 AAD を型システムで禁止すべき。
  - 現状 `aead.rs:95` のテスト `encrypt(..., b"")` が通っているので、誤って空 AAD 呼び出しに先祖返りした際にテストが落ちない。
- 修正案:
  - **追加**: `aead.rs` に `if aad.is_empty() { return Err(CryptoError::InvalidWireFormat("AAD must be non-empty".into())); }` を挟むか、新型 `Aad<'a>(&'a [u8])` を導入して `Aad::new(slice).ok_or(...)?` で空を弾く。本クレートの呼び出し元は `sealed_channel` 2 箇所だけなので影響範囲は小さい。
  - もしくは `sealed_channel.rs` 側で常に suite_id を含めることが明文化されているなら、`aead` のテスト `wrong_aad_fails` を「`b"request"` vs `b""` でも失敗する」ケースに拡張する。

### new-should-fix-002  `kem/ml_kem768.rs:46` の `encapsulate_deterministic` 選択理由が不在（must-fix-003 の修正後も残存）

- 場所: `crates/crypto/src/kem/ml_kem768.rs:43-50`
- 観察:
  ```rust
  fn encapsulate(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
      let mut m = ml_kem::B32::default();
      rand::rngs::OsRng.fill_bytes(&mut m);
      let (ct, ss) = self.ek.encapsulate_deterministic(&m);
      Ok((ss.to_vec(), ct.to_vec()))
  }
  ```
- 問題:
  - Round 1 で「`encapsulate(rng)` を使う方が自然」と提案したが、現状は `OsRng` から 32 バイト読んで `encapsulate_deterministic` に渡す形に「中途半端な」修正。これは
    - (a) シード品質は OsRng で同じ
    - (b) 失敗パスが消えた（`encapsulate(rng)` は `Result` を返すが `_deterministic` は infallible）
    という理由がありうるが、コードにも spec にも書かれていない。
  - また、`encapsulate_deterministic` を使うと **future ml-kem crate 0.4 系で API が再編された際にサイレント破壊**が起きる。0.4 系では `encapsulate_deterministic` が `Encapsulate` trait の標準 method に統合される可能性があり、現状の `KemCore` トレイト直接呼び出しはアップデートで壁にぶつかる。
- 修正案:
  - **書き直し or 追記**: 関数頭に 2 行コメントで「OsRng 経由で 32 バイト読み、決定的 encap を呼ぶ。`encapsulate(rng)` を使わない理由は infallible 化したいため」と明記。
  - もしくは Round 1 提案通り `Encapsulate::encapsulate(&self.ek, &mut OsRng)` を直接呼ぶ。両者 32 バイトを内部で消費するので同等。

### new-should-fix-003  `OpenedRequest.suite` チェックの説明文が「callers should check」のまま（should-fix-005 の修正失敗）

- 場所: `crates/crypto/src/sealed_channel.rs:20-28`
- 観察: コメントが Round 1 から文言だけ若干推敲されたが、API は変更なし。`orchestrator.rs:296` は依然として手書きで `if opened.suite != suite` を書いている。
- 問題:
  - Round 1 で「コメントで安全性を保証するのは弱い」と書いたが、その指摘自体が **コメントを若干書き直すだけで対応された** と読めるレベルにとどまっている。`should-fix-005` の本質（API 強制）は未対応。
- 修正案:
  - Round 1 修正案を再掲: `open_request(key_bundle, expected_suite, wire_payload)` に変更し、`OpenedRequest` から `pub suite` を消す（または `pub(crate)` に下げる）。orchestrator 側は `let opened = open_request(&key_bundle, suite, &fetched.content_bytes)?;` の 1 行で済む。
  - これにより `orchestrator.rs:292-302` のチェックブロック 10 行が消える。

### new-should-fix-004  `payload.rs:46` の `meta_len` cap 欠如は Round 1 から退行なし、ただし **暗号化前** であることの記述がない

- 場所: `crates/crypto/src/payload.rs:39-66`
- 観察: Round 1 should-fix-004 と同じ状態。
- 補足観察: `parse_payload` は AES-GCM 復号後の plaintext を受け取る。よって `meta_len` は「TEE-validated」な値であり、攻撃者は AEAD タグを通過する必要がある → AAD/key を知っている真の送信者でしか操作できない。
  - とはいえ、クライアント実装のバグや co-tenant TEE の悪意で `meta_len = u32::MAX` がペイロードに紛れる可能性はゼロではなく、64 KiB cap は依然として正当な防御。
- 問題:
  - Round 1 から進展なし。優先度は据え置き。
- 修正案:
  - Round 1 修正案そのまま。+ doc comment に「この関数は AES-GCM 復号後の plaintext を受け取るため、ここでの境界チェックは defense-in-depth であり、第一の保護は AEAD タグ検証である」を 1 行追記。

### new-nitpick-001  `sealed_channel.rs:43, 72` の AAD 構築 `let aad = [self.suite_id];` が重複している

- 場所:
  - `crates/crypto/src/sealed_channel.rs:43` (ResponseChannel::seal)
  - `crates/crypto/src/sealed_channel.rs:52` (ResponseChannel::open)
  - `crates/crypto/src/sealed_channel.rs:72` (seal_for)
  - `crates/crypto/src/sealed_channel.rs:97` (open_request)
- 観察: 同じ `let aad = [...];` パターンが 4 箇所、しかも `seal_for` と `open_request` は `[suite.suite_id()]` を使い、`ResponseChannel` は `[self.suite_id]` を使う、と微妙に違う。
- 問題:
  - 4 箇所のうち 1 箇所でも書き間違えると direction-specific AAD のような次の修正（new-must-fix-001）を入れた時に検知漏れする。
- 修正案:
  - **書き直し**: AAD 構築を関数化:
    ```rust
    fn request_aad(suite: EncryptionSuite) -> [u8; 1] { [suite.suite_id()] }
    fn response_aad(suite_id: u8) -> [u8; 1] { [suite_id] }
    ```
    あるいは現状なら 1 関数で十分。new-must-fix-001 を採用する際には自然に拡張できる。

### new-nitpick-002  `aead.rs:78-89` のテスト `encrypt_decrypt_roundtrip` で AAD が `b"request"` のみ

- 場所: `crates/crypto/src/aead.rs:78-107`
- 観察: 「`aad=b"request"`」と「`aad=b"correct"`/`b"wrong"`」と「`aad=b""`」の 3 種類しかテストされていない。`sealed_channel` から渡される実 AAD は `[suite_id]` (1 バイト) で、UTF-8 でないバイナリ。
- 問題:
  - GCM は AAD を opaque bytes として扱うので機能差はないが、テストデータが現実の使用法と乖離していると、回帰テストとしての価値が薄い。
- 修正案:
  - テストの AAD を `&[0x01u8]` のような実値に置換、または「`aad=&[0x01]` と `aad=&[0x02]` で復号が独立に失敗する」テストを追加。

---

## 全体所感

Round 1 で挙げた 16 件のうち、**fixed が 3 件、partially-fixed が 2 件、unchanged が 11 件**。修正適用率は名目上 31% だが、partially-fixed を「実害は減ったが指摘の意図は半分」と評価すると実質 25-30% にとどまる。

特に印象的だったのは:

1. **must-fix の優先度が反映されている**: must-fix-001 (P-256 ECDH 自前実装) と must-fix-004 (P-256 secret scalar bias) は完全に修正された。`p256::ecdh::diffie_hellman` と `SecretKey::random(rng)` への置換は綺麗で、Round 1 案そのまま採用された形。
2. **must-fix-002 の修正が表層的**: AAD 引数を増やしたものの、AAD 値が `[suite_id]` 1 バイトだけで wire ヘッダ全体を bind していない。Round 1 で要求した HPKE スタイルの「wire prefix を AAD に流す」設計は未実装。new-must-fix-001 として再指摘した。
3. **should-fix がほぼ全部据え置き**: 6 件中 0 件しか修正されていない。特に `OpenedRequest.suite` の API 強制（should-fix-005）は「コメント文言を若干書き直しただけ」で済まされており、本質である「API で強制する」が未対応。
4. **nitpick の取捨選択も恣意的**: 6 件中 1 件のみ修正（nitpick-006: 暗黙テストコメントの削除）。残りはコメントの整理など軽い修正だが、見送られている。

新規発見 7 件（must:1, should:4, nitpick:2）はすべて Round 1 で抽象的にしか触れなかった点を具体化したもので、独立した重大インシデントではない。一方で **new-must-fix-001 と new-should-fix-003 は Round 1 must-fix-002 / should-fix-005 の事実上の再指摘**であり、Round 2 で改めて優先度を上げて取り組むべき。

### Round 2 集計

- must-fix: 1 件新規（Round 1 から繰り越し 2 件 = partially-fixed）
- should-fix: 4 件新規（Round 1 から繰り越し 6 件 = unchanged）
- nitpick: 2 件新規（Round 1 から繰り越し 5 件 = unchanged）

合計フォローアップ事項: **18 件**（Round 1 unchanged 11 + Round 1 partially-fixed 2 + 新規 7 のうち、partially-fixed 2 件は新規発見と内容重複）。実質追跡対象は **16 件**。

### 退行（regression）

修正によってコードが**悪化した箇所はない**。AAD 追加で `aead::encrypt/decrypt` のシグネチャが変わったが、呼び出し側 `sealed_channel.rs` は同時に更新されており、コンパイル / テスト共に整合している（テストは未実行だが、コード読みではマッチ）。

`p256_ecdh.rs` の書き直しも `to_nonzero_scalar()` を直接渡す形になっており、Round 1 で指摘した「ProjectivePoint への変換とスカラー乗算を自分で書いている」問題は完全に解消。

ML-KEM-768 については `from_seed` が 64 バイトシードを要求するため `generate(rng)` が 32 → 64 バイト読みに変わった（`ml_kem768.rs:60-66`）が、これは ml-kem crate の API 要件であり退行ではない。

### Round 3 を見据えた優先度（再勧告）

1. **new-must-fix-001 (= Round 1 must-fix-002 の本体)**: wire ヘッダ全体を AAD に bind するか、Spec §2.4 に「encap_key を HKDF salt に置くことで wire 構文を間接的に key に bind する設計」と一文足す。どちらか必須。
2. **new-should-fix-003 (= Round 1 should-fix-005)**: `open_request` に `expected_suite` 引数を追加。orchestrator のチェック重複を解消。
3. **Round 1 should-fix-004**: `payload.rs` の `meta_len` に 64 KiB cap と `checked_add`。defense-in-depth として依然有効。
4. **Round 1 should-fix-006**: X25519 low-order point の all-zero shared_secret 検査。Nitro 実機での攻撃面は限定的だが、ライブラリとして公開するなら入れるべき。
5. nitpick 群は時間の空いた時にまとめて処理。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| must-fix-001 | fixed | Round 2 認定済み。 |
| must-fix-002 | fixed | `sealed_channel::suite_aad()` で `[suite_id, encap_key_len_be[0], encap_key_len_be[1]]` (3 バイト) を AAD として bind するように拡張。encap_key 本体は引き続き `hkdf.rs` の salt 経由で鍵スケジュールに含める設計を `hkdf.rs` に文書化。 |
| must-fix-003 | partially-fixed(encapsulate_deterministic 経由は infallible 化が目的の意図的選択。コメントで明文化済み。`Encapsulate::encapsulate(rng)` への切替は failure path を増やすため見送り) | `ml_kem768.rs` に rationale を 4 行コメントで追加。 |
| must-fix-004 | fixed | Round 2 認定済み。 |
| should-fix-001 | wontfix(型レベル one-shot 強制は type-state パターンが必要で API 破壊が広範。実害ゼロ) | |
| should-fix-002 | fixed | `hkdf.rs` のモジュール docstring に「salt=encap_key を選んだ理由」の節を追加。AAD が小サイズで済む設計上の必然性を明記。 |
| should-fix-003 | wontfix(`wire.rs` の 2 段化は構文チェックと意味チェックを分離するリファクタで、現状の 1 段で十分。退行リスクのほうが高い) | |
| should-fix-004 | fixed | `payload.rs` に `MAX_METADATA_LEN = 64 KiB` 上限と `checked_add` を追加。回帰テスト `metadata_len_above_cap_rejected` を追加。 |
| should-fix-005 | fixed | `open_request(key_bundle, expected_suite, wire)` の 3 引数 API に変更。`orchestrator.rs` の手書き突合 10 行を削除し、`CryptoError::EncryptionSuiteMismatch` に集約。回帰テスト `declared_suite_mismatch_rejected` を追加。 |
| should-fix-006 | fixed | `x25519.rs` に `reject_zero_shared_secret()` を追加。encapsulate/decapsulate 双方で all-zero shared_secret (low-order point 攻撃) を InvalidWireFormat で reject。テスト `low_order_point_rejected` を追加。 |
| nitpick-001 | wontfix(`lib.rs` モジュール一覧の重複は Rust の慣例。`pub mod` + `pub use` で内部 vs 公開 API を分けているため冗長ではない) | |
| nitpick-002 | wontfix(`HkdfError(String)` を構造化エラーに変更する価値が薄い。`hkdf` クレートの内部エラーは透過させるのが妥当) | |
| nitpick-003 | wontfix(`Nonce` type alias の位置はモジュール内 use と並ぶのが自然) | |
| nitpick-004 | wontfix(nonce 長エラーが `InvalidWireFormat` 配下なのは「wire 由来の不正値」という意味で整合) | |
| nitpick-005 | wontfix(`key_bundle.rs` の spec 参照は §1.4/§2.4 の境界を明示するために必要) | |
| nitpick-006 | fixed | Round 2 認定済み。 |
| new-must-fix-001 | fixed | must-fix-002 と統合対応。AAD に encap_key_len を含めた上で、encap_key 本体を HKDF salt で間接 bind する設計を `hkdf.rs` の docstring に明記。 |
| new-should-fix-001 | wontfix(`aead.rs` は GCM の薄いラッパで空 AAD は GCM 規格上正当。実呼び出し元 `sealed_channel` は常に 3 バイト AAD を渡すため経路上空にならない) | |
| new-should-fix-002 | fixed | must-fix-003 と統合対応。`ml_kem768.rs:encapsulate` に 4 行 rationale を追加。 |
| new-should-fix-003 | fixed | should-fix-005 と統合対応。 |
| new-should-fix-004 | fixed | should-fix-004 と統合対応。`MAX_METADATA_LEN` 上限 + AAD 検証後である defense-in-depth の位置付けを doc コメントに追加。 |
| new-nitpick-001 | fixed | `sealed_channel::suite_aad()` ヘルパーで AAD 構築を 1 箇所に集約。4 箇所の重複を解消。 |
| new-nitpick-002 | wontfix(`aead.rs` のテストは GCM API の roundtrip 性質を確認するもので、現実の `[suite_id]` 値とは独立に意味がある) | |
