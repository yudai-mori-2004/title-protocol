# タスク 17: 監査フィックスアップ

## 背景

タスク 16 の大監査で 21 エージェントが 459 件の指摘を出した（must-fix: 131, should-fix: 203, nitpick: 125）。
本タスクはこれらの修正を複数セッションに分けて段階的に実施する。

## ガバナンス決定事項

本タスク開始前に以下が決定済み:

- **SSRF / 認証 / リプレイ攻撃対策**: 過剰防御として却下。Gateway は trusted-but-not-secret であり、TEE が C2PA + Attestation で検証する。
- **`--debug-mode` PCR0=all-zero**: 既知の運用上の懸念。ドキュメントに明記するが、フラグ自体は開発用に残す。
- **Solana プログラム変更**: devnet 再デプロイが必要（プログラム ID: `43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs`、admin keypair: `keys/admin.json`）。
- **SP1 proof 生成**: 別セッションで対応（vkey 変更時は再証明が必要）。

## 除外（別タスクに先送り）

以下の指摘は確認済みだが、フィックスアップの範囲を超えるため先送り:

| 指摘 | 先送り理由 |
|---|---|
| K3-mf002 `process_request` を 6 関数に分割 | オーケストレータの大規模リファクタ |
| K3-sf001/sf002 ストリーミングフラグメント処理 + チケット連動 fetch | `ContentFetcher::fetch` の async ストリーミング再設計が必要 |
| D-mf001 クレート再編（tee-core/tee-server/extension-solana） | アーキテクチャ変更 |
| S-sf001 レガシー processor の移植（image-pdq, video-vpdq, cert-*） | 個別の実装タスク |
| S-sf003 OPERATIONS にコスト/料金セクション追加 | ビジネス文書であり、コード修正ではない |
| R-sf007 admin ローテーション / マルチシグ設計 | Phase 2 の設計タスク |
| E: CI/CD パイプライン構築 | インフラタスク |
| K4-sf009 TEE 再起動 e2e テストの flaky 問題（SO_REUSEADDR） | テスト基盤改善 |

## 優先度

- **P0（出荷阻止）**: セキュリティ、データ破損、サイレント障害、相互運用を壊す仕様違反
- **P1（高）**: 機能バグ、デッドな公開 API、誤解を招くドキュメント
- **P2（中）**: コード品質、コメント整理、テスト不足
- **P3（低）**: 細かい指摘、見た目、あれば嬉しい程度

## セッション計画

### セッション 17a: `crates/crypto` + `crates/attestation*`

**優先度: P0** | 約 40 件

暗号修正（K2）:
- mf001: P-256 TEE 側 ECDH — 手書きスカラー乗算を `p256::ecdh::diffie_hellman` に置換
- mf002: AES-256-GCM に AAD 追加（wire ヘッダの suite_id + encap_key をバインド）
- mf003: ML-KEM-768 で `encapsulate(&mut OsRng)` を使用（`rand::random()` + deterministic を廃止）
- mf004: P-256 鍵生成を `SecretKey::random(rng)` 経由に変更（生の seed バイト直接注入を廃止）
- sf001: nonce のワンショット保証をドキュメント化（consuming `seal(self, ...)` も検討）
- sf002: HKDF salt=encap_key の設計意図を記載
- sf003: wire パースの境界チェックを統合
- sf004: payload.rs に `MAX_METADATA_LEN`（64 KiB）+ checked_add を追加
- sf005: `open_request` に `expected_suite` 引数を追加
- sf006: X25519 低位元ポイント検査（shared secret が all-zero の場合のエラー）
- nitpick: エラー型整理、型エイリアス位置、仕様参照の重複削除

Attestation 修正（K1）:
- mf001: Root 自己署名 — pin のみを信頼し、`Cert::verify` から `Option` を除去
- mf002: `trusted_certs_len` パラメータを完全に削除
- mf003: `authenticate()` 内で `digest == "SHA384"` を検証
- mf004: 未知の COSE ヘッダ / `crit` を拒否
- mf005: `ec_decode_sig` を `Signature::from_der` に置換
- mf006: RSA/RSA-PSS コードパスを削除（Nitro では不使用）
- mf007: 未使用の `sha2` oid feature を削除
- sf001-011: ドキュメント修正、Expired variant 整理、MockPrefix を pub(crate) に、他
- nitpick: コメント圧縮、doc comment のフォーマット統一

**コミット**: `fix(crypto,attestation): ECDH, AAD, cert chain verification`

### セッション 17b: `crates/proxy` + `crates/tee`

**優先度: P0-P1** | 約 47 件

