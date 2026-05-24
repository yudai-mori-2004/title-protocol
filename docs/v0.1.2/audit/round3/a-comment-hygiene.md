# Round3 A. コメント癖

## Round2 残存確認

| 観点 | 件数 |
|---|---|
| resolved | 6 |
| partially | 4 |
| not-resolved | 14 |
| regression | 2 |
| 新規発見 | 8 |
| **合計 (round2 で挙がっていた未解決 + 新規)** | **26** |

Round2 で wontfix と分類された項目（new-must-fix-001/002, new-should-fix-002..004, new-nitpick-001/002, must-fix-016/017/019/023, should-fix-001..007/009/011/013..028 のうちログ末尾で wontfix 扱いされたもの）は、本ラウンドでも実態が残っているか個別に再確認した。「OSS 公開前の doc 仕上げで対応」と書いてある項目は v0.1.3 では実装されておらず、現コードに残っているため Round3 では「未対応」として再計上する。

## 重大度別内訳 (新規 + not-resolved)

| 重大度 | 件数 |
|---|---|
| must-fix | 5 |
| should-fix | 12 |
| nitpick | 9 |

## Round2 個別件の判定

### must-fix (Round2 → Round3)

- **004 CHANGELOG.md Unreleased**: **not-resolved** — `CHANGELOG.md:11-17` の `### Changed` セクションが「Trust model: Collection-based → Attestation Document-based」「Module system: WASM → Rust-native」など v0.1.0 差分を依然 6 項目列挙。Round1 の修正案「Added だけ残す」は wontfix された旨が Round2 ログに残るが、CHANGELOG は公開ドキュメントなので Round3 でも must-fix として継続。
- **010 c2pa_verify.rs `Task 04`**: **not-resolved** — `crates/core/src/c2pa_verify.rs:143` で `/// - The TEE orchestration layer (Task 04) to populate ...` がそのまま残っている。Round2 で「partially-fixed」と判定したが、本来「Task 04」という task ID 自体を出さないのが当初の修正案。
- **016 `from_slice` 過剰防御 3重 rationale**: **not-resolved** — `programs/title-whitelist/src/lib.rs:464-482` で「doc + 関数内コメント + debug_assert メッセージ」の 3 重構造が完全に残る。
- **017 ADMIN_AUTHORITY "Phase 1 / Future: multi-sig"**: **not-resolved** — `programs/title-whitelist/src/lib.rs:33-40` の `Phase 1: single wallet. Future: multi-sig / DAO migration plan: A) ... B) ...` ブロックが残り、Round2 修正案（OPERATIONS_JA §9 への集約）が反映されていない。
- **019 `tee_type_matches_attestation_vendor_tag` "Single source of truth"**: **not-resolved** — `crates/tee/src/vendor/aws.rs:203` で `// Single source of truth for the "aws-nitro" identifier.` がそのまま残存。
- **023 ASCII 装飾過多 (`// ---- 75 chars ----`)**: **not-resolved** — Round2 では 118 箇所と数えたが、Round3 で再カウントすると **160 箇所**（`grep -rn "// ---" crates --include='*.rs' | wc -l`）。`crates/gateway/src/endpoints.rs` 14 箇所 / `crates/gateway/src/lib.rs` 10 箇所 / `crates/tee/src/orchestrator.rs` 11 箇所 / その他 multi-crate に均等に分布。Round2 から増加。

### should-fix (Round2 → Round3)

