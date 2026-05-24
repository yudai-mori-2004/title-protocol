# Q. SPECS_JA.md 自己監査 — Round 3

## スコープと方針

担当: `docs/v0.1.2/SPECS_JA.md` 全文（現状 1338 行）のみ。

Round 3 は次の三点を行う:

1. Round 2 で挙げた 10 件（must-fix 2 / should-fix 6 / nitpick 2）の処理状況を 1 件ずつ追跡する。
2. Round 2 → Round 3 の修正で新たに発生した regression を洗い出す。
3. 「初めて実装する第三者 / 攻撃者 / 監査人」の 3 視点で再読し、Round 2 まででは気付かなかった新規問題を立てる。

評価ラベル:

- **resolved**: 指摘が解消され、新たな問題も発生していない
- **partial**: 指摘の一部は解消されたが残課題あり、または修正で新たな副作用が出ている
- **regression**: 修正で新たな矛盾を作っている
- **open**: 未対応

## Round 2 指摘の処理状況（サマリ）

Round 2 の処理ログ（`audit/round2/q-spec-self.md:174-176`）には `round2-N1..N10` が一括 `wontfix(...v0.1.3 SPECS_JA リライト時に著者判断で一括整理)` と記録されている。実際に SPECS_JA.md の該当箇所を当たり直すと、Round 2 で観察対象とした全行が文言上 Round 2 時点と同一であり、修正は加えられていない。したがって全件 **open**（未対応）として扱う。

| ID | 重要度 | Round 2 指摘 | 該当行（Round 3 時点） | 評価 |
|---|---|---|---|---|
| round2-N1 | must | §1.2 と §6.2 で PCR の扱いがずれている | L149 / L1218（変化なし） | **open** |
| round2-N2 | must | "管理者" ロール（whitelist / verifying_key_hash 集合 / measurement 集合 / revoke）が未整理 | L310 / L1196 / L1208 / L1216 / L1331 | **open** |
| round2-N3 | should | "三段" "三つ" "3 つ" の 3 種混在 | L37 / L201 / L296 / L1189 / L1196 / L1200 / L1202 | **open** |
| round2-N4 | should | §1.7 の "改変は以下のいずれかで検知される" の網羅性破綻 | L321-326 | **open** |
| round2-N5 | should | §1.6 と §2.4 の "ストレージ運営者への信頼" の扱いずれ | L308 / L547 | **open** |
| round2-N6 | should | §6.2 確認 2 末尾「TEE バイナリを変更したときのみ」が PCR1/PCR2 の更新と矛盾 | L1218 | **open** |
| round2-N7 | should | §2.4 ステップ 8 と §1.7 L326 が逆向きに読める | L326 / L489-490 | **open** |
| round2-N8 | should | §5.2 自己 Attestation 失敗時のみ規範が浮いている | L1043-1045 | **open** |
| round2-N9 | nitpick | §3.2 cert-* と §1.6 Root CA 列挙の整合 | L299 / L827-829 | **open** |
| round2-N10 | nitpick | §1.2 表中 "ベンダー証明書チェーン" 行と "ベンダールート証明書の信頼起点" 節が重複 | L151 / L164 | **open** |

加えて、Round 2 で同様に処理ログ上 `wontfix` 扱いとなった Round 1 の積み残し（must-002, must-004, must-006, should-003〜009, should-011〜013, nitpick-001〜007）も依然として open のままである。Round 2 → Round 3 の間に SPECS_JA.md への編集は確認できなかった。

regression: 0 件（修正自体が行われていないため）。

## Round 3 で新たに発見した問題

Round 2 の指摘漏れ・本文 2 回目通読で見えた論点を新規 ID `round3-N*` で立てる。

### round3-N1 (must-fix) §6.2 確認 3 で衝突を起こす user_data フォーマットの曖昧さ

- 場所: `docs/v0.1.2/SPECS_JA.md:155`, `:323`, `:1180`, `:1222`, `:1262`
- 観察:
  - コア処理: `user_data = SHA-256(JCS(signature_hash + results))` (L323)
  - Solana 鍵登録: `user_data = SHA-256(Solana公開鍵)` (L1180)
  - いずれも 32 バイト固定。Attestation Document のフォーマット上、両者は区別できない。
  - L1222 確認 3: "user_data フィールドが、今登録しようとしている署名鍵の公開鍵から導出される値と一致するか"
