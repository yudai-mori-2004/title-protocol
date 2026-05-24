# K6 — `crates/proxy` 縦深掘り監査（Round 3）

## 概要

- 担当範囲: `crates/proxy/{Cargo.toml,src/main.rs,src/handler.rs,src/protocol.rs}`、突合先 `crates/tee/src/proxy_fetcher.rs`、`deploy/aws/scripts/run-stack.sh`、`docs/v0.1.2/SPECS_JA.md` §5.2。
- 監査方針: Round 2 の 22 件（must-fix:6 / should-fix:10 / nitpick:6）について「修正済み・wontfix・残課題」の判定がコードと整合しているかを 1 件ずつ突き合わせ、Round 2 の修正（`CHUNKED_TRUNCATED` 導入、`MAX_*` 上限、`MIN_ACCEPTED_CID`、`try_send` backpressure）が新規の問題を生んでいないか、また Round 2 で見落としていた攻撃面が無いかを確認する。
- ファイル規模は Round 2 から `protocol.rs` が 126L→131L、`handler.rs` が 224L→249L、`main.rs` が 345L→366L と微増。`CHUNKED_TRUNCATED` 定数と TEE 側 `read_chunked_body` の対応が新規追加され、wire spec の docstring (`protocol.rs:18-36`) も両端で同期している。
- 件数サマリ: Round 2 由来 = fixed:7 / wontfix-accepted:13 / still-open:2、新規発見 = must-fix:2 / should-fix:3 / nitpick:3 = 計 8 件。

## 重大度別内訳（新規発見）

- must-fix: 2 件
- should-fix: 3 件
- nitpick: 3 件

## Round 2 指摘の処理状況

