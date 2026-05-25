// SPDX-License-Identifier: Apache-2.0

//! # FragmentedSource — CMAF fragmented MP4 用 lazy ContentSource
//!
//! 仕様 §4.3「フラグメント」のメモリパターン (ピーク = init + フラグメント 1 個分 +
//! Reader 内部状態) を構造的に実現する。
//!
//! ## 仕組み
//!
//! - `init.mp4` は数 KB〜数十 KB なので [`std::sync::Arc<[u8]>`] で常駐させる。
//! - 各 fragment は [`title_core::ContentSource`] (典型は `HttpRangeSource` か
//!   `InMemorySource`) を持つだけで、実バイト列はロード前。
//! - `FragmentedSource::open()` は [`FragmentedReader`] を返す。
//!   `FragmentedReader` は `Read + Seek` を実装し、論理位置 = `init + Σ fragments`
//!   の連結バイト列として振る舞う。
//! - 実体は **直近 1 個の fragment しかメモリに保持しない**。`read` / `seek` で
//!   別 fragment が必要になったら、現 fragment を drop してから新 fragment を
//!   ロードする。
//!
//! ## なぜ c2pa-rs に直接渡せるか
//!
//! BMFF fragmented MP4 は ftyp+moov (init) + moof+mdat (各 fragment) の連結が
//! そのまま valid な MP4 container として扱える。c2pa-rs の
//! `Reader::with_stream` は `Read + Seek` を seek/read で box ヘッダを辿るので、
//! `FragmentedReader` を介して 1 fragment ずつロードしながら検証できる。
//!
//! `c2pa::Reader::with_fragment` API はファイルシステムベースのケース (init.mp4
//! + N 個の seg-*.m4s ファイル) を想定した別経路。本実装は HTTP fetch ベース
//! ので、`with_stream` + lazy reader が筋。
//!
//! ## メモリ会計
//!
//! `peak_memory_hint()` は `init.len() + max(fragment_sizes)` を返す。
//! orchestrator はこの値を `ticket.extend` 1 回で予約する。FragmentedReader 内部
//! の fragment swap はこのバウンド以内で完結するので、追加 extend/shrink は不要。
//!
//! 仕様 §4.4 の `MAX_FRAGMENT_SIZE = 100 MB` × N fragments の理論上限を真に
//! 受けても、ピークは `init + 100 MB` で固定される (concat 方式なら N × 100 MB
//! まで膨らむ)。

use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;

use title_core::{ContentSource, ContentStream};

use crate::resource_pool::Ticket;

/// 連結された fragmented MP4 を lazy に表現する `ContentSource`。
///
/// 構築時に init は full fetch (in-memory)、各 fragment は HEAD 等で size を
/// 確認して `ContentSource` だけ保持する (実バイト列は open() で必要時にロード)。
///
/// ## Ticket 連携 (仕様 §4.3 の shrink ループ)
///
/// `with_ticket(Arc<Ticket>)` を呼ぶと、reader 内部の fragment swap で
/// `ticket.extend` / `ticket.shrink` を呼んで動的に実体メモリ使用量を反映する。
/// 呼ばれない場合は orchestrator 側で `peak_memory_hint` 分を一括予約する前提
/// (= 静的予約モード) で動作する。
pub struct FragmentedSource {
    init: Arc<[u8]>,
    fragments: Arc<[FragmentEntry]>,
    /// 各 fragment の論理開始 offset (init.len + Σ prev_fragment_sizes)。
    /// `fragments[i]` は `fragment_offsets[i]` から `fragment_offsets[i] + fragments[i].size` まで。
    fragment_offsets: Arc<[u64]>,
    total_size: u64,
    max_fragment_size: u64,
    /// オプションの ticket。指定された場合、reader が fragment swap 時に
    /// `extend` / `shrink` を呼ぶ。
    ticket: Option<Arc<Ticket>>,
}

struct FragmentEntry {
    source: Box<dyn ContentSource>,
    size: u64,
}

