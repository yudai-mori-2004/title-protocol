# K3: `crates/tee` 縦深掘り監査

## 概要

**担当範囲**: `crates/tee/` 全ファイル — `Cargo.toml`, `src/main.rs`, `src/lib.rs`, `src/server.rs`, `src/orchestrator.rs`, `src/content_fetch.rs`, `src/proxy_fetcher.rs`, `src/resource_pool.rs`, `src/limits.rs`, `src/runtime/{mod.rs, mock.rs}`, `src/vendor/{mod.rs, aws.rs}`.

**監査方針**: 仕様書 §4 / §5.2 / §6.2 と一文単位で突合。観点は (1) 仕様準拠、(2) 起動シーケンスの順序と依存逆転、(3) 並行性 (CAS, RAII, Send/Sync)、(4) `process_request` の責務肥大、(5) feature flag の整合、(6) コメント癖 (4.7 patterns)、(7) エラーマッピング、(8) FFI ライフサイクル。

**件数サマリ**: 全 31 件 (must-fix 6 / should-fix 14 / nitpick 11)。

## 重大度別内訳

- must-fix: 6 件
- should-fix: 14 件
- nitpick: 11 件

## 発見

---

### must-fix-001 起動シーケンスの順序が仕様 §5.2 と異なる

- 場所: `crates/tee/src/main.rs:84-171`
- 観察: 仕様 §5.2「起動シーケンス」は
  1. 暗号化用鍵ペア生成
  2. **自己 Attestation Document 取得 → measurement 保持**
  3. 公開鍵を Gateway に通知
  4. リクエスト受付開始
  と定めている。一方、`main.rs` の実装順序は (1) Runtime + Verifier 選択 → (2) 鍵束 → (3) Solana 鍵 → (4) Processor 登録 → (5) ResourcePool → (5.5) Self-attestation → (6) リッスン。`lib.rs:1-15` の doc も "Self-attestation" の項目を欠いている。
- 問題: 仕様上は self-attestation が鍵生成の直後に位置する重大ステップ (失敗時は起動中止) として規定されているが、main の構造ではコメント "Step 5.5" として後付け感がある。仕様駆動の体面が崩れている。また `lib.rs` 冒頭 doc の Startup sequence が現実 (rasterize order) と乖離。
- 修正案:
  - `lib.rs:1-15` の doc を実コードと一致させ「Self-attestation」を明示ステップに昇格 (1. Runtime select / 2. Key bundle / 3. Solana key / 4. **Self-attestation** / 5. Processors / 6. ResourcePool / 7. Listen)。
  - `main.rs` の "Step 5.5" コメントを "Step 4: Self-attestation" にリネームし、self-attestation を Solana 鍵生成の直後 (= Processor 登録より前) に物理的に移動。Solana 鍵公開鍵を `user_data` に紛れ込ませない仕様だが、仕様 §6.2 では `user_data = SHA-256(Solana 公開鍵)` で別途取り直すフローが規定されているため、self-attestation そのものは Solana 鍵生成「前後どちらでも可」だが、論理的な情報依存 (rapid debug) と仕様順序の一致のため Solana 鍵生成の直後に置くのが妥当。

---

### must-fix-002 `process_request` の責務肥大 — 単一関数で 11 ステップを一手に処理

- 場所: `crates/tee/src/orchestrator.rs:161-241`
- 観察: 1 関数で admit / fetch / decrypt / signature_hash compute / mismatch check / processor list build / processor exec / JCS hash / attestation get / seal すべてを直列に書いている。70 行強で 6 つの I/O・暗号操作を抱える。
- 問題: (a) ユニットテストは E2E (`pipeline_*`) しか書けず、フェーズごとの fuzz が困難。(b) 将来 `provenance-graph` や Range Request が来た時に変更が中央集権する。(c) `OrchestratorError` バリアントが 12 個に膨張し、責務とエラーマッピングが噛み合わない。
- 修正案: 以下に分割。
  - `admit(pool) -> Ticket`
  - `materialize(request, fetcher, ticket, key_bundle) -> Materialized { content, content_type, manifest, response_channel, declared_sig_hash }` (decrypt 含む)
  - `verify_signature(materialized) -> signature_hash`
  - `run_processors(registry, processor_ids, materialized) -> VerifiableResponse`
  - `attest(verifiable, runtime) -> ProcessResponse`
  - `seal_or_return(response, channel) -> ProcessOutcome`
  - `process_request` はこれらを順に呼ぶ薄いコーディネータに留める。