- 問題: 同一の TEE が（コア処理レスポンス用の Attestation と Solana 鍵登録用の Attestation を）両方発行している以上、コア処理時の user_data と鍵登録時の user_data は Attestation Document のバイト列レベルでは見分けがつかない。Solana プログラムは「これは鍵登録用に発行された Attestation である」ことを直接は確認できない。攻撃者は、コア処理レスポンスとして発行された任意の Attestation Document を流用し、「user_data = SHA-256(自分の Solana 公開鍵)」になるような JCS(signature_hash + results) を構築できれば、鍵登録に成功する。
  - 厳密には: 攻撃者が `results` の中身を自由に細工し、SHA-256(JCS(signature_hash + results)) = SHA-256(自分の公開鍵) を成り立たせる必要がある。32 バイト同士の前像衝突であり、SHA-256 の数学的強度の範囲内では実用上不可能だが、**仕様文書としては「コア user_data と Solana user_data はドメイン分離されている」と明示すべき**。例: `user_data = SHA-256("title:core" || JCS(...))` と `user_data = SHA-256("title:solana-key" || pubkey)` のようにドメインタグを入れる。
- 攻撃者視点: 現状の SPECS_JA だけを読んで実装した場合、ドメイン分離が抜ける可能性が高い。「user_data はプロトコルが何にでも使える 32 バイト」という説明（L150, L153）が一貫しており、用途ごとに区別する規範が書かれていない。
- 監査人視点: SP1 guest のソースを読まないと、現実装が衝突可能なのかが判定できない。仕様だけで安全性を主張できる形にすべき。
- 修正案: §1.2 末尾または §3 の冒頭で「user_data の中身はドメインタグで分離する」とプロトコル規約として明示し、§2.3 / §6.2 の各箇所を `domain_tag || payload` 形式に揃える。

### round3-N2 (must-fix) §0.6 構成表に §2.5 と §1.4 の暗号化が落ちている／§2 全体の節構造との不一致

- 場所: `docs/v0.1.2/SPECS_JA.md:97-107`
- 観察: §0.6 構成表は `2. 通信モデル | リクエスト・レスポンス形式、暗号化オプション、Gateway API` とまとめているが、本文は §2.1〜§2.5 まで 5 節に分かれており、特に §2.4「暗号化の仕組み」と §2.5「Gateway API」は本文の半分以上を占める実装規範である。
- 問題: 「初めて読む第三者」が §0.6 から目次的に節を探すと、§2.5 が存在することが分からない。Round 1 should-003 の指摘範囲だが、Round 2 でも未対応。Round 3 で明示的に再立て。
- 修正案: §0.6 表を「セクション・サブセクション」の 2 段組にするか、最低限 §2.5 を含む文言（"Gateway API（§2.5）を含む"）に変える。

### round3-N3 (must-fix) §2.3 L467 の user_data 計算式から `attestation` 自体が抜けるかどうかが書かれていない

- 場所: `docs/v0.1.2/SPECS_JA.md:441-456`, `:467`, `:323`
- 観察:
  - §2.3 レスポンスは `{ signature_hash, results, attestation }` の 3 フィールド。
  - L467: "レスポンスの `signature_hash` + `results` をJCS（…）で正規化した上でSHA-256ハッシュを計算し、Attestation Document内の`user_data`と照合する"
  - 自然な解釈: `attestation` 自身は user_data の計算対象外（自己参照を防ぐため当然）。だが §1.7 L323 の式は `JCS(signature_hash + results)` という擬似式で、**JSON 構造として `{ "signature_hash": ..., "results": ... }` を JCS する** のか、**signature_hash の文字列と results の JSON を連結（バイト結合）する** のかが判別できない。
- 問題: JCS は JSON テキスト前提なので前者と読むのが自然だが、L323 の `signature_hash + results` は「`+` 演算」と読める。実装者がバイト結合（前者: `"sha256:..." || {"c2pa-verify": ...}` のバイト列を JCS にかける）と JSON オブジェクト化（後者: `{"signature_hash": ..., "results": ...}` を JCS にかける）の二つで分岐する。SP1 guest 側と TEE 側の実装が分かれた場合、user_data 値が一致せず確認 3 が必ず失敗する。
- 攻撃者視点: 仕様の曖昧さを利用して、検証側ライブラリと TEE 側の実装で異なる計算式を採らせれば、任意の `results` を Attestation Document の偽の user_data に通せる可能性が（理論上）残る。
- 修正案: §1.7 L323 / §2.3 L467 / §5.2 L1075 の 3 箇所すべてに、`canonical_message = JCS({ "signature_hash": sig, "results": res })` と疑似コードで明示する。バイト結合解釈を可能にする `+` 表記を排する。