| ID | Round 2 判定 | Round 3 判定 | 備考 |
|---|---|---|---|
| must-fix-001 | fixed | **confirmed fixed** | `CHUNKED_SENTINEL` (`protocol.rs:46`) の wire spec 化と handler/fetcher 両端の対称実装は維持。`chunked_get_uses_sentinel` (`main.rs:313-365`) が end-to-end カバー。 |
| must-fix-002 | fixed | **confirmed fixed** | `MAX_METHOD_BYTES=16`, `MAX_URL_BYTES=8 KiB`, `MAX_REQUEST_BODY_BYTES=8 MiB`, `MAX_RESPONSE_BYTES=100 MiB` を `protocol.rs:55-62` に保持。`read_bytes_async/sync` (`protocol.rs:84-99, 119-131`) で事前に length 検証する設計は崩れていない。TEE 側 `proxy_fetcher.rs:171-178` も `max_body_bytes` 比較で対称的に拒否。 |
| must-fix-003 | partially-fixed | **accepted-as-wontfix** | `unsafe impl Send for VsockWriter` (`main.rs:170-176`) の Safety コメントは 6 行に拡張済み。Round 2 の処理ログで「vsock 0.5 の API 制約で OwnedFd 分割は不可」と判定済み。Round 3 で `vsock 0.5` のソースを確認しても `VsockStream` が `RawFd` を内蔵するだけで `into_raw_fd()` で剥がす以上の手段が無いことを再確認した。現状の `try_clone` + 1 タスク所有モデルは妥当。 |
| must-fix-004 | fixed | **confirmed fixed** | `handler.rs:90-97`（Content-Length 既知パス）、`handler.rs:140-148`（chunked パス）、`handler.rs:174-183`（非 GET / エラー経路）の 3 経路すべてで `MAX_RESPONSE_BYTES` 比較が入っている。`len as u32` キャストは 100 MiB 上限の保護下で安全。**ただし非 GET 経路の上限チェックは `response.bytes().await` の後に行われており、新規 must-fix-007 として再掲する。** |
| must-fix-005 | unchanged (wontfix) | **still-open** | `deploy/aws/scripts/run-stack.sh:49-59` は依然 `--privileged`。Round 2 処理ログでは「commit 54e034f の justification コメントで G ラウンド確定」とあるが、Round 3 で読んでも代替検証ログ（seccomp 単独・cap-add 単独で何が壊れたか）は `deploy/aws/README.md` にも `OPERATIONS_JA.md` にも残っていない。OSS 公開後の外部監査人が「`--privileged` の理由はコメント以外に無い」状態に変わりはなく、wontfix 判定はリスクを文書化していないだけで赤旗自体は残る。 |
| should-fix-001 | fixed | **confirmed fixed** | `tx.try_send` ＋ `tracing::warn!` (`main.rs:49-62`) は維持。 |
| should-fix-002 | fixed | **confirmed fixed** | `duration_ms`, `upstream_host` のアクセスログ統一は維持。`format!("{e:#}")` で source チェーンも残る (`handler.rs:71-74, 164-167`)。 |
| should-fix-003 | unchanged (wontfix) | **still-open** | must-fix-005 と同根。 |
| should-fix-004 | fixed | **confirmed fixed** | `PROXY_CONNECT_TIMEOUT_SECS` / `PROXY_REQUEST_TIMEOUT_SECS` env はデフォルト 10s/120s で維持 (`handler.rs:10-20, 39-46`)。 |
| should-fix-005 | partially-fixed | **accepted-as-wontfix** | `Content-Type` 強制付与は削除済み。任意 header pass-through は Solana RPC 動作で不要との Round 2 判定を再確認。 |
| should-fix-006 | partially-fixed | **accepted-as-wontfix(F観点へ移送)** | `handler.rs:54-55` のコメントは維持。`SPECS_JA.md` §5.2 への method allowlist 明記は F-docs 観点で別途扱う。 |
| should-fix-007 | unchanged | **accepted-as-wontfix(F観点へ移送)** | TLS 終端位置の SPECS_JA 明記は F-docs 観点で別途扱う。 |
| should-fix-008 | wontfix | **accepted-as-wontfix** | `poll_write` の blocking syscall は one-shot 前提で実観測なし。Round 3 で `forward_http_streaming` を再読しても、chunked 経路は `BufWriter` (cap 8 KiB) を経由するので「4 MiB ピース → BufWriter → 8 KiB 単位の `write(2)`」になり、1 ピース当たり最大 500 回の write syscall が同一ワーカで連続する。100 MiB なら 12,500 回。Round 2 で「one-shot/short」と評価したが、実測ベンチが無い以上「short」の根拠は弱い。観測ベース対応のままにするが、後述の should-fix-013 として doc コメントの誇張は引き下げたい。 |
| should-fix-009 | wontfix | **still-open** | must-fix-005 と同根。 |
| should-fix-010 | wontfix | **accepted-as-wontfix** | Round 2 で「OSS 公開前の品質強化フェーズで対応」と確定。 |
| nitpick-001 | fixed | **confirmed fixed** | `listen_port()` (`main.rs:17-22`) は維持。 |
| nitpick-002 | partially-fixed | **accepted-as-wontfix** | rationale コメント縮減は本質的振る舞いに無影響。 |
| nitpick-003 | unchanged | **accepted-as-wontfix** | `vsock_async` モジュール分離は本質的振る舞いに無影響。 |
| nitpick-004 | partially-fixed | **confirmed fixed** | `protocol.rs:1-40` の wire spec docstring が SoT になり、`proxy_fetcher.rs:5-14` のリード側 docstring は Spec §5.2 参照に変わっている。Round 2 の指摘箇所「Used when the TEE runs inside a Nitro Enclave: ... length-prefixed protocol carries the traffic over loopback.」は維持されているが、`protocol.rs` 側に詳細が集約されたため重複の重みは下がった。 |
| nitpick-005 | wontfix | **accepted-as-wontfix** | `STREAM_CHUNK_LIMIT` の命名は本質的振る舞いに無影響。 |
| nitpick-006 | wontfix | **accepted-as-wontfix** | `handle_tcp_connection` / `handle_vsock_connection` の二重実装は async/sync I/O プリミティブ差で正当化済み。 |
| must-fix-006 | fixed | **confirmed fixed but see must-fix-008** | `CHUNKED_TRUNCATED = u32::MAX` end-marker (`protocol.rs:53`) の導入と handler/fetcher 両端の対称処理 (`handler.rs:142-148`、`proxy_fetcher.rs:275-283`) で「proxy 打ち切りが TEE から見えない」silent failure は解消。ただし定数値が `CHUNKED_SENTINEL` と同じ `u32::MAX` で**文字通り同じビットパターン**であることが、別の混乱を招く（must-fix-008 参照）。 |

集計: **fixed 7 / wontfix-accepted 13 / still-open 2** = 22 件全件をカバー、Round 2 → Round 3 の退行は無し。

