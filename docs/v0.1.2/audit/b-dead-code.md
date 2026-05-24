# B. 死んでいるコード監査

監査範囲: `crates/`, `programs/`, `sp1-guests/`, `deploy/`, `docker/`
対象外: `legacy/`, `target/`, `keys/`, `Cargo.lock`

**集計: 32 件 (must-fix: 10, should-fix: 15, nitpick: 7)**

仕様書 §0–6 と突き合わせて「実装はあるが仕様にも production 経路にも対応していない」コードを優先的にマークした。`cargo build --workspace` の警告は出発点として使ったが、ほとんどの発見は rustc が pub 越しで気付けない論理的 dead を grep + コードリーディングで突き止めたもの。

---

## must-fix (10)

### B-1. `crates/core/src/error.rs:10` — `CoreError` 全体が dead

`CoreError` 型は宣言・export されているが、コード中で構築・返却している箇所は一つもない（grep で `CoreError` のヒットは定義/モジュール doc/re-export の 3 件のみ）。`title-core` のエラーは実際には `ProcessorError` (processor.rs) が全部担っており、`CoreError` は v0.1.0 → v0.1.2 移行で居残ったゴーストである。

修正案: **削除**。`crates/core/src/error.rs` を削除し、`lib.rs` から `pub mod error;` と `pub use error::CoreError;` を消す。

### B-2. `crates/core/src/processor_outputs.rs:94-231` — 5 つの output 型ファミリーが完全に dead

`ProvenanceGraphOutput`, `GraphNode`, `GraphEdge`, `ImagePdqOutput`, `VideoVpdqOutput`, `FrameHash`, `CertVerifyOutput`, `CertChainEntry` の 8 型は、対応する processor (provenance-graph / image-pdq / video-vpdq / cert-*) が実装されていないため、構造体定義・テスト以外の参照が一切ない。grep で `ProvenanceGraphOutput` 等を引いてもヒットは自分のテストだけ。

仕様書 §3.2 にはこれらの processor が並んでいるが、§3.3 が「processorの追加は TEE バイナリの再ビルドとデプロイで行われる」と明記しており、現行 v0.1.2 の出荷物では c2pa-verify のみ実装している（`crates/tee/src/main.rs:108` で C2paVerifyProcessor だけ register）。型だけ先取りしてあるのは「将来差し込む箱」だが、出力スキーマは v0.1.2 では契約していないので、その箱もまだ存在意義がない。

修正案: **削除**。`C2paVerifyOutput`, `SignerInfo`, `C2paAction` のみ残し、他は削除。実装時に再追加すればよい (Cargo の歴史から復元可能)。

### B-3. `crates/solana/src/whitelist.rs` 全体 (242 行) が dead

`crates/solana/src/whitelist.rs` は `WhitelistEntry`, `WhitelistInstruction`, `derive_whitelist_pda`, `derive_approved_vkeys_pda`, `derive_approved_measurements_pda`, `whitelist_program_id`, `KEY_EXPIRY_SECONDS`, `MAX_MEASUREMENT_LEN` 等を export しているが、外部からの import は皆無 (`grep -rn "title_solana::whitelist\|::whitelist::"` ゼロヒット)。`title-solana` の他のモジュール (`extension`, `signing_key`, `cnft`) も `crate::whitelist` を一切参照していない。

これらは on-chain プログラム (`programs/title-whitelist/src/lib.rs`) と同じ名前を持つクライアント側ミラーだが、TEE 内部ではホワイトリスト判定をしないし (Solana プログラムが担う)、Gateway も判定経路にない (`endpoints.rs` でも whitelist 検証はない)。client-side SDK が必要になった時に作ればよい性質のもので、現行の v0.1.2 出荷バイナリではただの dead weight。

修正案: **削除**。`crates/solana/src/whitelist.rs` ファイル削除 + `lib.rs` から `pub mod whitelist;` を消す。