### round3-N4 (should-fix) §1.4 と §1.7 で「非暗号化を選んでよい条件」がずれている

- 場所: `docs/v0.1.2/SPECS_JA.md:263`, `:325`
- 観察:
  - L263: "サーバーサイドからの利用など、通信経路の秘匿性がHTTPSで十分な場合は暗号化を省略できる"
  - L325: "サーバーサイド利用（非暗号化）: クライアントと Gateway を同一運営者が運用する前提"
- 問題: §1.4 では「HTTPS で経路が暗号化されていれば省略可」と読めるが、§1.7 では「同一運営者が運用する前提」という、より強い条件が出てくる。HTTPS だけが理由なら、サードパーティの Gateway 越しにサーバーサイド利用するケース（マネージドホスティング等）も省略可と読み取られる。§1.7 の前提は実は §1.4 の文面より厳しい。
- 修正案: §1.4 の "サーバーサイドからの利用など…" 文を削除し、「非暗号化モードを使ってよい運用条件は §1.7 を参照」と差し替える。あるいは §1.4 自体に「クライアントと Gateway が同一運営者であり、通信経路を運営者が自己統制している場合に限る」と書き込む。

### round3-N5 (should-fix) §2.4 「nonce 衝突は原理的に発生しない」は方向境界しか保証していない

- 場所: `docs/v0.1.2/SPECS_JA.md:508`
- 観察: "request_keyはクライアントがペイロードを暗号化する際に使用し、response_keyはTEEがレスポンスを暗号化する際に使用する。方向ごとに鍵が異なるため、同一鍵でのnonce衝突は原理的に発生しない。"
- 問題: 方向ごとに鍵を分けても、**同一方向内**（例えば、TEE が同じ response_key で多数のレスポンスを返す場合）の nonce 衝突は鍵の分離では防げない。AES-256-GCM の nonce は 96 bit で、ランダム nonce では誕生日攻撃で 2^48 メッセージ程度から衝突確率が無視できなくなる。
  - 実際の TEE は毎リクエストで KEM 鍵交換から共有秘密を新規導出する想定であれば、request_key / response_key もリクエストごとに新規（= ペイロード暗号化が 1 鍵 1 メッセージ）になるため衝突は起きないはず。だが仕様にその「1 鍵 1 メッセージ」が書かれておらず、「TEE の長寿命鍵で複数レスポンスを暗号化する」誤実装を許す文面になっている。
- 攻撃者視点: 誤実装が 1 鍵で多数のレスポンスを暗号化する形になれば、nonce 衝突から GHASH 鍵の漏洩 → 認証タグ偽造に繋がる。
- 修正案: §2.4「方向別鍵導出」の末尾に「リクエストごとに新しい shared_secret から request_key / response_key を導出し、各鍵は当該リクエスト内の 1 メッセージ暗号化のみに使用する。nonce はランダムまたは固定（例: ゼロ）どちらでも安全」と明示する。

### round3-N6 (should-fix) §6.2 利用フェーズの "オフチェーンデータ URL" の信頼起点が不明

- 場所: `docs/v0.1.2/SPECS_JA.md:1244-1271`
- 観察: Solana Extension リクエストは `offchain_data_url` を受け取り、TEE が「URL からデータを fetch → Attestation Document を検証」する。
- 問題: TEE はあくまで「URL の中身（= 渡された data）」の Attestation を検証するだけであり、URL 自体が cNFT に紐づくべきデータと一致するかは保証しない。攻撃者は「自分が出した別の正規 cNFT 用の Attestation Document を持つ古いオフチェーンデータ」を指す URL を渡せば、TEE は喜んで部分署名する。同じ TEE の同じ measurement で過去に発行した Attestation はすべて「正規」と見なされるため、TEE 側で URL ↔ cNFT のバインドはできない。
  - これは仕様で「リプレイは別チェーン的に防がれる」前提なのかが書かれていない。cNFT のメタデータ（merkle_tree, collection 等）が user_data に bind されていないため、同じ Attestation を別 merkle_tree / 別 collection で使い回す攻撃が考えられる。
