# F. ドキュメント整合性

## 概要

担当範囲: `docs/v0.1.2/SPECS_JA.md` / `COVERAGE.md` / `OPERATIONS_JA.md` / `docs/v0.1.2/tasks/**` / `docs/README.md` / ルート `README.md` / `CHANGELOG.md` / `deploy/aws/README.md` / `sp1-guests/README.md` / 主要ソースの doc comment。

監査方針: SPECS_JA を全文走査して、(a) 実装コードに該当 fn / 型 / コマンドが存在するか、(b) OPERATIONS / README に書かれた手順が現状の workspace で実行可能か、(c) COVERAGE の `[x]` が嘘ではないか、(d) ドキュメント間で同じ事実が同じ表現で書かれているか、を 1 文ずつ確認した。

件数サマリ: 24 件（must-fix: 7, should-fix: 11, nitpick: 6）。

## 重大度別内訳

- must-fix: 7 件
- should-fix: 11 件
- nitpick: 6 件

## 発見

### must-fix-001 仕様 §2.2 と §2.2 末尾で `encryption` の対応形式が矛盾している
- 場所: `docs/v0.1.2/SPECS_JA.md:390` および `docs/v0.1.2/SPECS_JA.md:409` vs `docs/v0.1.2/SPECS_JA.md:413`
- 観察: 各入力形式の表で「`encryption` | No | 暗号化スイート名（後述）」が `fragmented` (L390) と `sidecar` (L409) にも記載されている。一方 L413 に「本仕様では `input_type: "single"` に限り暗号化に対応する。fragmented / sidecar 形式での暗号化は将来の拡張とする」と明記。
- 問題: 実装者・読み手が「fragmented でも encryption が使える」と誤解するため、§2.2 内部で読み合わせれば矛盾が必ず発生する。実装 (`crates/core/src/request.rs`) では `EncryptionSuite` が `InputData` の全 variant に乗っているため、コードを書いた人がどう判定するか不明瞭。
- 修正案: fragmented と sidecar の表から `encryption` 行を削除し、表の直後（または冒頭）に「fragmented / sidecar には `encryption` フィールドを指定できない（将来拡張）」と一文加える。L413 のテキストはそのまま残す。

### must-fix-002 COVERAGE.md が存在しない `sandbox/` ディレクトリを実装根拠として参照している
- 場所: `docs/v0.1.2/COVERAGE.md:86,87,104`
- 観察: 例えば「sandbox/01-c2pa-range-request/ (Range Request sandbox)」「sandbox/02-c2pa-fragment/」「sandbox/03-sp1-attestation/ (... Groth16 ~479B fits Solana 1,232B ...)」が `[x]` の Implementation 欄に書かれている。
- 問題: workspace 直下に `sandbox/` ディレクトリは存在しない (`ls` で確認済み)。実装の所在として書かれているリンクが指す先がない。新参の検証者が `sandbox/03-sp1-attestation/` を読みに行くと面食らう。
- 修正案: 検証フェーズで作って後に削除したサンドボックスは「[~] sandbox 検証は実施済み（リポジトリ外で完了）」と注記し、Implementation 欄からは削除する。本実装側（`sp1-guests/attestation-aws-nitro/`、`crates/tee/src/content_fetch.rs`）のみを参照する。

### must-fix-003 タスク 14 の必須スコープ「GatewayAuth (Gateway→TEE 署名)」が実装されていない & 仕様書にも書かれていない
- 場所: `docs/v0.1.2/tasks/14-gateway-tee-integration/README.md:36-40` および対応する実装欠落 (`crates/gateway/src/` 配下に `auth.rs` の Gateway→TEE 署名はなし、`grep GatewayAuth` 0 ヒット)。
- 観察: タスク README が「GatewayAuth: Gateway が Ed25519 鍵ペアを保持、TEE への中継時にリクエストを署名で wrap、TEE 側で検証」を必須として明記。しかし SPECS_JA は「Gateway は薄い管理層」「内容の改変はできない」としか書いておらず Gateway→TEE 認証の話は一切ない。実装も `HttpTeeClient` (`crates/gateway/src/tee_client.rs`) がそのまま reqwest で POST しているだけ。
- 問題: タスクと仕様と実装の三者が不一致。最終仕様としては「実装しなかった = 不要と判断した」と読めるが、タスクが残っている限り「実装漏れ」とも読める。
- 修正案: タスク 14 README に「GatewayAuth は v0.1.2 では実装しない（SPECS_JA §1.7 の信頼モデル上、Gateway は内容を改変できないため認証層は不要と判断）」を追記して該当節を削除、もしくは Spec 側を強化して必要なら実装する。後者ならスコープ拡大なので前者推奨。

