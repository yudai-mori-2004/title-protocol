//! # Core処理: C2PA検証 + 来歴グラフ構築
//!
//! 仕様書 §2.1, §2.2, §5.1 Step 4

use base58::ToBase58;
use base64::Engine;

use title_types::{Attribute, CorePayload, SignedJson, SignedJsonCore};

use crate::config::TeeAppState;

use super::{b64, format_content_hash};

/// Core処理: C2PA検証 + 来歴グラフ構築 + signed_json生成。
/// 仕様書 §2.1, §2.2, §5.1 Step 4
pub(crate) fn process_core(
    state: &TeeAppState,
    content_bytes: &[u8],
    mime_type: &str,
    owner_wallet: &str,
    max_graph_size: usize,
) -> Result<SignedJson, String> {
    // C2PA検証
    let c2pa_result = title_core::verify_c2pa(content_bytes, mime_type)
        .map_err(|e| format!("C2PA検証エラー: {e}"))?;

    // content_hash計算
    let content_hash =
        title_crypto::content_hash_from_manifest_signature(&c2pa_result.active_manifest_signature);
    let content_hash_hex = format_content_hash(&content_hash);

    // 来歴グラフ構築
    let graph = title_core::build_provenance_graph(content_bytes, mime_type, max_graph_size)
        .map_err(|e| format!("来歴グラフ構築エラー: {e}"))?;

    // CorePayload構築
    let payload = CorePayload {
        content_hash: content_hash_hex.clone(),
        content_type: c2pa_result.content_type.clone(),
        creator_wallet: owner_wallet.to_string(),
        tsa_timestamp: c2pa_result.tsa_timestamp,
        tsa_pubkey_hash: c2pa_result.tsa_pubkey_hash.clone(),
        tsa_token_data: c2pa_result
            .tsa_token_data
            .as_ref()
            .map(|d| b64().encode(d)),
        nodes: graph.nodes,
        links: graph.links,
    };

    // attributes構築（cNFTオンチェーンメタデータ用）
    // 仕様書 §5.1 Step 4
    let attributes = vec![
        Attribute {
            trait_type: "protocol".to_string(),
            value: "Title-v1".to_string(),
        },
        Attribute {
            trait_type: "content_hash".to_string(),
            value: content_hash_hex,
        },
        Attribute {
            trait_type: "content_type".to_string(),
            value: c2pa_result.content_type,
        },
    ];

    // Step 6. signed_json構築 + TEE秘密鍵で署名（tee_signature）
    // 仕様書 §5.1 Step 4
    let payload_value = serde_json::to_value(&payload).map_err(|e| format!("payloadシリアライズエラー: {e}"))?;
    let attributes_value =
        serde_json::to_value(&attributes).map_err(|e| format!("attributesシリアライズエラー: {e}"))?;

    // 署名対象: payload + attributes の正規化JSON
    let sign_target = serde_json::json!({
        "payload": payload_value,
        "attributes": attributes_value,
    });
    let sign_bytes =
        serde_json::to_vec(&sign_target).map_err(|e| format!("署名対象のシリアライズエラー: {e}"))?;

    // TEE秘密鍵で署名
    let signature = state.runtime.sign(&sign_bytes);

    // TEE公開鍵（Base58エンコード）
    let tee_pubkey_b58 = state.runtime.signing_pubkey().to_base58();

    // Attestation Document（Base64エンコード）
    let attestation = state.runtime.get_attestation();
    let attestation_b64 = b64().encode(&attestation);

    // signed_json組み立て
    let signed_json = SignedJson {
        core: SignedJsonCore {
            protocol: "Title-v1".to_string(),
            tee_type: state.runtime.tee_type().to_string(),
            tee_pubkey: tee_pubkey_b58,
            tee_signature: b64().encode(&signature),
            tee_attestation: attestation_b64,
        },
        payload: payload_value,
        attributes,
    };

    Ok(signed_json)
}