- 修正案: Solana Extension リクエストの user_data（部分署名の対象）に `merkle_tree`, `collection`, `recent_blockhash` も bind することを §6.2 の確認 3 と「利用」で明示する。または、コア処理時の user_data に "今後 Solana Extension で再利用してよい識別子" を含めるリプレイ防止メカニズムを書き加える。

### round3-N7 (should-fix) §5.4 リプロデューシブルビルドの「同一バイナリ」を保証する具体性が薄い

- 場所: `docs/v0.1.2/SPECS_JA.md:1117-1128`
- 観察: "リプロデューシブルビルド" 節は「ソースコード、Dockerfile 等、Cargo.lock、Rust コンパイラのバージョン、ターゲットアーキテクチャを公開する」とのみ。
- 問題:
  - "Dockerfile 等" の「等」が広すぎる。Docker イメージ自体のダイジェスト固定（`FROM rust@sha256:...`）が要件か否か不明。
  - リンカ、ld のバージョン、glibc のバージョン、`/proc/sys/kernel/randomize_va_space`、ビルドホスト時刻（`SOURCE_DATE_EPOCH`）など、Rust の決定性に影響する要素が列挙されていない。
  - §1.6 信頼前提 3 でも「Rust toolchain / OS / 依存ピンの決定性が担保される」と書くが、「どこで切れば決定性が担保されるか」の境界線がない。
- 監査人視点: 監査人がビルドの再現性を確認する手順が、「公開された手順に従ってビルドを再現する」以上に具体化されておらず、再現できなかった場合の責任分界（クレーム者か検証者か）が定義されていない。
- 修正案: §5.4 に最低限「`SOURCE_DATE_EPOCH` の指定」「`RUSTFLAGS="--remap-path-prefix"` の使用」「ベース Docker イメージのダイジェスト固定」「Cargo の `[profile.release]` 設定の固定」を必要要素として列挙する。

### round3-N8 (should-fix) §4.4 上限値の根拠と運用変更手順が未定義

- 場所: `docs/v0.1.2/SPECS_JA.md:960-968`
- 観察:
  - "フラグメントの最大数 100,000"
  - "フラグメント 1 個の最大サイズ 100 MB"
  - "来歴グラフの最大サイズ 10,000"
- 問題: これらの数値は TEE バイナリにコンパイル時定数として埋め込まれることが暗黙で前提されている（measurement が変わるため）。だが、運用上「100 MB では足りないユースケースが出てきた」場合に、誰がどう判断して変更し、measurement の更新と検証者への周知が回るのかが書かれていない。
- 修正案: §4.4 末尾に「これらの上限は TEE バイナリのビルド時定数であり、変更には measurement の更新を伴う。集合の運用ポリシー（§6.2）に従う」と明記する。

### round3-N9 (should-fix) §6.2 ホワイトリスト追加権限の表現と "管理者" 描写の衝突

- 場所: `docs/v0.1.2/SPECS_JA.md:1196`, `:1208`, `:310`
- 観察:
  - L1196: ホワイトリスト PDA への新規署名鍵追加は「プログラムのみが持ち、三段の同一性確認をすべて通過した ZK proof でのみ…**人手による管理は介在しない**」
  - L1208: verifying_key_hash 集合の更新権限は **管理者のみ** が持つ
  - L310: Solana Extension は **whitelist 管理者** を信頼前提に加える
- 問題: L1196 の「人手による管理は介在しない」が強い断言であるため、初読者は §1.6 L310 の「whitelist 管理者を信頼前提に加える」が誤りに見える。実態は「署名鍵集合自体は無人で増えるが、その手前の verifying_key_hash 集合と measurement 集合と revoke 操作は管理者の権限」という三層構造。round2-N2 が指摘した管理者ロール未整理と本件は同根だが、Round 3 では「L1196 と L310 が文面上どう読めるか」の不整合をピンポイントで指摘しておく。
- 修正案: L310 の括弧を「Solana Extension は verifying_key_hash 集合・measurement 集合の管理者、および revoke 権限保持者を信頼前提に加える。これらの集合更新が正しく運用される限り、署名鍵集合への追加自体は ZK proof のみで自動化される」と書き直す。