### must-fix-004 OPERATIONS §2.5 の Nitro Docker イメージ名が `deploy/aws/scripts/build-images.sh` の出力と不一致
- 場所: `docs/v0.1.2/OPERATIONS_JA.md:155`
- 観察: 「`nitro-cli build-enclave --docker-uri title-protocol-tee:latest ...`」と書かれている。一方 `deploy/aws/README.md:95` では「`title-protocol-tee-nitro:latest` — base for the EIF (vendor-aws build, no mock)」となっており、実際の build スクリプトは `-nitro` サフィックス付きを出力する。
- 問題: 手順通りに叩くと `nitro-cli` が「no such image: title-protocol-tee:latest」で落ちる。実機で必ず詰まる。
- 修正案: OPERATIONS の例を `title-protocol-tee-nitro:latest` に直す。あるいは `> ⚠️ この章は AWS Nitro EC2 上での実機検証後に内容を追記する` ブロックなので削除して `deploy/aws/README.md` を見るよう一行で誘導する方が安全（重複定義を避ける）。

### must-fix-005 OPERATIONS §2.7 環境変数表が実装の半分しかカバーしていない
- 場所: `docs/v0.1.2/OPERATIONS_JA.md:191-198`
- 観察: 表に書かれているのは `TEE_ENDPOINT / API_KEYS / RATE_LIMIT_MAX / RATE_LIMIT_WINDOW_SECS / HEALTH_CHECK_INTERVAL_SECS / GATEWAY_BIND_ADDR` の 6 個。一方、`crates/tee/src/main.rs` を読むと TEE 側にも `TEE_RUNTIME / POOL_TOTAL_LIMIT / POOL_ADMISSION_LIMIT / PROXY_ADDR / TEE_BIND_ADDR` がある（L40, L113-123, L134, L189）。これらが表に一切ない。
- 問題: TEE のメモリ制限・プロキシ設定をいじりたい運用者がコードを grep しないと辿り着けない。本書は「運用者の手引き」を謳っているので致命的。
- 修正案: §2.7 の表を Gateway / TEE の 2 表に分け、TEE 側に上記 5 環境変数を追加。各エントリの default は `crates/tee/src/main.rs` の値そのもの (`mock` / `512 MB` / `total * 3/4` / `direct` / `0.0.0.0:4000`) を記載。

### must-fix-006 README.md L126 「Implementation in progress」がもはや事実と乖離している
- 場所: `/Users/forest/WebCreations/title-protocol/README.md:126`
- 観察: 「**v0.1.2 — Implementation in progress.**」と書かれているが、COVERAGE は §1, §2, §4, §5, §6 のコア項目がほぼ `[x]`、未着手は §3 の追加 processor（provenance-graph / image-pdq / video-vpdq / cert-*）と入力形式関連の `[~]` のみ。タスク 16 (audit) の理由付けにも「クローンした人が困らない状態である」を担保するとあり、フェーズはコード品質チューニング。
- 問題: 初見の OSS 訪問者に「まだ何も動かない」印象を与える。実際は Gateway+TEE が docker compose で動き、devnet にコントラクトが上がっており、AWS Nitro 実機での疎通も済んでいる。
- 修正案: 「**v0.1.2 — Core implementation complete, AWS Nitro verification in progress.**」など、現状（コア完了 / processor 追加と本番運用は段階的）を反映した一文に置き換える。COVERAGE の `[~]` 残項目を一覧で README に持ち出すのも可。

