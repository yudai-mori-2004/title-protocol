# Task 19: TEEパフォーマンスプロファイリング

## 目的

TEEノードのスケーリング特性を把握し、本番インスタンス選定とコスト最適化の根拠データを取得する。

## 背景

Task 18 で ffmpeg + exiftool を TEE Docker イメージに追加したことで、EIF サイズが 206MB → 519MB に増加し、Enclave の最低メモリ要件が 1024MB → ~2000MB に跳ね上がった。メモリ割り当てがパフォーマンスにどう影響するか、定量データが必要。

また、テスト基盤（`tests/perf/`）が整備されたため、体系的なプロファイリングが実施可能になった。

## 実施内容

### Phase 1: メモリパターン別ベースライン

同一EC2インスタンス（c5.xlarge, 4vCPU, 8GB RAM）上で Enclave メモリ割り当てを変えてベースライン計測を実施。

| パターン | Enclave メモリ | アプリ利用可能メモリ | 備考 |
|---------|---------------|-------------------|------|
| A | 2048 MB | ~50 MB | 最小動作（EIF ~2GB） |
| B | 3072 MB | ~1 GB | 現在のデフォルト |
| C | 4096 MB | ~2 GB | 余裕あり |
| D | 5120 MB | ~3 GB | インスタンス限界付近 |

各パターンで以下を計測:

```bash
ENCLAVE_MEMORY_MIB=<値> ./deploy/aws/setup-ec2.sh
cd tests/perf && npm run baseline
cd tests/perf && npm run throughput
cd tests/perf && npm run content-size
```

### Phase 2: スループット飽和点の比較

各メモリパターンで並列リクエスト数を段階的に増加（1, 2, 4, 8, 16, 32, 48, 64）し、飽和点の差異を比較。

期待される結果:
- メモリがボトルネックの場合: パターン A で早期飽和、D で後方にシフト
- メモリが無関係の場合: 全パターンで同じ飽和点 → vCPU がボトルネック

### Phase 3: コンテンツサイズ別のメモリ影響

大きなコンテンツ（1080p JPEG, 10s 720p MP4）のデコードがメモリ制約で失敗するかを確認。ResourcePool のピークメモリ推定と実メモリの関係を把握。

### Phase 4: レポート作成

結果を `performance-report.md` にまとめ、以下を導出:
- **比例コスト**: メモリ/vCPU に比例する処理（WASMイメージデコード、ffmpegフレーム抽出）
- **定数コスト**: スペックに依存しない処理（暗号操作、C2PAパース、ネットワークRTT）
- **推奨スペック**: コスト効率の良いEnclave設定
- **スケーリング指針**: ノード数 vs ノードスペックの判断基準

## 読むべきファイル

- `tests/perf/` — パフォーマンステスト群（baseline, throughput, sustained, content-size, resilience）
- `tests/perf/README.md` — テストの実行方法と読み方
- `deploy/aws/setup-ec2.sh` — `ENCLAVE_MEMORY_MIB` 環境変数
- `deploy/aws/terraform/variables.tf` — `enclave_memory_mib`, `enclave_cpu_count`, `instance_type`
- `crates/wasm-host/src/resource_pool.rs` — メモリバジェット管理

## 完了条件

- [ ] 4パターンのメモリ設定でベースライン計測完了
- [ ] スループット飽和点の比較データ取得
- [ ] コンテンツサイズ × メモリのマトリクス
- [ ] `performance-report.md` にグラフ/表で結果まとめ
- [ ] 推奨スペックとスケーリング指針の記載

## 出力

- `docs/v0.1.1/tasks/19-tee-performance-profiling/performance-report.md`
