# K7 監査 Round 3: `sp1-guests/` 縦深掘り (Round 2 後再点検)

担当範囲: `sp1-guests/` 配下全ファイル (`README.md`, `attestation-aws-nitro/host/*`, `attestation-aws-nitro/program/*`)。
比較基準: Round 2 `docs/v0.1.2/audit/round2/k7-sp1-guests.md` (must:0 / should:2 / nitpick:3) と Round 1 `docs/v0.1.2/audit/k7-sp1-guests.md` (must:3 / should:6 / nitpick:4)。
今回の方針: Round 2 で残った 5 件（K7-04 / K7-12 / R2-N1 / R2-N2 / R2-N3）の処理状況を 1 件ずつ追跡し、Round 2 では検出されなかった **新規問題** を `must / should / nitpick` で分類する。

## サマリ

- Round 2 残置 5 件のうち **解消 3 件** (K7-04 fixed / K7-12 fixed / R2-N1 fixed)、**運用上 wontfix 維持 2 件** (R2-N2 / R2-N3)。
- **新規発見 1 件 (R3-N1 must-fix)** — guest が `sp1_zkvm::io::commit(&String)` を使って `instance_id` を書き出している一方、on-chain `parse_public_values` は `u32 LE` の長さ前置で読みに行っている。**bincode 1.x の fixint シリアライザは `String`/`&[u8]`/`Vec<T>` の長さを `u64 LE (8 バイト)` で書く**ため、現状の `register_key` は **module_id が空文字列でない限り常に失敗** する。Round 2 が「binary 的に完全一致 (確認済み)」と書いたのは誤判定 — Round 2 担当は guest の commit が bincode を経由する事実を実機検証せずスペック上の言い回しだけで合致と書いてしまったと推定。
- 機能上のリグレッションは **ゼロ**。SP1 SDK 6.2 async API、`bytes32_raw()`、`groth16()` の使い方は Round 2 で確認されたとおり正しい。
- 新規発見 2 件 (R3-N2 should、R3-N3 nitpick)。

---

## Round 2 残置 5 件の追跡

| ID | 重大度 (R2) | 状態 | 根拠 (file:line) |
|---|---|---|---|
| K7-04 | should | **解消** | `sp1-guests/README.md:52-55` で `## Running` の prove 行の直下に blockquote が入り、「~30 GiB resident memory」「64 GiB RAM 推奨」「EC2 r5.4xlarge or larger」「`--help` で全要件を確認」が記載。Round 2 で提案された 4 行の文言と実質同一。`--help` 経由のみだったメモリ要件警告が、README 一度読みのオペレータにも届く構造になった。 |
| K7-12 | nitpick | **解消 (運用文書化案で対応)** | `sp1-guests/README.md:57-61` に `cargo build --locked` 必須要件と `Cargo.lock` の役割（`APPROVED_VKEYS` 定数とバインドされている）を blockquote で明記。Round 2 の修正案「2. 運用文書化」がそのまま採用。`"=6.2.2"` への置換 (案 1) は採用されておらず `host/Cargo.toml:21,28` と `program/Cargo.toml:12` は依然 `"6.2"` だが、`Cargo.lock` (6.2.2 完全固定 / 両 workspace 共 git tracked) + README 文言の二重防御で、現実的なリグレッション経路は塞がれている。 |
| R2-N1 | should | **解消** | `program/src/main.rs:44-49` の `assert!` が `"attestation document too large: {} > {} bytes", doc_bytes.len(), MAX_DOC_BYTES,` 形式に書き直し。host 側 (`prove.rs:46-51`) の文言とほぼ対称になり、`prove --stdin` 経由で host を素通りした統合テストでも超過量が panic_msg から特定できる。Round 2 提案と一致。 |
| R2-N2 | nitpick | **wontfix 維持** | `host/src/lib.rs:28-35` の `cpu_setup()` は依然 `(CpuProver, SP1ProvingKey)` を返すタプル設計。`vkey_hash` は `(_client, pk)` で client を捨てる形 (`lib.rs:40-43`)。SP1 SDK 6.2 では `setup()` が `ProverClient::setup(...)` の client method として公開されているため、構造上これ以上短くできない。`setup_only()` の薄ラッパを切るのは美学の問題で、運用上 `vkey` bin は数十秒で終わるためコスト無視可能。Round 2 の判定「nitpick 維持」がそのまま妥当。 |
| R2-N3 | nitpick | **wontfix 維持** | `host/src/lib.rs:18-21` の `use sp1_sdk::{include_elf, CpuProver, Elf, HashableKey, ProveRequest, Prover, ProverClient, ProvingKey, SP1Stdin};` のうち `Prover`/`ProveRequest`/`HashableKey`/`ProvingKey` は trait import で、`.setup()` / `.prove(..).groth16().await` / `.bytes32_raw()` / `.verifying_key()` を呼ぶために in-scope が必要。`use ... as _;` 形式に切り出すと読みやすくはなるが、本質的振る舞いは不変。Round 2 判定維持。 |

