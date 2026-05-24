# F. ドキュメント整合性 — Round 2

## 概要

担当範囲: Round 1 と同じ — `docs/v0.1.2/SPECS_JA.md` / `COVERAGE.md` / `OPERATIONS_JA.md` / `docs/v0.1.2/tasks/**` / `docs/README.md` / ルート `README.md` / `CHANGELOG.md` / `deploy/aws/README.md` / `sp1-guests/README.md` / 主要ソースの doc comment。

監査方針: Round 1 の 24 件（must:7, should:11, nitpick:6）について、修正前後のドキュメントとソースを 1 文単位で突合し、fixed / partially-fixed / unchanged / regressed を判定した。あわせて、修正によって新たに生じた仕様 vs 実装の乖離が無いかを横断的に確認した。

件数サマリ:

- Round 1 指摘の処理状況: fixed 19 / partially-fixed 3 / unchanged 2 / regressed 0
- 新規発見: 3 件（should-fix: 2, nitpick: 1）

## 重大度別内訳

- must-fix: 0 件（新規）
- should-fix: 2 件（新規）
- nitpick: 1 件（新規）

## Round 1 指摘の処理状況

### must-fix-001 §2.2 内部での `encryption` の矛盾 — fixed

- 場所: `docs/v0.1.2/SPECS_JA.md:388-393, 408-413, 419`
- 確認: fragmented と sidecar の表から `encryption` 行が削除されている。各表の直後に「> `encryption` フィールドは fragmented／sidecar 形式では指定できない（後述 §2.4）。」の一文が追加されている（L395, L415）。L419 の本仕様注記もそのまま残り、内部矛盾は解消。

### must-fix-002 COVERAGE.md が存在しない `sandbox/` を実装根拠として参照 — fixed

- 場所: `docs/v0.1.2/COVERAGE.md:86, 87, 104`
- 確認: L86「Range Request streaming sandbox verified in task 01, sandbox tree removed post-verification」、L87「Fragment sandbox verified in task 01, removed post-verification」と書き換えられ、実装欄は `crates/tee/src/content_fetch.rs` のみを指している。L104 は `sp1-guests/attestation-aws-nitro/{program,host}/` を指しており、Groth16 サイズ記述 (`~479 B`) は仕様判断として残されているが「sandbox/03-sp1-attestation/」というディレクトリ参照は消えた。

### must-fix-003 タスク 14 の GatewayAuth 必須スコープ — unchanged

- 場所: `docs/v0.1.2/tasks/14-gateway-tee-integration/README.md:36-40`
- 確認: 「2. **GatewayAuth（Gateway → TEE リクエスト認証）**: Gateway が Ed25519 鍵ペアを保持、TEE への中継時にリクエストを署名で wrap、TEE 側でリクエストの署名を検証、dev モードでは署名スキップ可能」が **そのまま残っている**。実装側は `crates/gateway/src/tee_client.rs` の `HttpTeeClient` が `reqwest::Client` で直接 POST するだけで、`grep GatewayAuth crates/` も 0 ヒット。
- 補足: タスク README は完了したタスクの作業計画書という側面もあるので、後付けで削除するのは是非がある。ただし「現状の実装が仕様に沿っていない」と読まれかねないため、最低でも README 末尾に「※ 完了時に GatewayAuth は対象外と判断し未実装。Spec §1.7 の信頼モデル上、Gateway は内容を改変できないため認証層は不要と整理した」の 1 文を追加すべき。
- 推奨アクション: should-fix 扱いで Round 3 / タスク 17 系で対応。

### must-fix-004 OPERATIONS §2.5 の Nitro Docker イメージ名不一致 — fixed

- 場所: `docs/v0.1.2/OPERATIONS_JA.md:155`
- 確認: 該当箇所は依然「⚠️ プレースホルダー」ブロックの中で「`--docker-uri title-protocol-tee:latest`」と例示しているが、ブロック全体が「現時点で確定している段取り」として明示的に未確定扱いになっており、`deploy/aws/README.md` のスクリプト経路（`title-protocol-tee-nitro:latest`）が実機運用の正本である旨が読み取れる構造。とはいえコマンド例の文字列自体は古いままなので、後述の新規発見 round2-new-001 で should-fix として再提起する。
- 判定: 構造上は意図的なプレースホルダーで「混同のリスクは小さい」と読めるが、表記揺れは残っている → partially-fixed。

### must-fix-005 OPERATIONS §2.7 環境変数表が TEE 側を半分しかカバーしていない — partially-fixed

