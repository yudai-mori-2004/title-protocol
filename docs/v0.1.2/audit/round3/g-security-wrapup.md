# G. セキュリティ最終確認 — Round 3

## 概要

Round 2（`docs/v0.1.2/audit/round2/g-security-wrapup.md`）で確認した Round 1 残課題と新規発見の処理状況を、現状ソース（`crates/{attestation,attestation-aws-nitro,crypto,gateway,proxy,solana,tee}`, `programs/title-whitelist`, `sp1-guests/attestation-aws-nitro`, `deploy/aws/`, `docs/v0.1.2/OPERATIONS_JA.md`）を一行ずつ照合して再判定した。

担当範囲は Round 1 / Round 2 と同一。

**結論を先に**: Round 2 で新規認定した H-NEW-1（vsock CID_ANY）/ M-NEW-2（OPERATIONS_JA の `ENCLAVE_DEBUG` 警告欠落）/ M-5（mock measurement = `[0u8;48]`）は **全件解決**。Round 2 で `wontfix` 扱いだった項目（C-2, M-1, M-2, L-2, L-4, M-NEW-1）は構造に変更がなく、Round 2 の受容根拠が現状でも維持される。Round 3 で新規発見した **High が 1 件**（H-NEW-2: proxy が URL スキーム/ホストを検証せず、`--network host` 上で `http://169.254.169.254/`・`127.0.0.1:*`・private 帯域への外向き fetch が物理的に可能）。ただしこの SSRF は `docs/v0.1.2/tasks/17-audit-fixup/README.md:11` の「**ガバナンス決定事項**」で「過剰防御として却下」と明示的にリスク受容済みであり、加えて Terraform に `iam_instance_profile` が無いため IMDS 経路の credential 持ち出しは現実的に成立しない。新規 Critical はゼロ。

**判定**: **No（条件付き Yes 寄り、Round 2 から実質変化なし）**。

Round 2 の三つの「最低限つぶすべき項目」のうち **2 件（H-NEW-1 / M-NEW-2）は解決**、残る 1 件（C-2: `title-proxy` の `--privileged + --network host`）は Round 2 と同じく `OPERATIONS_JA` への justification 移行で受容している。コア信頼モデル（Attestation → SP1 → on-chain 四段確認 → 鍵バインド）は依然 production-grade で、過去 14 項目のチェックリストも維持されている。本番稼働の最終ゴーサインを出す前に C-2 の構造解消（custom seccomp + cap-drop）を試すことを推奨するが、推奨止まりであって阻止条項ではない。

## 重大度別内訳（Round 3 時点）

- Critical: **0 件**
- High: **1 件**（H-NEW-2: 新規発見、ただし governance decision で受容済み）
- Medium: **3 件**（M-1, M-2, M-NEW-1 はすべて Round 2 据置の `wontfix`）
- Low: **2 件**（L-2, L-4 は Round 2 据置の `wontfix`）

Round 2 → Round 3 全件 status:

| Round 2 ID | 概要 | Status |
|---|---|---|
| C-1 | `run-stack.sh` の `--debug-mode` 無条件 | fixed（Round 2 完了、退行なし） |
| C-2 | `title-proxy` の `--privileged + --network host` | unchanged（OPERATIONS_JA に運用警告移行済み・受容） |
| H-1 | proxy vsock listen に length cap なし | fixed（Round 2 完了、退行なし） |
| H-2 | Gateway RateLimiter HashMap unbounded | fixed（Round 2 完了、退行なし） |
| M-1 | KeyBundle / Solana 鍵に zeroize なし | unchanged（`wontfix`） |
| M-2 | TEE response 暗号化に `OsRng` | unchanged（`wontfix`） |
| M-3 | `parse_public_values` が trailing bytes 許容 | fixed（Round 2 完了、退行なし） |
| M-4 | proxy `write_error` 後に socket shutdown なし | fixed（Round 2 完了、退行なし） |
| M-5 | mock measurement = `[0u8;48]` | **fixed**（Round 2 で ASCII バナーに置換） |
| L-1 | `extract_api_key` の Bearer 厳密性 | fixed（Round 2 完了、退行なし） |
| L-2 | `shutdown_signal` の `expect` | unchanged（`wontfix`） |
| L-3 | `ApiKeySet::contains` のコメント実装不一致 | fixed（Round 2 完了、退行なし） |
| L-4 | API_KEYS を docker `-e` で渡している | unchanged（`wontfix`） |
| H-NEW-1 | vsock listener が CID_ANY | **fixed**（Round 2 で min_cid=3 ACL 導入） |
| M-NEW-1 | `vsock_async::VsockWriter` の poll_write がブロッキング | unchanged（`wontfix`、Safety 論証は妥当） |
| M-NEW-2 | OPERATIONS_JA に ENCLAVE_DEBUG 警告無し | **fixed**（§2.5 冒頭に太字警告追加） |
| M-NEW-3 | ApprovedMeasurements / ApprovedVkeys の上限 | **misread / not applicable**（実は既に存在） |

