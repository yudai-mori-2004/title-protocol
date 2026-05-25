// SPDX-License-Identifier: Apache-2.0

//! # HTTP Range Request ベースの `ContentSource`
//!
//! 仕様 §4.3 — 50 GB 級の動画でもメモリに全展開せず、Range Request で
//! 必要部分だけを読みながら C2PA 検証を進めるための実装。
//!
//! ## 構成
//!
//! - [`HttpRangeSource`] — URL を保持する `ContentSource`。`open()` で
//!   [`http_range_client::HttpReader`] を新規構築して返す。`HttpReader` は
//!   `Read + Seek` を実装しており、内部で reqwest::blocking を使って
//!   HTTP Range Request を発行する。
//! - 構築時に HEAD でファイルサイズと `Accept-Ranges: bytes` を確認し、
//!   Range Request 非対応サーバーでは構築自体を失敗させる。これにより
//!   `fetch_streaming` の呼び出し側がフォールバック (full GET) を選択できる。
//!
//! ## メモリパターン
//!
//! 各 `open()` は新しい HTTP クライアントを構築して新しいバッファを持つ。
//! processor を並列実行する場合、各 reader が独立した Range Request を
//! 発行するため、ピークメモリは「各 reader のチャンクバッファ」のみ。
//!
//! ## 制約
//!
//! - HEAD 要求が拒否される (405) サーバーは扱えない。`probe` が失敗するので
//!   呼び出し側が full GET 経路にフォールバックする。
//! - HTTP/HTTPS のみ (`reqwest::blocking::Client` のサポート範囲)。
//! - HEAD レスポンスの `Content-Length` がない場合はエラー (サイズが不明だと
//!   `c2pa::Reader` の `Seek` 動作が壊れる場合がある)。

use std::io::{Read, Seek, SeekFrom};
use std::time::Duration;

use http_range_client::{HttpError, HttpReader, SyncBufferedHttpRangeClient, SyncHttpRangeClient};
use title_core::{ContentSource, ContentStream};

use crate::content_fetch::FetchError;

/// HEAD 要求のタイムアウト。fetch 全体のタイムアウト (60s) より短く取る。
const HEAD_TIMEOUT: Duration = Duration::from_secs(10);

/// HTTP Range Request 経由でコンテンツを公開する `ContentSource`。
///
/// 構築時に HEAD でサイズと Range サポートを確認する。以降の `open()` は
/// その情報を使って `HttpReader` を構築する (`HttpReader` 自体は遅延的に
/// 初回 read で HEAD を再発行するため、ここでの probe は冗長だが、
/// 「Range 非対応サーバーをこのレイヤで弾く」目的で残す)。
#[derive(Debug)]
pub struct HttpRangeSource {
    url: String,
    content_length: u64,
    etag: Option<String>,
    content_type_hint: Option<String>,
    /// 1 リクエストあたりの最小バイト数。`DEFAULT_MIN_REQ_SIZE` (4 MiB) が
    /// デフォルト。c2pa-rs の box header スキャンを 1 リクエストにまとめる
    /// 効果と、reader バッファのピークメモリ予約のバランスをこの値で取る。
    min_req_size: usize,
}

impl HttpRangeSource {
    /// デフォルトの最小リクエストサイズ (4 MiB)。
    ///
    /// c2pa-rs の `Reader::with_stream` は box ヘッダ (8-16 byte) を頻繁に
    /// seek/read するため、`min_req_size` が小さいと Range Request が大量に
    /// 発行され、ネットワーク往復で律速する。4 MiB に揃えることで:
    ///
    /// - JUMBF + 署名チェーン (典型 50 KB-500 KB) を 1 リクエストで取得
    /// - 50 GB ファイルでも全体で ~12,500 リクエスト程度
    /// - 各 reader のピークメモリも `min_req_size` で抑えられる (peak_memory_hint
    ///   が同じ値を申告するので ResourcePool が正しく予約する)
    ///
    /// テスト目的で `with_min_req_size` を使って小さい値に変更できる。
    pub const DEFAULT_MIN_REQ_SIZE: usize = 4 * 1024 * 1024;

