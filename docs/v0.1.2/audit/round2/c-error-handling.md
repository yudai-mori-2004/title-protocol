# C. エラーハンドリング (Round 2)

## 概要

担当範囲: `crates/*/src/**/*.rs`, `programs/title-whitelist/src/**/*.rs`, `sp1-guests/**/*.rs`, `deploy/aws/**/*.sh`。

Round 1 で挙げた 30 件（must:11 / should:12 / nitpick:7）の処理状況確認と、修正中に混入した新規問題の洗い出し。Round 2 ではコードに変更が入ったファイル（特に `crates/proxy/src/handler.rs`、`crates/tee/src/limits.rs`、`crates/tee/src/proxy_fetcher.rs`、`crates/gateway/src/rate_limit.rs`、`crates/solana/src/{whitelist,cnft}.rs` 等）を中心に精査した。

## Round 1 指摘の処理状況

### 件数集計

| カテゴリ | fixed | partially-fixed | unchanged | regressed |
|---|---|---|---|---|
| must-fix (11) | 4 | 1 | 6 | 0 |
| should-fix (12) | 3 | 0 | 9 | 0 |
| nitpick (7) | 2 | 0 | 5 | 0 |
| **計 (30)** | **9** | **1** | **20** | **0** |

### must-fix 個別状況

| # | 要旨 | Round 2 status | 備考 |
|---|---|---|---|
| 001 | `ResponseChannel::seal()` / `seal_for()` の nonce が `OsRng` 直接 | **unchanged** | `crates/crypto/src/sealed_channel.rs:42`, `:71` 改変なし。TEE 側 `tee_seeded_rng` 経由で KeyBundle 生成は丁寧にやっているのに、レスポンス封印 nonce だけが host OS RNG のまま。Nitro `/dev/urandom` の seed 起源依存問題は未解決。 |
| 002 | Proxy GET の `content_length.unwrap_or(0)` で client-side block | **fixed** | `crates/proxy/src/handler.rs:81-149` を「known-length」と「chunked sentinel (CHUNKED_SENTINEL = u32::MAX)」に分岐するよう書き換え。`title_proxy::protocol::CHUNKED_SENTINEL` と `ProxyContentFetcher::read_chunked_body` も追加実装され、フレーミングが整合している。 |
| 003 | non-200/POST 応答の `response.bytes().await.unwrap_or_default()` | **unchanged** | `crates/proxy/src/handler.rs:152` まだ `.unwrap_or_default()`。上流の body 読み取りエラーが「空 body + 元のステータスコード」として TEE に届く silent failure チェーンは未解消。修正案は Round 1 と同じ（`match { Ok(b) => ..., Err(e) => write_error(...) }`）。 |
| 004 | streamed bytes と Content-Length 不一致を warn のみ | **partially-fixed** | `written > len` 側は `crates/proxy/src/handler.rs:106-114` で truncate & break するようになった。一方 `written < len` 側 (L118-120) は依然 `tracing::warn!` のみで `Ok(())` 復帰。クライアント (`ProxyContentFetcher::fetch`) は `len` バイトを `read_exact` で待つため、コネクション close により `UnexpectedEof` として表面化はする（must-fix-002 同様、1-req-per-conn に救われた形）。explicit error frame (`write_error(w, PROXY_ERROR_STATUS, ...)`) を送るほうが TEE 側の診断が明確になる。 |
| 005 | Gateway の `keys()` 失敗を `keys_changed = false` で握る | **fixed** | `crates/gateway/src/state.rs:114-123` で `Err(e) => { tracing::warn!(...); true }` に変更。doc comment も「fail-safe is to re-fetch」と明記。 |
| 006 | NSM `GetRandom` が 0 バイト返したとき無限ループ | **fixed** | `crates/tee/src/vendor/aws.rs:71-75` で `random.is_empty()` を `TeeError::RandomFailed` として返すよう修正。 |
| 007 | `run-stack.sh` の TEE 起動待ちがタイムアウト失敗を検知しない | **unchanged** | `deploy/aws/scripts/run-stack.sh:81-88` 改変なし。`ready=false` フラグなし、break しなくても `for` ループが普通に終わり gateway 起動に進む。production で TEE 起動失敗が「health 503 を返す gateway」として 60s 後に live になり、運用者が気づきにくい。 |
| 008 | `run-stack.sh` の socat 起動失敗を検知しない | **unchanged** | `deploy/aws/scripts/run-stack.sh:78-79` 改変なし。`nohup socat ... &` の PID を捕捉せず `kill -0` チェックなし。must-fix-007 と組み合わさり「socat 即死 → port 4000 リッスンなし → 60s 待ち → 単に TEE not ready」という診断困難パスが残る。 |
| 009 | `signer.issuer` の `unwrap_or_else("unknown")` | **unchanged** | `crates/core/src/c2pa_verify.rs:252-258` 改変なし。`issuer` が None のシリアスシグナルが `"unknown"` 文字列として `validation: "valid"` とともに Attestation Document に封入される構造的バグは残存。型シグネチャを `Option<String>` のまま伝搬する修正未着手。 |
| 010 | `compute_global_timeout` の data_size_hint=0 固定問題 | **fixed** | `crates/tee/src/limits.rs:58-65` で `data_size_hint: Option<u64>` に変更、`None` で `MAX_GLOBAL_TIMEOUT` を返すよう修正。`crates/tee/src/orchestrator.rs:184-190` も `Fragmented` で `fragment_urls.len() × MAX_FRAGMENT_SIZE` を渡す、`Single`/`Sidecar` は `None`(=MAX) を渡すよう適応。仕様 §4.4 のサイズ適応的タイムアウトが意味のある動作になった。 |
| 011 | `programs/title-whitelist/src/lib.rs` の公開値パース `unwrap()` | **unchanged** | `programs/title-whitelist/src/lib.rs:347`, `:368` 改変なし。`require!` で長さチェックされているため到達不能なのは事実だが、防御深度として残るリスクは未解消。修正案は Round 1 通り（`.try_into().map_err(...)`）。 |

