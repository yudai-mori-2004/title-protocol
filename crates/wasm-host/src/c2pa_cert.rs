// SPDX-License-Identifier: Apache-2.0

//! # C2PA アクティブマニフェスト証明書チェーン検証
//!
//! コンテンツ内のC2PAアクティブマニフェストの署名証明書チェーンを
//! 信頼されたRoot CAの公開鍵に対して暗号的に検証する。
//!
//! ## 検証フロー
//! 1. JPEG APP11からJUMBFデータを抽出
//! 2. JUMBFから最後のマニフェスト（=アクティブマニフェスト）のCOSE_Sign1を抽出
//! 3. COSE_Sign1のprotectedヘッダからx5chain（証明書チェーン）を抽出
//! 4. 証明書チェーンを暗号的に検証（各証明書の署名を親の公開鍵で検証）
//! 5. チェーン末端の証明書が指定されたRoot CAの公開鍵で署名されていることを確認

use der::Decode;

// ---------------------------------------------------------------------------
// JPEG APP11 → JUMBF 抽出
// ---------------------------------------------------------------------------

/// JPEG APP11マーカーからJUMBFデータを抽出する。
///
/// C2PA JUMBF はJPEGのAPP11 (0xFFEB) セグメントに埋め込まれている。
/// 各セグメント先頭の8バイト（JP magic 4B + sequence number 4B）をスキップして
/// JUMBFペイロードを返す。
pub fn extract_jumbf_from_jpeg(data: &[u8]) -> Option<Vec<u8>> {
    let mut jumbf = Vec::new();
    let mut i = 0;

    while i + 4 < data.len() {
        if data[i] == 0xFF && data[i + 1] == 0xEB {
            let seg_len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
            let seg_start = i + 4;
            let seg_end = (i + 2 + seg_len).min(data.len());
            if seg_end <= seg_start + 8 {
                i = seg_end;
                continue;
            }
            let body = &data[seg_start..seg_end];

            // JP magic: 0x4A50 0x00 0x01
            if body.len() >= 8 && body[0] == 0x4A && body[1] == 0x50 {
                // Skip 8-byte CI header (JP magic 4B + sequence 4B)
                jumbf.extend_from_slice(&body[8..]);
            }
            i = seg_end;
        } else {
            i += 1;
        }
    }

    if jumbf.is_empty() {
        None
    } else {
        Some(jumbf)
    }
}

// ---------------------------------------------------------------------------
// JUMBF → 最後のCOSE_Sign1 CBOR 抽出
// ---------------------------------------------------------------------------

/// c2pa.signature の UUID（16バイト）
const CAI_SIGNATURE_UUID: [u8; 16] = [
    0x63, 0x32, 0x63, 0x73, 0x00, 0x11, 0x00, 0x10, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B,
    0x71,
];

/// JUMBF ボックスタイプ定数
const BOX_JUMB: u32 = 0x6A75_6D62; // 'jumb'
const BOX_JUMD: u32 = 0x6A75_6D64; // 'jumd'
const BOX_CBOR: u32 = 0x6362_6F72; // 'cbor'

/// ボックスヘッダを読み取る（size, type）。
/// 返り値: (box_size, box_type, header_size)
fn read_box_header(data: &[u8], offset: usize) -> Option<(u64, u32, usize)> {
    if offset + 8 > data.len() {
        return None;
    }
    let size = u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
    let box_type = u32::from_be_bytes([data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7]]);

    if size == 1 {
        // Extended size
        if offset + 16 > data.len() {
            return None;
        }
        let large = u64::from_be_bytes([
            data[offset + 8], data[offset + 9], data[offset + 10], data[offset + 11],
            data[offset + 12], data[offset + 13], data[offset + 14], data[offset + 15],
        ]);
        Some((large, box_type, 16))
    } else if size == 0 {
        None // box extends to end, not handled
    } else {
        Some((size as u64, box_type, 8))
    }
}

