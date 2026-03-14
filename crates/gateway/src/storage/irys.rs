// SPDX-License-Identifier: Apache-2.0

//! # Irys (Arweave) signed_jsonストレージ実装
//!
//! ANS-104 DataItem形式でsigned_jsonをIrys経由でArweaveに永続保存する。
//! cNFTメタデータURIとしてArweaveの永続保証が必要な場合に使用する。

use ed25519_dalek::{Signer, SigningKey as Ed25519SigningKey};
use sha2::{Digest, Sha384};

use super::SignedJsonStorage;
use crate::error::GatewayError;

/// Irys (Arweave) によるsigned_jsonストレージ実装。
///
/// ANS-104 DataItemを構築・署名し、Irysバンドラーにアップロードする。
/// データはArweaveに永続保存され、`https://arweave.net/<tx_id>` でアクセスできる。
pub struct IrysSignedJsonStorage {
    /// IrysアップローダーのベースURL
    node_url: String,
    /// Arweaveゲートウェイ（ダウンロードURL用）
    gateway_url: String,
    /// DataItem署名用Ed25519秘密鍵（Irysにファンド済みのウォレット）
    signing_key: Ed25519SigningKey,
    /// HTTPクライアント
    http_client: reqwest::Client,
}

impl IrysSignedJsonStorage {
    /// 環境変数から構築する。
    ///
    /// `IRYS_PRIVATE_KEY` が未設定の場合は `None` を返す（機能無効）。
    ///
    /// | 環境変数 | デフォルト | 説明 |
    /// |---------|----------|------|
    /// | `IRYS_PRIVATE_KEY` | (必須) | Ed25519秘密鍵（Base58、64バイトのSolanaキーペア形式） |
    /// | `IRYS_NODE_URL` | `https://uploader.irys.xyz` | Irysアップローダーエンドポイント |
    /// | `IRYS_GATEWAY_URL` | `https://arweave.net` | ArweaveゲートウェイURL |
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        let private_key_str = match std::env::var("IRYS_PRIVATE_KEY") {
            Ok(key) if !key.is_empty() => key,
            _ => return Ok(None),
        };

        // Base58エンコードされたSolanaキーペア（64バイト: 秘密鍵32 + 公開鍵32）
        let key_bytes = bs58::decode(&private_key_str)
            .into_vec()
            .map_err(|e| anyhow::anyhow!("IRYS_PRIVATE_KEY のBase58デコードに失敗: {e}"))?;

        let secret_bytes: [u8; 32] = if key_bytes.len() == 64 {
            // Solana形式: 先頭32バイトが秘密鍵
            key_bytes[..32].try_into().unwrap()
        } else if key_bytes.len() == 32 {
            key_bytes.try_into().unwrap()
        } else {
            return Err(anyhow::anyhow!(
                "IRYS_PRIVATE_KEY は32バイトまたは64バイト(Solanaキーペア形式)が必要です (got {})",
                key_bytes.len()
            ));
        };

        let signing_key = Ed25519SigningKey::from_bytes(&secret_bytes);

        let node_url = std::env::var("IRYS_NODE_URL")
            .unwrap_or_else(|_| "https://uploader.irys.xyz".to_string());
        let gateway_url = std::env::var("IRYS_GATEWAY_URL")
            .unwrap_or_else(|_| "https://arweave.net".to_string());

        tracing::info!(
            node_url = %node_url,
            gateway_url = %gateway_url,
            pubkey = %bs58::encode(signing_key.verifying_key().as_bytes()).into_string(),
            "Irys signed_jsonストレージを設定"
        );

        Ok(Some(Self {
            node_url,
            gateway_url,
            signing_key,
            http_client: reqwest::Client::new(),
        }))
    }
}

#[async_trait::async_trait]
impl SignedJsonStorage for IrysSignedJsonStorage {
    /// signed_jsonをIrys経由でArweaveにアップロードし、永続URIを返す。
    async fn store(&self, _key: &str, data: &[u8]) -> Result<String, GatewayError> {
        // ANS-104 DataItemを構築・署名
        let tags = vec![
            Tag { name: b"Content-Type".to_vec(), value: b"application/json".to_vec() },
        ];
        let data_item = create_signed_data_item(&self.signing_key, &tags, data);

        // Irysにアップロード
        let url = format!("{}/tx/solana", self.node_url);
        let response = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .body(data_item)
            .send()
            .await
            .map_err(|e| GatewayError::Storage(format!("Irysアップロード失敗: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown".to_string());
            return Err(GatewayError::Storage(format!(
                "Irysアップロード失敗: HTTP {status} - {body}"
            )));
        }

        let res_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| GatewayError::Storage(format!("Irysレスポンスのパースに失敗: {e}")))?;

        let tx_id = res_body
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                GatewayError::Storage(format!(
                    "IrysレスポンスにIDがありません: {res_body}"
                ))
            })?;

        Ok(format!("{}/{}", self.gateway_url, tx_id))
    }
}