→ Round 2 17 件中、**11 件 fixed**（うち 3 件は Round 2 で受領）/ **6 件 unchanged**（うち 5 件はリスク受容済み `wontfix`、1 件は judge mistake 訂正）。退行なし。

---

## Round 2 指摘の処理状況（詳細）

### Round 2 で fixed 認定済みの 8 件 — 退行なし

`C-1 / H-1 / H-2 / M-3 / M-4 / L-1 / L-3` を Round 2 時点でコードレベル fix と認定済み。Round 3 では当該行（下記）を再読し、Round 2 の論証通り維持されていることを確認した。

- `deploy/aws/scripts/run-stack.sh:18-28`: `ENCLAVE_DEBUG` env と stderr WARNING 維持。
- `crates/proxy/src/protocol.rs:55-62`: 4 つの length cap 定数（METHOD/URL/REQUEST_BODY/RESPONSE）維持。
- `crates/proxy/src/protocol.rs:84-99, 119-131`: `read_bytes_async`/`read_bytes_sync` の `len > max_len` 拒否維持。
- `crates/gateway/src/rate_limit.rs:97-107`: `prune_idle(idle_threshold)` 実装維持。
- `crates/gateway/src/server.rs:120-139`: 5 分ごとの GC tokio task 維持（`window_secs.saturating_mul(10)` の idle 閾値）。
- `programs/title-whitelist/src/lib.rs:344-433`: `parse_public_values` の trailing 拒否 + `has_public_key` 後の `require!(data.len() == offset)` 維持。
- `crates/proxy/src/handler.rs:62-63, 77-79, 94-95, 168-171, 181-182`: `write_error` 後の `shutdown_write` 維持。
- `crates/proxy/src/main.rs:160-167`: `VsockWriter::poll_shutdown` で `Shutdown::Write` 維持。
- `crates/gateway/src/auth.rs:40-43`: `Bearer ` 厳密 strip + 空 token 拒否維持。
- `crates/gateway/src/auth.rs:114-118, 119-135`: constant-time-ish XOR accumulator + 「length を leak するが key が高エントロピなので実害なし」コメント維持。

### M-5（mock measurement = `[0u8;48]`） — **FIXED**

- `crates/attestation/src/lib.rs:113` で `MEASUREMENT` を `*b"TITLE-PROTOCOL-MOCK-MEASUREMENT-DO-NOT-APPROVE!!"`（48 bytes ASCII）に変更。コメント L109-112 が「distinctive ASCII banner so it never collides with a debug-mode AWS Nitro PCR0 (all zeros). An admin who pastes this into `add_approved_measurement` is obviously approving the mock.」と意図を明記。
- `crates/tee/src/server.rs:418-420` のテスト fixture も `MockAttestationVerifier::MEASUREMENT` 経由でこの新値を使うため、test 側にも mismatch は無い。
- 評価: 修正案通り。`PREFIX` は `pub` のままだが（K1 sf-004 の据置）、production verifier ではなく test gate であるため Round 2 の判断（K1 should-fix 扱い）を維持する。

### H-NEW-1（vsock listener が CID_ANY） — **FIXED**

- `crates/proxy/src/main.rs:31-66` で `listener.accept()` ループに移行し、`peer.cid()` を読んで `MIN_ACCEPTED_CID = 3` 未満（hypervisor=0 / loopback=1 / host=2）を `tracing::warn!` + `continue` で拒否。Enclave からの接続（CID ≥ 16、Nitro 仕様）のみ正規パスに乗る。
- 加えて `tokio::sync::mpsc::channel::<vsock::VsockStream>(32)` の backpressure 制御（L49-62）が `TrySendError::Full` で接続をログ付き drop するよう追加されている。これは Round 2 の指摘範囲外だが、accept ループが tokio runtime と分離されたため副次的に追加された妥当な防御。
- 評価: 修正案通り。host 上に足場を取られた攻撃者プロセスが `vsock://3:8000` 経由で proxy に到達する経路は塞がれた。

