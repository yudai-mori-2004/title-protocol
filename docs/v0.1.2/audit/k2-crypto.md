# K2. `crates/crypto` 縦深掘り（暗号レビュー）

## 概要

担当範囲: `crates/crypto/` 配下の全モジュール（`aead`, `hkdf`, `kem/{x25519,p256_ecdh,ml_kem768}`, `key_bundle`, `payload`, `sealed_channel`, `wire`, `error`, `lib`）+ Cargo.toml。Spec §2.4 を Source of Truth として、

- KEM 実装の安全性（鍵入力検証・低位元・ML-KEM-768 用法）
- HKDF の domain separation（salt = encap_key, info の方向別分離）
- AES-256-GCM の nonce 衝突確率と AAD 未使用の妥当性
- wire / payload の境界チェック（length 偽造・トランケート・巨大 length）
- 鍵生成 RNG が TEE 側 NSM seed を受ける設計か（`OsRng` 直接使用の有無）
- 一定時間比較が必要な箇所
- ライブラリ選択（`ml-kem 0.3.2` の FIPS 203 準拠性）

レビューはコードを 1 文 1 文読んで `file:line` で発見を記録した。

## 重大度別内訳

- must-fix: 4 件
- should-fix: 6 件
- nitpick: 6 件

合計 16 件。

---

## 発見

### must-fix-001  P-256 共有秘密の表現が ECDH 標準と非互換（`raw_secret_bytes` vs `x_coordinate`）

- 場所:
  - `crates/crypto/src/kem/p256_ecdh.rs:42-47`（クライアント / Encapsulator 側）
  - `crates/crypto/src/kem/p256_ecdh.rs:83-87`（TEE / Decapsulator 側）
- 観察:
  - クライアント側は `eph.diffie_hellman(...).raw_secret_bytes()` を共有秘密として返す。`p256` crate の `SharedSecret::raw_secret_bytes` は「x 座標を big-endian 32 バイトで詰めた GenericArray」を返す。
  - TEE 側は `(ProjectivePoint::from(*eph_pk.as_affine()) * *scalar).to_affine().x().to_vec()` で「affine X を `to_vec()`」したものを返す。
  - つまり同じ x 座標を生成しているように見えるが、片方は `EphemeralSecret::diffie_hellman` 経由（標準 SEC1）、もう片方はスカラー乗算を手書きで再実装している。`to_affine()` の戻り型 `AffineCoordinates::x()` は `FieldBytes`（32B BE）だが、`x().to_vec()` は GenericArray の値ベースの to_vec で、`raw_secret_bytes()` が返す `FieldBytes` と等価という保証は**実装詳細に依存**している。
- 問題:
  - そもそも TEE 側が `p256::ecdh::diffie_hellman(scalar, point)`（公式 API）を呼ばず、ProjectivePoint への変換とスカラー乗算を自分で実装している。これは「自前で楕円曲線演算を書いた」ことに相当し、cofactor / point-at-infinity / point validation を `from_sec1_bytes` の検証だけに依存している。`from_sec1_bytes` は SEC1 デコードと曲線上判定はするが、small-subgroup check の責務は呼び出し側のコンテキストに依存する（P-256 は cofactor=1 なので実害は薄いが、設計として脆い）。
  - `raw_secret_bytes` と `x().to_vec()` のバイト表現が将来 `p256` crate のマイナーアップデートで微差（例: leading zero handling、内部 GenericArray 型変更）を起こすと、相互運用がサイレントに壊れる。テストでは roundtrip が同一プロセスのため検知不能。
- 修正案:
  - **削除して書き直し**: TEE 側も `p256::ecdh::diffie_hellman` を使う。
    ```rust
    use p256::ecdh::diffie_hellman;
    fn decapsulate(&self, encap_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let eph_pk = PublicKey::from_sec1_bytes(encap_key)
            .map_err(|_| CryptoError::EcdhError)?;
        let shared = diffie_hellman(self.secret.to_nonzero_scalar(), eph_pk.as_affine());
        Ok(shared.raw_secret_bytes().to_vec())
    }
    ```
  - 加えて、Encapsulator / Decapsulator の両方が `raw_secret_bytes()` を返すクロスベクトルテスト（既知の鍵ペア × encap_key → 既知の shared_secret）を `tests/` 配下に追加し、サイレント破壊を検知する。