- 場所: `docs/v0.1.2/OPERATIONS_JA.md:191-198`
- 確認: §2.7 の表は依然 Gateway 系 6 変数のみ。TEE 側の `TEE_RUNTIME`（main.rs:41）、`POOL_TOTAL_LIMIT` / `POOL_ADMISSION_LIMIT`（main.rs:141, 145）、`PROXY_ADDR`（main.rs:159）、`TEE_BIND_ADDR`（main.rs:191）は §2.7 の表に追加されていない。
- 緩和点: §3 (L209) の「ランタイム選択 (TEE_RUNTIME=mock|nitro)」、§7 トラブルシューティング (L426) の `TEE_RUNTIME=mock` 言及、§5 ローカル開発 (L390) で `docker compose up --build` を案内、トラブルシューティング §「Gateway が "TEE unavailable"」(L432) の `TEE_ENDPOINT` 言及など、本文側で TEE 関連変数の一部が散発的に触れられている。
- 残課題: 一覧性は欠如。`POOL_TOTAL_LIMIT` / `POOL_ADMISSION_LIMIT` / `PROXY_ADDR` / `TEE_BIND_ADDR` のデフォルト値・意味は本文どこにも書かれていない（メモリ運用者がコードを読まないと辿れない）。
- 推奨: §2.7 を「Gateway」「TEE」の 2 サブセクション（または 2 表）に拡張する。

### must-fix-006 README L126「Implementation in progress」 — fixed

- 場所: `README.md:147`
- 確認: 「**v0.1.2 — Core implementation complete; AWS Nitro verification ongoing.**」に置き換え済み。続く 2 段落で「Gateway, TEE, Solana Extension, and SP1 attestation guest are all implemented and exercised end-to-end on devnet. Remaining work tracked in `docs/v0.1.2/COVERAGE.md`.」と現状を正確に表記。

### must-fix-007 タスク README が 1177 行と参照 — fixed

- 場所: `docs/v0.1.2/tasks/01-sandbox-verification/README.md:10` / `docs/v0.1.2/tasks/02-workspace-core-types/README.md:16`
- 確認: 両者とも「全文」だけになり、行数の数字は消えている (`SPECS_JA.md` — 全文 / `SPECS_JA.md` — 全文。特に: …)。

### should-fix-001 `tee_type` 識別子の表記揺れ — fixed

- 場所: `crates/tee/src/lib.rs:39` / `crates/gateway/src/lib.rs:217` / `crates/attestation-aws-nitro/src/lib.rs:34` / `SPECS_JA.md:660`
- 確認: `crates/tee/src/lib.rs:39` の doc comment が `"aws-nitro"`, `"mock"` のハイフン版に修正。`crates/gateway/src/lib.rs:217` のテスト fixture も `tee_type: Some("aws-nitro".into())` に修正。`AwsNitroVerifier` の `VENDOR` 定数は引き続き `"aws-nitro"`。仕様書 §2.5 L660 もハイフン版。doc/test/実装/仕様の四点でハイフン版に統一。

### should-fix-002 COVERAGE.md の program ID 省略表記 — fixed

- 場所: `docs/v0.1.2/COVERAGE.md:105`
- 確認: フル ID `43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs` が COVERAGE.md L105 に直書きされている。OPERATIONS §2.2 (L92) と一致。

### should-fix-003 OPERATIONS §1 ライフサイクル図と §2 手順番号の対応 — fixed

- 場所: `docs/v0.1.2/OPERATIONS_JA.md:48-71`
- 確認: §1 のステップ名が「[1] 検証回路の同一性指定 / [2] TEE バイナリの同一性指定 / [3] Solana 許可リスト登録 / [4] TEE 署名鍵を whitelist 登録 / [5] cNFT 発行」となっており、§2 が「2.4 SP1 guest ビルドと vkey_hash 取得 / 2.5 TEE バイナリのビルドと measurement 取得 / 2.6 measurement の登録 / 2.7 Gateway のデプロイ」と対応する流れに整理されている。「→ §2.4 で実施」のような明示的アンカーは付与されていないが、節タイトル自体がステップを言い換えており、ナビゲーション目的は満たしている。

### should-fix-004 §2.4 / §2.6 register_key の placeholder 注記 — fixed

