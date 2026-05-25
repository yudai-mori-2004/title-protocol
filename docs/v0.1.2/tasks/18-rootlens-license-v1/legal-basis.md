# rootlens-license-v1 Processor: 法的根拠

本書は `rootlens-license-v1` processor の設計が法的に成立する根拠を整理する。法務レビュー用の内部文書。

> **注記**: 本文書は 2026-05-24 時点の法的環境に基づく。EU AI Act の欧州委員会施行細則 (TDM プロトコルリスト等) は未公表。日本の eシール制度は 2026 年 3 月開始だが判例は未形成。

---

## 1. Processor の役割と限界

### 1.1 何をするか

`rootlens-license-v1` は Title Protocol の TEE (Trusted Execution Environment) 内で動作する processor である。以下を行う:

1. コンテンツの SHA-256 ハッシュ (`content_hash`) を計算する
2. コンテンツの C2PA マニフェストに埋め込まれた **CAWG (Creator Assertions Working Group)** training-mining assertion (`cawg.training-mining`, CAWG v1.1 仕様) が存在することを検証する。注: TDM assertion は C2PA 仕様自体には含まれない — CAWG が C2PA マニフェスト拡張として定義した assertion である
3. RootLens のサブライセンス枠組みに関するメタデータ (`rootlens_binding`) を content_hash と共に出力する

出力は TEE のハードウェア署名 (AWS Nitro Enclaves Attestation Document) で封印される。第三者は TEE 署名を検証することで、上記 1-3 の結果が改ざんされていないことを確認できる。

### 1.2 何をしないか

- **ToS 同意の証明**: processor は撮影者が利用規約に同意したかどうかを検証しない
- **著作権の帰属確認**: 撮影者が著作権者であるかの検証はしない
- **ライセンスの発行**: processor はメタデータを生成するのみで、License NFT の発行や権利の付与は行わない

これらは意図的な設計であり、§2 で法的根拠を述べる。

---

## 2. 技術的証明と法的証明の分離

### 2.1 設計原則

本 processor の設計は、**技術的 (暗号的) に証明可能な事実** と **法的レイヤーで証明すべき事実** を明確に分離する。

TEE は「特定のコードが、特定の入力に対して、特定の出力を生成した」ことをハードウェアレベルで証明できる。しかし、人間の意思表示 (同意)、法的地位 (著作権者であること)、実世界の事実 (コンテンツに第三者 IP が含まれていないこと) は、TEE の証明能力の範囲外である。

これらを TEE に証明させようとすると、TEE に外部入力 (ToS 同意ログ、KYC データ等) を渡す必要が生じ、攻撃面が広がる。代わりに、各事実を最も適切なメカニズムで証明する:

| 証明すべき事実 | 証明メカニズム | 信頼の根拠 |
|---|---|---|
| コンテンツの同一性 | TEE 内の SHA-256 計算 | ハードウェア署名 (Attestation Document) |
| C2PA 署名の有効性 | TEE 内の c2pa-rs 検証 | 同上 |
| TDM opt-out 信号の存在 | TEE 内の CAWG assertion 検証 | 同上 |
| RootLens フレームワークでの処理 | rootlens_binding のハードコード定数 | PCR0 (TEE バイナリの測定値) |
| 撮影者の ToS 同意 | アプリの同意フロー + consent log API | アプリ設計 + サーバー記録 |
| 著作権の帰属 | KYC + 利用規約の表明保証 | 契約法 + 本人確認 |

### 2.2 ToS 同意の証明: 「NFT の存在 = 同意済みパイプラインの通過」

RootLens のアプリは以下の順序を強制する:

```
ToS 同意 (アプリ UI) → コンテンツ submit → TEE 処理 → Root NFT mint
```

ToS に同意していないユーザーはコンテンツを submit できない。したがって、Root NFT がオンチェーンに存在すること自体が「撮影者は ToS に同意した上でコンテンツを提出した」ことの間接的証拠となる。

この論理は、クリックスルー同意の法的有効性として確立されている:

- **米国**: *Meyer v. Uber Technologies, Inc.*, 868 F.3d 66 (2d Cir. 2017) — 合理的に明瞭な提示 + 明示的同意操作があれば、クリックスルー同意は拘束力ある契約として成立
- **日本**: 民法第 522 条 (諾成主義) + 第 548 条の 2 (定型約款) — 当事者の意思表示の合致で契約成立、書面不要。適切な提示 + 明示的同意操作で執行可能 (法曹実務の合意)

加えて、RootLens は同意記録を独立して保管する:

- `POST /api/v1/tos/consent` で `wallet_pubkey + tos_version + tos_hash + ip + user_agent + timestamp` を append-only で永続化
- `GET /api/v1/tos/consent?wallet=<pubkey>` で第三者が同意記録を照会可能

**consent log の限界**: この consent log は RootLens が運用する中央サーバーに保管される。ハードウェアに根ざした耐改ざん性 (TEE attestation のような) は持たない。「append-only」はアプリケーション設計上の制約であり、ストレージレベルでの不変性保証ではない。紛争時には、consent log の改ざんの有無が争点になり得る。ただし、以下の補強要素により実務上の証拠価値は確保される:
  - オンチェーンの cNFT mint トランザクション (タイムスタンプ + 署名者)
  - アプリコードの設計上、同意フローをバイパスできない構造 (ソースコード自体が証拠)
  - 将来的に consent log のハッシュをオンチェーンに定期 anchor する設計も可能

TEE processor が ToS 同意を「暗号的に証明」する必要はない。法的証明は上記の多層メカニズム (アプリ設計 + consent log + オンチェーン記録) で十分に成立する。

### 2.3 processor が `tos_version` を出力する意味

processor は `tos_version: "v1.0.0"` をハードコードで出力する。これは ToS 同意の証明ではなく、**「この TEE バイナリは ToS v1.0.0 のフレームワーク下で動作するよう構成されていた」という事実のラベル**である。

ToS が改訂されると:
1. processor のソースコード内の `tos_version` 定数を更新
2. TEE バイナリを再ビルド → PCR0 (バイナリ測定値) が変わる
3. 新 PCR0 を Solana の approved_measurements に追加
4. 旧 PCR0 で発行された Root NFT は旧 ToS バージョンにバインドされたまま

PCR0 と tos_version の 1:1 対応により、Root NFT がどの ToS バージョンのフレームワークで発行されたかが改ざん不可能な形で記録される。

---

## 3. TDM opt-out assertion 検証の法的意義

### 3.1 EU CDSM Art.4(3) の要件

EU Directive 2019/790 (CDSM) 第 4 条第 3 項は、著作権者が「適切な方法、オンラインで提供される場合は機械可読な手段を含む」で TDM 利用の留保を表明した場合、TDM 例外 (第 4 条第 1 項) が適用されないと定める。

EU AI Act (Regulation 2024/1689) 第 53 条第 1 項 (c) は、GPAI モデル提供者に対し、CDSM 第 4 条第 3 項の留保を「最先端の技術により」識別・遵守する方針の策定を義務付ける (2025 年 8 月施行済)。同条は法的拘束力を持つが、欧州委員会が策定する施行細則 (TDM プロトコルの具体的リスト等) は 2026 年 5 月時点で未公表。

**GPAI Code of Practice (2025 年 7 月)** は任意の自主規範であり、非署名者に対する法的拘束力はない。ただし、AI Act Art.56(2) により、Code への遵守は Art.53 義務の「推定的遵守」として認められる。

### 3.1.1 日本 著作権法 30 条の 4 但書

日本市場において特に重要。著作権法第 30 条の 4 は、著作物の「非享受利用」(情報解析を含む) を原則として権利制限するが、但書で「著作権者の利益を不当に害することとなる場合」を除外する。

文化審議会 著作権分科会 法制度小委員会 (2024 年 3 月報告書) は、以下の場合に但書が適用され得るとする:
- **ライセンス市場の存在**: 著作物について AI 学習用のライセンスが利用可能な場合 (RootLens はまさにこの市場を構築)
- **大規模・反復的な利用**: 特定の著作者の作品を意図的に大量収集する場合

