//! # Length-prefixed プロトコル
//!
//! 仕様書 §6.4
//!
//! TEE ↔ Proxy間の通信に使用するlength-prefixedバイナリプロトコル。
//! `prototype/enclave-c2pa/proxy/` と同一仕様。
//!
//! ## TEE → Proxy
//! ```text
//! [4B: method_len][method][4B: url_len][url][4B: body_len][body]
//! ```
//!
//! ## Proxy → TEE
//! ```text
//! [4B: status_code][4B: body_len][body]
//! ```

// ─────────────────────────────────────────────
// 非同期I/O（TCP経路: macOS / テスト用）
// ─────────────────────────────────────────────

/// ストリームから4バイトビッグエンディアンのu32を読み取る。
/// 仕様書 §6.4
#[cfg(any(not(target_os = "linux"), test))]
pub async fn read_u32_async<R: tokio::io::AsyncRead + Unpin>(r: &mut R) -> std::io::Result<u32> {
    use tokio::io::AsyncReadExt;
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf).await?;
    Ok(u32::from_be_bytes(buf))
}

/// length-prefixed文字列を読み取る。
/// 仕様書 §6.4
#[cfg(any(not(target_os = "linux"), test))]
pub async fn read_string_async<R: tokio::io::AsyncRead + Unpin>(r: &mut R) -> std::io::Result<String> {
    use tokio::io::AsyncReadExt;
    let len = read_u32_async(r).await? as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    String::from_utf8(buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// length-prefixedバイト列を読み取る。
/// 仕様書 §6.4
#[cfg(any(not(target_os = "linux"), test))]
pub async fn read_bytes_async<R: tokio::io::AsyncRead + Unpin>(r: &mut R) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let len = read_u32_async(r).await? as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    Ok(buf)
}

/// プロキシレスポンスを書き込む: [4B: status][4B: body_len][body]
/// 仕様書 §6.4
#[cfg(any(not(target_os = "linux"), test))]
pub async fn write_response_async<W: tokio::io::AsyncWrite + Unpin>(
    w: &mut W,
    status: u32,
    body: &[u8],
) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    w.write_all(&status.to_be_bytes()).await?;
    w.write_all(&(body.len() as u32).to_be_bytes()).await?;
    w.write_all(body).await?;
    w.flush().await?;
    Ok(())
}

// ─────────────────────────────────────────────
// 同期I/O（Linux vsock経路）
// ─────────────────────────────────────────────

/// ストリームから4バイトビッグエンディアンのu32を同期的に読み取る。
/// 仕様書 §6.4
#[cfg(target_os = "linux")]
pub fn read_u32_sync(r: &mut impl std::io::Read) -> std::io::Result<u32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_be_bytes(buf))
}

/// length-prefixed文字列を同期的に読み取る。
/// 仕様書 §6.4
#[cfg(target_os = "linux")]
pub fn read_string_sync(r: &mut impl std::io::Read) -> std::io::Result<String> {
    let len = read_u32_sync(r)? as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    String::from_utf8(buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// length-prefixedバイト列を同期的に読み取る。
/// 仕様書 §6.4
#[cfg(target_os = "linux")]
pub fn read_bytes_sync(r: &mut impl std::io::Read) -> std::io::Result<Vec<u8>> {
    let len = read_u32_sync(r)? as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// プロキシレスポンスを同期的に書き込む: [4B: status][4B: body_len][body]
/// 仕様書 §6.4
#[cfg(target_os = "linux")]
pub fn write_response_sync(
    w: &mut impl std::io::Write,
    status: u32,
    body: &[u8],
) -> std::io::Result<()> {
    use std::io::Write;
    w.write_all(&status.to_be_bytes())?;
    w.write_all(&(body.len() as u32).to_be_bytes())?;
    w.write_all(body)?;
    w.flush()?;
    Ok(())
}
