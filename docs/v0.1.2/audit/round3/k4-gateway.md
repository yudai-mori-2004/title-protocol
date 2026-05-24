# K4. crates/gateway 縦深掘り — Round 3

## 概要

- 担当範囲: `crates/gateway/{src/{lib,main,server,endpoints,error,auth,rate_limit,state,tee_client}.rs, tests/e2e.rs, Cargo.toml}` 全行
- 監査方針: Round 2 で挙がった 20 件（must:5 / should:9 / nitpick:6 + 新規 should:2 / nitpick:3）を 1 件ずつ実装と突き合わせ、修正中に混入した regression を拾い、新規問題を 21 観点で再走査。
- 件数サマリ: Round 2 指摘の状態は **resolved 18 / wontfix 7 / open 0**。Round 3 で発見した新規は must-fix 0 / should-fix 1 / nitpick 3。
- 全体評価: Round 2 で指摘した暗号化レスポンス透過と 5xx 透過は実装が入り、回帰テスト (`process_relays_encrypted_bytes_with_octet_stream_content_type`) も追加されている。仕様 §1.7 / §2.4 / §2.5 / §5.3 と整合的な状態に到達している。残課題は OSS 公開向けの細部（API 型 docstring 言語、Cargo.toml workspace 化、e2e flaky bind）と、軽微な観察事項のみ。

## Round 2 指摘の処理状況