Proxy 修正（K6）:
- mf001: chunked transfer-encoding の対応（content_length=None → バッファリングまたはセンチネル方式）
- mf002: `read_string`/`read_bytes` に `max_len` を追加（u32 length からの OOM 防止）
- mf003: `try_clone().expect()` + `unsafe impl Send` を適切な fd 分割または Mutex に置換
- mf004: 4 GiB 超レスポンスでの u32 オーバーフローを防止（ボディ上限を強制）
- mf005: レスポンス後に `shutdown(Write)` を呼び、one-request-per-connection をドキュメント化
- sf001-007: vsock バックプレッシャー、アクセスログ拡充、--privileged の削減、タイムアウト設定可能化、POST Content-Type の固定解除、メソッド許可リストの文書化、TLS 終端位置の仕様明記

TEE 修正（K3, C）:
- mf001: 起動シーケンスを仕様 §5.2 に整合（self-attestation のステップ番号を修正）
- mf003: FakeNsm の `RefCell` + `unsafe impl Send/Sync` を `Mutex` に置換
- mf004: `set_read/write_timeout` のエラーを `.ok()` ではなく伝播
- mf005: `compute_global_timeout(0)` の修正 — data_size_hint に `Option<u64>` を採用
- mf006: NitroRuntime fd の Drop 順序を文書化
- sf003-014: Mock measurement 値の改善、暗号化チェックの fetch 前移動、reqwest async/blocking 問題、axum body limit、NSM GetRandom のゼロバイトチェック、MockRuntime の重複統合、他
- C-mf001: TEE の AES-GCM nonce — 本番での NSM seed 経路を文書化
- nitpick: レガシー参照の削除、SS→§ 統一、エラーメッセージの (503) 除去

**コミット**: `fix(proxy,tee): chunked transfer, OOM guard, timeout, unsafe removal`

### セッション 17c: `crates/gateway` + `crates/core`

**優先度: P0-P1** | 約 42 件

Gateway 修正（K4）:
- mf001: 暗号化レスポンスのパススルー対応（octet-stream Content-Type の中継）
- mf002: ルータに `DefaultBodyLimit::max(64 * 1024)` を追加
- mf003: middleware 順序のコメント修正（axum の layer セマンティクスを正確に記述）
- mf004: TEE のステータスコードを保持（503→502 に化けないようにする）
- mf005: 非 UTF-8 Authorization ヘッダのエラーメッセージ修正
- sf001: rate-limit バケットの GC（バックグラウンドタスクまたは LRU 上限）
- sf002-009: Mutex poisoning 対策、リトライ戦略、health check interval、鍵変更検知、部分失敗ロールバック、Default 削除、ホットループガード

Core 修正（K8）:
- mf001: デッドな processor 出力型の削除（ProvenanceGraphOutput, ImagePdqOutput 等 8 型）
- mf002: デッドな `CoreError` enum の削除
- mf003: `jumbf::extract_signature_from_jumbf` の可視性を `pub(crate)` に修正
- mf004: `ProcessorOutput.data` — `Value` + flatten を `Map<String, Value>` に変更
- mf005: `c2pa::Reader` のキャッシュ化（二重パース回避）
- sf001-010: Registry execute の並列実行ドキュメント、sidecar+encryption バリデーション、JUMBF パーサ修正、re-export 整理

**コミット**: `fix(gateway,core): encrypted passthrough, dead code removal, body limits`

### セッション 17d: `crates/solana` + `programs/title-whitelist` + `sp1-guests`

**優先度: P0-P1** | 約 58 件

Solana クライアント修正（K5, R）:
- mf001: クライアント側 mirror struct の Borsh 互換性修正（StoredMeasurement）
- mf002: KEY_EXPIRY_SECONDS の定義を一元化
- mf003: mpl-bubblegum バージョンを `"=2.1.1"` に固定
- sf004: `process_extension` にホワイトリスト登録チェックのガードを追加
- sf005: mint TX に ComputeBudgetInstruction を追加
- sf008: デッドな `OffchainData` 型の削除
- sf009: デッドな `WhitelistInstruction` enum の削除
- R-sf006: `pubkey!` マクロで const Pubkey を定義
- R-sf013: `hash_suffix` を `strip_prefix` で明示的に処理
- R-nitpick019: legacy/v0.1.0 へのテスト依存を除去

プログラム修正（K5, R）:
- mf003: UpdateApproved* コンテキストに `admin_authority()` 制約を追加
- mf004: proof 長チェックを `> 4` から `== 4 + 256` に修正
- sf002: register_key の確認順序を最適化（Groth16 verify を最後に = CU 節約）
- sf003: parse_public_values を末尾まで読み切り、has_public_key を検証
- R-mf001: PDA close + 再登録の防止（不変条件の文書化 + CI grep）
- R-mf002: 手動 SIZE 計算を `#[derive(InitSpace)]` に置換
- R-mf004: ParsedPublicValues で `Vec<u8>` を `&[u8]` スライスに変更
- R-sf015: build.rs で VK ハッシュプレフィックスを事前計算
- R-nitpick021: ADMIN_AUTHORITY に `pubkey!` マクロを使用