### B-4. `crates/solana/src/extension.rs:30-35` — `OffchainData` 型が dead

`OffchainData` 構造体は宣言されているが、`extension.rs` 内ですら一度も使われていない (テスト含む)。代わりに `process_extension` は `&ProcessResponse` を直接受け取る (line 182)。serde flatten で `response` を露出させるラッパだが、ラップしている意味がない。

修正案: **削除**。`OffchainData` 構造体ごと消す。

### B-5. `crates/tee/src/main.rs:90` — `unreachable_code` warning (rustc 由来)

`cargo build` の唯一の dead-code 警告。`match runtime_name.as_str()` の `other =>` arm がデフォルト feature 構成 (`runtime-mock` も `vendor-aws` も無し) では唯一マッチし、必ず `return Err(...)` する。すると line 90 以降が全部到達不能だが、人間にとってこの状態は「production ビルド時に main 関数のかなりを誤って削っている」のと等価で、CI が「警告ゼロ」を要求するならビルドが落ちる。

仕様書 §5.2 は「TEEは vendor-aws か mock のどちらかを必ず搭載する」と読めるため、Cargo.toml の `default = []` 自体が「設定ミスを誘発する」設計で、開発者が `--features runtime-mock` を忘れて `cargo build` するとこの警告に出会う。

修正案: **書き直し**。次のいずれか:
1. `[features] default = ["runtime-mock"]` にして開発者の素手 build を mock に倒す (Cargo.toml の current `default = []` のコメントは「production 事故防止」を理由に挙げているが、本番ビルドは Dockerfile で明示 features 指定するので、default が mock でも危険はない)
2. main.rs を `cfg_if!` で書き直し、デフォルト feature 無し時はそもそも compile error にする (今は warn + 実行時 error)。

### B-6. `crates/gateway/src/endpoints.rs:90-106` — `handle_process` は暗号化レスポンスを通せない

`pub async fn handle_process(...) -> Result<Json<ProcessResponse>, GatewayError>` の戻り型が JSON 固定。一方、TEE 側 (`crates/tee/src/server.rs:148-159`) は plaintext なら JSON, 暗号化リクエストなら `application/octet-stream` で返す。Gateway はその octet-stream を JSON としてパースしようとして必ず失敗する。`TeeClient::process` も `Result<ProcessResponse, _>` シグネチャ (`crates/gateway/src/tee_client.rs:58`) なので、暗号化レスポンスは Gateway 経由では受け取れない。

仕様書 §2.4 は「暗号化モードでも Gateway はリクエスト/レスポンスを中継する」とあり、§1.7 では「Gatewayはリクエスト内容を改変する能力を持つ」と書かれているが、現実装ではそもそも中継できない。これは Gateway 経由の暗号化フローが**機能的に dead**であることを意味する。

修正案: **書き直し**。`TeeClient::process` を `Result<TeeResponse, _>` (新 enum: `Plaintext(ProcessResponse) | Encrypted(Vec<u8>)`) にし、`handle_process` で Content-Type に応じて分岐する。あるいは、暗号化モードを v0.1.2 の対応範囲から外して仕様書から削る (現状の Gateway 経由 e2e テストでは plaintext しか動かない)。

### B-7. `crates/solana/src/cnft.rs:25-115` — Bubblegum Tree 作成 helpers が production 経路で dead

`derive_tree_config` (line 25), `spl_account_compression_v2_id` (line 37), `merkle_tree_account_size` (line 43), `rent_exempt_minimum` (line 64), `build_create_tree_tx` (line 77) は production コード (`extension.rs` の `process_extension`, `crates/tee` 配下) からは呼ばれない。使うのは `crates/solana/tests/devnet_whitelist.rs` のみ。

仕様書 §6.2 末尾は「cNFTの発行に必要なMerkle Treeの作成と管理は、開発者（プロトコルのユーザー）が行う。Treeの構成は信頼の判定に影響しない。」と明言。TEE/Gateway はそもそも Tree を作る責任を持たない。

