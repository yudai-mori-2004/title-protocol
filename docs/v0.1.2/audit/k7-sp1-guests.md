# K7 監査: `sp1-guests/attestation-aws-nitro` 縦深掘り

担当範囲: `sp1-guests/` 配下全ファイル (program/、host/、README.md、build.rs、Cargo.toml) を 1 文単位で精査。
基準仕様: `docs/v0.1.2/SPECS_JA.md` §6.2 (検証プログラムの同一性: vkey + measurement の二段照合)。

## サマリ

- 件数: 13 件
- must-fix: 3
- should-fix: 6
- nitpick: 4

最も重要な発見:
- **K7-01 (must)**: guest が commit する第1フィールドの命名が `module_id` (AWS Nitro 固有用語) であるのに対し、オンチェーン parser (`programs/title-whitelist/src/lib.rs:319`) のドキュメントと SP1 guest doc comment では `instance_id` と矛盾している。仕様 §6.2 の「vendor-neutral envelope」を主張するなら、命名と doc を統一する必要がある。
- **K7-02 (must)**: `prove.rs` の出力ファイル名が `args.attestation.with_extension("")` → `with_extension("proof.bin")` のパターンで生成される。入力が `attestation.bin` の場合は `attestation.proof.bin` で意図通りだが、入力が `attestation` (拡張子なし) のときは `with_extension("")` が no-op となり、`attestation.proof.bin` ではなく `attestation.proof.bin` で正しい一方、入力が `foo.tar.gz` のような複合拡張子だと `foo.tar.proof.bin` になり混乱しうる。後述する。
- **K7-03 (must)**: 90 分かかる prove 処理に進捗表示が一切なく、stderr の最初の "Generating proof..." 1 行のみ。途中で OOM kill されても気づきにくい。

---

## 詳細

### K7-01 [must-fix] 公開値の命名矛盾 — `module_id` vs `instance_id`

**場所**:
- guest commit: `sp1-guests/attestation-aws-nitro/program/src/main.rs:13` (doc), `:54` (実コード)
- on-chain parser doc: `programs/title-whitelist/src/lib.rs:319` (`instance_id : Borsh String`)
- 上流型: `crates/attestation-aws-nitro/src/doc.rs:105` (`pub module_id: String`)

**問題**:
guest の doc コメント (program/src/main.rs:13) は公開値レイアウトを `module_id : Borsh String` と書き、実コードも `sp1_zkvm::io::commit(&doc.module_id)` で commit している。一方、オンチェーン parser の docstring は同じ位置を `instance_id : Borsh String` と書いており、`parse_public_values` 内のコメント (`programs/title-whitelist/src/lib.rs:330`) も `// instance_id: String (u32 len + bytes)` となっている。さらに SP1 host lib のモジュール doc (`host/src/lib.rs` の冒頭ではないが、program/src/main.rs:5-6) は「Spec §6.2 — Solana Extension preparation」と述べるが、SPECS_JA.md §6.2 周辺で `instance_id` / `module_id` の正規名称が明示されていない (検証要)。

vendor-neutral envelope を主張するなら、AWS Nitro の用語 (`module_id` は Nitro Attestation Document のフィールド名) を公開値レイヤーに漏らすべきではない。

**修正案 (書き直し)**:
1. guest の commit 順は変えない (vkey hash が変わる: must-not-touch)。
2. doc comment のみ統一する: guest 側 (program/src/main.rs:13) を `instance_id` に書き換える、または parser 側を `module_id` に書き換える。SPECS_JA との整合を踏まえると、vendor-neutral な `instance_id` で統一するのが望ましい。
3. 上流の `crates/attestation-aws-nitro/src/doc.rs` の `module_id` フィールドはベンダー固有なので名称維持で良いが、guest 側で `sp1_zkvm::io::commit(&doc.module_id)` するときの「これを vendor-neutral な instance_id として commit している」旨を 1 行注記する。

---

### K7-02 [must-fix] `prove.rs` の出力ファイル名生成が脆い

