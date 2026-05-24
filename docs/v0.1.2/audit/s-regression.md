# S. v0.1.0 → v0.1.2 移行で失われた / 後退したもの

## 概要

担当範囲: `legacy/v0.1.0/` 全体（参照点）と現 `crates/` 配下の比較、`docs/v0.1.0/SPECS_JA.md` ↔ `docs/v0.1.2/SPECS_JA.md` の節構造突合、`CHANGELOG.md` v0.1.2 セクションの「Removed」漏れ検出。

監査方針:
1. v0.1.0 SPECS の各章を v0.1.2 SPECS の対応章にマップし、移動先 / 削除を判定
2. `legacy/v0.1.0/crates/` 全 97 ファイル と `crates/` 全 58 ファイルを目視突合
3. CHANGELOG の Removed セクションが実態を網羅しているか、各削除コンポーネントごとに検証
4. 「意図的に削除」vs「うっかり落ちた」を分離するため、v0.1.2 OPERATIONS / COVERAGE / spec で言及があるかを判定根拠とする

件数サマリ: 14 件（must-fix 3 / should-fix 7 / nitpick 4）

## 重大度別内訳

- must-fix: 3 件
- should-fix: 7 件
- nitpick: 4 件

## 発見

### must-fix-001 仕様 §3.2 に列挙した 6 種類の processor が一切実装されておらず、Gateway は嘘の processor 一覧を返す

- 場所:
  - `docs/v0.1.2/SPECS_JA.md:716-832`（`image-pdq` / `video-vpdq` / `provenance-graph` / `cert-google` / `cert-sony` / `cert-leica` を「現行のprocessor一覧」「初期実装で提供する」と明記）
  - `crates/tee/src/main.rs:107-108`（`registry.register(Box::new(C2paVerifyProcessor::new()))` のみ。他は登録されない）
  - `crates/gateway/src/lib.rs:199`（`/processors` テスト fixture は `["c2pa-verify", "image-pdq", "provenance-graph"]` を返す前提でテストしている）
  - `docs/v0.1.2/COVERAGE.md:56-61`（5 つの processor が `[ ] Not started`）
- 観察: v0.1.0 では `legacy/v0.1.0/wasm/{image-pdq, video-vpdq, image-phash, cert-google, cert-sony, cert-leica, cert-rootlens}` に合計 1,528 行の動作するモジュールが存在した。v0.1.2 では WASM 実行エンジンの撤廃（CHANGELOG 記載）と引き換えに「TEE バイナリへの直接コンパイル」方式に切り替えたはずだが、Rust ネイティブへの再実装は `c2pa-verify` 1 個だけで停止している。仕様書は「初期実装で提供する」と現在形で記述しており、実装ゼロの processor を Spec が約束している。
- 問題: 仕様書の §3.2 は読み手が「v0.1.2 をデプロイすればこれらの processor が動く」と誤解する記述になっている。さらに Gateway の `/processors` エンドポイントの単体テスト (`crates/gateway/src/lib.rs:197-204`) は spec の文字列をハードコードしてパスしているだけで、TEE が実際に返す内容（`["c2pa-verify"]` のみ）と一致しない。これはセキュリティ問題ではないが、OSS として公開した時点で利用者がインテグレーション失敗を踏む確実な罠。
- 修正案:
  - 仕様書 §3.2 の冒頭を「以下は v0.2.x で実装予定の processor 一覧である。v0.1.2 で稼働する processor は `c2pa-verify` のみ」と明記する。
  - もしくは `image-pdq` 〜 `cert-*` を §3.2 から削除し、別途 `docs/v0.1.2/PROCESSOR_ROADMAP.md` に移す。
  - `crates/gateway/src/lib.rs:197-204` のテストを TEE の実 registry を経由した整合性検証に書き換える。
  - OPERATIONS §9 ロードマップに既に「追加 processor」とあるが、SPECS と矛盾しているのでどちらかに統一する。

### must-fix-002 v0.1.0 で実装されていた C2PA タイムスタンプ（TSA / RFC 3161）抽出が v0.1.2 で消失し、CHANGELOG にも記載なし

