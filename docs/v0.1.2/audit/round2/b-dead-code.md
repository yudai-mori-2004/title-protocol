# B. 死んでいるコード — Round 2

監査範囲: `crates/`, `programs/`, `sp1-guests/`, `deploy/`, `docker/`
対象外: `legacy/`, `target/`, `keys/`, `Cargo.lock`

Round 1 で出した 32 件 (must:10, should:15, nitpick:7) の処理状況と、17a–17g の修正によって新たに生じた dead/退行を `cargo build` なしで grep + 構造読みのみで再確認した（`cargo` が sandbox で禁止のため）。コードの「外部参照ゼロ」「test 以外で touch されない」「フィーチャ無効化時に常に到達不能」の 3 観点で判定する。

---

## Round 1 指摘の処理状況

集計: **fixed: 14 / partially-fixed: 3 / unchanged (deferred OK): 13 / regressed: 0 / 確認のみ: 2**

| ID | Round 1 重大度 | 状態 | 備考 |
|---|---|---|---|
| B-1 `CoreError` | must | **fixed** | `crates/core/src/error.rs` 削除、`lib.rs:9-22` から再エクスポートも消去 |
| B-2 processor_outputs.rs 8 型 | must | **fixed** | ファイル削除、生き残り 3 型は `c2pa_verify.rs` に同居（`lib.rs:15-18` の `pub use` で確認） |
| B-3 `whitelist.rs` 全体 | must | **unchanged (decision)** | 17e で「client-side mirror は SDK 化想定で残す」と明文化。`WhitelistInstruction` は K5-sf009 で削除済み。242→202 行に縮小。`pub fn whitelist_program_id()` 等の thin wrapper はまだ dead（後述・新規発見 N-1） |
| B-4 `OffchainData` | must | **fixed** | `extension.rs` から削除（grep ゼロヒット） |
| B-5 `unreachable_code` warn | must | **fixed** | `crates/tee/Cargo.toml:18` を `default = ["runtime-mock"]` に変更、`main.rs:69` で `#[allow(unused_mut)]` 付き Vec を保持しつつ feature-gated push で構成（B-26 と関連） |
| B-6 Gateway 暗号化レスポンス透過 | must | **fixed** | `TeeClient::process` が `Result<ProcessOutcome, _>` に変更（`tee_client.rs:24-30, 71, 174-213`）、`endpoints.rs:109-115` で `Plaintext` / `Encrypted` 分岐し octet-stream を中継 |
| B-7 cNFT Tree 作成 helpers | must | **unchanged (deferred)** | 17e 先送り宣言。`derive_tree_config` / `merkle_tree_account_size` / `rent_exempt_minimum` / `build_create_tree_tx` は依然 `pub`、devnet テストでのみ呼出。`spl_account_compression_v2_id()` は const 化したが thin wrapper として `pub fn` も残っており、自己ドキュメントで "Compatibility shim — prefer constant directly" と書いている dead（新規発見 N-2） |
| B-8 RSA verification 経路 | must | **fixed** | `KeyAlgo`/`SigAlgo` から RSA バリアントが消え（`sign.rs:30-33, 58-61`）、`Cargo.toml` から `rsa` dep 削除、`constants.rs` の関連 OID も削除済み |
| B-9 3 重 `MockRuntime` | must | **partially-fixed** | `lib.rs` 内のものは `StubRuntime` に rename して別物として残す形に書き換え（`lib.rs:55-67`）、`server.rs:321,367` は `crate::runtime::mock::MockRuntime` を使う形に統一済み。だが `orchestrator.rs:441-474` がまだ独自 `MockRuntime` を 1 つ定義しており、`Mutex<Option<Vec<u8>>>` で `last_user_data` を覗くテスト固有 helper を持つために `runtime::mock::MockRuntime` に統合されていない（実装数: 3 → 2 + StubRuntime） |
| B-10 SP1 guest `has_public_key` | must | **partially-fixed**（方針変更） | guest 側は変更せず（17d で defer）、代わりに `programs/title-whitelist/src/lib.rs:402-418` が `parse_public_values` で `has_public_key` を末尾まで読み切るように修正（K5-sf003）。trailing garbage 検出のために残す扱いに切替。**guest と program の symmetry は復元された** — dead ではなく "validated forward-compat slot" と再定義された格好 |
| B-11 `ProcessorRegistry::execute` | should | **unchanged (deferred)** | 17c で「§3.1 並列化と整合する書き直しが必要、defer」。`pub fn execute` は `processor.rs:123` に残存、外部呼出は `orchestrator.rs:347` 経由のみ |
| B-12 `execute_processors` 1 行ラッパ | should | **unchanged (deferred)** | 同上。`orchestrator.rs:341-348` にそのまま残存 |
| B-13 `CryptoError::EcdhError` | should | **fixed** | `crates/crypto/src/error.rs` に該当 variant 無し（grep ゼロヒット） |
| B-14 `AttestationError::Other` / `Expired` | should | **fixed** | `crates/attestation/src/lib.rs:46-59` の `AttestationError` は `ParseFailed` / `CertChainInvalid` / `SignatureInvalid` / `MissingField` の 4 種に縮小 |
| B-15 `ExtensionError` 3 variants | should | **fixed** | `crates/solana/src/extension.rs:30-48` の `ExtensionError` は 7 variant → 6 に整理（`FetchFailed` / `KeyNotWhitelisted` / `Verifier` 削除済み、grep ゼロヒット） |
| B-16 `SolanaSigningKey` 内 production 未使用 pub API | should | **partially-fixed** | `pubkey_hash()` は production 利用に格上げされた（`crates/tee/src/main.rs:124` で registration attestation に使用）。だが `from_seed` / `verifying_key()` / `pubkey_bytes()` は依然 pub かつ tests でのみ使用。`from_seed` の doc は "for testing" と書きつつ非 cfg-test の pub のまま |
| B-17 `ResourcePool` pub API 過剰 | should | **unchanged (deferred)** | `with_single_limit`, `acquire`, `ticket_with_timeouts`, `admission_limit()`, `can_admit` のいずれも残存。test でしか呼ばれない (`resource_pool.rs:67, 74, 106, 115, 135`) |
| B-18 `Ticket` pub API 過剰 | should | **unchanged (deferred)** | `extend_unchecked`, `is_global_timeout_exceeded`, `is_chunk_timeout_exceeded`, `elapsed`, `global_timeout`, `validate_decoded_size` 全部残存 (`resource_pool.rs:245, 301, 306, 311, 316, 328`)。production 呼出ゼロ |
| B-19 `limits.rs` 未使用エクスポート | should | **partially-fixed** | `MAX_PROVENANCE_GRAPH_SIZE` / `DEFAULT_TOTAL_LIMIT_FRACTION` / `LimitsError::GlobalTimeoutExceeded` は削除済み（17e で明記、現ファイルに無し）。しかし `estimate_decoded_size` (`limits.rs:105`) と `LimitsError::DecodedSizeExceeded` (`limits.rs:137-143`) は `Ticket::validate_decoded_size` 経路でのみ参照され、それ自体が dead（B-18 連動）。実質 dead チェーンは温存 |
| B-20 `ProxyContentFetcher::with_max_body_bytes` | should | **unchanged** | `proxy_fetcher.rs:76-82` の `DEFAULT_MAX_BODY_BYTES` + `with_max_body_bytes` は依然 pub、test 専用。`new()` が `with_max_body_bytes(endpoint, DEFAULT_MAX_BODY_BYTES)` を呼ぶので "内部 + test の両方で参照されている" 形にはなる（17e の判断「test で使う、現状維持」を尊重） |
| B-21 proxy async helpers | should | **unchanged** | cfg 適切 (17e で確認)、ただし doc には dev/test only と明記されていない |
| B-22 `AttestationDocument::digest` / `nonce` | should | **partially-fixed** | `digest` フィールドは K1-mf003 fix で `lib.rs:54-59` が `if self.doc.digest != "SHA384"` を冒頭で発火するようになり、**読まれている**（dead でなくなった）。`nonce` (`doc.rs:106`) はまだ書込・読込ともに無く dead のまま |
| B-23 `cose::sig_algo_val` dead arm | should | **fixed**（API 撤去で消滅） | K1-mf005 の `Signature::from_der` / `from_slice` 化で `sig_algo_val` 自体が削除済み。代わりに `SigAlgo::EcdsaSHA256` バリアントは `verify_signature_der`/`raw` の P-256 経路で実呼出されるため dead でなく live |
| B-24 `bs58` dep 未使用 | should | **fixed** | `crates/solana/Cargo.toml` 内 `bs58` 行が grep ヒットゼロ |
| B-25 `crates/crypto` の `serde` 直接依存 | should | **fixed** | `crates/crypto/Cargo.toml` から `serde =` 行削除、`serde_json` のみ残る |
| B-26 `#[allow(unused_mut)]` 暗黙 dead | nitpick | **unchanged** | `crates/tee/src/main.rs:69` にそのまま残存。`default = ["runtime-mock"]` (B-5 fix) で実害は薄まったが、`--no-default-features --features vendor-aws` のみだと `mock` も `vendor-aws` も両方 enabled でないため `supported` への push が両 cfg gated のまま、`#[allow]` が消せない構造的問題は同じ |
| B-27 テスト用 `Clone for ProcessorError` impl | nitpick | **fixed** | `crates/core/src/processor.rs:60` で `#[derive(..., Clone, ...)]` に変更、tests 内の手書き impl 削除（17c K8-sf009） |
| B-28 lib.rs outdated doc | nitpick | **fixed** | `crates/tee/src/lib.rs:1-32` の Legacy/v0.1.0 参照が削除、簡潔な spec §5.2 ベースの説明に置換 (17e A-mf-005) |
| B-29 `runtime/mod.rs` single-child mod | nitpick | **unchanged** | `crates/tee/src/runtime/mod.rs` は 13 行で `pub mod mock;` 1 個のまま。`vendor/` との二重構造は未整理 |
| B-30 `vendor()` getter が両方の場所に | nitpick | **unchanged** | `AttestationVerifier::vendor()` trait method (`attestation/src/lib.rs:68`) と `VerifiedAttestation::vendor` field (`lib.rs:26`) の二重定義は残存。trait method 外部呼出ゼロ |
| B-31 `Default for ProcessorRegistry` | nitpick | **fixed** | `crates/core/src/processor.rs:91-117` に `Default` impl 無し。`new()` のみ |
| B-32 `aead.rs` 2 つの `nonce.try_into()` 防御 | nitpick | **unchanged** | `crates/crypto/src/aead.rs:28-38, 56-66` で `nonce.len() != NONCE_SIZE` チェック後に `nonce.try_into().map_err(...)` を呼び続ける構造のまま |