**場所**: `sp1-guests/attestation-aws-nitro/host/src/bin/prove.rs:43-46`

```rust
let base = args.attestation.with_extension("");
let proof_path = base.with_extension("proof.bin");
let pv_path = base.with_extension("public_values.bin");
let vkey_path = base.with_extension("vkey_hash.hex");
```

**問題**:
- 入力 `attestation.bin` → base `attestation` → `attestation.proof.bin` (意図通り)
- 入力 `attestation` (拡張子なし) → base `attestation` → `attestation.proof.bin` (意図通り、ただし元入力を上書きする可能性がある)
- 入力 `nitro-2025-05-24.bin` → base `nitro-2025-05-24` → `nitro-2025-05-24.proof.bin` (意図通り)
- 入力 `nitro.v1.bin` → base `nitro.v1` → `nitro.v1.proof.bin` ではなく `nitro.v1.proof.bin` となり、`with_extension("proof.bin")` は `.v1` を `.proof.bin` に置換するため `nitro.proof.bin` になる。**ファイル名の意味的部分 `.v1` が失われる**。

`Path::with_extension` は最後の拡張子を置換する仕様であり、`base = "nitro.v1"` に対して `with_extension("proof.bin")` を呼ぶと `nitro.proof.bin` になる (`.v1` が拡張子と解釈される)。

**修正案 (書き直し)**:
`format!` で素直に組み立てる:
```rust
let stem = args.attestation.file_name()
    .and_then(|s| s.to_str())
    .ok_or_else(|| anyhow::anyhow!("invalid input filename"))?;
let dir = args.attestation.parent().unwrap_or(Path::new("."));
let proof_path = dir.join(format!("{stem}.proof.bin"));
let pv_path = dir.join(format!("{stem}.public_values.bin"));
let vkey_path = dir.join(format!("{stem}.vkey_hash.hex"));
```
これで入力ファイル名がそのまま suffix の前に温存される (`nitro.v1.bin.proof.bin` 等)。冗長性は受け入れて衝突を防ぐ。

---

### K7-03 [must-fix] 90 分処理に進捗表示がない

**場所**: `sp1-guests/attestation-aws-nitro/host/src/bin/prove.rs:39`

```rust
eprintln!("Generating proof (this takes ~90 minutes on CPU)...");
let artifacts = generate_groth16_proof(&doc).await?;
```

**問題**:
- SP1 SDK の prove フローは内部で `info!` ログを出すが、`tracing_subscriber` の初期化が host bin に無いため、何も表示されない。
- OOM kill (README で「several GB の working set」と注記しているが、実際は Groth16 wrap で 30 GB+ 食う) や中断時に、ユーザーは「動いてるのか死んでるのか」が分からない。

**修正案 (書き直し)**:
1. `main` 冒頭で `sp1_sdk::utils::setup_logger()` (もしくは同等の `tracing_subscriber::fmt().init()`) を呼ぶ。
2. 完了直前/直後に `eprintln!` で経過時間を出す: `let start = std::time::Instant::now();` → `eprintln!("Proof generated in {:.1?}", start.elapsed());`
3. README/OPERATIONS_JA.md に `RUST_LOG=info` 推奨を明記する。

---

### K7-04 [should-fix] README とコード間のメモリ見積もりの不整合

**場所**:
- `sp1-guests/README.md:18` 「proving alone takes ~90 minutes on CPU」 (メモリ言及なし)
- `host/src/bin/prove.rs:21-22` "Use a host with sufficient memory; the SP1 prover allocates several GB of working set."

**問題**:
SP1 Groth16 prove は Plonky3 → Groth16 wrap 段階で実測 20–32 GB 程度のピーク RSS を要する (SP1 6.x 系)。「several GB」は楽観的すぎる。タスク 15 で実機 EC2 (r5.4xlarge = 128 GB) を立てた経緯と矛盾する。