修正案: **削除**。これらは devnet テスト helper であって lib コードではない。`crates/solana/tests/devnet_whitelist.rs` か `tests/common/mod.rs` に移動。あるいは、誰のためのライブラリかを明示し doc コメントで「クライアント SDK 用 ヘルパー」と言い切る。

### B-8. `crates/attestation-aws-nitro/src/sign.rs:186-201` + `Cargo.toml:30` — RSA verification 経路が dead

`SigAlgo::RsaSHA256` (line 83), `SigAlgo::RsaSSAPSS` (line 82), `KeyAlgo::RSA` (line 46), およびそれらに対応する `verify_signature` の 2 ブランチ (line 186-201) は、AWS Nitro の証明書チェーンでは出現しない (Nitro チェーンは root から leaf まで全部 ECDSA P-384)。`Cargo.toml` の依存 `rsa = "0.9"` も実質ここでしか使われない。

`Cargo.toml:30` のコメントは "for non-Nitro cert chains that may appear" と書いているが、`title-attestation-aws-nitro` は名前通り AWS Nitro 専用クレートで、非 Nitro チェーンが「現れる」想定は仕様にもコードにもない。

修正案: **削除**。`KeyAlgo::RSA`, `SigAlgo::RsaSSAPSS`, `SigAlgo::RsaSHA256` バリアントとそれぞれの match arm を削除し、`OID_KEY_ALGO_PKCS1_V1_5`, `OID_SIG_ALGO_RSASSA_PSS`, `OID_SIG_ALGO_RSA_SHA256` も `constants.rs` から削除し、`rsa` dependency を `Cargo.toml` から外す。バイナリサイズと依存攻撃面が縮む。

### B-9. `crates/tee/src/lib.rs:99-143` — 重複した `MockRuntime` 実装

`crates/tee/src/lib.rs:104` で `#[cfg(test)] mod tests` 内に `struct MockRuntime;` を定義し `TeeRuntime` を impl している。同じ機能の本物 `MockRuntime` は `crates/tee/src/runtime/mock.rs` にあって、test code でも `crate::runtime::mock::MockRuntime` を import すれば足りる。

実際 `crates/tee/src/server.rs:277` は `use crate::runtime::mock::MockRuntime;` を使っているし、`crates/tee/src/orchestrator.rs:421` も独自にもう一つ `MockRuntime` を定義している (これも `runtime::mock::MockRuntime` で代替可能)。

合計 3 つの MockRuntime 実装がプロセス内で共存していて、仕様書 §5.2 が定義しているのは 1 つだけ。

修正案: **削除/統一**。lib.rs の `MockRuntime` (line 99-143 の tests mod 全体) を削除し、必要なテストは `runtime::mock::MockRuntime` を使う形に書き換え。orchestrator.rs:421 も同様に統一。

### B-10. `sp1-guests/attestation-aws-nitro/program/src/main.rs:70-75` — `has_public_key` 経路が on-chain では dead

SP1 guest は Attestation Document の `public_key` フィールドを処理し、`has_public_key: u8` と `public_key_hash: [u8; 32]` を public values に commit する (line 70-75)。一方、`programs/title-whitelist/src/lib.rs:327` の `parse_public_values` は `has_user_data` までしか読まず、`has_public_key` 以降は無視する (関数末尾の `Ok(ParsedPublicValues { measurement, has_user_data, user_data_hash })` で終わり)。

加えて、`crates/tee/src/vendor/aws.rs:87` の NSM Attestation request は `public_key: None` 固定なので、AWS Nitro から返ってくる Document の `doc.public_key` は常に `None`。つまり guest 側でも `has_public_key=0` 固定、何も hash しない。

仕様書 §6.2 の Attestation document 説明は `public_key` をオプション情報として持ち出すが、Title Protocol の信頼チェーンは「user_data = SHA-256(Solana公開鍵)」しか使っていない。

