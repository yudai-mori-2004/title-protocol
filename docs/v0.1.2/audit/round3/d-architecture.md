# D. アーキテクチャ・ディレクトリ構造 — Round 3

## 概要

担当範囲: workspace 構造（`Cargo.toml`）、`crates/`・`programs/`・`sp1-guests/`・`deploy/`・`docker/`・`docs/`・`keys/`・`legacy/` の配置、各 crate の責務・境界・公開 API、ファイルサイズ・多責務、命名、依存方向。

Round 2 で挙げた 23 件 + 新規 7 件のうち、Round 2 の処理ログでは **fixed: 4 / partially-fixed: 1 / wontfix: 25** と判定されている。本 Round 3 では (a) Round 2 で fixed と認定された 4 件が実コード上で本当に閉じているか、(b) wontfix 群が「v0.1.3 で対応」と書き残された状態で本当に放置されているか、(c) Round 2 → Round 3 の差分（修正・退行・新規発見）を実コード突合で確認した。

リポジトリ全景：

```
title-protocol/
├── Cargo.toml                # workspace = 8 crates + 3 exclude（順序の乱れ存続）
├── crates/
│   ├── core/      crypto/   attestation/   attestation-aws-nitro/
│   ├── tee/  (vendor/ + runtime/ 二層構造のまま)
│   ├── solana/  gateway/  proxy/
├── programs/title-whitelist/ # 単一 lib.rs (777 → 799 行に再増加)
├── sp1-guests/attestation-aws-nitro/{host,program}
├── keys/admin.json           # ファイル本体は work tree に残存
└── legacy/v0.1.0/
```

実測 (`wc -l`)：

| ファイル | Round 1 | Round 2 | Round 3 |
|---|---|---|---|
| `crates/tee/src/orchestrator.rs` | 1185 | 1205 | **1294** |
| `crates/tee/src/server.rs` | — | — | **646** |
| `crates/tee/src/main.rs` | — | — | 230 |
| `programs/title-whitelist/src/lib.rs` | 728 | 777 | **799** |
| `crates/gateway/src/lib.rs` | — | — | 275 |
| `crates/solana/src/extension.rs` | — | — | 322 |

Round 1 → Round 2 で逆行（増行）した 2 ファイル（`orchestrator.rs` / `programs/.../lib.rs`）は、Round 2 → Round 3 で **さらに行数が増えた**。分割リファクタは「v0.1.3 で対応」のラベル下にデフォーされた一方、ロジック追加は止まらず、原問題の規模は単調に拡大している。

## Round 2 指摘の処理状況

