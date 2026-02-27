// SPDX-License-Identifier: Apache-2.0

//! # TSAタイムスタンプ抽出
//!
//! 仕様書 §2.4 重複の解決
//!
//! C2PA COSE署名のunprotected headersからRFC 3161タイムスタンプトークンを抽出し、
//! TSAが証明した時刻（`gen_time`）を取得する。
//!
//! ## 処理フロー
//! 1. COSE_Sign1をパースし、`sigTst`/`sigTst2`ヘッダを検索
//! 2. TstContainer CBORをデシリアライズ → 生DERトークンバイト取得
//! 3. CMS ContentInfo → SignedData → EncapsulatedContentInfo → TstInfo を辿り
//!    `gen_time`（GeneralizedTime）を抽出
//!
//! ## TSA vs 自己申告時刻
//! `sigTst`/`sigTst2`ヘッダが存在する場合のみTSA証明済み時刻として扱う。
//! ヘッダが存在しない場合、TSAタイムスタンプは `None` となり、
//! `resolve_duplicate()` はSolana block timeをフォールバックとして使用する。

use crate::CoreError;
use coset::{CborSerializable, TaggedCborSerializable};
use der::{Decode, Header, Reader, SliceReader};
use sha2::{Digest, Sha256};

/// TSAタイムスタンプ抽出結果。
/// 仕様書 §2.4
#[derive(Debug, Clone)]
pub struct TsaInfo {
    /// TSA証明済みタイムスタンプ（Unix epoch秒）
    pub timestamp: u64,
    /// TSA証明書のSHA-256ハッシュ（hex文字列）。
    /// 信頼リストとの照合に使用する。
    pub cert_hash: Option<String>,
    /// 生のRFC 3161トークンバイト（将来の独立検証用）
    pub raw_token: Vec<u8>,
}

/// COSE署名バイト列からTSAタイムスタンプを抽出する。
/// 仕様書 §2.4
///
/// COSE_Sign1のunprotected headersから`sigTst2`（優先）または`sigTst`を検索し、
/// RFC 3161トークンからTSA証明済み時刻を取得する。
///
/// ヘッダが存在しない場合は `Ok(None)` を返す（TSAなし）。
pub fn extract_tsa_from_cose(cose_bytes: &[u8]) -> Result<Option<TsaInfo>, CoreError> {
    // COSE_Sign1をデシリアライズ（タグ付き/なし両方に対応）
    let sign1: coset::CoseSign1 =
        coset::CoseSign1::from_tagged_slice(cose_bytes)
            .or_else(|_| coset::CoseSign1::from_slice(cose_bytes))
            .map_err(|e| {
                CoreError::ContentHashExtractionFailed(format!("COSE_Sign1パースエラー: {e}"))
            })?;

    // unprotected headersから sigTst2（優先）→ sigTst（フォールバック）を検索
    let cbor_value = find_header_by_text(&sign1.unprotected.rest, "sigTst2")
        .or_else(|| find_header_by_text(&sign1.unprotected.rest, "sigTst"));

    let cbor_value = match cbor_value {
        Some(v) => v,
        None => return Ok(None),
    };

    // CBOR Value → TstContainer としてデシリアライズ
    let mut cbor_bytes: Vec<u8> = Vec::new();
    ciborium::into_writer::<ciborium::Value, _>(&cbor_value, &mut cbor_bytes).map_err(|e| {
        CoreError::ContentHashExtractionFailed(format!("TstContainer CBORシリアライズエラー: {e}"))
    })?;

    let container: TstContainer = ciborium::from_reader(cbor_bytes.as_slice()).map_err(|e| {
        CoreError::ContentHashExtractionFailed(format!("TstContainer CBORパースエラー: {e}"))
    })?;

    let token = container.tst_tokens.first().ok_or_else(|| {
        CoreError::ContentHashExtractionFailed("TstContainerにトークンがありません".to_string())
    })?;

    let raw_token = token.val.clone();
    let (timestamp, cert_hash) = parse_tst_token_der(&raw_token)?;

    Ok(Some(TsaInfo {
        timestamp,
        cert_hash,
        raw_token,
    }))
}

