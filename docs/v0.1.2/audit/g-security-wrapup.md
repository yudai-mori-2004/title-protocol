# G. セキュリティ最終確認

## 概要

過去 3 回のセキュリティ監査（k1 attestation / k2 crypto / k4 gateway / k6 proxy / k7 sp1-guests）で指摘された Critical / High はおおむね対応済み。本ラウンドでは (1) 過去指摘の残存チェックリスト、(2) 主要セキュリティ経路の独立再読、(3) 大監査の他観点（再現性／OSS 成熟度）で浮上した新規攻撃面 — の三点を統合的に確認する。

担当範囲は指示通り:

- `crates/attestation-aws-nitro/`（COSE / cert chain / root pin）
- `crates/solana/src/extension.rs`（attestation binding）
- `programs/title-whitelist/src/lib.rs`（四段確認 + on-chain 検証）
- `crates/tee/src/{main,orchestrator,proxy_fetcher}.rs`
- `crates/proxy/`
- `crates/crypto/`
- `crates/gateway/src/{auth,rate_limit}.rs`
- `sp1-guests/attestation-aws-nitro/`
- `deploy/aws/`（新規攻撃面）

結論を先に述べる: **コア信頼モデル（Attestation 検証 → ZK proof → on-chain whitelist）は意図通り組み上がっており、production-grade に十分耐えうる**。一方で AWS Nitro 上のデプロイ層に Critical / High の運用問題が複数残っており、これらを潰すまでは「本番稼働可」と言えない。

## 重大度別内訳

- Critical: **2 件**（C-1, C-2）
- High: **2 件**（H-1, H-2）
- Medium: **5 件**（M-1〜M-5）
- Low: **4 件**（L-1〜L-4）

## 過去指摘の残存確認チェックリスト

| # | 指摘内容 | 状態 | 根拠 |
|---|---|---|---|
| 1 | SP1 guest の `trusted_certs_prefix_len` を guest 入力から削除 | [x] 解消 | `sp1-guests/attestation-aws-nitro/program/src/main.rs:49-51` で `report.authenticate(0, …)` とハードコード。コメント L27-30 で削除理由を明文化 |
| 2 | AWS Nitro root CA SHA-256 ピン照合 | [x] 解消 | `crates/attestation-aws-nitro/src/constants.rs:26-29` に 32 byte ハッシュ、`doc.rs:72-80` で `root.digest() != AWS_NITRO_ROOT_CA_SHA256` を fail-close |
| 3 | `WhitelistEntry.measurement` の vendor-neutral 化（`StoredMeasurement`） | [x] 解消 | `programs/title-whitelist/src/lib.rs:411-445`：固定長 `[u8;64] + u8 len`、`as_slice()` で実体だけを参照 |
| 4 | `StoredMeasurement` のサイズ検証 | [x] 解消 | `lib.rs:120, 144, 356` の 3 箇所で `(1..=MAX_MEASUREMENT_LEN).contains(&len)` |
| 5 | `ApprovedVkeys` + `ApprovedMeasurements` allowlist で register_key が照合 | [x] 解消 | `register_key` 内 Step 1 (L183-186) と Step 4 (L194-203) で順序通り照合 |
| 6 | `delete_key` → `revoke_key` で PDA を close せず flag を立てる | [x] 解消 | `revoke_key` (L244-252) は `entry.revoked = true` のみ。コメント L240-243 で再投入攻撃を明示 |
| 7 | `sign.rs` の 5 つの unwrap → `?` 変換 | [x] 解消 | `sign.rs` 全域でエラー伝播が `anyhow::Error` 化。unwrap は残っていない |
| 8 | KeyBundle / Solana 鍵が NSM RNG seed 経由 | [x] 解消 | `tee/src/main.rs:91-99` で `tee_seeded_rng(runtime, …)` 経由。コメント L86-89 で OsRng 経路を意図的に避けた旨を明示 |
| 9 | self-attestation 失敗で起動 fatal | [x] 解消 | `tee/src/main.rs:152-167` で失敗時に `?` でプロセス終了 |
| 10 | `ApiKeySet::contains` が constant-time | [x] 部分解消 | `gateway/src/auth.rs:98-116` で XOR accumulator + branchless `is_zero`。ただし長さ不一致は `continue` するため、長さ枝の存在自体は依然タイミングでリーク（コメント L96-97 で受容を明示）。コメント L91-93 の「fixed zero buffer」記述は実装と不一致 → **L-3** |
| 11 | rate-limit middleware が auth 独立 | [x] 解消 | `gateway/src/server.rs:88-94` で rate_limit を最外層、`rate_limit.rs:99-105` で `API_KEYS` 空でも匿名バケット適用 |
| 12 | wire suite vs declared suite 突合 | [x] 解消 | `orchestrator.rs:276-281` で `opened.suite != suite` を fail-close (`EncryptionSuiteMismatch`) |
| 13 | content_fetch の timeout + body size cap | [x] 解消 | `tee/src/content_fetch.rs` で `FETCH_TIMEOUT` / `CONNECT_TIMEOUT` / `max_body_bytes` を運用。`proxy_fetcher.rs:132-141` も同様に header の body_len を読んだ瞬間にキャップ |
| 14 | `default = []` for title-tee | [x] 解消 | `crates/tee/Cargo.toml` の `[features]` で `default = []` を明示、コメントで本番ビルドが mock を選べないようにしている根拠を記述 |