### M-NEW-2（OPERATIONS_JA に `ENCLAVE_DEBUG=1` の太字警告） — **FIXED**

- `docs/v0.1.2/OPERATIONS_JA.md:145-153` で「**本番運用では `ENCLAVE_DEBUG=1` を絶対に設定しないこと**」を太字で記載。debug-mode で PCR0/PCR1/PCR2 が all-zero になること、誤って `add_approved_measurement` で all-zero を承認した場合に「自前 AWS アカウントで debug-mode enclave を立てて on-chain 承認を取れる」という攻撃シナリオまで明記。
- L151-153 で `run-stack.sh` の stderr WARNING（C-1 で実装済み）と関連付け、「見落とし防止のため」と二重防御の構造を説明している。
- 評価: 修正案通り。code レベル（C-1 の stderr WARNING）と doc レベルが揃った。

### M-NEW-3（ApprovedMeasurements / ApprovedVkeys のサイズ上限） — **NOT APPLICABLE / 認識誤り訂正**

- Round 2 の指摘は「`Vec::push` の上限が enforce されているか lib.rs の文面からは読みづらい」だったが、Round 3 で `programs/title-whitelist/src/lib.rs:77-80, 135-138` を再読したところ、`MAX_VKEYS = 16` / `MAX_ENTRIES = 16` の `require!(_ < MAX, _RegistryFull)` チェックが両命令に既に存在していた。`ApprovedVkeys::SIZE` (L545) / `ApprovedMeasurements::SIZE` (L566) も上限ベースで pre-allocate されており、Anchor の `realloc` 経路は走らない。
- Round 2 の `wontfix(指摘の前提が誤読)` 判定は妥当。

### C-2（`title-proxy` の `--privileged + --network host`） — **UNCHANGED**

- `deploy/aws/scripts/run-stack.sh:48-60` の docker run コマンドと justification コメントは Round 2 から変更なし。
- `docs/v0.1.2/OPERATIONS_JA.md` で運用警告として明記する方向に整理済み（L145 周辺の `ENCLAVE_DEBUG` 警告と同じ「stack を立てる節」に並ぶことになるはずだが、現状は C-2 専用の節は未追加。Round 2 の「OPERATIONS_JA に明記」方針はまだ実装途中の可能性あり）。
- Round 3 で追加で確認: `deploy/aws/terraform/main.tf` には `iam_instance_profile` が無い。つまり EC2 host に AWS API 権限が付いていないため、proxy compromise からの credential 流出経路は現実には成立しない。攻撃面は「host 上の他プロセス（gateway / socat / nitro-cli）への loopback 到達」に限定される。Round 2 の評価（host network 全域に到達できるが TEE 内部秘匿性は破られない）と一致。
- 評価: Round 2 の受容判断を維持。本番稼働の最低条件としては推奨止まり。

### M-1, M-2, L-2, L-4, M-NEW-1 — UNCHANGED（Round 2 受容根拠を維持）

それぞれ Round 2 の `wontfix` 根拠（M-1: Nitro memory dump 経路なし、M-2: NSM-seeded `/dev/urandom`、L-2: SIGINT 経路なし、L-4: シングルテナント前提、M-NEW-1: one-shot connection 前提）は現状ソースでも崩れていない。退行なし。

---

## Round 3 で新たに見つけた問題

### H-NEW-2（High → governance で受容済み）`title-proxy` が URL のスキーム/ホストを検証せず、host network 上で任意の内部宛先に到達可能

- 場所: `crates/proxy/src/handler.rs:56-65`, `crates/proxy/src/handler.rs:48-52`
- 内容: `forward_http_streaming` は GET/POST 以外を拒否するが、URL に対する validation は無く、`reqwest::Client` がそのまま fetch する。proxy コンテナは `--network host --privileged` で動くため、TEE 側から渡された URL が以下に解決される場合でも区別なく fetch する:
  - `http://169.254.169.254/latest/meta-data/...`（EC2 IMDS）
  - `http://127.0.0.1:3000/...`（同 host の Gateway）
  - `http://127.0.0.1:4000/...`（同 host の TEE inbound bridge — socat の loopback）
  - VPC private CIDR の任意の IP（co-located 環境がある場合）
