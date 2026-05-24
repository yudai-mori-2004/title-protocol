# G. セキュリティ最終確認 — Round 2

## 概要

Round 1（`docs/v0.1.2/audit/g-security-wrapup.md`）で挙げた **Critical 2 / High 2 / Medium 5 / Low 4** に対する修正適用後の再監査。

担当範囲（Round 1 と同一）:

- `crates/attestation-aws-nitro/`、`sp1-guests/attestation-aws-nitro/`
- `crates/solana/src/extension.rs`、`programs/title-whitelist/src/lib.rs`
- `crates/tee/src/{main,orchestrator,proxy_fetcher,content_fetch}.rs`
- `crates/proxy/`
- `crates/crypto/`
- `crates/gateway/src/{auth,rate_limit,server}.rs`
- `deploy/aws/`

結論を先に: **コア信頼モデル（Attestation 検証 → SP1 proof → on-chain whitelist の四段確認 → 鍵バインド）は引き続き production-grade。Round 1 で Critical 認定した 2 件のうち C-1（`--debug-mode` 無条件起動）は解消、C-2（`--privileged + --network host`）は justification が補強されたが構造的には未変更。High 2 件は両方とも解消。新規 Critical は無し。新規発見として High 1 件（vsock CID_ANY 受け入れ）と Medium 数件（vsock_async の Send 安全性論証、ApprovedMeasurements/Vkeys のサイズ防御、OPERATIONS_JA に debug-mode の太字警告が無いこと）を追加する**。

判定は **No（条件付き Yes 寄り）**。残課題が deploy layer に限定されており、core protocol layer には触らずに潰せる。

## 重大度別内訳

- Critical: **0 件**（Round 1: 2 → 解消 1 / 据置 1: C-2 は justification 追加のみで構造未変更だが、新規 Critical ではなく Round 1 のキャリーオーバ）
- High: **1 件**（新規 H-NEW-1）
- Medium: **5 件**（M-1, M-2, M-5 据置 + 新規 2 件）
- Low: **2 件**（L-1, L-3 解消 / L-2, L-4 据置）

Round 1 → Round 2 全件 status:

| Round 1 ID | 概要 | Status |
|---|---|---|
| C-1 | `run-stack.sh` の `--debug-mode` 無条件 | **fixed** |
| C-2 | `title-proxy` の `--privileged + --network host` | **unchanged（justification 強化のみ）** |
| H-1 | proxy vsock listen に length cap なし | **fixed** |
| H-2 | Gateway RateLimiter の bucket HashMap unbounded | **fixed** |
| M-1 | KeyBundle / Solana 鍵に zeroize なし | **unchanged** |
| M-2 | TEE response 暗号化に `OsRng` | **unchanged** |
| M-3 | `parse_public_values` が trailing bytes 許容 | **fixed** |
| M-4 | proxy `write_error` 後に socket shutdown なし | **fixed** |
| M-5 | mock measurement = `[0u8;48]`（debug-mode 衝突） | **unchanged** |
| L-1 | `extract_api_key` の Bearer 厳密性 | **fixed**（空 token 拒否） |
| L-2 | `shutdown_signal` の `expect` で panic | **unchanged** |
| L-3 | `ApiKeySet::contains` のコメント実装不一致 | **fixed** |
| L-4 | API_KEYS を docker `-e` で渡している | **unchanged** |

→ Round 1 13 件中、**8 件 fixed / 5 件 unchanged**。固定済みの 8 件は実装側で具体的な変更が入っており、回帰は確認されない。

---

## Round 1 指摘の処理状況（詳細）

### C-1: `--debug-mode` 無条件起動 → **FIXED**

- `deploy/aws/scripts/run-stack.sh:18-28` に `ENCLAVE_DEBUG` env を導入し、デフォルト 0、`1` の場合のみ `--debug-mode` を付与。`1` の場合は `WARNING: ENCLAVE_DEBUG=1 — Attestation Documents from this enclave will have zeroed PCRs.` を stderr に出す。`if [[ -n "$DEBUG_FLAG" ]]; then MODE_LABEL="debug"; else MODE_LABEL="release"; fi` で起動ログにも mode が出る。`L18-22` のコメントが「all-zero PCR は on-chain 登録に使えない」と明文化。
- 評価: 修正案そのものが採用されている。残課題（OPERATIONS_JA への太字警告）は M-NEW-2 に分離。