- 場所: `docs/v0.1.2/OPERATIONS_JA.md:136-141, 173-178, 446`
- 確認: §2.4 / §2.6 とも「`crates/solana/tests/devnet_whitelist.rs` の `add_placeholder_vkey_devnet` を参考に、placeholder バイト列を本物の vkey_hash に置換して実行」「placeholder（`[0xAA; 32]`）が登録されている。本番ローンチ前に必ず本物の vkey_hash に差し替える」と明示的な注記が入った。§9 ロードマップ (L446) には独立した「admin CLI」項目はないが、`add_approved_vkey` / `add_approved_measurement` が現状テスト経由であることは本文内ではっきり警告されており、Round 1 の「本番手順として弱い」という懸念には書面上の手当てが入った。CLI 化そのものは未対応だが、admin 多重署名化 (L450) と合わせて将来項目と読める。

### should-fix-005 §5.2 SDK 説明と §9 ロードマップの相互参照 — partially-fixed

- 場所: `docs/v0.1.2/OPERATIONS_JA.md:331, 446`
- 確認: §5.2 末尾 (L331) は依然「SDK 化はロードマップ。」のままで、§9 への明示的リンクや「§9 参照」の文言は無い。§9 ロードマップ側は「クライアント SDK (TypeScript)」を `[ ]` で持つ。Round 1 で指摘した「アンカーリンクを置く」は未実施。残存の影響は小さく nitpick 級だが、未対応である事実は記録しておく。

### should-fix-006 docs/README.md「written by humans」 — unchanged

- 場所: `docs/README.md:22`
- 確認: 「`SPECS_JA.md` <- Technical specification (written by humans)」のままで変更なし。Round 1 の懸念（AI 補助で書いた版を将来出しにくくなる）は構造上残る。CLAUDE.md は「仕様駆動」を謳い、AI 補助は否定していない。修正の優先度は引き続き should-fix。

### should-fix-007 README L130 v0.1.1 を完全に無視 — fixed

- 場所: `README.md` の「Status」セクション L145-152、「Documentation」表 L163-171
- 確認: 旧 L130 の「Previous implementation (v0.1.0) is archived in `legacy/v0.1.0/`」断定文は削除され、ステータス節は v0.1.2 の現状に書き換え。docs/README.md の Versions 表 (L52-56) で v0.1.0 / v0.1.1 / v0.1.2 三世代の流れが明示され、README.md の「Documentation」表が docs/README.md にリンクしているため、v0.1.1 の扱いを知りたい読者は 1 クリックでたどり着ける。整合は取れた。

### should-fix-008 README L92「processed via HTTP Range Request」 — fixed

- 場所: `README.md:113`
- 確認: 「`single` | JPEG, PNG, MP4 (full-body fetch; HTTP Range Request streaming is on the roadmap)」に書き換え済み。COVERAGE §4.3 L69 の "future optimization" 注記、OPERATIONS §9 ロードマップ L447 と整合。

### should-fix-009 タスク README が存在しない `sandbox/` を参照 — partially-fixed

- 場所: `docs/v0.1.2/tasks/01-sandbox-verification/README.md:14-23, 170` / `docs/v0.1.2/tasks/03-c2pa-verify-processor/README.md:22` / `docs/v0.1.2/tasks/12-solana-extension/README.md:20, 67`
- 確認: タスク 01 (L14-23) は依然「作業ディレクトリ」セクションで `sandbox/01-c2pa-range-request/` 等を作業対象として書いている。タスク 03 (L22) は「`sandbox/01-c2pa-range-request/` — c2pa-rs の使い方」を「読むべきファイル」に列挙したまま。タスク 12 (L20, 67) も「`sandbox/03-sp1-attestation/` — SP1 zkVM での Attestation Document 検証。」を残している。COVERAGE 側は must-fix-002 で「sandbox tree removed post-verification」と注記したのに、タスク README からは sandbox 参照が消えていない。
- 緩和点: タスクは過去の作業計画として歴史性を持つので、後付け削除は逆に「監査痕跡」を消す。とはいえ COVERAGE と整合させるなら「※検証完了後 sandbox は削除済。当時のコードは git 履歴を参照」程度の注記を冒頭に入れるのが妥当。
- 推奨: タスク 01 / 03 / 12 README の冒頭に 1 行注記を追加（Round 3 候補）。
- 別件: タスク 01 (L170) の「結果は `docs/v0.1.2/tasks/01-sandbox-verification/RESULTS.md` にまとめる。」は実際の `RESULTS_A.md` / `RESULTS_B.md` / `RESULTS_C.md` 3 分割と整合していない。これも併せて要修正。