### must-fix-007 タスク README が古い仕様書の行数（1177 行）を参照している
- 場所: `docs/v0.1.2/tasks/01-sandbox-verification/README.md:10` と `docs/v0.1.2/tasks/02-workspace-core-types/README.md:16`
- 観察: 両方とも「`docs/v0.1.2/SPECS_JA.md` — 全文（1177行）」と書く。現在の SPECS_JA は 1325 行（`wc -l` 確認）。
- 問題: 行数で参照させる設計自体がフラジャイル（仕様改訂で必ず古くなる）。読み手は「自分が見てる版が古い？それとも仕様が変わった？」と混乱する。
- 修正案: 行数を消して「全文を最初に読む」だけにする（タスク 16 の README はそうしている）。テンプレを揃える。

### should-fix-001 doc comment が示す `tee_type` 識別子の表記揺れ（ハイフン vs アンダースコア）
- 場所: `crates/tee/src/lib.rs:73` vs `crates/attestation-aws-nitro/src/lib.rs:43` vs `docs/v0.1.2/SPECS_JA.md:652`
- 観察:
  - SPECS_JA: 「`"aws-nitro"`, `"amd-sev"`, `"mock"` 等」（ハイフン）
  - `crates/tee/src/lib.rs:73` の doc comment: 「`"aws_nitro"`, `"amd_sev_snp"`, `"intel_tdx"`, `"mock"`」（アンダースコア）
  - 実コード: `VENDOR: &str = "aws-nitro"` （ハイフン、実体）
  - `crates/gateway/src/lib.rs:212` のテスト fixture: `tee_type: Some("aws_nitro".into())` （アンダースコア、テストデータ）
- 問題: doc comment と実装で識別子表記が違う。テストデータの fixture もハイフン版で書くべき。検証者が照合ロジックを書くとき、どっちが正なのか毎回確認する必要がある。
- 修正案: 仕様書がハイフン版で確定しているので、(a) `crates/tee/src/lib.rs:73` の doc comment 例をハイフン版に直す、(b) `crates/gateway/src/lib.rs:212` のテスト文字列を `"aws-nitro"` に直す。仕様書側の `amd-sev` も AWS との一貫性で `amd-sev-snp` に揃えるかは仕様判断（このフェーズでは触らない方が安全）。

### should-fix-002 COVERAGE.md が「devnet 既存 program ID」を 1 か所しか書かず冗長な省略表記をしている
- 場所: `docs/v0.1.2/COVERAGE.md:105`
- 観察: 「devnet redeployed 43y8E...」と省略形。実 ID は `43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs`。OPERATIONS_JA §2.2 と Anchor.toml には完全な ID が書いてある。
- 問題: COVERAGE の実装欄を grep して program ID を引きたい読者が引けない。
- 修正案: 完全な ID に置換、もしくは「devnet (program ID: see OPERATIONS_JA §2.2)」と参照に変える。

### should-fix-003 OPERATIONS §1 のライフサイクル図と §2 の手順番号が対応していない
- 場所: `docs/v0.1.2/OPERATIONS_JA.md:43-71` (§1) と `docs/v0.1.2/OPERATIONS_JA.md:77-201` (§2)
- 観察: §1 は `[1] 検証回路の同一性指定 / [2] TEE バイナリの同一性指定 / [3] Solana 許可リスト登録 / [4] TEE 署名鍵を whitelist 登録 / [5] cNFT 発行` という 5 ステップ。§2 は `2.1 前提 / 2.2 Solana コントラクトのデプロイ / 2.3 許可レジストリの初期化 / 2.4 SP1 guest ビルドと vkey_hash 取得 / 2.5 TEE バイナリのビルドと measurement 取得 / 2.6 measurement の登録 / 2.7 Gateway のデプロイ`。§1 の `[1]` は §2.4、`[2]` は §2.5、`[3]` は §2.6（vkey は §2.4 で個別に書かれているがそれは §1 の `[1]` と `[3]` の vkey 部分の融合）と読みづらい。
- 問題: 図で見た番号と実際の節番号の対応がパッと取れない。本番運用者が「step 3 を再実行したい」と思ったとき、どの節を読めばいいか迷う。
- 修正案: §1 の各ボックスに「→ §2.4 で実施」のような明示的なナビゲーションを追加する。