---

### must-fix-002  AES-256-GCM が AAD を一切バインドしていない（wire ヘッダ・suite_id・方向が認証外）

- 場所:
  - `crates/crypto/src/aead.rs:32-39, 57-64`（`encrypt`/`decrypt` シグネチャに AAD なし）
  - `crates/crypto/src/sealed_channel.rs:69, 90`（`aead::encrypt(&request_key, &nonce, plaintext)`）
  - `crates/crypto/src/sealed_channel.rs:42, 50`（`ResponseChannel::seal`/`open` も AAD なし）
- 観察: AES-256-GCM の AAD 引数が一切なく、`suite_id`、`encap_key_len`、`encap_key` といった wire ヘッダ前置部分はタグで保護されていない。
- 問題:
  - 攻撃者が wire の `suite_id` を別の値に書き換えても、`encap_key_len` の整合性チェック（`wire.rs:51`）に通ってしまえば、TEE は誤った suite で復号を試みる。実害は decapsulate 失敗による DoS 程度（GCM タグが弾く）だが、wire 構文が tag に bind されていないこと自体は HPKE-style の設計から見て見劣りする。
  - 方向（request/response）の混同は HKDF で別鍵にしているため成立しないが、wire ヘッダのいずれかフィールド（例: encap_key のビット）を flipping したケースで GCM が弾くまでに `from_public_key` / `decapsulate` の重い処理が走る。
- 修正案:
  - **書き直し**: AEAD 関数のシグネチャに `aad: &[u8]` を追加し、`sealed_channel` から「`suite_id` + `encap_key_len.to_be_bytes()` + `encap_key`」を AAD として渡す。Spec §2.4 にも「wire ヘッダ全体が GCM の AAD として認証される」と一文追記。
    ```rust
    pub fn encrypt(key: &[u8], nonce: &[u8], aad: &[u8], plaintext: &[u8]) -> ...
        cipher.encrypt(nonce, aes_gcm::aead::Payload { msg: plaintext, aad })
    ```
  - response 方向も同様に `[suite_id]` を AAD として bind すれば、response の途中の suite 取り違えを検知できる（実害は薄いが defense-in-depth として整合する）。

---

### must-fix-003  ML-KEM-768 のクライアント側 encap シードに `rand::random()` を直接使用

- 場所: `crates/crypto/src/kem/ml_kem768.rs:44`
- 観察:
  ```rust
  let m: [u8; 32] = rand::random();
  let mut m_arr = ml_kem::B32::default();
  m_arr.copy_from_slice(&m);
  let (ct, ss) = self.ek.encapsulate_deterministic(&m_arr);
  ```
- 問題:
  - `rand::random()` は内部的に `thread_rng` を呼ぶ。`encapsulate_deterministic` を選んでいるなら、シード源の品質はそのままセキュリティに直結する。クライアント側であってもライブラリの一貫性として `OsRng` を明示的に使うべき（thread_rng は reseed pool に依存しており、`OsRng` よりも一段間接的）。
  - 他の KEM（x25519/p256）は `rand::rngs::OsRng` を明示している（`x25519.rs:38`, `p256_ecdh.rs:40`）。ML-KEM だけ `rand::random()` を経由するのは一貫性を欠く。
  - また、ml-kem 0.3.2 には非 deterministic な `encapsulate(rng)` API があるので、そちらを使う方が自然。
- 修正案:
  - **書き直し**: `encapsulate` を使い、`OsRng` を直接渡す。
    ```rust
    fn encapsulate(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        use ml_kem::kem::Encapsulate;
        let (ct, ss) = self.ek
            .encapsulate(&mut rand::rngs::OsRng)
            .map_err(|_| CryptoError::EcdhError)?;
        Ok((ss.to_vec(), ct.to_vec()))
    }
    ```
  - 加えて crate 全体のクライアント側 RNG を `rand::rngs::OsRng` に統一し、`rand::random()` / `rand::thread_rng()` を grep で禁止するクリッピーまたは CI チェックを足す。