## 新規発見（Round 3）

### must-fix-007 非 GET / エラー経路は `response.bytes().await` で上限チェック前にメモリにロードされる

- 場所: `crates/proxy/src/handler.rs:159-191`
- 観察:
  ```rust
  // 非 GET、または status != 200 のとき
  let body_bytes = match response.bytes().await {
      Ok(b) => b.to_vec(),       // ← ここで body 全体を一気にメモリへ展開
      Err(e) => { ... }
  };
  if body_bytes.len() as u64 > MAX_RESPONSE_BYTES {
      // ← 上限超過の判定は **読んだ後**
      tracing::warn!(...);
      write_error(w, PROXY_ERROR_STATUS, &msg).await?;
      return shutdown_write(w).await;
  }
  ```
- 問題:
  - `reqwest` の `Response::bytes()` はデフォルトで body サイズ上限を持たない。攻撃者制御の上流（Solana RPC エンドポイントの偽装、または GET の非 200 応答）が 1 GiB の body を返した場合、proxy はそれを **メモリに全部読み切ってから** 「too large」を返す。
  - GET 200 経路 (`handler.rs:84-130`) では Content-Length 既知ならヘッダ段階で拒否、chunked ならストリーム読み中に `total > MAX_RESPONSE_BYTES` で打ち切るので、ストリーミング保護がかかっている。**非 GET / エラー経路だけは保護がない。**
  - `total_timeout = 120s` で時間的には bounded だが、100 Mbps の上流から 120s で受け取れるのは 1.5 GiB。proxy はメモリ 1 GiB を一瞬で確保しようとして OOM kill される可能性がある。Round 2 の must-fix-004 修正は「u32 オーバーフロー」を防いだが、「memory before bound」の問題は別物として残っている。
  - 攻撃シナリオ:
    1. クライアント（または攻撃者制御の TEE バイナリ）が 404 を返す URL に POST する。
    2. 攻撃者制御の上流が `Transfer-Encoding: chunked` で 1 GiB を返す。
    3. proxy は `response.bytes().await` で 1 GiB をバッファ。OOM 確率が高い。
    4. proxy OOM = AWS 上で再起動 → 隣接する TEE インスタンスがすべて fetch 失敗
- 修正案:
  - 短期: 非 GET / エラー経路でも Content-Length が `MAX_RESPONSE_BYTES` を超える時点で `response.bytes()` を呼ばずに拒否する。Content-Length 無しなら `response.bytes_stream()` をループで読みながら `total > MAX_RESPONSE_BYTES` で打ち切る（GET の chunked 経路と同じパターン）。
  - 中期: GET / 非 GET / エラーの 3 経路で body 読み出しヘルパを共通化する。`async fn read_bounded_body(response, max) -> Result<Vec<u8>, E>` を `handler.rs` 内に立て、3 経路から呼ぶ。
- 優先度根拠: must-fix-002 / must-fix-004 が塞いだはずの「攻撃者制御の length で OOM」が、非 GET 経路だけ未保護のまま残っている。同じ問題クラスなので must で再掲。

### must-fix-008 `CHUNKED_SENTINEL` と `CHUNKED_TRUNCATED` が同じ値 `u32::MAX` でワイヤ上で位置依存解釈

- 場所: `crates/proxy/src/protocol.rs:42-53`、`crates/tee/src/proxy_fetcher.rs:145-152`
- 観察:
  ```rust
  pub const CHUNKED_SENTINEL: u32 = u32::MAX;
  pub const CHUNKED_TRUNCATED: u32 = u32::MAX;
  ```
  TEE 側 (`proxy_fetcher.rs`) でも同じ重複定義。