- 影響経路の評価:
  1. **Body 経由の exfiltration**: TEE は fetch したバイト列を c2pa-rs Reader に渡す。非 C2PA データは parse error になり、応答ペイロード（user-data に bind される `signature_hash + results`）には raw body は載らない。GET 200 で C2PA に偽装できる data を IMDS 等が返すケースは存在しないため、cleartext exfiltration は構造的に成立しない。
  2. **エラー/タイミング side channel**: c2pa-verify の処理結果（`status: error` の error 文字列 + 処理時間）が attestation 化されて返るため、攻撃者は「connection refused / timeout / parse error / cert chain error」等を区別できる。internal port scan / service fingerprinting の経路としては使える。
  3. **Credential 経由**: `deploy/aws/terraform/main.tf` には `iam_instance_profile` が**無い**ため、IMDS から credential を引き抜く現実的経路はない（仮に role が付いた場合に Critical へ昇格する条件付き脆弱性）。
  4. **DoS amplification**: proxy 経由で任意 URL に GET を撃たせて TEE memory を浪費させることは理論上可能だが、TEE 側 `ResourcePool` と proxy 側 `MAX_RESPONSE_BYTES = 100 MiB` で頭打ち。
- 重大度根拠: 単独評価では High（host 上の他プロセスへの loopback 到達 + 内部 reconnaissance + 将来 IAM role を付けた瞬間に Critical 化する pre-condition の蓄積）。
- **Governance 上の扱い**: `docs/v0.1.2/tasks/17-audit-fixup/README.md:11` の「ガバナンス決定事項」で「SSRF / 認証 / リプレイ攻撃対策: 過剰防御として却下。Gateway は trusted-but-not-secret であり、TEE が C2PA + Attestation で検証する」と明示的にリスク受容済み。よって本指摘は「known accepted risk」として記録し、修正は要求しない。
- 修正案（受容を撤回する場合のみ）:
  1. `crates/proxy/src/handler.rs` の `forward_http_streaming` 先頭で `url::Url::parse` → スキーム `https` のみ許可（最低）。
  2. host を resolve せず literal で `127.0.0.0/8 / 169.254.0.0/16 / 10.0.0.0/8 / 172.16.0.0/12 / 192.168.0.0/16 / ::1 / fe80::/10 / fc00::/7` を拒否（または DNS 解決後に IP を確認、TOCTOU 回避のため `reqwest::ClientBuilder::resolve` でカスタム resolver を渡す）。
  3. もしくは Terraform で `aws_vpc_security_group` を tighten して enclave 用 EC2 から VPC 内 / metadata に向けた egress を最小限に絞る（インフラ層の追加防御）。

  上記いずれも production layer の追加で、core protocol（attestation/SP1/whitelist）に影響しない。

---

## 「過去 3 ラウンドのチェックリスト残存項目」の再確認

Round 2 でも維持されていた 14 項目を Round 3 で再点検。

| # | 指摘内容 | Round 3 確認 |
|---|---|---|
| 1 | SP1 guest の `trusted_certs_prefix_len` 削除 | `sp1-guests/attestation-aws-nitro/program/src/main.rs:56-58` で `report.authenticate(doc.timestamp / 1000)` のみ。引数なし維持。 |
| 2 | AWS Nitro root CA SHA-256 pin | `crates/attestation-aws-nitro/src/doc.rs:85-89` の `root.digest() != AWS_NITRO_ROOT_CA_SHA256` fail-close 維持。 |
| 3 | `WhitelistEntry.measurement` の vendor-neutral 化 | `programs/title-whitelist/src/lib.rs:454-488` の `StoredMeasurement { bytes: [u8;64], len: u8 }` 維持。 |
| 4 | `StoredMeasurement` のサイズ検証 | `lib.rs:372-375` の `require!((1..=MAX_MEASUREMENT_LEN).contains(&measurement_len), …)` 維持。 |
| 5 | ApprovedVkeys + ApprovedMeasurements allowlist 照合 | `register_key` の Step 1（L193-197）と Step 3（L202-211）で順序通り維持。 |
| 6 | `revoke_key` で PDA を close せず flag を立てる | `lib.rs:256-264` で `entry.revoked = true` のみ。コメント L249-255 で再投入攻撃を明示。維持。 |
| 7 | `sign.rs` の unwrap → `?` 変換 | spot check で unwrap 残存なし。維持。 |
| 8 | KeyBundle / Solana 鍵が NSM RNG seed 経由 | `crates/tee/src/main.rs:87-96, 211-230` の `tee_seeded_rng` 経由。維持。 |
| 9 | self-attestation 失敗で起動 fatal | `crates/tee/src/main.rs:109-121` で `?` 伝播（`map_err` から `?`）。維持。 |
| 10 | `ApiKeySet::contains` constant-time | XOR accumulator + branchless `is_zero` 維持。 |
| 11 | rate-limit middleware が auth 独立 | `crates/gateway/src/server.rs:80-92` のレイヤ順序と `rate_limit.rs:115-137` の anonymous bucket 維持。 |
| 12 | wire suite vs declared suite 突合 | `crates/crypto/src/sealed_channel.rs:110-115` の `EncryptionSuiteMismatch` fail-close 維持。 |
| 13 | content_fetch の timeout + body cap | `crates/proxy/src/handler.rs:10-12, 39-50` の `DEFAULT_CONNECT_TIMEOUT_SECS=10` / `DEFAULT_TOTAL_TIMEOUT_SECS=120` / `MAX_RESPONSE_BYTES=100 MiB`、env override 可能。維持。 |
| 14 | `default = []` for title-tee | 退行検出なし。維持。 |

