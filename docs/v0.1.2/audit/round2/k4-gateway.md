# K4. crates/gateway 縦深掘り — Round 2

## 概要

- 担当範囲: `crates/gateway/{src/*.rs, tests/e2e.rs, Cargo.toml}`
- 監査方針: Round 1 で指摘した 20 件（must:5, should:9, nitpick:6）を 1 件ずつ実装と突き合わせて確認し、修正中に混入した新規問題も拾う。
- 件数サマリ: Round 1 指摘の状態は **resolved 14 / partial 3 / open 3**。新規発見は must-fix 0 / should-fix 2 / nitpick 3。

## Round 1 指摘の処理状況

| ID | 重大度 | タイトル | 状態 | 評価 |
|---|---|---|---|---|
| must-fix-001 | must | 暗号化レスポンス透過 | **resolved** | `ProcessOutcome::{Plaintext, Encrypted}` 導入、Content-Type 判別、`handle_process → Response` 化、すべて正しい。残り課題はテスト不足（後述 new-should-001）。 |
| must-fix-002 | must | リクエストボディサイズ無制限 | **resolved** | `DefaultBodyLimit::max(64 * 1024)` を `/process` と `/extension/solana` に明示適用。`server.rs:55-80`。 |
| must-fix-003 | must | middleware order コメントが axum 意味論と食い違い | **resolved** | `server.rs:81-86` で「layers added LATER wrap EARLIER」と axum の挙動を正確に再記述。実装順序も意図と一致。 |
| must-fix-004 | must | TEE 503 が Gateway 502 に化ける | **resolved** | `endpoints.rs:36-52` で `tee_err` を status 別に分岐。503→`TeeUnavailable`、429→`RateLimited`、400-499→`TeeRejected{status}` 透過、5xx→`TeeError`。`GatewayError::TeeRejected` も新規追加され `error.rs:48-50` で元 status をそのまま返す。 |
| must-fix-005 | must | Authorization 非 UTF-8 が anonymous 化 | **resolved** | `auth.rs:21-44` に `enum AuthHeader { Missing, Bearer, Malformed }` を導入、`parse_auth_header` を auth と rate_limit で共有。Malformed は auth 側で「Malformed Authorization header」、rate_limit 側で anonymous バケットに集約され、攻撃者がローテーションでバケットを増やせない構造になっている。 |
| should-fix-001 | should | rate-limit バケットのメモリリーク | **resolved** | `RateLimiter::prune_idle` を実装 (`rate_limit.rs:97-106`)、`server::run` で 5 分 tick の GC タスクを起動 (`server.rs:121-140`)。アイドル閾値 `window_secs * 10`。`prune_drops_full_idle_buckets` テストもあり。 |
| should-fix-002 | should | `Mutex::lock().unwrap()` 多用 | **partial** | `rate_limit.rs:66-69, 98-101` は `unwrap_or_else(into_inner)` で poison 回収済み。一方 `server.rs` の MockTeeClient テストモックは `*.lock().unwrap()` のまま（`server.rs:211,215,222,230,234,240,244,251,255,266,269,281,288,292`）。テストコードなので致命ではないが、新規テストが panic 中の lock を取ると診断が複雑になる。 |
| should-fix-003 | should | reqwest に retry / connect_timeout / pool 設定がない | **partial** | `tee_client.rs:99-108` で `connect_timeout(5s) / pool_max_idle_per_host(16) / tcp_keepalive(60s)` は追加済み。**ただし retry は未実装**（GET 系の idempotent 呼び出しでも単発で `Unreachable` を返す）。 |
| should-fix-004 | should | health loop が `tokio::time::interval` を使わずズレる | **resolved** | `state.rs:153-167` で `tokio::time::interval` + `MissedTickBehavior::Delay` + 初回 tick 消費を実装。 |
| should-fix-005 | should | key change 検知の失敗握りつぶし | **resolved** | `state.rs:114-123` で `Err(e)` 時に `tracing::warn!` を出し `keys_changed=true` で強制 refresh する fail-safe を実装。 |
| should-fix-006 | should | `refresh_tee_info` が部分失敗をロールバックしない | **resolved** | `state.rs:82-98` で `new_cache` をローカルに組み立て、4 呼び出しすべて成功した場合にのみ `*self.tee_cache.write().await = new_cache` で原子的に swap。 |
| should-fix-007 | should | `Default for GatewayConfig` が 0.0.0.0:3000 を埋め込む | **resolved** | `Default` 実装を削除済み（`server.rs:26-46` に `Default` 派生なし）。`main.rs` と `tests/e2e.rs` はすべて明示構築なので問題なし。 |
| should-fix-008 | should | `health_check_interval_secs = 0` でホットループ | **resolved** | `state.rs:154` で `interval_secs.max(1)` を強制。`main.rs` 側で弾く形にしなかったのは妥当な選択（呼び出し元すべてを縛らない）。 |
| should-fix-009 | should | e2e restart テストの同一ポート再 bind が flaky | **open** | `tests/e2e.rs:404, 414` の `sleep(100ms)` → `TcpListener::bind(tee_addr)` の構造はそのまま。`SO_REUSEADDR` を立てる手当も、新ポート + endpoint 差し替えへの再設計も入っていない。macOS / Linux いずれでも稀に `Address already in use` で落ちる可能性が残る。 |
| nitpick-001 | nit | `## Legacy` セクション | **resolved** | 現 `lib.rs` に Legacy セクションなし。 |
| nitpick-002 | nit | doc コメント英日混在 | **partial** | モジュールヘッダーは英語で統一されているが、`lib.rs:37-162` の API 型 docstring（`KeysResponse` 等）は日本語のまま。OSS 公開時に揺れが残る。SPECS_JA を引く都合上、引用部のみ日本語にする方針なら現状でも許容だが、Round 1 の修正案（型 docstring 英語化）は未実施。 |
| nitpick-003 | nit | `ApiKeySet::contains` の constant-time コメントが実装と乖離 | **resolved** | `auth.rs:114-118` で「length-mismatched entries are skipped, so total runtime leaks the candidate's length (not which entry matched)」と実態を正確に記述。実装は branchless XOR で問題なし。`subtle::ConstantTimeEq` 化は採用していないが、コメントとの一致は取れている。 |
| nitpick-004 | nit | `Cargo.toml` ローカルバージョン指定 | **open** | `axum = "0.8"`, `tokio = { version = "1", ... }`, `reqwest = "0.12"`, `tracing = "0.1"`, `tracing-subscriber = "0.3"`, `async-trait = "0.1"` がすべてクレートローカルのまま。workspace 化されていない。 |
| nitpick-005 | nit | TEE エラーボディが client へ漏れる | **resolved** | `endpoints.rs:39-48` で `tracing::warn!(status, body=%body, ...)` にログを残し、client には `format!("TEE upstream returned HTTP {status}")` のみ返す。 |
| nitpick-006 | nit | `solana_extension` の二重チェック順序の意図 | **partial** | `endpoints.rs:170-182` のロジックは正しく動いているが、Round 1 提案の「TEE 不在のときは 404 より 503 が優先」コメントは付いていない。意図は読み取れるので致命ではないが説明不足。 |