---

### must-fix-003 `vendor/aws.rs` の `FakeNsm` で `unsafe impl Send + Sync`

- 場所: `crates/tee/src/vendor/aws.rs:166-169`
- 観察:
  ```rust
  // FakeNsm uses RefCell — fine for single-threaded tests. Mark Send + Sync
  // explicitly because the trait requires it.
  unsafe impl Send for FakeNsm {}
  unsafe impl Sync for FakeNsm {}
  ```
- 問題: テストコードであっても `RefCell` を含む型に `unsafe impl Sync` を付与するのは不健全。`NsmOps: Send + Sync` を `&self` で呼び出すため、`Mutex<...>` で済むケースで `unsafe` を持ち出している。誤って `FakeNsm` を本物の並行コードに mock として渡すと UB を踏む。仕様 §0.5 の "Stateless" 原則と健全性の観点で、`unsafe` の必要性を読み手に検証させるのは不適切。
- 修正案: `RefCell` を `std::sync::Mutex` に置換し、`unsafe impl` を削除。例:
  ```rust
  use std::sync::Mutex;
  struct FakeNsm {
      random: Mutex<Vec<Vec<u8>>>,
      attestation: Vec<u8>,
      last_user_data: Mutex<Option<Vec<u8>>>,
  }
  ```
  `RefCell` 依存箇所 (`.borrow_mut()`) は `.lock().unwrap()` に置換。

---

### must-fix-004 `set_read_timeout`/`set_write_timeout` のエラー無視

- 場所: `crates/tee/src/proxy_fetcher.rs:103-104`
- 観察:
  ```rust
  stream.set_read_timeout(Some(PROXY_IO_TIMEOUT)).ok();
  stream.set_write_timeout(Some(PROXY_IO_TIMEOUT)).ok();
  ```
- 問題: タイムアウト設定の失敗は黙殺。失敗するとそのソケットは「タイムアウトなし」のまま使われ、仕様 §4.4 の slow-loris 防御が成立しなくなる。さらに `vsock` 分岐ではそもそも `set_*_timeout` を呼んでいないため、Nitro 本番経路で chunk-level の TCP-layer 防御がない。
- 修正案: 両方とも `.map_err(|e| FetchError::HttpError { ... })?` で失敗を上層に伝播。`vsock` 分岐でも vsock の `set_read_timeout` (vsock 0.5 でサポート) を必ず呼び、設定不能なら起動を拒否するか、`Ticket::extend` 側の `chunk_timeout` だけが防御線である旨を doc に明記。

---

### must-fix-005 `compute_global_timeout` が CHUNK_TIMEOUT (60s) を境界として BASE_TIMEOUT と衝突

- 場所: `crates/tee/src/limits.rs:38-46, 68-73`
- 観察: `CHUNK_TIMEOUT = 60s`, `BASE_TIMEOUT = 60s`。`compute_global_timeout(0)` は `BASE_TIMEOUT = 60s` を返す。`Ticket::extend` (resource_pool.rs:232) は `chunk_timeout` も `global_timeout` も両方 60s の境界で判定する。
- 問題: `data_size_hint=0` で `try_admit` した場合 (実際 server.rs/orchestrator.rs では常に 0)、global timeout は base = 60s。一方 chunk timeout も 60s。極端な実例: 単一の大きな fetch (例えば 50MB JPEG) は通常 60s 以内に終わるが、ネットワーク条件次第で global timeout を chunk timeout より先に踏む。仕様 §4.4 「最大 30 分」のアダプティブ計算が `try_admit(0)` で死に、全リクエストが 60s で打ち切られる。
- 修正案: `server.rs:130` / `orchestrator.rs:170-173` で `try_admit(0)` を呼ぶ際、リクエストペイロードから `data_size_hint` を推定する。少なくとも `InputData::Fragmented` の `fragment_urls.len() * MAX_FRAGMENT_SIZE` を hint として渡す。または `try_admit` 内で `data_size_hint=0` の場合に `MAX_GLOBAL_TIMEOUT` を割り当てる (admission 時点でサイズ未知なら最大時間を確保) に変更し、実 fetch 開始時に再計算する。仕様 §4.4 ロジックを生かす唯一の方法。

---

### must-fix-006 `runtime` の Drop 順序未保証 — `NitroRuntime` の fd が State より先に閉じる可能性

