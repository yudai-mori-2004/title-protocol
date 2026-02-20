# タスク8: TypeScript SDK 全関数実装

## 前提タスク

- タスク6（Gateway）が完了していること（SDK→Gateway→TEEの経路が確立）

## 読むべきファイル

1. `docs/SPECS_JA.md` — §6.7「SDK」全体 + §6.4「ハイブリッド暗号化」
2. `sdk/ts/src/crypto.ts` — 現在のスタブ
3. `sdk/ts/src/register.ts` — 現在のスタブ
4. `sdk/ts/src/resolve.ts` — 現在のスタブ
5. `sdk/ts/src/discover.ts` — 現在のスタブ
6. `sdk/ts/src/storage.ts` — 現在のスタブ
7. `sdk/ts/src/types.ts` — 型定義（実装済み）

## 作業内容

### crypto.ts — E2EEクライアント側暗号化

Web Crypto API または `@noble/curves` + `@noble/hashes` を使用:

- `generateEphemeralKeyPair()`: X25519キーペア生成
- `deriveSharedSecret(ephemeralSk, teePk)`: ECDH共有秘密の導出
- `deriveSymmetricKey(sharedSecret)`: HKDF-SHA256でAES-256鍵を導出
- `encrypt(key, plaintext)`: AES-256-GCM暗号化（nonceをランダム生成）
- `decrypt(key, nonce, ciphertext)`: AES-256-GCM復号

依存追加: `@noble/curves`, `@noble/hashes` をpackage.jsonに追加。

### upload() — 新規作成

`sdk/ts/src/upload.ts` を新規作成し、`index.ts` からexport:

1. Gateway `/upload-url` を呼んで署名付きURLを取得
2. 暗号化済みペイロードを署名付きURLにPUT
3. `{ downloadUrl, sizeBytes }` を返す

### register() — 登録フロー

§6.7の内部処理フローに従い11ステップを実装:

1. TEEの `encryption_pubkey` を取得（discoverNodes or 直接指定）
2. エフェメラルキーペア生成
3. ClientPayload構築（content, owner_wallet, extension_inputs）
4. ペイロード暗号化（ECDH + HKDF + AES-GCM）
5. Temporary Storageにアップロード（upload()使用）
6. `/verify` 呼び出し
7. レスポンス復号（エフェメラル秘密鍵 + 共通鍵）
8. **wasm_hash検証**（セキュリティクリティカル）: Extension signed_jsonの `wasm_hash` を Solana RPCから直接取得したGlobal Configの `trusted_wasm_modules` と照合
9. signed_jsonをオフチェーンストレージにアップロード
10. `/sign` 呼び出し
11. **トランザクション検証** → ウォレット署名 → ブロードキャスト

`delegateMint: true` の場合はステップ10で `/sign-and-mint` を呼ぶ。

### resolve() — 検証フロー

§5.2の7ステップに従い実装:

1. content_hashに対応するcNFTをDAS APIで検索
2. コレクション所属確認（Global Config参照）
3. オフチェーンデータ取得
4. TEE署名検証（`ed25519_verify` 相当）
5. content_hash一致確認
6. 重複解決（同一content_hashに複数cNFT → TSA or block time比較）
7. 来歴グラフ解決（各ノードのcontent_hashに対して再帰的に1-5を実行）

### discoverNodes() — ノード発見

1. Solana RPCからGlobal Config PDAを読み取り
2. `trusted_tee_nodes` でstatus=Activeをフィルタ
3. 各ノードの `gateway_endpoint/.well-known/title-node-info` にアクセス
4. オプション条件（minSingleContentBytes等）でフィルタリング

### storage.ts — ArweaveStorage

`ArweaveStorage` クラスの実装:
- Irys SDK（`@irys/sdk`）を使用
- `upload(data)`: Arweaveにアップロードし `ar://...` URIを返す
- `download(uri)`: URIからデータを取得

依存追加: `@irys/sdk` をpackage.jsonに追加。

## 完了条件

- `cd sdk/ts && npm run build` が通る
- crypto.ts: 暗号化→復号のラウンドトリップテスト
- crypto.ts: Rust側（crates/crypto）と同一の入力で同一の出力が得られる相互運用テスト
- register(): MockRuntime TEE + Gateway に対してE2Eで登録フローが動作（docker-compose環境）
- resolve(): 登録済みcNFTに対してオーナーと来歴グラフが返る
- `docs/COVERAGE.md` の該当箇所を更新