    /// HEAD 経由でサーバーの能力を確認してから construct する。
    ///
    /// 失敗条件:
    /// - HEAD が non-2xx を返した
    /// - `Content-Length` が無い、もしくは 0
    /// - `Accept-Ranges` ヘッダが `bytes` ではない
    pub fn probe(url: &str) -> Result<Self, FetchError> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("title-tee/0.1.2 (range-probe)")
            .timeout(HEAD_TIMEOUT)
            .build()
            .map_err(|e| FetchError::HttpError {
                url: url.to_string(),
                reason: format!("Range probe client build failed: {e}"),
            })?;

        let resp = client
            .head(url)
            .send()
            .map_err(|e| FetchError::HttpError {
                url: url.to_string(),
                reason: format!("Range probe HEAD failed: {e}"),
            })?;

        if !resp.status().is_success() {
            return Err(FetchError::HttpStatus {
                status: resp.status().as_u16(),
                url: url.to_string(),
            });
        }

        let accepts_ranges = resp
            .headers()
            .get(reqwest::header::ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.eq_ignore_ascii_case("bytes"))
            .unwrap_or(false);

        if !accepts_ranges {
            return Err(FetchError::HttpError {
                url: url.to_string(),
                reason: "Server does not advertise Accept-Ranges: bytes".to_string(),
            });
        }

        // 注: reqwest の `content_length()` は HEAD レスポンスでは body 長 (=0)
        // を返してしまう。HEAD では Content-Length ヘッダを直接パースする必要が
        // ある (これは reqwest の意図的な挙動で、HEAD の body は常に 0 byte だから)。
        let content_length: u64 = resp
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| FetchError::HttpError {
                url: url.to_string(),
                reason: "HEAD response missing or invalid Content-Length header".to_string(),
            })?;

        if content_length == 0 {
            return Err(FetchError::EmptyContent(url.to_string()));
        }

        let etag = resp
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let content_type_hint = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        Ok(Self {
            url: url.to_string(),
            content_length,
            etag,
            content_type_hint,
            min_req_size: Self::DEFAULT_MIN_REQ_SIZE,
        })
    }

    /// 最小リクエストサイズを変更する (テスト・チューニング用)。
    pub fn with_min_req_size(mut self, size: usize) -> Self {
        self.min_req_size = size;
        self
    }

    /// HEAD で得た ETag を返す (将来の整合性チェック用)。
    pub fn etag(&self) -> Option<&str> {
        self.etag.as_deref()
    }

    /// HEAD で得た `Content-Type` を返す (`fetch_streaming` で使う)。
    pub fn content_type_hint(&self) -> Option<&str> {
        self.content_type_hint.as_deref()
    }

    /// 内部 URL を返す。診断用。
    pub fn url(&self) -> &str {
        &self.url
    }
}

impl ContentSource for HttpRangeSource {
    fn open(&self) -> std::io::Result<Box<dyn ContentStream>> {
        let mut reader = HttpReader::new(&self.url);
        reader.set_min_req_size(self.min_req_size);
        Ok(Box::new(SafeRangeReader::new(reader, self.content_length)))
    }

    fn size_hint(&self) -> Option<u64> {
        Some(self.content_length)
    }

    /// 仕様 §4.3 — Range Request 経由のピークメモリ見積もり。
    ///
    /// `SyncBufferedHttpRangeClient` のバッファは「`min_req_size` か、caller が
    /// `read_exact(&mut [0; N])` で要求した N の大きい方」まで膨張する。本実装の
    /// `min_req_size` デフォルト (4 MiB) は C2PA 検証で出る典型的な単発 read
    /// (JUMBF + COSE 署名 = 数百 KB) を充分に上回るため、`min_req_size` を
    /// そのままピーク見積もりとして使う。
    ///
    /// 50 GB ファイルでも申告は 4 MiB のみ。ResourcePool admission_limit 100 MB
    /// に対し 25 並列の Range Request が同居できる計算で、SPECS §4.3 の
    /// 「マニフェスト + チャンク 1 個分」要件と整合する。
    fn peak_memory_hint(&self) -> Option<u64> {
        Some(self.min_req_size as u64)
    }