- 問題:
  - **ワイヤ上では同じ 4 バイト (`FF FF FF FF`) が、出現位置によって意味が変わる**。
    - 「`status` 直後の 4 バイト」位置 → `CHUNKED_SENTINEL`（チャンクモード開始）
    - 「`chunk_len` 位置」 → `CHUNKED_TRUNCATED`（proxy 打ち切り）
  - 読み手 (`proxy_fetcher.rs::fetch`) は文脈で正しく分岐できているが、`read_chunked_body` のループ内で誤って `n == CHUNKED_SENTINEL` 判定を入れた場合、`u32::MAX` chunk_len として `body.resize(start + (u32::MAX as usize), 0)` を試みて OOM する。コードレビュー時に「同じ定数値を別文脈で再利用している」ことが見落とされやすい。
  - `read_chunked_body` 内では `n == 0` （正常終了）と `n == CHUNKED_TRUNCATED`（u32::MAX, エラー）の 2 つだけが特殊値で、それ以外を chunk_len として扱う。`CHUNKED_SENTINEL` が同じ値なので、もし将来「sentinel を chunk 境界に再出現させる」拡張（例: stream のチェックポイント）を入れたら、`read_chunked_body` は `CHUNKED_TRUNCATED` と区別できない。
  - 仕様書（`protocol.rs:18-40` docstring）にも「両者は文脈で区別する」とは書いていない。`CHUNKED_TRUNCATED` の docstring (`protocol.rs:48-52`) は「end-of-stream marker (in place of the normal `0u32`)」とだけ書いており、「同値の `CHUNKED_SENTINEL` と混同しないこと」「将来 `CHUNKED_TRUNCATED` の別値導入が必要になったら CHUNKED_SENTINEL を併用するな」など、設計上の罠への警告は無い。
- 修正案:
  - 短期: `CHUNKED_TRUNCATED` を `u32::MAX - 1` のような別値に変える。`u32::MAX - 1 = 4 GiB - 1` で、real chunk_len としてはあり得ない（`STREAM_CHUNK_LIMIT = 4 MiB`）ので衝突しない。doc コメントに「`CHUNKED_SENTINEL` と `CHUNKED_TRUNCATED` は別ビットパターンを持つ」と明示。
  - 中期: chunk_len の解釈空間を仕様化する。「`0` = clean EOF, `1..=STREAM_CHUNK_LIMIT` = real chunk, `u32::MAX - 1` = proxy truncation, `u32::MAX` = reserved (= status 後の sentinel と区別したい場合)」のような表を `protocol.rs` docstring に追加。
- 優先度根拠: 現状の実装は文脈で読み手が正しく分岐できているが、ワイヤフォーマットの根幹定数が衝突しているのは Rust の型システムでは捕捉できない。must-fix-006 の修正が連れてきた**設計上の地雷**で、将来の拡張時に silent regression を出すリスクがある。

### should-fix-011 accept 後の vsock/TCP ストリームに read/write timeout が設定されない（slow-read DoS）

- 場所: `crates/proxy/src/main.rs:41-66, 94-133`、`crates/proxy/src/handler.rs:219-249`
- 観察:
  ```rust
  // main.rs vsock accept
  match listener.accept() {
      Ok((s, peer)) => { ... tx.try_send(s) ... }
      ...
  }
  // ↑ accept 直後の VsockStream には set_read_timeout / set_write_timeout が無い
  ```
  TEE 側 (`proxy_fetcher.rs:104-114, 128-138`) では `PROXY_IO_TIMEOUT = 60s` を `connect/clone` 後に設定しているのに対し、proxy 側は **accept 直後のソケットに timeout を設定していない**。
- 問題:
  - 攻撃者（TEE と同一 EC2 ホスト上の悪意あるプロセス、または production では Enclave 内の compromised TEE バイナリ）が vsock 接続を開いて 1 バイト送信した後、無限に黙る。
  - vsock accept handler (`main.rs:106-113`) は `tokio::task::spawn_blocking` で `read_string_sync` を呼ぶ。`read_exact` は timeout 未設定なら無期限ブロック。
  - 1 接続 = 1 blocking thread の永続消費。tokio のデフォルト blocking thread pool は 512。512 個の slow-read で他のリクエストがすべて止まる。
  - vsock の `mpsc::channel` capacity 32 は accept レベルの backpressure だが、すでに blocking pool に積まれた 512 個には効かない。
  - TCP handler (`handler.rs:219-249`) でも同様: accepted `TcpStream` に timeout が設定されていない。dev/test モードでは攻撃対象ではないが、本番 vsock パスと挙動が乖離するのは混乱の元。
- 修正案:
  - vsock 側: `tx.try_send(s)` の前に `s.set_read_timeout(Some(Duration::from_secs(60)))` と `set_write_timeout` を入れる。`proxy_fetcher.rs` 側の 60s と揃える。
  - TCP 側: `handle_tcp_connection` 入り口で `stream.set_nodelay(true)` と共に accept timeout を tokio タイマー (`tokio::time::timeout`) で被せる。