/// JUMBFデータからアクティブマニフェスト（最後のマニフェスト）の
/// COSE_Sign1 CBORバイト列を抽出する。
pub fn find_active_cose_sign1(jumbf: &[u8]) -> Option<Vec<u8>> {
    // トップレベルsuperbox
    let (top_size, top_type, top_hdr) = read_box_header(jumbf, 0)?;
    if top_type != BOX_JUMB {
        return None;
    }
    let top_end = (top_size as usize).min(jumbf.len());

    // トップレベルのdescription boxをスキップ
    let (desc_size, desc_type, _) = read_box_header(jumbf, top_hdr)?;
    if desc_type != BOX_JUMD {
        return None;
    }

    let mut pos = top_hdr + desc_size as usize;
    let mut last_sig: Option<Vec<u8>> = None;

    // 各マニフェストsuperboxをスキャン
    while pos < top_end {
        let (child_size, child_type, child_hdr) = match read_box_header(jumbf, pos) {
            Some(v) => v,
            None => break,
        };
        if child_size == 0 {
            break;
        }
        let child_end = (pos + child_size as usize).min(top_end);

        if child_type == BOX_JUMB {
            // このマニフェスト内でc2pa.signatureを検索
            if let Some(sig) = find_signature_in_manifest(jumbf, pos + child_hdr, child_end) {
                last_sig = Some(sig);
            }
        }

        pos = child_end;
    }

    last_sig
}

/// マニフェストsuperbox内からc2pa.signature boxのCBORデータを探す。
fn find_signature_in_manifest(jumbf: &[u8], start: usize, end: usize) -> Option<Vec<u8>> {
    let mut pos = start;

    // まずdescription boxをスキップ
    if let Some((desc_size, desc_type, _)) = read_box_header(jumbf, pos) {
        if desc_type == BOX_JUMD {
            pos += desc_size as usize;
        }
    }

    while pos < end {
        let (box_size, box_type, box_hdr) = match read_box_header(jumbf, pos) {
            Some(v) => v,
            None => break,
        };
        if box_size == 0 {
            break;
        }
        let box_end = (pos + box_size as usize).min(end);

        if box_type == BOX_JUMB {
            // Description boxを読んでUUIDを確認
            if let Some((d_size, d_type, _)) = read_box_header(jumbf, pos + box_hdr) {
                if d_type == BOX_JUMD && pos + box_hdr + 8 + 16 <= jumbf.len() {
                    let uuid_start = pos + box_hdr + 8; // skip desc header
                    let uuid = &jumbf[uuid_start..uuid_start + 16];
                    if uuid == CAI_SIGNATURE_UUID {
                        // CBOR boxを探す
                        let after_desc = pos + box_hdr + d_size as usize;
                        return find_cbor_box(jumbf, after_desc, box_end);
                    }
                }
            }
        }

        pos = box_end;
    }

    None
}

/// superbox内から最初のCBOR boxのデータを抽出する。
fn find_cbor_box(jumbf: &[u8], start: usize, end: usize) -> Option<Vec<u8>> {
    let mut pos = start;
    while pos < end {
        let (box_size, box_type, box_hdr) = match read_box_header(jumbf, pos) {
            Some(v) => v,
            None => break,
        };
        if box_size == 0 {
            break;
        }

        if box_type == BOX_CBOR {
            let data_start = pos + box_hdr;
            let data_end = (pos + box_size as usize).min(end);
            if data_start < data_end {
                return Some(jumbf[data_start..data_end].to_vec());
            }
        }

        pos = (pos + box_size as usize).min(end);
    }
    None
}

// ---------------------------------------------------------------------------
// COSE_Sign1 → x5chain 抽出
// ---------------------------------------------------------------------------

