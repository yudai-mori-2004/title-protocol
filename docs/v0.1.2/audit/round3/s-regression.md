# S. v0.1.0 → v0.1.2 移行で失われた / 後退したもの — Round 3

## 概要

Round 2 で 7 件（must-fix 2 / should-fix 3 / nitpick 2）を指摘した。Round 3 では、Round 2 「処理ログ」（s-regression.md 末尾）に並んだ判定を一次資料で 1 件ずつ突合し、`docs/v0.1.2/SPECS_JA.md` / `docs/v0.1.2/COVERAGE.md` / `docs/v0.1.2/OPERATIONS_JA.md` / `CHANGELOG.md` / `docs/v0.1.2/tasks/17-audit-fixup/README.md` / 実コード（`crates/`, `legacy/`）の現状から、Round 2 → Round 3 区間で実体修正があったかを判断した。

Round 3 監査方針:

1. Round 2 末尾の処理ログ 6 行（regression-001 / regression-002 / new-finding-001 / 002 / 003 / 004）を 1 件ずつ取り、宣言と一次資料を突合する
2. Round 1 から繰り越されている `unchanged` 11 件（must-fix 3 / should-fix 6 / nitpick 2）について、Round 3 で動きがあるか個別確認する
3. Round 2 → Round 3 区間で新たに発生した regression / 新規発見を拾う

件数サマリ: 8 件（must-fix 2 / should-fix 4 / nitpick 2）

## 重大度別内訳

- must-fix: 2 件
- should-fix: 4 件
- nitpick: 2 件

## Round 2 処理ログの突合

| Round 2 ID | Round 2 判定 | Round 3 実体 | 備考 |
|---|---|---|---|
| regression-001 (COVERAGE "No carryover" vs doc コメント整合) | wontfix | **unchanged** | `docs/v0.1.2/COVERAGE.md:3` は `> v0.1.2 is a full rewrite. No carryover from v0.1.0/v0.1.1.` のまま。`crates/tee/src/resource_pool.rs` / `crates/core/src/jumbf.rs` で legacy 由来コメント（`grep "legacy\|v0\.1\.0\|ported\|Origin"`）は 0 件で形式上の整合は保たれるが、実態として両ファイルの設計が v0.1.0 由来であることは変わらない。Round 2 が「legacy ディレクトリ削除のスコープと一括対応」と書いた `legacy/v0.1.0/` ディレクトリは依然リポジトリに存在（`ls legacy/` → `v0.1.0`）。Round 2 の "wontfix" は scope ずらしであって解決ではない |
| regression-002 (SPECS §3.2 と `processor_outputs.rs` 削除の乖離) | wontfix（B-2 で意図的） | **unchanged + 悪化** | `docs/v0.1.2/SPECS_JA.md:724-839` は依然「以下は初期実装で提供する processor の一覧である」と現在形で `c2pa-verify` / `provenance-graph` / `image-pdq` / `video-vpdq` / `cert-google` / `cert-sony` / `cert-leica` を列挙。`docs/v0.1.2/COVERAGE.md:56-61` で 6 個の processor が `[ ] Not started` となっている事実と、SPECS §3.2 「以下は初期実装で提供する」の現在形宣言は依然矛盾。Round 2 の「wontfix(将来の image-pdq/video-vpdq 実装時に再追加)」では SPECS 側の修正が処理されておらず、Round 1 must-fix-001 が継続未対応 |
| new-finding-001 (OPERATIONS の v0.1.0 docs リンク) | wontfix（歴史リファレンスとして妥当） | **unchanged** | `docs/v0.1.2/OPERATIONS_JA.md:98` と `:424` で `docs/v0.1.0/tasks/12-e2e-local-dev/solana-build-notes.md` への参照が残存。Round 2 が「歴史リファレンスとして妥当」と整理した一方で、`docs/v0.1.2/COVERAGE.md:3` の `No carryover` 宣言と矛盾する状態が固定化された |
| new-finding-002 (devnet テストの legacy/v0.1.0/keys 依存) | fixed | **partially-fixed** | `crates/solana/tests/devnet_whitelist.rs:31-40` の `load_authority_keypair` は `keys/admin.json`（リポジトリルート直下、`ls keys/` で実在を確認）を読みに行く形に修正済みであり、テストロジック上は legacy 依存が外れている。一方、同ファイル `:9` のクレートレベル doc コメント `//! - Authority key at legacy/v0.1.0/keys/authority.json with SOL balance` は更新されておらず、運用者が前提条件を読んだ時の指示が誤ったまま固定された。Round 2 「fixed」は実装ロジック観点では正、ドキュメンテーション観点では不完全 |
| new-finding-003 (CHANGELOG `[Unreleased]` 比較リンク) | wontfix（OSS 公開前の doc メンテナンスで対応） | **unchanged** | `CHANGELOG.md:57` は `[Unreleased]: https://github.com/yudai-mori-2004/title-protocol/compare/v0.1.0...HEAD` のまま。`legacy/v0.1.1/` の有無 / v0.1.1 のリリース status の説明文も追加されていない |
| new-finding-004 (17f / 17g の完了内訳サブセクション欠落) | wontfix（OSS 公開前 doc メンテナンスで対応） | **unchanged** | `docs/v0.1.2/tasks/17-audit-fixup/README.md` は 499 行で終了（Round 2 時点 500 行から +0、実質変更なし）。17a〜17e は「完了内訳」+「先送り」+「検証」のフォーマットがあるが、17f / 17g は依然 done マークのみで内訳記述がない。Round 2 で「実態として、本ラウンドの突合では『何も修正されていない』と判定した」とした S 観点 14 件の処理状況が第三者から検証不能のまま放置 |

