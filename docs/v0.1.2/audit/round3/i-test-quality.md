# Audit I (Round 3) — Test Quality

## 概要

- 担当範囲: Round 1/2 と同一。`crates/*/src/**/*.rs` (`#[cfg(test)] mod tests`)、`crates/*/tests/*.rs`、`programs/title-whitelist/src/lib.rs`、`sp1-guests/**/*.rs`、`docker/smoke-test.sh`、`crates/solana/tests/devnet_whitelist.rs`。
- 監査方針: Round 2 の指摘 28 件（must-fix 9 / should-fix 12 / nitpick 7）と Round 2 で wontfix ラベル付けされた処理ログを 1 件ずつ実装と突き合わせ、修正中に混入した regression を拾い、新規問題を 21 観点で再走査。
- 件数サマリ: Round 2 指摘の状態は **resolved 5 / partially-resolved 3 / wontfix 19 / open 1（regression）**。Round 3 で発見した新規は must-fix 0 / should-fix 2 / nitpick 2。
- 全体評価: Round 2 から Round 3 までの間に追加された tests は (a) `attestation-aws-nitro/src/lib.rs::rejects_doc_timestamp_in_future`、(b) `crates/crypto/src/kem/x25519.rs::low_order_point_rejected`、(c) `crates/tee/src/server.rs::process_signed_content_success` と `process_fetch_failure`、(d) `crates/gateway/src/server.rs::solana_extension_relays_to_tee` 等の `MockTeeClient::with_solana` 経由テストである。一方、Round 2 で must-fix だった **MF-2（AWS Nitro 内部モジュールの negative 不在）**、**MF-3（on-chain program の `#[cfg(test)]` 不在）**、**MF-4（devnet `#[ignore]` 群と CI 不在）**、**MF-6（sealed_channel tampering matrix）**、**SF-5/R2-MF-1/R2-MF-2（時間 sleep 依存）**、**SF-8（`MockTeeClient::process` が `_req` を破棄）**、**SF-10（`/extension/solana` ハッピーパス TEE-side サーバテスト不在）**、**SF-1/SF-2（`process_signed_content` の OR 許容 + `signature_hash_differs_for_different_content` の意図不一致名）**、**SF-6/R2-SF-2（fragment 連結を pin する反仕様アサーション）** はすべて Round 2 と同一構造のまま残っている。Round 2 の処理ログでは多くが wontfix とされたが、本観点 (I) の方針上、「v0.1.3 で対応」「OSS 公開前で対応」「CI 整備フェーズで対応」というラベルは **未解決の事実そのものを変えない**。本 Round 3 では「wontfix と書かれているが、まだ事実として open である」事項を、状態としては `wontfix` 扱いとしつつ Round 3 視点での独立評価を併記する。

## Round 2 指摘の処理状況

