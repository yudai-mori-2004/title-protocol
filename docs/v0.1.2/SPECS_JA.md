# Title Protocol 技術仕様書

---

# 0. 前提

## 0.1 C2PAの仕組み

C2PA（Coalition for Content Provenance and Authenticity）は、Adobe、Microsoft、Google、Intel、BBC、Sony等が策定するデジタルコンテンツの来歴証明規格である。2022年にv1.0が公開され、v2.3（2026年1月）が現行安定版である。

C2PAは、コンテンツのファイル内に署名付きメタデータ（**マニフェスト**）を埋め込む。

```
ファイル（JPEG, MP4, PNG等）
├── コンテンツデータ（ピクセル、映像フレーム等）
├── 通常のメタデータ（EXIF等）
└── C2PAマニフェスト
     ├── アサーション群
     │    ├── ハードバインディング（コンテンツとの暗号的紐付け）
     │    ├── アクション情報（作成、編集等の履歴）
     │    ├── 素材情報（他コンテンツからの合成履歴）
     │    └── その他メタデータ（位置情報、機器情報、ライセンス等）
     ├── クレーム（アサーション一覧への参照）
     └── クレーム署名（電子署名）
```

マニフェスト内の**ハードバインディング**が、C2PAの信頼の核心である。ファイルのコンテンツ部分（マニフェスト自体を除く）のバイト列に対して暗号学的ハッシュを計算し、その値をマニフェスト内に記録する。署名後にコンテンツが1バイトでも変更されていれば、ハッシュが一致しなくなり、改ざんが検知される。

C2PAの検証（verify）は、署名の正当性（署名者が信頼された発行元に連鎖するか）と、コンテンツの同一性（ハッシュが一致するか）を暗号学的に確認する。検証に必要な全てのデータはファイル内に同梱されるため、オフラインで検証が完結する。

C2PAが採用する「事前の暗号署名」によるアプローチは、事後的なAI検出や電子透かしと異なり、コンテンツ生成技術の進歩に影響されない。署名の安全性は暗号アルゴリズムの数学的性質に依存しており、生成技術がいかに高度化しても、署名を偽造することはできない。

C2PAは静止画、完結した動画ファイル、ライブストリーミング（v2.3で追加）に対応している。動画やストリーミングではチャンクやセグメント単位でハッシュを管理するが、いずれの場合も、検証にはコンテンツデータそのものが必要であるという性質は共通する。

## 0.2 C2PA検証の構造的制約

C2PAの検証モデルには、規格自体の設計に起因する三つの構造的制約が存在する。

### 制約1: 検証にコンテンツの完全なコピーが必要である

ハードバインディングはバイト単位のハッシュ一致に依拠する。検証者がコンテンツの同一性を確認するには、対象ファイルのオリジナルバイナリ全体を手元に持ち、ハッシュを再計算しなければならない。

「改ざんされていないか」という一点を確認するために、高解像度の元データ全体が通信経路を流れる。必要な情報は検証結果だけであるにもかかわらず、入力として完全なファイルが要求される。

### 制約2: 検証がメタデータの全開示を伴う

マニフェストには、位置情報、撮影機器の識別情報、編集履歴など、プライバシーに関わるメタデータが含まれうる。検証はマニフェスト全体に対して行われるため、第三者に検証を求める場面では、検証目的に対して不必要な情報の露出が発生する。

コンテンツの所有者自身が検証する場合や、コンテンツが公開されている場合には問題にならない。しかし、第三者にコンテンツの真正性を証明したい場面では、この過剰な開示が障壁になる。

### 制約3: 流通経路でメタデータが消失する

主要なSNS・メッセージアプリは、アップロード時にファイルを再圧縮し、マニフェストを含むメタデータを削除する。コンテンツが最も広く流通し、真正性の検証が最も求められる経路において、C2PAメタデータは消失する。

C2PAはこの問題に対して、知覚ハッシュや不可視電子透かしを用いてクラウド上のマニフェスト保管庫から元のマニフェストを復元する仕組み（Durable Content Credentials）を規定しているが、保管庫の運営者への信頼が必要になる点、および技術の標準化が途上にある点で、制約が残る。

## 0.3 Title Protocolの役割

Title Protocolは、上記の構造的制約を解決する属性抽出レイヤーである。

C2PA署名付きコンテンツをTEE（Trusted Execution Environment: 信頼された実行環境）内で検証し、指定された属性を抽出し、Attestation Documentで封印する。これにより、**C2PA署名の信頼がTEEの信頼にデリゲートされる**。

TEEの内部処理はハードウェアレベルで保護されており、運営者を含む誰も処理中のデータを閲覧・改ざんできない。TEEが正規のTitle Protocolのコードを実行していたという事実は、Attestation Document（構成証明書）によって暗号学的に証明される。

TEEの出力は、元のコンテンツから独立して存在する。第三者はコンテンツの生データを持たなくても、この出力とAttestation Documentを用いて、データが正規のTEEで生成され改ざんされていないことを検証できる。

| | C2PA単体 | Title Protocol経由 |
|---|---|---|
| 検証に必要なもの | コンテンツのオリジナルバイナリ全体 | Attestation Document付きの出力のみ |
| メタデータの露出 | マニフェスト全体 | 指定された属性のみ |
| メタデータ消失時 | 検証不能 | 出力が存続する限り検証可能 |

Title Protocolが行うのは、ここまでである。封印されたデータをどこに保存するか、どのように利用するかは、プロトコルの関心外であり、アプリケーション層の責任である。

## 0.4 プロトコルの境界

Title Protocolが行うこと、行わないことを明確にする。

| 行うこと | 行わないこと |
|---|---|
| C2PA署名付きコンテンツから属性を抽出する | コンテンツの中身（何が映っているか等）を判断する |
| 抽出結果をAttestation Documentで封印する | 抽出結果の利用方法を規定する |
| 封印されたデータの事後検証手段を提供する | データの保存先や公開範囲を管理する |
| 暗号化によるコンテンツの秘匿性を提供する（オプション） | ブロックチェーンへの記録を強制する |

コンテンツのフィルタリング、ライセンスの解釈、収益分配のロジック、重複の検知・排除は全てアプリケーション層の責任である。

## 0.5 設計原則

| 原則 | 説明 |
|---|---|
| Content-Agnostic | コンテンツの生データはTEE内部でのみ処理され、プロトコルの運営者は内容を感知しない |
| Stateless | TEEはリクエスト間で状態を持たず、入力と計算のみに基づいて結果を返す |
| Neutral | 特定のアプリケーション、ストレージ、ブロックチェーンに依存しない |
| E2EE Optional | クライアントからTEEへの通信は、暗号化・非暗号化の両方を受け付ける |

## 0.6 本文書の構成

| セクション | 内容 |
|---|---|
| **0. 前提** | C2PAの仕組み、その構造的制約、Title Protocolの役割 |
| **1. プロトコルモデル** | 属性抽出と封印の抽象モデル。入力→TEE→Attestation Document付き出力 |
| **2. 通信モデル** | リクエスト・レスポンス形式、暗号化オプション、Gateway API |
| **3. Processor** | モジュール一覧と各processorの入出力定義 |
| **4. メモリ管理** | ResourcePool、Ticket、入力形式ごとのメモリパターン、攻撃防御 |
| **5. システム実装** | TEE起動シーケンス、リクエスト処理フロー、Gateway、リプロデューシブルビルド |
| **6. Extension** | Extensionの汎用定義、Solana Extension |

# 1. プロトコルモデル

## 1.1 処理の概要

Title Protocolが行う処理は、以下の3ステップに要約される。

1. クライアントが、コンテンツの所在（URL）と実行したい処理の一覧をTEEに送る
2. TEEがコンテンツを取得し、指定された処理を並列に実行し、結果をまとめる
3. TEEが結果のハッシュをAttestation Documentに埋め込んで取得し、結果とともに返す

```
Client                              TEE
  │                                  │
  │  コンテンツのURL                  │
  │  実行する処理の一覧               │
  │                                  │
  │─────────────────────────────────>│
  │                                  │
  │                          URLからコンテンツを取得
  │                          指定された処理を並列に実行
  │                          結果をまとめる
  │                          結果のハッシュを計算
  │                          Attestation Documentを取得
  │                            （ハッシュを埋め込む）
  │                                  │
  │<─────────────────────────────────│
  │                                  │
  │  処理結果                         │
  │  Attestation Document            │
  │                                  │
```

クライアントが受け取るのは、処理結果とAttestation Documentの組である。これがプロトコルの最終的な成果物であり、以降の利用方法（保存先、公開範囲、ブロックチェーンへの記録等）はプロトコルの関心外である。

## 1.2 Attestation Documentの役割

Attestation Document（構成証明書）とは、TEEのハードウェアが自動的に生成する証明書である。TEE上のプログラムが要求すると、ハイパーバイザー（TEEを管理するハードウェア層）が以下の情報を含む証明書を発行し、ハードウェアベンダーの秘密鍵で署名する。