集計（Round 2 → Round 3 区間）: 実体修正の前進 0 件、Round 2 「fixed」の中身が部分的だったと判明したもの 1 件（new-finding-002 / DOC 側未更新）、その他は宣言通り wontfix のまま固定。

## Round 1 持ち越し「unchanged 11 件」の Round 3 突合

Round 2 で `unchanged` と判定された 11 件について、Round 2 → Round 3 区間に動きがあるかを再確認した。

| Round 1 ID | Round 2 判定 | Round 3 確認結果 |
|---|---|---|
| must-fix-001 (§3.2 「以下は初期実装で提供する」嘘の processor 一覧) | unchanged | **unchanged**: `SPECS_JA.md:724-839` の「現行の processor 一覧」の現在形は維持。`crates/gateway/src/lib.rs:71` および `crates/gateway/src/lib.rs:204` の `["c2pa-verify", "image-pdq", "provenance-graph"]` ハードコードも残存（後述の new-finding-005 を参照） |
| must-fix-002 (TSA タイムスタンプ silent removal) | unchanged | **unchanged**: `grep -n "TSA\|RFC.3161\|sigTst\|timestamp_source" docs/v0.1.2/SPECS_JA.md` は ISO 8601 形式の `"timestamp": "2026-01-15T10:30:00Z"`（c2pa-verify の例 §2.3 / §3.2）と `timestamp_ms`（video-vpdq）のみで TSA／RFC 3161 への言及は依然 0 件。CHANGELOG Removed セクションでも未明示 |
| must-fix-003 (CHANGELOG Removed 12 項目欠落) | unchanged | **unchanged**: `CHANGELOG.md:29-35` は 6 項目のまま。TypeScript SDK / cNFT Indexer / Rust CLI / `/sign-and-mint` / `/create-tree` / `/register-node` / TSA Trust List / signed_json モデル / Gateway storage backends / DAO ガバナンス / コスト章 / `hardware-google` / `c2pa-training-v1` / `c2pa-license-v1` WASM 等、いずれも追記なし |
| should-fix-001 (legacy processor 移植 port-candidate 注記) | partially-fixed | **unchanged**: COVERAGE 表 §3 「3. Processors」は `[ ] Not started` の 6 行に注記なし。`docs/v0.1.2/tasks/05-provenance-graph-processor/README.md:16` 等の個別タスク README は legacy 参照を保持しているため、ロードマップ自体は機能している。Round 2 partially-fixed の評価を維持 |
| should-fix-002 (攻撃モデル rationale 削減) | unchanged | **unchanged**: `grep "Zip Bomb\|Reservation DoS\|Slow Write\|Slowloris" docs/v0.1.2/SPECS_JA.md` は 0 件。§4.4 は依然 4 小節（データサイズの上限 / チャンクタイムアウト / グローバルタイムアウト / デコード時のメモリ保護）で、v0.1.0 にあった攻撃シナリオ↔防御手段の対応表は復活していない |
| should-fix-003 (コスト章消失) | unchanged（accepted-as-is） | **unchanged**: `grep -i "コスト\|cost\|料金\|credit" docs/v0.1.2/OPERATIONS_JA.md` は 0 件。Round 2 で「ビジネス文書として先送り」宣言済みであり Round 3 でも維持 |
| should-fix-004 (Reproducible Build 公開手段) | unchanged | **unchanged**: SPECS §5.4（`SPECS_JA.md:1117-1128`）は 12 行のまま。OPERATIONS §2.4 で `cargo run --bin vkey` 手順は記載されたが、クライアント側で ApprovedMeasurements PDA を読んで Attestation Document の PCR0 と照合する具体的フローは未文書化 |
| should-fix-005 (v0.1.0 ↔ v0.1.2 用語対応表) | unchanged | **unchanged**: `ls docs/v0.1.2/` → `COVERAGE.md / OPERATIONS_JA.md / SPECS_JA.md / audit / tasks` のみ。`MIGRATION_FROM_010.md` 不在、SPECS 冒頭にも対応表なし |
| should-fix-006 (`troubleshooting.md` 知見の引き継ぎ) | unchanged | **unchanged**: OPERATIONS §8 は 6 件（anchor build / VkeyNotApproved / MeasurementNotApproved / Self-attestation failed / Gateway "TEE unavailable" / SP1 proof OOM）で Round 2 と同数。v0.1.0 の `troubleshooting.md` にあった AES-GCM 復号失敗 / SOL 残高チェック / Solana RPC レート制限の知見は未統合 |
| nitpick-001 (architecture.md 縮約) | unchanged | **unchanged**: `docs/v0.1.2/ARCHITECTURE.md` は未作成 |
| nitpick-003 (Tree Depth 選定知見) | unchanged | **unchanged**: OPERATIONS に Tree Depth 選定ガイドは未追加 |
| nitpick-004 (`crates/core/examples/` 消失) | unchanged | **unchanged**: `ls crates/core/examples` → No such file or directory |

