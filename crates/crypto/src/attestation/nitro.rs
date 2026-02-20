//! # AWS Nitro Attestation Document 検証
//!
//! 仕様書 §5.2 Step 4.1
//!
//! AWS Nitro EnclaveのAttestation Documentを検証し、
//! PCR値と公開鍵を抽出する。
//!
//! ## Attestation Document構造
//!
//! COSE Sign1エンベロープ（ES384署名）のペイロードがCBORマップ:
//! - `module_id`: Enclaveモジュール識別子
//! - `digest`: ハッシュアルゴリズム（"SHA384"）
//! - `timestamp`: Unix timestamp (ms)
//! - `pcrs`: PCR値マップ (インデックス → 48バイト)
//! - `certificate`: リーフ証明書（DER）
//! - `cabundle`: 中間証明書の配列（DER）
//! - `public_key`: リクエスト時に指定した公開鍵
//! - `user_data`: リクエスト時に指定したユーザーデータ
//! - `nonce`: リクエスト時に指定したノンス

use std::collections::BTreeMap;

use coset::CborSerializable;
use der::Decode;
use p384::ecdsa::signature::Verifier;

use super::AttestationError;

/// AWS Nitro Attestation PKIルート証明書（DER形式、base64エンコード）。
///
/// Subject: CN=aws.nitro-enclaves, O=Amazon, OU=AWS, C=US
/// Validity: 2019-10-28 ~ 2049-10-28
/// Algorithm: ECDSA P-384
const AWS_NITRO_ROOT_CERT_B64: &str = "\
MIICETCCAZagAwIBAgIRAPkxdWgbkK/hHUbMtOTn+FYwCgYIKoZIzj0EAwMwSTEL\
MAkGA1UEBhMCVVMxDzANBgNVBAoMBkFtYXpvbjEMMAoGA1UECwwDQVdTMRswGQYD\
VQQDDBJhd3Mubml0cm8tZW5jbGF2ZXMwHhcNMTkxMDI4MTMyODA1WhcNNDkxMDI4\
MTQyODA1WjBJMQswCQYDVQQGEwJVUzEPMA0GA1UECgwGQW1hem9uMQwwCgYDVQQL\
DANBV1MxGzAZBgNVBAMMEmF3cy5uaXRyby1lbmNsYXZlczB2MBAGByqGSM49AgEG\
BSuBBAAiA2IABPwCVOumCMHzaHDimtqQvkY4MpJzbolL//Zy2YlES1BR5TSksfbb\
48C8WBoyt7F2Bw7eEtaaP+ohG2bnUs990d0JX28TcPQXCEPZ3BABIeTPYwEoCWZE\
h8l5YoQwTcU/9KNCMEAwDwYDVR0TAQH/BAUwAwEB/zAdBgNVHQ4EFgQUkCW1DdkF\
R+eWw5b6cp3PmanfS5YwDgYDVR0PAQH/BAQDAgGGMAoGCCqGSM49BAMDA2kAMGYC\
MQCjfy+Rocm9Xue4YnwWmNJVA44fA0P5W2OpYow9OYCVRaEevL8uO1XYru5xtMPW\
rfMCMQCi85sWBbJwKKXdS6BptQFuZbT73o/gBh1qUxl/nNr12UO8Yfwr6wPLb+6N\
IwLz3/Y=";

/// AWS Nitro固有のAttestation Document検証結果。
/// 仕様書 §5.2 Step 4.1
///
/// Nitro固有の詳細（証明書チェーン、module_id等）が必要な場合はこの型を使用する。
/// TEE種別に依存しない共通情報のみ必要な場合は [`super::AttestationResult`] に変換可能。
#[derive(Debug, Clone)]
pub struct NitroAttestationResult {
    /// Enclaveモジュール識別子
    pub module_id: String,
    /// ハッシュアルゴリズム（通常 "SHA384"）
    pub digest: String,
    /// Attestation生成時のタイムスタンプ（Unix ms）
    pub timestamp: u64,
    /// PCR値マップ（インデックス → 測定値バイト列）
    pub pcrs: BTreeMap<u32, Vec<u8>>,
    /// リーフ証明書（DER）
    pub certificate: Vec<u8>,
    /// 中間証明書チェーン（DER配列）
    pub cabundle: Vec<Vec<u8>>,
    /// リクエスト時に指定した公開鍵（TEE署名用公開鍵）
    pub public_key: Option<Vec<u8>>,
    /// リクエスト時に指定したユーザーデータ（TEE暗号化用公開鍵）
    pub user_data: Option<Vec<u8>>,
    /// リクエスト時に指定したノンス
    pub nonce: Option<Vec<u8>>,
}