---

### must-fix-004  P-256 シードを「秘密スカラーの生バイト」として直接取り込んでいる

- 場所: `crates/crypto/src/kem/p256_ecdh.rs:56-64`、呼び出し側 `crates/crypto/src/key_bundle.rs:41-43`
- 観察:
  ```rust
  let mut p256_seed = [0u8; 32];
  rng.fill_bytes(&mut p256_seed);
  let p256 = P256Decapsulator::from_seed(&p256_seed)?;
  // 内部
  let sk = SecretKey::from_bytes(seed.into()).map_err(|_| ...)
  ```
- 問題:
  - `p256::SecretKey::from_bytes` は「32 バイトのスカラー」をそのまま受ける。バイト列が `[0]` または ≥ n（曲線位数）の場合は `Err` を返すが、TEE 起動時に NSM から取った 32 バイトをそのまま secret scalar に突っ込む設計は、ML-KEM のように「seed → KDF → key」ではなく **scalar の bias** を引き起こす（n に近い値のスカラーは modulo の偏りを生む）。実害は無視できるほど小さいが、FIPS / NIST の鍵生成基準（SP 800-133, 800-56A）に厳密準拠するなら "rejection sampling" または "extra-bits + modulo" を行うべき。
  - 同じ問題が X25519（`StaticSecret::from(arr)`）にもあるが、X25519 の場合は `clamp_scalar` で curve25519 の clamping が自動で行われるため OK。
  - 現状の `SecretKey::from_bytes(...).map_err(...)` のエラーパスは `InvalidKeyLength` を返すが、実際の失敗理由は「scalar が 0 か ≥ n」である可能性もあり、エラー型が誤誘導する。
- 修正案:
  - **書き直し**: P-256 の鍵生成は SP 800-133 rev2 §6.2.1（extra bits）に従い、48 バイト引いて mod (n-1) + 1 する、もしくは crate の `SecretKey::random(rng)` を介して NSM seed で作った ChaCha20Rng から生成する（`tee_seeded_rng` が既にそうしているなら、`p256::SecretKey::random(&mut rng)` を直接呼べばよい）。
    ```rust
    pub fn generate(rng: &mut (impl rand::RngCore + rand::CryptoRng))
        -> Result<Self, CryptoError>
    {
        Ok(Self { secret: SecretKey::random(rng) })
    }
    ```
  - エラーは `EcdhError` または新規 `KeyGenError` に正規化する。`InvalidKeyLength { expected: 32, actual: seed.len() }` は誤情報（32 バイト来ているのに「scalar が 0/≥n だった」可能性が混ざる）。

---

### should-fix-001  AES-GCM nonce がランダム生成（衝突確率 2^-32 を 2^32 メッセージで超える）

- 場所: `crates/crypto/src/sealed_channel.rs:40-41, 67-68`
- 観察:
  ```rust
  let mut nonce = [0u8; NONCE_SIZE];
  rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut nonce);
  ```
- 問題:
  - AES-GCM の安全境界は同一鍵での nonce 衝突である。12 バイトランダム nonce の場合、生日境界は約 2^32 messages/key で衝突確率 ~10^-9 になる（NIST SP 800-38D §8.3）。
  - 本プロトコルでは request_key / response_key は 1 リクエストにつき 1 回しか使われない（鍵が KEM ごとに新規派生）ので**現実的には衝突しない**が、設計上は「per-key で乱数 nonce を 1 回しか使わない」ことが暗黙の前提になっており、コードからは読み取れない。
  - 将来「同じ KEM 交換を pipeline で再利用」のような拡張を入れたとき、ランダム nonce では 2^32 で破綻する。