**修正案 (書き直し)**:
prove.rs の long_about を以下に置換:
```
Proving an Attestation Document on CPU takes ~90 minutes and peaks at
~30 GB resident memory during the Groth16 wrap. Use an instance with at
least 64 GB RAM (r5.4xlarge or larger).
```
README にも同等の追記を入れる。

---

### K7-05 [should-fix] guest 内 `expect` が複数あり、panic 時の振る舞いが不透明

**場所**: `sp1-guests/attestation-aws-nitro/program/src/main.rs:43, :51, :58`

```rust
let report = AttestationReport::parse(&doc_bytes).expect("COSE_Sign1 parse failed");
let _cert_chain = report.authenticate(0, doc.timestamp / 1000).expect("Attestation Document verification failed");
let measurement = doc.pcrs.get(&0).expect("PCR0 missing");
```

**問題**:
SP1 zkVM 内の panic は "unprovable" として host 側で `SP1ExecutionError` になる。これは意図通りだが、host (`generate_groth16_proof`) はそのエラーを `anyhow::anyhow!("SP1 prove failed: {e}")` で wrap するため、ユーザーには「prove failed」としか伝わらない。invalid な attestation を投げたのか、guest がバグっているのかが区別できない。

また、攻撃者目線では「panic 文言からどのフェーズで弾かれたかが漏れる」が、これは SP1 では公開値に含まれないため情報漏洩リスクは無い (✓ 確認済み)。

**修正案 (書き直し)**:
1. host 側で `execute()` を先に呼んで早期失敗させる pre-flight モードを足す (90 分待つ前に検出できる)。`prove.rs` に `--dry-run` フラグを追加し、`client.execute(ATTESTATION_ELF, stdin).await` で `panic_msg` を含むエラーを取り出し人間可読に表示する。
2. もしくは guest の `expect` を `panic!` + 明示メッセージにし、host の execute 段で文字列マッチで該当フェーズを表示する。

---

### K7-06 [should-fix] `trusted_certs_prefix_len = 0` のハードコードに関するセキュリティ理由がコメントのみに依存

**場所**: `sp1-guests/attestation-aws-nitro/program/src/main.rs:27-30, :50`

**問題**:
コメント (`:27-30`) は「prover に skip を許すと chain 検証バイパスを許す」と説明しているが、これは guest が将来の改修で `sp1_zkvm::io::read::<u32>()` を追加すると即座に攻撃面になる重大設計判断である。コメントだけが歯止めになっている。

**修正案 (書き直し)**:
1. 上流 `AttestationReport::authenticate` のシグネチャから第1引数 (trusted_certs_prefix_len) を削除し、内部で必ず 0 を使うように変更する。guest 側でも引数が消えるので将来の事故が物理的に発生しない。
2. それが crate 共有のため難しい場合、guest に test を追加: `#[test] fn cert_prefix_must_be_zero() { ... }` — guest crate にユニットテストは置けないため、host 側に「guest source の特定文字列を grep して 0 を確認する」ような contract test を置く (低品質だが歯止めになる)。

優先度: コメント運用で許容できれば nitpick だが、§6.2 の信頼根に直結するため should-fix。

---

### K7-07 [should-fix] host crate に attestation 入力サイズの上限チェックが無い

**場所**: `sp1-guests/attestation-aws-nitro/host/src/bin/prove.rs:33, host/src/lib.rs:64-65`

```rust
let doc = fs::read(&args.attestation)?;
...
stdin.write_slice(attestation_doc);
```

**問題**:
- AWS Nitro Attestation Document の実サイズは概ね 4–6 KB だが、悪意ある (または事故で間違えた) 入力で巨大ファイルを渡すと 90 分かけて結局 guest 内で OOM panic になる。
- guest 内の `sp1_zkvm::io::read_vec()` は sized 入力を受け取るため、巨大入力に対するメモリ消費は guest VM 側のサイクル数として爆発する。

