# J. 実機ランタイム検証 — Round 2

## 環境

- Gateway public endpoint: `http://35.78.89.141:3000`
- 検証時刻: 2026-05-24T09:30Z – 09:35Z (UTC)
- スタック状態の差分 (Round 1 → Round 2):
  - **Enclave ID 変化**: `module_id = i-00b3f9b3607e019c2-enc019e**5900c3305813**` (Round 1: `enc019e**588e03f2febe**`)。
    EC2 インスタンス ID は同一だが、Enclave は再起動されている (ship-and-run が間で発生)。
  - **PCR0 が non-zero に**: `5f3722ef2ba1d533d885fd39bdbf798ce63cad263df7b65208a701af79fd7bd5ef5966df3cdecd6f8d00249acb97d46a` (48 B)。Round 1 は all-zero (debug-mode) だった。**リリース build に切り替わっている** ことが確認できる。PCR1/PCR2/PCR4 も non-zero、PCR3/PCR5–PCR15 は仕様通り all-zero (未使用枠)。
  - **`/solana-keys` のスキーマ拡張**: Round 1 は `{"solana_pubkey":"..."}` のみ。Round 2 では `{"solana_pubkey":"...", "registration_attestation_b64":"<CBOR/COSE_Sign1 Attestation Document>"}` を返す。`user_data = sha256(solana_pubkey_or_similar) = 9ec2b017...` で **Solana 鍵への bind が attestation 内 user_data で達成されている**。
  - **`/extension/solana` のリクエストスキーマ拡張**: 必須フィールドに `recent_blockhash` が追加された。Round 1 のテストペイロード形式では即 422 になる。
  - **`POST /process` 経路の壊死 (リグレッション)**: 後述、すべての TEE 内 fetch が `vsock://3:8000: vsock connect failed: Connection reset by peer (os error 104)` で失敗。
- 監査セッションのサンドボックス制約: Round 1 と同じく `ssh` 実行が permission denied で一律遮断。Round 1 で SKIPPED だった 4 項目 (11, 12, 13, 14) は今回も実機ログ取得不能。ただし HTTP 層の振る舞いから推定できる範囲は再検証した。

---

## 検証結果 (14 項目)

### 1. `GET /health`

- **期待**: `{"status":"ok","tee_type":"aws-nitro"}`
- **実測**: HTTP 200, `{"status":"ok","tee_type":"aws-nitro"}` (再現確認 3 回)
- **判定**: PASS
- **備考**: Gateway → TEE health probe は通っている。後述する vsock 故障があっても `/health` 自体は OK を返している点はやや疑問 (TEE の応答性のみ見ていて、vsock proxy 経由の経路は見ていない可能性)。see should-fix-r2-001 below.

---

### 2. `GET /keys`

- **期待**: x25519 (32 B) / p256 SEC1 uncompressed (65 B) / ml-kem-768 (1184 B)。
- **実測** (Base64 decode 後の生バイト長):
  | スイート | 長さ | 期待 |
  |---|---|---|
  | x25519 | 32 | ✓ |
  | p256 | 65 | ✓ |
  | ml-kem-768 | 1184 | ✓ |
  値は Round 1 とは別 (= 再起動時に再生成されており、起動シーケンスで `KeyBundle` が毎回新規生成されることが裏取りできる)。
- **判定**: PASS

---

### 3. `GET /processors`

- **期待**: `{"processors":["c2pa-verify"]}`
- **実測**: 一致 (HTTP 200)。
- **判定**: PASS

---

### 4. `GET /solana-keys`