// ---------------------------------------------------------------------------
// ANS-104 DataItem
// ---------------------------------------------------------------------------

/// ANS-104タグ
struct Tag {
    name: Vec<u8>,
    value: Vec<u8>,
}

/// ANS-104 DataItemを構築し、Ed25519で署名する。
///
/// DataItem binary format:
/// ```text
/// [2B: sig_type] [64B: signature] [32B: owner]
/// [1B: target_present] [1B: anchor_present]
/// [8B: num_tags LE] [8B: tags_byte_len LE] [tags_bytes] [data]
/// ```
fn create_signed_data_item(
    signing_key: &Ed25519SigningKey,
    tags: &[Tag],
    data: &[u8],
) -> Vec<u8> {
    let owner = signing_key.verifying_key().to_bytes();
    let tags_bytes = encode_avro_tags(tags);

    // Deep hashで署名対象を計算
    let message = deep_hash_data_item(
        &owner,
        &[],     // no target
        &[],     // no anchor
        &tags_bytes,
        data,
    );

    let signature = signing_key.sign(&message);

    // DataItemバイナリを構築
    let mut buf = Vec::new();

    // Signature type: 2 = Ed25519 (2 bytes LE)
    buf.extend_from_slice(&2u16.to_le_bytes());
    // Signature (64 bytes)
    buf.extend_from_slice(&signature.to_bytes());
    // Owner (32 bytes)
    buf.extend_from_slice(&owner);
    // Target present: 0 (1 byte)
    buf.push(0);
    // Anchor present: 0 (1 byte)
    buf.push(0);
    // Number of tags (8 bytes LE)
    buf.extend_from_slice(&(tags.len() as u64).to_le_bytes());
    // Tags byte length (8 bytes LE)
    buf.extend_from_slice(&(tags_bytes.len() as u64).to_le_bytes());
    // Tags bytes
    buf.extend_from_slice(&tags_bytes);
    // Data
    buf.extend_from_slice(data);

    buf
}

/// ANS-104 DataItemのdeep hash計算。
///
/// `deepHash(["dataitem", "1", sigType, owner, target, anchor, tags, data])`
fn deep_hash_data_item(
    owner: &[u8],
    target: &[u8],
    anchor: &[u8],
    tags_bytes: &[u8],
    data: &[u8],
) -> Vec<u8> {
    let items: Vec<&[u8]> = vec![
        b"dataitem",
        b"1",
        b"2", // Ed25519 signature type
        owner,
        target,
        anchor,
        tags_bytes,
        data,
    ];

    deep_hash_list(&items)
}

/// Arweave deep hash — リスト版。
///
/// ```text
/// tag = SHA384("list" + length.toString())
/// for each element:
///     tag = SHA384(tag + deepHash(element))
/// return tag
/// ```
fn deep_hash_list(items: &[&[u8]]) -> Vec<u8> {
    let tag_input = format!("list{}", items.len());
    let mut acc = sha384(tag_input.as_bytes());

    for item in items {
        let item_hash = deep_hash_leaf(item);
        let mut combined = acc;
        combined.extend_from_slice(&item_hash);
        acc = sha384(&combined);
    }

    acc
}

/// Arweave deep hash — リーフ（バイト列）版。
///
/// ```text
/// tag = SHA384("blob" + byteLength.toString())
/// return SHA384(tag + SHA384(data))
/// ```
fn deep_hash_leaf(data: &[u8]) -> Vec<u8> {
    let tag_input = format!("blob{}", data.len());
    let tag_hash = sha384(tag_input.as_bytes());
    let data_hash = sha384(data);

    let mut combined = tag_hash;
    combined.extend_from_slice(&data_hash);
    sha384(&combined)
}