**修正案 (書き直し)**:
prove.rs の `let doc = fs::read(...)?` の直後に:
```rust
const MAX_DOC_BYTES: usize = 16 * 1024;
anyhow::ensure!(
    doc.len() <= MAX_DOC_BYTES,
    "attestation document too large ({} > {} bytes)",
    doc.len(), MAX_DOC_BYTES,
);
```
guest 側にも同じ上限を `read_vec()` 後にチェックして panic させる (公開値に含めないので情報漏洩なし)。

---

### K7-08 [should-fix] `vkey` バイナリの出力に「いつ取った値か」「SP1 SDK バージョン」が含まれない

**場所**: `sp1-guests/attestation-aws-nitro/host/src/bin/vkey.rs:11-13`

```rust
let hash = vkey_hash().await?;
println!("0x{}", hex::encode(hash));
```

**問題**:
vkey は SP1 SDK のバージョンと guest source から決まる。SDK バージョンを上げると vkey は変わる (それで 6.2 移行時に再キャプチャしたはず)。stdout が hex 32 バイトしか出ないため、運用上「いつどのバージョンで取った値か」をオペレータが手でメモする運用になる。

**修正案 (書き直し)**:
stdout は機械可読のまま (現状) にし、stderr に metadata を出す:
```rust
eprintln!("# guest: title-sp1-attestation-aws-nitro-program {}", env!("CARGO_PKG_VERSION"));
eprintln!("# sp1-sdk: 6.2");
eprintln!("# captured: {}", chrono::Utc::now().to_rfc3339());
println!("0x{}", hex::encode(hash));
```
(`chrono` 依存追加が嫌なら `std::time::SystemTime::now()` で epoch 秒のみ)

---

### K7-09 [should-fix] `lib.rs` の `cpu_setup` を 2 回呼ぶ無駄

**場所**: `sp1-guests/attestation-aws-nitro/host/src/lib.rs:40-42, :61-62`

`vkey_hash()` と `generate_groth16_proof()` はそれぞれ `cpu_setup()` を呼ぶ。これは単独利用なら正しい。しかし、SDK の `setup()` は SP1 SDK 6.2 で重い (vkey 計算のため数十秒)。ユーザーが両方を順に呼ぶ統合シナリオ (vkey 確認 → prove) では二度実行になる。

現在の API シナリオでは vkey は OPERATIONS フローで先に取得 → Solana 側に焼き込み → 後日 prove のため、別プロセスで OK。should-fix どまり。

**修正案 (削除 or 書き直し)**:
ライブラリ API として `pub struct ProverHandle { ... }` を導入し、`ProverHandle::new() -> ProverHandle` で 1 回 setup し、`handle.vkey_hash()` / `handle.prove(...)` を生やす。bin 2 つは現状維持。

---

### K7-10 [nitpick] doc comment の「once per TEE instance」表現

**場所**: `sp1-guests/attestation-aws-nitro/program/src/main.rs:5`

> //! Spec §6.2 — Solana Extension preparation (once per TEE instance).

**問題**:
「once per TEE instance」は誤解を招く。`register_key` 自体は EC2 instance ライフサイクルに紐づくが、SP1 proof 生成自体は「key を on-chain に登録するときに 1 回」であり、TEE instance が再起動しても proof を作り直す必要は通常無い (key が同じなら再利用可能)。仕様 §6.2 とのリンクとしては正確だが、コード reader への補助情報としては微妙。

**修正案 (書き直し)**:
```rust
//! Spec §6.2 — runs once when a signer key is registered on-chain.
```

---

### K7-11 [nitpick] `_cert_chain` の prefix underscore

**場所**: `sp1-guests/attestation-aws-nitro/program/src/main.rs:49`

```rust
let _cert_chain = report.authenticate(0, doc.timestamp / 1000).expect(...);
```

**問題**:
`authenticate()` は副作用 (検証) のために呼んでいる。戻り値の `_cert_chain` を捨てるなら、`let _ = report.authenticate(...).expect(...);` の方が「結果を使っていない」意図がより明確。`_cert_chain` という名前付き変数があると「使うつもりだったが忘れた」読みになりうる。