| ID | カテゴリ | Round 2 判定 | Round 3 status | 備考 |
|---|---|---|---|---|
| must-fix-001 | `keys/admin.json` 秘密鍵 | wontfix(v0.1.3) | **unchanged** | `keys/admin.json` は work tree に依然として存在。`.gitignore` の `keys/`（27 行目相当）はトラック済みファイルに無効。git 状態確認は本 audit 環境では実行できなかったが、ファイル現存 + Round 2 で `git rm --cached` 未実施が記録されていることから、状況は変わっていないと判定する。Round 2 処理ログが「`.gitignore` で新規 commit からは保護済み」と書くのは事実誤認（既トラック分は無関係）。v0.1.3 までの間に手元 clone から push が走るリスクが残置。 |
| must-fix-002 | `title-tee` → `title-solana` 静的依存 | wontfix(v0.1.3) | **unchanged** | `crates/tee/Cargo.toml:34` に `title-solana = { workspace = true }` がそのまま、`crates/tee/src/server.rs:29-30` で `title_solana::extension`・`signing_key` を直接 import、`router()` (`server.rs:78-98`) で `/solana-keys`・`/extension/solana` を unconditional 取付。Extension 差し替えは依然として実装的に不可能。 |
| must-fix-003 | vendor 識別子表記揺れ | partially-fixed | **unchanged** | `crates/tee/src/main.rs:41-47, 61, 72-76` の env キー `"nitro"` がそのまま。一方 `TeeRuntime::tee_type()` のドキュメント (`crates/tee/src/lib.rs:39`) は `"aws-nitro"` を例示しており、env キーと runtime tag で 2 系統並存。`vendor_tags` 中央集約は未着手。 |
| must-fix-004 | `orchestrator.rs` 1205 行 | wontfix(v0.1.3) | **regressed** | **1205 → 1294 行**（+89 行）。Round 1 → 2 で +20 行、Round 2 → 3 で +89 行と増速。`compute_jcs_hash` (orchestrator.rs:352) と `extension.rs:98` の二重定義もそのまま。 |
| must-fix-005 | `programs/title-whitelist/src/lib.rs` 777 行 | wontfix(v0.1.3) | **regressed** | **777 → 799 行**（+22 行）。Anchor 慣習の `state.rs / errors.rs / events.rs / instructions/` 分割は未実施で、`register_key` 周辺にコメント・require 順序の justification が追加されている分肥大化を続けている。 |
| should-fix-001 | `attestation/mock` feature 命名 | wontfix(v0.1.3) | **unchanged** | `crates/attestation/Cargo.toml:13` `mock = []`、`crates/tee/Cargo.toml:20` `runtime-mock = ["title-attestation/mock"]` の非対称も同じ。 |
| should-fix-002 | proxy ↔ tee の `vendor-aws` 同名・別意味 | wontfix(v0.1.3) | **unchanged** | `crates/proxy/Cargo.toml:21` と `crates/tee/Cargo.toml:22-27` の同名 feature が別意味のまま。 |
| should-fix-003 | JCS+SHA-256 二重実装 | wontfix(v0.1.3) | **unchanged** | `crates/tee/src/orchestrator.rs:347-358` (`compute_jcs_hash`) と `crates/solana/src/extension.rs:96-104` (`compute_verifiable_hash`) が並存。`extension.rs:97` の `// Spec §1.5, §2.3 — same as orchestrator.rs but standalone.` がまだ生きており、二重定義をコメントで自認している。 |
| should-fix-004 | `OffchainData` 未使用 struct | fixed | **fixed (regression なし)** | `crates/solana/src/extension.rs` に該当型は不在。Round 3 でも確認済み。 |
| should-fix-005 | `hex_short` / `hex_encode` 手書き | fixed | **fixed (regression なし)** | `crates/tee/src/main.rs:118` で `hex::encode` を使用。手書きエンコーダ復活は確認できなかった。 |
| should-fix-006 | `SolanaExtensionBody` ↔ `SolanaExtensionRequest` 二重 | wontfix(v0.1.3) | **unchanged** | `crates/tee/src/server.rs:232-240` `SolanaExtensionBody` と `crates/gateway/src/lib.rs:137-154` `SolanaExtensionRequest` が依然として別型。Gateway 側は `Json<SolanaExtensionRequest>` で受け取って `state.tee_client.solana_extension(&request)` 経由で再シリアライズし TEE へ転送し、TEE が `SolanaExtensionBody` で受け直すという二重通過のままで、フィールド名揺れの risk が温存されている。 |
| should-fix-007 | TEE crate doc が「v0.1.0 からの変更点」 | fixed | **fixed** | `crates/tee/src/lib.rs:1-49` は Spec §5.2 主軸の簡潔な doc に整理済み。 |
| should-fix-008 | `programs/title-whitelist/keypair.json` | wontfix(v0.1.3) | **unchanged** | `programs/title-whitelist/keypair.json` がファイル本体として存続（`ls` 確認、本 audit 環境で git ls-files 不可）。 |
| should-fix-009 | `programs/` `sp1-guests/` version hard-code | wontfix(v0.1.3) | **unchanged** | `programs/title-whitelist/Cargo.toml:3-7` に `version = "0.1.2"` を hard-code（workspace = false）。CI lint も未導入。 |
| should-fix-010 | `sp1-guests/attestation-aws-nitro` 命名 | wontfix(v0.1.3) | **unchanged** | `sp1-guests/{README.md, attestation-aws-nitro/{host,program}}` の構造のまま。`host-cli/` 集約なし。 |
| should-fix-011 | `gateway/src/lib.rs` API DTO 直書き | wontfix(v0.1.3) | **unchanged** | `crates/gateway/src/lib.rs:53-162` に DTO（`KeysResponse` / `ProcessorsResponse` / `HealthResponse` / `SolanaKeysResponse` / `SolanaExtensionRequest` / `SolanaExtensionResponse`）が直接定義されたまま。`title-api` 抽出は未実施。 |
| should-fix-012 | `vendor/` と `runtime/` 二層 | wontfix(v0.1.3) | **unchanged** | `crates/tee/src/runtime/mod.rs` (12 行、`pub mod mock;`)、`crates/tee/src/vendor/mod.rs` (18 行、`#[cfg(feature = "vendor-aws")] pub mod aws;`)。両者あわせて 30 行で、それぞれ実体が 1 モジュールずつしかない構造が固着。 |
| nitpick-001 | `SS` エスケープ | fixed | **fixed** | `crates/` 配下に `SS[0-9]` の残置なし。 |
| nitpick-002 | `gateway/src/lib.rs` 日英混在 | wontfix | **unchanged** | `:1-19` 英語、`:37-162` 日本語の構図のまま。 |
| nitpick-003 | `proxy/src/protocol.rs` cfg 順序 | wontfix | **unchanged** | `crates/proxy/src/protocol.rs:106-131` で `#[cfg(all(target_os = "linux", feature = "vendor-aws"))]` を 3 関数連続で繰り返す。`mod sync_io` 集約なし。 |
| nitpick-004 | `Cargo.toml` members 順序 | wontfix | **unchanged** | ルート `Cargo.toml:3-11` 順序は `attestation, attestation-aws-nitro, core, crypto, tee, gateway, proxy, solana`。`tee` が `crypto` の直後に置かれ alphabetical からズレるのも変わらず。 |
| nitpick-005 | `[[bin]] path = "src/main.rs"` 冗長 | wontfix | **unchanged** | `crates/{gateway,tee,proxy}/Cargo.toml:10-12` で `path = "src/main.rs"` 存続。 |
| nitpick-006 | `server.rs::handle_keys` の `json!` 直書き | wontfix | **unchanged** | `crates/tee/src/server.rs:108-128, 224-228, 352-354` で `Json(serde_json::json!({...}))` を 5 か所で生成。`KeysResponse`/`ProcessorsResponse`/`HealthResponse`/`SolanaKeysResponse` などの既存 DTO は依然として TEE 側で未使用。Round 2 で should-fix-011（gateway 側 DTO 重複）と並んで保留された結果、TEE と Gateway で別表現を抱える状態が固定化。 |