RootLens のサブライセンスフレームワークは、30 条の 4 但書における「著作権者の利益を不当に害する」の判断に影響する可能性がある。すなわち、RootLens がライセンスを提供しているにもかかわらず無断で AI 学習に利用した場合、但書適用の根拠が強まる。TEE で TDM opt-out 信号の存在を証明しておくことは、この文脈で証拠価値を持つ。

### 3.2 CAWG training-mining assertion の位置づけ

CAWG (Creator Assertions Working Group) の training-mining assertion (v1.1, 2025 年 5 月 ratified) は、C2PA マニフェストに埋め込む形で TDM 利用の許諾/禁止を表明する。ラベル: `cawg.training-mining`。

**注意: これは C2PA 仕様ではなく CAWG 仕様である。** C2PA は公式に TDM assertion を C2PA 仕様の一部ではないと表明している。CAWG が C2PA マニフェストの拡張ポイントを利用して定義した assertion であり、法的引用時には「C2PA assertion」ではなく「CAWG training-mining assertion (C2PA マニフェスト埋込)」と記載すべきである。

法的認知の現状:

- **EUIPO 研究 (2025 年 5 月)**: CAWG TDM assertion を 8 つの技術的手段の 1 つとしてリストアップ
- **欧州委員会 stakeholder consultation (2025 年 12 月開始)**: C2PA/CAWG を評価対象のプロトコルに含める。ただし最終的なプロトコルリストは 2026 年 5 月時点で未公表であり、CAWG の採用は不確定
- **GPAI Code of Practice (2025 年 7 月)**: robots.txt の遵守を明示的にコミット。CAWG は名指しされていないが、「その他の適切な機械可読プロトコル」として射程内。ただし Code of Practice 自体は任意の自主規範であり、非署名者を拘束しない (AI Act Art.53 自体は法的拘束力を持つ)
- **OLG Hamburg (2025 年 12 月, Kneschke v. LAION)**: 「機械可読」ではなく「**機械実行可能 (machine-actionable)**」でなければならないと判示。CAWG assertion は技術的には machine-readable だが、現在のところ主要な AI クローラーが CAWG assertion をパースする機能を実装していないため、machine-actionable の基準を満たさないリスクがある

**結論**: CAWG assertion 単独での TDM opt-out の法的有効性は未確定であり、特に OLG Hamburg の machine-actionable 基準への適合は疑わしい。ただし、RootLens は CAWG に加えて robots.txt + `.well-known/tdm.json` + `llms.txt` を既に deploy しており (root-lens `web/public/` 参照)、多層シグナルの一部として機能する。robots.txt は machine-actionable の基準を明確に満たす (主要クローラーが実際に遵守) ため、CAWG は「追加的シグナル」の位置づけとなる。

### 3.3 TEE での TDM 検証が追加する価値

TEE 内で `cawg.training-mining` assertion を検証することで、**「コンテンツ X が TEE に提出された時点で、TDM 留保信号が C2PA マニフェスト内に存在していた」** ことが TEE 署名で証明される。

これは以下の場面で証拠として有用:

1. **紛争時の立証**: AI 企業がコンテンツを無断でスクレイプした場合、「TDM 留保信号はスクレイプ以前から存在していた」ことを TEE attestation で立証できる
2. **EU AI Act 遵守の文書化**: GPAI 提供者がライセンスなしでコンテンツを使用した場合、「留保信号は最先端の技術 (C2PA) で機械可読に表明されていた」ことの証拠となる
3. **改ざん防止**: C2PA マニフェストは後から編集可能だが、TEE attestation は特定時点での状態を改ざん不可能な形で記録する

### 3.4 processor のゲート機能

本 processor は、`cawg.training-mining` assertion が存在しないコンテンツに対してエラーを返す (Root NFT の発行を阻止する)。

これにより、RootLens の Root NFT コレクション内の全コンテンツが TDM opt-out 信号を含むことが保証される。コレクション単位での一貫性は、AI 企業との交渉および法的紛争において「RootLens は体系的に TDM 留保を実施している」ことの証拠となる。

