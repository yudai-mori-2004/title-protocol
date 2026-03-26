# Task 16: WASMモジュール再設計 — cert-* + image-pdq + 整理

## 目的

WASMモジュール群を実用的な構成に再設計する。

1. C2PA証明書チェーン検証モジュール（cert-*）を4ベンダー分新設
2. PDQ 256-bit知覚ハッシュモジュール（image-pdq）をMeta ThreatExchange互換で実装
3. 不要モジュールの削除とディレクトリ名の正規化

## 背景

### cert-*: 証明書検証の方針転換

既存の `hardware-google` はバイトパターン検索のスタブで、実際の証明書チェーン検証を行っていなかった。
設計議論の結果、以下の方針に決定:

- WASMバイナリにRoot CA SPKIをハードコード → `wasm_hash` が信頼ポリシーのコミットメント
- `cert-` プレフィックスで命名統一（検証メカニズムで分類、用途はcNFT結果のSubject文字列で判断）
- ホスト関数 `c2pa_verify_active_cert_chain` を拡張しJSON結果（chain subjects含む）を返すように変更

### image-pdq: pHashからPDQへの移行

pHash (64-bit) の課題:
- false positive率が高い（64bitハッシュ空間が狭い）
- 動画版（vPDQ）への拡張パスがない

PDQ (256-bit) のメリット:
- Meta本番実績（ThreatExchange）、BSDライセンス
- 256bitでfalse positive率が桁違いに低い
- vPDQでフレーム列に自然に拡張可能
- quality metricで低情報フレームをフィルタリング可能

### WASM整理

スタブのまま放置されていた3モジュールを削除し、ext_idとディレクトリ名を一致させた。

## 実施内容

### Phase 1: ホスト関数拡張

#### 1-1. `c2pa_verify_active_cert_chain` の結果拡張

`crates/wasm-host/src/c2pa_cert.rs`:
- `CertChainResult`, `CertSubject` 構造体を追加
- `verify_active_cert_chain_detailed()` — 検証成否 + x5chain内の各証明書Subject文字列を返す
- `get_content_feature` op のJSON出力を `{"verified":bool,"chain":[{"subject":"..."},...]}` に変更

`Cargo.toml`: `serde` 依存を追加（Serialize derive用）

#### 1-2. Jaroszダウンサンプルの実装

`crates/wasm-host/src/jarosz.rs` を新規作成:
- Meta ThreatExchange `downscaling.cpp` (BSD) のRust移植
- 4フェーズ `box_1d_float` — パディング不要の適応的境界処理
- ダブルバッファ方式の `jarosz_filter`（行→列を nreps=2 回）
- 中心サンプリング `decimate` — `(outi + 0.5) * in_dim / out_dim`
- `compute_window_size` — `(old + 2*new - 1) / (2*new)`

`grayscale_resize` op を変更:
- `image::FilterType::Triangle` による u8 リサイズを廃止
- f32 luminance パイプライン（BT.601、u8量子化なし）+ Jarosz + decimate に置換

### Phase 2: cert-* WASMモジュール (4モジュール新設)

各モジュールはRoot CA SPKIをハードコードし、ホスト関数 `c2pa_verify_active_cert_chain` を呼んで結果をフォーマットする。

| ext_id | Root CA | 鍵種 | SPKIソース |
|--------|---------|------|-----------|
| cert-google | Google C2PA Root CA G3 | P-384 | C2PA公式Trust List + pki.goog + ITL（3ソース一致確認済み） |
| cert-sony | SONY C2PA Root CA G2 | P-384 | C2PA Interim Trust List (ITL) |
| cert-leica | Leica C2PA Root CA | P-256 | C2PA Interim Trust List (ITL) |
| cert-rootlens | RootLens Dev Root CA | P-256 | root-lens/certs/dev/ |

cNFT結果フォーマット（全モジュール共通）:
```json
{
  "verified": true,
  "chain": [
    {"subject": "CN=Google Photos,OU=Google Photos Android,O=Google LLC,C=US"},
    {"subject": "CN=Google C2PA Mobile A 1P ICA G3 L3,O=Google LLC,C=US"}
  ],
  "root_ca": "Google C2PA Root CA G3",
  "root_spki_hash": "..."
}
```