/// AWS Nitro Attestation Documentを検証し、内容を抽出する。
/// 仕様書 §5.2 Step 4.1
///
/// 検証手順:
/// 1. COSE Sign1をパース
/// 2. ペイロード（CBOR）をパースしてフィールド抽出
/// 3. 証明書チェーンをAWS Nitro PKIルートまで検証
/// 4. リーフ証明書の公開鍵でCOSE署名を検証
pub fn verify_nitro_attestation(
    document: &[u8],
) -> Result<NitroAttestationResult, AttestationError> {
    // 1. COSE Sign1をパース
    let cose_sign1 = coset::CoseSign1::from_slice(document)
        .map_err(|e| AttestationError::CoseParseError(format!("{:?}", e)))?;

    // 2. ペイロード（CBOR）をパース
    let payload_bytes = cose_sign1
        .payload
        .as_ref()
        .ok_or_else(|| AttestationError::MissingField("payload".into()))?;
    let result = extract_attestation_fields(payload_bytes)?;

    // 3. 証明書チェーンの検証
    verify_cert_chain(&result.certificate, &result.cabundle)?;

    // 4. COSE署名の検証（リーフ証明書の公開鍵で）
    verify_cose_signature(&cose_sign1, &result.certificate)?;

    Ok(result)
}

/// Attestation Documentのペイロードをパースのみ行う（署名検証なし）。
/// テストやデバッグ用途。
pub fn parse_attestation_payload(
    document: &[u8],
) -> Result<NitroAttestationResult, AttestationError> {
    let cose_sign1 = coset::CoseSign1::from_slice(document)
        .map_err(|e| AttestationError::CoseParseError(format!("{:?}", e)))?;

    let payload_bytes = cose_sign1
        .payload
        .as_ref()
        .ok_or_else(|| AttestationError::MissingField("payload".into()))?;
    extract_attestation_fields(payload_bytes)
}

/// PCR値が期待値と一致するか確認する。
/// 仕様書 §5.2 Step 4.1 — 測定値の照合
pub fn verify_pcr_values(
    result: &NitroAttestationResult,
    expected_pcrs: &BTreeMap<u32, Vec<u8>>,
) -> bool {
    expected_pcrs.iter().all(|(idx, expected)| {
        result
            .pcrs
            .get(idx)
            .map_or(false, |actual| actual == expected)
    })
}

/// 公開鍵が期待値と一致するか確認する。
/// 仕様書 §5.2 Step 4.1 — 公開鍵フィールドの一致確認
pub fn verify_public_key(result: &NitroAttestationResult, expected_pubkey: &[u8]) -> bool {
    result
        .public_key
        .as_ref()
        .map_or(false, |pk| pk == expected_pubkey)
}

// ─────────────────────────────────────────────
// 内部関数
// ─────────────────────────────────────────────

