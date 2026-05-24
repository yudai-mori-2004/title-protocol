# K3 Round 3: `crates/tee` 縦深掘り監査

## サマリ

Round 2 では must:6 / should:14 / nitpick:11（Round 1 由来）と新規 r2-001..r2-007 を計上し、最終的に Round 2 処理ログでは:
- fixed: must-fix-001/003/004/005、should-fix-003/004/005/006/007/008/009/010/011/012/013、nitpick-001/002/007/008/009/011、および新規 must-fix-r2-001 / should-fix-r2-001/002/004 / nitpick-r2-001/002
- wontfix: must-fix-002/006（process_request 9 分割と graceful in-flight）、should-fix-001/002（streaming I/F）、should-fix-014/r2-003（fetcher 関連の長期改修）、nitpick-003/004/005/006（doc 冗長）、nitpick-010、nitpick-r2-003

Round 3 再走査の結果:

- **Round 2 fixed 認定が現コードと一致しているか**: 22 件すべて該当箇所で確認、回帰なし。
- **Round 2 wontfix が現コードでもまだ妥当か**: must-fix-002 / 006、should-fix-001/002 は構造的未着手のまま固定。should-fix-014 / r2-003 / nitpick-003/004/005/006/010/r2-003 も同様。
- **新規発見**: 6 件 (must:1 / should:3 / nitpick:2)。中心は `handle_solana_extension` の Step 順序 (admission ticket 後に request パース失敗 → 401/400 でも ticket は drop されるが pool に同時並走を妨げる効果が残る) と、`extend_unchecked` の API 表面化、`Ticket` の Send-not-Sync 制約と `spawn_blocking` 経由クロージャの実質的安全性、それから `MAX_OFFCHAIN_DATA_BYTES` の二重ガード重複と post-fetch チェックの順序問題。

総合所感: K3 の中核問題（safety / 起動順 / admission control の網羅性）は Round 2 で実質決着。Round 3 で残る指摘は健全性ではなく「同じ関数の中で何 step 目に validation を置くべきか」「`pub fn extend_unchecked` の意味づけ」「処理結果の 1xx/2xx/4xx/5xx 分類の細部」といった味付けレベル。must-fix-r3-001 だけは admission control の運用品質に関わるので拾い上げる必要がある。

---

## Part 1: Round 2 指摘の処理状況の検証

### Round 2 「fixed」認定の追検証

各 fixed 認定の根拠コードを Round 3 時点でも追えるか、一行ずつ再走査して確認。

