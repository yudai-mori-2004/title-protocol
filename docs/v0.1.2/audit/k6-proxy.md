# K6 — `crates/proxy` 縦深掘り監査

## 概要

- 担当範囲: `crates/proxy/Cargo.toml`, `src/main.rs` (271L), `src/handler.rs` (160L), `src/protocol.rs` (85L)。境界の整合性を測るため `crates/tee/src/proxy_fetcher.rs` および `deploy/aws/scripts/run-stack.sh` / `deploy/aws/docker/title-proxy.Dockerfile` も参照した。
- 監査方針: 仕様 §5.2「TEE コンテンツ取得 (proxy-mediated transport)」と実装の往復で、(a) 攻撃者制御 length による DoS、(b) vsock+tokio ブリッジの正しさ、(c) ストリーミング転送のセマンティクス、(d) 運用面（権限・観測性・ハードコード）を 1 文単位で検証。
- 件数サマリ: must-fix 5, should-fix 7, nitpick 4 = 計 16 件。

## 重大度別内訳

- must-fix: 5 件
- should-fix: 7 件
- nitpick: 4 件

## 発見

### must-fix-001 chunked transfer-encoding で GET ストリームが切り捨てられる

- 場所: `crates/proxy/src/handler.rs:71-100`、突合先 `crates/tee/src/proxy_fetcher.rs:130-147`
- 観察:
  ```rust
  let content_length = response.content_length().unwrap_or(0);
  ...
  w.write_all(&(content_length as u32).to_be_bytes()).await?;
  let mut stream = response.bytes_stream();
  let mut written: u64 = 0;
  while let Some(chunk) = stream.next().await {
      let chunk = chunk.map_err(std::io::Error::other)?;
      w.write_all(&chunk).await?;
      written += chunk.len() as u64;
  }
  ```
  クライアント側 (`proxy_fetcher.rs:131`) は受信した `body_len` バイトちょうどを `read_exact` で読み戻す。
- 問題: 上流が `Transfer-Encoding: chunked` を返した場合 `Content-Length` ヘッダは無く、`content_length()` は `None` → `0`。プロキシは「長さ 0」のヘッダだけ送って大量本体を続けて書き込む。クライアントは 0 バイトしか読まず、本体は次回読み取りまでバッファ滞留 → C2PA 検証は空ボディで失敗するし、再利用キープアライブが無い設計（must-fix-005 参照）でも残バイトが次接続のフレームと食い違う恐れがある。仕様 §5.2「C2PA Merkle ハッシュで進めながら取得」の前提を満たさない。実体は data-loss 級バグ。
- 修正案: ストリーム長を事前に確定できないので、ワイヤ形式に「chunked モード」を導入する。例: `body_len` フィールドを `0xFFFF_FFFF` （sentinel）にした場合、以後は `[u32 chunk_len][bytes]...[u32 0]` の連結とする。`protocol.rs` の doc にも追記し、`ProxyContentFetcher::fetch` を `body_len == SENTINEL` で分岐させる。あるいは（最低限の応急処置として）chunked 応答を一旦バッファして既知長で返すモードに固定する旨を仕様と実装の両方に明示する。

### must-fix-002 攻撃者制御の length で proxy/enclave 双方が即時 OOM になる

- 場所: `crates/proxy/src/protocol.rs:43-46`, `:53-56`, `:73-77`, `:82-85`（および対称の `proxy_fetcher.rs:131-147`）
- 観察:
  ```rust
  let len = read_u32_async(r).await? as usize;
  let mut buf = vec![0u8; len];
  r.read_exact(&mut buf).await?;
  ```
