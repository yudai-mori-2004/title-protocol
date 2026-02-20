# Solana プログラム ビルド＆デプロイ手順

## 概要

Anchor Solanaプログラム (`programs/title-config`) のビルドとdevnetデプロイに関する注意事項。
2026年2月時点でのSolanaツールチェーン周りの罠と解決策をまとめる。

---

## 前提環境

- macOS (Apple Silicon / ARM64)
- Rust 1.85+ (システム)
- Solana CLI 2.1+ (`sh -c "$(curl -sSfL https://release.anza.xyz/v2.1.14/install)"`)
- Anchor CLI 0.30.1

---

## 問題1: Anchor CLI のインストールが失敗する（LLVM LTO不一致）

### 症状

```
error: linking with `cc` failed: exit status: 1
ld: could not parse bitcode object file ... 'LLVM21.1.8-rust-1.93.1-stable' Reader: 'LLVM APPLE_1_1700.3.9.908_0'
```

### 原因

Rust 1.93 の LLVM (21.x) と macOS システムリンカの LLVM (17.x) のバージョン不一致。
`spl_token_confidential_transfer_proof_*` クレートが LTO (Link-Time Optimization) を使っており、
LTO は LLVM バージョンの一致を要求する。

### 解決策

LTO を無効にしてインストール:

```bash
CARGO_PROFILE_RELEASE_LTO=off cargo install anchor-cli --version 0.30.1
```

**注意**: `cargo install --git https://github.com/coral-xyz/anchor anchor-cli` (最新版) でも同じ問題が発生する。
crates.io から特定バージョンを指定してインストールするのが安定。

---

## 問題2: `anchor build` / `cargo-build-sbf` で `edition2024` エラー

### 症状

```
error: failed to download `constant_time_eq v0.4.2`
  feature `edition2024` is required
  Cargo (1.79.0)
```

同様に `bytemuck_derive v1.9.1`, `wit-bindgen v0.51.0` 等でも発生する。

### 原因

`cargo-build-sbf` は Solana Platform Tools に同梱された専用 Cargo を使用する。
このCargoのバージョンが古い（1.79〜1.84）場合、Rust Edition 2024 を使うクレートの
マニフェストをパースできない。Edition 2024 は Cargo 1.85 で安定化された。

**重要**: `--locked` や依存バージョンのピン留めでは解決しない。
古いCargoはレジストリスキャン時にマニフェストをパースするため、
lockfileに含まれていないバージョンでもエラーになる。

### 解決策

Platform Tools のバージョンを **v1.52 以上** に指定する:

```bash
cargo-build-sbf --manifest-path Cargo.toml --tools-version v1.52
```

Platform Tools のバージョンと内蔵 Rust の対応（2026年2月時点）:

| Platform Tools | 内蔵 Rust | edition2024 |
|---------------|-----------|-------------|
| v1.42〜v1.46  | 1.79      | NG          |
| v1.47         | 1.84.1    | NG          |
| v1.52         | ~1.86+    | OK          |
| v1.53         | 1.89      | OK (ただし sbpfv3 フォーマットで `core` が見つからないエラーが出る場合あり) |

**v1.52 が最も安定**。v1.53 は sbpfv3 移行中のため互換性問題あり。

### `anchor build` から使う場合

`anchor build` は内部で `cargo-build-sbf` を呼ぶが、
`--tools-version` を直接渡す方法がない。
そのため、`cargo-build-sbf` を直接実行する:

```bash
cd programs/title-config
cargo-build-sbf --manifest-path Cargo.toml --tools-version v1.52
```

---

## 問題3: `anchor build` がプログラムを検出しない（0秒で完了）

### 症状

```bash
$ anchor build
$ # 0秒で完了、出力なし、.so ファイルなし
```

### 原因

`Anchor.toml` がプログラムディレクトリ内 (`programs/title-config/Anchor.toml`) にある場合、
Anchor はそこをワークスペースルートとみなし、`programs/title-config/programs/` を探す。
当然そんなディレクトリは存在しないので、何もビルドしない。

### 解決策

`Anchor.toml` を **プロジェクトルート** に配置する:

```
title-protocol/
├── Anchor.toml          ← ここ
├── programs/
│   └── title-config/
│       ├── Cargo.toml
│       └── src/lib.rs
```

---

## 問題4: `overflow-checks` が有効でない

### 症状

```
Error: `overflow-checks` is not enabled.
```

### 解決策

プログラムの `Cargo.toml` に追加:

```toml
[profile.release]
overflow-checks = true
```

---

## 問題5: Cargo.lock バージョン不一致

### 症状

```
error: failed to parse lock file
  lock file version 4 requires `-Znext-lockfile-bump`
```

### 原因

システムの Cargo (1.78+) が lockfile v4 を生成するが、
Platform Tools の古い Cargo が読めない。

### 解決策

Platform Tools v1.52 以上を使えば lockfile v4 を処理できる。
もし古い Tools を使う必要がある場合:

```bash
sed -i '' 's/^version = 4/version = 3/' Cargo.lock
```

---

## 問題6: `solana-test-validator` Docker イメージが ARM64 非対応

### 症状

```
WARNING: image with reference solanalabs/solana:v1.18.26 was found
  but does not match the specified platform
```

### 解決策

ローカル validator は使わず、外部 RPC プロバイダ（Helius devnet 等）を利用する。
`docker-compose.yml` から `solana-test-validator` サービスを削除し、
`SOLANA_RPC_URL` 環境変数で外部 RPC を指定:

```bash
SOLANA_RPC_URL=https://devnet.helius-rpc.com/?api-key=xxx docker compose up -d
```

---

## 推奨ビルド＆デプロイ手順（2026年2月時点）

```bash
# 1. Anchor CLI インストール（LTO無効）
CARGO_PROFILE_RELEASE_LTO=off cargo install anchor-cli --version 0.30.1

# 2. プログラムキーペア生成（初回のみ）
mkdir -p programs/title-config/target/deploy
solana-keygen new -o programs/title-config/target/deploy/title_config-keypair.json --no-bip39-passphrase

# 3. プログラムIDを確認し、declare_id! と Anchor.toml と init-config.mjs を更新
solana address -k programs/title-config/target/deploy/title_config-keypair.json

# 4. ビルド（cargo-build-sbf + Platform Tools v1.52）
cd programs/title-config
rm -f Cargo.lock && cargo generate-lockfile
cargo-build-sbf --manifest-path Cargo.toml --tools-version v1.52

# 5. デプロイ
solana program deploy target/deploy/title_config.so \
  --program-id target/deploy/title_config-keypair.json \
  --url devnet

# 6. SOLが不足する場合
solana airdrop 2 --url devnet
# またはfaucet: https://faucet.solana.com/
```

---

## デプロイ済み情報

| 項目 | 値 |
|------|-----|
| Program ID | `C2HryYkBKeoc4KE2RJ6au1oXc1jtKeKw3zrknQ455JQN` |
| クラスタ | Devnet |
| Platform Tools | v1.52 |
| Anchor | 0.30.1 |

---

## 参考リンク

- [Anchor edition2024 issue #3606](https://github.com/solana-foundation/anchor/issues/3606)
- [Agave edition2024 issue #8443](https://github.com/anza-xyz/agave/issues/8443)
- [Platform Tools releases](https://github.com/anza-xyz/platform-tools/releases)