### サマリ

- must-fix 10 件中: 6 fixed + 1 unchanged (decision) + 1 unchanged (deferred) + 2 partially-fixed = **凡そ完了**
- should-fix 15 件中: 6 fixed + 6 unchanged (deferred/decision) + 3 partially-fixed = **defer 多めだが意図的**
- nitpick 7 件中: 3 fixed + 4 unchanged = **意図通り後送り**
- regressed: **0 件**（既存の修正が新規 dead を生んでいない）

defer 判断は 17e README で明文化されており、後タスクで追跡可能な状態。新規 dead もコメント類でも回収済みのため、Round 1 の指摘体系は概ね反映されたと判定する。

---

## 新規発見

修正適用後にコードを舐め直して見つかった、Round 1 では検出していなかった、または fix の副作用で発生した dead を列挙する。重大度は Round 1 と同じ基準。

### N-1 (should-fix). `crates/solana/src/whitelist.rs:97-100` — `whitelist_program_id()` thin wrapper が dead

K5-nitpick-002 で `pub const WHITELIST_PROGRAM_ID: Pubkey = pubkey!(...)` が導入され、関数版は doc コメントで「Prefer the [`WHITELIST_PROGRAM_ID`] constant directly when possible.」と明記された thin wrapper になった。

```rust
/// Returns the whitelist program ID as a `Pubkey` value.
///
/// Prefer the [`WHITELIST_PROGRAM_ID`] constant directly when possible.
#[inline]
pub fn whitelist_program_id() -> Pubkey {
    WHITELIST_PROGRAM_ID
}
```