- 問題: `u32` 最大 4 GiB を読み込みサイズ上限の検証なしに `vec![0u8; len]` で先割り当てする。Enclave の隣接コンテナ（信頼境界の内側ではある）でも、悪意ある／壊れたクライアントから 1 接続で 4 GiB の RSS スパイクを誘発でき、proxy プロセスが OOM-kill されると TEE 全体が停止する。仕様 §4 の「メモリ管理」原則とも矛盾。クライアント側は `max_body_bytes=100MiB` のキャップを持つが proxy 側にはガードが無い。
- 修正案: `read_string` / `read_bytes` に `max_len: usize` 引数を追加し、`len > max_len` なら `InvalidData` で即エラー。method は 16 B、URL は 8 KiB、リクエストボディは 8 MiB を上限に固定する（method/URL/POST body はいずれも C2PA バイナリではないため小さい）。レスポンス側 (proxy → TEE) もボディ上限 (例 100 MiB、`ProxyContentFetcher::DEFAULT_MAX_BODY_BYTES` と一致) を導入し、超過時は `status=0` + reason 文字列で返す。

### must-fix-003 `VsockWriter` の `Send` 手動実装は二重所有を許す

- 場所: `crates/proxy/src/main.rs:78-108, 145-147`
- 観察:
  ```rust
  let read_result = tokio::task::spawn_blocking({
      let mut s = stream.try_clone().expect("vsock try_clone");
      move || { ... read on cloned fd ... }
  }).await;
  ...
  let mut writer = tokio::io::BufWriter::new(vsock_async::VsockWriter(stream));
  ...
  unsafe impl Send for VsockWriter {}
  ```
  read 側はクローンした fd、write 側はオリジナル fd を `VsockWriter` に渡し、tokio のワーカスレッド上で書き込む。
- 問題:
  1. `try_clone().expect()` は失敗で proxy 全体がパニック（接続 1 本の異常で他接続まで道連れ）。
  2. 読み取りタスクが `.await` で suspend している間にもう片方の fd 経由で書き込みを開始する可能性は手順上は無いが、read コードパスが将来「ヘッダだけ読んでボディはストリーミング」に変わると即座に race になる。`unsafe impl Send` のコメントは「Write access is single-task」を約束するが、コード上の不変条件で担保されていない。
  3. `vsock::VsockStream` の `Send` 安全性は `vsock` crate のソースに依存する内部実装事項。0.5.x で OS スレッド間移送が壊れる変更があると silent UB。
- 修正案: `stream.split()` 相当の所有権分割を導入する。`std::os::fd::OwnedFd` で fd を取り出し、`from_raw_fd` で読み書き別 `VsockStream` を構築（vsock crate に同等 API が無ければ自作）。`try_clone` が失敗したら接続をその場で閉じてログするだけにし、`expect` は除去。`unsafe impl Send` を残す場合は「なぜ安全か」の論証コメントを 1 行ではなく `// Safety:` 形式で記述。

### must-fix-004 上流応答ボディが 4 GiB を超えると u32 オーバーフローで silent truncation

- 場所: `crates/proxy/src/handler.rs:82, 106`
- 観察:
  ```rust
  w.write_all(&(content_length as u32).to_be_bytes()).await?;          // GET 経路
  w.write_all(&(body_bytes.len() as u32).to_be_bytes()).await?;        // 非 GET / エラー経路
  ```
  `content_length: u64` と `body_bytes.len(): usize` を `as u32` で切り詰めている。
- 問題: 4 GiB 超のレスポンス（例: 高解像度動画 + sidecar や悪意ある巨大応答）が来ると、フレームヘッダは下位 32 bit のみ、続く実バイト列はその数値と一致しない。TEE 側はヘッダ分だけ読んで残り（GB 単位）がソケットに滞留、続く接続でも protocol desync を引き起こす可能性。
- 修正案: ボディ上限を proxy 側でも明示的に enforce（must-fix-002 と同根）し、超過時は `status=0` + 「response too large」 reason で返す。プロトコルは u32 のままで良いが、`content_length > MAX_BODY` を事前判定し、超えた場合はストリームを drop して error フレームを送信する。

### must-fix-005 1 接続 1 リクエストの仮定が wire 仕様に書かれているだけで実装は検出しない