修正案: **削除**。guest からは `has_public_key` / `public_key_hash` を commit する 2 ブロックを削除。`parse_public_values` の doc コメント (line 325-326) からも該当行を削除。SP1 guest の vkey_hash も再生成し、whitelist の `APPROVED_VKEYS` 集合を更新する必要があるが、これは v0.1.2 が production 稼働する前のクリーニング window でやるべき作業。

---

## should-fix (15)

### B-11. `crates/core/src/processor.rs:122-143` — `ProcessorRegistry::execute` は 1 経路でしか使われない

`execute` は `crates/tee/src/orchestrator.rs:327` の `execute_processors` (これも 1 行ラッパ; B-12 参照) からしか呼ばれない (テスト除く)。仕様書 §3.1 は「processor は並列実行」と書いてあるが、`execute` の中身は `for id in processor_ids` で逐次実行。pub のままだが意味のある外部利用者がいない上、並列化のときに API ごと変える可能性が高い。

修正案: **そのまま削除しないなら `pub(crate)` に下げる**。orchestrator が `registry.get(id)` + `process()` を直接呼ぶ形にすれば `execute` 自体不要。

### B-12. `crates/tee/src/orchestrator.rs:321-328` — `execute_processors` は 1 行ラッパ

```rust
fn execute_processors(
    registry: &ProcessorRegistry,
    processor_ids: &[String],
    content: &[u8],
    content_type: &str,
) -> HashMap<String, ProcessorOutput> {
    registry.execute(processor_ids, content, content_type)
}
```

呼び出し側でも処理は完結する。`process_request` line 218 で `registry.execute(...)` を直書きすれば充分。

修正案: **削除**。

### B-13. `crates/crypto/src/error.rs:11` — `CryptoError::EcdhError` 未使用

`EcdhError` バリアントはどこからも構築されない (`grep -rn "CryptoError::EcdhError"` ゼロヒット; KEM 失敗時はすべて `InvalidKeyLength` や `InvalidWireFormat` で返している)。

修正案: **削除**。

### B-14. `crates/attestation/src/lib.rs:55,57,63-65` — `AttestationError::Other` / `Expired` 未使用

`AttestationError::Other(_)` と `AttestationError::Expired(_)` バリアントは production・テスト・ベンダー実装のどこでも構築されない。AWS Nitro 実装 (`crates/attestation-aws-nitro/src/lib.rs`) は `ParseFailed` / `SignatureInvalid` / `MissingField` の 3 つしか使わない。

修正案: **削除**。新ベンダー実装が必要になった時に追加すればよい。

### B-15. `crates/solana/src/extension.rs:39-66` — 3 つの `ExtensionError` バリアントが未使用

- `FetchFailed` (line 41): どこからも構築されない。仕様書では「TEE が URL から fetch する」だが、`process_extension` は既に fetch 済みの `ProcessResponse` を引数で受け取る (line 182)。fetch は `crates/tee/src/server.rs:213-220` で別エラーパス (`StatusCode::BAD_GATEWAY` を直接返す)。
- `KeyNotWhitelisted` (line 59): どこからも構築されない。仕様書 §6.2 で TEE は whitelist 判定しないので、これは意図通り。
- `Verifier(#[from] AttestationError)` (line 50): `?` で flow するための from 実装だが、`verify_attestation_binding` は `AttestationError` を `?` ではなく明示的に `AttestationInvalid(format!())` で wrap しており (line 141-143)、from impl は実は経由されない。

修正案: **削除**。3 バリアント全部消す。

### B-16. `crates/solana/src/signing_key.rs:28-31, 45-47, 50-52, 57-61` — production 未使用な pub API

