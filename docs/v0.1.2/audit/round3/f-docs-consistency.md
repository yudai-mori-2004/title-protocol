# F. ドキュメント整合性 — Round 3

## 概要

担当範囲: Round 1 / Round 2 と同じ — `docs/v0.1.2/SPECS_JA.md` / `COVERAGE.md` / `OPERATIONS_JA.md` / `docs/v0.1.2/tasks/**` / `docs/README.md` / ルート `README.md` / `CHANGELOG.md` / `deploy/aws/README.md` / `sp1-guests/README.md` / 主要ソースの doc comment。

監査方針: Round 2 の処理ログ（fixed / wontfix）を起点に、

- 「fixed」とされた項目が Round 3 時点でも実際に維持されているか（regression 検知）
- 「wontfix」とされた項目について、現状の文言と実装の乖離が新規の読み手にどう写るか
- Round 2 提出後に新規に書き換わった箇所で、新しい不整合が生じていないか

を、Source of Truth（仕様 vs 実装）を 1 行ずつ照合する形で確認した。

件数サマリ:

- Round 2 既決項目の維持確認: regression 0 / 維持 19
- Round 2 wontfix の現状再評価: 状況維持 11
- 新規発見: 4 件（must-fix: 1, should-fix: 2, nitpick: 1）

## 重大度別内訳（Round 3 新規）

- must-fix: 1 件
- should-fix: 2 件
- nitpick: 1 件

## Round 2 既決項目の維持確認

Round 2 で「fixed」とされた 19 件（must-fix-001/002/004/006/007、should-fix-001..004/007/008、nitpick-006）について、Round 3 時点でも対応が維持されていることを確認した。具体的に再点検した固定ポイント:

- `SPECS_JA.md:395 / 415 / 419` の「`encryption` は fragmented／sidecar では指定不可」注記 — 維持。
- `COVERAGE.md:86-87` の「sandbox tree removed post-verification」注記 — 維持。
- `README.md:147` の Status 文言「Core implementation complete; AWS Nitro verification ongoing.」 — 維持。
- `crates/tee/src/lib.rs:39` の doc comment 中 `"aws-nitro"`/`"mock"`、`crates/gateway/src/lib.rs:217` のテスト fixture、`SPECS_JA.md:660` の三点 — ハイフン版で揃っている。
- `COVERAGE.md:105` の program ID フル表記 `43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs` — 維持。`OPERATIONS_JA.md:92` と一致。
- `OPERATIONS_JA.md` §2.4 / §2.6 の placeholder 注記（`[0xAA; 32]` / `[0xBB; 48]` を「本番ローンチ前に必ず差し替える」） — 維持。
- タスク README の `SPECS_JA.md — 全文` 表記 — 維持。行数参照は再混入していない。
- `sp1-guests/README.md:52-53` のメモリ要件（30 GiB / 64 GiB） — Round 2 で nitpick-006 として fixed 認定された記述自体は維持。ただし他ファイルとの矛盾が新規発見 round3-new-002 として浮上した（後述）。

新規 regression は検出しなかった。

## Round 2 wontfix 項目の現状再評価

Round 2 の処理ログで wontfix（v0.1.3 OSS 公開前 doc 仕上げ等へ先送り）とされた項目について、Round 3 時点で読み手の混乱リスクを再評価した。原則として「wontfix の判断は妥当」と判定する。

### must-fix-003 タスク 14 GatewayAuth — wontfix 維持判定

- 現状: `docs/v0.1.2/tasks/14-gateway-tee-integration/README.md:36-40` で GatewayAuth が依然「やること」セクションに残り、`crates/gateway/src/tee_client.rs` の `HttpTeeClient` は素の `reqwest::Client` POST のまま。`grep -r GatewayAuth crates/` は 0 ヒット。
- 評価: README は「タスク開始時の作業計画」という性質を持つため、後付け削除には監査痕跡を消す副作用がある。一方で「TEE 側でリクエストの署名を検証」と読める文面は、信頼モデル（§1.7 で Gateway を trusted-but-not-secret と整理）と乖離している。v0.1.3 で「scope-out した旨」の 1 行注記を入れる予定なら維持で問題ないが、注記なしで OSS 公開を迎えると新参読者が確実に混乱する。