- 場所: `crates/proxy/src/protocol.rs:10`「one request = one connection so we don't need a request id」/ `crates/proxy/src/handler.rs:132-160` (`handle_tcp_connection`)
- 観察: ハンドラはレスポンスを書き終わったあとソケットを閉じる動作を `.flush()` のみで終え、`shutdown()` を呼ばない。TEE 側 (`proxy_fetcher.rs`) は per-fetch で `TcpStream::connect_timeout` する設計だが、proxy は同一接続上に 2 リクエスト目が来てもブロックしない（読み終わったら return するだけ）。
- 問題:
  1. クライアントが誤って 2 リクエスト目を送ると、proxy は静かに無視（fd は drop されて RST）し、クライアントは `body read failed` で再試行ループに入る。仕様 §5.2 と実装の整合は守られているが、誤用が即検出されない。
  2. `shutdown(Write)` を送らないため、TLS 再ハンドシェイク後の half-close を期待する upstream に対し、TIME_WAIT 滞留が早期に解消されない。
- 修正案: (a) `protocol.rs` のドキュメントに「2 リクエスト目を送ったら未定義動作」を明記し、(b) `forward_http_streaming` 終端で `w.shutdown().await` を呼ぶ。keep-alive を将来導入するなら request_id 4 バイトをフレーム先頭に足す前方互換策を仕様にメモ。

### should-fix-001 vsock 受け入れチャネルが満杯になると静かにブロック

- 場所: `crates/proxy/src/main.rs:31-44`
- 観察:
  ```rust
  let (tx, mut rx) = tokio::sync::mpsc::channel::<vsock::VsockStream>(32);
  ...
  if tx.blocking_send(s).is_err() { ... }   // チャネル容量超過時は send が待機する
  ```
  `blocking_send` は容量超過時に**スレッドをブロックする**。タイムアウトもメトリクスも無い。
- 問題: TEE 側が処理に詰まりレスポンス受信が遅延すると、tokio ランタイムでの `handle_vsock_connection` 完了が遅れ、`rx.recv()` の消化速度が落ち、最終的に accept スレッドが詰まる。観測ログには何も出ない。
- 修正案: `try_send` に切り替え、容量超過なら `tracing::warn!(queued = 32, "vsock accept backpressure, dropping connection")` でドロップする。あるいは tokio に accept 自体を任せる方法を再評価（`vsock` crate の最近版に async サポートがあれば移行）。

### should-fix-002 アクセスログに必要な属性が欠落（観測性）

- 場所: `crates/proxy/src/handler.rs:74-80, 104, 155` / `main.rs:102`
- 観察: 現状の構造化ログは `method, url, status, body_len` のみ。経過時間、上流のホスト名（IP 解決後）、バイト数のヒストグラム、`X-Request-Id`、TLS 失敗の理由（reqwest の `source()`）は出ない。
- 問題: Nitro 実機検証 (タスク 15) で「proxy 越しの何かが遅い」を切り分けるには `duration_ms` と `upstream_host` が最小限必要。仕様にも観測要件が無く、運用時にここで詰まる。
- 修正案: `forward_http_streaming` 冒頭で `let start = std::time::Instant::now();` を取り、最終 `info!` に `duration_ms = start.elapsed().as_millis() as u64` と `upstream_host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_owned))` を含める。エラー経路では `err = ?e` ではなく `err = format!("{e:#}")` で `source` チェーンも残す。

### should-fix-003 `--privileged` 起動は vsock を使うためだけには過剰

- 場所: `deploy/aws/scripts/run-stack.sh:38-47`
- 観察:
  ```bash
  # --privileged is required because AF_VSOCK socket bind() is gated by
  # capability checks the default Docker seccomp profile blocks.
  sudo docker run -d --name title-proxy --network host --privileged \
    title-protocol-proxy:latest
  ```
