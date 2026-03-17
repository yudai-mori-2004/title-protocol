# Title Protocol コスト分析

1コンテンツ = 3 cNFT（1 core + 2 extension）としたときの全コスト。

## レート（2026-03-17時点）

| 項目 | 値 |
|------|----|
| SOL | $94.06 |
| USD/JPY | 159.3 |
| SOL/JPY | ¥14,983 |
| Arweave永続保存 | ~$7/GB |

## 1登録あたりの変動費

3 cNFT は ALT により 1 TX（~955 bytes）に収まる。

| 項目 | 内訳 | SOL | USD | JPY |
|------|------|-----|-----|-----|
| Solana TX fee | 1 TX × 2署名 × 5,000 lamports | 0.000010 | $0.00094 | ¥0.15 |
| Irys fund TX fee | fund時のSolana送金TX（複数回分まとめて可） | ~0.000005 | $0.00047 | ¥0.07 |
| Arweave保存 | 3 signed_json × ~2KB = 6KB @ $7/GB | 0.000001 | $0.00004 | ¥0.01 |
| **変動費 合計** | | **0.000016** | **$0.0015** | **¥0.24** |

Arweave保存コストは6KBに対し$0.00004と極めて小さく、TX feeが変動費のほぼ全て。

## Merkle Tree 初期費用（ノード立ち上げ時、1回のみ）

Rent formula: `(128 + data_size) × 6,960 lamports`。buffer_size=64固定。
Title Protocol は Core Tree + Extension Tree の2本を使用。

3 cNFT/content のボトルネックは Ext Tree（2 leaves/content）。
最大登録数 = 2^(depth-1)。

### depth=14（小規模: 1.6万 leaves/tree）

| 項目 | 値 |
|------|----|
| Tree data size | 31,800 bytes |
| 1 tree rent | 0.2222 SOL |
| 2 trees rent | 0.4444 SOL ($41.80 / ¥6,660) |
| 最大登録数 | 8,192 件 |
| 1登録あたり Tree 償却 | 0.0000543 SOL ($0.0051 / ¥0.81) |

### depth=20（中規模: 100万 leaves/tree）

| 項目 | 値 |
|------|----|
| Tree data size | 44,280 bytes |
| 1 tree rent | 0.3089 SOL |
| 2 trees rent | 0.6178 SOL ($58.11 / ¥9,256) |
| 最大登録数 | 524,288 件 |
| 1登録あたり Tree 償却 | 0.0000012 SOL ($0.00011 / ¥0.018) |

### depth=24（大規模: 1,680万 leaves/tree）

| 項目 | 値 |
|------|----|
| Tree data size | 52,600 bytes |
| 1 tree rent | 0.3670 SOL |
| 2 trees rent | 0.7340 SOL ($69.02 / ¥10,994) |
| 最大登録数 | 8,388,608 件 |
| 1登録あたり Tree 償却 | 0.0000001 SOL ($0.000009 / ¥0.001) |

## 1登録あたり総コスト（変動費 + Tree償却）

| Tree 規模 | 最大登録数 | SOL | USD | JPY |
|-----------|----------|-----|-----|-----|
| depth=14 | 8,192件 | 0.000070 | $0.0066 | **¥1.05** |
| depth=20 | 52万件 | 0.000017 | $0.0016 | **¥0.26** |
| depth=24 | 839万件 | 0.000016 | $0.0015 | **¥0.24** |

## 結論

- depth=20 以上では Tree 償却が誤差レベルとなり、**変動費（TX fee）が支配的**
- 1コンテンツ（3 cNFT）あたり **約¥0.25**（depth=20以上）
- **100万件登録**しても Tree 初期投資はわずか **¥9,256**
- depth=20 が初期費用と容量のバランスが最も良い
- Arweave 永続保存コストは全体の 5% 未満と極めて安価