### round3-N10 (should-fix) §0.5 設計原則 "E2EE Optional" と §1.7 の "同一運営者前提" の整合

- 場所: `docs/v0.1.2/SPECS_JA.md:95`, `:325`
- 観察:
  - L95: 設計原則として "E2EE Optional | クライアントからTEEへの通信は、暗号化・非暗号化の両方を受け付ける"
  - L325: 非暗号化はクライアントと Gateway が同一運営者の場合のみ
- 問題: 設計原則の段では "両方を受け付ける" と無条件、§1.7 では「同一運営者前提」と条件付き。設計原則の節で条件を匂わせない書き方は、原則として書いたものを後段で否定するように読める（Round 1 should-007 と同根）。
- 修正案: L95 を「E2EE Optional | クライアントが秘匿性を必要としない、または通信経路を運営者が自己統制する場合は非暗号化も受け付ける」と書く。

### round3-N11 (nitpick) §2.4 「方向別鍵導出」と「対応スイート」表で `salt=encap_key` の意図が一切説明されない

- 場所: `docs/v0.1.2/SPECS_JA.md:504-508`
- 観察: HKDF-SHA256 の `salt` に `encap_key` を流用する設計が天下り式で書かれている。Round 2 must-002 で「セッション境界の意図が未定義」と指摘されたが、Round 3 時点でも説明追記なし。
- 問題: HKDF の `salt` は「セッション間の鍵分離」のために使うのが標準だが、`encap_key` は KEM 出力（X25519 なら公開エフェメラル鍵）であり、すでに `shared_secret` に取り込まれている要素を salt に再投入する設計の根拠が読者に伝わらない。salt が KEM 出力の場合、ハッシュ衝突耐性以上の意味は持たない。なぜそうしたかの 1 行が無い。
- 修正案: L505 の直下に「salt は HKDF 出力が KEM ごとに独立になることを保証する目的。`shared_secret` のみでは KEM 実装由来の弱点（出力が ephemeral 公開鍵に対して関数的）を吸収できないため、`encap_key` を salt に取り込み HKDF-Extract 段階で domain-separation する」程度の意図説明を加える。

### round3-N12 (nitpick) §3.2 cert-* の名称揺れ（Round 2 N9 / Round 1 nitpick-005 の継続）

- 場所: `docs/v0.1.2/SPECS_JA.md:827-829`
- 観察:
  - cert-google: "Google C2PA Root CA G3"
  - cert-sony: "SONY C2PA Root CA G2"
  - cert-leica: "Leica C2PA Root CA"（世代記号なし）
- 問題: Leica だけ世代記号が欠けている。実在しないのか、命名規則に揃えるべきかが読み取れない。L299「ルート証明書（Google / Sony / Leica 等）」も同じ揺れを引き継ぐ。
- 修正案: Leica の C2PA Root CA の現行世代を調査の上で記載するか、未確定なら「Leica C2PA Root CA（世代記号未定）」と明記する。

### round3-N13 (nitpick) §0.1 と §0.4 で C2PA v2.3 リリース日表記が孤立している

- 場所: `docs/v0.1.2/SPECS_JA.md:9`, `:33`
- 観察: L9 "v2.3（2026年1月）が現行安定版である"。L33 "ライブストリーミング（v2.3で追加）"。
- 問題: 本仕様書全体で C2PA バージョンに依存した記述（マニフェスト構造、フラグメンテッド形式の扱い）がいくつもあるが、L9 以外でバージョン依存性を明記している箇所はない。読者が「v2.3 のどの仕様を引いているか」を追えない。
- 修正案: §0.1 末尾に「以下、本書で C2PA と書く場合は C2PA v2.3 を指す。バージョン差異が問題になる箇所では明示する」と一文加える。

### round3-N14 (nitpick) §0.1 図中の罫線文字と本文中の図の罫線スタイル混在

