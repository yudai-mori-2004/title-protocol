# Task 11: SDK ノード選択改善 + CryptoProvider抽象化

## 背景

RootLensアプリでの動画登録テストにおいて2つの問題が発覚した：

1. **ノード選択が遅い**: `selectNode()` が逐次的にヘルスチェックしていたため、死んだノード（Node 1: `54.250.143.52`）に先に当たると10秒待ちが発生（50%の確率）
2. **React Nativeでの暗号化が遅い**: Hermesエンジン上で `crypto.subtle` のAES-GCM暗号化が9.7MBに対して20秒かかる（Node.jsでは一瞬）

## 作業内容

### 1. selectNode() の並列レース化

**変更前**: ランダムに1ノードずつ逐次ヘルスチェック → 失敗したら次
**変更後**: `Promise.any()` で全候補に同時ヘルスチェック → 最速応答を採用

- `HEALTH_CHECK_BATCH_SIZE = 8` で同時リクエスト数を制限（将来のノード数増加に備える）
- シャッフル → バッチ分割 → `Promise.any` → バッチ全滅なら次バッチ
- `HEALTH_CHECK_TIMEOUT_MS = 5000` で個別タイムアウト
- `healthCheck()` を共通ヘルパーに抽出、`selectNodeByEndpoint()` も統一

**効果**: Node 1が死んでいても常に30-60msで Node 2 を選択（以前: 50%の確率で10秒待ち）

### 2. CryptoProvider インターフェース導入

SDKの暗号化を差し替え可能に抽象化：

```typescript
interface CryptoProvider {
  encrypt(key: Uint8Array, plaintext: Uint8Array): Promise<{ nonce: Uint8Array; ciphertext: Uint8Array }>;
  decrypt(key: Uint8Array, nonce: Uint8Array, ciphertext: Uint8Array): Promise<Uint8Array>;
  toBase64(bytes: Uint8Array): string;
  fromBase64(str: string): Uint8Array;
}
```

- `defaultCryptoProvider`: `crypto.subtle` + `Buffer`（Node.js/ブラウザ向け、現行動作と同じ）
- `TitleClientOptions` でコンストラクタ時に差し替え可能
- ECDH/HKDF は `@noble/curves` のまま（32バイト演算で全環境高速）
- `register()` 内の暗号化・Base64変換が全て `this.crypto` 経由

**設計判断**:
- SDKはWeb標準API（`crypto.subtle`）をデフォルトで使い、プラットフォーム非依存を維持
- React Native等のモバイル環境では `expo-crypto` の `aesEncryptAsync` や `react-native-aes-gcm-crypto` 等をアプリ側で注入
- AES-GCMだけでなくBase64もプロバイダーに含めた（Hermes上の `Buffer.toString("base64")` も9.7MBで数秒かかるため）

## 変更ファイル

| ファイル | 変更内容 |
|----------|----------|
| `sdk/ts/src/crypto.ts` | `CryptoProvider` IF追加、`defaultCryptoProvider` export、全関数にprovider引数 |
| `sdk/ts/src/client.ts` | `TitleClientOptions` 追加、`selectNode()` 並列化、`register()` でprovider使用 |
| `sdk/ts/package.json` | `0.1.6` → `0.1.7` |

## 検証

- 全25ユニットテスト通過（crypto + chain）
- devnet実ノードで selectNode() 5回連続 30-60ms
- devnet Node 2 で動画(5.2MB mp4) の verify 成功 (400ms)
- `@title-protocol/sdk@0.1.7` npm publish 済み

## 完了条件

- [x] `selectNode()` が `Promise.any` ベースの並列レース
- [x] `HEALTH_CHECK_BATCH_SIZE` で同時リクエスト数制限
- [x] `CryptoProvider` インターフェース定義・export
- [x] `defaultCryptoProvider` がデフォルトで現行動作を維持
- [x] `TitleClient` コンストラクタでカスタム `CryptoProvider` 注入可能
- [x] 全テスト通過
- [x] npm publish (`@title-protocol/sdk@0.1.7`)