### Round 2 → Round 3 新規発見

| ID | カテゴリ | Round 2 判定 | Round 3 status | 備考 |
|---|---|---|---|---|
| round2-d-new-001 | `.gitignore` 追加のみ修正 | wontfix(v0.1.3) | **unchanged** | must-fix-001 と同根。`.gitignore` に頼った修正の危険性は変わらず。 |
| round2-d-new-002 | `ADMIN_AUTHORITY` rotation 計画コメント | wontfix(v0.1.3) | **unchanged**, さらに **growth** | `programs/title-whitelist/src/lib.rs:33-44` に Phase 1 → multisig 計画コメントが残置。さらに Round 2 → Round 3 で `register_key` 直上 (`:181-198`) に「require 順序を justification するコメント」（Anchor `require!` の DoS-resistance を 9 行で説明）が追加され、Round 1 で指摘した「rationale 長文化」パターンが program crate 全体に波及している。 |
| round2-d-new-003 | テストヘルパー多重定義 | wontfix(v0.1.3) | **unchanged** | `crates/tee/src/server.rs:599-645` の `create_test_jpeg` / `create_signed_jpeg` と、`crates/tee/src/orchestrator.rs` (1294 行) のテストモジュール内ヘルパーが依然として共存。`tests/common/` 統合は未着手。 |
| round2-d-new-004 | `mock` feature 責務拡大 | wontfix(v0.1.3) | **unchanged**, doc 改善も未着手 | `crates/attestation/Cargo.toml:12` のコメントは `# Enables MockAttestationVerifier. Test-only.` のまま。一方 `crates/tee/Cargo.toml:18` `default = ["runtime-mock"]` + `:70` `dev-dependencies` 経由の両ルートでの呼び出しは継続。 |
| round2-d-new-005 | `Spec §X --` ASCII separator | wontfix（意図的） | **unchanged** | Round 2 で「意図的な ASCII separator」と判定済み。Round 3 として再指摘はしないが、`crates/tee/src/server.rs` 群が `Spec §2.5, §5.2` 系の素直な表記、`orchestrator.rs` が `Spec §X.Y -- 説明`、`crates/tee/src/lib.rs:5` が `Spec §5.2 — ...` と em dash と、3 系統並存。 |
| round2-d-new-006 | `TeeAppState` 責務肥大 | wontfix(v0.1.3) | **regressed (本質)** | `crates/tee/src/server.rs:43-74` で `TeeAppState` 10 フィールドの構成自体は同じ。だが Round 2 で must-fix-002 とセットで「v0.1.3 で分離」と decode した直後の `crates/tee/src/main.rs:176-187` で 10 フィールドの bag を `Arc::new` するパターンが本番起動フローに組み込まれ、コア用 / Solana Extension 用の境界がコードベース横断で消えた。Extension 追加コストはむしろ Round 2 比で上がっている。 |
| round2-d-new-007 | `runtime/` と `vendor/` 中身 1 ファイルずつ | wontfix(v0.1.3) | **unchanged** | `crates/tee/src/runtime/mod.rs` 12 行・`crates/tee/src/runtime/mock.rs` 118 行、`crates/tee/src/vendor/mod.rs` 18 行・`crates/tee/src/vendor/aws.rs` 227 行。「mock = runtime/」「実 TEE = vendor/」のディレクトリ規範を doc に持ち、しかし両者とも `TeeRuntime` trait 実装でしかないという矛盾はそのまま。 |