- 場所: `docs/v0.1.2/SPECS_JA.md:13-25` (Unicode 罫線), `:119-139` (Unicode + 全角矢印), `:517-521` (ASCII 罫線), `:568-572` (Unicode 罫線)
- 観察: ASCII art の図が Unicode 罫線（`┌`, `─`, `└`）と全角矢印（`──→`）の混在で書かれている。一部の図（§4.2 等）は Unicode、別の図（§2.4 レスポンスフォーマット）は ASCII 風。同一文書内で図のスタイルが不揃い。
- 修正案: 1 つのスタイルに揃える。Unicode 罫線は等幅フォントを前提するため、エディタで開いたときに崩れる可能性がある。GitHub の `<pre>` 表示前提なら Unicode 推奨。

## 全体所感

Round 2 で挙げた 10 件は SPECS_JA.md の編集が行われなかったためすべて open のままであり、これは処理ログに `wontfix(v0.1.3 SPECS_JA リライト時に著者判断で一括整理)` と記録された方針通り。Round 3 監査としては、その方針を尊重しつつも、**v0.1.3 リライト時に確実に拾うべき構造的な問題** が積み上がっていることを示す結果になった。

Round 3 で新規発見した 14 件のうち、最も重要なのは:

- **round3-N1**: user_data ドメインの分離が仕様で未定義。コア user_data と Solana 鍵登録 user_data が原理上区別できない書きぶり。攻撃可能性は実用上 0 に近いが、仕様書としてドメイン分離規約を欠くことは OSS 公開品質に影響する。
- **round3-N3**: `JCS(signature_hash + results)` の `+` 演算の意味が二通りに読めるため、ライブラリ実装と TEE 実装が乖離した場合に user_data 不一致を生む。
- **round3-N6**: Solana Extension の URL 由来データのリプレイ防止が仕様化されていない。攻撃者が同 TEE の別の正規 Attestation を使い回せる経路が概念上残る。

これら 3 件は Round 2 の must-fix 群（特に round2-N1, N2）と合わせ、v0.1.3 リライトの最優先候補とすべきである。

修正計画への提言:

1. v0.1.3 リライトでは「§0 構成表の精緻化」「§1.6 信頼前提と §6.2 管理者ロールの統合節」「§1.7 検知レイヤの網羅性整理」「user_data ドメインタグの導入」の 4 軸で章立てを見直す。
2. 表記揺れ（round2-N3 三段/三つ/3 つ、round2-N9 / round3-N12 Leica 世代）は文体統一パスとして一括で処理する。
3. round3-N5（nonce 衝突）と round3-N11（HKDF salt 意図）は「暗号化セッションの寿命と鍵スコープ」の独立節を §2.4 内に新設するのが妥当。

---

## 処理ログ

| ID | 判定 |
|---|---|
| round2-N1..N10 | inherited-open（Round 2 wontfix の継承。v0.1.3 リライトで一括整理） |
| round3-N1 | fixed | core 処理用 user_data に `b"title:core"`、Solana 鍵登録用 user_data に `b"title:solana-key"` のドメインタグを導入。SPECS §1.7 / §2.3 / §5.2 / §6.2 に式と意図を明記。実装側は `crates/tee/src/orchestrator.rs` (core 計算)、`crates/solana/src/extension.rs` (core 検証)、`crates/solana/src/signing_key.rs::solana_key_user_data` (Solana 鍵側計算)、`programs/title-whitelist/src/lib.rs` (on-chain 検証) を全部新式に揃える。`pubkey_hash()` は新名 `solana_key_user_data()` に置換し、対応する unit test も新仕様に更新。 |
| round3-N2 | wontfix(v0.1.3 SPECS_JA リライト時に著者判断で一括整理。§0.6 構成表は本ラウンドの修正範囲外、Round 3 で SPECS_JA 大幅編集は避ける) |
| round3-N3 | fixed | §2.3 L467 の `JCS(signature_hash + results)` の `+` 演算曖昧表記を「`{ "signature_hash": ..., "results": ... }` を JCS 正規化」と擬似コードで明示する 4 ステップ手順に書き直し。§1.7 / §5.2 / §6.2 の関連箇所も同じ式に揃え。実装側 (`compute_jcs_hash`) は既に JSON オブジェクト経由なので修正不要。 |
| round3-N4 | open |
| round3-N5 | open |
| round3-N6 | open |
| round3-N7 | open |
| round3-N8 | open |
| round3-N9 | open |
| round3-N10 | open |
| round3-N11 | open |
| round3-N12 | open |
| round3-N13 | open |
| round3-N14 | open |
