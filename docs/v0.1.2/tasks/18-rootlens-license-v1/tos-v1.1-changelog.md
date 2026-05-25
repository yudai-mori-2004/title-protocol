# ToS v1.0.0 ドラフト v1.1 変更履歴

4法務エージェント (Singapore/EU/UK/Japan/起草品質) のレビュー結果を反映。

## 新規追加セクション

| 節 | 内容 | 解決した CRITICAL/HIGH |
|---|---|---|
| §1.11 | "Consumer" 定義 | Consumer Carve-Out の前提 |
| §2.1 | 同意フロー2段階分離 (ToS 同意 vs データ処理) | GDPR Art.7(2)(4) bundle 問題 |
| §2.4 | EU/UK 14日撤回権の明示放棄 | Directive 2011/83 Art.16(m) |
| **§2A** | **Consumer Carve-Out (10項)** | EU class waiver / 日本仲裁法附則3条 / Brussels I Art.18 / 消契法第8条 / Rome I Art.6 / 個情法第28条 / 消契法第3条 |
| §6.1A | Grantor liability 不引受 (undisclosed principal 防止) | SG 代理人モデル再分類リスク |
| §6.1B | 代理人義務範囲限定 (黙示信認義務排除) | 同上 |
| §6.4A | CRTPA invocation (第三者受益条項) | privity 問題 |
| §11.4 | 法定救済の保持カーブアウト | UK CRA / SGSA / 消契法第8条の2 |
| §12.3 | 上限例外の追加 (重過失・人身傷害・詐欺・統計消費者権) | UCTA s.2(1) / 消契法第8条 |
| §13.2-6 | GDPR 法的根拠の書き直し (legitimate interest 化)、EU 代表者、データ主体権利 | GDPR Art.6/7/27 |
| §16.2 | Pre-arbitration informal resolution (30日) | SIAC モデル節 |
| §16.5 | SIAC Rules との整合確保 (同意ベース併合) | class waiver の内部矛盾 |
| §16.6 | 裁判所救済の対称化 | CPFTA unfair practice 防止 |
| **§16A** | **DSA notice-and-action / appeals / ODR / trusted flaggers / 反復侵害者** | DSA Art.14-22 |

## 修正されたセクション

| 節 | 修正内容 |
|---|---|
| At-a-glance | Carve-Out と DSA への参照追加 |
| §1.6 | "cNFT" を "non-fungible token on Solana" に汎用化 |
| §1.8 | "Signal" 短縮形を定義、"CDSM Directive" 定義追加 |
| §1.10 | cross-ref を §17 → §17.9 に修正 |
| §3.2 | 54語の長文を (a)(b) に分割。UNSC と SG TSOFA 追加 |
| §5.4 | "obscene under jurisdictions where lawfully accessed" の論理矛盾を修正 |
| §7.1(c) | UK CDPA s.29A の誤った引用を正しい記述に修正 |
| §10 | 防御主体を被補償者に反転、Loss 定義語化、Consumer cap への ref |
| §11.1-2 | 本文を ALL CAPS BOLD 化 (米国 conspicuousness 基準) |
| §12.1-2 | 本文を ALL CAPS BOLD 化、上限を SGD 100 → EUR 500 |
| §14 | 変更理由列挙、30日通知期間、ユーザー終了権を明示 (UCTD Annex 1(j)(k) 対応) |
| §15.3 | survival list に §2A, §16A を追加 |
| §16.1 | 仲裁合意の準拠法を明示 (Anupam Mittal リスク回避)、非契約紛争カバー |
| §17.9 | ハッシュ手順を3段階に明文化 (LF 正規化、空白除去、ハッシュ自体の非包含) |

## 未解決の論点 (法務カウンセル判断必須)

1. **著作権等管理事業法該当性** — RootLens の代理人モデルが文化庁登録対象になる可能性。ToS 文言で対応不可、サービス構造の判断が必要
2. **資金決済法・前払式支払手段** — License NFT のフローが為替取引・資金移動業に該当しないことの整理
3. **KYC 統合** — 第5条の表明保証を本人確認なしで強制可能か
4. **登記住所・UEN・EU 代表者** — ローンチ前に確定要
5. **発効日** — 同上
6. **ToS v1.0.0 確定 → SHA-256 計算 → TP 側 `TOS_HASH` 定数埋め込み → TEE 再ビルド → 新 PCR0 を Solana 登録**

## 次のアクション

- root-lens 側で `/api/v1/tos/consent` API および同意 UI を実装 (Task 14 スコープ)
- root-lens 側で Privacy Policy、`/privacy/lia`、`/privacy/cross-border`、`/fees` を作成
- Singapore + EU + 日本の3拠点で正式法務カウンセル依頼
- 文化庁への著作権等管理事業法該当性照会
