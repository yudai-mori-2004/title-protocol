// SPDX-License-Identifier: Apache-2.0

//! Gateway HTTP client — CLI サブコマンドの共通 HTTP 呼び出し。

use reqwest::blocking::Client;
use serde_json::Value;

pub struct GatewayClient {
    base_url: String,
    api_key: Option<String>,
    http: Client,
}

impl GatewayClient {
    pub fn new(base_url: &str, api_key: Option<&str>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.map(|s| s.to_string()),
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn add_auth(&self, req: reqwest::blocking::RequestBuilder) -> reqwest::blocking::RequestBuilder {
        match &self.api_key {
            Some(key) => req.bearer_auth(key),
            None => req,
        }
    }

    /// GET して JSON を pretty-print する。
    pub fn get_json(&self, path: &str) -> Result<(), String> {
        let resp = self
            .add_auth(self.http.get(self.url(path)))
            .send()
            .map_err(|e| format!("request failed: {e}"))?;

        let status = resp.status();
        let body: Value = resp
            .json()
            .map_err(|e| format!("invalid JSON response: {e}"))?;

        if !status.is_success() {
            return Err(format!("HTTP {status}\n{}", serde_json::to_string_pretty(&body).unwrap()));
        }

        println!("{}", serde_json::to_string_pretty(&body).unwrap());
        Ok(())
    }

    /// GET /health — 特殊表示 (TEE type + status)
    pub fn health(&self) -> Result<(), String> {
        let resp = self
            .http
            .get(self.url("/health"))
            .send()
            .map_err(|e| format!("gateway unreachable: {e}"))?;

        let status = resp.status();
        let body: Value = resp
            .json()
            .map_err(|e| format!("invalid JSON: {e}"))?;

        if !status.is_success() {
            return Err(format!("HTTP {status}"));
        }

        let tee_type = body["tee_type"].as_str().unwrap_or("unknown");
        let uptime = body["uptime_secs"].as_u64().unwrap_or(0);

        println!("status:   healthy");
        println!("tee_type: {tee_type}");
        println!("uptime:   {uptime}s");
        Ok(())
    }

    /// POST /process (single input)
    pub fn process_single(&self, url: &str, processor_ids: &[String]) -> Result<(), String> {
        let body = serde_json::json!({
            "input_type": "single",
            "content_url": url,
            "processor_ids": processor_ids,
        });
        self.post_process(&body)
    }

    /// POST /process (sidecar input)
    pub fn process_sidecar(
        &self,
        manifest_url: &str,
        content_url: &str,
        processor_ids: &[String],
    ) -> Result<(), String> {
        let body = serde_json::json!({
            "input_type": "sidecar",
            "manifest_url": manifest_url,
            "content_url": content_url,
            "processor_ids": processor_ids,
        });
        self.post_process(&body)
    }

    fn post_process(&self, body: &Value) -> Result<(), String> {
        let resp = self
            .add_auth(self.http.post(self.url("/process")))
            .json(body)
            .send()
            .map_err(|e| format!("request failed: {e}"))?;

        let status = resp.status();
        let resp_body: Value = resp
            .json()
            .map_err(|e| format!("invalid JSON response: {e}"))?;

        if !status.is_success() {
            return Err(format!(
                "HTTP {status}\n{}",
                serde_json::to_string_pretty(&resp_body).unwrap()
            ));
        }

        println!("{}", serde_json::to_string_pretty(&resp_body).unwrap());
        Ok(())
    }

    /// POST /solana/create-tree — 部分署名済み create-tree TX を取得。
    pub fn create_tree(
        &self,
        payer: &str,
        max_depth: u32,
        max_buffer_size: u32,
        recent_blockhash: &str,
    ) -> Result<Value, String> {
        let body = serde_json::json!({
            "payer": payer,
            "max_depth": max_depth,
            "max_buffer_size": max_buffer_size,
            "recent_blockhash": recent_blockhash,
        });
        self.post_json("/solana/create-tree", &body)
    }

    /// POST /extension/solana — 部分署名済み mint TX を取得。
    pub fn solana_extension(
        &self,
        offchain_data_url: &str,
        payer: &str,
        merkle_tree: &str,
        recent_blockhash: &str,
        collection: Option<&str>,
    ) -> Result<Value, String> {
        let mut body = serde_json::json!({
            "offchain_data_url": offchain_data_url,
            "payer": payer,
            "merkle_tree": merkle_tree,
            "recent_blockhash": recent_blockhash,
        });
        if let Some(c) = collection {
            body["collection"] = Value::String(c.to_string());
        }
        self.post_json("/extension/solana", &body)
    }

    /// POST して JSON を返す。
    fn post_json(&self, path: &str, body: &Value) -> Result<Value, String> {
        let resp = self
            .add_auth(self.http.post(self.url(path)))
            .json(body)
            .send()
            .map_err(|e| format!("request failed: {e}"))?;

        let status = resp.status();
        let resp_body: Value = resp
            .json()
            .map_err(|e| format!("invalid JSON response: {e}"))?;

        if !status.is_success() {
            return Err(format!(
                "HTTP {status}\n{}",
                serde_json::to_string_pretty(&resp_body).unwrap()
            ));
        }

        Ok(resp_body)
    }
}