- 問題: `--privileged` は seccomp/AppArmor/cgroup の全制限を解除し、`/dev/*` 全公開、`CAP_SYS_ADMIN` 付与まで含む。AF_VSOCK の `bind(2)` を通すために必要なのは実際には seccomp プロファイルの調整（`socket(AF_VSOCK, ...)` を allow）か、最小 cap (`--cap-add NET_BIND_SERVICE` 程度) で済むことが多い。仕様の信頼境界では「proxy はホスト常駐の untrusted-but-isolated」位置付けなので、特権付与は監査上の赤旗。
- 修正案: 1. seccomp プロファイル `deploy/aws/docker/proxy-seccomp.json` を同梱し、デフォルトに `+socket(AF_VSOCK,*,*)` を許す。2. `--privileged` を `--security-opt seccomp=...` + `--device /dev/vsock` + `--cap-add NET_ADMIN`（必要なら）に置き換える。3. 実機で動かして検証し、不要な cap は削る。これが現実的にできなければ、コメントに「seccomp 調整での代替を試したが Nitro AMI 上では X の理由で動かない」と具体的根拠を残す。

### should-fix-004 リクエストタイムアウト 600 秒は単一ロックダウン値で運用調整できない

- 場所: `crates/proxy/src/handler.rs:22`
- 観察: `const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);`
- 問題: 大容量 C2PA メディアを想定した値だが、(a) 接続 idle と全リクエスト所要時間が同一定数で扱われる（reqwest 0.12 の `timeout` はリクエスト全体）、(b) 環境変数で上書きできず、運用で詰まったときに再ビルドが必要、(c) 600 秒は DoS 加速器にもなる（攻撃者が slow upstream を仕掛けて proxy のスレッド/接続を 10 分間掴める）。
- 修正案: `PROXY_REQUEST_TIMEOUT_SECS` env で上書き可能にし、`connect_timeout(10s)` と `read_timeout(60s)` （reqwest 0.12 から個別設定可）を分離。デフォルトは connect 10s / total 120s に下げ、必要なら運用側で延長。

### should-fix-005 すべての POST に `Content-Type: application/json` を勝手に付与

- 場所: `crates/proxy/src/handler.rs:43-50`
- 観察:
  ```rust
  "POST" => client.post(url).header("Content-Type", "application/json").body(body.to_vec()).send().await,
  ```
- 問題: 仕様 §5.2 は「method, url, body の透過転送」を意図しており、Content-Type を proxy が固定するのは越権。Solana RPC が JSON 固定だから合っているだけで、将来 multipart や CBOR を送りたくなった瞬間に壊れる。クライアントは Content-Type を指定する手段が無い（プロトコルにヘッダフィールドが無い）。
- 修正案: 短期: `Content-Type` ヘッダを送らず、reqwest デフォルト（`application/octet-stream` 相当）に任せる。中期: wire プロトコルに `[u32 headers_len][headers]` を 4 番目のフィールドとして追加し、`name:value\n` 区切りで複数ヘッダを許可（Host/Content-Type を弾く allowlist 必須）。仕様 §5.2 にもこの拡張ポイントを書く。

### should-fix-006 method allowlist が GET/POST のみ — 仕様との整合が不明

- 場所: `crates/proxy/src/handler.rs:41-57`
- 観察: `match method { "GET" => ..., "POST" => ..., other => 400 }`。PUT/PATCH/DELETE/HEAD は不可。
- 問題: 仕様 §5.2 はコンテンツ取得 (GET) と Solana RPC への送信 (POST) を想定。だが、現状コードコメントにも仕様書にも「PUT/DELETE を意図的に除外した理由」が無い。OSS 公開後、外部利用者が「なぜ DELETE が無い？」を辿る手段が無い。
- 修正案: SPECS_JA §5.2 に「TEE は外向き HTTP として GET（取得）と POST（送信）のみ使う。proxy はこの 2 メソッドに限定して攻撃面を縮減する」を明記。`handler.rs` の `match` には 1 行だけ「// 仕様 §5.2: GET/POST only」を残し、`HEAD` を許すかどうかも検討（Range Request の前提検査に使うため将来必要になる可能性が高い）。

### should-fix-007 TLS 終端が proxy 側であることが仕様書に明示されていない