| Round 2 ID | 件名 | Round 3 検証結果 | 根拠 |
|---|---|---|---|
| must-fix-001 | 起動順序 §5.2 逸脱 | 一致 | `main.rs:5-13` の doc コメント列 (Step1..7) が `main.rs:37, 86, 102, 123, 135, 154, 191` のコード位置と整合。Step 3 「Self-attestation」が Solana 鍵生成の直後 (`main.rs:102-121`) に物理的に配置されている。 |
| must-fix-003 | `FakeNsm` の `unsafe impl Send+Sync` | 一致 | `vendor/aws.rs:175-179` で `Mutex<Vec<Vec<u8>>>`, `Vec<u8>`, `Mutex<Option<Vec<u8>>>` のみ。`unsafe` キーワードはファイル中ゼロ。 |
| must-fix-004 | `set_*_timeout` のエラー無視 | 一致 | `proxy_fetcher.rs:103-114, 127-138` で TCP/vsock 両分岐とも `.map_err(...)?` 経由。 |
| must-fix-005 | `compute_global_timeout(0)` の BASE 衝突 | 一致 | `limits.rs:58-65` で `Option<u64>` 化、`None → MAX_GLOBAL_TIMEOUT`。`orchestrator.rs:184-192` で `Single`/`Sidecar` を `None`、`Fragmented` のみ `count × MAX_FRAGMENT_SIZE`。tests (`limits.rs:182-185`) も `compute_global_timeout(None) == MAX_GLOBAL_TIMEOUT` を断言。 |
| must-fix-r2-001 | `EncryptionUnsupportedForInputType` のメッセージ齟齬 | 一致 | `orchestrator.rs:94-97` に `EncryptionRequiresSingleInput` に改名され、文言は「encrypted requests require input_type="single" (fragmented/sidecar encryption is not implemented)」。仕様 §2.4 (l. 419) の「将来の拡張」表現と整合。 |
| should-fix-003 | Mock measurement bypass | 一致 | `lib.rs:51-87` の `tests::StubRuntime` は trait object safety を見るだけの最小実装で、measurement bypass 経路に組み込まれない。本番 mock は `runtime/mock.rs` に統一。`main.rs:117` の起動ログに `tee_type = "mock"` が出るので環境視認可能。 |
| should-fix-004 | `decrypt_single_payload` 早期 reject | 一致 | `orchestrator.rs:171-177` に Step 0 として fetch 前 reject。 |
| should-fix-005 | reqwest async/blocking 混在 | 一致 | `server.rs:142-154` の `/process`、`server.rs:278-282` の `/extension/solana` 両方とも `tokio::task::spawn_blocking` 経由。`reqwest::blocking::Client` 構築自体も `main.rs:164` で `spawn_blocking` 内。 |
| should-fix-006 | extension fetch サイズ未検証 | 一致 | `server.rs:267, 297-307` に `MAX_OFFCHAIN_DATA_BYTES = 1 MiB` を post-fetch でチェック。ただし Part 2 で詳述するように pre-admission のコメント (「same ResourcePool admission control」) と pre-fetch ticket 取得は揃ったが、`1 MiB` を強制する物理は fetcher 内部の `max_body_bytes = 100 MiB` のままで、post-fetch reject は依然「100 MiB まで RAM に乗ったあと弾く」可能性を残す。**must-fix-r3-001 で再掲。** |
| should-fix-007 | Json 抽出器のサイズ上限 | 一致 | `server.rs:79-96` で `DefaultBodyLimit::max(64 KiB)` を `/process` と `/extension/solana` 双方に layer 適用。 |
| should-fix-008 | `data_size_hint=0` 区別不能 | 一致 | should-005 と統合解消済み。 |
| should-fix-009 | NSM GetRandom 0 バイト無限ループ | 一致 | `vendor/aws.rs:72-76` で `random.is_empty()` → `RandomFailed`。 |
| should-fix-010 | `RealNsm::drop` 失敗握りつぶし | 一致 | `vendor/aws.rs:51-61` で nsm_exit 返り値なしを明示 + tracing::debug ログ。 |
| should-fix-011 | `MockRuntime` 3 重実装 | 部分一致 | `runtime/mock/MockRuntime` (main), `orchestrator::tests::MockRuntime` (観測 API 持ち、`last_user_data` 用)、`lib.rs::tests::StubRuntime` (trait object 用) の 3 つに整理。Round 2 認定通り、用途が分かれているので残置は妥当。 |
| should-fix-012 | `expected_measurement` Vec → Box | 一致 | `main.rs:115` `into_boxed_slice()`, `server.rs:67` `Box<[u8]>`, `main.rs:118-120` 起動ログに `measurement_len`。 |
| should-fix-013 | octet-stream silent fallback | 一致 | `content_fetch.rs:320-323` に `tracing::warn!`。 |
| should-fix-r2-001 | `handle_solana_extension` で blocking fetcher を async 内で直接呼ぶ | 一致 | `server.rs:278-295` で `spawn_blocking` ラップ、内側で `state.fetcher.fetch(&url)`。`Arc::clone(&state)` で move もきれい。 |
| should-fix-r2-002 | `handle_solana_extension` の fetch が ResourcePool admission を bypass | 一致（ただし Part 2 で精緻化） | `server.rs:267-276` で `state.pool.try_admit(Some(MAX_OFFCHAIN_DATA_BYTES as u64))`。`drop(ticket)` は `server.rs:308`。順序とサイズ強制は別途 must-fix-r3-001 で扱う。 |
| should-fix-r2-004 | `Err(_)` 分岐が全エラーを 400 に潰している | 一致 | `server.rs:184-207` の `orchestrator_error_to_response` で 503/502/500/400 にマッピング。`process_fetch_failure` test (`server.rs:561-576`) も `StatusCode::BAD_GATEWAY` 期待。 |
| nitpick-001/002/007/008/009/011 | 各 doc 衛生 | 一致 | `lib.rs:1-7` 簡潔、`Cargo.toml:14-18` 短縮、`content_fetch.rs:131-133` 2 行、`resource_pool.rs:1-21` legacy 言及なし、`SS` 0 件 (Bash 検索)、`hex_short` 関数なし (`main.rs:116` インライン)。 |
| nitpick-r2-001 | Step 番号体系のズレ | 一致 | `orchestrator.rs:9-19` に Step 0..9 が並び、`process_request` 本体コメント (`orchestrator.rs:171, 179, 197, 201, 218, 228, 236, 239, 247, 250`) と整合。 |
| nitpick-r2-002 | Ticket dropped here コメント | 一致 | `orchestrator.rs:260` 周辺に該当コメントなし。代わりに `orchestrator.rs:30-33` のモジュール冒頭解説に「The Ticket is dropped at the end of the function」を集約。 |

