# B. 死んでいるコード — Round 3

監査範囲: `crates/`, `programs/`, `sp1-guests/`, `deploy/`, `docker/`
対象外: `legacy/`, `target/`, `keys/`, `Cargo.lock`

Round 2 で fixed / wontfix と裁定された各件、および Round 2 の処理ログで「fixed」と申告された N-1〜N-8 を、コード現物と grep 横断で再確認する。`cargo build` は sandbox で不可のため、grep + 構造読みのみで判定した。判定基準は Round 2 と同じ「外部参照ゼロ」「test 以外で touch されない」「cfg gate で永久に無効」の 3 観点。

---

## Round 2 指摘の解決状況

集計: **fixed (確認): 22 / regressed: 2 / partially-fixed: 1 / unchanged (decision/deferred): 8**

### Round 2 が claim していた修正（再確認）

| ID | Round 2 申告 | Round 3 判定 | 備考 |
|---|---|---|---|
| B-1 `CoreError` | fixed | **fixed (確認)** | `crates/core/src/error.rs` 不在、`lib.rs:9-13` の `mod error` 無し |
| B-2 processor_outputs.rs 8 型 | fixed | **fixed (確認)** | ファイル不在。`SignerInfo` / `C2paAction` / `C2paVerifyOutput` は `c2pa_verify.rs:42, 55, 60, 73` に同居、`lib.rs:15-18` で再エクスポート、`grep` でも touch あり |
| B-4 `OffchainData` | fixed | **fixed (確認)** | `crates/solana/src/extension.rs` 全文に該当型なし |
| B-5 `unreachable_code` warn | fixed | **fixed (確認)** | `crates/tee/Cargo.toml:18` で `default = ["runtime-mock"]`、`main.rs:69-83` の `#[allow(unused_mut, clippy::vec_init_then_push)]` 構造で warn 鎮静 |
| B-6 Gateway 暗号化レスポンス透過 | fixed | **fixed (確認)** | `tee_client.rs:71` が `Result<ProcessOutcome, _>`、`process()` の `if is_octet_stream` 分岐で `Encrypted(Vec<u8>)` 経路が成立 (tee_client.rs:199-219) |
| B-8 RSA 経路 | fixed | **fixed (確認)** | `sign.rs:30-33, 61-65` の `KeyAlgo` / `SigAlgo` に RSA バリアントなし、`Cargo.toml` に `rsa` dep なし |
| B-13 `CryptoError::EcdhError` | fixed | **fixed (確認)** | `crates/crypto/src/error.rs` に該当 variant なし |
| B-14 `AttestationError::Other` / `Expired` | fixed | **fixed (確認)** | `attestation/src/lib.rs:46-56` の `AttestationError` は `ParseFailed` / `SignatureInvalid` / `MissingField` の 3 種に縮小（Round 2 で「4 種」と書かれていたが現コードは 3 種、`CertChainInvalid` も統合済み） |
| B-15 `ExtensionError` 3 variants | fixed | **fixed (確認)** | `crates/solana/src/extension.rs:30-48` は 6 variant（`ParseFailed` / `AttestationInvalid` / `MeasurementMismatch` / `UserDataMismatch` / `TxFailed` / `Base58Failed`） |
| B-22 `digest` / `nonce` | partially-fixed | **partially-fixed (確認)** | `digest` は K1-mf003 で読み出されている、`nonce` は依然書き込み・読出しともゼロ |
| B-23 `sig_algo_val` dead arm | fixed | **fixed (確認)** | `sign.rs` に `sig_algo_val` 関数なし、`SigAlgo::from_oid` (sign.rs:67) で OID から直接判定 |
| B-24 `bs58` dep 未使用 | fixed | **fixed (確認)** | `crates/solana/Cargo.toml` 内ヒットゼロ |
| B-25 crypto の `serde` 直接依存 | fixed | **fixed (確認)** | `crates/crypto/Cargo.toml` に `serde =` 行なし |
| B-27 `Clone for ProcessorError` test impl | fixed | **fixed (確認)** | `crates/core/src/processor.rs:60` で `#[derive(Debug, Clone, thiserror::Error)]`、test 内手書き impl なし |
| B-28 lib.rs outdated doc | fixed | **fixed (確認)** | `crates/tee/src/lib.rs:1-32` は v0.1.2 仕様準拠の説明に置換 |
| **B-31** `Default for ProcessorRegistry` | fixed | **regressed** | `crates/core/src/processor.rs:87` に `#[derive(Default)]` 再追加。`ProcessorRegistry::default()` の呼び出しは全リポジトリで 0 件（`new()` のみ）。Round 2 で削除と申告された後、別タスクが re-add した可能性が高い |
| **N-6** `MockAttestationVerifier` の `Default` | fixed | **regressed** | `#[derive(Default)]` は外れたが、手書きの `impl Default for MockAttestationVerifier { fn default() -> Self { Self::new() } }` が `attestation/src/lib.rs:100-104` に残存。`MockAttestationVerifier::default()` の呼び出しは grep 0 件（5 箇所すべて `::new()`）。fix が片方しかかかっていない |
| **N-8** `MockRuntime` の `Default` | fixed | **regressed** | 手書き `impl Default for MockRuntime` は除去されたが、その代わり `crates/tee/src/runtime/mock.rs:22` で `#[derive(Default)]` が付与された。`MockRuntime::default()` の呼び出しは grep 0 件。N-6 と双子の問題 |
| N-1 `whitelist_program_id()` | fixed | **fixed (確認)** | `crates/solana/src/whitelist.rs` から関数版が削除、`WHITELIST_PROGRAM_ID` const のみ |
| N-2 `spl_account_compression_v2_id()` | fixed | **fixed (確認)** | `crates/solana/src/cnft.rs` から関数版が削除、`SPL_ACCOUNT_COMPRESSION_V2_ID` const のみ |
| N-4 `derive_mpl_core_cpi_signer` | fixed | **fixed (確認)** | `cnft.rs:34` で `pub(crate) fn` に降格 |
| N-5 `ProcessOutcome` の `Clone` | fixed | **fixed (確認)** | `tee_client.rs:23` は `#[derive(Debug)]` のみ |

