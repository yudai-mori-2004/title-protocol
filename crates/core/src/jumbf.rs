//! JUMBF（ISO 19566-5）の最小限パーサー。
//!
//! C2PA JUMBF データから特定マニフェストの COSE 署名バイト列を抽出する。
//! 仕様書 §2.1: content_hash = SHA-256(Active Manifestの署名)

use crate::{CoreError, MAX_SIGNATURE_SIZE};
use std::io::{Cursor, Read, Seek, SeekFrom};

/// JUMBF ボックスヘッダサイズ（4バイトsize + 4バイトtype）
const HEADER_SIZE: u64 = 8;

/// JUMBF superbox タイプ "jumb" (0x6A756D62)
const BOX_TYPE_JUMB: u32 = 0x6A75_6D62;
/// JUMBF description box タイプ "jumd" (0x6A756D64)
const BOX_TYPE_JUMD: u32 = 0x6A75_6D64;
/// CBOR content box タイプ "cbor" (0x63626F72)
const BOX_TYPE_CBOR: u32 = 0x6362_6F72;

/// c2pa.signature の UUID（16バイト）
/// hex: "6332637300110010800000AA00389B71"
const CAI_SIGNATURE_UUID: [u8; 16] = [
    0x63, 0x32, 0x63, 0x73, 0x00, 0x11, 0x00, 0x10, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B,
    0x71,
];

/// ボックスヘッダ情報
struct BoxHeader {
    box_type: u32,
    size: u64,
}

/// Description box から読み取った情報
struct DescInfo {
    uuid: [u8; 16],
    label: String,
}

/// ボックスヘッダを読み取る。
/// EOFに達した場合は box_type=0, size=0 を返す。
fn read_header(reader: &mut Cursor<&[u8]>) -> Result<BoxHeader, CoreError> {
    let mut buf = [0u8; 8];
    if reader.read(&mut buf).map_err(|e| {
        CoreError::ContentHashExtractionFailed(format!("JUMBFヘッダ読み取りエラー: {e}"))
    })? < 8
    {
        return Ok(BoxHeader {
            box_type: 0,
            size: 0,
        });
    }

    let size = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let box_type = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);

    if size == 1 {
        // Extended size (u64)
        let mut ext_buf = [0u8; 8];
        reader.read_exact(&mut ext_buf).map_err(|e| {
            CoreError::ContentHashExtractionFailed(format!("JUMBF拡張サイズ読み取りエラー: {e}"))
        })?;
        let large_size = u64::from_be_bytes(ext_buf);
        Ok(BoxHeader {
            box_type,
            size: large_size,
        })
    } else {
        Ok(BoxHeader {
            box_type,
            size: size as u64,
        })
    }
}

/// Description box の内容（UUID + ラベル）を読み取る。
fn read_desc_info(reader: &mut Cursor<&[u8]>, content_size: u64) -> Result<DescInfo, CoreError> {
    if content_size < 17 {
        return Err(CoreError::ContentHashExtractionFailed(
            "JUMBF description boxが短すぎます".to_string(),
        ));
    }

    let mut uuid = [0u8; 16];
    reader.read_exact(&mut uuid).map_err(|e| {
        CoreError::ContentHashExtractionFailed(format!("UUID読み取りエラー: {e}"))
    })?;

    let mut toggles = [0u8; 1];
    reader.read_exact(&mut toggles).map_err(|e| {
        CoreError::ContentHashExtractionFailed(format!("トグル読み取りエラー: {e}"))
    })?;

    let mut label = String::new();
    if toggles[0] & 0x02 != 0 {
        // ラベル文字列がある（null終端）
        // content_sizeを超えないようにガード（不正データによる無限ループ防止）
        let max_label_len = (content_size - 17) as usize;
        let mut byte = [0u8; 1];
        loop {
            if label.len() >= max_label_len {
                return Err(CoreError::ContentHashExtractionFailed(
                    "JUMBFラベルがnull終端されていません".to_string(),
                ));
            }
            reader.read_exact(&mut byte).map_err(|e| {
                CoreError::ContentHashExtractionFailed(format!("ラベル読み取りエラー: {e}"))
            })?;
            if byte[0] == 0 {
                break;
            }
            label.push(byte[0] as char);
        }
    }

    // 残りのバイトをスキップ（padding, salt hash等）
    let read_so_far = 16 + 1 + if label.is_empty() { 0 } else { label.len() + 1 } as u64;
    if read_so_far < content_size {
        let skip = content_size - read_so_far;
        reader.seek(SeekFrom::Current(skip as i64)).map_err(|e| {
            CoreError::ContentHashExtractionFailed(format!("スキップエラー: {e}"))
        })?;
    }

    Ok(DescInfo { uuid, label })
}

