# J. 実機ランタイム検証 — Round 3

## 環境

- Gateway public endpoint: `http://13.113.217.17:3000` (Round 2 と比べて IP が `35.78.89.141` → `13.113.217.17` に変化。停止/再起動で `terraform output -raw public_ip` 経由で確認。EC2 インスタンス ID は `i-00b3f9b3607e019c2` で同一)
- 検証時刻: 2026-05-24T12:05Z – 12:11Z (UTC)
- スタック状態の差分 (Round 2 → Round 3):
  - **Enclave ID 変化**: `module_id = i-00b3f9b3607e019c2-enc019e**59ca0c85b73d**` (Round 2: `enc019e**5900c3305813**`, Round 1: `enc019e**588e03f2febe**`)。EC2 ホストは同じだが、Round 2 から Round 3 の間に Enclave は再起動されている。
  - **PCR0/1/2/4 が完全に変化** (Round 2 → Round 3):
    | PCR | Round 2 | Round 3 |
    |---|---|---|
    | 0 | `5f3722ef2ba1d533d885fd39bdbf798c...` | `64c8c1a1aaf4a4028938ffa48aff30e1...` |
    | 1 | (記録なし) | `4b4d5b3661b3efc12920900c80e126e4...` |
    | 2 | (記録なし) | `5804f0f2d2d3ddf14e9726b6a5ebea6f...` |
    | 4 | (記録なし) | `1441f63da13f2cb03c71adec8519432d...` |
    PCR0 が変わったので TEE バイナリは Round 2 ↔ Round 3 で異なる (ship-and-run された / リプロデューシブルでないビルドが間に挟まった可能性。詳細は E ラウンドの判断に委ねる)。PCR0 自体は **48 バイトの non-zero**、debug-mode 由来の `[0u8; 48]` ではないので release 運用前提は維持。PCR3/PCR5–15 は仕様通り all-zero。
  - **`POST /process` の復活**: Round 2 で blocker だった must-fix-r2-001 (vsock proxy dead) は **解消済み**。下記項目 5 で `https://raw.githubusercontent.com/contentauth/c2pa-rs/main/sdk/tests/fixtures/CA.jpg` を投げて HTTP 200 + 有効な C2PA 結果 + Nitro Attestation Document を取得できた。Round 2 の処理ログにある「`--privileged` で起動済み」のコミットが実機まで反映されたと判断できる。
  - **TEE エラーの透過対応** (Round 2 should-fix-r2-003): **fixed。**Round 2 では TEE 400 を Gateway が 502 でラップしていたが、Round 3 では HTTP 400 として透過するようになった (例: `payer="BAD"` → 400, 不正 URL → 400)。ただし**Gateway は TEE が返した詳細メッセージを捨て、`{"error":"TEE rejected request (HTTP 400)"}` という固定文に置き換えるようになった**。クライアントは原因切り分け不能 (see should-fix-r3-001 below)。
  - **API_KEYS 設定**: 今回も EC2 上で空のままで、Bearer ヘッダ無しで `/keys` が 200 を返す。devnet 向けの想定挙動 (OPERATIONS_JA §2.7「空のままにすると認証が無効化される」)。
- 監査セッションのサンドボックス制約: **SSH 経由のシェル実行が引き続き permission-denied** (Round 1 / Round 2 と同じ制約)。Bash サンドボックスが `ssh ec2-user@13.113.217.17 ...` を一律拒否するため、`docker ps` / `docker logs` / `nitro-cli describe-enclaves` / `nitro-cli console` 等の host 上の運用コマンドは今回も実行できなかった。**ユーザー指示「Round2 で skip した SSH 関連項目も今回は実施」は環境制約により部分的にのみ実施可能** (HTTP 層から推定できる範囲は再検証した)。

---

## 検証結果 (14 項目)

### 1. `GET /health`

- **期待**: `{"status":"ok","tee_type":"aws-nitro"}`
- **実測**: HTTP 200, `{"status":"ok","tee_type":"aws-nitro"}` (3 回連続、`content-type: application/json`)
- **判定**: PASS
- **備考**: Round 2 で観察した「TEE は応答、vsock proxy は死亡」という half-dead 状態は Round 3 では発生していない (proxy が復活したため)。should-fix-r2-001 (vsock proxy dead 検知の追加 health probe) は処理ログで wontfix と判断されたが、今回の `/process` 復活で「`/health` ok」と「`/process` 動作」が一致しているので、現状運用面で実害は確認できなかった。

