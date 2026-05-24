# C. エラーハンドリング (Round 3)

## 概要

担当範囲: `crates/*/src/**/*.rs`, `programs/title-whitelist/src/**/*.rs`, `sp1-guests/**/*.rs`, `deploy/aws/**/*.sh`。

Round 2 で挙げた既存 30 件 + 新規 5 件 = 計 35 件の処理状況確認、Round 2 → Round 3 の修正で混入した新規問題、および Round 2 ではスキャン外だった経路で見つけた新規発見の洗い出し。

Round 3 修正コミットを反映した状態（`crates/proxy/src/protocol.rs` に `CHUNKED_TRUNCATED` 追加、`crates/gateway/src/tee_client.rs::read_error_body` ヘルパ抽出、`crates/proxy/src/handler.rs` 非 GET/POST body 経路の `match` 化、`run-stack.sh` の `TEE_READY` flag と socat PID 生存確認、`crates/core/src/c2pa_verify.rs::SignerInfo::issuer` の `Option<String>` 化、`crates/core/src/jumbf.rs` の `content_size`/`box_end` checked 算術ヘルパ）を対象に再精査した。

## Round 2 指摘の処理状況

### 件数集計

| カテゴリ | 完了 (fixed/wontfix判定済) | 未対応として残存 | regression |
|---|---|---|---|
| must-fix (11) | 11 | 0 | 0 |
| should-fix (12) | 12 | 0 | 0 |
| nitpick (7) | 7 | 0 | 0 |
| new-must-fix (1) | 1 | 0 | 0 |
| new-should-fix (2) | 2 | 0 | 0 |
| new-nitpick (2) | 2 | 0 | 0 |
| **計 (35)** | **35** | **0** | **0** |

「完了」は Round 3 修正コミットで実コード変更があった件と、Round 2 ログで `wontfix` 判定の理由が妥当な件の合計。判定の妥当性は下表で個別に確認する。

### must-fix 個別状況（Round 3）

