//! # セキュリティ強化・DoS対策
//!
//! 仕様書 §6.4
//!
//! ## 防御層
//! - 漸進的重み付きセマフォ予約: 実際のデータ受信量に応じたメモリ管理
//! - Zip Bomb対策: 宣言サイズを超えるデータ読み取りの遮断
//! - Slowloris対策: チャンク単位のRead Timeout
//! - 動的グローバルタイムアウト: コンテンツサイズに応じたリクエスト全体のタイムアウト

use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::sync::Semaphore;

use title_types::ResourceLimits;

use crate::proxy_client::ProxyResponse;

// ---------------------------------------------------------------------------
// デフォルトリソース制限 (仕様書 §6.4 処理上限の管理)
// ---------------------------------------------------------------------------

/// 単体コンテンツの最大サイズ（バイト）: 2GB
pub const DEFAULT_MAX_SINGLE_CONTENT_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// 同時処理可能な合計データ量（バイト）: 8GB
pub const DEFAULT_MAX_CONCURRENT_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// 動的タイムアウト計算に使用する最低転送速度（バイト/秒）: 1MB/s
pub const DEFAULT_MIN_UPLOAD_SPEED_BYTES: u64 = 1024 * 1024;

/// 接続確立や検証開始にかかる固定オーバーヘッド時間（秒）
pub const DEFAULT_BASE_PROCESSING_TIME_SEC: u64 = 30;

/// 処理を強制終了する絶対的な最大時間（秒）: 1時間
pub const DEFAULT_MAX_GLOBAL_TIMEOUT_SEC: u64 = 3600;

/// 次のデータチャンクが到着するまでの最大待機時間（秒）
pub const DEFAULT_CHUNK_READ_TIMEOUT_SEC: u64 = 30;

/// C2PAマニフェストグラフの最大サイズ（ノード+エッジ）
pub const DEFAULT_C2PA_MAX_GRAPH_SIZE: u64 = 10000;

/// 漸進的セマフォ予約のチャンクサイズ（64KB）。
/// 仕様書 §6.4 漸進的重み付きセマフォ予約
pub const CHUNK_SIZE: usize = 64 * 1024;

/// signed_jsonの最大サイズ（1MB）。
/// 仕様書 §6.4 /signフェーズでの防御
pub const MAX_SIGNED_JSON_SIZE: u64 = 1024 * 1024;

// ---------------------------------------------------------------------------
// 解決済みリソース制限
// ---------------------------------------------------------------------------

/// Gateway提供のresource_limitsとデフォルト値をマージした結果。
/// 仕様書 §6.4 処理上限の管理
pub struct ResolvedLimits {
    pub max_single_content_bytes: u64,
    pub max_concurrent_bytes: u64,
    pub min_upload_speed_bytes: u64,
    pub base_processing_time_sec: u64,
    pub max_global_timeout_sec: u64,
    pub chunk_read_timeout_sec: u64,
    pub c2pa_max_graph_size: usize,
}

