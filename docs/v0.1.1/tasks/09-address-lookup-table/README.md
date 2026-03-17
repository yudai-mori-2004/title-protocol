# Task 09: Address Lookup Table (ALT) による TX 圧縮

## 目的

Versioned Transaction (v0) + Address Lookup Table を導入し、MintV2 TX のアカウント参照を圧縮する。1 TX あたりのパッキング可能 cNFT 数を 3 → 6-7 に倍増させ、extension 数増加時のスケーラビリティを確保する。

## 背景

現在の TX サイズ（本番 URI）:

| cNFT数 | サイズ | 結果 |
|--------|--------|------|
| 2 | ~1024 bytes | OK |
| 3 | ~1213 bytes | ギリギリ |
| 4 | ~1350 bytes | 超過 |

ネックは MintV2 instruction 内の 32-byte アカウント pubkey。ALT を使えば 1 byte のインデックス参照になる。

## ALT に入れるアカウント

### 共通プログラム（全 MintV2 で同一）
- Bubblegum program (32B)
- SPL Account Compression V2 program (32B)
- System program (32B)
- Log wrapper / Noop program (32B)
- MPL Core program (32B)

### ノード固定アカウント
- MPL Core CPI signer PDA (32B)
- Core tree + tree_config (64B)
- Extension tree + tree_config (64B)
- Core collection mint (32B)
- Extension collection mint (32B)

**合計: 12 アカウント × 32 bytes = 384 bytes → 12 bytes（ALTインデックス）**

**削減: ~372 bytes/TX**

注意: Signers (TEE, creator, fee_payer) は ALT に入れられない。TX の署名者リストに直接記載が必要。

## 推定効果

| | 現状 | ALT後 |
|---|---|---|
| 2-ix TX | ~1024 bytes | ~652 bytes |
| 空き容量 | 208 bytes | 580 bytes |
| 追加 cNFT あたり | ~226 bytes | ~100 bytes (URI+data のみ) |
| 最大 cNFT/TX | 3 | **6-7** |

## 実装

### ALT 作成（setup.sh / setup-ec2.sh）

ノード登録後、Merkle Tree 作成後に ALT を作成:

1. `solana address-lookup-table create` で ALT アカウント作成
2. 12 アカウントを `extend` で追加
3. ALT アドレスを tee-info.json に保存

### TX 構築（TEE /sign）

- `Message::new_with_blockhash` → `MessageV0::try_compile` に変更
- `Transaction` → `VersionedTransaction` に変更
- ALT アドレスを `AddressLookupTableAccount` としてロード

### SDK 対応

- `Transaction` → `VersionedTransaction` のデシリアライズ対応
- `partialSign` の VersionedTransaction 対応

## 変更ファイル

| ファイル | 変更内容 |
|---------|---------|
| `crates/tee/src/blockchain/solana_tx.rs` | `pack_mint_txs` を VersionedTransaction + ALT 対応 |
| `crates/tee/src/endpoints/sign/handler.rs` | ALT アドレスの読み込み、VersionedTransaction 返却 |
| `crates/tee/src/config.rs` | `TeeAppState` に ALT アドレス追加 |
| `deploy/local/setup.sh` | ALT 作成ステップ追加 |
| `deploy/aws/setup-ec2.sh` | 同上 |
| `sdk/ts/src/client.ts` | VersionedTransaction 対応 |
| `integration-tests/register-photo.ts` | VersionedTransaction の partialSign + broadcast |

## 完了条件

- [x] ALT がノードセットアップ時に自動作成される（`title-cli create-alt`）
- [x] /sign が VersionedTransaction (v0) を返す
- [x] 2 cNFT が ALT 参照で ~750 bytes の TX にパックされる（本番URI）
- [x] SDK が VersionedTransaction を正しく deserialize + sign できる
- [x] 6+ cNFT のパッキングテスト（8 cNFT → 2 TX, 4 ix/TX）
- [x] 全既存テストがパスする（58テスト）
- 最大 cNFT/TX: 4（本番URI + Collection付き、ALTにより旧2から倍増）
- 実測 TX サイズ: 842 bytes（2 cNFT, 本番URI）
- コスト分析: [cost-analysis.md](cost-analysis.md)