- `SolanaSigningKey::from_seed(seed: &[u8; 32])` (line 28): "for testing only" と doc に書いてあるのに pub で外に出している。
- `SolanaSigningKey::verifying_key()` (line 45): production 経路では呼ばれない (pubkey() / pubkey_base58() で十分)。
- `SolanaSigningKey::pubkey_bytes()` (line 50): 同上。
- `SolanaSigningKey::pubkey_hash()` (line 57): 仕様書 §6.2「user_data = SHA-256(Solana公開鍵)」を計算する関数。だが production では `extension::process_extension` ↔ TEE 側で attestation の user_data を直接照合 (`extension.rs:150` の `user_data != expected_hash`) しており、`pubkey_hash` は呼ばれない。テストでのみ参照。

修正案: **`pub(crate)` に下げる** か削除。`from_seed` は本当に test-only なら `#[cfg(test)]` を付けるべき。

### B-17. `crates/tee/src/resource_pool.rs:134-141, 86-88, 93-95, 125-131, 154-156` — `ResourcePool` の pub API 過剰

production からの実呼び出しがあるのは `new`, `try_admit`, `ticket`, `total_used`, `total_limit` の 5 メソッドだけ。残り全部 test 専用:

- `with_single_limit` (line 86): テストヘルパー。
- `can_admit` (line 93): production では `try_admit` の中だけで使う private 経路。
- `ticket_with_timeouts` (line 125): どこからも production で呼ばれない。
- `acquire` (line 134): one-shot helper。テスト以外で使われない。
- `admission_limit()` getter (line 154): どこからも使われない。

修正案: **`with_single_limit`, `acquire`, `ticket_with_timeouts` を `#[cfg(test)]` で囲うか `pub(crate)` に下げる。`can_admit` と `admission_limit()` は削除。**

### B-18. `crates/tee/src/resource_pool.rs:264-269, 320-322, 325-327, 330-332, 335-337, 347-356` — `Ticket` の pub API も過剰

- `extend_unchecked` (line 264): test だけが使う。doc も「For internal use」と書いているのに pub。
- `is_global_timeout_exceeded` (line 320): 未使用。`extend` 内部で同じ判定を行うので不要。
- `is_chunk_timeout_exceeded` (line 325): 同上。
- `elapsed()` (line 330): 未使用。
- `global_timeout()` (line 335): 未使用。
- `validate_decoded_size` (line 347): 仕様書 §4.4「デコード時のメモリ保護」のための API だが、現状デコードを伴う processor (image-pdq 等) が未実装 (B-2) なので呼ばれない。

修正案: **`extend_unchecked` を `pub(crate)`+test 用と明示。残りは削除**。decode validation は image-pdq 実装時に書き戻せばよい。

### B-19. `crates/tee/src/limits.rs:30, 26, 122-125, 164-167` — limits の未使用エクスポート

- `MAX_PROVENANCE_GRAPH_SIZE` (line 30): provenance-graph processor が未実装 (B-2) なので呼ばれない。
- `DEFAULT_TOTAL_LIMIT_FRACTION` (line 26): `main.rs` での total_limit 計算は固定値 `512 * 1024 * 1024` (line 119) で、この定数は経由しない。
- `estimate_decoded_size` (line 122): decode validation 経路が dead (B-18) なので呼ばれない。
- `LimitsError::GlobalTimeoutExceeded` (line 164): どこからも構築されない (timeout チェックは `TicketError::GlobalTimeout` の方を使う)。

修正案: **削除**。

### B-20. `crates/tee/src/proxy_fetcher.rs:76-80, 82-87` — `ProxyContentFetcher` の duplicate constructor

`new(endpoint)` と `with_max_body_bytes(endpoint, max_body_bytes)` の二択。後者は production で呼ばれず (テストだけ)、`DEFAULT_MAX_BODY_BYTES` も外部利用なし。

修正案: **`with_max_body_bytes` と `DEFAULT_MAX_BODY_BYTES` を `pub(crate)` または `#[cfg(test)]` に下げる**。

### B-21. `crates/proxy/src/protocol.rs:32-57` — async helpers の半分が dev/test だけ

