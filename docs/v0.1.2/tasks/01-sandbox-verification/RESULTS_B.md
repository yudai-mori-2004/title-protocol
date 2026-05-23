# Sandbox B: c2pa-rs CMAF フラグメント検証 — 結果

## 検証日

2026-05-23

## 環境

- Rust: 1.93.1 (stable)
- c2pa: 0.84.1 (features: file_io)
- OS: macOS (aarch64-apple-darwin)
- ffmpeg: 8.1 (libx264, DASH muxer)
- Signer: EphemeralSigner (Ed25519, 自己署名 CA + EE)

## 結果: 成功（全検証項目 PASS）

c2pa-rs v0.84 の `with_fragment` API は CMAF フラグメントの署名・検証に正しく対応している。
セグメント改ざん検知（BMFF ハッシュ）が機能し、フラグメント単位の独立検証（順序不問）も可能であることを確認した。

## 検証の信頼性を担保する 5 つのテスト

### テスト 1: v0.84 署名→検証ラウンドトリップ（10 フラグメント）

`EphemeralSigner` + `sign_fragmented_files` で署名した init.mp4 + seg-1〜10.m4s を `with_fragment` で全数検証。

- **結果**: 全 10 フラグメントで `ValidationState::Valid`
- **validation_status**: `signingCredential.untrusted`（self-signed cert）のみ
- **validation_results**: success=5, failure=1（failure は cert 信頼性のみ）
- **Manifest JSON hash**: 全 10 フラグメントで同一（プログラム的に一致を確認）
- **意味**: c2pa v0.84 のフラグメント署名→検証パイプラインは正しく動作する

### テスト 2: セグメント改ざん検知（BMFF ハッシュ）

署名済み seg-1.m4s の中間地点 1 バイトを反転し、with_fragment で検証。

- **結果**: `assertion.bmffHash.mismatch` が即座に検出
- **ベースライン**: failure=1（cert のみ）→ **改ざん後**: failure=2（cert + bmffHash）
- **検証状態**: `Valid` → `Invalid` に変化
- **意味**: フラグメント単位の BMFF ハードバインディングが正しく機能。1 バイトの改ざんでも検出する

### テスト 3: init.mp4 改ざん検知（領域別）

init.mp4 の 4 箇所を個別に改ざんし、検出パターンを分析。

| 位置 | 領域 | 結果 | 検出方法 |
|---|---|---|---|
| offset=100 | ftyp/moov 領域 | 検出 ✓ | Reader 構築失敗（"c2pa" block not found） |
| offset=400 | moov box 内部 | 検出 ✓ | Reader 構築失敗（UTF-8 decode error） |
| offset=7215 | JUMBF 領域（中間） | 未検出 ⚠ | Valid のまま |
| offset=14330 | 末尾付近 | 検出 ✓ | `assertion.bmffHash.mismatch` |

- **3/4 位置で検出**。未検出の offset=7215 は JUMBF マニフェスト box 内のパディング領域
- C2PA 仕様上、JUMBF box は BMFF ハッシュの計算対象から**意図的に除外**される（マニフェスト内にハッシュが含まれるため循環参照を避ける設計）
- COSE 署名は claims/assertions を保護するが、JUMBF box のパディングや構造的メタデータは署名対象外
- **意味**: moov/ftyp 等の BMFF 構造と末尾領域は保護されるが、JUMBF 内部のパディングは保護対象外。本プロトコルでは `signature_hash`（マニフェスト JSON の SHA-256）を Attestation Document に含めることで、マニフェスト全体の整合性を追加保護する

### テスト 4: 部分フラグメント検証（独立検証可能性）

全フラグメントを渡さずに、個別フラグメント + init.mp4 のみで検証。

- **最初のフラグメント（seg-1）のみ**: Valid ✓
- **中間のフラグメント（seg-6）のみ**: Valid ✓
- **最後のフラグメント（seg-10）のみ**: Valid ✓
- **意味**: フラグメントは順序に依存せず、任意の位置のフラグメントを個別に検証可能。全フラグメントが揃う前にストリーミング的な逐次検証が実現できる

### テスト 5: 順序逆転テスト

seg-10（最後）→ seg-1（最初）の順で検証を実行。