| # | Round 2 判定 | Round 3 検証 | 備考 |
|---|---|---|---|
| 001 | wontfix | accepted | `crates/crypto/src/sealed_channel.rs:55` の `ResponseChannel::seal()` で `OsRng` 直接利用は残存するが、Nitro Enclave の `/dev/urandom` は起動時に NSM seed されるため `OsRng` 経由でも cryptographic 品質は `tee_seeded_rng` と同等。G ラウンド M-2 で同根決定済との交差確認も妥当。ただし L84 の `seal_for()` 側はクライアント側経路（TEE 内部ではない）であり、そもそも `OsRng` で問題ない箇所だった点を補記。doc comment で「Nitro `/dev/urandom` is NSM seeded」を sealed_channel.rs に書き残すと、将来「TEE 内で OsRng を使うな」というルールが再び議論されたときに即決できる（将来作業として残置）。 |
| 002 | fixed | confirmed | `crates/proxy/src/handler.rs:84-157` の chunked / known-length 分岐 + `CHUNKED_SENTINEL` 経路、TEE 側 `crates/tee/src/proxy_fetcher.rs:163-188` の `body_len_field == CHUNKED_SENTINEL` 判定が wire 仕様 (`crates/proxy/src/protocol.rs:18-40` の docコメント) と整合。 |
| 003 | fixed | confirmed | `crates/proxy/src/handler.rs:160-191` で `response.bytes().await` を `match { Ok(b) => …, Err(e) => write_error(w, PROXY_ERROR_STATUS, …) }` に書き換え。TEE 側は status 0 (proxy internal error) を `FetchError::HttpError` として受け取る経路 (`proxy_fetcher.rs:192-197`) と接続している。 |
| 004 | partially-fixed | accepted | `crates/proxy/src/handler.rs:106-128`: `written > len` は truncate & break、`written < len` は warn のみ。Round 2 ログの「TEE 側 `read_exact` の `UnexpectedEof` で fail-close」検証: `crates/tee/src/proxy_fetcher.rs:180-186` で `body_len` バイトを `read_exact` するため、proxy が `< len` バイト書いて write 側 shutdown すると TEE は `UnexpectedEof` → `FetchError::HttpError`。fail-close は機能する。explicit error frame を入れていない設計判断は wire spec の二重拡張回避として合理的。 |
| 005 | fixed | confirmed | `crates/gateway/src/state.rs:114-123`: `Err(e) => { tracing::warn!(...); true }`、doc comment にも fail-safe 動作の根拠が明示。 |
| 006 | fixed | confirmed | `crates/tee/src/vendor/aws.rs:72-75` で `random.is_empty()` を `TeeError::RandomFailed` として返す。無限ループ閉鎖。 |
| 007 | fixed | confirmed | `deploy/aws/scripts/run-stack.sh:87-101`: `TEE_READY=0/1` 状態変数、60s 経過後 `/health` 未応答なら `exit 1` で gateway 起動前に abort。エラーメッセージに `nitro-cli console` ヒントも併記。 |
| 008 | fixed | confirmed | `deploy/aws/scripts/run-stack.sh:78-85`: `nohup socat ... &` 直後に `$!` で PID 捕捉、`sleep 1` 後 `kill -0` で生存確認。即死している場合 `socat.log` を案内して `exit 1`。 |
| 009 | fixed | confirmed | `crates/core/src/c2pa_verify.rs:60-69`: `SignerInfo::issuer` は `Option<String>`、`#[serde(skip_serializing_if = "Option::is_none")]`。`"unknown"` sentinel 文字列が `validation: "valid"` と共に Attestation に封入される構造的バグ解消。doc comment にも「'unknown' string so downstream trust logic doesn't mistake it for a real issuer named 'unknown'」と明記。 |
| 010 | fixed | confirmed | `crates/tee/src/limits.rs:58-65` で `data_size_hint: Option<u64>` シグネチャ、`None` で `MAX_GLOBAL_TIMEOUT` 返却。`crates/tee/src/orchestrator.rs:184-192` の呼び出し側も `InputData::Fragmented` で `fragment_urls.len() × MAX_FRAGMENT_SIZE`、`Single`/`Sidecar` で `None` を渡す。仕様 §4.4 の `timeout = min(MAX, BASE + size / MIN_SPEED)` がストリーミング/fragmented の両方で意味を持つ。 |
| 011 | wontfix | accepted | `programs/title-whitelist/src/lib.rs:349,370` の `try_into().unwrap()` は L348 / L368 の `require!(data.len() >= offset + 4, ...)` で長さチェック済み。スライス `[offset..offset + 4]` は 4 バイト固定で `[u8; 4]` への変換は型システム上失敗不能なため到達不能。program re-deploy のコスト vs defense-in-depth 価値の評価は妥当。代替案として `data[offset..].first_chunk::<4>()` の Rust 1.77+ API があるが anchor / solana-program の rustc 制約と合うかは別途確認が必要。 |

### should-fix 個別状況（Round 3）

