// SPDX-License-Identifier: Apache-2.0

//! # ContentStream and ContentSource
//!
//! Spec §4.3 — メモリ管理 (Range Request パターン)
//!
//! Processor がコンテンツを読むときの抽象。`&[u8]` ではなく `Read + Seek`
//! ベースのストリームで扱うことで、50 GB 級の動画でもメモリに全展開せず
//! HTTP Range Request 経由で必要な部分だけを読み込める。
//!
//! ## 二層構造
//!
//! - [`ContentStream`] — 個別の reader (`Read + Seek + Send`) を表すトレイト
//!   alias。`c2pa::Reader::with_stream` が要求する `Read + Seek + MaybeSend`
//!   と互換。
//! - [`ContentSource`] — reader を「何度でも新しく開ける」factory。
//!   processor を並列実行するとき、各 processor は `source.open()` で
//!   独立した reader を取得する。Range Request 経由なら新しい HTTP コネクション、
//!   in-memory なら同じバイト列を指す新しい `Cursor` が返る。
//!
//! ## なぜ factory か
//!
//! Processor は同一コンテンツを別々に走査する (c2pa-verify は JUMBF box、
//! image-pdq はピクセルデータ)。reader を共有すると seek 位置が衝突するため、
//! processor ごとに独立した reader が必要。`ContentSource::open()` で
//! 新しい reader を生成する。
//!
//! Range Request reader は new HTTP connection を張るが、同一 URL を指す
//! ので processor から見たバイト列は同一。in-memory reader (`InMemorySource`)
//! は `Arc<Vec<u8>>` を共有し、`Cursor` の position だけが独立する。

use std::io::{Cursor, Read, Seek};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// ContentStream
// ---------------------------------------------------------------------------

/// コンテンツ読み込み用のストリーム抽象。
///
/// `Read + Seek + Send` を要求する。`Send` は (a) `c2pa::Reader::with_stream`
/// の `MaybeSend` 制約と (b) processor を別スレッドに move する将来の並列化に
/// 備えるため。`Sync` は要求しない (`&mut dyn ContentStream` を別スレッドに
/// 同時に貸すユースケースはない)。
pub trait ContentStream: Read + Seek + Send {}

impl<T: Read + Seek + Send + ?Sized> ContentStream for T {}

// ---------------------------------------------------------------------------
// ContentSource
// ---------------------------------------------------------------------------

/// Reader factory。同一コンテンツに対する独立した reader を何度でも開ける。
///
/// 仕様 §4.3 のメモリパターンに対応するための抽象:
/// - In-memory: `Cursor<Arc<Vec<u8>>>` を返す (cheap, 既に取得済み)
/// - Range Request: `HttpReader` を返す (各 open() で新規 HTTP 接続)
/// - Proxy Range: `ProxyRangeReader` を返す (vsock 経由の新規接続)
///
/// 並列 processor 実行時、各 processor は `source.open()?` で自分専用の
/// reader を取得する。
pub trait ContentSource: Send + Sync {
    /// 新しい reader を開く。
    ///
    /// 同一 `ContentSource` から複数回 `open()` した場合、得られる reader は
    /// すべて同じバイト列を表す (position は独立)。失敗した場合は
    /// network/I/O エラーを返す。
    fn open(&self) -> std::io::Result<Box<dyn ContentStream>>;

    /// 既知のコンテンツ「論理サイズ」 (バイト)。ファイル全体の長さ。
    ///
    /// Range Request 経由なら HEAD 取得済みのサイズ、in-memory なら
    /// `bytes.len()`。サイズ不明の場合は `None`。
    ///
    /// **用途**: グローバルタイムアウト計算 (`compute_global_timeout`) で
    /// 大きなファイルほど長い timeout を割り当てるため。
    /// **メモリ予約には使わない** — それは [`Self::peak_memory_hint`] の責務。
    fn size_hint(&self) -> Option<u64> {
        None
    }

    /// このソースを 1 本 open() した reader が同時に保持する**ピークメモリ**
    /// の見込み (バイト)。仕様 §4.3 のメモリパターン:
    ///
    /// - In-memory: `bytes.len()` (= `size_hint`)。すでに heap にロード済み。
    /// - HTTP Range / Proxy Range: reader 内部の Range Request バッファのみ
    ///   (典型的に 64 KB〜数 MB)。50 GB のファイルでもメモリは 64 KB に
    ///   抑えられる。
    /// - サイズ不明の場合は `None`。
    ///
    /// `ResourcePool::extend` はこの値を使って漸進予約する。`size_hint`
    /// (論理サイズ) を間違えて extend に使うと、Range Request 経由の
    /// 50 GB ファイルが admission_limit で reject されてしまい、§4.3 の
    /// ストリーミング前提が崩れる。
    fn peak_memory_hint(&self) -> Option<u64> {
        // デフォルトは保守的に `size_hint` を返す (in-memory 想定)。
        // streaming source は override する。
        self.size_hint()
    }
}

