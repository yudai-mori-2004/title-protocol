//! # Gateway認証
//!
//! 仕様書 §6.2
//!
//! Gateway秘密鍵によるリクエスト署名の構築とTEEへのリクエスト中継。

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey as Ed25519SigningKey};
use title_types::*;

use crate::config::GatewayState;
use crate::error::GatewayError;

/// Base64エンジン（Standard）
pub(crate) fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// Gateway認証ラッパーを構築する。
/// 仕様書 §6.2: リクエスト内容 + resource_limits を含む構造体を構築し、Gateway秘密鍵で署名する。
pub(crate) fn build_gateway_auth_wrapper(
    signing_key: &Ed25519SigningKey,
    method: &str,
    path: &str,
    body: serde_json::Value,
    resource_limits: Option<ResourceLimits>,
) -> Result<GatewayAuthWrapper, GatewayError> {
    let sign_target = GatewayAuthSignTarget {
        method: method.to_string(),
        path: path.to_string(),
        body: body.clone(),
        resource_limits: resource_limits.clone(),
    };

    let sign_bytes = serde_json::to_vec(&sign_target)
        .map_err(|e| GatewayError::Internal(format!("署名対象のシリアライズに失敗: {e}")))?;

    let signature = signing_key.sign(&sign_bytes);
    let signature_b64 = b64().encode(signature.to_bytes());

    Ok(GatewayAuthWrapper {
        method: method.to_string(),
        path: path.to_string(),
        body,
        resource_limits,
        gateway_signature: signature_b64,
    })
}

/// TEEにリクエストを中継する。
/// 仕様書 §6.2: Gateway認証署名を付与してTEEにリクエストを転送する。
pub(crate) async fn relay_to_tee(
    state: &GatewayState,
    path: &str,
    body: serde_json::Value,
) -> Result<serde_json::Value, GatewayError> {
    let wrapper = build_gateway_auth_wrapper(
        &state.signing_key,
        "POST",
        path,
        body,
        Some(state.default_resource_limits.clone()),
    )?;

    let url = format!("{}{}", state.tee_endpoint, path);
    let response = state
        .http_client
        .post(&url)
        .json(&wrapper)
        .send()
        .await
        .map_err(|e| GatewayError::TeeRelay(format!("HTTP送信失敗: {e}")))?;

    let status = response.status();
    let response_body = response
        .text()
        .await
        .map_err(|e| GatewayError::TeeRelay(format!("レスポンス読み取り失敗: {e}")))?;

    if !status.is_success() {
        return Err(GatewayError::TeeRelay(format!(
            "TEEがエラーを返しました: HTTP {} - {}",
            status, response_body
        )));
    }

    serde_json::from_str(&response_body)
        .map_err(|e| GatewayError::TeeRelay(format!("レスポンスのパースに失敗: {e}")))
}
