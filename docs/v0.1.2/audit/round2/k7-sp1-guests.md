# K7 監査 Round 2: `sp1-guests/` 縦深掘り (修正後再点検)

担当範囲: `sp1-guests/` 配下全ファイル。
比較基準: Round 1 `docs/v0.1.2/audit/k7-sp1-guests.md` (must:3 / should:6 / nitpick:4)。
今回の方針: Round 1 指摘 13 件の処理状況を 1 件ずつ追跡し、修正中に混入した新規問題を `must / should / nitpick` で分類する。

## サマリ

- Round 1 既出 13 件のうち **解消 11 件**、**未対応 2 件** (K7-04 / K7-12)。
- 新規発見 **3 件** (R2-N1 should、R2-N2 nitpick、R2-N3 nitpick)。
- 機能上のリグレッションは **ゼロ**。SP1 SDK 6.2 async 化、`bytes32_raw()`、`groth16()` の使い方はすべて正しい。
- 公開値レイアウト (Borsh String + LE 整数 + 32 バイトハッシュ) は guest と on-chain parser でバイナリ的に完全一致 (確認済み)。

---

## Round 1 指摘の追跡

| ID | 重大度 | 状態 | 根拠 (file:line) |
|---|---|---|---|
| K7-01 | must | **解消** | `program/src/main.rs:14, :23-25, :58-60` で公開値 doc が `instance_id` に統一され、`module_id` を commit している箇所には「vendor-neutral な instance_id スロットとして commit する」注記が入った。On-chain parser (`programs/title-whitelist/src/lib.rs:334, :345`) と用語が完全一致。 |
| K7-02 | must | **解消** | `host/src/bin/prove.rs:63-79`。`with_extension` 連鎖を捨て、`file_name()` + `format!("{stem}.proof.bin")` ベースに置換。コメント (`:63-66`) で「なぜ `with_extension` ではないか」も明記。`parent()` が空文字列のときに `.` を代入する分岐 (`:72-76`) も入っており、`prove attestation.bin` のように親ディレクトリがない呼び出しでも安全。 |
| K7-03 | must | **解消** | `host/src/bin/prove.rs:41` で `sp1_sdk::utils::setup_logger()` を呼び、SP1 内部 tracing が stderr に流れるようになった。さらに `Instant::now()` / `eprintln!("Proof generated in {elapsed:.1?}")` (`:59, :85`) で経過時間も出る。コメント (`:38-40`) が「何が無いと何が起きるか」を端的に説明していて良い。 |
| K7-04 | should | **未対応** | `prove.rs:21-29` の `long_about` に「~30 GiB / 64 GiB RAM 推奨 / r5.4xlarge」が書かれた → `prove --help` 経由では正しく案内される。しかし **`sp1-guests/README.md:18, :48` 側は「~90 minutes on CPU」のみで、メモリ要件と RAM サイズが未記載**。リポジトリ表紙の README を読んだだけのオペレータは依然として OOM に当たる。下記「未対応詳細」参照。 |
| K7-05 | should | **解消** | guest 側の `assert!(doc_bytes.len() <= MAX_DOC_BYTES, ...)` (`program/src/main.rs:44-47`) と host 側の `anyhow::ensure!(doc.len() <= MAX_DOC_BYTES, ...)` (`prove.rs:46-51`) の二段防御で、巨大入力時の panic フェーズが「SP1 起動前」に前倒しされた。残る `expect` は本質的に必要なフェーズ (COSE parse / authenticate / PCR0 取得) のみで、メッセージは識別可能。Round 1 で推奨した `--dry-run` モードまでは入っていないが、prove 前に 90 分待たずに失敗する入力は事実上「壊れた CBOR」のみになり、リスクは大幅低減。 |
| K7-06 | should | **解消** | 上流 `crates/attestation-aws-nitro/src/doc.rs:53` で `authenticate` の第1引数 `trusted_certs_prefix_len` が削除され、シグネチャが `pub fn authenticate(&self, timestamp: u64)` になった。guest (`program/src/main.rs:54-56`) も `.authenticate(doc.timestamp / 1000)` の 1 引数呼びに変更済み。**「物理的歯止め」案がそのまま採用されている** — コメント運用ではなく型システムで保証された。 |
| K7-07 | should | **解消** | `prove.rs:16, :46-51` (host 側) と `program/src/main.rs:40, :44-47` (guest 側) の双方に `MAX_DOC_BYTES = 16 * 1024` の上限がある。Round 1 案の「両方に二重に置く」がそのまま実装された (host 側で早期失敗、guest 側で多層防御)。 |
| K7-08 | should | **解消** | `host/src/bin/vkey.rs:17-25` で `# guest: title-sp1-attestation-aws-nitro-program <CARGO_PKG_VERSION>` と `# captured: <unix秒>` を stderr へ出力し、stdout は hex 1 行のまま機械可読を維持。`chrono` 依存追加を回避し `SystemTime::now().duration_since(UNIX_EPOCH)` を使った点も Round 1 案と整合。 |
| K7-09 | should | **未対応 (許容)** | `host/src/lib.rs:28-35` の `cpu_setup()` は依然として `vkey_hash()` / `generate_groth16_proof()` の双方が独立に呼ぶ。ただし運用上 vkey 取得と prove は別プロセス・別タイミングで実行されるため (vkey は事前に Solana へ焼き込み、prove は別日)、現実的には影響なし。Round 1 で should-fix どまりとした判断を維持。**対処不要と判断**。 |
| K7-10 | nitpick | **解消** | `program/src/main.rs:5` が `Spec §6.2 — runs once when a signer key is registered on-chain.` に書き換わった。Round 1 案そのまま採用。 |
| K7-11 | nitpick | **解消** | `program/src/main.rs:54-56` が `let _ = report.authenticate(doc.timestamp / 1000).expect(...);` の形になり、`_cert_chain` という誤読を招く名前付き変数は消えた。 |
| K7-12 | nitpick | **部分対応** | `host/Cargo.toml:21, :28` および `program/Cargo.toml:12` は依然 `"6.2"` (キャレット相当)。一方で `Cargo.lock` (host / program 両方) は git tracked かつ `6.2.2` 完全固定 (`host/Cargo.lock:3811, :4293`, `program/Cargo.lock:1696`)。`cargo build --locked` を運用で徹底すれば実害なし。それでも「`=6.2.2`」表記の方が意図が明示的で OSS 公開コードとしては推奨。下記「未対応詳細」参照。 |
| K7-13 | nitpick | **解消 (撤回)** | `OPERATIONS_JA.md` の存在を確認 (`docs/v0.1.2/OPERATIONS_JA.md`)。README からの相対リンクも妥当。Round 1 の自己撤回どおり問題なし。 |