| 含まれる情報 | 意味 |
|---|---|
| measurement（測定値） | TEE内で実行されているプログラムのハッシュ。AWS Nitroでは PCR0（enclave image、48 バイト SHA-384）が主要な照合対象だが、より厳格な検証では PCR1（カーネル/initrd）や PCR2（アプリケーション）も合わせて比較できる。ベンダーごとに長さと算出方法が異なる |
| user_data | TEE内のプログラムが任意に指定できるデータ領域 |
| ベンダー証明書チェーン | ハードウェアベンダー（AWS等）のルート証明書に連鎖する署名 |

measurementはTEE起動時に計算され、TEEの稼働中は変化しない。どのリクエストを処理しても同じ値であり、「何のプログラムが動いているか」を証明する。一方、user_dataはプログラムがAttestation Documentを要求するたびに任意の値を指定でき、「そのプログラムがその時点で何を出力したか」をバインドするために使う。

Title Protocolは、このuser_dataフィールドに**処理結果のハッシュ**を埋め込む。

これにより、Attestation Documentは以下の二つを同時に証明する。

- **実行コードの正当性**: TEE内で動いていたプログラムのハッシュが記録されている。検証者はこのハッシュをTitle Protocolの公開されたソースコードのビルドハッシュと照合することで、正規のプログラムが実行されていたことを確認できる
- **処理結果の完全性**: user_data内のハッシュと、手元の処理結果のハッシュを照合することで、処理結果が改ざんされていないことを確認できる

Attestation Document自体はハードウェアベンダーの署名で保護されているため、TEEの運営者を含む誰も、その内容を偽造できない。

### ベンダールート証明書の信頼起点

Attestation Document に含まれる証明書チェーンは「ルート証明書 → 中間証明書 → リーフ証明書」と連鎖し、リーフの秘密鍵で Attestation Document 本体に署名する仕組みになっている。チェーン内部の各署名が正しいことだけを確認しても、ルート自体が攻撃者の用意した別の証明書である可能性は排除できない。攻撃者は自分の手元で自己署名ルートを作り、その下に正規に見えるチェーンを構築すれば、形式上は完全に検証可能な「偽の Attestation Document」を生成できてしまう。

これを防ぐため、検証者はベンダーのルート証明書のハッシュ値を予め保持し、提示されたチェーンの先頭がそれと一致することを確認する。たとえば AWS Nitro の場合、AWS が公開する `AWS_NitroEnclaves_Root-G1` の SHA-256 ハッシュをコードに埋め込み、Attestation Document を受け取るたびに照合する。

このルートハッシュは TEE ベンダーごとに固定であり、ベンダーがルート証明書を更新した場合のみ Title Protocol 側の埋め込み値も更新する。

## 1.3 処理の実行

### Processor

TEEが実行する個々の処理を**processor**と呼ぶ。各processorは独立したモジュールであり、コンテンツのデータを入力として受け取り、抽出した属性を出力する。

```
コンテンツ ──┬──→ processor A ──→ 属性A
             ├──→ processor B ──→ 属性B
             └──→ processor C ──→ 属性C
                                    │
                          全ての属性をまとめる
                                    │
                                    ▼
                               処理結果
```

processorの構成はプロトコルの実装によって決まる。各processorは対等であり、processor間に実行順序の依存関係は存在しない。

### C2PA署名の必須性とsignature_hash

Title ProtocolはC2PA署名付きコンテンツの属性抽出レイヤーであるため、**C2PA署名チェーンの検証は全リクエストで必須**である。ただしこれは「`c2pa-verify` processor を強制実行する」という意味ではなく、orchestrator が `signature_hash` を計算する段階で C2PA 署名の存在と整合性を検証することで強制される (署名のないコンテンツはこの段で reject される)。

**signature_hash** は、コンテンツの Active Manifest（最新のマニフェスト）の COSE 署名の SHA-256 ハッシュであり、プロトコルレベルのコンテンツ識別子として使用する。orchestrator が processor 実行の前段で計算するため、processor の指定有無に関わらず全レスポンスに含まれ、Attestation Document の `user_data` 経由でバインドされる。同一の C2PA コンテンツからは、誰が計算しても同一の `signature_hash` が得られる。

`c2pa-verify` processor は標準提供される processor の 1 つで、Active Manifest 内の属性（claim_generator、signer、actions など）を JSON として取り出す責務を持つ。`signature_hash` の計算とは独立しており、クライアントが `processor_ids` に明示指定した場合のみ実行される。`rootlens-license-v1` のように C2PA 検証ロジックを自前で内包したオールインワン processor を使うときは、`c2pa-verify` を別途指定する必要はない。

### 入力形式

コンテンツがストレージ上でどのような形で存在しているかによって、TEEへのデータの渡し方が異なる。以下の3つの形式に対応する。

**単一ファイル**: JPEG、PNG、完結したMP4など、1つのファイルとしてストレージに存在するコンテンツ。TEEは指定されたURLから1つのファイルを取得して処理する。ファイルが大きい場合（動画等）でも、TEEはHTTPストリームで少しずつ読み込みながら処理できるため、ファイル全体をメモリに載せる必要はない。

**フラグメント**: ストリーミング配信（DASH、HLS等）向けに分割された動画。ストレージ上に物理的に別々のファイルとして存在する。

```
ストレージ上のファイル構成:

  init.mp4       ← 初期化セグメント（コーデック情報等、数KB）
  seg-0.m4s      ← メディアセグメント（2〜10秒分の映像、数MB）
  seg-1.m4s
  seg-2.m4s
  ...
```

TEEは初期化セグメントを最初に取得してマニフェストを読み込み、その後メディアセグメントを順に取得して検証を進める。これは1つの大きなファイルをHTTPチャンクで分割転送するのとは異なり、ストレージ上で物理的に分離された複数ファイルを順に処理する。

**サイドカー**: C2PAマニフェストがコンテンツファイルの外部に分離して保存されている場合。ストレージ上にマニフェストファイルとコンテンツファイルの2つが存在する。TEEは両方を取得して突合する。

入力形式の違いはTEE内部で吸収され、processorが出力する属性の構造には影響しない。

クライアントは、リクエスト時にコンテンツの入力形式とURLを指定する。

```json
{
  "input_type": "single",
  "content_url": "https://r2.example.com/photo.jpg",
  "processor_ids": ["c2pa-verify", "image-pdq"]
}
```

```json
{
  "input_type": "fragmented",
  "init_url": "https://r2.example.com/video/init.mp4",
  "fragment_urls": [
    "https://r2.example.com/video/seg-0.m4s",
    "https://r2.example.com/video/seg-1.m4s",
    "https://r2.example.com/video/seg-2.m4s"
  ],
  "processor_ids": ["c2pa-verify"]
}
```

## 1.4 暗号化（オプション）

クライアントからTEEに送るコンテンツの秘匿性が必要な場合、クライアントはコンテンツを暗号化した状態でストレージに保存し、使用した暗号スイートをリクエストで申告する。

```json
{
  "input_type": "single",
  "content_url": "https://storage.example.com/encrypted.bin",
  "encryption": "x25519",
  "processor_ids": ["c2pa-verify"]
}
```

TEEは起動時に複数の暗号スイートに対応する鍵ペアを生成し、公開鍵をGateway経由でクライアントに提供する。クライアントは対応する公開鍵でコンテンツを暗号化する。TEEは申告されたスイートに対応する秘密鍵で復号し、処理を行う。

復号に失敗した場合（スイートの不一致、鍵の不一致、データ破損等）、TEEはエラーを返す。

`encryption`フィールドが省略された場合、TEEはコンテンツを暗号化されていない生データとして扱う。サーバーサイドからの利用など、通信経路の秘匿性がHTTPSで十分な場合は暗号化を省略できる。

暗号化はコンテンツの秘匿性を保護するものであり、処理結果の信頼性には影響しない。暗号化の有無にかかわらず、処理結果は同一のAttestation Documentで保護される。

## 1.5 検証モデル

第三者が処理結果を受け取ったとき、以下の手順でその正当性を検証できる。

```
手元にあるもの:
  ・処理結果
  ・Attestation Document

検証手順:

  1. Attestation Documentの署名を検証する
     → ハードウェアベンダーの証明書チェーンを辿り、
       正規のTEEハードウェアが発行したものであることを確認する

  2. Attestation Document内の測定値を確認する
     → Title Protocolの公開されたソースコードをビルドし、
       そのハッシュと測定値が一致することを確認する
     → これにより、TEEが正規のTitle Protocolを実行していたことが保証される

  3. 処理結果のハッシュを計算し、
     Attestation Document内のuser_dataと照合する
     → 一致すれば、処理結果が改ざんされていないことが保証される
```

この検証に外部への問い合わせは不要である。処理結果とAttestation Documentがあれば、オフラインで完結する。

