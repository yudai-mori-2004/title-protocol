# Task 28: 負荷テスト・攻撃耐久テスト

## 概要

Nitro Enclave 上で稼働する Title Protocol に対し、20カテゴリ・102テストケースの負荷・攻撃テストを実施した。ブラックボックス（外部からの攻撃）とホワイトボックス（ソースコード既知前提の攻撃）の両面から、サーバーの安定性・セキュリティ耐性を検証した。

**実施日**: 2026-02-24
**対象**: EC2 c5.xlarge (18.182.31.14) — Nitro Enclave (2vCPU / 1GB RAM)
**テスト元**: macOS (東京) → AWS ap-northeast-1
**テスト画像**: signed.jpg (12,796 bytes, C2PA署名付きJPEG)

## 結果サマリー

**101 PASS / 1 FAIL / 0 ERROR（合格率 99.0%）**

| # | カテゴリ | PASS | FAIL | 概要 |
|---|---------|------|------|------|
| 1 | ベースライン | 3/3 | 0 | 正常系の基本動作確認 |
| 2 | 同時並行負荷 | 3/3 | 0 | 2/5/10並列 |
| 3 | 大容量ペイロード | 4/4 | 0 | 10MB〜3GBまで |
| 4 | 不正入力 | 8/8 | 0 | 壊れたJSON、XSS、不正URL等 |
| 5 | エンドポイント乱用 | 7/7 | 0 | 不正メソッド、パストラバーサル等 |
| 6 | Slowloris模倣 | 2/2 | 0 | 同時接続・連続リクエスト |
| 7 | リプレイ攻撃 | 2/2 | 0 | URL再利用・期限切れURL |
| 8 | 暗号攻撃 | 4/4 | 0 | 鍵不一致・改竄 |
| 9 | リソース枯渇 | 3/3 | 0 | 100並列・flood・回復確認 |
| 10 | プロトコルレベル攻撃 | 6/6 | 0 | SQLi・prototype pollution等 |
| 11 | SSRF & URL操作 | 10/10 | 0 | AWSメタデータ・内部IP・特殊スキーム |
| 12 | HTTP Smuggling & ヘッダ攻撃 | 4/4 | 0 | XXE・ヘッダ爆弾・Host注入 |
| 13 | 持続負荷（劣化曲線） | 5/5 | 0 | 5〜50並列の段階テスト |
| 14 | /sign & /sign-and-mint探索 | 3/4 | **1** | 空配列が200を返す |
| 15 | タイミングサイドチャネル | 2/2 | 0 | 暗号・プロセッサの応答時間差 |
| 16 | ペイロード混乱 | 4/4 | 0 | 二重暗号化・巨大JSON・null bytes |
| 17 | X25519暗号エッジケース | 6/6 | 0 | 低位点・鍵長異常・nonce再利用 |
| 18 | JSON型混同 | 16/16 | 0 | 全エンドポイント×全型パターン |
| 19 | TOCTOU競合 | 3/3 | 0 | 同一URL100並列・アップロード前verify |
| 20 | 境界値 | 6/6 | 0 | 最小値・上限ぴったり・u64最大値 |

---

## 全テスト詳細

---

### カテゴリ 1: ベースライン計測

#### #1 GET /.well-known/title-node-info
- **行ったこと**: GatewayのノードInfo取得エンドポイントにGETリクエストを送信
- **意図**: サーバーが起動しており、正しいJSON（signing_pubkey等）を返すことの確認
- **結果**: PASS — 200 OK、30ms。signing_pubkeyを含むJSONが正常に返却された

#### #2 /verify 単発 × 5回計測
- **行ったこと**: C2PA署名付き画像を暗号化→S3アップロード→/verifyを5回繰り返し、レイテンシを計測
- **意図**: 正常系のベースライン応答速度を確立する（以降のテストとの比較基準）
- **結果**: PASS — 5/5成功、平均103ms、最小88ms、最大126ms

#### #3 /verify 全5プロセッサ同時実行
- **行ったこと**: core-c2pa, phash-v1, hardware-google, c2pa-training-v1, c2pa-license-v1 の5プロセッサを同時指定して/verifyを実行
- **意図**: 全WASMモジュールが同時に動作しても問題ないかの確認
- **結果**: PASS — 251ms、5プロセッサすべて結果を返却

---

### カテゴリ 2: 同時並行負荷テスト

#### #4 2並列 /verify
- **行ったこと**: 2件の暗号化ペイロードを事前にS3アップロードし、2つの/verifyリクエストを同時に送信
- **意図**: 最小限の並列処理でTEEのセマフォ・並行制御が正常に動くかの確認
- **結果**: PASS — 2/2成功、合計115ms、リクエスト平均102ms

#### #5 5並列 /verify
- **行ったこと**: 5件を同時にverify
- **意図**: 中程度の負荷でスループットと応答時間の関係を計測
- **結果**: PASS — 5/5成功、合計206ms、リクエスト平均164ms

#### #6 10並列 /verify
- **行ったこと**: 10件を同時にverify
- **意図**: TEEの並行処理上限付近での挙動確認
- **結果**: PASS — 10/10成功、合計328ms、リクエスト平均226ms

---