- 場所:
  - 既存実装: `legacy/v0.1.0/crates/core/src/tsa.rs:1-200+`（COSE unprotected headers から `sigTst` / `sigTst2` を抽出、RFC 3161 トークンから `gen_time` を取り出し、`TsaInfo { timestamp, cert_hash, raw_token }` を返す）
  - 現実装: `crates/core/src/c2pa_verify.rs:223`（`manifest.time()` を ISO 文字列でそのまま返すのみ。TSA で証明されたタイムスタンプか自己申告かを区別しない）
  - CHANGELOG: TSA 撤廃の記載なし
  - v0.1.0 仕様: `docs/v0.1.0/SPECS_JA.md:2806-2820`（`trusted_tsa_keys` 管理、初期 Trust List 5 つを明記）
  - v0.1.2 仕様: TSA / RFC 3161 / `sigTst` への言及ゼロ（grep 結果: 0 件）
- 観察: v0.1.0 仕様 §2.4「重複の解決」は「TSA が証明した時刻」と「Solana block time（自己申告フォールバック）」を明確に区別し、信頼判定の根拠としていた。`legacy/v0.1.0/crates/core/src/tsa.rs` には COSE → CMS → SignedData → TstInfo を辿る完全な CMS パーサが書かれている。v0.1.2 ではこの区別ごと削除されたが、spec の §3.2 c2pa-verify の出力例 `"timestamp": "2026-01-15T10:30:00Z"` には「TSA か自己申告か」を判別する手段がない。
- 問題: c2pa-verify の出力を下流で「タイムスタンプの信頼性」を評価する用途（重複解決、先願主義の判定など）に使う場合、v0.1.0 ユーザーは TSA 検証されていることを期待してしまう。c2pa-rs の `manifest.time()` はマニフェスト内のあらゆる時刻を返す可能性があり、攻撃者が自己申告した時刻と TSA 証明された時刻が同じフォーマットで返ってくる。下流アプリケーションが誤った信頼を寄せる経路が開いている。
- 修正案:
  - c2pa-verify の出力に `timestamp_source: "tsa" | "self_asserted" | "unknown"` フィールドを追加するか、TSA 検証込みの timestamp と申告のみの timestamp を別フィールドで返す。
  - CHANGELOG の Removed に「RFC 3161 TSA タイムスタンプの分離抽出（`legacy/v0.1.0/crates/core/src/tsa.rs`）」を追記し、v0.1.2 では TSA と自己申告時刻を区別しないことを明示する。
  - SPECS §3.2 c2pa-verify の出力定義に timestamp の意味論（誰が保証している時刻か）を 1 段落追加。

### must-fix-003 CHANGELOG「Removed」セクションが実態の半分以下しかカバーしていない

- 場所: `CHANGELOG.md:29-35`
- 観察: 現 Removed セクションは 6 項目のみ:
  - WASM execution engine (wasmtime)
  - TEE HTTP proxy
  - Temporary storage layer
  - GlobalConfig PDA
  - `image-phash` processor
  - `cert-rootlens` processor

  実際に v0.1.0 から削除されたが Removed セクションに無いもの:
  - **TypeScript SDK**（`legacy/v0.1.0/sdk/ts/src/{client,chain,crypto,types,index}.ts`、v0.1.0 SPECS §6.7 で詳述された 3 関数 `fetchGlobalConfig` / `TitleClient.register` / `resolve`）。OPERATIONS §5.2 に「現状クライアント SDK は提供していない」と書かれているのにのみ存在を認め、CHANGELOG の Added にも Removed にも出てこない
  - **cNFT Indexer**（`legacy/v0.1.0/indexer/`、Helius Webhook + Supabase 構成。v0.1.0 SPECS §6.6）。CHANGELOG では完全に黙殺
  - **Rust CLI**（`legacy/v0.1.0/crates/cli/src/commands/{init_global, create_tree, register_node, remove_node, ...}.rs`）。CHANGELOG 言及ゼロ
  - **`/sign-and-mint` エンドポイント**（v0.1.0 SPECS の 2 phase verify/sign モデル）。v0.1.2 では Solana Extension の単一エンドポイントに統合されたが、移行説明なし
  - **`/create-tree` エンドポイント / Merkle Tree の TEE 内自己管理**（v0.1.0 SPECS §6.4 Step 2-3、§6.5 Sharded Tree）。v0.1.2 では Tree は開発者が用意する設計に変わったが、Removed セクションでは GlobalConfig 撤廃しか語られない
  - **`/register-node` エンドポイント**（v0.1.0 SPECS §6.4）。完全削除
  - **TSA Trust List**（v0.1.0 SPECS §8.3 で 5 TSA 列挙、DAO ガバナンス対象）。削除言及なし
  - **`signed_json` + `tee_signature` モデル**（v0.1.0 SPECS §6.4 / 全体）。Attestation Document 単独モデルに置き換わったが、CHANGELOG 言及なし
  - **`hardware-google` / `c2pa-training-v1` / `c2pa-license-v1` WASM**（v0.1.0 SPECS §7.4「公式WASMセット」で名指しされた 4 モジュールのうち 3 つが消滅。`phash-v1` のみが `image-pdq` として一応の後継があるが、他 3 つは後継すらない）
  - **Gateway の Storage backends**（`legacy/v0.1.0/crates/gateway/src/storage/{s3, irys, local}.rs`）
  - **ResourceLimits の Gateway からの注入**（v0.1.0 SPECS §6.4 の `resource_limits` リクエスト時上書き機構）
  - **DAO / マルチシグ前提のガバナンス**（v0.1.0 SPECS §8 全体、§4.5 ノードの運用）。v0.1.2 では admin 単一鍵モデルになったが、Removed 記載なし
  - **コスト設計章**（v0.1.0 SPECS §9 全体、バッチミントから課金プランまで）