- **001 全 trait/struct/field `仕様書 §X` 機械添付**: **not-resolved** — `crates/core/src/processor.rs` 9 件、`crates/core/src/request.rs` 11 件、`crates/core/src/response.rs` 11 件で計 **31 件**（grep 結果）。Round2 から件数変化なし。
- **002 `KeysResponse` / `HealthResponse` / `SolanaKeysResponse` `# JSON例` 重複**: **not-resolved** — `crates/gateway/src/lib.rs:43-51, 68-72, 111-117` で 3 箇所すべての JSON 例ブロックが残存。
- **004 `endpoints.rs` 各ハンドラ doc `/// Spec §2.5` 重複**: **not-resolved** — `endpoints.rs:60, 79, 96, 125, 145, 164` で全 6 ハンドラに `/// Spec §2.5` ラベルが添付されたまま。さらに POST /extension/solana のように `Spec §2.5, §6.2` と二重に添付しているケースもある（line 164）。
- **005 `handle_solana_extension` "System clock failure here is fatal..." 4行 rationale**: **not-resolved** — `crates/tee/src/server.rs:319-322` で 4 行ブロックが残存。
- **006 `decrypt_single_payload` "Reject mismatches..."**: **partially-fixed** — `orchestrator.rs:303-306` で内容は別の防御 rationale（content_type 再検出の理由）に置換されたが、4 行 rationale という構造自体は残った。
- **007 `HttpContentFetcher` 本体 doc の攻撃モデル長文**: **not-resolved** — `content_fetch.rs:117-121` で「Enforces the size and timeout limits ... These prevent a malicious or misbehaving origin from stalling the TEE or exhausting its memory.」攻撃モデル説明 5 行が残存。
- **009 trait `verify` doc と impl の挙動矛盾**: **resolved** — `crates/attestation/src/lib.rs:73-76` の trait doc は据置だが、`crates/attestation-aws-nitro/src/lib.rs:60-65` の impl が `authenticate(now_unix_secs)` 経由で doc timestamp との比較を実行するため、両者は整合した。さらに `lib.rs:122-137` の `rejects_doc_timestamp_in_future` テストでコントラクトが裏付けられている。
- **011 "C2PA alone vs Title Protocol" 表の README/SPECS 二重掲載**: **not-resolved** — `README.md:49-53` に表が残る。
- **013 server.rs "Layer order: outermost runs first" rationale**: **not-resolved** — `crates/gateway/src/server.rs:80-85` で同種の 6 行説明が残存。Round2 で「より具体的に書き直された」と判定したが、`auth.rs:46-55`, `rate_limit.rs:109-114` でも同じ事実が異なる表現で繰り返されているため、重複問題は解消されていない。
- **014 main.rs "Built outside the async runtime..."**: **partially-fixed** — `crates/tee/src/main.rs:159-160` で 2 行に圧縮されたが、依然 Step 6 コメント内の付帯説明として残る。Round2 ログでは「再整理されたが長さは同じ」と判定済み。
- **015 sp1 feature 切替 "The rest of the source is untouched"**: **not-resolved** — `crates/attestation-aws-nitro/src/lib.rs:12-13` で `The rest of the source is untouched.` がそのまま残存（Round2 で再判定 unchanged 確定）。
- **016 `parse_public_values` `has_user_data` rationale 3行**: **not-resolved** — `programs/title-whitelist/src/lib.rs:383-385` で「has_user_data: u8 — must be canonical 0/1. Treating any non-1 value as `false` would let a SP1 guest ...」3 行 rationale が残存。
- **018 `verifies_real_aws_nitro_attestation` "tests don't depend on anything outside"**: **not-resolved** — `crates/attestation-aws-nitro/src/lib.rs:98-100` で `stored alongside this crate so tests don't depend on anything outside the crate tree` が残存。
- **019 "single wallet / multi-sig" 4箇所反復**: **not-resolved** — must-fix-017 とリンク。`programs/title-whitelist/src/lib.rs:35-40` が唯一の源として残る一方、ロードマップ部分は OPERATIONS_JA §9 に集約されていない。
- **020 `rate_limit_middleware` doc 「Runs independently...」重複**: **not-resolved** — `crates/gateway/src/rate_limit.rs:9-13` モジュール doc と `:113-114` middleware doc で「runs independently of authentication」/「Runs independently of API-key validation」がほぼ同じ意味で 2 回登場。
- **021 `fetch()` 2段階 cap 説明**: **not-resolved** — `content_fetch.rs:189-201` と `:215-217, :228-237` で「Content-Length 事前チェック」と「ストリーミング中の累計チェック」の rationale が両方残り、合計 4 行近い rationale を 2 箇所で展開。
- **022 `Ticket` `Cell<Instant>` 内部詳細**: **not-resolved** — `resource_pool.rs:148`「(it is `Send` but not `Sync` due to `Cell<Instant>`)」残存。
- **024 `tee_seeded_rng` "purpose is included only in error messages"**: **not-resolved** — `crates/tee/src/main.rs:217`「`purpose` is included only in error messages for debuggability.」残存。Round2 ログでも unchanged 確定済み。
- **025 KEY_EXPIRY_SECONDS 重複**: **partially-fixed** — `crates/solana/src/whitelist.rs:15-20` doc は短縮されたが Round2 修正案の「1 行に統一」までは届かず 5 行残る。
- **026 OPERATIONS_JA §2.5 プレースホルダ**: **not-resolved** — `OPERATIONS_JA.md:156-178` で「⚠️ この章は AWS Nitro EC2 上での実機検証後に内容を追記する（プレースホルダー）」が残存。`deploy/aws/README.md` が完成しているのに同章は未統合。
- **027 OPERATIONS_JA §5.2「現状クライアント SDK は提供していない / SDK 化はロードマップ」**: **not-resolved** — `OPERATIONS_JA.md:342` で残存。