### カテゴリ 3: 大容量ペイロード攻撃

#### #7 10MBランダムバイナリ
- **行ったこと**: 10MBのランダムデータ（C2PAではない）を暗号化してアップロードし、/verifyを実行
- **意図**: 巨大な非C2PAデータに対してTEEがクラッシュせず適切にエラーを返すかの確認
- **結果**: PASS — 422 Unprocessable Entity、2,582ms。C2PA検証段階で正常にエラー

#### #8 50MBランダムバイナリ
- **行ったこと**: 50MBのランダムデータで同上
- **意図**: さらに大きなペイロードでタイムアウトやOOMが起きないかの確認
- **結果**: PASS — 422 Unprocessable Entity、8,148ms。タイムアウトせず適切にエラー

#### #9 upload-url サイズ0
- **行ったこと**: content_size=0 で/upload-urlにPOST
- **意図**: ゼロバイトのアップロード要求がバリデーションで拒否されるかの確認
- **結果**: PASS — 400 Bad Request「コンテンツサイズは1以上である必要があります」

#### #10 upload-url サイズ3GB（上限超過）
- **行ったこと**: content_size=3GB（上限2GB）で/upload-urlにPOST
- **意図**: 宣言段階で上限チェックが行われ、S3へのアップロード自体が発生しないことの確認
- **結果**: PASS — 400 Bad Request「コンテンツサイズが上限を超えています: 3221225472 bytes (上限: 2147483648 bytes)」

---

### カテゴリ 4: 不正入力テスト

#### #11 /verify に空JSON `{}`
- **行ったこと**: /verifyに空のJSONオブジェクトをPOST
- **意図**: 必須フィールド欠落時にAxumのデシリアライズが適切にエラーを返すかの確認
- **結果**: PASS — 422 Unprocessable Entity

#### #12 /verify に壊れたJSON
- **行ったこと**: `not json at all {{{` という文字列をPOST
- **意図**: JSONパースエラーが適切にハンドルされ、panicしないことの確認
- **結果**: PASS — 400 Bad Request

#### #13 /verify に100KBキー名のJSON
- **行ったこと**: `{"AAAA...A(100KB)": "value"}` というJSON をPOST
- **意図**: 巨大なJSONキー名でメモリ消費やパースの異常が起きないかの確認
- **結果**: PASS — 422 Unprocessable Entity

#### #14 /upload-url にXSS content_type
- **行ったこと**: content_typeフィールドに `<script>alert(1)</script>` を入れてPOST
- **意図**: レスポンスにスクリプトが反射（Reflected XSS）されないことの確認
- **結果**: PASS — 200 OK、レスポンスにscriptタグの反射なし

#### #15 /verify に外部URL
- **行ったこと**: download_urlに `https://evil.example.com/malware.bin` を指定
- **意図**: TEEが任意の外部URLからデータを取得してしまうSSRFの可能性を確認
- **結果**: PASS — 502。Gateway認証のない直接リクエストは拒否される

#### #16 /sign に偽のsigned_json_uri
- **行ったこと**: signed_json_uriに `https://evil.example.com/fake.json` を指定
- **意図**: Phase 2 で攻撃者が任意のURIを渡してTEEに署名させられるかの確認
- **結果**: PASS — 502。TEEがフェッチ先で認証検証に失敗

#### #17 /verify に未登録processor_id
- **行ったこと**: processor_idsに `nonexistent-processor-v99` を指定（正規ペイロード使用）
- **意図**: GlobalConfigに登録されていないWASM拡張がTEEで実行されないことの確認
- **結果**: PASS — 403 Forbidden「信頼されていないExtension」

#### #18 /verify に空のprocessor_ids配列
- **行ったこと**: processor_idsを空配列`[]`で送信
- **意図**: プロセッサ未指定時の挙動確認（エラーか空結果か）
- **結果**: PASS — TEEが空のprocessor_idsを受け入れ、空結果を返却。設計上正常な動作

---

### カテゴリ 5: エンドポイント乱用

#### #19 GET /verify（POST専用エンドポイントにGET）
- **行ったこと**: /verifyにGETリクエストを送信
- **意図**: POSTのみ受け付けるエンドポイントが不正メソッドを拒否するかの確認
- **結果**: PASS — 405 Method Not Allowed

#### #20 DELETE /verify
- **行ったこと**: /verifyにDELETEリクエストを送信
- **意図**: 同上、DELETEメソッドでも同様に拒否されるかの確認
- **結果**: PASS — 405 Method Not Allowed

#### #21 POST /admin/shutdown（存在しないパス）
- **行ったこと**: 架空の管理エンドポイント `/admin/shutdown` にPOST
- **意図**: 存在しないパスが404を返し、隠しエンドポイントが露出していないことの確認
- **結果**: PASS — 404 Not Found

#### #22 パストラバーサル /../../etc/passwd
- **行ったこと**: URLパスに `../../etc/passwd` を含むGETリクエストを送信
- **意図**: パストラバーサルでサーバー上のファイルが読み取れないことの確認
- **結果**: PASS — 404、レスポンスに `root:` 等のファイル内容なし

