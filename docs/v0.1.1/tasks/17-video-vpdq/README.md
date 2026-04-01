# Task 17: 動画対応 — vPDQ + ビデオフレーム抽出ホスト関数

## 目的

動画コンテンツの知覚ハッシュ（vPDQ）を算出するExtensionモジュールと、それを支えるビデオフレーム抽出のホスト関数プリミティブを実装する。

## 背景

Task 16 で実装した image-pdq（Meta ThreatExchange互換の256-bit画像ハッシュ）を動画に拡張する。vPDQは各フレームにPDQハッシュを計算し、フレーム列として保持するシンプルなアルゴリズム。

### Meta vPDQ リファレンス概要

- **ハッシュ構造**: 可変長のフレームハッシュリスト `[(hash_256bit, quality, frame_number, timestamp), ...]`
- **フレームサンプリング**: `N = secondsPerHash * fps` フレームごとに1回ハッシュ（デフォルト: 1秒に1フレーム）
- **品質フィルタ**: `quality < 50` のフレームはスキップ
- **重複除去（dedupe）**: 前フレームとPDQハッシュが完全一致なら省略
- **マッチング**: bag-of-hashes方式。時間順序は無視、ハミング距離31以内でマッチ判定
- **動画デコード**: FFmpeg CLI

参考:
- C++: <https://github.com/facebook/ThreatExchange/tree/main/vpdq/cpp>
- Python: <https://github.com/facebook/ThreatExchange/tree/main/vpdq>

### ffmpeg採用の背景

`ffmpeg-next` (Rustバインディング) はビルドが不安定で依存が重い。vPDQリファレンスを含む多くのプロジェクトがffmpegをCLIサブプロセスで使用しており、これが実用的な選択。コンテンツはTEE内の `/dev/shm` (tmpfs = RAM上) に一時保存し、処理後即削除。TEEのインメモリ原則を維持。

### TEEセキュリティモデルとの整合性

- ffmpegバイナリはTEEイメージ（EIF/Docker）に含まれ、PCRアテステーション対象
- `/dev/shm` はRAMベースtmpfs — TEEメモリ空間内でホストオペレーターからは不可視
- `TempVideoFile` がDrop時に即削除するRAIIパターンで一時ファイルのライフサイクルを保証
- MP4のmoovアトムがファイル末尾にあるケース（大半のMP4）でffmpegのstdin入力が使えないため、`/dev/shm` を使用

## 実施内容

### Phase 1: ホスト関数 — ビデオフレーム抽出

#### 1-1. video.rs 新規作成

`crates/wasm-host/src/video.rs`:
- `supports()` — MP4/WebM/MOVのマジックバイト検出（ftyp box, EBML header）
- `probe()` — ffprobe CLIでメタデータ取得（frame_count, fps, width, height, duration）
- `extract_frame_rgb()` — ffmpeg CLIで指定タイムスタンプのフレームをRGB24抽出
- `TempVideoFile` — `/dev/shm` に一時保存、Drop時に自動削除（macOSは `temp_dir()` にフォールバック）

#### 1-2. decode.rs にビデオ対応追加

- `DecoderKind::Video` 追加
- `detect()` で `video::supports()` を呼ぶ（画像判定の後にフォールバック）
- `estimate_peak_bytes()` — 一時ファイル + 1フレーム分のRGB
- `decode()` — ffprobeでメタデータ返却（フレームデコードは遅延）
- メタデータ形式: `[frame_count:u32, fps_x100:u32, width:u32, height:u32, duration_ms:u32]` (20 bytes)

#### 1-3. get_decoded_feature に video_frame_grayscale op 追加

```json
{"op": "video_frame_grayscale", "frame": 0, "width": 64, "height": 64}
```

- 指定フレーム番号からタイムスタンプ計算（frame / fps）
- ffmpegでフレーム抽出（RGB24、元解像度）
- Jarosz f32パイプラインで64×64にダウンサンプル（jarosz.rs共用）
- u8バッファとして返却
- 1フレーム分のみメモリ保持（ストリーミング方式）

### Phase 2: video-vpdq WASMモジュール

`wasm/video-vpdq/` — image-pdq のDCT・Torben中央値・quality計算をそのまま再利用。

処理フロー:
1. `decode_content` → video metadata (frame_count, fps)
2. `sampling_mod = floor(fps)` (1秒に1フレーム)
3. フレーム反復:
   - `get_decoded_feature(video_frame_grayscale, frame=N)` → 64×64 grayscale
   - `compute_pdq_hash()` → 256-bit hash
   - `compute_quality()` → quality (0-100)
   - quality < 50 → skip
   - hash == prev_hash → skip (dedupe)