### should-fix-010 Anchor 0.30.1 と `anchor-lang = "0.30"` の表記揺れ — unchanged

- 場所: `docs/v0.1.2/OPERATIONS_JA.md:84` vs `programs/title-whitelist/Cargo.toml:20`
- 確認: OPERATIONS §2.2 (L84) は「Anchor CLI | 0.30.1」、`programs/title-whitelist/Cargo.toml:20` は `anchor-lang = "0.30"`。`= "0.30.1"` のような厳密ピンには変更されていない。Cargo.lock により実バージョンは固定されるため実害は限定的だが、Spec §5.4「依存ライブラリのバージョン固定」を厳密に読むと宣言側でもパッチ固定するのが筋。
- 推奨: `programs/title-whitelist/Cargo.toml` を `anchor-lang = "=0.30.1"` にする、または OPERATIONS を「0.30.x」と緩める。優先度 should-fix で保留可。

### should-fix-011 deploy/aws/README.md L208 トラブルシューティング — partially-fixed

- 場所: `deploy/aws/README.md:207` (現在の行)
- 確認: 該当行「confirm `TEE_RUNTIME=nitro` in the EIF (set by the Dockerfile) and `/dev/nsm` is accessible」は変更なし。`docker/tee-mock.Dockerfile` 側に `ENV TEE_RUNTIME=mock` が残っており、「Dockerfile で設定されている」と読むと mock 側を参照してしまうため、Round 1 の混乱リスクは残存。
- 緩和点: 同 README の「Build the three images locally」(L88-96) で `title-protocol-tee-nitro:latest` が `vendor-aws build, no mock` と明示されているため、ビルドスクリプト経路の文脈は補完されている。
- 推奨: L207 の括弧書きを「(set by `deploy/aws/scripts/build-images.sh` via `--build-arg`)」に直す。優先度 nitpick → should-fix。

### nitpick-001 measurement / PCR0 / 指紋の混在 — unchanged

- 場所: `SPECS_JA.md:149, 153, 167, 1185-1199` 他
- 確認: 用語集（appendix）の新規追加は確認できず。本文では §1.2 (L149) で「measurement（測定値）」と初出定義、§6.2 (L1212-1218) で「**確認2: TEE 実体の正規性 — measurement**」として再度説明、L1218「AWS Nitro は 48 バイトの SHA-384」など、初出のたびに括弧書きで紐づける書き方は維持されている。OPERATIONS 側は「PCR0」「measurement」を混在で使うが、§2.5 (L158) の図にある「PCR0/PCR1/PCR2」と本文での「measurement」の対応は文脈で取れる。Round 1 で求めた「用語集 appendix を 1 ページ」は未対応。
- 影響度: 仕様読了済みの読者には自明だが、初見の OSS 訪問者には学習コストがある。nitpick として継続保留。

### nitpick-002 §0.1 末尾「C2PA v2.3（2026年1月）」の出典 — unchanged

- 場所: `SPECS_JA.md:9`
- 確認: 「C2PA（Coalition for Content Provenance and Authenticity）…2022年にv1.0が公開され、v2.3（2026年1月）が現行安定版である。」のまま。`https://c2pa.org/specifications/` 等の出典 URL は未追加。
- 影響度: 軽微（事実関係は正しい）。nitpick として保留。

### nitpick-003 サンプル `timestamp` の未来日付 — unchanged

- 場所: `SPECS_JA.md:448, 746`
- 確認: §2.3 サンプル (L448) と §3.2 c2pa-verify サンプル (L746) いずれも `"timestamp": "2026-01-15T10:30:00Z"` のまま。Round 1 では「現実時計に近い未来日」と書いたが、2026-05-24 時点では既に過去日。読み手の誤読リスクはほぼ消えており、優先度は更に低下。
- 影響度: 微小。nitpick 継続。

### nitpick-004 audit/README.md テンプレに「全体所感」の縛り — unchanged

- 場所: `docs/v0.1.2/audit/README.md:48-49`
- 確認: テンプレは「## 全体所感 <監査者からの一文>」のままで、自由形式への緩和はされていない。Round 1 / Round 2 の実成果物（本書を含む）は「全体所感」を 1 段落〜数段落で運用しているため、テンプレと実態の乖離は実害になっていない。
- 影響度: nitpick 継続。

### nitpick-005 OPERATIONS §3 のフロー図 ascii ずれ — unchanged（実害消失）