/// 指定されたマニフェストラベルの COSE 署名バイト列を JUMBF データから抽出する。
///
/// 仕様書 §2.1: Active Manifest の署名を取得する
///
/// # 引数
/// * `jumbf_data` - `c2pa::jumbf_io::load_jumbf_from_memory` で取得した生のJUMBFバイト列
/// * `manifest_label` - 抽出対象のマニフェストラベル（例: Reader::active_label()の値）
pub fn extract_signature_from_jumbf(
    jumbf_data: &[u8],
    manifest_label: &str,
) -> Result<Vec<u8>, CoreError> {
    let mut reader = Cursor::new(jumbf_data);

    // トップレベルのsuperbox（c2pa store）を読む
    let top_header = read_header(&mut reader)?;
    if top_header.box_type != BOX_TYPE_JUMB {
        return Err(CoreError::ContentHashExtractionFailed(
            "トップレベルがJUMBF superboxではありません".to_string(),
        ));
    }

    // Description boxを読む
    let desc_header = read_header(&mut reader)?;
    if desc_header.box_type != BOX_TYPE_JUMD {
        return Err(CoreError::ContentHashExtractionFailed(
            "Description boxが見つかりません".to_string(),
        ));
    }
    let _top_desc = read_desc_info(&mut reader, desc_header.size - HEADER_SIZE)?;

    // 各マニフェスト（子superbox）をスキャンして対象ラベルを探す
    let top_end = top_header.size;
    while reader.position() < top_end {
        let child_start = reader.position();
        let child_header = read_header(&mut reader)?;
        if child_header.box_type == 0 || child_header.size == 0 {
            break;
        }

        if child_header.box_type == BOX_TYPE_JUMB {
            // マニフェストsuperbox: description boxからラベルを読む
            let desc_header = read_header(&mut reader)?;
            if desc_header.box_type == BOX_TYPE_JUMD {
                let desc = read_desc_info(&mut reader, desc_header.size - HEADER_SIZE)?;

                if desc.label == manifest_label {
                    // このマニフェスト内でc2pa.signatureボックスを探す
                    return find_signature_in_manifest(
                        &mut reader,
                        child_start + child_header.size,
                    );
                }
            }
        }

        // このボックスの残りをスキップ
        reader
            .seek(SeekFrom::Start(child_start + child_header.size))
            .map_err(|e| {
                CoreError::ContentHashExtractionFailed(format!("シークエラー: {e}"))
            })?;
    }

    Err(CoreError::ContentHashExtractionFailed(format!(
        "マニフェスト '{manifest_label}' が見つかりません"
    )))
}

/// マニフェストsuperbox内からc2pa.signature boxのCBORデータを抽出する。
fn find_signature_in_manifest(
    reader: &mut Cursor<&[u8]>,
    manifest_end: u64,
) -> Result<Vec<u8>, CoreError> {
    while reader.position() < manifest_end {
        let box_start = reader.position();
        let header = read_header(reader)?;
        if header.box_type == 0 || header.size == 0 {
            break;
        }

        if header.box_type == BOX_TYPE_JUMB {
            // Description boxを読んでUUIDを確認
            let desc_header = read_header(reader)?;
            if desc_header.box_type == BOX_TYPE_JUMD {
                let desc = read_desc_info(reader, desc_header.size - HEADER_SIZE)?;

                if desc.uuid == CAI_SIGNATURE_UUID {
                    // c2pa.signature superbox内のCBOR boxを探す
                    return find_cbor_in_box(reader, box_start + header.size);
                }
            }
        }

        // このボックスの残りをスキップ
        reader
            .seek(SeekFrom::Start(box_start + header.size))
            .map_err(|e| {
                CoreError::ContentHashExtractionFailed(format!("シークエラー: {e}"))
            })?;
    }

    Err(CoreError::ContentHashExtractionFailed(
        "c2pa.signature boxが見つかりません".to_string(),
    ))
}

/// superbox内から最初のCBOR boxのデータを抽出する。
fn find_cbor_in_box(
    reader: &mut Cursor<&[u8]>,
    box_end: u64,
) -> Result<Vec<u8>, CoreError> {
    while reader.position() < box_end {
        let box_start = reader.position();
        let header = read_header(reader)?;
        if header.box_type == 0 || header.size == 0 {
            break;
        }

        if header.box_type == BOX_TYPE_CBOR {
            let data_len = header.size - HEADER_SIZE;
            // 不正な巨大サイズによるOOMパニックを防止
            if data_len > MAX_SIGNATURE_SIZE {
                return Err(CoreError::ContentHashExtractionFailed(format!(
                    "CBOR boxのサイズが上限を超えています: {data_len} > {MAX_SIGNATURE_SIZE}"
                )));
            }
            let mut data = vec![0u8; data_len as usize];
            reader.read_exact(&mut data).map_err(|e| {
                CoreError::ContentHashExtractionFailed(format!("CBOR読み取りエラー: {e}"))
            })?;
            return Ok(data);
        }

        // このボックスをスキップ
        reader
            .seek(SeekFrom::Start(box_start + header.size))
            .map_err(|e| {
                CoreError::ContentHashExtractionFailed(format!("シークエラー: {e}"))
            })?;
    }

    Err(CoreError::ContentHashExtractionFailed(
        "CBOR boxが見つかりません".to_string(),
    ))
}
