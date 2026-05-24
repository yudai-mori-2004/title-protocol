# D. アーキテクチャ・ディレクトリ構造 — Round 2

## 概要

担当範囲: workspace 構造（`Cargo.toml`）、`crates/`・`programs/`・`sp1-guests/`・`deploy/`・`docker/`・`docs/`・`keys/`・`legacy/` の配置、各 crate の責務・境界・公開 API、ファイルサイズ・多責務、命名、依存方向。

Round 1 の指摘 23 件（must:5 / should:12 / nitpick:6）が修正適用後にどう処理されているかを実コード上で 1 件ずつ突合した。同時に、修正で生まれた退行・新規問題を検出した。

リポジトリ全景（変化なし — Round 1 と同じ）：

```
title-protocol/
├── Cargo.toml                # workspace = 8 crates + 3 exclude
├── crates/
│   ├── core/      crypto/   attestation/   attestation-aws-nitro/
│   ├── tee/  (vendor/ + runtime/ 二層構造のまま)
│   ├── solana/  gateway/  proxy/
├── programs/title-whitelist/ # 単一 lib.rs (777 行に増加)
├── sp1-guests/attestation-aws-nitro/{host,program}
├── keys/admin.json           # ファイルがまだ存在する
└── legacy/v0.1.0/
```

## Round 1 指摘の処理状況

