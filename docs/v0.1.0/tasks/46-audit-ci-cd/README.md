# Task 46: コード監査 — CI/CD

## 対象
`.github/workflows/` — GitHub Actions CI/CDパイプライン

## ファイル
- `ci.yml` — CI（Rust check/test/audit + TypeScript build/test）
- `publish.yml` — npm publish（SDK + Indexer、タグトリガー）

## 監査で発見された問題

### バグ
1. **`npm ci` が `package-lock.json` なしで実行される**:
   `ci.yml` と `publish.yml` の TypeScript ジョブで `npm ci` を使用しているが、
   `sdk/ts/` にも `indexer/` にも `package-lock.json` がコミットされていない。
   `npm ci` は `package-lock.json` を必須とするため、CI/publishとも確実に失敗する。
   両パッケージはライブラリ（npm publish対象）であり、lockfileをコミットしないのが慣習。
   → `npm ci` を `npm install` に変更。

### コード品質
2. **Node.js "24" は未リリースバージョン**:
   2026年2月時点でNode.js 24はリリースされていない（2026年4月予定）。
   `setup-node@v4` で指定すると失敗する。
   Node 20はEOL間近（2026年4月）のため、現行Active LTSのNode 22が適切。
   → "22" に修正。

### 設計メモ（修正不要）
- `ci.yml` のRustジョブ: checkout → toolchain(stable+wasm32) → cache → check → check(no-default-features) → test → WASM build。ベンダーニュートラルチェック(`--no-default-features`)があり良い
- `ci.yml` の `cargo-audit` ジョブ: セキュリティ監査が分離されており適切
- `publish.yml`: タグベーストリガー（`sdk-v*`, `indexer-v*`）で SDKとIndexerを独立publish。`--provenance` フラグで npm provenance 有効化。`id-token: write` 権限も正しい
- `publish.yml`: `NPM_TOKEN` は secrets 経由で適切に管理

## 完了基準
- [x] `ci.yml` / `publish.yml`: `npm ci` → `npm install` に変更
- [x] `ci.yml` / `publish.yml`: Node.js バージョンを "22" に修正

## 対処内容

### 1. `npm ci` → `npm install` に変更
`sdk/ts` と `indexer` はライブラリ（npm publish対象）であり、
`package-lock.json` をコミットしないのがライブラリの慣習。
`npm ci`（lockfile必須）ではなく `npm install` を使用することで、
lockfileなしでもCIが動作し、最新互換バージョンでのテストも兼ねる。

### 2. Node.js バージョンの修正
`ci.yml` と `publish.yml` の `node-version: "24"` を `"22"` に修正。
Node 22は現行Active LTS（EOL 2027年4月）。
Node 24は2026年4月リリース予定で未リリース、Node 20はEOL間近。