SP1 guest 修正（K7）:
- mf001: `module_id` vs `instance_id` の命名をドキュメント上で統一
- mf002: 出力ファイル名生成を `with_extension` から `format!` に修正
- mf003: `tracing_subscriber` の初期化と経過時間表示を追加
- sf004-009: メモリ見積もりドキュメント、guest の expect メッセージ改善、入力サイズ上限、vkey メタデータ出力、cpu_setup の重複排除

**注意**: プログラム変更は `anchor build` + devnet 再デプロイが必要。デプロイ前にユーザー確認を取る。

**コミット**:
- `fix(solana): client mirror, dead code, pubkey macros`
- `fix(title-whitelist): proof length, InitSpace, admin constraints`
- `fix(sp1-guests): naming, filename gen, progress display`

### セッション 17e: コメント整理 + デッドコード（一括）

**優先度: P2** | 約 97 件（A: 65, B: 32 のうち前セッションで未対応分）

コメント整理（A）— 全 crate で排除するパターン:
1. 「ないもの列挙」（"v0.1.0 ではこうだったが..."）→ 削除
2. legacy/v0.1.0 への参照 → コード内コメントから削除（CHANGELOG にのみ残す）
3. タスク番号の言及（Task 13, Task 14...）→ 削除
4. 過剰な rationale / 「やらなかった理由」ブロック → 1 行に圧縮または削除
5. 全関数への機械的な `§X.Y` 貼り付け → 型に残し、メソッドからは除去
6. Cargo.toml の ASCII 装飾コメント → 圧縮

デッドコード（B）— 前セッションで対処されなかった残り:
- 外部参照ゼロの `pub` アイテム
- 重複 MockRuntime 実装（runtime/mock.rs に統合）
- 未使用のエラーバリアント
- 到達不能な match アーム

**コミット**: crate ごとに 1 コミット:
- `refactor(tee): remove verbose comments and dead code`
- `refactor(crypto): compress doc comments`
- `refactor(gateway): remove legacy references`
- 他

### セッション 17f: ドキュメント・仕様修正

**優先度: P1-P2** | 約 67 件（Q: 29, F: 24, S: 14）

SPECS_JA 修正（Q）:
- mf001: `signer` フィールドの型を統一（§2.3 の文字列 vs §3.2 のオブジェクト）
- mf002: E2EE セッションの意味論を文書化（永続化非サポート）
- mf003: §6.2 の「二段/三つ」矛盾を「三段の同一性確認」に統一
- mf004: measurement 範囲を PCR0 のみ → PCR0-PCR2 に拡大
- mf005: ETag/If-Match は defense-in-depth であり主要防御ではないと明記
- mf006: signature_hash のバイト範囲を厳密に定義
- mf007: Solana Extension の有効/無効メカニズムを定義
- mf008: 「Gateway は改変できない」→「改変は検知可能」に修正
- mf009: §1.7 と §2.4 の TEE 側検知限界の記述を整合
- sf001-013: 信頼前提の列挙、セクション参照追加、§2.4 のサブセクション分割、他

ドキュメント整合性（F）:
- mf001-007: 環境変数テーブル、sandbox 参照、GatewayAuth、OPERATIONS イメージ名、他

CHANGELOG/移行（S）:
- mf001: processor の利用可能状況を明確化（v0.1.2 では c2pa-verify のみ）
- mf002: TSA タイムスタンプ削除を文書化
- mf003: Removed セクションの拡充（未記載の 12 項目を追記）
- sf001-007: レガシー processor 参照、攻撃モデルのドキュメント、コストセクション、COVERAGE 但し書き

**コミット**:
- `docs(specs): fix type inconsistencies, measurement scope, signature_hash definition`
- `docs: update CHANGELOG removed section, fix OPERATIONS placeholders`

### セッション 17g: ビルド/デプロイ + テスト + OSS 成熟度

**優先度: P1-P3** | 約 67 件（E: 23, I: 24, H: 20, G デプロイ関連, J: 4）

再現性（E）:
- sha2_sp1 の git 依存をコミットハッシュに固定
- Dockerfile に proxy マニフェストを追加
- Terraform AMI の `most_recent=true` を修正
- .terraform.lock.hcl をコミット
- 全 Cargo.toml のバージョンをパッチレベルまで固定

セキュリティ/デプロイ（G）:
- debug-mode PCR0=all-zero を OPERATIONS に明確に文書化
- `--privileged` を `--device /dev/vsock` + seccomp プロファイルに置換
- proxy vsock 読み取りに長さ上限を追加（17b で対応済み）
- rate-limit の GC を追加（17c で対応済み）

テスト品質（I）:
- `rejects_invalid_bytes` の裸の `matches!` → `assert!(matches!(...))` に修正
- AEAD 改竄テストを追加
- sealed_channel の方向不一致テストを追加
- Nitro verifier のフィクスチャテストを追加
- devnet テストをエラーパス別に分割