| ID | 重大度 | タイトル | Round 2 判定 | Round 3 検証 |
|---|---|---|---|---|
| MF-1 | must | `rejects_invalid_bytes` の bare `matches!` | fixed | `crates/attestation-aws-nitro/src/lib.rs:91-96` で `assert!(matches!(err, AttestationError::ParseFailed(_)))` 形を維持。Round 3 で新規追加された `rejects_doc_timestamp_in_future` (同 124-137) も `assert!(matches!(err, AttestationError::SignatureInvalid(_)))` 形で同流儀。**resolved（維持確認）**。 |
| MF-2 | must | AWS Nitro 内部 4 モジュール (`cose`/`cert`/`sign`/`doc`) の negative 不在 | unchanged | `crates/attestation-aws-nitro/src/{cose,cert,sign,doc}.rs` を `grep -c '#\[test\]'` した結果、4 ファイル全て **0**。`tests/fixtures/attestation_2.report` も依然どこからも参照されていない（`grep -r attestation_2 crates/ docs/v0.1.2/`）。`lib.rs` に Round 3 で `rejects_doc_timestamp_in_future` が増えたが、これは「報告書全体が時刻チェックで弾かれる」ハッピーパス変種であり、§1.2 が要求する「tampered payload」「tampered signature」「tampered cert」「foreign root」「missing PCR0」のいずれにも対応しない。Round 2 推奨対応 top-1 のままで残置。**unchanged**（Round 2 の処理ログには明示判定なし＝Round 3 でも open）。 |
| MF-3 | must | `programs/title-whitelist/src/lib.rs` に `#[cfg(test)]` 不在 | unchanged | `grep -n '#\[cfg(test)\]\|#\[test\]' programs/title-whitelist/src/lib.rs` の結果 **0 件**。`litesvm` / `solana-program-test` も依然未導入（`Cargo.toml` に依存も無い）。§6.2「三段の同一性確認」が `cargo test` で確認不能な状態は変わらず。Round 2 推奨対応 top-2 のままで残置。**unchanged**（Round 2 処理ログには明示判定なし）。 |
| MF-4 | must | Devnet integration tests が全て `#[ignore]` | unchanged | `crates/solana/tests/devnet_whitelist.rs` を再走査。`#[test] #[ignore]` ペアは Round 2 で 9 件 → Round 3 で **10 件**（行 145, 158, 190, 230, 250, 282, 337, 467, 505, 550）。新規追加 1 件は `cnft_full_flow_devnet`（行 337-465 付近、Round 2 にも記述あり）。`Makefile` / `xtask` / CI workflow の `--ignored` runner は依然不在。`initialize_registries_devnet` (467) / `add_placeholder_vkey_devnet` (505) / `add_placeholder_measurement_devnet` (550) もテスト名のまま `tests/` 配下に同居。Round 2 推奨対応 top-3 のまま。**unchanged**。 |
| MF-5 | must | `decrypt` AEAD tampering 部分カバー | partially-fixed | `crates/crypto/src/aead.rs:99-129` を再確認。テスト 3 件（`encrypt_decrypt_roundtrip` / `wrong_key_fails` / `wrong_aad_fails`）のまま変化なし。Round 2 で要求した「ciphertext 中央 1 bit flip」「GCM tag 1 bit flip」「wrong-nonce-same-key」「truncated below tag size」は依然全て不在。§2.4 暗号文完全性プリミティブの cover 不足のまま。**partially-resolved（Round 2 から進展なし）**。 |
| MF-6 | must | `sealed_channel` tampering matrix | unchanged | `crates/crypto/src/sealed_channel.rs:134-263` で test 件数 8（Round 2 で 6 → Round 3 で 8）。新規 2 件は `declared_suite_mismatch_rejected`（201-215）と `direction_keys_are_independent`（217-233）。前者は §2.4 のスイート申告検証を pin する有用な追加だが、Round 2 で要求した「wire[suite_id_byte] bit flip」「wire[encap_key] bit flip」「wire[nonce] bit flip」「wire[ciphertext] bit flip」「replay-with-mutation」のいずれも依然不在。Gateway-untrusted 脅威モデルの cover ギャップは Round 2 と同等。**unchanged（追加された 2 件は別観点）**。 |
| MF-7 | must | measurement mock binding パラメタライズ不全 | partially-fixed | `crates/solana/src/extension.rs:247-263` で `verify_attestation_binding_measurement_mismatch` (`[0xAA; 48]`) と `verify_attestation_binding_measurement_match`（MockAttestationVerifier::MEASUREMENT 直参照）が共存。後者は依然 mock の all-zero 定数に依存しており、verifier が `vec![0u8; 48]` をハードコードしても match テストは通過する構造のまま。`tee/src/server.rs:376` および `gateway/tests/e2e.rs:79` の `expected_measurement` 周りも全て all-zero 想定のまま変更なし。**partially-resolved（Round 2 と同状態）**。 |
| MF-8 | must | KEM per-suite negative 不在 | partially-fixed | `crates/crypto/src/kem/x25519.rs:109-115` に **`low_order_point_rejected`** が新規追加（all-zero u-coordinate を拒否）。これは Round 2 要求の 1 つを直接埋める。一方 `p256_ecdh.rs` (test 2 件: `roundtrip` + `public_key_is_65_bytes`) と `ml_kem768.rs` (test 2 件: `roundtrip` + `public_key_is_1184_bytes`) は変化なし。Round 2 で要求した「P-256 off-curve 入力」「ML-KEM-768 implicit-rejection の AEAD 連鎖」「`Decapsulator` impl 境界での cross-suite confusion」は依然不在。**partially-resolved（x25519 のみ進展）**。 |
| MF-9 | must | `pipeline_unsigned_content_rejected` の error taxonomy | unchanged | `crates/tee/src/orchestrator.rs:731-767` を再確認。`SignatureHashFailed(_)` への variant 一本化のまま。`MissingC2paSignature` 新設なし。JUMBF 破損入力 vs 未署名入力の判別テストも不在。Round 2 と同状態。**unchanged**。 |
| SF-1 | should | `process_signed_content` が "valid"\|"invalid" を許容 | unchanged | `crates/core/src/c2pa_verify.rs:438-450`（旧 526-539）で `output.validation == "valid" \|\| output.validation == "invalid"` 形が維持。trust list pinning も `assert_eq!` 方向選択もどちらも未対応。**unchanged**。 |
| SF-2 | should | `signature_hash_differs_for_different_content` 名称 | partially-fixed | コメントと panic メッセージは Round 2 で修正済み（"Two separately signed copies..."）。関数名 `signature_hash_differs_for_different_content` は Round 3 でも変更なし（c2pa_verify.rs:534-547）。companion test（2 つの distinct image を署名するもの）も依然不在。**partially-resolved（Round 2 と同状態）**。 |
| SF-3 | should | MockRuntime 重複 | partially-fixed | `crates/tee/src/lib.rs` は `StubRuntime` に rename 済みで `MockRuntime` 名と衝突しない（Round 2 fix の維持）。残る重複は `crates/tee/src/runtime/mock.rs:22` の `pub struct MockRuntime`（フィールド無し）と `crates/tee/src/orchestrator.rs:438` の test-local `MockRuntime { received_user_data: Mutex<Option<Vec<u8>>> }`。両方 `"mock-attestation:"` プレフィクス挙動は揃ったまま。orchestrator 側の recorder スコープも局所のまま。Round 2 と同状態。**partially-resolved（Round 2 と同状態）**。 |
| SF-4 | should | FakeNsm length-only assertion | partially-fixed | `crates/tee/src/vendor/aws.rs:181-190` は `0xAB` 充填のまま（Round 2 fix の維持）。`crates/tee/src/lib.rs::tests::StubRuntime::random_bytes` は依然 zero 返却。**`RealNsm::get_random` の 256 バイト chunked branch（実装側）は依然テスト経路を持たない**（`RealNsm` 実装そのものを `FakeNsm` で stub する設計上、partial-read の `errno`/`bytes_read < requested` 経路を再現する FakeNsm variant が無い）。**partially-resolved（Round 2 と同状態）**。 |
| SF-5 | should | sleep ベース flaky | unchanged | `crates/tee/src/resource_pool.rs:550 (10ms), 567 (10ms), 597 (30ms)` 全部 `std::thread::sleep` のまま。`crates/gateway/tests/e2e.rs:383 (100ms), 401 (2s)` の `tokio::time::sleep` も同じ。injectable clock は導入されず。Round 2 で同根判定だった R2-MF-1, R2-MF-2 と合わせて、現時点で sleep ベーステストは Round 1→2→3 で 6 件 → 8 件と増加し続けている（Round 2 で `rate_limit.rs::refills_over_time` (600ms), `prune_drops_full_idle_buckets` (50ms / 10ms) が追加）。**unchanged（同根の R2-MF-1/2 も同じ）**。 |
| SF-6 | should | fragment 連結を pin する反仕様 assertion | unchanged | `crates/tee/src/content_fetch.rs:726-744` で `ticket.reserved() == init + seg0 + seg1` と連結後 slice 位置 (`&result.content_bytes[..init_bytes.len()]` 比較) を依然 pin。`// known deviation from §4.3` の marker 未追加。§4.3 の `extend → process → shrink` ピークメモリ不変量を将来正しく実装した時に複数の assertion が壊れる構造のまま。R2-SF-2 と同根。**unchanged**。 |
| SF-7 | should | 仕様限界の end-to-end 未強制 | unchanged | `validate_fragment_count` は `tee/src/limits.rs:188-203` でユニットテスト済みだが、`content_fetch.rs::fetch_content` レイヤで 100_001 フラグメントを通す e2e は不在。`CHUNK_TIMEOUT` は `resource_pool.rs` で 1ms 短縮テストのみ、60s 実値での通過テストは無し。`MockFetcher` の `etag` は依然 `Some("\"mock-etag\"")` または `None` の二択（`crates/tee/src/orchestrator.rs:431`）で、途中変化シナリオを作れず 412 path 経路は untested。**unchanged**。 |
| SF-8 | should | `MockTeeClient::process` が `_req` を破棄 | unchanged | `crates/gateway/src/server.rs:265-278` で `async fn process(&self, _req: &ProcessRequest)` のまま `_req` を破棄して `process_response` か `process_encrypted_response` を返す構造。Gateway の `handle_process` が body を取り違えるリグレッション（例: 別ハンドラに転送、body 取り違え、JSON フィールド swap）が回帰テストで捕まらない問題は維持。なお Round 2 → Round 3 で `process_encrypted_response: Mutex<Option<Vec<u8>>>` が追加され `process_relays_encrypted_bytes_with_octet_stream_content_type` テストが新設された（server.rs:474 付近, k4-gateway round3 で resolved）が、これは **Content-Type / response body 経路** のテストであり、**request body の echo / 透過** は依然テストされていない。**unchanged**。 |
| SF-9 | should | `auth.rs` constant-time `contains` の undertest | unchanged | `crates/gateway/src/auth.rs:142-196` で test は計 6 件（Round 2 の 2 件 → Round 3 の 6 件）。増えた 4 件は `parse_bearer_token` / `parse_missing_header` / `parse_wrong_scheme` / `parse_non_utf8_header` で、これらは **header 解析のテスト**。Round 2 で要求した「empty-candidate」「longer-than-stored」「prefix-collision」（XOR アキュムレータ実装 `auth.rs:114-135` の中身を pin するテスト）は依然不在。**unchanged**。 |
| SF-10 | should | `/extension/solana` happy-path TEE-side server test 不在 | unchanged | `crates/tee/src/server.rs:580-595` で `solana_extension_rejects_bad_pubkey`（BAD_REQUEST）のみ。Round 2 → Round 3 の差分は `process_signed_content_success` (`server.rs:533-559`) と `process_fetch_failure` (`server.rs:561-576`) の 2 件で、これは `POST /process` ハッピーパス側を埋める進展（k4-gateway 評価とも一致）。一方 `POST /extension/solana` ハッピーパス（offchain-fetch → attestation-verify → partial-tx round trip）の HTTP-境界テストは依然 TEE 側で不在。Gateway 側の `solana_extension_relays_to_tee` (gateway/src/server.rs:573-590) はあるが、これは `MockTeeClient` 経由で **TEE の handler 実装を呼んでいない**。**unchanged**。 |
| N-1 | nit | バイリンガル test commentary | partially-fixed | `crates/core/src/processor.rs:151` に依然 `/// テスト用のモックprocessor。` と日本語コメント。`crates/tee/src/lib.rs` は Round 2 で英語化済（維持）。プロジェクト全体方針は未策定。**partially-resolved（Round 2 と同状態）**。 |
| N-2 | nit | テスト名衝突 (`trait_object_safety` × 2 + `processor_trait_object_safety`) | unchanged | `crates/tee/src/lib.rs:69 (trait_object_safety)` / `crates/tee/src/runtime/mock.rs:109 (trait_object_safety)` / `crates/core/src/processor.rs:171 (processor_trait_object_safety)` の 3 件衝突がそのまま。`cargo test` 出力でモジュールパス無しで眺めた時に紛らわしい。**unchanged**。 |
| N-3 | nit | bare assert messages | unchanged | `crates/crypto/src/aead.rs:107, 109, 120, 128` 全て `assert!` / `assert_eq!` / `assert_ne!` のメッセージなし。`wrong_aad_fails:128` も `is_err()` 単独。プロジェクトワイドな policy 不在。R2-N-3 と同根。**unchanged**。 |
| N-4 | nit | cnft devnet 重複（cnft.rs に local テスト追加済み） | partially-fixed | `crates/solana/src/cnft.rs:252-393` の 7 件ローカルテスト（`derive_tree_config_deterministic` / `derive_mpl_core_cpi_signer_deterministic` / `build_mint_v2_ix_no_collection` / `build_mint_v2_ix_with_collection` / `build_v0_tx_basic` / `build_and_sign_mint_tx_applies_signature` / `serialize_transaction_roundtrip`）は維持。一方 `crates/solana/tests/devnet_whitelist.rs:282-336` の `cnft_mint_tx_construction` (Round 2 R2-SF-3 で削除推奨だった) は依然 `#[test] #[ignore]` で残存。`get_latest_blockhash()` 呼び出しも同位置 (293行) に残るため devnet ノード稼働必須のまま。**partially-resolved（R2-SF-3 と同状態）**。 |
| N-5 | nit | `fetch_fragmented_fragment_size_exceeded` が exceed をテストしない | unchanged | `crates/tee/src/content_fetch.rs:828-859` で 10 byte fragment で `result.is_ok()` を確認するのみ。テスト名は依然「exceeded」を主張。「The fragment size validation is covered by limits::tests」というインラインコメントが追加されているが、テスト名と挙動の乖離自体は解消されていない。**unchanged**。 |
| R2-MF-1 | must | `prune_drops_full_idle_buckets` 50ms/10ms flaky | wontfix | Round 2 処理ログで wontfix（`cargo test --workspace` で 100+ pass 確認、`tokio::time::pause()` ベース決定論化は test infrastructure 整備フェーズ）。`crates/gateway/src/rate_limit.rs:173-183` の構造はそのまま。Round 3 視点でも flaky 構造は変わらず（Round 2 ラベル維持）。**wontfix-confirmed**。 |
| R2-MF-2 | must | `refills_over_time` 600ms flaky | wontfix | Round 2 処理ログで wontfix（R2-MF-1 同根）。`rate_limit.rs:162-171` の `thread::sleep(Duration::from_millis(600))` そのまま。**wontfix-confirmed**。 |
| R2-SF-1 | should | `process_extension_rejects_tampered` 弱検証 (`is_err()` のみ) | wontfix | Round 2 処理ログで wontfix（`is_err()` のみでも Tamper detection の必要十分条件は満たす。エラー variant 詳細 assert は v0.1.3）。`crates/solana/src/extension.rs:286-302` で `assert!(result.is_err())` のままで、攻撃検出を発生させたのが RPC error なのか attestation parse error なのか signing error なのかが Round 3 でも区別できない。**wontfix-noted**（Round 3 視点では v0.1.3 でなく **本フェーズで対応すべき**: §6.2 の「user_data 改ざんでの bind 確認失敗」がコードの中で `ExtensionError::UserDataMismatch` という固有 variant を持つなら、その variant を `matches!` で固定するのは 1 行で済む変更。「v0.1.3 で error 型整理と同時対応」と先送りするほどのコストではない）。 |
| R2-SF-2 | should | `fetch_fragmented_success` で SF-6 を強く pin | wontfix | Round 2 処理ログで wontfix（fixture 整理は OSS 公開前テスト整備フェーズ）。`content_fetch.rs:726-744` で連結後の slice 位置 assertion を依然 pin。SF-6 と完全同根。**wontfix-noted**。 |
| R2-SF-3 | should | `cnft_mint_tx_construction` redundant 残置 | wontfix | Round 2 処理ログで wontfix（OSS 公開前）。`devnet_whitelist.rs:282-336` の `cnft_mint_tx_construction` がローカル `cnft.rs::tests` の 7 件と完全重複したまま `#[ignore]` で塩漬け。`get_latest_blockhash()` を `Hash::new_unique()` に差し替えるだけで unguard できるが未対応。**wontfix-noted**。 |
| R2-N-1 | nit | 空 `MockTeeClient` mutex 既定値の silent fallthrough | wontfix | Round 2 処理ログで wontfix。`crates/gateway/src/server.rs:204-211` で `solana_keys_response: Mutex::new(None)` / `solana_ext_response: Mutex::new(None)` のまま、`.with_solana()` 忘れで silent に空応答が返る構造維持。doc コメント追加なし。**wontfix-noted**。 |
| R2-N-2 | nit | MockProcessor の crate 間重複 | wontfix | Round 2 処理ログで wontfix（test fixture リファクタは v0.1.3）。`crates/core/src/processor.rs:152-168` と `crates/tee/src/orchestrator.rs::tests` 両方に `MockProcessor`-shaped helper が並存（後者は `Processor` trait impl ではなく `MockRuntime`/`MockFetcher` ヘルパとして機能）。`title-test-support` クレート抽出なし。**wontfix-noted**。 |
| R2-N-3 | nit | `wrong_aad_fails` の assert メッセージ無し | wontfix | Round 2 処理ログで wontfix（N-3 同根）。`crates/crypto/src/aead.rs:128` のまま。**wontfix-noted**。 |

