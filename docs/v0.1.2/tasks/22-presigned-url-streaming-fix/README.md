# タスク22: presigned GET URL でも streaming 検証が動く probe 経路 (2026-05-31)

タスク 20 で完成した streaming verification は **公開 S3 URL** に対してのみ
正しく動いていた。RootLens iOS アプリから本番運用に入って初めて、
**R2 presigned GET URL** 経由のリクエストが TEE で 502 を返すことが判明:

```
TP /process 502: TEE upstream error (HTTP 502)
Content fetch failed: Memory limit:
Memory reservation would exceed total_limit (824659156 bytes request...
```

824 MB のリクエストが ResourcePool admission で弾かれている。実体は
「streaming 経路が成立しておらず in-memory full-fetch に fallback している」
ことが根本原因。本タスクで修正する。

## 完了基準 (Definition of Done)

- [x] R2 / S3 SigV4 presigned GET URL で TEE が streaming 検証を完走できる
- [x] proxy wire method の HEAD probe を撤廃、PROBE (= proxy 内部で
      `GET Range: bytes=0-0`) に置き換え
- [x] 200 fallback (= upstream が Range を無視) でも body を読まずに
      proxy が早期離脱して TEE 側に accepts_ranges=false を返す
- [x] POOL_TOTAL_LIMIT を Dockerfile で 2 GiB に明示し、MAX_CONTENT_BYTES と
      整合させる (defense in depth)
- [x] proxy + tee の unit test を緑にする
- [ ] PCR0 更新 → allowlist 再登録 → cNFT mint smoke (= 実機検証は別タスク)

## 観測した症状

- 端末: RootLens iOS app
- 状況: ~800 MB の家事撮影 mp4 を Pipeline 1 でアップロード
- 経路:
  1. iOS から R2 へ raw mp4 を直 PUT (= 別 presigned URL、成功)
  2. iOS → `POST /api/v1/tp-process` (RootLens web) →
     `presignSignedMp4Get` で **GET 用 presigned URL** を発行 →
     TP gateway `/process` へ POST
  3. TP gateway → TEE で content fetch 試行
- 結果: TEE が 502 + 上記メモリ超過エラー
- これに対して、タスク 20 STRESS_TEST は s3:// の **公開** URL で 4 GiB
  in-flight まで通っていた → presigned URL 特有の問題

## 根本原因 (2 段重ね)

### (1) Range probe (HEAD) が presigned GET URL に通らない

修正前: `crates/tee/src/proxy_fetcher.rs::ProxyRangeSource::probe` が、
proxy wire method "HEAD" を通じて HTTP HEAD を upstream に投げる設計だった。
proxy 側 `crates/proxy/src/handler.rs::handle_head` は素直に `client.head(url)` を
呼ぶ。

R2 / S3 の SigV4 presigned URL は **HTTP method が canonical signed string に
含まれる**。`GetObjectCommand` で発行された URL に HEAD を投げると
`SignatureDoesNotMatch` (403) で落ちる。

結果: `ProxyRangeSource::probe` が non-2xx を Err として返し、
`ProxyContentFetcher::fetch_streaming` が **InMemorySource に full body をロードする
fallback 経路**を取った。この InMemorySource の `peak_memory_hint` は body 全長
(= 824 MB) なので、`ticket.extend(824 MB)` で admission に弾かれた。

タスク 20 のストリーミング修正 (commit `15c308a`) は SafeRangeReader と
c2pa-rs fork の 2 段で per-reader peak を 16 MiB に圧縮していたが、
**そもそも Range 経路に入っていなかった**ためその効果はゼロだった。

### (2) POOL_TOTAL_LIMIT のデフォルト 512 MB が deploy 時に上書きされていなかった

`crates/tee/src/main.rs:142` の既定:
```rust
let total_limit: usize = std::env::var("POOL_TOTAL_LIMIT")
    .unwrap_or(512 * 1024 * 1024);
```

これに対し `deploy/aws/docker/tee-nitro.Dockerfile` は `MAX_CONTENT_BYTES=2 GiB`
を明示する一方で `POOL_TOTAL_LIMIT` を設定していなかった。
受け入れ上限 2 GiB と admission 上限 512 MB が分離した状態で本番稼働。
(1) が成立していれば隠れたままだが、(1) で fallback に落ちると即座に露呈する。

## 採用方針 — Option A: HEAD → Range GET probe

候補 3 案を比較した上で Option A を採用:

| 案 | 内容 | 採否 |
|---|---|---|
| A | proxy 内部の probe を `HTTP HEAD` から `GET Range: bytes=0-0` に切替 | ✓ 採用 |
| B | tp-process route 側で HEAD 用 presigned URL を別途発行して TP に渡す | ✗ API 互換性が崩れる、両側変更 |
| C | POOL_TOTAL_LIMIT を 2 GiB に揃えるだけ、streaming は諦める | ✗ streaming 検証の意義が消える |

**Range ヘッダは SigV4 の signed header に含まれない** (= 既定の signed headers
は `host` のみ)。`GET <presigned-url>` に `Range: bytes=0-0` を後付けしても
署名は通り、R2 / S3 は 206 + `Content-Range: bytes 0-0/<total>` を返す。
これだけ取れば HEAD と等価のメタデータ (全長 / ETag / Content-Type /
Range 対応の有無) が手に入る。

公開 S3 URL も同じ経路で 206 を返すので、タスク 20 STRESS_TEST の
URL 群もそのまま回る (= 退行なし)。

## 変更ファイル

### TEE side

| ファイル | 変更 |
|---|---|
| `crates/proxy/src/protocol.rs` | wire method 名 `HEAD` を `PROBE` に rename。`encode_head_response` → `encode_probe_response` 等の rename。`parse_content_range_total` helper を追加 (`bytes 0-0/12345` → `Some(12345)`)。 |
| `crates/proxy/src/handler.rs` | `handle_head` を撤廃、`handle_probe` を新設。`GET Range: bytes=0-0` を発行し、206 なら Content-Range から全長を抽出、200 なら body を消費せず drop で connection 解放して `accepts_ranges=false` を返す。 |
| `crates/tee/src/proxy_fetcher.rs` | wire method "HEAD" → "PROBE"、`decode_head_response` → `decode_probe_response`、エラーメッセージと doc comment 整理。 |
| `deploy/aws/docker/tee-nitro.Dockerfile` | `ENV POOL_TOTAL_LIMIT=2147483648` を追加。 |

### tests (緑)

- `protocol::tests::probe_response_*` (3 件) — encode/decode roundtrip
- `protocol::tests::content_range_total_*` (2 件) — Content-Range 分母抽出
- `main::tests::probe_returns_structured_response_on_206` — 206 経路
- `main::tests::probe_marks_range_unsupported_on_200` — 200 fallback
- `proxy_fetcher::tests::proxy_range_source_probe_succeeds` — 既存テスト、PROBE wire name に追従

## proxy wire 互換性

PROBE は内部 wire method 名であり、TEE バイナリと proxy バイナリは同じ
Dockerfile から build されて同じ EC2 上に常駐する (= rolling deploy しないので
旧 TEE が新 proxy と話す状況は発生しない)。HEAD wire method を破壊的に
撤廃して問題ない。

## デプロイ手順 (= 別タスクで実行)

PCR0 が変わる (= TEE バイナリ変更) ので:

1. `cd deploy/aws/docker && ./build.sh --verify` で新 PCR0 を 2 回連続一致で確認
2. `OPERATIONS_JA.md §4.1` の `prover-run.sh` を回して新 PCR0 用の SP1 proof を生成
   (= ユーザー方針: 「実運用開始時に新 PCR0 用 proof を allowlist に入れる」)
3. `title-cli add-measurement <新 PCR0>` で whitelist 更新
4. `title-cli register-key --bundle <prover 出力>` で新 TEE の鍵を登録
5. RootLens app から再アップロードして 200 OK + cNFT mint を確認

## 既知の周辺問題 (本タスク外)

- `crates/tee/src/range_source.rs::HttpRangeSource` (= direct HTTP モード、
  dev 用) は今もって HEAD で probe している。本番経路ではないので影響なし。
  将来 dev で presigned URL を扱いたくなったら同じ修正を入れる。
- HttpContentFetcher (= proxy なしの直 HTTPS fetch) の fallback も
  in-memory full body。これも dev 用なので放置。

## 関連タスク

- タスク 20: streaming verification 修正 (SafeRangeReader 4 MiB clamp +
  c2pa-rs MAX_HASH_BUF 256 → 4 MiB)。本タスクで初めて実運用 RootLens 経路で
  生きるようになる
- タスク 21: silent enclave death 検知 + 自動再起動。本タスクと独立、
  v0.1.3 以降で着手予定