### nitpick (Round2 → Round3)

- **002 `JCS(verifiable)` / `JCS(verifiable_response)` / `JCS(signature_hash + results)` 表記揺れ**: **not-resolved** — 3 表記とも各 1 件残存。`crates/tee/src/orchestrator.rs:376`, `crates/solana/src/extension.rs:111`, `crates/attestation/src/lib.rs:39`。
- **003 ダッシュ `--` / `—` 混在**: **not-resolved** — `crates/tee/src/orchestrator.rs` doc コメント先頭で `--` (line 5, 10, 13, 15) と `—` (line 144) が混在。`crates/gateway/src/endpoints.rs:7-13` の箇条書きも `--` 系。
- **004 ASCII 図3箇所重複**: **not-resolved** — `README.md` / `OPERATIONS_JA.md` / `deploy/aws/README.md` でそれぞれ異なる図が残る。`deploy/aws/README.md:14-33` には新規のアーキテクチャ図も追加。
- **005 emoji `⚠️`**: **not-resolved** — `OPERATIONS_JA.md:145, 156` で 2 箇所残る。
- **010 README.md `| | C2PA alone | ...` 空ヘッダー**: **not-resolved** — `README.md:49` で空ヘッダーセルのまま。
- **011 orchestrator.rs `Step N:` 番号付け**: **regressed (→ should-fix 級)** — Round3 で別個に new-must-fix-001 として再計上。下記参照。

---

## 新規発見

### new-must-fix-001 `crates/tee/src/main.rs` の Step 番号衝突が依然顕在

- 場所: `crates/tee/src/main.rs:37, 85, 92, 102, 123, 135, 154, 191`
- 観察: モジュール doc (`main.rs:5-13`) は `Step 1..7` の 7 段階を宣言する一方、本文には以下のラベルが並ぶ:
  ```
  Step 1: Runtime + matching Attestation verifier selection.
  Step 2: Generate encryption key bundle.
  Step 3: Generate Solana Extension signing key       ← 1 つ目の Step 3
  Step 3: Self-attestation — bind the TEE's measurement   ← 2 つ目の Step 3
  Step 4: Registration attestation.
  Step 5: Processors + ResourcePool.
  Step 6: Outbound content fetcher.                   ← 1 つ目の Step 6
  Step 6: Start Axum HTTP server                       ← 2 つ目の Step 6
  ```
  Round2 で `wontfix(K3 ラウンドで対応予定 → v0.1.3)` と判定されたが、v0.1.2 のコードは未修正。モジュール doc とコード本文の番号が 7 vs 8 と一致せず、`Step 3` と `Step 6` が二重に出現する。
- 問題: 読者が「§5.2 起動シーケンス」の項目と照合する手段がない。番号付け規約をコード本体が破っている状態は規律違反。
- 修正案: `Step N:` ラベルを全て削除し、`// Runtime selection`, `// Encryption key bundle`, `// Solana signing key`, `// Self-attestation`, `// Registration attestation`, `// Processors`, `// Content fetcher`, `// HTTP server start` のような意味的見出しに置換。

### new-must-fix-002 `crates/tee/src/orchestrator.rs` の Step 番号系統が 2 系統並存

- 場所: `crates/tee/src/orchestrator.rs:171-251` (本流) と `:368-388` (`build_attested_response`)
- 観察:
  ```
  // 本流（process_request）
  Step 0: Reject incompatible encryption + input combinations
  Step 1: Admit request
  Step 2: Fetch content
  Step 3: Decrypt if encrypted
  Step 4: Compute signature_hash
  Step 5: Verify declared signature_hash
  Step 6: Build processor ID list
  Step 7: Execute processors
  Step 8: JCS hash + Attestation Document + assembled ProcessResponse  ← まとめ
  Step 9: Seal the response

  // build_attested_response 内部
  Step 7: JCS canonicalize and hash         ← 別系統で再振り直し
  Step 8: Get Attestation Document
  Step 9: Base64-encode Attestation Document
  ```
  さらに `build_attested_response` の doc (line 363-367) は `1..5` の 5 段階で同じ処理を説明している。同一の動作が **3 系統の番号付け** で表現されている。