### should-fix-004 OPERATIONS §2.4 / §2.6 の `register_key` 説明が「テストの placeholder バイトを置換して実行」と書かれており本番手順として弱い
- 場所: `docs/v0.1.2/OPERATIONS_JA.md:137-141, 173-178`
- 観察: vkey 登録は「`crates/solana/tests/devnet_whitelist.rs` の `add_placeholder_vkey_devnet` を参考に、placeholder バイト列を本物の vkey_hash に置換して実行」と書く。measurement 登録も同様。
- 問題: 本番運用で「テストファイルを編集して走らせろ」は本来 CLI 化されているべき手順。誰がやっても再現可能なコマンド or バイナリを示すか、最低でも「本番手順は未整備（タスク 17 以降）」と注記する必要がある。
- 修正案: §9 ロードマップに「admin CLI（`add_approved_vkey` / `add_approved_measurement` を引数で受け取る）」を追加し、現状節には「現状はテストコード経由。CLI 化はロードマップ参照」と明記。

### should-fix-005 OPERATIONS §5.2「現状クライアント SDK は提供していない」が SDK 化ロードマップ §9 とリンクしていない
- 場所: `docs/v0.1.2/OPERATIONS_JA.md:331` vs `docs/v0.1.2/OPERATIONS_JA.md:447`
- 観察: §5.2 末尾「SDK 化はロードマップ。」、§9 では「クライアント SDK (TypeScript)」が `[ ]`。
- 問題: 同じ事実が 2 か所に書かれているが互いに参照していない。読み手は「いつ SDK 出るの？」とジャンプしたいときにスクロールを強いられる。
- 修正案: §5.2 末尾を「SDK 化はロードマップ（§9）。」または直接アンカーリンクに置き換え。

### should-fix-006 docs/README.md にある「(written by humans)」コメントが実態と微妙にずれる
- 場所: `docs/README.md:23`
- 観察: 「`SPECS_JA.md` — Technical specification (written by humans)」。
- 問題: CLAUDE.md には「仕様駆動」「仕様書 = Source of Truth」とあり、仕様書を AI が書くか人間が書くかは別軸。「written by humans」と断定すると、AI 補助で書いた版を将来出しにくくなる（嘘になる）。
- 修正案: 「Technical specification (canonical source of truth)」など、「誰が書いたか」ではなく「役割」を表す説明に変える。

### should-fix-007 README.md L130「Previous implementation (v0.1.0) is archived」が v0.1.1 を完全に無視
- 場所: `README.md:130`
- 観察: 「Previous implementation (v0.1.0) is archived in `legacy/v0.1.0/`」と書いてあるが、`docs/v0.1.1/` も存在し、`docs/README.md` の versions 表には v0.1.0 / v0.1.1 / v0.1.2 が並んでいる。実コードは v0.1.0 だけが `legacy/` 配下にあり、v0.1.1 はドキュメントのみ（実装はそのまま v0.1.2 に発展？）。
- 問題: 初見の人が「v0.1.1 は？」と混乱する。
- 修正案: 「Previous implementations: v0.1.0 source code is archived in `legacy/v0.1.0/`; v0.1.1 was a docs-only iteration and its specs remain in `docs/v0.1.1/`.」のような 1 文に置き換え。

