# K4. crates/gateway 縦深掘り

## 概要

- 担当範囲: `crates/gateway/` の全ファイル
  - `src/{lib.rs, main.rs, server.rs, endpoints.rs, auth.rs, rate_limit.rs, state.rs, tee_client.rs, error.rs}`
  - `tests/e2e.rs`
  - `Cargo.toml`
- 監査方針: 仕様書 §1.7 / §2.5 / §5.3 と実装の対応を 1 文単位で確認したうえで、API ハンドラ・middleware・TEE client・state 管理・テスト 5 観点を縦に掘る。仕様逸脱・暗号化レスポンスの取り扱い・middleware order・retry / timeout・bucket リーク・テストの本質性を重点的に確認する。
- 件数サマリ: must-fix 5, should-fix 9, nitpick 6（合計 20 件）

## 重大度別内訳

- must-fix: 5 件
- should-fix: 9 件
- nitpick: 6 件

## 発見

### must-fix-001 暗号化レスポンス（`application/octet-stream`）を扱えない致命バグ

- 場所: `crates/gateway/src/endpoints.rs:90-106`、`crates/gateway/src/tee_client.rs:118-141, 158-160`
- 観察:
  - `handle_process` のシグネチャは `Result<Json<ProcessResponse>, GatewayError>`。
  - `HttpTeeClient::post::<_, ProcessResponse>` は内部で `resp.json().await` を呼び、レスポンス本体を JSON としてデシリアライズする。
  - 一方 TEE は `crates/tee/src/server.rs:151-159` で、`encryption` 付きリクエストに対して `Content-Type: application/octet-stream` の `nonce || ciphertext` を返す（仕様書 §2.3, §2.4）。
- 問題: 仕様書 §1.7 / §2.5 では Gateway は POST /process を「中継」する役割と定義され、特に §2.3「暗号化モードでは上記の JSON が response_key で暗号化された状態で返却される」を踏まえると、暗号化リクエストは Gateway をそのまま通過するはずである。実装は JSON 前提のため、暗号化リクエストでは `TeeClientError::ParseError` → `GatewayError::TeeError` (BAD_GATEWAY) に化けて 502 が返る。仕様の主要シナリオ（クライアントサイド利用 §1.7）が**まったく動かない**。
- 修正案:
  - `TeeClient::process` の戻り値を `enum ProcessOutcome { Plaintext(ProcessResponse), Encrypted(Vec<u8>) }` に変更し、`HttpTeeClient::process` で `resp.headers().get(CONTENT_TYPE)` を判別。`application/octet-stream` なら `resp.bytes().await` を返す。
  - `handle_process` の戻り値を `axum::response::Response` 直接に変更し、Plaintext は `Json(response).into_response()`、Encrypted は `(StatusCode::OK, [(CONTENT_TYPE, "application/octet-stream")], bytes).into_response()` を返す（TEE 側 `crates/tee/src/server.rs:147-159` と同型）。
  - 併せて E2E に「暗号化リクエスト → 暗号化レスポンスがそのまま透過される」テストを 1 本追加（現状 `tests/e2e.rs` には暗号化往復のケースがない）。

### must-fix-002 リクエストボディサイズ無制限（DoS 経路）

- 場所: `crates/gateway/src/server.rs:71-96`
- 観察: ルータに `axum::extract::DefaultBodyLimit` の上書きが無い。axum 0.8 の `Json` 抽出器のデフォルト上限は 2 MiB だが、これは事実上ドキュメント上の値で、深部の middleware を後段で追加すると挙動が崩れることがある。さらに POST /extension/solana / /process いずれも明示的上限指定なし。
- 問題: 仕様書 §4.4「データサイズの上限」は TEE 側の責務として明記されているが、**Gateway が無制限にボディを読んで TEE に転送する**と、Gateway のメモリと帯域だけで枯渇させられる。コンテンツ本体は Gateway を通らないものの（§2.1 末尾）、リクエスト JSON 自体に巨大な `fragment_urls` 配列等を詰め込まれるとどこまでも蓄積する。
- 修正案:
  - `router(state)` の最後に `.layer(DefaultBodyLimit::max(64 * 1024))` を明示的に追加（仕様で要求していない以上、保守的に 64 KiB 程度。`fragment_urls` の現実的上限が分かれば §4.4 と整合する値に。）
  - `GatewayConfig` に `max_request_body_bytes: usize` を追加して環境変数 `GATEWAY_MAX_REQUEST_BODY_BYTES` から設定可能にする。