---

## 未対応詳細

### K7-04 [should-fix 継続] `sp1-guests/README.md` のメモリ要件記載漏れ

**場所**: `sp1-guests/README.md:18, :48`

**現状**:
- `prove.rs --help` の `long_about` には「~30 GiB peak / 64 GiB RAM 推奨 / r5.4xlarge」が正しく書かれている。
- しかし README 本文は `proving alone takes ~90 minutes on CPU` (`:18`) と `Generate a Groth16 proof (slow: ~90 min on CPU).` (`:48`) のみで、メモリへの言及がゼロ。
- 初見のオペレータはまず README を読み、`cargo run --release --bin prove ...` を試す → `--help` を見ずに進めるケースが大半。t3.medium 等で起動した結果 90 分後に OOM kill されると体験が悪い。

**修正案 (書き直し)**:
README の `## Running` セクション末尾、または `## Layout` の prove 行に以下を追記:
```markdown
> `prove` peaks at roughly 30 GiB resident memory during the Groth16 wrap.
> Use an instance with at least 64 GiB RAM (EC2 r5.4xlarge or larger).
> Run `cargo run --release --bin prove -- --help` for the full output-file
> layout and a recap of the resource requirements.
```

---

### K7-12 [nitpick 継続] `Cargo.toml` の SP1 SDK バージョン表記

**場所**: `sp1-guests/attestation-aws-nitro/host/Cargo.toml:21, :28`, `program/Cargo.toml:12`

**現状**:
```toml
sp1-sdk = "6.2"      # 解釈: >=6.2.0, <7.0.0
sp1-build = "6.2"
sp1-zkvm = "6.2"
```
`Cargo.lock` 2 ファイル (host / program) はいずれも git tracked かつ `6.2.2` で固定済み (`host/Cargo.lock:3811, 4293`、`program/Cargo.lock:1696`)。`cargo build --locked` ないし `cargo prove build` の lockfile 尊重前提で運用すれば、vkey の意図しないドリフトは起きない。

**残るリスク**:
- 上流の lockfile から `Cargo.lock` を消して clone した第三者が `cargo update -p sp1-sdk` を打つと無警告で 6.3 / 6.4 系に上がり、vkey が変わる。
- OSS 公開時の README に「必ず `--locked` で build せよ」とは明記されていない。

**修正案 (どちらか)**:
1. **物理的固定**: 全 3 箇所を `"=6.2.2"` に置換。`Cargo.lock` と二重防御になり、`cargo update` で上がっても表記と矛盾するため警告が出やすい。
2. **運用文書化**: `sp1-guests/README.md` に
   ```markdown
   > Always build with `cargo build --locked` (or set `CARGO_NET_OFFLINE=true`
   > after the first build) — the committed `Cargo.lock` pins SP1 SDK to a
   > version that produced the on-chain `APPROVED_VKEYS` constant.
   ```
   を追記。
   `vkey` bin が出す stderr メタデータ (`# guest: ... 0.1.2`) と相互参照すると更に安全。

