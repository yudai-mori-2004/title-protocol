# Task 19 — Streaming content fetch (HTTP Range Request)

**ステータス**: done (2026-05-25, 全 3 入力タイプの streaming 化 + 監査 + テスト強化完了)
**実装ブランチ**: main
**達成範囲**: **全 3 入力タイプ (`single` / `fragmented` / `sidecar`) で仕様 §4.3 のメモリパターン要件を達成**。`single` / `sidecar` は HTTP Range Request 経由で peak ≈ `min_req_size` (4 MiB)、`fragmented` は init + 1 fragment の lazy load (FragmentedReader)。Ticket は `AtomicU64` 化で Sync 化、fragment swap が動的に `extend` / `shrink` を呼ぶ shrink ループも有効化。proxy 経由 (vsock / TCP) も対応。50 GB MP4 / 数千 fragment の long-form 動画でも total_limit を超えない構造。実 C2PA 署名付き JPEG を split して FragmentedReader 経由で c2pa-verify が parse 成功する e2e テストで integration を保証。監査 round (アーキテクチャ / ロジック / 実装クリーンさ) で重大バグ D-14 を含む全指摘を修正。

---

## 1. 何が解決されたか

旧実装: `crates/tee/src/content_fetch.rs` の 3 入力タイプすべてが content を `Vec<u8>` に全バッファしてから processor に渡していた。10 GB / 50 GB の動画はメモリに載らず破綻、long-form fragmented も全 fragment concat で爆発する。

新実装は **全 3 入力タイプで仕様 §4.3 のメモリパターン要件を達成**:

| 入力タイプ | ピークメモリ | 実装 |
|---|---|---|
| `single` | `min_req_size` (4 MiB) | `HttpRangeSource` / `ProxyRangeSource` が Range Request 対応サーバーから必要部分だけ取得 |
| `sidecar` | manifest (~数十 KB) + `min_req_size` (4 MiB) | manifest は full fetch (small)、content は single と同じ Range Request 経路 |
| `fragmented` | `init + 1 fragment` | `FragmentedSource` が init を常駐、fragments を 1 個ずつ lazy load (swap 時に shrink) |

仕様 §4.3 の「Range Request パターン」: マニフェスト + チャンク 1 個分のピーク。c2pa-rs の `Reader::with_stream(content_type, &mut content)` は box header を seek で辿ってハッシュ検証もチャンク単位で行うため、Range Request reader を直接流せる。

仕様 §4.3 の「フラグメントパターン」: 初期化セグメントを読み込んだ後、メディアセグメントを 1 つずつ処理して解放する。`FragmentedReader` が `Read + Seek` を実装して連結ストリーム view を提供しつつ、内部で fragment 1 個のみ in-memory に保持。`Arc<Ticket>` を bind することで fragment swap が動的に `extend` / `shrink` を呼ぶ shrink ループも有効化。

監査 round1/2/3 の `should-fix-001` (フラグメント全 concat) と `should-fix-002` (漸進予約が事後カウンタ化) は **両方とも完全解消**。

---

## 2. 設計

### 2.1 抽象レイヤ

新しいコア型を `crates/core/src/content_stream.rs` に追加。

```rust
pub trait ContentStream: Read + Seek + Send {}
impl<T: Read + Seek + Send + ?Sized> ContentStream for T {}

pub trait ContentSource: Send + Sync {
    fn open(&self) -> std::io::Result<Box<dyn ContentStream>>;
    fn size_hint(&self) -> Option<u64> { None }           // 論理サイズ (timeout 計算用)
    fn peak_memory_hint(&self) -> Option<u64> { self.size_hint() }  // メモリ予約用
}

pub struct InMemorySource { bytes: Arc<[u8]> }
```

- `size_hint`: ファイル長。`compute_global_timeout` で使う (大きいファイルほど長い timeout)。
- `peak_memory_hint`: ソース 1 本の reader が同時に保持するピークメモリ。in-memory ソースなら full size、Range Request ソースなら reader バッファサイズ。これが「`size_hint` を直接 ticket.extend してはいけない」理由 (50 GB が admission_limit で reject されてしまう)。
- factory pattern: processor を並列実行するとき、各 processor が `source.open()` で独立した reader を取得する。