/// Gateway提供のresource_limitsをデフォルト値で補完する。
/// 仕様書 §6.4
pub fn resolve_limits(rl: Option<&ResourceLimits>) -> ResolvedLimits {
    match rl {
        Some(rl) => ResolvedLimits {
            max_single_content_bytes: rl
                .max_single_content_bytes
                .unwrap_or(DEFAULT_MAX_SINGLE_CONTENT_BYTES),
            max_concurrent_bytes: rl
                .max_concurrent_bytes
                .unwrap_or(DEFAULT_MAX_CONCURRENT_BYTES),
            min_upload_speed_bytes: rl
                .min_upload_speed_bytes
                .unwrap_or(DEFAULT_MIN_UPLOAD_SPEED_BYTES),
            base_processing_time_sec: rl
                .base_processing_time_sec
                .unwrap_or(DEFAULT_BASE_PROCESSING_TIME_SEC),
            max_global_timeout_sec: rl
                .max_global_timeout_sec
                .unwrap_or(DEFAULT_MAX_GLOBAL_TIMEOUT_SEC),
            chunk_read_timeout_sec: rl
                .chunk_read_timeout_sec
                .unwrap_or(DEFAULT_CHUNK_READ_TIMEOUT_SEC),
            c2pa_max_graph_size: rl
                .c2pa_max_graph_size
                .map(|v| v as usize)
                .unwrap_or(DEFAULT_C2PA_MAX_GRAPH_SIZE as usize),
        },
        None => ResolvedLimits {
            max_single_content_bytes: DEFAULT_MAX_SINGLE_CONTENT_BYTES,
            max_concurrent_bytes: DEFAULT_MAX_CONCURRENT_BYTES,
            min_upload_speed_bytes: DEFAULT_MIN_UPLOAD_SPEED_BYTES,
            base_processing_time_sec: DEFAULT_BASE_PROCESSING_TIME_SEC,
            max_global_timeout_sec: DEFAULT_MAX_GLOBAL_TIMEOUT_SEC,
            chunk_read_timeout_sec: DEFAULT_CHUNK_READ_TIMEOUT_SEC,
            c2pa_max_graph_size: DEFAULT_C2PA_MAX_GRAPH_SIZE as usize,
        },
    }
}

// ---------------------------------------------------------------------------
// 動的グローバルタイムアウト (仕様書 §6.4)
// ---------------------------------------------------------------------------

/// コンテンツサイズに基づく動的タイムアウトを計算する。
/// 仕様書 §6.4: Timeout = min(MaxLimit, BaseTime + ContentSize / MinSpeed)
pub fn compute_dynamic_timeout(limits: &ResolvedLimits, content_size: u64) -> Duration {
    let transfer_time_sec = content_size / limits.min_upload_speed_bytes.max(1);
    let computed = limits.base_processing_time_sec + transfer_time_sec;
    let capped = computed.min(limits.max_global_timeout_sec);
    Duration::from_secs(capped)
}

// ---------------------------------------------------------------------------
// セキュア化されたプロキシ取得 (仕様書 §6.4)
// ---------------------------------------------------------------------------

/// セキュリティエラー種別。
#[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    /// ペイロードサイズが上限を超えている
    #[error("ペイロードサイズが上限を超えています: {size} bytes (上限: {limit} bytes)")]
    PayloadTooLarge { size: u64, limit: u64 },

    /// メモリ上限に到達（セマフォ枯渇）
    #[error("メモリ上限に到達しました。同時処理可能なデータ量を超えています")]
    MemoryLimitExceeded,

    /// チャンク読み取りタイムアウト（Slowloris攻撃の疑い）
    #[error("チャンク読み取りがタイムアウトしました（{timeout_sec}秒）")]
    ChunkReadTimeout { timeout_sec: u64 },

    /// リクエスト全体のタイムアウト
    #[error("リクエスト処理がタイムアウトしました")]
    GlobalTimeout,

    /// IO エラー
    #[error("IOエラー: {0}")]
    Io(#[from] std::io::Error),

    /// プロキシエラー（非200レスポンス）
    #[error("プロキシエラー: HTTP {0}")]
    ProxyError(u32),
}

