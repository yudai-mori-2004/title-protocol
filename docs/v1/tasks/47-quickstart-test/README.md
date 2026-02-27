# Task 47: QUICKSTART.md ゼロベーステスト

## 目的

QUICKSTART.md の全ステップを新規開発者の視点でゼロから実行し、ドキュメントの正確性と完全性を検証する。

## スコープ

1. **Step 1-4**: Anchor プログラムビルド → devnet デプロイ → WASM ビルド → GlobalConfig 初期化
2. **Running a Node**: AWS example を使ったノードデプロイ
3. **TEE Node Registration + Merkle Tree**: init-devnet.mjs によるノード登録・Tree 作成
4. **Register Content**: SDK / integration-tests によるコンテンツ登録 E2E

## 前提

- 既存の devnet GlobalConfig (`GXo7dQ4kW8oeSSSK2Lhaw1jakNps1fSeUHEfeb7dRsYP`) を使用
- AWS Nitro example (`deploy/aws/`) でノードをデプロイ

## 完了条件

- [ ] 全ステップが手順通りに実行可能であることを確認
- [ ] 発見した問題点を QUICKSTART.md に反映
- [ ] E2E でコンテンツ登録 → cNFT ミントまで成功
