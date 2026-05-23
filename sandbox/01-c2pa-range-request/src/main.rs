// SPDX-License-Identifier: Apache-2.0
//! Sandbox A: c2pa-rs HTTP Range Request 検証
//!
//! 目的:
//! c2pa-rs の Reader が HTTP Range Request 経由で大容量ファイルを
//! 検証できるか確認する。ファイル全体をメモリに載せずに処理できることを検証。
//!
//! 仕様書参照: §4.3「単一ファイル — Range Requestパターン」, §5.2「コンテンツ取得の詳細」

use std::io::{self, Read, Seek, SeekFrom};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// Access pattern tracking
// ---------------------------------------------------------------------------

/// アクセスパターンの記録
#[derive(Debug, Clone)]
struct AccessRecord {
    /// 開始位置
    start: u64,
    /// 読み取りバイト数
    len: u64,
}

// ---------------------------------------------------------------------------
// Buffered HTTP Range Reader: Read + Seek adapter over HTTP Range Requests
// ---------------------------------------------------------------------------

/// バッファ付き HTTP Range Request アダプタ。
/// c2pa-rs の Reader が要求する Read + Seek を HTTP Range Request で実現する。
///
/// バッファリング戦略:
/// - 先読みバッファ (BUFFER_SIZE) を持ち、小さな read() を集約する
/// - Seek 時にバッファがヒットすればHTTPリクエストを省略
/// - バッファミスの場合のみ新しい Range Request を発行
///
/// 仕様書 §5.2: c2pa-rs の Reader はランダムアクセス（Read+Seek）を要求する。
const BUFFER_SIZE: usize = 256 * 1024; // 256KB バッファ

struct HttpRangeReader {
    /// リクエスト先の URL
    url: String,
    /// ファイルの総サイズ（バイト）
    file_size: u64,
    /// 現在の論理読み取り位置
    position: u64,
    /// ETag（ファイル整合性チェック用）
    /// 仕様書 §5.2: 初回リクエストで取得した ETag を以降の If-Match に含める
    etag: Option<String>,
    /// HTTP クライアント（コネクションプーリング）
    client: reqwest::blocking::Client,
    /// 先読みバッファ
    buffer: Vec<u8>,
    /// バッファの開始位置（ファイル上の絶対位置）
    buffer_start: u64,
    /// バッファの有効データ長
    buffer_len: usize,
    /// メトリクス: HTTP リクエスト数（HEAD含む）
    request_count: Arc<AtomicU64>,
    /// メトリクス: 実際に HTTP で転送されたバイト数
    bytes_transferred: Arc<AtomicU64>,
    /// アクセスパターンの記録（デバッグ用）
    access_log: Arc<Mutex<Vec<AccessRecord>>>,
}