// ---------------------------------------------------------------------------
// InMemorySource — Vec<u8> backed implementation
// ---------------------------------------------------------------------------

/// メモリ上のバイト列を `ContentSource` として公開する実装。
///
/// 小ファイル (JPEG/PNG)、サイドカーマニフェスト、復号後のペイロード、
/// テストフィクスチャなど「既にメモリにあるバイト列」をストリーミング処理
/// パイプラインに乗せるためのアダプタ。
///
/// 内部は `Arc<[u8]>` で共有され、`open()` ごとに新しい `Cursor` を返す。
/// バイト列のコピーは発生しない (`Arc` クローンのみ)。`Arc<Vec<u8>>` ではなく
/// `Arc<[u8]>` を使うのは、後者だけが `AsRef<[u8]>` を実装するため
/// (`std::io::Cursor<T>` は `T: AsRef<[u8]>` のときに `Read` を実装する)。
#[derive(Clone)]
pub struct InMemorySource {
    bytes: Arc<[u8]>,
}

impl InMemorySource {
    /// 既存の `Vec<u8>` を共有してソースを構築する。`Vec` の所有権は
    /// 引き取り、`Arc<[u8]>` に変換する。
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes: Arc::from(bytes),
        }
    }

    /// 既に `Arc<[u8]>` で共有されているバイト列からソースを構築する。
    pub fn from_arc(bytes: Arc<[u8]>) -> Self {
        Self { bytes }
    }

    /// 内部のバイト列への参照を返す。テストや診断用途。
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl ContentSource for InMemorySource {
    fn open(&self) -> std::io::Result<Box<dyn ContentStream>> {
        Ok(Box::new(Cursor::new(Arc::clone(&self.bytes))))
    }

    fn size_hint(&self) -> Option<u64> {
        Some(self.bytes.len() as u64)
    }
}

// ---------------------------------------------------------------------------
// Contract test helpers (always-public, no production impact)
// ---------------------------------------------------------------------------

/// [`ContentSource`] 実装の規約テストヘルパ。`title-tee` などの外部 crate からも
/// 呼べるよう `pub` で公開している (`#[doc(hidden)]` で API docs からは隠す)。
///
/// 仕様 §4.3 のメモリパターンと Rust の `std::io::Read` 規約を構造的に検証する。
/// 監査でしか発見できないバグ (例: 上流 http-range-client crate の `Read::read`
/// が `Ok(length)` を返すが実際の write は `bytes.len()` < length しかないケース)
/// を、ContentSource 実装ごとに同じヘルパで catch するための共通テスト。
#[doc(hidden)]
pub mod contract {
    use super::ContentSource;
    use std::io::{Read, Seek, SeekFrom};

    /// `ContentSource` 実装が満たすべきすべての規約を検証する。
    ///
    /// # Arguments
    /// * `source` — 検証対象の ContentSource
    /// * `expected_bytes` — `source` が返すべき完全なバイト列 (テスト fixture)
    ///
    /// 検証項目:
    /// 1. `size_hint()` が `expected_bytes.len()` と一致
    /// 2. `open()` で得た reader を `read_to_end` した結果が `expected_bytes` と一致
    /// 3. `seek(SeekFrom::Start(i)) → read(buf)` が `expected_bytes[i..i+n]` と一致
    /// 4. **D-14 検出**: `read(&mut [0xCC; N])` で N が file 末尾を跨ぐとき、
    ///    返り値 `K < N` であり、`buf[K..]` は 0xCC のまま (= read が touch していない)
    /// 5. EOF を超えた `read` は `Ok(0)` を返す
    /// 6. 複数 `open()` の reader が独立した position を持つ
    pub fn assert_content_source_contract(source: &dyn ContentSource, expected_bytes: &[u8]) {
        check_size_hint(source, expected_bytes);
        check_read_to_end(source, expected_bytes);
        check_seek_then_read(source, expected_bytes);
        check_read_does_not_lie_about_byte_count(source, expected_bytes);
        check_read_past_eof_returns_zero(source, expected_bytes);
        check_multiple_opens_are_independent(source, expected_bytes);
    }