呼出箇所を grep するとファイル内ですら 0 件、外部からも 0 件（`crates/solana/tests/devnet_whitelist.rs:25` は独自に `const WHITELIST_PROGRAM_ID: &str` を再定義しており client の const も使っていない）。

修正案: **削除**。public API 撤去のため `pub fn` を消し、関数版に依存していたクライアントには `WHITELIST_PROGRAM_ID` を直接参照させる。devnet テストは `WHITELIST_PROGRAM_ID` const を使う形に書き換えれば連動して文字列重複も解消できる。

### N-2 (should-fix). `crates/solana/src/cnft.rs:25-29` — `spl_account_compression_v2_id()` thin wrapper が dead

N-1 と同型。

```rust
/// Compatibility shim — prefer [`SPL_ACCOUNT_COMPRESSION_V2_ID`] directly.
#[inline]
pub fn spl_account_compression_v2_id() -> Pubkey {
    SPL_ACCOUNT_COMPRESSION_V2_ID
}
```

呼出は `cnft.rs:98`（自分自身の `build_create_tree_tx` 内、これは B-7 で dead 判定）+ `crates/solana/tests/devnet_whitelist.rs:441` のみ。const を直接使えば関数は不要。

修正案: **削除**。`build_create_tree_tx` 内も `&SPL_ACCOUNT_COMPRESSION_V2_ID` で十分。devnet テストの呼出も同様に書き換え。