- 優先度根拠: must-fix-002 が「length attack で OOM」を塞いだが、「accept だけして読まない」攻撃には未対応。同一クラスの DoS なので should-fix 上位。

### should-fix-012 `protocol.rs` の sync I/O 関数群が `#[cfg(all(target_os = "linux", feature = "vendor-aws"))]` で完全消滅し、テスト不可

- 場所: `crates/proxy/src/protocol.rs:106-131`
- 観察:
  ```rust
  #[cfg(all(target_os = "linux", feature = "vendor-aws"))]
  pub fn read_u32_sync(...) { ... }
  #[cfg(all(target_os = "linux", feature = "vendor-aws"))]
  pub fn read_string_sync(...) { ... }
  #[cfg(all(target_os = "linux", feature = "vendor-aws"))]
  pub fn read_bytes_sync(...) { ... }
  ```
- 問題:
  - これらの sync 関数は vsock パスでしか呼ばれず、Linux + vendor-aws feature の組合せでしかコンパイルされない。
  - CI で macOS / Windows / Linux-no-vendor-aws ビルドを通している場合、`read_*_sync` 群は **そもそもコンパイル対象に入らない** ため、length 検証 (`if len > max_len`) の単体テストが TCP パス (`read_*_async`) でしか実行されない。
  - 仕様上は両者で max_len 判定が対称的だが、**「sync 側にだけ length 検証が抜けている」regression を CI で捕捉できない**設計になっている。Round 2 の must-fix-002 の修正は両関数に対称に入っているので現時点では問題ないが、将来の修正で片方だけ漏れた場合、Linux + vendor-aws での integration test まで気付かない。
- 修正案:
  - `cfg` から `vendor-aws` feature を外し、`#[cfg(target_os = "linux")]` だけにする。vsock 自体は feature gate されたまま、protocol の sync helpers は Linux なら無条件にコンパイル対象に入れる。
  - もしくは sync helpers から `cfg` を完全に外し、unit test を sync/async 両方で書く。`std::io::Read` を実装する型なら何でも受けるので、`Cursor<Vec<u8>>` でテストできる。
  - `proxy_fetcher.rs::read_chunked_body` が `dyn Read` を取って sync で読んでいることを考えると、`protocol.rs` の sync helpers は本来 TEE 側からも参照したい（依存方向は `tee → proxy crate` を許すか別 crate に切り出すか別問題だが）。
- 優先度根拠: 現時点で実害は無いが、回帰テストの抜けを構造的に作っている。

### should-fix-013 `VsockWriter::poll_write` の doc コメント "fine here because connections are one-shot and short" は 100 MiB chunked GET 前提では誤り

- 場所: `crates/proxy/src/main.rs:142-145`
- 観察:
  ```rust
  /// Tokio `AsyncWrite` shim over the blocking `vsock::VsockStream`. Each
  /// `poll_write` blocks the worker thread for the duration of a single
  /// `write(2)` — fine here because connections are one-shot and short.
  ```
  Round 2 should-fix-008 でも指摘されたが、Round 2 処理ログでは「one-shot/short connection 前提で実観測なし」と wontfix。
- 問題:
  - `MAX_RESPONSE_BYTES = 100 MiB` を許す設計と「connections are one-shot and short」というコメントは矛盾している。100 MiB を `BufWriter` (デフォルト cap 8 KiB) 経由で流せば 12,500 回の `write(2)` syscall がワーカスレッド上で同期実行される。
  - 実観測なしの理由は OSS 公開前の負荷テストが無いから。コメント自体は「将来の問題には踏み込まない」という Round 2 wontfix 判定と整合するが、**コメントが「short」と言い切っているのは技術文書としての誠実さを欠く**。Round 2 should-fix-008 を wontfix にするなら、コメント側を「ワーカスレッドを最大 *timeout* 秒占有する。同時 chunked 接続数が多い場合は別途検討」のように書き換えるべき。
- 修正案:
  - コメントを「Connections can transfer up to `MAX_RESPONSE_BYTES` (100 MiB) over a single `BufWriter` (8 KiB cap), so a single `poll_write` is bounded but the **total** wall-clock time on the worker thread can reach several seconds. Acceptable as long as the tokio multi-thread runtime has enough workers (default = `available_parallelism()`).」のように書き換える。
  - もしくは `BufWriter` の cap を 1 MiB に上げて syscall 回数を 1/128 に減らす（メモリは 1 接続 +1 MiB なので budget 内）。