OSS 成熟度（H）:
- CONTRIBUTING.md の Getting Started セクションを更新
- README に Quickstart セクションを追加
- README から「Implementation in progress」を削除
- LICENSE/NOTICE ファイルの確認・追加

実機検証（J）:
- Gateway の 400→502 ラッピング動作を文書化
- OPERATIONS に debug-mode の注意事項を記載

**コミット**:
- `fix(build): pin dependencies, fix Dockerfile, Terraform`
- `test: fix dead assertions, add tampering tests`
- `docs: update README, CONTRIBUTING for OSS readiness`

## 検証

各セッション完了後:
1. `cargo check --workspace`
2. `cargo test --workspace`
3. `cargo test --workspace --features title-tee/vendor-aws`（TEE 変更時）
4. `cargo clippy --workspace`（利用可能な場合）

タスク 17 全体の完了判定:
- 全 `cargo test` パス
- 暗号化リクエストのパススルーを手動検証（17c 後）
- Solana devnet テストのパス確認（17d 後、再デプロイした場合）

## 進捗

| セッション | 状態 | 対象件数 |
|---|---|---|
| 17a 暗号+attestation | done | 約 40 |
| 17b proxy+tee | done | 約 47 |
| 17c gateway+core | done | 約 42 |
| 17d solana+sp1 | done | 約 58 |
| 17e コメント+デッドコード | pending | 約 97 |
| 17f ドキュメント+仕様 | pending | 約 67 |
| 17g ビルド+テスト+OSS | pending | 約 67 |

### 17a 完了内訳

**K2 暗号 (`crates/crypto/`)**
- K2-mf001 P-256 ECDH: 手動スカラー乗算 → `p256::ecdh::diffie_hellman`
- K2-mf002 AEAD: `encrypt/decrypt` に `aad: &[u8]` を追加、`sealed_channel` で `suite_id` を AAD として束ねる(wrong_aad テスト追加)
- K2-mf003 ML-KEM: `rand::random()` (thread_rng) → `OsRng::fill_bytes`
- K2-mf004 P-256 keygen: `from_seed` 撤廃、`SecretKey::random(rng)` を使う `generate(rng)` に統一(X25519 も `random_from_rng` に揃え、`KeyBundle::generate` を簡素化)
- K2-nitpick-006 sealed_channel: `direction_keys` テストを explicit cross-direction assertion に修正

**K1 attestation (`crates/attestation*/`)**
- K1-mf001 cert chain: `Cert::verify(&Self)` に統一、root の自己署名チェックを削除(pin で信頼起点)
- K1-mf002 cert chain: `trusted_certs_len` 引数を完全削除(`verify_chain()` / `authenticate(timestamp)`)
- K1-mf003 doc: `digest != "SHA384"` を冒頭で拒否
- K1-mf004 COSE: `protected` ヘッダの未知キー(`crit` 等)を Err で拒否
- K1-mf005 signature: 独自 `ec_decode_sig` 撤廃、`Signature::from_der` / `from_slice` に分離(X.509 = DER, COSE = raw r||s)
- K1-mf006 RSA: `KeyAlgo::RSA` / `SigAlgo::Rsa*` 経路を完全削除、`rsa` crate dep も削除
- K1-mf007 sha2: 未使用の `oid` feature を削除(sha2 / sha2_sp1 両方)
- K1-sf001 doc: `lib.rs` の「pin は呼び出し側に委ねろ」コメントを実装に合わせ書き換え
- K1-sf002 time: `min(now, doc.ts/1000)` の暗黙折り畳みを削除、契約通り `now_unix_secs` をそのまま渡す
- K1-sf007 pad: `pad_zero_to_length` を `ec_decode_sig` と一緒に削除
- K1-sf008 typo: "unsupport" → "unsupported"
- K1-sf010 cose: protected.alg 不在を `Err` ではなく `Ok(false)` で扱い API 一貫性向上
- K1-sf011 oid: `oid` crate dep を削除、`Oid::new(Cow::Borrowed(...))` に統一
- K1-nitpick-003 origin: `lib.rs` の由来コメントを 1 行に圧縮
- K1-nitpick-004 test: `matches!(...)` を `assert!(matches!(...))` に、無価値な `vendor_tag_consistent` を削除
- K1-nitpick-005 doc: `CertChain.certs` を `///` doc comment に
- K1-nitpick-001 mock: `attestation/Cargo.toml` の mock feature コメントを 1 行に
- SP1 guest: `program/src/main.rs` を `authenticate(timestamp)` 新シグネチャに更新