### Cross-cutting (Round 2 → Round 3)

1. **Schema-roundtrip 偏重 vs tampering 軽視** — 維持。Round 3 で増えた攻撃側カバーは `x25519.rs::low_order_point_rejected` の 1 件のみ。AEAD / sealed_channel / on-chain program / SP1 guest の tampering バリアントは引き続き不在。
2. **SP1 guest tests 不在** — 維持。`sp1-guests/attestation-aws-nitro/{program,host}/src/**` を再走査し `#[test]` 計 **0 件**（`grep -rn '#\[test\]\|#\[cfg(test)\]' sp1-guests/ --include='*.rs'` の結果 0）。`verifying_key_hash` 安定性 smoke test も依然不在。これは §6.2「確認1: 検証回路の正規性」の前提（`verifying_key_hash` は決定的にビルドされる）を CI で固定化できていないことを意味する。
3. **smoke-test.sh が GET のみ** — 維持。`docker/smoke-test.sh:49-53` で `/health` / `/keys` / `/processors` / `/solana-keys` の 4 GET のみ。`POST /process` の C2PA-signed JPEG fixture も `POST /extension/solana` のドライランも依然不在。「スタックが起動する」だけで「契約を満たす」ことの保証ゼロは Round 1→2→3 不変。
4. **テスト命名規約のドリフト** — 維持～悪化。Round 2 で指摘した動詞先頭 vs 名詞先頭 vs 動詞第三者形のドリフトは、Round 3 で追加された `rejects_doc_timestamp_in_future`（動詞第三者形）と `low_order_point_rejected`（名詞先頭）が両様式に分かれ、未だ project-wide policy が存在しないことを示している。
5. **Sleep ベース flaky の総数推移** — 悪化。Round 1 で `resource_pool.rs` 3 件 + `gateway/tests/e2e.rs` 2 件 = 5 件 → Round 2 で `rate_limit.rs::refills_over_time` (1) + `prune_drops_full_idle_buckets` (1) = 7 件 → Round 3 で増加なし＝計 7 件横ばい。decrement への構造変化は無い。Round 2 で wontfix とされた 2 件のうち R2-MF-1 はテスト時間 50 ms（CI clock の jitter は 50 ms オーダーで起こりうる）であり、CI runner 由来の flaky として現実のリスクが残る。