### must-fix-005 OPERATIONS §2.7 環境変数表 TEE 側欠落 — wontfix 維持判定

- 現状: §2.7 (L202-209) は依然 Gateway 系 6 変数のみ。`crates/tee/src/main.rs` を grep すると `TEE_RUNTIME` (L41)、`POOL_TOTAL_LIMIT` (L143)、`POOL_ADMISSION_LIMIT` (L147)、`PROXY_ADDR` (L161)、`TEE_BIND_ADDR` (L192) の 5 変数が runtime で参照される。
- 評価: 本文では §3 (L220) と §7 (L437) で `TEE_RUNTIME=mock|nitro` に触れているため致命傷ではない。だが `POOL_TOTAL_LIMIT` / `POOL_ADMISSION_LIMIT` / `PROXY_ADDR` の意味とデフォルト値は本文どこにも書かれておらず、本番でメモリ上限や proxy を切り替えたい運用者はコードを読まないと辿れない。v0.1.3 で §2.7 を「Gateway」「TEE」2 表化することを推奨。

### should-fix-009 タスク README の sandbox 参照 — wontfix 維持判定

- 現状: `tasks/01-sandbox-verification/README.md:17,170`、`tasks/03-c2pa-verify-processor/README.md:22`、`tasks/12-solana-extension/README.md:20,33,67` に `sandbox/` 参照が残っている。COVERAGE は「sandbox tree removed post-verification」と修正済み（must-fix-002）なので、COVERAGE とタスク README の方向は非対称。
- 評価: タスク README は歴史性のある作業記録であり後付け削除には抵抗がある、という Round 2 評価は妥当。一方、冒頭に「※検証完了後 sandbox は削除済。当時のコードは git 履歴を参照」と 1 行入れるだけで非対称性が解消するので、v0.1.3 で対応する場合は低コスト。

### round2-new-001 OPERATIONS §2.5 placeholder のイメージ名不一致 — wontfix 維持判定

- 現状: §2.5 (L166) は依然 `--docker-uri title-protocol-tee:latest`、`deploy/aws/scripts/build-images.sh` 経由の実運用名は `title-protocol-tee-nitro:latest` (`deploy/aws/README.md:94`)。
- 評価: §2.5 全体が「⚠️ プレースホルダー」ブロック内なので、コマンド文字列だけを切り出して使う読者リスクは限定的。ただし §2.5 を「`deploy/aws/README.md` を参照」の 1 段落に圧縮するだけで二重メンテが解消するので、v0.1.3 で対応する価値はある。

### round2-new-002 OPERATIONS §3 と `main.rs` のステップ順ずれ — wontfix 維持判定（ただし状況悪化）

- 現状: OPERATIONS §3 (L215-240) のフローは「1. ランタイム選択 → 2. KeyBundle → 3. Solana 署名鍵 → 4. Processor 登録 → 5. ResourcePool → 6. 自己 Attestation → 7. HTTP サーバー」の 7 ステップ。
- 一方、`crates/tee/src/main.rs:5-13` の module doc comment は「1. Select TeeRuntime / 2. Generate KeyBundle + Solana signing key / 3. Self-attest / 4. Capture the registration attestation / 5. Register processors and allocate ResourcePool / 6. Construct outbound content fetcher / 7. Start Axum HTTP server」の 7 ステップ。
- `main.rs` の Step ラベル本体は L37 (Step 1) / L85 (Step 2) / L92 (Step 3 Solana) / L102 (Step 3 Self-att) / L123 (Step 4 Registration) / L135 (Step 5) / L154 (Step 6) / L191 (Step 6 ← 重複) で、**Step 3 と Step 6 がそれぞれ 2 回登場**するという Round 2 時点の実装側ラベルの重複が残存している。
- 評価: OPERATIONS と実装の階層ずれ（registration attestation + outbound content fetcher の欠落）、および実装側のラベル重複の両方が未解消。これは新規読者の「TEE 起動時に何が走るか」の理解を確実に阻害する。v0.1.3 で OPERATIONS 側に 2 ステップ追加、`main.rs` のラベル直しを併せて入れるべき。