### must-fix-003 middleware order が仕様と意図ともに逆順で適用される

- 場所: `crates/gateway/src/server.rs:88-94`
- 観察:
  ```rust
  // Layer order: outermost runs first. We want rate limiting to gate
  // even unauthenticated requests, so it sits *outside* the auth layer.
  .layer(middleware::from_fn_with_state(state.clone(), api_key_auth))
  .layer(middleware::from_fn_with_state(state.clone(), rate_limit_middleware))
  ```
  axum / tower の `Router::layer` は「**後から追加した層が外側**」になる（`ServiceBuilder` と逆順）。したがって現状の適用順は **外側 = rate_limit → 内側 = auth** で、コメントが述べる挙動と「たまたま一致」している。
- 問題: コメントは正しいが、**理由付けが誤り**で読み手を惑わせる。仕様（§5.3）も意図（認証より先に rate-limit でガード）も axum の意味論を理解していないと「逆では？」と疑われ、リファクタ時に誤って入れ替えられる危険がある。実際にここで auth が外側になると、anonymous バケットの動作（§rate_limit.rs:84-89 の説明）が崩れ、未認証リクエストが Bucket を使わず 401 に弾かれてしまう。
- 修正案: コメントを書き直す。
  ```rust
  // axum/tower: layers added LATER wrap EARLIER ones (the last `.layer`
  // call becomes the outermost middleware). We add auth first, then
  // rate_limit, so the runtime order is:
  //   request -> rate_limit -> auth -> handler
  // This lets the anonymous bucket throttle unauthenticated traffic
  // before auth has a chance to 401 it.
  ```
  さらに `#[cfg(test)]` で「rate-limit が auth より先に走る」ことを直接検証する回帰テストを追加（例: 無効な API key で `RATE_LIMIT_MAX + 1` 件叩くと、最後の 1 件は 401 ではなく 429）。

### must-fix-004 TEE が 503 を返した場合 Gateway は 502 (Bad Gateway) に化ける

- 場所: `crates/gateway/src/endpoints.rs:34-42`, `crates/tee/src/server.rs:160-163`
- 観察: TEE の `AdmissionRejected` は 503 + JSON で返る。Gateway の `tee_err` は `HttpError` を一律 `GatewayError::TeeError` → 502 BAD_GATEWAY にマップしている。
- 問題: TEE が混雑時に「あとで再試行を」と 503 を返したのに、クライアントには 502（上流が壊れた）として届くため、リトライ戦略が誤る。`crates/gateway/src/error.rs:42` のマッピングは「Gateway 自身がリーチできない」ケースと「TEE が業務的に拒否したケース」を区別すべき。
- 修正案: `tee_err` 内で `HttpError { status, .. }` を見て、`503` は `GatewayError::TeeUnavailable(body)` に、`429` は `GatewayError::RateLimited` に、`400-499` は新規バリアント `GatewayError::TeeRejected { status, body }` (そのまま透過) に分岐する。仕様書 §2.5 が要求する「中継」を強めるならステータスコードは透過が望ましい。

### must-fix-005 `Authorization` ヘッダ抽出が UTF-8 でないと panic 経由で 500 を返す（実際は None だが要整理）

- 場所: `crates/gateway/src/auth.rs:21-27`, `crates/gateway/src/rate_limit.rs:99-105`
- 観察: 厳密に panic ではないが、`to_str()` が `Err` のとき `and_then` で None 落ちし、auth では `Missing Authorization header` という**誤った理由メッセージ**で 401 が返る。rate_limit 側では anonymous バケット扱いになり、本物の API key 持ちと anonymous が混ざる可能性がある。
- 問題: 認証エラーメッセージが事実と食い違うこと、および「invalid UTF-8 bearer を anonymous として扱うことで、攻撃者が anonymous バケットへの DoS 経路を握れる」状況が小さなリスクとして残る。
- 修正案:
  - `extract_api_key` を `Result<Option<String>, GatewayError>` に変更し、ヘッダがあるが非 UTF-8 や非 Bearer のときは `Unauthorized("Malformed Authorization header")` を返す。
  - rate_limit 側の identity 抽出も同関数を共有する（重複ロジックの解消も兼ねる）。

---