### should-fix 個別状況

| # | 要旨 | Round 2 status | 備考 |
|---|---|---|---|
| 001 | Gateway main の env var parse silent fallback | **unchanged** | `crates/gateway/src/main.rs:50-63` と `crates/tee/src/main.rs:141-148` 改変なし。typo 時 silent デフォルト fallback パスが温存。 |
| 002 | `HttpTeeClient` の body 読み取り `.unwrap_or_default()` | **unchanged** | `crates/gateway/src/tee_client.rs:125`, `:150`, `:186` 改変なし。さらに **新規追加** で `process()` ハンドラ内 L186 にも同パターン (`resp.text().await.unwrap_or_default()`) が登場。本観点では既存 unchanged として計上。 |
| 003 | 復号失敗時の `format!("{e:?}")` でエラー型情報喪失 | **unchanged** | `crates/tee/src/orchestrator.rs:290`, `:304` 改変なし。`OrchestratorError::DecryptionFailed(String)` のまま。 |
| 004 | `signature_hash` 計算失敗の `to_string()` | **unchanged** | `crates/tee/src/orchestrator.rs:220`, `:223` 改変なし。`OrchestratorError::SignatureHashFailed(String)` のまま。 |
| 005 | `ProcessorRegistry::execute` で `ProcessorError` を String に潰す | **unchanged** | `crates/core/src/processor.rs:135` 改変なし。API stability のための error_kind 別フィールド化は未着手。 |
| 006 | サイドカー manifest/content の hard binding 未検証 | **unchanged** | `crates/tee/src/orchestrator.rs:218-224` 改変なし（doc comment は `ProcessRequest.input` の sidecar 処理を「c2pa-verify はコンテンツに実行」と注釈追加されたが、binding 検証ロジック自体は追加されていない）。攻撃者の manifest A + content B 提示パスは依然許容される。 |
| 007 | `attestation` の Base64 デコード失敗を `AttestationInvalid` で握る | **unchanged** | `crates/solana/src/extension.rs:121-123` 改変なし。Round 1 で提案した `MalformedAttestation` variant 追加は未着手。 |
| 008 | RateLimiter の poisoned Mutex で `.unwrap()` panic | **fixed** | `crates/gateway/src/rate_limit.rs:66-69`, `:98-101` で `match self.buckets.lock() { Ok(g) => g, Err(poisoned) => poisoned.into_inner() }` に変更。`check_rate_limit` と `prune_idle` の両方で対応。doc comment も「instead of cascading into 500」と説明。 |
| 009 | `proxy_fetcher` の `set_*_timeout().ok()` 握りつぶし | **fixed** | `crates/tee/src/proxy_fetcher.rs:103-114`, `:125-136` で `.map_err(|e| FetchError::HttpError { ... })?` に変更。TCP と vsock 両方の経路で対応。スローロリス攻撃面を閉じた。 |
| 010 | JUMBF parser の `size - HEADER_SIZE` underflow | **unchanged** | `crates/core/src/jumbf.rs:179`, `:193`, `:241`, `:253`, `:290` 改変なし。`child_start + child_header.size` の `checked_add` 未適用箇所（`:202`, `:265`, `:299`）も同様。攻撃者制御の JUMBF 入力で panic/OOM 可能性が残る。 |
| 011 | `read_so_far` の型キャスト優先順位 | **fixed** | `crates/core/src/jumbf.rs:136-141` で `let label_bytes: u64 = if ... { 0 } else { label.len() as u64 + 1 };` に分解、`let read_so_far: u64 = 16 + 1 + label_bytes;` と明示。誤読リスク解消。 |
| 012 | `build_payload` の `expect("cannot fail")` | **unchanged** | `crates/crypto/src/payload.rs:21` 改変なし。 |