### should-fix-005/006/010, should-fix-011, nitpick-001..005 — 維持判定

各々 Round 2 の判定どおり、実害は軽微で v0.1.3 OSS 公開前 doc 仕上げで一括対応で問題ない。詳細追記は不要と判定。

## 新規発見（Round 3 で初めて検出）

### round3-new-001 SPECS §6.1 の `--features solana-ext` 記述が実装に存在しない — must-fix

- 場所: `docs/v0.1.2/SPECS_JA.md:1152` vs `crates/tee/Cargo.toml:14-27` / `crates/tee/src/server.rs:49` / `crates/gateway/src/endpoints.rs:144-181`。
- 観察:
  - SPECS §6.1「Extension の有効/無効」L1152 は「Extension の有効化は **TEE バイナリのビルド時点で固定される**。Solana Extension を有効化したビルドと無効化したビルドは別個の TEE バイナリであり、measurement も異なる。Gateway はその TEE バイナリの構成に応じて、対応する Extension エンドポイント（例: `POST /extension/solana`）の存在を判断し、未対応構成では 404 を返す。**実装側では `cargo build --features solana-ext` 相当のフラグで切り替える**。」と明言。
  - 一方、`crates/tee/Cargo.toml` の `[features]` セクションには `default = ["runtime-mock"]` / `runtime-mock` / `vendor-aws` の 3 つしか存在せず、**`solana-ext` という feature 名はワークスペース内に一切定義されていない**。
  - `title-solana` は `crates/tee/Cargo.toml:34` で **無条件依存**（`optional = true` ではない）として宣言。
  - `TeeAppState` (`crates/tee/src/server.rs:49`) の `signing_key: Arc<SolanaSigningKey>` も必須（`Option` ではない）。`/solana-keys` と `/extension/solana` のルートは `crates/tee/src/server.rs:92,94` で**無条件**に登録される。
  - Gateway 側の `/solana-keys` 404 (`crates/gateway/src/endpoints.rs:156`) はキャッシュに `solana_pubkey` が無いときのフォールバックで、SPECS が言う「TEE バイナリのビルド時点での切り替え」とは無関係。
- 問題: SPECS が宣言する build-time toggle が実装に存在しない。「Solana Extension を含まないビルドの measurement は別」という想定で whitelist 設計を読む読者は実装と整合しない理解に到達する。SPECS は実装が「Solana Extension は常時有効」であることを反映するか、または `solana-ext` feature を実装に追加するかの二択になる。
- 重大度: must-fix（Source of Truth 違反。仕様 vs 実装の二択を迫る性質）。
- 修正案:
  - (a) 仕様側を修正: §6.1 L1152 を「現行リリース（v0.1.2）では Solana Extension は常時有効。将来のリリースで build-time toggle を追加する場合は本節を更新する」と書き換える。COVERAGE.md にも「Solana Extension は常時有効。`solana-ext` 相当の feature は未実装」の旨を 1 行追記。
  - (b) 実装側を修正: `crates/tee/Cargo.toml` に `solana-ext` feature を追加し、`title-solana` を `optional = true` 化、`TeeAppState::signing_key` を `Option` 化、ルート登録を `#[cfg]` 化。— こちらは大規模リファクタなので v0.1.3 以降が現実的。
  - 推奨は (a)。

### round3-new-002 SP1 prover のメモリ要件が文書間で 8 GB / 30 GiB と食い違う — should-fix

- 場所: `docs/v0.1.2/OPERATIONS_JA.md:450` vs `sp1-guests/README.md:52-53` vs `deploy/aws/README.md:155-157`。
- 観察:
  - `OPERATIONS_JA.md:448-450` のトラブルシューティング「SP1 proof 生成が OOM で死ぬ」: 「prover はピーク 8 GB 程度メモリを使う。RAM 16 GB 以上のホストを推奨。」
  - `sp1-guests/README.md:52-53`: 「`prove` peaks at roughly 30 GiB resident memory during the Groth16 wrap. Use an instance with at least 64 GiB RAM (EC2 `r5.4xlarge` or larger).」
  - `deploy/aws/README.md:155-157` は「ローカルで」proof 生成するよう案内し、その上で `c5.xlarge`（8 GB RAM）を EC2 ホストとして指定。proof 生成のスペック自体は明示せず、暗黙に `sp1-guests/README.md` 側の数値を参照している構造。