### 2.2 Processor / orchestrator の streaming 化

```rust
// 旧
pub trait Processor: Send + Sync {
    fn process(&self, content: &[u8], content_type: &str) -> Result<...>;
}

// 新
pub trait Processor: Send + Sync {
    fn process(&self, content: &mut dyn ContentStream, content_type: &str) -> Result<...>;
}
```

`c2pa_verify.rs` の `compute_signature_hash` も `&[u8]` から `&mut dyn ContentStream` に変更。`c2pa::jumbf_io::load_jumbf_from_stream` (Read+Seek 受け取り) を `CaiReadAdapter` で噛ませて使う。JUMBF 抽出が単一パスになり旧実装の `Reader::with_stream` + `load_jumbf_from_memory` の二重パスが解消。

orchestrator: `FetchedContent` から `content_bytes: Vec<u8>` を `source: Box<dyn ContentSource>` に置換。`ProcessorRegistry::execute(processor_ids, &dyn ContentSource, content_type)` で各 processor 呼び出しの直前に `source.open()` を呼ぶ。

### 2.3 ストリーミング fetcher

`ContentFetcher` trait に `fetch_streaming` を追加。デフォルト実装は `fetch()` の結果を `InMemorySource` で包む (mock fetcher 等はこれで動く)。

```rust
trait ContentFetcher {
    fn fetch(&self, url: &str) -> Result<FetchResponse, FetchError>;
    fn fetch_streaming(&self, url: &str) -> Result<StreamingFetchResponse, FetchError> {
        // default: fall back to fetch + InMemorySource
    }
}
```

`HttpContentFetcher::fetch_streaming` の override:
1. `HttpRangeSource::probe(url)` で HEAD を打ち Accept-Ranges + Content-Length を確認
2. 対応していれば `HttpRangeSource` を返す
3. 対応していなければ `fetch()` 経由で `InMemorySource` にフォールバック

`HttpRangeSource::open()` は `http_range_client::HttpReader`(= `SyncBufferedHttpRangeClient<reqwest::blocking::Client>`) を新規構築する。c2pa-rs が期待する `Read + Seek` を実装。`min_req_size` を 64 KB に設定し、c2pa-rs の box header スキャンが過剰な小 Range Request を発行しないように緩衝する。

### 2.4 Proxy wire protocol 拡張

proxy (vsock) 経由でも Range Request を中継するため、`title-proxy` のワイヤープロトコルに 2 メソッドを追加:

| Method     | Request body                       | Response body                                          |
|------------|------------------------------------|--------------------------------------------------------|
| `HEAD`     | (empty)                            | `[u64 content_length][u8 accept_ranges][u32 etag_len][etag][u32 ct_len][ct]` |
| `GET_RANGE`| `[u64 begin][u64 length]` (16 byte)| 通常の `[u32 body_len][body]` (HTTP `Range: bytes=BEGIN-END` 結果) |

エンコーディング/デコードは `title_proxy::protocol::encode_head_response` / `decode_head_response` / `encode_get_range_body` / `decode_get_range_body` を SoT とし、tee 側 (`ProxyRangeSource`) と proxy 側 (`handler.rs`) で共有。

`ProxyRangeSource` は `http_range_client::SyncBufferedHttpRangeClient` に proxy backend (`ProxyRangeBackend`) を被せることで実装。これにより http-range-client の buffering と Read+Seek 実装をそのまま再利用。

### 2.5 メモリ会計

`fetch_single` は `ContentSource::peak_memory_hint` で `ticket.extend` する:
- In-memory ソース (mock / sidecar / 暗号化復号後): full size
- Range Request ソース: reader バッファサイズのみ (64 KB)

`size_hint` (論理サイズ) は `compute_global_timeout` 用に保持。50 GB ファイルでも `peak_memory_hint = 64 KB` で admission を通過、`total_limit` をほぼ使わずに完走できる。テスト `fetch_single_streaming_source_reserves_only_peak_memory` で検証。

EOF 処理: `http_range_client::HttpReader` の Read impl は HTTP 416 を `ErrorKind::UnexpectedEof` で返す (Rust の `Ok(0)` 規約から外れる)。`EofSafeHttpReader` / `EofSafeProxyRangeReader` アダプタで `Ok(0)` に正規化し、`read_to_end` 等の標準 API と互換にした。