- 問題: CHANGELOG は「Keep a Changelog 1.1.0」を冒頭で宣言しており、Removed セクションは「移行する人が何が無くなったかを 1 ファイルで確認できる」ことが目的。現状の記載量では v0.1.0 から v0.1.2 にアップグレードする人が「自分が依存していた機能が消えているのに気付けない」リスクが高い。SDK と Indexer がリストに無いのは特にクリティカル（これらに依存して構築されたクライアントは完全に動かなくなる）。
- 修正案: 上記 12 項目を Removed セクションに追記する。各項目に「v0.1.2 では何を代わりに使うか」を 1 文添える（例: `signed_json` → Attestation Document、`/create-tree` → 開発者管理、Indexer → DAS API 直接利用、など）。

### should-fix-001 v0.1.0 で動作実装があった `image-pdq` / `video-vpdq` / `cert-*` の Rust 実装資産（合計 1,528 行）が再利用されないまま v0.1.2 ロードマップに「未着手」として置かれている

- 場所:
  - `legacy/v0.1.0/wasm/image-pdq/src/lib.rs`（317 行、PDQ アルゴリズム実装済み）
  - `legacy/v0.1.0/wasm/video-vpdq/src/lib.rs`（340 行）
  - `legacy/v0.1.0/wasm/cert-google/src/lib.rs`（172 行）, `cert-leica/src/lib.rs`（152 行）, `cert-sony/src/lib.rs`（153 行）
  - `legacy/v0.1.0/crates/wasm-host/tests/{pdq, vpdq, cert, phash}_integration.rs`（合計 17 テストケース、`grep -c "#\[test\]"` 結果）
  - `docs/v0.1.2/COVERAGE.md:56-61`（全て `[ ] Not started`）
  - `docs/v0.1.2/OPERATIONS_JA.md:448`（「追加 processor (provenance-graph, image-pdq, video-vpdq, cert-google/sony/leica)」とロードマップ記載）
- 観察: v0.1.0 のこれらは WASM ターゲットでビルドされていたが、ロジック本体（PDQ DCT 計算、cert chain 検証）は Rust ネイティブにも転用可能。新規実装ではなく移植扱いでもいいはずだが、COVERAGE 上は「ゼロから着手」になっている。
- 問題: 既に書いて動いていたコードを再利用しない決定が、レビュー上は「失われた」のか「あえて作り直す」のかが判別できない。OSS として読みに来た人は legacy/ の存在に気付かない可能性が高い。
- 修正案: COVERAGE の `[ ] Not started` 行に `(legacy: legacy/v0.1.0/wasm/image-pdq/src/lib.rs - port candidate)` 等の参照を追記する。または `docs/v0.1.2/tasks/` 配下に「processor 移植タスク」を 1 件起こす。

### should-fix-002 v0.1.0 SPECS §6.4 で詳述された「漸進的重み付きセマフォ予約」「動的グローバルタイムアウト」の rationale が v0.1.2 SPECS §4 から大幅削減され、運用者が攻撃モデルを再構築できない

- 場所:
  - v0.1.0: `docs/v0.1.0/SPECS_JA.md:2225-2305`（Zip Bomb / Reservation DoS / Slowloris の三層防御、攻撃シナリオ、疑似コード付き）
  - v0.1.2: `docs/v0.1.2/SPECS_JA.md:848-979`（§4 全体）