| # | Round 2 判定 | Round 3 検証 | 備考 |
|---|---|---|---|
| 001 | wontfix | accepted with reservation | `crates/gateway/src/main.rs:50-63`、`crates/tee/src/main.rs:143-150,161,192` の env var typo silent fallback は仕様化されていれば実害なし。ただし現状 SPECS_JA には `POOL_TOTAL_LIMIT` 等の env var 名と default 値の表は **存在しない**（`docs/v0.1.2/SPECS_JA.md` §5 を grep して確認）。OSS 公開時には README または運用ガイドに env var 表を追加すべき。 |
| 002 | fixed | confirmed | new-must-fix-001 と統合解決。`read_error_body` ヘルパ抽出後、`crates/gateway/src/tee_client.rs:122,147,193` の 3 箇所が一貫して呼び出している（Round 1 で挙げた 3 箇所 + new-must-fix-001 で追加された `process()` ハンドラ）。`process()` 内 octet-stream 読み取り L210 と JSON 読み取り L216 は本来の成功経路で、エラー body 読み取りとは別分類。 |
| 003 | wontfix | accepted | `crates/tee/src/orchestrator.rs:297` の `OrchestratorError::DecryptionFailed(format!("{e:?}"))` は v0.1.3 の SDK 整備フェーズで `#[from]` 化する計画。現状は `CryptoError` の Debug 出力が含まれるため diag は可能。 |
| 004 | wontfix | accepted | `crates/tee/src/orchestrator.rs:258` の `ResponseSealFailed`、L301 の `PayloadMetadataInvalid` も同様。v0.1.3 で thiserror `#[from]` 化。 |
| 005 | wontfix | accepted | `crates/core/src/processor.rs:136` の `ProcessorError → e.to_string()` 潰しは Display 形式に依存する API 設計で、`error_kind` 別フィールド化は SDK 安定化フェーズの判断。 |
| 006 | wontfix | accepted with reservation | sidecar manifest/content の hard binding 未検証は SPECS_JA §0.1 のハードバインディング規定上、client 責務として整理されている、と Round 2 は判定。ただし §0.4 「TEE 内でコンテンツのバイトを処理する」「Stateless」原則と照らすと、sidecar 経路でクライアントの提示する manifest A + content B の不整合を TEE が見抜けない設計は、§1.6 の「処理結果が改ざんされていない」保証の射程内かグレー。OSS 公開ドキュメントで「sidecar 形式は client responsibility」を明示するのが望ましい。実装変更は v0.1.3 以降。 |
| 007 | wontfix | accepted | `crates/solana/src/extension.rs:121` の Base64 デコード失敗を `AttestationInvalid` で包む扱いは、エラー型分類整理（`MalformedAttestation` variant 追加）と統合される予定。 |
| 008 | fixed | confirmed | `crates/gateway/src/rate_limit.rs:66-69,98-101` で `match self.buckets.lock() { Ok(g) => g, Err(poisoned) => poisoned.into_inner() }`。doc comment にも「instead of cascading into 500」と説明あり。 |
| 009 | fixed | confirmed | `crates/tee/src/proxy_fetcher.rs:103-114,128-138` で `set_read_timeout` / `set_write_timeout` 失敗を `FetchError::HttpError` で伝搬。TCP と vsock の両経路で対応済み。slowloris 抑制が確実になった。 |
| 010 | fixed | confirmed | `crates/core/src/jumbf.rs:44-60` に `content_size()` (checked_sub) と `box_end()` (checked_add) ヘルパ新設。`size - HEADER_SIZE` / `child_start + child_header.size` の出現箇所はすべて checked 演算経由 (`jumbf.rs:198,212,221,257,269,273,281,304,306,313,331,345`)。攻撃者制御 JUMBF 入力で `panic` / overflow 不能。`MAX_SIGNATURE_SIZE` cap (L32) も維持。 |
| 011 | fixed | confirmed | `crates/core/src/jumbf.rs:155-160` で `let label_bytes: u64 = if … { 0 } else { label.len() as u64 + 1 };`、`let read_so_far: u64 = 16 + 1 + label_bytes;` と分解。誤読リスク解消。 |
| 012 | wontfix | accepted | `crates/crypto/src/payload.rs:29` の `serde_json::to_vec(metadata).expect("metadata serialization cannot fail")` は `EncryptedPayloadMetadata` の Serialize impl が文字列 + Option<String> のみで失敗経路がない documented invariant。 |

### nitpick 個別状況（Round 3）

| # | Round 2 判定 | Round 3 検証 | 備考 |
|---|---|---|---|
| 001 | wontfix | accepted | `crates/tee/src/main.rs:166,207`、`crates/gateway/src/tee_client.rs:107` の expect メッセージは OSS 公開前の文言統一フェーズで一括対応予定。 |
| 002 | wontfix | accepted | `deploy/aws/docker/tee-entrypoint.sh:18` の `ip link set lo up 2>/dev/null || true` 残存。loopback 起動失敗は後続の `socat`/`title-tee` 起動失敗で顕現するため致命ではない。 |
| 003 | wontfix | accepted | `crates/core/src/c2pa_verify.rs:212,242` の `"C2PA Reader construction failed: {e}"` 重複は OSS 公開前の文言統一で一括対応。 |
| 004 | wontfix | accepted | `crates/solana/src/extension.rs:47,88,93` の `Base58Failed` variant 兼用は変更なし、v0.1.3 のエラー分類整理に組み込む。 |
| 005 | fixed | confirmed | `crates/solana/src/whitelist.rs` および `crates/solana/src/cnft.rs` で `pubkey!` macro による const Pubkey 化。OnceLock より優れた解。 |
| 006 | wontfix | accepted | `crates/attestation/src/lib.rs:123` の "missing mock prefix" メッセージ残存、文言統一フェーズで対応。 |
| 007 | wontfix | accepted | `deploy/aws/scripts/{run,stop}-stack.sh` の cleanup ロジック 3 行重複は許容範囲、shellscript の共通ヘルパ化は別途検討。 |

### Round 2 新規発見の処理状況