---

## 4. ハードコード定数の正当性

### 4.1 なぜ外部入力ではなくハードコードか

`rootlens_binding` のフィールド (license_program_id, license_collection_mint, tos_version 等) は、processor のソースコードにハードコードされる。クライアントからの入力として受け取らない。

理由:

1. **攻撃面の縮小**: クライアント入力を受け取ると、偽の license_program_id や tos_version を送り込む攻撃が可能になる。ハードコードなら TEE 内部で完結する
2. **PCR0 による自然なバージョン管理**: 定数を変更すると TEE バイナリが変わり、PCR0 が変わる。旧 PCR0 と新 PCR0 は Solana 上で別々に管理される。どの Root NFT がどのバージョンの定数で発行されたかが PCR0 で一意に特定できる
3. **変更頻度の低さ**: これらの値は ToS 改訂やプログラム ID 変更時にしか変わらない (年に数回以下)。TEE 再ビルドのコストは許容範囲内

### 4.2 ToS 文書のハッシュが未定の状態での出荷

現時点で ToS フルテキスト文書は root-lens 側で未完成 (root-lens Task 14 のスコープ)。そのため、`tos_hash` と `tos_url` は初期バージョンの processor 出力に含めない。

これは法的に問題ない:

- processor が出力する `tos_version: "v1.0.0"` は「このフレームワークが参照する ToS バージョン」のラベルである
- ToS 文書のハッシュ検証は第三者検証チェーン (root-lens SPECS §4.4.6) の一部であり、ToS 文書が完成してホスティングされた時点で検証可能になる
- processor に `tos_hash` を追加した時点で TEE 再ビルド → 新 PCR0 → Solana 更新。検証チェーンの完全性はその時点で成立する

段階的な完成は設計上の意図であり、各段階で出力されるフィールドの範囲内では法的に一貫している。

**段階的追加の優先順位**: `binding_rule_hash` / `binding_rule_url` は `tos_hash` / `tos_url` よりも優先すべきである。理由: binding rule は TEE が出力する `rootlens_binding` の意味 (「この出力が何を意味するか」のルール定義) を規定するものであり、TEE 出力の解釈に直結する。一方 ToS は撮影者 (人間) とプラットフォーム間の契約であり、TEE 出力の解釈とは独立している。TEE 出力に対する第三者の信頼を先に確立するには、binding rule の完成を優先する方が合理的である。

---

## 5. TEE Attestation の証拠としての法的位置づけ

### 5.1 証拠能力

TEE attestation document (AWS Nitro Enclaves の COSE_Sign1 署名付き文書) は、以下の法的枠組みで証拠として認められる:

- **EU eIDAS (Regulation 910/2014) Art.46**: 電子文書は電子形式であることのみを理由に証拠能力を否定されない
- **EU eIDAS 2.0 (Regulation 2024/1183) Art.45b**: Electronic Attestation of Attributes (EAA) の新規定。TEE attestation は qualified EAA に該当しないが、non-qualified な電子属性証明として Art.46 の下で証拠能力を持つ。将来的に TEE attestation が EU trust service framework に位置づけられる可能性を示唆
- **日本 民事訴訟法 第 247 条 (自由心証主義)**: 裁判官が証拠を自由に評価。TEE attestation の暗号的保証 (ハードウェア署名、オペレータ偽造不可) は、DB ログよりも高い証拠としての信頼性を持つ
- **日本 eシール制度 (2026 年 3 月開始)**: 総務省の「e シールに係る認定制度」。法人の電子証明を対象とし、認定 e シールは電子文書の発信元と非改ざんを証明する。TEE attestation は現時点で e シールの枠組みには入らないが、法人署名 (RootLens as operator) の文脈で e シール認定との連携が将来的に検討対象となり得る
- **IETF RATS (RFC 9334)**: Remote Attestation Procedures のアーキテクチャ標準。TEE attestation の技術的フレームワークが国際標準で定義されていることは、裁判所が技術的信頼性を評価する際の参考材料となる
- **日本 電子署名法 第 3 条**: 構造的に適用対象外。同条の「十分な固有性」は「本人だけが行うことができることとなるもの」— すなわち自然人と署名鍵の結びつきを前提とする。TEE attestation の署名鍵はハードウェアが生成・管理するものであり、特定の自然人に帰属しない。よって推定効の適用は構造上不可
- **日本 民事訴訟法 第 228 条第 1 項**: 文書の証拠提出には「成立の真正」の証明が必要。TEE attestation の場合、(a) AWS Nitro root certificate chain の検証、(b) PCR0 測定値とビルド成果物の対応が立証手段となる。ただし (b) は現時点で reproducible build 未実装のため、RootLens 自身が公表する PCR0 値に依存する (§5.2 参照)