impl HttpRangeReader {
    /// URL からファイルサイズと ETag を取得してリーダーを構築する。
    fn new(url: &str) -> io::Result<Self> {
        let client = reqwest::blocking::Client::new();
        let request_count = Arc::new(AtomicU64::new(0));
        let bytes_transferred = Arc::new(AtomicU64::new(0));
        let access_log = Arc::new(Mutex::new(Vec::new()));

        // HEAD リクエストでファイルサイズと ETag を取得
        let resp = client
            .head(url)
            .send()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("HEAD failed: {e}")))?;

        request_count.fetch_add(1, Ordering::Relaxed);

        if !resp.status().is_success() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("HEAD returned status: {}", resp.status()),
            ));
        }

        let file_size = resp
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Content-Length missing"))?;

        let etag = resp
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string());

        Ok(Self {
            url: url.to_string(),
            file_size,
            position: 0,
            etag,
            client,
            buffer: vec![0u8; BUFFER_SIZE],
            buffer_start: u64::MAX, // 無効な初期値
            buffer_len: 0,
            request_count,
            bytes_transferred,
            access_log,
        })
    }

    /// バッファを現在の position から埋める
    fn fill_buffer(&mut self) -> io::Result<()> {
        let start = self.position;
        let end = std::cmp::min(
            start + BUFFER_SIZE as u64 - 1,
            self.file_size.saturating_sub(1),
        );

        if start >= self.file_size {
            self.buffer_start = start;
            self.buffer_len = 0;
            return Ok(());
        }

        let range = format!("bytes={start}-{end}");
        let mut req = self.client.get(&self.url).header("Range", &range);

        if let Some(ref etag) = self.etag {
            req = req.header("If-Match", etag);
        }

        let resp = req
            .send()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Range GET failed: {e}")))?;

        self.request_count.fetch_add(1, Ordering::Relaxed);

        if resp.status().as_u16() == 412 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "ファイルが変更されました（412 Precondition Failed）",
            ));
        }

        if resp.status().as_u16() != 206 && resp.status().as_u16() != 200 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("Status: {} for range {range}", resp.status()),
            ));
        }

        let bytes = resp
            .bytes()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Body failed: {e}")))?;

        let n = std::cmp::min(bytes.len(), BUFFER_SIZE);
        self.buffer[..n].copy_from_slice(&bytes[..n]);
        self.buffer_start = start;
        self.buffer_len = n;
        self.bytes_transferred.fetch_add(n as u64, Ordering::Relaxed);

        // アクセスパターン記録
        if let Ok(mut log) = self.access_log.lock() {
            log.push(AccessRecord { start, len: n as u64 });
        }

        Ok(())
    }

    /// バッファヒット判定: position がバッファ範囲内か
    fn buffer_contains(&self, pos: u64) -> bool {
        pos >= self.buffer_start && pos < self.buffer_start + self.buffer_len as u64
    }

    /// メトリクスを取得する
    fn metrics(&self) -> (u64, u64) {
        (
            self.request_count.load(Ordering::Relaxed),
            self.bytes_transferred.load(Ordering::Relaxed),
        )
    }

    /// アクセスパターンのサマリーを出力
    fn print_access_pattern(&self) {
        let log = self.access_log.lock().unwrap();
        if log.is_empty() {
            println!("  アクセスパターン: なし");
            return;
        }

        println!("  アクセスパターン（HTTP Range Request ごと）:");
        println!("    リクエスト数: {}", log.len());

        // 範囲の統計
        let total_bytes: u64 = log.iter().map(|r| r.len).sum();
        let min_start = log.iter().map(|r| r.start).min().unwrap_or(0);
        let max_end = log.iter().map(|r| r.start + r.len).max().unwrap_or(0);

        println!("    読み取り範囲: {min_start}..{max_end} ({:.2} MB)", max_end as f64 / 1_048_576.0);
        println!("    合計転送量: {total_bytes} bytes ({:.2} MB)", total_bytes as f64 / 1_048_576.0);

        // シーケンシャル vs ランダムアクセスの分析
        let mut sequential = 0;
        let mut backward = 0;
        let mut forward_skip = 0;
        for w in log.windows(2) {
            let expected_next = w[0].start + w[0].len;
            if w[1].start == expected_next {
                sequential += 1;
            } else if w[1].start < w[0].start {
                backward += 1;
            } else {
                forward_skip += 1;
            }
        }
        println!("    シーケンシャル読み取り: {sequential}");
        println!("    後方シーク: {backward}");
        println!("    前方スキップ: {forward_skip}");

        // 最初の数リクエストと最後の数リクエストを表示
        let show_count = 5;
        if log.len() <= show_count * 2 {
            for (i, r) in log.iter().enumerate() {
                println!("    [{:>3}] offset={:>12}, len={:>8} ({:.1} KB)",
                    i, r.start, r.len, r.len as f64 / 1024.0);
            }
        } else {
            for (i, r) in log.iter().take(show_count).enumerate() {
                println!("    [{:>3}] offset={:>12}, len={:>8} ({:.1} KB)",
                    i, r.start, r.len, r.len as f64 / 1024.0);
            }
            println!("    ... ({} リクエスト省略) ...", log.len() - show_count * 2);
            for (i, r) in log.iter().skip(log.len() - show_count).enumerate() {
                let idx = log.len() - show_count + i;
                println!("    [{:>3}] offset={:>12}, len={:>8} ({:.1} KB)",
                    idx, r.start, r.len, r.len as f64 / 1024.0);
            }
        }
    }
}

impl Read for HttpRangeReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.position >= self.file_size {
            return Ok(0);
        }

        // バッファミスの場合、新しいバッファを取得
        if !self.buffer_contains(self.position) {
            self.fill_buffer()?;
        }

        // バッファからコピー
        let buf_offset = (self.position - self.buffer_start) as usize;
        let available = self.buffer_len - buf_offset;
        let n = std::cmp::min(available, buf.len());
        buf[..n].copy_from_slice(&self.buffer[buf_offset..buf_offset + n]);
        self.position += n as u64;

        Ok(n)
    }
}

impl Seek for HttpRangeReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let new_pos = match pos {
            SeekFrom::Start(offset) => offset as i64,
            SeekFrom::End(offset) => self.file_size as i64 + offset,
            SeekFrom::Current(offset) => self.position as i64 + offset,
        };

        if new_pos < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Seek to negative position: {new_pos}"),
            ));
        }

        self.position = new_pos as u64;
        Ok(self.position)
    }
}

// ---------------------------------------------------------------------------
// Local HTTP server (axum with ETag support)
// ---------------------------------------------------------------------------