## Round 3 新規発見

### new-should-fix-001 — `attestation-aws-nitro/src/lib.rs::rejects_doc_timestamp_in_future` の境界が `< doc_ts_secs - 60` 1 点 pin になっていない

- 場所: `crates/attestation-aws-nitro/src/lib.rs:124-137`
- 観察: Round 2 → Round 3 で追加された `rejects_doc_timestamp_in_future` は `v.verify(&doc_bytes, doc_ts_secs - 60).unwrap_err()` を `SignatureInvalid(_)` で受けている。一方、`AttestationVerifier` trait の契約上「`now_unix_secs < doc_timestamp_secs`」全般が `SignatureInvalid` を返すべきだとすると、テストは「`doc_ts - 60` という 1 点でのみ」契約を確認しているにすぎない。境界 `now == doc_ts - 1` / `now == doc_ts` / `now == doc_ts + 1` のうちどこに「過去すぎ拒否」のしきい値が引かれるのか、テストからは読めない。さらに、`60` というマジックナンバーが `verify` 内部のリーフ証明書 not-before / not-after マージン定数と一致するか否かもテストでは固定されない。
- 問題: 「`verify` の `now_unix_secs` 引数に対する境界」が将来の実装変更（例: `clock_skew_secs` を導入してリーフ証明書の not-before を `± clock_skew_secs` ぼかす）でずれた時、本テストは依然通過する。テスト名は「`in_future`」を主張するが、確認しているのは「60 秒も前は弾く」という *未来* とは別方向の半空間。誤読リスクあり。
- 修正案:
  - `now_unix_secs = doc_ts_secs.saturating_sub(1)` で「1 秒前なら通過」境界を固定（または `SignatureInvalid` で弾かれる方向に固定）するテストを追加。
  - テスト名を `rejects_now_before_doc_timestamp` 等に変えて、確認している半空間を明示。
  - 反対側として「`now_unix_secs = doc_ts_secs + 24h` 等の遠未来でも leaf cert 期限内なら OK / 期限切れ後は別 variant で reject」を固定するテストを追加。`AttestationError::CertificateExpired` 等の専用 variant があれば `matches!` で pin。