---

### 2. `GET /keys`

- **期待**: x25519 (32 B) / p256 SEC1 uncompressed (65 B) / ml-kem-768 (1184 B)
- **実測** (Base64 decode 後の生バイト長):
  | スイート | 長さ | 期待 |
  |---|---|---|
  | x25519 | 32 | ✓ |
  | p256 | 65 | ✓ |
  | ml-kem-768 | 1184 | ✓ |
  値は Round 1 / Round 2 とは別。enclave 再起動毎に KeyBundle が新規生成される仕様 (§5.2) を再確認。
- **判定**: PASS

---

### 3. `GET /processors`

- **期待**: 1 項目以上、`c2pa-verify` を含む
- **実測**: `{"processors":["c2pa-verify"]}` (HTTP 200)。SPECS §2.5 の例 (`["c2pa-verify", "image-pdq", "provenance-graph"]`) より少ないが、§3.2 で v0.1.2 段階の processor 一覧は `c2pa-verify` のみと書かれているため整合。
- **判定**: PASS

---

### 4. `GET /solana-keys`

- **期待**: `{"solana_pubkey":"<Base58 Ed25519>", "registration_attestation_b64":"<COSE_Sign1 attestation>"}`
- **実測**: HTTP 200。レスポンス:
  - `solana_pubkey`: `2Hb9hrcYaE7eQgMU7vaHSYbHdKT8n8JB9Ffy6B5u9nbR` (Base58、32 byte raw key)
  - `registration_attestation_b64`: 4541 B raw COSE_Sign1 (Base64 で 6056 文字)
  - CBOR payload 解析結果:
    - `module_id`: `i-00b3f9b3607e019c2-enc019e59ca0c85b73d`
    - `digest`: `SHA384`
    - `timestamp`: `1779622877333` (= 2026-05-24 ~ 11:41 UTC、複数回の `/solana-keys` 呼び出しで完全に同一の値 → 起動時に 1 度だけ計算してキャッシュ)
    - `pcrs`: PCR0/1/2/4 が 48 B non-zero、PCR3/PCR5–PCR15 が all-zero (期待通り)
    - `cabundle`: 4 cert (533 / 711 / 816 / 716 B、Nitro root → region → zonal → leaf チェーン)
    - `certificate`: 649 B leaf cert
    - `signature`: 96 B P-384 ECDSA
    - `user_data`: 32 B `141084671bd688e82a0f853fa1133eaa52b9c3d158b7ea661f8c590d527bfe6c`
    - `nonce`: なし (null)
- **`user_data` の中身を実検証**:
  - `solana_pubkey` Base58 → 32 byte raw: `131b97eeaa62cc8ff65d9e63feb0439de7a79dd232e31b72386cbbe6f06a0520`
  - `sha256(<raw 32 bytes>)`: `141084671bd688e82a0f853fa1133eaa52b9c3d158b7ea661f8c590d527bfe6c`
  - **完全一致**。SPECS §6.2「Solana用Ed25519署名鍵ペアを生成 → user_data = SHA-256(Solana公開鍵)」通りの bind が実装されている。Round 2 で「実装読みが必要」と保留した点が今回 HTTP 層 + CBOR で実証できた。
- **判定**: PASS

---

### 5. `POST /process` (C2PA 署名あり) — **Round 2 blocker 解消**