/// COSE_Sign1 CBORからx5chain（DER証明書チェーン）を抽出する。
///
/// COSE_Sign1はCBOR配列: [protected(bstr), unprotected(map), payload, signature]
/// protectedヘッダはCBORエンコードされたmap。label 33 = x5chain。
pub fn extract_x5chain(cose_cbor: &[u8]) -> Result<Vec<Vec<u8>>, &'static str> {
    // COSE_Sign1: tag(18) + array(4)
    let value: ciborium::Value =
        ciborium::from_reader(cose_cbor).map_err(|_| "COSE_Sign1のCBORパースに失敗")?;

    // tag 18 (COSE_Sign1) をアンラップ
    let array = match &value {
        ciborium::Value::Tag(18, inner) => match inner.as_ref() {
            ciborium::Value::Array(a) => a,
            _ => return Err("COSE_Sign1がCBOR配列ではありません"),
        },
        ciborium::Value::Array(a) => a,
        _ => return Err("COSE_Sign1がCBOR配列ではありません"),
    };

    if array.len() < 4 {
        return Err("COSE_Sign1の要素数が不足しています");
    }

    // protected header (bstr)
    let protected_bytes = match &array[0] {
        ciborium::Value::Bytes(b) => b,
        _ => return Err("protectedヘッダがbstrではありません"),
    };

    // protectedヘッダをCBORマップとしてパース
    let protected_map: ciborium::Value =
        ciborium::from_reader(protected_bytes.as_slice())
            .map_err(|_| "protectedヘッダのCBORパースに失敗")?;

    let map = match &protected_map {
        ciborium::Value::Map(m) => m,
        _ => return Err("protectedヘッダがCBORマップではありません"),
    };

    // label 33 (x5chain) を探す
    for (key, value) in map {
        let is_33 = match key {
            ciborium::Value::Integer(i) => {
                let n: i128 = (*i).into();
                n == 33
            }
            _ => false,
        };
        if !is_33 {
            continue;
        }

        // x5chain: 単一cert (Bytes) または 配列 (Array of Bytes)
        return match value {
            ciborium::Value::Bytes(b) => Ok(vec![b.clone()]),
            ciborium::Value::Array(arr) => {
                let mut certs = Vec::new();
                for item in arr {
                    match item {
                        ciborium::Value::Bytes(b) => certs.push(b.clone()),
                        _ => return Err("x5chainの要素がbstrではありません"),
                    }
                }
                Ok(certs)
            }
            _ => Err("x5chainがbstrまたは配列ではありません"),
        };
    }

    Err("x5chain (label 33) がprotectedヘッダに見つかりません")
}

// ---------------------------------------------------------------------------
// X.509 証明書チェーン検証
// ---------------------------------------------------------------------------

/// OID: ecdsa-with-SHA256 (1.2.840.10045.4.3.2)
const OID_ECDSA_SHA256: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x02];
/// OID: ecdsa-with-SHA384 (1.2.840.10045.4.3.3)
const OID_ECDSA_SHA384: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x03];

/// DER証明書から署名検証に必要な情報を抽出する。
struct CertInfo {
    /// tbsCertificate の DER バイト列（署名対象）
    tbs_bytes: Vec<u8>,
    /// 署名アルゴリズムOIDのバイト列
    sig_alg_oid: Vec<u8>,
    /// 署名値のバイト列（DERエンコードされたECDSA署名）
    signature: Vec<u8>,
    /// SubjectPublicKeyInfo の DER バイト列
    spki_der: Vec<u8>,
}

/// DER証明書をパースして検証に必要な情報を抽出する。
fn parse_cert(cert_der: &[u8]) -> Result<CertInfo, &'static str> {
    let cert = x509_cert::Certificate::from_der(cert_der)
        .map_err(|_| "X.509証明書のDERパースに失敗")?;

    // tbsCertificate のDERバイト列
    let tbs_bytes = der::Encode::to_der(&cert.tbs_certificate)
        .map_err(|_| "tbsCertificateのDERエンコードに失敗")?;

    // 署名アルゴリズムOID
    let sig_alg_oid = cert.signature_algorithm.oid.as_bytes().to_vec();

    // 署名値（BitStringから取得）
    let signature = cert.signature.raw_bytes().to_vec();

    // SubjectPublicKeyInfo のDER
    let spki_der = der::Encode::to_der(&cert.tbs_certificate.subject_public_key_info)
        .map_err(|_| "SPKIのDERエンコードに失敗")?;

    Ok(CertInfo {
        tbs_bytes,
        sig_alg_oid,
        signature,
        spki_der,
    })
}