### Round 2 集計

- fixed: **4** (should-fix-004, should-fix-005, should-fix-007, nitpick-001) — Round 3 でも closed
- partially-fixed: **1** (must-fix-003) — Round 3 で **unchanged**（partial のまま停止）
- wontfix(v0.1.3 デフォー): **25** — Round 3 で **23 件 unchanged / 2 件 regressed**

**Round 3 で新たに完全 close した Round 2 指摘はゼロ件。** wontfix と判定された 25 件のうち 2 件（must-fix-004, must-fix-005）は規模を拡大しており、Round 1 → Round 2 → Round 3 と一貫して悪化する単調退行が起きている。

## 新規発見（Round 3 で初出）

### round3-d-new-001 must-fix `crates/tee/src/main.rs` が `tee_seeded_rng` + 起動シーケンス + bin の三役を抱え、`tee` crate のレイアウトを乱す

- 場所: `crates/tee/src/main.rs:33-202` (`fn main`) と `:211-230` (`fn tee_seeded_rng`)
- 観察: 230 行の `main.rs` のうち、トップレベル関数は `main` (170 行) と `tee_seeded_rng` (20 行)、`shutdown_signal` (6 行) の 3 つ。`tee_seeded_rng` は「NSM GetRandom → ChaCha20Rng」という再利用性の高いヘルパーだが、`bin/main.rs` 内に閉じ込められており、`#[cfg(test)]` テストからも `crates/tee/src/orchestrator.rs` からも呼べない。
- 問題: Round 1〜2 で必出の `cfg(feature = "runtime-mock")` / `cfg(feature = "vendor-aws")` の runtime 選定ロジックも同じ `main.rs:41-83` に置かれているため、(a) bin が大規模化し続け（230 行）、(b) tests が「mock runtime 選定 + 起動シーケンス + 自己 attestation」の一連を独立に検証できない。`fn select_runtime() -> (Box<dyn TeeRuntime>, Box<dyn AttestationVerifier + Send + Sync>)` と `fn tee_seeded_rng` を `crates/tee/src/lib.rs` または `crates/tee/src/bootstrap.rs` に外出しすべき。
- 修正案: (a) `crates/tee/src/bootstrap.rs` を新設し `select_runtime`, `tee_seeded_rng`, `build_app_state` を export、(b) `crates/tee/src/main.rs` は `tokio::main` 直下で `bootstrap::run().await` を呼ぶだけにする、(c) integration test (`crates/tee/tests/bootstrap.rs`) で mock 起動フローを直接検証できるようにする。

