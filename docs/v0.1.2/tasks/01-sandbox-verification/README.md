# タスク01: サンドボックス技術検証

## 目的

v0.1.2 仕様の実装に入る前に、技術的不確実性が高い3領域を独立したサンドボックスで検証する。
各サンドボックスのゴールは「動くか動かないか」の確認であり、本実装品質のコードを書くことではない。

## 読むべきファイル

1. `docs/v0.1.2/SPECS_JA.md` — 全文（1177行）
2. `legacy/v0.1.0/crates/core/src/lib.rs` — v0.1.0 の c2pa-rs 統合パターン（参考）
3. `legacy/v0.1.0/crates/crypto/src/attestation/nitro.rs` — Nitro Attestation Document パース（参考）

## 作業ディレクトリ

```
sandbox/
├── 01-c2pa-range-request/    ← Sandbox A
├── 02-c2pa-fragment/         ← Sandbox B
└── 03-sp1-attestation/       ← Sandbox C
```

各サンドボックスは独立した Cargo プロジェクト（`cargo init`）とする。ワークスペースには含めない。

---

## Sandbox A: c2pa-rs HTTP Range Request 検証

### 仕様書の該当箇所

- §4.3「単一ファイル — Range Requestパターン」
- §5.2「コンテンツ取得の詳細 — 単一ファイル」

### 検証内容

c2pa-rs の Reader が HTTP Range Request 経由で大容量ファイルを検証できるか確認する。

**背景**: c2pa-rs の Reader は `Read + Seek` を要求する。ファイル全体をメモリに載せずに処理するには、Seek を HTTP Range Request に変換するアダプタが必要。`http-range-client` クレートがこの用途に使える可能性がある。

### 手順

1. C2PA署名付きの MP4 ファイル（10MB以上）をHTTPで配信可能な場所に置く（ローカルHTTPサーバーでよい）
2. `http-range-client` の `HttpReader`（または同等のアダプタ）を `Read + Seek` として c2pa-rs の `Reader` に渡す
3. マニフェスト読み取り、署名検証、signature_hash 算出が成功するか確認する
4. 実際のHTTPリクエスト数・転送量を記録し、ファイル全体ダウンロードと比較する

### 成功基準

- [x] c2pa-rs Reader が Range Request 経由で C2PA 検証を完了する
- [x] ファイル全体をメモリに載せていないことを確認（メモリ使用量 or 転送量）
- [x] ETag / If-Match による整合性チェックの実現方法を確認

### 想定される問題

- Seek パターンがランダムすぎて HTTP リクエスト数が爆発する
- `http-range-client` がリクエスト粒度の最適化に不十分
- サーバーが Range Request 非対応の場合のフォールバック

### 使用クレート

- `c2pa` = "0.84" （最新安定版。v0.1.0 は v0.78 だった）
- `http-range-client` または同等品
- `actix-web` or `axum`（ローカルHTTPサーバー、テスト用）

---

## Sandbox B: c2pa-rs CMAF フラグメント検証

### 仕様書の該当箇所

- §1.3「入力形式 — フラグメント」
- §4.3「フラグメント」

### 検証内容

c2pa-rs の `with_fragment` API で init.mp4 + seg-*.m4s のフラグメント検証ができるか確認する。

**背景**: c2pa-rs v0.77 以降、`Reader::with_fragment(format, init_stream, fragment_stream)` API がある。C2PA v2.3 のストリーミング署名（Merkle tree ベース）に対応しているとされるが、実際に動作するか未検証。

### 手順

1. C2PA署名付きの CMAF フラグメントセット（init.mp4 + 複数の seg-*.m4s）を生成する
   - `c2patool` の `fragment` サブコマンドで署名
   - または ffmpeg で CMAF 出力 → c2patool で署名
2. c2pa-rs の `with_fragment` API で init + 各フラグメントを順に検証する
3. 1フラグメントずつ処理・解放するパターン（§4.3のメモリパターン）が実現できるか確認する
4. 全フラグメントを渡さずに、一部だけの検証が成功するか確認する

### 成功基準

- [x] init.mp4 + seg-*.m4s の署名・検証ラウンドトリップが成功する
- [x] フラグメントを1つずつ渡して逐次検証できる
- [x] 検証後にフラグメントデータを解放できる（メモリパターンの確認）

### 想定される問題

- CMAF フラグメントの生成方法（c2patool のフラグメント署名の実際の動作）
- c2pa-rs の `with_fragment` がフラグメント順序に依存するか
- Merkle tree 検証のインクリメンタル性（全フラグメント必要 vs 部分検証可能）

### 使用クレート

- `c2pa` = "0.84"

### 外部ツール

- `c2patool` CLI（フラグメント署名の生成）
- `ffmpeg`（CMAF セグメント生成）

---

## Sandbox C: SP1 zkVM Attestation Document 検証

### 仕様書の該当箇所