- 修正案:
  - **書き直し**: nonce 生成を sealed_channel 関数内で固定し、コメントで「per-key one-shot」を明示。
    ```rust
    // Spec §2.4 — request_key / response_key are derived per-request via fresh
    // KEM. Each key is used exactly once, so a random 12-byte nonce is safe;
    // never reuse a key for multiple messages.
    ```
  - さらにテストに「`seal_for` を 100 万回呼んでも nonce が一意」を assert する性能テストよりも、**型レベルで one-shot を強制する** API を検討（`ResponseChannel` を `seal(self, ...) -> Vec<u8>` の consuming method にする等）。

---

### should-fix-002  HKDF salt に encap_key を使う設計が IETF 慣行（HPKE）と逆

- 場所: `crates/crypto/src/hkdf.rs:23-37`、Spec §2.4 lines 496-497
- 観察:
  ```
  request_key  = HKDF-SHA256(shared_secret, info="title-request-key",  salt=encap_key)
  ```
  HKDF API: `Hkdf::<Sha256>::new(salt=Some(encap_key), ikm=shared_secret)`
- 問題:
  - RFC 9180（HPKE）では「IKM = shared_secret, salt = 空 or context」「info = context」が標準で、`encap_key` は通常 `info` 側または extract の psk_id に入れる。本実装は「salt = encap_key」「info = direction」を採用しており、HPKE と直接互換にはならない。これは独自プロトコルなので問題ではないが、**理由がコメントにない**。
  - HKDF-Extract の salt は「全エントロピー」を強くするための値で、`encap_key`（攻撃者が観察可能）を salt に置くのは security 的には equivalence class が変わらない（HKDF は extract-then-expand なので salt を public にしても安全）。だが、暗号レビュー時に「HPKE と違う」ことが目立つため、選択理由を残すべき。
- 修正案:
  - **追記**: `hkdf.rs` の doc comment に「HPKE （RFC 9180）の `KEM.encap_key` を salt に持ち込み HKDF-Extract のエントロピーを bind することで、shared_secret が ad-hoc に再利用されても context が分離される」旨を 2-3 行で明記。
  - 将来 HPKE 準拠への移行可能性を残したいなら、`hkdf.rs` を `legacy_kdf.rs` にリネームし、HPKE Adapter を別実装で並走させる選択肢を Spec §2.4 のあとに「将来拡張」として注記。

---

### should-fix-003  wire の `encap_key_len` を 2 バイト BE で読むが、ML-KEM-768 のサイズ整合性を `encap_key_len(suite)` でしか確認していない

- 場所: `crates/crypto/src/wire.rs:49-55`
- 観察:
  ```rust
  let ek_len = u16::from_be_bytes([payload[1], payload[2]]) as usize;
  let expected_ek_len = encap_key_len(suite);
  if ek_len != expected_ek_len {
      return Err(...)
  }
  ```
- 問題:
  - `u16` の最大値は 65535、suite ごとの実値は 32 / 65 / 1088。攻撃者は `ek_len = 1088` を申告して TEE に「お、1088 だな」と通させた後、ペイロードを 1088 + 12 バイトより短く trim できる。これは line 60-64 の `if payload.len() < nonce_end { ... }` でガードされているのでパスしないが、「`ek_len` の整合性検証」と「`payload.len()` の境界検証」が**別々のステップ**に分離されており、リファクタで片方を消しがち。
  - また、`encap_key_len(suite)` は固定値なので、wire に `encap_key_len` を入れる必要がそもそも薄い（suite から導出可能）。冗長フィールドは攻撃面を増やすだけ。
- 修正案:
  - **削除 or 簡略化**: `encap_key_len` フィールドを wire から落とすことを次回 spec 改訂で検討（互換性 break）。今すぐの修正としては、`if ek_len != expected_ek_len` を `if u16::from_be_bytes(...) as usize != expected_ek_len` に「raw value 直比較」に書き換え、`usize` キャストを境界チェック後にずらす。
  - parse_request の bound check 群を一つにまとめ、`let required = 3 + expected_ek_len + NONCE_SIZE; if payload.len() < required { return Err(...) }` のように一度の guard で済ませて読みやすくする。

---