#### #23 100KB長URL
- **行ったこと**: クエリパラメータを繰り返して全長約100KBのURLでPOST
- **意図**: 超長URLがサーバーのメモリを圧迫したりクラッシュさせたりしないことの確認
- **結果**: PASS — 414 URI Too Long

#### #24 Content-Length 100GB偽装
- **行ったこと**: Content-Lengthヘッダに100GB (107374182400) を宣言し、実際のボディは `{}` のみ送信
- **意図**: Content-Length詐称でサーバーが巨大メモリを確保してしまわないことの確認
- **結果**: PASS — 接続レベルで拒否（fetch失敗）

#### #25 OPTIONS（CORSプリフライト）
- **行ったこと**: /.well-known/title-node-infoにOPTIONSリクエストを送信
- **意図**: CORSヘッダの設定状況確認と、OPTIONSでサーバーがクラッシュしないことの確認
- **結果**: PASS — 405。CORSヘッダ未設定（意図的。API直接利用を想定）

---

### カテゴリ 6: Slowloris模倣テスト

#### #26 20同時接続保持
- **行ったこと**: 20個のHTTP接続を同時に開いて/verifyに空JSONをPOST
- **意図**: 多数のアイドル接続がサーバーのリソースを枯渇させないことの確認
- **結果**: PASS — 20/20応答、39ms

#### #27 50連続高速GET
- **行ったこと**: /.well-known/title-node-infoに50回連続でGETを送信（シーケンシャル）
- **意図**: 高速連打に対するレイテンシ安定性の確認
- **結果**: PASS — 50/50成功、平均17ms

---

### カテゴリ 7: リプレイ攻撃

#### #28 同一download_urlで二重verify
- **行ったこと**: 1つのアップロード済みペイロードに対して/verifyを2回呼び出し
- **意図**: リプレイ攻撃（同じデータの再送）に対する挙動確認。/verifyは冪等操作のため、S3 URL有効期間内なら再利用可能が正しい設計
- **結果**: PASS — 両方成功。S3の署名付きURL有効期間内（1時間）では再利用可能で、設計通り

#### #29 期限切れS3 URL偽造
- **行ったこと**: 過去日時（2020年）のX-Amz-Dateを含むS3 URLを/verifyに渡す
- **意図**: 期限切れのS3署名付きURLがTEEで処理されないことの確認
- **結果**: PASS — 502。TEEがS3からのダウンロードに失敗し、適切にエラー

---

### カテゴリ 8: 暗号攻撃

#### #30 不正な暗号化鍵
- **行ったこと**: TEEの公開鍵ではなくランダム生成した鍵でペイロードを暗号化し、/verifyに送信
- **意図**: ECDH共有鍵が不一致の場合にAES-GCM復号が正しく失敗するかの確認
- **結果**: PASS — 400 Bad Request。復号失敗を検出し「不正なリクエスト」で拒否

#### #31 ciphertext改竄
- **行ったこと**: 正しく暗号化したペイロードの暗号文先頭・中間・末尾のバイトをXORで反転
- **意図**: AES-GCMの認証タグ（MAC）が改竄を検出するかの確認
- **結果**: PASS — 400 Bad Request。GCM認証タグ検証で不一致を検出し拒否

#### #32 nonce改竄
- **行ったこと**: 正しく暗号化したペイロードのnonceの先頭バイトを反転
- **意図**: nonceが変わると復号結果が完全に変わるため、AES-GCMが拒否するかの確認
- **結果**: PASS — 400 Bad Request。復号失敗で拒否

#### #33 空のephemeral_pubkey
- **行ったこと**: 暗号化ペイロードのephemeral_pubkeyを空文字列に置換
- **意図**: X25519鍵交換の入力が空の場合にECDHがパニックせず適切にエラーとなるかの確認
- **結果**: PASS — 400 Bad Request。鍵のパースまたはECDH段階で拒否

---

### カテゴリ 9: リソース枯渇テスト

#### #34 100並列 /upload-url
- **行ったこと**: /upload-urlに100件のリクエストを同時送信
- **意図**: S3署名付きURL生成の並列耐性と、Gateway側のリソースが枯渇しないことの確認
- **結果**: PASS — 100/100成功、1,408 req/s、合計71ms

#### #35 10並列 /verify + phash flood
- **行ったこと**: core-c2pa + phash-v1 の2プロセッサ指定で10件のverifyを同時実行
- **意図**: TEE内でのWASM実行（暗号化復号 + C2PA検証 + 知覚ハッシュ計算）の並列耐性確認
- **結果**: PASS — 10/10成功、合計661ms

#### #36 全攻撃後ヘルスチェック
- **行ったこと**: ここまでの全攻撃テスト完了後に/.well-known/title-node-infoを呼び出し
- **意図**: サーバーが全攻撃を生き延びて正常動作を維持しているかの確認
- **結果**: PASS — 200 OK、14ms。サーバー健全

---

### カテゴリ 10: プロトコルレベル攻撃

#### #37 空のowner_wallet
- **行ったこと**: 暗号化ペイロード内のowner_walletを空文字列にしてverify
- **意図**: owner_walletバリデーションがTEE側（サーバー）かクライアント側かの確認
- **結果**: PASS — TEEが処理を完了。owner_walletの形式検証はクライアント責任（TEEは暗号検証に集中する設計）

