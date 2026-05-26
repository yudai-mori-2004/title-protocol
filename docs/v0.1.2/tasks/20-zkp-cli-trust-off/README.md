# タスク20: ZKP安定化 + title-cli運用化 + c2pa trust-off (2026-05-25 〜 26)

タスク15 (Docker) と 16/17 (audit) の積み残しを片付け、**clone直後の人が
プログラム/コード読まずに本番 TEE → ZKP → register-key → cNFT 発行まで通せる**
状態にしたセッションの記録。

## 完了基準 (Definition of Done)

- [x] TEE Docker stack の PCR0 再現性 (`build.sh --verify` exit 0)
- [x] SP1 v6 → v5.2.4 ダウングレード (`sp1-solana 0.1.0` 互換確保)
- [x] `title-cli` に whitelist サブコマンド一式追加 (operator がテストファイル
      編集せず admin 操作可能)
- [x] prover EC2 ライフサイクル自動化 (`deploy/aws/scripts/prover-{launch,setup,prove,run}.sh`)
- [x] OPERATIONS_JA.md を「clone → cNFT 発行」一気通貫 narrative に書き直し
- [x] c2pa-verify を trust-off ポリシーに変更 (RootLens 等の自前 cert chain でも通る)
- [x] devnet で full chain 検証完了 (PCR0 → ZKP → register-key → cNFT mint)

## やったこと (実装単位)

### 1. PCR0 再現性
詳細: `15-docker-deployment/PCR0_REPRODUCIBILITY_INVESTIGATION.md`

ポイントは:
- `Cargo.toml [profile.release]`: `codegen-units=1`, `lto="fat"`, `strip="symbols"`
- `tee-nitro.Dockerfile`: `ARG SOURCE_DATE_EPOCH=0`, `CARGO_INCREMENTAL=0`,
  ベースイメージ digest pin, apt version pin