### 5.2 法的推定効との差 — qualified timestamp との比較

TEE attestation は eIDAS の qualified electronic timestamp や qualified electronic seal の法的推定効は持たない。つまり、裁判所が TEE attestation の内容を「正しい」と推定する法的義務はない。

**qualified timestamp との関係は「強弱」ではなく「証明対象の差異」**:

| 属性 | Qualified Timestamp | TEE Attestation |
|---|---|---|
| 時刻の正確性 | 法的推定効あり | なし (Solana ブロックで補完) |
| データ非改ざん | ハッシュで証明 | ハッシュで証明 (同等) |
| コード同一性 (PCR0) | 不可 | **TEE 固有の能力** |
| オペレータ非介入性 | 不可 | **TEE 固有の能力** |
| 法的推定効 | あり (eIDAS) | なし |

TEE attestation は qualified timestamp が証明できない属性 (コード同一性、オペレータ非介入性) を証明できる。逆に、時刻の法的推定効は持たない。両者は補完関係にあり、単純な強弱比較は不適切である。

TEE のハードウェア署名チェーン (AWS Nitro → AWS root certificate → COSE_Sign1 → PCR0 measurement → user_data hash) が実務上高い証拠価値を持つ理由:

1. **ハードウェアの物理的隔離**: 署名鍵は NSM (Nitro Security Module) 内にあり、ソフトウェアからアクセスできない
2. **オペレータの非介入性**: TEE 内のコードはオペレータ (RootLens) が実行時に変更できない。PCR0 がコードの同一性を保証する
3. **検証の再現性**: PCR0 は TEE バイナリのビルドで独立に検証できる。ただし、**reproducible build は現時点で未実装**であり、PCR0 の第三者検証は RootLens が公表する期待値に依存する。これは現時点での実用上の限界であり、将来的に reproducible build を実装することで解消される
4. **AWS の信頼チェーン**: AWS の root certificate は公開されており、第三者が独立に attestation を検証可能
5. **IETF RATS (RFC 9334)** に沿った Remote Attestation アーキテクチャであり、国際標準に基づく技術的信頼性を持つ

### 5.3 ブロックチェーンとの組み合わせ

TEE attestation のハッシュが Solana の cNFT として記録されることで、以下の追加的証拠価値が生まれる:

- **タイムスタンプ**: Solana のブロック timestamp により、attestation が特定時点で存在したことが分散合意で証明される
- **改ざん不可能性**: cNFT の leaf hash は Merkle tree に含まれ、事後的な変更が検知される
- **公開検証**: 誰でも Solana の RPC から cNFT のメタデータを取得し、TEE 署名を検証できる

---

## 6. 法務レビュー依頼事項

本 processor の実装に関して、以下の論点について法務レビューを依頼する:

### 6.1 確認すべき論点