/// CBORペイロードからAttestation Documentのフィールドを抽出する。
fn extract_attestation_fields(
    payload_bytes: &[u8],
) -> Result<NitroAttestationResult, AttestationError> {
    let value: ciborium::Value = ciborium::from_reader(payload_bytes)
        .map_err(|e| AttestationError::CborParseError(e.to_string()))?;

    let map = match &value {
        ciborium::Value::Map(m) => m,
        _ => {
            return Err(AttestationError::CborParseError(
                "ペイロードがCBORマップではありません".into(),
            ))
        }
    };

    let module_id = get_text_field(map, "module_id")?;
    let digest = get_text_field(map, "digest")?;
    let timestamp = get_integer_field(map, "timestamp")?;
    let pcrs = get_pcrs_field(map)?;
    let certificate = get_bytes_field(map, "certificate")?;
    let cabundle = get_bytes_array_field(map, "cabundle")?;
    let public_key = get_optional_bytes_field(map, "public_key");
    let user_data = get_optional_bytes_field(map, "user_data");
    let nonce = get_optional_bytes_field(map, "nonce");

    Ok(NitroAttestationResult {
        module_id,
        digest,
        timestamp,
        pcrs,
        certificate,
        cabundle,
        public_key,
        user_data,
        nonce,
    })
}

/// 証明書チェーンを検証する。
/// リーフ → 中間CA群 → AWS Nitroルート の順にECDSA-P384署名を検証する。
fn verify_cert_chain(
    leaf_cert_der: &[u8],
    cabundle: &[Vec<u8>],
) -> Result<(), AttestationError> {
    // AWS Nitroルート証明書をデコード
    let root_der = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        AWS_NITRO_ROOT_CERT_B64,
    )
    .map_err(|e| AttestationError::Base64Error(e.to_string()))?;

    // 全証明書をパース: [leaf, ...intermediates, root]
    let mut chain_ders: Vec<&[u8]> = Vec::new();
    chain_ders.push(leaf_cert_der);
    for ca_cert in cabundle {
        chain_ders.push(ca_cert);
    }
    chain_ders.push(&root_der);

    // 各ペア(child, parent)の署名を検証
    for i in 0..chain_ders.len() - 1 {
        let child = x509_cert::Certificate::from_der(chain_ders[i])
            .map_err(|e| AttestationError::CertParseError(format!("child[{}]: {}", i, e)))?;
        let parent = x509_cert::Certificate::from_der(chain_ders[i + 1])
            .map_err(|e| {
                AttestationError::CertParseError(format!("parent[{}]: {}", i + 1, e))
            })?;

        verify_cert_signature(&child, &parent).map_err(|e| {
            AttestationError::CertChainError(format!(
                "証明書[{}]→[{}]の検証失敗: {}",
                i,
                i + 1,
                e
            ))
        })?;
    }

    // ルート証明書は自己署名を検証
    let root = x509_cert::Certificate::from_der(&root_der)
        .map_err(|e| AttestationError::CertParseError(format!("root: {}", e)))?;
    verify_cert_signature(&root, &root).map_err(|e| {
        AttestationError::CertChainError(format!("ルート証明書の自己署名検証失敗: {}", e))
    })?;

    Ok(())
}

/// X.509証明書の署名を親証明書の公開鍵で検証する。
fn verify_cert_signature(
    child: &x509_cert::Certificate,
    parent: &x509_cert::Certificate,
) -> Result<(), String> {
    // 親の公開鍵を抽出
    let parent_spki = &parent.tbs_certificate.subject_public_key_info;
    let parent_pubkey_bits = parent_spki.subject_public_key.raw_bytes();

    let verifying_key = p384::ecdsa::VerifyingKey::from_sec1_bytes(parent_pubkey_bits)
        .map_err(|e| format!("P-384公開鍵のパースに失敗: {}", e))?;

    // 子のTBSCertificateをDERエンコード（署名対象）
    let tbs_der = der::Encode::to_der(&child.tbs_certificate)
        .map_err(|e| format!("TBSCertificateのDERエンコードに失敗: {}", e))?;

    // 子の署名をデコード（DER形式のECDSA署名）
    let sig_bytes = child.signature.raw_bytes();
    let der_sig = p384::ecdsa::DerSignature::from_bytes(sig_bytes)
        .map_err(|e| format!("ECDSA署名のデコードに失敗: {}", e))?;

    // 検証
    verifying_key
        .verify(&tbs_der, &der_sig)
        .map_err(|e| format!("署名検証に失敗: {}", e))
}