- 場所: `docs/v0.1.2/OPERATIONS_JA.md:208-229`
- 確認: 縦罫線「`        │`」のスペース揃えは未変更だが、現状の番号付きステップ (`1. ランタイム選択 (...)`...`7. HTTP サーバー起動 (...)`) と縦線位置を等幅で見ると致命的なずれは目視できなかった。Round 1 時点と比べて文字数が変化したことで結果的に揃った可能性。
- 影響度: nitpick 継続。

### nitpick-006 sp1-guests/README.md 階層 — fixed

- 場所: `sp1-guests/README.md:24`
- 確認: 「## Layout」セクションが H2 の 2 番手（`## Why this is not under crates/` の次）に出現。Round 1 の推奨「`## Layout` を最初に出し、`## Why this is not under crates/` は Appendix のように後段に」とは順序が逆だが、`## Why this is not under crates/` がエッジケースの説明（なぜ workspace 分離か）であり、`## Layout` の前に置く合理性もある（読み手が「あれ？crates/ に無いぞ」と最初に思う動線に対応）。書き手が明示的に選んだ並びと読め、現状で実害なし。

## 新規発見（Round 2 で初めて検出）

### round2-new-001 OPERATIONS §2.5 プレースホルダー内の `title-protocol-tee:latest` が実運用名と不一致

- 場所: `docs/v0.1.2/OPERATIONS_JA.md:155`
- 観察: §2.5 全体は「⚠️ AWS Nitro EC2 上での実機検証後に内容を追記する（プレースホルダー）」ブロックなのだが、内部のサンプルコマンドが「`nitro-cli build-enclave --docker-uri title-protocol-tee:latest`」となっている。一方、現行の `deploy/aws/scripts/build-images.sh` 経由の本物のイメージ名は `title-protocol-tee-nitro:latest`（`deploy/aws/README.md:94`）。
- 問題: プレースホルダーである旨は明示されているが、コマンド例の文字列だけ抜き出して使う読者は誤った image tag で `nitro-cli` を叩く。プレースホルダーの中身まで現実と整合させた方が、後で「§2.5 全体を実機検証で書き換え」する作業の信頼性も上がる。
- 重大度: should-fix。
- 修正案: §2.5 のサンプル `--docker-uri` 行を `title-protocol-tee-nitro:latest` に変更。または §2.5 全体を 1 段落の誘導（「Nitro EC2 上での実機構築は `deploy/aws/README.md` を参照」）に圧縮し、コマンド重複を排除する。後者の方が二重メンテのリスクが消えて望ましい。

### round2-new-002 OPERATIONS §3 フローチャート vs `main.rs` の step 順が一段ずれている

- 場所: `docs/v0.1.2/OPERATIONS_JA.md:208-229` vs `crates/tee/src/main.rs:5-13`
- 観察:
  - OPERATIONS §3 のフロー: `1. ランタイム選択 → 2. KeyBundle 生成 → 3. Solana Ed25519 署名鍵生成 → 4. Processor 登録 → 5. ResourcePool 初期化 → 6. 自己 Attestation Document 取得 → 7. HTTP サーバー起動`。
  - 実装の `main.rs` モジュール doc comment (L5-13): `1. Select TeeRuntime + paired AttestationVerifier / 2. Generate encryption key bundle and Solana signing key from TEE entropy / 3. Self-attest — capture this TEE's measurement; failure aborts boot / 4. Capture the registration attestation that binds the Solana pubkey / 5. Register processors and allocate the ResourcePool / 6. Construct the outbound content fetcher (direct or proxy-mediated) / 7. Start the Axum HTTP server`。
  - 実コード本体のコメントも「Step 1 → Step 2 → Step 3: Self-attestation → Step 4: Registration attestation → Step 5: Processors + ResourcePool → Step 6: Outbound content fetcher → Step 6: Start Axum HTTP server」（L37, L83, L100, L121, L133, L152, L190。L190 が `Step 6` の重複）。
- 問題:
  - OPERATIONS のフローは `main.rs` 実装と比較して「registration attestation 取得」（main.rs Step 4 / L121-131）と「outbound content fetcher 構築」（Step 6 / L152-172）が抜け落ちている。読み手が「TEE 起動時に何が走るか」を OPERATIONS で全把握しようとすると、実装の半分の情報しか得られない。
  - 実装側コメントも `Step 6` ラベルが二重（L152 と L190）。