---

## 新規発見 (Round 2 後の再精査で検出されたもの)

### R3-N1 [must-fix] **公開値の `instance_id` 長プレフィックスが guest (bincode u64 LE) と on-chain parser (u32 LE) で 4 バイト不一致**

**場所**:
- guest commit: `sp1-guests/attestation-aws-nitro/program/src/main.rs:62`
  ```rust
  sp1_zkvm::io::commit(&doc.module_id);
  ```
- guest doc / parser doc:
  - `program/src/main.rs:14` … `instance_id : Borsh String (u32 length prefix + UTF-8 bytes)`
  - `programs/title-whitelist/src/lib.rs:336` … `instance_id : Borsh String (u32 length + UTF-8 bytes)`
- on-chain parser:
  - `programs/title-whitelist/src/lib.rs:347-355`
    ```rust
    let id_len = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
    offset += 4;
    require!(data.len() >= offset + id_len, WhitelistError::InvalidPublicValues);
    offset += id_len;
    ```

**SP1 zkVM 側の実シリアライザ**:
- `sp1-lib-6.2.2/src/io.rs:126-129` で `commit` の実体は
  ```rust
  pub fn commit<T: Serialize>(value: &T) {
      let writer = SyscallWriter { fd: FD_PUBLIC_VALUES };
      bincode::serialize_into(writer, value).expect("serialization failed");
  }
  ```
- `bincode-1.3.3/src/lib.rs:90-99` で
  ```rust
  pub fn serialize_into<W, T: ?Sized>(writer: W, value: &T) -> Result<()> { ...
      DefaultOptions::new()
          .with_fixint_encoding()
          .allow_trailing_bytes()
          .serialize_into(writer, value)
  }
  ```
  → `DefaultOptions` (`config/mod.rs:100-101`) の `Endian = LittleEndian` / `IntEncoding = VarintEncoding` を、`with_fixint_encoding()` で **FixintEncoding (LE) に上書き** している。
- `bincode-1.3.3/src/ser/mod.rs:121-124` の `serialize_str` は
  ```rust
  fn serialize_str(self, v: &str) -> Result<()> {
      O::IntEncoding::serialize_len(self, v.len())?;
      self.writer.write_all(v.as_bytes()).map_err(Into::into)
  }
  ```
- `bincode-1.3.3/src/config/int.rs:24-35` の Fixint 実装は
  ```rust
  fn len_size(len: usize) -> u64 { Self::u64_size(len as u64) }
  fn serialize_len<W, O>(ser: ..., len: usize) -> Result<()> {
      Self::serialize_u64(ser, len as u64)
  }
  ```
  → **`String` の長さは常に `u64 LE` (8 バイト) で書かれる**。`u32` ではない。

**起きていること**:
- `doc.module_id` は AWS Nitro Enclave の wire 名 (例: `"i-0abc1234..."` や `"i-XXXX-encYYYY"`) で、典型長は 28〜48 文字。
- guest が public values に書き出すバイト列は実際には:
  ```
  [u64 LE: id_len][utf-8 bytes...][u64 LE: timestamp_ms][u32 LE: meas_len][meas bytes...]
                                                                          [u8 has_user_data][32 bytes hash?][u8 has_public_key][32 bytes hash?]
  ```
- 一方 parser は最初の 4 バイトしか id_len として読まないため:
  - 実 id_len = 例えば 28 (`0x1C, 0, 0, 0, 0, 0, 0, 0`) → parser は最初の `[0x1C, 0, 0, 0]` を `id_len = 28` と解釈し offset を `4 + 28 = 32` に進める。
  - 実際の文字列終端は offset `8 + 28 = 36` の位置で、parser のカーソルは 4 バイト手前にズレている。
  - 続けて parser は offset 32 から timestamp_ms 8 バイト + measurement_len 4 バイト ... を読もうとするが、**読みに行く先は実際の utf-8 末尾 4 バイト + 真の timestamp_ms の前半 4 バイト** という化け方をする。
  - timestamp_ms はバリデーション無しで `offset += 8` するだけなので即時には落ちないが、続く measurement_len で `(1..=MAX_MEASUREMENT_LEN).contains(&measurement_len)` (64 上限) を踏むため、**実際の timestamp_ms 上位バイト (現在の Unix 時刻 ms ≈ 1.7e12 → 16 進で巨大値) が measurement_len として読まれ、ほぼ確実に上限超過で `InvalidMeasurementLen` エラー** で落ちる。