/// COSE Sign1の署名をリーフ証明書の公開鍵で検証する。
fn verify_cose_signature(
    cose_sign1: &coset::CoseSign1,
    leaf_cert_der: &[u8],
) -> Result<(), AttestationError> {
    // リーフ証明書の公開鍵を抽出
    let leaf = x509_cert::Certificate::from_der(leaf_cert_der)
        .map_err(|e| AttestationError::CertParseError(format!("leaf: {}", e)))?;
    let leaf_pubkey_bits = leaf
        .tbs_certificate
        .subject_public_key_info
        .subject_public_key
        .raw_bytes();

    let verifying_key = p384::ecdsa::VerifyingKey::from_sec1_bytes(leaf_pubkey_bits)
        .map_err(|e| AttestationError::CertParseError(format!("P-384公開鍵: {}", e)))?;

    // Sig_structure（COSE署名対象）を構築
    let aad: Vec<u8> = Vec::new();
    let tbs_data = cose_sign1.tbs_data(&aad);

    // COSE ES384署名はraw形式（r || s、各48バイト = 96バイト）
    let signature = p384::ecdsa::Signature::from_slice(&cose_sign1.signature)
        .map_err(|_| AttestationError::SignatureVerificationFailed)?;

    verifying_key
        .verify(&tbs_data, &signature)
        .map_err(|_| AttestationError::SignatureVerificationFailed)
}

// ─────────────────────────────────────────────
// CBORフィールド抽出ヘルパー
// ─────────────────────────────────────────────

type CborMap = Vec<(ciborium::Value, ciborium::Value)>;

fn find_field<'a>(map: &'a CborMap, key: &str) -> Option<&'a ciborium::Value> {
    map.iter().find_map(|(k, v)| match k {
        ciborium::Value::Text(s) if s == key => Some(v),
        _ => None,
    })
}

fn get_text_field(map: &CborMap, key: &str) -> Result<String, AttestationError> {
    match find_field(map, key) {
        Some(ciborium::Value::Text(s)) => Ok(s.clone()),
        Some(_) => Err(AttestationError::CborParseError(format!(
            "フィールド '{}' がテキストではありません",
            key
        ))),
        None => Err(AttestationError::MissingField(key.into())),
    }
}

fn get_integer_field(map: &CborMap, key: &str) -> Result<u64, AttestationError> {
    match find_field(map, key) {
        Some(ciborium::Value::Integer(i)) => {
            let val: i128 = (*i).into();
            Ok(val as u64)
        }
        Some(_) => Err(AttestationError::CborParseError(format!(
            "フィールド '{}' が整数ではありません",
            key
        ))),
        None => Err(AttestationError::MissingField(key.into())),
    }
}

fn get_bytes_field(map: &CborMap, key: &str) -> Result<Vec<u8>, AttestationError> {
    match find_field(map, key) {
        Some(ciborium::Value::Bytes(b)) => Ok(b.clone()),
        Some(_) => Err(AttestationError::CborParseError(format!(
            "フィールド '{}' がバイト列ではありません",
            key
        ))),
        None => Err(AttestationError::MissingField(key.into())),
    }
}

fn get_optional_bytes_field(map: &CborMap, key: &str) -> Option<Vec<u8>> {
    match find_field(map, key) {
        Some(ciborium::Value::Bytes(b)) => Some(b.clone()),
        Some(ciborium::Value::Null) => None,
        _ => None,
    }
}

fn get_bytes_array_field(
    map: &CborMap,
    key: &str,
) -> Result<Vec<Vec<u8>>, AttestationError> {
    match find_field(map, key) {
        Some(ciborium::Value::Array(arr)) => {
            let mut result = Vec::new();
            for item in arr {
                match item {
                    ciborium::Value::Bytes(b) => result.push(b.clone()),
                    _ => {
                        return Err(AttestationError::CborParseError(format!(
                            "フィールド '{}' の配列要素がバイト列ではありません",
                            key
                        )))
                    }
                }
            }
            Ok(result)
        }
        Some(_) => Err(AttestationError::CborParseError(format!(
            "フィールド '{}' が配列ではありません",
            key
        ))),
        None => Err(AttestationError::MissingField(key.into())),
    }
}