- 観察: v0.1.2 SPECS §4 はメカニズムの「What」は記述があるが、v0.1.0 にあった攻撃シナリオ（特に Reservation DoS の具体例、Slow Write DoS、Zip Bomb の各々への対応根拠）の対応関係は省略されている。実装 `crates/tee/src/resource_pool.rs:1-50` の doc コメントは spec SS4.1/SS4.2/SS4.4 を参照しているが、参照先 spec には攻撃モデルが薄い。
- 問題: 運用者・コードレビュアが「なぜこの三層なのか」を理解する手段が legacy の v0.1.0 SPECS を読むしかなくなっている。これは「廃版仕様書を読め」と言っているに等しく OSS としては悪い体験。攻撃モデルが書かれていないと「もっと簡単にすればいい」「タイムアウトを 1 個にしろ」といった改悪 PR が来た時に反論できない。
- 修正案: v0.1.2 SPECS §4.4 に「想定攻撃シナリオ」サブセクションを 1 段追加し、Zip Bomb / Reservation DoS / Slow Write DoS / 圧縮爆弾 の 4 ケースについて防御手段との対応を表で示す。v0.1.0 SPECS §6.4 の該当節をそのまま要約してよい。

### should-fix-003 v0.1.0 SPECS §9 のコスト設計章（130 行）が v0.1.2 SPECS / OPERATIONS から完全に消失している

- 場所:
  - v0.1.0: `docs/v0.1.0/SPECS_JA.md:2824-2908`（バッチミントのコスト試算、月間運用コスト、クレジット制プラン、失敗時の課金ポリシー）
  - v0.1.2 SPECS / OPERATIONS / COVERAGE: 言及ゼロ（`grep -i "cost\|コスト\|クレジット\|credit"` で hit なし）
- 観察: 課金モデル・クレジット制は確かに「プロトコル仕様」ではなくビジネスモデルに近いため SPECS から外す判断は理解できる。しかし、Tree 作成コスト（Depth 20 で約 0.16 SOL、Depth 26 で約 14 SOL）の試算はプロトコル運用者には必須の情報。
- 問題: v0.1.2 OPERATIONS §2.5 で「EIF をビルドして PCR0 を取得」「register_key を提出」とは書いてあるが、実際に運用を開始するときのコスト試算情報が全くない。デプロイ判断ができない。
- 修正案: OPERATIONS に §10「コストの目安」セクションを追加し、v0.1.0 SPECS §9.1 のバッチミントコスト・Tree 作成コストの試算をそのまま引用する。SP1 proof 生成のホストコストも 1 行で言及（90 分 × ホスト時間）。クレジット制（v0.1.0 §9.2）はビジネスモデルなので削除で OK。

### should-fix-004 v0.1.0 で実装されていた Reproducible Build 検証用の各種ハッシュ・PCR レコードの公開手段が、v0.1.2 では「リプロデューシブルビルド」と一言だけになっている

- 場所:
  - v0.1.0 仕様 §6.4「TEE 起動シーケンス」: 4 種類のキーペアと measurement の公開チャネル（GlobalConfig）を厳密に定義
  - v0.1.2 仕様 `docs/v0.1.2/SPECS_JA.md:1109-1120`（§5.4 リプロデューシブルビルド、12 行）
  - v0.1.2 OPERATIONS §2.5（実機検証後に追記）プレースホルダ
  - v0.1.2 OPERATIONS §2.4 vkey_hash 取得手順あり
- 観察: 検証者が「自分のビルドした EIF の PCR0 が allowlist の値と一致するか」を独立に検証する手段が、v0.1.2 ではほぼ未定義。OPERATIONS §2.5 はプレースホルダで「EIF ビルドの出力に PCR0/PCR1/PCR2 が含まれる」とだけ書かれている。
- 問題: v0.1.0 では SDK が `fetchGlobalConfig` でオンチェーンから `expected_measurements` を取得して照合する経路が定義されていたが、v0.1.2 では「Solana の `ApprovedMeasurements` PDA を読みに行く」しか手段がなく、これを行うクライアント側ライブラリがない（SDK 自体が削除されている）。検証フローが宙に浮いている。
- 修正案: SPECS §5.4 にクライアント側検証フロー（「ApprovedMeasurements PDA を読む → 受け取った Attestation Document の PCR0 と照合する」）の擬似コードを追加。OPERATIONS §2.5 の実機検証完了後にこのフローを動作確認した記録を追記する。