- §6.2「Solana Extension — 準備（TEEインスタンスごとに一度）」

### 検証内容

SP1 zkVM で AWS Nitro Attestation Document の証明書チェーンを検証し、ZK proof を生成できるか確認する。最終目標は Solana 上での proof 検証だが、このサンドボックスでは proof 生成とローカル検証までを対象とする。

**背景**: SP1 v6 "Hypercube" が現行版。`sp1-solana` クレートで Solana 上の Groth16 proof 検証が可能。ただし `sp1-solana` の公開バージョンは SP1 v5 までの verification key しか含んでいない可能性がある。Automata Network の `aws-nitro-enclave-attestation` が同様のユースケース（SP1 guest で Nitro Attestation 検証）を実装済み。

### 手順

1. SP1 の開発環境をセットアップする（`sp1up` でツールチェーンインストール）
2. Automata の `aws-nitro-enclave-attestation` を参考に、SP1 guest プログラムを作成する
   - COSE_Sign1 パース
   - X.509 証明書チェーン検証（AWS Root CA → 中間 → エンクレーブ証明書）
   - PCR 値の照合
   - user_data（公開鍵ハッシュ）の抽出
3. テスト用の Attestation Document を用意する（モック or 実際の Nitro 出力）
4. SP1 でローカル proof 生成を実行し、成功するか確認する
5. proof サイズ、生成時間、サイクル数を記録する
6. 可能であれば `sp1-solana` でのローカル検証も試す

### 成功基準

- [x] SP1 guest 内で Attestation Document の証明書チェーン検証が完了する
- [x] ZK proof が生成される
- [x] proof サイズが Solana トランザクションサイズ制限（1,232B）に収まる見込みがある
- [x] 生成時間が実用的である（TEE インスタンス起動時の1回限りなので、数分以内なら許容）

### 想定される問題

- **SP1 v6 と sp1-solana の互換性**: sp1-solana が v6 の verification key を含んでいない場合、v5 を使うか sp1-solana を自前でビルドする必要がある
- **P-384 の性能**: AWS Nitro は ECDSA P-384 を使用。SP1 に P-384 precompile はなく、ソフトウェアエミュレーション。~300M サイクル（Automata の実績値）。proof 生成に数分かかる可能性
- **テスト用 Attestation Document の入手**: 実際の Nitro Enclave がないとリアルな Attestation Document が取れない。モック or Automata のテストフィクスチャを使う
- **Solana トランザクションサイズ**: Groth16 proof (260B) + public inputs が 1,232B に収まるか

### 使用クレート / ツール

- `sp1-sdk`（ホスト側）
- `sp1-zkvm`（ゲスト側）
- `sp1-solana`（Solana 検証、可能であれば）
- `aws-nitro-enclave-attestation`（参考実装）
- `coset`（COSE パース）
- `x509-cert`（証明書チェーン検証）

---

## 完了条件

3つのサンドボックス全てについて:

1. 検証結果（成功 / 失敗 / 条件付き成功）を記録する
2. 発見した制約・注意点を記録する
3. 本実装に向けた推奨事項を記録する

結果は `docs/v0.1.2/tasks/01-sandbox-verification/RESULTS.md` にまとめる。

## 実装時の注意

- **legacy を積極的に参照すること**: `legacy/v0.1.0/` にはビルドが通る完全な実装がある。Cargo.toml の依存関係の書き方、c2pa-rs の API の呼び方、Attestation Document のパース、テストフィクスチャの生成方法など、迷ったらまず legacy を見る。特に以下:
  - c2pa-rs の使い方: `legacy/v0.1.0/crates/core/src/lib.rs`
  - Attestation Document パース: `legacy/v0.1.0/crates/crypto/src/attestation/nitro.rs`
  - COSE / CBOR: `legacy/v0.1.0/crates/crypto/src/attestation/mod.rs`
  - C2PA テストフィクスチャ生成: `legacy/v0.1.0/crates/core/examples/gen_c2pa_fixtures.rs`
  - テスト用の署名済みファイル: `legacy/v0.1.0/tests/fixtures/c2pa/signed/`
  - Cargo workspace 設定の参考: `legacy/v0.1.0/Cargo.toml`
- ただし legacy は v0.78 の c2pa API（`Reader::from_stream()` 等）を使っている。v0.84 ではビルダーパターン（`Reader::default().with_stream()`）に変わっているので、API の呼び方はそのままコピーせず、パターンだけ参考にする
- 各サンドボックスは独立した `cargo init` プロジェクトで、ワークスペースに含めない。依存関係の試行錯誤がワークスペース全体に影響しないようにする

## 備考

- 各サンドボックスは独立しており、並列に進められる
- ただし1タスク1セッションの原則上、1セッションで1サンドボックスを完了させるのが現実的
- A → B → C の順で進めることを推奨（A, B は c2pa-rs のバージョン・API感覚が共通、C は完全に別スタック）