### round3-d-new-002 should-fix `TeeAppState` を `Arc::new` で固めるパターンが `main.rs` と `server.rs::tests::test_state_with_fetcher` の 2 箇所で重複している

- 場所: `crates/tee/src/main.rs:176-187`、`crates/tee/src/server.rs:406-424`
- 観察: 本番 (`main.rs`) と test (`server.rs:test_state_with_fetcher`) が同じ `TeeAppState { runtime, key_bundle, solana_key, registry, pool, fetcher, attestation_verifier, expected_measurement, registration_attestation, started_at }` を別々に組み立てている。Round 2 round2-d-new-006 の責務肥大が「フィールド追加コストの倍率化」として現れている：将来 Ethereum Extension を追加すると、(a) `TeeAppState` の新フィールド、(b) `main.rs` の組み立て、(c) `server.rs::test_state` の組み立て、(d) Gateway 側 cache の対応、の 4 箇所同時更新になる。
- 問題: round2-d-new-006 (`TeeAppState` 肥大化) + should-fix-002 (must-fix-002 / Solana 静的依存) と複合し、Extension 追加コストは Round 2 比で実質倍化。
- 修正案: round2-d-new-006 の `CoreState` + `ExtensionRegistry` 分離と一括で対応する。当面のミニマル対応として、`crates/tee/src/server.rs` 内に `pub fn test_state(...)` factory を `#[cfg(any(test, feature = "test-support"))]` で export し、`main.rs` 側との差分（key_bundle 生成方法 / measurement 取得有無）だけを明示する。

### round3-d-new-003 should-fix `crates/gateway/src/state.rs::TeeInfoCache` の `solana_keys: Option<SolanaKeysResponse>` が Solana Extension の「ON/OFF 判定」と「キャッシュ未取得」を兼用していて、Extension 追加で破綻する

- 場所: `crates/gateway/src/state.rs:28-44`、`crates/gateway/src/endpoints.rs:148-192`
- 観察: `TeeInfoCache.solana_keys` が `None` のとき、`handle_solana_keys` / `handle_solana_extension` は `GatewayError::NotFound("Solana Extension not enabled".into())` を返す（`endpoints.rs:155, 180`）。一方 `refresh_tee_info` (`state.rs:82-99`) は `self.tee_client.solana_keys().await?` で取得し、`Some(keys)` を入れる（解禁 `tee_client.rs` を別途確認）。つまり `None` は「Extension 未対応」と「初回 refresh 未完了」を兼ねるが、後者は spec §6.1 の「Extension 有効化は TEE バイナリビルド時固定」と整合しない（Extension 有効ビルドの TEE が再起動中に Gateway 起動した瞬間に同じ `None` で 404 が返り、後で `Some` に化ける）。
- 問題: Extension が増えると `solana_keys: Option<...>`、`ethereum_keys: Option<...>` のように N 個追加されつつ、それぞれ「unknown / disabled / enabled-but-not-yet-fetched」の 3 状態を 2 値で表現せざるを得なくなる。Spec §6.1 の「ビルド時固定」前提を Gateway 側に明示する `enum ExtensionStatus { Disabled, Enabled(SolanaKeysResponse) }` への置換、または `TeeInfoCache` 自体を `Option<TeeInfoCache>` でラップして「初回 refresh 未完了」を別軸にする。
- 修正案: (a) `TeeInfoCache` を「refresh 完了後の immutable snapshot」と再定義し `RwLock<Option<Arc<TeeInfoCache>>>` でホールド、(b) Extension の ON/OFF は `Disabled | Enabled(payload)` enum で表現、(c) `refresh_tee_info` 内で `solana_keys()` が `404` を返した場合のみ `Disabled` にする経路を実装。