**先送り(17b/17c 範囲)**
- K1-sf003 `AttestationError::Expired` variant(public API 変更)
- K1-sf004 `MockAttestationVerifier::PREFIX` の `pub(crate)` 化 + `build_mock_attestation` helper(solana/extension.rs 連動)
- K1-sf005 `MEASUREMENT = [0u8; 48]` を識別しやすい値に(gateway/tee/solana テスト連動)
- K1-sf006 `now_unix_secs` docstring 拡充
- K1-sf009 `CoseSign1::Deserialize` の可視性絞り込み

**検証**: `cargo test --workspace` 全グリーン(crypto 28/28, attestation-aws-nitro 2/2 含む実機 fixture、tee 100/100、gateway 41/41、solana 31/31、core 48/48)。SP1 guest は `cargo check --manifest-path sp1-guests/attestation-aws-nitro/host/Cargo.toml` でビルド確認済。

### 17b 完了内訳

**K6 proxy (`crates/proxy/`)**
- K6-mf001 chunked transfer: `CHUNKED_SENTINEL = u32::MAX` を導入。upstream の `Content-Length` 不明時(Transfer-Encoding: chunked 等)は `[u32 chunk_len][bytes]…[u32 0]` で送出。TEE 側 `proxy_fetcher.rs` も sentinel を読み取りループへ分岐。新テスト `chunked_get_uses_sentinel` 追加
- K6-mf002 OOM ガード: `protocol.rs` に `MAX_METHOD_BYTES=16`, `MAX_URL_BYTES=8KiB`, `MAX_REQUEST_BODY_BYTES=8MiB`, `MAX_RESPONSE_BYTES=100MiB` を定義し、`read_string`/`read_bytes` に `max_len` を強制
- K6-mf003 unsafe: `try_clone().expect()` を proper Err ハンドリングに、`VsockWriter` の `unsafe impl Send` に `// Safety:` 形式の論証コメント、`poll_shutdown` で `Shutdown::Write` を実発行
- K6-mf004 オーバーフロー: GET path で `content_length > MAX_RESPONSE_BYTES` を事前拒否、chunked path でも累積バイト数を `MAX_RESPONSE_BYTES` で打ち切り
- K6-mf005 shutdown: 全ての応答パスで `shutdown(Write)` を呼び half-close、protocol.rs doc に「一接続一リクエスト、二回目は未定義動作」を明記
- K6-sf001 backpressure: `tx.blocking_send` → `tx.try_send`、容量超過時は `tracing::warn!` で drop
- K6-sf002 observability: `duration_ms`, `upstream_host` をすべての info ログに追加、err は `source` チェーン込みで描画
- K6-sf003 `--privileged`: `deploy/aws/scripts/run-stack.sh` から削除、`--device /dev/vsock` のみに変更(⚠ EC2 再デプロイ時に AF_VSOCK bind 動作確認が必要)
- K6-sf004 timeouts: `REQUEST_TIMEOUT` を `PROXY_CONNECT_TIMEOUT_SECS`/`PROXY_REQUEST_TIMEOUT_SECS` env で個別調整可能化、デフォルト connect 10s / total 120s に短縮
- K6-sf005 Content-Type: POST に勝手に付ける `application/json` ヘッダを削除、reqwest デフォルトに委譲
- K6-nitpick-001 port: `LISTEN_PORT` を `PROXY_LISTEN_PORT` env から解決

**K3 tee (`crates/tee/`)**
- K3-mf001 起動シーケンス: `lib.rs` doc と `main.rs` のステップ番号を仕様 §5.2 に整合させ、self-attestation + registration-attestation を鍵生成直後(processor/pool/fetcher より前)に移動
- K3-mf003 unsafe: `FakeNsm` の `RefCell` + `unsafe impl Send/Sync` を `Mutex` に置換
- K3-mf004 timeout: TCP / vsock 両ブランチで `set_read_timeout` / `set_write_timeout` のエラーを `.ok()` ではなく Err で propagate
- K3-mf005 timeout hint: `compute_global_timeout(Option<u64>)` に変更、`None` で `MAX_GLOBAL_TIMEOUT` を割り当て。orchestrator は `Fragmented` 入力時に `fragment_urls.len() × MAX_FRAGMENT_SIZE` を hint、`Single`/`Sidecar` は `None`
- K3-mf006 Drop 順序: `NitroRuntime` の doc に「Arc 共有時は graceful shutdown で in-flight を待ってから drop」を追記
- K3-sf004 encryption pre-check: fetch 前に `encryption + !Single` を `EncryptionUnsupportedForInputType` で即拒否
- K3-sf006 offchain validation: `/extension/solana` で `MAX_OFFCHAIN_DATA_BYTES = 1 MiB` を強制、超過は 413
- K3-sf007 axum body limit: `/process` / `/extension/solana` に `DefaultBodyLimit::max(64 KiB)` をレイヤ適用
- K3-sf009 NSM zero-bytes: `GetRandom` が空応答を返した場合のループ無限化を `RandomFailed` で防御
- K3-sf010 nsm_exit log: `RealNsm::drop` で `tracing::debug!` を残す
- K3-sf012 measurement 型: `expected_measurement` を `Vec<u8>` から `Box<[u8]>` に、起動ログに `tee_type` と `measurement_len` を追加
- K3-sf013 octet-stream warn: `detect_content_type` のフォールバック時に `tracing::warn!` で URL を記録
- K3-nitpick-011 `hex_short`: 自作関数を削除、`hex::encode(&[..8])` に置き換え

