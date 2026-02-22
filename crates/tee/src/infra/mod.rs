//! # TEEインフラストラクチャモジュール
//!
//! 仕様書 §6.4
//!
//! TEEの外部通信・認証・セキュリティに関するモジュール。
//! - `gateway_auth`: Gateway認証検証
//! - `proxy_client`: vsock/HTTPプロキシクライアント
//! - `security`: DoS対策・リソース制限

pub mod gateway_auth;
pub mod proxy_client;
pub mod security;