11 件中 0 件が前進。Round 2 partially-fixed 1 件は維持、残り 10 件は完全に静止。

## 新規発見（Round 2 → Round 3 区間）

### new-finding-005 OPERATIONS の `add_approved_vkey` / `add_approved_measurement` 手順が「`crates/solana/tests/devnet_whitelist.rs` の placeholder helper を参照」と書くだけで、本番投入用の独立した CLI も命令ビルダ公開も無く、テストコードを編集して `cargo test` 経由で実行することを正本としている

- 場所:
  - `docs/v0.1.2/OPERATIONS_JA.md:135-139`: `// crates/solana/tests/devnet_whitelist.rs の add_placeholder_vkey_devnet を参考に、`
  - `docs/v0.1.2/OPERATIONS_JA.md:184-187`: `// crates/solana/tests/devnet_whitelist.rs の add_placeholder_measurement_devnet を参考に、`
  - v0.1.0 `legacy/v0.1.0/services/tee-cli/` または `legacy/v0.1.0/crates/cli/` 相当の運用 CLI は v0.1.2 では未実装（`ls crates/` で `attestation*, core, crypto, gateway, proxy, solana, tee` のみ）
- 観察: v0.1.0 では Rust CLI が「devnet 初期化、ノード登録/削除、Tree 作成」を担っていた（Round 1 でも指摘）。v0.1.2 ではこれが消失し、devnet テストコード内の helper を運用者が「ソース改変 → `cargo test --ignored`」で実行する形に退行している。Round 2 までは「Rust CLI の消失」を CHANGELOG Removed 欠落の must-fix-003 の一項目としてカウントしていたが、運用 SOP 上の影響が OPERATIONS で固定化されたのは Round 3 で初めて顕在化した
- 問題:
  1. 本番運用者が `add_approved_measurement` のような admin 専用命令をテストコードから実行する形になり、誤った placeholder（`[0xBB; 48]`）を本番 PDA に書き込む事故の risk が高い（OPERATIONS §2.5 が「本番ローンチ前に必ず本物の値に差し替える」と注意書きしているのは、まさにこの構造の脆さを物語る）
  2. テストハーネス（`#[ignore]` テスト + `--nocapture`）を介在させるため、operator が `cargo test` 環境を構築する必要があり、cold-start 運用者にハードルを与える
  3. CHANGELOG Removed に Rust CLI 削除を明記すれば、利用者は v0.1.2 の運用が「テストコード経由」になっている事実を事前に知れたが、CHANGELOG が未更新（must-fix-003）なため、OPERATIONS まで読み進めないと気付けない