fn get_pcrs_field(map: &CborMap) -> Result<BTreeMap<u32, Vec<u8>>, AttestationError> {
    match find_field(map, "pcrs") {
        Some(ciborium::Value::Map(pcr_map)) => {
            let mut result = BTreeMap::new();
            for (k, v) in pcr_map {
                let idx = match k {
                    ciborium::Value::Integer(i) => {
                        let val: i128 = (*i).into();
                        val as u32
                    }
                    _ => {
                        return Err(AttestationError::CborParseError(
                            "PCRインデックスが整数ではありません".into(),
                        ))
                    }
                };
                let val = match v {
                    ciborium::Value::Bytes(b) => b.clone(),
                    _ => {
                        return Err(AttestationError::CborParseError(
                            "PCR値がバイト列ではありません".into(),
                        ))
                    }
                };
                result.insert(idx, val);
            }
            Ok(result)
        }
        Some(_) => Err(AttestationError::CborParseError(
            "pcrsフィールドがマップではありません".into(),
        )),
        None => Err(AttestationError::MissingField("pcrs".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用のAttestation Documentを生成する。
    /// P-384鍵ペアで自己署名したCOSE Sign1ドキュメント。
    fn create_test_attestation_document(
        public_key: Option<&[u8]>,
        user_data: Option<&[u8]>,
    ) -> (Vec<u8>, p384::ecdsa::SigningKey) {
        use p384::ecdsa::SigningKey;

        // P-384署名用鍵ペアを生成
        let signing_key = SigningKey::random(&mut rand::rngs::OsRng);

        // 自己署名証明書を作成（テスト用簡易版）
        let cert_der = create_self_signed_cert(&signing_key);

        // CBORペイロードを構築
        let mut pcrs = Vec::new();
        for i in 0..3u32 {
            pcrs.push((
                ciborium::Value::Integer(i.into()),
                ciborium::Value::Bytes(vec![0u8; 48]),
            ));
        }

        let mut payload_map: Vec<(ciborium::Value, ciborium::Value)> = vec![
            (
                ciborium::Value::Text("module_id".into()),
                ciborium::Value::Text("test-enclave".into()),
            ),
            (
                ciborium::Value::Text("digest".into()),
                ciborium::Value::Text("SHA384".into()),
            ),
            (
                ciborium::Value::Text("timestamp".into()),
                ciborium::Value::Integer(1700000000u64.into()),
            ),
            (
                ciborium::Value::Text("pcrs".into()),
                ciborium::Value::Map(pcrs),
            ),
            (
                ciborium::Value::Text("certificate".into()),
                ciborium::Value::Bytes(cert_der),
            ),
            (
                ciborium::Value::Text("cabundle".into()),
                ciborium::Value::Array(vec![]),
            ),
        ];

        if let Some(pk) = public_key {
            payload_map.push((
                ciborium::Value::Text("public_key".into()),
                ciborium::Value::Bytes(pk.to_vec()),
            ));
        } else {
            payload_map.push((
                ciborium::Value::Text("public_key".into()),
                ciborium::Value::Null,
            ));
        }

        if let Some(ud) = user_data {
            payload_map.push((
                ciborium::Value::Text("user_data".into()),
                ciborium::Value::Bytes(ud.to_vec()),
            ));
        } else {
            payload_map.push((
                ciborium::Value::Text("user_data".into()),
                ciborium::Value::Null,
            ));
        }

        payload_map.push((
            ciborium::Value::Text("nonce".into()),
            ciborium::Value::Null,
        ));

        let payload_value = ciborium::Value::Map(payload_map);
        let mut payload_bytes = Vec::new();
        ciborium::into_writer(&payload_value, &mut payload_bytes).unwrap();

        // COSE Sign1を構築
        let mut cose_sign1 = coset::CoseSign1Builder::new()
            .protected(
                coset::HeaderBuilder::new()
                    .algorithm(coset::iana::Algorithm::ES384)
                    .build(),
            )
            .payload(payload_bytes)
            .build();

        // 署名
        let tbs = cose_sign1.tbs_data(&[]);
        use p384::ecdsa::signature::Signer;
        let sig: p384::ecdsa::Signature = signing_key.sign(&tbs);
        cose_sign1.signature = sig.to_bytes().to_vec();

        let doc_bytes = cose_sign1.to_vec().unwrap();
        (doc_bytes, signing_key)
    }

    /// テスト用の自己署名P-384証明書を作成する。
    fn create_self_signed_cert(signing_key: &p384::ecdsa::SigningKey) -> Vec<u8> {
        use der::Encode;

        let verifying_key = signing_key.verifying_key();
        let pubkey_sec1 = verifying_key.to_sec1_bytes();

        // SubjectPublicKeyInfo for P-384
        let spki_oid =
            der::asn1::ObjectIdentifier::new_unwrap("1.2.840.10045.2.1"); // id-ecPublicKey
        let curve_oid =
            der::asn1::ObjectIdentifier::new_unwrap("1.3.132.0.34"); // secp384r1

        let algorithm = x509_cert::spki::AlgorithmIdentifierOwned {
            oid: spki_oid,
            parameters: Some(der::asn1::Any::from(&curve_oid)),
        };

        let subject_public_key =
            der::asn1::BitString::from_bytes(&pubkey_sec1).unwrap();

        let spki = x509_cert::spki::SubjectPublicKeyInfoOwned {
            algorithm,
            subject_public_key,
        };

        // Minimal TBSCertificate
        let serial =
            x509_cert::serial_number::SerialNumber::new(&[1]).unwrap();
        let sig_alg_oid =
            der::asn1::ObjectIdentifier::new_unwrap("1.2.840.10045.4.3.3"); // ecdsa-with-SHA384
        let sig_alg = x509_cert::spki::AlgorithmIdentifierOwned {
            oid: sig_alg_oid,
            parameters: None,
        };

        let name = x509_cert::name::Name::default();

        let not_before = x509_cert::time::Time::GeneralTime(
            der::asn1::GeneralizedTime::from_date_time(
                der::DateTime::new(2020, 1, 1, 0, 0, 0).unwrap(),
            ),
        );
        let not_after = x509_cert::time::Time::GeneralTime(
            der::asn1::GeneralizedTime::from_date_time(
                der::DateTime::new(2049, 12, 31, 23, 59, 59).unwrap(),
            ),
        );
        let validity = x509_cert::time::Validity {
            not_before,
            not_after,
        };

        let tbs = x509_cert::TbsCertificate {
            version: x509_cert::certificate::Version::V3,
            serial_number: serial,
            signature: sig_alg.clone(),
            issuer: name.clone(),
            validity,
            subject: name,
            subject_public_key_info: spki,
            issuer_unique_id: None,
            subject_unique_id: None,
            extensions: None,
        };

        // Sign TBS
        let tbs_der = tbs.to_der().unwrap();
        use p384::ecdsa::signature::Signer;
        let sig: p384::ecdsa::DerSignature = signing_key.sign(&tbs_der);
        let sig_bits =
            der::asn1::BitString::from_bytes(sig.as_bytes()).unwrap();

        let cert = x509_cert::Certificate {
            tbs_certificate: tbs,
            signature_algorithm: sig_alg,
            signature: sig_bits,
        };

        cert.to_der().unwrap()
    }

    /// CBOR形式のAttestation Documentペイロードをパースできることを確認
    #[test]
    fn test_parse_attestation_payload() {
        let test_pubkey = vec![1u8; 32];
        let test_userdata = vec![2u8; 32];
        let (doc, _) =
            create_test_attestation_document(Some(&test_pubkey), Some(&test_userdata));

        let result = parse_attestation_payload(&doc).unwrap();

        assert_eq!(result.module_id, "test-enclave");
        assert_eq!(result.digest, "SHA384");
        assert_eq!(result.timestamp, 1700000000);
        assert_eq!(result.pcrs.len(), 3);
        assert_eq!(result.pcrs[&0], vec![0u8; 48]);
        assert_eq!(result.public_key, Some(test_pubkey));
        assert_eq!(result.user_data, Some(test_userdata));
        assert_eq!(result.nonce, None);
    }

    /// 自己署名証明書付きAttestation Documentの署名検証
    #[test]
    fn test_verify_cose_signature_self_signed() {
        let (doc, _) = create_test_attestation_document(Some(&[1u8; 32]), None);

        let cose_sign1 = coset::CoseSign1::from_slice(&doc).unwrap();
        let payload_bytes = cose_sign1.payload.as_ref().unwrap();
        let result = extract_attestation_fields(payload_bytes).unwrap();

        let verify_result = verify_cose_signature(&cose_sign1, &result.certificate);
        assert!(
            verify_result.is_ok(),
            "COSE署名検証に失敗: {:?}",
            verify_result.err()
        );
    }

    /// PCR値の照合テスト
    #[test]
    fn test_verify_pcr_values() {
        let (doc, _) = create_test_attestation_document(None, None);
        let result = parse_attestation_payload(&doc).unwrap();

        // 全ゼロのPCR値と一致するはず
        let mut expected = BTreeMap::new();
        expected.insert(0, vec![0u8; 48]);
        expected.insert(1, vec![0u8; 48]);
        expected.insert(2, vec![0u8; 48]);
        assert!(verify_pcr_values(&result, &expected));

        // 異なるPCR値では不一致
        let mut wrong = BTreeMap::new();
        wrong.insert(0, vec![1u8; 48]);
        assert!(!verify_pcr_values(&result, &wrong));
    }

    /// 公開鍵の照合テスト
    #[test]
    fn test_verify_public_key() {
        let test_pubkey = vec![42u8; 32];
        let (doc, _) = create_test_attestation_document(Some(&test_pubkey), None);
        let result = parse_attestation_payload(&doc).unwrap();

        assert!(verify_public_key(&result, &test_pubkey));
        assert!(!verify_public_key(&result, &[0u8; 32]));
    }

    /// X.509証明書の自己署名検証テスト
    #[test]
    fn test_self_signed_cert_verification() {
        let signing_key = p384::ecdsa::SigningKey::random(&mut rand::rngs::OsRng);
        let cert_der = create_self_signed_cert(&signing_key);

        let cert = x509_cert::Certificate::from_der(&cert_der).unwrap();
        let result = verify_cert_signature(&cert, &cert);
        assert!(result.is_ok(), "自己署名検証失敗: {:?}", result.err());
    }

    /// AWS Nitroルート証明書のBase64デコードテスト
    #[test]
    fn test_aws_root_cert_decode() {
        let root_der = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            AWS_NITRO_ROOT_CERT_B64,
        )
        .expect("AWS Nitroルート証明書のBase64デコードに失敗");

        // DERパースが成功すること
        let cert = x509_cert::Certificate::from_der(&root_der)
            .expect("AWS Nitroルート証明書のパースに失敗");

        // 自己署名であること
        let result = verify_cert_signature(&cert, &cert);
        assert!(
            result.is_ok(),
            "AWS Nitroルート証明書の自己署名検証失敗: {:?}",
            result.err()
        );
    }

    /// NitroAttestationResult → AttestationResult 変換テスト
    #[test]
    fn test_convert_to_common_result() {
        let (doc, _) = create_test_attestation_document(Some(&[1u8; 32]), Some(&[2u8; 32]));
        let nitro_result = parse_attestation_payload(&doc).unwrap();

        let common: super::super::AttestationResult = nitro_result.into();

        assert_eq!(common.tee_type, "aws_nitro");
        assert_eq!(common.measurements.len(), 3);
        assert_eq!(common.measurements["PCR0"], vec![0u8; 48]);
        assert_eq!(common.measurements["PCR1"], vec![0u8; 48]);
        assert_eq!(common.measurements["PCR2"], vec![0u8; 48]);
        assert_eq!(common.public_key, Some(vec![1u8; 32]));
        assert_eq!(common.user_data, Some(vec![2u8; 32]));
        assert_eq!(common.timestamp, Some(1700000000));
    }
}