- 問題: SPECS_JA §5.2 のフローと照合不能。Round2 の new-should-fix-001 が「fixed」と判定されているがコードは未修正。
- 修正案: `build_attested_response` 内部の `Step 7/8/9` を削除し、本流側の `Step 8` だけにフローを集約。doc 側も `1..5` を消し、責務（"Hash, attest, base64-encode and assemble."）を 1 行で記述。

### new-must-fix-003 `deploy/aws/README.md` に新規「ない」列挙が追加された

- 場所: `deploy/aws/README.md:57-58`
- 観察:
  ```
  You will **not** need: Solana CLI on EC2, Rust on EC2, the AWS SDK locally —
  all of that is handled by the scripts.
  ```
- 問題: Round1 must-fix-001 で `main.tf` 冒頭の「ない」列挙が問題視されて修正済みだったのに、別ファイルで Round2 → Round3 期間に新規発生。Opus 4.7 系の「読者の期待をリセットする `not`-prefixed」癖がそのまま再発。
- 修正案: 削除する。読者が必要なら Prerequisites の表で正項目だけ列挙していれば足りる。

### new-must-fix-004 `MockAttestationVerifier::MEASUREMENT` の 4 行 rationale

- 場所: `crates/attestation/src/lib.rs:109-113`
- 観察:
  ```rust
  /// Measurement reported by the mock — distinctive ASCII banner so
  /// it never collides with a debug-mode AWS Nitro PCR0 (all zeros).
  /// 48 bytes to match the PCR0 wire size. An admin who pastes this
  /// into `add_approved_measurement` is obviously approving the mock.
  pub const MEASUREMENT: [u8; 48] = *b"TITLE-PROTOCOL-MOCK-MEASUREMENT-DO-NOT-APPROVE!!";
  ```
- 問題: 値そのものが `b"TITLE-PROTOCOL-MOCK-MEASUREMENT-DO-NOT-APPROVE!!"` という自己説明的 ASCII banner であり、「DO-NOT-APPROVE」と書いてある時点で 4 行 doc は冗長。
- 修正案: 1 行 `/// Distinctive ASCII banner; 48 bytes to match PCR0 wire size.` のみに圧縮。

### new-must-fix-005 `crates/tee/src/main.rs:68-69` の cfg-gated push に対する弁明コメント

- 場所: `crates/tee/src/main.rs:68-70`
- 観察:
  ```rust
  // cfg-gated pushes can't collapse into a vec![..] literal — each
  // entry depends on a different feature flag.
  #[allow(unused_mut, clippy::vec_init_then_push)]
  ```
- 問題: clippy 警告抑制の理由はアトリビュート横に書くのが慣例だが、ここでは「なぜリテラルを使わないか」という選択肢の説明を 2 行展開している。Round1 で削った「ない列挙」と同型（"can't collapse into..."）。
- 修正案: コメント削除し、`#[allow]` だけ残す（clippy lint 名で意図は十分自明）。

### new-should-fix-001 `crates/tee/src/server.rs:262-267` `MAX_OFFCHAIN_DATA_BYTES` rationale 過剰

- 場所: `crates/tee/src/server.rs:262-267`
- 観察:
  ```rust
  // Fetch off-chain data. A `ProcessResponse` is metadata + hashes only
  // (no media bytes) — 1 MiB is well past anything realistic and stops
  // a malicious URL from flooding the JSON parser. Run under the same
  // ResourcePool admission control as /process so the extension path
  // cannot be used to bypass the TEE memory budget.
  const MAX_OFFCHAIN_DATA_BYTES: usize = 1024 * 1024;
  ```
- 問題: 攻撃モデル + サイズ根拠 + 追加防御の 3 段で 5 行使う。Round2 の new-should-fix-003 を wontfix とした判定がそのまま残るが、本観点では依然「過剰 rationale」。
- 修正案: 1 行 `// 1 MiB cap: ProcessResponse is metadata-only.` に短縮、`/process` との admission 共有はモジュール doc に集約。

### new-should-fix-002 `crates/tee/src/server.rs:81-85` JSON envelope 上限 rationale