    /// Range Request 経路は常駐ゼロ。`open()` ごとに `min_req_size` のバッファを
    /// 持つ reader が生成されるだけで、source 自体は URL + content_length 等の
    /// メタデータのみ。
    fn is_in_memory_resident(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// SafeRangeReader — http-range-client の Read impl バグ + EOF 規約を吸収する
// 共通アダプタ。HttpRangeSource (direct HTTP) と ProxyRangeSource (proxy 経由)
// の両方で再利用する。
// ---------------------------------------------------------------------------

/// `http_range_client::SyncBufferedHttpRangeClient<T>` の `Read` 実装は次の
/// 規約違反を持つため、本ラッパで吸収する:
///
/// 1. `read(&mut buf)` が常に `Ok(buf.len())` を返すが、実書き込みバイト数は
///    `bytes.len()` (file 末尾を跨ぐと < buf.len())。残りのバッファは
///    呼び出し側のデータが残ったまま valid 扱いされ、c2pa-rs に偽データを
///    渡してしまう。
/// 2. EOF を `Ok(0)` ではなく HTTP 416 → `ErrorKind::UnexpectedEof` で返すため
///    `read_to_end` などの標準 API が壊れる。
/// 3. `get_request_range` が `begin+length > tail` の判定だけで「先に fetch」を
///    決め、`split_to` を呼んでバッファを縮める。直後の fetch が 416 で失敗
///    すると、`split_to` で残ったバッファ末尾の未消費 valid データも読めない
///    状態で error が伝播する。
///
/// 対策: `file_size` を保持し、EOF を超える `get_bytes(N)` を発行する前に
/// `N = min(buf.len(), file_size - pos)` で clip。これで `begin+length > tail`
/// が「最後の有効バイトの直後」までしか発火せず、未消費バッファのロストも
/// 余計な 416 リクエストも buffer 越境の偽データも起きない。
///
/// `Seek` は内部にデリゲートしつつ自前で `pos` を追跡し、`Read` と同じ EOF 判定
/// を共有する。`HttpRangeSource` / `ProxyRangeSource` の両方で再利用される。
pub(crate) struct SafeRangeReader<T: SyncHttpRangeClient> {
    inner: SyncBufferedHttpRangeClient<T>,
    file_size: u64,
    pos: u64,
}

impl<T: SyncHttpRangeClient> SafeRangeReader<T> {
    /// 構築時に `file_size` を渡す。これは `ContentSource` 構築時の HEAD probe
    /// で既に判明している値で、wire 越しに再取得する必要はない。
    pub(crate) fn new(inner: SyncBufferedHttpRangeClient<T>, file_size: u64) -> Self {
        Self {
            inner,
            file_size,
            pos: 0,
        }
    }
}

impl<T: SyncHttpRangeClient> Read for SafeRangeReader<T> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() || self.pos >= self.file_size {
            return Ok(0);
        }
        // EOF を超えるリクエストを未然に防ぐ。これにより上流の get_request_range が
        // 「fetch 失敗 → 未消費 buffer ロスト」経路を踏まなくなる。
        let remaining = self.file_size - self.pos;
        let request_len = (buf.len() as u64).min(remaining) as usize;

        match self.inner.get_bytes(request_len) {
            Ok(bytes) => {
                let n = bytes.len();
                // 上流の Read impl は `Ok(buf.len())` 固定だが、実際に書き込めるのは
                // `bytes.len()` (≤ buf.len())。ここで実 length を素直に返すことで
                // Rust 標準 `Read::read` 規約に準拠する。
                buf[..n].copy_from_slice(bytes);
                self.pos += n as u64;
                Ok(n)
            }
            // 念のため 416 も明示ハンドリング (`request_len` clip により本来は発火しない)。
            Err(HttpError::HttpStatus(416)) => Ok(0),
            Err(e) => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )),
        }
    }
}