### should-fix-001 rate-limit バケットがプロセス寿命の間ずっと積もり続ける（メモリリーク）

- 場所: `crates/gateway/src/rate_limit.rs:33-82`
- 観察: `HashMap<String, TokenBucket>` に `entry().or_insert()` するだけで、満タンになった bucket を削除する処理がない。anonymous 以外は API key ごとに 1 バケット。
- 問題: 攻撃者が認証バイパスせずとも、ローテーションで使い捨ての Bearer 値を投げ続けるだけで `HashMap` が無制限に成長する。30 文字キー想定で 100 万キー ≒ 数十 MiB だが、攻撃が長期化すれば容易に GiB 規模になる。
- 修正案:
  - background task で 5 分おきに `bucket.tokens >= max_tokens && now - last_refill > 10*window` のエントリを GC する。
  - もしくは LRU (`linked-hash-map` か `hashlink`) で最大エントリ数を環境変数で制御。

### should-fix-002 `Mutex::lock().unwrap()` 多用（panic 経路を作る）

- 場所: `crates/gateway/src/rate_limit.rs:62`、テスト `server.rs:211,222,233,247,258,268` 他多数
- 観察: 本体側の `let mut buckets = self.buckets.lock().unwrap();` は、別タスクが lock 中に panic すると `PoisonError` で Gateway 全体が落ちる。
- 問題: `axum::extract::State` から共有される `RateLimiter` は全リクエストの相互排他ポイントになる。panic 一発で恒久的に 500 を返す状態に陥る。
- 修正案: `parking_lot::Mutex` に置換するか、`buckets.lock().unwrap_or_else(|e| e.into_inner())` の defensive 回収を入れる。並行性能の観点では bucket 単位で `tokio::sync::Mutex` か `dashmap` への移行が望ましい。

### should-fix-003 reqwest::Client に retry 戦略が無い

- 場所: `crates/gateway/src/tee_client.rs:80-93`
- 観察: `timeout(300s)` だけが指定されている。connect timeout / retry / pool 上限の指定なし。
- 問題: TEE が再起動中（再起動シーケンス §5.2 / §5.3）の数秒間、`GET /keys` 等が単発で失敗し `tee_available=false` になる。実体は瞬断だが、ユーザリクエストが続けて 503 を喰らう。
- 修正案:
  - `connect_timeout(5s)`、`pool_max_idle_per_host(16)`、`tcp_keepalive(60s)` を追加。
  - `health()` / `keys()` / `processors()` には `tokio_retry` で「100ms から 3 回指数バックオフ」を入れる（idempotent な GET のみ）。
  - `process()` には retry しない（副作用がある + Attestation の冪等性は仕様に書かれていない）。

### should-fix-004 health check loop が `tokio::time::interval` を使わずズレる

- 場所: `crates/gateway/src/state.rs:135-143`
- 観察: `loop { check_and_refresh().await; sleep(interval).await; }` 構造のため、`check_and_refresh` 自体の実行時間ぶん間隔が伸びる。TEE health の `timeout=300s` と組み合わさると最悪 5 分以上ポーリングが止まる。
- 修正案: `let mut ticker = tokio::time::interval(interval); ticker.set_missed_tick_behavior(MissedTickBehavior::Delay); loop { ticker.tick().await; state.check_and_refresh().await; }` に書き直す。

### should-fix-005 key change 検知が「フルマップ全等価比較」で偽陽性／偽陰性のリスク

- 場所: `crates/gateway/src/state.rs:104-110`
- 観察:
  ```rust
  let keys_changed = match self.tee_client.keys().await {
      Ok(live_keys) => {
          let cache = self.tee_cache.read().await;
          cache.keys.as_ref() != Some(&live_keys)
      }
      Err(_) => false,
  };
  ```
  - `Err(_)` を握りつぶしているため、`keys()` が失敗してもキャッシュ更新が走らない。
  - `KeysResponse` は `HashMap<String, String>` で `PartialEq` 比較が「キー集合 + 各値」を見る。スイートが片方追加された場合は変化扱いだが、TEE がエラー文字列を返してフォールバック値を入れていた等の境界ケースで誤検知しうる。
- 問題: TEE 再起動検知（§5.3）の信頼性に直結する。
- 修正案:
  - `Err(e)` のときは `tracing::warn!(error=%e)` でログを残し、`keys_changed=true` 扱いにして強制 refresh する（fail-safe 側）。
  - もしくは TEE に instance_id / boot_id を返させて、それで一致判定する（仕様書を §2.5 GET /health に拡張するなら別タスクで提案）。