### Round 2 が decision/deferred と裁定した件

| ID | Round 2 申告 | Round 3 判定 | 備考 |
|---|---|---|---|
| B-3 `whitelist.rs` 全体 | unchanged (decision) | **unchanged** | 17e の SDK 化想定で温存、構造変化なし |
| B-7 cNFT Tree helpers | unchanged (deferred) | **unchanged** | `derive_tree_config` / `merkle_tree_account_size` / `rent_exempt_minimum` / `build_create_tree_tx` (cnft.rs:26, 40, 61, 74) 残存、devnet test のみ |
| B-9 3 重 `MockRuntime` | partially-fixed | **partially-fixed (確認)** | `orchestrator.rs:438-471` に独自 `MockRuntime` 残存。`last_user_data` 観測のため `runtime::mock::MockRuntime` には統合不可能、現状で妥当 |
| B-10 SP1 guest `has_public_key` | partially-fixed (方針変更) | **fixed (確認)** | `programs/title-whitelist/src/lib.rs:413-422` で末尾位置まで読み切り、`data.len() == offset` で trailing garbage を検出。dead から forward-compat slot へ再分類 |
| B-11 `ProcessorRegistry::execute` | unchanged (deferred) | **unchanged** | `processor.rs:124` 残存、`orchestrator.rs:344` の `execute_processors` 経由でのみ呼ばれる |
| B-12 `execute_processors` 1 行ラッパ | unchanged (deferred) | **unchanged** | `orchestrator.rs:338-345` 残存 |
| B-16 `SolanaSigningKey` 未使用 pub API | partially-fixed | **partially-fixed (確認)** | `pubkey_hash` は `main.rs:126` で production 利用、`pubkey_bytes` は内部 (`signing_key.rs:59`)、`from_seed` / `verifying_key` は test のみ。SDK 公開予定で温存判断 |
| B-17 `ResourcePool` pub API 過剰 | unchanged (deferred) | **partially-fixed** | `with_single_limit` (resource_pool.rs:67) は `crates/tee/src/server.rs:415` / `gateway/tests/e2e.rs:74` で test fixture として使われており、convenience-constructor として正当性あり。`can_admit` (74) は `try_admit` 内部 (85) のみ、`acquire` (115) は test のみ、`admission_limit()` (135) は test のみ、`ticket_with_timeouts` (106) は test のみ。**production 呼出ゼロが 4 件残存** |
| B-18 `Ticket` pub API 過剰 | unchanged (deferred) | **unchanged** | `extend_unchecked` / `is_global_timeout_exceeded` / `is_chunk_timeout_exceeded` / `elapsed` / `global_timeout` / `validate_decoded_size` 全部残存、test のみ |
| B-19 `limits.rs` 未使用エクスポート | partially-fixed | **unchanged** | `estimate_decoded_size` / `DecodedSizeExceeded` チェーン dead 温存（Round 2 N-3 で同件 wontfix 確認済み） |
| B-20 `ProxyContentFetcher::with_max_body_bytes` | unchanged | **unchanged** | `proxy_fetcher.rs:76` 周辺 残存、test 専用 |
| B-21 proxy async helpers | unchanged | **unchanged** | cfg 配置妥当 |
| B-26 `#[allow(unused_mut)]` 暗黙 dead | unchanged | **unchanged** | `crates/tee/src/main.rs:70` に残存、構造的問題は同じ |
| B-29 `runtime/mod.rs` single-child mod | unchanged | **unchanged** | `pub mod mock;` 1 行のまま、`vendor/mod.rs` との二重構造維持 |
| B-30 `vendor()` getter 二重定義 | unchanged | **unchanged** | `AttestationVerifier::vendor()` trait method + `VerifiedAttestation::vendor` field、trait method の外部呼出ゼロ |
| B-32 `aead.rs` 2 つの `nonce.try_into()` 防御 | unchanged | **unchanged** | コード現物未変更 |
| N-3 decode-validation dead チェーン | wontfix | **unchanged (確認)** | `estimate_decoded_size` / `validate_decoded_size` / `DecodedSizeExceeded` の 3 段、test のみ |
| N-7 `MissingField` | wontfix | **wontfix (確認)** | `crates/attestation-aws-nitro/src/lib.rs:72` で `PCR0` 取得失敗時に発火、live |