| ID | カテゴリ | Round 1 | Round 2 status | 備考 |
|---|---|---|---|---|
| must-fix-001 | `keys/admin.json` 秘密鍵コミット | must | **partially-fixed** | `.gitignore` に `keys/` `keypair.json` を追加した形跡あり (.gitignore:28-31)。しかし `keys/admin.json` のファイル自体は依然としてリポジトリに存在し中身も同じ秘密鍵。`.gitignore` は **既にトラックされているファイルには効かない**。`git rm --cached` か履歴消去が必要。 |
| must-fix-002 | `title-tee` が `title-solana` に静的依存 | must | **unchanged** | `crates/tee/Cargo.toml:34` は `title-solana = { workspace = true }` のまま（optional ですらない）。`crates/tee/src/server.rs:29-30` で `title_solana::extension`・`signing_key` を直接 import、`/solana-keys` `/extension/solana` を unconditional に router へ取り付けている。Extension の差替・無効化は実装的に不可能なまま。 |
| must-fix-003 | `aws-nitro` / `aws_nitro` / `nitro` 表記揺れ | must | **partially-fixed** | テストの `"aws_nitro"` は `"aws-nitro"` に修正された（`crates/gateway/src/lib.rs:217`）。一方で `main.rs:41-47, 61, 72-74` の env キー `"nitro"` は健在で、`runtime_name == "nitro"` 分岐がそのまま残る。中央集約のための `vendor_tags` モジュールは新設されていない（`crates/attestation/src/lib.rs` には `VENDOR` 定数が `MockAttestationVerifier` impl 内 1 件あるだけ）。識別子は依然として 2 系統（`"aws-nitro"` と `"nitro"`）に分裂したまま。 |
| must-fix-004 | `crates/tee/src/orchestrator.rs` 1185 行 | must | **regressed** | 1185 → **1205 行** に増加（`wc -l`）。指摘した分割（`orchestrator/{mod,error,encryption,attest}.rs` + `tests/fixtures.rs`）は実施されておらず、テストヘルパー `create_signed_jpeg` は `server.rs:565-597` にも依然として複製。問題範囲は拡大した。 |
| must-fix-005 | `programs/title-whitelist/src/lib.rs` 728 行 | must | **regressed** | 728 → **777 行** に増加。Anchor の `state.rs / errors.rs / events.rs / instructions/` 分割は未実施で、`ADMIN_AUTHORITY` 周辺に Phase 1 → multisig 移行計画コメントが追加されたぶん長くなっている。 |
| should-fix-001 | `attestation/mock` feature の命名一貫性 | should | **unchanged** | `crates/attestation/Cargo.toml:13` は `mock = []` のまま。`title-tee` の `runtime-mock = ["title-attestation/mock"]` (`crates/tee/Cargo.toml:20`) との非対称も同じ。 |
| should-fix-002 | proxy と tee の `vendor-aws` 同名・別意味 | should | **unchanged** | `crates/proxy/Cargo.toml:21` (`vendor-aws = ["dep:vsock"]`)、`crates/tee/Cargo.toml:22-27` (NitroRuntime + NSM + serde_bytes + vsock) ともに `vendor-aws` を別意味で使い続けている。 |
| should-fix-003 | JCS+SHA-256 ロジック二重実装 | should | **unchanged** | `crates/tee/src/orchestrator.rs:355` `fn compute_jcs_hash` と `crates/solana/src/extension.rs:98` `fn compute_verifiable_hash` の両方が残る。`extension.rs:97` のコメント `// Spec §1.5, §2.3 — same as orchestrator.rs but standalone.` も残存し、依然として「2 箇所定義」が明示されている。`title-core` への抽出は未実施。 |
| should-fix-004 | `OffchainData` 未使用 struct | should | **fixed** | `crates/solana/src/extension.rs` から削除済（grep ヒットなし）。 |
| should-fix-005 | `hex_short` / `hex_encode` 手書きエンコーダ重複 | should | **fixed** | 両関数とも消滅、`hex::encode` に統一（`crates/solana/src/extension.rs:141-142`、`crates/tee/src/main.rs:116`）。 |
| should-fix-006 | `SolanaExtensionBody` ↔ `SolanaExtensionRequest` 別型 | should | **unchanged** | `crates/tee/src/server.rs:210-218` (`SolanaExtensionBody`) と `crates/gateway/src/lib.rs:137-154` (`SolanaExtensionRequest`) が依然として別型。共有 crate (`title-api` 等) は新設されていない。 |
| should-fix-007 | TEE crate doc が「v0.1.0 からの変更点」 | should | **fixed** | `crates/tee/src/lib.rs:1-49` から `## Legacy参照` `# v0.1.0からの変更点` セクションが消え、Spec §5.2 を主軸にした簡潔な doc に書き換わっている。 |
| should-fix-008 | `programs/title-whitelist/keypair.json` コミット | should | **partially-fixed** | `.gitignore:31` に `keypair.json` を追加したが、ファイル本体（`programs/title-whitelist/keypair.json`, 233 B, mode 600）はまだ work tree に存在し、内容も同じ deploy keypair。`git rm --cached` 未実施の典型形。`Anchor.toml` の `[provider].wallet` は別途要確認。 |
| should-fix-009 | `programs/` `sp1-guests/` の version 等重複 | should | **unchanged** | `programs/title-whitelist/Cargo.toml:3-7` は `version = "0.1.2"` を hard-code したまま。CI lint / `cargo workspaces` 等の自動化も入っていない。 |
| should-fix-010 | `sp1-guests/attestation-aws-nitro/` の名前空間がベンダー固定 | should | **unchanged** | ディレクトリ構造は `sp1-guests/attestation-aws-nitro/{host,program}` のまま。`sp1-guests/host-cli/` への統合は行われていない。 |
| should-fix-011 | `gateway/src/lib.rs` に API DTO 直書き | should | **unchanged** | `crates/gateway/src/lib.rs:29-162` で `KeysResponse` / `ProcessorsResponse` / `HealthResponse` / `SolanaKeysResponse` / `SolanaExtensionRequest` / `SolanaExtensionResponse` を依然として直接定義。`title-api` 等への分離未実施。 |
| should-fix-012 | `vendor/` と `runtime/` の二層分割 | should | **unchanged** | `crates/tee/src/vendor/mod.rs` (18 行、`#[cfg(feature = "vendor-aws")] pub mod aws;` のみ)、`crates/tee/src/runtime/mod.rs` (12 行、`pub mod mock;` のみ) が二つ並んでいる現状は維持。 |
| nitpick-001 | `SS` エスケープ表記 | nitpick | **fixed** | `crates/` 配下を grep しても `SS[0-9]` の残りなし。 |
| nitpick-002 | `gateway/src/lib.rs` 日英混在 | nitpick | **unchanged** | `lib.rs:1-19` 英語、`:37-162` 日本語のまま。 |
| nitpick-003 | `proxy/src/protocol.rs` の cfg 順序 | nitpick | **unchanged** | `crates/proxy/src/protocol.rs:94-100` 以降で `#[cfg(all(target_os = "linux", feature = "vendor-aws"))]` を関数ごとに繰り返す形のまま。`mod sync_io` への束ね込みは未実施。 |
| nitpick-004 | `Cargo.toml` members/exclude の順序 | nitpick | **unchanged** | members の並びは `attestation, attestation-aws-nitro, core, crypto, tee, gateway, proxy, solana`（`tee` が `crypto` の直後で alphabetical からズレる）。 |
| nitpick-005 | `[[bin]] path = "src/main.rs"` 冗長 | nitpick | **unchanged** | `crates/{gateway,tee,proxy}/Cargo.toml:10-12` に依然として `path = "src/main.rs"`。 |
| nitpick-006 | `server.rs::handle_keys` の `json!` 直書き | nitpick | **unchanged** | `crates/tee/src/server.rs:108-128` で `Json(serde_json::json!({...}))` を 3 か所で生成。`KeysResponse` 等 DTO は不使用のまま。 |

