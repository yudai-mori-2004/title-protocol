# Task 45: コード監査 — integration-tests/

## 対象
`integration-tests/` — E2Eテスト・負荷テスト・セキュリティテストスクリプト

## ファイル
- `register-photo.ts` — SDK経由の実画像登録E2Eスクリプト（7ステップ: node-info→暗号化→upload→verify→Irysアップロード→sign→broadcast）
- `stress-test.ts` — 負荷テスト・攻撃耐久テスト（20カテゴリ、80+テストケース）
- `package.json` — 依存関係（private, `@title-protocol/sdk`はfileリンク）
- `tsconfig.json` — TypeScript設定
- `fixtures/` — テスト画像（C2PAメタデータ付きJPEG×5）

## 監査結果

### 設計メモ（修正不要）
- `register-photo.ts` は完全なE2Eフローを実装: node-info取得→ECDH暗号化→S3アップロード→/verify→Irys/Arweave保存→/sign→broadcast。各ステップに詳細なログ出力があり、デバッグ容易
- `stress-test.ts` は20カテゴリの包括的テストスイート:
  1. ベースライン計測（5回計測+全プロセッサ）
  2. 同時並行負荷（2/5/10並列）
  3. 大容量ペイロード（10MB/50MB）
  4. 不正入力（空JSON, 不正JSON, 巨大キー, XSS, 外部URL, 偽URI, 不存在プロセッサ）
  5. エンドポイント乱用（不正メソッド, パストラバーサル, 超長URL, Content-Length不一致, CORS）
  6. Slowloris模倣（20同時接続保持, 50連続GET）
  7. リプレイ攻撃（URL再利用, 期限切れURL偽造）
  8. 暗号攻撃（鍵不一致, ciphertext改竄, nonce改竄, 空ephemeral_pubkey）
  9. リソース枯渇（100並列upload-url, 10並列verify）
  10. プロトコルレベル攻撃（空wallet, SQLi, 不正Base64, prototype pollution, 100プロセッサ, パストラバーサル）
  11. SSRF（localhost, AWS metadata, 内部IP, file://, gopher://, data:, IPv6, decimal IP）
  12. HTTP Smuggling（巨大ヘッダ, Hostインジェクション, XXE, 巨大Content-Type）
  13. 持続負荷（5→10→20→30→50並列の劣化曲線）
  14. /sign探索（空requests, 100リクエストamplification, 直接/sign-and-mint, javascript:URI）
  15. タイミングサイドチャネル（有効/無効暗号化、存在/不存在プロセッサ）
  16. ペイロード混乱（二重暗号化, 10万キーJSON, null byte, クロスセッション鍵混同）
  17. X25519暗号エッジケース（全ゼロ/全0xFF/短い/長いephemeral_pubkey, nonce再利用, 短いnonce）
  18. JSON型混同（各フィールドに不正型: string/float/negative/bool/array/null/MAX_SAFE_INTEGER+1）
  19. TOCTOU競合（100並列同一URL, アップロード前verify, verify+sign同時）
  20. 境界値（size=1/2GB/2GB+1/u64MAX, 重複プロセッサID, 10KB MIME）
- `Irys`の動的importは `crypto.subtle` 破壊回避のため意図的（コメントで説明済み）
- `output-*.json` は `.gitignore` で追跡外（正しい）
- `private: true` のため公開パッケージ品質基準は不要
- 両スクリプトのヘルパー重複（parseArgs, log, TitleClient構築等）は実験スクリプトとして許容範囲

## 完了基準
- [x] 全ファイルの監査完了
- [x] 修正が必要な問題なし — OSSクオリティとして十分