**先送り(17e 範囲)**
- K3-mf002 `process_request` 関数分割: 17 README で除外項目に指定済(scope 外)
- K3-sf002 漸進予約 streaming fetcher: 同上
- K3-sf008 `data_size_hint` Option 化: 17b で K3-mf005 と一緒に対応済
- K3-sf003/sf005/sf011/sf014 + K3-nitpick-001..010: コメント整理・MockRuntime 3 重実装統合は 17e でまとめて対応
- K6-sf006/sf007 spec 記述 + K6-nitpick-002..004 + protocol.rs doc 統一: 17f で対応

**検証**: `cargo test --workspace` 全グリーン(proxy 5/5 含む新規 chunked テスト、tee 101/101、gateway 41/41 + 8/8 e2e、attestation-aws-nitro 2/2、crypto 28/28、solana 31/31、core 48/48)。

**⚠ 実機確認必須**: `deploy/aws/scripts/run-stack.sh` の `--privileged` 削除は EC2 上で `--device /dev/vsock` のみで title-proxy が起動するか次回再デプロイ時に確認すること。失敗時は `--privileged` を一旦戻す。

### 17c 完了内訳

**K4 gateway (`crates/gateway/`)**
- K4-mf001 encrypted response transparency: `TeeClient::process` を `Result<ProcessOutcome, _>` (`Plaintext(ProcessResponse)` | `Encrypted(Vec<u8>)`) に変更。`HttpTeeClient::process` が `Content-Type` を見て分岐、`/process` ハンドラは `application/octet-stream` をバイト透過する `Response` を返す
- K4-mf002 body limit: `/process` と `/extension/solana` に `DefaultBodyLimit::max(64 KiB)` をレイヤ適用
- K4-mf003 middleware comment: layer ordering の説明を axum/tower semantics と実行順序の両方を明示する形に書き直し
- K4-mf004 status mapping: TEE upstream の 503→`TeeUnavailable`、429→`RateLimited`、4xx→新規 `TeeRejected{status}`(透過)、その他→`TeeError`。`tee_err` で upstream body は warn ログのみに留めクライアントには露出しない
- K4-mf005 auth UTF-8: `parse_auth_header` を `Missing` / `Bearer(_)` / `Malformed` を区別する enum に。malformed は `Unauthorized("Malformed Authorization header")` で 401、rate_limit は anonymous バケットに集約
- K4-sf001 bucket GC: `RateLimiter::prune_idle(Duration)` を追加し、`server::run` で 5 分おきに `window × 10` を idle 閾値として実行
- K4-sf002 Mutex poison: `buckets.lock()` を `unwrap_or_else(|e| e.into_inner())` で defensive リカバリ
- K4-sf003 reqwest tuning: `connect_timeout(5s)` / `pool_max_idle_per_host(16)` / `tcp_keepalive(60s)` を `HttpTeeClient::new` に追加
- K4-sf004 ticker interval: `spawn_health_check` を `tokio::time::interval` + `MissedTickBehavior::Delay` に。最初の即時 tick は skip
- K4-sf005 fail-safe refresh: `check_and_refresh` の `keys()` 失敗時を `false`(無視)から `true`(強制 refresh)に変更し、stale キーの serving を防ぐ
- K4-sf006 atomic cache swap: `refresh_tee_info` をローカル `TeeInfoCache` 組み立て → `*self.tee_cache.write() = new` に変更し、部分失敗で半端な cache が見える状態を解消
- K4-sf007 `Default` 削除: `GatewayConfig::Default` を撤廃、production の誤起動経路を断つ
- K4-sf008 hot loop: `spawn_health_check` で `interval_secs.max(1)` を強制
- K4-nitpick-003: `ApiKeySet::contains` の長すぎる "constant-time" コメントを実態に合わせた短い形に書き直し