### サマリ

- Round 2 fixed と申告された 22 件のうち **3 件が regressed**（B-31, N-6, N-8）。いずれも `#[derive(Default)]` の付与または手書き `impl Default` の取り残しという同型ミス。fix の cleanup 漏れが集中している
- decision/deferred 群は全件 Round 2 の判断と一致、新たな regression なし
- partially-fixed の B-17 については production 呼出ゼロが 4 メソッド残るが、`with_single_limit` を server.rs/e2e.rs が convenience として使っており Round 2 の deferred 判断は妥当

---

## 新規発見

Round 2 N-1〜N-8 を fix 適用済みとした上で再度コードを舐め、Round 1〜Round 2 で検出していなかった、または Round 2 の fix の副作用で発生した dead を列挙する。

### N3-1 (should-fix). `crates/attestation-aws-nitro/src/lib.rs:41` — `AwsNitroVerifier` の `Default` と `Clone` derive

```rust
#[derive(Debug, Default, Clone)]
pub struct AwsNitroVerifier;

impl AwsNitroVerifier {
    pub fn new() -> Self {
        Self
    }
}
```

`AwsNitroVerifier::default()` および `.clone()` の呼び出しは全リポジトリで 0 件（grep `AwsNitroVerifier::default`、`AwsNitroVerifier::new` の各々確認）。構築箇所はすべて `AwsNitroVerifier::new()`:

- `crates/tee/src/main.rs:65`
- `crates/attestation-aws-nitro/src/lib.rs:93, 109, 134` (test)

unit struct のため `Default` 派生は意味も薄い。`Clone` は内部状態が無いため複製不要。

N-6（`MockAttestationVerifier`）と同型の問題で、attestation 系の 2 つの verifier に対して同じ derive 汚染が並列して存在している。Round 2 のチェックが片方（mock）しか網羅しなかったための取り残し。

修正案: `#[derive(Debug, Default, Clone)]` → `#[derive(Debug)]` に絞る。

### N3-2 (should-fix). `crates/attestation/src/lib.rs:100-104` — `MockAttestationVerifier` 手書き `impl Default` の取り残し（N-6 regression の本体）

Round 2 N-6 は「`#[derive(Default)]` を削除」と申告したが、現コードでは確かに derive は消えている一方、**手書きの `impl Default for MockAttestationVerifier`** が残存している:

```rust
impl Default for MockAttestationVerifier {
    fn default() -> Self {
        Self::new()
    }
}
```

このブロックを通過する `default()` の呼び出しは 0 件。N-6 fix の cleanup が `#[derive]` だけで止まり、手書き impl まで到達しなかったケース。

修正案: 上記 `impl Default` ブロックごと削除。N3-1 と同じ修正単位として扱える。

### N3-3 (should-fix). `crates/tee/src/runtime/mock.rs:22` — `MockRuntime` の `#[derive(Default)]` 再追加（N-8 regression）

Round 2 N-8 は「`impl Default for MockRuntime` を削除」と申告。確かに impl block は消えているが、**`#[derive(Default)]` が新たに付与されている**:

```rust
#[derive(Default)]
pub struct MockRuntime;

impl MockRuntime {
    pub fn new() -> Self {
        Self
    }
}
```

`MockRuntime::default()` の呼び出しは全リポジトリで 0 件（構築箇所はすべて `MockRuntime::new()`、12 箇所確認）。

N-8 fix で `impl Default` を消す際、unit struct への定石として `#[derive(Default)]` を機械的に補ったが、それ自体も呼ばれない。

修正案: `#[derive(Default)]` を外す。

### N3-4 (should-fix). `crates/core/src/processor.rs:87` — `#[derive(Default)] for ProcessorRegistry` の regression（B-31 regression）

Round 2 B-31 は「`Default` impl 無し、`new()` のみ」と申告。現コードでは `#[derive(Default)]` が再付与されている:

```rust
#[derive(Default)]
pub struct ProcessorRegistry {
    processors: Vec<Box<dyn Processor>>,
}

impl ProcessorRegistry {
    pub fn new() -> Self { ... }
    ...
}
```

`ProcessorRegistry::default()` の呼び出しは全リポジトリで 0 件（11 箇所すべて `::new()`）。

N3-1〜N3-4 は **すべて「Default derive / impl が呼ばれない unit struct 系」の同型 dead** で、Round 2 → Round 3 間の cleanup が浅かった結果として4 つも残ってしまった。1 PR で機械的に外せるレベル。

修正案: `#[derive(Default)]` を外す。

### N3-5 (should-fix). `programs/title-whitelist/src/lib.rs:766` — `WhitelistError::EmptyProof` が dead

```rust
#[error_code]
pub enum WhitelistError {
    #[msg("SP1 proof is empty")]
    EmptyProof,
    #[msg("SP1 proof has unexpected length (expected 4 + 256 bytes)")]
    InvalidProofLength,
    ...
}
```

`verify_sp1_groth16` (lib.rs:279-317) は proof の長さチェックを `proof.len() == 4 + 256` で行い、その失敗時には `InvalidProofLength` を投げる。`proof.is_empty()` を別途チェックする経路は存在せず、`EmptyProof` を `require!` / `error!()` する箇所は grep 0 件。

ただし、本ファイルには明示的なコメント