### Round 2「wontfix」認定の追検証

| Round 2 ID | 件名 | Round 3 判定 |
|---|---|---|
| must-fix-002 | `process_request` 肥大 | 妥当な wontfix。Step 0..9 が doc 番号と一致しており、Round 2 で nitpick-r2-001 を fix した今、`process_request` の 90 行構造は読める。9 関数分割は引き続き v0.1.3 で OK。 |
| must-fix-006 | Drop 順 / graceful shutdown | 妥当な wontfix。`main.rs:196-198` は `axum::serve(..).with_graceful_shutdown(shutdown_signal())` で、`shutdown_signal()` は `tokio::signal::ctrl_c()` を await。`axum 0.8` の `with_graceful_shutdown` は新規 connection の拒否までは保証するが in-flight request の完了待ちは hyper-util ベースに移行しないと厳密化できない。`NitroRuntime` doc (`vendor/aws.rs:117-125`) の運用注意で当面カバー。 |
| should-fix-001 | フラグメント全 concat | 妥当な wontfix。`content_fetch.rs:395-434` は `combined.extend_from_slice(&frag_resp.body)` で全展開、ピーク = init + Σ fragments。仕様 §4.3 (l. 929-948) の「extend → 検証 → shrink」ループは依然未実装だが、これは `c2pa::Reader` を fragment 単位 push する API が core 側に必要で、TEE crate 単体では片付かない。 |
| should-fix-002 | 漸進予約が事後カウンタ化 | 妥当な wontfix。`HttpContentFetcher::fetch` (`content_fetch.rs:216-239`) は 64 KB チャンクの読み込みループは持つが、`ticket.extend` を chunk 内部から呼ぶフックは `fetcher` trait 経由で公開されていないので、`fetch_single` (`content_fetch.rs:373`) で `ticket.extend(resp.body.len())` の事後一括。これも streaming trait 化が必要。`max_body_bytes = 100 MiB` + admission_limit で多層防御は機能している。 |
| should-fix-014 | proxy `write_string(... url, url)` | 妥当な wontfix。`proxy_fetcher.rs:160` で `write_string(&mut socket, url, url)`。コメント (`proxy_fetcher.rs:158`) で「Request: [u32 method_len][method][u32 url_len][url][u32 body_len][body]」とプロトコルを示してあるので意図は読み取れる。 |
| should-fix-r2-003 | `compute_signature_hash` の content_bytes 三重走査 | 妥当な wontfix。core trait 変更を伴うため v0.1.3。 |
| nitpick-003/004/005/006 | Cargo.toml / docstring 冗長 | 妥当な wontfix。`Cargo.toml:14-18, 47-51, 56-58` 確認、いずれも将来 OSS 公開時の一括整理対象として認識済み。 |
| nitpick-010 | `(503)` がエラー文に焼き付け | 妥当な wontfix。`orchestrator.rs:64` `"Request rejected: memory admission limit exceeded (503)"`。ログ視認性のための意図的な焼き付け。 |
| nitpick-r2-003 | `write_string(w, url, url)` | should-014 と同じ理由で妥当。 |

回帰なし。Round 2 で fix と判定された全項目について、Round 3 時点のコードでも当該箇所が同じ修正のままで残っており、別 PR で巻き戻された形跡はない。

---

## Part 2: Round 3 で新たに発見した問題

### must-fix-r3-001 `/extension/solana` で admission ticket と物理サイズ強制が同居しない（admission control の意味弱体化）

- 場所: `crates/tee/src/server.rs:267-308`
- 観察:
  ```rust
  const MAX_OFFCHAIN_DATA_BYTES: usize = 1024 * 1024;
  let ticket = state
      .pool
      .try_admit(Some(MAX_OFFCHAIN_DATA_BYTES as u64))
      .ok_or_else(|| { ... })?;
  let offchain_resp = tokio::task::spawn_blocking({
      let state = Arc::clone(&state);
      let url = body.offchain_data_url.clone();
      move || state.fetcher.fetch(&url)
  }) .await ... ?;
  if offchain_resp.body.len() > MAX_OFFCHAIN_DATA_BYTES {
      return Err(( PAYLOAD_TOO_LARGE, ... ));
  }
  drop(ticket);
  ```