### 集計

- fixed: **4** (should-fix-004, should-fix-005, should-fix-007, nitpick-001)
- partially-fixed: **3** (must-fix-001, must-fix-003, should-fix-008)
- unchanged: **14**
- regressed: **2** (must-fix-004, must-fix-005)

**Round 1 の must-fix 5 件のうち、完全解決はゼロ件**。3 件が partially-fixed、2 件が逆行（行数増）という結果になっている。

## 新規発見

### round2-d-new-001 must-fix `keys/admin.json` の「.gitignore 追加だけで安心している」誤った修正パターン

- 場所: `.gitignore:28-31`、`keys/admin.json` (現存)、`programs/title-whitelist/keypair.json` (現存)
- 観察: Round 1 が指摘した秘密鍵に対し、修正は `.gitignore` パターン追加で止まっている。`git ls-files` はサンドボックス制約で直接確認できなかったが、ファイルが work tree に残っており、ファイル mode (`-rw-r--r--`) も依然として読み取り可能。
- 問題: `.gitignore` はトラック済みファイルには無効。OSS 公開直前にこの状態で push すれば履歴に秘密鍵が残る。「修正したつもり」状態の方が、最初から未修正より危険（次の作業者が「もう gitignore したから大丈夫」と誤解する）。
- 修正案: 順序を厳密に — (1) `git rm --cached keys/admin.json programs/title-whitelist/keypair.json` を実行、(2) 同コミットで `keys/admin.json` の中身に対応する `ADMIN_AUTHORITY` (programs/title-whitelist/src/lib.rs:41-44) と Anchor の `programDataAddress` を **ローテーション**、(3) 履歴に残る秘密鍵を `git filter-repo` で除去、(4) deploy 鍵の運用方法を `deploy/aws/scripts/` 配下に手順化。`.gitignore` だけでは must-fix は閉じない。

### round2-d-new-002 should-fix `programs/title-whitelist/src/lib.rs` 内 `ADMIN_AUTHORITY` の「Phase 1 → multisig 移行計画」コメント追加が、Round 1 が削除推奨したパターンに合致

- 場所: `programs/title-whitelist/src/lib.rs:33-40`
- 観察: Round 1 後に新たに追加されたとみられるコメント `/// Phase 1: single wallet. Future: multi-sig / DAO migration plan: A) ... B) ...` が、Anchor program のコード本体より長く挿入されている。
- 問題: タスク 16 README §「4.7 の癖の例」の (4)「やらなかった理由・将来やる予定の長文 rationale」典型例。仕様書もコードも参照しない「設計者の頭の中の TODO」が rustdoc に焼き付いていて、初見の読み手にとってはノイズ。`ADMIN_AUTHORITY` の rotation が現状 `anchor upgrade` 経由でしかできない事実は、コードの挙動からは導けない暗黙の前提として残るほうが価値が低い。
- 修正案: rotation 計画を `docs/v0.1.2/SPECS_JA.md` § 6.2 または `docs/v0.1.2/ROADMAP.md` に移し、コードコメントは `/// Admin authority pubkey: wrVwsTuRzbsDutybqqpf9tBE7JUqRPYzJ3iPUgcFmna.` の 1 行に絞る。