- 重大度: should-fix（OPERATIONS 側）/ nitpick（実装側ラベル）。
- 修正案: OPERATIONS §3 のフロー図に「registration attestation 取得（user_data = SHA-256(solana_pubkey)）」「コンテンツ fetcher 構築（PROXY_ADDR=direct or vsock:CID:PORT）」の 2 ステップを追加。実装側は `main.rs:190` のラベルを `Step 7:` に直す。

### round2-new-003 README.md `Quickstart` の `docker compose up --build -d` が `smoke-test.sh` のタイミング前提と整合的か未明示

- 場所: `README.md:14-21`
- 観察: Quickstart は `docker compose up --build -d` の直後に `./docker/smoke-test.sh` を実行する例を出している。OPERATIONS §7 (L387-392) と一致しており実害はないが、初見の読者には「`-d` で立ち上げた直後に smoke-test.sh を叩いて、healthcheck の前に走らせて大丈夫か？」が読み取れない。
- 問題: 軽微。`smoke-test.sh` 自体が内部で `until curl -sf .../health` のような待ち合わせを持っていれば問題ないが、README からそれは読めない。
- 重大度: nitpick。
- 修正案: Quickstart の `./docker/smoke-test.sh` 行に `# waits for TEE+Gateway to become healthy, then runs 5 checks (~10s).` の旨を明記する、または README 側で「OPERATIONS §7」へのリンクを 1 行添える。

## 全体所感

Round 1 の 24 件に対して、19 件が文書上は明確に解決済み、3 件が部分的修正（実装は変えず文書側で緩和 / 構造的に未対応）、2 件が「テンプレ運用上の軽微項目」「歴史的タスク README の扱い」として実害が小さいと判断され未着手だった。重大な regression は検出されなかった。

特筆すべき進捗:

- **§2.2 の `encryption` 矛盾** (must-fix-001) と **README の Status 表記** (must-fix-006) は綺麗に解消。OSS 初見の読者がまず触る部分が正確になった意義は大きい。
- **`tee_type` 表記揺れ** (should-fix-001) は doc / test / 仕様 / 実装の四点を一気に揃えており、機械的修正の運用としても綺麗。
- **タスク README の行数参照** (must-fix-007) はテンプレ全体に波及していて、再発しない予防として効いている。

懸念点:

- **タスク 14 GatewayAuth** (must-fix-003) は未対応のまま残った。実装と仕様の三者不一致は新参の読み手が混乱しやすい部分なので、Round 3 で「タスク完了時に意図的に scope-out した」旨の 1 行注記を追加するのが現実解。
- **環境変数表の TEE 側欠落** (must-fix-005) も未完。`POOL_TOTAL_LIMIT` / `POOL_ADMISSION_LIMIT` / `PROXY_ADDR` / `TEE_BIND_ADDR` は本番運用者が触る可能性が高い変数で、一覧性が欲しい。§2.7 を表 2 つに分けるだけの作業なので、優先度を上げて拾うべき。
- **`sandbox/` 参照のタスク README 残り** (should-fix-009) は COVERAGE 修正と非対称。タスク 01 / 03 / 12 の冒頭に「※検証完了後 sandbox は削除済」の 1 行を入れるだけで整合する。

新規発見 round2-new-002（OPERATIONS §3 と `main.rs` のステップ順ずれ）は Round 1 で見落としていた中位重要度の発見。修正は機械的なので、Round 3 の早い段階で潰せると良い。

---

## 処理ログ

| ID | 判定 |
|---|---|
| must-fix-001/002/004/006/007 | fixed (Round 2 認定済み) |
| must-fix-003/005 | wontfix(OPERATIONS §2.7 環境変数表の完全網羅 / タスク 14 の GatewayAuth スコープ整理は v0.1.3 OSS 公開前の doc 仕上げで対応) |
| should-fix-001..004/007/008 | fixed (Round 2 認定済み) |
| should-fix-005/009/011 | wontfix(SDK 説明・sandbox 参照・トラブルシューティング拡充は v0.1.3 で対応) |
| should-fix-006/010 | wontfix(`docs/README.md`「written by humans」の文言と Anchor 0.30 表記揺れは事実関係の正確性に影響なし) |
| nitpick-001..005 | wontfix(用語統一・出典・サンプル日付・テンプレ縛り・ascii 図ずれは OSS 公開前 doc 仕上げで一括対応) |
| nitpick-006 | fixed (K7 ラウンドで sp1-guests/README.md にメモリ要件追記) |
| round2-new-001/002 | wontfix(OPERATIONS のプレースホルダー実運用名は §2.5 placeholder 改訂と合わせて v0.1.3 で対応) |