- 場所: 仕様 §5.2「コンテンツ取得の詳細」（`docs/v0.1.2/SPECS_JA.md:1081` 付近）/ 実装 `crates/proxy/src/handler.rs:14-18`
- 観察: 実装の doc コメントには「TLS termination happens here, not in the Enclave. ... C2PA 署名が出自を担保するので weakening にならない」と書いてあるが、SPECS_JA §5.2 には書かれていない。
- 問題: TLS 終端位置は信頼境界の中核論点。読み手（外部監査者・OSS 利用者）が仕様を読むだけでは「Enclave 内で TLS 終端しているはず」と誤解する可能性。実装コメントだけに依存すると、仕様⇔実装の乖離リスク（観点 F）が高い。
- 修正案: SPECS_JA §5.2 に節を追加: 「HTTPS の TLS は proxy（TEE 外）で終端する。proxy は upstream の平文を一度見るが、C2PA Merkle 署名がコンテンツ整合性を独立に担保するため、攻撃者が proxy を制御してもコンテンツ改ざんは検出される。proxy が観測できるのは URL とレスポンス本体に限られ、TEE 秘密鍵は到達しない。」を明記。実装コメントは仕様参照に縮める。

### nitpick-001 vsock CID/port のハードコード

- 場所: `crates/proxy/src/main.rs:19` `const LISTEN_PORT: u32 = 8000;`
- 観察: コンパイル時固定。`PROXY_LISTEN_PORT` 等の環境変数で変更できない。
- 修正案: `std::env::var("PROXY_LISTEN_PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(8000)` で起動時に解決。`tracing::info!` で実値をログする（現状はする）。Cargo.toml / Dockerfile に既定値ドキュメントを書く。

### nitpick-002 過剰な「やらなかった理由」の rationale コメント

- 場所: `crates/proxy/src/handler.rs:11-18`（streaming と TLS の二段落 doc コメント）
- 観察: モジュール doc に「なぜストリーミングするか」「なぜ TLS を proxy 終端するか」の rationale が本コードより長い。観点 A（コメント癖）にも該当。
- 修正案: ストリーミング理由はコード直上の 1 行コメントに圧縮。TLS 終端の論拠は仕様書側（should-fix-007）に移譲し、ここからは削除して `// 仕様 §5.2 参照` だけ残す。

### nitpick-003 `vsock_async::VsockWriter` モジュールが匿名すぎる

- 場所: `crates/proxy/src/main.rs:110-147`
- 観察: `main.rs` に 38 行のサブモジュールがインラインで埋め込まれている。テストも無く、`#[cfg(...)]` ゲートで隠れているため初見の読み手はスキップしがち。
- 修正案: `crates/proxy/src/vsock_writer.rs` に分離し、`mod vsock_writer;` で参照。`unsafe impl Send` の妥当性は単体ファイルで完結させた方が監査しやすい。

### nitpick-004 「one request = one connection」の前提が proxy/client 双方の doc コメントに重複

- 場所: `crates/proxy/src/protocol.rs:10` と `crates/tee/src/proxy_fetcher.rs:124` 周辺
- 観察: 同じ言明が 2 箇所、しかも微妙に表現が違う。
- 修正案: SPECS_JA §5.2 にワイヤ仕様の節を新設し、両 doc は `// 仕様 §5.2 — wire format` 参照に統一。

## 全体所感

`crates/proxy` は v0.1.2 の信頼境界の外に出る唯一の経路で、TEE の trust model 上は untrusted コンポーネントだが、ここが落ちれば TEE 全体が停止する単一障害点でもある。実装は短く読みやすく、Nitro 実機で動いている実績はあるが、(1) **chunked transfer の取り扱いミスで GET 応答が silent truncation する**点、(2) **u32 length を上限チェックなしで `vec![0u8; len]` する**点、(3) **`unsafe impl Send` の論拠がコメントどまり**な点、(4) **`--privileged` での運用が放置されている**点は OSS 公開前に必ず処理しておきたい。プロトコル拡張余地（ヘッダ転送・chunked sentinel）は SPECS_JA §5.2 と protocol.rs の doc を同期させて前方互換に書いておくと、将来の手戻りを避けられる。