- 場所: `crates/tee/src/server.rs:81-85`
- 観察:
  ```rust
  // ProcessRequest / SolanaExtensionBody are pure metadata JSON — content
  // bytes come from `fetcher`, not the request body. Cap the JSON envelope
  // at 64 KiB so a runaway Gateway can't push a 100-MB document into the
  // TEE before admission control sees the request.
  ```
  `crates/gateway/src/server.rs:56-58` でもほぼ同じ rationale が再録（"so a malicious client can't exhaust the Gateway by streaming 100 MB JSON envelopes."）。
- 問題: 同種の防御 rationale が TEE 側と Gateway 側で別表現として並走。
- 修正案: TEE 側を 1 行 `// JSON envelope only; content arrives via fetcher.` に短縮。Gateway 側も同様に短縮。

### new-should-fix-003 `crates/tee/src/server.rs:178-183` HTTP ステータスマッピング doc が retry policy を語る

- 場所: `crates/tee/src/server.rs:178-183`
- 観察:
  ```rust
  /// Map orchestrator errors to HTTP status codes. Gateway-side retry logic
  /// keys off these codes, so the buckets matter:
  /// - 5xx: TEE / upstream failure, client may retry.
  /// - 502: the upstream URL the client provided is unreachable / malformed.
  /// - 503: admission control rejected the request, retry later.
  /// - 4xx: client supplied something the TEE cannot accept; do not retry.
  ```
- 問題: 「retry してよいか」は TEE の責務ではなく Gateway / クライアントの責務。TEE のコメントでクライアントへの retry 指示を 4 行展開するのは責務外。実装側でステータスコードが正しいかだけが大事で、retry 方針は OPERATIONS_JA / クライアント実装ガイドに置くべき。
- 修正案: doc を 1 行 `/// Map orchestrator errors to HTTP status codes (5xx for TEE/upstream, 4xx for invalid input).` に短縮。retry 方針は SPECS_JA か OPERATIONS_JA に集約。

### new-should-fix-004 `crates/gateway/src/endpoints.rs:38-41` の弁明コメント

- 場所: `crates/gateway/src/endpoints.rs:40-42`
- 観察:
  ```rust
  // Log the upstream body for debugging but don't leak it to the
  // caller — clients see only the status code class.
  tracing::warn!(status, body = %body, "TEE returned HTTP error");
  ```
- 問題: 「ログに残すが API レスポンスには出さない」は実装そのままで自明（`tracing::warn!` の後で body を Response に詰めていないことが明らか）。
- 修正案: コメント削除。

### new-should-fix-005 `crates/gateway/src/endpoints.rs:169-172` "Order matters" rationale

- 場所: `crates/gateway/src/endpoints.rs:169-184`
- 観察:
  ```rust
  // Order matters: a downed TEE returns 503 (transient), which beats the
  // 404 we'd otherwise return for the extension cache being empty. Once
  // the TEE is back up the cache is rebuilt and the 404 path becomes
  // a real "Solana Extension not enabled" answer.
  if !state.is_tee_available() {
      return Err(GatewayError::TeeUnavailable("TEE is not available".into()));
  }
  ```
- 問題: 4 行 rationale で 503/404 のコンテキスト依存を説明。実装の if 順序が `availability → cache` の自然な並びになっており、コメントなしでも読める。
- 修正案: 1 行 `// Check availability first so a TEE outage returns 503, not 404.` に短縮。

### new-should-fix-006 `crates/tee/src/vendor/aws.rs:54-58` の `nsm_exit` 弁明

- 場所: `crates/tee/src/vendor/aws.rs:54-58`
- 観察:
  ```rust
  // `nsm_exit` returns nothing — there is no error path for the
  // host driver to report — but log the close at debug so a leaked
  // fd shows up in traces.
  driver::nsm_exit(self.fd);
  tracing::debug!(fd = self.fd, "nsm_exit called");
  ```
- 問題: `nsm_exit` が値を返さない事実を 3 行で説明。`Drop` 内で fd を閉じてログを出すのは慣例的なパターン。
- 修正案: コメント削除 or 1 行 `// debug-log so a leaked fd is visible in traces.`

### new-should-fix-007 `crates/gateway/src/rate_limit.rs:62-69` poison 回復 rationale