### round3-d-new-004 should-fix `crates/tee/Cargo.toml` の `[dev-dependencies]` に `title-attestation = { workspace = true, features = ["mock"] }` を持つにも関わらず、`[dependencies]` 側で `title-attestation = { workspace = true }` を別途持つ — feature unification が `cargo build` の経路に依存する

- 場所: `crates/tee/Cargo.toml:32` と `:70`
- 観察: `[dependencies]` の `title-attestation` には feature を付けない一方、`[dev-dependencies]` で `features = ["mock"]` を有効化。`cargo test` / `cargo build --tests` では feature が unify されて `mock` が有効化された `title-attestation` がリンクされるが、`cargo build` だと `mock` が無効。これ自体は cargo の正式挙動だが、production build (`--no-default-features --features vendor-aws`) で `title-attestation` の `mock` が必要になる場面（例: e2e binary、bench、example）が現れたとき、暗黙ルールが破綻する。
- 問題: Round 2 should-fix-001 + round2-d-new-004 と同根。`mock` feature を `dev-dependencies` 経由で「裏口的に有効化」する pattern は、`cargo build --release` で挙動が変わる test harness を温存している（例: `MockAttestationVerifier::MEASUREMENT` 定数を non-test コードが参照する誘惑）。
- 修正案: (a) `mock` feature を `mock-verifier` に rename し、`title-tee` 側で `runtime-mock = ["title-attestation/mock-verifier"]` と明示、(b) `[dev-dependencies]` の features 指定を撤去し、production runtime + mock-verifier の同居が成立可能なことを `[features]` 表で表明、(c) Round 1 should-fix-001 と同時着手。

### round3-d-new-005 should-fix `crates/tee/src/main.rs:41-47` の `runtime_name` 既定値選択が `cfg!(feature = "runtime-mock")` ↔ `match runtime_name.as_str()` の二重 cfg で構成され、両 feature を同時 enable した build で `"mock"` が選ばれる

- 場所: `crates/tee/src/main.rs:41-47, 48-83`
- 観察: 既定値計算 `if cfg!(feature = "runtime-mock") { "mock".to_string() } else { "nitro".to_string() }` は、`cargo build --features "runtime-mock vendor-aws"` のような両 enable builds で「`runtime-mock` が enabled なら無条件で mock を選ぶ」挙動になる。後段の `match` 自体は `#[cfg(feature = "...")]` で arm を切り替えるため、両方 enabled なら両 arm が出現し `"mock"` arm が実行される。
- 問題: Spec §6.2 / §5.4 の reproducible build 前提では「TEE バイナリは ON/OFF 2 値で固定」というのが暗黙ルール。Cargo は features を additive 設計で扱うため、依存 crate が `runtime-mock` を要求すると `vendor-aws` バイナリでも mock が静かに混入する事故が起きうる。
- 修正案: (a) `runtime-mock` と `vendor-aws` を mutually exclusive にする `compile_error!` を `crates/tee/src/lib.rs` に追加、(b) `runtime_name` 既定値計算を `#[cfg(all(feature = "runtime-mock", not(feature = "vendor-aws")))]` で gate、(c) `cargo build` で何も指定しないと build error にし、開発時は明示的に `--features runtime-mock` を要求するよう default を撤去。

### round3-d-new-006 nitpick `crates/tee/src/lib.rs:9-16` の module 宣言順が alphabetical でなく、`runtime` と `vendor` が末尾に分離されて TeeRuntime 実装の所在が直感に反する