| ID | 重大度 | タイトル | Round 2 判定 | Round 3 検証 |
|---|---|---|---|---|
| must-fix-001 | must | 暗号化レスポンス透過 | resolved | `endpoints.rs:101-118` で `ProcessOutcome::{Plaintext,Encrypted}` を分岐、後者は `application/octet-stream` で透過。`tee_client.rs:181-220` で TEE 応答の Content-Type を見て切り替え。Round 2 で言及した「テスト追加」も `server.rs:475-511` の `process_relays_encrypted_bytes_with_octet_stream_content_type` で追加済み。**resolved**。 |
| must-fix-002 | must | リクエストボディサイズ無制限 | resolved | `server.rs:54-79` で `DefaultBodyLimit::max(64 * 1024)` を `/process` と `/extension/solana` に適用。GET 系は body を取らないので影響なし。**resolved**。 |
| must-fix-003 | must | middleware order コメント | resolved | `server.rs:80-86` で「layers added LATER wrap EARLIER」と axum の semantics を正しく記述し、実行順序 `request → rate_limit → auth → handler` の意図と一致。**resolved**。 |
| must-fix-004 | must | TEE 503 が Gateway 502 に化ける | resolved + extended | `endpoints.rs:36-53` で 4xx/5xx 両分岐に拡張。503→`TeeUnavailable`、429→`RateLimited`、4xx→`TeeRejected{status}`、5xx→`TeeUpstreamError{status}`。`error.rs:55-57` で `StatusCode::from_u16(*status).unwrap_or(BAD_GATEWAY)` を介して元 status を透過。**resolved**。 |
| must-fix-005 | must | Authorization 非 UTF-8 で anonymous 化 | resolved | `auth.rs:21-44` の `AuthHeader::{Missing,Bearer,Malformed}` を `parse_auth_header` 経由で auth/rate_limit が共有。`rate_limit.rs:127-130` で Malformed は明示的に `ANONYMOUS_IDENTITY` に寄せ、`parse_non_utf8_header` テスト（`auth.rs:186-196`）でカバー。**resolved**。 |
| should-fix-001 | should | rate-limit メモリリーク | resolved | `rate_limit.rs:97-106` の `prune_idle` + `server.rs:124-139` の 5 分 tick GC。`prune_drops_full_idle_buckets` テスト (`rate_limit.rs:174-183`) で挙動を確認。**resolved**。 |
| should-fix-002 | should | `Mutex::lock().unwrap()` 多用 | wontfix | 本番側 (`rate_limit.rs:66-69, 98-101`) は `unwrap_or_else(into_inner)` で poison 回収済み。残るは `server.rs::tests::MockTeeClient` のテストコードのみ。Round 2 で wontfix 認定。Round 3 でも妥当と判断（テスト用 mock の panic 中 lock は本番に波及しない）。 |
| should-fix-003 | should | reqwest retry 未実装 | wontfix | `tee_client.rs:98-108` で `connect_timeout(5s) / pool_max_idle_per_host(16) / tcp_keepalive(60s) / timeout(300s)` は適用済み。Round 2 で「retry は ALB 等のレイヤで対応」とポリシー確定。Round 3 で再評価しても妥当（idempotent GET でも上流副作用の予測が難しい）。 |
| should-fix-004 | should | health loop drift | resolved | `state.rs:153-167` で `interval` + `MissedTickBehavior::Delay` + 初回 tick 消費を実装。`server.rs:128-138` の GC タスクも同パターン。**resolved**。 |
| should-fix-005 | should | key 変化検知の失敗握りつぶし | resolved | `state.rs:114-123` で `Err(e) => { warn!; true }` の fail-safe。**resolved**。 |
| should-fix-006 | should | `refresh_tee_info` の partial rollback | resolved | `state.rs:82-99` で 4 つの upstream call をローカル `new_cache` に組み立て、全成功時のみ `*self.tee_cache.write().await = new_cache` で原子的 swap。`tee_available` セットも全成功後。**resolved**。 |
| should-fix-007 | should | `Default for GatewayConfig` | resolved | `server.rs:26-46` から `Default` 派生を削除。`main.rs:72-79` と `tests/e2e.rs:100-107` は明示構築。**resolved**。 |
| should-fix-008 | should | `health_check_interval_secs = 0` | resolved | `state.rs:154` で `interval_secs.max(1)` を強制。GC tick 側 (`server.rs:126`) でも `window_secs.saturating_mul(10)` と `max(1)` で同様の防御。**resolved**。 |
| should-fix-009 | should | e2e restart テストの再 bind flaky | wontfix | `tests/e2e.rs:382-396` の `sleep(100ms)` → `TcpListener::bind(tee_addr)` パターンはそのまま。Round 2 で「OSS 公開前に SO_REUSEADDR + retry で対応」とラベル。Round 3 でも構造は同じ。**wontfix（Round 2 判定維持）**。 |
| nitpick-001 | nit | `## Legacy` セクション | resolved | `lib.rs` に Legacy なし。 |
| nitpick-002 | nit | doc 英日混在 | wontfix | API 型 docstring (`lib.rs:37-162`) は日本語 + `# JSON例`、module ヘッダーは英語。Round 2 で「SPECS_JA 引用部の意図的バイリンガル」と整理し、OSS 公開時に統一する方針。 |
| nitpick-003 | nit | `ApiKeySet::contains` constant-time コメント乖離 | resolved | `auth.rs:114-118` で「Length-mismatched entries are skipped, so total runtime leaks the candidate's length (not which entry matched)」と実態と一致。実装 (`auth.rs:119-135`) は branchless XOR で短絡なし。 |
| nitpick-004 | nit | Cargo.toml ローカルバージョン | resolved | `Cargo.toml:14-25` を確認したところ、`title-core = { workspace = true }`、`serde / serde_json / thiserror = { workspace = true }` に既に workspace 化済み。`axum = "0.8"` 等の axum / tokio / reqwest / tracing / async-trait はクレートローカル指定のままだが、これは「axum ファミリは gateway 専用」の妥当な切り分け。Round 2 で「workspace 化は依存整合確認必要、本 audit スコープ外」と判定し wontfix だったが、Round 3 視点でも実害なし。**resolved（gateway 専用依存として整理済み）**。 |
| nitpick-005 | nit | TEE エラーボディ漏れ | resolved | `endpoints.rs:39-49` で `tracing::warn!(status, body = %body, ...)` にログを残しつつ、client には `format!("TEE upstream returned HTTP {status}")` のみ返す。 |
| nitpick-006 | nit | `solana_extension` 二重チェック順序の意図 | resolved | `endpoints.rs:169-184` に「Order matters: a downed TEE returns 503 (transient), which beats the 404 we'd otherwise return ... Once the TEE is back up the cache is rebuilt and the 404 path becomes a real ...」と 4 行コメント。Round 2 の new-nitpick-002 と統合対応済み。 |
| new-should-fix-001 | should | 暗号化レスポンス透過パスのテスト不在 | resolved | `server.rs:174-179, 268-278, 474-511` で `MockTeeClient::process_encrypted_response: Mutex<Option<Vec<u8>>>` を導入し、`process_relays_encrypted_bytes_with_octet_stream_content_type` テストが Content-Type と body の透過を確認。 |
| new-should-fix-002 | should | 5xx 透過 | resolved | `error.rs:33-34` に `TeeUpstreamError { status }` 追加、`endpoints.rs:47` で 5xx (503/429 を除く) を透過。500/502/504 が 502 に潰れる挙動を解消。 |
| new-nitpick-001 | nit | API 型 docstring 英日混在 | wontfix | nitpick-002 と同根。OSS 公開時に統一の方針。 |
| new-nitpick-002 | nit | `handle_solana_extension` 順序コメント | resolved | nitpick-006 と統合。 |
| new-nitpick-003 | nit | `prune_idle` doc 詳細 | wontfix | `rate_limit.rs:89-96` に「`last_refill` is updated on every request, so age since last refill is an upper bound...」と挙動を記述。Round 2 で「将来の rate-limit 拡張時に整理」と判定。Round 3 視点でも記述は十分（doc に「未消費トークン残でも閾値過ぎたら削除」の一文があると親切だが nitpick の範疇）。 |