- 問題:
  1. `state.fetcher` は `HttpContentFetcher`（または `ProxyContentFetcher`）で、コンストラクタ (`main.rs:164-166` または `main.rs:171-172`) は `DEFAULT_MAX_BODY_BYTES = 100 MiB` のままで作っている。つまり「TEE がメモリに置いてもよい上限」は依然 100 MiB であり、`MAX_OFFCHAIN_DATA_BYTES = 1 MiB` は **fetch 完了後の reject** にしか効かない。
  2. 一方 `ticket` 側は `try_admit(Some(1 MiB))` で「1 MiB 分の admission tax」を取得しているが、`ticket.extend(...)` を呼ばないので `pool.used` は実際には 0 のまま増えない。つまりこの ticket は admission 判定（`can_admit()` の boolean ゲート）にしか効いていない。
  3. 結果として、攻撃者が同一の `/extension/solana` を 100 並列で叩くと:
     - admission ゲートは `pool.used < admission_limit` で 100 並列とも通過（誰も `ticket.extend` していないので `used = 0`）。
     - 各 fetch は最大 100 MiB のレスポンスを受け取り、TEE プロセスの heap に 100 並列 × 100 MiB = 10 GB を載せる可能性がある。
     - `/process` 系には admission + fetcher 内部 cap + `ticket.extend` の三段ガードがあるが、`/extension/solana` は admission のブール判定だけで、サイズ強制は post-fetch のみ。
  4. Round 2 should-fix-r2-002 では「同じ ResourcePool 経由に揃える」と述べたが、現実装は ticket を取るだけで「reserved bytes をプール側に通知する」操作（`extend`）が欠落しており、`/extension/solana` から `POOL_TOTAL_LIMIT` を実質的に bypass できる構図が残っている。
- 影響: HTTP 100 並列で TEE の OOM を誘発可能。`/extension/solana` の `offchain_data_url` を攻撃者が指定できる Gateway 経由経路で、100 MiB のレスポンスを返すストレージを攻撃者が用意できれば成立する。
- 修正案: (a) 専用 fetcher を持つ。`main.rs` で `extension_fetcher = HttpContentFetcher::with_max_body_bytes(MAX_OFFCHAIN_DATA_BYTES)` を構築し `TeeAppState.extension_fetcher` に格納。`handle_solana_extension` は `state.extension_fetcher.fetch(...)` を使う。これで fetcher 内部の `max_body_bytes` cap が 1 MiB になり、ストリーミング途中で打ち切る。(b) または、ticket を取った後に `ticket.extend(MAX_OFFCHAIN_DATA_BYTES)` で事前予約してから fetch する。これで `pool.used` に 1 MiB が積まれ、100 並列なら admission_limit (デフォルト `total_limit * 3 / 4`) を 100 × 1 MiB = 100 MiB 消費して頭打ちにできる。(a) と (b) 併用が筋。

### should-fix-r3-001 `Ticket` が `!Sync` なのに `spawn_blocking` 経由でクロージャに move される（コンパイル安全だが意味論的に注意）

- 場所: `crates/tee/src/server.rs:278-289`
- 観察: `handle_solana_extension` では `ticket` を関数スコープに保持したまま、`tokio::task::spawn_blocking({ move || state.fetcher.fetch(&url) })` を await。`ticket` 自体はクロージャに move されていないので問題ない（クロージャは `state` clone と `url` だけ move）。`handle_process` (`server.rs:142-154`) の方は `orchestrator::process_request` がクロージャ内で `ticket` を生成して使うので閉じている。
- 問題: `Ticket` は `Cell<Instant>` を持つため `!Sync`。現状の使い方では `ticket` を別スレッド (blocking pool) に渡していないので Rust の型システムが落としているが、将来 `spawn_blocking` クロージャを再構成するときに `ticket` を渡せそうに見えるトラップになる。`Ticket` の doc (`resource_pool.rs:147-149`) に「`Send` but not `Sync` due to `Cell<Instant>`. Belongs to a single request thread」とあるが、`spawn_blocking` は別スレッドなので「single request thread」の意味が分岐する。`!Sync` のおかげで `&Ticket` を別スレッドに渡せないだけで、`Ticket` 所有権を move すれば渡せる（型としては安全）。
- 影響: 現状コンパイル＆動作は正しい。リファクタ時の事故源。
- 修正案: `Ticket` doc を「`!Sync`: never share `&Ticket` between threads. `Send`: ownership can be moved to a blocking task if the original thread no longer holds it.」のように明示する。または `last_activity` を `Cell` から `AtomicU64` (nanos since epoch) にして `Sync` にする。後者の方が将来の streaming 化と相性が良い。

