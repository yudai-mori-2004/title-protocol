# C. エラーハンドリング

## 概要

担当範囲: `crates/*/src/**/*.rs`, `programs/title-whitelist/src/**/*.rs`, `sp1-guests/**/*.rs`, `deploy/aws/**/*.sh`。

監査方針: 攻撃者制御可能な入力（HTTP body, Attestation Document バイト, wire payload, 外部ストレージ応答, env vars）に到達するすべての `.unwrap()` / `.expect()` / `panic!` / silent fallback を一文単位で精査。TEE 内 panic はリクエスト 1 件の失敗で済むため許容 (request handler 単位で隔離)、startup 失敗は fail-fast として許容、ただしどちらも意図が明示されているか確認。Silent fallback (`unwrap_or(0)`, `.ok()` 連鎖, `Result` を `Ok(())` に潰す) は罪が重いとして特に厳格に評価。

## 重大度別内訳

- must-fix: 11 件
- should-fix: 12 件
- nitpick: 7 件

## 発見

### must-fix-001 TEE 内 AES-GCM nonce が `OsRng` 由来 — Enclave 内エントロピーをバイパス

- 場所: `crates/crypto/src/sealed_channel.rs:41`, `crates/crypto/src/sealed_channel.rs:67`
- 観察:
  ```rust
  rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut nonce);
  ```
  `ResponseChannel::seal()` (TEE 側でレスポンス封印に使用) と `seal_for()` の nonce 生成が `rand::rngs::OsRng` を直接呼ぶ。
- 問題: `crates/tee/src/main.rs:86-89` のコメントで明確に「Using the host kernel's `OsRng` directly would defeat the point: enclave-internal entropy must be vendor-attestable, and Nitro's /dev/urandom has no guaranteed seed source other than NSM」と述べているにもかかわらず、レスポンス封印の nonce 生成だけが `OsRng` を使っている。Nitro Enclave の `/dev/urandom` が NSM から正しくシードされない場合、AES-GCM の nonce 衝突が原理上発生しうる。AES-GCM の nonce 再利用は同一鍵下でカタストロフィック（認証鍵漏洩 + 平文回復）。
- 修正案: `ResponseChannel` に `Arc<dyn TeeRuntime>` を持たせる、あるいは `seal()` が `&mut impl CryptoRng + RngCore` を受け取るように変更し、orchestrator から TEE-seeded RNG を注入する。`seal_for()` はクライアントサイドのみで使われるため OsRng のままで OK だが、ドキュメントでクライアント専用と明示すべき。

### must-fix-002 Proxy: Content-Length 欠落時に `body_len=0` を送信し client-side block

- 場所: `crates/proxy/src/handler.rs:73`, `crates/proxy/src/handler.rs:82`
- 観察:
  ```rust
  let content_length = response.content_length().unwrap_or(0);
  // ...
  w.write_all(&(content_length as u32).to_be_bytes()).await?;
  ```
  続けて L86-90 でストリームを書き出すが、length 既に 0 と申告済み。
- 問題: クライアント (`ProxyContentFetcher::fetch` `proxy_fetcher.rs:131-147`) は `body_len = read_u32(...)` を読んだ後、`vec![0u8; body_len]` を `read_exact` で埋める。`body_len=0` の場合、`read_exact(&mut [])` は即返るが proxy 側はまだストリームデータを書き続けており、TCP バッファに残ったデータが次のリクエストで誤読される（コネクションは 1 req per conn なので即 close されて穏当に終わるが、データ転送そのものは失敗）。
- 修正案: GET の場合は `content_length` を `unwrap_or(0)` せず、None なら一度メモリに集めて正しい長さを返す。あるいは wire protocol を可変長フレーミング（チャンク終端マーカー）に拡張する。

### must-fix-003 Proxy: 応答ボディ読み取り失敗時に空 body で `200` を返却

- 場所: `crates/proxy/src/handler.rs:103`
- 観察:
  ```rust
  let body_bytes = response.bytes().await.unwrap_or_default().to_vec();
  ```