---

## 3. 変更ファイル一覧

### 新規ファイル
- `crates/core/src/content_stream.rs` — `ContentStream` / `ContentSource` / `InMemorySource`
- `crates/tee/src/range_source.rs` — `HttpRangeSource` (http-range-client backed)
- `crates/proxy/src/lib.rs` — protocol / handler を library として公開

### 変更ファイル
- `Cargo.toml` (workspace) — http-range-client, bytes 追加なし (tee 側 Cargo.toml のみ)
- `crates/core/src/lib.rs` — content_stream module 公開
- `crates/core/src/processor.rs` — `Processor::process` signature 変更、`ProcessorRegistry::execute` を ContentSource 経路に
- `crates/core/src/c2pa_verify.rs` — `compute_signature_hash` を Read+Seek 化、CaiReadAdapter 追加、`load_jumbf_from_stream` 経由に変更
- `crates/core/src/rootlens_license_v1.rs` — `process` を Read+Seek 化、テスト helper 追加
- `crates/tee/Cargo.toml` — `http-range-client = "0.9"`, `bytes = "1"`, `title-proxy` 追加
- `crates/tee/src/lib.rs` — `pub mod range_source`
- `crates/tee/src/content_fetch.rs` — `FetchedContent` を ContentSource ベースに、`StreamingFetchResponse` 追加、`fetch_streaming` default impl、`fetch_single` を `peak_memory_hint` ベース予約に
- `crates/tee/src/orchestrator.rs` — `Materialized` 中間型、`compute_signature_hash_from_source` helper、ContentSource 経路に書き換え、`Vec<u8>`/`Cursor` 依存除去
- `crates/tee/src/proxy_fetcher.rs` — `ProxyRangeSource` / `ProxyRangeBackend` 追加、`fetch_streaming` override、proxy crate からの定数 import
- `crates/proxy/Cargo.toml` — `[lib]` 追加
- `crates/proxy/src/main.rs` — `mod handler/protocol` → `use title_proxy::{handler, protocol}`
- `crates/proxy/src/protocol.rs` — HEAD/GET_RANGE エンコーディング関数 + テスト
- `crates/proxy/src/handler.rs` — `handle_head` / `handle_get_range` 追加、method whitelist 拡張

### ドキュメント
- `docs/v0.1.2/COVERAGE.md` — §3.1/§3.2/§4.3/§5.2 を task 19 言及付きで更新
- `docs/v0.1.2/audit/k3-tee.md` — should-fix-001 (partial fixed) / should-fix-002 (fixed) を追記
- `docs/v0.1.2/audit/round2/k3-tee.md` — 同上
- `docs/v0.1.2/audit/round3/k3-tee.md` — should-fix-001/002 の wontfix 判定を fixed/fixed-partial に更新

---

## 4. テスト追加

最初の Phase 1〜5 実装で約 22 件、監査 round で **+14 件 (contract suite + 境界条件 + adversarial)**、3-input streaming round で **+19 件 (FragmentedSource + dynamic ticket + e2e c2pa-verify)** を追加。**workspace 合計 334 tests pass**。

### `crates/core`

- `content_stream::contract::assert_content_source_contract` — **ContentSource 規約テストヘルパ** (always-public, `#[doc(hidden)]`)。外部 crate のテストから呼べる。検証項目: size_hint / read_to_end / seek+read / **Read::read 規約準拠 (D-14 検出)** / EOF→Ok(0) / 独立 open。
- `content_stream::tests::in_memory_source_contract_various_sizes` — 1, 7, 64, 100, 255, 256, 257, 1023, 1024, 1025 byte (境界含む) で InMemorySource を contract に通す。
- `content_stream::tests::in_memory_source_contract_empty` — 空入力の contract。
- `c2pa_verify::tests::signature_hash_idempotent_on_same_stream` — 同一 reader 2 回呼び出しで結果一致。
- `rootlens_license_v1::tests::process_rewinds_stream_each_call` — 同上。
- `processor::tests::registry_execute_gives_each_processor_independent_reader` — **並列 processor 実行の factory パターン契約**。2 processor が両方 read_to_end しても各々完全な payload を見る。
- `processor::tests::registry_execute_isolated_state_across_processors` — 1 番目が 3 byte 部分読みしても 2 番目が先頭から見える (reader 独立性)。