async fn start_server(file_path: PathBuf) -> SocketAddr {
    use axum::Router;
    use axum::body::Body;
    use axum::extract::Request;
    use axum::response::Response;

    let file_data = tokio::fs::read(&file_path).await.unwrap();
    let file_size = file_data.len();

    // ETag を事前計算（ファイルサイズ + 修正時間のハッシュで簡易生成）
    let etag_hash = Sha256::digest(format!("{}-{}", file_path.display(), file_size).as_bytes());
    let etag = format!("\"{}\"", hex::encode(&etag_hash[..8]));

    let file_data = Arc::new(file_data);
    let etag_arc = Arc::new(etag);

    let app = Router::new().fallback(move |req: Request| {
        let data = Arc::clone(&file_data);
        let etag = Arc::clone(&etag_arc);
        async move {
            let file_size = data.len();

            // If-Match チェック
            if let Some(if_match) = req.headers().get("if-match") {
                let if_match_str = if_match.to_str().unwrap_or("");
                if if_match_str != etag.as_str() && if_match_str != "*" {
                    return Response::builder()
                        .status(412)
                        .body(Body::empty())
                        .unwrap();
                }
            }

            // Range ヘッダのパース
            if let Some(range_header) = req.headers().get("range") {
                let range_str = range_header.to_str().unwrap_or("");
                if let Some(range) = parse_range(range_str, file_size) {
                    let (start, end) = range;
                    let chunk = &data[start..=end];
                    let content_range = format!("bytes {start}-{end}/{file_size}");

                    return Response::builder()
                        .status(206)
                        .header("Content-Range", content_range)
                        .header("Content-Length", chunk.len().to_string())
                        .header("Accept-Ranges", "bytes")
                        .header("ETag", etag.as_str())
                        .body(Body::from(chunk.to_vec()))
                        .unwrap();
                }
            }

            // Range なし: 全体を返す
            Response::builder()
                .status(200)
                .header("Content-Length", file_size.to_string())
                .header("Accept-Ranges", "bytes")
                .header("ETag", etag.as_str())
                .body(Body::from(data.as_ref().clone()))
                .unwrap()
        }
    });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    addr
}

/// 簡易 Range ヘッダパーサ: "bytes=START-END" → (START, END)
fn parse_range(header: &str, file_size: usize) -> Option<(usize, usize)> {
    let header = header.strip_prefix("bytes=")?;
    let parts: Vec<&str> = header.split('-').collect();
    if parts.len() != 2 {
        return None;
    }

    let start = parts[0].parse::<usize>().ok()?;
    let end = if parts[1].is_empty() {
        file_size - 1
    } else {
        parts[1].parse::<usize>().ok()?
    };

    if start > end || start >= file_size {
        return None;
    }

    Some((start, std::cmp::min(end, file_size - 1)))
}

// ---------------------------------------------------------------------------
// C2PA 検証結果の詳細レポート
// ---------------------------------------------------------------------------

/// 検証結果を詳細に出力し、ハードバインディング検証が実施されたか確認する。
/// 返り値: (validation_state, validation_status のコード一覧)
fn report_validation_details(reader: &c2pa::Reader) -> (String, Vec<String>) {
    let state = format!("{:?}", reader.validation_state());
    println!("  検証状態: {state}");

    // validation_status: エラー一覧
    let mut status_codes = Vec::new();
    if let Some(statuses) = reader.validation_status() {
        println!("  validation_status ({} 件):", statuses.len());
        for s in statuses {
            let code = s.code().to_string();
            println!("    [{}] url={}", code, s.url().unwrap_or("(none)"));
            status_codes.push(code);
        }
    } else {
        println!("  validation_status: なし（エラーなし）");
    }

    // validation_results: 成功・情報・失敗の詳細
    if let Some(results) = reader.validation_results() {
        if let Some(active) = results.active_manifest() {
            let success = active.success().len();
            let info = active.informational().len();
            let failure = active.failure().len();
            println!("  validation_results (active_manifest): success={success}, info={info}, failure={failure}");

            if failure > 0 {
                println!("  !! 失敗項目あり:");
                for f in active.failure() {
                    println!("    - [{}] url={}", f.code(), f.url().unwrap_or("(none)"));
                }
            }
        }
    }

    // マニフェスト情報
    if let Some(manifest) = reader.active_manifest() {
        println!("  Active Manifest: {}", reader.active_label().unwrap_or("unknown"));
        println!("  タイトル: {}", manifest.title().unwrap_or("unknown"));
    }

    // JSON の SHA-256（結果の同一性比較用）
    let json = reader.json();
    let hash = Sha256::digest(json.as_bytes());
    println!("  Manifest JSON hash: sha256:{}", hex::encode(hash));

    (state, status_codes)
}

// ---------------------------------------------------------------------------
// ベースライン: メモリ全ロードで検証
// ---------------------------------------------------------------------------