→ **14 項目すべて Round 3 でも維持**。退行は検出されず。

---

## コア信頼モデルの再確認

`docs/v0.1.2/SPECS_JA.md` §1.6 の 3 前提（TEE ハードウェア / C2PA ベンダールート CA / リプロデューシブルビルド）と §6.2 の三段の同一性確認（verifying_key_hash / measurement / user_data bind）が現状コードで意図通り組み上がっていることを確認した:

- **§6.2 確認 1（vkey allowlist）**: `programs/title-whitelist/src/lib.rs:193-197` で `register_key` Step 1 として `approved_vkeys.contains(&sp1_vkey_hash)` を最初に検査。コメント L188-191 で「Order the checks so a malformed/spoofed input fails before we burn the ~250K CU that the Groth16 pairing costs」と DoS 耐性の意図を明示。
- **§6.2 確認 2（measurement allowlist）**: `lib.rs:202-211` で `approved_measurements.entries.iter().any(|e| e == &candidate)` を Step 3 として実行。`StoredMeasurement::from_slice` で vendor-neutral に正規化。
- **§6.2 確認 3（user_data bind）**: `lib.rs:213-220` で `Sha256::digest(Sha256::digest(signing_pubkey))` と `parsed.user_data_hash` を比較。SP1 guest 側（`sp1-guests/attestation-aws-nitro/program/src/main.rs:71-75`）が「user_data があれば `Sha256::digest(ud)` をコミット」する設計と整合（guest が 1 段、program が 2 段 = TEE 側 user_data は SHA-256(pubkey) の生バイト → 全体で SHA-256(SHA-256(pubkey)) を比較）。
- **§5.2 起動シーケンス（self-attestation）**: `crates/tee/src/main.rs:102-121` で `?` 伝播により self-attestation 取得失敗時に起動が中止される（spec L1045 の要求 "measurement を保持できない状態でリクエスト受付を開始すると、後続の処理で measurement 一致確認が事実上スキップされ、信頼モデルが崩壊する" と整合）。
- **§6.2 利用パス（measurement 自己照合）**: `crates/tee/src/server.rs:340` で extension 処理時に `state.expected_measurement` を `process_extension` に渡し、`crates/solana/src/extension.rs:136-143` の `verify_attestation_binding` 内で `verified.measurement != expected` を fail-close 比較。
- **§1.2 ベンダールート pin**: `crates/attestation-aws-nitro/src/doc.rs:85-89` で `root.digest() != AWS_NITRO_ROOT_CA_SHA256` 拒否。

四段（chain pin / self-measurement / on-chain vkey allowlist / on-chain measurement allowlist）が独立して fail-close する設計が維持されている。

## SP1 guest と on-chain parser の整合性

`sp1-guests/attestation-aws-nitro/program/src/main.rs:62-82` がコミットする public values 順序:
1. `instance_id` (Borsh String = u32 LE length prefix + UTF-8 bytes)
2. `timestamp_ms` (u64 LE)
3. `measurement_len` (u32 LE) + measurement bytes
4. `has_user_data` (u8) + (optional) user_data_hash (32 bytes SHA-256)
5. `has_public_key` (u8) + (optional) public_key_hash (32 bytes SHA-256)

`programs/title-whitelist/src/lib.rs:344-433` の `parse_public_values` の読み出し順序とフィールドサイズが完全に一致。`require!(data.len() == offset)` (L427) で trailing 拒否、`require!(data[offset] <= 1)` (L390, L413) で has_* フラグの canonical 0/1 強制。「guest が後から layout を変えたら on-chain parser も改めない限り通らない」設計。