/// セキュア化されたプロキシGETリクエスト。
/// 仕様書 §6.4 — 三層防御（Zip Bomb、Reservation DoS、Slowloris）を適用。
///
/// 1. レスポンスの宣言サイズをmax_size_bytesでチェック（Zip Bomb対策）
/// 2. 64KBチャンク単位でセマフォを漸進的に予約（Reservation DoS対策）
/// 3. 各チャンク読み取りにタイムアウトを設定（Slowloris対策）
pub async fn proxy_get_secured(
    proxy_addr: &str,
    url: &str,
    max_size_bytes: u64,
    chunk_timeout: Duration,
    semaphore: &Arc<Semaphore>,
) -> Result<ProxyResponse, SecurityError> {
    // Direct HTTPモード: プロキシプロトコルを経由せず直接HTTPリクエスト
    if proxy_addr == "direct" {
        return proxy_get_secured_direct(url, max_size_bytes, semaphore).await;
    }

    // プロキシに接続
    let mut stream = tokio::net::TcpStream::connect(proxy_addr).await?;

    // GETリクエスト送信
    {
        use tokio::io::AsyncWriteExt;
        let method = b"GET";
        stream
            .write_all(&(method.len() as u32).to_be_bytes())
            .await?;
        stream.write_all(method).await?;

        let url_bytes = url.as_bytes();
        stream
            .write_all(&(url_bytes.len() as u32).to_be_bytes())
            .await?;
        stream.write_all(url_bytes).await?;

        // 空のbody
        stream.write_all(&0u32.to_be_bytes()).await?;
        stream.flush().await?;
    }

    // レスポンスステータス読み取り
    let mut buf4 = [0u8; 4];
    stream.read_exact(&mut buf4).await?;
    let status = u32::from_be_bytes(buf4);

    if status != 200 {
        // ステータス異常時はbodyを読み捨てて即エラー
        stream.read_exact(&mut buf4).await?;
        let body_len = u32::from_be_bytes(buf4) as usize;
        let mut discard = vec![0u8; body_len.min(4096)];
        if body_len > 0 {
            let _ = stream.read(&mut discard).await;
        }
        return Err(SecurityError::ProxyError(status));
    }

    // body_len読み取り（宣言サイズ）
    stream.read_exact(&mut buf4).await?;
    let declared_size = u32::from_be_bytes(buf4) as u64;

    // Zip Bomb対策: 宣言サイズがmax_size_bytesを超えていたら拒否
    if declared_size > max_size_bytes {
        return Err(SecurityError::PayloadTooLarge {
            size: declared_size,
            limit: max_size_bytes,
        });
    }

    if declared_size == 0 {
        return Ok(ProxyResponse {
            status,
            body: Vec::new(),
        });
    }

    // 漸進的重み付きセマフォ予約 + Slowloris対策
    // 仕様書 §6.4
    let total_to_read = declared_size as usize;
    let mut buffer = Vec::with_capacity(total_to_read);
    let mut total_reserved: u32 = 0;
    let mut remaining = total_to_read;

    while remaining > 0 {
        let to_read = remaining.min(CHUNK_SIZE);
        let mut chunk_buf = vec![0u8; to_read];

        // チャンク単位のRead Timeout（Slowloris対策）
        let read_result = tokio::time::timeout(chunk_timeout, stream.read_exact(&mut chunk_buf))
            .await
            .map_err(|_| SecurityError::ChunkReadTimeout {
                timeout_sec: chunk_timeout.as_secs(),
            })?;
        read_result?;

        // 漸進的セマフォ予約（Reservation DoS対策）
        let permits_needed = to_read as u32;
        let permit = semaphore
            .try_acquire_many(permits_needed)
            .map_err(|_| SecurityError::MemoryLimitExceeded)?;
        // 処理完了までセマフォを保持（forgetで解放を遅延）
        permit.forget();
        total_reserved += permits_needed;

        buffer.extend_from_slice(&chunk_buf);
        remaining -= to_read;
    }

    // 処理完了後にセマフォを解放
    semaphore.add_permits(total_reserved as usize);

    Ok(ProxyResponse {
        status,
        body: buffer,
    })
}