/// SPKI DERバイト列とアルゴリズムOIDを使って署名を検証する。
fn verify_signature(
    spki_der: &[u8],
    sig_alg_oid: &[u8],
    tbs_bytes: &[u8],
    signature: &[u8],
) -> Result<bool, &'static str> {
    use ecdsa::signature::Verifier;

    if sig_alg_oid == OID_ECDSA_SHA384 {
        // P-384 + SHA-384
        let spki = x509_cert::spki::SubjectPublicKeyInfoOwned::from_der(spki_der)
            .map_err(|_| "親SPKIのDERパースに失敗")?;
        let key_bytes = spki.subject_public_key.raw_bytes();
        let vk = p384::ecdsa::VerifyingKey::from_sec1_bytes(key_bytes)
            .map_err(|_| "P-384公開鍵の構築に失敗")?;
        let sig = p384::ecdsa::Signature::from_der(signature)
            .map_err(|_| "P-384 DER署名のパースに失敗")?;
        // Verifier::verify はメッセージを内部でSHA-384ハッシュする
        Ok(vk.verify(tbs_bytes, &sig).is_ok())
    } else if sig_alg_oid == OID_ECDSA_SHA256 {
        // P-256 + SHA-256
        let spki = x509_cert::spki::SubjectPublicKeyInfoOwned::from_der(spki_der)
            .map_err(|_| "親SPKIのDERパースに失敗")?;
        let key_bytes = spki.subject_public_key.raw_bytes();
        let vk = p256::ecdsa::VerifyingKey::from_sec1_bytes(key_bytes)
            .map_err(|_| "P-256公開鍵の構築に失敗")?;
        let sig = p256::ecdsa::Signature::from_der(signature)
            .map_err(|_| "P-256 DER署名のパースに失敗")?;
        // Verifier::verify はメッセージを内部でSHA-256ハッシュする
        Ok(vk.verify(tbs_bytes, &sig).is_ok())
    } else {
        Err("未対応の署名アルゴリズム")
    }
}

/// 証明書チェーンを検証する。
///
/// - `certs_der`: DER証明書の配列（leaf first）
/// - `root_spki_der`: Root CAのSubjectPublicKeyInfo DERバイト列
///
/// cert[0]をcert[1]の公開鍵で検証、cert[1]をroot_spkiで検証、のように連鎖検証する。
pub fn verify_cert_chain(
    certs_der: &[Vec<u8>],
    root_spki_der: &[u8],
) -> Result<bool, &'static str> {
    if certs_der.is_empty() {
        return Err("証明書チェーンが空です");
    }

    let mut infos: Vec<CertInfo> = Vec::new();
    for cert in certs_der {
        infos.push(parse_cert(cert)?);
    }

    // cert[i] の署名を cert[i+1] の公開鍵で検証
    for i in 0..infos.len() - 1 {
        let child = &infos[i];
        let parent = &infos[i + 1];

        if !verify_signature(&parent.spki_der, &child.sig_alg_oid, &child.tbs_bytes, &child.signature)? {
            return Ok(false);
        }
    }

    // 最後の証明書をRoot CAの公開鍵で検証
    let last = &infos[infos.len() - 1];
    verify_signature(root_spki_der, &last.sig_alg_oid, &last.tbs_bytes, &last.signature)
}

// ---------------------------------------------------------------------------
// エントリポイント
// ---------------------------------------------------------------------------