- 場所: `crates/tee/src/main.rs:174-184` & `crates/tee/src/vendor/aws.rs:51-57`
- 観察: `TeeAppState` は `Box<dyn TeeRuntime>` (= `NitroRuntime`) を含み、`shutdown_signal()` 後に `axum::serve` から戻った段階で `Arc<TeeAppState>` の参照カウントが残っていれば State はドロップされない。`RealNsm::drop` で `nsm_exit(fd)` が呼ばれるが、in-flight タスク (= `tokio::task::spawn_blocking` の中で `runtime.get_attestation_document` を呼んでいる) が完了する前に main の Arc を落とすと、`Arc::strong_count` は減らないが、もし将来 Arc を手放す処理が増えると、fd が閉じられた後の attestation 取得で SIGBUS / EBADF を踏みうる。
- 問題: 現状は graceful shutdown と Arc 参照で守られているが、(a) `with_graceful_shutdown` は新規接続だけを止め in-flight ハンドラを await しない仕様 (axum 0.8 デフォルト)。(b) `spawn_blocking` の handle を await している間に Ctrl+C が来た場合、待機継続するが、その判定はテストされていない。
- 修正案: (a) `axum::serve` の `with_graceful_shutdown` に加え、`hyper-util` の `graceful` などで in-flight 待機を明示。(b) `RealNsm::drop` を `tracing::warn!` で fd close 失敗を記録し、`NitroRuntime` を `Arc<NitroRuntime>` でラップして強参照カウントを露出。(c) shutdown 手順をテスト (integration) で覆い、最低限 in-flight `/process` が完走することを確認。

---

### should-fix-001 仕様 §4.3 のシナリオ「init + フラグメント1個」を実装が逸脱

- **処理**: partial fixed (task 19 で trait は streaming 化、fragment 経路の c2pa-rs `with_fragment` 置換は v0.1.3)
- 場所: `crates/tee/src/content_fetch.rs:404-443`
- 観察: 仕様 §4.3 フラグメントは「extend → 検証 → shrink」のループで各フラグメント終了後に解放するパターン (ピーク = init + フラグメント1個 + Reader 内部状態) を要求。実装は「全 fragment を `combined` に concat、shrink 呼ばず」。コメント `// ## Memory pattern (SS4.3) ... Peak memory = init + all fragments.` で逸脱を自認。
- 問題: 仕様逸脱を doc で正当化しているだけで COVERAGE 上は実装済み扱いになる懸念。仕様 §4.4 の「フラグメント1個の最大 100MB × 10万個 = 10TB」を total_limit=512MB の前提で受け切れない実装になっている。
- 修正案: 二段階で。(a) 短期: `FetchedContent::content_bytes` を `Vec<u8>` から `Box<dyn Read+Seek>` に抽象化するか、`Vec<Vec<u8>>` でフラグメント単位に保持し、processor 側で順次 feed する。(b) 抽象化が大規模になるなら、せめて doc コメントから「Currently accumulates ... future optimization」表現を消し、`TODO(spec-§4.3): streaming fragment processing` で COVERAGE に明示 deviation を記録。
- **task 19 での対応**: `FetchedContent` を `Box<dyn ContentSource>` ベースに抽象化済み (修正案 (a) の前半)。`single` 入力は Range Request 経由の streaming に切り替わったため、50 GB MP4 が `peak_memory_hint = 64 KB` で通る。`fragmented` 入力は `c2pa::Reader::with_fragment(init, fragment)` API への置き換えが必要 (現在は init+全 fragment を `Vec<u8>` に concat してから `c2pa::Reader::with_stream` に流している)。v0.1.3 で `with_fragment` 経路に移行する別 task を切る前提。

---

### should-fix-002 `process_request` の Ticket admission が `extend(0)` 後の漸進予約をパスしている