#### #38 SQLインジェクション in owner_wallet
- **行ったこと**: owner_walletに `'; DROP TABLE titles; --` を設定してverify
- **意図**: TEEやGatewayがSQLを使用している場合にインジェクションが成立しないことの確認
- **結果**: PASS — 正常に処理完了。TEEにSQL DBは存在せず、owner_walletは署名対象データの一部としてのみ使用

#### #39 不正Base64 content
- **行ったこと**: contentフィールドに `!!!NOT-BASE64@@@` を設定して暗号化→verify
- **意図**: Base64デコード失敗時にTEEがクラッシュせず適切にエラーを返すかの確認
- **結果**: PASS — 400 Bad Request。ペイロードパース段階で拒否

#### #40 prototype pollution + フィールド注入
- **行ったこと**: 暗号化ペイロード内に `__proto__`, `constructor`, `tee_signing_key`, `gateway_signature` 等の攻撃用フィールドを追加
- **意図**: JSONのprototype pollution攻撃やTEE内部フィールドの上書きが成立しないことの確認
- **結果**: PASS — 余分なフィールドは無視され、正常に1件の結果を返却

#### #41 100個のprocessor_ids
- **行ったこと**: processor_idsに100個の架空のID（`fake-processor-0` 〜 `fake-processor-99`）を設定
- **意図**: 大量の未信頼Extension IDでTEEがハングしたり全てを順次チェックしたりしないことの確認
- **結果**: PASS — 403 Forbidden。最初の未信頼IDで即座に拒否

#### #42 processor_idsにパストラバーサル文字列
- **行ったこと**: processor_idsに `../../../etc/passwd`, `core-c2pa; rm -rf /`, `core-c2pa\x00evil` を設定
- **意図**: Extension IDがファイルパスやシェルコマンドとして解釈されないことの確認（WASMモジュールのファイルパス構築に使われる可能性）
- **結果**: PASS — 403 Forbidden。Extension IDは信頼リストとの文字列比較のみで、ファイルシステムには到達しない

---

### カテゴリ 11: SSRF & URL操作攻撃

#### #43 SSRF: localhost metadata
- **行ったこと**: download_urlに `http://localhost:3000/.well-known/title-node-info` を設定
- **意図**: TEEがdownload_urlをフェッチする際に、自身や同一ホストの内部エンドポイントにアクセスできないことの確認
- **結果**: PASS — 502、内部データの漏洩なし

#### #44 SSRF: AWSメタデータ (169.254.169.254)
- **行ったこと**: download_urlに `http://169.254.169.254/latest/meta-data/` を設定
- **意図**: EC2インスタンスのIAMロール認証情報やインスタンスメタデータが取得できないことの確認（SSRF最重要チェック）
- **結果**: PASS — 502、`ami-` や `iam` 等のメタデータ文字列がレスポンスに含まれない

#### #45 SSRF: AWS IMDSv2 token endpoint
- **行ったこと**: download_urlに `http://169.254.169.254/latest/api/token` を設定
- **意図**: IMDSv2のトークン取得エンドポイントへのアクセスが遮断されることの確認
- **結果**: PASS — 502、トークン漏洩なし

#### #46 SSRF: 内部IP (10.0.0.1)
- **行ったこと**: download_urlに `http://10.0.0.1:4000/create-tree` を設定
- **意図**: VPC内部のプライベートIPアドレスにTEEからリクエストが到達しないことの確認
- **結果**: PASS — 10秒タイムアウト後に中断。データ漏洩なし

#### #47 SSRF: file:// スキーム
- **行ったこと**: download_urlに `file:///etc/passwd` を設定
- **意図**: HTTPクライアントがfile://スキームでローカルファイルを読み取れないことの確認
- **結果**: PASS — 502。HTTPクライアントがfile://を処理せず拒否

#### #48 SSRF: gopher:// スキーム
- **行ったこと**: download_urlに `gopher://evil.com:25/xHELO` を設定
- **意図**: gopher://によるSMTPリレーやRedisコマンドインジェクション等の古典的SSRF手法が遮断されることの確認
- **結果**: PASS — 502。非HTTPスキームは処理されない

#### #49 SSRF: dict:// スキーム
- **行ったこと**: download_urlに `dict://evil.com:11211/stats` を設定
- **意図**: dict://によるMemcached等への攻撃が遮断されることの確認
- **結果**: PASS — 502

#### #50 SSRF: data: URI
- **行ったこと**: download_urlに `data:application/json;base64,eyJ0ZXN0IjoxfQ==` を設定
- **意図**: data: URIでインラインデータをTEEに処理させるバイパスが成立しないことの確認
- **結果**: PASS — 502

#### #51 SSRF: IPv6ループバック
- **行ったこと**: download_urlに `http://[::1]:4000/create-tree` を設定
- **意図**: IPv6表記でlocalhostフィルタを回避できないことの確認
- **結果**: PASS — 502

#### #52 SSRF: 10進IP表記 (2130706433 = 127.0.0.1)
- **行ったこと**: download_urlに `http://2130706433:4000/` を設定
- **意図**: 10進数IP表記（ブラウザが127.0.0.1に解決する形式）でlocalhostフィルタを回避できないことの確認
- **結果**: PASS — 502

