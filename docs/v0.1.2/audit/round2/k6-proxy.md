# K6 — `crates/proxy` 縦深掘り監査（Round 2）

## 概要

- 担当範囲: Round 1 と同じ `crates/proxy/{Cargo.toml,src/main.rs,src/handler.rs,src/protocol.rs}`、突合先 `crates/tee/src/proxy_fetcher.rs`、`deploy/aws/scripts/run-stack.sh`、`deploy/aws/docker/title-proxy.Dockerfile`。
- 監査方針: Round 1 の 16 件（must:5 / should:7 / nitpick:4）が修正で消えたか、修正過程で新規問題が生まれていないかを 1 件ずつ突き合わせる。
- ファイル規模は Round 1 から拡大（`handler.rs` 160 → 224L、`main.rs` 271 → 345L、`protocol.rs` 85 → 126L）。`MAX_*` 定数群と `CHUNKED_SENTINEL` の導入、env-driven タイムアウト、`shutdown_write`、`try_send` backpressure ログなど、Round 1 must-fix の主要部分には実装が入っている。
- 件数サマリ: Round 1 由来の処理状況 = fixed:8 / partially-fixed:5 / unchanged:3、新規発見 = must-fix:1 / should-fix:3 / nitpick:2 = 計 6 件。

## 重大度別内訳（新規発見）

- must-fix: 1 件
- should-fix: 3 件
- nitpick: 2 件

## Round 1 指摘の処理状況