- **seg-10**: Valid ✓（cert 以外のエラーなし）
- **seg-1**: Valid ✓（cert 以外のエラーなし）
- **意味**: フラグメント間に順序依存なし。任意の順で検証できるため、リクエスト到着順に処理可能

## 成功基準の達成状況

### 1. init.mp4 + seg-*.m4s の署名・検証ラウンドトリップが成功する — 達成

`EphemeralSigner` + `Builder::sign_fragmented_files()` で署名し、`Reader::from_context(ctx).with_fragment(format, init_stream, frag_stream)` で検証が完了。全 10 フラグメントで `ValidationState::Valid`、cert 信頼性以外のエラーなし。

### 2. フラグメントを1つずつ渡して逐次検証できる — 達成

各フラグメントに対して個別に `with_fragment` を呼ぶことで、1つずつ検証が可能。フラグメント間に依存関係はなく、任意の順序で検証できる（逆順テストで確認済み）。

### 3. 検証後にフラグメントデータを解放できる（メモリパターンの確認） — 達成

ピークメモリ = init.mp4 (14KB) + 最大フラグメント 1 個分 (18KB) = **~32KB**。フラグメントデータはスコープ終了時に解放され、次のフラグメント処理時には前のデータは不要。§4.3 のメモリパターンに合致。

### 4. 改ざんしたフラグメントが正しく検出される — 達成

セグメント内容の 1 バイト改ざんで `assertion.bmffHash.mismatch` が検出される。init.mp4 の BMFF 構造領域の改ざんも検出される（JUMBF 内部パディングを除く）。

## メトリクス

20 秒 640x480 30fps H.264 映像（GOP=60, 2 秒セグメント）:

| ファイル | 署名前 | 署名後 | 増加量 |
|---|---|---|---|
| init.mp4 | 835 B | 14,430 B | +13,595 B (C2PA マニフェスト) |
| seg-1.m4s | 17,570 B | 17,779 B | +209 B |
| seg-2.m4s | 16,937 B | 17,146 B | +209 B |
| seg-3.m4s | 16,774 B | 16,983 B | +209 B |
| seg-4.m4s | 17,189 B | 17,398 B | +209 B |
| seg-5.m4s | 16,992 B | 17,201 B | +209 B |
| seg-6.m4s | 17,011 B | 17,220 B | +209 B |
| seg-7.m4s | 17,274 B | 17,483 B | +209 B |
| seg-8.m4s | 17,231 B | 17,372 B | +141 B |
| seg-9.m4s | 17,054 B | 17,195 B | +141 B |
| seg-10.m4s | 16,851 B | 17,060 B | +209 B |

- init.mp4 のサイズ増加は C2PA JUMBF マニフェスト（署名、証明書チェーン、アサーション）の埋め込みによる
- セグメントのサイズ増加は一定（+141〜209B）— C2PA フラグメントメタデータ（BMFF ハッシュ参照情報）

## 発見した制約・注意点

### 1. ffmpeg DASH muxer の制約

**パスの制約**: `-init_seg_name` / `-media_seg_name` に絶対パスを渡すと、MPD 出力ディレクトリとの二重パスが発生する。**ファイル名のみ**を渡し、MPD の出力先で制御する。

**マルチストリームの制約**: 複数ストリーム（video + audio）で同一セグメント名テンプレートを使うとファイル名が競合する。本検証では video-only で実施。

**GOP の制約**: DASH muxer はキーフレーム境界でのみセグメント分割が可能。`-g 60 -keyint_min 60`（30fps で 2 秒間隔のキーフレーム）を `-seg_duration 2` と一致させる必要がある。一致しない場合、セグメント数が想定より少なくなる。

### 2. sign_fragmented_files のパス保持動作

`sign_fragmented_files(signer, init_path, glob, output_dir)` は入力ファイルのディレクトリ構造を output_dir 内に再現する。

```
入力: work/raw/init.mp4, work/raw/seg-*.m4s
出力: work/signed/raw/init.mp4, work/signed/raw/seg-*.m4s
               ^^^
               入力のディレクトリ名が保持される
```

本実装では出力後に `init.mp4` を再帰検索して実際の出力先を特定する必要がある。