優先度: nitpick 維持。`Cargo.lock` 固定が現実的にカバーしているため。

---

## 新規発見 (Round 1 後の修正で混入したもの)

### R2-N1 [should-fix] `program/src/main.rs:44` の `assert!` 文言が情報不足

**場所**: `sp1-guests/attestation-aws-nitro/program/src/main.rs:44-47`

```rust
assert!(
    doc_bytes.len() <= MAX_DOC_BYTES,
    "attestation document too large"
);
```

**問題**:
host 側 (`prove.rs:46-51`) は `"attestation document too large ({} > {} bytes); aborting before SP1 setup"` と長さを含めた診断を出すが、guest 側は実長/上限のいずれも文言に含まない。host を経由せず direct stdin で guest を叩く統合シナリオ (タスク 18 以降に想定) で「どれだけオーバーしたか」が見えない。

guest panic は SP1 SDK 6.2 では host の `prove()` エラー文字列に伝搬する (panic_msg として乗る)。文字列の情報密度がそのまま運用デバッグ性に直結する。

**修正案 (書き直し)**:
```rust
assert!(
    doc_bytes.len() <= MAX_DOC_BYTES,
    "attestation document too large: {} > {} bytes",
    doc_bytes.len(),
    MAX_DOC_BYTES,
);
```
guest コードサイズへの影響は数十バイト RISC-V 命令で、サイクル数には実質影響なし (panic path のみ)。

---

### R2-N2 [nitpick] `host/src/lib.rs:28-35` の `cpu_setup` 戻り値で `client` を捨てる場面がある

**場所**: `sp1-guests/attestation-aws-nitro/host/src/lib.rs:40-43`

```rust
pub async fn vkey_hash() -> anyhow::Result<[u8; 32]> {
    let (_client, pk) = cpu_setup().await?;
    Ok(pk.verifying_key().bytes32_raw())
}
```

**問題**:
- `vkey_hash` は client を使わないため `_client` で受けている。これは正しい書き方だが、`cpu_setup()` の戻り値型が「常に client + pk」のタプルなのは少し過剰。
- vkey 計算は `client.setup()` の副産物として `pk` が得られれば十分で、prover client 本体 (Plonky3 prover state 等を含む) のフル構築まではしなくて良いケースがある。SP1 SDK 6.2 の `ProverClient::builder().cpu().build()` は内部で worker thread を立てる可能性があり、`vkey` bin の起動コストが prove と同じになる。
- ただし実測で `vkey` bin は数十秒で終わるはずで、運用影響は無視できる。

**修正案 (書き直し)**:
SDK 公開 API の制約上、`setup()` が client method なので構造上はこれが最短。気になるなら lib に
```rust
async fn setup_only() -> anyhow::Result<sp1_sdk::SP1ProvingKey> {
    let (_client, pk) = cpu_setup().await?;
    Ok(pk)
}
```
を切り出して `vkey_hash` から呼び、「ここでは client は使わない」を型で表現するのが綺麗。優先度低、nitpick 維持。

---

### R2-N3 [nitpick] `host/src/lib.rs:18-21` の use 行が `Prover`, `ProveRequest`, `ProvingKey` を import するが直接は未使用

**場所**: `sp1-guests/attestation-aws-nitro/host/src/lib.rs:18-21`

```rust
use sp1_sdk::{
    include_elf, CpuProver, Elf, HashableKey, ProveRequest, Prover, ProverClient, ProvingKey,
    SP1Stdin,
};
```

**問題**:
- `ProveRequest` および `Prover` は trait であり、`.prove(...)` メソッドの呼び出しに必要な trait import (`Prover`) と builder pattern の trait (`ProveRequest`) として **暗黙に必要**。`ProvingKey` も同様に trait として必要 (型は `SP1ProvingKey` 別名)。
- 一見すると「未使用 import」だが、rust の trait resolution の都合上削れない。これは読み手を混乱させる。

**修正案 (書き直し)**:
trait 用途を明示するコメントを 1 行入れる:
```rust
use sp1_sdk::{
    include_elf,
    CpuProver,
    Elf,
    HashableKey,                          // .bytes32_raw()
    ProveRequest,                         // builder: client.prove(..).groth16()
    Prover,                               // .setup() / .prove()
    ProverClient,
    ProvingKey,                           // trait: .verifying_key()
    SP1Stdin,
};
```
あるいは `#[allow(unused_imports)]` を避け、必要 trait は `use sp1_sdk::Prover as _;` 形式で「名前は import しないが trait は in scope」にする方が読みやすい:
```rust
use sp1_sdk::{include_elf, CpuProver, Elf, ProverClient, SP1Stdin};
use sp1_sdk::{HashableKey as _, ProveRequest as _, Prover as _, ProvingKey as _};
```