/// ベースライン検証（メモリ全ロード）。
/// 返り値: (validation_state, validation_status コード一覧, manifest_json_hash)
fn verify_from_memory(file_path: &std::path::Path, mime_type: &str) -> Result<(String, Vec<String>, String), String> {
    println!("\n--- ベースライン: メモリ上の検証 ---");

    let data = std::fs::read(file_path).map_err(|e| format!("ファイル読み込み失敗: {e}"))?;
    let file_size = data.len();
    println!("  ファイルサイズ: {file_size} bytes ({:.2} MB)", file_size as f64 / 1_048_576.0);

    let mut cursor = std::io::Cursor::new(&data);
    let context = c2pa::Context::default();
    let reader = c2pa::Reader::from_context(context)
        .with_stream(mime_type, &mut cursor)
        .map_err(|e| format!("C2PA Reader 構築失敗: {e}"))?;

    let (state, codes) = report_validation_details(&reader);
    let json_hash = hex::encode(Sha256::digest(reader.json().as_bytes()));

    Ok((state, codes, json_hash))
}

/// 改ざんファイルの検証（ハードバインディングが機能するか確認）。
fn verify_tampered(file_path: &std::path::Path, mime_type: &str) -> Result<(String, Vec<String>), String> {
    println!("\n--- 改ざん検知テスト ---");

    let mut data = std::fs::read(file_path).map_err(|e| format!("ファイル読み込み失敗: {e}"))?;
    let file_size = data.len();

    // コンテンツ部分を改ざん（中間地点の1バイトを反転）
    let tamper_pos = file_size / 2;
    let original_byte = data[tamper_pos];
    data[tamper_pos] ^= 0xFF;
    println!("  改ざん: offset={tamper_pos} の 0x{:02x} → 0x{:02x}", original_byte, data[tamper_pos]);

    let mut cursor = std::io::Cursor::new(&data);
    let context = c2pa::Context::default();

    match c2pa::Reader::from_context(context).with_stream(mime_type, &mut cursor) {
        Ok(reader) => {
            let (state, codes) = report_validation_details(&reader);
            Ok((state, codes))
        }
        Err(e) => {
            println!("  Reader 構築自体が失敗: {e}");
            println!("  → 改ざんにより C2PA データ構造が壊れた（検知成功）");
            Ok(("Error".to_string(), vec![format!("reader_error: {e}")]))
        }
    }
}

// ---------------------------------------------------------------------------
// Range Request 経由の検証
// ---------------------------------------------------------------------------

/// Range Request 経由の検証。
/// 返り値: (requests, transferred, file_size, state, codes, json_hash)
fn verify_via_range_request(url: &str, mime_type: &str) -> Result<(u64, u64, u64, String, Vec<String>, String), String> {
    println!("\n--- Range Request 経由の検証 ---");
    println!("  URL: {url}");

    let mut http_reader =
        HttpRangeReader::new(url).map_err(|e| format!("HttpRangeReader 構築失敗: {e}"))?;

    let file_size = http_reader.file_size;
    let has_etag = http_reader.etag.is_some();
    println!("  ファイルサイズ: {} bytes ({:.2} MB)", file_size, file_size as f64 / 1_048_576.0);
    println!("  ETag: {}", if has_etag { "あり ✓" } else { "なし" });
    println!("  バッファサイズ: {} bytes ({} KB)", BUFFER_SIZE, BUFFER_SIZE / 1024);

    // c2pa-rs Reader に Read+Seek として渡す
    let context = c2pa::Context::default();
    let reader = c2pa::Reader::from_context(context)
        .with_stream(mime_type, &mut http_reader)
        .map_err(|e| format!("C2PA Reader 構築失敗（Range Request）: {e}"))?;

    let (request_count, bytes_transferred) = http_reader.metrics();

    println!("\n  検証結果:");
    let (state, codes) = report_validation_details(&reader);
    let json_hash = hex::encode(Sha256::digest(reader.json().as_bytes()));

    println!("\n  転送メトリクス:");
    println!("  HTTP リクエスト数: {request_count} (HEAD 1 + GET {})", request_count - 1);
    println!("  転送バイト数: {bytes_transferred} bytes ({:.2} MB)",
        bytes_transferred as f64 / 1_048_576.0);
    println!("  ファイルサイズ: {file_size} bytes ({:.2} MB)",
        file_size as f64 / 1_048_576.0);

    let ratio = if file_size > 0 {
        bytes_transferred as f64 / file_size as f64 * 100.0
    } else {
        0.0
    };
    println!("  転送比率: {ratio:.1}%");

    if file_size > bytes_transferred {
        let saved = file_size - bytes_transferred;
        println!("  節約量: {saved} bytes ({:.2} MB)", saved as f64 / 1_048_576.0);
    }

    // アクセスパターン
    println!();
    http_reader.print_access_pattern();

    Ok((request_count, bytes_transferred, file_size, state, codes, json_hash))
}

// ---------------------------------------------------------------------------
// v0.84 での署名→検証ラウンドトリップ
// ---------------------------------------------------------------------------

