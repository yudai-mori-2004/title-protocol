# Task 19 — Streaming content fetch (HTTP Range Request)

**ステータス**: planning (初見エージェントへの引き継ぎ用文書)
**前提**: Round 3 監査完了後、c2pa-verify の強制注入を外した直後の `main` ブランチ
**緊急度**: 本番運用ブロッカー。50 GB 級の動画 (`single` 入力 / MP4) を扱う必要があり、現状実装はメモリで破綻する

---

## なぜこのタスクが必要か

現状の `crates/tee/src/content_fetch.rs::fetch_single` は content_url の本体を `Vec<u8>` に全バッファしてから processor に渡している。10 GB / 50 GB の動画は載らない。

ところが仕様 (`docs/v0.1.2/SPECS_JA.md` §4.3「メモリ管理」) はこの問題を**既に解いた前提で書かれている**:

- ピークメモリ = 「マニフェスト + 現在処理中のチャンク 1 個分」
- HTTP Range Request で必要な部分だけ取得し、`Ticket::extend` / `Ticket::shrink` で pool 残量を増減させる

つまり SPECS が要求している水準と実装が乖離している。Round 1 / 2 / 3 監査でも `crates/tee/src/audit/round*/k3-tee.md` の should-fix-001 / 002 が「streaming I/F 化は v0.1.3 持ち越し」と書いてきたが、ユーザーから「50 GB が明日来るので v0.1.3 ではなく今必要」と明示された (Round 3 完了直後のフィードバック)。

その指摘を受けて本 task を起こすが、変更は workspace を 5 crate またぐ重い refactor で、Round 3 完了直後の context では 1 セッションで安全に通せないと判断したため、**新しい session でフルアテンションを当てて進める**。

---

## 最初に読む順番 (鵜呑み禁止、必ず一次資料に当たること)

1. `CLAUDE.md` — プロジェクト全体規約
2. `docs/v0.1.2/SPECS_JA.md` §4.3「メモリ管理」(L920-970 付近) — 本タスクが満たすべき要件の Source of Truth
3. `docs/v0.1.2/SPECS_JA.md` §1.3「入力形式」+ §3.1「Processor 概要」 — single / fragmented / sidecar の区別と processor 契約
4. `docs/v0.1.2/COVERAGE.md` の §4 Memory Management 周辺 — Range Request streaming が「future optimization」と既明記されている事実を確認 (= SPECS と実装の乖離を文書側もすでに知っていた)
5. `docs/v0.1.2/audit/round1/k3-tee.md` should-fix-001 / 002 と、`audit/round2/k3-tee.md` / `audit/round3/k3-tee.md` の同じ id を時系列で追う — 何故 3 ラウンド先送りされたか、何が技術的な障害だったかが書いてある
6. `legacy/v0.1.0/` の content fetch 系コード (`legacy/v0.1.0/crates/tee/src/` 配下) — v0.1.0 でどう streaming を扱っていたか (sandbox/01 で Range Request 試作した履歴も含む。**設計の参考になる可能性があるが、v0.1.0 は WASM 経路があり構造が違うので鵜呑み禁止**)
7. **c2pa-rs (v0.84) のドキュメントと `Reader` API**: 公式 docs.rs と GitHub README を当たる。特に `Reader::from_stream` / `Reader::with_stream` が `Read + Seek` を要求すること、`fragmented` 入力で `init` セグメント + 各 `media` セグメントを別々に投入する API があるかを公式から確認する。**訓練データの知識は古いので必ず最新 docs を読むこと**
8. `http-range-client` crate の docs.rs — `HttpReader` が `Read + Seek` を実装するか、サーバー側が `Accept-Ranges: bytes` 必須かを確認
9. `crates/tee/src/content_fetch.rs` 現実装 (特に `fetch_single`、`HttpContentFetcher`) と `crates/tee/src/proxy_fetcher.rs` (`ProxyContentFetcher`、wire protocol)
10. `crates/tee/src/resource_pool.rs` の `Ticket::extend` / `Ticket::shrink` (既に shrink API があるかも要確認、無ければ追加が要る)
11. `crates/core/src/processor.rs` の `Processor` trait — signature 変更が必要な範囲を見定める
12. `crates/core/src/c2pa_verify.rs` と `crates/core/src/rootlens_license_v1.rs` — 現行 processor の実装が `Read + Seek` 化でどう書き換わるか把握

