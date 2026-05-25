# Task 18: rootlens-license-v1 Processor

## 目的

RootLens の Root NFT 発行パイプラインが必要とする `rootlens-license-v1` processor を実装する。

このprocessorは、コンテンツのSHA-256ハッシュと、RootLensのサブライセンス枠組みに関するメタデータ（`rootlens_binding`）をTEE署名で結合する。第三者は Root NFT の TEE 署名付き出力から、「このコンテンツは RootLens フレームワーク下で処理された」ことを暗号学的に検証できる。

法的根拠と設計判断の詳細は [`legal-basis.md`](legal-basis.md) を参照。

## 読むべきファイル

1. `CLAUDE.md`
2. `docs/v0.1.2/SPECS_JA.md` §3.1 (Processor規約), §3.2 (processor一覧)
3. `docs/v0.1.2/COVERAGE.md`
4. `crates/core/src/processor.rs` — Processor trait
5. `crates/core/src/c2pa_verify.rs` — 既存 processor 実装パターン
6. 本タスクの `legal-basis.md` — 法的根拠文書
7. `/Users/forest/WebCreations/root-lens/document/v0.1.2/SPECS_JA.md` §3.4 (rootlens-license-v1 要件)
8. `/Users/forest/WebCreations/root-lens/document/v0.1.2/legal-rationale.md` §2.7 (TDM), §2.2 (ToS)

## 前提知識

### TEE が証明するもの (技術的・暗号的)

- `content_hash`: コンテンツのSHA-256。TEE署名と結合されることで、特定コンテンツの同一性を暗号学的に証明
- C2PA署名の有効性: TEE内でc2pa-rsが検証した結果
- CAWG `cawg.training-mining` assertion の存在: コンテンツにTDM opt-out信号が含まれていたこと
- `rootlens_binding`: コンテンツがRootLensフレームワーク下で処理されたことを示すメタデータ

### TEE が証明しないもの (法的レイヤーで補完)

- **ToS同意**: アプリ側の同意フロー強制 + consent log APIで証明。Root NFTのオンチェーン存在自体が「同意済みパイプラインを通過した」間接的証拠
- **著作権の帰属**: KYC + 利用規約の表明保証
- **第三者IP非含有**: VLM gate + 撮影者の表明保証

この分離の法的根拠は `legal-basis.md` §2 で詳述。

## スコープ

### やること

1. **processor 実装** (`crates/core/src/rootlens_license_v1.rs`)
   - `Processor` trait を実装する `RootLensLicenseV1Processor`
   - ID: `"rootlens-license-v1"`
   - 入力: content バイト列 + content_type
   - 出力: `{ "content_hash": "0x...", "rootlens_binding": { ... } }`

2. **CAWG training-mining assertion のゲート検証**
   - c2pa-rs `Reader` でコンテンツのC2PAマニフェストを読み取り
   - `cawg.training-mining` assertion (CAWG v1.1, label: `"cawg.training-mining"`) の存在を確認
   - 不在の場合は `ProcessorError` を返す (Root NFTの発行を阻止)
   - assertion の `entries` 内容は出力に含めない (ゲートとしてのみ機能)

3. **rootlens_binding の定数出力**
   - 全フィールドをソースコードに `const` でハードコード
   - ToS文書・binding rule文書が完成した時点で `tos_hash`, `tos_url`, `binding_rule_hash`, `binding_rule_url` を追加 (TEE再ビルド → PCR0更新)

4. **ProcessorRegistry への登録**
   - `crates/tee/src/main.rs` で `registry.register(Box::new(RootLensLicenseV1Processor::new()))`

5. **単体テスト**
   - TDM assertion を含むコンテンツ → 正常出力
   - TDM assertion を含まないコンテンツ → エラー
   - C2PA manifest が無いコンテンツ → エラー

### やらないこと