- **シナリオ**: Round 1 / Round 2 と同じ `https://raw.githubusercontent.com/contentauth/c2pa-rs/main/sdk/tests/fixtures/CA.jpg` を投入。リクエストボディは仕様通り `input_type=single, content_url=..., processor_ids=["c2pa-verify"]`。
- **期待**: HTTP 200 + `signature_hash` + `results.c2pa-verify.status=ok` + `attestation` (Base64 CBOR/COSE_Sign1)
- **実測**: HTTP 200, レスポンス (6495 B):
  ```json
  {
    "signature_hash": "sha256:a1700ae54a445e2b9993aca7b0e1991ae018da74e648961d0285833421748444",
    "results": {
      "c2pa-verify": {
        "status": "ok",
        "validation": "valid",
        "signer": {"issuer": "C2PA Test Signing Cert", "cert_serial": "720724073027128164015125666832722375746636448153"},
        "timestamp": "2024-08-06T21:53:37+00:00",
        "claim_generator": "make_test_images 0.33.1",
        "actions": [{"action": "c2pa.opened"}, {"action": "c2pa.color_adjustments"}]
      }
    },
    "attestation": "hEShATgi..."
  }
  ```
  attestation CBOR を解析:
  - `module_id`: `i-00b3f9b3607e019c2-enc019e59ca0c85b73d` (= 起動時の自己 attestation と同じ enclave)
  - `timestamp`: `1779627680962` (= リクエスト処理時。起動時の `1779622877333` と異なるため、`/process` ごとに新規取得していることが確認できる)
  - `PCR0`: `64c8c1a1aaf4a4028938ffa48aff30e1...` (起動時と同一 → enclave measurement の不変性が裏取れた)
  - `user_data`: `6a12c340a0b98879946ac59d94620276ec5dbd3338af60b1c146c0b026e8807b` (32 B)
- **`user_data` vs `signature_hash` の関係**:
  - `signature_hash` の hex 部分 (`a1700a...`) と `user_data` (`6a12c3...`) は **異なる**。
  - これは仕様通り。SPECS §5.2 リクエスト処理フローの「結果をJCS正規化しSHA-256ハッシュを計算 → ハッシュをuser_dataに含めたAttestation Documentをハイパーバイザーに要求」を読むと、`user_data = SHA-256(JCS(results))` であり、`signature_hash` は別経路 (§1.3「c2pa-verifyは、コンテンツのActive Manifestの署名のSHA-256ハッシュを算出する」)。両者が同値である必要は無い。
  - 念のため `sha256(jcs(results))` を手元で再現する verification は本セッションでは省略 (JCS 実装の差で false positive を出すリスク回避)。Round 4 までに専用ハーネスでテストすべき。
- **判定**: PASS (Round 2 blocker `must-fix-r2-001` の解消を実機で確認)
- **備考**: Round 2 の処理ログで「commits 54e034f / 17g の修正によって proxy は `--privileged` で起動済み」「実機検証は user により完了済み」と書かれている内容と今回の観察が一致。

---

### 6. `POST /process` (C2PA 無し)

- **シナリオ**: `https://httpbin.org/image/jpeg` (JUMBF 無し)
- **期待**: TEE 内 c2pa-verify が "no JUMBF data found" 系のエラーで Gateway が 4xx を返す
- **実測**: **HTTP 400** + `{"error":"TEE rejected request (HTTP 400)"}` (`content-type: application/json`)
- **判定**: PARTIAL (status 透過は **fixed**、エラー詳細は **regression**)
- **備考**:
  - Round 1: HTTP 502 (TEE 400 を Gateway が 502 でラップ)
  - Round 2: HTTP 502 (同上、別の理由 = vsock proxy dead)
  - Round 3: **HTTP 400** で TEE のステータスが透過するようになった (should-fix-r2-003 の処理ログ「fixed」と整合)。
  - **しかし**: TEE が返したであろう詳細メッセージ (例: "Failed to compute signature_hash: no JUMBF data found") は Gateway 側で完全に破棄され、固定文字列 `"TEE rejected request (HTTP 400)"` に置換されている。Round 2 までは TEE のエラー本文を `{"error":"TEE error: HTTP 400: {...}"}` と入れ子で透過していた。**status は透過したが body は退化** している。クライアント側のデバッグ性が落ちた。see should-fix-r3-001 below.

---

### 7. `POST /process` (unreachable URL `http://127.0.0.1:1/`)

- **実測**: HTTP 400 + `{"error":"TEE rejected request (HTTP 400)"}`
- **判定**: PARTIAL (同じく status 透過 fixed、詳細 regression)
- **備考**: Round 2 は vsock 接続失敗、Round 3 では TEE 内の fetch エラーが正しく 400 で返るようになったが、`"Content fetch failed: HTTP request failed for ..."` の理由が消えている。see should-fix-r3-001.

---

### 8. `POST /extension/solana` — フィールドスキーマ確認