### should-fix-004  `payload.rs` の `metadata_len` u32 がトラフィック上限なしで読み放題

- 場所: `crates/crypto/src/payload.rs:46-55`
- 観察:
  ```rust
  let meta_len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
  let meta_end = 4 + meta_len;
  if data.len() < meta_end { return Err(...) }
  ```
- 問題:
  - `meta_len = u32::MAX` を申告して `data.len() < meta_end` を満たさないので Err を返すのは正しいが、**事前に "meta_len ≤ data.len() - 4"** をチェックしているだけで、攻撃者は AES-GCM の中で復号後のペイロードに任意の meta_len を埋め込める。
  - 加えて、`meta_end = 4 + meta_len` は **usize オーバーフロー**の余地がある（32-bit ターゲットの場合）。`u32::MAX + 4` が `usize::MAX` を超えると wrap して 3 になる。TEE は 64-bit Linux なので実害ゼロだが、「TEE は 64-bit である」前提がコードに書かれていない。
  - また、メタデータ JSON が巨大（数 MB）でも parse_payload は通してしまう。Spec §2.4 のメタデータは `{"signature_hash":"sha256:..."}` だけのはずなので、現実的な上限（例: 64 KiB）を hard-cap すべき。
- 修正案:
  - **書き直し**: 明示的上限と checked add で防御:
    ```rust
    const MAX_METADATA_LEN: usize = 64 * 1024; // 64 KiB
    let meta_len = u32::from_be_bytes(data[..4].try_into().unwrap()) as usize;
    if meta_len > MAX_METADATA_LEN {
        return Err(CryptoError::InvalidPayload(
            format!("metadata_len {meta_len} exceeds cap {MAX_METADATA_LEN}")));
    }
    let meta_end = 4_usize.checked_add(meta_len)
        .ok_or_else(|| CryptoError::InvalidPayload("metadata_len overflow".into()))?;
    if data.len() < meta_end {
        return Err(CryptoError::InvalidPayload(format!(
            "payload too short: need {meta_end} bytes for metadata, have {}", data.len())));
    }
    ```
  - Spec §2.4 にも「metadata は 64 KiB を上限とする」と一文足す。

---

### should-fix-005  `OpenedRequest.suite` のチェック義務がコメントで書かれているだけ（型強制なし）

- 場所:
  - `crates/crypto/src/sealed_channel.rs:20-28`
  - 呼び出し側: `crates/tee/src/orchestrator.rs:269-282` で実際にチェックされている
- 観察: コメントが「Callers should check this against any suite declared out-of-band」と促すだけで、API として強制していない。
- 問題:
  - 監査者として「コメントで安全性を保証する」は弱い。orchestrator が将来リファクタされて check を消した場合、wire の suite_id を任意に書き換えて TEE に別 suite で処理させる攻撃（実害は GCM が弾くので限定的だが）が通る。
  - また、`open_request` は wire から **任意の** suite を取り出すので、呼び出し側は事実上「申告 suite」と「wire suite」を毎回突き合わせなければならない。これを API で吸収すべき。
- 修正案:
  - **書き直し**: `open_request` に `expected_suite: EncryptionSuite` 引数を追加し、ライブラリ内部で `parsed.suite != expected_suite` を弾く:
    ```rust
    pub fn open_request(
        key_bundle: &KeyBundle,
        expected_suite: EncryptionSuite,
        wire_payload: &[u8],
    ) -> Result<OpenedRequest, CryptoError> {
        let parsed = wire::parse_request(wire_payload)?;
        if parsed.suite != expected_suite {
            return Err(CryptoError::InvalidWireFormat(format!(
                "wire suite {:?} does not match declared {:?}", parsed.suite, expected_suite)));
        }
        // ...
    }
    ```
  - これにより orchestrator のチェックロジック（line 276-282）が消えて重複が解消する。

---

### should-fix-006  X25519 / P-256 の公開鍵が all-zero（または low-order）でも `from_public_key` が通る

- 場所:
  - `crates/crypto/src/kem/x25519.rs:22-33`
  - `crates/crypto/src/kem/p256_ecdh.rs:25-35`