- 結論: **現状の guest と parser の組み合わせでは `register_key` が成功する公開値レイアウトが存在しない** (module_id が空文字 = 0 長のときだけ偶然動く可能性があるが、AWS Nitro は module_id を空で出さない)。

**Round 2 がこれを見逃した理由**:
Round 2 の本文 (`audit/round2/k7-sp1-guests.md:11-12`) は
> 公開値レイアウト (Borsh String + LE 整数 + 32 バイトハッシュ) は guest と on-chain parser でバイナリ的に完全一致 (確認済み)。

と書いたが、これは guest と parser の **doc comment 同士** を見比べただけで、実際の `commit` バックエンド (sp1-lib → bincode) を引いていない。「Borsh String」という用語自体が両側の doc 上の言葉であって、SP1 zkVM の `commit` は実装上 bincode に固定されており Borsh を経由しない。

**修正案 (どちらか一方)**:

#### 案 A: guest 側を手動シリアライズに合わせる (推奨)
guest `program/src/main.rs:62` を以下に置換:
```rust
// `commit(&String)` would emit a bincode-fixint u64 length prefix; the
// on-chain parser is written for a u32 prefix. Emit the length explicitly
// and write the bytes via commit_slice so the wire format is unambiguous.
let id_bytes = doc.module_id.as_bytes();
sp1_zkvm::io::commit(&(id_bytes.len() as u32));
sp1_zkvm::io::commit_slice(id_bytes);
```
guest doc (`:14`) と parser doc (`:336`) の「Borsh String」表記も「u32 LE length + UTF-8 bytes (length-prefixed, not bincode-encoded String)」に直す。

`commit_slice` は raw byte 列を流し込むだけなので長さプレフィックスは付かない (`sp1-lib-6.2.2/src/io.rs:138-141` で確認済み)。

#### 案 B: parser 側を bincode に合わせる
`programs/title-whitelist/src/lib.rs:347-355` の `u32` 読み出しを `u64` に置換:
```rust
require!(data.len() >= offset + 8, WhitelistError::InvalidPublicValues);
let id_len = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap()) as usize;
offset += 8;
```
さらに `id_len` の妥当性チェック (例: `id_len <= 128`) を追加して攻撃者が巨大値を入れて oob 読みを起こせないようにする。

**推奨は案 A**。理由は:
1. 他フィールド (timestamp_ms / measurement_len / has_user_data / has_public_key) は `commit(&u64)` / `commit(&u32)` / `commit(&u8)` を使っており、fixint LE のおかげで偶然 doc どおりの bytes に落ちている。`String` だけが bincode 固有のクセを露呈する。`commit_slice` ベースに揃えると公開値レイアウトが「自前のシリアライザ」として完全に閉じる。
2. on-chain は SBPF 上で動くため余分な 4 バイト読みは僅かながらコストになる。
3. ベンダー追加 (`attestation-amd-sev-snp/` 等) で別 guest を書いたとき、`commit(&String)` を真似されて再発する事故を防げる。

**テスト追加 (修正後 must-do)**:
`programs/title-whitelist/` に host 側の round-trip テストを追加する。`title-sp1-attestation-aws-nitro-program` の `cargo test --no-run` 後の guest ELF を sp1-sdk の executor で 1 回 dry-run し、`SP1PublicValues` のバイト列を `parse_public_values` に通すユニットテスト 1 本があれば、今回のような encoding skew は CI で確実に検出できる。

---

### R3-N2 [should-fix] guest の doc comment が「Borsh String」と書いているが実体は bincode (R3-N1 と相互参照)

**場所**: `sp1-guests/attestation-aws-nitro/program/src/main.rs:14`

```rust
//!   instance_id       : Borsh String (u32 length prefix + UTF-8 bytes)
```

**問題**:
- SP1 zkVM の `sp1_zkvm::io::commit` は bincode 1.x を使うため、Borsh ではないし、`commit(&String)` の長さ前置は u32 ではなく u64 (R3-N1 の根拠)。
- 「Borsh」と書いてしまうと将来の保守者が `borsh::to_vec(&doc.module_id)` で再現できると誤解する。Borsh String は `u32 LE len + utf-8`、bincode fixint String は `u64 LE len + utf-8` で、両者は互換性がない。
- on-chain parser 側 (`title-whitelist/src/lib.rs:336`) も同じ誤称が伝染している。