### should-fix-r3-002 `extend_unchecked` が pub で外に出ているが安全な使用条件が doc 化されていない

- 場所: `crates/tee/src/resource_pool.rs:245-250`
- 観察:
  ```rust
  pub fn extend_unchecked(&self, additional: usize) -> Result<(), TicketError> {
      if additional == 0 { return Ok(()); }
      self.extend_inner(additional)
  }
  ```
  doc は「For internal use where timeout enforcement is handled at a higher level, or in concurrent tests where timing is unpredictable.」
- 問題:
  1. `pub fn` なので外部 crate からも呼べる。Rustdoc 上は通常の API として並ぶ。
  2. 「higher level で timeout が見られている前提」を破る呼び出し（例: streaming fetcher を実装した第三者）は、Spec §4.4 の chunk timeout / global timeout を黙って bypass できる。
  3. 現 codebase 内 grep で `extend_unchecked` を呼んでいるのは `resource_pool::tests` のみ（`extend_unchecked_skips_timeout` / `extend_unchecked_respects_total_limit` / `concurrent_extend_shrink_no_data_race` / `concurrent_ticket_lifecycle`）。本物の請求書 (production path) は `Ticket::extend` のみ使用。
- 影響: API 表面汚染。外部利用者が誤って timeout を skip する経路に乗る可能性。
- 修正案: (a) `pub(crate) fn extend_unchecked` に visibility を絞る。`resource_pool::tests` は同モジュール内なので `pub(crate)` で十分通る。(b) または `#[doc(hidden)]` + テスト用 helper の trait extension に切り出す。

### should-fix-r3-003 `handle_solana_extension` のシステム時刻取得を ticket 取得後に置いている（fail-late）

- 場所: `crates/tee/src/server.rs:267-333`
- 観察: フローは「admission ticket → fetch → JSON parse → SystemTime::now()」の順。`SystemTime::now()` 失敗は 500 を返すが、これは `extension::process_extension` 直前の `server.rs:323-333` で行われる。
- 問題: 仕様上は ticket を取って fetch して JSON を parse してから「あ、システム時計がない、500 で 580 ms 食って帰ります」となる。Round 2 で `now_unix_secs` 取得失敗を `silent 0 fallback` から 500 に変えた修正自体は良い (`server.rs:319-322` コメント参照) が、`fail fast` の観点では admission の前か、少なくとも fetch の前で取りたい。
- 影響: 攻撃時の DoS 振幅が拡大する程度で、健全性は守られている。
- 修正案: `handle_solana_extension` 冒頭、`ext_request` 構築直後に `let now_unix_secs = ...` を移動。`handle_process` 側にも同じパターンが将来増えそうなので、`server.rs::current_unix_secs()` 関数として分離するのが筋。

### nitpick-r3-001 `orchestrator.rs:29-33` のモジュール冒頭「Memory management」セクションに `Ticket is dropped at the end of the function` が残存

- 場所: `crates/tee/src/orchestrator.rs:28-33`
- 観察:
  ```text
  //! ## Memory management (§4.1, §4.2)
  //!
  //! Each request gets a Ticket from the ResourcePool. Memory is tracked
  //! throughout the pipeline via Ticket.extend() calls in the content fetch
  //! layer. The Ticket is dropped at the end of the function, releasing all
  //! reserved memory.
  ```
- 問題: Round 2 nitpick-r2-002 で本文中の RAII コメントは削除されたが、モジュール冒頭の同じ説明は残った。本体コードから消したのにモジュール doc には残っていると、読者は「なぜ doc には書いて本体には書かないのか」と勘ぐる。Round 2 の対応コメント (Round 2 ログの nitpick-r2-002 「モジュール冒頭の解説は維持」) は意図的選択だが、Round 3 視点で再読すると本体 doc から RAII の説明はもう不要で、`ResourcePool` / `Ticket` の doc に集約した方が DRY。
- 影響: 軽微。
- 修正案: `orchestrator.rs:28-33` を `//! ## Memory management (§4.1, §4.2)` 〜 「Memory is tracked throughout the pipeline via Ticket.extend() calls in the content fetch layer.」までに圧縮し、RAII drop は `resource_pool.rs` の `Ticket` doc に任せる。