> Anchor numbers error variants from 6000 in declaration order, and those
> codes are part of the program's external ABI: clients ... match on
> the resulting hex codes. **Only append new variants at the end.** Reordering
> or inserting in the middle silently breaks every consumer ...

があり、削除は ABI（Anchor の error code 6000 始まりの enum 値）を全てシフトさせる。一方で `EmptyProof` は variant の先頭にあるため、削除すると後続の全コードがズレ、最も影響が大きい。

修正案: **wontfix を提案**。残置するなら少なくとも次のいずれか:

1. 末尾追加ルールどおり、将来の variant 削除も `EmptyProof` を deprecated コメント付きで残す前提を明文化する（現状の doc コメントだけでは「dead だが消せない」事情がプログラム外からは不可視）
2. 将来 V2 program を出すタイミングで discriminator ごと刷新する際にまとめて掃除

**判定**: dead だが ABI 制約により wontfix が現実解。新規 ABI 設計タスク（K5 系）で扱うべき項目として記録のみ。

### N3-6 (nitpick). `crates/proxy/src/protocol.rs` — `pub const` / `pub fn` の可視性が過剰

`title-proxy` crate は `[[bin]]` 専用（`Cargo.toml:10-12`、`name = "title-proxy", path = "src/main.rs"`）で、ライブラリとして外部から `use title_proxy::protocol::*;` される経路は無い。実際 `crates/tee/src/proxy_fetcher.rs` は `title-proxy` への dependency すら持たず、`CHUNKED_SENTINEL` / `CHUNKED_TRUNCATED` の値を独自に再定義している (`proxy_fetcher.rs:147, 152`)。

```rust
pub const CHUNKED_SENTINEL: u32 = u32::MAX;
pub const CHUNKED_TRUNCATED: u32 = u32::MAX;
pub const MAX_METHOD_BYTES: usize = 16;
pub const MAX_URL_BYTES: usize = 8 * 1024;
pub const MAX_REQUEST_BODY_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_RESPONSE_BYTES: u64 = 100 * 1024 * 1024;

pub async fn read_u32_async<R: ...>(r: &mut R) -> ...
pub async fn read_string_async<R: ...>(...) -> ...
pub async fn read_bytes_async<R: ...>(...) -> ...
pub fn read_u32_sync(...)         // #[cfg(...)] 付き
pub fn read_string_sync(...)
pub fn read_bytes_sync(...)
```

これらはすべて `crates/proxy/src/{main.rs, handler.rs}` 内からしか参照されない。`pub` を `pub(crate)` にすべき。

ただし bin crate の場合 `pub` でも実害（外部から見える surface）は無く、もっと大きな副作用は別途存在する: **TEE 側で同じ定数を再定義しているため、proxy crate と TEE crate のどちらかが値を変更すると wire format が齟齬を起こす**。これは可視性の問題というより重複定義のリスクで、b-dead-code よりは d-architecture か e-reproducibility が扱う領域。

修正案 (本観点として): `pub` → `pub(crate)`。`MAX_REQUEST_BODY_BYTES` 等は外部から見える必要が無いので絞る。重複定義の解消は別観点でフォロー。

### N3-7 (nitpick). `crates/solana/src/whitelist.rs:21` — `KEY_EXPIRY_SECONDS` が test 経由でしか参照されない

```rust
pub const KEY_EXPIRY_SECONDS: i64 = 90 * 24 * 60 * 60;
```

呼び出し箇所は `crates/solana/src/whitelist.rs` 内 test (120, 127, 128, 129, 130, 135, 140) のみ。`is_valid_at` / `is_expired_at` (whitelist.rs:80, 84) はこの定数を直接参照していない（フィールド値 `expires_at` のみ使用）。

ただし本定数は **on-chain プログラム (`programs/title-whitelist/src/lib.rs:27`) のミラー** であり、doc コメントで「Authoritative source is on-chain; this constant is for client-side `is_valid_at` checks and rotates with the program.」と明記されている。クライアント SDK 化想定で温存と整合（Round 2 確認済みの判断と同じ）。