| # | Round 2 判定 | Round 3 検証 | 備考 |
|---|---|---|---|
| new-must-fix-001 | fixed | confirmed | `crates/gateway/src/tee_client.rs:160-165` で `async fn read_error_body(resp) -> String` ヘルパ抽出、`<body read failed: {e}>` 形式で読み取り失敗をエラー文字列に明示。L122,L147,L193 の 3 箇所すべてに適用済み。Round 2 should-fix-002 と統合解決。 |
| new-should-fix-001 | fixed | confirmed | `crates/proxy/src/protocol.rs:48-53` で `CHUNKED_TRUNCATED` sentinel 追加（chunked stream 末尾マーカー位置の `u32::MAX`）、`crates/proxy/src/handler.rs:140-148` で budget 超過時に書き出し、`crates/tee/src/proxy_fetcher.rs:275-283` で `FetchError::HttpError` として fail-close。silent truncation の経路を閉じた良い修正。`CHUNKED_SENTINEL` と `CHUNKED_TRUNCATED` がいずれも `u32::MAX` という値の二重定義は wire format 上は body_len field と chunk_len field で位置が排他のため衝突しないが、`CHUNKED_TRUNCATED: u32 = u32::MAX` を冗長な const として残している点は意図的な「位置別の意味付け」をコード上で示す trade-off（後述）。 |
| new-should-fix-002 | wontfix | accepted | `crates/proxy/src/handler.rs:207-211` の `shutdown_write` 内 `_ = w.shutdown().await` 残存。同じ shape が `crates/proxy/src/main.rs:160-167` の `VsockWriter::poll_shutdown` にもあり、コメントで「best-effort: OS will tear the socket down」と意図明示。tracing 追加は visual noise との判断は妥当。 |
| new-nitpick-001 | wontfix → 一部進展 | confirmed | `crates/tee/src/vendor/aws.rs:51-61` の `Drop for RealNsm` で `if self.fd >= 0 { driver::nsm_exit(self.fd); ... }` ガードが入った。Round 2 が提案した `self.fd = -1` setter ではないが、`fd < 0` 初期不正値からの drop は no-op になる方向の hardening。Round 2 ログでは wontfix 扱いだが、コード差分上は前進している。`Option<OwnedFd>` への置き換えは未着手で、二重 drop 完全防止は未達。 |
| new-nitpick-002 | wontfix | accepted | `crates/tee/src/server.rs:412` の `KeyBundle::generate(&mut rand::rngs::OsRng).unwrap()` は test fixture 内。production 経路は `crates/tee/src/main.rs:88` の `tee_seeded_rng` 経由。ファイル冒頭コメント追加は v0.1.3 リファクタで対応予定。 |

## 新規発見（Round 3）

Round 2 でスキャン外だった経路 + Round 3 修正コミットで新たに加わったコードを精査した結果、以下を発見した。

### new-r3-nitpick-001 `CHUNKED_SENTINEL` と `CHUNKED_TRUNCATED` の値が両方 `u32::MAX`

- 場所: `crates/proxy/src/protocol.rs:46`, `:53`、`crates/tee/src/proxy_fetcher.rs:147`, `:152`
- 観察:
  ```rust
  pub const CHUNKED_SENTINEL: u32 = u32::MAX;
  pub const CHUNKED_TRUNCATED: u32 = u32::MAX;
  ```
- 問題: wire 上では「`body_len` フィールド位置」と「chunk-stream 内の `chunk_len` フィールド位置」で文脈が分離しているため、両方が `u32::MAX` でも parse は曖昧にならない。しかしコード読みでは「`CHUNKED_TRUNCATED == CHUNKED_SENTINEL` だが意味は別」というメンタルモデルが必要で、将来「ストリーム末尾の chunk_len を実 chunk と区別するため `CHUNKED_TRUNCATED` を `u32::MAX - 1` に変更」というような片側変更が、TEE 側 `body_len_field == CHUNKED_SENTINEL` 判定（`proxy_fetcher.rs:167`）と意味的に独立であることに気付きにくい。
- 重大度: nitpick。実害なし、読みやすさのみ。
- 修正案: protocol.rs の doc comment にもう一段「同じ値だが、位置によって意味が分かれる。両方が衝突しない理由はフレーミングが排他であること」を明示する。または `CHUNKED_TRUNCATED` を `0xFFFF_FFFE` に分けて値レベルでも分離する（wire 互換性を切る変更なので OSS 公開前のみ可）。