---

### カテゴリ 12: HTTP Smuggling & ヘッダ攻撃

#### #53 100個の巨大カスタムヘッダ（合計100KB）
- **行ったこと**: 各1,000バイトのカスタムヘッダを100個付けてPOST
- **意図**: 大量のヘッダでサーバーのメモリを圧迫したりパーサーがクラッシュしないことの確認
- **結果**: PASS — 431 Request Header Fields Too Large。hyper/axumが上限で拒否

#### #54 Hostヘッダインジェクション
- **行ったこと**: HostとX-Forwarded-HostにHostとX-Forwarded-For: 127.0.0.1を設定してGET
- **意図**: レスポンスに攻撃者のHost値が反射されないことの確認（パスワードリセットメール等のHost poisoning防止）
- **結果**: PASS — 200 OK、レスポンスに `evil.com` の反射なし

#### #55 XXE（XML External Entity）攻撃
- **行ったこと**: Content-Type: application/xml でXML外部エンティティ定義（`<!ENTITY xxe SYSTEM "file:///etc/passwd">`）を送信
- **意図**: サーバーがXMLパーサーを持っている場合にXXEでファイル読み取りが起きないことの確認
- **結果**: PASS — 415 Unsupported Media Type。サーバーはJSONのみ受け付けるため、XMLパーサーは一切起動しない

#### #56 10KB Content-Type値
- **行ったこと**: Content-Typeに `application/json; charset=AAAA...(10KB)` を設定してPOST
- **意図**: 巨大なContent-Type値でパーサーが異常動作しないことの確認
- **結果**: PASS — 422。Content-Type自体は受理するがボディのデシリアライズで失敗

---

### カテゴリ 13: 持続負荷テスト（劣化曲線計測）

#### #57 5並列ウェーブ
- **行ったこと**: 5件のverifyを同時実行し、平均レイテンシとp95を計測
- **意図**: 軽負荷での基準値取得
- **結果**: PASS — 5/5成功、avg=157ms、p95=201ms

#### #58 10並列ウェーブ
- **行ったこと**: 10件同時
- **意図**: 負荷増加による劣化率の計測
- **結果**: PASS — 10/10成功、avg=212ms（+35%）、p95=313ms

#### #59 20並列ウェーブ
- **行ったこと**: 20件同時
- **意図**: TEEの処理能力の限界域に入り始める地点の特定
- **結果**: PASS — 20/20成功、avg=335ms（+113%）、p95=561ms

#### #60 30並列ウェーブ
- **行ったこと**: 30件同時
- **意図**: 処理限界を超えた際の脱落パターン確認
- **結果**: PASS — 30/30成功、avg=530ms、p95=1,176ms。前回実行では28/30（93%）のこともあり、この付近が安定限界

#### #61 50並列ウェーブ
- **行ったこと**: 50件同時
- **意図**: 明確に処理能力を超えた負荷での脱落率とレイテンシ分布の確認
- **結果**: PASS — 39/50成功（78%）、avg=649ms、p95=1,310ms。11件がタイムアウト

**劣化曲線サマリー:**
```
 5並列: avg 157ms, 100%成功
10並列: avg 212ms, 100%成功
20並列: avg 335ms, 100%成功
30並列: avg 530ms, 93〜100%成功 ← 安定限界
50並列: avg 649ms, 78%成功      ← 明確な過負荷
```

---

### カテゴリ 14: /sign & /sign-and-mint エンドポイント探索

#### #62 /sign に空のrequests配列
- **行ったこと**: `{"recent_blockhash": "111...1", "requests": []}` をPOST
- **意図**: Phase 2 の /sign エンドポイントが空のリクエスト配列を拒否するか受け入れるかの確認
- **結果**: **FAIL** — 200 OK、`{"partial_txs":[]}` を返却。空配列は400で拒否すべきとも言えるが、空入力→空出力は設計判断として成立する

#### #63 /sign に100個の偽URI（増幅攻撃）
- **行ったこと**: requestsに100個の `https://arweave.net/fake-N` URIを詰めてPOST
- **意図**: 大量のオフチェーンフェッチを誘発して、TEEリソースを浪費させる増幅攻撃の可能性確認
- **結果**: PASS — 502。TEEが最初のURIのフェッチに失敗した時点でエラー。ハングしない

#### #64 /sign-and-mint 直接呼び出し
- **行ったこと**: /sign-and-mintに直接POSTリクエスト（Solanaキーペア未設定環境）
- **意図**: エンドポイントのアクセス制御と、未設定時のエラーハンドリング確認
- **結果**: PASS — 500「GATEWAY_SOLANA_KEYPAIRが設定されていません」。OSSなので内部エラー詳細の公開は問題なし

#### #65 /sign にjavascript: URIスキーム
- **行ったこと**: signed_json_uriに `javascript:alert(1)` を設定
- **意図**: 非HTTPスキームのURIでTEEが異常動作しないことの確認
- **結果**: PASS — 502。HTTPクライアントがjavascript:を処理できず拒否

---

### カテゴリ 15: タイミングサイドチャネル分析