/// COSE unprotected headers のrestフィールドからテキストラベルで検索する。
fn find_header_by_text(
    rest: &[(coset::Label, ciborium::Value)],
    name: &str,
) -> Option<ciborium::Value> {
    rest.iter().find_map(|(label, value)| match label {
        coset::Label::Text(text) if text == name => Some(value.clone()),
        _ => None,
    })
}

// ---------------------------------------------------------------------------
// CBOR構造体（c2pa-crypto内部と同一構造）
// ---------------------------------------------------------------------------

/// C2PA sigTst/sigTst2 ヘッダのCBOR構造。
#[derive(serde::Deserialize)]
struct TstContainer {
    #[serde(rename = "tstTokens")]
    tst_tokens: Vec<TstToken>,
}

/// TSAトークン。`val`フィールドにDERエンコードされたRFC 3161トークンを格納。
#[derive(serde::Deserialize)]
struct TstToken {
    #[serde(with = "serde_bytes")]
    val: Vec<u8>,
}

// ---------------------------------------------------------------------------
// DER解析（der クレート使用）
// ---------------------------------------------------------------------------

/// DERエンコードされたRFC 3161 TimeStampToken（CMS ContentInfo）から
/// TstInfo.gen_timeとTSA証明書ハッシュを抽出する。
///
/// フラット方式: `Header::decode`でCONSTRUCTED型のTag+Lengthを消費して
/// Valueの先頭に進め、`AnyRef::decode`で不要フィールドのTLV全体をスキップする。
/// DERは自己記述的なため、リニアに読み進めるだけでネスト構造を辿れる。
///
/// 構造:
/// ```text
/// ContentInfo ::= SEQUENCE {
///   contentType  OBJECT IDENTIFIER (id-signedData),
///   content      [0] EXPLICIT SignedData
/// }
/// SignedData ::= SEQUENCE {
///   version          INTEGER,
///   digestAlgorithms SET,
///   encapContentInfo EncapsulatedContentInfo,
///   certificates     [0] IMPLICIT SET OF Certificate (OPTIONAL),
///   ...
/// }
/// EncapsulatedContentInfo ::= SEQUENCE {
///   eContentType  OBJECT IDENTIFIER (id-ct-TSTInfo),
///   eContent      [0] EXPLICIT OCTET STRING (DER-encoded TstInfo)
/// }
/// TstInfo ::= SEQUENCE {
///   version         INTEGER,
///   policy          OBJECT IDENTIFIER,
///   messageImprint  SEQUENCE,
///   serialNumber    INTEGER,
///   genTime         GeneralizedTime,  ← 抽出対象
///   ...
/// }
/// ```
fn parse_tst_token_der(token_der: &[u8]) -> Result<(u64, Option<String>), CoreError> {
    let mut r = SliceReader::new(token_der).map_err(map_der)?;

    // ContentInfo SEQUENCE に入る
    Header::decode(&mut r).map_err(map_der)?;
    // contentType OID（スキップ）
    der::asn1::AnyRef::decode(&mut r).map_err(map_der)?;
    // content [0] EXPLICIT に入る
    Header::decode(&mut r).map_err(map_der)?;
    // SignedData SEQUENCE に入る
    Header::decode(&mut r).map_err(map_der)?;
    // version INTEGER（スキップ）
    der::asn1::AnyRef::decode(&mut r).map_err(map_der)?;
    // digestAlgorithms SET（スキップ）
    der::asn1::AnyRef::decode(&mut r).map_err(map_der)?;
    // encapContentInfo SEQUENCE に入る
    Header::decode(&mut r).map_err(map_der)?;
    // eContentType OID（スキップ）
    der::asn1::AnyRef::decode(&mut r).map_err(map_der)?;
    // eContent [0] EXPLICIT に入る
    Header::decode(&mut r).map_err(map_der)?;
    // OCTET STRING (TstInfo DERバイト列)
    let octet = der::asn1::OctetStringRef::decode(&mut r).map_err(map_der)?;
    let tst_info_bytes = octet.as_bytes();

    // TstInfoからgen_time抽出
    let timestamp = parse_tst_info(tst_info_bytes)?;

    // certificates [0] IMPLICIT（OPTIONAL）からTSA証明書ハッシュ抽出
    let cert_hash = extract_cert_hash_from_reader(&mut r);

    Ok((timestamp, cert_hash))
}

