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
| `fragmented` | `init + (in-memory 常駐合計) + (active Range バッファ最大)` | `FragmentedSource` が init を常駐、fragments を 1 個ずつ lazy load。`fetch_fragmented` は probe loop 内で fragment ごとに `ticket.extend` を漸進呼び、最後に `peak_memory_hint` 確定値まで `shrink` で正味化 (legacy v0.1.0 verify handler の漸進予約パターンと同思想) |

仕様 §4.3 の「Range Request パターン」: マニフェスト + チャンク 1 個分のピーク。c2pa-rs の `Reader::with_stream(content_type, &mut content)` は box header を seek で辿ってハッシュ検証もチャンク単位で行うため、Range Request reader を直接流せる。

仕様 §4.3 の「フラグメントパターン」: 初期化セグメントを読み込んだ後、メディアセグメントを 1 つずつ処理して解放する。`FragmentedReader` が `Read + Seek` を実装して連結ストリーム view を提供しつつ、内部で fragment 1 個のみ in-memory に保持。`fetch_fragmented` の probe loop で **漸進予約** (各 fragment 取得時点で ticket.extend) することで、admission gate は loop 中の中間状態を見て並列リクエストを throttle できる。

`ContentSource::is_in_memory_resident()` で source の常駐性を明示判別 (default heuristic は `peak >= size`、各 impl で override 推奨)。`peak_memory_hint` は in-memory 常駐 fragments の合計 + Range fragments の active 1 reader 分 (max) を分けて計算するため、Range 非対応サーバーで全 fragments が in-memory fallback しても実 RAM を正確に申告 (D-13 対策)。

監査 round1/2 の `should-fix-001` (フラグメント全 concat) と `should-fix-002` (漸進予約が事後カウンタ化) は **両方とも完全解消**。round 3 で発見された loop 中 admission bypass / Seek 規約違反 / heuristic 脆さも漸進予約モード + `is_in_memory_resident` API で構造的に解決。

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

`HttpRangeSource::open()` は `http_range_client::HttpReader`(= `SyncBufferedHttpRangeClient<reqwest::blocking::Client>`) を新規構築する。c2pa-rs が期待する `Read + Seek` を実装。`min_req_size` を 4 MiB に設定し、c2pa-rs の box header スキャンが過剰な小 Range Request を発行しないように緩衝する。

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
- Range Request ソース: reader バッファサイズのみ (4 MiB)

`size_hint` (論理サイズ) は `compute_global_timeout` 用に保持。50 GB ファイルでも `peak_memory_hint = 4 MiB` で admission を通過、`total_limit` をほぼ使わずに完走できる。テスト `fetch_single_streaming_source_reserves_only_peak_memory` で検証。

EOF 処理: `http_range_client::HttpReader` の Read impl は HTTP 416 を `ErrorKind::UnexpectedEof` で返す (Rust の `Ok(0)` 規約から外れる)。`SafeRangeReader` (HttpRangeSource / ProxyRangeSource の両方で共有) で `Ok(0)` に正規化し、`read_to_end` 等の標準 API と互換にした。

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

**workspace 合計 340 tests pass**。テスト内訳: 初期実装 + Round 1 監査強化 (contract suite + 境界条件 + adversarial) + 3-input streaming 拡張 (FragmentedSource + e2e c2pa-verify) + Round 4 漸進予約モード追加分 (Range fragment peak / i64::MIN 境界 / D-13 adversarial)。

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

### 3-input streaming round 追加分 (Fragmented + Sidecar Range + e2e c2pa-verify)