### should-fix-006 `refresh_tee_info` が部分失敗をロールバックしない

- 場所: `crates/gateway/src/state.rs:77-92`
- 観察: `health → keys → processors → solana_keys` を順に呼び、どれか一つでも Err なら関数全体が早期 return する。一方で `tee_available` は最後にしか `true` にしないので、部分失敗時にキャッシュは触られず一見安全。しかし `solana_keys` で失敗すると、`keys` と `processors` の最新値は反映されない（既存キャッシュが残る）。
- 問題: TEE 再起動直後、新しい keys が取れたのに solana_keys だけ過渡的に失敗 → `/keys` には古い鍵が残り続け、クライアントが古い公開鍵で暗号化 → 復号失敗で詰む。
- 修正案: ローカルに新 `TeeInfoCache` を組み立て、全フィールドが揃ったときだけ swap する。あるいは取得した個別エンドポイントごとに `tee_cache` の対応フィールドのみ更新する。

### should-fix-007 `Default for GatewayConfig` が production で誤起動を招く

- 場所: `crates/gateway/src/server.rs:48-61`
- 観察: `Default` 実装が `0.0.0.0:3000` と `http://localhost:4000` を含む。プロダクションでは `Default::default()` 経由で気づかず公開バインドする恐れがある。
- 問題: `main.rs` は環境変数経由で必ず明示的に組み立てるため現状無害だが、E2E テスト等で `.. Default::default()` を許すと、テスト中に 0.0.0.0:3000 を奪う／重複バインドする事故を招く。
- 修正案: `Default` を削除し、テスト用に明示的な `for_testing()` コンストラクタを用意する。あるいは `bind_addr: Option<String>` + `Default` で `None` にする。

### should-fix-008 `health_check_interval_secs = 0` でホットループ化する

- 場所: `crates/gateway/src/main.rs:60-63`, `crates/gateway/src/state.rs:135-143`
- 観察: `HEALTH_CHECK_INTERVAL_SECS=0` が来た場合、`Duration::from_secs(0)` で `sleep(0)` となり TEE を秒間数千回叩く。
- 修正案: `main.rs` で `interval_secs.max(1)` を強制、もしくは parse 段階で 0 を弾く。

### should-fix-009 e2e の TEE 再起動テストが「同一ポート bind」に依存して flaky

- 場所: `crates/gateway/tests/e2e.rs:362-432`
- 観察: TEE #1 を `abort()` 後 `sleep(100ms)`、即座に同 `tee_addr` を再 bind。OS によっては `SO_REUSEADDR` 無しで `TIME_WAIT` に取られ失敗する（macOS では特に）。
- 修正案: `TcpListener::bind` を socket2 経由で SO_REUSEADDR/SO_REUSEPORT を立てる、もしくは TEE #2 を別ポートに上げ、Gateway の `HttpTeeClient` を新しい endpoint に差し替える（このための setter が無いなら追加）。最も妥当なのは「TEE Endpoint を `RwLock<String>`」化、あるいは health check の対象を変更可能にする実装の拡張（既に should-fix 候補）。

---

### nitpick-001 lib.rs `## Legacy` セクションが「ない情報」を埋め込んでいる

- 場所: `crates/gateway/src/lib.rs:21-23`
- 観察: `//! legacy/v0.1.0/crates/gateway/ -- Previous Gateway implementation (Axum).`
- 問題: 旧版の場所を OSS 読者に告知する価値が薄い。CHANGELOG / README で扱うべき情報。
- 修正案: セクションごと削除。

### nitpick-002 doc コメントが英日混在

- 場所: `crates/gateway/src/lib.rs`, `endpoints.rs`, `auth.rs`, `error.rs`, `state.rs`, `server.rs` 全般
- 観察: 同じファイル内で `//! # Gateway Error Type` (英) と `/// GET /keys レスポンス。` (日) が混在。
- 問題: OSS 公開時の読者層が広がるので、どちらかに統一すべき。CLAUDE.md 流の「Doc comments with spec section references」は維持しつつ、見出しレベルの doc は日英のどちらかに揃える。
- 修正案: モジュールヘッダー (`//!`) は英語、型 / 関数の docstring も英語に統一（仕様参照は `// 仕様書 §X.Y` のまま）。`SPECS_JA` を引く都合上、引用部だけ日本語可。