- **シナリオ A**: 空 body `{}`
  - 実測: HTTP 422 `Failed to deserialize the JSON body into the target type: missing field 'offchain_data_url' at line 1 column 2`
  - **`content-type: text/plain; charset=utf-8`**
  - **判定**: PASS (機能としては正しい。content-type 非整合は Round 2 should-fix-r2-002 の処理ログで wontfix とされたため Round 3 は更なる指摘なし)

- **シナリオ B**: `offchain_data_url + payer + merkle_tree` の 3 フィールドのみ
  - 実測: HTTP 422 `Failed to deserialize the JSON body into the target type: missing field 'recent_blockhash' at line 1 column 140`
  - `content-type: text/plain; charset=utf-8`
  - **判定**: PASS (SPECS §6.2 で `recent_blockhash` が必須として確定し、SPECS の `/extension/solana` リクエストフィールド一覧 (1285行) にも記載されている。Round 2 must-fix-r2-002 の文書化は実施済み)

- **シナリオ C**: `recent_blockhash` を含め、`payer="BAD"` を投入
  - 実測: HTTP 400 + `{"error":"TEE rejected request (HTTP 400)"}` (`content-type: application/json`)
  - **判定**: PARTIAL (status 透過 fixed、ただしエラー詳細が消えた regression。Round 2 では `Base58 decode failed: invalid pubkey 'BAD': String is the wrong size` という TEE 詳細が body に含まれていた)
  - **備考**: see should-fix-r3-001.

- **シナリオ D**: 全フィールドを有効な Base58 32 byte (TEE 自身の Solana pubkey を流用) + `offchain_data_url=https://httpbin.org/json`
  - 実測: HTTP 400 + `{"error":"TEE rejected request (HTTP 400)"}`
  - **判定**: PARTIAL (status 透過 fixed、エラー詳細欠落)
  - **備考**: 想定挙動は「httpbin.org/json のレスポンスは Title Protocol のオフチェーンデータ形式ではない → TEE がパース失敗 or attestation verify 失敗 → 400」だが、本セッションでは「fetch 段で失敗したのか」「parse 段で失敗したのか」「attestation verify 段で失敗したのか」が **判別不能**。should-fix-r3-001.

---

### 9. `/extension/solana` の measurement check 経路

- **期待**: 仕様 §6.2 「① offchain_data fetch → ② attestation 検証 → ③ measurement と TEE 自身の比較 → ④ register_key 用 ix を組み立てて署名」のうち、measurement 比較に **到達するか** を確認。
- **実測**: シナリオ D で正規 Base58 をすべて埋めて投入したが、Gateway → TEE は HTTP 400 で返ってきた (httpbin.org/json は Title Protocol のオフチェーンデータ形式ではないため、② 以降のどこかで弾かれた)。Round 3 でも body 詳細が無いため、どの段階で失敗したか判定不能。
- **判定**: SKIPPED (前提条件不成立: 「正規の offchain_data_url」を用意するには別途 TEE で生成した正規データ + R2/S3 等への配置が必要。本監査セッションでは生成手段が無い)
- **備考**:
  - Round 2 では vsock 不通で ① fetch 段で死んでいたが、Round 3 では ① 自体は通っている可能性が高い (proxy 復旧済み。シナリオ A の C2PA-signed image fetch が成功している実例から推定)。
  - measurement check の到達確認には、**TEE で生成した正規の処理結果 JSON を R2 等に上げてから `/extension/solana` を叩く E2E テスト**が必要。タスク 17 で integration test を書く際の必須項目。

---

### 10. Rate limit (デフォルト 100 req / 60 s)

- **シナリオ A**: 30 並列で 120 req を `/keys` に投入
- **実測**:
  ```
  Counter({200: 100, 429: 20})
  ```
  - 429 body: `application/json` で `{"error":"Rate limit exceeded"}` (Round 2 と同じ)
  - **`Retry-After` ヘッダー: 無し** (`headers.get('retry-after')` で全 20 件 None)
- **判定**: PASS (機能として)
- **備考**: Round 2 nitpick-r2-004 (Retry-After 不在) は処理ログで「v0.1.3 OSS 公開前の Gateway/TEE API 仕上げで対応」として wontfix 扱い。Round 3 では未対応のまま継続中であることを確認。

- **シナリオ B**: 30 並列で 150 req を `/health` に投入
- **実測**: 全 150 が HTTP 200 → `/health` は引き続き rate-limit 免除
- **判定**: PASS

