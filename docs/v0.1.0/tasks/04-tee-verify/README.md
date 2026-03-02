# タスク4: TEE /verify エンドポイント

## 前提タスク

- タスク1（MockRuntime）が完了していること
- タスク2（Proxy）が完了していること
- タスク3（C2PA Core）が完了していること

## 読むべきファイル

1. `docs/SPECS_JA.md` — §1.1「二段階の処理—Phase 1: Verify」§6.4「/verify フェーズの内部処理」のみ
2. `crates/tee/src/endpoints/verify.rs` — 現在のスタブ
3. `crates/tee/src/main.rs` — AppState構造体、ルーティング
4. `crates/tee/src/runtime/mock.rs` — タスク1で実装済みのMockRuntime
5. `crates/tee/src/proxy_client.rs` — プロキシクライアント（同時に実装する）
6. `crates/crypto/src/lib.rs` — 暗号プリミティブ（実装済み）
7. `crates/core/src/lib.rs` — C2PA検証（タスク3で実装済み）
8. `crates/types/src/lib.rs` — VerifyRequest, VerifyResponse, SignedJson等

## 作業内容

### 1. proxy_client.rs の実装

`crates/tee/src/proxy_client.rs` の `proxy_get()` / `proxy_post()` を実装する。
`prototype/enclave-c2pa/enclave/src/main.rs` のHTTP呼び出しパターンを参考にする。

- vsock（Linux）またはTCP localhost:8000（macOS）に接続
- length-prefixed プロトコルでリクエスト送信・レスポンス受信
- tokio の非同期I/Oを使用

### 2. /verify エンドポイントの7ステップ

```
1. Gateway署名検証 → 今はスキップ、TODOコメントを残す
2. resource_limits適用 → 今はデフォルト値を使用
3. download_urlからプロキシ経由で暗号化ペイロードを取得
4. ペイロード復号（ECDH + HKDF + AES-GCM）
5. processor_idsに基づきCore（C2PA検証+来歴グラフ）およびExtension（WASM実行）を処理
   → Extension（WASM）は今はスキップ、TODOコメントを残す。Core のみ処理
6. signed_json構築 + TEE秘密鍵で署名（tee_signature）
7. レスポンスを共通鍵で暗号化して返却
```

### signed_json構築の詳細（§5.1 Step 4参照）

```json
{
  "protocol": "Title-v1",
  "tee_type": "aws_nitro",
  "tee_pubkey": "Base58(signing_pubkey)",
  "tee_signature": "Base64(sign(payload + attributes))",
  "tee_attestation": "Base64(attestation_document)",
  "payload": {
    "content_hash": "0x...",
    "content_type": "image/jpeg",
    "creator_wallet": "Base58(owner_wallet)",
    "tsa_timestamp": null,
    "tsa_pubkey_hash": null,
    "tsa_token_data": null,
    "nodes": [...],
    "links": [...]
  },
  "attributes": [...]
}
```

## 完了条件

- MockRuntime + TCPプロキシフォールバックで、ローカルテストが動作する
- テスト: 暗号化ペイロード作成 → /verify 呼び出し → レスポンス復号 → signed_json 検証
  - signed_json 内の tee_signature を tee_pubkey で検証成功
  - content_hash が期待値と一致
- `cargo check --workspace && cargo test --workspace` が通る
- `docs/COVERAGE.md` の該当箇所を更新