### nitpick 個別状況

| # | 要旨 | Round 2 status | 備考 |
|---|---|---|---|
| 001 | `expect` メッセージが状況を説明していない | **unchanged** | `crates/tee/src/main.rs:164`, `:206`, `crates/gateway/src/tee_client.rs:107` で同型の generic メッセージが残存。 |
| 002 | `tee-entrypoint.sh` の `ip link` 失敗 silent | **unchanged** | `deploy/aws/docker/tee-entrypoint.sh:18` 改変なし。 |
| 003 | `extract_active_manifest_signature` の重複文言 | **unchanged** | `crates/core/src/c2pa_verify.rs:202`, `:234` で同一の `"C2PA Reader construction failed: {e}"` を独立に生成。 |
| 004 | `Base58Failed` variant が pubkey/hash 兼用 | **unchanged** | `crates/solana/src/extension.rs:47`, `:88`, `:93` 改変なし。 |
| 005 | `whitelist_program_id` で `Pubkey::from_str(...).unwrap()` | **fixed** | `crates/solana/src/whitelist.rs:91-100` で `pubkey!` macro による `const Pubkey` 化 + `#[inline]` 関数経由。`crates/solana/src/cnft.rs:22-29` の `spl_account_compression_v2_id` も同様に修正済み。Round 1 提案 (`OnceLock`) ではなく Solana の標準 `pubkey!` macro 採用で、より良い解。 |
| 006 | `MockAttestationVerifier::PREFIX` の "missing mock prefix" メッセージ | **unchanged** | `crates/attestation/src/lib.rs:123` 改変なし。 |
| 007 | `run-stack.sh` と `stop-stack.sh` の cleanup ロジック重複 | **unchanged** | `deploy/aws/scripts/run-stack.sh:42-45` と `stop-stack.sh:16-19` の 3 行重複は残存。 |

## 新規発見

修正中に追加されたコードや、Round 1 ではスキャン外だった経路で見つかった新規問題。

### new-must-fix-001 `HttpTeeClient::process` でも body 読み取り `unwrap_or_default()` を踏襲

- 場所: `crates/gateway/src/tee_client.rs:186`
- 観察:
  ```rust
  if !resp.status().is_success() {
      let body = resp.text().await.unwrap_or_default();
      return Err(TeeClientError::HttpError { status, body });
  }
  ```
- 問題: Round 1 の should-fix-002 で指摘した GET/POST と同型のパターンが、新しく追加された `process` ハンドラ (encrypted/plaintext 両対応) にもコピペ追加されている。修正が広がる前に共通ヘルパ化すべきだったケース。重大度は should-fix 同等だが、修正対象の拡大として明示しておく。
- 修正案: `crates/gateway/src/tee_client.rs` 内に `async fn read_error_body(resp: reqwest::Response) -> String` を抽出し、`.unwrap_or_else(|e| format!("<body read failed: {e}>"))` に統一する。