| ID | 重大度 | 概要 | 状況 | 備考 |
|---|---|---|---|---|
| must-fix-001 | must | chunked transfer で GET ストリーム切り捨て | **fixed** | `CHUNKED_SENTINEL = u32::MAX` をワイヤ形式に導入。`handler.rs:123-149`（chunked 経路）と `proxy_fetcher.rs:160`/`read_chunked_body` が対称に処理。`protocol.rs:18-31` で wire 仕様も明文化。`chunked_get_uses_sentinel` テストで end-to-end カバー。 |
| must-fix-002 | must | 攻撃者制御 length で proxy/enclave OOM | **fixed** | `MAX_METHOD_BYTES=16`, `MAX_URL_BYTES=8 KiB`, `MAX_REQUEST_BODY_BYTES=8 MiB`, `MAX_RESPONSE_BYTES=100 MiB` を `protocol.rs:43-50` に追加し、`read_bytes_{async,sync}` で事前判定。クライアント側 (`proxy_fetcher.rs:163-172`, `:264-272`) でも対称的にチェック。 |
| must-fix-003 | must | `VsockWriter` の `unsafe impl Send` の論拠が薄い | **partially-fixed** | (a) `try_clone().expect()` を取り除き、失敗時はログして接続を閉じるだけにした (`main.rs:84-90`)。(b) `unsafe impl Send` の Safety コメントを 1 行から 6 行に拡張 (`main.rs:159-165`)。一方で **(c) `vsock::VsockStream` を本物の `OwnedFd` ベースで read/write に分割する設計には踏み込んでいない**（依然 `try_clone` した別 fd + オリジナル fd の 2 本体制）。論証コメントの強化で実害は下がったが、Round 1 で提案した「読み書きの所有権分割」は未実装。回帰防止のための単体テストも無い。 |
| must-fix-004 | must | u32 オーバーフローで silent truncation | **fixed** | (a) Content-Length 既知パスは `MAX_RESPONSE_BYTES` 越えを 0 status + reason で即拒否 (`handler.rs:86-93`)。(b) chunked パスでも `total > MAX_RESPONSE_BYTES` で打ち切り (`handler.rs:131-140`)。(c) 非 GET / エラー経路もボディ長を上限チェック (`handler.rs:152-158`)。`len as u32` のキャストは 100 MiB 上限の保護下で安全。 |
| must-fix-005 | must | `--privileged` 過剰権限 | **unchanged** | `deploy/aws/scripts/run-stack.sh:49-57` は依然として `--privileged`。Round 1 では「seccomp プロファイル同梱で代替せよ」を提案したが、コミットされたのは「**`--privileged` is the only combination that works without shipping a custom seccomp.json**」という長尺コメントのみ。検証ログ・代替案検討の痕跡は無く、Round 1 と同じ位置に同じ赤旗が残る。 |
| should-fix-001 | should | vsock accept backpressure 静かにブロック | **fixed** | `blocking_send` → `try_send` に切り替え、容量超過時は `tracing::warn!(queued=32, "vsock accept backpressure; dropping incoming connection")` を出して落とす (`main.rs:36-48`)。 |
| should-fix-002 | should | アクセスログに duration/upstream 欠落 | **fixed** | `duration_ms`, `upstream_host` を全 info!/warn! に追加、エラーは `format!("{e:#}")` で source チェーン残す (`handler.rs:33-37, 66-71, 122, 148, 159`)。 |
| should-fix-003 | should | `--privileged` の seccomp 代替を試していない | **unchanged** | must-fix-005 と同根。 |
| should-fix-004 | should | 600s 単一タイムアウト | **fixed** | `PROXY_CONNECT_TIMEOUT_SECS` / `PROXY_REQUEST_TIMEOUT_SECS` env で上書き可、デフォルトは `connect=10s` / `total=120s` に短縮 (`handler.rs:10-20, 39-47`)。 |
| should-fix-005 | should | POST に固定 `Content-Type: application/json` 付与 | **partially-fixed** | 短期対応として `Content-Type` ヘッダの強制付与は削除済み (`handler.rs:54`)。中期対応の「wire プロトコルにヘッダフィールドを追加して任意の header pass-through を許す」拡張は未着手。Solana RPC は `reqwest` のデフォルト（`octet-stream`）でも通る前提だが、新規 should-fix-A（後述）として一応 JSON-RPC server 側との互換確認が必要。 |
| should-fix-006 | should | method allowlist 仕様未記載 | **partially-fixed** | `handler.rs:50-51` に `// Spec §5.2 — proxy only forwards GET ... POST ...` のコメントが入った。一方で `docs/v0.1.2/SPECS_JA.md` §5.2 を読んでも「proxy が GET/POST のみ通す」「TEE 外向き HTTP の attack surface 縮減」の節は見つからない（§5.2 = "TEE" の章で、proxy の wire spec / method allowlist は依然ノータッチ）。 |
| should-fix-007 | should | TLS 終端位置が仕様書に明示されていない | **unchanged** | `handler.rs:3-6` の module doc には「TLS terminated here; integrity comes from C2PA, not transport (Spec §5.2)」と書いてあるが、SPECS_JA §5.2 本文には TLS 終端が proxy 側 (= TEE 外) であることを示す節は無い。仕様⇔実装の trust-boundary 説明が依然非対称。 |
| nitpick-001 | nit | vsock CID/port のハードコード | **fixed** | `listen_port()` ヘルパが `PROXY_LISTEN_PORT` env を読む (`main.rs:13-20`)。 |
| nitpick-002 | nit | 過剰な rationale コメント | **partially-fixed** | `handler.rs` の module doc は 4 行に圧縮されたが、TLS 終端の rationale は依然ここに居座る（仕様側に移して should-fix-007 を片付けるのが本筋）。`main.rs` の `vsock_async` モジュール内コメントは 6 行に増えており（must-fix-003 の Safety 注釈）、トータルでは rationale 行数の縮減効果が薄い。 |
| nitpick-003 | nit | `vsock_async` インライン化 | **unchanged** | `main.rs:121-166` に 46 行インラインのまま。`crates/proxy/src/vsock_writer.rs` への分離は未実施。 |
| nitpick-004 | nit | wire 仕様の doc 二重記述 | **partially-fixed** | `protocol.rs:1-35` に正式な wire spec block が入り、ここが SoT になった。一方 `proxy_fetcher.rs:7-14` には依然「Used when the TEE runs inside a Nitro Enclave: ... length-prefixed protocol carries the traffic over loopback.」が独立に書かれており、`// Spec §5.2 — wire format` 参照 1 行に集約する Round 1 案は未実施。 |

集計: **fixed 8 / partially-fixed 5 / unchanged 3** = 16 件全件をカバー、退行は無し。

## 新規発見（Round 2）

### must-fix-006 chunked 上限超過時に TEE は「正常な短い 200 応答」と区別できない

- 場所: `crates/proxy/src/handler.rs:127-147`、対応する読み手 `crates/tee/src/proxy_fetcher.rs:160-179, 253-279`
- 観察:
  ```rust
  // handler.rs（chunked 経路）
  w.write_all(&status.to_be_bytes()).await?;            // 200 を既に送信済み
  w.write_all(&CHUNKED_SENTINEL.to_be_bytes()).await?;  // sentinel 送信済み
  ...
  if total > MAX_RESPONSE_BYTES {
      tracing::warn!(...);
      w.write_all(&0u32.to_be_bytes()).await?;          // ← end marker (= 正常終了と同じ)
      w.flush().await?;
      return shutdown_write(w).await;
  }
  ```
  クライアント側 (`read_chunked_body`) は `n == 0` を見て `Ok(body)` を返す。