### `crates/tee`

- `content_fetch::tests::fetch_single_streaming_source_reserves_only_peak_memory` — 50 GB 想定の streaming source で reserved = `peak_memory_hint` のみ。
- `content_fetch::tests::default_fetch_streaming_wraps_full_body_as_in_memory_source` — default 実装が InMemorySource を返す。
- `content_fetch::tests::default_fetch_streaming_rejects_empty_body` — 空 body は EmptyContent。
- `range_source::tests::*` (5 件: probe / open+read / seek+read / 複数 open / contract suite × 3) — HttpRangeSource 全機能。
- `range_source::tests::http_range_source_contract_*` (3 件) — 末尾跨ぎ (file_size % min_req_size != 0) / aligned / min_req > body の境界条件で contract suite を回す。
- `range_source::tests::probe_fails_when_*` (4 件) — HEAD 405 / Accept-Ranges 欠落 / Accept-Ranges: none / Content-Length 0 で probe が正しく失敗する。
- `proxy_fetcher::tests::proxy_range_source_contract_*` (3 件) — 同じ contract suite を proxy backend にも適用。direct と proxy で同質性が保証される。
- `proxy_fetcher::tests::proxy_range_source_*` + `proxy_fetcher_fetch_streaming_uses_range_source` (4 件) — fake proxy で HEAD + GET_RANGE wire を end-to-end 検証。

### `crates/proxy`

- `protocol::tests::head_response_roundtrip_*` / `get_range_body_*` (5 件) — encode/decode roundtrip + truncated / 不正サイズ rejection。
- `tests::head_returns_structured_response` / `get_range_returns_requested_slice` / `get_range_rejects_invalid_body_size` / `get_range_rejects_overflow_begin_plus_length` / `get_range_zero_length_returns_empty_without_upstream` (5 件) — handler の end-to-end (axum upstream + TCP proxy)。adversarial 入力 (overflow / length=0) を構造的に弾く経路を verify。

### 3-input streaming round 追加分 (Fragmented + Ticket shrink + e2e c2pa-verify)

- `fragmented_source::tests::*` (16 件) — `FragmentedSource` / `FragmentedReader` の網羅テスト:
  - `size_hint_equals_init_plus_all_fragments` / `peak_memory_hint_is_init_plus_max_fragment` — メモリ会計の宣言値検証
  - `read_to_end_returns_concatenated_bytes` / `seek_into_middle_fragment_returns_correct_bytes` / `read_at_eof_returns_zero` — Read+Seek 基本動作
  - `fragment_swap_drops_previous_fragment_bytes` / `multiple_readers_load_fragments_independently` — fragment swap + 並列 reader の独立性
  - `fragmented_source_contract_basic` / `_uneven_fragments` / `_init_only_contract` / `_many_small_fragments` — 共通 contract suite を fragmented backend に適用 (D-14 系の read 規約準拠を含む)
  - **`dynamic_ticket_extends_and_shrinks_on_fragment_swap`** — `with_ticket(Arc<Ticket>)` bind 時に fragment swap が `extend(new) → shrink(old)` を呼び、`pool.used` が「init + 現 fragment」に動的に反映されることを実測検証
  - `dynamic_ticket_rejects_oversized_fragment` — extend 失敗で `OutOfMemory` を返し、reserved 量が崩れないこと
  - **`c2pa_verify_via_fragmented_source_init_only` / `_split_into_segments` / `_three_segments`** — 実 C2PA 署名付き JPEG を任意位置で split し FragmentedSource 経由で `c2pa::Reader::with_stream` に流して parse 成功することを e2e 検証。c2pa-rs が FragmentedReader の連結 view を JPEG として正しく扱う互換性を実 C2PA データで保証
- `content_fetch::tests::fetch_fragmented_*` 更新 — 動的予約モード (init.len のみ事前予約 + reader 内部で extend/shrink) に合わせて assertion を書き直し
- `content_fetch::tests::fetch_sidecar_*` — content fetch が `fetch_streaming` 経路を通ることを確認
- `resource_pool::tests` 更新 — `Ticket` が `AtomicU64` ベースに移行 (Sync 化) しても全 timeout/threshold テストが pass