順序はあくまで推奨。SPECS → 監査ログ → c2pa-rs 公式 docs → 現実装 という流れで「要件 → 過去議論 → 外部前提 → 現状」を順に固めてから refactor 計画に入るのが推奨。

---

## c2pa 仕様で必ず確認してほしいこと

**鵜呑み禁止**。本 README を書いた agent (私) も c2pa の最新仕様を完全に把握していない。以下は調査すべき項目で、調査結果でこの README を上書きしてほしい。

1. **single と fragmented の正確な定義** (C2PA 2.x の最新仕様で):
   - `single` 入力 (MP4 / JPEG / PNG) は「ファイル全体に 1 つの JUMBF box」が埋め込まれているのか、それとも mp4 box 構造の途中に置かれているのか
   - JUMBF box の位置はファイル先頭 ~1 MB に必ず入るのか (Range Request で先頭だけ取れば検証できるのか)
   - ハードバインディング (full-file hash) の計算で c2pa-rs の `Reader::with_stream` が `Seek` をどう使うか
2. **fragmented (CMAF) の検証フロー**:
   - init segment にマニフェスト、各 media segment に hash chain が入る
   - c2pa-rs は segment ごとに `Reader` を継続できるか、それとも 1 Reader で全 segment 走らせるか
3. **MP4 (single 扱い) を Range Request で検証できる確証**:
   - moov box が先頭にあるとき (faststart MP4) と末尾にあるとき (通常の MP4) で挙動が違う
   - 後者の場合は末尾の moov を取りに行く Range が必要 (file size を `HEAD` で取得 → 末尾 Range)
4. **Title Protocol が受け入れるべき MP4 のサブセット**:
   - faststart only か、両方扱うか
   - 仕様側で制約をかける選択肢もある

これらの結果を踏まえて、本 README の「実装計画」を書き換えること。

---

## 影響する layer (高レベル)

`crates/` を crate 単位で見たときに、変更が必要なのは以下。各 layer の細部設計は調査後に上書きしてほしい。

| Layer | 何が変わるか (粒度: high-level) |
|---|---|
| `crates/core/src/processor.rs` | `Processor::process` の入力型を `&[u8]` から streaming 抽象に変更 |
| `crates/core/src/c2pa_verify.rs` | `c2pa::Reader::with_stream` 経由に書き換え |
| `crates/core/src/rootlens_license_v1.rs` | 同上 (Task 18 と一体) |
| `crates/core/src/c2pa_verify.rs::compute_signature_hash` | streaming 入力対応 |
| `crates/tee/src/orchestrator.rs` | content_bytes を Vec<u8> で持たない経路に。Ticket の shrink を pipeline に組み込む |
| `crates/tee/src/content_fetch.rs` | `fetch_single` が streaming reader を返す形に。HTTP Range Request 経由の reader を実装 |
| `crates/tee/src/proxy_fetcher.rs` | proxy 経由でも Range Request を伝搬する必要 (= proxy crate の wire protocol 拡張) |
| `crates/proxy/src/protocol.rs` + `handler.rs` | wire format が現状 `[method][url][body]` のみ。Range Request 表現 (header) を載せる必要 |
| `crates/tee/src/resource_pool.rs` | `Ticket::shrink` API (or 同等) の有無確認、無ければ追加 |
| 各 crate の `tests/` / `#[cfg(test)]` | テスト fixture は `Vec<u8>` 想定。`Cursor::new(...)` で wrap する必要 |

特に **proxy 側の wire protocol 拡張**は影響が大きい。本番運用は proxy 経由 (vsock) なので、proxy が Range Request を中継できない限り 50 GB 動画は本番で動かない。

---

## 進め方の推奨