- **処理**: fixed (task 19)
- 場所: `crates/tee/src/orchestrator.rs:171-173` & `crates/tee/src/content_fetch.rs:380, 419, 434, 463, 470`
- 観察: `pool.try_admit(0)` で 0 byte ticket 発行 → `fetch_content` 内で `ticket.extend(body.len())` を呼ぶ。だが `body` は既に reqwest 内部で完全にメモリに展開されている (`HttpContentFetcher::fetch` は同期 `Vec<u8>` を返す)。
- 問題: 仕様 §4.2「漸進的予約 = 実データ到着時に予約」が達成できていない。reqwest が 100MB を読み込んだ「後で」ticket.extend を呼ぶため、reqwest のバッファでメモリピークを既に踏んでいる。`max_body_bytes` cap (100MB) が事実上の唯一のガードで、ResourcePool は事後カウンタにしかなっていない。
- 修正案: `ContentFetcher::fetch` を `fn fetch_streaming(&self, url, on_chunk: &mut dyn FnMut(&[u8]) -> Result<(), FetchError>) -> Result<_, FetchError>` に変更し、`on_chunk` で `ticket.extend(chunk.len())` を呼ぶ。少なくとも `HttpContentFetcher` の 64KB ループ (l. 228-248) を Ticket-aware にする。`ProxyContentFetcher::fetch` (l. 143-147) も `read_exact` を chunked loop に変更。
- **task 19 での対応**: `ContentFetcher::fetch_streaming` を追加し、`HttpContentFetcher` / `ProxyContentFetcher` がそれぞれ `HttpRangeSource` / `ProxyRangeSource` を返す経路を実装。`fetch_single` は `ContentSource::peak_memory_hint` (= reader バッファサイズ、典型 64 KB) で `ticket.extend` するように変更。50 GB ファイルの Range Request でもメモリ予約は 64 KB のみで、`POOL_TOTAL_LIMIT` 内で完走することを `fetch_single_streaming_source_reserves_only_peak_memory` テストで検証。reqwest による事後カウンタ問題は Range Request 経路では消滅 (per-Range で必要部分だけ取る)。Range 非対応サーバーは従来の full fetch にフォールバックするが、これは仕様 §4.4 の `total_limit` で守られる範囲内。

---

### should-fix-003 `MockAttestationVerifier::MEASUREMENT` が固定値で TEE 識別を bypass する

- 場所: `crates/tee/src/server.rs:332` & `crates/tee/src/main.rs:159-167` (self-attest フロー)
- 観察: テストでは `expected_measurement = MockAttestationVerifier::MEASUREMENT.to_vec()` を固定埋め込み。production の main では `verifier.verify(&self_attestation, now)?.measurement` を採用。一方 `MockRuntime::get_attestation_document` (mock.rs:47-51) は `"mock-attestation:" + user_data` を返すだけで、measurement は埋め込まない。
- 問題: `runtime-mock` で main を起動した場合、`AttestationVerifier::verify` が "mock-attestation:" バイト列をどう解釈し measurement を返すかが不透明。コード上 `MockAttestationVerifier` の verify 実装はこの crate 内になく、`title_attestation` crate 側に依存。仕様 §6.2 で要求される「measurement 一致」が mock 環境ではトリビアルに成立する設計だが、これが development 用 fallback として透明化されていない。
- 修正案: `MockRuntime` の attestation 形式を、ヘッダに `MEASUREMENT` (固定) を含むよう変更し、`MockAttestationVerifier` がそれを抽出するように整合させる。`main.rs:151` の "Self-attestation measurement captured" ログに `tee_type` を含め、mock 環境であることを誰でも視認できるようにする。

---

### should-fix-004 `decrypt_single_payload` が `InputData::Sidecar` / `Fragmented` を弾く位置が遅い

- 場所: `crates/tee/src/orchestrator.rs:177-194`
- 観察: `fetch_content` を先に呼んでから `if let Some(suite) = request.encryption` で `decrypt_single_payload` に進み、その中で `EncryptionUnsupportedForInputType` を返す。
- 問題: Fragmented + encryption の場合、全 fragment を fetch (100MB × N) し終わってからエラーを返す。仕様 §2.4 で encryption は single のみと明記されているのだから、fetch 前に弾くべき。
- 修正案: `process_request` の Step 1 直後 (Step 1.5) で input_type と encryption の整合を validate。
  ```rust
  if let Some(_) = request.encryption {
      if !matches!(request.input, InputData::Single { .. }) {
          return Err(OrchestratorError::EncryptionUnsupportedForInputType);
      }
  }
  ```

---

### should-fix-005 reqwest async runtime と blocking client の混在