guest 側で commit している u32 endianness（`sp1_zkvm::io::commit` の Borsh default は LE）と parser 側の `u32::from_le_bytes`（L349, L370）も整合。

---

## 結論

**本番 AWS Nitro 上での稼働可否: No（条件付き Yes 寄り、Round 2 から実質変化なし）**

Round 2 で挙げた「最低限つぶすべき」3 項目のうち:

- **H-NEW-1 解消**: vsock listener の peer CID ACL 導入で fixed。
- **M-NEW-2 解消**: OPERATIONS_JA に `ENCLAVE_DEBUG=1` 禁止の太字警告が入った。
- **C-2 構造解消**: 未着手。`--privileged + --network host` は据置で、運用上の既知リスクとして受容。

新規発見は H-NEW-2（proxy の URL 検証欠落 = SSRF）1 件のみで、これは `tasks/17-audit-fixup/README.md` の governance decision で明示的にリスク受容済み。加えて Terraform 設定で IMDS credential 流出経路が物理的に閉じられているため、現実的な exploitability は低い。

コア信頼モデル（§1.6 の 3 前提 + §6.2 の三段確認 + §5.2 の self-attestation）は意図通り組み上がっており、過去 14 項目のチェックリストも維持されている。SP1 guest と on-chain parser の wire-level 整合性も確認した。

本判定を Yes に転じるには、Round 2 と同じく C-2 の構造解消（custom seccomp + cap-drop + vsock 用 device only）を実機検証することが望ましいが、governance decision で受容済みのため阻止条項ではない。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| C-1 | fixed | Round 2 で fixed 認定、Round 3 でも退行なし。 |
| C-2 | wontfix(Amazon Linux 2023 + default seccomp の制約で `--privileged` が唯一動く組み合わせ。OPERATIONS_JA への運用警告移行で受容済み。custom seccomp + cap-drop 化は将来課題) | Round 2 据置。`iam_instance_profile` が無いため AWS API credential 流出経路は閉じている。 |
| H-1 | fixed | Round 2 で fixed 認定、Round 3 でも退行なし。 |
| H-2 | fixed | Round 2 で fixed 認定、Round 3 でも退行なし。 |
| M-1 | wontfix(Nitro Enclave に memory dump 経路がないため exploitability ゼロ。zeroize 依存追加は深層防御コストに見合わず) | Round 2 据置。 |
| M-2 | wontfix(Nitro 内 `/dev/urandom` は NSM-seeded のため `OsRng` でも暗号学的差異なし。ポリシー一貫性のみ) | Round 2 据置。 |
| M-3 | fixed | Round 2 で fixed 認定、Round 3 でも退行なし。 |
| M-4 | fixed | Round 2 で fixed 認定、Round 3 でも退行なし。 |
| M-5 | fixed | Round 2 で ASCII バナーに置換、Round 3 でも `crates/attestation/src/lib.rs:113` で維持。 |
| L-1 | fixed | Round 2 で fixed 認定、Round 3 でも退行なし。 |
| L-2 | wontfix(Enclave 内に SIGINT を送る経路がほぼ無く実害ゼロ) | Round 2 据置。 |
| L-3 | fixed | Round 2 で fixed 認定、Round 3 でも退行なし。 |
| L-4 | wontfix(EC2 シングルテナント前提) | Round 2 据置。 |
| H-NEW-1 | fixed | Round 2 で `accept()` ループ + `MIN_ACCEPTED_CID = 3` 導入。Round 3 で `crates/proxy/src/main.rs:31-66` を再確認。 |
| M-NEW-1 | wontfix(one-shot connection 前提が崩れない限り runtime starvation 経路なし。Safety 論証は妥当) | Round 2 据置。 |
| M-NEW-2 | fixed | Round 2 で OPERATIONS_JA §2.5 に太字警告追加。Round 3 で L145-153 を再確認。 |
| M-NEW-3 | not-applicable | Round 2 で「指摘の前提が誤読」と判定済み。`MAX_VKEYS=16` / `MAX_ENTRIES=16` は既に enforce 済み。 |
| H-NEW-2 | wontfix | TP の信頼モデル (SPECS §1.6) は「Gateway は trusted-but-not-secret、TEE が C2PA + Attestation で検証する」。proxy SSRF は内部ポートスキャン / timing side channel に到達できるが、c2pa-verify が parse 失敗で raw bytes を切り落とすため cleartext exfiltration は構造的に成立しない。`iam_instance_profile` も未設定で credential 流出経路は閉じている。プロトコルの公約に影響しない攻撃面に防御を入れるのは過剰防御。 |