### new-should-fix-001 chunked 経路の budget 超過時、status だけ送って 0 長フレームを書く

- 場所: `crates/proxy/src/handler.rs:132-140`
- 観察:
  ```rust
  if total > MAX_RESPONSE_BYTES {
      tracing::warn!(total, max = MAX_RESPONSE_BYTES, ...);
      // Terminate the stream with a zero-length chunk so the
      // TEE sees a clean EOF, then surface the failure through
      // the next request (proxy is one-shot per connection).
      w.write_all(&0u32.to_be_bytes()).await?;
      w.flush().await?;
      return shutdown_write(w).await;
  }
  ```
- 問題: TEE 側 (`ProxyContentFetcher::read_chunked_body`) はゼロ長チャンクを「正常終端」として扱い、現状の `body` (途中で打ち切られた不完全データ) を `Ok(body)` で返す。`status = 200`、`body` が `MAX_RESPONSE_BYTES` 直前まで詰まった状態で c2pa-verify に渡され、c2pa パーサが「manifest 不完全」エラーで失敗する。ユーザに返るエラーが「コンテンツが不完全」のような曖昧な表示になり、proxy budget 超過が原因と特定しづらい。
- コメント自体が「surface the failure through the next request」と認めているが、1 req per connection なので "next request" は別 TCP 接続でしかなく、現リクエストでは silent truncation が成立する。
- 修正案: budget 超過時はチャンク状態を維持したまま、エラー専用の負ステータス（または別の sentinel）を送る wire 拡張、もしくは header 送出前にバッファして `Content-Length` 既知パスに合流させる（後者は memory 圧迫の懸念あり）。最小修正としては、`status` を送出した後にエラーフレームを差し込めないため、proxy 側で `status` 送信を chunked モードでは「ヘッダ + 最初の正常チャンク到着確認後」まで遅延させるリファクタが必要。

### new-should-fix-002 `handler.rs::shutdown_write` が `_ = w.shutdown().await` で結果を捨てる

- 場所: `crates/proxy/src/handler.rs:181-185`
- 観察:
  ```rust
  async fn shutdown_write<W: tokio::io::AsyncWrite + Unpin>(w: &mut W) -> std::io::Result<()> {
      use tokio::io::AsyncWriteExt;
      let _ = w.shutdown().await;
      Ok(())
  }
  ```
- 問題: socket 半閉じの失敗 (EPIPE 等) を握りつぶしている。GET 200 の長尺ストリーム成功後 → クライアントが TCP RST → `w.shutdown()` が `BrokenPipe` を返したとしても何も起きない。これは観測性の問題で attack 経路は薄いが、Round 1 の方針（silent fallback は厳格に評価）に従い nitpick より一段上で挙げる。
- 修正案: `if let Err(e) = w.shutdown().await { tracing::debug!(error = %e, "writer shutdown failed"); }` で意図的な無視であることを明示する。あるいは戻り値 `io::Result<()>` を活かして呼び出し側で warn する。

### new-nitpick-001 `RealNsm::Drop` で `nsm_exit` の戻り値情報がない

- 場所: `crates/tee/src/vendor/aws.rs:51-61`
- 観察:
  ```rust
  driver::nsm_exit(self.fd);
  tracing::debug!(fd = self.fd, "nsm_exit called");
  ```
- 問題: `nsm_exit` は `()` を返す API なので失敗を観測する手段がない。コメントで「returns nothing — there is no error path」と明記しているのは正しいが、二重 close (fd を別所有で複製→drop が複数回呼ばれる) のような操作ミスがあった場合に検出できない。`Drop` で `self.fd = -1` のように invariant を維持しても良い。重大度は低く、nitpick。
- 修正案: `Drop::drop` の中で `self.fd = -1` を設定し、次回（万一発生した場合の）呼出を no-op にする。あるいは `fd` を `Option<OwnedFd>` で表現して RAII を強制する。

### new-nitpick-002 `tee_seeded_rng` 経由ではない `OsRng` 使用が server.rs テスト初期化に残る