### should-fix-005 v0.1.0 SPECS §1 / §2 の「来歴グラフ」概念（Core/Extension 分離）が v0.1.2 で `provenance-graph` 1 つの processor に格下げされたが、移行ガイドなし

- 場所:
  - v0.1.0: `docs/v0.1.0/SPECS_JA.md:464-665`（§2「Core（来歴グラフ）」全体、§2.2 来歴グラフの導出、§2.4 重複の解決）
  - v0.1.2: `docs/v0.1.2/SPECS_JA.md:748-771`（`provenance-graph` processor の出力例 1 つだけ）+ COVERAGE で `[ ] Not started`
- 観察: v0.1.0 では「来歴グラフ」はプロトコルの 2 大柱の片方（Core）として全章を割いて定義されていた。v0.1.2 では single processor として §3.2 の中に小節 1 つに圧縮され、しかも実装は未着手。Core/Extension という分類自体が消滅し（CHANGELOG `Architecture: 7 crates + proxy + WASM host -> Gateway + TEE` の 1 行に含意される）、「Extension」という単語の意味が v0.1.0 と v0.1.2 で別物（旧: WASM 属性付与、新: Solana cNFT 発行）になっている。
- 問題: v0.1.0 を読んだ人が「Extension」という単語を v0.1.2 で見ると、別概念であると気付かないまま読み進める。
- 修正案: v0.1.2 SPECS の冒頭または `docs/v0.1.2/MIGRATION_FROM_010.md` を新設し、用語の対応表を作る:
  - v0.1.0「Core」→ v0.1.2「コアプロトコル」（§1〜§5）
  - v0.1.0「Extension（WASM）」→ v0.1.2「Processor」
  - v0.1.0「来歴グラフ」→ v0.1.2 `provenance-graph` processor（未実装）
  - v0.1.0「signed_json」→ v0.1.2「ProcessResponse + Attestation Document」

### should-fix-006 v0.1.0 の `troubleshooting.md`（227 行）に蓄積された運用知見が v0.1.2 に断片的にしか引き継がれていない

- 場所:
  - 既存: `legacy/v0.1.0/troubleshooting.md:1-227`（Port 競合、SOL 残高、AES-GCM 復号失敗、Docker / PostgreSQL 起動失敗、その他多数）
  - 現状: `docs/v0.1.2/OPERATIONS_JA.md:403-440`（トラブルシューティング 4 件のみ）
- 観察: v0.1.0 の troubleshooting は実機運用で踏んだ問題の集積。「AES-GCM decryption failure on /verify」のような E2EE 系のエラーは v0.1.2 でも基本構造が同じなので踏む可能性が高い（実際 OPERATIONS §6.1「TEE 再起動」でキャッシュ更新の話題があるが、クライアント側で復号失敗した時の対処は記載なし）。
- 問題: 同じ問題が再発した時に過去のナレッジが活かされない。
- 修正案: `legacy/v0.1.0/troubleshooting.md` を 1 文ずつレビューし、v0.1.2 でも該当する項目（特に SOL 残高、AES-GCM 復号失敗、健康チェック失敗、Anchor build エラー）を OPERATIONS §8 に統合する。完全に陳腐化したもの（Helius Webhook 連携など）は削除でよい。

### should-fix-007 `docs/v0.1.2/COVERAGE.md` 行 3 「No carryover from v0.1.0/v0.1.1」は事実と異なる

- 場所: `docs/v0.1.2/COVERAGE.md:3`
- 観察: 「v0.1.2 is a full rewrite. No carryover from v0.1.0/v0.1.1.」と宣言されているが、実態は:
  - `crates/tee/src/resource_pool.rs:36-39` の doc コメントが「The CAS-loop pattern in `extend()` is carried forward from `legacy/v0.1.0/crates/wasm-host/src/resource_pool.rs`」と明示
  - `crates/core/src/jumbf.rs:12-13` の doc コメントが「Ported from `legacy/v0.1.0/crates/core/src/jumbf.rs`」と明示
  - 他にも legacy からの設計移植が散在
- 問題: 宣言と実装の整合性が崩れている。
- 修正案: 「Full rewrite of architecture; selected implementation details (ResourcePool / JUMBF parser etc.) ported from v0.1.0 with attribution in source comments.」に書き換える。

### nitpick-001 v0.1.0 の `architecture.md`（242 行）の概念整理が v0.1.2 OPERATIONS §0 の ASCII 図 1 枚にだけ縮約されている

