# Task 40: コード監査 — sdk/ts/

## 対象
`sdk/ts/` — TypeScript SDK (`@title-protocol/sdk` npm パッケージ)

## ファイル
- `sdk/ts/src/index.ts` — エントリポイント（re-export）
- `sdk/ts/src/types.ts` — 共有型定義（crates/types対応）
- `sdk/ts/src/client.ts` — TitleClientクラス（HTTPクライアント）
- `sdk/ts/src/crypto.ts` — E2EE暗号処理（X25519 + AES-256-GCM）
- `sdk/ts/src/__tests__/crypto.test.ts` — 暗号テスト（10テスト）
- `sdk/ts/package.json` — npmパッケージ定義
- `sdk/ts/tsconfig.json` — TypeScript設定
- `sdk/ts/README.md` — パッケージドキュメント

## 監査で発見した問題

### npm配布品質の問題（全て修正済み）

#### 1. stale dist/ファイル
`src/` から削除済みの旧ファイル（`discover.ts`, `register.ts`, `resolve.ts`, `storage.ts`）が
`dist/` にコンパイル済みバイナリとして残存。`"files": ["dist"]` で指定されているため、
npm publishで配布パッケージに含まれてしまう。

- `dist/discover.js` / `dist/discover.d.ts` — ノード発見（削除済み）
- `dist/register.js` / `dist/register.d.ts` — 登録フロー（削除済み）
- `dist/resolve.js` / `dist/resolve.d.ts` — 解決（削除済み）
- `dist/storage.js` / `dist/storage.d.ts` — ストレージプロバイダ（削除済み）

→ `npm run clean && npm run build` で解消。

#### 2. 日本語エラーメッセージ・JSDoc
npm配布コード内のエラーメッセージとJSDocコメントが日本語。
国際的なnpmパッケージとして英語化が必要。

対象ファイル:
- `src/client.ts` — エラーメッセージ4箇所 + JSDoc全体
- `src/types.ts` — JSDoc全体
- `src/crypto.ts` — JSDoc全体
- `src/index.ts` — モジュールdoc

テストファイル（`src/__tests__/`）は npm配布対象外のため日本語のまま。

#### 3. tsconfig.json の型定義不足
`"lib": ["ES2022"]` のみで `"types": ["node"]` がなく、
Node.js 18+のグローバル（`fetch`, `TextEncoder`, `crypto.subtle`）と
`node:test` / `node:assert/strict` モジュールの型が解決できない。
以前のビルドでは既存dist/が残っていたため発覚していなかった。

→ `"types": ["node"]` を追加。

#### 4. README Quick Startのapi不一致
- `session.encryptionPubkey` をそのまま `deriveSharedSecret` に渡しているがBase64文字列→Uint8Array変換が必要
- `VerifyRequest` のフィールド名が実際の型定義と不一致（`encrypted_payload_url` → `download_url`）
- `SignRequest` の構造が実際の型定義と不一致
- Crypto関数テーブルが `generateEphemeralKeyPair` と `deriveSharedSecret` のみで、`encrypt`, `decrypt`, `encryptPayload`, `decryptResponse` が欠落

→ Quick Start例を実際のAPIに合わせて全面書き直し。Crypto関数テーブルに7関数を記載。

### 設計上の判断

#### register.ts の削除について
仕様書§6.7では `register()` を11ステップのオーケストレーション関数として定義しているが、
現在のSDKは意図的にthin clientアーキテクチャを採用:
- `selectNode` → `encryptPayload` → `upload` → `verify` → `decryptResponse` → `sign`
の各ステップを個別に公開し、アプリケーション側で組み合わせる設計。

これにより:
- off-chain storage（Arweave等）の選択がアプリケーション側に委ねられる
- `delegateMint` の有無によるフロー分岐がアプリケーション側で制御可能
- wasm_hash検証やトランザクション検証のタイミングをアプリケーションが決定可能

v0.1.0としてはこの設計で十分。将来的に `register()` ヘルパーを追加する場合は
StorageProvider抽象の再導入が必要。

#### 未使用型定義の整理
`ResolveResult` / `ResolvedExtension` — resolve()関数が削除されたため使用箇所なし。
ただし、SDK利用者がresolve結果を扱う際の型として有用な可能性があるため、
types.tsからは削除せず残す選択肢もあった。今回は使われていない型を削除してクリーンに保つ方針とした。

## 修正不要と判断した項目

- **CommonJS出力**: `module: "commonjs"` はnpm生態系で広く互換性あり。ESM移行はv2で検討
- **Buffer.from() 使用**: crypto.tsでBase64変換に使用。Node.js SDK前提のためOK
- **GlobalConfig注入方式**: オンチェーンPDAからの直接読み取りは将来の改善。現行の注入方式はテスト容易性で優れる
- **テストカバレッジ**: crypto 10テスト。clientはHTTP依存のためintegration testの領域。E2Eはinit-config.mjsが担う

## 修正ファイル一覧

| ファイル | 変更内容 |
|---------|---------|
| `sdk/ts/src/index.ts` | JSDoc英語化 |
| `sdk/ts/src/types.ts` | JSDoc英語化、未使用型削除 |
| `sdk/ts/src/client.ts` | JSDoc英語化、エラーメッセージ英語化 |
| `sdk/ts/src/crypto.ts` | JSDoc英語化 |
| `sdk/ts/tsconfig.json` | `"types": ["node"]` 追加 |
| `sdk/ts/README.md` | Quick Start修正、Crypto関数テーブル拡充 |
| `sdk/ts/dist/` | staleファイル削除（clean + rebuild） |

## 完了基準
- [x] npm配布コードのJSDoc/エラーメッセージ英語化
- [x] stale dist/ファイル削除
- [x] tsconfig.json修正（types: ["node"]）
- [x] README Quick Start APIを実際の型定義と一致させる
- [x] `npm run build` パス
- [x] `npm test` パス（10テスト）
- [x] `npm pack --dry-run` でstaleファイルが含まれないことを確認
- [x] `cargo test --workspace` パス（143テスト）