- 場所: `crates/tee/src/server.rs:368-369`
- 観察:
  ```rust
  key_bundle: KeyBundle::generate(&mut rand::rngs::OsRng).unwrap(),
  solana_key: SolanaSigningKey::generate(&mut rand::rngs::OsRng),
  ```
- 問題: テスト fixture なので production 安全性には影響しないが、`#[cfg(test)]` 内ではなく `fn fixtures()` のような共有テストヘルパなのでパッと見の見分けが付きづらい。本観点ではテスト中の `.unwrap()` は許容範囲だが、production-style コードと混ぜると `must-fix-001` の「TEE 内 OsRng 禁止」議論が後で別エージェントに再発見されたとき毎回精査されるノイズになる。
- 修正案: ファイル冒頭に `// Test fixtures only — production paths use tee_seeded_rng (see main.rs).` のヘッダコメントを置く、あるいは関数名を `test_*` プレフィックスにする。

## 全体所感

Round 1 で挙げた 30 件のうち 9 件 (30%) が実際に修正された。修正の質は概ね良好で、特に以下は丁寧な改善が見られる:

- **proxy chunked framing** (must-fix-002): wire protocol を sentinel ベースで拡張し、client/server 両方を同時に書き換えた良い修正
- **NSM empty random ガード** (must-fix-006): 1 行追加で無限ループを潰した
- **timeout 仕様準拠** (must-fix-010): `Option<u64>` API リファクタで仕様 §4.4 が動く形になった
- **RateLimiter poison 復旧** (should-fix-008): doc comment まで丁寧
- **proxy_fetcher timeout 設定エラー伝搬** (should-fix-009): すべての経路で `.ok()` を排除
- **pubkey! const 化** (nitpick-005): `OnceLock` 提案より優れた解（Solana 標準）
- **JUMBF キャスト明示化** (should-fix-011): 軽微だが意図が明確になった

一方で **20 件 (67%) が未対応**として残り、特に以下のクラスタは引き続きリスク:

1. **TEE 内 OsRng (must-fix-001)**: 最も重要な指摘だが手付かず。Nitro `/dev/urandom` の seed 起源依存問題は依然 AES-GCM nonce 衝突の理論リスクを残す
2. **deploy script の fail-fast 不足 (must-fix-007/008)**: TEE 起動失敗が production で silent に進む構造的バグが残存
3. **エラー型潰し (should-fix-003/004/005)**: thiserror の `#[from]` を活かさず `format!("{:?}")` で潰すアンチパターンが定着
4. **JUMBF parser の checked 算術 (should-fix-010)**: 攻撃者制御入力経路のため修正優先度高いはずだが未着手
5. **sidecar hard binding (should-fix-006)**: 仕様 §0.1 のハードバインディング保証が実装側で履行されない設計欠落が残る
6. **`unknown` issuer フォールバック (must-fix-009)**: 下流 trust 判定を誤らせる構造

新規発見 (5 件: must 1, should 2, nitpick 2) はすべて Round 1 で見つけた既存パターンの**漏れ**や**修正の波及不足**であり、本格的な regression は確認されなかった。新規追加コード (gateway `process` ハンドラ、chunked proxy framing) は概ね Round 1 の指摘を意識して書かれており、品質低下は見られない。