- 場所: `crates/tee/src/main.rs:134-147` & `crates/tee/src/server.rs:122-145`
- 観察: `HttpContentFetcher::new` を `tokio::task::spawn_blocking` で構築 (理由コメント: reqwest::blocking::Client が内部で tokio runtime を spawn するため async context 内で panic)。同時に `handle_process` は `spawn_blocking` で orchestrator を呼ぶ。
- 問題: (a) async サーバーの中で blocking HTTP client を blocking で叩くという 2-layer 構成。`spawn_blocking` thread pool (デフォルト 512) と reqwest 内部 runtime の thread が二重に積み上がる。(b) `extension::process_extension` (server.rs:247) はバニラの async ハンドラ内で `state.fetcher.fetch` を呼ぶ。`fetcher` が blocking なら async runtime を 60 秒ブロックする可能性がある。
- 修正案: (a) `ContentFetcher::fetch` を `async fn` に変更し、`reqwest::Client` (async) を採用。HttpContentFetcher::new から spawn_blocking を削除。(b) もしくは少なくとも `handle_solana_extension` 内の `state.fetcher.fetch` 呼び出しも `spawn_blocking` に包み込む。現状の (b) は async runtime をブロックする明確なバグ。

---

### should-fix-006 `handle_solana_extension` で fetch のサイズ・形式バリデーションがない

- 場所: `crates/tee/src/server.rs:211-228`
- 観察: `offchain_data_url` からの fetch 結果を `ProcessResponse` に直接 deserialize。サイズ制限、JSON 構造の事前 sanity check なし。
- 問題: 攻撃者が `offchain_data_url` に 100MB の JSON を仕込むと serde_json で巨大 alloc。`fetcher` が `HttpContentFetcher` なら body cap (100MB) で守られるが、Extension のレスポンスは通常数 KB のはずなので、より厳しい cap が必要。さらに ResourcePool ticket を取らずに alloc しているため、admission control も bypass。
- 修正案: extension 用に `MAX_OFFCHAIN_DATA_BYTES = 1 * 1024 * 1024` (1MB) を定義し、`fetcher.fetch` 前に ticket を取得・extend、deserialize 後に解放。または `extension::process_extension` 内部で再度サイズ check。

---

### should-fix-007 `axum::Json` 抽出器のサイズ上限が未指定

- 場所: `crates/tee/src/server.rs:122-125, 193-196`
- 観察: `Json(request): Json<ProcessRequest>` / `Json(body): Json<SolanaExtensionBody>` の body サイズに上限がない (axum 0.8 デフォルト 2MB だが、`DefaultBodyLimit` レイヤなしで暗黙依存)。
- 問題: Gateway が信頼境界とはいえ、TEE 内部 HTTP もマルウェア Gateway / proxy 取り違え経路を想定すると、明示的にレイヤ化すべき。仕様 §5.3 「Gateway は中継のみ」だが、深層防御として TEE 側で cap を持つのが正道。
- 修正案: `router(state)` で `.layer(axum::extract::DefaultBodyLimit::max(64 * 1024))` (=64 KB) を `/process` と `/extension/solana` に適用。

---

### should-fix-008 `compute_global_timeout` の `data_size_hint=0` が hint なしと「TEE 起動済みストリーミング」を区別できない

- 場所: `crates/tee/src/limits.rs:60-73` & `crates/tee/src/resource_pool.rs:103-114`
- 観察: `try_admit(0)` だと `BASE_TIMEOUT = 60s` 一定。仕様 §4.4 のアダプティブ計算が機能する条件は data_size_hint > 0。
- 問題: must-fix-005 と同根。`Option<u64>` か `enum DataSizeHint { Known(u64), Unknown }` を導入し、`Unknown` の場合は `MAX_GLOBAL_TIMEOUT` を割り当てるのが仕様に忠実。
- 修正案: `compute_global_timeout(hint: Option<u64>) -> Duration`、`None => MAX_GLOBAL_TIMEOUT`。`try_admit` / `ticket` のシグネチャを `Option<u64>` に変更。

---

### should-fix-009 NSM `get_random` ループの上限と progress check 不在

- 場所: `crates/tee/src/vendor/aws.rs:60-83`
- 観察:
  ```rust
  while out.len() < len {
      match driver::nsm_process_request(self.fd, Request::GetRandom) {
          Response::GetRandom { random } => {
              let take = (len - out.len()).min(random.len());
              out.extend_from_slice(&random[..take]);
          }
          ...
      }
  }
  ```