→ 過去指摘 14 項目すべて、コードレベルでは解消。L-3 のみドキュメント文言の不整合が残る。

---

## 発見（新規）

### C-1（Critical）`run-stack.sh` が `nitro-cli run-enclave --debug-mode` で Enclave を起動している

- 場所: `deploy/aws/scripts/run-stack.sh:54`
- 内容: `nitro-cli run-enclave … --debug-mode` を無条件で実行している
- 攻撃シナリオ:
  - `--debug-mode` で起動した Nitro Enclave は **PCR0 / PCR1 / PCR2 が全 0** を返す（AWS Nitro の仕様）
  - その結果、本番運用で正規 measurement を `ApprovedMeasurements` に登録しても、debug-mode Enclave からは「全 0 measurement」しか出てこないため、両者は永久に不一致 → register_key が常に失敗
  - 万一 admin が「動かすために全 0 measurement を allowlist に追加」してしまうと、**誰でも自分の AWS アカウントで debug-mode Enclave を立て、任意のコードで生成した Attestation Document を on-chain で承認させられる**（mock 攻撃と等価）
  - すでに `MockAttestationVerifier::MEASUREMENT = [0u8; 48]` がコード上に存在する（`crates/attestation/src/lib.rs:114`）ため、mock 値と debug-mode 値が衝突する点も悪い
- 修正案:
  - **削除**: `--debug-mode` フラグを外す
  - スクリプトに `RELEASE=1` などの環境変数を導入し、`if [[ -z "${RELEASE:-}" ]]; then DEBUG_MODE="--debug-mode"; fi` のように切替式にする
  - 加えて README の "experimental, not a hardened production blueprint" の文言だけでは弱いので、`docs/v0.1.2/OPERATIONS_JA.md` で「`--debug-mode` のまま on-chain register_key を打ってはならない」を太字で警告
- 重大度根拠: production AWS Nitro 上での稼働可否判定における核心。これを残したまま `register_key` を流すと信頼モデルが空洞化する

### C-2（Critical）`title-proxy` コンテナが `--privileged + --network host` で動作している

- 場所: `deploy/aws/scripts/run-stack.sh:43-47`
- 内容: `docker run -d --name title-proxy --network host --privileged title-protocol-proxy:latest`
- 攻撃シナリオ:
  - `--privileged` は全 capability + デバイス全アクセス + AppArmor / Seccomp 無効化。コンテナ脱出を実質防がない
  - `--network host` でホストの全ネットワーク名前空間を共有 → コンテナ内から `127.0.0.1:4000`（TEE inbound bridge）や `127.0.0.1:3000`（Gateway）にも到達可能
  - title-proxy コンテナイメージ内に既知 CVE のあるバイナリが含まれた場合、proxy 経由で `(method, url, body)` を制御できる攻撃者は、proxy プロセスを乗っ取って host ネットワークから socat bridge を経由して TEE に直接 HTTP 投入できる