## 1.6 信頼の前提

検証者が受け入れる必要があるコア処理の前提は、最小限以下の 3 つに整理される。

1. **TEE ハードウェア**: ベンダー（AWS、AMD、Intel 等）が提供するハードウェアとファームウェアが、Attestation Document の測定値を正直に報告すること。
2. **C2PA ベンダールート CA**: c2pa-verify が連鎖を確認するルート証明書（Google / Sony / Leica 等）が当該ベンダーの正規署名鍵を反映していること。これは C2PA 規格自体への前提でもある。
3. **リプロデューシブルビルド**: 公開ソースから誰でも同じバイナリ measurement を再現できる ＝ measurement を見れば「どのコードが動いたか」が独立に検証可能であること。Rust toolchain / OS / 依存ピンの決定性が担保されていることに依存する。

この 3 前提のもとで、以下が成り立つ。

- 測定値がソースコードのビルドハッシュと一致する → 正規のプログラムが実行されていた
- TEEの内部処理はハードウェアレベルで保護されている → 運営者を含む誰も処理中のデータを閲覧・改ざんできない
- user_data内のハッシュが処理結果と一致する → 処理結果は改ざんされていない

プロトコルの運営者、Gateway、ストレージの提供者、その他いかなる主体への信頼も不要である。

ただし、本節の信頼モデルはコア処理に限る。Extension（§6）は追加の信頼前提を持つ場合がある（例: Solana Extension は whitelist 管理者を信頼前提に加える）。

## 1.7 Gatewayの位置づけ

GatewayはTEEの手前に配置される薄い管理層であり、以下を担当する。

- クライアント認証（APIキー等）
- TEEが現在保持している暗号化用公開鍵の提供
- 対応しているprocessorの一覧の提供
- リクエストのTEEへの中継

Gateway は信頼されない構成要素である。リクエスト内容（content_url、processor_ids 等）と返却結果いずれも改変する能力を物理的には持つが、改変は以下のいずれかで検知される。具体的なエンドポイントとフレーム定義は §2.5 を参照。

- **処理結果の改変**: Attestation Document 内の user_data（= `SHA-256(b"title:core" || JCS({"signature_hash":..., "results":...}))`）と一致しなくなるため検知される。`b"title:core"` は core 処理用のドメインタグであり、Solana 鍵登録用 user_data（タグ `b"title:solana-key"`、§6.2 参照）と SHA-256 入力レベルで分離される。
- **リクエスト内容の改変**: 利用形態に応じて検知レイヤが変わる:
  - **サーバーサイド利用（非暗号化）**: クライアントと Gateway を同一運営者が運用する前提。改変は意図的ではないため対象外。
  - **クライアントサイド利用（暗号化）**: コンテンツと signature_hash が暗号化ペイロードに封入される。Gateway が content_url を別ペイロードに差し替えた攻撃は TEE 内では検知不能だが、クライアントが事前計算した signature_hash をレスポンスと照合することで検知される（詳細は §2.4 ステップ 12）。

# 2. 通信モデル

## 2.1 リクエストの流れ

クライアントからTEEへのリクエストは、Gatewayを経由する。

```
Client                 Gateway                TEE                  外部ストレージ
  │                       │                    │                        │
  │── リクエスト ─────────>│                    │                        │
  │   (JSON)              │                    │                        │
  │                       │── 中継 ───────────>│                        │
  │                       │                    │                        │
  │                       │                    │── コンテンツ取得 ──────>│
  │                       │                    │<── データ ─────────────│
  │                       │                    │                        │
  │                       │                    │   処理実行              │
  │                       │                    │   Attestation取得       │
  │                       │                    │                        │
  │<── レスポンス ─────────│<── 結果返却 ────────│                        │
  │   (処理結果 +          │                    │                        │
  │    Attestation Doc)    │                    │                        │
```

Gateway はリクエストを物理的には改変可能だが、改変は §1.7 で述べた経路で検知される。TEE は、クライアントが指定した外部ストレージの URL からコンテンツを直接取得し、コンテンツ本体は Gateway を経由しない。

## 2.2 リクエスト形式

### 単一ファイル

```json
{
  "input_type": "single",
  "content_url": "https://r2.example.com/photo.jpg",
  "processor_ids": ["c2pa-verify", "image-pdq"]
}
```

| フィールド | 型 | 必須 | 説明 |
|---|---|---|---|
| `input_type` | String | Yes | `"single"` 固定 |
| `content_url` | String | Yes | コンテンツの取得URL |
| `processor_ids` | Array\<String\> | Yes | 実行するprocessorのID一覧 |
| `encryption` | String | No | 暗号化スイート名（後述） |

### フラグメント

```json
{
  "input_type": "fragmented",
  "init_url": "https://r2.example.com/video/init.mp4",
  "fragment_urls": [
    "https://r2.example.com/video/seg-0.m4s",
    "https://r2.example.com/video/seg-1.m4s",
    "https://r2.example.com/video/seg-2.m4s"
  ],
  "processor_ids": ["c2pa-verify"]
}
```

| フィールド | 型 | 必須 | 説明 |
|---|---|---|---|
| `input_type` | String | Yes | `"fragmented"` 固定 |
| `init_url` | String | Yes | 初期化セグメントのURL |
| `fragment_urls` | Array\<String\> | Yes | メディアセグメントのURL一覧（順序保持） |
| `processor_ids` | Array\<String\> | Yes | 実行するprocessorのID一覧 |

> `encryption` フィールドは fragmented 形式では指定できない（後述 §2.4）。

### サイドカー

```json
{
  "input_type": "sidecar",
  "manifest_url": "https://r2.example.com/photo.c2pa",
  "content_url": "https://r2.example.com/photo.jpg",
  "processor_ids": ["c2pa-verify"]
}
```

| フィールド | 型 | 必須 | 説明 |
|---|---|---|---|
| `input_type` | String | Yes | `"sidecar"` 固定 |
| `manifest_url` | String | Yes | C2PAマニフェスト（.c2pa）のURL |
| `content_url` | String | Yes | コンテンツ本体のURL |
| `processor_ids` | Array\<String\> | Yes | 実行するprocessorのID一覧 |

> `encryption` フィールドは sidecar 形式では指定できない（後述 §2.4）。

### 暗号化あり

`encryption`フィールドを追加することでコンテンツの暗号化を申告できる。本仕様では `input_type: "single"` に限り暗号化に対応する。fragmented / sidecar 形式での暗号化は将来の拡張とする（複数ファイルをどのようにペイロードへまとめるかの定義が必要なため）。

```json
{
  "input_type": "single",
  "content_url": "https://r2.example.com/encrypted.bin",
  "encryption": "x25519",
  "processor_ids": ["c2pa-verify"]
}
```

`content_url`が指す先のバイナリは、セクション2.4で定義する暗号化ペイロード（wire format）になる。TEEはこれを取得してから復号する。

`encryption`が省略された場合、コンテンツは暗号化されていないものとして扱われる。

## 2.3 レスポンス形式

TEEは、全processorの結果をまとめた処理結果と、その処理結果のハッシュを埋め込んだAttestation Documentを返す。

**Response:**

```json
{
  "signature_hash": "sha256:abcdef1234...",
  "results": {
    "c2pa-verify": {
      "status": "ok",
      "validation": "valid",
      "signer": { "issuer": "Google LLC", "cert_serial": "..." },
      "timestamp": "2026-01-15T10:30:00Z"
    },
    "image-pdq": {
      "status": "ok",
      "pdqhash": "a95669d1..."
    }
  },
  "attestation": "(Attestation Documentのバイナリ、Base64エンコード)"
}
```

| フィールド | 型 | 説明 |
|---|---|---|
| `signature_hash` | String | Active ManifestのC2PA署名のSHA-256ハッシュ。`"sha256:"` プレフィクス付き。処理結果を特定のコンテンツにバインドする |
| `results` | Object | processor IDをキー、各processorの出力を値とするマップ。内部構造はprocessorごとに異なる（セクション3参照） |
| `attestation` | String | Attestation DocumentのバイナリをBase64エンコードしたもの |

各 processor の出力構造はセクション3を Source of Truth とする。本節の `results` 例は概念図であり、フィールド形式の正典は §3.2 にある。

検証者は次の手順で結果の完全性を確認できる:

1. `signature_hash` と `results` を JSON オブジェクト `{ "signature_hash": ..., "results": ... }` として組み立てる
2. その JSON を JCS（JSON Canonicalization Scheme, RFC 8785）で正規化する
3. core 処理用ドメインタグ `b"title:core"` を SHA-256 入力の先頭に置き、続けて JCS バイト列を入れて SHA-256 ハッシュを計算する
4. その値が Attestation Document の `user_data` フィールドと一致することを確認する

擬似式: `user_data = SHA-256(b"title:core" || JCS({"signature_hash":..., "results":...}))`