`read_u32_async` / `read_string_async` / `read_bytes_async` は vsock 経路 (production) では sync 版 (`read_u32_sync` 等) が使われ、async 版は TCP 経路 (dev/test) でのみ呼ばれる。`#[cfg(not(all(target_os = "linux", feature = "vendor-aws")))]` の `main.rs` フロー専用。

production リリース時にはこちらが dead だが、CI/dev のためには残す必要がある。

修正案: **そのまま (cfg は適切)、ただし doc コメントに「dev/test only」と明記**。

### B-22. `crates/attestation-aws-nitro/src/doc.rs:107, 113` — `AttestationDocument` の `digest` / `nonce` フィールドが読まれない

`AttestationDocument::digest: String` と `AttestationDocument::nonce: Option<ByteBuf>` は serde で deserialize されるが、その後 production 経路で読まれない (`AwsNitroVerifier::verify` も SP1 guest も touch しない)。仕様書には Attestation Document の `nonce` フィールドへの言及があるが、Title Protocol は nonce を要求しない設計。

修正案: **フィールドを残すか、`#[serde(default)]` のままで `#[allow(dead_code)]` を付けてコメントで「CBOR forward-compat のため deserialize 経路のみ存続」と書く**。完全削除でもよい (CBOR は欠落 field を許容するので動作は変わらない)。

### B-23. `crates/attestation-aws-nitro/src/cose.rs:25-31` — `sig_algo_val` の dead arm

`sig_algo_val` の `match alg` で `SigAlgo::EcdsaSHA256` を `-7` にマップしているが、AWS Nitro の COSE_Sign1 protected header は必ず `-35` (ES384) を使う。Nitro 文脈で `EcdsaSHA256` は出現しない。

`sig_algo_val(SigAlgo::EcdsaSHA256) = -7` は CBOR mapping としては正しいが、production で呼ばれることはない。

修正案: B-8 の RSA 除去と合わせて、`SigAlgo` 全体を `EcdsaSHA384` のみに削減。

### B-24. `crates/solana/Cargo.toml:22` — `bs58` 依存が未使用

`bs58 = "0.5"` を declare しているが `crates/solana/src` 内で `bs58::` や `use bs58` の参照ゼロ。Base58 変換は `solana_sdk::pubkey::Pubkey::from_str` / `to_string` 経由で行われており、bs58 直接利用はない。

修正案: **`bs58` 依存を Cargo.toml から削除**。

### B-25. `crates/crypto/Cargo.toml:19` — `serde` 直接依存が未使用

`crates/crypto/src` 配下で `serde::` や `use serde\b` や derive Serialize/Deserialize のヒットゼロ。`serde_json` のみ使う。`serde` 自体は `serde_json` の transitive dep として入っているので問題ないが、Cargo.toml の直接依存は嘘になっている。

修正案: **`crates/crypto/Cargo.toml` の `serde = { workspace = true }` 行を削除**。

---

## nitpick (7)

### B-26. `crates/tee/src/main.rs:70-71` — `#[allow(unused_mut)]` 暗黙 dead

`let mut supported: Vec<&str> = Vec::new();` に `#[allow(unused_mut)]` が付いているが、これは `push` する arm がすべて feature gated なため。feature 構成によっては `mut` が真に不要になる、という rustc の「不要 mut」検知を抑止している。論理的には正しい防御だが、`#[allow]` を付けてまで形を維持する価値は薄い。

修正案: **`Vec::new()` の代わりに `["mock", "nitro"]` のような feature-gated literal 配列で構築する形に書き換え、`#[allow]` を消す**。

### B-27. `crates/core/src/processor.rs:176-189` — テスト専用 `Clone for ProcessorError` impl

テストの中だけで使う `MockProcessor` のために `impl Clone for ProcessorError` を実装している。本物の `ProcessorError` を Clone する必要は production にない。