- 優先度根拠: 機能変更不要だが、技術文書としての正確性。Round 2 wontfix 判定との整合性。

### nitpick-007 `STREAM_CHUNK_LIMIT` (`handler.rs:13`) と `protocol.rs` の他定数が別ファイル

- 場所: `crates/proxy/src/handler.rs:13`、`crates/proxy/src/protocol.rs:42-62`
- 観察: wire format に直接出現する定数 (`CHUNKED_SENTINEL`, `CHUNKED_TRUNCATED`, `MAX_*`) は `protocol.rs` にあるが、chunked stream の 1 chunk 上限である `STREAM_CHUNK_LIMIT` は `handler.rs` にだけ存在。
- 問題:
  - wire format の docstring (`protocol.rs:27`) には「`[4B u32 BE: chunk_len][chunk bytes]`」とあるが、chunk_len の上限が `STREAM_CHUNK_LIMIT = 4 MiB` であることはここからは読めない。TEE 側 (`proxy_fetcher.rs::read_chunked_body`) は `max_body_bytes` でしか上限チェックしておらず、1 chunk あたりの上限は信用していない。
  - Round 2 nitpick-005 で「STREAM_CHUNK_LIMIT の命名・doc 拡充」が wontfix されたが、Round 3 では「**そもそも protocol.rs に移動して wire spec の一部として位置付けるべき**」だと考える。命名問題ではなくモジュール配置問題として再提起。
- 修正案:
  - `STREAM_CHUNK_LIMIT` を `protocol.rs` に移動し、`MAX_WIRE_CHUNK_BYTES` に rename。docstring に「proxy は chunk_len ≤ `MAX_WIRE_CHUNK_BYTES` を保証する。TEE は再チェックしてもよい」と書く。
  - TEE 側 `read_chunked_body` でも「`n > MAX_WIRE_CHUNK_BYTES` なら proxy 故障とみなしエラー」のチェックを足す。これは must-fix-008 で `CHUNKED_TRUNCATED` を別値にする際にあわせてやると整合的。

### nitpick-008 `unsupported_method_rejected` テストの status code が wire spec と食い違う

- 場所: `crates/proxy/src/main.rs:286-298`、`crates/proxy/src/handler.rs:59-64`
- 観察:
  ```rust
  // handler.rs
  other => {
      tracing::warn!(...);
      let msg = format!("Unsupported method: {other}").into_bytes();
      write_error(w, 400, &msg).await?;
      return shutdown_write(w).await;
  }
  ```
  ```rust
  // protocol.rs docstring
  //! Status `0` is reserved for proxy-internal errors (network failure,
  //! timeout, decode failure). HTTP status codes from the upstream pass
  //! through unchanged.
  ```
- 問題:
  - 「unsupported method」は upstream 由来ではなく **proxy 内部の決定** だが、proxy_internal_error_status = 0 ではなく HTTP 400 を返している。
  - TEE 側 (`proxy_fetcher.rs:192-204`) は `status == 0` のときだけ body を `FetchError::HttpError`（reason 文字列）として扱い、それ以外は `FetchError::HttpStatus`（数値）として扱う。よって unsupported method は TEE 側で「上流が 400 を返した」と誤解される。
  - 仕様上の attack surface は限定的だが、エラーの観測性（「proxy が拒否したのか上流が拒否したのか」の切り分け）が壊れる。Round 2 で must-fix-006 が「打ち切りを silent failure にしないため」CHUNKED_TRUNCATED を導入したのと同じ思想に立てば、ここも proxy 内部エラーは status 0 で返すのが正しい。
- 修正案:
  - `write_error(w, PROXY_ERROR_STATUS, &msg).await?;` に変更（`handler.rs:62`）。
  - テスト `unsupported_method_rejected` (`main.rs:286-298`) の `assert_eq!(status, 400)` を `assert_eq!(status, 0)` に変更し、`reason.contains("Unsupported method")` で確認する。
  - `protocol.rs` docstring の「Status `0` is reserved for proxy-internal errors」リストに「`Unsupported method`」を追記。

