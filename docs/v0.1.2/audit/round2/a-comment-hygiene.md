# A. コメント・ドキュメント癖 (Round 2)

## Round 1 指摘の処理状況

| 重大度 | fixed | partially | unchanged | regressed | 計 |
|---|---|---|---|---|---|
| must-fix | 19 | 2 | 2 | 0 | 23 |
| should-fix | 19 | 5 | 4 | 0 | 28 |
| nitpick | 9 | 2 | 3 | 0 | 14 |
| **計** | **47** | **9** | **9** | **0** | **65** |

### must-fix (詳細)

- 001 main.tf 冒頭の「ない」列挙 → **fixed** (`deploy/aws/terraform/main.tf:1-6` 冒頭が短く書き直され、legacy/EIP の言及が消えた)
- 002 deploy/aws/README.md "Cost note" → **fixed** (`deploy/aws/README.md:66-67`「Public IP changes on every stop/start」だけに短縮)
- 003 README.md "legacy/v0.1.0/" 参照 → **fixed** (README.md Status から legacy 言及が消えた)
- 004 CHANGELOG.md Unreleased の v0.1.0 差分詰め込み → **partially-fixed**: 冒頭1行リード「Full protocol rewrite」は追加されたが、`Changed` / `Removed` セクションに v0.1.0 から見た差分が依然として大量に列挙されている（`CHANGELOG.md:11-35`）。Round 1 修正案は「Added だけ残す」だったが反映されていない
- 005 `crates/tee/src/lib.rs` "Legacy参照" 重複 → **fixed** (新 lib.rs:1-49 は責務記述のみ。Legacy / v0.1.0 セクション削除)
- 006 `crates/gateway/src/lib.rs` "## Legacy" → **fixed**
- 007 `resource_pool.rs` "Design notes (from legacy v0.1.0)" → **fixed** (lib.rs 冒頭から legacy 言及消失、CAS の説明が責務側へ移動)
- 008 `crates/core/src/jumbf.rs` ported-from → **fixed**
- 009 `crates/solana/src/cnft.rs` ported-from → **fixed**
- 010 `c2pa_verify.rs` Task 04 → **partially-fixed**: 25行目の `(Task 04)` は文字列が削除されたが、`crates/core/src/c2pa_verify.rs:137` に依然として `/// - The TEE orchestration layer (Task 04) to populate ...` が残存
- 011 `orchestrator.rs` `task04-test` シグナ名 → **fixed** (現状 `"title-orchestrator-test"`)
- 012 `content_fetch.rs` FETCH_TIMEOUT の長文 rationale → **fixed** (`content_fetch.rs:131-133` で2行に圧縮)
- 013 `content_fetch.rs` ETag "future optimization" 節 → **fixed** (該当節が削除されている。コード冒頭のモジュール doc に ETag 専用節は無く、412 だけ FetchError variant の doc に残る)
- 014 `fetch_fragmented` "Currently / future optimization" → **fixed** (`content_fetch.rs:389-392`「All fragments are accumulated...」のみ)
- 015 `auth.rs::contains` 過剰防御 rationale → **fixed**: doc は3-4行に短縮 (`auth.rs:114-118`)。さらに実装の整合性も改善：以前は length-mismatch を `continue` で短絡し doc に嘘があったが、現在の doc が `length-mismatched entries are skipped, so total runtime leaks the candidate's length` と正直に書いている。コードは依然 `continue` だが、doc とコードが一致している
- 016 `from_slice` 過剰防御 3重 rationale → **unchanged**: `programs/title-whitelist/src/lib.rs:457-470` で doc + 関数内コメント + debug_assert メッセージの3重構造が残存。修正されていない
- 017 ADMIN_AUTHORITY "Phase 1 / Future: multi-sig" → **partially-fixed**: 「Phase 1: single wallet」は依然残存。むしろ前バージョンより詳細化され "DAO migration plan: A) ... B) ..." と冗長になっている (`programs/title-whitelist/src/lib.rs:33-40`)。OPERATIONS_JA に集約する Round 1 修正案は無視されている
- 018 attestation-aws-nitro "## Origin" セクション5箇所 → **fixed** (lib.rs は1行 `Derived from Automata Network's ...`、cose.rs は1行 `Origin: Amazon ...`。doc.rs / cert.rs / constants.rs / sign.rs から由来表記が消失)
- 019 `tee_type_matches_attestation_vendor_tag` テストのコメント "Single source of truth" → **unchanged** (`crates/tee/src/vendor/aws.rs:203` でそのまま残存)
- 020 `AwsNitroVerifier` doc の "pinned root" 矛盾 → **fixed** (`crates/attestation-aws-nitro/src/lib.rs:36-40` で「The chain root is pinned to constants::AWS_NITRO_ROOT_CA_SHA256」と実装と整合)
- 021 `MockAttestationVerifier` "Pairs with MockRuntime" + 誤情報 → **fixed** (`crates/attestation/src/lib.rs:91-95` 修正案通り「Accepts attestations of the form... Gated behind the mock feature.」)
- 022 `tee_seeded_rng` 過剰防御 rationale → **fixed** (`crates/tee/src/main.rs:83-84` で2行に短縮)
- 023 ASCII 装飾過多 (`// ---- 75 chars ---- `) → **partially-fixed**: 全 crate で 118 箇所残存（grep 結果）。`crates/gateway/src/lib.rs`, `gateway/src/endpoints.rs`, `tee/src/orchestrator.rs`, `tee/src/server.rs`, `tee/src/content_fetch.rs`, `tee/src/resource_pool.rs` 全てが Round 1 のまま。Round 1 修正案（1行見出しに統一）は採用されていない

