# タスク3: C2PA検証（Core）

## 読むべきファイル

1. `docs/SPECS_JA.md` — §2.1「コンテンツの識別子」§2.2「来歴グラフの導出」のみ読む
2. `crates/core/src/lib.rs` — 現在のスタブ（3関数）
3. `crates/types/src/lib.rs` — `GraphNode`, `GraphLink`, `CorePayload` の型定義
4. `crates/crypto/src/lib.rs` — `content_hash_from_manifest_signature()`（実装済み）

## 作業内容

`crates/core/src/lib.rs` の3関数を実装する。

### 要件

#### verify_c2pa(content_bytes) → C2paVerificationResult

- `c2pa::Reader` でコンテンツバイト列を読み込む
- 署名チェーンを検証し、`ValidationStatus` を収集して返す
- Active Manifest（最新のManifest）を特定する

#### extract_content_hash(content_bytes) → String

- Active Manifest の署名を取得する
- `crypto::content_hash_from_manifest_signature()` で content_hash を算出
- `"0x"` プレフィックス付きhex文字列で返す

#### build_provenance_graph(content_bytes) → ProvenanceGraph

- Active Manifest から ingredient 情報を再帰的に抽出
- 各 ingredient の Manifest 署名から content_hash を算出
- `GraphNode`（id, type）と `GraphLink`（source, target, role）の DAG を構築
- ルートノードは `type: "final"`、素材は `type: "ingredient"`

### c2pa クレート v0.47 の API

`c2pa::Reader` を使用する。具体的なAPIは `c2pa` クレートのドキュメントを確認して実装すること。`Manifest` の `ingredients()` メソッドで素材情報を取得し、再帰的にグラフを構築する。

### テスト用フィクスチャ

`crates/core/tests/fixtures/` ディレクトリを作成し、C2PA付きテスト画像を用意する必要がある。

テスト画像の作成方法:
1. c2paクレートの `Builder` APIを使ってテスト内でC2PA付き画像を生成する（推奨）
2. または小さなC2PA付きサンプル画像をフィクスチャとして配置する

## 完了条件

- `cargo test -p title-core` が通る
- テスト: C2PA付きコンテンツに対して verify → content_hash算出 → グラフ構築の一連が動作
- テスト: C2PAなしコンテンツに対して適切な `CoreError` を返す
- テスト: ingredient付きコンテンツで来歴グラフに正しいノードとリンクが含まれる
- `docs/COVERAGE.md` の該当箇所を更新