- 観察:
  - X25519: `PublicKey::from(arr)` は単なる 32 バイトラップで、low-order point（small subgroup attack 用の 8 点）の検査をしない。
  - P-256: `PublicKey::from_sec1_bytes` は SEC1 デコードと「曲線上判定」までは行うが、`from_public_key` は「コフェクター乗算が non-identity」を確認しない（P-256 のコフェクターは 1 なので実害ゼロ）。
- 問題:
  - X25519 で recipient_pubkey に low-order point を設定された TEE は「常に 0 の shared_secret」を生成する。**これは TEE の公開鍵を攻撃者が改変できる状況**でしか発生せず、本プロトコルでは TEE 自身が起動時に生成して Gateway 経由で配布するため、攻撃面は限定的。
  - ただし、クライアント側で `from_public_key(attacker_supplied_bytes)` のテストパスを通る場合（CLI tool 等）、low-order 攻撃が成立する余地が残る。
- 修正案:
  - **追加**: X25519 では `x25519-dalek` 推奨の検査を追加:
    ```rust
    let shared = eph_secret.diffie_hellman(&self.recipient_pubkey);
    if shared.as_bytes().iter().all(|&b| b == 0) {
        return Err(CryptoError::EcdhError);
    }
    ```
    （contributory behavior; RFC 7748 §6.1 推奨）。
  - もしくは shared が all-zero になったケースを `EcdhError` として伝播。本プロトコルでは TEE が自分で鍵生成するので practical risk は低いが、defense in depth として入れる価値はある。

---

### nitpick-001  `lib.rs` の doc comment が「ない」モジュールを列挙している（4.7 癖）

- 場所: `crates/crypto/src/lib.rs:10-19`
- 観察:
  ```
  //! ## Modules
  //! - `error` — Error types
  //! - `kem` — Key Encapsulation Mechanism (X25519, P-256, ML-KEM-768)
  //! ...
  ```
- 問題: モジュール一覧は `pub mod` の宣言と二重管理。rustdoc が同じものを自動生成する。Spec §2.4 の構造をクレートのトップで再掲する意義はあるが、各 1 行説明はモジュールの doc comment 側に置く方が DRY。
- 修正案: **削除 or 簡略化**。トップ doc comment は「Spec §2.4 の暗号プリミティブ群。詳細は各モジュール参照」程度に切り詰める。

---

### nitpick-002  `error.rs` の `HkdfError(String)` だけ String を持つ非対称

- 場所: `crates/crypto/src/error.rs:14-15`
- 観察: 他は `EncryptError` / `DecryptError` のように単純列挙、`InvalidKeyLength` は構造化、`HkdfError(String)` だけが理由の String を持つ。
- 問題: HKDF の失敗は実質「Expand の OKM 長 > 255 × HashLen」だけで、String を持つ必然性がない。
- 修正案: `HkdfError` を unit variant にし、`hkdf.rs:31, 35` の `map_err` を `map_err(|_| CryptoError::HkdfError)` に書き換える。

---

### nitpick-003  `aead.rs:10` の Nonce 型エイリアスがファイル先頭にあり読みにくい

- 場所: `crates/crypto/src/aead.rs:10`
- 観察:
  ```rust
  type Nonce = aes_gcm::Nonce<aes_gcm::aead::consts::U12>;
  ```
- 問題: `use` セクションの直前に type alias が割り込んでおり、import の流れを切る。
- 修正案: `use aes_gcm::aead::consts::U12;` を `use` グループに含め、`type Nonce = aes_gcm::Nonce<U12>;` を最後に置くか、関数内で `let nonce: &aes_gcm::Nonce<aes_gcm::aead::consts::U12> = ...` に inline する。本質ではないので nitpick。

---

### nitpick-004  `aead.rs` の `InvalidWireFormat` を nonce 長エラーに流用している