### nitpick-009 `proxy_fetcher.rs::CHUNKED_TRUNCATED` / `CHUNKED_SENTINEL` が proxy crate と独立定義で同期がコメント依存

- 場所: `crates/tee/src/proxy_fetcher.rs:145-152`
- 観察:
  ```rust
  /// Sentinel value in `body_len` that signals chunked-stream framing — must
  /// match `title_proxy::protocol::CHUNKED_SENTINEL`.
  const CHUNKED_SENTINEL: u32 = u32::MAX;
  ```
- 問題:
  - TEE crate が proxy crate を依存に持っていないため、定数が手動コピー。docstring で「must match」と書くだけで型システムによる強制は無い。
  - proxy crate 側で `CHUNKED_SENTINEL` の値を変えると、TEE 側で silent ABI break。must-fix-008 で `CHUNKED_TRUNCATED` を別値にした瞬間、TEE 側も同じ変更を入れないと chunked truncation を検出できなくなる。
  - Round 2 では nitpick-004 が「wire spec の doc 二重記述」を partially-fixed で済ませたが、**定数の二重定義はそれより重い問題**。
- 修正案:
  - `crates/proxy-protocol` のような shared crate を切り出し、proxy / TEE 両方から depend する。`protocol.rs` の `CHUNKED_*` 定数と `MAX_*` 定数、`read_*_async` / `read_*_sync` helpers をそこに移す。
  - もしくは TEE crate が `title-proxy` の `protocol` モジュールだけを `[features]` 経由で参照できるよう、proxy crate に `library` ターゲットを足す。
  - 短期では `tests/wire_constants_match.rs` のような integration test で `assert_eq!(title_proxy::protocol::CHUNKED_SENTINEL, title_tee::proxy_fetcher::CHUNKED_SENTINEL)` を強制（ただし可視性の問題で要 `pub`）。

## 全体所感

Round 2 で挙げた **must-fix-006 の `CHUNKED_TRUNCATED` 導入は技術的には正しい方向の修正**で、proxy 打ち切りを TEE が明示的にエラーとして surface できるようになった点は Round 1 → Round 2 → Round 3 の流れの中で最大の前進。一方で Round 3 で新規に拾った 8 件のうち、**重い 2 件 (must-fix-007 / must-fix-008) は Round 2 までの修正が漏らした攻撃面 (非 GET 経路の OOM) と設計上の地雷 (sentinel 値衝突)** を指摘している。

特に **must-fix-007（非 GET 経路の OOM）は Round 1 must-fix-002 の修正が「chunked GET 経路だけ」に偏った結果生まれた抜け穴**で、Round 1 → Round 2 ではこの経路の修正が「`body_bytes.len() as u64 > MAX_RESPONSE_BYTES` の事後チェック」で済まされた。事後チェックは u32 overflow は防げるが、メモリ確保攻撃は防げない。must-fix-002 / must-fix-004 の論理を非 GET 経路に一貫適用する必要がある。

**must-fix-008（`CHUNKED_SENTINEL` と `CHUNKED_TRUNCATED` が同値）は実害ゼロだが回帰リスク高**。現状のコードは文脈で正しく分岐できているが、wire format の根幹定数が衝突している事実は、コードレビュー時に見落とされやすく、将来の拡張（チェックポイント、再開、別の特殊マーカー）で silent regression を生む土壌になっている。Round 3 で潰しておきたい。

should-fix 群では **should-fix-011（accept 後のソケットに timeout 無し）が運用上の盲点**。TEE 側は 60s の I/O timeout を設定しているのに proxy 側は無設定で、slow-read DoS で 512 blocking thread を埋められる可能性がある。

Round 2 で wontfix された 13 件のうち、`--privileged` 系（must-fix-005 / should-fix-003 / should-fix-009）は依然として技術的赤旗が残っており、Round 3 の判定もそれを維持する。本リリースで対応しないなら、せめて `deploy/aws/README.md` か `OPERATIONS_JA.md` に「`--privileged` を外せない理由（seccomp 単独・cap-add 単独で何が壊れたか）」を 1 段落残すべき。コメントだけでは外部監査人の検証コストが高すぎる。