1. **「NFT の存在 = ToS 同意済み」の論理の有効性**: アプリが ToS 同意を submit の前提条件として強制する設計において、Root NFT のオンチェーン存在を ToS 同意の間接的証拠として法的に援用できるか。各法域 (シンガポール法、日本法、EU法) での評価。consent log の中央サーバー管理に起因する改ざんリスクへの評価も含む
2. **CAWG `cawg.training-mining` assertion の EU CDSM Art.4(3) 適合性**: CAWG assertion (C2PA 仕様外) が「適切な方法…機械可読な手段」に該当するか。OLG Hamburg の machine-actionable 基準との整合性。robots.txt + tdm.json との多層シグナルで補完は十分か
3. **TEE attestation の証拠価値**: AWS Nitro Attestation Document を著作権紛争の証拠として提出する場合の証拠力の評価。民訴法 228 条 1 項の成立の真正の立証方法。電子署名法第 3 条は構造上適用外 (§5.1 参照) — 代替的な法的根拠の有無
4. **日本 著作権法 30 条の 4 但書**: ライセンス市場 (RootLens) の存在が但書適用に与える影響。TEE で TDM opt-out 信号の存在を証明することの証拠価値
5. **ハードコード定数の段階的追加**: `binding_rule_hash` を `tos_hash` より優先する判断の妥当性。段階的出荷が第三者検証チェーンの完全性に影響しないか
6. **PCR0 によるバージョン管理の法的解釈**: TEE バイナリの測定値 (PCR0) を「どの ToS バージョンのフレームワークで発行されたか」の識別子として法的に援用できるか。reproducible build 未実装の現状での限界

### 6.2 関連する一次資料

- root-lens `legal-rationale.md` §2.2 (クリックスルー同意), §2.7 (EU CDSM TDM)
- EU CDSM Directive 2019/790 Art.4(3), Recital 18
- EU AI Act (Reg. 2024/1689) Art.53(1)(c), Recitals 105-107; Art.56(2) (Code of Practice の推定的遵守)
- OLG Hamburg, 5 U 104/24 (2025-12-10): Kneschke v. LAION (「machine-actionable」基準)
- CAWG Training and Data Mining Assertion v1.1 (2025-05 ratified) — C2PA 仕様外
- eIDAS Regulation 910/2014 Art.46; eIDAS 2.0 (Reg. 2024/1183) Art.45b (EAA)
- 日本 民事訴訟法 第 228 条第 1 項 (成立の真正)、第 247 条 (自由心証主義)
- 日本 電子署名法 第 3 条 (推定効 — TEE 署名は構造上適用外、§5.1 参照)
- 日本 著作権法 第 30 条の 4 但書; 文化審議会 著作権分科会 法制度小委員会 報告書 (2024 年 3 月)
- 日本 e シール制度 (総務省、2026 年 3 月開始)
- IETF RFC 9334 (RATS: Remote Attestation Procedures Architecture)
- *Meyer v. Uber Technologies, Inc.*, 868 F.3d 66 (2d Cir. 2017)
- 日本 民法 第 522 条、第 548 条の 2

---

## 7. 結論

`rootlens-license-v1` processor の設計は以下の法的構造に基づく:

1. **TEE は技術的に証明可能な事実のみを証明する** (コンテンツ同一性、C2PA検証、CAWG TDM assertion存在、フレームワーク帰属)
2. **法的事実は各々に適切なメカニズムで証明する** (ToS同意→consent log+アプリ設計+オンチェーン記録、著作権→KYC+表明保証)
3. **CAWG の TDM assertion 検証は EU CDSM / 日本著作権法 30 条の 4 但書対応の多層シグナルの一環** として法的に有用。単独で十分かは未確定 (特に OLG Hamburg の machine-actionable 基準への適合が疑問) だが、robots.txt + tdm.json + llms.txt との併用で防御層を構成する
4. **ハードコード定数は TEE の信頼モデルに適合し、PCR0 によるバージョン管理と整合する**
5. **TEE attestation は法的推定効こそ持たないが、qualified timestamp とは証明対象が異なる固有の価値を持つ** (コード同一性、オペレータ非介入性)。IETF RATS (RFC 9334) に沿った国際標準ベースの技術的信頼性を有する
6. **現時点の限界**: reproducible build 未実装 (PCR0 の第三者検証が制限)、consent log の中央サーバー管理 (耐改ざん性の限界)、電子署名法第 3 条の推定効は構造上適用外。これらは設計上の認識済み制約であり、将来の改善で対処可能