ドメインタグ `b"title:core"` は §6.2 で定義する Solana 鍵登録用タグ `b"title:solana-key"` と SHA-256 入力レベルで分離する役割を持つ。同じ TEE が両方の Attestation Document を発行しても、user_data のバイト並びが意味的に区別できる。

暗号化モードでは、上記のJSONがresponse_keyで暗号化された状態で返却される（セクション2.4参照）。クライアントは復号後に同じJSON構造を得る。

## 2.4 暗号化の仕組み

### 鍵束

TEEは起動時に、対応する暗号スイートごとに鍵ペアを生成する。各鍵ペアの秘密鍵はTEEのメモリ内にのみ存在し、外部には公開鍵だけが提供される。

TEEが再起動すると鍵ペアは失われ、新しい鍵ペアが生成される。古い公開鍵で暗号化されたデータは復号できなくなるため、クライアントはリクエストの直前にGatewayから最新の公開鍵を取得する必要がある。

### 暗号化フロー

```
1. クライアントがC2PAコンテンツからsignature_hashをローカルで計算する
2. クライアントがGatewayから公開鍵一覧を取得する
3. クライアントがペイロードを構築する（signature_hash + コンテンツ本体）
4. クライアントがペイロードを暗号化し、自身のストレージにアップロードする
5. クライアントがリクエストを送信する（URLと使用したスイートを申告）
6. TEEがURLからデータを取得し、復号する
7. TEEがコンテンツのC2PA検証を実行し、signature_hashを算出する
8. ペイロード内のsignature_hashと算出値が一致するか検証する
   → 不一致ならエラー（ペイロード内部の改ざん検知）
9. 他のprocessorを並列実行し、結果を組み立てる
10. TEEがレスポンスをレスポンス方向の鍵で暗号化して返却する
11. クライアントがレスポンスを復号する
12. クライアントがレスポンスのsignature_hashをローカルで算出した値と照合する
    → 不一致であれば結果を破棄する（コンテンツが差し替えられた可能性がある）
```

### 方向別鍵導出

暗号化モードでは、鍵交換で得られた共有秘密からリクエスト方向とレスポンス方向で独立した対称鍵を導出する。

```
shared_secret = KEM鍵交換(TEE公開鍵, クライアントのエフェメラル秘密鍵)
request_key   = HKDF-SHA256(shared_secret, info="title-request-key",  salt=encap_key)
response_key  = HKDF-SHA256(shared_secret, info="title-response-key", salt=encap_key)
```

request_keyはクライアントがペイロードを暗号化する際に使用し、response_keyはTEEがレスポンスを暗号化する際に使用する。方向ごとに鍵が異なるため、同一鍵でのnonce衝突は原理的に発生しない。

クライアントが選択したスイートがリクエストとレスポンスの両方に適用される。鍵交換の方式（X25519、P-256、ML-KEM-768）はスイートによって異なるが、対称暗号（AES-256-GCM）は全スイートで共通であるため、レスポンスの復号方法はスイートに依存しない。クライアントはスイートを一度選ぶだけで、暗号化から復号まで追加の判断は不要である。

### レスポンスの暗号化ワイヤーフォーマット

暗号化モードでは、TEEのレスポンスは以下の形式で返却される。

```
┌───────────┬────────────┐
│   nonce   │ ciphertext │
│  (12B)    │ (残り全て)  │
└───────────┴────────────┘
```

リクエスト側のワイヤーフォーマットと異なり、suite_idとencap_keyは含まれない。クライアントは同一のKEM交換から導出したresponse_keyを既に保持しているため、nonceとciphertextだけで復号できる。

Gatewayはリクエスト（ペイロード）もレスポンス（処理結果）も読めない。Gatewayにできるのはリクエストの中継か拒否だけである。

### 暗号化ペイロードの内部構造

暗号化前の平文ペイロードは以下のバイナリ形式をとる。

```
[4B: metadata_len (ビッグエンディアン u32)]
[metadata_len バイト: メタデータJSON]
[残り: コンテンツの生バイナリ]
```

メタデータJSON:

```json
{
  "signature_hash": "sha256:abcdef1234..."
}
```

コンテンツはBase64変換せず、メタデータの直後に生バイナリとして結合する。このペイロード全体がrequest_keyで暗号化される。

TEEからのレスポンス（処理結果 + Attestation Document）はresponse_keyで暗号化されて返却される。ストレージの運営者とGatewayは、リクエスト（ペイロード）もレスポンス（処理結果）も読めない。

Gatewayがcontent_urlを別のペイロードに差し替える攻撃は、TEE内部では検知できない（攻撃者のペイロード内部のsignature_hashとコンテンツは整合するため）。この攻撃の検知はステップ12のクライアント側照合に依存する。クライアントはレスポンスのsignature_hashが自分のコンテンツのものと一致しなければ、結果を破棄する。

### 対応スイート

TEEが起動時に生成する鍵ペアのスイートは、TEEのビルド構成で決定される。

| スイート名 | suite_id | 鍵交換 | KDF | 対称暗号 |
|---|---|---|---|---|
| `x25519` | `0x01` | X25519 ECDH | HKDF-SHA256 | AES-256-GCM |
| `p256` | `0x02` | ECDH P-256 | HKDF-SHA256 | AES-256-GCM |
| `ml-kem-768` | `0x03` | ML-KEM-768 (FIPS 203) | HKDF-SHA256 | AES-256-GCM |

`x25519`と`p256`はクライアント環境のハードウェアアクセラレーション対応状況に応じて選択する。`ml-kem-768`はポスト量子暗号（FIPS 203, 2024年標準化）であり、量子コンピュータによる鍵交換の解読リスクに対応する。

### ワイヤーフォーマット

暗号化されたコンテンツは、以下のバイナリ形式でストレージに保存される。

```
┌──────────┬───────────────┬────────────┬───────────┬────────────┐
│ suite_id │ encap_key_len │ encap_key  │   nonce   │ ciphertext │
│  (1B)    │  (2B, BE)     │ (可変長)   │ (可変長)  │  (残り全て) │
└──────────┴───────────────┴────────────┴───────────┴────────────┘
```

| フィールド | 説明 |
|---|---|
| `suite_id` | 使用した暗号スイートの識別子（1バイト）。`0x01`〜`0x03` |
| `encap_key_len` | 鍵交換データの長さ（ビッグエンディアン、2バイト） |
| `encap_key` | 鍵交換データ（X25519: 32バイト、P-256: 65バイト、ML-KEM-768: 1088バイト） |
| `nonce` | AEAD用のnonce（AES-256-GCM: 12バイト。長さはsuite_idから決定） |
| `ciphertext` | 暗号文と認証タグ（残り全て） |

クライアントはエフェメラル鍵ペアを生成し、TEEの公開鍵との鍵交換で共有秘密を導出する。共有秘密からHKDF-SHA256で対称鍵を導出し、AES-256-GCMでコンテンツを暗号化する。

TEEはsuite_idからフォーマットを判別し、対応する秘密鍵で復号する。リクエストJSONの`encryption`フィールドとsuite_idが一致しない場合はエラーを返す。

## 2.5 Gateway API

Gatewayは以下のエンドポイントを公開する。

### GET /keys

TEEが現在保持している暗号化用公開鍵の一覧を返す。

**Response:**

```json
{
  "keys": {
    "x25519": "(公開鍵、Base64エンコード)",
    "p256": "(公開鍵、Base64エンコード)",
    "ml-kem-768": "(公開鍵、Base64エンコード)"
  }
}
```

| フィールド | 型 | 説明 |
|---|---|---|
| `keys` | Object | スイート名をキー、Base64エンコードされた公開鍵を値とするマップ |

TEE再起動時に鍵は更新される。クライアントは暗号化の直前にこのエンドポイントを呼び出し、最新の鍵を取得する。

---

### GET /processors

対応しているprocessorの一覧を返す。実体は TEE バイナリにビルド時固定で登録された processor 群。

**Response (v0.1.2 の現行ビルド):**

```json
{
  "processors": ["c2pa-verify"]
}
```

将来 processor が追加されると配列が拡張される (§3.2 を参照)。

| フィールド | 型 | 説明 |
|---|---|---|
| `processors` | Array\<String\> | 対応しているprocessor IDの一覧 |

---

### POST /process

属性抽出リクエストを受け付け、TEEに中継する。

**Request:** セクション2.2で定義したリクエスト形式。

**Response:** セクション2.3で定義したレスポンス形式。

Gateway はリクエストとレスポンスを中継する。改変能力は持つが、検知レイヤは §1.7 のとおり。

---

### GET /health

TEEの稼働状態を返す。

**Response:**

```json
{
  "status": "ok",
  "tee_type": "aws-nitro"
}
```

| フィールド | 型 | 必須 | 説明 |
|---|---|---|---|
| `status` | String | Yes | `"ok"`: リクエスト受付可能。`"unavailable"`: TEEが利用不可 |
| `tee_type` | String | No | TEE実行環境の種別。`"aws-nitro"`, `"amd-sev"`, `"mock"` 等 |