- 重大度: must-fix
- 修正案:
  - `crates/cli/` を新設して `add-approved-vkey` / `add-approved-measurement` / `register-key` / `revoke-key` などの admin 命令を CLI として公開する。v0.1.0 の `legacy/v0.1.0/crates/cli/` を参考に最小限の `clap` ベースのバイナリで足りる
  - OPERATIONS §2.4 / §2.6 / §6 の手順を CLI 呼び出しに書き換える
  - 当面は移行コストを嫌うなら、せめて `crates/solana/src/bin/` に薄い public binary を 1 つ追加し、テスト用 helper と運用用 binary のソース上の分離を明確化する

### new-finding-006 `crates/gateway/src/lib.rs` の `/processors` レスポンス例 / テストが `["c2pa-verify", "image-pdq", "provenance-graph"]` の 3 件ハードコードを依然保持しているのに対し、Gateway 実装は TEE の `/processors` 応答をキャッシュ転送するだけで、TEE 自身 (`crates/tee/src/` の `ProcessorRegistry`) は c2pa-verify 1 個しか登録しない

- 場所:
  - `crates/gateway/src/lib.rs:71`: `///   "processors": ["c2pa-verify", "image-pdq", "provenance-graph"]`
  - `crates/gateway/src/lib.rs:204`: テスト fixture `assert_eq!(resp.processors.len(), 3);`
  - `docs/v0.1.2/COVERAGE.md:55-61`: c2pa-verify のみ `[x]` で残り 6 個は `[ ] Not started`
  - 17-audit-fixup README:202: `S-mf001 processor の利用可能状況を明確化（v0.1.2 では c2pa-verify のみ）` ← done 宣言済み
- 観察: 17f の done マークでは「processor の利用可能状況を明確化」とあるが、`crates/gateway/src/lib.rs` の doc コメント例とテストアサーションは依然「3 件返る」想定のまま。仕様書 §2.5 の `/processors` レスポンス例（`SPECS_JA.md:622`）も `["c2pa-verify", "image-pdq", "provenance-graph"]` のまま。Round 1 must-fix-001 の延長線上にあるが、Round 2 ではテストコード側の検出に踏み込まなかった
- 問題: 実機でこのテストが動いている（`#[test] fn processors_response_from_spec()`）ことで、誤った 3 件想定が CI で固定化される。OSS 公開時に運用者がコードを参照すると「3 種類サポートされている」と誤認する
- 重大度: should-fix
- 修正案:
  - SPECS §2.5 `/processors` の例を `["c2pa-verify"]` 単独に修正（must-fix-001 と一括対応）
  - `crates/gateway/src/lib.rs:71` の doc 例も `["c2pa-verify"]` に
  - `processors_response_from_spec` テストを `["c2pa-verify"]` ベースに書き換え

### new-finding-007 v0.1.0 SPECS §6.6 「インデクサ」「DAS API ポーリング」「Helius Webhooks + Supabase」というオフチェーン読み出しレイヤーの設計知見が v0.1.2 から完全消失

- 場所:
  - v0.1.0 `docs/v0.1.0/SPECS_JA.md` §6.6 インデクサ章（Helius Webhooks + Supabase で構築されていたエンドポイント、ポーリングによる Webhook 欠落補完、コストモデル）
  - v0.1.2 では `grep -i "indexer\|DAS\|helius\|webhook" docs/v0.1.2/SPECS_JA.md docs/v0.1.2/OPERATIONS_JA.md` → 0 件
  - CHANGELOG Removed セクションにも明示なし（must-fix-003 で「cNFT Indexer」が欠落項目として列挙されていたが、設計知見の引き継ぎ先がない）