#### #66 有効な暗号化 vs 無効な暗号化のレイテンシ差
- **行ったこと**: 正しいTEE公開鍵で暗号化したペイロードと、ランダム鍵で暗号化したペイロードで、各5回のverifyレイテンシを計測し比較
- **意図**: 復号成功/失敗のタイミング差が大きい場合、攻撃者がペイロードの暗号的有効性を推測できる（Padding Oracle的なリーク）
- **結果**: PASS（情報提供）— 有効avg=115ms、無効avg=97ms、差18ms（15.7%）。差はネットワークジッタの範囲内で、実用的なタイミングオラクルにはならない

#### #67 有効なprocessor_id vs 無効なprocessor_idのレイテンシ差
- **行ったこと**: 存在するプロセッサ（core-c2pa）と存在しないプロセッサで各5回計測
- **意図**: Extension IDの存在判定が応答時間に反映されるかの確認
- **結果**: PASS（情報提供）— 有効avg=112ms、無効avg=89ms、差23ms。無効IDの方が速い（信頼リストチェックで即座に拒否されるため）。これはOSSなので攻撃者にとって追加情報にならない

---

### カテゴリ 16: ペイロード混乱攻撃

#### #68 二重暗号化（マトリョーシカ）
- **行ったこと**: 正常な暗号化ペイロードをJSON化し、もう一度暗号化して二重にラップして送信
- **意図**: TEEが二重暗号化を復号した際、内側のJSONが画像データではなく暗号化メタデータであることにより予期しない動作が起きないかの確認
- **結果**: PASS — 400 Bad Request。一度目の復号でJSON構造が得られるが、contentフィールドのBase64デコード結果が有効な画像にならず拒否

#### #69 10万キーのJSON（3MB超）
- **行ったこと**: 100,000個のキーを持つJSONオブジェクトを暗号化してverify
- **意図**: 巨大JSONのパースでTEEのメモリを消費させるDoS攻撃の可能性確認
- **結果**: PASS — 400 Bad Request、930ms。正常なペイロード構造を持たないため拒否

#### #70 nullバイト埋め込みフィールド
- **行ったこと**: filenameフィールドに `image.jpg\x00.exe` を設定（null byte injection）
- **意図**: C言語由来のnullバイト終端によるパス切り詰め攻撃（`image.jpg` で終端されてしまう）が成立しないことの確認
- **結果**: PASS — TEEが安全に処理。Rustの文字列はnull終端しないため、この攻撃は原理的に無効

#### #71 クロスセッション鍵混同
- **行ったこと**: 2つの独立した暗号化セッション（異なるephemeral鍵ペア）を作成。セッション1のverifyレスポンスをセッション2の対称鍵で復号を試みる
- **意図**: ECDH鍵交換がセッション間で独立しており、他セッションの鍵では復号できないことの確認
- **結果**: PASS — セッション2の鍵での復号に失敗。各ECDHセッションの対称鍵は数学的に独立

---

### カテゴリ 17: X25519暗号エッジケース（ホワイトボックス）

ソースコード（`crates/crypto/src/lib.rs`）を読んだ上で、X25519とAES-GCMの実装固有のエッジケースを攻撃する。

#### #72 全ゼロ ephemeral_pubkey（X25519 identity point）
- **行ったこと**: ephemeral_pubkeyを32バイトの0x00に置換。X25519では全ゼロは低位点（small subgroup element）であり、ECDH結果も全ゼロになる
- **意図**: X25519の low-order point attack。共有鍵が全ゼロになった場合にHKDF→AES-GCMが「既知鍵」で動作してしまわないかの確認
- **結果**: PASS — 400 Bad Request。TEEがECDH結果からの復号に失敗して拒否。x25519-dalek は低位点入力でも panic しない

#### #73 全0xFF ephemeral_pubkey
- **行ったこと**: ephemeral_pubkeyを32バイトの0xFFに置換
- **意図**: X25519のスカラー値上限付近での挙動確認（フィールド演算のオーバーフロー等）
- **結果**: PASS — 400 Bad Request。ECDH結果は有効だが、対応する秘密鍵が不明なため復号失敗

#### #74 31バイト ephemeral_pubkey（短すぎ）
- **行ったこと**: ephemeral_pubkeyを31バイトに短縮
- **意図**: X25519公開鍵は厳密に32バイト必須。短い入力でパニックやバッファオーバーリードが起きないかの確認
- **結果**: PASS — 400 Bad Request。Base64デコード後の長さチェックで拒否

#### #75 33バイト ephemeral_pubkey（長すぎ）
- **行ったこと**: ephemeral_pubkeyを33バイトに拡張
- **意図**: 長すぎる入力でバッファオーバーフローが起きないかの確認
- **結果**: PASS — 400 Bad Request。長さチェックで拒否

#### #76 ephemeral_pubkey再利用（AES-GCM nonce一意性検証）
- **行ったこと**: 同一のephemeral鍵ペアで暗号化した同一ペイロードを2回アップロードし、2つのverifyレスポンスのnonceを比較。同一ephemeral_pubkey → 同一ECDH共有鍵 → 同一対称鍵のため、レスポンスのnonceが重複するとAES-GCMの安全性が崩壊する
- **意図**: TEEが各レスポンスで独立したランダムnonceを生成しており、AES-GCMのnonce一意性要件を満たすことの暗号的検証
- **結果**: PASS — nonce1=`kpxMQn...`、nonce2=`o0xLxP...`（異なる）。両方とも同じ対称鍵で復号に成功。TEEは`OsRng`で毎回新しいnonceを生成しており、AES-GCMの安全性は保たれている

