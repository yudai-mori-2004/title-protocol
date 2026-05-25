# K3 Round 2: `crates/tee` 縦深掘り監査

## サマリ

Round 1 で 31 件 (must:6 / should:14 / nitpick:11) 指摘。Round 2 で実コードを再走査した結果:

- **解消**: 21 件 (must:4 / should:9 / nitpick:8)
- **部分対応 (回帰なし)**: 4 件 (must:1 / should:3)
- **未対応**: 6 件 (must:1 / should:2 / nitpick:3)
- **新規発見**: 8 件 (must:1 / should:4 / nitpick:3)

Round 1 の中核 4 must (起動順 / process_request 肥大 / unsafe Send+Sync / chunk×global timeout 衝突) のうち 3 件 (#001, #003, #005) は解消。残る #002 (process_request 肥大) は構造的には未着手 (encryption の早期 reject などで多少改善はしたが本体は単一関数のまま)。must-fix-006 (Drop 順 / graceful shutdown) は未対応。

総合所感: 仕様逸脱・健全性レベルの問題はほぼ片付き、残課題は (a) `process_request` の責務分割、(b) graceful shutdown の in-flight 待機、(c) `ContentFetcher` の streaming I/F 化 (Round 1 should-002 の根本対応)、(d) 新規発見の `extension` 経路の admission control 抜けと async/blocking 混在。

---

## Part 1: Round 1 指摘の処理状況

### must-fix

| # | 件名 | 状況 | 備考 |
|---|---|---|---|
| 001 | 起動順序 §5.2 逸脱 | 解消 | `lib.rs:1-15` doc が削除され、`main.rs:5-13` に正しいステップ列を再配置。Self-attestation は Step 3 として明示昇格、Solana 鍵生成の直後に物理的に配置。 |
| 002 | `process_request` 肥大 | 未対応 (回避策のみ) | 関数本体は 100 行弱で 7 ステップを直列。`decrypt_single_payload` のみ分離。残りは依然オーケストレータ単一関数。 |
| 003 | `FakeNsm` の `unsafe impl Send+Sync` | 解消 | `vendor/aws.rs:175-178` で `RefCell` → `Mutex` に置換、`unsafe impl` 削除。 |
| 004 | `set_*_timeout` のエラー無視 | 解消 | `proxy_fetcher.rs:103-114, 125-136` で TCP/vsock 両分岐とも `.map_err(...)?` で伝播。 |
| 005 | `compute_global_timeout(0)` の BASE 衝突 | 解消 | `limits.rs:58-65` で `Option<u64>` 化、`None → MAX_GLOBAL_TIMEOUT`。`orchestrator.rs:184-190` で `Fragmented` は `count × MAX_FRAGMENT_SIZE` をヒント、`Single`/`Sidecar` は `None`。`try_admit` / `ticket` のシグネチャも `Option<u64>` に統一。 |
| 006 | Drop 順 / graceful shutdown 未保証 | 部分対応 | `RealNsm::drop` に `tracing::debug!` 追加 (`vendor/aws.rs:51-61`)。in-flight 待機は `axum::serve(..).with_graceful_shutdown(...)` のまま — Round 1 で指摘した hyper-util graceful の導入は未着手。新たに `NitroRuntime` doc (`vendor/aws.rs:117-125`) に「`Arc` を共有する場合は in-flight が解決してから drop」と注意書きが追加されたが、強制機構はなく操作者規律依存。 |

### should-fix

| # | 件名 | 状況 | 備考 |
|---|---|---|---|
| 001 | フラグメント全 concat 仕様逸脱 | 部分対応 → task 19 で trait は streaming 化 (fixed-partial) | `content_fetch.rs:255-258, 389-392` でコメントは「peak = init + Σ fragments」に書き直され「将来最適化」表現は消えた。実装は依然 `combined.extend_from_slice` で全 fragment を保持。仕様 §4.3 の「extend → 検証 → shrink」ループは未実装。**task 19 (2026-05)**: `FetchedContent` を `ContentSource` ベース (Read+Seek factory) に抽象化。single 入力は `HttpRangeSource` / `ProxyRangeSource` 経由でストリーミング完了。fragmented は `c2pa::Reader::with_fragment(init, fragment)` API への置き換えが残作業 (v0.1.3 で別 task)。 |
| 002 | 漸進予約が事後カウンタ化 | 未対応 → task 19 で fixed | `HttpContentFetcher::fetch` (`content_fetch.rs:166-243`) は依然 `Vec<u8>` で完全展開した「あと」に `ticket.extend` を呼ぶ構造。`ContentFetcher` トレイトの streaming 化は未着手。`max_body_bytes` cap が唯一のメモリ防御線という構図は不変。**task 19 (2026-05)**: `ContentFetcher::fetch_streaming` を追加、Range Request 経路で `HttpRangeSource` / `ProxyRangeSource` を返す。`fetch_single` は `ContentSource::peak_memory_hint` (= reader バッファサイズ 64 KB) で予約。50 GB streaming reservation テストで実証。Range 非対応サーバーは full fetch にフォールバック (従来通り `max_body_bytes` cap で守る)。 |
| 003 | Mock measurement bypass | 解消 | `lib.rs` の `tests::MockRuntime` を削除し `runtime/mock/MockRuntime` に統一。Self-attestation のログ (`main.rs:114-119`) で `tee_type` を併記しており mock 環境が視認可能。 |
| 004 | `decrypt_single_payload` の早期 reject | 解消 | `orchestrator.rs:172-177` に Step 0 として encryption × non-Single を fetch 前に reject。 |
| 005 | reqwest async/blocking 混在 | 部分対応 | `handle_process` (`server.rs:142-154`) は `spawn_blocking` でラップ済み。一方 `handle_solana_extension` (`server.rs:244-252`) は依然バニラ async 内で blocking `fetcher.fetch` を呼ぶ。**新規発見 #2 として再掲**。 |
| 006 | extension fetch サイズ未検証 | 解消 | `server.rs:243-264` で `MAX_OFFCHAIN_DATA_BYTES = 1 MiB` の post-fetch チェックを追加。ただし fetch 前に admission/ticket を取得しない点は残課題 — **新規発見 #1** で再掲。 |
| 007 | Json 抽出器のサイズ上限 | 解消 | `server.rs:79-96` で `DefaultBodyLimit::max(64 KiB)` を `/process` と `/extension/solana` 双方に layer 適用。 |
| 008 | `data_size_hint=0` 区別不能 | 解消 | should-005 と統合解消 (`Option<u64>` 化)。 |
| 009 | NSM GetRandom 0 バイト無限ループ | 解消 | `vendor/aws.rs:71-76` で `random.is_empty()` 時に `RandomFailed` で抜ける。 |
| 010 | `RealNsm::drop` 失敗握りつぶし | 解消 (緩和) | `vendor/aws.rs:54-58` のコメントで「返り値なし」を明示、`tracing::debug!` で fd ログ。idempotency は doc 化。 |
| 011 | `MockRuntime` 3 重実装 | 部分対応 | `lib.rs` 側の `tests::MockRuntime` は削除され `StubRuntime` のミニ版に置換。`orchestrator.rs:441-474` の Mutex 付き `MockRuntime` は `last_user_data` 観測のため残存。`runtime/mock.rs` と統合する余地はあるが、テスト用ユーティリティの観測 API を pub にする副作用と引き換えなので残置は妥当。 |
| 012 | `expected_measurement` Vec → Box | 解消 | `server.rs:67` で `Box<[u8]>`、`main.rs:113` で `.into_boxed_slice()`、起動ログに `measurement_len` 出力 (`main.rs:117`)。 |
| 013 | octet-stream silent fallback | 解消 | `content_fetch.rs:318-322` で `tracing::warn!` を追加。 |
| 014 | proxy `write_string(... url, url)` 紛らわしさ | 未対応 | `proxy_fetcher.rs:152-154` は同形のまま。可読性ノイズで影響は軽微。 |

### nitpick

| # | 件名 | 状況 |
|---|---|---|
| 001 | lib.rs Legacy参照 | 解消 (`lib.rs:1-7` に doc 簡潔化) |
| 002 | TeeRuntime v0.1.0 変更点 | 解消 (`lib.rs:34-38`) |
| 003 | Cargo.toml feature 冗長 | 部分対応 (`Cargo.toml:14-18` で 3 行に短縮、まだ rationale が残る) |
| 004 | rand_chacha コメント | 未対応 (`Cargo.toml:48-51`、3 行のまま — Round 1 指摘の「歴史的妥当性主張」が残る) |
| 005 | vsock Linux-only コメント | 部分対応 (`Cargo.toml:56-58` で 3 行 → 2 行に短縮) |
| 006 | proxy_fetcher 冒頭 doc 二重 | 部分対応 (`proxy_fetcher.rs:3-14` でモジュール冒頭は維持、`ProxyEndpoint::parse` doc は短縮済み) |
| 007 | `FETCH_TIMEOUT` 長文 doc | 解消 (`content_fetch.rs:131-133` で 2 行) |
| 008 | resource_pool 「legacy v0.1.0」参照 | 解消 (`resource_pool.rs:1-21` に書き直し済み、legacy 言及なし) |
| 009 | `SS` prefix 混在 | 解消 (`orchestrator.rs` / `content_fetch.rs` / `resource_pool.rs` / `limits.rs` の `SS` 検索 0 件、全て `§` に統一) |
| 010 | `(503)` がエラー文に焼き付け | 未対応 (`orchestrator.rs:63` で依然 `"... (503)"`) |
| 011 | `hex_short` 冗長 | 解消 (`main.rs:116` で `hex::encode(&expected_measurement[..min(8)])`、`hex_short` 関数削除) |

---

## Part 2: 修正で生じた / 残存する新規問題

### must-fix-r2-001 `process_request` Step 0 の reject が `Sidecar` 入力をブロックしている — 仕様の柔軟性を縮減

- 場所: `crates/tee/src/orchestrator.rs:172-177`
- 観察:
  ```rust
  if request.encryption.is_some()
      && !matches!(request.input, InputData::Single { .. })
  {
      return Err(OrchestratorError::EncryptionUnsupportedForInputType);
  }
  ```
- 問題: Round 1 should-fix-004 の修正自体は正しいが、エラーメッセージは `"encryption is only supported for input_type=\"single\" in this protocol version"` (l. 92-94)。一方仕様 §2.4 を再走査すると「encrypted 単体」と「sidecar の manifest を含めた encrypted」は将来サポートを示唆する書き方になっており、現実装の挙動 (= sidecar/fragmented + encryption をすべて 400 で reject) と「protocol version」表現には齟齬がある。仕様改訂を経ないリリースなら、メッセージは「current protocol version」「sidecar / fragmented encryption is not implemented」のいずれかに整え、`OrchestratorError` のバリアント名 (`EncryptionUnsupportedForInputType`) も `EncryptionRequiresSingleInput` のような実装事実ベースに改名するのが正道。
- 影響: 仕様ドキュメントを読んだ開発者が「いずれサポートされる」と期待してインプリ依頼を出した場合、エラーメッセージとの食い違いで混乱が生じる。
- 修正案: (a) メッセージを `"encryption with fragmented/sidecar input is not implemented (this protocol version supports it only for single)"` に書き直す、または (b) 仕様書 §2.4 側に「encryption は当面 single 限定」を明記してから揃える。どちらか一方を選んで両者を一致させる。

### should-fix-r2-001 `handle_solana_extension` で blocking fetcher を async runtime 内で直接呼ぶ — async runtime ブロック

- 場所: `crates/tee/src/server.rs:244-252`
- 観察:
  ```rust
  let offchain_resp = state
      .fetcher
      .fetch(&body.offchain_data_url)
      .map_err(|e| ...)?;
  ```
  `fetcher` は `HttpContentFetcher` (`reqwest::blocking::Client`) または `ProxyContentFetcher` (`TcpStream::read_exact`)。どちらも同期 blocking。
- 問題: `handle_process` (l. 142-154) は同じ blocking 呼び出しを `tokio::task::spawn_blocking` でラップして async runtime をブロックしないように配慮しているが、`handle_solana_extension` は async ハンドラ本体で直接 `fetch` を呼ぶ。`HttpContentFetcher::FETCH_TIMEOUT = 60s` のため最悪 60 秒間 tokio worker thread をブロックする。同じファイル内で対処の有無が分かれており、Round 1 should-005 の中途半端な修正の結果になっている。
- 修正案: `handle_solana_extension` 全体を `spawn_blocking` でラップするか、せめて `state.fetcher.fetch(...)` の呼び出しを `tokio::task::spawn_blocking({ let state = Arc::clone(&state); let url = body.offchain_data_url.clone(); move || state.fetcher.fetch(&url) }).await??` の形にする。

### should-fix-r2-002 `handle_solana_extension` の fetch が ResourcePool admission を bypass

- 場所: `crates/tee/src/server.rs:244-252`
- 観察: `handle_process` 系は `pool.try_admit(...)` を経由するが、`handle_solana_extension` は admission を取らずに直接 fetcher を起動。fetch のサイズ上限 (`MAX_OFFCHAIN_DATA_BYTES = 1 MiB`) は post-fetch チェックなので、fetcher 内部の `max_body_bytes` (= 100 MiB) まではメモリを使ってしまう。
- 問題: 100 同時 extension リクエストで 10 GB のヒープ確保が起き得る (TEE インスタンスメモリを超える)。`POOL_TOTAL_LIMIT = 512 MB` のはずだが、extension 経路はこのガードを通らない。攻撃者は `/extension/solana` を選んで実行することで `/process` の admission を回避できる。
- 修正案: (a) `MAX_OFFCHAIN_DATA_BYTES` を `HttpContentFetcher::with_max_body_bytes` に渡せるように `state.fetcher` 構築を分け、extension 用に別 fetcher (1 MiB cap) を持つ。または (b) `handle_solana_extension` の冒頭で `state.pool.try_admit(Some(MAX_OFFCHAIN_DATA_BYTES as u64))` を取り、`ticket.extend(offchain_resp.body.len())` を呼んでから JSON deserialize する。(b) が ResourcePool の意図にも合致する。

### should-fix-r2-003 Step 5 の `compute_signature_hash` がメモリ確保された `content_bytes` を再走査

- 場所: `crates/tee/src/orchestrator.rs:218-224`
- 観察: 暗号化なし経路では `fetched.content_bytes` がそのまま `content_bytes` に流れる。`compute_signature_hash(&content_bytes, &content_type)` で再度バイト列全体を読み、C2PA 解析 → SHA-256。
- 問題: ロジックそのものは正しいが、`process_request` の責務肥大 (must-fix-002 残存) と合わせて、ヒープ上の 100 MiB JPEG を 3 周 (fetch / signature_hash / processor) 舐めるコールパスになっており、CPU/メモリ帯域の観点で非効率。Round 1 should-002 の streaming I/F に揃えて、`compute_signature_hash` も `&[u8]` ではなく `impl Read` を受ける形に揃える長期改修ターゲット。
- 修正案: 短期は doc コメントで「データ全走査が 3 回発生する」を明示。長期は `title-core::compute_signature_hash` を `Read` 受けに変更し、`Vec<u8>` を持ち回さない設計へ。

### should-fix-r2-004 `handle_process` の `Err(_)` 分岐が全エラーを 400 BAD_REQUEST に潰している

- 場所: `crates/tee/src/server.rs:176-184`
- 観察:
  ```rust
  Err(orchestrator::OrchestratorError::AdmissionRejected) => Err((503, ...)),
  Err(e) => Err((StatusCode::BAD_REQUEST, ...)),
  ```
- 問題: `AttestationFailed` (TEE ハードウェア故障)、`JcsFailed` (内部 serde バグ)、`JsonError` (内部 serialize 失敗)、`ResponseSealFailed` (KEM/AEAD 失敗) は 500 Internal Server Error が正しい。一方 `FetchFailed` (上流ストレージの 404 など) も 400 で返るが、これは Gateway 視点では「クライアントが指定した URL が読めない」= 502 BAD_GATEWAY が妥当。エラー分類が雑で、Gateway 側のリトライ判断ができない。
- 修正案: `OrchestratorError` を `IntoResponse` 実装 (または専用マッピング関数) で個別に StatusCode に対応付ける:
  - `AdmissionRejected` → 503
  - `FetchFailed(...)` → 502 (上流要因)
  - `AttestationFailed` / `JcsFailed` / `JsonError` / `ResponseSealFailed` → 500
  - `EncryptionUnsupportedForInputType` / `EncryptionSuiteMismatch` / `PayloadMetadataInvalid` / `SignatureHashMismatch` / `DecryptionFailed` / `SignatureHashFailed` → 400

### nitpick-r2-001 `orchestrator.rs` Step 番号の整合性ズレ

- 場所: `crates/tee/src/orchestrator.rs` 全体
- 観察: モジュール冒頭 doc (l. 6-18) は「1. Admit → 2. Fetch → 3. Compute signature_hash → 4. Ensure c2pa-verify → 5. Execute processors → 6. Assemble → 7. JCS hash → 8. Attestation → 9. ProcessResponse」の 9 ステップ。一方 `process_request` 本体は Step 0/1/2/3/4/5/6/7/8 のコメントを振っているが、Step 4 (signature_hash) が doc 上は Step 3、Step 6 (processor list) が doc 上は Step 4、というように 1 ステップ ずれている (Step 0 = 早期 reject を後付けしたため)。
- 修正案: モジュール冒頭の doc に「Step 0: Pre-flight validation (encryption × input type)」を加えるか、`process_request` 内のコメントの番号を doc に揃える。

### nitpick-r2-002 `orchestrator.rs:260` の「Ticket is dropped here」コメント

- 場所: `crates/tee/src/orchestrator.rs:260`
- 観察:
  ```rust
      }
      // Ticket is dropped here, releasing all reserved memory.
  }
  ```
- 問題: `match` の閉じ括弧直後にコメント。`ticket` は `let ticket = pool.try_admit(...)?` で得られ、関数末尾でスコープアウト。RAII は明示しなくても Rustacean には自明。Round 1 で指摘した「やらなかった理由 / 言わずもがな」rationale の典型例で、4.7 癖の残響。
- 修正案: 削除。残すなら `ticket` 変数の宣言箇所 (l. 191-193) に `// Reserved memory released when the function returns.` を 1 行で。

### nitpick-r2-003 `proxy_fetcher.rs` の `write_string(&mut socket, url, url)` 紛らわしさ (Round 1 should-014 残存)

- 場所: `crates/tee/src/proxy_fetcher.rs:152-154`
- 観察: 既述。`write_string(w, value, url_for_err)` で `value` と `err_context` が両方 URL になる 2 行目を残存。
- 修正案: Round 1 修正案を再掲。`write_method` / `write_url` / `write_body` の薄ラッパで意図を表現。

---

## Part 3: 全体所感

Round 1 で挙げた「safety / 仕様逸脱 / コメント癖」のうち、コード健全性 (unsafe Send+Sync, NSM 無限ループ, JSON body cap, timeout 衝突) と doc 衛生はほぼ刈り取られた。残る大物は 3 つ:

1. **`process_request` の責務分割 (must-002)**: 仕様 §5.2 の 9 ステップを 9 関数に分けるリファクタは未着手。Step 0 (encryption pre-flight) の挿入で番号体系が doc とずれ始めており、いずれ着手すべき。
2. **`ContentFetcher` の streaming I/F 化 (should-002)**: ResourcePool が事後カウンタになっている根本問題。`HttpContentFetcher` は内部で 64 KB ループまではしているのでフックポイントは存在 (`content_fetch.rs:216-235`)。トレイトに `fn fetch_streaming(&self, url, on_chunk: &mut dyn FnMut(&[u8]) -> Result<(), FetchError>) -> Result<FetchMetadata, FetchError>` を生やし、`Ticket::extend` を chunk ごとに呼ぶよう改める。
3. **`/extension/solana` 経路の admission control 抜け (新規 should-r2-002)**: 同じ TEE 内で `/process` だけがメモリ管理されており、extension は無防備。同じプール経由に揃えるべき。

加えて must-006 (graceful shutdown) は実機を 1 度叩いてみないと顕在化しにくいので、実機検証タスクで in-flight `/process` が SIGTERM で打ち切られないか確認する integration test の整備を推奨。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| must-fix-001/003/004/005 | fixed | Round 2 認定済み。 |
| must-fix-002 | wontfix(`process_request` 9 ステップ分割は 200+ 行のリファクタで、ステップ間の型受け渡しを大きく変える必要がある。Step 0 追加で番号体系のズレは生じたが、Round 2 nitpick-r2-001 のドキュメント番号修正で吸収) | |
| must-fix-006 | wontfix(graceful shutdown の hyper-util 移行は axum メジャー API 変更を伴う。`NitroRuntime` 共有 Arc の drop タイミング注意書きは追加済みで運用上カバー) | |
| should-fix-001/002 | wontfix(`ContentFetcher` streaming I/F 化は大規模リファクタ。現状 ResourcePool の事後カウンタは `MAX_BODY_BYTES` と組み合わせて防御線として機能) | |
| should-fix-003/004/006/007/008/009/010/011/012/013 | fixed | Round 2 認定済み。 |
| should-fix-005 | fixed | should-fix-r2-001 と統合対応。`handle_solana_extension` でも `spawn_blocking` ラップを適用し async/blocking 混在を解消。 |
| should-fix-014 | wontfix(`write_string(w, url, url)` の引数重複は API ヘルパ細分化が必要で価値が薄い) | |
| nitpick-001/002/007/008/009/011 | fixed | Round 2 認定済み。 |
| nitpick-003/004/005/006 | wontfix(Cargo.toml / docstring の冗長コメントは将来 OSS 公開時に一括整理) | |
| nitpick-010 | wontfix(`"... (503)"` をエラー文字列に焼き付けるのはログ閲覧時の即時識別性のため意図的) | |
| must-fix-r2-001 | fixed | `OrchestratorError::EncryptionUnsupportedForInputType` を `EncryptionRequiresSingleInput` に改名し、エラーメッセージを「fragmented/sidecar encryption is not implemented」に書き直した。「protocol version」の曖昧表現を排除し、実装事実ベースに整列。 |
| should-fix-r2-001 | fixed | `handle_solana_extension` の `state.fetcher.fetch(...)` を `tokio::task::spawn_blocking` でラップ。tokio worker thread が最大 60 秒ブロックする経路を解消。 |
| should-fix-r2-002 | fixed | `handle_solana_extension` 冒頭で `state.pool.try_admit(Some(MAX_OFFCHAIN_DATA_BYTES))` を取得。fetch 完了後に `drop(ticket)`。`/extension/solana` から `POOL_TOTAL_LIMIT` を bypass される経路を塞いだ。 |
| should-fix-r2-003 | wontfix(`compute_signature_hash` を `impl Read` 受けに変える長期改修は title-core の trait シグネチャ変更を伴い、現状の Vec ベース API への影響範囲が広い。短期は streaming I/F 化と一緒に v0.1.3 で再検討) | |
| should-fix-r2-004 | fixed | `server.rs::orchestrator_error_to_response` を新設し、`OrchestratorError` 各 variant を 502/503/500/400 に分類マッピング。`process_fetch_failure` テストも 502 を期待するように更新。Gateway 側のリトライ判断が可能になった。 |
| nitpick-r2-001 | fixed | `orchestrator.rs` モジュール冒頭の Step リストに Step 0 (encryption pre-flight) を追加し、Step 3 (decrypt) も加えて 10 ステップに再番号付け。本体コメントとの整合確認済み。 |
| nitpick-r2-002 | fixed | `process_request` 末尾の `// Ticket is dropped here, releasing all reserved memory.` を削除（RAII は Rustacean に自明）。モジュール冒頭の解説は維持。 |
| nitpick-r2-003 | wontfix(should-fix-014 と同じ理由) | |