1. **Phase 0**: 上記「最初に読む順番」を全部読む。**読了後に本 README を一度上書き**して、自分の理解で要件・前提・調査結果を更新する (次に来る人がさらに迷わないように)
2. **Phase 1**: c2pa 仕様調査結果を踏まえて、Processor trait の新 signature を確定する。サンプル processor (c2pa-verify) で 1 つだけ書き換えて動くことを確認
3. **Phase 2**: orchestrator の compute_signature_hash と pipeline を streaming 化。テスト fixture を `Cursor` wrap で通す
4. **Phase 3**: content_fetch (direct, mock / dev 経路) に Range Request reader を追加。50 GB 動画を mock fetcher 経由で通せることを test で確認
5. **Phase 4**: proxy 経由の Range Request サポート。proxy wire protocol を拡張、`ProxyContentFetcher` を Range Request 対応に
6. **Phase 5**: Ticket shrink + memory accounting の検証。実機 (AWS Nitro) で 50 GB 動画を流して `POOL_TOTAL_LIMIT` 内で完走するか測定

各 Phase ごとに `cargo test --workspace` を通すこと。Phase 3 完了で「dev 環境で 50 GB 動画が通る」が達成、Phase 4 完了で「本番 Nitro でも通る」が達成。

---

## 既存インフラの確認ポイント

以下は「もしかしたら既に使える」可能性のあるもの。**ただし鵜呑みせず、実コードで確認すること**。

- `crates/tee/src/resource_pool.rs` に `Ticket::shrink` 系 API が**既に**あるか? Round 2 / 3 監査では `extend` / `extend_unchecked` のみ確認している
- `legacy/v0.1.0/` に streaming 経路の試作が残っているか
- `sandbox/01-c2pa-range-request/` のディレクトリが**git に残っているか**消されているか。Round 3 監査では「sandbox は post-verification で削除済み」と記録があるが、git log で `sandbox/01` を grep すれば復元コミットが見つかるかも
- `http-range-client` crate がすでに `Cargo.toml` workspace dep に入っているか

---

## 監査で関連する findings

実装に着手したら、関連する監査 finding の処理ログも同時に更新する (これらは「v0.1.3 持ち越し」と書かれているが、本 task で実質解消する):

- `docs/v0.1.2/audit/round1/k3-tee.md` should-fix-001 (フラグメント全 concat) / should-fix-002 (漸進予約が事後カウンタ化)
- `docs/v0.1.2/audit/round2/k3-tee.md` の同 id
- `docs/v0.1.2/audit/round3/k3-tee.md` の同 id

完了後、各 round の k3-tee.md の処理ログを `fixed (task 19 で対応)` に書き換える。

---

## 進め方 (重要)

このタスクは **「初見の agent が本 README だけ読んで迷わず動ける」** を目標に書いてある。実装に着手する agent は以下のサイクルを回すこと:

1. 「最初に読む順番」を全部読む
2. **本 README を自分の理解で上書きする**。具体的には:
   - c2pa 仕様調査の結果 (single MP4 を Range Request で検証できる根拠)
   - Processor trait の新 signature 案
   - proxy wire protocol 拡張案
   - 各 Phase ごとの具体的なファイル / 関数粒度の patch 計画
   - 既に試した case (動いた / 動かなかった) のログ
3. ユーザーと方向性を確認 (Phase 1 着手前に user confirmation を取る)
4. Phase 1 から順に実装、各 Phase ごとに commit
5. 完了後、本 README を「task done」状態に書き換えて、関連監査 finding の処理ログを更新

**しないこと**:
- v0.1.3 への先送り (ユーザーから「v0.1.3 では遅い」と明示された)
- 部分対応で「dev は通るが本番では通らない」状態で task done 扱い (Phase 4 まで完走必要)
- 単一 commit で workspace 全部書き換え (Phase ごとに分ける、テスト通過を確認しながら)

---

## メモ: なぜ Round 3 完了時点でこの task が独立してあるか

Round 3 監査の k3-tee should-fix-001 / 002 (streaming I/F 化) は私が Round 3 で「v0.1.3 持ち越し」として wontfix にした。ユーザーから「50 GB が明日来るのでそれは仕事の放棄、今必要」と明示の指摘を受け、本 task を独立タスクとして立てた。

私 (Round 3 で監査を回した agent) はそのタイミングで context が枯渇しかかっており、安全に実装を完走できないと判断したため、設計と要件を本 README に固めて新 session に引き継ぐ判断をした。本 README が**初見の agent への完全な引き継ぎ書**として機能することが、私が context を切る前の最後の責任。

次の agent へ: 上の「最初に読む順番」を真面目に追ってほしい。SPECS と c2pa 仕様の調査をサボると、proxy wire protocol 拡張のところで詰む。