認証なしでアクセス可能。

---

### GET /solana-keys

Solana Extension用の公開鍵情報を返す。Solana Extensionが無効の場合は404を返す。

**Response:**

```json
{
  "solana_pubkey": "(Solana Ed25519公開鍵、Base58エンコード)"
}
```

| フィールド | 型 | 説明 |
|---|---|---|
| `solana_pubkey` | String | TEEが保持するSolana用Ed25519公開鍵（Base58エンコード） |

---

### POST /extension/solana

Solana Extensionリクエストを受け付け、TEEに中継する。Solana Extensionが無効の場合は404を返す。

**Request:** セクション6.2で定義したリクエスト形式。

**Response:** セクション6.2で定義したレスポンス形式。

# 3. Processor

## 3.1 概要

Processorは、コンテンツのデータから属性を抽出する独立したモジュールである。各processorはRustで実装され、TEEのバイナリにコンパイルされる。

クライアントはリクエスト時に`processor_ids`で実行するprocessorを指定する。指定された全processorが並列に実行され、それぞれの出力がまとめられて処理結果となる。

### Processorの規約

全てのprocessorは以下の規約に従う。

- コンテンツのデータを入力として受け取り、属性をJSON構造で出力する
- 他のprocessorの実行結果に依存しない。processor間に実行順序の制約は存在しない
- 処理が失敗した場合、エラー情報を出力する。他のprocessorの実行には影響しない

```json
{
  "results": {
    "c2pa-verify": { "status": "ok", ... },
    "image-pdq":   { "status": "ok", ... },
    "some-proc":   { "status": "error", "error": "unsupported format" }
  }
}
```

あるprocessorがエラーになっても、他のprocessorの結果は正常に返される。processorの成否の組み合わせをどう扱うかは、アプリケーション層の判断に委ねる。

### C2PA署名の必須性

Title ProtocolはC2PA署名付きコンテンツの属性抽出レイヤーであり、**C2PA署名のないコンテンツに対してはリクエスト全体が拒否される**。これは orchestrator が `signature_hash` を計算する段階で署名検証を兼ねるためであり、processor リストに何が指定されているかとは独立に強制される (§1.3)。

**`c2pa-verify` processor 自体は他の processor と並列の関係**であり、protocol レベルで強制実行されることはない。Title Protocol が標準提供する processor の 1 つとして扱う。クライアントは `processor_ids` で明示指定して実行する。`rootlens-license-v1` のように C2PA 検証ロジックを自前で内包したオールインワン processor を 1 本指定すれば、`c2pa-verify` を別途指定する必要はなくレスポンスにも現れない (実装上の二重パースを避けられる)。

## 3.2 現行のprocessor一覧

以下が初期実装としてプロトコル仕様に定義された processor 群である。**現行リリース (v0.1.2) で実装されているのは `c2pa-verify` のみ**で、`provenance-graph` / `image-pdq` / `video-vpdq` / `cert-google` / `cert-sony` / `cert-leica` は将来リリースで実装する。processor の追加・有効化は TEE バイナリの再ビルドと measurement の更新を伴う。

### c2pa-verify

C2PA署名チェーンの検証を行い、マニフェストの情報を抽出する。

| 項目 | 内容 |
|---|---|
| 入力 | C2PA署名付きコンテンツ |
| 処理 | 署名チェーンの検証、マニフェストの解析 |
| 出力 | 検証結果、署名者情報、タイムスタンプ、アクション履歴 |

```json
{
  "status": "ok",
  "validation": "valid",
  "signer": {
    "issuer": "Google LLC",
    "cert_serial": "..."
  },
  "timestamp": "2026-01-15T10:30:00Z",
  "claim_generator": "Google Pixel 10 Camera",
  "actions": [
    { "action": "c2pa.created" }
  ]
}
```

C2PA署名が無効な場合、`validation`が`invalid`となり、失敗理由が記録される。署名が無効でもprocessor自体はエラーにならない（検証結果が「無効だった」という情報も属性として有効である）。

### provenance-graph

C2PAマニフェストから素材情報を再帰的に抽出し、有向非巡回グラフ（DAG）として出力する。コンテンツがどの素材から合成されたかの構造を表す。

| 項目 | 内容 |
|---|---|
| 入力 | C2PA署名付きコンテンツ |
| 処理 | マニフェスト内の素材（ingredient）情報を再帰的に抽出 |
| 出力 | ノード（各コンテンツ）とエッジ（素材→派生の関係） |

```json
{
  "status": "ok",
  "nodes": [
    { "id": "sha256:abcd1234...", "title": "final_video.mp4" },
    { "id": "sha256:ef567890...", "title": "background_music.mp3" },
    { "id": "sha256:1234abcd...", "title": "photo.jpg" }
  ],
  "edges": [
    { "source": "sha256:ef567890...", "target": "sha256:abcd1234...", "role": "audio" },
    { "source": "sha256:1234abcd...", "target": "sha256:abcd1234...", "role": "image" }
  ]
}
```

### image-pdq

画像の知覚ハッシュをPDQアルゴリズムで算出する。知覚ハッシュとは、画像の視覚的な特徴を固定長のビット列に変換したもので、見た目がほぼ同じ画像は近い値になる。リサイズや再圧縮に対して頑健であり、類似画像の検出に使用される。

| 項目 | 内容 |
|---|---|
| 入力 | 画像データ（JPEG、PNG等） |
| 処理 | ピクセルデータをグレースケール化し、64×64にダウンサンプルした後、離散コサイン変換（DCT）で256ビットのハッシュを算出 |
| 出力 | PDQハッシュ値（256ビット）と品質スコア |

```json
{
  "status": "ok",
  "pdqhash": "a95669d1...",
  "quality": 85
}
```

### video-vpdq

動画の各フレームにPDQハッシュを適用し、フレームハッシュ列として出力する。

| 項目 | 内容 |
|---|---|
| 入力 | 動画データ（MP4等） |
| 処理 | 1fpsでフレームを抽出し、各フレームにPDQ算出を適用 |
| 出力 | フレームごとのPDQハッシュ列 |

```json
{
  "status": "ok",
  "frame_hashes": [
    { "frame": 0, "timestamp_ms": 0, "pdqhash": "...", "quality": 90 },
    { "frame": 1, "timestamp_ms": 1000, "pdqhash": "...", "quality": 88 }
  ]
}
```

品質が低いフレームや、前フレームと同一ハッシュのフレームは除去される。

### cert-*（証明書チェーン検証）

C2PA署名の証明書チェーンが特定のルート証明書に連鎖するかを検証する。機器メーカーやサービスごとに個別のprocessorとして実装される。

| processor_id | 検証対象 |
|---|---|
| `cert-google` | Google C2PA Root CA G3 |
| `cert-sony` | SONY C2PA Root CA G2 |
| `cert-leica` | Leica C2PA Root CA |

```json
{
  "status": "ok",
  "verified": true,
  "chain": [
    { "subject": "Google Pixel 10", "issuer": "Google C2PA ICA G3" },
    { "subject": "Google C2PA ICA G3", "issuer": "Google C2PA Root CA G3" }
  ]
}
```

特定のルート証明書への連鎖が確認できれば`verified: true`。確認できなければ`verified: false`。コンテンツがそもそもC2PA署名を持たない場合はエラーとなる。

## 3.3 Processorの追加

新しいprocessorの追加は、以下の手順で行われる。

1. processorをRustで実装する
2. TEEバイナリに組み込んで再ビルドする
3. 再ビルドされたTEEをデプロイする

processorの追加・変更の判断はプロトコルの運営者が行う。

TEEのビルドハッシュ（Attestation Documentの測定値）はprocessorの構成を含むため、processorが追加・変更されると測定値も変わる。検証者は測定値を照合することで、どのprocessorが含まれたビルドで処理されたかを確認できる。

# 4. メモリ管理

TEEは限られたメモリ上で動作し、メモリ枯渇（OOM）はTEEプロセスの停止と再起動を意味する。悪意あるリクエストや異常なデータによるOOMを防ぐため、メモリの使用量をリクエスト単位で管理する。

## 4.1 ResourcePool

TEE全体のメモリ使用量を単一の値で管理する仕組みをResourcePoolと呼ぶ。全リクエストの合計使用量を一つのカウンタで追跡し、上限を超える割り当てを拒否する。

ResourcePoolは2つの閾値を持つ。

```
|← 新規リクエスト受付可能 →|← 進行中リクエスト専用 →|← OS等の領域 →|
0                   admission_limit           total_limit    TEEメモリ上限
```

- `admission_limit`: 新規リクエストの受付上限。これを超えると新規リクエストを拒否する（HTTP 503）
- `total_limit`: 進行中リクエストが使用できる絶対上限