### round2-d-new-003 must-fix `crates/tee/src/orchestrator.rs` が増えたぶんの内容を確認すると、テストヘルパー多重定義が更に深刻化

- 場所: `crates/tee/src/orchestrator.rs:1205` 行、`crates/tee/src/server.rs:565-597`、`crates/gateway/tests/e2e.rs` (要再確認)
- 観察: Round 1 で 1185 → Round 2 で 1205 行に。`compute_jcs_hash` (orchestrator.rs:355) はそのまま、テストモジュールは依然として 800 行超。`create_signed_jpeg` も `crates/tee/src/server.rs:565` で 33 行の同等実装が共存。
- 問題: Round 1 must-fix-004 がそのまま延びている。共通 fixture (`crates/tee/tests/fixtures.rs` などの統合 test helper crate) が無いまま、`#[cfg(test)] mod tests` の中に EphemeralSigner ベースの JPEG 生成を貼り付けるパターンが固定化しつつある。
- 修正案: Round 1 と同じ分割案 + `crates/tee/tests/common/mod.rs` (integration test 専用 fixture module) を新設して `pub fn create_signed_jpeg() -> Vec<u8>` を一箇所に集約。

### round2-d-new-004 should-fix `crates/attestation/Cargo.toml` の `mock` feature がいつのまにかテスト用必須コンポーネントに格上げされている

- 場所: `crates/attestation/Cargo.toml:13`、`crates/tee/Cargo.toml:70` (`title-attestation = { workspace = true, features = ["mock"] }` in `[dev-dependencies]`)、`crates/gateway/Cargo.toml:33`、`crates/tee/src/server.rs:374` (`title_attestation::MockAttestationVerifier::new()`)
- 観察: `mock` feature は元々「test-only」と宣言されているが、現状は `title-tee` の `[dev-dependencies]` と `default = ["runtime-mock"]` 経由の両方から要求され、`server.rs` のテストハーネスからも `MockAttestationVerifier` を `tests` ではなく `runtime-mock` feature ビルドの bin が利用する状態。
- 問題: Round 1 should-fix-001 で指摘した命名非対称 (`mock` ↔ `runtime-mock`) が解消されないまま、`mock` の責務が「テスト専用」から「dev binary でも有効」に拡大。コメント `// Enables MockAttestationVerifier. Test-only.` が実態と乖離している。production ビルドで feature 無効化が漏れる risk が増えた（仮に `default = ["runtime-mock"]` を維持しつつ production を `--no-default-features --features vendor-aws` で組むという運用に依存）。
- 修正案: (a) `mock` の doc string を「Dev binary & test usage. MUST be disabled in production via `--no-default-features`.」に改める、(b) feature 名を `mock-verifier` にリネームし用途を明示、(c) `crates/tee/src/main.rs` 起動時に production runtime と mock verifier が同居していたら panic する safety net を追加。

### round2-d-new-005 nitpick `crates/tee/src/orchestrator.rs:5` doc comment の Spec 参照が `§` ではなく `--` に置換されている

- 場所: `crates/tee/src/orchestrator.rs:5, 9, 11, 12, 13, 14, 15, 16, 17, 22, 31, 58, 64, 72, 73, ...`
- 観察: `Spec §5.2 -- TEE request processing flow` のように `--` (em dash 代用) が併用されている。一方 `crates/tee/src/server.rs:5-15` は `Spec §2.5, §5.2` を `--` 無しで素直に書く。nitpick-001 で `SS` を `§` に直したついでに、`--` の使い方が orchestrator.rs だけ独自スタイルになった。
- 問題: 同一 crate 内でも書式が割れる。doc 生成時の見た目（rustdoc は `--` を en dash にレンダする）が他ファイルと違ってしまう。
- 修正案: orchestrator.rs の `Spec §X.Y -- 説明` を `Spec §X.Y — 説明` （em dash）または `Spec §X.Y. 説明` に統一。crate 全体で 1 方針。