impl FragmentedSource {
    /// 構築する。各 fragment の `size` は呼び出し側が事前に確定済みであることを
    /// 前提とする (典型は `fetch_streaming` 経由の `size_hint` から取得)。
    ///
    /// # Panics
    /// `fragment_sources.len() != fragment_sizes.len()` のとき panic。
    pub fn new(
        init: Vec<u8>,
        fragment_sources: Vec<Box<dyn ContentSource>>,
        fragment_sizes: Vec<u64>,
    ) -> Self {
        assert_eq!(
            fragment_sources.len(),
            fragment_sizes.len(),
            "fragment_sources and fragment_sizes must have the same length"
        );

        let init_len = init.len() as u64;
        let max_fragment_size = fragment_sizes.iter().copied().max().unwrap_or(0);

        let mut fragment_offsets = Vec::with_capacity(fragment_sources.len());
        let mut acc = init_len;
        for size in &fragment_sizes {
            fragment_offsets.push(acc);
            acc = acc.saturating_add(*size);
        }
        let total_size = acc;

        let fragments: Vec<FragmentEntry> = fragment_sources
            .into_iter()
            .zip(fragment_sizes)
            .map(|(source, size)| FragmentEntry { source, size })
            .collect();

        Self {
            init: Arc::from(init),
            fragments: Arc::from(fragments),
            fragment_offsets: Arc::from(fragment_offsets),
            total_size,
            max_fragment_size,
            ticket: None,
        }
    }

    /// 仕様 §4.3 の「extend → 検証 → shrink」ループを動的に駆動する ticket を
    /// バインドする。指定された ticket は reader の fragment swap で
    /// `extend(new_fragment_size)` / `shrink(old_fragment_size)` を呼ばれ、
    /// 実体メモリに即した予約が `pool.used` に反映される。
    ///
    /// 呼ばれない場合は orchestrator が `peak_memory_hint` 相当を一括予約する
    /// 静的モード。動的モードでは orchestrator は init 分のみ先行予約し、
    /// fragment 分は reader が動的に管理する。
    pub fn with_ticket(mut self, ticket: Arc<Ticket>) -> Self {
        self.ticket = Some(ticket);
        self
    }

    /// 連結後の論理サイズ。テスト/監視用。
    pub fn total_size(&self) -> u64 {
        self.total_size
    }
}

impl ContentSource for FragmentedSource {
    fn open(&self) -> std::io::Result<Box<dyn ContentStream>> {
        Ok(Box::new(FragmentedReader {
            init: Arc::clone(&self.init),
            fragments: Arc::clone(&self.fragments),
            fragment_offsets: Arc::clone(&self.fragment_offsets),
            total_size: self.total_size,
            current_fragment: None,
            pos: 0,
            ticket: self.ticket.as_ref().map(Arc::clone),
        }))
    }

    fn size_hint(&self) -> Option<u64> {
        Some(self.total_size)
    }

    /// 仕様 §4.3 — ピークメモリ = init + フラグメント 1 個分。
    ///
    /// `max_fragment_size` を保守的に採用 (実際は現在ロード中の fragment が
    /// より小さいケースが大半だが、orchestrator の admission control は上限で
    /// 予約する必要がある)。
    fn peak_memory_hint(&self) -> Option<u64> {
        Some(self.init.len() as u64 + self.max_fragment_size)
    }
}

// ---------------------------------------------------------------------------
// FragmentedReader — lazy concatenated stream
// ---------------------------------------------------------------------------

/// `FragmentedSource::open` が返す `Read + Seek`。
///
/// 論理位置 0..init.len() は init を、init.len().. は各 fragment を順に
/// 並べたものとして振る舞う。内部状態として「現在ロード中の 1 fragment」だけを
/// 保持する。`read`/`seek` で別 fragment が必要になったら drop してロード。
struct FragmentedReader {
    init: Arc<[u8]>,
    fragments: Arc<[FragmentEntry]>,
    fragment_offsets: Arc<[u64]>,
    total_size: u64,
    /// `Some((fragment_idx, bytes))` で現在ロード中の fragment。
    /// 別 fragment にアクセスが行ったら置き換える (古い Vec は drop される)。
    current_fragment: Option<(usize, Vec<u8>)>,
    pos: u64,
    /// 動的 ticket 連携 (optional)。fragment swap 時に extend/shrink を呼ぶ。
    ticket: Option<Arc<Ticket>>,
}