修正案: **wontfix**。doc コメントが既に意図を明示しており、整合性チェック (test) も走っているため dead ではない。Round 2 で `derive_approved_vkeys_pda` 等を温存した判断と同じカテゴリ。

### N3-8 (nitpick). `crates/solana/src/signing_key.rs:50` — `pubkey_bytes()` の `pub` 可視性過剰

`pubkey_bytes()` は production では同ファイル内 `pubkey_hash()` (signing_key.rs:59) からしか呼ばれず、外部利用は test のみ（`signing_key.rs:142, 148, 169, 178`）。SDK の最小 surface としては `pubkey` (Solana Pubkey 返却) / `pubkey_base58` / `pubkey_hash` / `sign` / `sign_transaction` で十分。

修正案: `pub fn pubkey_bytes` → `pub(crate) fn pubkey_bytes`、または `fn pubkey_bytes`（同モジュール内 helper 化）。`from_seed` / `verifying_key` は Round 2 B-16 で「SDK 公開予定」温存判断、`pubkey_bytes` も同じ扱いにするなら `pub` でも整合する。

**判定**: Round 2 B-16 の温存方針との一貫性を取るなら **wontfix**。surface 縮小の判断は SDK 設計段階で再評価。

---

## 確認したが dead ではなかった（Round 3 で新規確認）

- `ApiKeySet::is_empty` (gateway/src/auth.rs:110) — `auth.rs:65` の production middleware で「API key 設定が空ならミドルウェアをスキップ」判定に使われている
- `RateLimiter::prune_idle` (gateway/src/rate_limit.rs:97) — `server::run` 内のタイマーで実呼出（Round 2 確認済み、変更なし）
- `is_tee_available` (gateway/src/state.rs:141) — `endpoints.rs:105, 132, 173` で `/process`、`/health`、`/extension/solana` の処理可否判定に使用
- `AttestationVerifier::vendor()` trait method (B-30) — `attestation/src/lib.rs:64` 定義、外部呼出はまだ 0 件だが trait の object-safety を担保する API surface として残置妥当（仕様 §6.2 で vendor 識別子がプロトコルレベルで意味を持つ）
- `SigStructure::as_bytes` (cose.rs:186) と `SigStructure::new_sign1` (cose.rs:176) — `verify_signature` (cose.rs:86-87) で連鎖呼出、live
- `Cert::sig_algo` / `Cert::signature` / `Cert::tbs_certificate` / `Cert::verify` — `Cert::verify` (cert.rs:77) と `CertChain::verify_chain` (cert.rs:142) でフル使用、live
- `KEM` 各実装の構造体 (`crates/crypto/src/kem/*`) — `key_bundle.rs` 経由で `KeyBundle::generate` 内から実際に生成、live

---

## 全体所感

Round 2 で「fixed」とした 22 件のうち、**3 件 (B-31, N-6, N-8) が `Default` derive / impl 系の同型 regression**。これは Round 2 監査 → fix 反映の cleanup フェーズが「`#[derive(Default)]` か `impl Default` のどちらか片方しかチェックしていない」ことが原因と推測される。N3-1 (`AwsNitroVerifier`) は元々 Round 2 で見逃されていた同型例で、Round 2 のチェックが mock 側だけを舐めた結果と整合する。**4 件まとめて 1 PR で機械的に外せる**。

ABI 凍結に保護されている `WhitelistError::EmptyProof` (N3-5) は dead だが触れず、deprecated 扱いとする運用 doc を 1 行入れるだけで十分。

defer 群（B-3 / B-7 / B-11 / B-12 / B-17 / B-18 / B-19 / B-20 / B-21 / B-26 / B-29 / B-30 / B-32）は 17e README の判断と整合した状態で全件温存されており、Round 2 → Round 3 で新たな悪化なし。