## 新規発見

### new-should-fix-001 暗号化レスポンス透過パスのテストが存在しない

- 場所: `crates/gateway/tests/e2e.rs`（全体）、`crates/gateway/src/server.rs:740-763`（`process_with_auth`）
- 観察: must-fix-001 の修正で `ProcessOutcome::Encrypted` 経路が新設されたが、`tests/e2e.rs` にも `server.rs::tests` にも「TEE が `application/octet-stream` を返す → Gateway がそのまま透過する」シナリオが無い。MockTeeClient (`server.rs:262-275`) は `Plaintext` だけを返すため、`Encrypted` バリアントは型上は使われているが回帰テストでカバーされていない。
- 問題: Round 1 で「実装の前提が崩れている」と指摘した中核バグの修正にテストが伴っていない。将来誰かが `handle_process` を再度 `Json<ProcessResponse>` に戻しても CI で気づかない。仕様 §2.4 の主要シナリオ。
- 修正案:
  - `MockTeeClient::process_response` を `Mutex<Option<ProcessOutcome>>` に変更し、`encrypted_bytes` を返すコンストラクタを追加。
  - `server.rs::tests` に `process_relays_encrypted_bytes_with_octet_stream_content_type` を追加（200 + `Content-Type: application/octet-stream` + body == 期待バイト列を assert）。
  - e2e 側にも TEE の暗号化応答（`crates/tee/src/server.rs:151-159`）を経由する 1 本を追加するのが望ましい（key_bundle で nonce+ciphertext を生成 → response_key で復号できることまで通すと TEE↔Gateway↔Client の §2.3-§2.4 が full coverage）。

### new-should-fix-002 `tee_err` 内 5xx (502/504/500 等) が同一バリアントに潰れる