- 問題: 上流からの body 読み取りが失敗（接続切断、TLS error 等）したとき、`unwrap_or_default()` で空 Vec を返し、`status` は元のままで TEE に転送される。TEE は「ステータス 200, body 空」を見て `EmptyContent` エラー扱いになるが、本来は proxy-internal error (status 0) を返すべき。
- 修正案:
  ```rust
  let body_bytes = match response.bytes().await {
      Ok(b) => b.to_vec(),
      Err(e) => {
          write_error(w, PROXY_ERROR_STATUS,
                      format!("Body read failed: {e}").as_bytes()).await?;
          return Ok(());
      }
  };
  ```

### must-fix-004 Proxy: streamed bytes と Content-Length の不一致を warn だけして続行

- 場所: `crates/proxy/src/handler.rs:93-99`
- 観察:
  ```rust
  if content_length > 0 && written != content_length {
      tracing::warn!(written, content_length,
          "streamed byte count differs from Content-Length");
  }
  Ok(())
  ```
- 問題: ヘッダで申告した `content_length` バイトと実際に書いた `written` が違うとき、warning ログを出すだけで `Ok(())` を返す。クライアントは `read_exact(content_length)` するため、`written < content_length` の場合は永遠に block するか接続切断で `UnexpectedEof`。`written > content_length` の場合は次の u32 読み取りでゴミを掴む。**signed C2PA コンテンツが部分的にしか TEE に届かない**ことになり、`compute_signature_hash` が静かに「無効」を返す可能性。
- 修正案: 不一致は `Err(std::io::Error::other(...))` で接続切断し、TEE 側に明確なエラーを伝える。

### must-fix-005 Gateway: TEE keys 取得失敗時に「鍵未変更」と誤判定 → 再起動を見逃す

- 場所: `crates/gateway/src/state.rs:103-110`
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
- 問題: TEE の health は OK なのに keys 取得が失敗 (HTTP 503, parse error 等) すると、`keys_changed = false` となり cache 再フレッシュがスキップされる。TEE が再起動して新鍵を生成した後で `keys` API が一瞬不安定だった場合、Gateway は古い public key を配り続け、クライアントの暗号化リクエストが（古い鍵で暗号化されているため）TEE 側で復号失敗を起こす。
- 修正案: keys 取得失敗は不確定状態として「再フレッシュを試みる」または `tee_available = false` にして次回ループで再判定する。少なくともログレベルは `warn!` 以上に。

### must-fix-006 NSM: `get_random` が 0 バイト応答で無限ループする可能性

- 場所: `crates/tee/src/vendor/aws.rs:63-82`
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
- 問題: NSM が `Response::GetRandom { random: vec![] }` を成功扱いで返した場合、`take = 0`, `extend_from_slice(&[])`, `out.len()` 不変 → 無限ループ。NSM の現実装で 0 バイト応答が起こりうるかは仕様外（API ドキュメントに保証なし）。たった一度のスレッド hang で鍵生成 (`tee_seeded_rng` 経由) が停止し、startup が止まる。
- 修正案: `random.is_empty()` で明示的に `Err(TeeError::RandomFailed("NSM returned empty random".into()))` を返す。

### must-fix-007 `run-stack.sh`: TEE 起動待ちタイムアウトでも gateway を起動する

- 場所: `deploy/aws/scripts/run-stack.sh:63-78`
- 観察:
  ```bash
  echo "==> Waiting for TEE HTTP to come up..."
  for i in {1..60}; do
    if curl -sf http://127.0.0.1:4000/health > /dev/null 2>&1; then
      echo "    TEE ready (${i}s)"
      break
    fi
    sleep 1
  done

  echo "==> Starting title-gateway (host network, port 3000)"
  ```
- 問題: 60 秒で TEE が ready にならなくてもループは終わり、そのまま gateway を起動する。`set -euo pipefail` が立っていてもループの「break しなかった」事実は failure として認識されない。production で TEE が壊れていても gateway が「unhealthy だが live」状態で公開ポートを開いてしまい、リクエストはすべて 503 を返すが運用者は気づきにくい。
- 修正案:
  ```bash
  ready=false
  for i in {1..60}; do
    if curl -sf http://127.0.0.1:4000/health > /dev/null 2>&1; then
      ready=true; break
    fi
    sleep 1
  done
  if [ "$ready" != true ]; then
    echo "TEE failed to come up within 60s" >&2
    sudo nitro-cli describe-enclaves
    exit 1
  fi
  ```