優先度: nitpick。動作には完全に無影響。

---

## SP1 SDK 6.2 移行に関する確認

Round 1 修正で混入した async 化と新 API 利用について、SDK 公開コード (`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/sp1-sdk-6.2.2/`) と突合した結果:

- `ProverClient::builder().cpu().build().await` (`lib.rs:29`): 正しい。SDK 6.2 で `build()` は async fn (内部で `tokio::spawn_blocking` を呼ぶため)。
- `client.setup(ATTESTATION_ELF).await` (`lib.rs:30-33`): 正しい。`Prover::setup` の async シグネチャと一致。
- `pk.verifying_key().bytes32_raw()` (`lib.rs:42, :74`): 正しい。`HashableKey::bytes32_raw` は `[u8; 32]` を返し、`hash_bytes()` (`Vec<u8>`) より型安全。
- `client.prove(&pk, stdin).groth16().await` (`lib.rs:65-68`): 正しい。`ProveRequest::groth16` で proof mode を選択 → await で実行。
- `include_elf!` (`lib.rs:24`): 正しい。`Elf` 型として埋め込まれる (旧 `pub const ELF: &[u8] = include_bytes!(env!("...")) ` パターンから移行済み)。

`tokio::main` + `rt-multi-thread, macros` feature (`host/Cargo.toml:22`) も SDK の async 要件と整合 (SDK は内部で `tokio::task::spawn_blocking` を使うため multi-thread runtime が必須)。

---

## カウント

| 重大度 | 件数 | ID |
|---|---|---|
| must-fix | 0 | (Round 1 の must 3 件はすべて解消) |
| should-fix | 2 | K7-04 (継続), R2-N1 (新規) |
| nitpick | 3 | K7-12 (継続), R2-N2 (新規), R2-N3 (新規) |
| **合計** | **5** | — |

Round 1 比: 13 件 → 5 件。must は 3 → 0。

## 補遺: 構造的に良いと感じた Round 2 変更点

- guest と host の二段 `MAX_DOC_BYTES` チェック (`program/src/main.rs:40`, `prove.rs:16`) で攻撃面 / 事故面の両方に多層防御。
- `host/src/bin/prove.rs:63-66` のコメントが「なぜ `Path::with_extension` を使わないか」を 4 行で簡潔に説明していて、Round 1 の指摘根拠 (`nitro.v1.bin` → `nitro.proof.bin` 問題) が将来の読み手にも届く形になっている。
- `vkey.rs` のメタデータが stderr / stdout を分けている設計 (`vkey > vkey_hash.hex` がきれいに通る) は OSS 運用ツールとして模範的。
- `crates/attestation-aws-nitro/src/doc.rs:53` での `authenticate(timestamp)` シグネチャ変更により、guest 側が `prefix_len = 0` をうっかり変えてしまう将来事故が型システムで物理的に不可能になった (K7-06 の「物理的歯止め」案がそのまま採用)。
- `Cargo.lock` 2 ファイルが両 workspace に git tracked で存在し、`6.2.2` 完全固定。SDK minor bump による vkey ドリフトは現状の運用 (`--locked` build) で阻止できる状態。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| K7-01..03, K7-05..08, K7-10/11/13 | fixed | Round 2 認定済み。 |
| K7-04 | fixed | `sp1-guests/README.md` の `## Running` セクション末尾にメモリ要件（~30 GiB peak / 64 GiB RAM 推奨 / r5.4xlarge）を blockquote で追記。 |
| K7-09 | wontfix(`cpu_setup` の vkey 計算と prove client 構築が二重になるが、vkey 取得と prove は別タイミング・別プロセスで実行されるため運用上影響ゼロ) | |
| K7-12 | fixed | `sp1-guests/README.md` に `cargo build --locked` の必須要件と Cargo.lock の役割（APPROVED_VKEYS とのバインド）を 4 行 blockquote で追記。`"=6.2.2"` への変更は Cargo.lock 固定で二重防御として残置。 |
| R2-N1 | fixed | `program/src/main.rs:44` の `assert!` 文言に実長と上限を埋め込む形に書き直し。`"attestation document too large: {} > {} bytes"` 形式。 |
| R2-N2 | wontfix(`cpu_setup` の `(client, pk)` タプル分離は SP1 SDK の API 制約で `setup()` が client method。`vkey_hash` で `_client` で受ける現行が SDK 公開 API 上の最短) | |
| R2-N3 | wontfix(`Prover`/`ProveRequest`/`ProvingKey` の trait import は rust の trait resolution 上必須。`use ... as _` への置換は読み手分割の好みで本質的振る舞いに無影響) | |