- コメント (`run-stack.sh:38-42`) は「AF_VSOCK socket bind() は capability check で seccomp profile に阻まれるから `--privileged` が必要」と説明するが、これは不正確:
  - 必要なのは `CAP_NET_ADMIN` + `--device=/dev/vsock` のみ。`--privileged` は過剰
- 修正案:
  - **書き直し**: `--privileged` を `--cap-add=NET_ADMIN --device=/dev/vsock`（または vsock module ロード + `--cap-add=NET_BIND_SERVICE`）に絞る
  - `--network host` を外し、`vsock` だけが必要なら `--network none` でも動くはず（vsock は netns に縛られない）
  - run-stack.sh の justification コメントを書き直し
- 重大度根拠: production stack で title-proxy が compromised された瞬間に attacker は host ネットワーク全体を得る。最小権限原則に反する

### H-1（High）`title-proxy` の vsock listen が認証なし + length cap なし

- 場所:
  - `crates/proxy/src/main.rs:27` `vsock::VsockListener::bind_with_cid_port(VMADDR_CID_ANY, LISTEN_PORT)`
  - `crates/proxy/src/protocol.rs:43-46, 53-56, 73-77, 82-85` `vec![0u8; len]` で u32 length をそのまま `Vec` に
- 攻撃シナリオ:
  - `VMADDR_CID_ANY` は「任意 CID からの接続を受ける」設定。AWS Nitro 上で他に enclave を同居させる運用はまず無いが、host 上の任意プロセス（攻撃者が EC2 ホストに足場を得た場合）が `vsock://3:8000` に接続して `(method, url, body)` を送れる
  - `read_string_async` / `read_bytes_async` は u32 length をそのまま `vec![0u8; len]` に渡すので、攻撃者が `len = u32::MAX (4 GiB)` を送れば proxy 側で即座に **4 GiB allocation を試みる → OOM-kill**
  - これは `proxy_fetcher.rs:131-141` のクライアント側 cap（`max_body_bytes`）とは独立。proxy 側で同じ防御が無い
- 修正案:
  - `protocol.rs` に `MAX_METHOD_LEN = 16`, `MAX_URL_LEN = 8192`, `MAX_BODY_LEN = 100 * 1024 * 1024` 等の cap を導入。`read_u32_*` 直後で `len > MAX_*` ならエラー
  - 認証を導入したい場合は事前共有鍵で HMAC-SHA256 を request 先頭に付け、proxy 側で照合
- 重大度根拠: 隣接攻撃面（host 上のローカル攻撃者）に対する DoS。production deployment では同居 enclave がないため exploitability は低いが、深層防御として High

### H-2（High）Gateway `RateLimiter` のバケット HashMap が無制限に増える

- 場所: `crates/gateway/src/rate_limit.rs:33-37, 61-68`
- 内容: `buckets: Mutex<HashMap<String, TokenBucket>>` を identity（API key or `__anonymous__`）でキー付け。**エントリの上限なし、stale entry の eviction なし**
- 攻撃シナリオ:
  - 攻撃者が `Authorization: Bearer xxxx-<rand>` を毎リクエスト変えて Gateway を叩く
  - 各リクエストで新しい identity が登録され、HashMap が無限に成長 → Gateway プロセスの RSS が線形に増え、最終的に OOM-kill
  - 1 リクエストあたり ~100 byte 程度（String key + TokenBucket）なので 10 M リクエストで ~1 GB
- 修正案:
  - 容量上限（例: 100_000 buckets）を設け、超過時は LRU で eviction
  - もしくは `last_refill` が `window_secs * 2` より古いバケットを定期削除する background task
  - 最も簡単な代替: `governor` crate に置き換える（quota + LRU 内蔵）
- 重大度根拠: 認証不要で Gateway を不可用化できる。実運用で API_KEYS 設定済みでも、`Authorization` header をパースする前に rate_limit middleware が走るため、未登録キーも HashMap に入る

### M-1（Medium）TEE Enclave の Solana 鍵 / KeyBundle に zeroize なし