admission_limitとtotal_limitの間の領域は、すでに処理を開始したリクエストのための余裕である。処理中にデータが展開されてメモリ使用量が増えた場合でも、新規リクエストに余裕を奪われることなく完了できる。

## 4.2 Ticket

Ticketは、ResourcePoolからメモリを予約・解放するための仕組みである。各リクエストは処理開始時にTicketを取得し、データの到着に応じてメモリを予約し、不要になったら解放する。リクエスト完了時にTicketが破棄されると、そのリクエストが予約していたメモリは全て自動的に解放される。

### 漸進的予約

メモリは宣言時ではなく、実際のデータ到着時に予約する。

```
悪意あるリクエストの例:
  「2GBのファイルを送ります」と宣言
  → 2GB分のメモリを予約
  → 1バイトも送らずに接続を維持
  → 他のリクエストがメモリ不足で拒否される

漸進的予約による防御:
  「2GBのファイルを送ります」と宣言
  → この時点ではメモリを予約しない
  → 64KBのデータが到着 → 64KB分を予約
  → さらに64KB到着 → さらに64KB分を予約
  → データが来なければ、予約も増えない
```

この方式により、実際に到着したデータ量だけがメモリを消費する。

## 4.3 入力形式ごとのメモリパターン

### 単一ファイル

処理内容によってメモリパターンが異なる。

C2PA検証のみ（大容量MP4等）の場合、HTTP Range Requestでファイルの必要な部分だけを取得しながら、Merkle treeベースのチャンク単位検証を進められる。ファイル全体をメモリに保持する必要はない。

```
Range Requestパターン:
  マニフェスト取得:    ticket.extend（数KB〜数十KB）
  チャンク検証の繰り返し: ticket.extend → 検証 → ticket.shrink
完了:                 ticket解放
```

ピークメモリ: マニフェスト + チャンク1個分。

画像のデコードを伴うprocessor（image-pdq等）を同時に実行する場合は、対象データをメモリ上に展開する必要がある。

```
デコードを伴うパターン:
  ダウンロード: チャンクごとにticket.extend
  処理:         デコード対象のデータがメモリ上に存在
  完了:         ticket解放
```

ピークメモリ: デコード対象データ + 展開後のワーキングメモリ。total_limitを超える場合は拒否される。

### フラグメント

初期化セグメントを読み込んだ後、メディアセグメントを1つずつ処理して解放する。

```
ticket.extend(init)          ← 数KB
Reader構築

繰り返し:
  ticket.extend(fragment)    ← 数MB
  Readerにフラグメントを渡す（検証が進む）
  フラグメントデータを解放
  ticket.shrink(fragment)

全フラグメント完了
processorを実行
ticket解放
```

ピークメモリ: 初期化セグメント + フラグメント1個分 + Readerの内部状態。Readerの内部状態（検証の進捗情報）は動画の長さに比例するが、映像データそのものに比べて小さい固定的なオーバーヘッドとして扱える。

フラグメント形式は、単一ファイル形式よりピークメモリが大幅に小さくなる。

### サイドカー

マニフェストとコンテンツを個別にダウンロードし、処理する。メモリパターンは単一ファイルとほぼ同じで、マニフェスト分（数KB〜数十KB）が追加される。

## 4.4 攻撃への防御

### データサイズの上限

| パラメータ | 値 | 説明 |
|---|---|---|
| 単一ファイルの最大サイズ | 制限なし | Range Requestで必要部分のみ取得するため、ファイルサイズ自体は制約にならない。Range Request非対応のストレージの場合、total_limitが実質的な上限となる |
| フラグメントの最大数 | 100,000 | 2秒セグメント × 100,000 ≈ 55時間分 |
| フラグメント1個の最大サイズ | 100 MB | 10秒セグメントの高解像度映像に対応 |
| 同時メモリ使用量（total_limit） | TEEメモリの80% | 残り20%をOS・ランタイム用に確保 |
| 来歴グラフの最大サイズ | 10,000 | provenance-graphが抽出するノード+エッジの合計上限。超過時はエラーを返す |

上限を超えるデータは処理を開始せずに拒否する。

### チャンクタイムアウト

データのダウンロード中、60秒以内に次のデータチャンクが到着しなければ接続を切断する。これにより、極めて低速でデータを送り続けてリソースを長時間占有する攻撃を防ぐ。

### グローバルタイムアウト

リクエスト全体の処理時間に上限を設ける。最大30分。

```
タイムアウト = min(最大時間, 基本時間 + データサイズ / 最低転送速度)
```

小さなファイルには短いタイムアウト、大きなファイルには長いタイムアウトが適用される。ただし、最大時間を超えることはない。

### デコード時のメモリ保護

画像や動画のデコード処理では、圧縮データが展開されてメモリ使用量が急増する可能性がある（圧縮爆弾）。デコード前にファイルのヘッダからピクセル数やビット深度を読み取り、展開後のメモリサイズを事前に推定する。推定値がtotal_limitを超える場合、デコードを実行せずにエラーを返す。

# 5. システム実装

## 5.1 構成

Title Protocolの実行環境は、GatewayとTEEの2つのコンポーネントで構成される。

```
Client ──→ Gateway ──→ TEE ──→ 外部ストレージ（ユーザー管理）
```

| コンポーネント | 実行環境 | 役割 |
|---|---|---|
| Gateway | 通常のサーバー（EC2等） | クライアント認証、TEE情報の提供、リクエスト中継 |
| TEE | 信頼された実行環境（AWS Nitro Enclaves等） | コンテンツ取得、検証、属性抽出、Attestation取得 |

コンテンツの保存先はユーザーが管理し、TEEは指定されたURLから直接取得する。

## 5.2 TEE

### 実行環境の要件

TEEは、以下の性質を持つ実行環境で動作する。

- 内部の処理とメモリが外部から閲覧・改ざんできないこと
- 実行中のプログラムのハッシュを含むAttestation Documentを、ハードウェアレベルで生成できること
- Attestation Documentに任意のuser_dataを埋め込めること

AWS Nitro Enclavesを推奨実装とする。AMD SEV-SNP、Intel TDXも上記の要件を満たす。

### 起動シーケンス

TEEインスタンスは起動時に以下の初期化を行う。

```
TEE起動
  │
  ▼
暗号化用鍵ペアの生成
  各スイート（x25519, p256等）ごとに鍵ペアを生成
  秘密鍵はTEEメモリ内にのみ保持
  │
  ▼
自身のAttestation Documentを取得
  measurementを抽出して保持
  → 「自分は何者か」をTEE自身が把握する
  │
  ▼
公開鍵をGatewayに通知
  │
  ▼
リクエスト受付開始
```

鍵ペアの生成にはTEE内部の乱数生成器を使用する。秘密鍵はTEEのメモリ上にのみ存在し、ディスクや外部ストレージには一切書き出されない。TEEが再起動すると鍵は失われ、新しい鍵ペアが生成される。

自身の Attestation Document を取得する目的は、TEE が「自分の measurement は何か」を稼働中に参照可能にすることである。Extension 処理（セクション 6）で他のオフチェーンデータに含まれる Attestation Document を検証する際、その measurement が自分のものと一致するかを比較するために使う。これにより、攻撃者が別の Title Protocol インスタンスや別バージョンのコードで生成した正当な Attestation Document を、現行 TEE のリクエスト処理に紛れ込ませる経路を遮断する。

自己 Attestation の取得に失敗した場合、TEE は起動を中止する。measurement を保持できない状態でリクエスト受付を開始すると、後続の処理で measurement 一致確認が事実上スキップされ、信頼モデルが崩壊するためである。

### リクエスト処理フロー

TEEは各リクエストを以下の手順で処理する。

```
リクエスト受信
  │
  ▼
コンテンツ取得
  指定されたURLからデータをダウンロード
  入力形式（single / fragmented / sidecar）に応じた取得方法を選択
  │
  ▼
復号（暗号化ありの場合）
  申告されたスイートに対応する秘密鍵で復号
  復号失敗 → エラーを返して終了
  │
  ▼
Processor実行
  指定されたprocessorを並列に実行
  各processorがコンテンツから属性を抽出
  │
  ▼
結果の組み立て
  全processorの出力をまとめる
  │
  ▼
Attestation Document取得
  結果を JCS 正規化し、ドメインタグ b"title:core" を先頭に付けて SHA-256
  ハッシュを計算（詳細は §2.3）
  ハッシュをuser_dataに含めたAttestation Documentをハイパーバイザーに要求
  │
  ▼
レスポンス返却
  結果 + Attestation Documentをクライアントに返す
  │
  ▼
メモリ解放
  コンテンツデータ、中間データを全て破棄
```

各リクエストは独立して処理され、リクエスト間で状態を共有しない。コンテンツの生データはリクエスト処理中にのみメモリ上に存在し、処理完了後に即座に破棄される。

### コンテンツ取得の詳細