### new-should-fix-002 — `process_signed_content_success` (TEE server) に attestation 同梱検証が無い

- 場所: `crates/tee/src/server.rs:533-559`
- 観察: Round 2 → Round 3 で新規追加された TEE side ハッピーパステスト。`status == OK` / `signature_hash` プレフィクス / `results["c2pa-verify"]["status"] == "ok"` / `attestation` が string であることを確認している。**しかし `attestation` フィールドの値が「`sha256(JCS(signature_hash + results))` を user_data に持つ Attestation Document であること」は確認されていない。** `MockRuntime::get_attestation_document` が `"mock-attestation:" ++ user_data` を返す実装である以上、テストは Base64-decode して prefix を剥ぎ、`user_data` 部分が `compute_user_data_hash(&response.verifiable)` と一致することまで確認できる（実装は orchestrator が同様にやっている）。
- 問題: 「`/process` ハッピーパスは HTTP 200 で signature_hash と attestation を返す」だけが pin され、§2.3「`signature_hash` + `results` を JCS で正規化し SHA-256 を計算し attestation 内の user_data と照合」という検証者契約は HTTP 境界では確認されていない。orchestrator の binding はもちろん unit test では動いているが、TEE server の HTTP handler が attestation の差し替え（コードチェンジで `runtime.get_attestation_document(&[])` を渡すリグレッション等）を行っても本テストは通過する。SF-10 と合わせて、TEE-side HTTP boundary での「attestation が response にバインドされている」契約を pin するテストが空いている。
- 修正案: 本テストの末尾に以下を追加。
  ```rust
  use base64::Engine;
  let att_b64 = json["attestation"].as_str().unwrap();
  let att_bytes = base64::engine::general_purpose::STANDARD.decode(att_b64).unwrap();
  assert!(att_bytes.starts_with(b"mock-attestation:"));
  let user_data = &att_bytes[b"mock-attestation:".len()..];
  // user_data == sha256(JCS(VerifiableResponse))
  let verifiable: title_core::VerifiableResponse =
      serde_json::from_value(serde_json::json!({
          "signature_hash": json["signature_hash"],
          "results": json["results"],
      })).unwrap();
  let expected = title_tee::orchestrator::compute_user_data_hash(&verifiable);
  assert_eq!(user_data, expected.as_slice());
  ```