- 場所: `crates/gateway/src/endpoints.rs:43-48`
- 観察: status 別の分岐は `503 → TeeUnavailable`、`429 → RateLimited`、`400..=499 → TeeRejected{status}`、それ以外（500, 502, 504 等の 5xx）はすべて `TeeError(...)` に集約 → 502 BAD_GATEWAY。
- 問題: must-fix-004 で「TEE のセマンティックを透過すべき」と指摘した方向性が、4xx 側だけ実装されて 5xx 側は未対応。たとえば TEE が `504 Gateway Timeout`（処理が長くてタイムアウト）を返した場合、Gateway は 502 に化ける。リトライ戦略はタイムアウト原因と上流クラッシュで異なるべき。
- 修正案:
  - `GatewayError::TeeUpstreamError { status }` を追加（あるいは `TeeRejected` を 4xx/5xx 両対応にリネーム）し、5xx も `StatusCode::from_u16(status).unwrap_or(BAD_GATEWAY)` でそのまま返す。
  - `tee_err` を `400..=599` 一括分岐に変更:
    ```rust
    s @ 500..=599 => GatewayError::TeeUpstreamError { status: s },
    s @ 400..=499 => GatewayError::TeeRejected { status: s },
    _ => GatewayError::TeeError(format!("TEE upstream returned unexpected HTTP {status}")),
    ```

### new-nitpick-001 `KeysResponse` 等 API 型の docstring が日本語、`# JSON例` セクションが OSS 読者を選ぶ

- 場所: `crates/gateway/src/lib.rs:37-162`
- 観察: モジュールヘッダー (`//!`) は英語、型 (`KeysResponse`, `ProcessorsResponse`, `HealthResponse`, `SolanaKeysResponse`, `SolanaExtensionRequest`, `SolanaExtensionResponse`) は日本語 docstring + `# JSON例` セクション。
- 問題: nitpick-002 (partial) の残債。`cargo doc` で生成すると英日が混在し、API リファレンスとしての品質が落ちる。
- 修正案: 型 docstring を英語化し、仕様参照は `// 仕様書 §X.Y` の line comment に分ける。JSON 例は英語の `# Example` セクションに統一。

### new-nitpick-002 `handle_solana_extension` の二重ガードに意図コメントなし

- 場所: `crates/gateway/src/endpoints.rs:166-190`
- 観察: 先に `is_tee_available()` で 503、次に `cache.solana_keys.is_none()` で 404 を返す順序になっている。これは「TEE が落ちているなら 404 (extension 未対応) より 503 (一時不在) を優先する」設計だが、コードを読んだだけでは意図が伝わらない。
- 修正案: 1 行コメントを追加:
  ```rust
  // §2.5: TEE 落ちは 404 (extension 未対応) より 503 が優先。
  // 復旧後に再度 cache を見て extension の有無を判断する。
  ```

### new-nitpick-003 `RateLimiter::prune_idle` のドキュメントが `last_refill` の更新条件を厳密に述べていない

- 場所: `crates/gateway/src/rate_limit.rs:89-106`
- 観察: doc に「`last_refill` is updated on every request」と書いてあり、コード (`rate_limit.rs:79`) もその通り。`prune_idle` の閾値 `window_secs * 10` (`server.rs:128`) で、リクエストが完全に止まったバケットだけ消える。
- 問題: 致命ではないが、「アイドル中もバケットを維持するクライアントが居る → リクエスト無しでも `last_refill` は更新されない → 閾値超過で消える」 vs 「リクエストが間欠的に届く → 閾値リセット」の挙動差を将来読み手が誤解しないよう、prune の意味論を 1 行加えるとよい。
- 修正案: doc に追記:
  ```
  /// バケット内に未消費トークンが残っていても、`idle_threshold` を
  /// 過ぎた時点で削除する（再作成時は full capacity に戻るため動作は同等）。
  ```

## 全体所感

Round 1 指摘の主要 must-fix 5 件は全件解消、should-fix も 7/9 が完了している。特に must-fix-001（暗号化レスポンス透過）、must-fix-003（middleware コメント）、must-fix-004（503 マッピング）、should-fix-006（partial-failure rollback）の修正は仕様 §1.7 / §2.4 / §2.5 / §5.3 と整合的で、設計判断としても妥当。

残課題は (a) **新設パスのテスト不足**（new-should-001）、(b) **5xx 透過の積み残し**（new-should-002）、(c) **OSS 公開品質**（Cargo.toml workspace 化・doc 英語化・e2e flaky の改善）の 3 つに集約される。タスク 17 で一括対応するのが効率的。

致命的な仕様逸脱・セキュリティ問題は Round 2 では発見されなかった。