---

## 5. 未着手 / 持ち越し

### v0.1.3 で対応すべき項目

1. **暗号化 single 入力の同時多重持ち** (audit B-6)

   `decrypt_single_payload` 内部で、同じバイト列分が瞬間最大 4 重にメモリ上に存在する:
   - `fetched.source` 内の `Arc<[u8]>` (ciphertext, ~100 MB max)
   - `cipher_bytes: Vec<u8>` (read_to_end でのコピー, ~100 MB max)
   - `opened.plaintext: Vec<u8>` (復号後, ~100 MB max)
   - `parsed.content.to_vec()` (`InMemorySource` 用クローン, ~100 MB max)

   **絶対メモリ量** は `MAX_RESPONSE_BYTES = 100 MB` × 4 ≈ 400 MB で、TEE 物理メモリ (1-2 GB) に対しては OOM しない。

   **問題は admission control の精度**: `ticket.extend` は ciphertext 分 (~100 MB) しか申告していないため、`admission_limit` の concurrency 制御が実態より 4 倍緩い。並列暗号化リクエスト 4 件で admission 上 400 MB 計上 → 実態 1.6 GB 使用、という乖離が出る。

   修正には `title_crypto::payload::parse_payload` を owned 返却に変える or `InMemorySource` を `Arc` 共有で plaintext と alias させる経路が必要 → task 19 のスコープを超えるので別 task。

### 完了済みだが補足

**実 fragmented MP4 fixture での c2pa-verify 検証**: 現状 e2e テストは「実 C2PA 署名付き JPEG を任意位置で split → FragmentedSource → c2pa-verify が parse 成功」で **FragmentedReader が c2pa-rs と互換に動作することを実 C2PA データで証明済み** (`fragmented_source::tests::c2pa_verify_via_fragmented_source_*`)。ただし「実 fragmented MP4 (init.mp4 + seg-*.m4s) の C2PA fragment hash 検証」は fixture 生成 (ffmpeg + c2pa CLI) が必要で別 task として整理。`with_stream` 経由で c2pa-rs が fragmented MP4 を完全 validate するかは production 投入前に実 fixture で確認する。

### Range Request 非対応サーバー対策

Cloudflare R2 / S3 互換ストレージ等は Range Request 標準対応。古い nginx / 一部 CDN の特殊エンドポイントは未対応のケースあり。`HttpRangeSource::probe` が失敗すれば自動的に full fetch にフォールバックする (=従来挙動)。本番の content URL の Range 対応状況は contained — オンチェーン storage は S3 互換が多いため実害は限定的。

---

## 6. 実機検証 (まだ)

50 GB の実 MP4 を AWS Nitro Enclave 上で processor 通過させる実機テストは未実施。本 task で構造的に「メモリ上は安全」になったが、実 storage の Range Request スループット / vsock の Range レイテンシ等は実測が必要。EC2 ノード (54.250.143.52) は現状 FileLoader モード (旧経路) なので、Range Request 対応版をデプロイするのは別 task。

---

## 7. 監査ラウンドと修正 (Phase 1〜5 完了直後に実施)

Phase 1〜5 完走後、ユーザー指示で 3 つの監査エージェント (アーキテクチャ / ロジック / 実装クリーンさ) を並行で回した。重大バグ 1 件を含む指摘を全て修正した。

### 7.1 重大バグ修正 — D-14: `http_range_client::SyncBufferedHttpRangeClient::Read::read` の規約違反継承

上流 crate の `Read::read` 実装が以下のバグを持つ:
```rust
fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
    let length = buf.len();
    let mut bytes = self.get_bytes(length)?;
    bytes.copy_to_slice(&mut buf[0..bytes.len()]);
    Ok(length)  // ← BUG: 実書き込み量は bytes.len() (< length のことがある)
}
```

ファイル長が `min_req_size` の倍数でない (本番 MP4 ではほぼ常にそう) とき、末尾を跨ぐ read で要求 N バイト・実取得 K バイト・`Ok(N)` 返却。残り N-K バイトは呼び出し側のヒープゴミがそのまま valid 扱いされる。c2pa-rs に偽データを渡して signature 検証失敗 (or worse, 偽 signature を成功と判定) する経路だった。