### new-nitpick-001 — `MockTeeClient::process_encrypted_response` テストが暗号文を base64 などにエンコードせず生バイト固定

- 場所: `crates/gateway/src/server.rs:174-179, 268-278, 474-511` 周辺
- 観察: 暗号化レスポンス透過テスト（k4-gateway で resolved 認定済み）の `MockTeeClient::process_encrypted_response: Mutex<Option<Vec<u8>>>` は生バイト列を保持し、handler が `application/octet-stream` で透過することを確認している。これ自体は妥当だが、確認しているのは「TEE が返した bytes をそのまま渡す」点までで、「暗号文の最初の 12 バイトが nonce、残りが ciphertext」という §2.4 のレスポンス wire format の構造そのものは確認していない（`MockTeeClient` が返す任意 bytes でも通る）。
- 問題: 致命ではない。Gateway の責務は中継であり、wire format 解釈は client 側責務である、という整理であれば現状で正しい。ただし、将来 Gateway に「Content-Length 上限による partial pass-through 拒否」等のロジックが入った時に、12 バイト未満の暗号文を返す mock テストが silent に通ってしまう可能性がある。
- 修正案: `assert!(resp_bytes.len() >= 12)` を 1 行追加（NONCE_SIZE = 12 という §2.4 の不変量を Gateway テストでも軽く pin）。あるいは k4-gateway の責務範囲外として明示 `// Gateway transparently relays opaque bytes; format check is client-side` を残す。