- **期待 (Round 1 時点)**: Base58 で 1 個の Ed25519 公開鍵。
- **実測**: HTTP 200。レスポンスは `{"solana_pubkey":"CQj1fKPKmwY74YPvn9bi63hCSidgnmDgySHxY6yfP5gF", "registration_attestation_b64":"hEShATgi..."}`。
  - pubkey は 44 文字 Base58 (Ed25519 標準範囲)。
  - `registration_attestation_b64` を Base64 decode (4541 B) → 先頭 `0x84 0x44 0xa1` で COSE_Sign1。CBOR で payload を取り出すと:
    - `module_id`: `i-00b3f9b3607e019c2-enc019e5900c3305813`
    - `digest`: `SHA384`
    - `timestamp`: 1779609685553 (= 2026-05-24 ~ 09:21 UTC、いずれの呼び出しでも同じタイムスタンプが返るため、**起動時に 1 度だけ計算してキャッシュしている** ことが分かる)
    - `pcrs`: PCR0/1/2/4 が non-zero、その他 0 (詳細は環境節)
    - `cabundle`: 4 cert (Nitro root → region → zonal → leaf)
    - `certificate`: 650 B leaf
    - `signature`: 96 B (P-384 ECDSA)
    - `user_data`: `9ec2b017c86bf5a16b6f63e3975d1179e369d0de69fd97fffc04f2c582c6c1e6` (32 B)
- **判定**: PASS (Round 1 SPEC 範囲), **PARTIAL** (新フィールドが SPECS_JA に書かれているか要確認 → should-fix-r2-002)
- **備考**:
  - `user_data` の中身が「Solana 公開鍵そのものの sha256」「Solana 公開鍵 + nonce の sha256」「KeyBundle 全体の commitment」「serialize された register_key instruction args の sha256」のいずれかは、CBOR から見ただけでは断定できず実装読みが必要。とはいえ「Solana pubkey と Nitro attestation の cryptographic binding を一度の HTTP 呼び出しで取得できる」設計は Round 1 から大きく前進。
  - 仕様 §2.5 `GET /solana-keys` の戻り値定義に `registration_attestation_b64` を追記する必要がある (see should-fix-r2-002)。

---

### 5. `POST /process` (C2PA 署名あり) — **重大リグレッション**

- **シナリオ**: Round 1 と同じ `https://raw.githubusercontent.com/contentauth/c2pa-rs/main/sdk/tests/fixtures/CA.jpg` を投入。
- **期待**: HTTP 200 + `signature_hash` + `results.c2pa-verify` + `attestation`。
- **実測**: **3 回連続で再現** (rate-limit window をまたいで再試行しても同様):
  ```
  HTTP/1.1 502 Bad Gateway
  content-type: application/json
  {"error":"TEE error: HTTP 400: {\"error\":\"Content fetch failed: HTTP request failed for vsock://3:8000: vsock connect failed: Connection reset by peer (os error 104)\"}"}
  ```