- `Processor` trait の変更 (extension_inputs の追加等)
- `creator_wallet` の出力 (cNFT owner として on-chain に記録されるため不要)
- devnet / EC2 への deploy (Task 17 で AMI pin + program redeploy 済。PCR0 変更を含む deploy は別途)
- ToS フルテキスト文書の作成 (root-lens Task 14 のスコープ)
- binding rule 文書の作成 (root-lens Task 14 のスコープ)

## 出力スキーマ

C2PA 検証 (c2pa-verify 同等) + CAWG TDM ゲート + ライセンスバインディングを一体で出力。
RootLens は `processor_ids: ["rootlens-license-v1"]` だけ指定すれば Root NFT 発行に必要な全属性が得られる。
コンテンツ識別は `ProcessResponse.signature_hash` に委譲。

```json
{
  "c2pa": {
    "validation": "valid",
    "signer": { "issuer": "...", "cert_serial": "..." },
    "timestamp": "2026-05-24T00:00:00Z",
    "claim_generator": "...",
    "actions": [{ "action": "c2pa.created" }]
  },
  "rootlens_binding": {
    "binding_protocol_version": "rootlens-license-v1",
    "purpose": "sublicense-grant-eligibility",
    "license_program_id": "G1PWd1nMe63isDaYT3iijcyWac9d4RE1CBrvaKZFjpV8",
    "license_collection_mint": "BvhuJiTWDW6n5cSzE4XmzYcwLry7vcstS1U7fD7n9N1b",
    "license_nft_terms_url_template": "https://rootlens.io/licenses/{type}/{terms_hash}.json",
    "tos_version": "v1.0.0",
    "tos_consent_log_endpoint": "https://www.rootlens.io/api/v1/tos/consent"
  }
}
```

### 将来追加されるフィールド (ToS・binding rule 文書完成後)

- `tos_hash`: `"0x<SHA-256 of ToS text>"`
- `tos_url`: `"https://rootlens.io/tos/v1.0.0/<tos_hash>.txt"`
- `binding_rule_hash`: `"0x<SHA-256 of binding rule JSON>"`
- `binding_rule_url`: `"https://rootlens.io/extensions/rootlens-license-v1/<rule_hash>.json"`

追加時は TEE 再ビルド → 新 PCR0 を Solana に `add_approved_measurement` で登録。

## 定数値の出典

| 定数 | 値 | 出典 |
|---|---|---|
| `binding_protocol_version` | `"rootlens-license-v1"` | root-lens SPECS §3.4 |
| `purpose` | `"sublicense-grant-eligibility"` | root-lens SPECS §3.4.4 |
| `license_program_id` | `G1PWd1nMe63isDaYT3iijcyWac9d4RE1CBrvaKZFjpV8` | root-lens Anchor.toml (devnet deployed) |
| `license_collection_mint` | `BvhuJiTWDW6n5cSzE4XmzYcwLry7vcstS1U7fD7n9N1b` | root-lens keys/license-collection.json |
| `license_nft_terms_url_template` | `https://rootlens.io/licenses/{type}/{terms_hash}.json` | root-lens SPECS §5.5.3 Layer 2 |
| `tos_version` | `"v1.0.0"` | root-lens SPECS §4.4.2 |
| `tos_consent_log_endpoint` | `https://www.rootlens.io/api/v1/tos/consent` | root-lens SPECS §4.4.6 step 5 |

## 影響範囲

- 新ファイル 1 個 (`crates/core/src/rootlens_license_v1.rs`)
- `crates/core/src/lib.rs` にモジュール追加 + re-export
- `crates/tee/src/main.rs` に registry 登録 1 行追加
- TEE バイナリの PCR0 が変わる (新 processor のコードが追加されるため)

## 完了の定義

- `cargo test --workspace` がパス
- `cargo clippy --workspace` が警告なし
- processor が正しい JSON スキーマを出力する
- TDM assertion ゲートが機能する (不在時にエラー)
- `legal-basis.md` が法務レビュー可能な状態