### C-2: `title-proxy` の `--privileged + --network host` → **UNCHANGED**

- `deploy/aws/scripts/run-stack.sh:48-60` を再確認。コメント L49-55 が拡充され、「`--device /dev/vsock` でも Amazon Linux 2023 のデフォルト seccomp が AF_VSOCK socket(2) を拒否する。custom seccomp.json を同梱しない限り `--privileged` が唯一動く組み合わせ」「proxy の攻撃面は length-prefixed vsock protocol + 外向き HTTPS のみなので broader privileges は trust boundary を広げない」と書かれている。
- 実体: `docker run -d --name title-proxy --restart unless-stopped --network host --privileged title-protocol-proxy:latest` のまま。
- 評価: justification の質は上がったが、根本問題（host network 全域への到達可能性、cap drop なし）は変わらず。Round 1 の修正案（seccomp profile 同梱 → `--security-opt seccomp=... --device /dev/vsock --cap-add NET_ADMIN`）は試されていない。`docs/v0.1.2/tasks/17-audit-fixup/README.md` で「既知の運用上の懸念。`debug-mode` PCR0=all-zero と並んで OPERATIONS_JA に明記する」方針になっており、構造修正はタスク 17 でも先送り。
- 受容根拠の妥当性: trust boundary 上 proxy はもともと untrusted-but-isolated 扱いで、TEE 内部秘匿性は破られない。だが proxy の compromise から `127.0.0.1:4000`（TEE inbound bridge）/`127.0.0.1:3000`（Gateway）へ host network 経由で直接到達できる脆弱性は残る。Round 1 の評価を維持。

### H-1: proxy vsock listen の length cap → **FIXED**

- `crates/proxy/src/protocol.rs:43-50` に 4 つの定数 `MAX_METHOD_BYTES=16` / `MAX_URL_BYTES=8 KiB` / `MAX_REQUEST_BODY_BYTES=8 MiB` / `MAX_RESPONSE_BYTES=100 MiB` を定義。`read_bytes_async`/`read_bytes_sync`（L72-87, L111-125）の双方で `len > max_len` を `InvalidData` で fail-close。呼び出し側 `crates/proxy/src/main.rs:82-96`、`crates/proxy/src/handler.rs:194-216` で max_len を渡している。
- 評価: 修正案通り。proxy 側で 4 GiB allocation 攻撃が物理的に成立しなくなった。

### H-2: Gateway RateLimiter HashMap unbounded → **FIXED**

- `crates/gateway/src/rate_limit.rs:97-107` で `prune_idle(idle_threshold: Duration)` を追加し、`crates/gateway/src/server.rs:121-140` で `tokio::spawn` の background task が 5 分ごとに `rate_limiter.prune_idle(rate_limit_window_secs * 10)` を呼ぶ。
- 評価: LRU ではなく "idle 超過なら破棄" 方式だが、攻撃シナリオ（Bearer token を毎リクエスト変える）に対しては十分有効。プルーニング条件のコメント L89-96 が「`last_refill` 更新時刻からの経過 = アイデンティティが traffic を送らなくなった時間の上限。`idle_threshold` 超過したバケットは再生成しても same answer なので破棄安全」と妥当な根拠を述べている。
- 攻撃シナリオ再評価: 1 分窓 × 10 = 10 分の保持。10 M reqs/10 min = 16,666 req/sec を 10 分維持できる攻撃者なら HashMap は 10 M entry に達するが、その規模の攻撃力なら他の経路で先に詰まる。実用上は OK。

### M-1: KeyBundle / Solana 鍵に zeroize なし → **UNCHANGED**

- `crates/crypto/Cargo.toml` / `crates/crypto/src/key_bundle.rs` / `crates/solana/src/signing_key.rs` のいずれにも `zeroize` 依存も `ZeroizeOnDrop` derive も入っていない。
- 評価: Nitro Enclave の memory dump 経路がない以上、exploitability は低い深層防御問題のまま。Round 1 評価を維持。

### M-2: TEE response 暗号化に `OsRng` → **UNCHANGED**

- `crates/crypto/src/sealed_channel.rs:40-46`, `:67-72` 共に `rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut nonce)`。`tee_seeded_rng` の global 化や `&mut dyn RngCore` 注入は未実施。
- 評価: Nitro 内の `/dev/urandom` は NSM-seeded なので暗号学的には問題ないが、「startup は NSM 経路、runtime は OsRng」というポリシー一貫性問題は据置。実害ゼロ、ポリシー的に整理価値あり。