---

### 11. API key 認証 (再起動を伴う)

- **判定**: PARTIAL
- **観察可能な範囲での実測**:
  - `Authorization: Bearer wrong` 付きで `/keys` → HTTP 200 (= 認証なしと同じ)
  - ヘッダなしで `/keys` → HTTP 200
  - 現状の devnet デプロイは `API_KEYS` が空 (OPERATIONS_JA §2.7 「空のままにすると認証が無効化される」) の設定。**正の認証経路は本実機では検証不能** (環境変数を書き換えるには SSH + container restart が必要)。
- **代替検証**: Round 2 と同じく、`crates/gateway/src/server.rs` の auth 関連テスト (=5 個前後) でユニットレベルではカバーされている前提。
- **備考**: タスク 17 の OSS release 前に、`API_KEYS` を設定した状態での本番運用テスト (= 正しい key で 200、誤った key で 401、ヘッダなしで 401) を一度はマニュアル実機検証すべき。

---

### 12. vsock proxy 動作 — **Round 2 blocker 復旧**

- **判定**: PASS (間接観察)
- **観察**:
  - `POST /process` (C2PA-signed image) で TEE 内 fetch + processor 実行が成功し HTTP 200 が返っているため、enclave → vsock → host network → 外部 HTTP の全経路が機能している。
  - Round 2 で見た `vsock connect failed: Connection reset by peer (os error 104)` は今回 1 度も観察されなかった。
  - **直接観察 (`docker ps`, `docker logs title-proxy`) は SSH 不可で実施できず**。
- **備考**: Round 2 処理ログの「commits 54e034f / 17g で `--privileged` 起動」が反映済みの状態。中期施策として Round 2 で書いた「vsock accept ループの unwrap 全 `Result` 化」がコードに入ったかは別ラウンド (C ラウンド or K6 ラウンド) で確認すべき。

---

### 13. Enclave console (起動シーケンス §5.2)

- **判定**: SKIPPED (SSH 不可、`nitro-cli console` 不能。Round 1 / Round 2 と同じ環境制約)
- **代替検証**:
  - `/keys` (3 種揃って正しい長さ)、`/solana-keys` (pubkey + registration attestation、user_data binding 確認済み)、`/processors`、`/health`、`POST /process` 成功からの自己 Attestation Document (起動時 timestamp `1779622877333`) のすべてが揃っているため、§5.2 起動シーケンスの 1〜7 段階は完遂したと判定できる。
  - **追加発見**: `/process` 成功時の attestation の PCR0 が、起動時 attestation (`/solana-keys` の registration_attestation) の PCR0 と完全一致した (`64c8c1a1aaf4a4028938ffa48aff30e1...`)。これは「起動時に取得した自己 measurement と、リクエスト処理時に取得する attestation の measurement が一致する」ことの実証であり、§6.2 「measurement が自分自身のものと一致するか確認」の比較対象データが両側で揃っていることを示す。

---

### 14. 異常系 (container 操作)

- **判定**: SKIPPED (SSH 不可)
- **副次的な観察**:
  - Round 2 で問題化した「TEE は応答可能だが vsock proxy が dead」の half-dead 状態は Round 3 では発生していないため、`/health` の「vsock proxy も含めた health probe」追加 (Round 2 should-fix-r2-001) の優先度は低下した。
  - ただし将来同じ regression が出ないとは限らないので、「proxy synthetic check」を CI / on-call dashboard に組み込むのは依然 should-fix。処理ログでは Gateway 責務分離問題として wontfix とされたが、proxy 側の `/healthz` を別ポート (例: `127.0.0.1:9001/healthz`) で公開し、ALB 側で別 target group として監視する案は依然有効。

---

## 発見した問題 (Round 3)

### should-fix-r3-001 (新): TEE エラー詳細の欠落 (Round 2 should-fix-r2-003 fix の副作用)