- 場所: `crates/gateway/src/rate_limit.rs:62-69`
- 観察:
  ```rust
  // Recover from a poisoned mutex (another task panicked while
  // holding it) instead of cascading into 500 for every subsequent
  // request — the bucket data itself is in a recoverable state.
  let mut buckets = match self.buckets.lock() {
      Ok(g) => g,
      Err(poisoned) => poisoned.into_inner(),
  };
  ```
  同パターンが `:97-101` でも繰り返される（`prune_idle`）。
- 問題: 同じ判断（"poisoned mutex を `into_inner` で回復"）を 2 箇所で 3 行ずつ説明。
- 修正案: `RateLimiter` 型 doc に 1 行集約。`match` には注釈なしで `// recoverable poison` のみ。

### new-should-fix-008 `crates/gateway/src/server.rs:120-123` GC ヒューリスティクスの rationale

- 場所: `crates/gateway/src/server.rs:120-123`
- 観察:
  ```rust
  // Background GC for the per-identity rate-limit buckets. Runs every
  // 5 minutes and drops buckets that have been full and untouched for
  // 10× the rate-limit window — long enough that the identity has
  // clearly stopped sending traffic.
  ```
- 問題: 「5 分」「10×」という具体的ヒューリスティクスの根拠を 4 行で展開。実数値は環境変数化されていないので調整も不可。`RateLimiter::prune_idle` の doc に集約すべき。
- 修正案: 1 行 `// Periodic GC: prune buckets idle for 10× the rate-limit window.`

### new-should-fix-009 `crates/tee/src/main.rs:155-160` Proxy fetcher 三択 doc

- 場所: `crates/tee/src/main.rs:154-160`
- 観察:
  ```rust
  // Step 6: Outbound content fetcher. Spec §5.2.
  //   PROXY_ADDR=direct (or unset) → reqwest direct (dev / mock runtimes)
  //   PROXY_ADDR=vsock://CID:PORT  → vsock to title-proxy (Nitro production)
  //   PROXY_ADDR=HOST:PORT         → TCP to title-proxy (dev with real proxy)
  ```
- 問題: 環境変数の取り得る値とその意味を 4 行で表示。`ProxyEndpoint::parse` の doc に置くか OPERATIONS_JA に集約すべき情報（運用者向け）が main.rs に居る。
- 修正案: `// Proxy fetcher; see ProxyEndpoint docs for PROXY_ADDR formats.` に短縮し、詳細は `proxy_fetcher.rs` の doc に集約。

### new-should-fix-010 `crates/attestation-aws-nitro/src/cose.rs:60-63` の RFC 引用 + 防御 rationale

- 場所: `crates/attestation-aws-nitro/src/cose.rs:60-63`
- 観察:
  ```rust
  // RFC 8152 §3 — the only header key we understand is `alg` (label 1).
  // Reject any other entry rather than silently ignoring it; in
  // particular this catches `crit` (label 2) extensions we cannot
  // validate.
  ```
- 問題: RFC §引用 + 拒否方針 + 例（crit）の 3 段構成。コードはわずか 9 行のループで自明。
- 修正案: 1 行 `// RFC 8152 §3: only `alg` (label 1) is accepted; reject `crit` etc.` に短縮。

### new-should-fix-011 `crates/proxy/src/main.rs:35-39` CID 拒否 rationale

- 場所: `crates/proxy/src/main.rs:35-39`
- 観察:
  ```rust
  // Minimum CID accepted from peers. AWS Nitro assigns enclave CIDs
  // starting at 16; values 0–2 are reserved (hypervisor / local loopback /
  // host). Rejecting < 3 means a co-tenant host process cannot connect
  // to this proxy via vsock loopback.
  const MIN_ACCEPTED_CID: u32 = 3;
  ```
  モジュール doc (`main.rs:5-9`) でも同じことが書かれており重複。
- 問題: モジュール doc とインライン定数 doc で同じ事実が 2 度展開。
- 修正案: モジュール doc 側を維持し、定数横は `// see module doc for CID reservation.` のみ。

### new-should-fix-012 `crates/core/src/c2pa_verify.rs:137-153` の 5 段 doc

- 場所: `crates/core/src/c2pa_verify.rs:137-153`
- 観察:
  ```rust
  /// Computes the signature_hash for C2PA-signed content.
  /// Spec §1.3 — signature_hash = SHA-256(Active Manifest's COSE signature)
  ///
  /// Returns the hash in `"sha256:hex..."` format.
  ///
  /// This function is used by:
  /// - The TEE orchestration layer (Task 04) to populate `ProcessResponse.signature_hash`
  /// - Verification of encrypted payloads (spec §2.4 step 8)
  ///
  /// # Arguments
  /// ...
  /// # Determinism
  /// The same content always produces the same signature_hash, regardless
  /// of who computes it. ...
  ```