**K8 core (`crates/core/`)**
- K8-mf001 dead public API: `processor_outputs.rs` を削除(`ProvenanceGraphOutput`/`GraphNode`/`GraphEdge`/`ImagePdqOutput`/`VideoVpdqOutput`/`FrameHash`/`CertVerifyOutput`/`CertChainEntry` は使われていない予示型)。残す `C2paVerifyOutput`/`SignerInfo`/`C2paAction` は `c2pa_verify.rs` に同居させる
- K8-mf002 dead error type: `error.rs` を削除(`CoreError` は使われていない)、`ProcessorError` に統一
- K8-mf003 visibility: `extract_signature_from_jumbf` を `pub` → `pub(crate)` に
- K8-mf004 serde flatten guard: `ProcessorOutput::ok` で `data.is_object()` を強制、非オブジェクトは `error()` に振り替えて wire 形式を保護
- K8-sf003 read_so_far: `if-as` precedence の罠を明示的に `label_bytes` 変数で展開
- K8-sf004 ASCII guard: `read_desc_info` のラベル byte 読み込みで `is_ascii()` をチェック、非 ASCII は `C2paVerificationFailed` で reject
- K8-sf005 MAX_SIGNATURE_SIZE: 16 MiB → 256 KiB に縮小(現実的な COSE 署名 + 証明書チェーン上限)
- K8-sf006 read_header: `Ok(BoxHeader { size:0, type:0 })` のセンチネルを廃止し `Result<Option<BoxHeader>>` に。truncated header はエラーで明示
- K8-sf007 active_label: doc コメントに C2PA 2.1 §13.4 の出典を追加
- K8-sf008 module visibility: `c2pa_verify`/`processor`/`request`/`response`/`jumbf` を `pub` → 私有モジュール+トップレベル `pub use` flat 再エクスポートに統一
- K8-sf009 ProcessorError Clone: `#[derive(Clone)]` を本体側で宣言、テスト内の手書き impl を削除

**先送り(17e 範囲)**
- K8-mf005 c2pa Reader 重複: orchestrator まで巻き込む API 変更が必要なため別セッションで対応
- K8-sf001 ProcessorRegistry::execute 並列化: 仕様 §3.1 と実装の整合は spec 側で再検討(17f)
- K8-sf002 ProcessRequest 型レベル不変条件: orchestrator 側の pre-check(17b-3 で対応済み)で実用上は十分
- K8-sf010 image dev-dep: テスト fixture 化は scope 大、17g build/test セッションへ
- K8-nitpick-001..007: 17e で一括対応

**検証**: `cargo test --workspace` 全グリーン(core 39/39 — dead code 削除で 48→39、gateway 43/43 + 8/8 e2e — prune_idle テスト追加で 41→43、その他は不変)。

### 17d 完了内訳

**K7 SP1 guest + host (`sp1-guests/attestation-aws-nitro/`)**
- K7-01 doc 整合: guest doc コメントで `module_id` → `instance_id`(vendor-neutral)に統一、実コードの commit 内容(AWS Nitro の `doc.module_id` を vendor-neutral スロットに入れる)を 1 行注記で説明。on-chain parser 側コメントと整合
- K7-02 出力ファイル名: `prove.rs` で `Path::with_extension` を `format!` ベースに置換。`nitro.v1.bin` 等の複合拡張子で意味的部分が失われない
- K7-03 進捗ログ: `sp1_sdk::utils::setup_logger()` を `prove.rs::main` 冒頭で呼び、`info!` レベルの SP1 進捗バーが表示されるように
- K7-04 メモリ見積もり: `long_about` を「~90 minutes, ~30 GiB peak, r5.4xlarge 以上推奨」に書き換え
- K7-07 doc サイズ上限: `prove.rs` に `MAX_DOC_BYTES = 16 KiB` ガードを `fs::read` 直後に追加。guest 側にも同じ assert を入れて zkVM cycle 爆発を防止
- K7-08 vkey metadata: `vkey.rs` で stderr に guest CARGO_PKG_VERSION + Unix エポック秒を出力、stdout は hex のまま(機械可読維持)
- K7-10 doc: `program/src/main.rs` の「once per TEE instance」を「runs once when a signer key is registered on-chain」に
- K7-11 `_cert_chain`: `let _ = report.authenticate(...).expect(...);` の戻り値捨て idiom に変更
- K7-06 cert prefix: `trusted_certs_prefix_len` 引数自体は 17a で削除済み。`authenticate()` がパラメータを取らないので physical guard が成立

**先送り(後フォローアップ)**
- K7-05 dry-run: `--dry-run` フラグでの `execute()` early-failure
- K7-09 ProverHandle: vkey + prove を共有 setup で 1 回化
- K7-12 SP1 SDK pin: `=6.2.x` への strict pin(現在は `"6.2"` で minor 範囲)。SP1 SDK 6.3+ がリリースされると vkey 不一致リスクあり