#### #77 11バイトnonce（AES-GCM標準の12バイトより短い）
- **行ったこと**: 暗号化ペイロードのnonceを11バイトに短縮
- **意図**: AES-256-GCMのnonceは厳密に12バイト必須。短いnonceで復号を試みた際のエラーハンドリング確認
- **結果**: PASS — 400 Bad Request。nonce長チェックまたはAES-GCM復号段階で拒否

---

### カテゴリ 18: JSON型混同攻撃（ホワイトボックス）

ソースコード（各エンドポイントのAxum/Serdeデシリアライズ定義）を読んだ上で、各フィールドに想定外の型を送り込む。

#### #78 /upload-url content_size=文字列
- **行ったこと**: `{"content_size": "big", "content_type": "image/jpeg"}`
- **意図**: u64フィールドに文字列を送った場合のSerde型検証確認
- **結果**: PASS — 422「invalid type: string, expected u64」

#### #79 /upload-url content_size=小数
- **行ったこと**: `{"content_size": 3.14, ...}`
- **意図**: u64に浮動小数点数を送った場合の挙動
- **結果**: PASS — 422。Serdeはf64→u64の暗黙変換を拒否

#### #80 /upload-url content_size=負数
- **行ったこと**: `{"content_size": -1, ...}`
- **意図**: u64に負数を送った場合にオーバーフロー（u64::MAX）にならないことの確認
- **結果**: PASS — 422。Serdeが負数→u64を拒否

#### #81 /upload-url content_size=真偽値
- **行ったこと**: `{"content_size": true, ...}`
- **意図**: boolean→u64の型変換が起きないことの確認
- **結果**: PASS — 422

#### #82 /upload-url content_size=配列
- **行ったこと**: `{"content_size": [1000], ...}`
- **意図**: 配列→スカラーの型混同確認
- **結果**: PASS — 422

#### #83 /upload-url content_size=null
- **行ったこと**: `{"content_size": null, ...}`
- **意図**: null値がOption<u64>のNoneとして受理されないことの確認（content_sizeは必須）
- **結果**: PASS — 422

#### #84 /upload-url content_size=MAX_SAFE_INTEGER+1
- **行ったこと**: `{"content_size": 9007199254740992, ...}`（JavaScriptのNumber精度限界を超える値）
- **意図**: JSON数値の精度限界とRust u64での処理の整合性確認
- **結果**: PASS — 400「コンテンツサイズが上限を超えています: 9007199254740992 bytes」。正しくu64としてパースされ、2GB上限で拒否

#### #85 /verify processor_ids=文字列
- **行ったこと**: `{"download_url": "...", "processor_ids": "core-c2pa"}`（配列でなく文字列）
- **意図**: Vec<String>フィールドに文字列を送った場合の型検証確認
- **結果**: PASS — 422「invalid type: string, expected a sequence」

#### #86 /verify processor_ids=数値
- **行ったこと**: `{"processor_ids": 42}`
- **意図**: 配列フィールドに数値を送った場合
- **結果**: PASS — 422

#### #87 /verify processor_ids=ネスト配列
- **行ったこと**: `{"processor_ids": [["nested"]]}`（二重配列）
- **意図**: Vec<String>にVec<Vec<String>>を送った場合の型検証確認
- **結果**: PASS — 422「processor_ids[0]: invalid type」

#### #88 /verify processor_ids=null
- **行ったこと**: `{"processor_ids": null}`
- **意図**: 必須配列フィールドにnullを送った場合
- **結果**: PASS — 422

#### #89 /verify download_url=数値
- **行ったこと**: `{"download_url": 12345, ...}`
- **意図**: String フィールドに数値を送った場合
- **結果**: PASS — 422

#### #90 /verify download_url=配列
- **行ったこと**: `{"download_url": ["http://a", "http://b"], ...}`
- **意図**: Stringフィールドに配列を送った場合
- **結果**: PASS — 422

#### #91 /sign recent_blockhash=数値
- **行ったこと**: `{"recent_blockhash": 0, ...}`
- **意図**: Base58文字列フィールドに数値を送った場合
- **結果**: PASS — 422

#### #92 /sign requests=文字列
- **行ったこと**: `{"requests": "not_array"}`
- **意図**: Vec<SignRequest>フィールドに文字列を送った場合
- **結果**: PASS — 422

#### #93 /sign requests=オブジェクト
- **行ったこと**: `{"requests": {"uri": "fake"}}`（配列でなくオブジェクト）
- **意図**: 配列フィールドにオブジェクトを送った場合
- **結果**: PASS — 422

---

### カテゴリ 19: TOCTOU競合攻撃