### nitpick-r3-002 `Cargo.toml` の `tokio = { version = "1", features = [...] }` が dependencies と dev-dependencies で別定義（DRY 違反 + 機能ドリフトリスク）

- 場所: `crates/tee/Cargo.toml:44, 65`
- 観察:
  ```toml
  [dependencies]
  tokio = { version = "1", features = ["rt-multi-thread", "macros", "net", "signal"] }
  ...
  [dev-dependencies]
  tokio = { version = "1", features = ["full"] }
  ```
- 問題: dev-dependencies で `full` を引いているので、テスト時の tokio features は production と異なる。`full` には `time` / `sync` / `process` / `fs` などが入り、production では使えない API を test で呼んでも気付かない。Round 2 の処理ログには上がっていなかったが、本来の dependency 衛生としては `[dependencies]` と `[dev-dependencies]` で features を addition のみにすべき。
- 影響: 将来 tokio API を test だけで使い始めて production で消えていることに気付かないリスク。
- 修正案: `[dev-dependencies]` の `tokio` から `full` を外し、test で追加に必要な features (例: `time`, `test-util`) のみを足す。または `tokio = { workspace = true, features = [...] }` で workspace に集約。

---

## Part 3: Round 3 で再確認した観点別所感

### orchestrator 暗号化パイプライン

`orchestrator.rs:163-262` (`process_request` 本体) + `orchestrator.rs:270-317` (`decrypt_single_payload`) で完結。

- **Step 0**: 暗号化 × non-Single の reject (l. 175-177)。`EncryptionRequiresSingleInput`。Round 2 で改名済み。
- **Step 1**: `pool.try_admit(size_hint)` (l. 193-195)。`Fragmented` のみサイズヒントあり。Round 2 で fix 済み。
- **Step 2**: `fetch_content` (l. 199)。`content_fetch.rs:336` 経由で `fetch_single` / `fetch_fragmented` / `fetch_sidecar`。
- **Step 3**: `decrypt_single_payload` (l. 205-216)。`title_crypto::sealed_channel::open_request` で `KEM unseal → HKDF → AEAD decrypt → metadata + raw content + response channel`。
- **Step 4**: `signature_hash` 計算 (l. 218-226)。sidecar は manifest data から、他は content + content_type から。
- **Step 5**: 暗号化 declared vs 計算結果照合 (l. 228-234)。仕様 §2.4 step 8 と一致。
- **Step 6**: `ensure_c2pa_verify` (l. 237)。c2pa-verify を先頭に挿入。
- **Step 7**: `registry.execute(...)` (l. 240)。
- **Step 8**: `compute_jcs_hash` + `runtime.get_attestation_document(jcs_hash)` + base64 (l. 247-248 → `build_attested_response` l. 368-388)。
- **Step 9**: response_channel があれば `channel.seal(&response_json)` (l. 252-261)。

仕様 §2.4 「対応スイート」(l. 551-580) と `EncryptionSuite` の x25519 / p256 / ml-kem-768 の三スイートを `title_crypto::KeyBundle` が保持。`open_request` 内で suite_id mismatch を `CryptoError::EncryptionSuiteMismatch { wire, declared }` で返し、orchestrator が `OrchestratorError::EncryptionSuiteMismatch` に翻訳 (l. 291-297)。

暗号化パイプラインのテストは `orchestrator.rs:1138-1293` に 3 件:
- `encrypted_pipeline_x25519_roundtrip`: 正常系。response_channel.open で復号確認。
- `encrypted_pipeline_signature_hash_mismatch_rejected`: 攻撃者が metadata で偽の signature_hash を宣言した場合の Step 5 reject。
- `encrypted_pipeline_rejects_fragmented_input`: Step 0 で `EncryptionRequiresSingleInput`。

p256 / ml-kem-768 のラウンドトリップは欠落しているが、`title_crypto::sealed_channel` の crate 内テストで suite 別に網羅されているので tee crate 側は x25519 だけで OK。健全。

### NitroRuntime NSM 操作

`vendor/aws.rs` 全 227 行。

