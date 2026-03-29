# Task 18: デコーダー統一 + アーキテクチャ整理

## 目的

コンテンツフォーマット検出を `file-format` crateに統一し、RAW画像デコードを追加。
併せて、セッション中に発見されたアーキテクチャ上の問題を修正する。

## 背景

Task 16-17 で cert-*, image-pdq, video-vpdq を実装した過程で以下の課題が判明:

1. フォーマット検出が分散（`image` crate + 自前マジックバイト判定）
2. RAW画像（ARW, NEF, CR3等）のデコード非対応
3. `DecodedContent` が struct で動画を `channels=0` マジックナンバーで表現
4. cert-* の `sha256_hex` が名前詐欺（SHA-256を計算していない）
5. `hmac_content` ホスト関数が未使用デッドコード
6. テストフィクスチャが未整理

## 実施内容

### Phase 1: フォーマット検出の統一

`file-format` crateベースの統一検出に移行。

```rust
pub fn detect(content: &[u8]) -> Option<DecoderKind> {
    let fmt = FileFormat::from_bytes(content);
    match fmt {
        Jpeg | Png | Gif | Webp | Bmp | HEIC系  → Image,
        TagImageFileFormat                        → RawImage (exiftool → image crate fallback),
        Mpeg4 | MKV | MOV | AVI | FLV            → Video,
        SonyAlphaRaw | CanonRaw2/3 | NEF | ...   → RawImage,
        MP3 | Id3v2 | WAV | FLAC | AAC | OGG系   → Audio (認識のみ),
        AVIF/HEIC (ISO BMFF fallback)             → Image,
        _                                          → None,
    }
}
```

対応フォーマット:
- **Image**: JPEG, PNG, WebP, GIF, BMP, TIFF, HEIC/HEIF, AVIF
- **Video**: MP4, MKV, MOV, AVI, FLV
- **RawImage**: ARW, CR2, CR3, CRW, NEF, RAF, ORF, RW2 + DNG/TIFF系RAW
- **Audio**: MP3 (ID3v2含む), WAV, FLAC, M4A, AAC, OGG Vorbis/Opus/FLAC, AIFF, MKA

`video::supports()` の自前マジックバイト判定を廃止。

### Phase 2: RAWデコーダー

`raw.rs` 新規作成:
- exiftool CLIでプレビューJPEG抽出（`/dev/shm` + RAII削除）
- `JpgFromRaw` → `PreviewImage` のフォールバック
- 抽出されたJPEGを `image` crateで通常デコード

TIFF系RAWの判定:
- `TagImageFileFormat` は全て `RawImage` パスに統一
- exiftoolでプレビュー抽出を試み、失敗したら `image` crateでフォールバック
- ヒューリスティック（ファイルサイズ vs ピクセル数）を廃止し、確定的なロジックに

### Phase 3: DecodedContent の enum 化

`channels=0` マジックナンバーを廃止し、型安全な enum に変更:

```rust
enum DecodedContent {
    Image { data: Vec<u8>, width: u32, height: u32, channels: u32 },
    Video { width: u32, height: u32, fps: f64 },
}
```

- 動画の `fps` が `DecodedContent::Video` に閉じる
- `video_frame_grayscale` op が fps をキャッシュ済みの `DecodedContent::Video` から取得（ffprobe二重呼び出し解消）
- `grayscale_resize` と `video_frame_grayscale` が enum match で型安全に分岐
- 将来 `Audio { sample_rate, channels, duration_ms }` の追加が自然

### Phase 4: cert-* の root_spki 修正

`sha256_hex()` を4モジュールから削除。実態は:
- SHA-256を計算していない（名前が嘘）
- SPKI hexの先頭16バイトをhexエンコードしていただけ

修正: `root_spki_hash` → `root_spki` にリネームし、`ROOT_SPKI_HEX` をそのまま書き込む。
P-384で240文字、P-256で182文字。cNFT JSON内で許容範囲。

### Phase 5: hmac_content ホスト関数の削除

- どのWASMモジュールも使っていないデッドコード
- 仕様書で「ZK証明のBinding確認」用途として定義されていたが、既存の暗号基盤（E2EE + TEEアテステーション + Ed25519署名 + C2PA cert chain検証）でカバー済み
- HMACが必要な具体的ユースケースが存在しない