TEEは外部ストレージのURLに対してHTTPリクエストを発行してコンテンツを取得する。取得時にはセクション4で定義したメモリ管理が適用される。

**単一ファイル**: URLに対してHTTPリクエストを発行する。c2pa-rsのReaderはランダムアクセス（Read+Seek）を要求する。大容量ファイルの場合、HTTP Range Requestを用いてファイルの任意の位置にシークすることで、ファイル全体をメモリに保持せずに処理できる。C2PAのMerkle treeベースのハッシュ検証により、必要なチャンクだけを取得して検証を進めることが可能である。

Range Request を用いる場合、初回リクエストで取得した ETag を以降のリクエストの If-Match ヘッダに含める。取得の途中でストレージ上のファイルが変更された場合、412 Precondition Failed が返され、処理を中断する。これは性能上のフェイルファスト目的の defense-in-depth であり、整合性の根拠は C2PA の Merkle ハッシュ照合（§4.3）と TEE 内ハッシュ照合にある。If-Match を返さないストレージでは省略可。

**フラグメント**: 初期化セグメントのURLを最初に取得し、その後メディアセグメントのURLを順に取得する。各セグメントは処理後にメモリから解放できる。

**サイドカー**: マニフェストURLとコンテンツURLの2つに対してそれぞれHTTPリクエストを発行する。

## 5.3 Gateway

### 役割

Gatewayは通常のサーバー（TEEの外部）で動作し、以下を担当する。

- **クライアント認証**: APIキーの検証、レート制限
- **TEE情報の提供**: 暗号化用公開鍵、対応processor一覧の返却
- **リクエスト中継**: クライアントのリクエストをTEEに転送し、TEEのレスポンスをクライアントに返す

GatewayとTEEは同一ネットワーク内（または同一マシン上）に配置される。TEEのネットワークエンドポイントは外部に公開しない。

### TEE再起動時の挙動

TEEが再起動すると新しい鍵ペアが生成される。Gatewayは再起動を検知し、新しい公開鍵を取得して`/keys`エンドポイントの返却値を更新する。古い公開鍵で暗号化されたデータは復号できないため、該当リクエストにはエラーが返される。

## 5.4 リプロデューシブルビルド

検証者がAttestation Document内の測定値を照合するには、Title Protocolのソースコードから同一のバイナリをビルドし、そのハッシュを比較する必要がある。このためには、同一のソースコードから常に同一のバイナリが生成されること（リプロデューシブルビルド）が求められる。

ビルドの再現性を確保するために、以下を公開する。

- ソースコード（GitHubリポジトリ）
- ビルド手順（Dockerfile等）
- 依存ライブラリのバージョン固定（Cargo.lock）
- ビルド環境の指定（Rustコンパイラのバージョン、ターゲットアーキテクチャ）

検証者は公開された手順に従ってビルドを再現し、得られたバイナリのハッシュとAttestation Document内の測定値を照合する。

# 6. Extension

## 6.1 概要

Title Protocolのコア（セクション1〜5）は、コンテンツから属性を抽出し、Attestation Documentで封印して返すところで完結する。Extensionは、コアの成果物を入力として、特定の用途に向けた追加処理を行うレイヤーである。

Extensionはコアとは別のリクエストとして実行される。コア処理の成果物を一度受け取り、保存した上で、必要に応じてExtensionに渡す。

```
リクエスト1（コア処理）:
  コンテンツ → TEE → 処理結果 + Attestation Document

  ↓ クライアントが処理結果を保存（オフチェーンストレージ等）

リクエスト2（Extension）:
  保存した処理結果のURL → 同じTEE → 追加の成果物
```

コアとExtensionを別リクエストにすることで、コア処理の結果を確認してからExtensionに進むかどうかを判断できる。Extensionを使わない場合、コア処理の成果物がそのまま最終成果物となる。

### Extension の有効/無効

Extension の有効化は **TEE バイナリのビルド時点で固定される**。Extension を有効化したビルドと無効化したビルドは別個の TEE バイナリであり、measurement も異なるべきである。Gateway はその TEE バイナリの構成に応じて、対応する Extension エンドポイント（例: `POST /extension/solana`）の存在を判断し、未対応構成では 404 を返す。

**現行リリース (v0.1.2) の実装**: Solana Extension は常時有効としてビルドされており、`title-solana` crate は `crates/tee/Cargo.toml` で無条件依存。build-time toggle (`--features solana-ext` 相当) は将来のリリースで Extension が複数化する際に導入する。それまでの間、Gateway 側の `/solana-keys` 404 は「TEE が Extension を持たない」のではなく「キャッシュ未初期化」を意味する。

## 6.2 Solana Extension

Solana Extensionは、コア処理の成果物をSolanaブロックチェーン上にcNFT（Compressed NFT）として記録するための仕組みである。

### 解決する課題

コアの成果物（処理結果 + Attestation Document）は自己完結した検証可能なデータだが、これをブロックチェーンに記録する場合、「このデータは本当にTPのTEEで検証されたものか」をオンチェーンで確認できる仕組みが必要になる。

Solana Extensionは、TEEの署名鍵のホワイトリストをオンチェーンに構築し、cNFT発行トランザクションにホワイトリスト済みの署名が含まれているかどうかという一点で、信頼の判定を行えるようにする。

### 準備（TEEインスタンスごとに一度）

#### 署名鍵の生成と登録

TEEは起動時に、コア処理の暗号化鍵ペア（セクション5.2）に加え、Solana用のEd25519署名鍵ペアを生成する。秘密鍵はTEEメモリ内にのみ保持される。

この署名鍵が正規のTEE内で生成されたことをオンチェーンで証明し、ホワイトリストに登録する。

```
TEE起動
  │
  ▼
Solana用Ed25519署名鍵ペアを生成
  │
  ▼
Attestation Documentを取得
  user_data = SHA-256(b"title:solana-key" || Solana公開鍵)
  → 「この公開鍵は正規のTPコードを実行するTEE内で生成された」ことの証明
  → ドメインタグ b"title:solana-key" は core 処理用 user_data
    (タグ b"title:core"、§2.3 参照) と SHA-256 入力レベルで分離する
  │
  ▼
Attestation Documentからゼロ知識証明（ZK proof）を生成
  → オフチェーンで実行
  │
  ▼
ZK proofをSolanaプログラムに提出
  → プログラムが四段の照合を実施
    1. 検証回路が正規のものか（verifying_key_hash 照合）
    2. TEE 実体が正規のものか（measurement 照合）
    3. ZK proof の対象が今登録する署名鍵に紐づくか（user_data bind 確認）
    4. ZK proof の数学的検証（Groth16 ペアリング）
  → 全段通過時のみ、署名鍵をホワイトリストPDAに登録
```

ホワイトリストPDAはSolanaプログラムが管理するオンチェーンアカウントである。更新権限はプログラムのみが持ち、後述する四段の register_key 検証をすべて通過した ZK proof でのみ新しい署名鍵を追加できる。人手による管理は介在しない。

Solana上でAttestation Documentの証明書チェーンを直接検証するのは計算コストが高いため、ゼロ知識証明を用いる。ZKスキームにはSP1（Succinct）を採用する。SP1はRustプログラムをそのままzkVM上で実行し、実行結果のゼロ知識証明を生成する汎用zkVMであり、Solana上での証明検証をサポートしている（`sp1_solana` crate）。Attestation Documentの検証ロジック（証明書チェーン検証、measurement照合）を通常のRustコードとして記述し、SP1がゼロ知識証明にコンパイルする。カスタム回路の設計は不要である。

#### 四段の register_key 検証

ZK proof は「あるプログラムが、ある入力に対して、ある出力を返した」ことを数学的に保証するだけで、「そのプログラムが何だったか」「その入力が何だったか」は proof の中身を見ても分からない。Title Protocol の信頼を成立させるには、Solana プログラム側で確認 1〜3 で「正規性」3 つを、確認 4 で「proof 自体の数学的整合性」を別々に検証する。実装 (`programs/title-whitelist/src/lib.rs::register_key`) は DoS 耐性のため安価な確認 (1 → 2 → 3) を先に通し、最もコストの高い Groth16 ペアリング (4) を最後に置く。

**確認1: 検証回路の正規性 — verifying_key_hash**

検証プログラム（Attestation Document を検証する Rust コード。SP1 上で zkVM 実行される）の指紋を **verifying_key_hash** と呼ぶ。これがチェックされていないと、攻撃者は「Attestation 検証を省略した、何もしないダミープログラム」を自分で書いて proof を生成し、その proof を Solana プログラムに提出することで、検証を素通りさせられる。

Solana プログラムは「許容する verifying_key_hash の集合」をオンチェーンに保持し、register_key 命令は提出された proof の verifying_key_hash がこの集合に含まれることを最初に確認する。集合の更新権限は管理者のみが持つ。