/// コンテンツ内のC2PAアクティブマニフェストの証明書チェーンを
/// 指定されたRoot CAの公開鍵に対して検証する。
///
/// # 引数
/// * `content` - コンテンツの生バイト列（JPEG等）
/// * `root_spki_hex` - Root CAのSubjectPublicKeyInfo DERのhex文字列
///
/// # 戻り値
/// * `Ok(true)` - チェーン検証成功（信頼されたRoot CAに連なる）
/// * `Ok(false)` - チェーン検証失敗
/// * `Err` - 構造エラー（C2PAデータなし等）
pub fn verify_active_cert_chain(content: &[u8], root_spki_hex: &str) -> Result<bool, String> {
    let root_spki = hex::decode(root_spki_hex)
        .map_err(|e| format!("root_spki_hexのデコードに失敗: {e}"))?;

    let jumbf = extract_jumbf_from_jpeg(content)
        .ok_or_else(|| "JUMBFデータが見つかりません".to_string())?;

    let cose_cbor = find_active_cose_sign1(&jumbf)
        .ok_or_else(|| "アクティブマニフェストのCOSE_Sign1が見つかりません".to_string())?;

    let certs = extract_x5chain(&cose_cbor)
        .map_err(|e| format!("x5chainの抽出に失敗: {e}"))?;

    verify_cert_chain(&certs, &root_spki)
        .map_err(|e| format!("証明書チェーン検証エラー: {e}"))
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Google C2PA Root CA G3 の SubjectPublicKeyInfo (DER, hex)
    /// http://pki.goog/c2pa/root-g3.crt から取得
    const GOOGLE_ROOT_SPKI_HEX: &str =
        "3076301006072a8648ce3d020106052b81040022036200\
         0486ff5ffe3b8a70fa5edc59bb78021232e4b24beb41c6\
         7d1a6070bcdc9faa02c15644418df69e8f37f381a28b8f\
         ce9385471beb956a16980237a75957c8f8381377a0ed23\
         42860a29508a62846bbaaa584ff2b2d77f7a7c6e123915\
         343631a176";

    #[test]
    fn test_extract_jumbf_from_plane() {
        let data = std::fs::read("../../integration-tests/fixtures/pixel_photo_plane.jpg").unwrap();
        let jumbf = extract_jumbf_from_jpeg(&data).expect("JUMBF should be found");
        assert!(jumbf.len() > 1000);
        // Top box type should be 'jumb'
        assert_eq!(&jumbf[4..8], b"jumb");
    }

    #[test]
    fn test_extract_cose_from_plane() {
        let data = std::fs::read("../../integration-tests/fixtures/pixel_photo_plane.jpg").unwrap();
        let jumbf = extract_jumbf_from_jpeg(&data).unwrap();
        let cose = find_active_cose_sign1(&jumbf).expect("COSE_Sign1 should be found");
        assert!(cose.len() > 100);
        // Should start with CBOR tag 18 (0xD2)
        assert_eq!(cose[0], 0xD2);
    }

    #[test]
    fn test_extract_x5chain_from_plane() {
        let data = std::fs::read("../../integration-tests/fixtures/pixel_photo_plane.jpg").unwrap();
        let jumbf = extract_jumbf_from_jpeg(&data).unwrap();
        let cose = find_active_cose_sign1(&jumbf).unwrap();
        let certs = extract_x5chain(&cose).expect("x5chain should be extracted");
        assert_eq!(certs.len(), 2, "Should have 2 certs (leaf + intermediate)");
    }

    #[test]
    fn test_verify_plane_against_google_root() {
        let data = std::fs::read("../../integration-tests/fixtures/pixel_photo_plane.jpg").unwrap();
        let result = verify_active_cert_chain(&data, GOOGLE_ROOT_SPKI_HEX);
        assert!(result.is_ok(), "Verification should not error: {:?}", result.err());
        assert!(result.unwrap(), "plane.jpg should verify against Google Root CA");
    }

    #[test]
    fn test_verify_ramen_against_google_root() {
        let data = std::fs::read("../../integration-tests/fixtures/pixel_photo_ramen.jpg").unwrap();
        let result = verify_active_cert_chain(&data, GOOGLE_ROOT_SPKI_HEX);
        assert!(result.is_ok(), "Verification should not error: {:?}", result.err());
        assert!(result.unwrap(), "ramen.jpg should verify against Google Root CA");
    }

    #[test]
    fn test_verify_plane_against_wrong_root() {
        let data = std::fs::read("../../integration-tests/fixtures/pixel_photo_plane.jpg").unwrap();
        // Use a fake root SPKI (valid P-384 SPKI format but wrong key)
        let fake_root = "3076301006072a8648ce3d020106052b8104002203620004\
                         0000000000000000000000000000000000000000000000000\
                         0000000000000000000000000000000000000000000000000\
                         0000000000000000000000000000000000000000000000000\
                         000000000000000000000000000000000000000000000";
        let result = verify_active_cert_chain(&data, fake_root);
        // Should either return Ok(false) or Err (key construction failure)
        match result {
            Ok(false) => {} // expected
            Err(_) => {}    // also acceptable (invalid key)
            Ok(true) => panic!("Should NOT verify against wrong root"),
        }
    }

    #[test]
    fn test_no_jumbf() {
        let data = vec![0xFF, 0xD8, 0xFF, 0xD9]; // minimal JPEG
        let result = verify_active_cert_chain(&data, GOOGLE_ROOT_SPKI_HEX);
        assert!(result.is_err());
    }
}