削除:
- `hmac_content` ホスト関数実装（~80行）
- `hmac` crate依存
- テスト用WATからの `hmac_content` import宣言
- hmac専用テスト

### Phase 6: テストフィクスチャ整理

旧フラット構造から整理済みディレクトリ構造に移行:

```
integration-tests/fixtures/
├── images/
│   ├── jpeg/     pixel_plane.jpg, pixel_ramen.jpg, ingredient_a.jpg, ingredient_b.jpg
│   ├── png/      sample.png
│   ├── webp/     sample.webp
│   ├── gif/      sample.gif
│   ├── bmp/      sample.bmp
│   ├── tiff/     sample.tiff
│   ├── avif/     sample.avif (c2pa-rs)
│   └── heic/     sample.heic (c2pa-rs)
├── video/
│   └── mp4/      sample.mp4
├── audio/
│   ├── mp3/      sample.mp3 (c2pa-rs)
│   ├── wav/      sample.wav (c2pa-rs)
│   └── flac/     sample.flac (c2pa-rs)
├── raw/
│   ├── arw/      sample.arw (Sony A700, f-spot/raw-samples)
│   ├── cr2/      sample.cr2 (Canon 400D, f-spot/raw-samples)
│   ├── dng/      sample.dng (Leica M8, f-spot/raw-samples)
│   └── nef/      sample.nef (Nikon D90, f-spot/raw-samples)
└── c2pa/
    ├── signed/   sample.jpg, sample.mp3, sample.wav, sample.tiff (c2pa-rs bench)
    └── unsigned/ sample.jpg, sample.mp3, sample.wav, sample.tiff (c2pa-rs bench)
```

全テストのフィクスチャパスを新構造に更新。旧フラットファイル削除。

レガシー削除:
- `integration-tests/test-phash-similarity.ts`
- `crates/core/examples/gen_phash_fixtures.rs`
- `integration-tests/fixtures/phash-test/`

### Phase 7: ドキュメント反映

以下のドキュメントを更新:

#### CLAUDE.md
- ホスト関数一覧から `hmac_content` を削除
- WASMモジュール一覧に `video-vpdq` が反映済みであることを確認

#### docs/reference.md
- ホスト関数リファレンスから `hmac_content` を削除
- `TRUSTED_EXTENSIONS` のデフォルト値が最新であることを確認

#### docs/architecture.md
- ホスト関数のアーキテクチャ図を更新（8関数、3層構造）

#### docs/v0.1.1/COVERAGE.md
- Task 18 の実施項目を反映

## ホスト関数最終構成（8関数、3層）

```
ライフサイクル制御:
  decode_content          画像/動画/RAW判定+デコード（DecodedContent enum）
  get_extension_input     補助入力の取得

コンテンツアクセス（生バイナリ）:
  read_content_chunk      pull: チャンク読み取り
  get_content_length      pull: 全長取得
  get_content_feature     feature: SHA-256/384/512, C2PA cert chain検証

デコード済みアクセス（ピクセル/フレーム）:
  read_decoded_chunk      pull: チャンク読み取り
  get_decoded_length      pull: 全長取得
  get_decoded_feature     feature: grayscale_resize, video_frame_grayscale
```

## 完了条件

- [x] `file-format` crateベースの統一フォーマット検出
- [x] `raw.rs` — exiftool CLIでRAWプレビュー抽出
- [x] TIFF系RAW判定のヒューリスティック廃止（exiftool → image crate fallback）
- [x] `DecodedContent` enum化（`channels=0` マジックナンバー廃止）
- [x] ffprobe二重呼び出し解消（fps を DecodedContent::Video にキャッシュ）
- [x] cert-* `sha256_hex` 削除 → `root_spki` にSPKI hexそのまま書き込み
- [x] `hmac_content` ホスト関数・依存・テスト削除
- [x] `video::supports()` デッドコード削除
- [x] テストフィクスチャ整理（35件のdecodeテスト全パス）
- [x] レガシー phash テスト/example/fixtures 削除
- [x] `cargo check --workspace && cargo test --workspace` パス（全249テスト）
- [x] ドキュメント反映（CLAUDE.md, SPECS_JA.md, COVERAGE.md）

## 依存関係

- Task 16 (cert-*, image-pdq) + Task 17 (video-vpdq) — **完了済み**
- `file-format` crate 0.29.0
- システムに `exiftool` がPATH上に存在すること（RAWデコード用）