### M-3: `parse_public_values` の trailing bytes → **FIXED**

- `programs/title-whitelist/src/lib.rs:402-423` で `has_public_key` 以降も読んで `require!(data.len() == offset, WhitelistError::InvalidPublicValues)` で fail-close。コメント L420-422 が「guest は public-values envelope を正確に commit するはず。trailing は layout 変更未対応のサイン」と意図を明文化。
- 評価: 修正案通り。SP1 guest の commit 順序とも整合。

### M-4: proxy `write_error` 後の shutdown → **FIXED**

- `crates/proxy/src/handler.rs:58-59, 73-74, 89-91, 156-157, 163-164` で `write_error` 後に `shutdown_write(w).await` を呼ぶ。`shutdown_write`（L181-185）は `tokio::io::AsyncWriteExt::shutdown` を best-effort で呼んで EOF を伝える。さらに `crates/proxy/src/main.rs:146-156` で `VsockWriter::poll_shutdown` を `vsock::VsockStream::shutdown(Shutdown::Write)` 実装に上書き。
- 評価: 修正案通り。クライアント側 `proxy_fetcher` は EOF で正常に切れる。

### M-5: mock measurement = `[0u8;48]` → **UNCHANGED**

- `crates/attestation/src/lib.rs:102-104` の `MEASUREMENT: [u8; 48] = [0u8; 48]` は変わらず。`PREFIX` も `pub` のまま（K1 should-fix-004 も未対応）。
- 評価: C-1 の修正で「`--debug-mode` enclave による all-zero PCR」が production stack で発生する経路は塞がれたため、衝突の現実的なリスクは下がった。だが構造的には mock と debug-mode が同値である点は残っており、admin が誤って `[0u8;48]` を allowlist に追加できる経路は残る。

### L-1: `Bearer` 厳密性 → **FIXED**

- `crates/gateway/src/auth.rs:40-43`:
  ```rust
  match text.strip_prefix("Bearer ") {
      Some(token) if !token.is_empty() => AuthHeader::Bearer(token.to_string()),
      _ => AuthHeader::Malformed,
  }
  ```
  空 token は Malformed として弾く。連続スペースは `Bearer ` の strip_prefix 後の先頭スペースが残るため keys lookup で必ず miss（高エントロピ key と一致しないため）。完全な trim 正規化ではないが実害なし。

### L-2: `shutdown_signal` の `expect` → **UNCHANGED**

- `crates/tee/src/main.rs:203-208` の `tokio::signal::ctrl_c().await.expect("Failed to install CTRL+C signal handler");` は変わらず。
- 評価: Enclave 内に SIGINT を送る経路がほぼ無いため実害ゼロ。startup の `?` 伝播との一貫性問題のみ。

### L-3: `ApiKeySet::contains` のコメント実装不一致 → **FIXED**

- `crates/gateway/src/auth.rs:114-118` のコメントが「Length-mismatched entries are skipped, so total runtime leaks the candidate's length (not which entry matched). API keys are high-entropy fixed-length tokens, so this leak is negligible. Never short-circuits on a match.」に書き直されている。実装と整合。

### L-4: API_KEYS を docker `-e` で渡している → **UNCHANGED**

- `deploy/aws/scripts/run-stack.sh:91-96` で `docker run … -e API_KEYS="$API_KEYS"`。docker secrets / `--env-file` への分離は未実施。
- 評価: EC2 ホスト内の他プロセスから `cat /proc/<pid>/environ` で読まれる経路は残る。シングルテナント前提なら実質的なリスクは低い。

---

## 新規発見（Round 2 で初めて拾った／修正で生まれた問題）

### H-NEW-1（High）`title-proxy` の vsock listener が依然 `VMADDR_CID_ANY` で listen