- 問題: NSM が `random.len() == 0` を返したら無限ループ。仕様上 GetRandom は最大 256 バイトを保証しているが、デバイス異常で 0 を返す可能性をハンドルしていない。take = 0 だが loop continue。
- 修正案: `if random.is_empty() { return Err(TeeError::RandomFailed("NSM returned 0 bytes".into())); }` をマッチアームに追加。

---

### should-fix-010 `RealNsm::drop` で `nsm_exit` の失敗を捨てている

- 場所: `crates/tee/src/vendor/aws.rs:51-57`
- 観察:
  ```rust
  impl Drop for RealNsm {
      fn drop(&mut self) {
          if self.fd >= 0 {
              driver::nsm_exit(self.fd);
          }
      }
  }
  ```
- 問題: `nsm_exit` の返り値は捨てられる。NSM 仕様上は通常 OK だが、`tracing::warn!` 程度のログは残すべき。テスト時の fd リーク発見にも有用。
- 修正案: 関数は `void` 風だが、結果型があれば `if let Err(e) = ... { tracing::warn!(?e, "nsm_exit failed"); }` を入れる。`nsm_exit` が `()` を返すなら不要だが、idempotency を文書化。

---

### should-fix-011 `lib.rs` の `tests::MockRuntime` と `runtime::mock::MockRuntime` の重複

- 場所: `crates/tee/src/lib.rs:99-143` & `crates/tee/src/runtime/mock.rs:22-61`
- 観察: `lib.rs` のテストが独自 `MockRuntime` (zero-fill random, identity attestation) を定義。`runtime/mock.rs` には公式 `MockRuntime` (OsRng random, "mock-attestation:" prefix attestation) がある。
- 問題: 監査ガイドの「死んでいるコード / 移植漏れ」観点。`lib.rs` の `tests::MockRuntime` は `runtime::mock::MockRuntime` で代替可能。さらに `orchestrator.rs:421-454` にも `MockRuntime` (Mutex でラップして user_data を記録) があり 3 重実装。
- 修正案: `runtime/mock.rs` の `MockRuntime` を拡張し、テスト用に `last_user_data` を取り出せる API を `#[cfg(test)]` で追加。`lib.rs` / `orchestrator.rs` のテスト内 MockRuntime を削除。

---

### should-fix-012 `expected_measurement: Vec<u8>` の clone 戦略不在

- 場所: `crates/tee/src/server.rs:66` & `crates/tee/src/main.rs:182`
- 観察: 起動時に取得した measurement を `Vec<u8>` で保持し、`extension::process_extension` 呼び出し時 (server.rs:252) に `Some(&state.expected_measurement)` で借用。
- 問題: (a) `&[u8]` で渡しているのは正しいが、`Vec<u8>` ではなく `[u8; 48]` (Nitro 仕様の SHA-384 固定長) または `Box<[u8]>` で確保するのが意図を表す。(b) measurement の長さがベンダーごとに異なるため (`AWS Nitro: 48 bytes`)、`Vec<u8>` のまま運用するなら `assert!` で長さ検証を起動時に入れるべき。
- 修正案: `pub expected_measurement: Box<[u8]>` に変更。`main.rs` で `verifier.verify(...)?.measurement.into_boxed_slice()`。起動ログに長さも出力。

---

### should-fix-013 `detect_content_type` のヒューリスティクスが silent fallback "application/octet-stream"

- 場所: `crates/tee/src/content_fetch.rs:289-332`
- 観察: magic bytes / server header / URL ext で判定できなければ `"application/octet-stream"` を返す。orchestrator はこの値を c2pa-verify processor に渡し、processor 側で MIME 不明として処理する。
- 問題: octet-stream で c2pa-verify が呼ばれた場合の挙動 (success / failure) がここからは見えない。仕様 §3.2 c2pa-verify は MIME を要求するため、誤検出で processor が "validation: invalid" を返す可能性。攻撃シナリオではないが、操作者の debug を困難にする。
- 修正案: octet-stream fallback に至ったケースで `tracing::warn!(url, "Could not determine content type, falling back to octet-stream")` をログ。または `FetchError::UnknownContentType` を新設して上層で reject。

---

### should-fix-014 `ProxyContentFetcher::fetch` のリクエスト方向 1 行目で `write_string(... , "GET", url)` が url を error context として渡す紛らわしさ