### round2-d-new-006 should-fix `crates/tee/src/server.rs::TeeAppState` が「コア・Solana Extension 状態の bag」になり責務肥大化

- 場所: `crates/tee/src/server.rs:43-74`
- 観察: `TeeAppState` 1 struct に `runtime / key_bundle / solana_key / registry / pool / fetcher / attestation_verifier / expected_measurement / registration_attestation / started_at` の 10 フィールドが入る。コア処理用 (`runtime`, `key_bundle`, `registry`, `pool`, `fetcher`) と Solana Extension 用 (`solana_key`, `attestation_verifier`, `expected_measurement`, `registration_attestation`) が混在。
- 問題: must-fix-002 と同根。仕様 §6.1 は「コアと Extension は別レイヤー」と謳うが、サーバー状態の表現でも分離されておらず、新 Extension（Ethereum 等）を増やすたびにフィールドが膨らむ。
- 修正案: (a) `pub struct CoreState { runtime, key_bundle, registry, pool, fetcher }` + `pub struct SolanaExtensionState { solana_key, attestation_verifier, expected_measurement, registration_attestation }` に分け、`TeeAppState { core: Arc<CoreState>, extensions: ExtensionRegistry }` に再構成、(b) 各 Extension は `trait TeeExtension { fn routes(&self, core: Arc<CoreState>) -> Router; }` を実装して `extensions.attach(router)` で組み立てる。仕様 §6.1 の境界とコード境界を一致させられる。

### round2-d-new-007 should-fix `runtime/` ディレクトリは存続しているのに mock のみ、`vendor/` は AWS のみという「中身が 1 ファイルずつの 2 階層」が放置

- 場所: `crates/tee/src/runtime/{mod.rs, mock.rs}`、`crates/tee/src/vendor/{mod.rs, aws.rs}`
- 観察: Round 1 should-fix-012 で指摘した中途半端な分割が、修正されていないどころか他の修正で動かなかったことが明らかに。
- 問題: 「実 TEE = vendor/」「mock = runtime/」というルール自体は doc コメント (`vendor/mod.rs:3-15`) で表明されているが、「mock も TeeRuntime の実装」「実 TEE も TeeRuntime の実装」というのが本質で、ディレクトリ分けに正当性がない。MockRuntime も「dev runtime vendor」と捉えれば全部 `vendor/{mock,aws}.rs` で済む。
- 修正案: Round 1 同案: `crates/tee/src/runtime/{mod.rs, mock.rs, aws_nitro.rs}` に統合。`vendor/` を廃止。`mod.rs` で `#[cfg(feature = "runtime-mock")] pub mod mock;` / `#[cfg(feature = "vendor-aws")] pub mod aws_nitro;` を並べる。

## 提案する new layout

Round 1 と同じ。実装側で何も動いていないので、提案レイアウトを更新する根拠もない:

```
title-protocol/
├── crates/
│   ├── core/                  # request/response, Processor trait, jcs_sha256 helper (新設)
│   ├── api/                   # 新設: 全 HTTP DTO（§2.5 / §6.2）
│   ├── crypto/                # 暗号原語
│   ├── attestation/           # AttestationVerifier trait + vendor_tags 新設
│   ├── attestation-aws-nitro/
│   ├── tee-core/              # TeeRuntime trait + ResourcePool + content_fetch + orchestrator
│   │   └── src/runtime/{mock,aws_nitro}.rs
│   ├── tee-server/            # axum server, main.rs（Extension Registry で組み立て）
│   ├── extension-solana/      # 旧 title-solana を改名（tee 依存なし）
│   ├── gateway/               # 薄い relay
│   └── proxy/                 # HTTP forwarder
├── programs/title-whitelist/  # state.rs / events.rs / errors.rs / instructions/ 分割
└── sp1-guests/
    ├── host-cli/              # ベンダー切替 CLI
    └── attestation-aws-nitro/ # guest 専用
```