旧 `EofSafeHttpReader` / `EofSafeProxyRangeReader` は `Ok(n) => Ok(n)` で透過していたためバグを継承していた。

**修正**: `crates/tee/src/range_source.rs` に統合した `SafeRangeReader<T: SyncHttpRangeClient>` で:
- 内部の `get_bytes(buf.len())` を直接叩き、実 length (= `bytes.len()`) を返す
- file_size を保持し、EOF を超える request を未然に防ぐ (`get_request_range` が `split_to` でバッファを縮めた直後の 416 で「未消費バッファロスト」も起きないように)
- HTTP 416 → Ok(0) の正規化も含める

`HttpRangeSource` と `ProxyRangeSource` で共通アダプタを共有 (実装重複の解消)。

### 7.2 その他の audit findings

| ID | 種別 | 修正内容 |
|---|---|---|
| B-4 | 過小申告 | `peak_memory_hint` を `min_req_size` (= 4 MiB) ベースに統一。COSE 署名 + 中間/ルート証明書チェーンを 1 リクエストで取れる + 仕様 §4.3「マニフェスト + チャンク 1 個分」要件をカバー。 |
| C-9 | adversarial 入力 | proxy `handle_get_range` で `begin.checked_add(length)` overflow チェック追加。攻撃者が `begin=u64::MAX, length=2` を送っても upstream に流れない。 |
| H-27 | 重複 | `EofSafeHttpReader` + `EofSafeProxyRangeReader` を `SafeRangeReader<T>` ジェネリックに統合。 |
| H-28 | 重複 | `ProxyContentFetcher::open(&self)` を free function `open_socket(&endpoint, url)` を呼ぶ薄ラッパに整理。 |
| C-30 | dead code | `proxy_fetcher.rs::probe` 内の `let body = wire::encode_get_range_body(0, 0); let _ = body;` (2 行) を削除。 |
| B-6 doc | 古い doc | `proxy_fetcher.rs` の "Range Requests aren't implemented yet" を fetch_streaming 経路導入を反映した記述に書き換え。 |
| Phase 番号 | 内部スケジュール漏洩 | `content_fetch.rs` doc の `(Phase 3 で追加)` / `(Phase 5 で置き換え予定)` を `(task 19)` / `(v0.1.3 持ち越し)` に書き換え。 |
| log level | 運用視認性 | `fetch_streaming` の Range probe 失敗時を `tracing::debug!` → `tracing::warn!` に格上げ (50 GB ファイルが Range 非対応サーバーで全展開される経路を運用者が即座に検知できる)。 |

### 7.3 デフォルト値の再考: `min_req_size = 64 KB` → `4 MiB`

監査でユーザーから「64 KB は小さすぎ、数 MB でいい」と指摘を受け、`DEFAULT_MIN_REQ_SIZE` を 4 MiB に拡大した。理由:

- c2pa-rs の box header スキャン (8-16 byte read 多発) を 1 リクエストにまとめる効果が大きい
- JUMBF + COSE 署名 (典型 50 KB-500 KB) を 1 リクエストで取得
- 50 GB ファイルでも ~12,500 リクエスト程度に収まる
- `peak_memory_hint` を `min_req_size` と同値にできるため API がシンプル
- ResourcePool admission_limit (default 100 MB) に対し 25 並列の Range Request を許容する計算で、SPECS §4.3 と整合

### 7.4 全 audit 指摘の処理ログ

| 監査 | 件数 | 処理 |
|---|---|---|
| アーキテクチャ | 0 修正必要、1 軽微 (fetch_streaming fallback 可視性) | log level 格上げで対応 |
| ロジック | 1 重大 + 1 修正必要 + 1 推奨 | 全て修正済み |
| 実装クリーンさ | 3 要修正 + 3 改善余地 | 全て修正済み |

---

## 8. テスト設計の反省点と強化方針

監査 round で「**監査でしか発見できないテスト設計はダメ**」というユーザー指摘を受けた。Phase 1〜5 完走時のテストでは以下が抜けていた:

### 8.1 Phase 1〜5 のテスト設計の欠陥