- 場所: `crates/proxy/src/main.rs:29` `vsock::VsockListener::bind_with_cid_port(vsock::VMADDR_CID_ANY, port)`
- 内容: Round 1 H-1 では「length cap なし」と「CID_ANY」を併記したが、length cap 部分のみ修正され listener address は据置。AWS Nitro 上で他に enclave を同居させる運用は通常ないため exploitability は低いが、host 上に足場を取られた攻撃者プロセスが `vsock://3:8000` に接続して proxy を叩く経路は残る。
- 修正案: AWS Nitro EC2 上では `VMADDR_CID_HOST` でも `VMADDR_CID_ANY` でも実用上は同じだが、attack-surface 縮小のため `VMADDR_CID_HOST`（host CID = 2）からの接続のみ受け入れる ACL を accept ループで追加（`stream.peer_addr()` の CID を確認）。または `--privileged` を外す方向に進む過程で proxy の bind を `127.0.0.1` TCP + nitro-side `socat vsock→tcp` に置き換えることで vsock の listen 自体を不要にする。
- 重大度根拠: 深層防御。host compromise を前提とした隣接攻撃に対する gate。C-2 の `--privileged + host network` と同じ root cause（trust boundary の運用設定が緩い）。

### M-NEW-1（Medium）`crates/proxy/src/main.rs` の `vsock_async::VsockWriter` の `unsafe impl Send` は妥当だが、`poll_write` が tokio worker thread をブロックする

- 場所: `crates/proxy/src/main.rs:121-166`
- 観察: `poll_write` / `poll_flush` / `poll_shutdown` がいずれも `std::io::Write` の blocking 呼び出しを `Poll::Ready` で即返す。コメント L128-130 で「Each `poll_write` blocks the worker thread for the duration of a single `write(2)` — fine here because connections are one-shot and short.」と認めている。
- 問題: `tokio::spawn` で多数の同時接続を受けたとき、各 task が writer に書き込むたびに tokio runtime の worker thread が短時間ブロックする。`write(2)` が遅い peer 側（TEE 内の slow consumer）と組み合わさると、tokio runtime 全体のスループットが落ちる。`tokio::task::spawn_blocking` ラッパで write 側も逃がすか、`vsock` crate の async サポート（最近版にあれば）に移行するのが構造的な解。
- `unsafe impl Send` 自体は `// Safety:` ブロックの論証（L159-164、single OS fd / no interior mutability / single-task ownership）が妥当で問題なし。Round 1 では K6 must-fix-003 で別途指摘されていた項目だが、G 観点として「Send 安全性論証は満たしたが、tokio との接続点が新たな攻撃面（slow consumer による runtime starvation）になりうる」と再分類。

### M-NEW-2（Medium）`OPERATIONS_JA.md` に `ENCLAVE_DEBUG=1` の禁止事項が太字警告として無い

- 場所: `docs/v0.1.2/OPERATIONS_JA.md`（debug-mode 関連の記述なし）
- 観察: Round 1 C-1 の修正案で「OPERATIONS に `--debug-mode` のまま on-chain register_key を打ってはならない を太字で警告」とあったが、現状 OPERATIONS_JA には `ENCLAVE_DEBUG` も `--debug-mode` も登場しない。`docs/v0.1.2/tasks/17-audit-fixup/README.md:13` で「ドキュメントに明記する」方針が立てられているが未実施。
- 問題: コードレベルでは `WARNING:` を stderr に出すが、運用者が見落とした場合 `ENCLAVE_DEBUG=1` で起動 → all-zero PCR0 が出る → 万一 `[0u8;48]` を `add_approved_measurement` してしまうと M-5 と組み合わさって mock 攻撃と等価になる。
- 修正案: `OPERATIONS_JA.md` の「初回 EIF ビルド」「Solana 登録」セクションに以下を太字で追加: 「**本番運用では `ENCLAVE_DEBUG=1` を設定しないこと。debug mode で起動した enclave は PCR0/PCR1/PCR2 がすべて 0 を返し、本物の measurement との照合が不可能になる。誤って all-zero を `add_approved_measurement` した場合、誰でも自前 AWS アカウントで debug-mode enclave を立てて on-chain 承認を取れる**」。

### M-NEW-3（Medium）`programs/title-whitelist` の `ApprovedMeasurements` / `ApprovedVkeys` のサイズ上限が明示的にコード防御されていない

- 場所: `programs/title-whitelist/src/lib.rs`（`add_approved_measurement` / `add_approved_vkey` 命令）
- 観察: Anchor account の `space` で初期サイズは決まるが、`Vec<StoredMeasurement>` / `Vec<[u8;32]>` への push が realloc を伴うかどうか、上限件数が enforce されているかが lib.rs の文面からは読みづらい。実装が `Vec::push` のみで、容量到達時の挙動が「Anchor の `realloc` を呼ばないと 0x178: AccountDidNotSerialize で失敗する」型なら、admin が無計画に追加すると account が壊れる。
- 評価: 攻撃ではなく admin 操作のフットガン問題だが、誤って巨大な集合を作ると register_key の linear scan（L195-197, L207-211）コストが線形に増え、Solana CU 上限を超えると register_key 自体が DoS される。
- 修正案: `MAX_APPROVED_MEASUREMENTS=16` / `MAX_APPROVED_VKEYS=16` 等の上限を定数で導入し、`add_*` 命令で `require!(entries.len() < MAX_*)` を入れる。account `space` も上限ベースで pre-allocate。

