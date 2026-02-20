# タスク11: NitroRuntime + Enclave本番ビルド

## 前提タスク

- タスク1（MockRuntime）が完了していること
- タスク2（Proxy）が完了していること

## 読むべきファイル

1. `docs/SPECS_JA.md` — §6.4「TEE」「鍵管理」「TEE起動シーケンス」§5.2 Step 4.1「Attestation Documentの検証」
2. `crates/tee/src/runtime/nitro.rs` — 現在のスタブ
3. `crates/tee/src/runtime/mock.rs` — 参考実装（タスク1で完成済み）
4. `prototype/enclave-c2pa/enclave/src/main.rs` — Nitro NSM API使用例
5. `docker/tee.Dockerfile` — 本番ビルド用
6. `scripts/build-enclave.sh` — EIF生成スクリプト

## 作業内容

### NitroRuntime実装

- `generate_signing_keypair()`: NSM API (nsm-io) 経由のエントロピーでEd25519キーペア生成。秘密鍵はEnclave内メモリにのみ保持
- `generate_encryption_keypair()`: 同様にX25519キーペア生成
- `get_attestation()`: NSM APIで `Attestation { public_key, user_data, nonce }` をリクエスト。公開鍵をuser_dataに含めたAttestation Documentを取得

依存追加: `nsm-lib` または `nsm-io` をCargo.tomlに追加（Linux条件付き）。

### build-enclave.sh の完成

現在コメントアウトされている部分を実装:
1. `docker build` でLinuxバイナリをビルド
2. `nitro-cli build-enclave` でEIFを生成
3. PCR値（PCR0, PCR1, PCR2）を表示
4. EIFファイルのパスを出力

### Attestation Document検証ユーティリティ

`crates/crypto` または新規ユーティリティに、Attestation Document検証関数を追加:
- AWS Nitro Attestation PKIルート証明書でCOSE Sign1を検証
- PCR値の抽出
- 公開鍵フィールドの抽出と一致確認

これはSDKのresolve()（§5.2 Step 4.1のオプショナル検証）でも使える。

## 完了条件

- `cargo build -p title-tee --target x86_64-unknown-linux-gnu` が通る（クロスコンパイル or CI）
- NitroRuntimeのユニットテスト（NSM APIのモック使用）
- build-enclave.sh が実行可能（Nitro CLI環境が必要なため、CIでの自動テストは任意）
- Attestation Document検証関数のテスト（固定のテストドキュメントを使用）
- `docs/COVERAGE.md` の該当箇所を更新
