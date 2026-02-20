//! # Gateway認証検証ユーティリティ
//!
//! 仕様書 §6.2
//!
//! TEE側でGateway署名を検証し、リクエスト本文とリソース制限を抽出する。
//! Gateway認証により、信頼されたGateway経由のリクエストのみを受け付ける。

use axum::http::StatusCode;
use base64::Engine;

use title_crypto::Ed25519VerifyingKey;
use title_types::{GatewayAuthSignTarget, GatewayAuthWrapper, ResourceLimits};

/// Base64エンジン（Standard）
fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// Gateway認証を検証し、内部のリクエストボディとリソース制限を返す。
/// 仕様書 §6.2
///
/// - `gateway_pubkey` が `Some` の場合: Gateway認証が必須。署名なしまたは不正署名は拒否。
/// - `gateway_pubkey` が `None` の場合: Gateway認証をスキップ（開発環境用）。
///
/// 受信bodyが GatewayAuthWrapper 形式（`gateway_signature` フィールドあり）なら署名を検証し、
/// `body` フィールドと `resource_limits` を返す。
/// 直接リクエスト形式の場合は `gateway_pubkey` が `None` のときのみ許可する。
pub fn verify_gateway_auth(
    gateway_pubkey: Option<&Ed25519VerifyingKey>,
    body: &serde_json::Value,
) -> Result<(serde_json::Value, Option<ResourceLimits>), (StatusCode, String)> {
    if body.get("gateway_signature").is_some() {
        // GatewayAuthWrapper形式
        let wrapper: GatewayAuthWrapper = serde_json::from_value(body.clone()).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("GatewayAuthWrapperのパースに失敗: {e}"),
            )
        })?;

        if let Some(pubkey) = gateway_pubkey {
            // 署名対象を再構築（gateway_signatureを除いた部分）
            let sign_target = GatewayAuthSignTarget {
                method: wrapper.method.clone(),
                path: wrapper.path.clone(),
                body: wrapper.body.clone(),
                resource_limits: wrapper.resource_limits.clone(),
            };
            let sign_bytes = serde_json::to_vec(&sign_target).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("署名対象のシリアライズに失敗: {e}"),
                )
            })?;

            // 署名をデコード
            let sig_bytes = b64().decode(&wrapper.gateway_signature).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("gateway_signatureのBase64デコードに失敗: {e}"),
                )
            })?;
            let sig_arr: [u8; 64] = sig_bytes.try_into().map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    "gateway_signatureは64バイトである必要があります".to_string(),
                )
            })?;
            let signature = ed25519_dalek::Signature::from_bytes(&sig_arr);

            // Ed25519署名を検証
            title_crypto::ed25519_verify(pubkey, &sign_bytes, &signature).map_err(|_| {
                (
                    StatusCode::FORBIDDEN,
                    "Gateway署名の検証に失敗しました".to_string(),
                )
            })?;
        }
        // gateway_pubkeyがNoneの場合は署名検証をスキップ（開発環境・後方互換性）

        Ok((wrapper.body, wrapper.resource_limits))
    } else {
        // 直接リクエスト形式
        if gateway_pubkey.is_some() {
            return Err((
                StatusCode::UNAUTHORIZED,
                "Gateway認証が必要です。gateway_signatureを含むGatewayAuthWrapper形式で送信してください".to_string(),
            ));
        }
        Ok((body.clone(), None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey as Ed25519SigningKey};

    /// Gateway署名が正しく検証されることを確認
    #[test]
    fn test_verify_valid_signature() {
        let signing_key = Ed25519SigningKey::generate(&mut rand::rngs::OsRng);
        let verifying_key = Ed25519VerifyingKey::from(&signing_key);

        let body = serde_json::json!({"download_url": "http://example.com", "processor_ids": ["core-c2pa"]});
        let resource_limits = Some(ResourceLimits {
            max_single_content_bytes: Some(1024),
            max_concurrent_bytes: None,
            min_upload_speed_bytes: None,
            base_processing_time_sec: None,
            max_global_timeout_sec: None,
            chunk_read_timeout_sec: None,
            c2pa_max_graph_size: None,
        });

        // 署名対象を構築して署名
        let sign_target = GatewayAuthSignTarget {
            method: "POST".to_string(),
            path: "/verify".to_string(),
            body: body.clone(),
            resource_limits: resource_limits.clone(),
        };
        let sign_bytes = serde_json::to_vec(&sign_target).unwrap();
        let signature = signing_key.sign(&sign_bytes);
        let sig_b64 = b64().encode(signature.to_bytes());

        let wrapper = serde_json::json!({
            "method": "POST",
            "path": "/verify",
            "body": body,
            "resource_limits": resource_limits,
            "gateway_signature": sig_b64,
        });

        let result = verify_gateway_auth(Some(&verifying_key), &wrapper);
        assert!(result.is_ok());

        let (inner_body, limits) = result.unwrap();
        assert_eq!(inner_body, body);
        assert!(limits.is_some());
    }

    /// 不正なGateway署名が拒否されることを確認
    #[test]
    fn test_verify_invalid_signature() {
        let signing_key = Ed25519SigningKey::generate(&mut rand::rngs::OsRng);
        let other_key = Ed25519SigningKey::generate(&mut rand::rngs::OsRng);
        let other_verifying = Ed25519VerifyingKey::from(&other_key);

        let body = serde_json::json!({"test": "data"});

        let sign_target = GatewayAuthSignTarget {
            method: "POST".to_string(),
            path: "/verify".to_string(),
            body: body.clone(),
            resource_limits: None,
        };
        let sign_bytes = serde_json::to_vec(&sign_target).unwrap();
        let signature = signing_key.sign(&sign_bytes);
        let sig_b64 = b64().encode(signature.to_bytes());

        let wrapper = serde_json::json!({
            "method": "POST",
            "path": "/verify",
            "body": body,
            "gateway_signature": sig_b64,
        });

        // 別の公開鍵で検証 → 403
        let result = verify_gateway_auth(Some(&other_verifying), &wrapper);
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    /// Gateway認証が必須の場合に署名なしリクエストが拒否されることを確認
    #[test]
    fn test_verify_missing_signature_when_required() {
        let signing_key = Ed25519SigningKey::generate(&mut rand::rngs::OsRng);
        let verifying_key = Ed25519VerifyingKey::from(&signing_key);

        let body = serde_json::json!({"download_url": "http://example.com", "processor_ids": ["core-c2pa"]});

        // 直接リクエスト形式（gateway_signatureなし）
        let result = verify_gateway_auth(Some(&verifying_key), &body);
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    /// Gateway認証が不要な場合に直接リクエストが許可されることを確認
    #[test]
    fn test_verify_direct_request_without_gateway() {
        let body = serde_json::json!({"download_url": "http://example.com", "processor_ids": ["core-c2pa"]});

        let result = verify_gateway_auth(None, &body);
        assert!(result.is_ok());

        let (inner_body, limits) = result.unwrap();
        assert_eq!(inner_body, body);
        assert!(limits.is_none());
    }
}