impl FragmentedReader {
    /// `logical_pos` がどの segment (init or fragment N) に属するかを返す。
    ///
    /// 戻り値: `(SegmentLocation, offset_within_segment)`
    /// - SegmentLocation::Init: pos < init.len()
    /// - SegmentLocation::Fragment(idx): init.len() + Σ ... の範囲
    /// - None: pos >= total_size (= EOF)
    fn locate(&self, logical_pos: u64) -> Option<(SegmentLocation, u64)> {
        if logical_pos >= self.total_size {
            return None;
        }
        if logical_pos < self.init.len() as u64 {
            return Some((SegmentLocation::Init, logical_pos));
        }
        // fragments は順序通り、offsets[i] = fragments[0..i] の総バイト数 + init.len。
        // logical_pos が含まれる fragment を線形探索 (典型 N <= 数千で十分高速)。
        // 二分探索化は将来の最適化余地。
        for (i, &start) in self.fragment_offsets.iter().enumerate() {
            let end = start + self.fragments[i].size;
            if logical_pos < end {
                return Some((SegmentLocation::Fragment(i), logical_pos - start));
            }
        }
        None
    }

    /// 必要な fragment がまだロードされていなければロードする。
    /// 既存の current_fragment は drop される (Vec が解放される)。
    ///
    /// 仕様 §4.3 の「extend → 検証 → shrink」ループに従い、ticket が bind
    /// されている場合は古い fragment 分を shrink してから新 fragment 分を
    /// extend する。順序は (shrink → extend) で、admission_limit を一時的に
    /// 越える状態を作らない。
    fn ensure_fragment_loaded(&mut self, fragment_idx: usize) -> std::io::Result<()> {
        if let Some((cur_idx, _)) = &self.current_fragment {
            if *cur_idx == fragment_idx {
                return Ok(()); // 既にロード済み
            }
        }

        // 古い fragment があれば shrink + drop。新 fragment の extend が
        // 失敗しても古いメモリは解放される (リソース leak 防止)。
        if let Some((old_idx, _)) = &self.current_fragment {
            if let Some(ticket) = &self.ticket {
                let old_size = self.fragments[*old_idx].size as usize;
                ticket.shrink(old_size);
            }
        }
        self.current_fragment = None;

        let entry = &self.fragments[fragment_idx];

        // 新 fragment 分を extend (ticket 連携モード時のみ)。
        if let Some(ticket) = &self.ticket {
            ticket.extend(entry.size as usize).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::OutOfMemory,
                    format!("ticket extend for fragment {fragment_idx} failed: {e}"),
                )
            })?;
        }

        let mut reader = entry.source.open()?;
        let mut bytes = Vec::with_capacity(entry.size as usize);
        if let Err(e) = reader.read_to_end(&mut bytes) {
            // read 失敗時は extend した分を即 shrink して整合性を保つ。
            if let Some(ticket) = &self.ticket {
                ticket.shrink(entry.size as usize);
            }
            return Err(e);
        }

        // 宣言サイズと実取得サイズの check。
        if (bytes.len() as u64) != entry.size {
            if let Some(ticket) = &self.ticket {
                ticket.shrink(entry.size as usize);
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "fragment {} size mismatch: expected {}, got {}",
                    fragment_idx,
                    entry.size,
                    bytes.len()
                ),
            ));
        }

        self.current_fragment = Some((fragment_idx, bytes));
        Ok(())
    }
}