impl<T: SyncHttpRangeClient> Seek for SafeRangeReader<T> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        // 内部にデリゲートして得た新 position を保持する。SeekFrom::End / Current
        // も含めて inner が正しく解決した値を信用する。
        let new_pos = self.inner.seek(pos)?;
        self.pos = new_pos;
        Ok(new_pos)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    /// 仕様 §4.3 の Range Request パターンを満たす最小限の HTTP サーバー。
    /// HEAD と `Range: bytes=` GET を解釈し、固定バイト列を返す。
    /// 各テストごとに spawn して、テスト終了で自動 drop。
    struct FakeRangeServer {
        addr: String,
        _handle: thread::JoinHandle<()>,
    }

    impl FakeRangeServer {
        fn spawn(body: Vec<u8>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap().to_string();
            let handle = thread::spawn(move || loop {
                let (stream, _) = match listener.accept() {
                    Ok(s) => s,
                    Err(_) => break,
                };
                let body = body.clone();
                thread::spawn(move || handle_conn(stream, body));
            });
            Self {
                addr,
                _handle: handle,
            }
        }

        fn url(&self, path: &str) -> String {
            format!("http://{}{path}", self.addr)
        }
    }

    fn handle_conn(mut stream: TcpStream, body: Vec<u8>) {
        // 簡易 HTTP/1.1 パーサ。テスト用なので CRLF 駆動で十分。
        let mut buf = [0u8; 4096];
        let n = match stream.read(&mut buf) {
            Ok(n) if n > 0 => n,
            _ => return,
        };
        let req = String::from_utf8_lossy(&buf[..n]);
        let mut lines = req.lines();
        let request_line = lines.next().unwrap_or("");
        let method = request_line.split_whitespace().next().unwrap_or("");

        let mut range = None;
        for line in lines {
            if line.is_empty() {
                break;
            }
            // HTTP ヘッダ名は case-insensitive。reqwest は小文字 (`range:`) で送る。
            let lower = line.to_ascii_lowercase();
            if let Some(v) = lower.strip_prefix("range: bytes=") {
                if let Some((start_s, end_s)) = v.split_once('-') {
                    let start: usize = start_s.parse().unwrap_or(0);
                    let end: usize = if end_s.is_empty() {
                        body.len() - 1
                    } else {
                        end_s.parse().unwrap_or(body.len() - 1)
                    };
                    range = Some((start, end));
                }
            }
        }

        match method {
            "HEAD" => {
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nContent-Type: video/mp4\r\nETag: \"test-etag\"\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            }
            "GET" => {
                let (start, end) = range.unwrap_or((0, body.len() - 1));
                // 範囲が body を完全に超えている → 416 Range Not Satisfiable。
                // http-range-client が EOF として処理する。
                if start >= body.len() {
                    let resp = format!(
                        "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nContent-Range: bytes */{}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(resp.as_bytes());
                } else {
                    let clipped_end = end.min(body.len() - 1);
                    let slice = &body[start..=clipped_end];
                    let status = if range.is_some() {
                        "206 Partial Content"
                    } else {
                        "200 OK"
                    };
                    let content_range = format!("bytes {start}-{clipped_end}/{}", body.len());
                    let resp_headers = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Range: {}\r\nConnection: close\r\n\r\n",
                        slice.len(),
                        content_range,
                    );
                    let _ = stream.write_all(resp_headers.as_bytes());
                    let _ = stream.write_all(slice);
                }
            }
            _ => {
                let _ = stream.write_all(b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            }
        }
        // 確実にクローズ (reqwest が EOF を待つ場合がある)
        let _ = stream.shutdown(std::net::Shutdown::Both);
    }

    #[test]
    fn probe_returns_source_with_content_length() {
        let body = vec![0xAAu8; 1024];
        let server = FakeRangeServer::spawn(body.clone());
        let source = HttpRangeSource::probe(&server.url("/test.mp4")).expect("probe");
        assert_eq!(source.size_hint(), Some(1024));
        assert_eq!(source.etag(), Some("\"test-etag\""));
        assert_eq!(source.content_type_hint(), Some("video/mp4"));
    }

    #[test]
    fn open_then_read_returns_body() {
        // 4 KB 程度のサイズで Range Request 経由の Read を検証。
        let body: Vec<u8> = (0..4096).map(|i| (i % 251) as u8).collect();
        let server = FakeRangeServer::spawn(body.clone());
        let source = HttpRangeSource::probe(&server.url("/data.bin"))
            .expect("probe")
            .with_min_req_size(512);
        let mut reader = source.open().expect("open");

        let mut got = Vec::new();
        reader.read_to_end(&mut got).expect("read_to_end");
        assert_eq!(got, body);
    }

    #[test]
    fn seek_then_read_returns_correct_slice() {
        let body: Vec<u8> = (0..4096).map(|i| (i % 251) as u8).collect();
        let server = FakeRangeServer::spawn(body.clone());
        let source = HttpRangeSource::probe(&server.url("/data.bin"))
            .expect("probe")
            .with_min_req_size(256);
        let mut reader = source.open().expect("open");

        reader.seek(SeekFrom::Start(2000)).expect("seek");
        let mut buf = vec![0u8; 100];
        reader.read_exact(&mut buf).expect("read_exact");
        assert_eq!(buf, body[2000..2100]);
    }

    // ---- ContentSource contract suite (HTTP Range backend) ----
    //
    // `title_core::content_stream::contract` を呼んで Read::read 規約準拠を含む
    // 全プロパティを検証する。特に「末尾跨ぎ read で未初期化バイトを valid と
    // 誤申告しない」を境界サイズで catch する。

    fn body_with_pattern(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    fn run_contract_suite(body_len: usize, min_req_size: usize) {
        let body = body_with_pattern(body_len);
        let server = FakeRangeServer::spawn(body.clone());
        let source = HttpRangeSource::probe(&server.url("/contract.bin"))
            .expect("probe")
            .with_min_req_size(min_req_size);
        title_core::content_stream::contract::assert_content_source_contract(&source, &body);
    }

    /// 末尾跨ぎ (file_len % min_req_size != 0) を意図的にテストする。
    /// 上流の `Read::read` が `Ok(buf.len())` を嘘で返すと、未初期化の毒バイト
    /// 0xCC が valid データ扱いされて contract が fail する経路を catch する。
    #[test]
    fn http_range_source_contract_tail_not_aligned_to_min_req() {
        run_contract_suite(/* body_len */ 1023, /* min_req */ 256);
        run_contract_suite(1024 + 1, 256); // 1 byte over a boundary
        run_contract_suite(255, 256); // body smaller than min_req
        run_contract_suite(257, 256); // body just over min_req
    }

    /// `min_req_size` の倍数のケース (従来 fixture 相当)。確認のためここでも回す。
    #[test]
    fn http_range_source_contract_aligned_sizes() {
        run_contract_suite(1024, 256);
        run_contract_suite(4096, 512);
    }

    /// `min_req_size` を file 全長より大きく設定 (1 リクエストで全部取れるケース)。
    #[test]
    fn http_range_source_contract_min_req_larger_than_body() {
        run_contract_suite(500, 64 * 1024);
    }

    // ---- HEAD probe fallback / 非対応サーバー ----

    /// HEAD が 405 (Method Not Allowed) を返した場合、probe は HttpStatus エラーを
    /// 返す。fetcher 経路はこれを見て full fetch にフォールバックする想定。
    /// この test は probe 単体の挙動を assertion する。
    #[test]
    fn probe_fails_when_head_returns_405() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        thread::spawn(move || loop {
            let (mut stream, _) = match listener.accept() {
                Ok(s) => s,
                Err(_) => break,
            };
            // 何を受けても 405 を返す
            thread::spawn(move || {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let resp =
                    b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(resp);
                let _ = stream.shutdown(std::net::Shutdown::Both);
            });
        });

        let err = HttpRangeSource::probe(&format!("http://{addr}/v.mp4")).expect_err("405 must fail");
        match err {
            FetchError::HttpStatus { status, .. } => assert_eq!(status, 405),
            other => panic!("expected HttpStatus(405), got {other:?}"),
        }
    }

    /// Accept-Ranges ヘッダが欠落していたら probe は失敗するべき
    /// (= Range 非対応サーバー扱い)。
    #[test]
    fn probe_fails_when_no_accept_ranges_header() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        thread::spawn(move || loop {
            let (mut stream, _) = match listener.accept() {
                Ok(s) => s,
                Err(_) => break,
            };
            thread::spawn(move || {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                // Content-Length はある、Accept-Ranges は無い
                let resp = b"HTTP/1.1 200 OK\r\nContent-Length: 1024\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(resp);
                let _ = stream.shutdown(std::net::Shutdown::Both);
            });
        });

        let err = HttpRangeSource::probe(&format!("http://{addr}/v.mp4"))
            .expect_err("missing Accept-Ranges must fail probe");
        match err {
            FetchError::HttpError { reason, .. } => {
                assert!(
                    reason.contains("Accept-Ranges"),
                    "error should mention Accept-Ranges, got: {reason}"
                );
            }
            other => panic!("expected HttpError, got {other:?}"),
        }
    }

    /// Accept-Ranges: none (= 明示的に Range 非対応) も probe 失敗扱い。
    #[test]
    fn probe_fails_when_accept_ranges_is_none() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        thread::spawn(move || loop {
            let (mut stream, _) = match listener.accept() {
                Ok(s) => s,
                Err(_) => break,
            };
            thread::spawn(move || {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let resp = b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nAccept-Ranges: none\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(resp);
                let _ = stream.shutdown(std::net::Shutdown::Both);
            });
        });

        let err = HttpRangeSource::probe(&format!("http://{addr}/v.mp4"))
            .expect_err("Accept-Ranges: none must fail probe");
        assert!(matches!(err, FetchError::HttpError { .. }));
    }

    /// Content-Length が 0 のときは EmptyContent を返す。
    #[test]
    fn probe_fails_when_content_length_zero() {
        let server = FakeRangeServer::spawn(Vec::new());
        let err = HttpRangeSource::probe(&server.url("/empty.mp4"))
            .expect_err("zero-length must fail probe");
        assert!(matches!(err, FetchError::EmptyContent(_)));
    }

    #[test]
    fn multiple_opens_are_independent() {
        let body: Vec<u8> = (0..1024).map(|i| (i % 251) as u8).collect();
        let server = FakeRangeServer::spawn(body.clone());
        let source = HttpRangeSource::probe(&server.url("/data.bin")).expect("probe");

        // 2 本の reader が独立した position を持つことを確認。
        let mut r1 = source.open().unwrap();
        let mut r2 = source.open().unwrap();

        let mut b1 = [0u8; 100];
        r1.read_exact(&mut b1).unwrap();
        let mut b2 = [0u8; 100];
        r2.read_exact(&mut b2).unwrap();

        assert_eq!(b1, body[..100]);
        assert_eq!(b2, body[..100]);

        // 続きを読む: 両者とも 100 から再開する
        let mut b1_cont = [0u8; 50];
        r1.read_exact(&mut b1_cont).unwrap();
        assert_eq!(b1_cont, body[100..150]);
    }
}