**K5 client (`crates/solana/`)**
- K5-mf001 mirror struct Borsh 整合: `StoredMeasurement { bytes: [u8; 64], len: u8 }` を client にも導入、`WhitelistEntry.measurement` を `Vec<u8>` から `StoredMeasurement` に置換。SIZE 計算式も on-chain layout 1:1 に
- K5-nitpick-002 / R-006 pubkey! const: `whitelist_program_id` / `spl_account_compression_v2_id` を `solana_sdk::pubkey!` 経由の `pub const` に書き換え、関数は thin wrapper に
- K5-sf005 mint TX compute budget: `build_and_sign_mint_tx` 冒頭に `ComputeBudgetInstruction::set_compute_unit_limit(400_000 or 250_000)` を追加(collection 有無で分岐)
- K5-sf008 OffchainData 削除: 使われていない dead struct を撤廃
- K5-sf009 WhitelistInstruction 削除: serde JSON enum で wire 互換性のない dead code、Anchor IDL or 直接 ix builder で代替予定なので一旦削除
- K5-sf013 sign_transaction: `for` ループを `iter().take(num_signers).position(...)` に書き換え、意図が明示的に
- K5-nitpick-003 hash_suffix: `signature_hash.strip_prefix("sha256:").unwrap_or(...)` の安全な切り出しに
- K5-nitpick-007 hex_encode 自作削除: `hex` crate を `[dependencies]` に昇格して `hex::encode(bytes)` に統一
- mpl-bubblegum pin tighten: `"2.0"`(2.x 全部) → `"~2.1"`(2.1.x 限定)で `MetadataArgsV2` の field shape 変更を回避

**先送り(scope 外 / 17e)**
- K5-mf002 KEY_EXPIRY_SECONDS 共有: client crate が program crate を `no-entrypoint` 依存できる構造改修が必要、後フォローアップ
- K5-sf004 process_extension whitelist check: TEE 起動時 register 状態を internal state に持つ大規模変更、scope 外
- K5-sf006 rent_exempt RPC: client/テスト側の test runner 整備、後フォローアップ
- K5-sf007 PDA bump cache + R-005 OnceLock: 性能最適化、必須でない
- K5-sf010..012 + K5-nitpick-001/004..006: devnet テスト品質改善、17e/17g
- R-014 solana-sdk version pinning: workspace 全体の依存整理、17g

**K5/R program (`programs/title-whitelist/`)** — ⚠ devnet 再デプロイが必要
- K5-mf003 admin double check + R-007 admin rotation note: `Update*` accounts に `constraint = admin.key() == ADMIN_AUTHORITY` を併記、`ADMIN_AUTHORITY` の doc に Phase 2 migration plan を明記、未使用 `admin_authority()` 関数を削除
- K5-mf004 proof length strict: `proof.len() > 4` → `proof.len() == 4 + 256` の厳密チェック + 新規 `InvalidProofLength` error variant
- K5-sf002 register_key 順序入替: cheap → expensive で並べ替え(vkey allowlist → parse → measurement allowlist → bind check → Groth16 verify)、不正入力での 250k CU 浪費を防止
- K5-sf003 parse_public_values 末尾検証: `has_public_key` + (optional) `public_key_hash` を読み切り、`require!(data.len() == offset, ...)` で末尾余剰バイトを reject。`has_public_key` の canonical 0/1 validation も追加
- R-021 pubkey! const: `ADMIN_AUTHORITY: [u8; 32]` → `Pubkey::new_from_array(...)` の `pub const Pubkey` に。`admin.key() == ADMIN_AUTHORITY` の直接比較に統一、`admin_authority()` 関数を削除

**先送り(影響大 or scope 外)**
- K5-sf001 revoke 未登録 PDA: 現状 Anchor `AccountNotInitialized (3012)` で機能的には正しく拒否される、文言改善のみ defer
- R-001 register init guard: `WhitelistRegistryHead` PDA 追加が必要、構造的設計変更で scope 外
- R-002 `#[derive(InitSpace)]`: Anchor 0.30 → 0.31 移行と組み合わせると効率良いので、別タスク
- R-004 `Vec<u8>` → slice 化: `ParsedPublicValues` ライフタイム化、効率改善だが現状動作に影響なし、defer
- R-009 revoke instruction seeds: CU 削減リファクタ、defer
- R-015 vk hash build-time precompute: build.rs 追加、defer

**devnet 再デプロイ必要事項**(ユーザー判断)
- Solana program ID `43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs` は不変、`anchor upgrade --program-id 43y8E... --provider.cluster devnet` で in-place アップグレード
- Admin keypair: `keys/admin.json`(`wrVwsTuRzbsDutybqqpf9tBE7JUqRPYzJ3iPUgcFmna`)
- 既存の ApprovedVkeys / ApprovedMeasurements / WhitelistEntry PDA はアカウントレイアウトに後方互換あるため再 init 不要
- 再デプロイ後、devnet テスト `cargo test --test devnet_whitelist` で `register_key` フローが新しい順序+strict proof length チェックを通過することを確認

**検証**: `cargo test --workspace` 全グリーン(solana 31/31、proxy 5、tee 101、gateway 43 + 8 e2e、core 39、attestation-aws-nitro 2、crypto 28)。SP1 host + guest も `cargo check` 通過。program は `cargo check --no-default-features` で warning 19 件(Anchor cfg ノイズ、機能影響なし)、error 0 件。