- 場所: `crates/tee/src/proxy_fetcher.rs:124-127`
- 観察:
  ```rust
  write_string(&mut socket, "GET", url)?;
  write_string(&mut socket, url, url)?;
  write_bytes(&mut socket, &[], url)?;
  ```
  `write_string(w, value, url_for_err)` のシグネチャ。第 2 行は value と err context が同じ url、第 1 行は value="GET" で err context="<the URL we are trying to fetch>"。
- 問題: 関数シグネチャを知らないと「メソッド名なのに URL?」と二度見する。可読性低下。
- 修正案: `write_method(w, "GET", url)` / `write_url(w, url)` / `write_body(w, &[], url)` のラッパ薄関数を追加するか、エラーコンテキストを `&str` 引数ではなく構造体 (`WireContext { url }`) で渡す。

---

### nitpick-001 `lib.rs` Doc コメント「Legacy 参照」が初見者には情報過多

- 場所: `crates/tee/src/lib.rs:16-22`
- 観察:
  ```rust
  //! ## Legacy参照
  //!
  //! `legacy/v0.1.0/crates/tee/src/runtime/` — 前バージョンのTeeRuntime実装。
  //! v0.1.0ではcrypto固有メソッド（signer, decapsulator等）がTeeRuntimeに含まれていたが、
  //! v0.1.2ではTEEハードウェア抽象化に専念し、暗号操作は別層で扱う。
  ```
- 問題: 「ない」ものの説明 + 過去経緯の埋め込み。タスク 16 README §「4.7 の癖の例」に直接該当。
- 修正案: 削除。残すなら CHANGELOG / migration guide 側に。

---

### nitpick-002 `TeeRuntime` トレイト doc「v0.1.0からの変更点」も同様

- 場所: `crates/tee/src/lib.rs:59-64`
- 観察: 「v0.1.0からの変更点 / v0.1.2ではTEEハードウェア抽象化に専念」のブロック。
- 問題: 同上。
- 修正案: 削除。

---

### nitpick-003 `Cargo.toml` の feature コメントが冗長

- 場所: `crates/tee/Cargo.toml:15-31`
- 観察:
  ```toml
  # Default ships zero runtimes — a downstream binary must explicitly opt in
  # to either `runtime-mock` (dev / CI) or a vendor runtime such as
  # `vendor-aws` (production). This makes it a compile-time error to release a
  # TEE binary that has no real runtime, and prevents the mock from being
  # accidentally selected at runtime in production builds.
  ```
- 問題: rationale が長い。READMEに置くべき設計判断。
- 修正案: 1 行に圧縮: `# Default has no runtime; pick one of runtime-mock or vendor-aws.`。詳細は `docs/v0.1.2/SPECS_JA.md §5.4` のリプロデューシブルビルド節に統合。

---

### nitpick-004 `rand_chacha` の依存コメント

- 場所: `crates/tee/Cargo.toml:52-55`
- 観察:
  ```toml
  # Used to wrap NSM-supplied seed bytes into a CryptoRng + RngCore for the
  # per-suite key-generation APIs. ChaCha20 has long been the standard CSPRNG
  # pairing with `rand` and is what the NSM-seeded TEE startup uses.
  ```
- 問題: 「ChaCha20 has long been the standard CSPRNG pairing」は読み手に判定不能な歴史。
- 修正案: `# Wraps NSM entropy into rand::CryptoRng for key-bundle generation.` に圧縮。

---

### nitpick-005 `vsock` Linux-only コメント

- 場所: `crates/tee/Cargo.toml:60-63`
- 観察:
  ```toml
  # vsock is Linux-only; gate at the target level so non-Linux dev builds
  # (Mac/Windows) don't try to compile it.
  ```
- 問題: `[target.'cfg(target_os = "linux")']` 自体が自己説明的。コメントは「Linux-only because vsock kernel module」の 1 行で足る。
- 修正案: `# vsock kernel interface is Linux-only.`

---

### nitpick-006 `proxy_fetcher.rs` 冒頭 doc 「The same crate works in pure-TCP mode for local development」

- 場所: `crates/tee/src/proxy_fetcher.rs:11-15`
- 観察: TCP loopback モードの存在意義を 4 行で説明。
- 問題: `ProxyEndpoint::parse` の doc に同じ説明があり (`l. 38-41`)、二重記述。
- 修正案: モジュール冒頭は「Used inside Nitro Enclave (vsock) or in local dev (TCP loopback). Wire protocol defined in `title-proxy` crate.」に短縮。

---

### nitpick-007 `HttpContentFetcher::FETCH_TIMEOUT` コメントが「Spec §4.4 の意図解説」を再録