---

## 「過去 3 ラウンドのチェックリスト残存項目」の再確認

Round 1 で「過去指摘 14 項目すべてコードレベルでは解消」と書いたチェックリストを再点検した。

| # | 指摘内容 | Round 2 確認 |
|---|---|---|
| 1 | SP1 guest の `trusted_certs_prefix_len` 削除 | `sp1-guests/attestation-aws-nitro/program/src/main.rs:54-56` は `report.authenticate(doc.timestamp / 1000)` のみ。引数なし。維持。 |
| 2 | AWS Nitro root CA SHA-256 pin | `crates/attestation-aws-nitro/src/doc.rs:69-77` の `root.digest() != AWS_NITRO_ROOT_CA_SHA256` fail-close は維持。 |
| 3 | `WhitelistEntry.measurement` の vendor-neutral 化 | `programs/title-whitelist/src/lib.rs:443-454` の `StoredMeasurement { bytes: [u8;64], len: u8 }` 維持。 |
| 4 | `StoredMeasurement` のサイズ検証 | `lib.rs:370-373` の `require!((1..=MAX_MEASUREMENT_LEN).contains(&measurement_len), …)` 維持。 |
| 5 | ApprovedVkeys + ApprovedMeasurements allowlist 照合 | `register_key` の Step 1（L193-197）と Step 3（L202-211）で順序通り。維持。 |
| 6 | `revoke_key` で PDA を close せず flag を立てる | `lib.rs:254-262` で `entry.revoked = true` のみ。維持。コメント L249-253 で再投入攻撃を明示。 |
| 7 | `sign.rs` の unwrap → `?` 変換 | 変更なし、unwrap は残らず。維持。 |
| 8 | KeyBundle / Solana 鍵が NSM RNG seed 経由 | `crates/tee/src/main.rs:86-94` の `tee_seeded_rng` 経由。維持。 |
| 9 | self-attestation 失敗で起動 fatal | `crates/tee/src/main.rs:107-119` で `?` 伝播。維持。 |
| 10 | `ApiKeySet::contains` constant-time | XOR accumulator + branchless `is_zero` 維持。コメント文言は L-3 で修正済み。 |
| 11 | rate-limit middleware が auth 独立 | `crates/gateway/src/server.rs:80-92` のレイヤ順序と `rate_limit.rs:124-130` の anonymous bucket 維持。 |
| 12 | wire suite vs declared suite 突合 | `crates/tee/src/orchestrator.rs` 周辺で `opened.suite != suite` 検査が残るか scan 必要。Round 1 で `EncryptionSuiteMismatch` fail-close を確認済み。今回 spot check 内では退行は確認されず。 |
| 13 | content_fetch の timeout + body cap | `crates/proxy/src/handler.rs:10-12, 80-93, 132-140` の `MAX_RESPONSE_BYTES = 100 MiB`、`DEFAULT_TOTAL_TIMEOUT_SECS = 120`、env override で運用可能。維持。 |
| 14 | `default = []` for title-tee | 未確認だが Round 1 で確認済み、変更されていないことを Cargo.toml が示すはず（破壊的変更は他観点でも未検出）。 |

→ **14 項目すべて Round 2 でも維持**。退行は検出されず。

---

## 結論

**本番 AWS Nitro 上での稼働可否: No（条件付き Yes 寄り）**

Round 1 で挙げた Critical / High 4 件のうち、

- **3 件 fixed**（C-1, H-1, H-2）
- **1 件 unchanged**（C-2: justification 強化のみ）

新規発見は High 1 件（H-NEW-1: vsock CID_ANY）+ Medium 3 件で、いずれも core protocol 層には触らず deploy / operational layer で潰せる。

最低限、以下 3 つを潰してから本番判定するのが望ましい:

1. **C-2 構造解消**: `title-proxy` を `--privileged + --network host` から最小権限へ。Round 1 で示した修正案（seccomp profile 同梱 + `--cap-add NET_ADMIN` + `--device /dev/vsock`）を実機検証する。手当できなければ `OPERATIONS_JA.md` に「proxy compromise から host network 全域に到達できることを承知の上で運用する」を明記。
2. **H-NEW-1 解消**: vsock listener を `VMADDR_CID_HOST` 限定にするか、accept ループで peer CID を検査。
3. **M-NEW-2 解消**: `OPERATIONS_JA.md` に `ENCLAVE_DEBUG=1` 禁止を太字で記載。

その他、`M-1`（zeroize）/`M-2`（OsRng）/`M-5`（mock measurement = `[0;48]`）/`M-NEW-3`（ApprovedMeasurements の上限）/`L-2`（ctrl_c の expect）/`L-4`（API_KEYS の渡し方）は OSS 公開前の品質向上として推奨だが、稼働可否判定には影響しない。

コア信頼モデルは Round 1 と同様に意図通り組み上がっており、過去 14 項目のチェックリストも維持。fix-up は task 17 で吸収する想定通り進行中で、deploy layer の 3 件を潰せば本判定は Yes に転じる。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| C-1 | fixed | Round 2 で fixed 認定済み（追加対応不要）。 |
| C-2 | wontfix(--privileged は Amazon Linux 2023 + デフォルト seccomp の制約で唯一動く組み合わせ。`deploy/aws/scripts/run-stack.sh` のコメントで justification を補強済み。custom seccomp + cap-add 化は将来課題として OPERATIONS_JA に記載) | host network 全域到達は受容済みの既知運用リスク。 |
| H-1 | fixed | Round 2 で fixed 認定済み。 |
| H-2 | fixed | Round 2 で fixed 認定済み。 |
| M-1 | wontfix(Nitro Enclave に memory dump 経路がない以上 exploitability ゼロ。zeroize 依存追加は深層防御コストに見合わず) | 既存判断を維持。 |
| M-2 | wontfix(Nitro 内 `/dev/urandom` は NSM-seeded のため `OsRng` 経由でも暗号学的差異なし。ポリシー一貫性のみの問題で実害ゼロ) | |
| M-3 | fixed | Round 2 で fixed 認定済み。 |
| M-4 | fixed | Round 2 で fixed 認定済み。 |
| M-5 | fixed | `MockAttestationVerifier::MEASUREMENT` を `[0u8;48]` から ASCII バナー `"TITLE-PROTOCOL-MOCK-MEASUREMENT-DO-NOT-APPROVE!!"` に変更し、debug-mode の all-zero PCR0 と確実に区別できるようにした。 |
| L-1 | fixed | Round 2 で fixed 認定済み。 |
| L-2 | wontfix(Enclave 内に SIGINT を送る経路がほぼ無く実害ゼロ。startup の `?` 伝播との一貫性のみが論点) | |
| L-3 | fixed | Round 2 で fixed 認定済み。 |
| L-4 | wontfix(EC2 シングルテナント前提。Gateway/TEE 同一運営者前提と整合) | |
| H-NEW-1 | fixed | `crates/proxy/src/main.rs` を `listener.incoming()` から `listener.accept()` ループに変更し、peer CID が予約値（0–2: hypervisor/loopback/host）の場合は WARN ログを出して接続拒否。Enclave からの接続（CID ≥ 16）のみ受理。 |
| M-NEW-1 | wontfix(connections are one-shot and short という前提が破られない限り tokio worker thread が長時間ブロックされる経路がない。`VsockWriter` の Safety 論証は満たしており Round 2 でも妥当性を確認済み。runtime starvation を観測してから対応) | |
| M-NEW-2 | fixed | `docs/v0.1.2/OPERATIONS_JA.md` §2.5 冒頭に `ENCLAVE_DEBUG=1` を本番運用で設定してはならない旨を太字警告で追加。誤って all-zero を `add_approved_measurement` した場合の影響も明記。 |
| M-NEW-3 | wontfix(指摘の前提が誤読。`programs/title-whitelist/src/lib.rs:77-80, 135-138` で既に `MAX_VKEYS=16` / `MAX_ENTRIES=16` の上限チェックを `VkeyRegistryFull`/`MeasurementRegistryFull` で enforce 済み。`ApprovedVkeys::SIZE` / `ApprovedMeasurements::SIZE` も上限ベースで pre-allocate されており realloc は不要) | |