- **`FROM scratch` squash stage**: Docker whiteout タイムスタンプ問題
  ([moby/moby#50063](https://github.com/moby/moby/issues/50063)) の唯一の回避策

検証: TEE ノードで 2 回 `--no-cache` ビルド → 同じ PCR0 `bab9ec51...`。

### 2. SP1 v5.2.4 ダウングレード
詳細: `15-docker-deployment/PCR0_REPRODUCIBILITY_INVESTIGATION.md` 末尾の SP1 version pin 節

`sp1-solana = "0.1.0"` (Solana 上の唯一公開された on-chain SP1 検証 crate) が
SP1 v5 wire format しかサポートしていない。v6 の 5-public-input / 356 バイト proof は
拒否される。業界調査 (Termina, Soon, automata-dcap-attestation 等) でも全 Solana
SP1 プロジェクトは v5 ベース。CVE-2026-40323 は v6.0.0–6.0.2 限定で v5 に影響なし。

変更ファイル:
- `sp1-guests/attestation-aws-nitro/program/Cargo.toml`: `sp1-zkvm = "=5.2.4"`
- `sp1-guests/attestation-aws-nitro/host/Cargo.toml`: `sp1-sdk = "=5.2.4"`, `sp1-build = "=5.2.4"`
- `sp1-guests/attestation-aws-nitro/host/src/lib.rs`: v5 同期 API (`tokio`削除、`.run()`)
- `programs/title-whitelist/vk/groth16_vk_v5.0.0.bin` (新規、sp1-solana 0.1.0 から抽出)
- `programs/title-whitelist/src/lib.rs`: `GROTH16_VK_BYTES` を v5 に切替

トレードオフ:
- prove 時間 ~18 分 (v6) → ~90 分 (v5) per attestation
- 将来 `sp1-solana` が v6 対応したら v6 へ戻す ([sp1-solana#23](https://github.com/succinctlabs/sp1-solana/issues/23) watch)

### 3. `title-cli` whitelist サブコマンド
変更ファイル:
- `crates/solana/src/whitelist_ix.rs` (新規): instruction builders を test ファイル
  から lib に lift (`build_register_key_ix`, `build_add_approved_vkey_ix` 等 + 共通
  `proof_bytes_for_program` ヘルパー)
- `crates/cli/src/whitelist.rs` (新規): CLI subcommand 実装
- `crates/cli/src/main.rs`: clap subcommand 追加
- `crates/cli/Cargo.toml`: `title-solana`, `hex`, `bs58` 依存追加

追加サブコマンド:
```
init-registries / add-vkey / remove-vkey / add-measurement / remove-measurement
register-key --bundle <dir> / revoke-key / describe-whitelist [--signing-pubkey <pk>]
```

`register-key` は `fetch-registration-bundle.sh` + `prover-run.sh` の出力 4 ファイル
を bundle ディレクトリでまるごと受ける。SP1 SDK 出力 (260 or 356 バイト) を
自動 normalize し、`ComputeBudgetInstruction::set_compute_unit_limit(400_000)` を
prepend (sp1_solana::verify_proof_raw が ~280K CU 消費)。

### 4. prover EC2 ライフサイクルスクリプト
変更ファイル:
- `deploy/aws/scripts/prover-launch.sh` (新規): EC2 起動 + IP 取得
- `deploy/aws/scripts/prover-setup.sh` (新規): toolchain + 16 GiB swap + docker
- `deploy/aws/scripts/prover-prove.sh` (新規): tarball 受け取り → build → prove
- `deploy/aws/scripts/prover-run.sh` (新規): 上記を end-to-end 自動化

特徴:
- ソース転送は `git archive HEAD` の tarball を `scp`（GitHub アクセス不要、
  SSH agent 転送不要）
- Docker (SP1 v5 の Groth16 wrap で必要) も自動 install
- `--keep-alive` で debug 用にインスタンス残せる

### 5. OPERATIONS_JA.md 書き直し
- §1 lifecycle 図に `title-cli` サブコマンド対応を併記
- §2.1 prerequisites を「ローカル開発マシン視点」で再構成 (AWS region 設定、
  Solana RPC 設定、admin keypair の状況分け、SP1 toolchain 必須化)
- §2.4 vkey: 「ビルドホストによって違う値が出る」既知制約を明記
- §2.6/§2.7: setup-host.sh の再ログイン要件、GATEWAY_URL env export
- §2.9: `terraform output -raw public_ip` で `$PUBLIC_IP` capture
- §4.1: `prover-run.sh` ワンライナー強調 (合計 ~110 分 / ~$4)
- §4.4: cNFT 発行 CLI フロー (create-tree → process → mint) を完全例示
- §6.2: `build.sh --verify` を再現性検証手段として明記
- §8 troubleshooting: 旧 anchor instruction 名 → `title-cli` 統一、error code 修正
  (6006/6008/6011)

### 6. c2pa-verify trust-off ポリシー
変更ファイル:
- `crates/core/src/c2pa_verify.rs`: `pub fn c2pa_context()` で `c2pa::Settings`
  の `verify.verify_trust = false` を設定 (一箇所集約)
- `crates/core/src/rootlens_license_v1.rs`: 同じ context を使用

意図と影響:
- **enforce 維持**: `assertion.dataHash.match` (改ざん検知), `claimSignature`
  数学的検証, manifest structural validation
- **relax**: `signingCredential.untrusted` (C2PA 公式 trust list 非掲載の cert
  が fail にならない)
- **理由**: RootLens 等の自前 cert chain でも通せる必要がある。Title Protocol は
  「TEE が見た事実」を記録するレイヤーで、trust 判断は consumer 側に委ねる設計
- C2PA spec の推奨 (trust 検証必須) からは意図的に外れる選択

検証 (TEE 上で実行):
| asset | failure codes | TEE 判定 |
|---|---|---|
| `c2pa-properly-signed.jpg` (self-signed cert + 正規 action) | `signingCredential.untrusted` のみ | **status: ok** ✓ |
| `c2pa-tampered.jpg` (上記を 1 バイト改ざん) | dataHash.mismatch | status: error (Invalid) ✓ |
| `c2pa-sample.jpg` (legacy fixture, malformed action) | malformed action | status: error (Invalid) ✓ |

## devnet 検証履歴 (この session の実機ログ)

### TEE node (c5.xlarge, 13.113.217.17)

| イベント | 値 |
|---|---|
| 初期ビルド PCR0 (v6 era) | `e1343ee6...` |
| 再現性確認 PCR0 (squash 後) | `bab9ec51...` (2 回ビルド連続一致) |
| 最終 PCR0 (trust-off 追加後) | `bc3cbddb8afd33cde74998b734b374c52757260cabc6afb97d9737b68ee7a6f4a0355794fad2440a15478f74683bf917` |
| 現行 solana_pubkey | `Bs8ARKGKuwVMJ18GmtHSns5M9tqW4RQ12WX4epjSFbKf` |

### Solana devnet program (`43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs`)

| イベント | tx / 値 |
|---|---|
| プログラム再デプロイ (v6→v5 VK) | `5VDEbKq3kheLrSsWj1MQT4dc87xptmsQScyWqLJ4tinfiKFbmEFexdFZmURREN1YTwTMiSmHw3y8csaY3thutRtc` |
| プログラム再デプロイ (trust-off は program 不変) | (実施せず — c2pa-verify 変更は TEE バイナリ側のみ) |
| add-vkey (現行 vkey) | TBD (新 PCR0 用 proof 完了後に追加) |
| add-measurement (`bc3cbddb...`) | TBD |
| register-key | TBD |
| cNFT mint (旧 PCR0 期) | tx `seR5EzPNxFyjpKZRwFVprvZmoDcPeSKweHECSMeeJa2gSBrq5wiUH4cpntwETEW14ra4HoCWKtGuUxHJBTVq7Cy` |
| Merkle tree | `7h8YLnfexU9W8YMX6TxevDcGMcEwmkPZtmJgwHg25vN6` |

## 未解決の既知問題

### vkey の host 非決定性
SP1 SDK v5.2.4 + `--remap-path-prefix` でも host (macOS arm64 vs AL2023 x86_64)
を変えると別の vkey が出る。`build.rs` の rustflags 範囲外でホスト情報が
ELF に漏れている。

| host | vkey_hash |
|---|---|
| Mac arm64 | `0x00034549b1d12550031ec07953cfcfdcf6a4a026fc961336776cd715bd83803e` |
| AL2023 x86_64 (prover EC2) | `0x0071fff4b7217786401fa6a7be505a4a13ed06dc65cb18d25faee73da7b1db99` |

回避策: 「prover EC2 (AL2023 x86_64) で取った vkey を allowlist の基準とする」
運用方針を OPERATIONS に明記。

深掘りには `objdump -h` で v5 vs build.rs の rustflags 範囲外の DWARF / build-id /
plt セクション diff が必要。次セッション以降。

### sp1-solana の v6 対応
[Issue #23](https://github.com/succinctlabs/sp1-solana/issues/23) を継続 watch。
Succinct 公式が動かない場合は `MavenRain/realms-zk-voting` の patch を独自監査の上
取り込む選択肢を検討する。upgrade で prove 時間が ~90 分 → ~18 分に短縮できる。

## 次にやるべきこと候補

1. **大容量コンテンツ stress test** (本タスクの一部として実施予定)
2. SP1 guest vkey の host 非決定性原因特定
3. mainnet 用 multi-sig admin program 設計 (現状単一 pubkey hardcode)
4. cNFT 発行後の verifier-side reference 実装 (TS SDK)