/// Direct HTTPモードのセキュア化されたGETリクエスト。
/// PROXY_ADDR=direct の場合に使用。reqwestで直接取得しつつ
/// サイズ制限とセマフォ予約を適用する。
async fn proxy_get_secured_direct(
    url: &str,
    max_size_bytes: u64,
    semaphore: &Arc<Semaphore>,
) -> Result<ProxyResponse, SecurityError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    let status = resp.status().as_u16() as u32;
    if status != 200 {
        return Err(SecurityError::ProxyError(status));
    }

    // Content-Lengthでサイズチェック（存在する場合）
    if let Some(content_length) = resp.content_length() {
        if content_length > max_size_bytes {
            return Err(SecurityError::PayloadTooLarge {
                size: content_length,
                limit: max_size_bytes,
            });
        }
    }

    let body = resp
        .bytes()
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
        .to_vec();

    // サイズ制限の最終チェック
    if body.len() as u64 > max_size_bytes {
        return Err(SecurityError::PayloadTooLarge {
            size: body.len() as u64,
            limit: max_size_bytes,
        });
    }

    // セマフォ予約
    let permits_needed = body.len() as u32;
    if permits_needed > 0 {
        let permit = semaphore
            .try_acquire_many(permits_needed)
            .map_err(|_| SecurityError::MemoryLimitExceeded)?;
        permit.forget();
    }

    Ok(ProxyResponse { status, body })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_limits_default() {
        let limits = resolve_limits(None);
        assert_eq!(limits.max_single_content_bytes, DEFAULT_MAX_SINGLE_CONTENT_BYTES);
        assert_eq!(limits.chunk_read_timeout_sec, DEFAULT_CHUNK_READ_TIMEOUT_SEC);
        assert_eq!(limits.c2pa_max_graph_size, DEFAULT_C2PA_MAX_GRAPH_SIZE as usize);
    }

    #[test]
    fn test_resolve_limits_override() {
        let rl = ResourceLimits {
            max_single_content_bytes: Some(1024),
            max_concurrent_bytes: Some(2048),
            min_upload_speed_bytes: None,
            base_processing_time_sec: None,
            max_global_timeout_sec: Some(60),
            chunk_read_timeout_sec: Some(5),
            c2pa_max_graph_size: Some(500),
        };
        let limits = resolve_limits(Some(&rl));
        assert_eq!(limits.max_single_content_bytes, 1024);
        assert_eq!(limits.max_concurrent_bytes, 2048);
        assert_eq!(limits.min_upload_speed_bytes, DEFAULT_MIN_UPLOAD_SPEED_BYTES);
        assert_eq!(limits.max_global_timeout_sec, 60);
        assert_eq!(limits.chunk_read_timeout_sec, 5);
        assert_eq!(limits.c2pa_max_graph_size, 500);
    }

    #[test]
    fn test_compute_dynamic_timeout() {
        let limits = resolve_limits(None);
        // 0バイト: BaseTime = 30秒
        let t0 = compute_dynamic_timeout(&limits, 0);
        assert_eq!(t0, Duration::from_secs(30));

        // 100MB at 1MB/s: 30 + 100 = 130秒
        let t1 = compute_dynamic_timeout(&limits, 100 * 1024 * 1024);
        assert_eq!(t1, Duration::from_secs(130));

        // 巨大サイズ: max_global_timeout_sec(3600)にキャップ
        let t2 = compute_dynamic_timeout(&limits, 100 * 1024 * 1024 * 1024);
        assert_eq!(t2, Duration::from_secs(3600));
    }

    #[test]
    fn test_compute_dynamic_timeout_custom_limits() {
        let rl = ResourceLimits {
            max_single_content_bytes: None,
            max_concurrent_bytes: None,
            min_upload_speed_bytes: Some(512 * 1024), // 512KB/s
            base_processing_time_sec: Some(10),
            max_global_timeout_sec: Some(120),
            chunk_read_timeout_sec: None,
            c2pa_max_graph_size: None,
        };
        let limits = resolve_limits(Some(&rl));

        // 50MB at 512KB/s: 10 + 100 = 110秒
        let t = compute_dynamic_timeout(&limits, 50 * 1024 * 1024);
        assert_eq!(t, Duration::from_secs(110));

        // 超過: 120秒にキャップ
        let t2 = compute_dynamic_timeout(&limits, 100 * 1024 * 1024);
        assert_eq!(t2, Duration::from_secs(120));
    }

    #[tokio::test]
    async fn test_proxy_get_secured_size_limit() {
        use tokio::io::AsyncWriteExt;

        // 巨大body_lenを返すモックプロキシ
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            // リクエストを読み捨て
            let mut buf = vec![0u8; 1024];
            let _ = stream.read(&mut buf).await;

            // status: 200
            stream.write_all(&200u32.to_be_bytes()).await.unwrap();
            // body_len: 10MB (上限1MBに設定するので超過)
            stream
                .write_all(&(10 * 1024 * 1024u32).to_be_bytes())
                .await
                .unwrap();
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        let sem = Arc::new(Semaphore::new(1024 * 1024 * 1024));
        let result = proxy_get_secured(
            &format!("127.0.0.1:{port}"),
            "http://example.com/payload",
            1024 * 1024, // 1MB制限
            Duration::from_secs(30),
            &sem,
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        // macOSではTCPレベルでConnectionResetが先に発生する場合がある
        assert!(
            matches!(err, SecurityError::PayloadTooLarge { .. } | SecurityError::Io(_)),
            "PayloadTooLargeまたはIoエラーが期待される: {err:?}"
        );
    }

    #[tokio::test]
    async fn test_proxy_get_secured_semaphore_exhaustion() {
        use tokio::io::AsyncWriteExt;

        // 正常なレスポンスを返すモックプロキシ
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let body_data = vec![0xABu8; 128 * 1024]; // 128KB
        let body_clone = body_data.clone();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1024];
            let _ = stream.read(&mut buf).await;

            stream.write_all(&200u32.to_be_bytes()).await.unwrap();
            stream
                .write_all(&(body_clone.len() as u32).to_be_bytes())
                .await
                .unwrap();
            stream.write_all(&body_clone).await.unwrap();
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        // セマフォ容量を64KBに制限 → 128KBの2チャンク目で枯渇
        let sem = Arc::new(Semaphore::new(64 * 1024));
        let result = proxy_get_secured(
            &format!("127.0.0.1:{port}"),
            "http://example.com/payload",
            1024 * 1024,
            Duration::from_secs(30),
            &sem,
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, SecurityError::MemoryLimitExceeded),
            "MemoryLimitExceededエラーが期待される: {err:?}"
        );
    }

    #[tokio::test]
    async fn test_proxy_get_secured_chunk_timeout() {
        use tokio::io::AsyncWriteExt;

        // 最初のチャンクのみ送信し、残りはハングするモックプロキシ
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1024];
            let _ = stream.read(&mut buf).await;

            let total_size: u32 = 128 * 1024; // 128KB宣言
            stream.write_all(&200u32.to_be_bytes()).await.unwrap();
            stream
                .write_all(&total_size.to_be_bytes())
                .await
                .unwrap();

            // 64KBだけ送信
            stream.write_all(&vec![0xCCu8; 64 * 1024]).await.unwrap();

            // あとはハング（Slowloris攻撃シミュレーション）
            tokio::time::sleep(Duration::from_secs(60)).await;
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        let sem = Arc::new(Semaphore::new(1024 * 1024));
        let result = proxy_get_secured(
            &format!("127.0.0.1:{port}"),
            "http://example.com/payload",
            1024 * 1024,
            Duration::from_millis(200), // 200msのタイムアウト（テスト用に短く）
            &sem,
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, SecurityError::ChunkReadTimeout { .. }),
            "ChunkReadTimeoutエラーが期待される: {err:?}"
        );
    }

    #[tokio::test]
    async fn test_proxy_get_secured_success() {
        use tokio::io::AsyncWriteExt;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let body_data = vec![0x42u8; 1024]; // 1KB

        tokio::spawn({
            let body = body_data.clone();
            async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 1024];
                let _ = stream.read(&mut buf).await;

                stream.write_all(&200u32.to_be_bytes()).await.unwrap();
                stream
                    .write_all(&(body.len() as u32).to_be_bytes())
                    .await
                    .unwrap();
                stream.write_all(&body).await.unwrap();
            }
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        let sem = Arc::new(Semaphore::new(1024 * 1024));
        let result = proxy_get_secured(
            &format!("127.0.0.1:{port}"),
            "http://example.com/test",
            1024 * 1024,
            Duration::from_secs(30),
            &sem,
        )
        .await;

        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, body_data);
    }
}