### must-fix-008 `run-stack.sh`: socat bridge 起動失敗が検知されない

- 場所: `deploy/aws/scripts/run-stack.sh:60-61`
- 観察:
  ```bash
  nohup sudo socat TCP-LISTEN:4000,reuseaddr,fork VSOCK-CONNECT:$ENCLAVE_CID:4000 \
    > "\$REMOTE_DIR/socat.log" 2>&1 &
  ```
- 問題: `nohup ... &` で起動した socat が即死 (port 4000 が既使用、vsock CID 不正等) しても、続く `curl /health` は loopback の TCP:4000 がリッスンしていないため 60 秒待って単に「TEE ready にならず」になる（must-fix-007 と合わさると気づかれない）。socat が起動したことを `sleep 0.5; kill -0 $!` などで確認すべき。
- 修正案: PID をキャプチャ → `sleep 1; kill -0 $SOCAT_PID || { echo "socat failed"; exit 1; }`。

### must-fix-009 c2pa-verify: 署名者の `issuer` 欠落を `"unknown"` で握りつぶし

- 場所: `crates/core/src/c2pa_verify.rs:213-219`
- 観察:
  ```rust
  let signer = manifest.signature_info().map(|sig_info| SignerInfo {
      issuer: sig_info.issuer.clone().unwrap_or_else(|| "unknown".to_string()),
      cert_serial: sig_info.cert_serial_number.clone(),
  });
  ```
- 問題: C2PA 署名の `issuer` が None のケースは、署名検証パイプライン上「正規の証明書が見つからない / parse できない」ことを意味するシリアスな信号だが、これを `"unknown"` という文字列に変換して `validation: "valid"` と一緒に Attestation Document へ封印してしまう。下流のアプリ層が `signer.issuer == "Google LLC"` のような文字列マッチで信頼判定すると、`"unknown"` を本来の発行元名と同じ扱いで通すバグを誘発する。
- 修正案: `issuer: Option<String>` のまま伝搬する（型シグネチャを変更）。あるいは `issuer` が None の場合は `signer` 全体を `None` にする。

### must-fix-010 `compute_global_timeout`: `data_size_bytes / MIN_TRANSFER_SPEED` が小さすぎる data_size_hint で除算切り捨て → 短すぎるタイムアウト

- 場所: `crates/tee/src/limits.rs:69`, `crates/tee/src/resource_pool.rs:103-110`
- 観察:
  ```rust
  let transfer_secs = data_size_bytes / MIN_TRANSFER_SPEED;
  ```
  `pool.try_admit(0)` (orchestrator.rs:172) は常に `data_size_hint = 0` を渡しているため、毎リクエストの global_timeout が常に `BASE_TIMEOUT = 60s` 固定。
- 問題: Spec §4.4 では「`min(最大時間, 基本時間 + データサイズ / 最低転送速度)`」と謳い、サイズ適応的なタイムアウトを規定している。実装は data_size_hint=0 のため適応的動作が**事実上死んでいる**。大容量ファイル (100MB) の場合 60 秒では fetch だけで足りずタイムアウトし、まともなクライアントが正当にリクエストを失敗させられる。仕様 vs 実装の整合性問題でもある。
- 修正案: `orchestrator.rs:172` で `request` から推定可能なサイズヒントを抽出して渡す（fragment 数 × MAX_FRAGMENT_SIZE の上限など）、または admission 時点では timeout = MAX に設定し、コンテンツ取得時に動的に縮める。

### must-fix-011 `programs/title-whitelist/src/lib.rs`: 公開値パース中の `unwrap()` 連発

- 場所: `programs/title-whitelist/src/lib.rs:332`, `:353`
- 観察:
  ```rust
  let id_len = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
  ```
  続く measurement_len も同様。