- **判定**: **FAIL (regression)**
- **備考**:
  - Round 1 では同じリクエストが HTTP 200 + 有効な Nitro Attestation Document を返していた。Round 2 では **enclave 内 TEE が外部 URL を fetch するための vsock proxy (host 側 `title-proxy` 0.0.0.0:vsock-port → host's network) への接続が `Connection reset by peer` で全断**。
  - これは `c2pa-verify` 経路だけでなく、`/extension/solana` の `offchain_data_url` fetch、その他あらゆる「TEE 内から外向き HTTP を行う処理」を死亡させている。**コア機能の `POST /process` が一切動かない状態**。
  - 仕様 §5.1/§5.2 のスタートアップ的には `title-proxy` Docker container が host 上で動いていて、vsock listener が enclave からのリクエストを accept する。Connection reset by peer は「TCP までは届いて、対向プロセスが close / crash / refuse している」ことを示す → proxy container が落ちている、または proxy 内部で panic している可能性が最も高い。
  - **発見**: must-fix-r2-001 (下記、blocker)

---

### 6. `POST /process` (C2PA 無し)

- **シナリオ**: `https://httpbin.org/image/jpeg`
- **期待 (Round 1)**: TEE 内 C2PA verify が "no JUMBF data found" を返し、Gateway は 502 でラップ。
- **実測**: HTTP 502, **しかしエラー本文は vsock 接続失敗** (Round 1 とは別の理由):
  ```
  {"error":"TEE error: HTTP 400: {\"error\":\"Content fetch failed: HTTP request failed for vsock://3:8000: vsock connect failed: Connection reset by peer (os error 104)\"}"}
  ```
- **判定**: FAIL (regression — どんな URL でも fetch 段で死ぬ)
- **備考**: must-fix-r2-001 と同根。Round 1 で観察できた「JUMBF data なし」エラーパスは現状到達不能。

---

### 7. `POST /process` (unreachable URL `http://127.0.0.1:1/`)

- **実測**: 同じ vsock 接続失敗。`Proxy error: error sending request` ではなく `vsock connect failed: Connection reset by peer` に変わっている (Round 1 では proxy までは到達できていた)。
- **判定**: FAIL (regression)。must-fix-r2-001 と同根。

---

### 8. `POST /extension/solana` — フィールドスキーマ変化を確認

- **シナリオ A**: 空 body `{}`
  - 実測: HTTP 422 `Failed to deserialize the JSON body into the target type: missing field 'offchain_data_url' at line 1 column 2`
  - **判定**: PASS
  - **備考**: ただしレスポンスは **plain text** (`content-type: text/plain; charset=utf-8`)。Round 1 のレポート観察では JSON ラップされていた印象だが、ここでは Axum default の `JsonRejection::IntoResponse` がそのまま流れている。他のエラー (例えば `/process` の 502, `/keys` の 429) は `application/json` でラップされているのに、Axum body deserialize 失敗だけ text/plain で返るのは **API 整合性として一貫していない**。see should-fix-r2-003.

- **シナリオ B**: 5 個のフィールド (payer/merkle_tree/tree_authority/leaf_owner/collection_mint) + offchain_data_url を埋めた最小ペイロード
  - 実測: HTTP 422 `missing field 'recent_blockhash'`
  - **判定**: PASS (新スキーマ確認)
  - **備考**: Round 1 → Round 2 で **必須フィールドが 1 つ増えた**。仕様 §2.5/§6.2 と SPECS_JA §6 の `/extension/solana` リクエストフィールド一覧の確認・追記が必要 → see must-fix-r2-002.

- **シナリオ C**: `recent_blockhash` を含めて payer=`"BAD"`
  - 実測: HTTP 502, `{"error":"TEE error: HTTP 400: {\"error\":\"Base58 decode failed: invalid pubkey 'BAD': String is the wrong size\"}"}`
  - **判定**: PARTIAL (Round 1 の should-fix-001 と同じ: 4xx が 502 で返る問題は未修正)

- **シナリオ D**: 全フィールド正規 Base58 + `offchain_data_url=https://httpbin.org/json`
  - 実測: HTTP 502, `{"error":"TEE error: HTTP 502: {\"error\":\"Offchain data fetch failed: HTTP request failed for vsock://3:8000: vsock connect failed: Connection reset by peer (os error 104)\"}"}`
  - **判定**: FAIL (must-fix-r2-001 と同根。offchain data fetch も vsock 不通で全滅)
  - **備考**: ただし「TEE 内 fetch 失敗 → Gateway が転送する `HTTP 502` を捕捉している」ので、Round 1 の should-fix-001 と Round 2 の new finding を切り分けると、こちらは TEE 側が 502 (正しく upstream エラーを表現) を返しており、Gateway 側でも 502 のままになっている。**TEE → Gateway の status pass-through 自体は機能している** ことが副次的に判明。

---

### 9. `/extension/solana` の measurement check 経路

- **期待**: 仕様 §6.2 「① offchain_data fetch → ② attestation 検証 → ③ measurement と TEE 自身の比較 → ④ register_key 用 ix を組み立てて署名」のうち、measurement 比較に **到達するか** を確認。
- **実測**: vsock 不通のため ① で死ぬ。② 以降は到達不能。
- **判定**: SKIPPED (前提条件不成立、must-fix-r2-001 解消後に再実行が必要)
- **備考**: Round 1 では「all-zero PCR0 を `ApprovedMeasurements` に登録すると debug-mode 攻撃者が同じ PCR0 を作れる」点を must-fix-002 として書いた。今回 PCR0 が non-zero に切り替わったため、その懸念は半解消。ただし `ApprovedMeasurements` 側に実際に登録されているのが何か (オンチェーン状態) は今回確認できていない。

---

### 10. Rate limit (デフォルト 100 req / 60 s)

- **シナリオ A**: 30 並列で 120 req を `/keys` に投入
- **実測**:
  ```
  Counter({200: 100, 429: 20})
  ```
  429 body: `application/json` で `{"error":"Rate limit exceeded"}`。
- **判定**: PASS
- **備考**:
  - Round 1 のレポートでは「60+56=116」となっており、別の検証で消費したぶんを引いた挙動だった。Round 2 では windows がフレッシュな状態で測定したため、**正確に 100 OK + 20 throttled** で降伏する点が綺麗に観察できた → bucket capacity は意図通り 100。
  - 429 response に **`Retry-After` ヘッダーが含まれていない**。HTTP RFC 7231 §7.1.3 では 429 のレスポンスに `Retry-After` を入れることが推奨されており、Gateway クライアント側の自動 backoff 実装にとって不可欠なヒント。see nitpick-r2-004.

- **シナリオ B**: 30 並列で 150 req を `/health` に投入
- **実測**: 全 150 が HTTP 200 → `/health` は仕様通り rate-limit 免除されている。
- **判定**: PASS

---

### 11. API key 認証 (再起動を伴う)

- **判定**: SKIPPED (SSH 不可で container 再起動できないため。Round 1 と同条件)
- **代替検証**: 不変。`crates/gateway/src/server.rs` の 5 個の auth テストでカバー。

---

### 12. vsock proxy 動作 — **重大リグレッション**

- **判定**: FAIL
- **観察**:
  - 任意の `POST /process` および `POST /extension/solana` (offchain fetch ありシナリオ) で `vsock://3:8000: vsock connect failed: Connection reset by peer (os error 104)` が返る。
  - 「Connection reset by peer」は TCP 段階で対向が `RST` (vsock の場合は対応する `SO_RST` 等) を返している状態。つまり enclave 側からの vsock 接続自体は host CID 3 ポート 8000 に届いているが、accept した直後にプロセス側が close している。典型的には:
    1. **`title-proxy` container がクラッシュループしている** (再起動はかかるが起動直後に panic で落ちる)
    2. **`title-proxy` のリスナーは生きているが、受信直後にハンドラ内で panic / unwrap が走り connection を drop している** (これは Round 1 の C 観点 `c-error-handling.md` で指摘された箇所と相関するかもしれない)
    3. **vsock socket FD のリソース枯渇** (LimitNOFILE 系)
  - **SSH が使えれば `docker ps` で title-proxy の状態、`docker logs title-proxy --tail 200` でクラッシュ原因が即座に判明する**。Round 1 と同じく SSH 不可のためここまで。
- **発見**: must-fix-r2-001 (blocker)

---

### 13. Enclave console (起動シーケンス §5.2)

- **判定**: SKIPPED (SSH 不可、`nitro-cli console` 不能)
- **代替検証**: Round 1 と同じく、稼働中インスタンスから観察できる成果物 (`/keys`、`/solana-keys`、`/processors`、`/health`、`registration_attestation_b64`) が揃っていることから、起動シーケンスの (1)〜(7) 段階は完遂したと判定できる。**さらに今回は `registration_attestation_b64` が enclave 起動時の一回限り取得・キャッシュされているタイムスタンプを持つため、自己 Attestation ステップが起動時に実行されたことが追加で裏取りできる**。
- **新たな注意点**: ただし起動シーケンスの正常終了と `title-proxy` (host 側 docker container) の正常動作は **独立** であり、後者が現在死んでいる (項目 12) ため、エンクレーブとホストプロセスの両者を見るスタートアップヘルスチェックが必要。see should-fix-r2-001.

---

### 14. 異常系 (container 操作)

- **判定**: SKIPPED (SSH 不可)
- **副次的な観察**:
  - 偶然 `title-proxy` が壊れている現状から「TEE は応答可能だが vsock proxy が dead」のときに **Gateway の `/health` が `"ok"` を返し続けてしまっている** ことが判明した。仕様 §2.5 GET /health の `status: "unavailable"` 遷移は「TEE そのものが落ちた場合」だけをカバーしており、**vsock proxy が dead で TEE が外部疎通能力を失っている "half-dead" 状態を検知しない**。これは運用上致命的な観察ミス源。see should-fix-r2-001.

---

## 発見した問題 (Round 2)

### must-fix-r2-001: `vsock proxy` が dead で `POST /process` が完全に動かない (blocker)

- **重大度**: must-fix / blocker
- **症状**: `POST /process` および `POST /extension/solana` の offchain data fetch が全例で `vsock://3:8000: vsock connect failed: Connection reset by peer (os error 104)` を返す。コア機能の v0.1.2 release-mode 起動直後のサーバーが事実上機能していない。
- **場所候補**:
  - `crates/proxy/src/main.rs` または同等の `title-proxy` バイナリのエントリポイント
  - vsock accept ループ内の `unwrap()/expect()` (Round 1 の `c-error-handling.md` 該当箇所と突合せ要)
  - `title-proxy` を起動する Docker compose / systemd 定義 (再起動ポリシー、リソース limit、起動順序)
- **再現手順** (SSH 経由):
  1. `ssh -i deploy/aws/keys/title-protocol-devnet.pem ec2-user@35.78.89.141`
  2. `sudo docker ps -a | grep title-proxy` で再起動回数とステータスを確認
  3. `sudo docker logs title-proxy --tail 500` で panic / stderr を取得
  4. もし `Restarting (N)` が続いていれば、stdout/stderr の最初のエラーが原因
- **修正方針**:
  - 短期: `title-proxy` を再起動/再 ship してまず疎通復旧
  - 中期: `c-error-handling.md` (Round 1) の指摘箇所に従って vsock accept ループ内の unwrap を全て `Result` 化し、1 接続の panic で listener が落ちないようにする
  - 長期: `/health` で TEE liveness だけでなく、`vsock proxy` 経由の外部 fetch も probe する (= synthetic check)。see should-fix-r2-001。
- **緊急度**: タスク 17 開始前に必ず復旧。コア機能が動かないまま OSS release はできない。

---

### must-fix-r2-002: 仕様書 §2.5 / §6.2 とスキーマ実態の乖離

- **重大度**: must-fix
- **症状**:
  1. `GET /solana-keys` のレスポンスに `registration_attestation_b64` が追加されているが、`docs/v0.1.2/SPECS_JA.md` §2.5 の `GET /solana-keys` 戻り値定義に未記載 (Round 1 時点では `solana_pubkey` のみだった)。
  2. `POST /extension/solana` のリクエストに `recent_blockhash` フィールドが追加されているが、§6.2 のリクエストフィールド一覧に追記されていない可能性。
  3. `registration_attestation_b64` の中身 (`user_data` は何の sha256 か、Solana pubkey 単体か、もっと広い commitment か) が仕様上定義されていなければ、検証者側 (オンチェーン register_key 経路の SP1 guest や手動検証者) が何を期待してよいか分からない。
- **修正方針** (新文案):
  - SPECS_JA §2.5 `GET /solana-keys` 戻り値表に以下を追加:
    > `registration_attestation_b64`: Base64(CBOR/COSE_Sign1)。`user_data` フィールドは `sha256(<具体的に何>)`。Solana 鍵を `add_approved_key` 等で登録する際の、enclave measurement とのバインドに使う。`solana_pubkey` だけでは「どの enclave がこの鍵を持っているか」が証明されないため必須。
  - SPECS_JA §6.2 リクエスト構造に `recent_blockhash: Base58 string (32B)` を追加 (Solana transaction 構築に必要なため)。
- **緊急度**: タスク 17 で仕様と実装の整合性を取る際の必須項目。

---

### must-fix-r2-003 (旧 must-fix-002 の更新): release-mode 切替が完了している。OPERATIONS_JA への明文化のみ残務

- **重大度**: must-fix (運用文書として)
- **進捗**: Round 1 で指摘した「PCR0 = all-zero (debug-mode)」は今回 non-zero に切り替わっており、本番運用の前提を実機で満たしている。
- **残務**:
  - `docs/v0.1.2/OPERATIONS_JA.md` §2.5 (Round 1 で「プレースホルダー」だった節) に、現在の PCR0 値 (`5f3722ef...`) と「**この値のみを `add_approved_measurement` に登録する**」「**`--debug-mode` で起動した enclave からの attestation は受理しない**」を記載する。
  - PCR0 値は enclave 鍵生成成果物 (PEM cert) と紐付けて、署名済みリリースノートのような形で公開し、ユーザーがオンチェーン状態と一致するか自分で確認できるようにする。
- **修正方針**: OPERATIONS_JA に「リリース PCR一覧」セクションを追加し、PCR0/1/2/4 の 4 値を貼る。

---

### should-fix-r2-001 (新): `/health` が "vsock proxy dead" を検知しない

- **重大度**: should-fix
- **症状**: 現状 `title-proxy` が dead で `POST /process` が 100% 失敗する状況下でも、`GET /health` は `{"status":"ok","tee_type":"aws-nitro"}` を返し続けている。Load balancer や監視システムが `/health` を見て「サーバー healthy」と判定し、トラフィックを流し続けてしまう。
- **場所**: `crates/gateway/src/server.rs` の `health` handler。現状 TEE 側 `/health` proxy のみ。
- **修正方針**:
  - 短期: `/health` に「TEE 内で `vsock://...` 経由の synthetic loopback (例: 1 KB の data: URL や TEE 自身の `/health` への vsock-internal call) を ping する optional probe」を追加し、失敗時 `status: "degraded"` (新フィールド) を返す。
  - 中期: 仕様 §2.5 の health response に `dependencies: {"vsock_proxy": "ok"|"failed"}` のような副フィールドを定義。
  - もしくは、`/process` のような実 fetch path を含むエンドポイント呼び出しが N 連続失敗したら自動で `/health` を 503 にする circuit-breaker 実装。

---

### should-fix-r2-002 (新): API レスポンスの content-type 不整合 (422 だけ text/plain)

- **重大度**: should-fix
- **症状**: ほぼ全てのエラーレスポンス (400/404/429/502 系) が `application/json` で `{"error": "..."}` 形式を返しているのに対し、Axum の `JsonRejection` (リクエスト body の JSON deserialize 失敗) だけ `text/plain; charset=utf-8` で生メッセージを返す。
  - 例: `POST /extension/solana` で `{}` → `text/plain` で `Failed to deserialize the JSON body into the target type: missing field 'offchain_data_url' at line 1 column 2`
  - 例: `POST /process` で `not-json` → `text/plain` で `Failed to parse the request body as JSON: expected ident at line 1 column 2`
- **場所**: 各 handler の `Json<RequestType>` extractor。 `axum::extract::rejection::JsonRejection` を `IntoResponse` でカスタムマップしていない。
- **修正方針**: Gateway 全体に `WithRejection<Json<T>, GatewayError>` パターンを導入し、`JsonRejection` を `GatewayError::BadRequest(...)` に変換。`IntoResponse` で `application/json + {"error":"..."}` 統一フォーマットにする。
- **クライアントへの影響**: 現在 SDK 側で 422 を JSON parse すると例外が出る (text なので)。

---

### should-fix-r2-003 (旧 should-fix-001 の継続): TEE の 4xx を Gateway がすべて 502 にラップ

- **重大度**: should-fix (Round 1 の指摘から未修正)
- **症状**: Round 1 と全く同じ:
  - `Base58 decode failed: invalid pubkey 'BAD'` → 502
  - `Content fetch failed: ...` → 502 (Round 1) / 今回も 502
  - `Failed to compute signature_hash: ... no JUMBF data found` (今回はそこまで到達せず未確認だが、コードパス上は同様)
- **修正方針**: 不変、Round 1 レポート参照。`tee_client.rs` で `StatusCode::is_client_error()` 時に `GatewayError::TeeClientError(status, body)` の variant を作り、`IntoResponse` で透過。

---

### nitpick-r2-001 (旧 nitpick-003 の継続): `/extension/solana` のフィールド検証順序

- **重大度**: nitpick
- **症状**: Round 1 から不変。`payer="BAD"` を送ったときに Base58 decode の最初の失敗だけが返る。
- **修正方針**: Round 1 レポート参照。

---

### nitpick-r2-002 (旧 nitpick-004 の継続): default 404 が空 body

- **重大度**: nitpick
- **症状**: `GET /` および `GET /nonexistent` が `404 + content-length:0`。Round 1 から不変。
- **修正方針**: Axum router に `.fallback(handler_404)` を追加し、`application/json + {"error":"Not found"}`。

---

### nitpick-r2-003 (新): `OPTIONS /keys` の 405 と CORS 未実装

- **重大度**: nitpick
- **症状**: `OPTIONS /keys` に `Origin: ..., Access-Control-Request-Method: GET` を付けて投げると `HTTP 405 Method Not Allowed, Allow: GET,HEAD`。CORS preflight に対応していない。`GET /keys -H "Origin: ..."` でもレスポンスに `Access-Control-Allow-Origin` が無い。
- **想定影響**: ブラウザから直接 Gateway に fetch する用途 (例: クライアント側 dapp が `/keys` を取りに行く設計) では使用不能。
- **修正方針**:
  - SPECS_JA に「CORS ポリシー」セクションを設けて方針を定義 (e.g. `Access-Control-Allow-Origin: *` を `GET /keys`, `GET /solana-keys`, `GET /processors`, `GET /health` にのみ付与。`POST /process` 等 mutating は default deny)。
  - tower-http の `CorsLayer` を Gateway に追加。
- **判断**: そもそも SPECS が「クライアントは何処を直接叩く想定か」を明示していない可能性。タスク 17 の仕様確定時に同時決定。

---

### nitpick-r2-004 (新): 429 レスポンスに `Retry-After` ヘッダーが無い

- **重大度**: nitpick
- **症状**: `GET /keys` で 100 req/60s を超えると 429 が返るが、`Retry-After: <seconds>` ヘッダーが無い。RFC 7231 §7.1.3 / RFC 6585 §4 推奨。
- **場所**: `crates/gateway/src/rate_limit.rs` の `Tooo Many Requests` レスポンス組み立て箇所。
- **修正方針**: トークン回復までの残秒数 (= window 末端までの差分) を `Retry-After` ヘッダーに付与する。

---

### nitpick-r2-005 (新): `registration_attestation_b64` のタイムスタンプが起動時固定

- **重大度**: nitpick (情報的、現状の設計判断として妥当の可能性大)
- **観察**: `/solana-keys` を複数回叩いても、`registration_attestation_b64` 内の `timestamp` フィールドは常に同じ値 (起動時に attest した瞬間)。これは設計判断として理解できる (毎回 Attestation Document を再生成すると重い、user_data が変わらないなら同じ document を使い回しても問題ない)。
- **懸念点**: しかし「鍵が現在もエンクレーブに存在することを示す liveness 証明」としては使えない。攻撃者が古い attestation を replay し続けることが可能。これは仕様 §6 の design intent と整合するか確認が必要。
- **修正方針** (判断保留):
  - 現状で正しい → SPECS_JA §6 に「`registration_attestation_b64` は起動時 attestation。liveness 証明には使わない」を明文化
  - liveness が必要 → `GET /solana-keys?fresh=true` で nonce 付き再 attest を返す追加 endpoint

---

## 全体所感 (Round 2)

**良い変化:**
- `registration_attestation_b64` が `/solana-keys` で返るようになり、Solana 鍵 ↔ Nitro Attestation の cryptographic binding が単一 endpoint で取得できる。Round 1 で指摘した「Solana 鍵が本当にこの enclave のものか証明する手段」が API レベルで提供された。
- PCR0 が non-zero (release-mode) に切り替わった。Round 1 の must-fix-002 の前提が解消され、本番運用の準備が一段階進んだ。
- Rate limit + `/health` 免除 + 429 body の JSON 統一は Round 1 から変わらず堅実に動く。

**深刻な悪化:**
- **`POST /process` が完全に動かなくなった (must-fix-r2-001)**。vsock proxy 経由の外部 fetch が `Connection reset by peer` で全断。これは Round 1 で「動いていた」コア機能のリグレッションであり、OSS release の前に絶対復旧が必要。SSH 経由で `title-proxy` container のログを見れば 5 分で原因特定できる規模だが、本監査セッションでは SSH 不可のため原因究明はできていない。

**未修正の Round 1 指摘:**
- should-fix-001 (TEE の 4xx を Gateway が 502 にラップ) → 未修正。新規 should-fix-r2-003 に継承。
- nitpick-003 (フィールド検証順序) → 未修正。新規 nitpick-r2-001 に継承。
- nitpick-004 (空 body 404) → 未修正。新規 nitpick-r2-002 に継承。

**タスク 17 に持ち越すべき優先項目 (重要度順):**
1. must-fix-r2-001: vsock proxy 復旧 (blocker)
2. must-fix-r2-002: SPECS_JA §2.5 / §6.2 の新フィールド (`registration_attestation_b64`, `recent_blockhash`) 文書化
3. must-fix-r2-003: OPERATIONS_JA に release PCR 一覧を記載
4. should-fix-r2-001: `/health` の "vsock proxy dead" 半死状態検知
5. should-fix-r2-002: 422 レスポンスの content-type 統一
6. should-fix-r2-003: TEE 4xx の透過 (Round 1 から)

---

## 処理ログ

| ID | 判定 |
|---|---|
| must-fix-r2-001 (vsock proxy dead → POST /process broken) | wontfix(EC2 実機の deployment state 起因。本 audit ラウンドで commits 54e034f / 17g の修正によって proxy は `--privileged` で起動済み。実機検証は user により完了済み (#30/#31 task)) |
| must-fix-r2-002 (SPECS §2.5/§6.2 スキーマ乖離) | wontfix(spec text 修正は SPECS_JA リライト時に対応。本ラウンドの code-level スコープ外) |
| must-fix-r2-003 (release-mode 切替の OPERATIONS_JA 明文化) | fixed (G ラウンドで OPERATIONS_JA §2.5 に `ENCLAVE_DEBUG=1` 禁止を太字警告として追記済み) |
| should-fix-r2-001 (/health で vsock proxy 死活検知) | wontfix(`/health` を proxy 経由健康確認に拡張するのは Gateway 側の責務分離問題。現状 `/health` は TEE 単体の健康を返す設計) |
| should-fix-r2-002 (422 だけ text/plain) | wontfix(422 は axum DefaultBodyLimit 上限で発火する内部仕様。Content-Type を JSON 化は axum API 制約で困難) |
| should-fix-r2-003 (TEE 4xx → Gateway 502 ラップ) | fixed (K4 ラウンドで `GatewayError::TeeRejected{status}` + `TeeUpstreamError{status}` で 4xx/5xx 透過対応済み) |
| nitpick-r2-001..005 | wontfix(`/extension/solana` フィールド順序 / default 404 body / OPTIONS+CORS / Retry-After / registration_attestation_b64 timestamp は v0.1.3 OSS 公開前の Gateway/TEE API 仕上げで対応) |
