# タスク21: TEE 安定性調査・自己回復化

## 背景

タスク20 で streaming verification 修正 + stress test を回し、TEE が
**N=32 × 500 MiB (16 GiB content) を完走** するまで耐えられることを確認した
(`docs/v0.1.2/tasks/20-zkp-cli-trust-off/STRESS_TEST.md`)。
ただしその後、**実運用中に enclave が静かに死んだ** 事象が 1 件確認された:

- 観察 (2026-05-26 19:29 JST):
  - EC2 ホスト (`title-protocol-node`, c5.xlarge) 生存、uptime 2 日
  - `title-gateway` / `title-proxy` コンテナ生存
  - **`sudo nitro-cli describe-enclaves` が `[]`** (enclave が存在しない)
  - `GET /health` → `{"status":"unavailable","tee_type":"aws-nitro"}`
  - `POST /process` → 503 `TEE is not available`
- 最後の `nitro-cli run-enclave` から **約 13.5 時間経過** していた
- `dmesg` に OOM-kill 等の痕跡なし、`journalctl` にも明示的なエラーなし
  → enclave が「静かに終了した」状態

復旧自体は `bash deploy/aws/scripts/run.sh` 一発で完了 (PCR0 不変、
allowlist 再登録不要)。問題は **死んだことに気付かない / 自動回復しない** こと。

## 目的

1. **enclave が死ぬ条件を再現** (現状は推測ベース)
2. **死んだら自動的に再起動** する仕組みを入れる
3. **死亡時にアラートを上げる** 経路 (CloudWatch / Slack 通知等)
4. (任意) enclave 内のソフトクラッシュを log として吸い上げる

## やること候補

### 1. 死因の特定

枯渇候補:

- **pthread / fd limit**: タスク20 stress test で N=64 並列時に
  `failed to spawn thread: EAGAIN` を観測済み。連続的な高並列で
  enclave 内 thread pool が落ち、panic で enclave 自己終了している可能性
- **長時間 idle で何かが timeout**: NSM connection、socat、vsock keepalive 等
- **メモリ断片化**: streaming で済むようになったが、Vec<u8> 断片化で
  巨大連続領域が取れず alloc 失敗
- **c2pa-rs panic**: 異常 input で reader 内部が panic、上層で catch されず enclave 終了

調査手段:
- enclave を `ENCLAVE_DEBUG=1` で起動 → `nitro-cli console` で stderr を捕捉
- `crates/tee/src/main.rs` の panic hook で trace を NSM 経由で外に出す
  (debug-mode 限定でも価値あり)
- stress test を時間をかけて回し続け、死ぬまでのリクエスト数を測る
  (Mean Time To Failure を出す)

### 2. 自動再起動

systemd service として `nitro-cli run-enclave` 起動を wrap:

```
/etc/systemd/system/title-protocol-enclave.service
[Service]
Type=forking
ExecStart=/usr/bin/sudo /home/ec2-user/title-protocol/deploy/aws/scripts/run-enclave-only.sh
Restart=on-failure
RestartSec=10
[Install]
WantedBy=multi-user.target
```

`run.sh` から「enclave 起動だけする `run-enclave-only.sh`」を切り出して
それを systemd 配下に入れる。docker (title-gateway / title-proxy) は
既に Docker daemon 経由で auto-restart している (`--restart unless-stopped`
が `run.sh` で設定済み)。enclave だけ単独の watchdog が必要。

「enclave が `nitro-cli describe-enclaves` で見つからなくなったら再起動」
を毎分チェックする health-loop も systemd timer で。

### 3. アラート

最小:
- `Gateway /health` が `tee_type: aws-nitro` 状態で `unavailable` を返したら
  CloudWatch にメトリクス送信 + Slack/Discord webhook
- EC2 上に軽い cron / systemd timer で `curl localhost:3000/health` を
  毎分叩いて、`unavailable` 時に通知

中期:
- 公式 CloudWatch Agent で nitro-enclaves-allocator のメトリクス取得
- enclave PID が消えたら即時イベント

### 4. ソフトクラッシュ log

現状 enclave 内 panic は基本的に外から見えない (release mode の制約)。
debug-mode は PCR を 0 にするので本番不可。妥協案:
- title-tee 起動時に panic hook を仕掛け、enclave 内に小さなログバッファに残す
- 死ぬ直前に NSM の `extend_pcr` で PCR16 に何か書く (= 後から
  `nitro-cli describe-enclaves` 履歴で読める…が enclave 死んだら describe
  も無理なので意味なし)
- 本気で取りたければ vsock 経由で host 側に常時 stderr stream を流す
  (`socat` で逆方向ブリッジ)。enclave 死亡直前の最後の数行が取れる

優先度は低いが、再現困難な突然死を追うには必要。

## 関連ファイル

- `deploy/aws/scripts/run.sh` — 起動エントリポイント。enclave 起動と
  docker 起動が混ざっているので分離が必要
- `crates/tee/src/main.rs` — panic hook 追加 / log 強化の起点
- `crates/tee/src/limits.rs` — pthread limit / fd limit 等のカウンタ追加先
- `crates/tee/src/resource_pool.rs` — admission limit を保守的にして
  pthread spawn 失敗を未然に防ぐパス

## 完了基準

- [ ] enclave が死ぬ条件を 1 つ以上再現できる (= 制御された負荷で MTTF が出る)
- [ ] enclave 死亡時に 30 秒以内に自動復旧する
- [ ] 復旧イベントが運用者に通知される (Slack / Email いずれか 1 経路)
- [ ] 再発時に死因解析できる最低限の log が残る

## 補足

タスク20 で扱った streaming 修正 (`commit 15c308a`) と独立。
streaming 自体は健全に動作している (16 GiB 並列処理を通せている)。
本タスクは「正常動作するが、ごく稀に静かに死ぬ」という運用品質の問題。
v0.1.3 以降の優先タスクで処理する想定。