- 場所: `crates/tee/src/content_fetch.rs:136-143`
- 観察: 8 行のコメント。Spec §4.4 の chunk timeout / overall timeout の使い分けを再説明。
- 問題: `const` の `///` doc としては「単一の wall-clock budget. Spec §4.4」程度で十分。詳細解説は仕様書側に置く。
- 修正案: 短縮。

---

### nitpick-008 `resource_pool.rs` doc 「Design notes (from legacy v0.1.0)」

- 場所: `crates/tee/src/resource_pool.rs:35-39`
- 観察: 「The CAS-loop pattern in `extend()` is carried forward from `legacy/v0.1.0/crates/wasm-host/src/resource_pool.rs`」
- 問題: 過去どこから来たかをコード冒頭に書いてある。初見の読み手に価値なし。
- 修正案: 削除。

---

### nitpick-009 `orchestrator.rs` の SS prefix がコメントに混在

- 場所: `crates/tee/src/orchestrator.rs:4, 27, 53, 58, ...` (合計 20 箇所以上)
- 観察: 仕様セクション参照を `SS5.2` (たぶん `§5.2` のエスケープ事故) と `§4.1` (本文では `§` 表記) が混在。
- 問題: コードを grep するときに不統一。`§` は UTF-8 だが、最初の `//! Spec SS5.2` あたりはどこかで `§` が `SS` に化けたか、機械的置換した跡 (4.7 の AI 自動編集の典型)。
- 修正案: `sed -i 's/SS\([0-9]\)/§\1/g'` 相当で `orchestrator.rs` / `content_fetch.rs` / `resource_pool.rs` / `limits.rs` の `SS` を `§` に統一。

---

### nitpick-010 `OrchestratorError` のメッセージ末尾「(503)」が混入

- 場所: `crates/tee/src/orchestrator.rs:63-65`
- 観察:
  ```rust
  #[error("Request rejected: memory admission limit exceeded (503)")]
  AdmissionRejected,
  ```
- 問題: HTTP ステータスコードを `OrchestratorError` のメッセージに焼き付けている。`server.rs` の handler 側で `StatusCode::SERVICE_UNAVAILABLE` にマッピングしているので二重情報。
- 修正案: `(503)` を削除。HTTP ステータスはレイヤ責務なので error 文字列に含めない。

---

### nitpick-011 `main.rs:233-242` の `hex_short` が `tracing` フィールドエスケープと噛み合わない

- 場所: `crates/tee/src/main.rs:230-242`
- 観察:
  ```rust
  fn hex_short(bytes: &[u8]) -> String {
      let take = bytes.len().min(8);
      ...
  }
  ```
  使用箇所は `tracing::info!(measurement = %hex_short(&expected_measurement), ...)`.
- 問題: `tracing` には `hex` フォーマッタ (`{:x}`) があり、`tracing::field::display` でアロケーション無しに描画可能。`hex_short` は専用 8-byte truncate を行うため意図はあるが、本来 `hex::encode(&bytes[..bytes.len().min(8)])` で十分 (既に `hex = workspace` 依存)。
- 修正案: 削除し、呼び出し側で `measurement = %hex::encode(&expected_measurement[..expected_measurement.len().min(8)])` を使う。

---

## 全体所感

`crates/tee` は仕様駆動の意図が随所に見えるが、**Title Protocol の中で最も「責務肥大」と「過剰 rationale コメント」が同居する crate**。must-fix の中核は 4 件: (a) 起動順序の仕様逸脱 (#001)、(b) `process_request` 巨大関数 (#002)、(c) `unsafe impl Send + Sync` の安易な投入 (#003)、(d) chunk vs global timeout の境界バグ (#005)。これらは「動いている」コードであるため放置されているが、仕様駆動を旗印にする以上、§5.2 の起動シーケンス図と §4.3 のメモリパターン図に1対1で対応するモジュール分割を行うべき。次フェーズで `process_request` を 6 段階の関数に分割するリファクタを task 17 のサブタスクとして切ることを推奨する。

加えて should-fix-002 (漸進予約の事後カウンタ化) は仕様 §4.2 の核心を骨抜きにしており、`ContentFetcher::fetch` を streaming I/F に変更しない限り ResourcePool は事実上ザル。これも task 17 で `streaming-fetcher` として独立サブタスク化を強く推奨。