### nitpick-003 `ApiKeySet::contains` の constant-time 主張のコメントが長すぎる + 部分的に正確でない

- 場所: `crates/gateway/src/auth.rs:86-116`
- 観察: 「length-mismatched entries still consume a constant number of comparisons against a fixed zero buffer」と書いているが、実装は `continue` しているだけで「ゼロバッファ比較」はしていない。コメントと実装が一致しない。
- 問題: 監査読み手は実装通り「長さ不一致は短絡」と読めるが、コメントは「定数時間」と書く。コメントが嘘になっている。
- 修正案: コメントを実態に合わせて書き直す。
  ```rust
  /// Compare candidate against every configured key using a branchless
  /// XOR accumulator. Length-mismatched entries are skipped (so total
  /// time leaks the candidate's length but not which entry matched).
  /// API keys are high-entropy fixed-length tokens, so this leak is
  /// negligible. Never short-circuits on a match.
  ```
  または `subtle::ConstantTimeEq` を導入して真の定数時間にする（後者推奨）。

### nitpick-004 `Cargo.toml` のバージョン指定が workspace 化されていない

- 場所: `crates/gateway/Cargo.toml:19-24`
- 観察: `axum = "0.8"`, `tokio = { version = "1", ... }`, `reqwest = "0.12"`, `tracing = "0.1"`, `tracing-subscriber = "0.3"`, `async-trait = "0.1"` がすべて crate ローカル指定。ワークスペース他 crate（tee, proxy）と二重定義になる。
- 問題: 同一依存のバージョン drift が起きる。実際 `Cargo.toml` workspace に `reqwest = "0.12", default-features = false, features = ["blocking", "rustls-tls"]` があるのに、gateway 側は `["json", "rustls-tls"]` で独立定義。feature 解決はマージされるが、`blocking` が無自覚に有効化される。
- 修正案: workspace deps に `axum`, `tokio`, `reqwest = { ... features = ["json", "rustls-tls"] }`（blocking は別 alias）, `tracing`, `tracing-subscriber`, `async-trait` を追加し、各 crate は `axum = { workspace = true }` 形式へ。

### nitpick-005 endpoints.rs の `TeeError("HTTP {status}: {body}")` でレスポンスボディが利用者に漏れる

- 場所: `crates/gateway/src/endpoints.rs:38`
- 観察: TEE が返したエラーボディがそのまま `to_string()` で error JSON に同梱される。
- 問題: TEE がスタックトレース・内部 path・処理対象 URL の一部を漏らした場合、Gateway 経由でクライアントに漏れる。仕様書 §1.7「Gateway は中継のみ」を考えるとボディ透過はむしろ正しいが、ログとレスポンス本文を分けるべき。
- 修正案: クライアントには `{"error": "TEE upstream returned HTTP 500"}` のような短縮形を返し、詳細は `tracing::warn!` に出す。

### nitpick-006 `solana_extension` ハンドラの `is_tee_available` ガードが二重チェックになっている

- 場所: `crates/gateway/src/endpoints.rs:155-178`
- 観察: `is_tee_available()` で 503 を返したあと、`cache.solana_keys.is_none()` で 404 を返す。TEE が落ちている瞬間にも `solana_keys` がキャッシュに残っているケース（再起動直前にキャッシュされた値）では「TEE 落 + Solana 有効」と認識されて 503、その後の 404 ロジックには到達しない。順序自体は正しい。
- 修正案: コメントで意図を 1 行明記（「TEE 不在のときは 404 より 503 が優先」）。

## 全体所感

仕様 §2.5 の 6 エンドポイントを薄く中継するという設計は明快で、テストも十分な数（unit + e2e）がある。一方で、仕様の根幹である「暗号化リクエスト/レスポンスの透過」(§2.3, §2.4) を Gateway が**ハードコードで JSON のみに固定**しており、これは仕様逸脱のレベルにある（must-fix-001）。さらに DoS 経路（must-fix-002, should-fix-001）と middleware order のコメント誤り（must-fix-003）は OSS としての品質に直結するため、タスク 17 で優先的に潰すべき。constant-time 主張・dependency 重複・layer 順序の説明など「思考過程の embedded comment」が随所にあり、A 観点（comment-hygiene）とも重なる。