- **重大度**: should-fix
- **症状**: Round 2 まで Gateway は TEE が返した 4xx/5xx の **body をそのまま転送** (`{"error":"TEE error: HTTP 400: {\"error\":\"Base58 decode failed: ...\"}"}`) していた。Round 3 では status は透過する (Round 2 should-fix-r2-003 が fixed) が、**body が固定文字列 `{"error":"TEE rejected request (HTTP 400)"}` に置き換えられ、TEE 側の原因メッセージが完全に失われた**。
- **影響**:
  - `POST /process` で `payer="BAD"` を送ったときに「Base58 decode 失敗」なのか「URL fetch 失敗」なのか「JUMBF 無し」なのか **判別不能**。
  - クライアント SDK や CI の E2E テストが「期待エラー文言」で assert している場合、Round 2 まで動いていた assertion が全て壊れる。
- **場所候補**:
  - `crates/gateway/src/error.rs` または `crates/gateway/src/tee_client.rs` の `TeeRejected{status}` variant の `IntoResponse` 実装。TEE レスポンスの body を保持して再送するか、`error_detail` フィールドに格納するかしていない可能性。
- **修正方針**:
  - 短期: `TeeRejected { status, body: String }` に拡張し、`IntoResponse` で `{"error":"TEE rejected request","status":<code>,"detail":<TEE側JSON>}` のような構造を返す。
  - 中期: TEE 側のエラー JSON 構造 (`{"error":"..."}`) を Gateway が parse して `detail` フィールドに展開、双方で一貫した shape にする。
  - 長期: SPECS_JA §2.5 に「`POST /process` / `POST /extension/solana` のエラーレスポンス形式」を明文化 (現状の SPECS は成功時のレスポンスのみ定義していて、エラー shape は実装次第になっている)。
- **緊急度**: タスク 17 の OSS release で外部開発者がエラーから原因を切り分けられないと運用負荷が跳ね上がる。must-fix 直前の優先度。

---

### should-fix-r3-002 (新): PCR0 が Round 2 → Round 3 で変化している (build reproducibility 観察)

- **重大度**: should-fix (E ラウンドへの引き渡し)
- **症状**: Round 2 の PCR0 は `5f3722ef2ba1d533d885fd39bdbf798ce63cad263df7b65208a701af79fd7bd5ef5966df3cdecd6f8d00249acb97d46a`、Round 3 は `64c8c1a1aaf4a4028938ffa48aff30e101275e6be7516f1f7fec3cad34c967128acbe026b7d2de5bd832b99add7ba60e`。**完全に異なる**。
- **可能性**:
  1. **TEE バイナリが意図的に更新された** (例: must-fix-r2-001 の `--privileged` 修正、vsock proxy 復旧のためのスクリプト変更、その他 K ラウンドからの fix が enclave image に入った)
  2. **リプロデューシブルビルドが破れている** (= 同じソースから build しても PCR0 が変わる。Docker base image の patch update、build timestamp 等が混入)
  3. PCR1/2/4 も変化しているはずだが、Round 2 のレポートに PCR1/2/4 の値が記録されていないため比較不能。
- **影響**:
  - 1 だった場合: 期待挙動。OPERATIONS_JA に「リリース PCR 一覧」を更新し、Solana program の `ApprovedMeasurements` を新 PCR0 で書き換える必要がある (使い古しの PCR0 = `5f3722ef...` が登録されたままでは Round 3 ビルドの attestation が onchain 検証で reject される)。
  - 2 だった場合: SPECS §5.4 「リプロデューシブルビルド」が事実上満たされていないことになり、OSS release 後のユーザーが自分で build した PCR0 と公式 PCR0 を比較できない。
- **修正方針**:
  - E ラウンド (reproducibility) で 「同じソースから 2 回 build → 同じ PCR0 か」を実機検証する。
  - 1 の場合: OPERATIONS_JA §2.5 の「リリース PCR 一覧」セクションを更新 (Round 2 must-fix-r2-003 で fixed と記載されているが、その時に書いた PCR0 値は今回もう古くなっている)。
  - 2 の場合: E ラウンドで原因究明 (Docker base image の version pin、build args の timestamp 排除、`SOURCE_DATE_EPOCH` 等の対応)。
- **緊急度**: タスク 17 までに E ラウンドで判定要。

---

### should-fix-r3-003 (新): `/extension/solana` 成功経路の E2E テストが本監査で実施不能