- `fragmented_source::tests::*` — `FragmentedSource` / `FragmentedReader` の網羅テスト:
  - `size_hint_equals_init_plus_all_fragments` / `peak_memory_hint_with_in_memory_fragments_sums_all_sizes` — メモリ会計の宣言値検証
  - **`peak_memory_hint_with_range_fragments_takes_active_max`** / **`peak_memory_hint_mixed_in_memory_and_range_separates_accounting`** / **`peak_memory_hint_range_source_with_buf_equal_to_size_not_treated_as_resident`** / **`peak_memory_hint_two_range_sources_with_buf_eq_size_take_max_not_sum`** — Mock Range Source で `is_in_memory_resident = false` 経路の peak 計算を独立検証 (Round 3 監査 観点 1/2/16 対策)
  - `read_to_end_returns_concatenated_bytes` / `seek_into_middle_fragment_returns_correct_bytes` / `read_at_eof_returns_zero` — Read+Seek 基本動作
  - **`seek_to_negative_position_returns_invalid_input` / `seek_overflow_returns_invalid_input` / `seek_with_i64_min_offset_handles_boundary`** — `add_signed_offset` ヘルパによる `std::io::Cursor::seek` 互換 (Round 2 監査 B-7 対策、i64::MIN 境界を含む)
  - `fragment_swap_drops_previous_fragment_bytes` / `multiple_readers_load_fragments_independently` — fragment swap + 並列 reader の独立性
  - `fragmented_source_contract_basic` / `_uneven_fragments` / `_init_only_contract` / `_many_small_fragments` — 共通 contract suite を fragmented backend に適用 (D-14 系の read 規約準拠を含む)
  - **`c2pa_verify_via_fragmented_source_init_only` / `_split_into_segments` / `_three_segments`** — 実 C2PA 署名付き JPEG を任意位置で split し FragmentedSource 経由で `c2pa::Reader::with_stream` に流して parse 成功することを e2e 検証。c2pa-rs が FragmentedReader の連結 view を JPEG として正しく扱う互換性を実 C2PA データで保証
- `content_fetch::tests::fetch_fragmented_concatenates_segments` / `_memory_limit_mid_fetch` — 漸進予約モードに合わせて assertion 更新 (probe loop 内 extend → 最終 peak shrink)
- **`content_fetch::tests::fetch_fragmented_adversarial_in_memory_fragments_reject_mid_loop`** — 大量 in-memory fragments で loop 中の N 個目で reject されることを実証 (Round 3 監査 観点 6/9 = D-13 残存問題対策の reproduce)
- `content_fetch::tests::fetch_sidecar_*` — content fetch が `fetch_streaming` 経路を通ることを確認
- `resource_pool::tests` — `Ticket` が `AtomicU64` ベースに移行 (Sync 化) しても全 timeout/threshold テストが pass

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

2. **in-memory fallback fragment の `Vec::with_capacity + read_to_end` で 1 fragment 分追加メモリ**

   `FragmentedReader::ensure_fragment_loaded` で in-memory source (Range 非対応 fallback の `InMemorySource` 等) を読むとき、`entry.source.open()` が `Cursor<Arc<[u8]>>` を返し、`read_to_end(&mut bytes)` で別の `Vec<u8>` にバイト列をコピーする。`Arc<[u8]>` と `Vec<u8>` が並列存在する瞬間ピークが `peak_memory_hint` の申告値より 1 fragment 分多い。

   `peak_memory_hint` には織り込まれていないため、admission gate が実態より 1 fragment 分緩い (= 並列 N リクエストで N × max_fragment 分の余剰 RAM)。修正案は `ContentSource::as_arc_bytes() -> Option<Arc<[u8]>>` のような fast-path を trait に追加し、`FragmentedReader` 側で in-memory のときは Vec コピーを省く。

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

### 7.5 Round 2 / Round 3 監査結果 (3-input streaming 完走後の精査)

3-input streaming 実装直後に Round 2 監査、それを受けた静的予約モード移行後に Round 3 監査、それを受けた漸進予約モード移行後に Round 4 監査 (= 本 task 完了確認用) を回した。各 round の主要発見と処理:

#### Round 2 主要発見 (動的 ticket shrink モード時点)

- **D-13 (重大)**: `fetch_fragmented` で各 fragment が `Arc<Ticket>` 共有経由で動的 extend/shrink するが、in-memory fallback (非 Range サーバー) のとき `ticket.extend(init)` のみ申告で残りの fragment が pool.used に出ない経路あり (admission bypass)。
- **B-7 (修正必要)**: `FragmentedReader::seek` が負位置を silent に u64 wrap (例: -1 → u64::MAX) する。`std::io::Cursor::seek` 規約は `InvalidInput` で reject すべき。
- **should-fix-001/002** (admission control 形骸化、reader 状態整合性問題)

→ Round 3 で **動的 shrink モード撤回 → 静的予約一本化** で 4 件まとめて消す方向に倒した (legacy v0.1.0 の動的パターンは参考にしたが、fragmented では admission gate 精度を優先)。`B-7` は `add_signed_offset` ヘルパで `std::io::Cursor::seek` 互換に修正。

#### Round 3 主要発見 (静的予約モード移行後の精査)