- 場所: `crates/crypto/src/key_bundle.rs:25-29`, `crates/solana/src/signing_key.rs`
- 内容: 秘密鍵を保持する struct に `ZeroizeOnDrop` が無く、Drop 時もメモリは残る
- 攻撃シナリオ: Nitro Enclave 内部は外から観測不能なので exploitability は低い。ただし enclave クラッシュ時にメモリダンプが NSM 経由で取れる経路があれば残骸が見える可能性
- 修正案:
  - `zeroize = { version = "1", features = ["derive"] }` を追加し、`X25519Decapsulator` / `P256Decapsulator` / `MlKem768Decapsulator` / `SolanaSigningKey` に `ZeroizeOnDrop` derive
- 重大度根拠: 深層防御

### M-2（Medium）TEE の Response 暗号化に `OsRng` を使っている

- 場所: `crates/crypto/src/sealed_channel.rs:40-41`, `:67-68`
- 内容: AES-256-GCM の nonce 生成で `rand::rngs::OsRng`。`tee/src/main.rs` の startup で NSM RNG を使い分けているのに、ランタイム呼び出しは OsRng
- 評価: Nitro Enclave 内の `/dev/urandom` は NSM-seeded なので暗号学的には問題ない。ただし「entropy は全部 NSM 経由」というポリシーが startup と runtime で割れているのは一貫性に欠ける
- 修正案:
  - `ResponseChannel::seal` に nonce 生成用の `&mut dyn RngCore` を渡す API に変更し、orchestrator 側で `TeeRuntime::random_bytes` から作った RNG を注入
  - もしくは `tee/src/main.rs:208-227` の `tee_seeded_rng` を `OnceCell<Mutex<ChaCha20Rng>>` で global 化し、暗号モジュールから参照
- 重大度根拠: ポリシー一貫性問題。直接的な脆弱性ではない

### M-3（Medium）`parse_public_values` が tail bytes を許容（trailing data 無視）

- 場所: `programs/title-whitelist/src/lib.rs:327-391`
- 内容: 仕様上は `user_data_hash` の後に `has_public_key + public_key_hash` が続くはずだが、parser はそこまで読まずに早期 return する。trailing bytes をエラーにしない
- 評価: SP1 proof は public_values 全体の SHA-256 commit があるため、trailing bytes の追加は proof との不整合となり受理されない（実質的には安全）。しかし parser として「読み残し」を許す設計は将来 layout 変更時にバグを生みやすい
- 修正案:
  - `has_public_key` 以降も読んで、最終 offset == data.len() を要求するか、明示的に「trailing bytes は無視する」とコメントで宣言
- 重大度根拠: 直接攻撃可能ではないが、フォーマット契約の曖昧さを残している

### M-4（Medium）proxy の `forward_http_streaming` がメソッドを GET/POST に限定するが、`PUT/DELETE/PATCH` 等の処理は 400 でクライアントが切断しないと無限待機しうる

- 場所: `crates/proxy/src/handler.rs:51-56`
- 内容: 未対応メソッドは 400 を返して `Ok(())` で抜けるが、`write_error` 後に socket がそのまま放置される。クライアント（TEE）側は `read_exact` で次のデータを待ち続けるとブロックする可能性
- 評価: 現状の `proxy_fetcher.rs` は status + body_len + body だけを読んで close を期待しているので致命的ではないが、proxy 側で明示的に shutdown するのが安全
- 修正案:
  - `write_error` の最後で `w.shutdown().await` を呼ぶ
  - もしくは TCP connection を drop してクライアントに EOF を伝える
- 重大度根拠: 接続リーク → 緩慢な resource 枯渇

### M-5（Medium）`MockAttestationVerifier::MEASUREMENT = [0u8; 48]` と Nitro `--debug-mode` の PCR0 が完全に一致する