4. JSON出力

結果フォーマット:
```json
{
  "frames": [
    {"pdqhash": "<64 hex>", "quality": 100, "timestamp": 0.0},
    {"pdqhash": "<64 hex>", "quality": 100, "timestamp": 1.0}
  ],
  "frame_count": 2,
  "algorithm": "vpdq",
  "sampling_fps": 1
}
```

### Phase 3: デプロイ・GlobalConfig更新・動作確認

#### 3-1. GlobalConfig更新（ローカルCLIから実行）

旧モジュール削除:
```bash
title-cli remove-wasm --extension-id hardware-google
title-cli remove-wasm --extension-id c2pa-training
title-cli remove-wasm --extension-id c2pa-license
```

旧TEEノード削除（stoppedのノード0）:
```bash
title-cli remove-node --signing-pubkey 5tpmk6JR3bKJTvWZvq5awtC3vGSuRFELcxhXnh7CSGHL
```

新モジュール登録（Arweaveアップロード + PDA作成 + GlobalConfig追加）:
```bash
title-cli register-wasm --extension-id image-pdq --wasm-path wasm-modules/image-pdq.wasm
title-cli register-wasm --extension-id video-vpdq --wasm-path wasm-modules/video-vpdq.wasm
title-cli register-wasm --extension-id cert-google --wasm-path wasm-modules/cert-google.wasm
title-cli register-wasm --extension-id cert-sony --wasm-path wasm-modules/cert-sony.wasm
title-cli register-wasm --extension-id cert-leica --wasm-path wasm-modules/cert-leica.wasm
title-cli register-wasm --extension-id cert-rootlens --wasm-path wasm-modules/cert-rootlens.wasm
```

既存モジュール更新（image-phash: リネーム後の新バイナリ）:
```bash
title-cli add-wasm-version --extension-id image-phash --wasm-path wasm-modules/image-phash.wasm
```

#### 3-2. EC2ノード更新

稼働中ノード (52.69.105.233) に対して:
1. `git pull` — コード更新
2. WASMモジュール再ビルド（7モジュール）
3. `setup-ec2.sh` 再実行 — TEEバイナリ再ビルド + Enclave再起動 + ノード再登録

WASMバイナリはOnChainLoader経由でArweaveから取得されるため、ローカルのwasm-modules/コピーは不要。TEE再起動が必要なのはホスト関数（jarosz.rs, video.rs, decode.rs, c2pa_cert.rs）の変更があるため。

#### 3-3. 動作確認

**画像 + image-pdq + cert-google**:
```bash
npx tsx register-photo.ts 52.69.105.233 ./fixtures/pixel_photo_plane.jpg \
  --wallet ../keys/operator.json --broadcast \
  --processors core-c2pa,image-pdq,cert-google
```

結果:
- core-c2pa: C2PA検証OK、来歴2ノード+1リンク、TSAタイムスタンプ取得
- image-pdq: `a95669d1...ad56` (quality=100, Meta互換距離2)
- cert-google: verified=true, chain=`[Google Photos Android, Mobile A 1P ICA G3 L3]`
- cNFT TX: Finalized (`5iJzYEgr...`)

**動画 + video-vpdq**:
```bash
npx tsx register-photo.ts 52.69.105.233 ./fixtures/test_video.mp4 \
  --wallet ../keys/operator.json --skip-sign \
  --processors core-c2pa,video-vpdq
```

結果:
- core-c2pa: content_type=video/mp4、来歴1ノード
- video-vpdq: 3フレームハッシュ列（0秒、1秒、2秒）

全てAWS Nitro TEE (`tee_type: aws_nitro`) 内で実行確認済み。

#### 3-4. GlobalConfig最終状態

SDK `fetchGlobalConfig()` で確認:
- TEEノード: 1台 (`7XhBeVdDfZXqtvNeGpMWSogFfmELGEFYYFYNe3ogFkf4`, active)
- WASMモジュール: 7個 (image-phash, image-pdq, video-vpdq, cert-google, cert-sony, cert-leica, cert-rootlens)

## テスト結果

### ローカルテスト（全216テストパス）

test_video.mp4 (1920×1080, 30fps, 2.5秒):
```
Frame 0: hash=d24ca749...f765 quality=100 ts=0.000
Frame 1: hash=af4ea562...ff65 quality=100 ts=1.000
Frame 2: hash=8f4e2132...ff65 quality=100 ts=2.000
```

### EC2本番テスト

| テスト | processors | 結果 | 所要時間 |
|--------|-----------|------|---------|
| Pixel写真 (637KB) | core-c2pa, image-pdq, cert-google | OK, cNFT Finalized | 1.3秒(verify) + 1.3秒(sign) |
| 動画 (5.3MB) | core-c2pa, video-vpdq | OK (verify only) | 2.4秒 |

