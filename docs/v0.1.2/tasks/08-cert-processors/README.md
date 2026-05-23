# タスク08: cert-* Processors 実装

## 目的

C2PA 署名の証明書チェーンが特定のルート証明書に連鎖するかを検証する cert-google / cert-sony / cert-leica の3つの processor を実装する。

3つとも同じパターン（ルート証明書が違うだけ）なので1タスクにまとめる。

## 読むべきファイル

1. `CLAUDE.md`
2. `docs/v0.1.2/SPECS_JA.md` — 特に:
   - §3.2 cert-* の入出力定義（verified + chain の JSON 構造）
   - §3.2 各 processor_id と対応するルート証明書
3. `docs/v0.1.2/COVERAGE.md`
4. `crates/core/src/processor.rs` — Processor trait
5. `crates/core/src/processor_outputs.rs` — CertVerifyOutput, CertChainEntry 型
6. `legacy/v0.1.0/wasm/cert-*/` — **前バージョンの証明書チェーン検証 WASM。ルート証明書の埋め込み方、X.509 パースパターンがある。**

## スコープ

### やること

1. **汎用 cert 検証ロジック**:
   - C2PA 署名から証明書チェーンを抽出
   - 指定されたルート証明書への連鎖を検証
   - チェーンの各証明書の subject/issuer を抽出
   - `CertVerifyOutput` として構築

2. **3つの processor 実装**:
   - `CertGoogleProcessor` — Google C2PA Root CA G3
   - `CertSonyProcessor` — SONY C2PA Root CA G2
   - `CertLeicaProcessor` — Leica C2PA Root CA
   - 各 processor はルート証明書が違うだけで、検証ロジックは共通

3. **テスト**:
   - モック証明書チェーンでの検証テスト
   - 連鎖しない場合の verified: false テスト
   - `CertVerifyOutput` の serde 互換

### やらないこと

- C2PA 署名自体の検証（それは c2pa-verify — Task 03）
- 新しいルート証明書の追加メカニズム

## 依存

- Task 02: Processor trait + CertVerifyOutput
- Task 03: C2PA 署名からの証明書チェーン抽出パターン

## 成功基準

- [ ] 3つの cert processor が `Processor` trait を実装
- [ ] 正しいルート証明書への連鎖で verified: true を返す
- [ ] 連鎖しない場合に verified: false を返す
- [ ] `cargo test` 合格
- [ ] COVERAGE.md 更新