- 場所: `crates/attestation/src/lib.rs:114` + `deploy/aws/scripts/run-stack.sh:54`
- 内容: mock の measurement 値が、`--debug-mode` で起動した Nitro Enclave の PCR0（仕様上 all-zero）と区別がつかない
- 攻撃シナリオ: C-1 を補強する形。`ApprovedMeasurements` に「テスト用」と思って `[0u8;48]` を入れた瞬間、mock vendor と Nitro debug-mode vendor の両方が通る
- 修正案:
  - mock の MEASUREMENT を `[0xAA; 48]` 等の明らかに本番では現れない値にする
  - もしくは `MockAttestationVerifier` の verify 内で `vendor: "mock"` を返し、whitelist program 側で vendor タグを検証する余地を作る（ただし現行 ZK proof パブリック出力に vendor は含まれていない）
- 重大度根拠: 単独では C-1 を悪化させる材料

### L-1（Low）`extract_api_key` が `Bearer ` のスペース後を厳密に検証しない

- 場所: `crates/gateway/src/auth.rs:21-27`
- 内容: `.strip_prefix("Bearer ")` は仕様通りだが、`Bearer  xxx`（連続スペース）等の準拠外フォーマットを拒否しないため、攻撃にはならないが warn ログが出ない
- 修正案: 受容するキーの空白を `trim()` で正規化

### L-2（Low）`tee/src/main.rs:201-205` の `shutdown_signal` が `expect` で panic する

- 場所: `crates/tee/src/main.rs:201-205`
- 内容: `tokio::signal::ctrl_c().await.expect("Failed to install CTRL+C signal handler")` — Nitro Enclave 内で signal handler 登録に失敗するとプロセスが panic で死ぬ
- 評価: enclave 内では signal はそもそも来ないので実害なし。ただし panic で死ぬのは startup の `?` 伝播と一貫性がない
- 修正案: `expect` → `unwrap_or_else(|_| { tracing::error!(…); })` で graceful degrade

### L-3（Low）`ApiKeySet::contains` のドキュメント文言が実装と不一致

- 場所: `crates/gateway/src/auth.rs:86-97`
- 内容: コメントは「Length-mismatched entries still consume a constant number of comparisons against a fixed zero buffer」と書くが、実装 (L103-105) は `continue` で枝刈り。実装は意図的（高エントロピ固定長 token を前提）だがドキュメントを書き直すべき
- 修正案: コメント L90-93 を実装に合わせて書き直し（「length mismatch ではスキップする — 高エントロピ固定長 token 前提」）

### L-4（Low）`run-stack.sh` が API_KEYS を docker run の `-e` で渡している

- 場所: `deploy/aws/scripts/run-stack.sh:73-78`
- 内容: `docker run -d -e API_KEYS="$API_KEYS"` で渡すと EC2 ホスト内の他プロセスから `cat /proc/<pid>/environ` 等で読まれうる
- 評価: EC2 ホストは攻撃者が root を取った時点で全部見えるので、現状のシングルテナント運用では実質変わらない
- 修正案: docker secrets / docker-compose env_file への分離。production を想定するなら AWS Secrets Manager 統合が筋

---

## 結論

**本番 AWS Nitro 上での稼働可否: No**

コア信頼モデル（Attestation 検証 / SP1 guest / on-chain whitelist の四段確認 / 鍵バインド / vendor-neutral measurement）は仕様通り組み上がっており、`crates/` 配下のコードは production-grade に到達している。過去 14 項目の指摘もすべて解消済み。

ただし `deploy/aws/` 配下に Critical 2 件（**C-1: `--debug-mode` 無条件起動 / C-2: `--privileged + host-network` proxy**）と High 2 件（**H-1: proxy 入力に length cap なし / H-2: rate-limit HashMap unbounded**）が残っており、これらは production EC2 上で実際に動作するリスクを直接生む。

最低限、以下 4 つを潰してから本番判定すること:

1. `run-stack.sh` の `--debug-mode` を本番モードに切替可能なフラグへ
2. `title-proxy` を `--privileged + --network host` から最小権限へ
3. `crates/proxy/src/protocol.rs` の `read_*_async` に length cap
4. `crates/gateway/src/rate_limit.rs` の bucket HashMap に上限 + LRU

これらは全て deploy / operational layer の問題であり、core protocol 層には触らずに修正できる。修正タスクは 17（fix-up タスク）で吸収するのが妥当。
