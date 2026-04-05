// SPDX-License-Identifier: Apache-2.0

//! # 暗号アルゴリズム識別子
//!
//! 3ペア（署名・KEM・AEAD）のアルゴリズム列挙型と、
//! KEM+AEADの組であるCryptoSuite。
//!
//! 根拠: RustCrypto `signature`/`kem`/`aead` crates, HPKE (RFC 9180)

use serde::{Deserialize, Deserializer, Serialize, Serializer};

// ---------------------------------------------------------------------------
// 署名アルゴリズム
// ---------------------------------------------------------------------------

/// 署名アルゴリズム識別子。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SigningAlgorithm {
    /// Ed25519 (RFC 8032)。32バイト鍵、64バイト署名。
    Ed25519,
    // 将来: MlDsa65 (FIPS 204), SlhDsa128f (FIPS 205), etc.
}

impl SigningAlgorithm {
    /// 正規文字列識別子。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ed25519 => "ed25519",
        }
    }

    /// 文字列からパース。未知のアルゴリズムは `None`。
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "ed25519" => Some(Self::Ed25519),
            _ => None,
        }
    }
}

impl Serialize for SigningAlgorithm {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SigningAlgorithm {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).ok_or_else(|| serde::de::Error::unknown_variant(&s, &["ed25519"]))
    }
}

// ---------------------------------------------------------------------------
// KEM（鍵カプセル化メカニズム）アルゴリズム
// ---------------------------------------------------------------------------

/// KEMアルゴリズム識別子。KDFを含む。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KemAlgorithm {
    /// X25519 ECDH + HKDF-SHA256。32バイト鍵、32バイトencapsulated_key。
    X25519HkdfSha256,
    // 将来: MlKem768HkdfSha256 (FIPS 203), etc.
}

impl KemAlgorithm {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::X25519HkdfSha256 => "x25519-hkdf-sha256",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "x25519-hkdf-sha256" => Some(Self::X25519HkdfSha256),
            _ => None,
        }
    }
}

impl Serialize for KemAlgorithm {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for KemAlgorithm {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s)
            .ok_or_else(|| serde::de::Error::unknown_variant(&s, &["x25519-hkdf-sha256"]))
    }
}

// ---------------------------------------------------------------------------
// AEAD（認証付き暗号）アルゴリズム
// ---------------------------------------------------------------------------

/// AEADアルゴリズム識別子。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AeadAlgorithm {
    /// AES-256-GCM。12バイトnonce、16バイトタグ。
    Aes256Gcm,
    // 将来: ChaCha20Poly1305, Aes256GcmSiv, etc.
}

impl AeadAlgorithm {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Aes256Gcm => "aes-256-gcm",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "aes-256-gcm" => Some(Self::Aes256Gcm),
            _ => None,
        }
    }

    /// このアルゴリズムのnonce長（バイト）。
    pub fn nonce_size(&self) -> usize {
        match self {
            Self::Aes256Gcm => 12,
        }
    }
}

impl Serialize for AeadAlgorithm {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AeadAlgorithm {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).ok_or_else(|| serde::de::Error::unknown_variant(&s, &["aes-256-gcm"]))
    }
}

// ---------------------------------------------------------------------------
// CryptoSuite（KEM + AEADの組）
// ---------------------------------------------------------------------------

/// KEM+AEADの組。ワイヤーフォーマットのsuite_idバイトに対応。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CryptoSuite {
    /// X25519-HKDF-SHA256 + AES-256-GCM
    X25519Aes256Gcm = 0x01,
    // 将来: MlKem768Aes256Gcm = 0x02, etc.
}

impl CryptoSuite {
    pub fn kem_algorithm(&self) -> KemAlgorithm {
        match self {
            Self::X25519Aes256Gcm => KemAlgorithm::X25519HkdfSha256,
        }
    }

    pub fn aead_algorithm(&self) -> AeadAlgorithm {
        match self {
            Self::X25519Aes256Gcm => AeadAlgorithm::Aes256Gcm,
        }
    }

    pub fn nonce_size(&self) -> usize {
        self.aead_algorithm().nonce_size()
    }

    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            0x01 => Some(Self::X25519Aes256Gcm),
            _ => None,
        }
    }

    pub fn id(&self) -> u8 {
        *self as u8
    }
}