/// DERエラーをCoreErrorに変換するヘルパー。
fn map_der(e: der::Error) -> CoreError {
    CoreError::ContentHashExtractionFailed(format!("DERパースエラー: {e}"))
}

/// SignedData.certificates [0] IMPLICIT から最初の証明書DERを取得し、
/// SHA-256ハッシュをhex文字列で返す。
///
/// リーダーはencapContentInfo直後に位置していること。
/// 証明書フィールドが存在しない場合はNoneを返す。
fn extract_cert_hash_from_reader(r: &mut SliceReader<'_>) -> Option<String> {
    // certificates [0] IMPLICIT は CONTEXT-SPECIFIC CONSTRUCTED tag 0xA0
    let header = r.peek_header().ok()?;
    if !header.tag.is_context_specific()
        || !header.tag.is_constructed()
        || header.tag.number().value() != 0
    {
        return None;
    }

    // [0] IMPLICIT ヘッダを読み飛ばし → 内部は証明書列
    Header::decode(r).ok()?;

    // 最初の Certificate SEQUENCE のTLV全体を取得してSHA-256ハッシュ
    let first_cert_tlv = r.tlv_bytes().ok()?;
    let hash = Sha256::digest(first_cert_tlv);
    Some(hex::encode(hash))
}

/// TstInfo DERバイト列からgen_time（GeneralizedTime）をUnix epoch秒として抽出する。
///
/// `der::asn1::GeneralizedTime` を使用。der crateはDER厳密モードで小数秒を
/// 拒否するため、BERエンコードの小数秒が含まれる場合はフォールバック処理を行う。
fn parse_tst_info(tst_info_der: &[u8]) -> Result<u64, CoreError> {
    let mut r = SliceReader::new(tst_info_der).map_err(map_der)?;

    // TstInfo SEQUENCE に入る
    Header::decode(&mut r).map_err(map_der)?;
    // version INTEGER（スキップ）
    der::asn1::AnyRef::decode(&mut r).map_err(map_der)?;
    // policy OID（スキップ）
    der::asn1::AnyRef::decode(&mut r).map_err(map_der)?;
    // messageImprint SEQUENCE（スキップ）
    der::asn1::AnyRef::decode(&mut r).map_err(map_der)?;
    // serialNumber INTEGER（スキップ）
    der::asn1::AnyRef::decode(&mut r).map_err(map_der)?;
    // genTime GeneralizedTime
    match der::asn1::GeneralizedTime::decode(&mut r) {
        Ok(gt) => generalized_time_to_epoch(gt),
        Err(_) => {
            // BERエンコードの小数秒（例: "20240101000000.500Z"）に備え、
            // 小数秒を除去した上でGeneralizedTimeとして再デコードする
            parse_tst_info_strip_fractional(tst_info_der)
        }
    }
}

/// GeneralizedTime → Unix epoch秒への変換。
fn generalized_time_to_epoch(gt: der::asn1::GeneralizedTime) -> Result<u64, CoreError> {
    let dt: std::time::SystemTime = gt.into();
    let epoch = dt.duration_since(std::time::UNIX_EPOCH).map_err(|_| {
        CoreError::ContentHashExtractionFailed("GeneralizedTimeがUNIX_EPOCH以前です".into())
    })?;
    Ok(epoch.as_secs())
}