- 問題: 直前の `require!(data.len() >= offset + 4, ...)` で長さチェックされているため理論上 `.unwrap()` は到達不能。だが将来の編集で require! を消したり offset 計算をいじったりすると **Solana プログラムが panic** する。Solana プログラム panic はトランザクション失敗で済むが、panic メッセージはチェーン上に出るためデバッグも難しい。SP1 proof verification を経由した公開値という攻撃者間接制御パスにあり、防御深度として `unwrap` をやめるべき。
- 修正案:
  ```rust
  let id_len_bytes: [u8; 4] = data[offset..offset + 4]
      .try_into()
      .map_err(|_| error!(WhitelistError::InvalidPublicValues))?;
  let id_len = u32::from_le_bytes(id_len_bytes) as usize;
  ```

---

### should-fix-001 Gateway main: 環境変数 parse 失敗が silent fallback

- 場所: `crates/gateway/src/main.rs:50-63`
- 観察:
  ```rust
  let rate_limit_max: u32 = std::env::var("RATE_LIMIT_MAX")
      .ok()
      .and_then(|s| s.parse().ok())
      .unwrap_or(100);
  ```
- 問題: 運用者が `RATE_LIMIT_MAX=1000` のつもりで `RATE_LIMIT_MAX=1k` と typo すると、エラーも warn も出ずデフォルト 100 で起動する。本人は「設定したつもり」、実態は「設定されていない」というギャップを生む。`HEALTH_CHECK_INTERVAL_SECS`, `RATE_LIMIT_WINDOW_SECS`, `POOL_TOTAL_LIMIT`, `POOL_ADMISSION_LIMIT` (`crates/tee/src/main.rs:116-123`) も同パターン。
- 修正案: `.unwrap_or_else(|| { tracing::warn!(...); 100 })` で警告するか、set されていて parse できない場合は startup error にする (`return Err(...)`)。

### should-fix-002 Gateway: HttpTeeClient のレスポンス body 読み取り失敗を silent

- 場所: `crates/gateway/src/tee_client.rs:109`, `:134`
- 観察:
  ```rust
  let body = resp.text().await.unwrap_or_default();
  return Err(TeeClientError::HttpError { status, body });
  ```
- 問題: TEE が non-2xx を返したときの body 読み取りで `unwrap_or_default()`。本来 TEE のエラーメッセージが格納されているはずの body が、読み取り自体の失敗を区別できず空 string になる。デバッグ時に「TEE がエラー出してるが内容が空」と「TEE エラー出してるが body 読み取りに失敗」を混同する。
- 修正案: `.unwrap_or_else(|e| format!("<body read failed: {e}>"))`。

### should-fix-003 Sealed channel: 復号失敗時にエラー型情報が完全に消える

- 場所: `crates/tee/src/orchestrator.rs:270`, `:284`
- 観察:
  ```rust
  let opened = open_request(key_bundle, &fetched.content_bytes)
      .map_err(|e| OrchestratorError::DecryptionFailed(format!("{e:?}")))?;
  // ...
  let parsed = payload::parse_payload(&opened.plaintext)
      .map_err(|e| OrchestratorError::PayloadMetadataInvalid(format!("{e:?}")))?;
  ```
- 問題: `CryptoError` enum (variant: `UnsupportedSuite`, `InvalidKeyLength`, `DecryptError`, `HkdfError` 等) を `format!("{:?}")` で潰してしまうため、オーケストレータ呼び出し側がエラー種別で分岐できない。`#[from] CryptoError` で構造的に保持すべき。
- 修正案: `OrchestratorError::DecryptionFailed(#[from] CryptoError)` に。`Debug` 出力で文字列化するのは log 直前まで遅延。

### should-fix-004 Orchestrator: signature_hash 計算失敗の `to_string()` 連鎖

- 場所: `crates/tee/src/orchestrator.rs:200`, `:203`
- 観察:
  ```rust
  compute_signature_hash_from_manifest_data(manifest_data)
      .map_err(|e| OrchestratorError::SignatureHashFailed(e.to_string()))?
  ```
- 問題: `ProcessorError` を `to_string()` 化して `SignatureHashFailed(String)` に詰める。`ProcessorError::C2paVerificationFailed`, `ParseFailed`, `Internal` の区別が失われる。should-fix-003 と同根。
- 修正案: `OrchestratorError::SignatureHashFailed(#[from] ProcessorError)`。

### should-fix-005 ProcessorRegistry::execute: `ProcessorError` を String に潰す

- 場所: `crates/core/src/processor.rs:135`
- 観察:
  ```rust
  Err(e) => ProcessorOutput::error(e.to_string()),
  ```