verifying_key_hash は検証プログラムのソースコード次第で決まり、TEE バイナリの構成や再起動とは無関係に変化しない。検証プログラムの実装を変更した場合のみ、新しい hash を集合に追加する。

**確認2: TEE 実体の正規性 — measurement**

verifying_key_hash の確認は「Attestation Document が（何らかの）正規 TEE で生成されたこと」までしか保証しない。たとえば誰でも自分の AWS アカウントで Nitro Enclave を立ち上げ、自前のコードを動かして真正な Attestation Document を取得できる。検証回路はその Attestation Document を「AWS が署名した正当なもの」として通してしまう。

これを防ぐため、Solana プログラムは「許容する measurement の集合」も別途オンチェーンに保持する。register_key 命令は、ZK proof の公開出力に含まれる measurement がこの集合に含まれることを確認する。Title Protocol の正規 TEE バイナリの measurement だけが集合に登録されているため、別人が自前 TEE で生成した Attestation Document は弾かれる。

measurement は TEE バイナリのビルド結果から決まる固定値であり、ベンダー（AWS Nitro / AMD SEV-SNP / Intel TDX 等）によって長さや算出方法が異なる（AWS Nitro は 48 バイトの SHA-384）。TEE バイナリを変更したときのみ、新しい measurement を集合に追加する。

**確認3: 鍵と Attestation の bind 確認**

確認 1・確認 2 で正規性が担保された上で、最後に「Attestation Document の user_data フィールドが、今登録しようとしている署名鍵の公開鍵から導出される値と一致するか」を確認する。これにより、攻撃者が別の Attestation Document の proof を流用して別の鍵を登録することを防ぐ。

具体的には `user_data == SHA-256(b"title:solana-key" || signing_pubkey)` を検証する。ドメインタグ `b"title:solana-key"` は core 処理用 user_data（タグ `b"title:core"`、§2.3 参照）と SHA-256 入力レベルで分離するためのものであり、同じ TEE が両方の Attestation を発行しても user_data のバイト並びが意味的に重ならないことを保証する。攻撃者が core 処理レスポンスとして発行された任意の Attestation を流用しても、user_data の先頭バイト列が `b"title:core"` から始まる SHA-256 入力で計算されているため、`SHA-256(b"title:solana-key" || pubkey)` とは原理的に一致しない。

**集合の運用**

`verifying_key_hash` の集合と `measurement` の集合は独立したライフサイクルを持つ。検証プログラムを更新しても TEE バイナリが同じであれば、新しい verifying_key_hash を追加するだけで済む。逆に TEE バイナリだけを更新した場合は新しい measurement を追加する。古い値は必要に応じて削除可能だが、通常運用では追加のみで構わない（古い proof を再生成しなければならないケースは稀）。

#### コレクションの準備（開発者が行う）

コレクションはTEEではなく、開発者（プロトコルのユーザー）が作成・管理する。

```
1. 開発者が自分のコレクションを作成する（例: RootLensNFTコレクション）
2. コレクションの発行権限をTEEの署名鍵にdelegateする
   → Gatewayから取得したTEEのSolana公開鍵を指定
```

これにより、TEEの署名鍵が開発者のコレクションにcNFTを発行できるようになる。コレクションの設計（名前、メタデータ構造等）は開発者が自由に決められる。

### 利用（リクエストごと）

開発者がcNFTを発行する流れは以下の通り。

```
前提:
  コア処理が完了し、処理結果 + Attestation Documentが手元にある
  これをオフチェーンストレージに保存し、URLを取得済み

Solana Extensionリクエスト:
  │
  │  オフチェーンデータのURL
  │  コレクションアドレス
  │  Merkle Treeアドレス
  │
  ▼
TEEが処理:
  1. URLからオフチェーンデータをfetchする
  2. データ内のAttestation Documentを検証する
     - ベンダー証明書チェーンの検証（ルートはベンダーごとに固定の値を埋め込み照合）
     - measurement が自分自身のものと一致するか確認
       （起動時に取得した自己 Attestation Document の値と比較）
     - user_data == SHA-256(b"title:core" || JCS({"signature_hash":..., "results":...})) を検証
       （core 処理用 Attestation のみ受理する。Solana 鍵登録用 Attestation
        は b"title:solana-key" タグで作られているため、ここで弾かれる）
  3. 検証に成功した場合:
     cNFT発行トランザクションを構築し、TEEの署名鍵で部分署名する
  4. 部分署名済みトランザクションを返却する
  │
  ▼
開発者（またはGateway）が最終署名してブロードキャスト
```

TEEの内部で行われるAttestation Documentの検証がこの仕組みの核心である。TEEは渡されたデータが正規のTPコア処理で生成されたものであることを確認した上でのみ、部分署名を行う。

#### リクエスト形式

```json
{
  "offchain_data_url": "https://r2.example.com/output/abc123.json",
  "payer": "Base58エンコードされたpayer公開鍵（= leaf_owner / fee payer）",
  "merkle_tree": "Base58エンコードされたMerkle Treeアドレス",
  "recent_blockhash": "Base58エンコードされたBlockhash",
  "collection": "Base58エンコードされたコレクションアドレス（任意）"
}
```

| フィールド | 型 | 必須 | 説明 |
|---|---|---|---|
| `offchain_data_url` | String | Yes | コア処理結果（処理結果 + Attestation Document）のオフチェーンデータURL |
| `payer` | String | Yes | Fee payerかつleaf_ownerのSolana公開鍵（Base58エンコード） |
| `merkle_tree` | String | Yes | cNFT発行先のMerkle Treeアドレス（Base58エンコード） |
| `recent_blockhash` | String | Yes | クライアントが直前に取得したBlockhash（Base58エンコード） |
| `collection` | String | No | コレクションアドレス（Base58エンコード）。開発者が選択するもので、信頼モデルの一部ではない |

#### レスポンス形式

```json
{
  "partial_tx": "Base64エンコードされた部分署名済みトランザクション"
}
```

| フィールド | 型 | 説明 |
|---|---|---|
| `partial_tx` | String | TEEの署名鍵で部分署名済みのトランザクション（Base64エンコード）。クライアントが最終署名してブロードキャストする |

クライアントは返却されたトランザクションに自身のウォレットで最終署名を行い、Solanaにブロードキャストする。Blockhashの有効期限（約60〜90秒）内にブロードキャストを完了しなかった場合、トランザクションは無効となり、新しいBlockhashで再リクエストが必要になる。

### 検証（誰でもいつでも）

Solana上のcNFTが信頼できるかどうかは、以下の一点で判定できる。

**cNFTの発行トランザクションに、ホワイトリスト済みの署名鍵の署名が含まれているか。**

含まれていれば、そのcNFTは以下の過程を経て発行されたことが保証される。

1. 署名鍵は正規のTEE内で生成された（ZK proofで証明済み）
2. そのTEEのコードには「Attestation Documentを検証してからmintする」ロジックが含まれている（measurement照合で証明済み）
3. したがって、この署名鍵が部分署名したトランザクション → 必ずAttestation検証を経ている

N個のcNFTに対してN回のAttestation検証は不要である。署名鍵のホワイトリスト登録（一度だけ）で、そのTEEインスタンスが発行する全てのcNFTの信頼性が保証される。

### 運用

#### 署名鍵の有効期限

ホワイトリストに登録された署名鍵には、新規発行に対する有効期限（目安: 90日）を設ける。有効期限を過ぎた署名鍵では新しいcNFTを発行できないが、有効期限内に発行されたcNFTは永続的に有効である。

定期的な鍵のローテーションはTEEの再起動（停止→起動）で自然に行われる。再起動により新しい署名鍵が生成され、新たにホワイトリスト登録される。ホワイトリストには鍵が増えていく一方であり、これは正常な状態である。

#### ホワイトリスト鍵の取り消し（revoke）

通常運用では鍵の取り消しは行わない。TEEの侵害等、特別な事情が発生した場合にのみ、管理者が該当する鍵を取り消す。

取り消しはホワイトリストPDA自体を削除するのではなく、そのエントリに「revoked」フラグを立てる形で行う。PDAを削除（close）してしまうと、登録時のZK proofを攻撃者が再投入して同じ鍵を再度ホワイトリストに登録できてしまうため、PDAは存続させた上でフラグだけを更新する。登録命令はPDAが既に存在するエントリに対しては失敗するため、取り消し後の再登録は構造的に不可能になる。

取り消された鍵で過去に発行されたcNFTは、ブロックチェーン上に残り続ける。ただし、ホワイトリストの現在の状態で信頼を判定するアプリケーションでは、取り消し後は信頼されなくなる可能性がある。「発行時点で有効だったか」を判定するか、「現在も有効か」を判定するかは、アプリケーション層の設計による。

#### Merkle Treeの管理

cNFTの発行に必要なMerkle Treeの作成と管理は、開発者（プロトコルのユーザー）が行う。Treeの構成は信頼の判定に影響しない。信頼の根拠はあくまで発行トランザクションの署名者である。