修正案: `ProcessorError` 自体に `#[derive(Clone)]` を付けてしまうか、`MockProcessor` 側で `Box<dyn Fn() -> Result<...>>` のような生成器パターンに切り替える。今のままでも害はないが、テストファイル内で他の型の trait impl を書くのはコードの居場所として不自然。

### B-28. `crates/tee/src/lib.rs:14-21` — outdated doc コメント

`lib.rs:14-21` の docコメントが「`vendor-aws` — AWS Nitro Enclaves実装」「（デフォルト）— トレイト定義のみ、ベンダー実装なし」と書いており、その下に「legacy/v0.1.0/crates/tee/src/runtime/ — 前バージョンのTeeRuntime実装」と過去言及。

「過去はこうだった」は git log で済む情報で、初見の読み手にはノイズ。仕様書 §0–6 にも v0.1.0 ↔ v0.1.2 の移行差分は出てこない。

修正案: 14-21 行 (Legacy参照ブロック) を削除。

### B-29. `crates/tee/src/runtime/mod.rs:11` — single-child mod の `pub mod mock;`

`runtime/mod.rs` は実質 `pub mod mock;` 一行だけ。`mock` 直下 1 ファイルだけのために `runtime/` ディレクトリを切る意味が薄い。

修正案: `runtime/mock.rs` → `runtime_mock.rs` にフラット化、または `mock_runtime` モジュールとして `tee/src` 直下に置く。今のレイアウトは将来 `runtime/nitro.rs` 等を追加する想定だろうが、現在 `vendor/aws.rs` が別の場所に出てしまっている (Nitro は `vendor/` 下)。`runtime/` と `vendor/` の二重立てが整理されていない。

### B-30. `crates/attestation/src/lib.rs:71-87` — `vendor()` getter が両方の場所に

`AttestationVerifier::vendor() -> &'static str` (trait method) と `VerifiedAttestation::vendor: &'static str` (struct field) が同じ情報を別の場所で持つ。trait の `vendor()` は実装が `Self::VENDOR` 定数を返すだけで、検証結果の `verified.vendor` と必ず一致する (test `vendor_tag_consistent` で確認)。

修正案: trait method を消す (struct field だけにする) か、struct field を消して必要時に `verifier.vendor()` で取り直す。

### B-31. `crates/core/src/processor.rs:146-150` — `Default for ProcessorRegistry`

`ProcessorRegistry::new()` が `Self { processors: Vec::new() }` を返すだけなのに、わざわざ `Default` impl も付けている。`Default` は使われない (production の `main.rs:107` も `ProcessorRegistry::new()` で構築)。

修正案: `Default` impl を削除。

### B-32. `crates/crypto/src/aead.rs` — 2 つの `nonce.try_into()` 防御

`encrypt`/`decrypt` 共に、(1) `if nonce.len() != NONCE_SIZE` で先に長さチェック、(2) その後 `nonce.try_into().map_err(...)` で `&[u8; 12]` 変換。(1) が通った後に (2) で失敗するパスは原理上ない (Rust の slice → fixed-array conversion は長さでのみ失敗する)。

修正案: (1) のチェック後は `unsafe { &*(nonce.as_ptr() as *const [u8; 12]) }` か `nonce.first_chunk::<12>().unwrap()` 等に置き換えるか、unreachable! にする。コードリーディング時に「ここで失敗するケースあるのか?」と読み返す手間を減らす。

---

## 補遺: 確認したが dead ではなかったもの

念のため疑って読んだが、production 経路で生きていたコード:

- `KeyBundle::public_key_bytes(suite)` — tests でのみ呼ばれているように見えるが、orchestrator の暗号化テストで実際に invoke される (encryption mode のテストはあるので、関数自体は仕様 §2.4 を実装する義務がある)。
- `FetchError::EtagMismatch` / `NoFragments` — production の `content_fetch.rs` 内部で実際に構築・返却される。
- `Default for HttpContentFetcher` — production からは直接呼ばれないが、`#[derive(Default)]` が普及した Rust の慣習として置いておく価値あり。

---

監査終了。
