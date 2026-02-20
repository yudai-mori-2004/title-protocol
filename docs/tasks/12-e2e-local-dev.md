# タスク12: E2Eインテグレーションテスト + ローカル開発環境

## 前提タスク

- タスク1〜9が全て完了していること

## 読むべきファイル

1. `docker-compose.yml` — 7サービス構成
2. `scripts/setup-local.sh` — 現在のスタブ
3. `docs/SPECS_JA.md` — §1.1 登録フロー全体、§1.2 検証フロー全体

## 作業内容

### setup-local.sh の完成

docker-compose up 後に実行する初期化スクリプト:

1. Solana test-validatorの起動待ち（ヘルスチェック）
2. Anchorプログラムのデプロイ（`anchor deploy`）
3. MinIOバケット作成（`mc mb` コマンド）
4. Global Config初期化（Anchorクライアント or CLIスクリプト）
   - authority設定
   - core_collection_mint, ext_collection_mint の作成と設定
   - MockRuntimeの公開鍵をtrusted_tee_nodesに追加
   - WASMモジュール情報をtrusted_wasm_modulesに追加
5. TEE-mock の /create-tree 呼び出し → Merkle Tree作成
6. 全サービスの動作確認

### E2Eテストスイート

`tests/e2e/` ディレクトリを作成。TypeScriptで記述（SDKを直接使用）:

#### テスト1: 基本登録フロー
1. C2PA付きテスト画像を用意
2. `sdk.discoverNodes()` でノード取得
3. `sdk.upload()` でアップロード
4. `sdk.register({ processorIds: ["core-c2pa"] })` で登録
5. Solanaでcnftの存在を確認
6. `sdk.resolve(contentHash)` で検証
7. オーナーが一致することを確認

#### テスト2: Extension付き登録
1. `sdk.register({ processorIds: ["core-c2pa", "phash-v1"] })` で登録
2. Core cNFT と Extension cNFTの両方が発行される
3. resolve でpHash属性が取得できる

#### テスト3: 来歴グラフ
1. 素材A, Bを登録
2. A, Bを素材としたC2PA付きコンテンツCを作成
3. Cを登録
4. resolve(C) で来歴グラフにA, Bのノードが含まれる
5. A, Bのオーナーが正しく解決される

#### テスト4: 重複解決
1. 同一コンテンツを2回登録
2. resolve で先行登録が優先される

#### テスト5: Burn
1. コンテンツを登録
2. cNFTをBurn
3. resolve でburnt扱いになる（所有者なし）

#### テスト6: 鍵ローテーション
1. /verifyでsigned_jsonを取得
2. TEEを再起動（MockRuntime再初期化）
3. 旧signed_jsonで/signを呼ぶ → 拒否される

### CIへの統合

`.github/workflows/ci.yml` にE2Eテストジョブを追加:
- docker-compose up
- setup-local.sh 実行
- E2Eテスト実行
- docker-compose down

## 完了条件

- `docker-compose up` + `scripts/setup-local.sh` で完全なローカル環境が立ち上がる
- 全E2Eテストが通る
- CI上でE2Eテストが自動実行される（Optional: Nitro環境は不要、MockRuntimeで実行）
- `docs/COVERAGE.md` を最終更新