- 問題: 各 processor が返す構造化 `ProcessorError` を文字列化して `ProcessorOutput::error(String)` に。エラー型が変化したときレスポンス JSON のフォーマットが silently に変わる。API 安定性のためにも `ProcessorOutput` に error_kind を別フィールドで持たせるべき。
- 修正案: `ProcessorOutput::error` を `(kind: &str, message: String)` に拡張、もしくは `error_kind` enum を JSON に出す。

### should-fix-006 Sidecar manifest 経由の `compute_signature_hash_from_manifest_data` がコンテンツとマニフェストのバインドを検証していない

- 場所: `crates/tee/src/orchestrator.rs:198-204`
- 観察: sidecar 入力の場合、`manifest_data` から signature_hash を計算し、`content_bytes` をそのまま processor へ渡すが、マニフェスト内の hard binding (content hash) と content_bytes のハッシュが一致するかは確認していない。
- 問題: 攻撃者が「manifest A + content B」を提示すると、signature_hash は manifest A のもので、processor 出力は content B から抽出されたもの、という不整合が許容される。spec §0.1 が定める「ハードバインディング」の意義を実装側で履行していない。これはエラーハンドリングというより設計欠落だが、検証スキップというパスがエラーで止まっていない点で本観点に該当。
- 修正案: c2pa-verify が内部で hard binding を検査するか確認し、しない場合は sidecar 処理時に明示的に `c2pa::Reader::with_stream_and_manifest_data` 経由で content と manifest の bind を強制し、不一致なら `SignatureHashMismatch` 相当のエラーを返す。

### should-fix-007 `extension.rs`: `attestation` の Base64 デコード失敗を `AttestationInvalid` で握る

- 場所: `crates/solana/src/extension.rs:139-141`
- 観察:
  ```rust
  let attestation_bytes = base64::engine::general_purpose::STANDARD
      .decode(&response.attestation)
      .map_err(|e| ExtensionError::AttestationInvalid(format!("Base64 decode: {}", e)))?;
  ```
- 問題: Base64 デコード失敗は仕様逸脱 (`attestation` フィールドは spec §2.3 で Base64 と規定) でクライアント側エラーだが、`AttestationInvalid` という「署名検証失敗」と同じカテゴリに丸められる。クライアントは「自分の入力が悪い」のか「TEE 内部の attestation が壊れている」のか区別できない。
- 修正案: 新 variant `ExtensionError::MalformedAttestation(String)` を追加し、Base64 / parse 段階のエラーはそちらへ。

### should-fix-008 RateLimiter: poisoned Mutex で **すべての後続リクエストが panic**

- 場所: `crates/gateway/src/rate_limit.rs:62`
- 観察:
  ```rust
  let mut buckets = self.buckets.lock().unwrap();
  ```
- 問題: 何らかの理由 (panic from inside) で `Mutex` が poison すると、以後の全リクエストで `.unwrap()` が panic。axum のハンドラ panic はリクエストを 500 にするだけだが、本来 rate limit を bypass すべきでないので少なくとも `unwrap_or_else` で poison から復旧するか、`parking_lot::Mutex` のような poison しない Mutex を使うべき。
- 修正案: `parking_lot::Mutex` に切替、もしくは `match self.buckets.lock() { Ok(g) => g, Err(p) => p.into_inner() }`。

### should-fix-009 proxy_fetcher: socket timeout 設定失敗が silent

- 場所: `crates/tee/src/proxy_fetcher.rs:103-104`
- 観察:
  ```rust
  stream.set_read_timeout(Some(PROXY_IO_TIMEOUT)).ok();
  stream.set_write_timeout(Some(PROXY_IO_TIMEOUT)).ok();
  ```
- 問題: timeout 設定の失敗を `.ok()` で握りつぶす。プラットフォーム依存で失敗する可能性は低いが、失敗した場合 60 秒タイムアウトが効かず、悪意あるストレージサーバが永遠にコネクションを掴むスローロリス攻撃を許す。
- 修正案: `set_*_timeout` の戻り値を `FetchError::HttpError` に変換して返す。設定できないプラットフォームなら起動時に検出して fail fast。