Round 3 を回した結果、proxy crate の wire protocol 部分は **CHUNKED_TRUNCATED の値衝突 (must-fix-008) と非 GET 経路の OOM (must-fix-007) を潰せば、Round 1 から続く `crates/proxy` の信頼境界モデルはほぼ完成する**。残りは仕様書側の節新設（F-docs 観点）と本番デプロイ側の seccomp 整備（K3-tee / 運用観点）で、proxy crate 自体は v0.1.2 リリース可能水準に達しつつある。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| must-fix-001/002/004/006 | confirmed fixed | Round 2 認定済み。Round 3 で再確認、退行なし。 |
| must-fix-003 | accepted-as-wontfix | `vsock 0.5` の API 制約で OwnedFd 分割は不可。Safety コメント 6 行で論証維持。 |
| must-fix-005 | still-open | `--privileged` justification が SPECS_JA / README に未記載。本リリースで対応しないなら OSS 公開前に検証ログ追加必須。 |
| should-fix-001/002/004 | confirmed fixed | Round 2 認定済み。 |
| should-fix-003/009 | still-open | must-fix-005 と同根。検証ログ追加を OSS 公開前にやり切る。 |
| should-fix-005/006/007/008/010 | accepted-as-wontfix | Round 2 判定維持。`should-fix-006/007` は F-docs 観点で SPECS_JA §5.2 への節新設が残る。 |
| nitpick-001/004 | confirmed fixed | nitpick-004 は `protocol.rs` 側に wire spec が集約され重複の重みが下がった。 |
| nitpick-002/003/005/006 | accepted-as-wontfix | Round 2 判定維持。 |
| must-fix-007 | fixed | `handler.rs:159-220` の非 GET / エラー経路を `bytes_stream()` ループに書き換え。Content-Length が既知なら事前チェック、不明なら逐次累積で `MAX_RESPONSE_BYTES` を強制。1 GiB body で OOM する経路を塞いだ。 |
| must-fix-008 | fixed | `CHUNKED_TRUNCATED = u32::MAX - 1` に分離 (proxy `protocol.rs:53`、TEE `proxy_fetcher.rs:152`)。`CHUNKED_SENTINEL = u32::MAX` と別ビットパターンに。`protocol.rs` docstring に chunk_len の解釈空間を表化。TEE 側 `read_chunked_body` に `chunk_len > MAX_WIRE_CHUNK_BYTES` の追加チェックも入れた。 |
| should-fix-011 | fixed | vsock accept 後に `set_read_timeout(60s)` / `set_write_timeout(60s)` を設定するコードを追加 (`main.rs:46-58`)。TEE 側 `PROXY_IO_TIMEOUT` と揃え、slow-read DoS で blocking thread pool が埋まる経路を物理的に塞いだ。失敗時は connection を drop。 |
| should-fix-012 | fixed | `protocol.rs` の sync I/O 関数群の cfg ガードを `#[cfg(all(target_os = "linux", feature = "vendor-aws"))]` から `#[cfg(target_os = "linux")]` のみに緩めた。これで Linux ビルドでは vendor-aws feature を問わず compile 対象に入り、CI で length 検証 regression を検出できるようになる。 |
| should-fix-013 | fixed | `VsockWriter` の doc コメントを書き換え。「one-shot and short」の誇張を削り、`MAX_RESPONSE_BYTES = 100 MiB` chunked GET の実 syscall 回数 (最大 12,500 回) と accept 時 timeout (60s) でブロックが bounded である事実を明示。 |
| nitpick-007 | fixed | `STREAM_CHUNK_LIMIT` を `handler.rs:13` から削除、`protocol.rs:62` に `MAX_WIRE_CHUNK_BYTES = 4 MiB` として移動。wire spec の一部として位置付け、TEE 側でも同名定数で参照。 |
| nitpick-008 | fixed | unsupported method の status を `400` から `PROXY_ERROR_STATUS = 0` に変更 (`handler.rs:63`)。proxy 内部での拒否を上流由来 400 と区別可能に。テスト `unsupported_method_rejected` も `assert_eq!(status, 0)` に更新。 |
| nitpick-009 | wontfix | TEE 側 `CHUNKED_*` 定数の proxy crate からの独立コピーは、shared crate 切り出しが workspace 構成変更で大きいため見送り。must-fix-008 で TEE と proxy 両方の `CHUNKED_TRUNCATED` を同時更新したことでコメントベースの同期は当面回せる。v0.1.3 で shared crate を整理する。 |