**修正案 (書き直し)**:
```rust
report
    .authenticate(0, doc.timestamp / 1000)
    .expect("Attestation Document verification failed");
```
(末尾セミコロンで戻り値破棄。代入なし)

---

### K7-12 [nitpick] `host/Cargo.toml` で SP1 SDK のバージョンが minor 固定のみ

**場所**: `sp1-guests/attestation-aws-nitro/host/Cargo.toml:21, :28`, `program/Cargo.toml:12`

```toml
sp1-sdk = "6.2"
sp1-build = "6.2"
sp1-zkvm = "6.2"
```

**問題**:
Cargo は `"6.2"` を `>=6.2.0, <7.0.0` と解釈する。SP1 6.3.0 がリリースされると自動でアップデートされ、その瞬間に **vkey が変わる** (HashableKey の出力は SDK 内部実装に依存)。Solana 側に焼き込んだ vkey 定数と不一致になり、register_key が全部 reject される。

タスク 16 (再現性) の観点でも、`Cargo.lock` を commit しているならまだ救われるが、host crate は独立 workspace なので独自 lock を持つ。

**修正案 (書き直し)**:
全て `"=6.2.2"` に固定する (現在 cargo registry にあるバージョン):
```toml
sp1-sdk = "=6.2.2"
sp1-build = "=6.2.2"
sp1-zkvm = "=6.2.2"
```
ついでに `Cargo.lock` を 2 つの workspace 両方で commit する (現状未確認、要確認)。

---

### K7-13 [nitpick] README.md の OPERATIONS_JA リンクパスが誤り

**場所**: `sp1-guests/README.md:52`

```markdown
See [docs/v0.1.2/OPERATIONS_JA.md](../docs/v0.1.2/OPERATIONS_JA.md) §2.4 / §4
```

**問題**:
README の所在は `sp1-guests/README.md`。`../docs/v0.1.2/OPERATIONS_JA.md` は `docs/v0.1.2/OPERATIONS_JA.md` を指し、正しい。ただし、表示テキスト `docs/v0.1.2/OPERATIONS_JA.md` と相対リンク先が一致しており GitHub では機能する。確認した限り問題なし — **撤回: 軽微な指摘なし**。

ただし `OPERATIONS_JA.md` が実在するかは未検証 (担当範囲外)。存在しなければリンク切れ。**該当する場合は should-fix に格上げ**。

---

## 補遺: 構造的に良いと感じた点

これらは保持を推奨:
- 独立 workspace 構造 (`sp1-guests/README.md:7-21`) の説明が明快で、なぜメイン workspace から除外しているかが端的に書かれている。
- guest が `commit_slice` ではなく `commit(&doc.module_id)` で Borsh String を出す選択は、SP1 が自動で `<u32 len><bytes>` 形式にしてくれるため on-chain parser と整合する (`program/src/main.rs:54` ↔ `programs/title-whitelist/src/lib.rs:330-338`)。
- 可変長フィールド (`user_data`, `public_key`) を SHA-256 で固定長化する判断 (`program/Cargo.toml:18-19` のコメント) は、on-chain accounts のサイズ予測可能性を担保していて良い。
- `authenticate(0, ...)` の `0` をハードコードしてコメントで理由を残した判断 (`program/src/main.rs:27-30`) は妥当 — ただし K7-06 で物理的歯止めを推奨。
- host の async API 化 (SP1 SDK 6.2 への追随) は SDK の最新 trait シグネチャと整合している (`sp1-sdk-6.2.2/src/prover.rs:59, :62` を確認)。
- `include_elf!` マクロの使用、`bytes32_raw()` での vkey hash 取り出し、`groth16()` での proof モード指定は全て SP1 SDK 6.2 の正規 API で正しい。

---

## カウント

| 重大度 | 件数 | ID |
|---|---|---|
| must-fix | 3 | K7-01, K7-02, K7-03 |
| should-fix | 6 | K7-04, K7-05, K7-06, K7-07, K7-08, K7-09 |
| nitpick | 4 | K7-10, K7-11, K7-12, K7-13 |
| **合計** | **13** | |