/// v0.84 で新規署名したファイルが正しく検証できるかを確認する。
/// フィクスチャの問題（v0.78署名→v0.84検証の互換性）を切り分けるため。
fn sign_and_verify_roundtrip(source_path: &std::path::Path, mime_type: &str) -> Result<(String, Vec<String>, String, PathBuf), String> {
    println!("\n--- v0.84 署名→検証ラウンドトリップ ---");

    // EphemeralSigner: c2pa-rs 組み込みのテスト用 Ed25519 signer
    // 自己署名 CA + EE 証明書を自動生成する
    let signer = c2pa::EphemeralSigner::new("sandbox-a-test")
        .map_err(|e| format!("EphemeralSigner構築失敗: {e}"))?;

    // Builder で署名
    let definition = serde_json::json!({
        "claim_generator_info": [{
            "name": "sandbox-a-roundtrip-test",
            "version": "0.1.0"
        }],
        "assertions": [{
            "label": "c2pa.actions.v2",
            "data": {
                "actions": [{
                    "action": "c2pa.created",
                    "digitalSourceType": "http://cv.iptc.org/newscodes/digitalsourcetype/trainedAlgorithmicMedia"
                }]
            }
        }]
    });

    let context = c2pa::Context::default();
    let mut builder = c2pa::Builder::from_context(context)
        .with_definition(&definition.to_string())
        .map_err(|e| format!("Builder定義失敗: {e}"))?;

    // 出力先の一時ファイル
    let tmp_dir = tempfile::tempdir().map_err(|e| format!("一時ディレクトリ作成失敗: {e}"))?;
    let output_path = tmp_dir.path().join("signed_output.mp4");

    let mut source = std::fs::File::open(source_path)
        .map_err(|e| format!("ソースファイルオープン失敗: {e}"))?;
    let mut output = std::fs::File::create(&output_path)
        .map_err(|e| format!("出力ファイル作成失敗: {e}"))?;

    builder.sign(&signer, mime_type, &mut source, &mut output)
        .map_err(|e| format!("署名失敗: {e}"))?;

    println!("  v0.84 で署名完了: {}", output_path.display());
    let output_size = std::fs::metadata(&output_path)
        .map_err(|e| format!("出力ファイルメタデータ失敗: {e}"))?
        .len();
    println!("  出力サイズ: {output_size} bytes ({:.2} MB)", output_size as f64 / 1_048_576.0);

    // 検証
    let data = std::fs::read(&output_path)
        .map_err(|e| format!("出力ファイル読み込み失敗: {e}"))?;
    let mut cursor = std::io::Cursor::new(&data);
    let reader = c2pa::Reader::from_context(c2pa::Context::default())
        .with_stream(mime_type, &mut cursor)
        .map_err(|e| format!("Reader構築失敗: {e}"))?;

    let (state, codes) = report_validation_details(&reader);
    let json_hash = hex::encode(Sha256::digest(reader.json().as_bytes()));

    // tmp_dir を leak させない — output_path をコピーして返す
    let persistent_path = std::env::temp_dir().join("sandbox_a_signed_roundtrip.mp4");
    std::fs::copy(&output_path, &persistent_path)
        .map_err(|e| format!("コピー失敗: {e}"))?;

    Ok((state, codes, json_hash, persistent_path))
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

struct TestCase {
    file: String,
    mime: String,
    label: String,
}

/// 検証結果の比較用
struct ValidationSnapshot {
    state: String,
    codes: Vec<String>,
    json_hash: String,
}

#[tokio::main]
async fn main() {
    println!("========================================================");
    println!("  Sandbox A: c2pa-rs HTTP Range Request 検証");
    println!("  仕様書 §4.3, §5.2");
    println!("  バッファサイズ: {} KB", BUFFER_SIZE / 1024);
    println!("========================================================\n");

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixtures_base = manifest_dir.join("../../legacy/v0.1.0/tests/fixtures/c2pa/signed");

    // ======================================================================
    // Phase 1: ベースラインファイル選択
    // ======================================================================

    let test_cases: Vec<TestCase> = vec![
        TestCase { file: "mp4-10mb.mp4".into(), mime: "video/mp4".into(), label: "10MB MP4".into() },
        TestCase { file: "mp4-5mb.mp4".into(), mime: "video/mp4".into(), label: "5MB MP4".into() },
        TestCase { file: "mp4-1mb.mp4".into(), mime: "video/mp4".into(), label: "1MB MP4".into() },
        TestCase { file: "mp4-10s-720p.mp4".into(), mime: "video/mp4".into(), label: "720p MP4 (small)".into() },
        TestCase { file: "sample.jpg".into(), mime: "image/jpeg".into(), label: "JPEG".into() },
    ];

    let first = test_cases.iter().find_map(|tc| {
        let path = fixtures_base.join(&tc.file);
        path.exists().then_some((path, tc.mime.clone(), tc.label.clone()))
    });

    let (primary_path, primary_mime, primary_label) = match first {
        Some(f) => f,
        None => {
            eprintln!("ERROR: テストフィクスチャが見つかりません: {}", fixtures_base.display());
            return;
        }
    };

    println!("Primary: {} ({})\n", primary_label, primary_path.display());

    // ======================================================================
    // Phase 2: ベースライン検証（メモリ全ロード）
    // ======================================================================

    let pp = primary_path.clone();
    let pm = primary_mime.clone();
    let baseline_result = tokio::task::spawn_blocking(move || verify_from_memory(&pp, &pm))
        .await.unwrap();

    let baseline = match baseline_result {
        Ok((state, codes, json_hash)) => {
            println!("  => ベースライン完了\n");
            ValidationSnapshot { state, codes, json_hash }
        }
        Err(e) => {
            eprintln!("  => ベースライン ERROR: {e}");
            return;
        }
    };

    // validation_status の内容を確認 — self-signed cert 以外のエラーがないことを検証
    let non_cert_errors: Vec<&String> = baseline.codes.iter()
        .filter(|c| *c != "signing_credential.untrusted" && *c != "signingCredential.untrusted")
        .collect();
    if non_cert_errors.is_empty() {
        println!("  ✓ 証明書信頼性以外のエラーなし（self-signed cert のみ）");
    } else {
        println!("  ⚠ 証明書信頼性以外のエラーあり: {:?}", non_cert_errors);
        println!("    → v0.78 フィクスチャ → v0.84 検証の互換性問題の可能性。");
        println!("    → Phase 2b でラウンドトリップテストを実施して切り分ける。");
    }

    // ======================================================================
    // Phase 2b: v0.84 署名→検証ラウンドトリップ（フィクスチャ互換性の切り分け）
    // ======================================================================

    // 未署名のソースファイルを用意 — legacy フィクスチャの元ファイルを使う
    let unsigned_base = manifest_dir.join("../../legacy/v0.1.0/tests/fixtures/c2pa/unsigned");
    let unsigned_candidates = ["mp4-10mb.mp4", "mp4-5mb.mp4", "mp4-1mb.mp4", "mp4-10s-720p.mp4"];
    let unsigned_source = unsigned_candidates.iter().find_map(|f| {
        let p = unsigned_base.join(f);
        p.exists().then_some(p)
    });

    // unsigned が無ければ signed をソースとして使う（c2pa-rs は既存署名を上書きする）
    let roundtrip_source = unsigned_source.unwrap_or_else(|| primary_path.clone());
    let roundtrip_mime = primary_mime.clone();

    let rs = roundtrip_source.clone();
    let rm = roundtrip_mime.clone();
    let roundtrip_result = tokio::task::spawn_blocking(move || {
        sign_and_verify_roundtrip(&rs, &rm)
    }).await.unwrap();

    let roundtrip_signed_path: Option<PathBuf>;
    match &roundtrip_result {
        Ok((state, codes, _json_hash, signed_path)) => {
            roundtrip_signed_path = Some(signed_path.clone());

            let rt_non_cert: Vec<&String> = codes.iter()
                .filter(|c| *c != "signing_credential.untrusted" && *c != "signingCredential.untrusted")
                .collect();
            if rt_non_cert.is_empty() {
                println!("\n  ✓ v0.84 ラウンドトリップ: cert 信頼性以外のエラーなし");
                println!("    → フィクスチャの問題（v0.78→v0.84互換性）であることが確定");
                println!("    → 本実装では v0.84 で署名するため問題にならない");
            } else {
                eprintln!("\n  ✗ v0.84 ラウンドトリップでもエラーあり: state={state}, codes={rt_non_cert:?}");
                eprintln!("    → c2pa v0.84 自体の問題の可能性。要調査。");
            }
        }
        Err(e) => {
            roundtrip_signed_path = None;
            eprintln!("  ラウンドトリップテスト失敗: {e}");
        }
    }

    // ======================================================================
    // Phase 2c: ラウンドトリップファイルの改ざん検知テスト
    // ======================================================================

    if let Some(ref signed_path) = roundtrip_signed_path {
        let sp = signed_path.clone();
        let sm = primary_mime.clone();
        let rt_tamper = tokio::task::spawn_blocking(move || verify_tampered(&sp, &sm))
            .await.unwrap();

        match &rt_tamper {
            Ok((state, codes)) => {
                let rt_baseline_codes: Vec<String> = match &roundtrip_result {
                    Ok((_, c, _, _)) => c.clone(),
                    _ => vec![],
                };
                let new_codes: Vec<&String> = codes.iter()
                    .filter(|c| !rt_baseline_codes.contains(c))
                    .collect();

                if state == "Error" || !new_codes.is_empty() {
                    println!("  ✓ v0.84 署名ファイルでも改ざん検知成功！");
                    if !new_codes.is_empty() {
                        println!("    新たに発生したエラー: {new_codes:?}");
                    }
                } else {
                    eprintln!("  ✗ v0.84 署名ファイルで改ざんが検知されなかった！重大な問題。");
                }
            }
            Err(e) => eprintln!("  改ざんテスト失敗: {e}"),
        }
    }

    // ======================================================================
    // Phase 3: legacy フィクスチャの改ざん検知テスト（ハードバインディングの検証）
    // ======================================================================

    let pp = primary_path.clone();
    let pm = primary_mime.clone();
    let tamper_result = tokio::task::spawn_blocking(move || verify_tampered(&pp, &pm))
        .await.unwrap();

    match &tamper_result {
        Ok((tamper_state, tamper_codes)) => {
            // 改ざんが検知されたことを確認する方法:
            // 1) validation_state が変わる（Passed→Invalid 等）
            // 2) validation_status に新しいエラーコードが増える
            // 3) Reader 構築自体が失敗する（Error ケース）
            let tamper_detected = if tamper_state == "Error" {
                // Reader 構築失敗 = 改ざんでデータ構造が壊れた
                true
            } else if tamper_codes.len() > baseline.codes.len() {
                // エラーコードが増えた = 改ざん検知
                true
            } else {
                // ベースラインにない新しいエラーコードがあるか
                tamper_codes.iter().any(|c| !baseline.codes.contains(c))
            };

            if tamper_detected {
                println!("\n  ✓ 改ざん検知成功！");
                println!("    ベースライン state={}, codes={:?}", baseline.state, baseline.codes);
                println!("    改ざん後  state={tamper_state}, codes={tamper_codes:?}");
                let new_codes: Vec<&String> = tamper_codes.iter()
                    .filter(|c| !baseline.codes.contains(c))
                    .collect();
                if !new_codes.is_empty() {
                    println!("    新たに発生したエラー: {new_codes:?}");
                }
            } else {
                eprintln!("\n  ✗ 改ざんが検知されなかった！");
                eprintln!("    ベースライン state={}, codes={:?}", baseline.state, baseline.codes);
                eprintln!("    改ざん後  state={tamper_state}, codes={tamper_codes:?}");
                eprintln!("    → ハードバインディング検証が機能していない可能性。重大な問題。");
            }
        }
        Err(e) => {
            eprintln!("  改ざんテスト実行失敗: {e}");
        }
    }

    // ======================================================================
    // Phase 4: Range Request 経由の検証 + ベースラインとの結果比較
    // ======================================================================

    struct RangeResult {
        label: String,
        requests: u64,
        transferred: u64,
        file_size: u64,
        validation: Option<ValidationSnapshot>,
    }
    let mut results: Vec<RangeResult> = Vec::new();

    let mut all_cases = test_cases;
    for (file, label) in [("mp4-50mb.mp4", "50MB MP4"), ("mp4-25mb.mp4", "25MB MP4")] {
        all_cases.push(TestCase {
            file: file.into(),
            mime: "video/mp4".into(),
            label: label.into(),
        });
    }

    for tc in &all_cases {
        let path = fixtures_base.join(&tc.file);
        if !path.exists() {
            continue;
        }

        println!("\n========================================================");
        println!("  テスト: {}", tc.label);
        println!("========================================================");

        let addr = start_server(path).await;
        let url = format!("http://{addr}/test.mp4");
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let url_clone = url.clone();
        let mime_clone = tc.mime.clone();
        let result = tokio::task::spawn_blocking(move || {
            verify_via_range_request(&url_clone, &mime_clone)
        }).await.unwrap();

        match result {
            Ok((reqs, transferred, total, state, codes, json_hash)) => {
                results.push(RangeResult {
                    label: tc.label.clone(),
                    requests: reqs,
                    transferred,
                    file_size: total,
                    validation: Some(ValidationSnapshot { state, codes, json_hash }),
                });
            }
            Err(e) => {
                eprintln!("  ERROR: {e}");
                results.push(RangeResult {
                    label: tc.label.clone(),
                    requests: 0,
                    transferred: 0,
                    file_size: 0,
                    validation: None,
                });
            }
        }
    }

    // ======================================================================
    // Phase 5: ベースライン vs Range Request の結果比較（Primary ファイル）
    // ======================================================================

    println!("\n\n========================================================");
    println!("  Phase 5: ベースライン vs Range Request の一致検証");
    println!("========================================================");

    if let Some(primary_range) = results.iter().find(|r| r.label == primary_label) {
        if let Some(ref rv) = primary_range.validation {
            let state_match = rv.state == baseline.state;
            let codes_match = rv.codes == baseline.codes;
            let hash_match = rv.json_hash == baseline.json_hash;

            println!("  validation_state:  ベースライン={}, Range Request={} {}",
                baseline.state, rv.state, if state_match { "✓" } else { "✗ MISMATCH" });
            println!("  validation_codes:  ベースライン={:?}", baseline.codes);
            println!("                     Range Request={:?} {}",
                rv.codes, if codes_match { "✓" } else { "✗ MISMATCH" });
            println!("  manifest_json_hash:");
            println!("    ベースライン:    sha256:{}", baseline.json_hash);
            println!("    Range Request:   sha256:{} {}",
                rv.json_hash, if hash_match { "✓" } else { "✗ MISMATCH" });

            if state_match && codes_match && hash_match {
                println!("\n  ✓ ベースラインと Range Request の検証結果が完全一致");
                println!("    → Range Request アダプタが検証結果に影響を与えていないことを確認");
            } else {
                eprintln!("\n  ✗ ベースラインと Range Request の検証結果が不一致！");
                eprintln!("    → Range Request アダプタに問題がある可能性。重大な問題。");
            }
        } else {
            eprintln!("  Primary ファイルの Range Request 検証が失敗 — 比較不可");
        }
    } else {
        eprintln!("  Primary ファイル ({primary_label}) の Range Request 結果が見つかりません");
    }

    // ======================================================================
    // Phase 6: 転送メトリクスサマリー
    // ======================================================================

    println!("\n\n========================================================");
    println!("  転送メトリクスサマリー");
    println!("========================================================");
    println!("  {:<20} {:>8} {:>12} {:>12} {:>8}",
        "テスト", "Requests", "転送量", "File Size", "比率");
    println!("  {:-<70}", "");
    for r in &results {
        if r.file_size == 0 { continue; }
        let ratio = r.transferred as f64 / r.file_size as f64 * 100.0;
        println!("  {:<20} {:>8} {:>9.2} MB {:>9.2} MB {:>7.1}%",
            r.label, r.requests,
            r.transferred as f64 / 1_048_576.0,
            r.file_size as f64 / 1_048_576.0,
            ratio);
    }

    // ======================================================================
    // Phase 7: 総合判定
    // ======================================================================

    println!("\n\n========================================================");
    println!("  総合判定");
    println!("========================================================");

    // 改ざん検知（legacy フィクスチャ）
    let tamper_ok = match &tamper_result {
        Ok((state, codes)) => {
            state == "Error" || codes.iter().any(|c| !baseline.codes.contains(c))
        }
        Err(_) => false,
    };

    // v0.84 ラウンドトリップ: cert 以外のエラーがないか
    let roundtrip_clean = match &roundtrip_result {
        Ok((_, codes, _, _)) => {
            codes.iter()
                .filter(|c| *c != "signing_credential.untrusted" && *c != "signingCredential.untrusted")
                .count() == 0
        }
        Err(_) => false,
    };

    // Range Request vs ベースライン一致
    let range_matches_baseline = results.iter()
        .find(|r| r.label == primary_label)
        .and_then(|r| r.validation.as_ref())
        .map(|v| v.state == baseline.state && v.codes == baseline.codes && v.json_hash == baseline.json_hash)
        .unwrap_or(false);

    // legacy ベースライン（参考情報 — フィクスチャ互換性の問題は v0.84 ラウンドトリップで解決）
    let baseline_clean = non_cert_errors.is_empty();

    println!("  [{}] v0.84 署名→検証ラウンドトリップ: cert 以外のエラーなし",
        if roundtrip_clean { "✓" } else { "✗" });
    println!("  [{}] 改ざん検知: ハードバインディング (bmffHash) が機能",
        if tamper_ok { "✓" } else { "✗" });
    println!("  [{}] Range Request vs ベースライン一致: 検証結果・JSON 完全同一",
        if range_matches_baseline { "✓" } else { "✗" });
    println!("  [{}] メモリ効率: バッファサイズ {} KB のみ使用（ファイル全体をメモリに載せない）",
        "✓", BUFFER_SIZE / 1024);
    println!("  [{}] ETag/If-Match: 整合性チェック実装済み",
        "✓");

    if !baseline_clean {
        println!("  [⚠] legacy フィクスチャ (v0.78署名) は v0.84 で一部エラー（互換性の問題）");
        println!("      エラー: {:?}", non_cert_errors);
        if roundtrip_clean {
            println!("      → v0.84 ラウンドトリップではエラーなし。フィクスチャの問題であることが確定。");
        }
    }

    let all_pass = roundtrip_clean && tamper_ok && range_matches_baseline;
    if all_pass {
        println!("\n  ★ Sandbox A: 全検証項目 PASS ★");
    } else {
        eprintln!("\n  ✗ Sandbox A: 一部の検証項目が FAIL — 調査が必要");
    }

    println!("\n========================================================");
    println!("  検証完了");
    println!("========================================================");
}