## 全体所感

Round 1 で挙げた 23 件のうち、**must-fix 5 件は 1 件も完全解決していない**。3 件が partial（.gitignore 追加で止まる / テストの一部表記揺れ修正で止まる / vendor_tags 統合無しで identifier 揺れの一部分のみ解消）、2 件は逆行している（行数が増加した）。must-fix-002（`title-tee` → `title-solana` 静的依存）と must-fix-003（vendor 識別子表記揺れ）は次フェーズの「ベンダー追加」「Extension 差し替え」の難易度を直接決める骨格的問題で、本番化フェーズの検収を通すには round2-d-new-006（`TeeAppState` 責務肥大化）と合わせた一括リファクタリングが必要。

特に **must-fix-001 の対応パターン（`.gitignore` 追加のみ）は OSS 公開直前で最も危険な誤修正**で、「修正済みに見えるが実は秘密鍵が残っている」状態を作っている。Round 2 で再指摘するだけでなく、修正計画タスク（17）の最優先項目として `git rm --cached` + 鍵ローテーション + 履歴消去を一連で扱うべき。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| must-fix-001 | wontfix(`keys/admin.json` の `git rm --cached` + 履歴消去 + admin 鍵ローテーションは破壊的操作で、admin 権限の連鎖変更を伴う。本 audit ラウンドのスコープを超えるため、別タスク (v0.1.3 OSS 公開準備) で `git filter-repo` + 鍵ローテーションを一括実施。`.gitignore` 追加で新規 commit からは保護済み) | |
| must-fix-002 | wontfix(`title-tee → title-solana` 静的依存は SPECS_JA §6 で Extension が core 機能の一部として組み込まれている前提の設計。optional 化は extension の plugin 化リファクタを伴い v0.1.3 で対応) | |
| must-fix-003 | partially-fixed(`"aws-nitro"` 表記の test 側は修正済み。`main.rs` の env キー `"nitro"` 残置は将来の vendor 切替実装と併せて整理) | |
| must-fix-004 | wontfix(`orchestrator.rs` 1205 行の分割リファクタは責務境界の再設計を伴い、K3 round 1 with same issue でも defer 判定。v0.1.3 で対応) | |
| must-fix-005 | wontfix(`programs/title-whitelist/src/lib.rs` 777 行の Anchor 慣習に従った分割は IDL 生成・テスト整合の再構築を伴い、program 再 deploy を要する。v0.1.3 SDK 整備フェーズで対応) | |
| should-fix-001..003/006/008..012 | wontfix(naming一貫性 / API 重複 / Anchor wallet / extension DTO 共有 / vendor naming は v0.1.3 SDK 整備フェーズで一括対応。本 audit ラウンドのコスト対効果と合致せず) | |
| should-fix-004/005/007 | fixed | Round 2 認定済み。 |
| nitpick-001 | fixed | Round 2 認定済み。 |
| nitpick-002..006 | wontfix(doc 英日統一 / Cargo manifest 整理 / DTO 化は OSS 公開前の品質向上フェーズで対応) | |
| round2-d-new-001 | wontfix(must-fix-001 と同根。秘密鍵ローテーションは別タスク) | |
| round2-d-new-002 | wontfix(`ADMIN_AUTHORITY` の Phase 1 → multisig migration plan コメントは OSS 公開時の OSS reader 向け情報として価値あり。SPECS_JA への移動は v0.1.3) | |
| round2-d-new-003 | wontfix(must-fix-004 と同根。test fixture 共有化は orchestrator.rs 分割と同時実施) | |
| round2-d-new-004 | wontfix(`mock` feature の責務拡大は B-5 で意図的に `default = ["runtime-mock"]` 化済み。doc string の更新は v0.1.3 で対応) | |
| round2-d-new-005 | wontfix(`Spec §X --` の `--` 表記は意図的な ASCII separator。rustdoc レンダリング上問題なし) | |
| round2-d-new-006 | wontfix(must-fix-004 / must-fix-002 と同根の責務分離リファクタ。v0.1.3 で対応) | |