### should-fix-008 README L92「single | JPEG, PNG, MP4 — large files processed via HTTP Range Request」が実装と乖離
- 場所: `README.md:92`
- 観察: 表に「large files processed via HTTP Range Request」と断言。一方 `crates/tee/src/content_fetch.rs:27, 141, 147` のコメントは Range Request を「future optimization」「streaming Range Request with shrink is future optimization; current impl fetches full file」と説明。COVERAGE.md L69 も同じ注記、OPERATIONS §9 にも「Range Request 対応の大容量コンテンツ fetch」が `[ ]` ロードマップとして残っている。
- 問題: README だけが「対応済」と読める。
- 修正案: 「JPEG, PNG, MP4 — large files (Range Request optimization on roadmap)」のような断り書きに変える。

### should-fix-009 タスク README が `sandbox/` 配下のディレクトリを参照しているが存在しない
- 場所: `docs/v0.1.2/tasks/01-sandbox-verification/README.md:14-23, 170` および `docs/v0.1.2/tasks/03-c2pa-verify-processor/README.md:22` および `docs/v0.1.2/tasks/12-solana-extension/README.md:20, 67`
- 観察: 「`sandbox/01-c2pa-range-request/`」「`sandbox/03-sp1-attestation/`」等を「読むべきファイル」「依存」として列挙。`RESULTS.md` も「`docs/v0.1.2/tasks/01-sandbox-verification/RESULTS.md` にまとめる」と書いているが実際は `RESULTS_A.md / RESULTS_B.md / RESULTS_C.md` の 3 ファイル分割。
- 問題: タスクの完了報告ファイル名がドキュメントとずれている。
- 修正案: タスク README 冒頭に「※検証完了後 sandbox は削除済。RESULTS は分割版 (`RESULTS_A.md` / `RESULTS_B.md` / `RESULTS_C.md`) を参照」と注記。または must-fix-002 と合わせて「sandbox は外部リポで完了」扱いに統一。

### should-fix-010 OPERATIONS §2.2 の Anchor 0.30.1 と Cargo.toml の `anchor-lang = "0.30"` の表記揺れ
- 場所: `docs/v0.1.2/OPERATIONS_JA.md:84` vs `programs/title-whitelist/Cargo.toml:20`
- 観察: OPERATIONS は「Anchor CLI | 0.30.1」とパッチバージョンまで固定。`anchor-lang` は `"0.30"`（=ワイルドカード解釈で 0.30.x のいずれか）。
- 問題: 厳密にはバージョンが一致しない。リプロデューシブルビルドの観点では仕様書 §5.4 と矛盾する（とはいえ Cargo.lock に最終的なパッチが固定されているので深刻ではない）。
- 修正案: `anchor-lang = "=0.30.1"` に固定する、もしくは OPERATIONS 側を「0.30.x」と緩める。前者が望ましい。

### should-fix-011 deploy/aws/README.md L208 トラブルシューティングの「`TEE_RUNTIME=nitro` in the EIF (set by the Dockerfile)」が実態と乖離
- 場所: `deploy/aws/README.md:208`
- 観察: 「confirm `TEE_RUNTIME=nitro` in the EIF (set by the Dockerfile)」と書く。`docker/tee-mock.Dockerfile` には `ENV TEE_RUNTIME=mock` がある (L42)。Nitro 用 Dockerfile はリポ内に明示的に置かれていない（`deploy/aws/scripts/build-images.sh` が建てる）。
- 問題: 読者が `docker/tee-mock.Dockerfile` の env を確認しに行くと `mock` と書いてあって混乱する。
- 修正案: 「Nitro 用 Dockerfile は `deploy/aws/scripts/build-images.sh` 内で `--build-arg` 経由で `TEE_RUNTIME=nitro` を設定する。確認したいときはこのスクリプトを参照」と一文書く。

### nitpick-001 docs/v0.1.2/SPECS_JA.md 内で「測定値 / measurement / PCR0 / 指紋」が混在
- 場所: `SPECS_JA.md:149, 153, 167, 1185, 1191, 1199` その他多数
- 観察: 同一概念に対し「measurement（測定値）」「PCR0」「指紋」「測定値」「verifying_key_hash」を文脈に応じて使い分けているが、初出時のみ括弧書きで紐づけて以降は同義語として使う、というルールが守られていない箇所がある（特に §6.2）。
- 修正案: 用語集（appendix）を 1 ページ追加し、measurement = PCR0 (AWS Nitro 文脈), verifying_key_hash = 検証回路の指紋 を一覧化する。本文では括弧書きを最小化。

