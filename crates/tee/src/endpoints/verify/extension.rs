// SPDX-License-Identifier: Apache-2.0

//! # Extension処理: WASM実行
//!
//! 仕様書 §3.1, §5.1 Step 5, §7.1

use base58::ToBase58;
use base64::Engine;

use title_types::{Attribute, ExtensionPayload, SignedJson, SignedJsonCore};

use crate::config::TeeAppState;

use super::format_content_hash;
use crate::endpoints::b64;

/// Extension処理: WASM実行 + Extension signed_json生成。
/// 仕様書 §3.1, §5.1 Step 5, §7.1
///
/// WASMバイナリはWasmLoaderトレイト経由で取得する。
/// エクスポート関数名は標準化された `process` を使用する。
pub(crate) async fn process_extension(
    state: &TeeAppState,
    content_bytes: &[u8],
    mime_type: &str,
    owner_wallet: &str,
    extension_id: &str,
    extension_input: Option<&serde_json::Value>,
) -> Result<serde_json::Value, String> {
    // WASMローダーを取得
    let loader = state
        .wasm_loader
        .as_ref()
        .ok_or_else(|| "WASMローダーが設定されていません。Extension実行には WASM_DIR または WASM_BASE_URL の設定が必要です".to_string())?;

    // WASMバイナリをロード（ファイルまたはHTTP経由）
    let wasm_binary = loader.load(extension_id).await?;

    // WASMバイナリのSHA-256ハッシュを計算
    let wasm_hash = title_crypto::sha256(&wasm_binary.bytes);
    let wasm_hash_hex = format_content_hash(&wasm_hash);

    // Extension補助入力をシリアライズ
    let ext_input_bytes = extension_input
        .map(|v| serde_json::to_vec(v))
        .transpose()
        .map_err(|e| format!("extension_inputのシリアライズに失敗: {e}"))?;

    // extension_inputのハッシュ（存在する場合）
    let ext_input_hash = ext_input_bytes.as_ref().map(|bytes| {
        let hash = title_crypto::sha256(bytes);
        format_content_hash(&hash)
    });

    // WASMランナーで実行（仕様書 §7.1）
    // 標準エクスポート関数名 "process" を使用
    let runner = title_wasm_host::WasmRunner::with_resource_pool(
        100_000_000, // Fuel制限: 1億命令
        64 * 1024 * 1024, // Memory制限: 64MB
        std::sync::Arc::clone(&state.resource_pool),
    );

    let wasm_result = runner
        .execute(
            &wasm_binary.bytes,
            content_bytes,
            ext_input_bytes.as_deref(),
            crate::wasm_loader::STANDARD_EXPORT_NAME,
        )
        .map_err(|e| format!("WASM実行エラー: {e}"))?;

    // content_hash計算（C2PA検証結果から取得）
    let c2pa_result = title_core::verify_c2pa(content_bytes, mime_type)
        .map_err(|e| format!("C2PA検証エラー: {e}"))?;
    let content_hash =
        title_crypto::content_hash_from_manifest_signature(&c2pa_result.active_manifest_signature);
    let content_hash_hex = format_content_hash(&content_hash);

    // ExtensionPayload構築（仕様書 §5.1 Step 5）
    let payload = ExtensionPayload {
        content_hash: content_hash_hex.clone(),
        content_type: mime_type.to_string(),
        creator_wallet: owner_wallet.to_string(),
        extension_id: extension_id.to_string(),
        wasm_source: wasm_binary.source.clone(),
        wasm_hash: wasm_hash_hex.clone(),
        extension_input_hash: ext_input_hash.clone(),
        result: wasm_result.output,
    };

    // attributes構築
    let attributes = vec![
        Attribute {
            trait_type: "protocol".to_string(),
            value: "Title-Extension-v1".to_string(),
        },
        Attribute {
            trait_type: "content_hash".to_string(),
            value: content_hash_hex.clone(),
        },
        Attribute {
            trait_type: "extension_id".to_string(),
            value: extension_id.to_string(),
        },
    ];

    // 署名対象の構築と署名（仕様書 §5.1 Step 5）
    // Core signed_jsonと同じSignedJson構造体を使用し、構造を統一する
    let payload_value = serde_json::to_value(&payload)
        .map_err(|e| format!("payloadシリアライズエラー: {e}"))?;
    let attributes_value = serde_json::to_value(&attributes)
        .map_err(|e| format!("attributesシリアライズエラー: {e}"))?;

    let sign_target = serde_json::json!({
        "payload": payload_value,
        "attributes": attributes_value,
    });
    let sign_bytes = serde_json::to_vec(&sign_target)
        .map_err(|e| format!("署名対象のシリアライズエラー: {e}"))?;

    let signature = state.runtime.sign(&sign_bytes);
    let tee_pubkey_b58 = state.runtime.signing_pubkey().to_base58();
    let attestation_b64 = b64().encode(state.runtime.get_attestation());

    // Extension signed_json構築（Core同様にSignedJson構造体を使用）
    // 仕様書 §5.1 Step 5: 外殻(protocol等) + payload(nested) + attributes
    let signed_json = SignedJson {
        core: SignedJsonCore {
            protocol: "Title-Extension-v1".to_string(),
            tee_type: state.runtime.tee_type().to_string(),
            tee_pubkey: tee_pubkey_b58,
            tee_signature: b64().encode(&signature),
            tee_attestation: attestation_b64,
        },
        payload: payload_value,
        attributes,
    };

    let signed_json_value = serde_json::to_value(&signed_json)
        .map_err(|e| format!("signed_jsonシリアライズエラー: {e}"))?;

    Ok(signed_json_value)
}