/// GeneralizedTimeの小数秒を除去してリトライするフォールバック。
///
/// BERエンコードされたTSAトークンでは小数秒が含まれる場合がある。
/// DER厳密モードの`der::asn1::GeneralizedTime`は小数秒を拒否するため、
/// 小数秒を除去した文字列をDER再エンコードして`GeneralizedTime`でデコードする。
fn parse_tst_info_strip_fractional(tst_info_der: &[u8]) -> Result<u64, CoreError> {
    let mut r = SliceReader::new(tst_info_der).map_err(map_der)?;
    Header::decode(&mut r).map_err(map_der)?;
    for _ in 0..4 {
        der::asn1::AnyRef::decode(&mut r).map_err(map_der)?;
    }
    // genTime を生バイトとして読む（タグは0x18 GeneralizedTime）
    let raw = der::asn1::AnyRef::decode(&mut r).map_err(map_der)?;
    let time_str = std::str::from_utf8(raw.value()).map_err(|_| {
        CoreError::ContentHashExtractionFailed("GeneralizedTimeがUTF-8ではありません".into())
    })?;

    // 小数秒を除去: "YYYYMMDDHHmmSS.fracZ" → "YYYYMMDDHHmmSSZ"
    let cleaned = if let Some(dot_pos) = time_str.find('.') {
        format!("{}Z", &time_str[..dot_pos])
    } else {
        time_str.to_string()
    };

    // DER再エンコードしてGeneralizedTimeとしてデコード
    let cleaned_bytes = cleaned.as_bytes();
    let mut der_bytes = Vec::with_capacity(2 + cleaned_bytes.len());
    der_bytes.push(0x18); // GeneralizedTime tag
    der_bytes.push(cleaned_bytes.len() as u8);
    der_bytes.extend_from_slice(cleaned_bytes);

    let mut reader = SliceReader::new(&der_bytes).map_err(map_der)?;
    let gt = der::asn1::GeneralizedTime::decode(&mut reader).map_err(|_| {
        CoreError::ContentHashExtractionFailed(format!(
            "小数秒除去後のGeneralizedTimeデコードエラー: {time_str}"
        ))
    })?;
    generalized_time_to_epoch(gt)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- ヘルパー関数 -----

    /// テスト用: DER長エンコード
    fn der_encode_length(len: usize, out: &mut Vec<u8>) {
        if len < 0x80 {
            out.push(len as u8);
        } else if len <= 0xff {
            out.push(0x81);
            out.push(len as u8);
        } else {
            out.push(0x82);
            out.push((len >> 8) as u8);
            out.push(len as u8);
        }
    }

    /// テスト用: バイト列をDER SEQUENCEでラップ
    fn wrap_sequence(content: &[u8]) -> Vec<u8> {
        let mut result = Vec::new();
        result.push(0x30);
        der_encode_length(content.len(), &mut result);
        result.extend_from_slice(content);
        result
    }

    /// テスト用: 最小TstInfo DERを構築
    fn build_minimal_tst_info(gen_time_str: &[u8]) -> Vec<u8> {
        let mut tst = Vec::new();

        // version INTEGER 1
        tst.extend_from_slice(&[0x02, 0x01, 0x01]);
        // policy OID 1.2.3.4
        tst.extend_from_slice(&[0x06, 0x03, 0x2a, 0x03, 0x04]);
        // messageImprint SEQUENCE { algorithm SEQUENCE { OID sha-256 }, digest OCTET STRING }
        let oid_bytes = [0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01];
        let algo_seq = wrap_sequence(&oid_bytes);
        let mut imprint = Vec::new();
        imprint.extend_from_slice(&algo_seq);
        imprint.push(0x04);
        imprint.push(0x20);
        imprint.extend_from_slice(&[0u8; 32]);
        tst.extend_from_slice(&wrap_sequence(&imprint));
        // serialNumber INTEGER 42
        tst.extend_from_slice(&[0x02, 0x01, 0x2a]);
        // genTime GeneralizedTime
        tst.push(0x18);
        tst.push(gen_time_str.len() as u8);
        tst.extend_from_slice(gen_time_str);

        wrap_sequence(&tst)
    }

    /// テスト用: ContentInfo → SignedData → EncapContentInfo → TstInfo のフルDERを構築
    fn build_tst_token(gen_time_str: &[u8], cert_der: Option<&[u8]>) -> Vec<u8> {
        let tst_info_der = build_minimal_tst_info(gen_time_str);

        // EncapsulatedContentInfo
        let mut econtent_inner = Vec::new();
        econtent_inner.push(0x04); // OCTET STRING
        der_encode_length(tst_info_der.len(), &mut econtent_inner);
        econtent_inner.extend_from_slice(&tst_info_der);
        let mut econtent_tagged = vec![0xa0]; // [0] EXPLICIT
        der_encode_length(econtent_inner.len(), &mut econtent_tagged);
        econtent_tagged.extend_from_slice(&econtent_inner);

        let mut eci_inner = Vec::new();
        // eContentType OID (id-ct-TSTInfo = 1.2.840.113549.1.9.16.1.4)
        eci_inner.extend_from_slice(&[
            0x06, 0x0b, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x09, 0x10, 0x01, 0x04,
        ]);
        eci_inner.extend_from_slice(&econtent_tagged);
        let eci = wrap_sequence(&eci_inner);

        // SignedData
        let mut sd_inner = Vec::new();
        sd_inner.extend_from_slice(&[0x02, 0x01, 0x03]); // version INTEGER 3
        sd_inner.extend_from_slice(&[0x31, 0x00]); // digestAlgorithms SET (empty)
        sd_inner.extend_from_slice(&eci);
        // certificates [0] IMPLICIT (OPTIONAL)
        if let Some(cert) = cert_der {
            sd_inner.push(0xa0); // [0] IMPLICIT CONSTRUCTED
            der_encode_length(cert.len(), &mut sd_inner);
            sd_inner.extend_from_slice(cert);
        }
        let signed_data = wrap_sequence(&sd_inner);

        // ContentInfo
        let mut tagged = vec![0xa0]; // content [0] EXPLICIT
        der_encode_length(signed_data.len(), &mut tagged);
        tagged.extend_from_slice(&signed_data);
        let mut ci_inner = Vec::new();
        // contentType OID (id-signedData = 1.2.840.113549.1.7.2)
        ci_inner.extend_from_slice(&[
            0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x02,
        ]);
        ci_inner.extend_from_slice(&tagged);
        wrap_sequence(&ci_inner)
    }

    // ----- COSE層テスト -----

    #[test]
    fn test_extract_tsa_from_cose_no_tsa() {
        let sign1 = coset::CoseSign1Builder::new()
            .payload(vec![1, 2, 3])
            .build();
        let cose_bytes = sign1.to_vec().unwrap();

        let result = extract_tsa_from_cose(&cose_bytes).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_tsa_from_cose_invalid_bytes() {
        let result = extract_tsa_from_cose(&[0x00, 0x01, 0x02]);
        assert!(result.is_err());
    }

    // ----- TstInfo パーステスト -----

    #[test]
    fn test_parse_tst_info_valid() {
        let tst_info_der = build_minimal_tst_info(b"20240101000000Z");
        let ts = parse_tst_info(&tst_info_der).unwrap();
        assert_eq!(ts, 1704067200); // 2024-01-01T00:00:00Z
    }

    #[test]
    fn test_parse_tst_info_with_fractional_seconds() {
        let tst_info_der = build_minimal_tst_info(b"20240101000000.500Z");
        let ts = parse_tst_info(&tst_info_der).unwrap();
        assert_eq!(ts, 1704067200);
    }

    #[test]
    fn test_parse_tst_info_empty_input() {
        assert!(parse_tst_info(&[]).is_err());
    }

    #[test]
    fn test_parse_tst_info_truncated() {
        // SEQUENCEヘッダのみ、中身なし
        assert!(parse_tst_info(&[0x30, 0x00]).is_err());
    }

    // ----- フルTSTトークン パーステスト -----

    #[test]
    fn test_parse_full_tst_token_no_certs() {
        let content_info = build_tst_token(b"20260226153045Z", None);
        let (ts, cert_hash) = parse_tst_token_der(&content_info).unwrap();
        assert!(ts > 1700000000);
        assert!(ts < 1900000000);
        assert!(cert_hash.is_none());
    }

    #[test]
    fn test_parse_full_tst_token_with_cert() {
        // ダミー証明書: 最小SEQUENCE { INTEGER 1 }
        let dummy_cert = wrap_sequence(&[0x02, 0x01, 0x01]);
        let content_info = build_tst_token(b"20240101000000Z", Some(&dummy_cert));
        let (ts, cert_hash) = parse_tst_token_der(&content_info).unwrap();
        assert_eq!(ts, 1704067200);
        // cert_hashはダミー証明書のSHA-256
        assert!(cert_hash.is_some());
        let hash = cert_hash.unwrap();
        assert_eq!(hash.len(), 64); // hex-encoded SHA-256 = 64 chars
        // 同じ入力からは同じハッシュが得られること
        let (_, cert_hash2) = parse_tst_token_der(&content_info).unwrap();
        assert_eq!(Some(hash), cert_hash2);
    }

    #[test]
    fn test_parse_tst_token_empty_input() {
        assert!(parse_tst_token_der(&[]).is_err());
    }

    #[test]
    fn test_parse_tst_token_garbage() {
        assert!(parse_tst_token_der(&[0xff, 0xff, 0xff]).is_err());
    }

    // ----- ヘルパー関数テスト -----

    #[test]
    fn test_find_header_by_text_found() {
        let rest = vec![(
            coset::Label::Text("sigTst".into()),
            ciborium::Value::Integer(42.into()),
        )];
        assert!(find_header_by_text(&rest, "sigTst").is_some());
    }

    #[test]
    fn test_find_header_by_text_not_found() {
        let rest = vec![(
            coset::Label::Text("other".into()),
            ciborium::Value::Integer(42.into()),
        )];
        assert!(find_header_by_text(&rest, "sigTst").is_none());
    }

    #[test]
    fn test_find_header_by_text_prefers_exact_match() {
        let rest = vec![
            (
                coset::Label::Text("sigTst".into()),
                ciborium::Value::Integer(1.into()),
            ),
            (
                coset::Label::Text("sigTst2".into()),
                ciborium::Value::Integer(2.into()),
            ),
        ];
        let v = find_header_by_text(&rest, "sigTst2").unwrap();
        assert_eq!(v, ciborium::Value::Integer(2.into()));
    }

    // ----- sigTst2優先テスト -----

    #[test]
    #[allow(non_snake_case)]
    fn test_extract_tsa_sigTst2_preferred_over_sigTst() {
        // sigTst と sigTst2 の両方が存在する場合、sigTst2 が優先される。
        // 仕様書 §2.4: sigTst2（優先）→ sigTst（フォールバック）

        // 2つの異なるTSTトークンを構築（異なるタイムスタンプ）
        let token_sig_tst = build_tst_token(b"20240601000000Z", None);
        let token_sig_tst2 = build_tst_token(b"20240101000000Z", None);

        // TstContainer CBOR を構築
        fn build_tst_container_cbor(token_der: &[u8]) -> ciborium::Value {
            // { "tstTokens": [ { "val": <token_der> } ] }
            ciborium::Value::Map(vec![(
                ciborium::Value::Text("tstTokens".into()),
                ciborium::Value::Array(vec![ciborium::Value::Map(vec![(
                    ciborium::Value::Text("val".into()),
                    ciborium::Value::Bytes(token_der.to_vec()),
                )])]),
            )])
        }

        // COSE_Sign1 にsigTst と sigTst2 の両方をセット
        let mut sign1 = coset::CoseSign1Builder::new()
            .payload(vec![1, 2, 3])
            .build();
        sign1.unprotected.rest.push((
            coset::Label::Text("sigTst".into()),
            build_tst_container_cbor(&token_sig_tst),
        ));
        sign1.unprotected.rest.push((
            coset::Label::Text("sigTst2".into()),
            build_tst_container_cbor(&token_sig_tst2),
        ));
        let cose_bytes = sign1.to_vec().unwrap();

        let result = extract_tsa_from_cose(&cose_bytes).unwrap().unwrap();

        // sigTst2 のタイムスタンプ (2024-01-01) が使われるべき
        assert_eq!(result.timestamp, 1704067200); // 2024-01-01T00:00:00Z
        // sigTst のタイムスタンプ (2024-06-01) ではない
        assert_ne!(result.timestamp, 1717200000);
    }
}