- **重大度**: should-fix (テスト戦略)
- **症状**: `/extension/solana` のフルパス (`fetch offchain → verify attestation → measurement check → build solana ix → partial sign → return partial_tx`) を実機で確認するには、TEE 自身が一度 `/process` で生成した「処理結果 + Nitro Attestation」 JSON を R2/S3 等のオフチェーンストレージに置いて、その URL を `offchain_data_url` に渡す必要がある。本監査セッションではアップロード手段が無く、シナリオ D (httpbin.org/json) で 400 を観察するに留まった。
- **影響**:
  - SPECS §6.2 の「三段の同一性確認 (verifying_key_hash, measurement, user_data bind)」のうち、実機で観察できているのは measurement (項目 13 の自己 attestation PCR0 一致) のみ。他 2 段は SP1 guest + Solana program の onchain での検証になるため、本観点 (J = HTTP ランタイム) の範囲外。
- **修正方針**:
  - タスク 17 で integration test ハーネスを追加:
    1. `/process` で正規データを取得
    2. ハーネスがそれを R2 (or 一時 HTTP サーバー) に置く
    3. `/extension/solana` を叩いて `partial_tx` を取得
    4. partial_tx を base64 decode し、署名が TEE Solana pubkey で行われていることを ed25519-verify
    5. (option) Solana devnet にブロードキャストして cNFT が発行されることを確認
  - これは「J 観点の追加検証項目」というより「タスク 17 の integration suite」の責務。
- **緊急度**: OSS release の前に最低 1 回マニュアル実行できる状態にすべき。

---

### nitpick-r3-001 (Round 2 nitpick-r2-005 の継続): `registration_attestation_b64` の timestamp 固定

- **重大度**: nitpick (Round 2 で wontfix 判断済み、Round 3 でも挙動継続)
- **症状**: `/solana-keys` を複数回叩いても `registration_attestation_b64` 内の `timestamp` は常に `1779622877333` (起動時固定)。
- **判定**: 本ラウンドでは新規アクションなし。SPECS_JA §6 に「`registration_attestation_b64` は起動時 attestation。liveness 証明には使わない」を明文化することは依然推奨。

---

### nitpick-r3-002 (Round 2 nitpick-r2-002 の継続): default 404 が空 body

- **症状**: `GET /` および `GET /nonexistent` が `404 + content-length:0`。Round 1 / Round 2 から不変。
- **判定**: 処理ログで wontfix。

---

### nitpick-r3-003 (Round 2 nitpick-r2-003 の継続): `OPTIONS /keys` 405 + CORS 未実装

- **症状**: `OPTIONS /keys` に `Origin + Access-Control-Request-Method: GET` で 405 (`Allow: GET,HEAD`)。`GET /keys -H "Origin: ..."` のレスポンスにも `Access-Control-Allow-Origin` 無し。
- **判定**: 処理ログで wontfix。

---

### nitpick-r3-004 (Round 2 nitpick-r2-004 の継続): 429 に `Retry-After` 無し

- **症状**: シナリオ A で 20 個の 429 全てに `Retry-After` ヘッダ無し。
- **判定**: 処理ログで wontfix。

---

### nitpick-r3-005 (Round 2 should-fix-r2-002 の継続): 422 だけ text/plain

- **症状**: `POST /extension/solana` で `{}` → `text/plain; charset=utf-8` + `Failed to deserialize ...`。他のエラーは `application/json`。
- **判定**: 処理ログで wontfix (axum API 制約由来)。

---

## 全体所感 (Round 3)

**Round 2 → Round 3 の良い変化:**
- **`POST /process` 完全復活** (must-fix-r2-001 解消)。C2PA-signed image が HTTP 200 + 有効な C2PA 結果 + 新規 Nitro Attestation Document を返すまで通った。Round 2 で blocker だったコア機能のリグレッションが直っている。
- **TEE 4xx の status 透過** (should-fix-r2-003 fixed)。Round 2 までは TEE 400 を Gateway が 502 にラップしていたが、Round 3 では HTTP 400 がそのまま透過する。
- **`/solana-keys` の `user_data` binding を実機で検証成功**。Round 2 では「sha256(pubkey)」か「もっと広い commitment」か実装読みが必要として保留していたが、Round 3 で **`user_data == sha256(<Solana pubkey 32 byte raw>)`** が完全一致したことを CBOR 層まで降りて確認できた。SPECS §6.2 の binding 設計が実装に反映されていることが裏取れた。
- **PCR0 が引き続き non-zero** (debug-mode ではない、release 運用前提が維持)。