- `NsmOps` trait (l. 29-32): `get_random(len) -> Vec<u8>` と `get_attestation_doc(user_data: Option<&[u8]>) -> Vec<u8>`。private trait。
- `RealNsm::new()` (l. 40-48): `driver::nsm_init()` 失敗時は `InitializationFailed`。fd 負の場合のメッセージで「is this running inside a Nitro Enclave?」がデバッグに親切。
- `RealNsm::drop()` (l. 51-61): `nsm_exit(fd)` + tracing::debug。Round 2 should-fix-010 で改善済み。
- `RealNsm::get_random()` (l. 63-93): NSM GetRandom が ~256 byte 上限なのでループ。`random.is_empty()` の 0 バイト無限ループは Round 2 must-fix-009 で fix 済み。`Response::Error(err)` と `other` の両方を `RandomFailed` に集約。`take = (len - out.len()).min(random.len())` の min は overrun 防止。
- `RealNsm::get_attestation_doc()` (l. 95-111): `Request::Attestation { public_key: None, user_data, nonce: None }`。NSM API の生形そのまま。`Response::Attestation { document }` 以外は error。

`NitroRuntime::get_attestation_document` (l. 151-158) で `user_data.is_empty()` を `None` に変換しているのは AWS NSM の暗黙仕様（self-attestation で空 user_data を渡すと NSM がエラーになるか自身の measurement のみのドキュメントを返すかが文書化されていない）への defensive code。`main.rs:109-111` の self-attestation 呼び出しが `&[]` を渡すのと整合。

`tests::FakeNsm` (l. 175-198) は `Mutex` で内部状態を守って `Send + Sync`。Round 2 must-fix-003 解消。

特に新規問題なし。

### proxy_fetcher 設計

`proxy_fetcher.rs` 全 431 行。

- `ProxyEndpoint::parse` (l. 41-66): `vsock://CID:PORT` か `HOST:PORT`。vsock は `target_os = "linux"` + `feature = "vendor-aws"` の条件付き。non-Linux / vendor-aws 無効ビルドで `vsock://` を渡すと「only supported when built with --features vendor-aws on Linux」エラー。
- `ProxyContentFetcher::open` (l. 89-142): TCP / vsock 両分岐。両方とも `set_read_timeout` / `set_write_timeout` の失敗を伝播 (Round 2 must-fix-004)。
- ワイヤープロトコル (l. 158-220):
  - Request: `[u32 method_len][method][u32 url_len][url][u32 body_len][body]`
  - Response: `[u32 status][u32 body_len or CHUNKED_SENTINEL][body...]`
  - `CHUNKED_SENTINEL = u32::MAX`、終端は `[u32 0]`。
  - `CHUNKED_TRUNCATED = u32::MAX` (= sentinel と同値だが、終端後に来る場合のセマンティクス)。
- `read_chunked_body` (l. 264-302): `n == 0` で終端、`n == CHUNKED_TRUNCATED` で upstream budget 超過エラー、累積 `> max_body_bytes` で reject。`body.resize(start + n, 0); r.read_exact(&mut body[start..])` でゼロ詰めから上書き。
- `ReadWrite` blanket impl (l. 306-307): `T: Read + Write + Send` で `TcpStream` も `VsockStream` も使える。

`CHUNKED_SENTINEL == CHUNKED_TRUNCATED == u32::MAX` の同値は意図的（仕様: `body_len_field` が `CHUNKED_SENTINEL` ならチャンク列、チャンク列の中の `n == u32::MAX` は truncated）。読みづらいが固定値を変えると proxy 側と互換性が崩れるので残置。

特に新規問題なし。

### 起動シーケンス (§5.2 整合)

仕様 §5.2 (l. 1017-1045) の 5 ステップ:
1. 鍵ペア生成
2. 自身の attestation 取得 (measurement 抽出)
3. 公開鍵を Gateway に通知
4. リクエスト受付開始

実装 (`main.rs`) は:
1. Runtime + Verifier 選択 (l. 37-83)
2. 暗号化 KeyBundle 生成 (l. 86-90)
3. Solana 鍵生成 (l. 92-100)
4. Self-attestation (l. 102-121) ← 仕様 §5.2 step 2
5. 登録 attestation (l. 123-133) ← 仕様 §6.2 用
6. Processors + ResourcePool (l. 135-152)
7. Fetcher 構築 (l. 154-173)
8. Axum 起動 (l. 191-198)

仕様 §5.2 と一対一ではないが「鍵生成 → self-attestation → public key 通知 (= /keys endpoint で expose) → 受付開始」の順序は守られている。Step 4 で self-attestation 失敗時に boot abort も仕様 §5.2 l. 1045「自己 Attestation の取得に失敗した場合、TEE は起動を中止する」と整合 (`main.rs:111` の `?` で `main` から早期 return)。