## Round 3 新規発見

### new-should-fix-001 `TeeUpstreamError`/`TeeRejected` の HTTP 透過テストが無い

- 場所: `crates/gateway/src/endpoints.rs:36-53`、`crates/gateway/src/error.rs:50-66`、`crates/gateway/src/server.rs` (tests)
- 観察: Round 2 で 5xx 透過を実装し、Round 2 → Round 3 で `TeeUpstreamError { status }` バリアントが新設されたが、`MockTeeClient` (`server.rs:169-300`) は `TeeClientError::Unreachable` しか返せず、`TeeClientError::HttpError { status, body }` を注入する経路が無い。結果として:
  - 「TEE が 504 を返したら Gateway も 504」
  - 「TEE が 400 を返したら Gateway も 400」
  - 「TEE が 429 を返したら Gateway も 429（`RateLimited`）」
  という must-fix-004/new-should-fix-002 の合意点が回帰テストで保護されていない。`error.rs:69-95` の `error_status_codes` も `TeeRejected`/`TeeUpstreamError` バリアントを含めていない（fold 漏れ）。
- 問題: 将来 `tee_err` の match arm を誤って書き換えても CI で気付けない。must-fix-004 の修正が「コードは正しい」だけで「振る舞いがテストに固定されていない」状態。Round 2 で同じ理由から指摘した new-should-fix-001（暗号化透過のテスト不足）と並行のギャップ。
- 修正案:
  - `MockTeeClient` に `process_http_error: Mutex<Option<(u16, String)>>` を足し、`process()` が `Some((s, b))` の場合に `Err(TeeClientError::HttpError { status: s, body: b })` を返すコンストラクタを追加。
  - `server.rs::tests` に `process_propagates_429_as_rate_limited / 503_as_unavailable / 504_as_504 / 400_as_400` を追加（各 status での `response.status()` を assert）。
  - `error.rs::tests::error_status_codes` に `TeeRejected { status: 403 } → 403`、`TeeUpstreamError { status: 504 } → 504` を追加。

### new-nitpick-001 `handle_solana_extension` の cache read を中継後まで持ち越さない設計が暗黙

- 場所: `crates/gateway/src/endpoints.rs:177-191`
- 観察: 184 行で `cache` の `RwLock` ガードを明示スコープ `{ ... }` で落としてから 186 行で `tee_client.solana_extension(&request).await` を呼んでいる。これは `RwLock` の read ガードを `.await` 越しに持ち越すと別タスクの write がブロックされるのを避けるため意図的に分けてある。が、コメントが無いので将来 `if let Some(..) = cache.solana_keys` のような「ガード越え」リファクタが入ると、レスポンス到着まで write を止めることになる。
- 問題: 致命ではない。ただ Round 3 視点では `handle_keys` / `handle_processors` / `handle_solana_keys` も同様にガードを早く落としており、Gateway 全体で「`tee_cache.read()` は最短スコープ」のルールが暗黙合意になっている。コメントなしの暗黙ルールは事故の元。
- 修正案: `endpoints.rs:177` のブロック冒頭に 1 行:
  ```rust
  // Drop the cache read guard before the .await below: holding a tokio::sync::RwLock
  // read guard across an await would block writers for the duration of the TEE call.
  ```