- 場所: `crates/tee/src/lib.rs:9-16`
- 観察: 現在の宣言順は `content_fetch, limits, orchestrator, proxy_fetcher, resource_pool, runtime, server, vendor`。`runtime` と `vendor` は `TeeRuntime` trait の実装が両方含まれるが、宣言順が `runtime` (mock のみ) → `server` → `vendor` (AWS のみ) で server 越しに分離されている。
- 問題: round2-d-new-007（`runtime/` と `vendor/` 中身 1 ファイルずつ）と複合し、「`TeeRuntime` の実装を探すなら mod ツリーのどこを見ればいい？」が一見不明。
- 修正案: round2-d-new-007 と同時に `runtime/{mod, mock, aws_nitro}` 統合へ。当面のミニマル対応として `lib.rs:9-16` を alphabetical 並びにする。

### round3-d-new-007 should-fix `crates/gateway/Cargo.toml` の `[dev-dependencies]` で `title-tee = { path = "../tee" }` を抱え、`workspace = true` を使わない — workspace ピン管理から外れる

- 場所: `crates/gateway/Cargo.toml:30`
- 観察: workspace internal crate 参照は基本 `workspace = true`（root `Cargo.toml:39-46`）。だが `crates/gateway/Cargo.toml:30` だけ `title-tee = { path = "../tee" }` と直接 path 指定。Round 1〜2 で should-fix-009（version hard-code）と関連するが、明示的指摘はなかった。
- 問題: workspace 全体で internal crate 参照を `workspace = true` に統一していたのに、Round 2 → Round 3 の間（または それ以前）から gateway だけ規約破り。version bump / path 変更で gateway だけ追従漏れする。
- 修正案: `crates/gateway/Cargo.toml:30` を `title-tee = { workspace = true }` に変更、root `Cargo.toml` の `[workspace.dependencies]` に `title-tee` が既登録（root:42）であることを確認済み。1 行修正で済む。

## 提案する new layout

Round 1〜2 と同じ。実装が動いていないので、提案を更新する根拠もない:

```
title-protocol/
├── crates/
│   ├── core/                  # request/response, Processor trait, jcs_sha256 helper (新設、現 orchestrator.rs / extension.rs の二重 JCS+SHA-256 を吸収)
│   ├── api/                   # 新設: 全 HTTP DTO（§2.5 / §6.2）。TEE と Gateway が共有
│   ├── crypto/                # 暗号原語
│   ├── attestation/           # AttestationVerifier trait + vendor_tags 新設
│   ├── attestation-aws-nitro/
│   ├── tee-core/              # TeeRuntime trait + ResourcePool + content_fetch + orchestrator
│   │   └── src/runtime/{mock,aws_nitro}.rs
│   ├── tee-server/            # axum server, main.rs（Extension Registry で組み立て）
│   ├── extension-solana/      # 旧 title-solana を改名（tee 依存なし、tee 側から trait impl で取り込む）
│   ├── gateway/               # 薄い relay
│   └── proxy/                 # HTTP forwarder
├── programs/title-whitelist/  # state.rs / events.rs / errors.rs / instructions/ 分割
└── sp1-guests/
    ├── host-cli/              # ベンダー切替 CLI
    └── attestation-aws-nitro/ # guest 専用
```

## 全体所感

Round 2 で「v0.1.3 で対応」と decode された wontfix 25 件は、Round 3 では 23 件 unchanged / 2 件 regressed。**「次フェーズに送る」ことが「単調に肥大する」と等価になっており**、特に `crates/tee/src/orchestrator.rs` は Round 1 (1185) → Round 2 (1205) → Round 3 (1294) と +109 行、`programs/title-whitelist/src/lib.rs` は (728 → 777 → 799) と +71 行。Round 1 で must-fix-004 / must-fix-005 が「分割しないと管理不能になる」と警告した直後から、両者は予告どおり管理コストを増やし続けている。

Round 3 新規 7 件は、Round 2 round2-d-new-006（`TeeAppState` 責務肥大）が production code path にまで波及した結果（round3-d-new-002）と、`mock` feature の「ふつうに見えて scope が広い」性質（round3-d-new-004 / 005）に集中している。Solana Extension の plug-in 化（must-fix-002）を v0.1.3 まで先送りしたまま、運用フローは Solana 前提で固まりつつあり、Extension 追加コストは Round 2 比で実質倍化（round3-d-new-002）。