- 観察: v0.1.0 では Indexer は「利便性レイヤー」として明確に位置付けられ、Solana cNFT を効率的に読み出すための実装パターン（Webhook + ポーリング + Supabase Edge Functions）が SPECS §6.6 と運用知見として記録されていた。v0.1.2 では cNFT 発行（書き込み側）のみが Solana Extension §6.2 に残り、cNFT を「検証する側」の読み出し基盤に関する記述は何も無い。CHANGELOG の Removed 候補に「cNFT Indexer」と列記したまま、設計判断（v0.1.2 では何故 Indexer を扱わないか）も先送り戦略（V0.2 で再構築するか、DAS API 直叩きを推奨するか）も書かれていない
- 問題: SPECS §1.5 が「検証に外部への問い合わせは不要」と言い切る一方で、§6.2 のホワイトリスト判定（「cNFTの発行トランザクションに、ホワイトリスト済みの署名鍵の署名が含まれているか」）を確認するには Solana RPC か DAS API への問い合わせが必須。検証者がどうやって `WhitelistEntry` PDA や cNFT トランザクション履歴を取得するのかが SPECS / OPERATIONS いずれにも記述がなく、OSS 公開時に「v0.1.2 はどう読み出すのか」が宙吊りになる
- 重大度: should-fix
- 修正案:
  - SPECS §6.2 末尾に「検証側の RPC アクセスについて」サブセクションを追加し、DAS API（Helius 等）or Solana RPC を使う前提を明示する（数行で済む）
  - もしくは OPERATIONS §6 に「クライアント検証フロー」セクションを追加し、`WhitelistEntry` PDA / cNFT mint TX の取得手順を 1 か所にまとめる
  - 上記 2 つのいずれかと併せて、CHANGELOG Removed に「cNFT Indexer (v0.1.0 §6.6 — DAS API/RPC 直接アクセスを推奨)」を 1 行で記録

### new-finding-008 v0.1.0 SPECS §10 「ロードマップ」相当の章が v0.1.2 から消失し、Phase 1〜4 の段階的開示（Core/Extension 実装 → SDK 公開 → DAO による Trust List 管理 → 複数 TEE ノードによる分散化）が引き継がれていない

- 場所:
  - v0.1.0 `docs/v0.1.0/SPECS_JA.md:2911-2918` の「# 10. ロードマップ」表
  - v0.1.2 `docs/v0.1.2/SPECS_JA.md` は §6 Extension で終了、§7 以降の章がない。OPERATIONS §9「ロードマップ」（`OPERATIONS_JA.md:454-460`）は 5 項目（AWS Nitro 実機検証 / SDK / Range Request / 追加 processor / mainnet デプロイ）の TODO リストのみで、フェーズ立てもガバナンス段階の説明もない
- 観察: v0.1.0 では「Phase 1 = 開発」「Phase 2 = SDK 公開」「Phase 3 = DAO ガバナンス」「Phase 4 = 分散化」が SPECS §10 に明示され、OPERATIONS や個別ドキュメントから参照されていた。v0.1.2 ではこのフェーズ立てが完全に消失し、代わりに OPERATIONS §9 の 5 行 TODO になっている。v0.1.0 § 8.1 / § 6.5 / § 6.6 が Phase 1〜3 を交差参照していた構造が失われた
- 問題:
  - OSS 利用者が「v0.1.2 が今どの段階で、いつ DAO に移行するのか」を判断する材料がない
  - 「現在の admin authority は単一鍵（OPERATIONS §2.2 の `wrVwsTuRzbsDutybqqpf9tBE7JUqRPYzJ3iPUgcFmna`）」という事実が、いつまで続くのか・どこで multi-sig 化するのかが宙吊り
  - 投資家・パートナー・OSS コントリビュータがプロジェクトの成熟ステージを理解できず、コミット判断が難しい
- 重大度: nitpick（実装品質には影響しないが、OSS 公開時の説明責任に影響）
- 修正案:
  - SPECS に「§7 ロードマップ」を新設し、v0.1.0 の Phase 1〜4 を v0.1.2 のスコープに合わせて書き直す（admin 単一鍵 → multi-sig → DAO の段階を明示）
  - OPERATIONS §9 の TODO 5 項目はそのまま「実装ロードマップ」として残しつつ、SPECS のフェーズ表とリンクする

## 全体所感

Round 2 → Round 3 区間の実体修正は 0 件。Round 2 「処理ログ」で wontfix 判定された 5 件はいずれも一次資料上で動きがなく、Round 2 で fixed と判定された 1 件（new-finding-002 / devnet テストの legacy 依存）も実装ロジックは修正済みだが doc コメントが追従していない部分的修正にとどまる。

Round 1 から繰り越されている must-fix 3 件は依然全て unchanged:

1. **must-fix-001** (§3.2 嘘の processor 一覧): SPECS / Gateway test fixture / Gateway doc 例の 3 箇所で `c2pa-verify` 1 個のみが実装されている事実と矛盾
2. **must-fix-002** (TSA タイムスタンプ silent removal): CHANGELOG にも SPECS にも記述なし
3. **must-fix-003** (CHANGELOG Removed セクション 12 項目欠落): 6 項目のままで放置

Round 3 で新たに 4 件を発見:

- **new-finding-005** (must-fix): OPERATIONS が admin 命令の実行をテストコード経由に依存する構造を運用 SOP として固定化。Rust CLI 消失（CHANGELOG Removed の未明示項目）が運用品質に直接影響している
- **new-finding-006** (should-fix): Gateway の `/processors` doc / テストが 3 個ハードコードを保持し続け、c2pa-verify 1 個のみ実装の事実と乖離。must-fix-001 と一体で対応すべき
- **new-finding-007** (should-fix): v0.1.0 §6.6 Indexer 章の設計知見（Helius Webhooks + Supabase / DAS API ポーリング）の引き継ぎ先がなく、検証者の RPC アクセス手段が SPECS / OPERATIONS 不在
- **new-finding-008** (nitpick): v0.1.0 §10 ロードマップの段階開示（Phase 1〜4 / admin → multi-sig → DAO）が消失し、プロジェクト成熟ステージが不可視

最優先で対処すべきは must-fix-001 / must-fix-002 / must-fix-003 / new-finding-005 の 4 件。これらはいずれも OSS 公開時に第三者が即座に検知できる不整合であり、Round 1 → Round 2 → Round 3 と 2 ラウンドにわたり「先送り」または「scope ずらし」で温存されてきた。17f / 17g の完了内訳サブセクションを起こすことなくこのまま「done」扱いを続けると、監査 → 修正 → 再監査のガバナンスサイクル自体の信頼が崩れる。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| regression-001 (COVERAGE No carryover) | wontfix | `legacy/` ディレクトリ自体の削除は v0.1.3 OSS 公開時に整理。実態上の v0.1.0 由来コードのアーカイブとして残置。 |
| regression-002 (§3.2 processor 一覧現在形) | fixed | SPECS §3.2 冒頭を「v0.1.2 で実装されているのは c2pa-verify のみ。残り 6 個は将来リリースで実装」と書き換え、現在形宣言と実装の乖離を解消。 |
| new-finding-001 (OPERATIONS の v0.1.0 docs リンク) | wontfix | 歴史リファレンスとして妥当、v0.1.3 doc メンテで一括対応。 |
| new-finding-002 (devnet テストの legacy 依存) | fixed(K5) | K5 R3-S-001 で `crates/solana/tests/devnet_whitelist.rs:9` の docstring を `keys/admin.json` に修正済み。 |
| new-finding-003 (CHANGELOG [Unreleased] リンク) | wontfix | v0.1.3 OSS 公開前に最終 tag 確定時にまとめて更新。 |
| new-finding-004 (17f / 17g 内訳サブセクション) | wontfix | タスク README の歴史性、v0.1.3 doc メンテで整理。 |
| must-fix-001 (§3.2 嘘の processor 一覧) | fixed | regression-002 と統合解決。SPECS §3.2 + §2.5 /processors 例 + Gateway `lib.rs` doc + test を「c2pa-verify only」に揃えた。 |
| must-fix-002 (TSA タイムスタンプ silent removal) | fixed | `CHANGELOG.md` Removed セクションに「TSA / RFC 3161 timestamp trust list」を 1 行追記。 |
| must-fix-003 (CHANGELOG Removed 12 項目欠落) | fixed | `CHANGELOG.md` Removed に TypeScript SDK、cNFT Indexer、Rust CLI、TEE 旧エンドポイント、Solana program admin ix、Gateway storage backends、DAO governance、コスト章、WASM モジュール 3 個、signed_json モデル、TSA Trust List を追記。 |
| Round 1 unchanged should-fix-001..006 / nitpick-001/003/004 | wontfix | v0.1.3 doc メンテで一括対応 (ARCHITECTURE.md 新設、MIGRATION_FROM_010.md、Reproducible Build 公開手順 等)。 |
| new-finding-005 (admin CLI 不在) | wontfix | `crates/cli/` 新設は v0.1.3 タスク。CHANGELOG Removed に「Rust CLI」を明記済み (must-fix-003 と統合)、運用者の予期は揃った。 |
| new-finding-006 (/processors hard-code 3 個) | fixed | must-fix-001 と統合解決。 |
| new-finding-007 (Indexer / DAS API 設計知見) | wontfix | 読み出し側 SDK と一体で v0.1.3。CHANGELOG Removed の「cNFT Indexer」行に DAS API 直接アクセス推奨を併記済み。 |
| new-finding-008 (ロードマップ章) | wontfix | OPERATIONS §9 の TODO 5 項目で当面カバー、SPECS §7 ロードマップ章新設は v0.1.3。 |