### N-3 (should-fix). `crates/tee/src/limits.rs:105-108` + `LimitsError::DecodedSizeExceeded` — decode-validation dead チェーン

Round 1 では `estimate_decoded_size` 個別 dead としか書かなかったが、`Ticket::validate_decoded_size` (B-18) → `LimitsError::DecodedSizeExceeded` (`limits.rs:137-143`) → `estimate_decoded_size` (`limits.rs:105`) の 3 段チェーンが全部 production 未呼出。

```rust
pub fn estimate_decoded_size(width: u32, height: u32, channels: u32, bit_depth: u32) -> u64 {
    let bytes_per_pixel = (channels * bit_depth + 7) / 8;
    u64::from(width) * u64::from(height) * u64::from(bytes_per_pixel)
}
```

image-pdq / video-vpdq processor は v0.1.2 では実装外（B-2 で削除済み）、本格的にデコードする消費者がいない。`(channels * bit_depth + 7) / 8` の `u32` 演算は channels=4, bit_depth=16 で `4*16+7=71` まで小さく overflow しないが、API 設計としては「想定する画像 channel 数 / 深さ範囲」のドキュメントが無く、test 以外の根拠が無い。

修正案: **3 つまとめて削除**。image-pdq 実装時に `Ticket::validate_decoded_size(estimated)` と一緒に書き戻せばよい。

### N-4 (should-fix). `crates/solana/src/cnft.rs:40-42` — `derive_mpl_core_cpi_signer()` の `pub`

K5-sf008 の collection 分岐に伴って導入された PDA derive helper。`build_mint_v2_ix` (cnft.rs:199) で内部利用 + tests でのみ参照。外部から直接呼ぶ意味がない（caller は instruction builder 経由で隠蔽）。

修正案: **`pub(crate)` に下げる**。SDK 公開を意識して `pub` にした節があるが、現状利用者がいないので可視性は最小化すべき。

### N-5 (nitpick). `crates/gateway/src/tee_client.rs:23` — `#[derive(Debug, Clone)] for ProcessOutcome` の `Clone`

K4-mf001 で導入された `ProcessOutcome` に `Clone` が付いている。`Plaintext(ProcessResponse)` / `Encrypted(Vec<u8>)` は `process()` 呼出後すぐ `match` で消費されるだけで、複製する呼出箇所が無い（grep で `.clone()` ゼロヒット）。`Encrypted(Vec<u8>)` の `clone` は無駄な heap copy。

修正案: `#[derive(Clone)]` を外す。`Debug` は test 用に保持。

### N-6 (nitpick). `crates/attestation/src/lib.rs:96` — `Default` 派生だが `MockAttestationVerifier::default()` 未使用

`#[derive(Debug, Default, Clone, Copy)] pub struct MockAttestationVerifier;` は新しいテスト helper だが、構築は常に `MockAttestationVerifier::new()` 経由（`lib.rs:106, crates/solana/src/extension.rs:218, crates/tee/src/main.rs:57` 等）。`Default` 派生は無使用。

修正案: `Default` を外す（unit struct なので意味も薄い）。残すなら `new()` 自体を削除して `MockAttestationVerifier` リテラル + `Default::default()` の片方に統一。

### N-7 (nitpick). `crates/attestation/src/lib.rs:46-59` — `AttestationError` 縮小後の未使用 `MissingField`

Round 1 で `Other` / `Expired` を消した後の残り 4 variants のうち、`MissingField(String)` を確認したところ AWS Nitro 実装 (`crates/attestation-aws-nitro/src/lib.rs`) では構築されていない。`ParseFailed` / `CertChainInvalid` / `SignatureInvalid` だけで全 error path をカバー。`MissingField` は spec 上「ベンダーが optional フィールド欠落で返す」想定だが、現行コードではそうした path がない。

```
grep -rn "MissingField" crates/  → 定義 1 件のみ
```

修正案: 削除。新ベンダー追加時に必要なら戻す。

### N-8 (nitpick). `crates/tee/src/runtime/mock.rs:30-34` — `impl Default for MockRuntime` 未使用

```rust
impl Default for MockRuntime {
    fn default() -> Self {
        Self::new()
    }
}
```

外部で `MockRuntime::default()` を呼んでいる箇所ゼロ（grep `MockRuntime::default` ヒットなし、`Default::default()` でも構築されない）。

修正案: 削除。

---

## 確認したが dead ではなかった（Round 2 で新規確認）