特に **must-fix-001 の `keys/admin.json` は Round 2 処理ログで「`.gitignore` で新規 commit からは保護済み」と書かれているが、これは Round 1 round2-d-new-001 が指摘した誤った修正パターンそのもの**であり、Round 3 でもファイル本体が work tree に残置されたまま。v0.1.3 OSS 公開準備タスクに `git filter-repo` + 鍵 rotation を明示的に lock すべき。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| must-fix-001 | wontfix（Round 2 と同根、v0.1.3 OSS 公開準備で `git filter-repo` + admin key rotation を一括実施） | |
| must-fix-002 | wontfix（v0.1.3 Extension plug-in 化と一体で対応） | |
| must-fix-003 | partially-fixed（Round 2 のまま停止） | |
| must-fix-004 | regressed（v0.1.3 まで `orchestrator.rs` 分割をブロックしない。1294 行のまま放置すると Round 4 で +100 行追加のリスク） | |
| must-fix-005 | regressed（v0.1.3 Anchor 慣習分割と一体で対応） | |
| should-fix-001..003/006/008..012 | wontfix（v0.1.3） | |
| should-fix-004/005/007 | fixed | |
| nitpick-001 | fixed | |
| nitpick-002..006 | wontfix | |
| round2-d-new-001 | wontfix（must-fix-001 と同根） | |
| round2-d-new-002 | wontfix（v0.1.3。`register_key` 直上の require 順 justification が増えた点だけ flag） | |
| round2-d-new-003 | wontfix（must-fix-004 と同根） | |
| round2-d-new-004 | wontfix（v0.1.3 で doc string + feature rename） | |
| round2-d-new-005 | wontfix（Round 2 で意図と認定済み。3 系統並存だけ flag） | |
| round2-d-new-006 | regressed（main.rs 側に 10 フィールド組立コードが入った点で本質的に悪化） | |
| round2-d-new-007 | wontfix（v0.1.3 layout 統合と一体） | |
| round3-d-new-001 | wontfix | v0.1.3 の `main.rs` 分割リファクタと一体で対応。現状 `tee_seeded_rng` を bin 外に出す利得は test fixture 追加のみで小さい。 |
| round3-d-new-002 | wontfix | round2-d-new-006 (`TeeAppState` 責務肥大) と一体で v0.1.3 リファクタ。`test_state_with_fetcher` の重複は許容範囲。 |
| round3-d-new-003 | wontfix | Extension が複数になった時点で `ExtensionStatus` enum 化。現状 Solana 単一なら `Option<SolanaKeysResponse>` で実害なし。 |
| round3-d-new-004 | wontfix | `mock` feature の `dev-dependencies` 経由有効化は cargo の正式挙動。production 経路に mock が混入する具体例なし。should-fix-001 + round2-d-new-004 と一体で v0.1.3。 |
| round3-d-new-005 | fixed | `crates/tee/src/lib.rs` に `#[cfg(all(feature = "runtime-mock", feature = "vendor-aws"))] compile_error!(...)` を追加。両 feature 同時有効化を build error として fail-fast。仕様 §5.4 の「TEE バイナリは ON/OFF 2 値で固定」を構造的に強制する。 |
| round3-d-new-006 | wontfix | 監査の指摘前提が誤り。`lib.rs:9-16` は既に alphabetical (content_fetch, limits, orchestrator, proxy_fetcher, resource_pool, runtime, server, vendor) で並んでいる。「runtime と vendor が末尾」は alphabetical の結果として正しい配置。 |
| round3-d-new-007 | fixed | `crates/gateway/Cargo.toml:30` を `title-tee = { path = "../tee" }` から `title-tee = { workspace = true }` に変更。workspace 規約に揃え、root `Cargo.toml` 側の path 定義 1 箇所で管理可能に。 |