**未修正の must-fix 6 件と should-fix 9 件は、特に OSS 公開前に必ず対処すべき**。中でも must-fix-001 (TEE OsRng) と must-fix-009 (unknown issuer) は単独でセキュリティバグとして致命的なため、本観点としては「v0.1.2 OSS 公開 No-Go」判定を維持する。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| must-fix-001 | wontfix(Nitro `/dev/urandom` は NSM seeded で `OsRng` 経由でも cryptographic 差異なし。G ラウンド M-2 と同根決定) | |
| must-fix-002 / 005 / 006 / 010 | fixed | Round 2 認定済み。 |
| must-fix-003 | fixed | `crates/proxy/src/handler.rs` non-GET/POST body 経路の `response.bytes().unwrap_or_default()` を `match { Ok(b)/Err(e) }` に書き換え。upstream body 読み取り失敗は `write_error(PROXY_ERROR_STATUS, ...)` で TEE に明示通知。 |
| must-fix-004 | partially-fixed(`written > len` truncate は対応済み、`written < len` は TEE 側 `read_exact` の `UnexpectedEof` で表面化するため fail-close は機能。explicit error frame 追加は wire spec の二重拡張を伴うため見送り) | |
| must-fix-007 | fixed | `deploy/aws/scripts/run-stack.sh` TEE 起動待ちに `TEE_READY=0/1` flag を導入。60s 経過後も `/health` が応答しない場合は `exit 1` で gateway 起動前に fail。トラブルシュート用の `nitro-cli console` ヒントもエラーメッセージに含めた。 |
| must-fix-008 | fixed | socat 起動直後に `$!` で PID を捕捉し、`kill -0` で生存確認。即死している場合は socat.log を案内して `exit 1`。 |
| must-fix-009 | fixed | `crates/core/src/c2pa_verify.rs::SignerInfo::issuer` を `String` から `Option<String>` に変更。`"unknown"` の sentinel 文字列が `validation: "valid"` と共に Attestation に封入される構造的バグを解消。`#[serde(skip_serializing_if)]` でフィールドの欠落として表現。 |
| must-fix-011 | wontfix(`programs/title-whitelist` の `parse_public_values` 内 `unwrap()` は事前の `require!` 長さチェックで到達不能。`try_into` への置き換えは program 再 deploy を要し、defense-in-depth の価値とコストが見合わず) | |
| should-fix-001 | wontfix(env var typo silent fallback は運用上の慣習。`default` 行動が明示的に文書化されていれば実害なし) | |
| should-fix-002 | fixed | new-must-fix-001 と統合対応。 |
| should-fix-003/004/005 | wontfix(error 型潰しの thiserror 構造化は API 安定化フェーズ (v0.1.3 SDK 整備) で対応) | |
| should-fix-006 | wontfix(sidecar manifest/content の hard binding は SPECS_JA §0.1 の規定で client 責務として整理済み。TEE 側で再検証する設計変更は本 audit 範囲を超える) | |
| should-fix-007 | wontfix(`MalformedAttestation` variant 追加は `crates/solana` のエラー分類整理と統合フェーズで対応) | |
| should-fix-008 / 009 / 011 | fixed | Round 2 認定済み。 |
| should-fix-010 | fixed | `crates/core/src/jumbf.rs` に `content_size()` (`checked_sub`) と `box_end()` (`checked_add`) ヘルパを新設し、`size - HEADER_SIZE` / `child_start + child_header.size` の 9 箇所をすべて checked 演算経由に書き換え。攻撃者制御 JUMBF 入力での panic/overflow を fail-close。 |
| should-fix-012 | wontfix(`build_payload` の `expect("cannot fail")` は serde_json の Map serialization で型システム上失敗しない経路。documented invariant) | |
| nitpick-001..004/006/007 | wontfix(エラーメッセージ細部整理は OSS 公開前の文言統一フェーズで一括対応) | |
| nitpick-005 | fixed | Round 2 認定済み。 |
| new-must-fix-001 | fixed | `crates/gateway/src/tee_client.rs` に `read_error_body()` ヘルパを抽出し、4 箇所の `resp.text().await.unwrap_or_default()` を `<body read failed: e>` 形式の明示エラー文字列付き読み取りに置き換え。 |
| new-should-fix-001 | fixed | K6 must-fix-006 (CHUNKED_TRUNCATED) で統合対応済み。 |
| new-should-fix-002 | wontfix(`shutdown_write` の `_ = w.shutdown().await` は connection cleanup の best-effort で、socket クローズ失敗はログ価値ゼロ。本観点での tracing 追加は visual noise) | |
| new-nitpick-001 | wontfix(`RealNsm::drop` の fd invariant 維持は `Option<OwnedFd>` 化を伴い、現状の `nsm_exit()` API への適合コストが見合わず) | |
| new-nitpick-002 | wontfix(`server.rs::tests::test_state` 内の `OsRng` 使用は test fixture で production 経路と隔離済み。ファイル冒頭コメントは v0.1.3 リファクタで対応) | |