### should-fix-010 JUMBF parser: `desc_header.size - HEADER_SIZE` が underflow しうる

- 場所: `crates/core/src/jumbf.rs:170`, `:185`, `:230`, `:245`, `:286`
- 観察:
  ```rust
  let _top_desc = read_desc_info(&mut reader, desc_header.size - HEADER_SIZE)?;
  ```
- 問題: `desc_header.size` は攻撃者が制御可能な JUMBF ファイル由来。`size < HEADER_SIZE (=8)` だと debug ビルドで panic、release で wrap して巨大な `content_size` になり、その後 `read_exact` で OOM 級の vec を要求しうる。`MAX_SIGNATURE_SIZE = 16 MiB` のチェックは CBOR box にしかない。
- 修正案:
  ```rust
  let content_size = desc_header.size.checked_sub(HEADER_SIZE).ok_or_else(|| {
      ProcessorError::C2paVerificationFailed(
          format!("JUMBF desc box size {} < header size", desc_header.size))
  })?;
  ```
  同様に `child_start + child_header.size` の seek 計算 (`:193`, `:259`, `:297`, `:337`) は `checked_add` を使う。

### should-fix-011 JUMBF parser: read_so_far の型キャスト優先順位がコードレビューで読みづらく fragile

- 場所: `crates/core/src/jumbf.rs:133-134`
- 観察:
  ```rust
  let read_so_far =
      16 + 1 + if label.is_empty() { 0 } else { label.len() + 1 } as u64;
  ```
- 問題: Rust の演算子優先順位上 `as u64` は else 枝にのみ適用される。型推論が辻褄を合わせて動くが、人間が「`if-else` 全体を u64 にキャスト」と誤読しやすく、将来の編集で実バグを誘発する。
- 修正案: 括弧で意図を明示:
  ```rust
  let read_so_far: u64 = 16 + 1 + if label.is_empty() {
      0u64
  } else {
      (label.len() + 1) as u64
  };
  ```

### should-fix-012 build_payload: メタデータ serialize の `expect("cannot fail")`

- 場所: `crates/crypto/src/payload.rs:21`
- 観察:
  ```rust
  let meta_json = serde_json::to_vec(metadata).expect("metadata serialization cannot fail");
  ```
- 問題: 現在の `EncryptedPayloadMetadata` は単一 String フィールドで失敗しないが、将来 fields を追加した時に static guarantee が崩れる。エラー型として `CryptoError::SerializationFailed` を返すほうが防御深度として正しい。クライアント呼び出しなので panic でもプロセス停止以上の影響はないが、library として fail-loud は避けるべき。
- 修正案: 戻り値を `Result<Vec<u8>, CryptoError>` に変更し、`?` で伝搬。

---

### nitpick-001 `expect` メッセージが状況を説明していない

- 場所: `crates/tee/src/main.rs:139`, `crates/tee/src/server.rs:324`, `crates/gateway/src/tee_client.rs:91`
- 観察: `.expect("Failed to build HTTP client")`, `.expect("KeyBundle gen")` 等。
- 問題: panic 時にメッセージから「何の操作が、なぜ失敗したのか」が分かりにくい。
- 修正案: `.expect("HTTP client build failed at TEE startup; required for content fetching")` のように context を含める。テストコードはこの限りではない（test 名で文脈分かる）。

### nitpick-002 `tee-entrypoint.sh` の `ip link` 失敗 silent

- 場所: `deploy/aws/docker/tee-entrypoint.sh:18`
- 観察:
  ```sh
  ip link set lo up 2>/dev/null || true
  ```
- 問題: コメントで「The slim debian runtime doesn't enable loopback by default」と説明はあるが、エラー出力を破棄しているため別の理由（コマンドが無い等）で失敗してもそのまま socat に進む。
- 修正案: `2>/dev/null` を外し、次の socat 失敗ログと合わせて読めるようにする。

### nitpick-003 `extract_active_manifest_signature`: ProcessorError 文字列が長すぎる

- 場所: `crates/core/src/c2pa_verify.rs:162-163`
- 観察: `format!("C2PA Reader construction failed: {e}")` を 2 か所で同一文言生成。
- 修正案: ヘルパー関数 `c2pa_err(prefix, err)` を抽出。