    fn check_size_hint(source: &dyn ContentSource, expected: &[u8]) {
        assert_eq!(
            source.size_hint(),
            Some(expected.len() as u64),
            "size_hint mismatch"
        );
    }

    fn check_read_to_end(source: &dyn ContentSource, expected: &[u8]) {
        let mut reader = source.open().expect("open #1");
        let mut got = Vec::new();
        reader.read_to_end(&mut got).expect("read_to_end");
        assert_eq!(got, expected, "read_to_end yielded wrong bytes");
    }

    fn check_seek_then_read(source: &dyn ContentSource, expected: &[u8]) {
        if expected.len() < 2 {
            return;
        }
        let mut reader = source.open().expect("open #2");
        let offset = expected.len() / 2;
        let want_len = (expected.len() - offset).min(64);
        reader
            .seek(SeekFrom::Start(offset as u64))
            .expect("seek to middle");
        let mut buf = vec![0u8; want_len];
        reader.read_exact(&mut buf).expect("read_exact from middle");
        assert_eq!(
            buf,
            &expected[offset..offset + want_len],
            "seek-then-read slice mismatch"
        );
    }

    /// D-14: `read(&mut buf) -> Ok(N)` が返ったとき、`buf[..N]` のみが実書き込み
    /// された保証を検証する。末尾を跨ぐ場合に N < buf.len() となるはずで、
    /// `buf[N..]` は呼び出し側が事前に置いた毒パターン (0xCC) のままであるべき。
    ///
    /// http-range-client の `Read::read` バグ (`Ok(length)` 固定返却) を踏むと
    /// このテストは fail する: 呼び出し側は `Ok(length)` を信じて `buf[..length]`
    /// を valid と見なすが、buf[K..length] は 0xCC のままなので一致しない。
    fn check_read_does_not_lie_about_byte_count(source: &dyn ContentSource, expected: &[u8]) {
        if expected.is_empty() {
            return;
        }
        let mut reader = source.open().expect("open #3");

        // 末尾の 5 バイト前に seek し、10 バイト要求する (5 バイトは EOF を超える)。
        let tail_offset = expected.len().saturating_sub(5);
        let tail_remaining = expected.len() - tail_offset; // 5 or less
        reader
            .seek(SeekFrom::Start(tail_offset as u64))
            .expect("seek to tail");

        let probe_len = tail_remaining + 5; // EOF を 5 byte 跨ぐ
        let mut buf = vec![0xCCu8; probe_len];

        // read を必要なだけ繰り返して valid な部分を収集する。
        // Read 規約: `Ok(0)` で EOF。1 回の read で全部取れない実装でも OK。
        let mut total_read = 0;
        loop {
            match reader.read(&mut buf[total_read..]) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    // 規約検証: read が n を返したら buf[total_read..total_read+n]
                    // のみが実書き込みされ、それ以降は 0xCC のまま。
                    // ここで毒パターン chunk が壊れていたら read が嘘をついている。
                    let untouched_start = total_read + n;
                    for (i, &b) in buf[untouched_start..].iter().enumerate() {
                        assert_eq!(
                            b,
                            0xCC,
                            "Read::read returned Ok({}) but wrote into buf[{}], \
                             which should have been untouched (poison byte 0xCC). \
                             This is the http-range-client D-14 bug or equivalent: \
                             returning `Ok(buf.len())` without filling the whole buffer. \
                             At byte offset {} from poison region start.",
                            n,
                            untouched_start + i,
                            i
                        );
                    }
                    total_read += n;
                    if total_read >= probe_len {
                        break;
                    }
                }
                Err(e) => panic!("read failed: {e}"),
            }
        }

        // 実際に読めたバイト数は `tail_remaining` (EOF までの残り) に一致するはず。
        assert_eq!(
            total_read, tail_remaining,
            "expected to read exactly {} bytes (file remaining), got {}",
            tail_remaining, total_read
        );
        assert_eq!(
            &buf[..tail_remaining],
            &expected[tail_offset..],
            "tail bytes content mismatch"
        );
    }

    fn check_read_past_eof_returns_zero(source: &dyn ContentSource, expected: &[u8]) {
        let mut reader = source.open().expect("open #4");
        reader
            .seek(SeekFrom::Start(expected.len() as u64))
            .expect("seek to EOF");
        let mut buf = [0u8; 16];
        let n = reader.read(&mut buf).expect("read at EOF");
        assert_eq!(n, 0, "read at EOF must return Ok(0)");
    }

    fn check_multiple_opens_are_independent(source: &dyn ContentSource, expected: &[u8]) {
        if expected.len() < 4 {
            return;
        }
        let mut r1 = source.open().expect("open r1");
        let mut r2 = source.open().expect("open r2");

        // r1 で 2 byte 進める
        let mut b1 = [0u8; 2];
        r1.read_exact(&mut b1).expect("r1 read 2");

        // r2 は依然先頭から
        let mut b2 = [0u8; 2];
        r2.read_exact(&mut b2).expect("r2 read 2");
        assert_eq!(b1, b2, "two readers should see same head bytes");
        assert_eq!(&b1, &expected[..2]);

        // r1 の続き
        let mut b1_cont = [0u8; 2];
        r1.read_exact(&mut b1_cont).expect("r1 cont");
        assert_eq!(&b1_cont, &expected[2..4]);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Seek, SeekFrom};

    #[test]
    fn in_memory_source_open_returns_independent_readers() {
        let source = InMemorySource::new(b"hello world".to_vec());

        let mut r1 = source.open().unwrap();
        let mut buf1 = [0u8; 5];
        r1.read_exact(&mut buf1).unwrap();
        assert_eq!(&buf1, b"hello");

        // 2 本目の reader は position 0 から始まる
        let mut r2 = source.open().unwrap();
        let mut buf2 = [0u8; 5];
        r2.read_exact(&mut buf2).unwrap();
        assert_eq!(&buf2, b"hello");

        // 1 本目はまだ続きから読める
        let mut buf3 = [0u8; 6];
        r1.read_exact(&mut buf3).unwrap();
        assert_eq!(&buf3, b" world");
    }

    #[test]
    fn in_memory_source_seek_works() {
        let source = InMemorySource::new(b"0123456789".to_vec());
        let mut r = source.open().unwrap();
        r.seek(SeekFrom::Start(5)).unwrap();
        let mut buf = [0u8; 5];
        r.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"56789");
    }

    #[test]
    fn in_memory_source_size_hint_matches_bytes() {
        let source = InMemorySource::new(vec![0u8; 1234]);
        assert_eq!(source.size_hint(), Some(1234));
    }

    #[test]
    fn in_memory_source_empty() {
        let source = InMemorySource::new(Vec::new());
        let mut r = source.open().unwrap();
        let mut buf = [0u8; 1];
        let n = r.read(&mut buf).unwrap();
        assert_eq!(n, 0);
        assert_eq!(source.size_hint(), Some(0));
    }

    #[test]
    fn content_stream_blanket_impl_covers_cursor() {
        // Cursor<Vec<u8>> is Read + Seek + Send → implements ContentStream.
        fn assert_content_stream<T: ContentStream + ?Sized>(_: &T) {}
        let cursor = Cursor::new(vec![1u8, 2, 3]);
        assert_content_stream(&cursor);
    }

    #[test]
    fn content_source_is_object_safe() {
        let _source: Box<dyn ContentSource> = Box::new(InMemorySource::new(vec![1, 2, 3]));
    }

    // ---- ContentSource contract suite (in-memory backend) ----
    //
    // contract::assert_content_source_contract で in-memory 実装が全規約を
    // 満たすことを境界条件込みで検証する。同じヘルパを Range Request 系
    // (HttpRangeSource / ProxyRangeSource) のテストにも掛け、実装ごとの
    // ばらつきを構造的に防ぐ。

    /// 様々な長さで contract を回す。境界条件 (1 byte / 7 byte / 256 byte 等
    /// 末尾跨ぎを意図したサイズ) を網羅する。
    #[test]
    fn in_memory_source_contract_various_sizes() {
        for &len in &[1usize, 7, 64, 100, 255, 256, 257, 1023, 1024, 1025] {
            let bytes: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            let source = InMemorySource::new(bytes.clone());
            super::contract::assert_content_source_contract(&source, &bytes);
        }
    }

    /// 空入力の contract: read_to_end は空、seek/read 系はノーオペで EOF。
    #[test]
    fn in_memory_source_contract_empty() {
        let source = InMemorySource::new(Vec::new());
        // size_hint と read_to_end / EOF だけ確認 (seek_then_read 系は len < 2 でスキップ)
        super::contract::assert_content_source_contract(&source, &[]);
    }
}