- 問題: chunked パスで「100 MiB を超えたから打ち切った」という事実が TEE に伝わらない。TEE は `status = 200` + 100 MiB ちょうどの body を **正常に取得した GET レスポンス**として処理し、後段の C2PA 検証 (Merkle ルートが合わない) でようやくエラーになる。失敗原因が「コンテンツ改ざん」「ストレージ破損」「proxy 側打ち切り」のどれか切り分けできず、運用切り分けの土台が崩れる。Round 1 must-fix-001/002/004 の修正で chunked 経路を新設した結果生まれた **修正起因の新規バグ**である。
- 影響: 100 MiB ちょうどの C2PA メディアを真面目に取りに行った場合と、200 MiB のものを 100 MiB で打ち切られた場合が、TEE のログレベルでは同一に見える。攻撃者は「内容は正しいが巨大なメディア」を上流に置くだけで、TEE 側に「C2PA 検証失敗」しか出させない（実際は proxy で削られている）状況を作れる。
- 修正案:
  - 短期: 上限超過時は end marker 直前に `[CHUNKED_SENTINEL][reason_len][reason_bytes][0u32]` 形式の「打ち切りトレーラ」を 1 つ送り、`read_chunked_body` が sentinel を二度目に見たら次の 4 バイトを reason 長として読んで `FetchError::HttpError` に変換する。プロトコル拡張は前方互換に保てる。
  - 中期: そもそも GET 応答全体に「end status」フィールドを後置する (proxy → TEE の終端 4 バイトを `0 = ok` / 非 0 = proxy 内部理由) のが筋。`protocol.rs` doc とテスト (`chunked_get_uses_sentinel`) を同時更新する。
- 優先度根拠: must-fix-001 の「silent truncation」を消すための修正が、別形の silent truncation を作っている。同じ問題クラスなので must で再掲。

### should-fix-008 `VsockWriter::poll_write` が tokio reactor をブロックする

- 場所: `crates/proxy/src/main.rs:131-145`
- 観察:
  ```rust
  impl AsyncWrite for VsockWriter {
      fn poll_write(self: Pin<&mut Self>, _cx: &mut Context<'_>, buf: &[u8])
          -> Poll<std::io::Result<usize>>
      {
          Poll::Ready(self.get_mut().0.write(buf))  // ← 同期 write(2) を直接呼ぶ
      }
      ...
  }
  ```
  module doc には「Each `poll_write` blocks the worker thread for the duration of a single `write(2)` — fine here because connections are one-shot and short.」とある。
- 問題: tokio multi-thread runtime のワーカスレッド上で blocking syscall を発行している。`forward_http_streaming` は chunked GET 経路で 4 MiB 単位の write を loop で叩くため、1 接続が 25 回（100 MiB 想定）連続でワーカを掴む可能性。同時接続が多くなれば（read 側は `spawn_blocking` で別スレッド化されているのに対し）他 task が starve する。Round 1 では read 側だけ `spawn_blocking` 化されていた問題を見落としており、Round 2 で改めて指摘する。
- 修正案: (a) BufWriter の出力先を `tokio::io::DuplexStream` に向け、別 `spawn_blocking` ループが受け取って vsock に flush する、もしくは (b) 各 `poll_write` を `tokio::task::block_in_place` でラップする (multi-thread runtime 限定の救済策)。最終的には vsock crate の async サポートに移行するのが王道。少なくとも doc コメントの "fine here because connections are one-shot and short" は楽観的すぎる—100 MiB chunked 応答は短くない。

### should-fix-009 上限超過時の `--privileged` ガード未検証＋仕様にも明記なし

- 場所: `deploy/aws/scripts/run-stack.sh:49-57`、`docs/v0.1.2/SPECS_JA.md` §5.2 全域、`README.md`（deploy/aws）
- 観察: Round 1 should-fix-003 / must-fix-005 が「unchanged」のまま、コメントだけが厚くなった。SPECS_JA §5.2 にも信頼境界の文章（「proxy は untrusted-but-isolated」）は依然書かれていない。
- 問題: OSS 公開後、外部監査人が `deploy/aws/scripts/run-stack.sh` を読むだけで「`--privileged` で動かす理由 = seccomp 制約だけ」だと信じる根拠が、コメント以外にどこにも無い。Nitro 実機で代替試行をしたログ・コミットも残らない。代替方針 (`--security-opt seccomp=...` + `--cap-add NET_ADMIN`) を試した記録（成功でも失敗でも）を `deploy/aws/README.md` に残すか、SPECS_JA §5.2 の信頼境界節で「proxy は host 上で root 同等の権限で動く前提」を明記する必要がある。
- 修正案: 最低限 `deploy/aws/README.md` に「seccomp 単独・cap-add 単独で試した結果、AF_VSOCK の socket(2) で X が起きた」のような検証ログを 1 段落残す。理想は seccomp プロファイルを同梱して `--privileged` を外す。

### should-fix-010 chunked end-marker と「`Content-Length` 通り送られて来なかった」シナリオの TEE 動作が未テスト

