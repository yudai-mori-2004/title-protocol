# ToS v1.0.0 ハッシュ確定前の前提条件

本書は、`tos-v1.0.0-draft.md` (および日本語版) を確定し、SHA-256 ハッシュを計算し、TEE に `TOS_HASH` として埋め込み、Solana に新 PCR0 を登録するまでに、root-lens 側または運営者側で完了している必要のある事項を列挙する。

ToS 本文に書かれた約束のうち、これらが満たされない状態で deploy するとそれは「文書上の嘘」になる。

---

## カテゴリA: 文書本文が直接参照する成果物

ToS 本文が `参照により取り込まれる` または明示的に URL を参照しているもの。これが存在しないまま deploy すると ToS は内部参照切れになる。

### A-1. Privacy Policy 文書 (CRITICAL — §13.1 がハッシュ共埋め込み参照)

- パス: `https://rootlens.io/privacy` (ToS と同一ドメイン)
- 要件:
  - GDPR 第13条・第14条の通知義務を独立して満たすこと
  - 個人情報保護法第21条の通知事項を満たすこと
  - 越境移転に関する第28条所要の情報 (日本ユーザーへの情報提供) を含むこと
  - データ主体の権利の行使方法 (連絡先は ToS §18 と同じ `yudai.mori@moodai.jp`)
  - 法的根拠を明示 (ToS §13.2 と整合)
  - 保管期間、アクセス制御、データ主体の権利
- **Privacy Policy v1.0.0 の SHA-256 ハッシュも独立に計算し、TEE ソフトウェアに `PRIVACY_POLICY_HASH` 定数として共埋め込みする**。これは ToS §13.1 が要求する。Privacy Policy も同様に正本テキストの境界マーカー方式でハッシュ可能な形式 (Markdown) で作成し、ToS と同じハッシュ手順を適用する。
- 担当: 運営者 / 法務カウンセル

### A-2. ToS 正本コピーの公開 (HIGH — §17.9 が参照)

- パス: `https://rootlens.io/tos/v1.0.0/{Authoritative Hash}.txt`
- 同 `/ja/{Authoritative Hash}.txt` (日本語版)
- 計算手順 (§17.9):
  1. 本文ファイルの先頭バイトから「正本本文の末尾」行末の LF (0x0A) まで
  2. 行末改行は LF に正規化
  3. 各行の末尾空白を除去
  4. ハッシュ値そのものは本文に含めない
- 担当: 運営者

### A-3. License テンプレート (済 — §6.3 が参照)

- パス: `https://rootlens.io/licenses/{type}/{hash}.json`
- 状態: 4種 deploy 済 (commercial-v1, non-commercial-v1, training-only-v1, redistribution-v1)
- 不要追加作業: License 内の `governing_law: Singapore` と ToS の準拠法を一致させる確認のみ

---

## カテゴリB: ToS が約束する運用処理

ToS が運営者の行為として約束しているもの。インフラを作らないと「実行できない約束」になる。

### B-1. 二段階同意フロー (CRITICAL — §2.1)

- 第1画面「I Agree to the Terms」: ToS への同意
- 第2画面: 個人データ処理の説明 (法的根拠、処理内容、ユーザーの権利)
- 担当: root-lens app

### B-2. consent log の保管 (CRITICAL — §13.2(b))

- 保管項目: `wallet_pubkey + tos_version + tos_hash + ip + user_agent + timestamp`
- ストレージ: append-only (アプリケーション設計上の)
- アクセス制御: 同意立証目的以外には利用しない
- 担当: root-lens server / DB

### B-3. 違法コンテンツ通報受付 (MEDIUM — §16A)

- 受付窓口: `yudai.mori@moodai.jp`
- 受領確認の運用 (時間内に返信できる体制)
- 判断後の通知 (通知者および対象クリエイターへの理由付き回答)
- 担当: 運営者

### B-4. データ主体の権利受付 (MEDIUM — §13.5)

- 受付窓口: `yudai.mori@moodai.jp`
- GDPR Art.15-22 + 個情法対応の処理手続 (Privacy Policy で詳細を定義)
- 担当: 運営者

### B-5. EU consumer 撤回権の明示 UI (HIGH — §2.4)

- ミント実行画面に「直ちに履行を開始することを要求し、14日撤回権を失う」明示的確認 UI
- ユーザーが EEA/UK/CH 居住の場合のみ表示する判定
- 担当: root-lens app

### B-6. 変更通知の30日前周知 (LOW — §14.2)

- アプリ内通知機能
- 担当: root-lens app (将来要件)

---

## カテゴリC: 運営者の法的整備

ToS の発効と矛盾しないために、運営者側で完了している必要のあるもの。

### C-1. 屋号「moodai」での個人事業の届出 (HIGH)

- 税務署への開業届
- 屋号の届出 (本人確認時に屋号が表示できる形)
- 担当: 運営者

### C-2. 著作権等管理事業法 該当性の構造的回避 (運営者判断: 文化庁照会は行わない)

ToS v1.1 で構造をリフレーム済み (代理人モデル → マーケットプレイス/発行支援者モデル)。著作権等管理事業法の登録要件を以下の構造で回避する:

- **ユーザーが個別に価格を決定**: 価格設定画面でいつでも自由設定可能 (ToS §6.7(a))
- **デフォルトは市場参考値の機械的提示**: 運営者の裁量による価格設定ではない (ToS §6.7(b))
- **非推奨価格は売れにくいが、それは買い手の自由意志による市場結果**: 運営者がアルゴリズム的に非推奨価格を劣後させない (ToS §6.7(c))
- **当社はライセンスを管理せず、交渉せず、条件を決定しない**: ToS §6.1B で明示
- **ユーザーが付与者、当社は発行を技術的に支援するのみ**: ToS §6.1 で明示
- **エンフォースは別途授権がある場合のみ**: 管理事業の典型行為と切り離し (ToS §6.2(b))

**運用上の必須遵守**:
1. 推奨価格は機械的に導出された市場参考値であること (例: 過去30日の類似コンテンツ成約価格中央値)
2. 推奨価格の算出方法は公開すること
3. UI 上、非推奨価格を意図的に劣後表示しないこと
4. ユーザーが価格を自由に変更できる UI を常に提供すること

文化庁への正式照会は費用面・時間面の負担を考慮し、現時点では行わない。万一監督官庁から問い合わせがあった場合は上記構造を根拠に説明する。違法判定を受けた場合は (1) 該当機能の停止または (2) 著作権等管理事業者として登録する選択肢を残す。

- 担当: 運営者

### C-3. 資金決済法・前払式支払手段の整理 (HIGH)

- License NFT のフローが為替取引・資金移動業・前払式支払手段に該当しない整理
- 購入者→クリエイター直接決済を維持し、運営者は手数料のみ受領する建付け
- 担当: 法務カウンセル

### C-4. 法務カウンセル正式レビュー (HIGH)

- Singapore + EU + 日本3拠点
- 本ドラフトを発効可能な状態に最終調整
- 担当: 法務カウンセル

### C-5. ドメイン rootlens.io の所有確認 (LOW)

- ドメイン所有者が運営者本人 (または運営者が支配する組織) であることを確認
- WHOIS プライバシーの設定確認
- 担当: 運営者

---

## カテゴリD: rootlens-license-v1 processor 側の対応

ToS が確定し SHA-256 が算出されたら、TP 側で:

### D-1. `TOS_HASH` および `PRIVACY_POLICY_HASH` 定数の追加 (`crates/core/src/rootlens_license_v1.rs`)

```rust
const TOS_HASH: &str = "0x<64-hex SHA-256 of ToS v1.0.0 EN>";
const PRIVACY_POLICY_HASH: &str = "0x<64-hex SHA-256 of Privacy Policy v1.0.0>";
```

### D-2. `tos_url` および `privacy_policy_url` 定数の追加

```rust
const TOS_URL: &str = "https://rootlens.io/tos/v1.0.0/<TOS_HASH>.txt";
const PRIVACY_POLICY_URL: &str = "https://rootlens.io/privacy/<PRIVACY_POLICY_HASH>.txt";
```

### D-3. `RootLensBinding` 構造体に `tos_hash` / `tos_url` / `privacy_policy_hash` / `privacy_policy_url` フィールド追加

ToS §13.1 が要求するとおり、Privacy Policy ハッシュも TEE アテステーションに記録する。

### D-4. TEE 再ビルド → 新 PCR0 を Solana の `approved_measurements` に登録

### D-5. binding_rule 文書の作成と hash 埋め込み (別タスク)

これは TP 固有の責務。`rootlens_binding.purpose: "sublicense-grant-eligibility"` の意味、各フィールドの解釈ルールを定義する文書。完成次第 `BINDING_RULE_HASH` / `BINDING_RULE_URL` を processor に追加。

---

## 完了の順序

1. **C-1, C-3 運営者整備 + C-2 構造的回避の運用設計**: 屋号届出、推奨価格算出方法の文書化と公開
2. **C-4 法務カウンセル正式レビュー** (任意だが推奨)
3. **A-1 Privacy Policy 作成** (ハッシュ算出含む)
4. **B-1, B-2, B-5 root-lens 側実装**: 同意 UI + consent log + 撤回権 UI + 価格設定 UI
5. **A-2 ToS 正本の deploy**: ハッシュ計算 + URL 公開
6. **D-1〜D-4 TP 側埋め込み**: TOS_HASH + PRIVACY_POLICY_HASH を TEE に共埋め、新 PCR0 を Solana に登録
7. **B-3, B-4 運用窓口の稼働確認**

---

## 完了するまでの位置づけ

それまでの間、`tos-v1.0.0-draft.md` の `Status:` フィールドは `DRAFT — NOT FOR DEPLOYMENT` のままとする。完了したら `Status: ACTIVE` に更新し、その変更も含めて再度 SHA-256 を計算する。

ToS の `Status:` フィールドの変更前後でハッシュが変わる点に注意 — ハッシュ計算は Status が `ACTIVE` の状態で行う。draft の状態のハッシュは記録に意味がない。

---

## 旧 changelog の扱い

`tos-v1.1-changelog.md` は法務レビュー結果の変更履歴であり、本書は前提条件のチェックリスト。両者は別物として残す。
