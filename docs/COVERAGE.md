# 実装カバレッジレポート

本文書は、技術仕様書（SPECS_JA.md ver.9）の各セクションに対する現在のコードベースの実装状況を整理したものである。

凡例:
- **実装済み**: ロジックが動作する状態
- **型のみ**: データ構造・インターフェースの定義のみ（ロジックなし）
- **スタブ**: 関数シグネチャは存在するが `todo!()` / `throw new Error("Not implemented")`
- **未着手**: 対応するコードが存在しない

---

## 全体サマリー

| カテゴリ | 実装済み | 型のみ | スタブ | 未着手 |
|---------|---------|--------|-------|--------|
| 暗号プリミティブ (crates/crypto) | 8/8 | — | — | — |
| データ型 (crates/types) | — | 全型 | — | — |
| C2PA検証 (crates/core) | 3/3 | — | — | — |
| WASMホスト (crates/wasm-host) | **2/2** — execute_inner(wasmtime), ホスト関数4種, テスト5件 | — | — | — |
| TEEサーバー (crates/tee) | MockRuntime+NitroRuntime完了, /verify(Core+Extension)・/sign・/create-tree・proxy_client・solana_tx・gateway_auth・security(DoS対策)実装済み | — | — | — |
| Gateway (crates/gateway) | **実装済み** — 5エンドポイント+Gateway認証+S3署名付きURL+sign-and-mint | — | — | — |
| Proxy (crates/proxy) | **実装済み** | — | — | — |
| Solanaプログラム (programs/title-config) | 4/4（Devnetデプロイ済み: `C2HryYkBKeoc4KE2RJ6au1oXc1jtKeKw3zrknQ455JQN`） | — | — | — |
| WASMモジュール (wasm/*) | **4/4** — phash-v1, hardware-google, c2pa-training-v1, c2pa-license-v1 | — | — | — |
| TS SDK (sdk/ts) | **実装済み** — crypto(E2EE), TitleClient, register(11ステップ), StorageProvider, テスト10件 | 全型 | — | — |
| インデクサ (indexer) | **実装済み** — DasClient(Helius DAS)、Webhook(MINT/BURN/TRANSFER)、Poller、DB CRUD、テスト6件 | 全型 | — | — |
| インフラ (Docker/CI) | 全ファイル | — | — | — |

---

## セクション別カバレッジ

### §0 前提

仕様書のみの内容（用語定義・設計原則）であり、実装対象なし。

---

### §1 プロトコルモデル

#### §1.1 登録モデル（Register）

| 仕様の要素 | 対応コード | 状態 |
|-----------|-----------|------|
| E2EE暗号化（ECDH→HKDF→AES-GCM） | `crates/crypto/src/lib.rs` | **実装済み** |
| クライアント側の暗号化フロー | `sdk/ts/src/crypto.ts` | **実装済み** — X25519+HKDF+AES-GCM、テスト10件 |
| Phase 1: /verify（TEE内部処理） | `crates/tee/src/endpoints/verify.rs` | **実装済み** — Core処理7ステップ+Extension WASM実行（process_extension）実装 |
| Phase 2: /sign（TEE内部処理） | `crates/tee/src/endpoints/sign.rs` | **実装済み** — signed_jsonフェッチ(1MB制限)、tee_signature検証、Bubblegum V2 MintV2構築（MPL-Coreコレクション対応）、TEE部分署名 |
| signed_json生成（Core） | `crates/tee/src/endpoints/verify.rs` の `process_core()` | **実装済み** — Core signed_json構築+TEE署名 |
| オフチェーンアップロード | `sdk/ts/src/storage.ts` | **実装済み** — ArweaveStorage(注入式) + HttpStorage(開発用) |
| クライアントによるTx署名・ブロードキャスト | `sdk/ts/src/register.ts` | **実装済み** — 11ステップ全実装 |

#### §1.2 検証モデル（Resolve）

| 仕様の要素 | 対応コード | 状態 |
|-----------|-----------|------|
| Global Config参照 | `programs/title-config/src/lib.rs` | **実装済み** — PDA定義・更新命令あり |
| コレクション所属確認 | — | **対象外** — インデクサ側の責務 |
| オフチェーンデータ取得・署名検証 | — | **対象外** — インデクサ側の責務 |
| 来歴グラフの解決 | — | **対象外** — インデクサ側の責務 |

---

### §2 Core（来歴グラフ）

#### §2.1 コンテンツの識別子

| 仕様の要素 | 対応コード | 状態 |
|-----------|-----------|------|
| content_hash = SHA-256(Active Manifest署名) | `crates/crypto/src/lib.rs` の `content_hash_from_manifest_signature()` | **実装済み** |
| C2PA署名チェーン検証 | `crates/core/src/lib.rs` の `verify_c2pa()` | **実装済み** |
| content_hash抽出 | `crates/core/src/lib.rs` の `extract_content_hash()` | **実装済み** |

#### §2.2 来歴グラフの導出

| 仕様の要素 | 対応コード | 状態 |
|-----------|-----------|------|
| 素材情報の再帰的抽出 | `crates/core/src/lib.rs` の `build_provenance_graph()` | **実装済み** |
| DAG構造の構築 | `crates/types/src/lib.rs` の `GraphNode`, `GraphLink` + `crates/core/src/lib.rs` | **実装済み** |

#### §2.3 グラフを伴う登録と解決

| 仕様の要素 | 対応コード | 状態 |
|-----------|-----------|------|
| 登録時のグラフ構築→signed_json封入 | `crates/tee/src/endpoints/verify.rs` の `process_core()` | **実装済み** |
| 遅延解決（Lazy Resolution） | — | **対象外** — インデクサ側の責務 |

#### §2.4 重複の解決

| 仕様の要素 | 対応コード | 状態 |
|-----------|-----------|------|
| TSAタイムスタンプ/Solana block time比較 | — | **未着手** |
| 先行作成者の優先ロジック | — | **未着手** |

---

### §3 Extension（属性の付与）

#### §3.1–3.2 WASMモジュールによる属性導出

| 仕様の要素 | 対応コード | 状態 |
|-----------|-----------|------|
| WASMホスト実行エンジン（wasmtime直接使用） | `crates/wasm-host/src/lib.rs` の `execute_inner()` | **実装済み** — wasmtime Engine/Store/Linker、テスト5件 |
| Fuel/Memory制限 | `crates/wasm-host/src/lib.rs` の `WasmRunner` | **実装済み** — `consume_fuel(true)` + `StoreLimitsBuilder` |
| ホスト関数: `read_content_chunk` | `crates/wasm-host/src/lib.rs` | **実装済み** — チャンク読み取り+WASMメモリコピー |
| ホスト関数: `hash_content` | `crates/wasm-host/src/lib.rs` | **実装済み** — SHA-256/384/512対応 |
| ホスト関数: `get_extension_input` | `crates/wasm-host/src/lib.rs` | **実装済み** — 補助入力コピー |
| ホスト関数: `get_content_length` | `crates/wasm-host/src/lib.rs` | **実装済み** — コンテンツ全長取得 |
| 補助入力の隔離（extension_inputs分配） | `crates/tee/src/endpoints/verify.rs` の `process_extension()` | **実装済み** — extension_inputs[extension_id]のみWASMに渡す |

#### §3.3–3.4 属性を伴う登録と解決・重複解決

スタブ（§2.3–2.4と同等の状態）。

---

### §4 運用モデル

仕様書のみの内容（ユースケース・境界定義）であり、直接的な実装対象ではない。
ただし以下の設計方針はコードに反映済み:
- Content-Agnostic: TEEはコンテンツ内容を評価しない設計
- Stateless TEE: `TeeState` enumによる状態管理（Inactive/Active のみ）

---

### §5 データ構造

#### §5.1 登録フローのデータ構造

| 仕様の要素 | Rust型 (`crates/types`) | TS型 (`sdk/ts/src/types.ts`) | 状態 |
|-----------|------------------------|------------------------------|------|
| Step 1: ClientPayload | `ClientPayload` | `ClientPayload` | **型のみ** |
| Step 2: EncryptedPayload | `EncryptedPayload` | `EncryptedPayload` | **型のみ** |
| Step 3: VerifyRequest | `VerifyRequest` | `VerifyRequest` | **型のみ** |
| Step 4: signed_json (Core) | `SignedJson`, `SignedJsonCore`, `CorePayload` | `SignedJson`, `CorePayload` | **型のみ** |
| Step 5: signed_json (Extension) | `SignedJsonExtension`, `ExtensionPayload` | `SignedJson`, `ExtensionPayload` | **型のみ** |
| Step 6: VerifyResponse | `VerifyResponse` | `VerifyResponse` | **型のみ** |
| Step 8: SignRequest | `SignRequest` | `SignRequest` | **型のみ** |
| Step 10: SignResponse | `SignResponse` | `SignResponse` | **型のみ** |
| Step 11: cNFT Metadata | `CnftMetadata`, `Attribute` | `CnftMetadata`, `Attribute` | **型のみ** |

#### §5.2 検証フローのデータ構造

| 仕様の要素 | 対応コード | 状態 |
|-----------|-----------|------|
| Step 1: Global Config | `GlobalConfig`, `TrustedTeeNode`, etc. + Anchorプログラム | **型のみ + Solanaプログラム実装済み** |
| Step 2–7: 検証ロジック | `sdk/ts/src/resolve.ts` | **スタブ** |

---

### §6 システム実装

#### §6.1 コンポーネント構成

全コンポーネントのクレートまたはパッケージが存在する:

| コンポーネント | クレート/パッケージ | 状態 |
|--------------|-------------------|------|
| Gateway | `crates/gateway` | **実装済み** — 5エンドポイント+Gateway認証+S3署名付きURL+sign-and-mint |
| TEE | `crates/tee` | **実装済み** — /verify・/sign・/create-tree、MockRuntime+NitroRuntime、proxy_client(direct HTTPモード対応) |
| Proxy (vsock) | `crates/proxy` | **実装済み** — vsock(Linux)/TCPフォールバック(macOS)、非同期、テスト3件 |
| SDK | `sdk/ts` | **実装済み** — TitleClient, register(11ステップ), crypto(E2EE), StorageProvider |
| Indexer | `indexer` | **実装済み** — DasClient、Webhook、Poller、DB CRUD |

#### §6.2 Gateway

| 仕様の要素 | 対応コード | 状態 |
|-----------|-----------|------|
| API: POST /upload-url | `crates/gateway/src/main.rs` の `handle_upload_url()` | **実装済み** — S3/MinIO署名付きURL生成、EDoSサイズ制限チェック |
| API: POST /verify | `crates/gateway/src/main.rs` の `handle_verify()` | **実装済み** — Gateway認証付きTEE中継 |
| API: POST /sign | `crates/gateway/src/main.rs` の `handle_sign()` | **実装済み** — Gateway認証付きTEE中継 |
| API: POST /sign-and-mint | `crates/gateway/src/main.rs` の `handle_sign_and_mint()` | **実装済み** — TEE中継+Gatewayウォレット署名+Solana RPCブロードキャスト |
| GET /.well-known/title-node-info | `crates/gateway/src/main.rs` の `handle_node_info()` | **実装済み** — signing_pubkey, supported_extensions, limits返却 |
| Gateway認証（Ed25519署名付与） | `crates/gateway/src/main.rs` の `build_gateway_auth_wrapper()` | **実装済み** — GatewayAuthSignTarget署名+GatewayAuthWrapper構築 |
| Gateway認証検証（TEE側） | `crates/tee/src/gateway_auth.rs` の `verify_gateway_auth()` | **実装済み** — /verify, /signの両方で検証、GATEWAY_PUBKEY環境変数でON/OFF |
| resource_limits付与 | `crates/gateway/src/main.rs` + `crates/tee/src/endpoints/verify.rs` | **実装済み** — Gateway→TEE転送時に付与、TEE側でc2pa_max_graph_size等を適用 |
| レート制限・APIキー管理 | — | **未着手** |

#### §6.3 Temporary Storage

| 仕様の要素 | 対応コード | 状態 |
|-----------|-----------|------|
| S3署名付きURL発行 | `crates/gateway/src/main.rs` の `handle_upload_url()` | **実装済み** — rust-s3クレートでpresigned PUT/GET URL生成 |
| MinIO（ローカル開発） | `docker-compose.yml` | **実装済み** — MinIOサービス定義済み |

#### §6.4 TEE

| 仕様の要素 | 対応コード | 状態 |
|-----------|-----------|------|
| 鍵生成（Ed25519署名用） | `crates/tee/src/runtime/mod.rs` の `generate_signing_keypair()` | **実装済み**（Mock + Nitro） |
| 鍵生成（X25519暗号化用） | `crates/tee/src/runtime/mod.rs` の `generate_encryption_keypair()` | **実装済み**（Mock + Nitro） |
| Attestation Document取得 | `crates/tee/src/runtime/mod.rs` の `get_attestation()` | **実装済み**（Mock + Nitro） |
| 署名 | `crates/tee/src/runtime/mod.rs` の `sign()` | **実装済み**（Mock + Nitro） |
| 署名用公開鍵取得 | `crates/tee/src/runtime/mod.rs` の `signing_pubkey()` | **実装済み**（Mock + Nitro） |
| 暗号化用鍵取得 | `crates/tee/src/runtime/mod.rs` の `encryption_secret_key()` / `encryption_pubkey()` | **実装済み**（Mock + Nitro） |
| MockRuntime（ローカル開発） | `crates/tee/src/runtime/mock.rs` | **実装済み** — 全メソッド実装（tree keypair含む）、テスト6件 |
| NitroRuntime（本番） | `crates/tee/src/runtime/nitro.rs` | **実装済み** — NSM API抽象化（NsmOpsトレイト）、RealNsm(Linux)/MockNsm(テスト)、テスト8件 |
| Attestation Document検証 | `crates/crypto/src/attestation.rs` | **実装済み** — COSE Sign1パース、P-384証明書チェーン検証（AWS Nitro PKIルート）、PCR抽出、公開鍵一致確認、テスト6件 |
| TEE起動シーケンス | `crates/tee/src/main.rs` | **部分的** — MOCK_MODE分岐あり、MockRuntime時は鍵生成動作 |
| /create-tree エンドポイント | `crates/tee/src/endpoints/create_tree.rs` | **実装済み** — Bubblegum create_tree Tx構築、tree+signing部分署名、状態遷移 |
| inactive→active 状態遷移 | `crates/tee/src/main.rs` + `create_tree.rs` | **実装済み** — `TeeState` enum |
| /verify 内部処理（7ステップ） | `crates/tee/src/endpoints/verify.rs` | **実装済み** — Core処理+Extension(WASM)実行完了、Gateway認証・resource_limits実装済み |
| /sign 内部処理（5ステップ） | `crates/tee/src/endpoints/sign.rs` | **実装済み** — signed_jsonフェッチ(1MB制限)、tee_signature検証、Bubblegum V2 MintV2構築（MPL-Coreコレクション対応）、TEE部分署名 |
| ハイブリッド暗号化（ECDH+HKDF+AES-GCM） | `crates/crypto/src/lib.rs` | **実装済み** — プリミティブは完備 |
| Gateway認証検証（TEE側） | `crates/tee/src/gateway_auth.rs` | **実装済み** — Ed25519署名検証、GATEWAY_PUBKEY環境変数で制御 |
| resource_limits適用 | `crates/tee/src/security.rs`, `crates/tee/src/endpoints/verify.rs` | **実装済み** — 全7パラメータ完全適用（resolve_limits）、/verifyと/signの両方で適用 |
| 漸進的重み付きセマフォ予約 | `crates/tee/src/security.rs` の `proxy_get_secured()` | **実装済み** — 64KBチャンク単位でSemaphore予約、枯渇時は即座に接続切断、テスト3件 |
| 動的グローバルタイムアウト | `crates/tee/src/security.rs` の `compute_dynamic_timeout()` | **実装済み** — min(MaxLimit, BaseTime + ContentSize/MinSpeed)、/verifyに適用 |
| Verify on Sign 防御 | `crates/tee/src/endpoints/sign.rs`, `crates/tee/src/security.rs` | **実装済み** — proxy_get_securedによるサイズ制限+チャンクタイムアウト+セマフォ、tee_signature検証 |
| メモリ管理（Zip Bomb/Slowloris対策） | `crates/tee/src/security.rs` | **実装済み** — 宣言サイズ事前検証（Zip Bomb）、チャンク単位Read Timeout（Slowloris）、テスト2件 |
| 不正WASMインジェクション防御（TEE側） | `crates/tee/src/endpoints/verify.rs`, `crates/tee/src/main.rs` | **実装済み** — TRUSTED_EXTENSIONS環境変数による許可リスト制御、テスト1件 |
| vsock経由の通信（プロキシクライアント） | `crates/tee/src/proxy_client.rs` | **実装済み** — TCPフォールバック+直接HTTPモード(PROXY_ADDR=direct)、vsockはLinux vsockクレートで実装 |

#### §6.5 Merkle Tree

| 仕様の要素 | 対応コード | 状態 |
|-----------|-----------|------|
| Sharded Treeアーキテクチャ | `crates/tee/src/endpoints/create_tree.rs` | **部分的** — 単一Treeのみ（Shardingは未実装） |
| Tree作成トランザクション構築 | `crates/tee/src/solana_tx.rs` の `build_create_tree_tx()` | **実装済み** — create_account + Bubblegum V2 CreateTreeConfigV2（`mpl-bubblegum` 2.1クレート使用） |
| Bubblegum V2/SPL Account Compression V2連携 | `crates/tee/src/solana_tx.rs` | **実装済み** — `mpl-bubblegum` 2.1クレートの `CreateTreeConfigV2Builder` + `MintV2Builder` 使用。MPL-Coreコレクション対応 |

#### §6.6 インデクサ

| 仕様の要素 | 対応コード | 状態 |
|-----------|-----------|------|
| Webhookハンドラ | `indexer/src/webhook.ts` | **実装済み** — MINT(Core/Extension振り分け)・BURN・TRANSFER対応 |
| DAS APIポーラー | `indexer/src/poller.ts` | **実装済み** — 全件取得+DB差分同期+Burn検出、startPollerで定期実行 |
| DAS APIクライアント | `indexer/src/das.ts` | **実装済み** — Helius DAS、フラットURL配列+ランダム選択、ページネーション対応、テスト6件 |
| DBスキーマ（CoreRecord/ExtensionRecord） | `indexer/src/db/schema.ts` | **型のみ** — DDLはclient.tsのmigrateで実体化 |
| DB実体（PostgreSQL） | `indexer/src/db/client.ts` | **実装済み** — CRUD(insert/find/markBurned/updateOwner)、べき等マイグレーション |
| エントリポイント | `indexer/src/index.ts` | **実装済み** — DB初期化→マイグレーション→Webhookサーバー→ポーラー起動 |

#### §6.7 SDK

| 仕様の要素 | 対応コード | 状態 |
|-----------|-----------|------|
| TitleClient（ノード管理） | `sdk/ts/src/client.ts` | **実装済み** — フラットURL配列、ランダム選択、セッションアフィニティ、upload/verify/sign/signAndMint |
| upload() | `sdk/ts/src/client.ts` の `TitleClient.upload()` | **実装済み** — /upload-url取得+署名付きURLにPUT |
| register() | `sdk/ts/src/register.ts` | **実装済み** — 11ステップ全実装（暗号化・アップロード・verify・復号・wasm_hash検証・offchainアップロード・sign・Tx検証・署名・ブロードキャスト） |
| resolve() | — | **対象外** — インデクサ側の責務（SDKでは不要） |
| discoverNodes() | — | **対象外** — TitleClientのフラットURL配列で代替 |
| wasm_hash検証（セキュリティクリティカル） | `sdk/ts/src/register.ts` Step 8 | **実装済み** — GlobalConfig.trusted_wasm_modulesと照合 |
| トランザクション検証 | `sdk/ts/src/register.ts` Step 11 | **実装済み** — trusted_tee_nodesの署名者確認 |
| E2EEクライアント側暗号化 | `sdk/ts/src/crypto.ts` | **実装済み** — X25519 ECDH + HKDF-SHA256 + AES-256-GCM、Rust相互運用可能、テスト10件 |
| StorageProvider（Arweave） | `sdk/ts/src/storage.ts` | **実装済み** — ArweaveStorage（upload関数注入）+ HttpStorage（開発用） |

---

### §7 WASM実装詳細

#### §7.1 安全性確保

| 仕様の要素 | 対応コード | 状態 |
|-----------|-----------|------|
| Fuel/Memory制限 | `crates/wasm-host/src/lib.rs` | **スタブ** — 値は保持するが適用なし |
| catch_unwind | `crates/wasm-host/src/lib.rs` の `execute()` | **実装済み** |
| Core→Extension処理順序の保証 | — | **未着手** |
| 補助入力の分配（extension_inputs隔離） | — | **未着手** |
| read_content_chunk ホスト関数 | — | **未着手** — WASMに `extern` 宣言あり、ホスト側未実装 |
| hash_content ホスト関数 | — | **未着手** — 同上 |
| hmac_content ホスト関数 | — | **未着手** |
| get_extension_input ホスト関数 | — | **未着手** |

#### §7.4 公式WASMセット

| WASM ID | 対応コード | 状態 |
|---------|-----------|------|
| phash-v1 | `wasm/phash-v1/src/lib.rs` | **実装済み** — SHA-256ベース簡易pHash（`hash_content`ホスト関数使用） |
| hardware-google | `wasm/hardware-google/src/lib.rs` | **実装済み** — ハードウェアアサーションマーカー検出（`read_content_chunk`使用） |
| c2pa-training-v1 | `wasm/c2pa-training-v1/src/lib.rs` | **実装済み** — `c2pa.training-mining`アサーション検出+notAllowed判定 |
| c2pa-license-v1 | `wasm/c2pa-license-v1/src/lib.rs` | **実装済み** — Creative Commonsライセンス7種+c2pa.rights+schema.org検出 |

全WASMモジュールは `#![no_std]` + `dlmalloc` + パニックハンドラ + `alloc()` + length-prefixed JSON結果フォーマットで**実装済み**。

---

### §8 ガバナンス

| 仕様の要素 | 対応コード | 状態 |
|-----------|-----------|------|
| Global Config管理（authority） | `programs/title-config/src/lib.rs` の `Initialize` context | **実装済み** |
| TEEノードの追加・削除 | `programs/title-config/src/lib.rs` の `update_tee_nodes()` | **実装済み** |
| WASMモジュールの管理 | `programs/title-config/src/lib.rs` の `update_wasm_modules()` | **実装済み** |
| TSA Trust Listの管理 | `programs/title-config/src/lib.rs` の `update_tsa_keys()` | **実装済み** |
| Collection Authority Delegate | — | **未着手** |

---

### §9 コスト設計

仕様書のみの内容（コスト試算・課金モデル定義）。
クレジット制の実装は未着手。

---

### §10 ロードマップ

実装対象なし。

---

## インフラ・ビルドの状態

| 項目 | 状態 | 備考 |
|-----|------|------|
| `cargo check --workspace` | **通過** | 全クレートがコンパイル可能 |
| `cargo test --workspace` | **通過** | crypto 6件(attestation), core 8件, gateway 6件, proxy 3件, tee 44件（mock 6件 + nitro 8件 + verify 4件 + sign 4件 + create_tree 2件 + solana_tx 8件 + gateway_auth 4件 + security 8件）, wasm-host 5件 |
| WASMビルド (`wasm32-unknown-unknown`) | **通過** | 4モジュール全てビルド可能 |
| TypeScript SDK ビルド (`tsc`) | **通過** | テスト10件通過 |
| TypeScript Indexer ビルド (`tsc`) | **通過** | テスト6件通過（DasClient） |
| Docker Compose | **定義済み** | 7サービス構成。起動はローカル環境依存 |
| Dockerfile (5ファイル) | **定義済み** | tee, tee-mock, gateway, proxy, indexer |
| CI (.github/workflows/ci.yml) | **定義済み** | cargo check/test, WASMビルド, TSビルド, E2Eテスト |
| scripts/setup-local.sh | **実装済み** | 7ステップ初期化（ツール確認→Solana待機→MinIO→Anchor→フィクスチャ→GlobalConfig→ヘルスチェック） |
| scripts/init-config.mjs | **実装済み** | Global Config初期化+TEEノード登録+/create-tree（Node.js ESM） |
| scripts/build-enclave.sh | **実装済み** | Docker build + nitro-cli build-enclave + PCR値表示 |
| E2Eテストスイート (tests/e2e) | **実装済み** | 9テスト全合格（ヘルスチェック×3、Gateway API、E2EE verify、signフロー、来歴グラフ、鍵ローテーション拒否、重複コンテンツ） |
| C2PAフィクスチャ生成 (gen_fixture) | **実装済み** | `cargo run --example gen_fixture -p title-core` — 4ファイル生成（signed, ingredient_a/b, with_ingredients） |

---

## 実装の優先度マップ

現時点でスタブの項目について、仕様書§10のロードマップ（Phase 1: 2026 Q1）に基づく優先度:

### 最優先（Phase 1必須・クリティカルパス）

1. **TEE Runtime実装**（MockRuntime）— 鍵生成がなければ全フローが動作しない
1. + Proxy (prototype/enclave-c2pa/proxy/ を参考にtokio化するだけ)
2. **C2PA検証** (`crates/core`) — Core機能の根幹
3. **TEE /verify エンドポイント** — 登録フロー Phase 1
4. **TEE /sign エンドポイント** — 登録フロー Phase 2
5. **Gateway全ハンドラ** — クライアントとTEEの中継
6. **TS SDK crypto** — E2EEクライアント側
7. **TS SDK register()** — 登録フローのクライアント実装
8. **TS SDK resolve()** — 検証フローのクライアント実装

### 高優先度

9. **WASMホスト実行エンジン** (`crates/wasm-host`) — Extension機能
10. **WASMモジュール実装** (phash-v1が最優先) — Extension機能
11. **Gateway認証** — セキュリティ要件
12. **vsock Proxy** — Nitro Enclave通信
13. **インデクサ** — 検索・利便性レイヤー

### 中優先度

14. **NitroRuntime** — **実装済み**（タスク11）
15. **漸進的重み付きセマフォ予約** — メモリ管理
16. **重複解決ロジック** — §2.4の判定ロジック
17. **TS SDK discoverNodes()** — ノード選択
18. **TS SDK upload()** — コンテンツアップロード
19. **ArweaveStorage** — オフチェーンストレージ
20. **セットアップスクリプト** — ローカル開発環境

---

## カバーできていないもの（概念・設計レベル）

以下は仕様書に記述があるが、現在のコードに対応する実装も型定義も存在しない領域:

| 仕様箇所 | 内容 | 備考 |
|---------|------|------|
| §2.4 | TSAタイムスタンプによる重複解決 | TSA検証ロジック全般 |
| §4.3 | Burn済みトークンの除外ロジック | resolve側の実装が必要 |
| §6.2 | Gateway認証（署名の付与・検証の双方） | **実装済み（タスク06）** — Gateway側署名付与 + TEE側署名検証 |
| §6.2 | レート制限・APIキー管理・課金ティア | 運用機能 |
| §6.4 | 漸進的重み付きセマフォ予約 | **実装済み（タスク10）** — security.rsのproxy_get_secured |
| §6.4 | 動的グローバルタイムアウト | **実装済み（タスク10）** — compute_dynamic_timeout |
| §6.4 | Verify on Sign 防御 | **実装済み（タスク05+10）** — proxy_get_securedによる三層防御 |
| §6.4 | 不正WASMインジェクション防御 | **実装済み（タスク10）** — TEE側trusted_extension_ids + SDK側wasm_hash検証 + E2EEレスポンス暗号化 |
| §6.5 | Bubblegum V2/SPL Account Compression V2連携 | **実装済み（タスク05）** — `mpl-bubblegum` 2.1クレート使用、MPL-Coreコレクション対応 |
| §6.7 | SDK upload() | **実装済み（タスク08）** — TitleClient.upload() |
| §6.7 | SDK wasm_hash検証 | **実装済み（タスク08）** — register.ts Step 8 |
| §6.7 | SDK トランザクション検証 | **実装済み（タスク08）** — register.ts Step 11 |
| §7.1 | hmac_content ホスト関数 | WASM実行基盤（sha256/384/512は実装済み、HMACのみ未着手） |
| §7.1 | Core→Extension処理順序保証（メモリ解放後にExtension実行） | メモリ安全性（現在は順次実行だがメモリ解放は未実装） |
| §8.2 | Collection Authority Delegate | ガバナンス |
| §9.2 | クレジット制課金 | 運用機能 |