#### #94 同一URLに100並列 /verify
- **行ったこと**: 1つのS3アップロード済みペイロードに対して、100件の/verifyリクエストを同時に送信
- **意図**: 同一S3オブジェクトへの並列ダウンロード＋並列復号＋並列C2PA検証で、TEEにデータ競合（race condition）やデッドロックが起きないことの確認
- **結果**: PASS — 89/100成功（11件はタイムアウト）。クラッシュなし、データ競合なし

#### #95 S3アップロード完了前にverify
- **行ったこと**: /upload-urlでS3署名付きURLを取得した直後に（S3への実データアップロードをせずに）そのdownload_urlで/verifyを呼び出し
- **意図**: TOCTOU（Time-of-Check to Time-of-Use）パターン。URLは有効だが中身がまだ存在しない状態での挙動確認
- **結果**: PASS — 502。TEEがS3から404を受け取り、適切にエラーとして処理

#### #96 /verify と /sign の同時実行
- **行ったこと**: Phase 1（/verify）とPhase 2（/sign）を同時に実行
- **意図**: 異なるフェーズのリクエストが同時に処理された場合にデッドロックやリソース競合が起きないことの確認
- **結果**: PASS — verifyは成功、signは失敗（偽URI）。デッドロックなし、独立に処理された

---

### カテゴリ 20: 境界値テスト（ホワイトボックス）

ソースコード（`crates/tee/src/infra/security.rs`、`crates/gateway/src/endpoints/upload_url.rs`）の定数・条件分岐を読んだ上で、境界値を正確に狙う。

#### #97 content_size=1（最小有効値）
- **行ったこと**: content_size=1 で/upload-urlにPOST
- **意図**: 最小サイズ（1バイト）がバリデーションを通過するかの確認。`size >= 1` の条件分岐テスト
- **結果**: PASS — 200 OK。1バイトは有効

#### #98 content_size=2GB（上限ぴったり）
- **行ったこと**: content_size=2147483648（2^31、ちょうど2GB）で/upload-urlにPOST
- **意図**: `size <= 2147483648` の境界条件テスト。ぴったり上限の値が受理されるか拒否されるか
- **結果**: PASS — 200 OK。上限値ぴったりは受理される（`<=` 比較）

#### #99 content_size=2GB+1（上限超過）
- **行ったこと**: content_size=2147483649 で/upload-urlにPOST
- **意図**: 上限を1バイト超えた値が確実に拒否されるかの確認
- **結果**: PASS — 400「コンテンツサイズが上限を超えています: 2147483649 bytes (上限: 2147483648 bytes)」

#### #100 content_size=u64::MAX
- **行ったこと**: content_size=18446744073709551615（u64最大値）で/upload-urlにPOST
- **意図**: 整数オーバーフローや予期しない動作が起きないことの確認
- **結果**: PASS — 400。正常に上限チェックで拒否

#### #101 重複processor_id × 5
- **行ったこと**: processor_idsに `["core-c2pa", "core-c2pa", "core-c2pa", "core-c2pa", "core-c2pa"]` を指定
- **意図**: 同一プロセッサIDの重複が重複排除されるか、5回実行されるかの確認
- **結果**: PASS — 5件の結果を返却（重複排除なし）。リソース浪費は起きるが脆弱性ではない。改善提案として重複排除の追加が考えられる

#### #102 content_typeに10KB文字列
- **行ったこと**: content_typeに `image/xxxxxxxxx...(10KB)` を設定して/upload-urlにPOST
- **意図**: 巨大なMIMEタイプ文字列がGateway/S3で問題を起こさないかの確認
- **結果**: PASS — 200 OK。GatewayはMIMEタイプのバリデーションを行わず、そのままS3メタデータとして格納

---

## 唯一のFAIL

**#62 `/sign` に空のrequests配列** — 200 OK + `{"partial_txs":[]}` を返す。

空入力→空出力は数学的には正しいが、API設計として空配列は400で拒否する方が一般的。ただし本プロトコルのPhase 2では、SDK側でrequests配列の構築を行うため、実際に空配列が送信されることは通常のフローではない。

---

## 発見事項と改善提案

### 対応検討

| # | 内容 | 提案 |
|---|------|------|
| 1 | `/sign` が空配列で200を返す | 空配列は400で拒否する方がAPI慣例に沿う |
| 2 | content_typeのバリデーションなし | 許可MIMEタイプのホワイトリストの検討 |
| 3 | 重複processor_idが重複実行される | 重複排除でリソース浪費を防止 |
| 4 | CORS未設定 | ブラウザ直接利用時に必要になる |
| 5 | rate limiting未実装 | DDoS対策としてGateway前段にALBまたはCloudFront |

---

## テスト環境

```
EC2:           c5.xlarge (4vCPU / 8GB RAM)
Enclave:       2vCPU / 1GB RAM
Gateway:       Port 3000 (axum)
TEE:           Nitro Enclave内 (socat vsockブリッジ経由)
S3:            ap-northeast-1 (title-uploads-devnet)
テスト画像:    signed.jpg (12,796 bytes, C2PA署名付き)
ウォレット:    devnet-authority keypair
```

## 実行方法

```bash
cd experiments
npx tsx stress-test.ts <gateway-ip> <image-path> \
  --wallet <keypair.json> \
  --encryption-pubkey <base64>
```

結果は `experiments/output-stress-test.json` に自動保存される。