## 完了条件

- [x] `video.rs` — ffmpeg CLI経由のフレーム抽出（`/dev/shm` 一時保存、RAII削除）
- [x] `decode.rs` — `DecoderKind::Video` 追加
- [x] `get_decoded_feature` — `video_frame_grayscale` op
- [x] `video-vpdq` WASMモジュール — ビルド・実行成功
- [x] `test_video.mp4` でフレームハッシュ列生成（ローカル）
- [x] 決定性テスト（同一動画 → 同一結果）
- [x] `cargo check --workspace && cargo test --workspace` パス（全216テスト）
- [x] GlobalConfig更新 — 旧モジュール削除、新モジュール登録
- [x] EC2ノード更新 — git pull + TEE再ビルド + Enclave再起動
- [x] EC2本番テスト — 画像(PDQ+cert) + 動画(vPDQ) の `/verify` 成功
- [x] cNFTオンチェーン発行 — Finalized確認済み
- [x] deploy スクリプト更新 — WASM_TARGETS, TRUSTED_EXTENSIONS に video-vpdq 追加

## クライアント側 vPDQ 検証の知見 (2026-04-01)

RootLens (root-lens/web) のブラウザ検証で、TEEが登録した動画のvPDQハッシュとブラウザ再計算のハッシュが一致しない問題を調査した。

### 検証に使用した動画

- RootLens撮影動画 (iPhone, H.264, 1920×1080, rotation:-90°, ~6秒)
- edit list: empty edit (media_time=-1) + play from media_time=0
- キーフレーム間隔: 約1秒

### 排除した仮説

| 仮説 | 結果 | 検証方法 |
|------|------|---------|
| Jarosz実装差 (Rust vs TS) | 無関係 | 同一ピクセルデータで両実装ともdist=0。half_window計算の差は1920×1080→64×64では丸め後に消失 |
| edit listオフセット未適用 | 無関係 | mp4box.jsはedit list適用済みCTSを返す。ffmpegの-ssも同じdisplay time空間で動作。オフセット調整は逆効果 |
| -noautorotateの問題 | 無関係 | TEE/ブラウザ共にraw sensor orientationで処理 |

### 根本原因

ffmpeg input seeking (`-ss T -i`) とWebCodecs のフレーム選択ロジックの差異:

- **ffmpeg**: target PTS **以上**の最初のフレームを返す (ceil)
- **WebCodecs**: target PTS に**最も近い**フレームを返す (nearest)

30fpsで1フレーム=0.033秒。targetがフレーム境界の直前にあると、ffmpegは次のフレーム、WebCodecsは前のフレームを選ぶ。隣接フレームでも暗い/均一なシーンではPDQ距離が100以上になりうる（DCT係数がメディアン付近に集中し、微差で符号反転するため）。

### 全フレームPDQ解析で確認

動画の全175フレームのPDQハッシュをffmpegデコーダで計算し、TEEの6ハッシュと照合:

| TEE timestamp | closest frame (nearest) | first-at-or-after (ceil) |
|--------------|------------------------|--------------------------|
| 0s | frame[0] dist=2 | frame[0] dist=2 |
| 0.968s | frame[29] dist=18 | frame[30] dist=0 |
| 1.935s | frame[58] dist=156 | frame[59] dist=2 |
| 2.903s | frame[87] dist=122 | frame[88] dist=0 |
| 3.871s | frame[116] dist=116 | frame[117] dist=4 |
| 4.838s | frame[145] dist=30 | frame[146] dist=2 |

nearest方式: 3/6 matched → FAIL。ceil方式: 6/6 matched (dist 0-4) → PASS。

### 修正 (root-lens/web)

`web/lib/verify/pdq.ts` の `extractFrames` で、WebCodecsのデコーダ出力から各タイムスタンプに対して「target以上の最初のフレーム」を選択するよう変更。

### 残存リスク

- WebCodecsとffmpegのH.264デコーダはピクセル単位で一致しない（Iフレームでもdist 2程度）。P/Bフレームではデコーダドリフトが蓄積しうる。
- 上記の全フレーム解析はffmpegのピクセルデータ同士の比較。ブラウザのWebCodecsデコーダ出力ではdistが数ビット増加する可能性がある。

## 依存関係

- Task 16 の image-pdq + jarosz.rs — **完了済み**
- システムに `ffmpeg` + `ffprobe` がPATH上に存在すること
- TEE Dockerイメージに ffmpeg を含めること（本番デプロイ時）