- 問題: SP1 prover のメモリ要件が「8 GB / 16 GB host」と「30 GiB peak / 64 GiB host」と 4 倍以上乖離している。Round 2 認定済みの nitpick-006「メモリ要件追記」が `sp1-guests/README.md` 側で 30 GiB として確定したのに対し、`OPERATIONS_JA.md` 側の旧記述 8 GB が更新されておらず、両者が並立している。OPERATIONS を信じて 16 GB RAM のホストを用意した運用者は確実に OOM で proof 生成に失敗する。
- 重大度: should-fix（運用者の即時実害につながる）。
- 修正案: `OPERATIONS_JA.md:450` を「prover は Groth16 wrap でピーク約 30 GiB を要する。64 GiB RAM 以上のホスト（EC2 `r5.4xlarge` 以上）を推奨。詳細は `sp1-guests/README.md` 参照」に書き換える。

### round3-new-003 「三段の同一性確認」 vs COVERAGE / OPERATIONS の「four-step verification」 — should-fix

- 場所: `docs/v0.1.2/SPECS_JA.md:1189, 1196, 1200, 1202` vs `docs/v0.1.2/COVERAGE.md:105` vs `docs/v0.1.2/OPERATIONS_JA.md:270-275`。
- 観察:
  - SPECS §6.2 は「**三段の同一性確認**」(L1200) を冒頭に置き、**確認1: verifying_key_hash / 確認2: measurement / 確認3: 鍵と Attestation の bind 確認** の 3 段で構成する。
  - COVERAGE.md L105 は「Solana Extension: Whitelist PDA + **four-step** register_key verification」と書く。
  - OPERATIONS_JA.md §4 Step 3 (L270-275) は「オンチェーンで以下が順に確認される（Spec §6.2）: 1. `sp1_vkey_hash` が `ApprovedVkeys` に含まれる / 2. SP1 Groth16 proof が数学的に有効 / 3. `measurement` が `ApprovedMeasurements` に含まれる / 4. `user_data_hash == SHA-256(SHA-256(signing_pubkey))`」と 4 ステップで列挙。
- 問題: 実装（`programs/title-whitelist/`）は SPECS の 3 段に加えて「SP1 proof の数学的検証」を独立ステップとして持つ。SPECS はこれを「ZK proof の数学的検証に成功した場合のみ」（L1193）という前置きで暗黙化しているが、COVERAGE と OPERATIONS は 4 段として明示している。SPECS が Source of Truth として「3 段」と宣言しつつ、派生ドキュメントが「4 段」と書くと、新規読者は「どちらの内訳が正典か」を判断できない。
- 重大度: should-fix（Source of Truth 不一致だが、内容自体は両立可能なので運用上の即時実害は無い）。
- 修正案: SPECS §6.2 「三段の同一性確認」見出しを「四段の register_key 検証」に更新し、本文を「確認0: SP1 Groth16 proof の数学的検証 / 確認1: verifying_key_hash / 確認2: measurement / 確認3: 鍵と Attestation の bind 確認」と 4 段化する。または、COVERAGE / OPERATIONS 側を 3 段表現に揃える。前者の方が実装の挙動と直接対応するため推奨。

### round3-new-004 `crates/tee/src/main.rs` の `Step` ラベル重複 — nitpick