### new-r3-nitpick-002 `tee-entrypoint.sh` の `socat` 起動失敗は外側 health probe 任せ

- 場所: `deploy/aws/docker/tee-entrypoint.sh:22-23`
- 観察:
  ```sh
  socat VSOCK-LISTEN:4000,fork,reuseaddr TCP:127.0.0.1:4000 &
  SOCAT_PID=$!
  ```
- 問題: Round 2 の must-fix-008 で同種パターン (`run-stack.sh` の host 側 socat) を `kill -0` 確認で fail-fast 化したが、enclave 内の socat 起動失敗は本スクリプト内でチェックされていない。`set -eu` は `&` バックグラウンドジョブの即死を検知しない。失敗時の症状は「enclave は起動しているが vsock:4000 がリッスンされていない」=「外側 run-stack.sh の `/health` probe が 60s 失敗 → exit 1」。最終的には Round 3 修正の must-fix-007 経由で発覚するため fail-close は機能するが、診断ログが「TEE が答えない」止まりで、原因切り分けに `nitro-cli console` が必要。
- 重大度: nitpick。外側でカバーされているが、enclave 内の単一プロセス障害として可視化する価値がある。
- 修正案: `run-stack.sh` と同じく `sleep 1; kill -0 "$SOCAT_PID" || { echo "socat in enclave died" >&2; exit 1; }` を挟む。enclave 内で `exit 1` すると enclave 自体が落ちて nitro-cli describe-enclaves で `State: TERMINATED` として見える。

### new-r3-nitpick-003 `proxy/src/handler.rs::forward_http_streaming` の `unsupported method` 経路は最終的に `shutdown_write` の `Ok(())` を返す

- 場所: `crates/proxy/src/handler.rs:60-64`
- 観察:
  ```rust
  other => {
      tracing::warn!(method = other, …, "rejecting unsupported HTTP method");
      let msg = format!("Unsupported method: {other}").into_bytes();
      write_error(w, 400, &msg).await?;
      return shutdown_write(w).await;
  }
  ```
- 問題: `write_error` で `status = 400` + body を書いた後、`shutdown_write` の中身は `let _ = w.shutdown().await; Ok(())`。shutdown 自体の失敗（new-should-fix-002 と同じ shape）はここでも握りつぶされる。Round 2 で `new-should-fix-002` として既に挙げた挙動なので新規ではないが、unsupported method 経路だけ別の場所で同パターンを再採用しており、リファクタ時の見落としリスクがある。
- 重大度: nitpick。既存挙動の確認。
- 修正案: なし（new-should-fix-002 wontfix の方針を継承）。

## 全体所感

Round 3 修正は Round 2 の 35 件すべてに対して明確な判定（fixed / wontfix の根拠付き）を確定させた。修正の質は Round 2 と同様に高く、特に以下は丁寧:

- **chunked 末尾 sentinel の 2 種類化** (new-should-fix-001): wire 仕様の non-backward-compat 拡張だが、ドキュメント・両端実装・テストが揃った段階で導入されており、silent truncation を明確に fail-close に振った
- **`read_error_body` ヘルパ抽出** (new-must-fix-001): Round 1 の should-fix-002 を含めて 3 箇所 + 新規 1 箇所を一括解決、`<body read failed: {e}>` という診断文字列で読み取り失敗を可視化
- **`Option<String> issuer`** (must-fix-009): 型レベルで `"unknown"` sentinel の混入を不能にした構造的修正
- **`TEE_READY` flag + socat PID 生存確認** (must-fix-007/008): 60s timeout を本当に意味のある fail-fast に変えた
- **`SignerInfo::issuer` doc comment**: 「unknown という名前の発行者と混同されないよう」と将来の読み手への注意も同梱

未対応として残ったが Round 3 で `wontfix` 判定が確定したもの:

1. **must-fix-001 (TEE 内 OsRng)**: Nitro `/dev/urandom` の NSM seed 経由で cryptographic 同等という根拠。G-M-2 と整合
2. **must-fix-011 (program parse unwrap)**: `require!` 長さチェック後の到達不能 `unwrap` で、program re-deploy コストとの天秤
3. **should-fix-003/004/005 (`format!("{e:?}")` error 潰し)**: v0.1.3 SDK 安定化での `#[from]` 化に保留
4. **should-fix-006 (sidecar hard binding)**: client 責務として SPECS_JA §0.1 で整理、ただし docs 上の明文化は推奨
5. **nitpick 文言系**: OSS 公開前の文言統一フェーズで一括対応