### Phase 3: image-pdq WASMモジュール

`wasm/image-pdq/` — Meta ThreatExchange PDQ互換の256-bit知覚ハッシュ。

ホスト側（jarosz.rs）とWASM側の分担:
- ホスト: 画像デコード → f32 luminance → Jarosz 4パスボックスフィルタ → decimate → 64×64 u8
- WASM: DCT + Torben中央値量子化 + quality計算

C++リファレンスからの移植箇所:
| C++ソース | Rust実装 | 配置 |
|-----------|---------|------|
| `downscaling.cpp` — `box1DFloat`, `jaroszFilterFloat`, `decimateFloat`, `computeJaroszFilterWindowSize` | `jarosz.rs` | ホスト |
| `pdqhashing.cpp` — `dct64To16`, `pdqBuffer16x16ToBits`, `pdqImageDomainQualityMetric` | `image-pdq/src/lib.rs` | WASM |
| `torben.cpp` — `torben` | `image-pdq/src/lib.rs` | WASM |

Meta互換性検証結果（`pip install pdqhash` との比較）:
| テスト画像 | 距離 |
|-----------|------|
| pixel_photo_ramen.jpg | **0**（完全一致） |
| pixel_photo_plane.jpg | **2**（f32丸め差、アルゴリズム的限界） |

### Phase 4: WASM整理

#### 削除
- `wasm/c2pa-license-v1/` — スタブのまま未使用
- `wasm/c2pa-training-v1/` — スタブのまま未使用
- `wasm/hardware-google/` — cert-googleに置換

#### リネーム
- `wasm/phash-v1/` → `wasm/image-phash/`（ext_id `image-phash` と一致させる）
- Cargo.toml の `name` も `phash-v1` → `image-phash` に変更

#### 参照更新（全箇所）
- `.env.example` — `TRUSTED_EXTENSIONS`
- `deploy/local/setup.sh` — `WASM_TARGETS` + `TRUSTED_EXTENSIONS`
- `deploy/aws/setup-ec2.sh` — 同上
- `CLAUDE.md` — ビルドコマンド + アーキテクチャ表
- `crates/types/src/lib.rs` — docコメント + テスト
- `crates/cli/src/main.rs` — CLIヘルプ例
- `crates/cli/src/anchor.rs` — テスト
- `crates/tee/src/main.rs` — コメント
- `crates/tee/src/endpoints/verify/tests.rs` — テスト
- `crates/gateway/src/storage/mod.rs` — docコメント
- `crates/wasm-host/tests/phash_integration.rs` — パス

## 最終的なWASMモジュール構成

```
wasm/
├── image-phash/     ← pHash 64-bit（既存、deprecate予定）
├── image-pdq/       ← PDQ 256-bit（新規、image-phashの後継）
├── cert-google/     ← Google C2PA Root CA G3
├── cert-sony/       ← SONY C2PA Root CA G2
├── cert-leica/      ← Leica C2PA Root CA
└── cert-rootlens/   ← RootLens Root CA
```

## 完了条件

- [x] `c2pa_verify_active_cert_chain` がJSON結果（chain subjects含む）を返す
- [x] `jarosz.rs` — Meta `downscaling.cpp` のRust移植、f32パイプライン
- [x] `cert-google` — Pixel写真テスト3件パス
- [x] `cert-sony`, `cert-leica`, `cert-rootlens` — ビルド成功
- [x] `image-pdq` — Meta PDQリファレンスとの距離 ≤ 2
- [x] 不要モジュール削除（c2pa-license-v1, c2pa-training-v1, hardware-google）
- [x] `phash-v1` → `image-phash` リネーム + 全参照更新
- [x] `cargo check --workspace && cargo test --workspace` パス（全210テスト）
- [x] コメントをOSS品質に整理（元実装のライセンス・出典明記）

## 依存関係

- `crates/wasm-host`: `serde` 依存追加
- 既存テストフィクスチャ: `pixel_photo_plane.jpg`, `pixel_photo_ramen.jpg`（cert + PDQ両方で使用）