- 問題: must-fix-010 と直接リンク (Task 04 残存)。さらに `# Determinism` セクション 3 行は SPECS_JA §1.5 と重複。`This function is used by:` は呼び出し側を docs から逆引きする型で、責務漏れ。
- 修正案: 「Spec §1.3 — signature_hash = SHA-256(Active Manifest's COSE signature)」 + 引数だけ残して、`This function is used by`/`# Determinism` は削除。Task 04 行も削除。

### new-nitpick-001 `CHANGELOG.md` リード行と本文の不整合

- 場所: `CHANGELOG.md:7-17`
- 観察: 「Full protocol rewrite. See [Technical Spec](docs/v0.1.2/SPECS_JA.md).」とリード 1 行を置いた直後に v0.1.0 差分の `### Changed` が 6 項目並び、リードと中身が整合していない。Round2 でも new-nitpick-001 として挙げたが wontfix。
- 修正案: must-fix-004 と同じ。

### new-nitpick-002 docstring 例 JSON 内のキー順が SPECS_JA と微妙にずれ

- 場所: `crates/gateway/src/lib.rs:43-51` (KeysResponse 例)
- 観察: コメント JSON 例の `x25519` / `p256` / `ml-kem-768` 順は SPECS_JA §2.5 の表（`x25519` / `p256` / `ml-kem-768`）と一致しているが、SPECS_JA §2.4 末尾の表は別の文脈で同順なため確認は冗長。should-fix-002 で削除すれば不要。
- 修正案: should-fix-002 を実施すれば自動的に解決。

### new-nitpick-003 `crates/tee/src/orchestrator.rs:5, 10, 13, 15` の `--` 表記

- 場所: モジュール doc 内 `// 5: For encrypted requests, verify the inner `signature_hash` matches (§2.4)` の前 `5.` までは順序リスト、説明本文 `1. Admit request (§4.1 -- ResourcePool admission check)` の `--` がコメント中で全角と半角混在。Round2 nitpick-003 のサブセット。
- 修正案: `--` → `—` に統一。

### new-nitpick-004 SP1 ビルド時の「短く言い切れない」コメント

- 場所: `crates/attestation-aws-nitro/src/lib.rs:12-13`
- 観察:
  ```rust
  // When built for SP1, shadow the standard `sha2` and `p256` crates with
  // SP1-precompile-accelerated forks. The rest of the source is untouched.
  ```
- 問題: should-fix-015 と同じ箇所。後半「The rest of the source is untouched.」は読者を安心させる為だけのフレーズ。
- 修正案: 後半 1 文だけ削除。前半は技術的事実なので残す。

### new-nitpick-005 `crates/tee/src/main.rs:5-13` モジュール doc 7 段リスト

- 場所: `crates/tee/src/main.rs:5-13`
- 観察: モジュール doc が `Spec §5.2 startup sequence:` の 7 段リストを掲げる一方、本文の `Step N:` ラベルは 8 個（うち 2 つ重複番号）。must-fix-001 の根本原因。
- 修正案: モジュール doc 側の番号リストを削除し、§5.2 起動シーケンスを参照する 1 行に置き換える。

---

## 全体所感

Round2 で「OSS 公開前の doc 仕上げで一括対応」「v0.1.3 で対応予定」として wontfix とされた項目の大半が、Round3 時点でも未着手のまま残存している。観点別に整理すると:

- **解決系統**:
  - 「ない列挙」「ported-from の内輪話」「mock vs nitro 矛盾 doc」など Round1 で象徴的に挙がったケースは概ね削除済み（must-fix-001..009/018, should-fix-008/010/012 等）。
  - attestation trait と impl の動作矛盾は impl 側の `authenticate(now_unix_secs)` 経路で実質整合済み。

- **未解決系統**（量が多く、Round3 の must-fix / should-fix の大宗）:
  - **Step N: 番号付け**: `main.rs` で `Step 3 / 3 / 6 / 6` の衝突、`orchestrator.rs` で `Step 7/8/9` が 2 系統並走。SPECS_JA §5.2 と照合不能。
  - **field/method 単位の `仕様書 §X` 機械添付**: `core/` 配下 3 ファイルで 31 件残存。
  - **JSON 例 doc の重複**: gateway/src/lib.rs の 3 箇所と SPECS_JA §2.5 で四重掲載。
  - **`Phase 1 / Future:` 表現**: programs/title-whitelist で `ADMIN_AUTHORITY` の運用ロードマップを inline doc に展開。
  - **ASCII `// ---` 区切り**: 160 箇所と Round2 から 1.36 倍に増加。
  - **過剰 rationale**: `// xxx so a malicious xxx ... cannot xxx` パターンが server.rs / rate_limit.rs / content_fetch.rs / endpoints.rs / aws.rs / cose.rs / main.rs 全てに散在。

- **新規退行**:
  - `deploy/aws/README.md:57-58` に「ない」列挙が新規追加（Round1 must-fix-001 が修正された後）。
  - `crates/tee/src/main.rs:68-69` の cfg-gated push に対する弁明コメント (Opus 4.7 系の「選ばなかった選択肢の説明」癖)。

OSS 公開向け doc 仕上げを v0.1.3 でまとめてやる方針自体は否定しないが、Step 番号衝突（must-fix-001/002）と新規「ない」列挙（must-fix-003）の 3 件は本観点として継続提示する。それ以外の should-fix / nitpick も「wontfix（v0.1.3 で実施）」と一括処理してきた経緯が残るため、Round3 では現状の事実を一覧で記録するに留める。

---

## Round 3 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| new-must-fix-001 (main.rs Step 番号衝突) | fixed(F) | F round3-new-004 で main.rs の Step ラベル再振り直し済み。Step 2 に Solana 鍵生成を統合、Step 3 = Self-attestation、以下整合。 |
| new-must-fix-002 (orchestrator.rs Step 7/8/9 重複) | fixed | `build_attested_response` の doc 5 段 + 本体 Step 7/8/9 コメントを削除、関数全体を 1 行 doc + body コメントゼロに圧縮。本流 Step 8 (assembled ProcessResponse) のみが残る。 |
| new-must-fix-003 (deploy/aws/README の "ない" 列挙) | fixed | `deploy/aws/README.md:57-58` の "You will not need: ..." 行を削除。Prerequisites 表の正項目だけで足りる。 |
| new-must-fix-004 (MEASUREMENT 4 行 doc) | fixed | `crates/attestation/src/lib.rs:109-113` の 4 行 rationale を 2 行 (「自己説明的 ASCII banner」) に圧縮。値自体が `DO-NOT-APPROVE` を含むので十分。 |
| new-must-fix-005 (main.rs:68-69 弁明コメント) | fixed | `// cfg-gated pushes can't collapse into a vec![..] literal` 2 行を削除、`#[allow(clippy::vec_init_then_push)]` だけ残す。lint 名で意図は自明。 |
| must-fix-010 (Task 04 ref) | fixed | `crates/core/src/c2pa_verify.rs:137-153` の 17 行 doc を 3 行に圧縮。`Task 04` への内輪リンクと `# Determinism` 重複説明を削除。 |
| must-fix-019 ("Single source of truth") | fixed | `crates/tee/src/vendor/aws.rs:203` の 1 行コメントを削除。assert 文 1 つで意図は自明。 |
| must-fix-004 (CHANGELOG ### Changed) | wontfix | CHANGELOG は歴史性のあるリリース文書、v0.1.0 → v0.1.2 差分は OSS リリース時に参照される。Round 1 削除案は wontfix 維持。 |
| must-fix-016 (from_slice 3 重 rationale) | wontfix | `programs/title-whitelist/src/lib.rs:464-482` の 3 重防御 (doc + 関数内 + debug_assert) は意図的な layered guard。v0.1.3 で `InitSpace` 化と一体で見直す。 |
| must-fix-017 (Phase 1 / Future) | wontfix | `ADMIN_AUTHORITY` の rotation 計画コメントは OPERATIONS §9 集約待ち。v0.1.3 で `transfer_admin` ix と一体で対応。 |
| must-fix-023 (ASCII `---` 160 件) | wontfix | 機械的整理。v0.1.3 doc メンテで一括 (個別 fix のコスト > 整理効果)。 |
| should-fix 群 / nitpick 群 (Round 2 残存 + Round 3 新規 9 件) | wontfix | v0.1.3 OSS 公開前の doc メンテで一括対応。本観点の must-fix のみ Round 3 で fix し、should/nitpick は監査記録として残置。 |