ただし主モジュール doc (`main.rs:5-13`) と仕様 §5.2 の step 数が違う:
- 仕様: 4 step (鍵 → self-attest → 公開鍵通知 → 受付)
- 実装 doc: 7 step (Runtime 選択を 1 ステップに昇格、Solana 鍵を分離、Registration attestation を分離、Processors+Pool を分離、Fetcher を分離)

仕様逸脱ではなく実装詳細展開。健全。

---

## Part 4: 全体所感

K3 ラウンド 3 で残る問題は admission control の運用品質 (`/extension/solana` の物理サイズ強制) 1 件と doc / API 表面の磨き込み 5 件。must-fix-006 (graceful shutdown) と must-fix-002 (`process_request` 分割) は v0.1.3 で再検討する位置のままで Round 3 でも触らないのが妥当。

優先順位:
1. **must-fix-r3-001**: `/extension/solana` 専用 fetcher (1 MiB cap) を持つ。これは TEE が OOM で落ちる経路を残しているので必須。
2. **should-fix-r3-001**: `Ticket` の `!Sync` doc 整備。リファクタ事故予防。
3. **should-fix-r3-002**: `extend_unchecked` を `pub(crate)` に絞る。API 衛生。
4. **should-fix-r3-003**: `now_unix_secs` の fail-fast 化。
5. **nitpick-r3-001/002**: doc DRY、Cargo features。

Round 2 で構造的な健全性問題はほぼ解消済みで、Round 3 は「磨き込みフェーズ」に入った印象。`process_request` 9 ステップの分割と `ContentFetcher` streaming I/F 化は v0.1.3 の TEE crate 大改修 (= Range Request 対応) に合わせて一気にやるのが筋。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| Round 2 fixed 全 22 件 | 確認 | Round 3 時点のコードで該当箇所がそのまま残っており回帰なし。 |
| Round 2 wontfix 全件 | 確認 | v0.1.3 持ち越しの判断は Round 3 でも妥当。 |
| must-fix-r3-001 | fixed | (a) `TeeAppState` に `extension_fetcher` フィールドを追加し、`main.rs` で `HttpContentFetcher::with_max_body_bytes(1 MiB)` / `ProxyContentFetcher::with_max_body_bytes(_, 1 MiB)` で構築。`handle_solana_extension` は `state.extension_fetcher` 経由で fetch するので fetcher 内部の max_body_bytes が 1 MiB に絞られる。(b) admission ticket 取得直後に `ticket.extend(MAX_OFFCHAIN_DATA_BYTES)` で 1 MiB を `pool.used` に積む。これにより N 並列で admission_limit までで頭打ち、かつ各 fetch も 1 MiB を超えるバイトを物理的に受けない。100 並列で 10 GiB を載せられる経路を塞いだ。 |
| should-fix-r3-001 | fixed | `Ticket` の doc に「`Send` (所有権 move は OK、`spawn_blocking` クロージャに乗せられる) / `!Sync` (`&Ticket` を別スレッドに同時貸し不可、借用検査で弾かれる)」を明記。`AtomicU64` 化は影響範囲が広く、得るものが doc 1 段で済むのと釣り合わないため見送り。 |
| should-fix-r3-002 | fixed | `Ticket::extend_unchecked` を `pub fn` から `pub(crate) fn` に絞った。production の唯一の caller は `Ticket::extend` 内部、外部 caller は `resource_pool::tests` のみで同 crate 内のため `pub(crate)` で通る。外部 crate が timeout を skip する経路を物理的に塞いだ。 |
| should-fix-r3-003 | fixed | `handle_solana_extension` 冒頭で `SystemTime::now()` を取得して fail-fast 化。admission ticket 取得や fetch を走らせる前にシステム時計の異常を 500 で弾く。 |
| nitpick-r3-001 | fixed | `orchestrator.rs:28-33` のモジュール冒頭 Memory management 説明から RAII drop の文を削除し、「RAII drop semantics and pool accounting details live with ResourcePool / Ticket in resource_pool.rs」に置き換え。doc DRY 違反を解消。 |
| nitpick-r3-002 | wontfix | `[dev-dependencies]` の tokio = `full` 維持。test の方便であって production code への漏れは CI の `cargo build --release` で検出可能。features を細かく指定する修正コスト > 得られる利得。 |