regression は確認されなかった。Round 3 修正コミットは Round 2 の指摘範囲内で完結しており、新規追加コード (`CHUNKED_TRUNCATED` sentinel、`read_error_body` ヘルパ、`TEE_READY` フロー、`content_size`/`box_end` checked 算術) はいずれも防御的設計で、新規問題を増やしていない。

新規発見は 3 件すべて nitpick（読みやすさ・診断性向上の提案）で、致命的なエラーハンドリングバグは見つからなかった。

**判定**: 本観点 (C エラーハンドリング) における Round 3 ステータスは **OSS 公開可**。未対応の `wontfix` 件は v0.1.3 マイルストーンに移送、新規 nitpick 3 件は OSS 公開前の文言/コメント統一フェーズで合流させる。Round 2 で「No-Go」判定した must-fix-001 / must-fix-009 のうち、009 は構造的に解消、001 は wontfix 根拠の妥当性を本観点で再確認したため Go 判定に変更する。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| must-fix-001 | wontfix | Nitro `/dev/urandom` は NSM seeded、`OsRng` 経由でも品質同等。G-M-2 と整合。doc コメント追記は v0.1.3 に持ち越し。 |
| must-fix-002/003/005/006/007/008/009/010 | fixed | Round 2 認定済み修正を Round 3 でも再確認、すべて意図どおり動作。 |
| must-fix-004 | partially-fixed | TEE 側 `read_exact` の `UnexpectedEof` で fail-close は機能、explicit error frame は wire spec 二重拡張回避として見送り。 |
| must-fix-011 | wontfix | `require!` 長さチェック後の到達不能 unwrap、program re-deploy コストとのバランス判断。 |
| should-fix-001 | wontfix(reservation) | env var typo silent fallback は仕様化前提。README に env var 表追加を OSS 公開前タスクとして残置。 |
| should-fix-002 | fixed | new-must-fix-001 と統合解決済み。 |
| should-fix-003/004/005 | wontfix | v0.1.3 SDK 安定化フェーズで thiserror `#[from]` 化。 |
| should-fix-006 | wontfix(reservation) | sidecar hard binding は SPECS_JA §0.1 上 client 責務、docs 明文化を OSS 公開前タスクに残置。 |
| should-fix-007 | wontfix | `MalformedAttestation` variant 追加はエラー分類整理フェーズで対応。 |
| should-fix-008/009/010/011 | fixed | Round 2 認定済み、Round 3 で再確認。 |
| should-fix-012 | wontfix | `serde_json::to_vec(metadata)` は型システム上失敗不能 documented invariant。 |
| nitpick-001..004/006/007 | wontfix | OSS 公開前の文言統一フェーズで一括対応。 |
| nitpick-005 | fixed | Round 2 認定済み。 |
| new-must-fix-001 | fixed | `read_error_body` ヘルパで 3 箇所一括解決。 |
| new-should-fix-001 | fixed | `CHUNKED_TRUNCATED` sentinel で silent truncation 経路閉鎖。 |
| new-should-fix-002 | wontfix | `shutdown_write` の best-effort 設計を維持、tracing 追加は visual noise。 |
| new-nitpick-001 | wontfix(進展あり) | `if self.fd >= 0` ガード追加で部分 hardening、`Option<OwnedFd>` 化は v0.1.3 リファクタへ。 |
| new-nitpick-002 | wontfix | test fixture の `OsRng` は production と隔離、コメント追加は v0.1.3 に。 |
| new-r3-nitpick-001 | fixed(K6) | K6 must-fix-008 で `CHUNKED_TRUNCATED = u32::MAX - 1` に値分離済み。wire レベルで物理的に区別される。 |
| new-r3-nitpick-002 | fixed | `tee-entrypoint.sh` の socat 起動直後に `sleep 1; kill -0 "$SOCAT_PID"` を追加。失敗時は enclave 自体を `exit 1` で落として TERMINATED 可視化。外側 60s health probe より早期検知可能に。 |
| new-r3-nitpick-003 | wontfix | unsupported method 経路の `shutdown_write` 握りつぶしは Round 2 new-should-fix-002 と同根。監査自身が「修正案なし、wontfix 方針継承」と判定。 |