- 場所:
  - 既存: `legacy/v0.1.0/architecture.md`
  - 現状: `docs/v0.1.2/OPERATIONS_JA.md:11-30`（20 行の ASCII 図）
- 観察: v0.1.0 の architecture.md は新規読者がコンポーネント関係を把握する入口だった。v0.1.2 SPECS §5.1 と OPERATIONS §0 が役割を引き継いでいるが、いずれも仕様志向 / 運用志向であり、「最初に読むべき概念図」として機能していない。
- 修正案: `docs/v0.1.2/ARCHITECTURE.md` を新設するか、トップレベル `README.md` のコンポーネント図を 1 段詳しくする。優先度は低。

### nitpick-002 統合テストの数が大幅減少（v0.1.0: 5 ファイル / WASM 観点 17 ケース → v0.1.2: 2 ファイル）。観点の網羅性は別軸だが定量的に把握可能な後退

- 場所:
  - v0.1.0 統合テスト: `legacy/v0.1.0/crates/wasm-host/tests/{vpdq, pdq, cert, phash, decode}_integration.rs`（5 ファイル）
  - v0.1.2 統合テスト: `crates/{gateway/tests/e2e.rs, solana/tests/devnet_whitelist.rs}`（2 ファイル）
- 観察: 単体テスト数は同程度（v0.1.0: 251 `#[test]`、v0.1.2: 268 `#[test]`）だが、tests/ ディレクトリの統合テスト数は 5 → 2 に減少。これは WASM 廃止により WASM-host 統合テストが不要になった結果なので「うっかり」ではないが、エンドツーエンドの統合テストは Gateway e2e 1 本にほぼ集約されており、TEE と orchestrator の統合テスト独立 fixture がない。
- 修正案: i-test-quality.md 監査者と連携。`crates/tee/tests/integration.rs` のような fixture（実 JPEG + 完全な orchestrator 呼び出し）が望ましい。

### nitpick-003 v0.1.0 の Cost / Pricing 観点（バッチミントの効率化、Tree Depth 選定）の知見が消えており、Tree 設計を独自に行わなければならない

- 場所:
  - v0.1.0 SPECS: `docs/v0.1.0/SPECS_JA.md:2336-2356`（Tree Depth と最大発行数・コスト表、Depth 選定の擬似コード）
  - v0.1.2: 該当情報なし
- 観察: v0.1.2 では「コレクションは開発者が管理する」モデルに変わったため、Tree 設計の判断は開発者責任。しかし spec / OPERATIONS どちらにも判断材料が無い。これは v0.1.2 の境界変更によって発生した「ドキュメント空白」。
- 修正案: OPERATIONS §10「クライアント開発者向け」セクションに、cNFT mint 先 Tree の Depth 選定ガイドを 1 段落で追加する。should-fix-003 と統合可能。

### nitpick-004 v0.1.0 `crates/core/examples/{sign_one, gen_fixture, gen_phash_fixtures, gen_c2pa_fixtures}.rs` 等の開発支援サンプルが v0.1.2 では完全に消失

- 場所:
  - 既存: `legacy/v0.1.0/crates/core/examples/`（4 サンプル）
  - 現状: `crates/core/` 配下に examples/ ディレクトリなし
- 観察: examples/ は「この crate を試したい新規開発者」が最初に読む場所。`gen_c2pa_fixtures.rs` のような C2PA テストフィクスチャ生成スクリプトは、processor の新規実装時に再利用価値がある。
- 修正案: `crates/core/examples/` を復活させ、最低 1 つ（例: c2pa-verify を JPEG 1 枚に対して走らせる簡易 example）を提供する。OSS の hello-world として機能する。

## 全体所感

v0.1.2 は CHANGELOG が宣言する通り full rewrite であり、設計コンセプトの簡素化（trust model の単純化、攻撃面の縮小）は良い方向。一方で「失った機能の説明責任」が不十分で、特に CHANGELOG の Removed セクションは実態の半分しか書かれていない（must-fix-003）。「意図的に削った」と「うっかり落ちた」を読者が判別できない状態は OSS として致命的で、自分で legacy/ を grep して比較する手間を読者全員に強要している。SPECS §3.2 で約束した 6 processor のうち実装が 1 つしかない件（must-fix-001）と、TSA タイムスタンプ抽出の silent removal（must-fix-002）は ship 前に解決すべき。良いニュースは、ResourcePool や JUMBF パーサ等の中核実装は legacy から丁寧に移植されており（コメントで明示）、品質後退ではない点。