- 場所: `crates/tee/src/proxy_fetcher.rs:253-279`（`read_chunked_body`）、`crates/proxy/src/main.rs:292-344`（`chunked_get_uses_sentinel`）
- 観察: proxy 側 `chunked_get_uses_sentinel` テストは「2 chunk + 0 marker = `hello world`」のハッピーパスのみ。TEE 側 `proxy_fetcher.rs` の単体テスト (`fetch_success_round_trips_protocol` 他) はいずれも非 sentinel 経路だけを叩いており、`read_chunked_body` を呼ぶケースがゼロ。さらに「Content-Length 通りに上流が body を送らなかった (`written < len`)」シナリオ（`handler.rs:118-120`）も TEE 側で再現テストが無い。
- 問題: must-fix-006 で指摘した「proxy が途中で打ち切ったときに TEE が気付かない」は、現状のテストでは絶対に拾えない。回帰テストとして、(a) `spawn_fake_proxy` を chunked mode に対応させ、(b) 「途中で 0 marker が早く来る」「chunk_len が後続 bytes と一致しない」「sentinel の後で connection が突然閉じる」の 3 ケースを TEE 側で検証する必要がある。
- 修正案: `tests/proxy_chunked_*.rs` を 3 本追加。proxy 側でも `chunked_truncated_by_budget` テストを 1 本足し、must-fix-006 の修正案（打ち切りトレーラ）まで含めて end-to-end で検証する。

### nitpick-005 `STREAM_CHUNK_LIMIT` の定数名が proxy → TEE 方向の上限であることを示していない

- 場所: `crates/proxy/src/handler.rs:13`
- 観察: `const STREAM_CHUNK_LIMIT: u32 = 4 * 1024 * 1024;` のみ。doc コメント無し。chunked 経路 (`handler.rs:141`) で `chunk.chunks(STREAM_CHUNK_LIMIT as usize)` として使われ、ワイヤ上の chunk 1 個の上限になっている。
- 修正案: `/// Maximum size of a single re-chunked frame in the chunked-stream wire format (proxy → TEE).` の doc を付け、`MAX_WIRE_CHUNK_BYTES` のようにリネーム。`protocol.rs` 側に移して `CHUNKED_SENTINEL` と並べるのも筋。

### nitpick-006 `handle_tcp_connection` と `handle_vsock_connection` のリクエスト読み取りが二重実装

- 場所: `crates/proxy/src/main.rs:80-119` (vsock) と `crates/proxy/src/handler.rs:193-223` (tcp)
- 観察: 両者ともに `(method, url, body) = (read_string, read_string, read_bytes)` の同じ 3 段読みを書き下している。async/sync の I/O プリミティブ差異だけが理由で関数本体が重複。
- 修正案: `protocol.rs` に `pub struct ProxyRequest { method, url, body }` と `impl ProxyRequest { pub async fn read_async(...) }` / `pub fn read_sync(...)` を生やし、`handle_*_connection` 側は `let req = ProxyRequest::read_*(...)?` 1 行に集約。`MAX_*` 定数の参照も内側に閉じる。Round 1 で見落としていた重複箇所。

## 全体所感

Round 1 で挙げた 5 件の must-fix のうち、技術本体に近い 4 件 (chunked truncation / u32 OOM / 4 GiB オーバーフロー / unsafe Send) は実装上は前進している。特に **`CHUNKED_SENTINEL` の導入と `MAX_*` 上限のシステマチックな適用** は、proxy/TEE 双方を矛盾なく書き換える必要があったところを破綻なく着地させており、ワイヤ仕様 (`protocol.rs:1-35`) も SoT として読みやすい。テストカバレッジも `chunked_get_uses_sentinel` が新設され最低限の動作証拠は付いた。

一方で残課題は明確で、(1) **`--privileged` 問題は実質ノータッチ**、(2) **`unsafe impl Send` は理論的にはまだ完全に証明されていない**、(3) **TLS 終端位置・method allowlist が SPECS_JA §5.2 に未記載**、という Round 1 で指摘した「コメントではなく仕様に書け」「実装ではなく構造で守れ」型の指摘群が、結果としてコメント追加で片付けられた格好になっている。Round 2 で新規に拾った must-fix-006（chunked 打ち切りが TEE から見えない）は、must-fix-001 の修正が連れてきた新しい silent failure であり、最終リリース前に必ず潰したい。

`crates/proxy` は v0.1.2 で唯一信頼境界の外に出るコンポーネントであり、ここでの「黙って失敗する」経路はそのまま「TEE 起点の検証ストーリーが説明できなくなる」ことを意味する。Round 3 を回す余裕があるなら、必ず (a) must-fix-006 のトレーラ導入、(b) `--privileged` 解消、(c) SPECS_JA §5.2 への wire spec / TLS 終端 / method allowlist の節新設、の 3 つを揃えて読みたい。