**`Default` 系の cleanup と `pub(crate)` 化さえ通れば、本観点は build-time の `#![deny(dead_code)]` を `crates/solana` / `crates/attestation*` / `crates/core` から段階的に有効化できる地点に到達する**。Round 2 所感の延長としてそのまま継続提案。

監査終了。

---

## 処理ログ

| ID | 判定 | 内容 |
|---|---|---|
| B-1/B-2/B-4/B-5/B-6/B-8/B-13/B-14/B-15/B-22/B-23/B-24/B-25/B-27/B-28 | fixed (確認) | Round 2 認定を Round 3 で再確認、コード現物と整合。 |
| B-3/B-7/B-11/B-12/B-17/B-18/B-19/B-20/B-21/B-26/B-29/B-30/B-32 | wontfix(17e の defer/decision README で「SDK 化想定 / 並列化リファクタ / test 専用境界」として明文化済み、本ラウンドでも判断を維持) | |
| B-9 | partially-fixed (確認) | `orchestrator.rs` 独自 `MockRuntime` は test 内 `last_user_data` 観測のため統合不可、現状妥当。 |
| B-10 | fixed (確認) | `programs/title-whitelist/src/lib.rs:413-422` で末尾位置までパース、trailing garbage 検出ロジックとして live。 |
| B-16 | partially-fixed (確認) | `pubkey_hash` は production 使用、`from_seed` / `verifying_key` は SDK 公開 API として温存判断。 |
| B-31 | **regressed → fix 推奨** | `#[derive(Default)] for ProcessorRegistry` が再付与されている (processor.rs:87)、`default()` 呼出ゼロ。N3-4 として再起票。 |
| N-1/N-2/N-4/N-5 | fixed (確認) | Round 2 申告通りコード反映済み。 |
| N-3/N-7 | wontfix (確認) | `estimate_decoded_size` 系は image-pdq 復活時の resource、`MissingField` は aws-nitro:72 で live。 |
| N-6 | **regressed → fix 推奨** | `#[derive(Default)]` は外れたが手書き `impl Default` が `attestation/src/lib.rs:100-104` に残存。N3-2 として再起票。 |
| N-8 | **regressed → fix 推奨** | 手書き impl は消えたが `#[derive(Default)]` が `runtime/mock.rs:22` に再付与。N3-3 として再起票。 |
| N3-1 | fixed | `AwsNitroVerifier` の `#[derive(Debug, Default, Clone)]` → `#[derive(Debug)]` に。`new()` に `#[allow(clippy::new_without_default)]` を付与し、clippy --fix が再付与する経路を物理的に塞ぐ。 |
| N3-2 | fixed | `MockAttestationVerifier` の手書き `impl Default` を削除。`new()` に `#[allow(clippy::new_without_default)]` を付与。 |
| N3-3 | fixed | `MockRuntime` の `#[derive(Default)]` を削除、`new()` に `#[allow]` 付与。 |
| N3-4 | fixed | `ProcessorRegistry` の `#[derive(Default)]` を削除、`new()` に `#[allow]` 付与。Default 系の cleanup 4 件を一括処理。 |
| N3-5 | wontfix(Anchor error code は ABI、`WhitelistError::EmptyProof` を削除すると後続 variant の hex code が全てシフトしクライアント互換性が壊れる。残置は意図的、doc コメントを追加すれば十分) | |
| N3-6 | fix 推奨 (nitpick) | `crates/proxy/src/protocol.rs` の `pub const` / `pub fn` を `pub(crate)` に絞る（bin crate のため `pub` でも実害は薄いが surface 縮小の方針として）。重複定義の解消は別観点で扱う。 |
| N3-7 | wontfix(`KEY_EXPIRY_SECONDS` は on-chain プログラムのクライアントミラー、doc コメントで意図明示済み。Round 2 の whitelist.rs 温存判断と整合) | |
| N3-8 | wontfix(Round 2 B-16 の SDK 公開 API 温存方針と整合。SDK 設計フェーズで surface 縮小を再評価) | |