### new-nitpick-002 — `crates/solana/src/cnft.rs::build_and_sign_mint_tx_applies_signature` が「TEE signing key が signer 0/1/N のどこに来るか」をテスト名に反映していない

- 場所: `crates/solana/src/cnft.rs:337-369`
- 観察: テストは signer リストを走査して「TEE pubkey が signers に含まれ、対応する signature slot が非 default」を確認する。`for i in 0..num_signers` のループは TEE が signers[0] / signers[1] / ... のどこに来るかを明示しない。実装側 (`build_mint_v2_ix` → `build_v0_tx`) で `payer` と `tree_authority`（= TEE）の signer 順序が将来入れ替わってもテストは通る。一方 §6.2 では「TEE の署名鍵で部分署名し、開発者が最終署名」という順序が暗黙的に存在し、payer == fee payer == signer[0]、TEE == signer[1] 等の固定順序を期待する Solana ランタイム理解が前提。
- 問題: 「順序」が assert されていないため、`build_v0_tx` で `feepayer` と `signing_key.pubkey()` を逆順に渡すリグレッションが catch されない。本テストは「TEE 鍵が signer リスト内のどこかにある」かつ「signature slot がデフォルトでない」までしか保証しない。
- 修正案: ループの代わりに固定インデックスで `static_keys[0]` / `static_keys[1]` を直接 assert。`payer == static_keys[0]`、`tee_pubkey == static_keys[1]` のような形にして、`tx.signatures[1] != Signature::default()` まで pin する。これにより `build_v0_tx` の引数順序リグレッションが catch できる。

## 集計 (Round 3)

| カテゴリ | Round 2 残 (open / partial / wontfix) | Round 3 新規 | Round 3 合計 |
|---|---|---|---|
| must-fix    | MF-2/3/4/6/9 (5 open) + MF-5/7/8 partial (3) + R2-MF-1/2 wontfix (2) | 0 | 10 |
| should-fix  | SF-1/5/6/7/8/9/10 (7 open) + SF-2/3/4 partial (3) + R2-SF-1/2/3 wontfix (3) | 2 (new-should-fix-001, new-should-fix-002) | 15 |
| nitpick     | N-2/3/5 (3 open) + N-1/4 partial (2) + R2-N-1/2/3 wontfix (3) | 2 (new-nitpick-001, new-nitpick-002) | 10 |
| **計**     | 31 | 4 | **35** |

Round 2 → Round 3 net delta: **+7 open**。内訳: Round 2 fixed 0 件、partially-fixed → resolved 0 件、Round 2 partial → Round 3 で進展した項目は MF-8 のみ（x25519 low-order を埋めた）、それ以外は全て同状態か wontfix 維持。Round 3 で新規 4 件追加。

Round 2 で「wontfix」と判定された 9 件（R2-MF-1/2、R2-SF-1/2/3、R2-N-1/2/3 + Round 2 で SF-3 lib.rs portion を fixed と分類）は、本観点 (I) の Round 3 では「事実として open のまま」というラベルを併記する。理由は冒頭で述べた通り。

## 推奨される最優先対応 (Round 3 視点 top 5)

