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

## 実施内容

### Phase 1: ホスト関数 — ビデオフレーム抽出

#### 1-1. video.rs 新規作成

`crates/wasm-host/src/video.rs`:
- `supports()` — MP4/WebM/MOVのマジックバイト検出
- `probe()` — ffprobe CLIでメタデータ取得（frame_count, fps, width, height, duration）
- `extract_frame_rgb()` — ffmpeg CLIで指定タイムスタンプのフレームをRGB24抽出
- `TempVideoFile` — `/dev/shm` に一時保存、Drop時に自動削除（macOSは `temp_dir()` にフォールバック）

#### 1-2. decode.rs にビデオ対応追加

- `DecoderKind::Video` 追加
- `detect()` で `video::supports()` を呼ぶ
- `estimate_peak_bytes()` — 一時ファイル + 1フレーム分のRGB
- `decode()` — ffprobeでメタデータ返却（フレームデコードは遅延）
- メタデータ形式: `[frame_count:u32, fps_x100:u32, width:u32, height:u32, duration_ms:u32]` (20 bytes)

#### 1-3. get_decoded_feature に video_frame_grayscale op 追加

```json
{"op": "video_frame_grayscale", "frame": 0, "width": 64, "height": 64}
```

- 指定フレーム番号からタイムスタンプ計算（frame / fps）
- ffmpegでフレーム抽出（RGB24）
- Jarosz f32パイプラインで64×64にダウンサンプル
- u8バッファとして返却

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

## テスト結果

test_video.mp4 (1920×1080, 30fps, 2.5秒):

```
Frame 0: hash=d24ca749...f765 quality=100 ts=0.000
Frame 1: hash=af4ea562...ff65 quality=100 ts=1.000
Frame 2: hash=8f4e2132...ff65 quality=100 ts=2.000
```

- 3フレーム抽出（1fps サンプリング）
- 全フレーム quality=100
- 決定性テスト: 同一動画 → 同一ハッシュ列

## 完了条件

- [x] `video.rs` — ffmpeg CLI経由のフレーム抽出（`/dev/shm` 一時保存）
- [x] `decode.rs` — `DecoderKind::Video` 追加
- [x] `get_decoded_feature` — `video_frame_grayscale` op
- [x] `video-vpdq` WASMモジュール — ビルド・実行成功
- [x] `test_video.mp4` でフレームハッシュ列生成
- [x] 決定性テスト（同一動画 → 同一結果）
- [x] `cargo check --workspace && cargo test --workspace` パス（全216テスト）

## 依存関係

- Task 16 の image-pdq + jarosz.rs — **完了済み**
- システムに `ffmpeg` + `ffprobe` がPATH上に存在すること