### 3. JUMBF 領域の改ざん検知の限界

C2PA 仕様上、init.mp4 内の JUMBF マニフェスト box は BMFF ハッシュの計算対象外。JUMBF 内部のパディング領域を改ざんしても `bmffHash.mismatch` は発生しない。

ただし、これは C2PA 仕様の意図された設計であり、実用上の問題は限定的:
- claims/assertions の内容は COSE 署名で保護される
- moov/ftyp 等の BMFF 構造は BMFF ハッシュで保護される
- 本プロトコルでは `signature_hash`（マニフェスト JSON の SHA-256）を Attestation Document に含めることで、マニフェスト全体の整合性を追加保護する

### 4. フラグメントの独立検証とメモリパターン

各フラグメントは init.mp4 + 単一セグメントの組み合わせで独立に検証可能。全フラグメントを揃える必要がない。順序にも依存しない。これは §4.3 の「フラグメントを1つ処理したら解放」パターンと完全に合致する。

検証の流れ:
```
init.mp4 をメモリに保持（~14KB）
  → seg-1 を読み込み → with_fragment(init, seg-1) → 検証 → seg-1 を解放
  → seg-2 を読み込み → with_fragment(init, seg-2) → 検証 → seg-2 を解放
  → ...（任意の順序で可能）
init.mp4 を解放
```

### 5. Manifest JSON hash の同一性

全 10 フラグメントで `reader.json()` の SHA-256 ハッシュが同一（プログラム的に確認済み）。init.mp4 にマニフェストが格納されており、フラグメントごとに異なるマニフェストは生成されない。`signature_hash` の計算はどのフラグメントの検証結果からでも同一値が得られる。

## 本実装に向けた推奨事項

### フラグメント検証フロー

1. init.mp4 を取得してメモリに保持（~14KB、マニフェストサイズに依存）
2. 各セグメントを逐次取得 → `with_fragment` で検証 → セグメント解放
3. 全セグメントの検証完了後、init.mp4 を解放
4. `signature_hash` は任意のフラグメント検証結果から取得可能（全フラグメントで同一値）

### メモリ管理との統合（§4.3）

```
ResourcePool から Ticket を取得
  → ticket.extend(init.mp4 サイズ) で init 分を予約
  → init.mp4 を取得
  → for each segment:
      ticket.extend(segment サイズ) でセグメント分を予約
      segment を取得 → with_fragment(init, segment) → 検証
      ticket.shrink(segment サイズ) でセグメント分を解放
  → ticket.shrink(init.mp4 サイズ) で init 分を解放
  → Ticket 解放
```

ピークメモリ = init.mp4 サイズ + 最大セグメントサイズ + c2pa-rs 内部状態（数十KB程度）

### 改ざん検知判定ロジック

```
validation_status() のコードを全て列挙:
  signingCredential.untrusted → 無視（self-signed cert は TEE 環境で想定内）
  assertion.bmffHash.mismatch → コンテンツ改ざん（致命的エラー）
  claimSignature.mismatch → 署名不正（致命的エラー）
  assertion.*.mismatch → アサーション改ざん（致命的エラー）
  reader_error: * → データ構造破壊（致命的エラー）
```

### セグメントサイズの実運用見積もり

テスト映像（640x480 30fps H.264, bitrate ~66kbps, 2秒セグメント）:
- セグメントサイズ: ~17KB/セグメント
- C2PA オーバーヘッド: +141〜209B/セグメント（セグメントサイズの ~1.2%）

実運用映像（1080p 60fps H.264, bitrate ~5Mbps, 2秒セグメント）の推定:
- セグメントサイズ: ~1.25MB/セグメント
- C2PA オーバーヘッド: ~200B/セグメント（セグメントサイズの ~0.02%）
- init.mp4: ~14KB（映像解像度に依存しない、C2PA マニフェストが支配的）

### CMAF 生成時の注意

- キーフレーム間隔（`-g`）を seg_duration と一致させること
- マルチストリーム（audio+video）の場合は適切な adaptation_sets 設定が必要
- `sign_fragmented_files` の出力パスが入力構造を保持するため、出力先の検出ロジックが必要

## コード

`sandbox/02-c2pa-fragment/src/main.rs`