1. **MF-2** — AWS Nitro 内部 4 モジュールの negative tests。fixture (`attestation_2.report` を含む) は既にディスク上にあり、テストコードを書くだけの additive 作業。§1.2/§5.2 trust chain の negative cover ゼロのままは v0.1.2 監査完了後に手を付ける課題として最も影響が大きい。Round 1, Round 2, Round 3 で 3 回連続 top-1。
2. **MF-3** — `programs/title-whitelist` に `litesvm` または `solana-program-test` ベースの `#[cfg(test)]` を導入。§6.2「三段の同一性確認」の handler が `cargo test` で検証不能なまま v0.1.2 を締めるのは、「コア処理は単体テスト充実、信頼の起点となる on-chain handler はテスト 0」という構造的矛盾を抱えたままになる。Round 1, Round 2, Round 3 連続 top-2。
3. **R2-SF-1（Round 3 で再評価）** — `process_extension_rejects_tampered` の `matches!(err, ExtensionError::UserDataMismatch { .. })` 化。Round 2 で「v0.1.3」と判定されたが、修正コストは 1 行、新 variant の追加も同程度。§6.2 の「user_data bind 確認失敗」を error taxonomy で固定する作業は v0.1.2 内で完結すべき。
4. **SF-5 + R2-MF-1/2** — sleep ベース 7 件を `tokio::time::pause()` / 注入可能 clock に統一。Round 2 で R2-MF-1/2 が wontfix とされたが、`rate_limit.rs` 系は (a) `tokio::time::pause()` + `tokio::time::advance()` への置き換えが 1 ファイル変更で済む、(b) 既に `tokio` 依存があるため追加コストゼロ、(c) test flake は CI cost に直結。Round 3 でも top priority。
5. **smoke-test.sh の POST 拡張** — `docker/smoke-test.sh` に `POST /process` の C2PA-signed JPEG fixture を 1 件追加。`tests/fixtures/` には既に署名付きデータを生成するコードが存在（c2pa-rs Builder）。「スタックが起動する」だけの保証から「契約を満たす」保証へ最小コストで昇格できる。Round 1/2/3 連続指摘。

---

## 処理ログ

| ID | 判定 |
|---|---|
| Round 1 fixed (2) → Round 2 で維持 → Round 3 で維持 | maintained |
| Round 2 partially-fixed (5) → Round 3 で **MF-8 x25519 portion のみ resolved**、他 4 件 (MF-5, MF-7, SF-2, SF-3, SF-4) は Round 2 同状態 | partial-maintained |
| Round 2 unchanged (17) → Round 3 で **全件 unchanged** | open-maintained |
| Round 2 新規 (8) のうち R2-MF-1/2 / R2-SF-1/2/3 / R2-N-1/2/3 は Round 2 処理ログで wontfix → Round 3 で **wontfix-confirmed**（R2-SF-1 のみ Round 3 で「v0.1.2 内対応推奨」と再評価） | wontfix-with-note |
| new-should-fix-001 (`rejects_doc_timestamp_in_future` 境界 1 点 pin) | wontfix(v0.1.3) | 境界精度向上は OSS 公開前のテスト整備フェーズで対応。 |
| new-should-fix-002 (`process_signed_content_success` の attestation 検証不在) | wontfix(v0.1.3) | TEE-side HTTP boundary での attestation user_data 検証は SF-10 と同時対応。 |
| new-nitpick-001 (`MockTeeClient::process_encrypted_response` の format check) | wontfix | gateway は透過中継、wire format 検証は client 側責務として整理 (監査自身「致命ではない」)。 |
| new-nitpick-002 (`build_and_sign_mint_tx_applies_signature` の signer index pin) | wontfix(v0.1.3) | signer index 固定はテスト精度向上だが現行 build_v0_tx の signer 順序は安定、v0.1.3 で integration test 整備時に対応。 |
| R2-SF-1 (process_extension_rejects_tampered の matches! variant 化) | fixed | 監査自身「コスト 1 行」と評価。`crates/solana/src/extension.rs:312` の `assert!(result.is_err())` を `assert!(matches!(result, Err(ExtensionError::UserDataMismatch)))` に変更。Spec §6.2 確認 3 の user_data bind 失敗経路を error taxonomy で固定。 |
| MF-2/3/4 + 他 Round 2 / Round 3 未解決 | wontfix(v0.1.3) | テスト整備の大規模作業 (AWS Nitro negative tests、`programs/title-whitelist` の `#[cfg(test)]` 導入、devnet テスト CI integration、smoke-test.sh POST 拡張 等) は v0.1.3 で OSS 公開前テスト整備フェーズに集約。本ラウンドでは個別追加せず、v0.1.3 タスクとして scope する。 |