### new-nitpick-002 `handle_health` は cache lock を握ったまま `is_tee_available` の atomic load を行う

- 場所: `crates/gateway/src/endpoints.rs:129-138`
- 観察: 130 行で `state.tee_cache.read().await` を取り、132 行で `state.is_tee_available()` を呼ぶ。`is_tee_available` は atomic load (`state.rs:141-143`) なのでブロックしないが、`tee_cache` の read ガードは `tee_type.clone()` 完了まで保持される。`/health` は常時呼ばれ得るパス（特にロードバランサや Gateway 自身の `spawn_health_check`）であり、read 同士は競合しないものの、`refresh_tee_info` の write を一瞬待たせる可能性がある。
- 問題: 致命ではない。`HealthResponse` の `tee_type` は `Option<String>` なので clone コストは小さい。ただ「health はどんな状態でも即答できるべき」という §2.5 の要求（認証なしで応答、Gateway 自身の health checker も叩く）に対し、最短経路にできる。
- 修正案: clone を read ガード外に出す:
  ```rust
  let tee_type = {
      let cache = state.tee_cache.read().await;
      cache.tee_type.clone()
  };
  let status = if state.is_tee_available() { "ok" } else { "unavailable" };
  Json(HealthResponse { status: status.to_string(), tee_type })
  ```

### new-nitpick-003 `handle_health` は TEE 不在時も 200 OK を返す

- 場所: `crates/gateway/src/endpoints.rs:129-138`、`crates/gateway/src/server.rs:408-416` (`health_returns_unavailable_when_tee_down`)
- 観察: TEE が落ちている状態でも `/health` は **HTTP 200** + `{"status":"unavailable"}` を返す。テスト `health_returns_unavailable_when_tee_down` も `StatusCode::OK` を assert する。SPECS §2.5 (lines 644-662) には `status` の値仕様しかなく、「unavailable のとき HTTP は何を返すか」は明示されていない。
- 問題: 多くのロードバランサ (k8s liveness/readiness, ALB target group) は HTTP 5xx 以外を healthy と扱う。`status:"unavailable"` body を JSON で読むには別ロジックが要る。本番運用で `/health` を readiness probe にすると、TEE 落ちでも Gateway pod が回り続け、5xx は出ないがリクエストは 503 を吐く奇妙な状態が長く続く可能性がある。
- 問題が「致命ではない」理由: SPECS §2.5 が「常に 200」を明示しているとも読める（status 値で判別する設計）し、運用側で `jq .status` で readiness probe を組めば動く。設計判断としては正当。
- 修正案（仕様確認が必要）:
  - SPECS §2.5 に「`unavailable` の際の HTTP status は 200 / 503 のいずれか」を明文化。
  - 200 維持なら、コードコメントに「LB の readiness probe を直結する場合は jq で `.status` を見ること」と一行注意書きを追加。
  - 503 に変えるなら、`server.rs:408-416` テストを更新。仕様変更で破壊的になるので独立タスクが妥当。

## Round 2 → Round 3 で混入した regression

なし。Round 2 で resolved 認定された項目は Round 3 でも実装と挙動が一致。`TeeUpstreamError` 追加で `error.rs:50-66` の match arm が `TeeRejected | TeeUpstreamError` の or-pattern になっているが、両者の status 取り出しが同一構造なので問題なし。

## 全体所感

Round 1 → Round 2 → Round 3 を通じて、Gateway の must-fix 系課題（暗号化透過 / middleware order / TEE status 透過 / Authorization 解析 / body size）はすべて実装＋コメント＋テストが揃った。Round 3 視点で残るのは:

1. **テスト網羅**: `TeeClientError::HttpError` パスの mock 注入機構が無く、Round 2 で実装した 4xx/5xx 透過の回帰が固定されていない（new-should-fix-001）。これは Round 2 の new-should-fix-001（暗号化透過のテスト追加）と同じ構造の「コードは正しいがテストが守れていない」ギャップ。タスク追加の余地が大きい。
2. **OSS 公開品質**: API 型 docstring の英日混在、Cargo.toml の axum 系ローカル指定、e2e restart テストの flaky bind は Round 2 から「OSS 公開時にまとめる」と整理されている。Round 3 でも判定は維持。
3. **設計の暗黙ルール**: `tee_cache` の read ガードを `.await` 越しに持ち越さないルール、`/health` が 200 OK 固定で返るルール、`prune_idle` の閾値選定（`window_secs * 10`）の根拠など、コードからは読めるがコメントが薄い箇所がいくつか。new-nitpick-001/002/003 はこの種の積み残し。

仕様 §1.7 / §2.4 / §2.5 / §5.3 / §6.2 と Gateway 実装の semantics は整合している。クリティカルな脆弱性・仕様逸脱は Round 3 でも発見されなかった。次の動きとしては「タスク 17（または後続）で new-should-fix-001 のテスト追加 + new-nitpick-001/002/003 のコメント補強」を 1 セッションでまとめるのが効率的。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| must-fix-001..005 | fixed | Round 2 で resolved、Round 3 で再確認。 |
| should-fix-001/004/005/006/007/008 | fixed | Round 2 で resolved、Round 3 で再確認。 |
| should-fix-002 | wontfix(MockTeeClient テストのみ。Round 2 判定維持) | |
| should-fix-003 | wontfix(reqwest retry は ALB レイヤで対応。Round 2 判定維持) | |
| should-fix-009 | wontfix(e2e restart の flaky bind は OSS 公開時にまとめて対応。Round 2 判定維持) | |
| nitpick-001/003/005/006 | fixed | Round 2 で resolved、Round 3 で再確認。 |
| nitpick-002 | wontfix(API 型 docstring の英日混在は OSS 公開時に統一) | |
| nitpick-004 | fixed | gateway 専用依存は workspace 化不要、workspace 化済み依存と切り分け確認。 |
| new-should-fix-001 (Round2) | fixed | `process_encrypted_response` mock + `process_relays_encrypted_bytes_with_octet_stream_content_type` テストで確認。 |
| new-should-fix-002 (Round2) | fixed | `TeeUpstreamError { status }` で 5xx を透過。 |
| new-nitpick-001 (Round2) | wontfix(nitpick-002 と同根) | |
| new-nitpick-002 (Round2) | fixed | `handle_solana_extension` の順序コメントが明文化。 |
| new-nitpick-003 (Round2) | wontfix(`prune_idle` の挙動は doc に書かれており致命ではない) | |
| new-should-fix-001 (Round3) | fixed | `MockTeeClient` に `process_http_error: Mutex<Option<(u16, String)>>` フィールドを追加し、`process()` 内で `Some` なら `TeeClientError::HttpError` を返すよう実装。4xx/5xx 透過の回帰テスト 4 本 (429→429, 503→503, 400→400, 504→504) を `server.rs` に追加。`error.rs::error_status_codes` にも `TeeRejected { status: 403 }`, `TeeUpstreamError { status: 504 }`, `TeeRejected { status: 0 }` (フォールバック確認) を追加。 |
| new-nitpick-001 (Round3) | fixed | `handle_solana_extension` の cache 早期 drop ブロックに「RwLock read ガードを `.await` 越しに持ち越すと writer が止まる」理由を 3 行コメント化。 |
| new-nitpick-002 (Round3) | fixed | `handle_health` の `tee_cache.read()` ガードを最短スコープに移動。`tee_type.clone()` だけブロック内で行い、`is_tee_available()` の atomic load は外。 |
| new-nitpick-003 (Round3) | wontfix | `/health` の TEE 不在時 HTTP は 200 維持。SPECS §2.5 は status 値判別設計と読める。LB の readiness probe は `jq .status` で吸収可能。仕様変更を nitpick 範囲で出さない。 |