### nitpick-004 `parse_pubkey` / `parse_hash` のエラー variant 名が紛らわしい

- 場所: `crates/solana/src/extension.rs:104-112`
- 観察:
  ```rust
  s.parse::<Pubkey>()
      .map_err(|e| ExtensionError::Base58Failed(format!("invalid pubkey '{}': {}", s, e)))
  ```
- 問題: `Base58Failed` は「base58 デコード失敗」を示唆するが、`Hash::parse` も同じ variant で返している。`Pubkey` parse は base58 + 長さチェックなので別 variant が適切。
- 修正案: `InvalidPubkey(String)`, `InvalidHash(String)` の 2 つに分離。

### nitpick-005 `whitelist.rs`: `whitelist_program_id` で `.unwrap()`

- 場所: `crates/solana/src/whitelist.rs:78`
- 観察:
  ```rust
  pub fn whitelist_program_id() -> Pubkey {
      Pubkey::from_str("43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs").unwrap()
  }
  ```
- 問題: 静的定数の parse で `.unwrap()` を使っている。文字列リテラルなので失敗はあり得ないが、PDA 派生のたびに parse + unwrap が呼ばれるパフォーマンスの無駄もある。
- 修正案: `expect("hard-coded program ID must parse")` + `OnceLock<Pubkey>` でキャッシュ。`crates/solana/src/cnft.rs:38` の `spl_account_compression_v2_id` も同様。

### nitpick-006 `attestation/lib.rs` の `MockAttestationVerifier::PREFIX` が変更されると静かに認証失敗

- 場所: `crates/attestation/src/lib.rs:131-134`
- 観察:
  ```rust
  let user_data = doc_bytes
      .strip_prefix(Self::PREFIX)
      .ok_or_else(|| AttestationError::ParseFailed("missing mock prefix".into()))?
      .to_vec();
  ```
- 問題: エラーメッセージが具体性に欠ける。`"missing mock-attestation: prefix"` のように prefix 内容を含めれば cfg`feature="mock"` での誤用が即わかる。
- 修正案: メッセージに `Self::PREFIX` の中身を埋める。

### nitpick-007 deploy scripts: stop-stack.sh と run-stack.sh で同じ cleanup ロジックの重複

- 場所: `deploy/aws/scripts/run-stack.sh:30-35`, `deploy/aws/scripts/stop-stack.sh:16-20`
- 観察: cleanup の 3 行が両方に存在し、片方を更新したらもう一方を忘れる典型構造。
- 修正案: `deploy/aws/scripts/_stop-stack-inner.sh` を抽出して両方から呼ぶ。エラーハンドリング観点としては `|| true` の連鎖を一箇所で定義することで「どこを silent にすべきか」の判断が一元化される。

## 全体所感

エラー型 (`OrchestratorError`, `FetchError`, `CryptoError`, `ExtensionError` 等) の thiserror 設計は概ね thoughtful で、`#[from]` の活用も適切。一方、**エラーをまたぐレイヤー境界で `to_string()` / `format!("{:?}")` を介して構造を潰している**箇所が散見され (`SignatureHashFailed(String)`, `DecryptionFailed(String)` 等)、せっかくの型情報が呼び出し側で取り出せない。これは must-fix ではないが、ライブラリとしての成熟度を下げている。

最も罪が重いのは **proxy crate のレスポンス転送 (`handler.rs`)** で、`unwrap_or(0)` / `unwrap_or_default()` のチェーンによって「上流の障害が TEE には silent empty response として届く」という silent failure チェーンが完成している。Nitro 実機での疎通検証 (タスク 15) が通っているのは正常系のみで、TEE-proxy 間のエッジケースは未検証の疑い。タスク J (実機検証) で proxy エラーパスを叩く value がある。

セキュリティ面では must-fix-001 (TEE 内 OsRng で AES-GCM nonce 生成) が最も深刻。Nitro Enclave kernel が `/dev/urandom` を NSM 起源でシードする保証は AWS ドキュメント上明確ではなく、nonce reuse → 鍵漏洩 のリスクを抱えている。`main.rs` で他の鍵生成は丁寧に NSM 経由にしているのに、レスポンス封印 nonce だけ抜けているのは見落としと見られる。