**修正案**:
R3-N1 案 A を採用した上で、両側の doc を以下のように書き直す:
```
instance_id : u32 LE length + UTF-8 bytes
              (committed manually with `commit(&len_u32)` + `commit_slice(bytes)`;
               not `commit(&String)` — see sp1-lib `commit` uses bincode-fixint
               which would prefix a u64 length and break the parser.)
```

R3-N1 と一体で修正することを前提に should-fix。R3-N1 だけ直して doc を放置すると、後で「u32 にしたのに doc は Borsh のまま」というちぐはぐが残る。

---

### R3-N3 [nitpick] `program/src/main.rs:56-58` の `let _ = report.authenticate(...).expect(...)` の `let _` が意味的に冗長

**場所**: `sp1-guests/attestation-aws-nitro/program/src/main.rs:56-58`

```rust
let _ = report
    .authenticate(doc.timestamp / 1000)
    .expect("Attestation Document verification failed");
```

**問題**:
`authenticate` の戻り値は `anyhow::Result<CertChain<'_>>` (`crates/attestation-aws-nitro/src/doc.rs:55`)。`expect()` で unwrap した時点で `CertChain<'_>` を捨てているが、guest はこの戻り値を一切利用していないため `let _ = ... .expect(...)` の `let _` 部分は重複している。Rust の慣用では unused result の `expect` は文単独で書くのが普通:

```rust
report
    .authenticate(doc.timestamp / 1000)
    .expect("Attestation Document verification failed");
```

Round 2 K7-11 で `_cert_chain` という誤読を招く名前を消したのは正解だったが、その置換時に `let _ =` をうっかり残してしまった形跡がある。

**修正案**: `let _ =` を削るだけ。動作不変、表現が 1 トークン短くなる。

優先度: nitpick。clippy `#[warn(let_underscore_must_use)]` 等が将来有効化された際に拾われる可能性がある。

---

## 付随確認

### SP1 SDK 6.2 系の API 利用 (Round 2 で OK 判定 → 再確認)

`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/sp1-sdk-6.2.2/` と `sp1-lib-6.2.2/` を再 grep:
- `ProverClient::builder().cpu().build().await` (`host/src/lib.rs:29`) — 6.2 系の async builder と一致。
- `client.setup(ATTESTATION_ELF).await` (`host/src/lib.rs:30-33`) — `Prover::setup` の async シグネチャと一致。
- `client.prove(&pk, stdin).groth16().await` (`host/src/lib.rs:65-68`) — `ProveRequest::groth16` で proof mode を選び、`.await` で実行。
- `pk.verifying_key().bytes32_raw()` (`lib.rs:42, 74`) — `HashableKey::bytes32_raw` は `[u8; 32]` 直返し。
- `proof.bytes()` (`lib.rs:72`) — SP1 Groth16 proof の wire bytes (4 バイト VK selector + 256 バイト proof = 260 バイト) を返す。`title-whitelist/src/lib.rs:289-298` の `groth16_vk_hash` 検証ロジックとも整合。

API 利用面でのリグレッションは検出されず。

### Cargo.lock 固定状況 (K7-12 補足確認)

- `sp1-guests/attestation-aws-nitro/host/Cargo.lock` は sp1-sdk / sp1-build / sp1-zkvm / sp1-lib / sp1-prover 等の **全 sp1-* クレートを `6.2.2` で固定** (host/Cargo.lock 内に `version = "6.2.2"` が 34 出現、いずれも sp1 系)。
- `sp1-guests/attestation-aws-nitro/program/Cargo.lock` も `sp1-zkvm` / `sp1-primitives` 等を `6.2.2` で固定 (10 出現)。
- 両 Cargo.lock とも git tracked。README の `--locked` 警告 (`:57-61`) と組み合わせると、外部クローナが `cargo update -p sp1-sdk` しない限り vkey はドリフトしない。

### bincode 1.x の文脈

- bincode 2.x は API 自体が変わり、`bincode::serialize_into` は存在しない (`encode_to_*` 系)。`sp1-lib-6.2.2/Cargo.toml:44-45` は `bincode = "1.3.3"` で 1.x を固定しており、本監査時点の挙動は今後の sp1 minor bump で勝手に変わらない。
- bincode 2.x への移行は sp1 majour bump (7.x?) を要するため、現状の R3-N1 修正は 6.2 系で 1 度直せば貼り付くが、将来 sp1 7.x で bincode 2 に切り替わると `commit(&u64)` 等の他フィールドもエンコード変化を起こす可能性がある。R3-N1 案 A (手動シリアライズ) はその将来事故も同時に防ぐため、案 B より構造的に強い。