### nitpick-002 SPECS_JA §0.1 末尾「C2PA v2.3（2026年1月）が現行安定版」が将来日付として未確定
- 場所: `SPECS_JA.md:9`
- 観察: 現在日 (2026-05-24 の環境) からみて過去だが、仕様書執筆当時の未来日付かどうか不明。C2PA 公式の v2.3 リリース時期が確定したら、出典をつける形に固める。
- 修正案: 「v2.3 (2026-01 公開、出典: <C2PA spec URL>)」のように出典を追加。

### nitpick-003 SPECS_JA §2.3 のサンプル `timestamp` 文字列が日付未来 (2026-01-15)
- 場所: `SPECS_JA.md:442, 738`
- 観察: 「`"timestamp": "2026-01-15T10:30:00Z"`」を例として 2 か所で使用。現実時計に近い未来日であり、本物の Google C2PA 出力例と勘違いされうる。
- 修正案: 「YYYY-MM-DDTHH:MM:SSZ」のような表記、もしくは明示的に過去日付（例: 2024-06-01）に変更。

### nitpick-004 docs/v0.1.2/audit/README.md のテンプレに「ヒアリング」セクションがなく「全体所感」のみ
- 場所: `docs/v0.1.2/audit/README.md:48-49`
- 観察: テンプレは `## 全体所感 <監査者からの一文>` のみ。「全体所感」を一文に縛ると、横断的な観察（個別 finding に分解しづらいパターン）を書く場所がない。
- 修正案: 「## 全体所感（自由形式 / 数段落可）」に緩める。

### nitpick-005 OPERATIONS §3 のフロー図 ascii 罫線の縦揃えが微妙にずれている
- 場所: `docs/v0.1.2/OPERATIONS_JA.md:208-229`
- 観察: 「`        │`」の縦線が、上下のステップ番号の桁の中心とずれる。等幅フォントでも視覚的に直線にならない。
- 修正案: スペース 2 個分ずらして縦線を numbered list の真下に揃える。読み心地のみの問題。

### nitpick-006 sp1-guests/README.md の見出し階層が直下に `Why this is not under crates/` を H2 で置いており、Title Protocol 公式 README の階層ポリシーと衝突しない確認が必要
- 場所: `sp1-guests/README.md:1-7`
- 観察: トップが `# sp1-guests/`、続いて `## Why this is not under crates/`。`docs/README.md` などサブ README は通常 H1 のあとに「概要」→「構成」と来る。
- 修正案: `## Layout` を最初に出し、`## Why this is not under crates/` は Appendix のように後段に置くと読みやすい（任意）。

## 全体所感

ドキュメントの「事実関係」自体は概ね正確に書かれており、重大な「嘘」は見つからなかった。一方で、(1) 検証フェーズに作って消した sandbox を COVERAGE が参照し続けている (must-fix-002)、(2) タスク 14 が SPECS にない GatewayAuth を要求している (must-fix-003)、(3) OPERATIONS の手順が `docker compose build tee` (mock) と Nitro 用イメージ名を混同している (must-fix-004)、(4) 環境変数表が TEE 側を半分カバーしていない (must-fix-005) など、「実機で叩いて初めて気づく」レベルの不整合が点在する。

タスク 17 で修正計画を立てる際は、まず COVERAGE.md を「現状のディレクトリ構造に対する正確なポインタ」として整備し直し、それを起点に OPERATIONS の手順を再走させる（mental simulate ではなく実機 or ローカル docker compose で）のが効率的と思う。

doc comment と実装の表記揺れ (`aws-nitro` vs `aws_nitro` should-fix-001) のような小粒は機械的な find/replace でまとめて潰せるので、修正計画の最初の 1 PR にまとめると良い。