- 場所: `crates/crypto/src/aead.rs:26-30, 50-55`
- 観察: nonce 長不一致のとき `CryptoError::InvalidWireFormat("nonce must be 12 bytes, got N")` を返している。
- 問題: nonce 長は AEAD パラメータの問題で、wire format の問題ではない。エラー分類が誤誘導する。`InvalidKeyLength` を `InvalidLength { name, expected, actual }` に汎化して使う方が自然。
- 修正案: `CryptoError::InvalidKeyLength` を `InvalidLength { what: &'static str, expected: usize, actual: usize }` にリファクタするか、専用の `InvalidNonceLength` variant を追加。

---

### nitpick-005  `key_bundle.rs:36` の `Spec §2.4 — TEE startup key generation` が情報量ゼロ

- 場所: `crates/crypto/src/key_bundle.rs:31-36`
- 観察: 関数 doc に `/// Spec §2.4 — TEE startup key generation.` とあるが、すでに type の doc にも `Spec §2.4` が貼られている。
- 問題: 4.7 癖（spec 参照の過剰貼り）の例。本ファイルだけで `Spec §2.4` が 4 箇所ある。
- 修正案: type 側の `Spec §2.4` だけ残し、各メソッドの spec 参照は削除。コメント自体は「The RNG should be backed by TeeRuntime::random_bytes in production.」だけ残せばよい。

---

### nitpick-006  `sealed_channel.rs:182-186` のテストが「テストしていないことを説明するコメント」になっている

- 場所: `crates/crypto/src/sealed_channel.rs:182-186`
- 観察:
  ```rust
  // TEE's response channel cannot open the request (different keys)
  // and client's channel cannot open with request key
  // (This is implicitly tested by the fact that seal/open work correctly
  // with their respective keys)
  ```
- 問題: 4.7 癖（やってないことの長文 rationale）。「方向別鍵が独立であること」をテストするなら、明示的に `client_channel.open(&request_wire).is_err()` を assert すべき。コメントだけで「暗黙にテストされてる」と主張するのは監査体験として最悪。
- 修正案: コメントを削除し、以下のような assertion を追加。
  ```rust
  // request 方向の暗号文を response_channel で開こうとしても失敗する
  let parsed = wire::parse_request(&wire).unwrap();
  let wrong_wire = wire::build_response(parsed.nonce, parsed.ciphertext);
  assert!(client_channel.open(&wrong_wire).is_err());
  ```

---

## 全体所感

`crates/crypto` は構成（KEM trait / KeyBundle / sealed_channel）として綺麗に分離されており、Spec §2.4 とのトレーサビリティも高い。一方で、暗号実装としては以下の 3 つの「意図が見えない選択」が気になった:

1. **P-256 の TEE 側 ECDH を自前のスカラー乗算で書いている**（must-fix-001）。`p256::ecdh::diffie_hellman` を素直に呼ぶべき。
2. **AAD を一切使っていない**（must-fix-002）。HPKE と意図的に違うなら理由を spec に残すべきで、漏れているならヘッダを AAD に bind する。
3. **P-256 secret scalar の bias**（must-fix-004）。NSM の 32 バイトをそのまま `SecretKey::from_bytes` に流すのは SP 800-133 から見て弱い。`SecretKey::random(rng)` を介すれば一発で解決する。

加えて、`open_request` で wire suite と申告 suite の照合を **API レベルで強制**できていない（should-fix-005）のは、orchestrator のレイヤーに防御を依存していて脆い。crypto crate 内で完結させるべき。

ML-KEM-768 については `ml-kem 0.3.2` を使っており FIPS 203 Final 準拠（RustCrypto の最新リリース）であること自体は OK だが、`encapsulate_deterministic` + `rand::random()` の組み合わせは選択理由が見えず、`encapsulate(rng)` を直接呼ぶ方が一貫性がある（must-fix-003）。

なお、`Cargo.toml:13-17` の暗号系 crate version は固定（`x25519-dalek = "2.0.1"` 等）されているが、`hkdf = "0.12"` のように minor だけ指定もあり、`Cargo.lock` への依存が暗黙の前提になっている。reproducible build を謳う以上、全エントリーを patch version まで固定する方がトレーサビリティが高い（観点 E と重複）。