1. **境界条件の網羅不足**: `min_req_size = 256, body_len = 4096` のような「割り切れる」サイズだけテストしており、末尾跨ぎ (file_size % min_req_size != 0) のケースが一切無かった。D-14 がこの隙間に潜んでいた。
2. **Read trait 規約の検証なし**: `read()` が返した N に対して `buf[..N]` が valid であることを直接 assertion していなかった。
3. **adversarial 入力の検証なし**: overflow / 不正なヘッダ / 非対応サーバーへの fallback path が test されていなかった。
4. **factory パターンの契約 test なし**: 並列 processor 実行時に「各 processor が独立 reader を取得する」契約が暗黙のままだった。

### 8.2 強化後のテスト設計原則

audit round で以下の原則を確立し、全 ContentSource 実装に適用:

1. **Contract test helpers** (`title_core::content_stream::contract::assert_content_source_contract`): trait の規約を 1 つのヘルパ関数で一括検証。新しい `ContentSource` 実装を追加するときは、このヘルパに通すだけで全プロパティ (size_hint / read_to_end / seek / **Read::read 規約準拠** / EOF / 独立 open) が検証される。
2. **境界条件の網羅**: body_len ∈ { 1, 7, 64, 100, 255, 256, 257, 1023, 1024, 1025 } を境界として網羅。`min_req_size` との関係も `==`, `> body`, `< body`, `% body != 0` のパターンで網羅。
3. **adversarial 入力ファースト**: 不正な wire body / HEAD 405 / overflow input / 空 body などを必ず 1 件以上 test。production code がこれらを silent fallback しないことを構造的に保証する。
4. **factory パターンの契約 test**: `ProcessorRegistry::execute` で 2 processor 並列実行し、各々が完全な payload を見る (=独立 reader)、かつ部分読みが他に漏れない (=state isolation) を assertion。
5. **differential testing**: `HttpRangeSource` と `ProxyRangeSource` で同じ contract suite を回す。direct HTTP と proxy 経由で挙動が一致することを保証する。

### 8.3 上流 crate のバグへの一般的対策

`http-range-client` の D-14 のような上流 crate のバグは、自前 wrapper で吸収するのが現実解。教訓:

- **trait 経由の I/O 規約は信用しない**: Read::read が `Ok(N)` を返したからといって `buf[..N]` が valid とは限らない。アダプタで明示的に検証する。
- **`get_bytes` のような lower-level API を直接叩く**: 上流の Read impl を信用する代わりに、より低レベルな API (e.g. `get_bytes`) から自前で組み立てる方が安全。
- **境界条件テストを最初から書く**: file_size が min_req_size の倍数のケースだけテストすると、本番でほぼ確実に踏むバグを見逃す。

---

## 9. 実機検証 (まだ)

50 GB の実 MP4 を AWS Nitro Enclave 上で processor 通過させる実機テストは未実施。本 task で構造的に「メモリ上は安全」になったが、実 storage の Range Request スループット / vsock の Range レイテンシ等は実測が必要。EC2 ノード (54.250.143.52) は現状 FileLoader モード (旧経路) なので、Range Request 対応版をデプロイするのは別 task。

---

## 引き継ぎ事項

ユーザーから「50 GB が明日来るので v0.1.3 では遅い、今必要」と明示の指摘を受けて立てた本 task は、`single` 入力の streaming 化までを安全に完走した。`fragmented` の per-fragment 化は本 task のスコープを超えており、v0.1.3 で `c2pa::Reader::with_fragment` API への置き換えを別 task で扱う前提。

monkey-patching 的な対応 (本番 fetch だけ修正、テスト/COVERAGE 反映なし) を避け、(a) trait の streaming 化、(b) 監査 finding の処理ログ更新、(c) COVERAGE.md 反映、(d) テストでの裏付けまで揃えた。Phase 1〜5 + audit round すべてで `cargo test --workspace --features title-tee/runtime-mock` の pass を維持 (workspace 合計 315 tests pass)。

監査エージェントは「重大バグ D-14 はテストでは絶対 catch できない設計だった」と指摘した。これを受けて contract test ヘルパ + 境界条件 + adversarial 入力テストを抜本的に強化した (§8 参照)。今後の `ContentSource` 実装は新規追加するたびに `content_stream::contract::assert_content_source_contract` に通すことで、同種のバグを構造的に防ぐ。