/// SHA-384ハッシュ
fn sha384(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha384::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

/// タグをAvro形式でエンコードする（ANS-104仕様）。
///
/// Avro array encoding:
/// - Block count (zigzag varint)
/// - For each tag: name (bytes), value (bytes)
/// - End marker: 0
fn encode_avro_tags(tags: &[Tag]) -> Vec<u8> {
    if tags.is_empty() {
        return vec![0]; // empty array
    }

    let mut buf = Vec::new();

    // Block count
    encode_avro_long(&mut buf, tags.len() as i64);

    for tag in tags {
        // name: bytes
        encode_avro_bytes(&mut buf, &tag.name);
        // value: bytes
        encode_avro_bytes(&mut buf, &tag.value);
    }

    // End of array
    buf.push(0);

    buf
}

/// Avro long (zigzag + varint) エンコード
fn encode_avro_long(buf: &mut Vec<u8>, n: i64) {
    let mut zigzag = ((n << 1) ^ (n >> 63)) as u64;
    loop {
        if zigzag <= 0x7F {
            buf.push(zigzag as u8);
            break;
        }
        buf.push(((zigzag & 0x7F) | 0x80) as u8);
        zigzag >>= 7;
    }
}

/// Avro bytes エンコード (length + data)
fn encode_avro_bytes(buf: &mut Vec<u8>, data: &[u8]) {
    encode_avro_long(buf, data.len() as i64);
    buf.extend_from_slice(data);
}

// ---------------------------------------------------------------------------
// bs58 — Base58 decode/encode (Solanaキーペア用)
// ---------------------------------------------------------------------------
// base58 crate (0.2) は encode のみ。decode には bs58 を使用。
// gateway は base58 (encode) + solana-sdk (bs58 経由) を持っている。

mod bs58 {
    pub fn decode(input: &str) -> Decoder {
        Decoder(input.to_string())
    }

    pub fn encode(input: &[u8]) -> Encoder {
        Encoder(input.to_vec())
    }

    pub struct Decoder(String);
    impl Decoder {
        pub fn into_vec(self) -> Result<Vec<u8>, String> {
            // Use solana_sdk's bs58 re-export
            solana_sdk::bs58::decode(&self.0)
                .into_vec()
                .map_err(|e| e.to_string())
        }
    }

    pub struct Encoder(Vec<u8>);
    impl Encoder {
        pub fn into_string(self) -> String {
            solana_sdk::bs58::encode(&self.0).into_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_avro_empty_tags() {
        let encoded = encode_avro_tags(&[]);
        assert_eq!(encoded, vec![0]);
    }

    #[test]
    fn test_avro_single_tag() {
        let tags = vec![Tag {
            name: b"Content-Type".to_vec(),
            value: b"application/json".to_vec(),
        }];
        let encoded = encode_avro_tags(&tags);
        // block count: zigzag(1) = 2, varint = [0x02]
        assert_eq!(encoded[0], 0x02);
        // name length: zigzag(12) = 24, varint = [0x18]
        assert_eq!(encoded[1], 0x18);
        // "Content-Type" (12 bytes)
        assert_eq!(&encoded[2..14], b"Content-Type");
        // value length: zigzag(16) = 32, varint = [0x20]
        assert_eq!(encoded[14], 0x20);
        // "application/json" (16 bytes)
        assert_eq!(&encoded[15..31], b"application/json");
        // end marker
        assert_eq!(encoded[31], 0x00);
    }

    #[test]
    fn test_deep_hash_deterministic() {
        let hash1 = deep_hash_leaf(b"hello");
        let hash2 = deep_hash_leaf(b"hello");
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 48); // SHA-384 = 48 bytes
    }

    #[test]
    fn test_data_item_structure() {
        let signing_key = Ed25519SigningKey::generate(&mut rand::rngs::OsRng);
        let tags = vec![Tag {
            name: b"Content-Type".to_vec(),
            value: b"application/json".to_vec(),
        }];
        let data = b"test data";
        let item = create_signed_data_item(&signing_key, &tags, data);

        // Verify structure
        assert_eq!(&item[0..2], &2u16.to_le_bytes()); // sig type
        // signature: 64 bytes at offset 2
        // owner: 32 bytes at offset 66
        let owner = &item[66..98];
        assert_eq!(owner, signing_key.verifying_key().as_bytes());
        // target present: 0 at offset 98
        assert_eq!(item[98], 0);
        // anchor present: 0 at offset 99
        assert_eq!(item[99], 0);
    }

    #[test]
    fn test_avro_long_encoding() {
        let mut buf = Vec::new();
        encode_avro_long(&mut buf, 0);
        assert_eq!(buf, vec![0x00]);

        let mut buf = Vec::new();
        encode_avro_long(&mut buf, 1);
        assert_eq!(buf, vec![0x02]);

        let mut buf = Vec::new();
        encode_avro_long(&mut buf, 64);
        assert_eq!(buf, vec![0x80, 0x01]);
    }
}