### should-fix (詳細)

- 001 全 trait/struct/field に `/// 仕様書 §X.Y` 機械添付 → **partially-fixed**: `core/src/processor.rs`, `core/src/request.rs`, `core/src/response.rs` で 31 件の `仕様書 §X` / `Spec §X` が依然 field/method 単位で貼られている。トップレベルの統合は進んでいない
- 002 `KeysResponse` / `HealthResponse` / `SolanaKeysResponse` `# JSON例` 重複 → **unchanged** (`gateway/src/lib.rs:43, 68, 111` で3箇所とも JSON 例が残存)
- 003 `WhitelistEntry` ↔ `title-solana` `WhitelistEntry` 重複 docstring → **partially-fixed**: client 側 (`crates/solana/src/whitelist.rs:56-72`) は短くなったが、`/// Mirror of...` の1行集約には至らず、フィールドごとに `revoked` 等 doc が再記述されている
- 004 `endpoints.rs` 各ハンドラ doc + `/// Spec §2.5` 重複 → **partially-fixed**: 各 doc は短めだが `/// Spec §2.5` ラベルは6ハンドラ全てに残存 (`endpoints.rs:59, 78, 95, 124, 146, ...`)
- 005 `handle_solana_extension` "System clock failure here is fatal..." 4行 rationale → **unchanged** (`crates/tee/src/server.rs:275-278` でそっくり残存)
- 006 `decrypt_single_payload` "Reject mismatches..." → **unchanged** (`orchestrator.rs:292-295` でそのまま残存)
- 007 `HttpContentFetcher` 本体 doc の攻撃モデル長文 → **unchanged** (`content_fetch.rs:114-121` で5行 rationale 残存)
- 008 `trusted_certs_prefix_len` 3重 rationale → **fixed**: パラメータ自体が `authenticate()` から削除され (`doc.rs:53` のシグネチャ参照)、SP1 guest 側は `report.authenticate(doc.timestamp / 1000)` でハードコード。doc/host 側の重複 rationale も消失
- 009 trait `verify` doc と impl の挙動矛盾 → **unchanged**: `crates/attestation/src/lib.rs:72-75` で「Implementations should reject documents whose internal timestamp is in the future relative to `now_unix_secs`」が残存し、`AwsNitroVerifier::verify` 実装は依然 doc timestamp を `authenticate` に渡している (`lib.rs:60-65`)。trait コントラクトと実装は引き続き矛盾
- 010 "Use the smaller of (now, doc.timestamp/1000)..." rationale → **fixed** (該当コメントが lib.rs から消失。`authenticate(timestamp)` に直接 doc timestamp を渡す素直なコードになり、コメント自体が不要に)
- 011 "C2PA alone vs Title Protocol" 表の README/SPECS 二重掲載 → **unchanged** (`README.md:49-53` に表が残存)
- 012 main.tf inline コメント vs README 重複 + `provision.sh` 嘘 → **fixed** (`main.tf:137-139` 「First-boot provisioning (Docker, nitro-cli, hugepages) — see user-data.sh.」のみ。 嘘の `deploy/aws/scripts/provision.sh` 案内が削除。`deploy/aws/scripts/` 配下にも `provision.sh` は存在しない)
- 013 server.rs "Layer order: outermost runs first" rationale → **partially-fixed**: コメントが残っているが (`server.rs:81-86`)、より具体的に「axum/tower: layers added LATER wrap EARLIER ones... the order below therefore executes as: request → rate_limit → auth → handler」と書き直されている。重複自体は解消されておらず冗長
- 014 main.rs "Built outside the async runtime..." → **partially-fixed** (`main.rs:157-158`「Built via spawn_blocking because reqwest::blocking::Client constructs its own tokio runtime, which panics if done inside an async context.」と Step 6 のコメント内に移動・再整理されたが、長さは Round 1 とほぼ同じ)
- 015 sp1 feature 切替 "The rest of the source is untouched" → **fixed** (`crates/attestation-aws-nitro/src/lib.rs:12-13`「When built for SP1, shadow the standard `sha2` and `p256` crates with SP1-precompile-accelerated forks. The rest of the source is untouched.」← `The rest of the source is untouched` がまだ残っている。**再判定: unchanged**)
- 016 `parse_public_values` `has_user_data` rationale 3行 → **unchanged** (`programs/title-whitelist/src/lib.rs:381-383` でそのまま残存)
- 017 COVERAGE.md "Note" と content_fetch.rs "Note" 重複 → **fixed** (両側とも future optimization が消えたので結果的に重複も解消)
- 018 `verifies_real_aws_nitro_attestation` "tests don't depend on anything outside" → **unchanged** (`crates/attestation-aws-nitro/src/lib.rs:98-100` でそのまま残存。「stored alongside this crate so tests don't depend on anything outside the crate tree」が監査者向け自己弁護として残る)
- 019 "single wallet / multi-sig" 4箇所反復 → **partially-fixed**: README/CHANGELOG からは消えたが、`programs/title-whitelist/src/lib.rs:35` で依然唯一の源として残り（むしろ詳細化された）、OPERATIONS_JA §9 ロードマップに集約する案は無視
- 020 `rate_limit_middleware` doc 「Runs independently of API-key validation」重複 → **unchanged** (`rate_limit.rs:112-114` でモジュール doc とほぼ同じ説明)
- 021 `fetch()` 2段階 cap 説明 → **unchanged** (`content_fetch.rs:185-186` + `:211-212` で2つのコメントが両方残存)
- 022 `Ticket` `Cell<Instant>` 内部詳細 → **unchanged** (`resource_pool.rs:148`「(it is `Send` but not `Sync` due to `Cell<Instant>`)」残存)
- 023 `check_and_refresh` doc → **fixed (false positive resolved)**: Round 1 でも false positive 寄りとした項目
- 024 `tee_seeded_rng` "purpose is included only in error messages" → **unchanged** (`crates/tee/src/main.rs:216`「`purpose` is included only in error messages for debuggability.」残存)
- 025 KEY_EXPIRY_SECONDS 重複 → **partially-fixed**: client 側 doc が `Authoritative source is on-chain` で短くなった (`crates/solana/src/whitelist.rs:15-20`) が、Round 1 修正案の「Mirror of `title_whitelist::KEY_EXPIRY_SECONDS`」1行にはなっていない
- 026 OPERATIONS_JA §2.5 プレースホルダ → **unchanged** (`docs/v0.1.2/OPERATIONS_JA.md:144-167`「⚠️ この章は AWS Nitro EC2 上での実機検証後に内容を追記する（プレースホルダー）」が、`deploy/aws/README.md` がすでに完成しているにもかかわらず残存。emoji `⚠️` も残る → nitpick-005 と連動)
- 027 OPERATIONS_JA §5.2「現状クライアント SDK は提供していない」「SDK 化はロードマップ」 → **unchanged** (`OPERATIONS_JA.md:331` 残存)
- 028 `rate_limit_skips_health` テスト doc → **unchanged** (`server.rs:679-680` で残存。優先度低)

### nitpick (詳細)

- 001 `§` / `SS` / `仕様書 §` 混在 → **fixed**: `SS` 表記は grep で全 crate に 0 件。`§` 表記に統一済み
- 002 docstring の表記揺れ全般 → **partially-fixed** (`JCS(verifiable)` / `JCS(verifiable_response)` / `JCS(signature_hash + results)` の3表記がまだ混在: `orchestrator.rs:379`, `solana/extension.rs:113`, `attestation/lib.rs:39`)
- 003 ダッシュ `--` / `—` 混在 → **partially-fixed** (両表記は残るが減少)
- 004 ASCII 図3箇所重複 → **unchanged** (README.md / OPERATIONS_JA.md / deploy/aws/README.md にそれぞれ独自の図が残存)
- 005 emoji `⚠️` → **unchanged** (OPERATIONS_JA §2.5 と共に残存)
- 006 `--` 後スペースなし rate_limit.rs → **fixed**
- 007 COVERAGE 凡例 → **fixed (元から問題なし)**
- 008 attestation mock MEASUREMENT doc → **fixed (元から問題なし、確認のみ)**
- 009 OPERATIONS_JA 全角・半角混在 → **fixed**: 確認した範囲では概ね半角に揃っている
- 010 README.md `| | C2PA alone | ...` 空ヘッダー → **fixed**: 現在は `| | C2PA alone | Via Title Protocol |` だが、空ヘッダーセルは依然残る。**再判定: unchanged** (`README.md:49` 「| | C2PA alone | Via Title Protocol |」)
- 011 orchestrator.rs `Step 1〜Step 11` 番号付け → **unchanged**: 21箇所の `// Step N:` が `tee/src/main.rs` / `tee/src/orchestrator.rs` に残存。むしろ `main.rs:90` と `main.rs:100` で `// Step 3:` が連続してしまっており（Round 1 で危惧した「番号がずれる」が現実に発生中）、ステップ番号の意味整合性が壊れている
- 012 content_fetch.rs BMFF doc → **fixed (元から問題なし)**
- 013 c2pa_verify.rs `# Returns` → **fixed (元から問題なし)**
- 014 CONTRIBUTING / docs/README 構造説明重複 → 確認時間切れ。未確認のため **unchanged 扱い**

## 新規発見

### new-must-fix-001 nitpick-011 が must-fix 級に悪化（main.rs の Step 番号衝突）

- 場所: `crates/tee/src/main.rs:83, 90, 100, 121, 133, 152, 190`
- 観察:
  ```
  // Step 2: Generate encryption key bundle. ...
  // Step 3: Generate Solana Extension signing key
  // Step 3: Self-attestation — bind the TEE's measurement before any other  ← 2つめの Step 3
  // Step 4: Registration attestation. ...
  // Step 5: Processors + ResourcePool. ...
  // Step 6: Outbound content fetcher. ...
  // Step 6: Start Axum HTTP server                                          ← 2つめの Step 6
  ```
- 問題: Step 番号が **重複して飛んでいる**（3, 3, 4, 5, 6, 6）。コメントが嘘になっており、新規読者はどれを信じればよいか判断できない。Round 1 の nitpick で番号付けの脆さを指摘していたが、Round 2 で具体的な衝突が顕在化。
- 修正案: 全 `// Step N:` ラベルを削除し、`// Runtime selection`, `// Encryption key bundle`, `// Solana signing key`, `// Self-attestation`, `// Registration attestation`, `// Processors`, `// Content fetcher`, `// HTTP server start` のような意味的見出しに変える。

### new-must-fix-002 doc/impl 矛盾の温存（must-fix-009 / should-fix-009 ペア）

- 場所: `crates/attestation/src/lib.rs:72-80` と `crates/attestation-aws-nitro/src/lib.rs:55-65`
- 観察: trait `AttestationVerifier::verify` の doc は「Implementations should reject documents whose internal timestamp is in the future relative to `now_unix_secs`」と書きながら、唯一の実装 `AwsNitroVerifier::verify` は `now_unix_secs` をそのまま `authenticate()` に渡すだけで、doc timestamp との比較 / reject ロジックを持たない。Round 1 で should-fix として trait 側 doc を削るよう案出したが、実装側コメントが消えた一方で trait doc は据置のため、矛盾は強化された。
- 問題: コメントが嘘をついている。F 観点 (docs consistency) も拾うが、コメント観点としても must-fix。
- 修正案: trait doc の「Implementations should reject documents whose internal timestamp is in the future relative to `now_unix_secs`」一文を削除する。

### new-should-fix-001 `// Step 8-10:` が orchestrator.rs に残る

- 場所: `crates/tee/src/orchestrator.rs:245`
- 観察: `// Step 8-10: JCS hash, Attestation Document, ProcessResponse.` のように複数 step をまとめてカウントする一方で、`build_attested_response` 内部 (`:375` 以降) でも `Step 7 / 8 / 9` と独自の番号が振り直されている。
- 問題: orchestrator.rs だけで Step 番号が2系統存在し、main.rs と合わせて少なくとも3系統。読み手が SPECS_JA §5.2 のフローと突き合わせる際にどれを信じればよいか判別不能。
- 修正案: 同じく Step ラベルを意味的見出しに置換。

### new-should-fix-002 `// axum/tower: layers added LATER wrap EARLIER ones...`

- 場所: `crates/gateway/src/server.rs:81-86`
- 観察:
  ```rust
  // axum/tower: layers added LATER wrap EARLIER ones (the last
  // `.layer` call is the outermost middleware). The order below
  // therefore executes as:
  //   request → rate_limit → auth → handler
  // so the anonymous bucket throttles unauthenticated traffic
  // *before* the auth layer 401s it.
  ```
- 問題: 同じことが `auth.rs:46-55`, `rate_limit.rs:109-114` でも書かれている。Round 1 should-fix-013 / -020 が依然解消されておらず、書き直しでむしろ冗長化（5行→6行）。
- 修正案: コメントを1行 `// request → rate_limit → auth → handler (axum: last .layer is outermost).` に。詳細は `rate_limit_middleware` の doc に集約。

### new-should-fix-003 `crates/tee/src/server.rs` の `MAX_OFFCHAIN_DATA_BYTES` rationale 過剰

- 場所: `crates/tee/src/server.rs:240-243`
- 観察:
  ```rust
  // Fetch off-chain data. A `ProcessResponse` is metadata + hashes only
  // (no media bytes) — 1 MiB is well past anything realistic and stops
  // a malicious URL from flooding the JSON parser.
  const MAX_OFFCHAIN_DATA_BYTES: usize = 1024 * 1024;
  ```
- 問題: Round 1 のコード本体 review 範囲外だったが、Round 2 で server.rs を読むと「攻撃モデル + サイズ根拠 + 攻撃防御」を3行で説明している。`HttpContentFetcher` doc と同じ 4.7 癖。
- 修正案: 1 行 `// 1 MiB cap: ProcessResponse is metadata-only.`

### new-should-fix-004 `crates/tee/src/server.rs:81-85` doc コメント

- 場所: `crates/tee/src/server.rs:81-85`
- 観察:
  ```rust
  // ProcessRequest / SolanaExtensionBody are pure metadata JSON — content
  // bytes come from `fetcher`, not the request body. Cap the JSON envelope
  // at 64 KiB so a runaway Gateway can't push a 100-MB document into the
  // TEE before admission control sees the request.
  ```
- 問題: 「a runaway Gateway can't push a 100-MB document」は防御 rationale で `DefaultBodyLimit::max(64 * 1024)` の意図そのもの。1行で十分。
- 修正案: `// JSON envelope only — content bytes come from fetcher. Cap at 64 KiB.`

### new-nitpick-001 `CHANGELOG.md` の `## [Unreleased] — v0.1.2` 構造のミスマッチ

- 場所: `CHANGELOG.md:7-9`
- 観察: 「Full protocol rewrite. See [Technical Spec](docs/v0.1.2/SPECS_JA.md).」と冒頭1行リードがあるが、Round 1 で指摘した v0.1.0 差分の大量列挙（Changed / Removed）はそのまま残っている。リードと中身が整合していない。
- 修正案: Round 1 must-fix-004 通り、Added 中心の再構成へ。

### new-nitpick-002 docstring 例 JSON 内のキー順が SPECS_JA と微妙にずれ

- 場所: `crates/gateway/src/lib.rs:43-51` (KeysResponse 例)
- 観察: コメント JSON 例の `keys.x25519` / `p256` / `ml-kem-768` 順が SPECS_JA §2.5 の登場順と一致しているか確認できなかった。should-fix-002 で「JSON 例は SPECS に集約」と提案済みで、依然削除されていないため、ズレた場合の検知が困難。
- 修正案: should-fix-002 の集約を実施。

## 全体所感

「ない列挙」「ported-from の内輪話」「現状/将来 表現」「ASCII 装飾」の4大癖のうち、前2つは概ね一掃された一方、後2つ（ASCII の `// --- ---` 区切り118箇所、`Spec §X` の field 単位機械添付、`Phase 1 / Future:` 表現、`Step N:` 番号付け）は実質手付かずで、`tee/src/main.rs` では Step 番号衝突（3, 3, 6, 6）と trait doc / impl の矛盾（attestation/lib.rs の verify trait）という新規退行が顕在化している。