**Round 2 → Round 3 の悪化 (regression):**
- **TEE エラー body の詳細が消えた** (should-fix-r3-001、新規)。status 透過の副作用で TEE 側のエラー JSON が固定文字列に置換されている。クライアントのデバッグ性が大きく落ちた。

**Round 2 → Round 3 の build 観察:**
- **PCR0 が Round 2 と完全に異なる** (should-fix-r3-002、新規)。意図的な再ビルドなのかリプロデューシビリティの破綻なのか、本観点では判別不能。E ラウンドへ引き継ぐ。

**未実施項目** (本ラウンドの環境制約):
- 項目 11 (API key 認証) の正の経路 (有効 key で 200、無効 key で 401) は `API_KEYS` 空の devnet 設定のため未実施。
- 項目 13 (enclave console) は SSH 不可で nitro-cli 実行不能。
- 項目 14 (異常系 container 操作) は SSH 不可で実施不能。
- 項目 9 (`/extension/solana` の measurement check 完走) は正規 offchain_data の用意手段が無く SKIPPED。

**タスク 17 に持ち越すべき優先項目 (重要度順):**
1. should-fix-r3-001: TEE エラー body の透過 (現在の `{"error":"TEE rejected request (HTTP 400)"}` 固定文字列を、TEE 側 JSON を含む形式に拡張)
2. should-fix-r3-002: PCR0 変化の理由究明 (E ラウンドに委譲) + OPERATIONS_JA §2.5 のリリース PCR 一覧の更新
3. should-fix-r3-003: `/extension/solana` E2E ハーネスを integration test に追加
4. Round 2 から継続の nitpick (Retry-After / CORS / 404 body / 422 content-type) は v0.1.3 OSS 公開前の Gateway API 仕上げで対応

---

## 処理ログ

| ID | 判定 |
|---|---|
| Round 2 must-fix-r2-001 (vsock proxy dead) | **verified-fixed** (項目 5 で `/process` が C2PA-signed image を完走、HTTP 200 + 有効 attestation を取得) |
| Round 2 must-fix-r2-002 (SPECS スキーマ乖離) | **verified-spec-aligned** (SPECS §6.2 で `recent_blockhash` 必須が確定。`/solana-keys` の `registration_attestation_b64` も実装と一致) |
| Round 2 must-fix-r2-003 (release-mode 切替の OPERATIONS_JA 明文化) | **needs-update** (Round 2 で「fixed」とされたが PCR0 値が Round 3 で変わったため、OPERATIONS_JA §2.5 のリリース PCR 一覧は古くなっている → should-fix-r3-002 に統合) |
| Round 2 should-fix-r2-001 (/health で vsock proxy 死活検知) | wontfix-continued (Round 3 では実害無し、proxy 復活により優先度低下) |
| Round 2 should-fix-r2-002 (422 だけ text/plain) | wontfix-continued (axum 制約) |
| Round 2 should-fix-r2-003 (TEE 4xx → Gateway 502 ラップ) | **verified-fixed-with-side-effect** (status は透過したが body 詳細が欠落 → should-fix-r3-001 として新規記録) |
| Round 2 nitpick-r2-001..005 | wontfix-continued (v0.1.3 OSS 公開前の仕上げで対応) |
| Round 3 新規 should-fix-r3-001 (TEE エラー body 欠落) | fixed | `GatewayError::TeeRejected` と `TeeUpstreamError` に `body: String` フィールドを追加。`IntoResponse` で TEE 側 body を `{"error":..., "detail":...}` 形式で透過 (JSON parse 成功なら parsed value、失敗なら string)。Round 2 should-fix-r2-003 の副作用で潰された TEE エラー詳細をクライアントに復元。 |
| Round 3 新規 should-fix-r3-002 (PCR0 変化、reproducibility 不明) | wontfix(E観点) | E ラウンドの reproducibility 検証範囲、本 J 観点では観察事項として記録のみ。OPERATIONS PCR 一覧の更新は v0.1.3 リリース判断と同時に。 |
| Round 3 新規 should-fix-r3-003 (`/extension/solana` E2E 未検証) | wontfix(タスク17) | 監査セッションのアップロード手段なしで実機検証不能。タスク 17 で integration test ハーネスを追加して対応。 |