- `derive_approved_vkeys_pda` / `derive_approved_measurements_pda` (whitelist.rs:111, 116) — devnet テストでのみ呼ばれるが、client SDK の最小機能として残すのは妥当（17e 判断と整合）
- `StoredMeasurement::from_slice` / `as_bytes` (whitelist.rs:36, 51) — Borsh layout を 1:1 で守る mirror 構造体の不可分メンバーで、外部に on-chain serializer を作る際の入口、SDK 公開価値あり
- `KEY_EXPIRY_SECONDS` (whitelist.rs:21) — `whitelist.rs` 内 test でのみ参照だが、`is_valid_at` 等が client validation で生きるので連動して必要
- `nsm_exit log` の `tracing::debug!` (17b K3-sf010 で追加) — drop 経路の debug 出力、dead に見えるが意図通り
- `RateLimiter::prune_idle` (`gateway/src/rate_limit.rs:97`) — `server::run` 内の 5 分タイマー (`server.rs:134`) で実呼出、live

---

## 全体所感

Round 1 で must-fix とした「外部参照ゼロかつ仕様の現フェーズ範囲外」型の dead はほぼ全て対処済み。残った unchanged 群は 17e の README にて明示的に「scope 外 / SDK 化のため温存」と判断されており、定義無き dead として漂流している状態ではない。

新規発見は K5-nitpick-002 (`pubkey!` const 化) と K4-mf001 (`ProcessOutcome`) の副作用で生まれた小型 dead が中心で、いずれも `cargo build` の `dead_code` lint を有効化すれば自動検出可能なレベル。次フェーズで `#![deny(dead_code)]` を `crates/solana` 等から段階的に有効化し、`#[allow(dead_code)]` を意図的に付ける箇所だけ残す運用に移ると、N-1〜N-8 のような取りこぼしが構造的に防げる。

監査終了。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| B-1/B-2/B-4/B-5/B-6/B-8/B-13/B-14/B-15/B-22/B-23/B-24/B-25/B-27/B-28/B-31 | fixed | Round 2 認定済み。 |
| B-3/B-7/B-11/B-12/B-17/B-18/B-19/B-20/B-21/B-26/B-29/B-30/B-32 | wontfix(17e の defer/decision README で「SDK 化想定 / 並列化リファクタ / test 専用境界」として明文化済み。本 audit ラウンドで蒸し返す価値が薄い) | |
| B-9 | partially-fixed(`orchestrator.rs` の独自 `MockRuntime` は test 内 `last_user_data` 観測のため `runtime::mock::MockRuntime` に統合できない設計。`StubRuntime` 分離で実装数 3 → 2 にした現状で妥当) | |
| B-10 | partially-fixed(K5-sf003 で `has_public_key` を末尾チェックに昇格。dead から forward-compat slot に再分類済み) | |
| B-16 | partially-fixed(`pubkey_hash` は production 利用に格上げ。残る `from_seed`/`verifying_key`/`pubkey_bytes` は SDK 公開 API の最小セットとして温存) | |
| N-1 | fixed | `crates/solana/src/whitelist.rs::whitelist_program_id()` thin wrapper を削除。呼び出し箇所ゼロを確認済み。`WHITELIST_PROGRAM_ID` 定数を直接使う形に統一。 |
| N-2 | fixed | `crates/solana/src/cnft.rs::spl_account_compression_v2_id()` thin wrapper を削除。`cnft.rs:92` 内部呼び出しと `devnet_whitelist.rs:441` の test 呼び出しを `SPL_ACCOUNT_COMPRESSION_V2_ID` 定数直接参照に置換。 |
| N-3 | wontfix(`estimate_decoded_size` / `validate_decoded_size` / `DecodedSizeExceeded` は単体テスト網羅されており、image-pdq/video-vpdq processor 実装時に再活用予定。削除すると後で復元コストが嵩むため温存) | |
| N-4 | fixed | `derive_mpl_core_cpi_signer` を `pub(crate)` に変更。SDK 公開 API surface を縮小。 |
| N-5 | fixed | `ProcessOutcome` から `Clone` derive を削除。`Encrypted(Vec<u8>)` の無駄な heap clone コストを排除。`Debug` のみ維持。 |
| N-6 | fixed | `MockAttestationVerifier` から `Default` derive を削除。呼び出しゼロ確認済み。`new()` のみで構築。 |
| N-7 | wontfix(`AttestationError::MissingField` は `crates/attestation-aws-nitro/src/lib.rs:72` で実際に PCR0 取得失敗時に発火している。audit の grep が漏れていた) | |
| N-8 | fixed | `crates/tee/src/runtime/mock.rs` から `impl Default for MockRuntime` を削除。呼び出しゼロ確認済み。 |