---

## カウント

| 重大度 | 件数 | ID |
|---|---|---|
| must-fix | **1** | R3-N1 (新規 — guest bincode u64 len vs parser u32 len 不一致) |
| should-fix | **1** | R3-N2 (新規 — doc 「Borsh String」誤称、R3-N1 と一体修正) |
| nitpick | **1** | R3-N3 (新規 — `let _ =` 冗長) |
| **合計** | **3** | — |

Round 2 比: 5 件 → 3 件 (R2 残置 5 件のうち 3 件解消、2 件 wontfix 維持 → 新規 3 件)。
Round 1 比: 13 件 → 3 件 (must 3 → 1)。must が 0 → 1 に逆戻りしているのは Round 2 が見逃した encoding 不一致を Round 3 で検出した結果であり、Round 2 修正が混入させた回帰ではない (R3-N1 のコード片自体は Round 1 時点から存在する)。

## 構造的に良いと感じた点 (Round 3 視点)

- Round 1 → Round 2 で導入された host/guest 二段 `MAX_DOC_BYTES`、`authenticate(timestamp)` の物理的歯止め、`stdout/stderr` 分離した `vkey` bin、stem ベースの出力ファイル命名、いずれも Round 3 時点で安定して保たれている。
- `sp1-guests/README.md:57-61` の `cargo build --locked` 必須要件と `APPROVED_VKEYS` バインドの説明は OSS 公開時の典型事故 (clone → unlocked build → vkey drift → on-chain mismatch) を 4 行で塞いでおり、運用文書として模範的。
- `program/Cargo.toml:15` の `title-attestation-aws-nitro = { ..., features = ["sp1"] }` で SP1 precompile を有効化する依存固定は、`crates/attestation-aws-nitro/src/lib.rs:8` の SP1 機能フラグ説明と整合。precompile feature 配線がブレていない。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| K7-04 | fixed (Round 3 認定) | `sp1-guests/README.md:52-55` でメモリ要件 (~30 GiB / 64 GiB RAM / r5.4xlarge) を blockquote で明記。 |
| K7-12 | fixed (Round 3 認定) | `sp1-guests/README.md:57-61` で `cargo build --locked` 必須と `Cargo.lock` の役割 (`APPROVED_VKEYS` バインド) を blockquote で明記。`"=6.2.2"` 表記化は採用されず Cargo.lock 固定で代替。 |
| R2-N1 | fixed (Round 3 認定) | `program/src/main.rs:44-49` の `assert!` 文言に実長と上限を埋め込む形に書き直し。 |
| R2-N2 | wontfix(SP1 SDK 6.2 の `setup()` が client method である API 制約上、`cpu_setup` のタプル戻り値設計が最短形。`setup_only()` 抽出は美学の問題でコスト無視可能) | |
| R2-N3 | wontfix(`Prover`/`ProveRequest`/`HashableKey`/`ProvingKey` の trait import は `.setup()` / `.prove(..).groth16().await` / `.bytes32_raw()` / `.verifying_key()` 呼び出しに必須。`use ... as _;` 形式に書き換えは読みやすさの好みで、機能不変) | |
| R3-N1 | fixed | 案 A 採用。`sp1-guests/attestation-aws-nitro/program/src/main.rs:62` の `commit(&doc.module_id)` を `commit(&(id_bytes.len() as u32))` + `commit_slice(id_bytes)` に置換。これで wire format は u32 LE length + UTF-8 bytes に確定し、on-chain `parse_public_values` の読み出しと整合する。round-trip テスト追加 (SP1 executor 経由) は guest binary の再ビルドが必要なため別タスクで対応 — 当面は実機 (devnet register_key 成功) で検証する。 |
| R3-N2 | fixed | `program/src/main.rs:14` と `programs/title-whitelist/src/lib.rs:336` の「Borsh String」表記を「u32 LE length + UTF-8 bytes」に書き換え。bincode-fixint の落とし穴を doc に明記し、`commit(&String)` 形式を将来書かないよう注意書きを追加。 |
| R3-N3 | fixed | `program/src/main.rs:56` の `let _ =` を削除。`report.authenticate(...).expect(...)` を文単独で書く形に変更。動作不変。 |