- **観点 6/9 (重大)**: 静的予約モードでも `fetch_fragmented` の probe loop 中に in-memory fragments が `Arc<[u8]>` で確保される間、`ticket.extend` は loop 完了後の 1 回だけ。100 並列 × 100 fragments × 100 MB のシナリオで 10 GB が ticket 未申告でメモリに乗る経路 (D-13 が形を変えて残存)。
- **観点 1/2 (懸念)**: `peak_memory_hint` の `peak >= size` heuristic が Range fragment の `min_req_size == size` で false-positive。
- **観点 3 (要修正)**: `resource_pool.rs` Ticket doc が動的モード撤回後も「FragmentedReader が shrink を呼ぶ」と架空ユースケース言及。
- **observ 13/15/16 (テスト欠落)**: `peak < size` 経路、i64::MIN 境界、D-13 adversarial の単体テストなし。

→ Round 4 で **漸進予約モード移行 + `is_in_memory_resident()` API 追加 + 6 件のテスト追加 + doc 全面同期** で全件解消。

#### Round 4 (本 task 完了確認)

漸進予約モード + `is_in_memory_resident()` 明示 API + 関連テスト 6 件追加 + README/COVERAGE/Ticket doc 同期。Round 3 の指摘事項を全件解消し、新たな発見ゼロが目標 (= 監査を 1 周しても指摘が出ないレベルまで仕上げる)。

#### 設計の収束方向 (Round 1 → 2 → 3 → 4)

| Round | アプローチ | 何が良くて何が悪かったか |
|---|---|---|
| 1 | 全 concat | シンプルだが 50 GB で破綻 |
| 2 | 動的 Arc<Ticket> shrink | 仕様 §4.3 擬似コードに忠実だが (a) admission bypass + (b) 状態整合性 + (c) reader 状態破壊 の 3 大問題 |
| 3 | 静的 peak 1 回 extend | 動的の 3 大問題を消すが loop 中の bypass が残存 (D-13) |
| 4 | 漸進予約 (legacy 流儀) + `is_in_memory_resident()` 明示 API | loop 中の bypass を構造的解消、heuristic 脆さも明示 API で消去 |

「仕様 §4.3 擬似コードは結果不等式の例示であって規範ではない」という読み方の成熟と、「legacy v0.1.0 の漸進予約パターンが正解 (admission gate に loop 中の状態を伝える)」という発見の両方が Round 4 で揃った。

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

## 引き継ぎ事項

ユーザーから「50 GB が明日来るので v0.1.3 では遅い、今必要」と明示の指摘を受けて立てた本 task は、**全 3 入力タイプ (single / sidecar / fragmented) の streaming 化** + 仕様 §4.3 のメモリパターン要件達成まで完走した。

3-input streaming round で動的 ticket shrink モードを試みたが Round 2 監査で admission bypass / reader 整合性問題が浮上 → 静的予約モード一本化 → Round 3 監査で loop 中 admission bypass (D-13) の残存が発覚 → **漸進予約モード (legacy v0.1.0 verify handler パターン) + `is_in_memory_resident()` 明示 API** に最終収束。`fetch_fragmented` は probe loop 内で各 fragment を fetch_streaming した直後に `ticket.extend` し、loop 完了時に `peak_memory_hint` 確定値まで `shrink` で正味化する。これにより admission gate は loop 中の任意の時点で正確な pool.used を見て並列リクエストを throttle できる。

monkey-patching 的な対応 (本番 fetch だけ修正、テスト/COVERAGE 反映なし) を避け、(a) trait の streaming 化、(b) 監査 finding の処理ログ更新、(c) COVERAGE.md 反映、(d) テストでの裏付けまで揃えた。Phase 1〜5 + audit round 1〜3 + 3-input round + Round 4 漸進予約への振り戻しすべてで `cargo test --workspace --features title-tee/runtime-mock` の pass を維持。

監査エージェントは Round 1 で「重大バグ D-14 はテストでは絶対 catch できない設計だった」と指摘した。これを受けて contract test ヘルパ + 境界条件 + adversarial 入力テストを抜本的に強化し、Round 3 監査の D-13 / 観点 1 (heuristic 脆さ) / 観点 13 (i64::MIN 境界) も Round 4 で reproduce → green の adversarial test として追加した (§4 参照)。今後の `ContentSource` 実装は新規追加するたびに `content_stream::contract::assert_content_source_contract` に通し、`is_in_memory_resident()` を明示 override することで、同種のバグを構造的に防ぐ。