impl Drop for FragmentedReader {
    /// reader を捨てるときに最後にロード中の fragment 分を shrink する。
    /// orchestrator が `peak_memory_hint` で予約した分は ticket drop で解放されるが、
    /// 動的 ticket モードでは extend した fragment 分を明示的に戻す必要がある。
    fn drop(&mut self) {
        if let (Some((idx, _)), Some(ticket)) = (&self.current_fragment, &self.ticket) {
            let size = self.fragments[*idx].size as usize;
            ticket.shrink(size);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SegmentLocation {
    Init,
    Fragment(usize),
}

impl Read for FragmentedReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() || self.pos >= self.total_size {
            return Ok(0);
        }

        let (location, offset_in_segment) = match self.locate(self.pos) {
            Some(loc) => loc,
            None => return Ok(0),
        };

        // 1 回の read で複数 segment を跨ぐと、新しい fragment ロードが連鎖して
        // ピークメモリを越える可能性がある。1 segment 分だけ読んで返す
        // (Read 規約: 部分 read は OK、caller は次の read で続きを取れる)。
        let n = match location {
            SegmentLocation::Init => {
                let remaining = self.init.len() as u64 - offset_in_segment;
                let take = remaining.min(buf.len() as u64) as usize;
                buf[..take]
                    .copy_from_slice(&self.init[offset_in_segment as usize..offset_in_segment as usize + take]);
                take
            }
            SegmentLocation::Fragment(idx) => {
                self.ensure_fragment_loaded(idx)?;
                // current_fragment を借りてコピー (借用衝突を避けるため一旦取得)
                let (_, ref bytes) = self.current_fragment.as_ref().expect("loaded above");
                let remaining = bytes.len() as u64 - offset_in_segment;
                let take = remaining.min(buf.len() as u64) as usize;
                buf[..take].copy_from_slice(
                    &bytes[offset_in_segment as usize..offset_in_segment as usize + take],
                );
                take
            }
        };

        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for FragmentedReader {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let new_pos = match pos {
            SeekFrom::Start(p) => p,
            SeekFrom::End(p) => ((self.total_size as i64) + p) as u64,
            SeekFrom::Current(p) => ((self.pos as i64) + p) as u64,
        };
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
    use std::io::Read;
    use title_core::InMemorySource;

    /// テスト用ヘルパ: 固定パターンのバイト列で fragment を作る。
    fn build_source(init: &[u8], frags: &[&[u8]]) -> FragmentedSource {
        let fragment_sources: Vec<Box<dyn ContentSource>> = frags
            .iter()
            .map(|f| Box::new(InMemorySource::new(f.to_vec())) as Box<dyn ContentSource>)
            .collect();
        let fragment_sizes: Vec<u64> = frags.iter().map(|f| f.len() as u64).collect();
        FragmentedSource::new(init.to_vec(), fragment_sources, fragment_sizes)
    }

    #[test]
    fn size_hint_equals_init_plus_all_fragments() {
        let src = build_source(b"INIT", &[b"AAAA", b"BBBB", b"CCCC"]);
        assert_eq!(src.size_hint(), Some(4 + 4 + 4 + 4)); // 16
        assert_eq!(src.total_size(), 16);
    }

    #[test]
    fn peak_memory_hint_is_init_plus_max_fragment() {
        // init = 4, max fragment = 12
        let src = build_source(b"INIT", &[b"AA", b"BBBBBBBBBBBB", b"CC"]);
        assert_eq!(src.peak_memory_hint(), Some(4 + 12));
    }

    #[test]
    fn read_to_end_returns_concatenated_bytes() {
        let init = b"INIT_";
        let f1 = b"FRAG1_";
        let f2 = b"FRAG2_";
        let f3 = b"FRAG3";
        let src = build_source(init, &[f1, f2, f3]);

        let mut reader = src.open().unwrap();
        let mut got = Vec::new();
        reader.read_to_end(&mut got).unwrap();

        let mut expected = Vec::new();
        expected.extend_from_slice(init);
        expected.extend_from_slice(f1);
        expected.extend_from_slice(f2);
        expected.extend_from_slice(f3);
        assert_eq!(got, expected);
    }

    #[test]
    fn seek_into_middle_fragment_returns_correct_bytes() {
        let init = b"AAAA"; // 0..4
        let f1 = b"BBBB"; // 4..8
        let f2 = b"CCCC"; // 8..12
        let f3 = b"DDDD"; // 12..16
        let src = build_source(init, &[f1, f2, f3]);

        let mut reader = src.open().unwrap();
        reader.seek(SeekFrom::Start(6)).unwrap();
        let mut buf = [0u8; 4];
        reader.read_exact(&mut buf).unwrap();
        // pos=6 in concatenated stream → fragment 0 (f1) offset 2 → "BB" + load f2 → "CC"... wait we read exact 4
        // Actually our read() returns at most 1 segment per call.
        // 1st read: gets f1[2..4] = "BB" (2 bytes)
        // 2nd read (inside read_exact): gets f2[0..2] = "CC" (2 bytes)
        // Total: "BBCC"
        assert_eq!(&buf, b"BBCC");
    }

    #[test]
    fn read_at_eof_returns_zero() {
        let src = build_source(b"INIT", &[b"AAAA"]);
        let mut reader = src.open().unwrap();
        reader.seek(SeekFrom::Start(8)).unwrap(); // EOF
        let mut buf = [0u8; 4];
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn fragment_swap_drops_previous_fragment_bytes() {
        let src = build_source(b"AAAA", &[b"BBBBBBBBBB", b"CCCCCCCCCC"]);
        let mut reader = src.open().unwrap();

        // Force load of fragment 0 by seeking into it and reading
        reader.seek(SeekFrom::Start(5)).unwrap();
        let mut buf = [0u8; 2];
        reader.read_exact(&mut buf).unwrap();
        // current_fragment should be Some((0, [B;10]))
        // Now seek into fragment 1
        reader.seek(SeekFrom::Start(15)).unwrap();
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"CC");
        // The old fragment 0 bytes should have been dropped.
        // We can't directly observe drop here, but the test confirms the swap completes.
    }

    /// ContentSource 規約テスト suite を通す。Read::read 規約準拠を含む。
    #[test]
    fn fragmented_source_contract_basic() {
        // body = "INIT" + "AAAA" + "BBBB" = 12 bytes
        let init = b"INIT";
        let frags: &[&[u8]] = &[b"AAAA", b"BBBB"];
        let src = build_source(init, frags);

        let mut expected = Vec::new();
        expected.extend_from_slice(init);
        for f in frags {
            expected.extend_from_slice(f);
        }
        title_core::content_stream::contract::assert_content_source_contract(&src, &expected);
    }

    /// 末尾跨ぎ + 不均一サイズ fragment での contract suite。
    /// Read::read 規約 (返却 N に対し buf[..N] のみ valid) を境界で検証。
    #[test]
    fn fragmented_source_contract_uneven_fragments() {
        // init=10, frag1=1, frag2=7, frag3=3 → total=21 (奇数で末尾跨ぎ多発)
        let init = vec![0u8; 10];
        let f1 = vec![1u8; 1];
        let f2 = vec![2u8; 7];
        let f3 = vec![3u8; 3];

        let src = build_source(&init, &[&f1, &f2, &f3]);

        let mut expected = init.clone();
        expected.extend_from_slice(&f1);
        expected.extend_from_slice(&f2);
        expected.extend_from_slice(&f3);
        title_core::content_stream::contract::assert_content_source_contract(&src, &expected);
    }

    /// fragments がゼロ個の場合 (= init だけ)。
    #[test]
    fn fragmented_source_init_only_contract() {
        let src = build_source(b"JUST_INIT_BYTES_FOR_TEST", &[]);
        title_core::content_stream::contract::assert_content_source_contract(
            &src,
            b"JUST_INIT_BYTES_FOR_TEST",
        );
    }

    /// 多数の小 fragment (典型 DASH ストリームの簡易模擬)。
    /// 1 セグメントあたりの read 分割が正しく機能することを確認。
    #[test]
    fn fragmented_source_many_small_fragments() {
        let init = vec![0xFFu8; 16];
        let frags_data: Vec<Vec<u8>> = (0..20).map(|i| vec![i as u8; 8]).collect();
        let frag_refs: Vec<&[u8]> = frags_data.iter().map(|v| v.as_slice()).collect();
        let src = build_source(&init, &frag_refs);

        let mut expected = init.clone();
        for f in &frags_data {
            expected.extend_from_slice(f);
        }
        title_core::content_stream::contract::assert_content_source_contract(&src, &expected);
    }

    // ---- 動的 Ticket 連携テスト (仕様 §4.3 の shrink ループ) ----

    use crate::resource_pool::ResourcePool;
    use title_core::Processor;

    /// `with_ticket` を bind した状態で fragment swap が ticket.extend/shrink を
    /// 正しく呼ぶことを検証する。仕様 §4.3 の動的メモリパターンの核心。
    #[test]
    fn dynamic_ticket_extends_and_shrinks_on_fragment_swap() {
        use std::io::Read;
        let pool = std::sync::Arc::new(ResourcePool::with_single_limit(10_000));
        let ticket = std::sync::Arc::new(pool.ticket(Some(0)));

        // init = 100 bytes, frag0 = 1000, frag1 = 500
        let init = vec![0u8; 100];
        let f0 = vec![1u8; 1000];
        let f1 = vec![2u8; 500];

        // 外側で init 分を予約 (orchestrator が fetch_fragmented で行う相当)
        ticket.extend(init.len()).unwrap();
        assert_eq!(ticket.reserved(), 100);

        let src = build_source(&init, &[&f0, &f1]).with_ticket(std::sync::Arc::clone(&ticket));
        let mut reader = src.open().unwrap();

        // fragment 0 にアクセス: extend(1000)
        reader.seek(SeekFrom::Start(150)).unwrap();
        let mut buf = [0u8; 10];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(
            ticket.reserved(),
            100 + 1000,
            "after loading fragment 0, reserved = init + f0"
        );

        // fragment 1 へ swap: shrink(1000) → extend(500)
        reader.seek(SeekFrom::Start(1200)).unwrap();
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(
            ticket.reserved(),
            100 + 500,
            "after swap to fragment 1, reserved = init + f1 (smaller)"
        );

        // reader drop: shrink(500)
        drop(reader);
        assert_eq!(ticket.reserved(), 100, "reader drop releases f1");
    }

    /// fragment extend が `total_limit` を超えると `OutOfMemory` エラーが返り、
    /// reserved は変化しない (atomic な extend 失敗)。
    #[test]
    fn dynamic_ticket_rejects_oversized_fragment() {
        use std::io::Read;
        let pool = std::sync::Arc::new(ResourcePool::with_single_limit(200));
        let ticket = std::sync::Arc::new(pool.ticket(Some(0)));

        let init = vec![0u8; 50];
        let big_frag = vec![1u8; 500]; // 200 - 50 = 150 < 500、limit 超え

        ticket.extend(init.len()).unwrap();

        let src = build_source(&init, &[&big_frag]).with_ticket(std::sync::Arc::clone(&ticket));
        let mut reader = src.open().unwrap();
        reader.seek(SeekFrom::Start(60)).unwrap();
        let mut buf = [0u8; 1];
        let err = reader
            .read(&mut buf)
            .expect_err("fragment extend should fail with OOM");
        assert_eq!(err.kind(), std::io::ErrorKind::OutOfMemory);
        // extend 失敗後の reserved は init.len のまま
        assert_eq!(ticket.reserved(), init.len());
    }

    // ---- 実 C2PA データを使う end-to-end テスト ----
    //
    // FragmentedReader が「init + fragments」を c2pa-rs が parse できる連結
    // ストリームとして提示できるかを実 C2PA 署名付き JPEG で検証する。
    //
    // 真の fragmented MP4 fixture は別 task (ffmpeg + c2pa CLI で生成) だが、
    // ここでは JPEG を任意位置で分割して FragmentedSource に通し、c2pa-verify
    // が parse できることを確認する。これにより:
    //   - FragmentedReader の seek + read across segment boundary が機能している
    //   - c2pa::Reader::with_stream が FragmentedReader を受け入れる
    // という 2 つの統合契約を保証する。

    fn create_signed_jpeg_fixture() -> Vec<u8> {
        use image::{ImageBuffer, ImageEncoder, Rgb};
        use std::io::Cursor;
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::from_fn(8, 8, |x, y| Rgb([(x * 30) as u8, (y * 30) as u8, 128]));
        let mut buf = Cursor::new(Vec::new());
        image::codecs::jpeg::JpegEncoder::new(&mut buf)
            .write_image(img.as_raw(), 8, 8, image::ExtendedColorType::Rgb8)
            .unwrap();
        let test_jpeg = buf.into_inner();
        let signer =
            c2pa::EphemeralSigner::new("title-tee-fragmented-test").expect("EphemeralSigner");
        let definition = serde_json::json!({
            "claim_generator_info": [{ "name": "title-tee-fragmented-test", "version": "0.1.2" }],
            "assertions": [{ "label": "c2pa.actions.v2", "data": { "actions": [{ "action": "c2pa.created" }] }}]
        });
        let mut builder = c2pa::Builder::from_context(c2pa::Context::default())
            .with_definition(definition.to_string())
            .expect("Builder");
        let mut source = Cursor::new(&test_jpeg);
        let mut dest = Cursor::new(Vec::new());
        builder
            .sign(&signer, "image/jpeg", &mut source, &mut dest)
            .expect("sign");
        dest.into_inner()
    }

    /// FragmentedSource (init-only) で c2pa::Reader::with_stream が機能する。
    /// 「fragments が空 = init だけが全コンテンツ」のミニマムケース。
    #[test]
    fn c2pa_verify_via_fragmented_source_init_only() {
        use std::io::Read;
        let signed_jpeg = create_signed_jpeg_fixture();
        let src = FragmentedSource::new(signed_jpeg.clone(), vec![], vec![]);
        let mut reader = src.open().unwrap();

        let mut got = Vec::new();
        reader.read_to_end(&mut got).unwrap();
        assert_eq!(got, signed_jpeg, "init-only source = full content");

        // 実 c2pa-rs に流して parse できることを確認
        let proc = title_core::C2paVerifyProcessor::new();
        let mut reader2 = src.open().unwrap();
        let result = proc.process(reader2.as_mut(), "image/jpeg");
        assert!(
            result.is_ok(),
            "c2pa-verify should parse init-only FragmentedSource: {result:?}"
        );
    }

    /// 実 C2PA JPEG を「先頭 = init、残り = 1 fragment」に分割して FragmentedSource
    /// で連結提示。c2pa::Reader::with_stream がこの連結を JPEG として parse できる
    /// ことを確認 (= 連結バイト列が原 JPEG と一致 + FragmentedReader の seek が
    /// segment boundary を跨いで正しく動く)。
    #[test]
    fn c2pa_verify_via_fragmented_source_split_into_segments() {
        let signed_jpeg = create_signed_jpeg_fixture();
        let mid = signed_jpeg.len() / 2;
        let init_bytes = signed_jpeg[..mid].to_vec();
        let frag_bytes = signed_jpeg[mid..].to_vec();

        // FragmentedSource(init, [frag]) を構築
        let src = build_source(&init_bytes, &[&frag_bytes]);

        // バイト列レベルで原 JPEG と一致
        let mut reader = src.open().unwrap();
        let mut got = Vec::new();
        std::io::Read::read_to_end(&mut reader, &mut got).unwrap();
        assert_eq!(got, signed_jpeg);

        // c2pa-verify が parse できる (FragmentedReader 経由でも c2pa-rs の
        // BMFF/JPEG seek 動作が壊れていない)。
        let proc = title_core::C2paVerifyProcessor::new();
        let mut reader2 = src.open().unwrap();
        let result = proc.process(reader2.as_mut(), "image/jpeg");
        assert!(
            result.is_ok(),
            "c2pa-verify should parse split FragmentedSource: {result:?}"
        );
    }

    /// 3 segments で split (init + 2 fragments)。fragment swap が複数回起きる
    /// パスを実 C2PA データで end-to-end 検証。
    #[test]
    fn c2pa_verify_via_fragmented_source_three_segments() {
        let signed_jpeg = create_signed_jpeg_fixture();
        let len = signed_jpeg.len();
        let init = signed_jpeg[..len / 3].to_vec();
        let f0 = signed_jpeg[len / 3..2 * len / 3].to_vec();
        let f1 = signed_jpeg[2 * len / 3..].to_vec();

        let src = build_source(&init, &[&f0, &f1]);

        let proc = title_core::C2paVerifyProcessor::new();
        let mut reader = src.open().unwrap();
        let result = proc.process(reader.as_mut(), "image/jpeg");
        assert!(
            result.is_ok(),
            "c2pa-verify should parse 3-segment FragmentedSource: {result:?}"
        );
    }

    /// 独立した 2 reader を開いてそれぞれ別 fragment を見ている状態でも干渉しない。
    #[test]
    fn multiple_readers_load_fragments_independently() {
        let init = b"INIT";
        let f0 = b"AAAA";
        let f1 = b"BBBB";
        let src = build_source(init, &[f0, f1]);

        let mut r1 = src.open().unwrap();
        let mut r2 = src.open().unwrap();

        // r1 は fragment 0 にアクセス
        r1.seek(SeekFrom::Start(5)).unwrap();
        let mut b1 = [0u8; 2];
        r1.read_exact(&mut b1).unwrap();
        assert_eq!(&b1, b"AA");

        // r2 は fragment 1 にアクセス
        r2.seek(SeekFrom::Start(9)).unwrap();
        let mut b2 = [0u8; 2];
        r2.read_exact(&mut b2).unwrap();
        assert_eq!(&b2, b"BB");

        // r1 は依然 fragment 0 を保持
        r1.seek(SeekFrom::Start(4)).unwrap();
        let mut b1b = [0u8; 2];
        r1.read_exact(&mut b1b).unwrap();
        assert_eq!(&b1b, b"AA");
    }
}