- 場所: `crates/tee/src/main.rs:92, 102, 154, 191`。
- 観察: `Step 3` が L92 (Solana signing key) と L102 (Self-attestation) で 2 回出現、`Step 6` が L154 (Outbound content fetcher) と L191 (Start Axum HTTP server) で 2 回出現。module doc comment (L5-13) では Step 7 までで整合しているが、本体側のインラインコメント番号が更新されていない。
- 問題: Round 2 の round2-new-002 で実装側ラベルの重複（旧 L190 が `Step 6`）として指摘されたが、Round 3 時点でも「Step 3 重複」「Step 6 重複（L154 と L191）」として残存。
- 重大度: nitpick（コメントの誤記、機能影響なし）。
- 修正案: L92 を `Step 3:`、L102 を `Step 4:`、L123 を `Step 5:`、L135 を `Step 6:`、L154 を `Step 7:`、L191 を `Step 8:` に振り直し、module doc comment も 8 ステップに更新する。あるいは module doc が 7 ステップ前提なら L92 を Step 2 の続き（コメント分離せず）、L102 を Step 3 にまとめる。

## 全体所感

Round 1 → Round 2 で 19 件が修正、Round 2 → Round 3 で regression は 0 件。Round 2 wontfix とされた 11 件は引き続き「v0.1.3 OSS 公開前の doc 仕上げ」での解消が現実解として有効。Round 3 で新たに浮上した 4 件のうち、特に注意を要するのは:

- **round3-new-001（must-fix）**: SPECS §6.1 が宣言する `--features solana-ext` build-time toggle は実装に存在しない。これは Round 1 / Round 2 では発見されていなかった素朴な仕様 vs 実装の乖離で、最低でも仕様側に「現行リリースでは常時有効」と注記する必要がある。Solana Extension の measurement 取り扱いをめぐる読者の理解に直接影響するため、OSS 公開前の対応必須。
- **round3-new-002（should-fix）**: SP1 prover メモリ要件が文書間で 8 GB / 30 GiB と乖離。OPERATIONS 側の数字を信じた運用者は OOM で確実に失敗する。30 GiB へ揃えるのは 1 行修正で済む。
- **round3-new-003（should-fix）**: SPECS の「三段」と COVERAGE / OPERATIONS の「four-step」の表現不一致。実装は 4 段で動いているため SPECS を 4 段に更新するのが望ましい。
- **round3-new-004（nitpick）**: `main.rs` の Step ラベル重複は Round 2 で半分指摘され、その後手付かず。機械的修正なので付随的に処理可。

Round 2 で「最大の懸念」とされた must-fix-003（GatewayAuth）、must-fix-005（環境変数表）、should-fix-009（sandbox 参照）は引き続き未着手で残るが、いずれも「読者の誤解を引き起こすが即時実害は無い」という Round 2 評価のまま。v0.1.3 の doc 仕上げセッションで一括して片付けるのが妥当。

---

## 処理ログ

| ID | 判定 |
|---|---|
| Round 2 fixed 19 件 | regression なし。維持確認のみ。 |
| Round 2 wontfix（must-fix-003/005、should-fix-005/006/009/010/011、nitpick-001..005、round2-new-001/002） | 状況維持。v0.1.3 OSS 公開前 doc 仕上げで一括対応の方針を継続。 |
| round3-new-001 | fixed | SPECS §6.1 L1162 の `--features solana-ext` 記述を「現行リリース (v0.1.2) では Solana Extension は常時有効。build-time toggle は将来リリースで導入」と書き換え。実装と整合。 |
| round3-new-002 | fixed | `OPERATIONS_JA.md:450` を「prover は Groth16 wrap でピーク約 30 GiB を要する。RAM 64 GiB 以上のホスト (EC2 r5.4xlarge 以上) を推奨」に更新。`sp1-guests/README.md` 参照リンクも付与。 |
| round3-new-003 | fixed | SPECS §6.2 の「三段の同一性確認」見出しを「四段の register_key 検証」に書き換え、ZK proof の数学的検証を確認 4 として明示。順序の根拠 (DoS 耐性のため安価チェック先行、Groth16 ペアリング最後) もコメント。COVERAGE / OPERATIONS の 4 段表現と整合。 |
| round3-new-004 | fixed | `crates/tee/src/main.rs` の Step ラベル重複を解消。Step 2 を encryption key bundle + Solana signing key の 2 つに merge、Step 3 = Self-attestation、以降 4/5/6/7 で整合。module doc comment と一致。 |
