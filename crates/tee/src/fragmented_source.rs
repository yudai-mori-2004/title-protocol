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
//! `peak_memory_hint()` は `init.len() + (in-memory 常駐合計) + (active Range
//! バッファ最大)` を返す:
//!
//! - **in-memory 常駐 fragment** (`is_in_memory_resident() == true`、例:
//!   `InMemorySource` や fetch_streaming の Range 非対応 fallback): source 自体
//!   が `Arc<[u8]>` でバイト列を保持しており、source が drop されるまで size 分
//!   が常駐。peak 寄与は **Σ size**。
//! - **Range fragment** (`is_in_memory_resident() == false`、例: `HttpRangeSource` /
//!   `ProxyRangeSource`): source 自体は URL とメタデータだけで常駐ゼロ。
//!   FragmentedReader が active 1 個だけロードするため peak 寄与は **max(peak)**。
//!
//! orchestrator (`fetch_fragmented`) は probe loop 内で各 fragment 取得直後に
//! `ticket.extend` を漸進呼びし、loop 完了後に `peak_memory_hint` 確定値まで
//! `shrink` で正味化する。これにより admission gate は loop 中の任意の時点で
//! 正確な `pool.used` を見て並列リクエストを throttle できる。
//! 参考: `legacy/v0.1.0/crates/tee/src/endpoints/verify/handler.rs:91-127`。
//!
//! ## なぜ reader 内で動的 extend/shrink しないか
//!
//! 仕様 §4.3 の擬似コードは「extend → 検証 → shrink」のループを示すが、
//! reader 側に `Arc<Ticket>` を渡して swap ごとに動的会計させると (a) probe 中の
//! admission gate が `init` 分しか見えないため並列 N リクエストで一気に
//! `N × max_fragment` まで積まれる、(b) extend 失敗時の reader 状態整合 vs
//! 一時的 pool.used 多重持ちで別のトレードオフが発生する。
//!
//! fetch 側 (`fetch_fragmented`) で漸進予約することで (a) を解消し、reader は
//! 確定 peak のバウンド内で fragment swap を完結させるので (b) も発生しない。

use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;

use title_core::{ContentSource, ContentStream};

/// 連結された fragmented MP4 を lazy に表現する `ContentSource`。
///
/// 構築時に init は full fetch (in-memory)、各 fragment は HEAD 等で size を
/// 確認して `ContentSource` だけ保持する (実バイト列は open() で必要時にロード)。
/// メモリ会計は orchestrator (`fetch_fragmented`) が漸進予約モードで管理し、
/// reader 側は静的バウンド内で fragment swap を完結させる (モジュール doc 参照)。
pub struct FragmentedSource {
    init: Arc<[u8]>,
    fragments: Arc<[FragmentEntry]>,
    /// 各 fragment の論理開始 offset (init.len + Σ prev_fragment_sizes)。
    /// `fragments[i]` は `fragment_offsets[i]` から `fragment_offsets[i] + fragments[i].size` まで。
    fragment_offsets: Arc<[u64]>,
    total_size: u64,
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
        }
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
        }))
    }

    fn size_hint(&self) -> Option<u64> {
        Some(self.total_size)
    }

    /// 仕様 §4.3 — ピークメモリ = init + (in-memory fragments の常駐合計) +
    /// (active 1 reader の Range バッファ最大)。
    ///
    /// FragmentedReader は内部に 1 fragment しか保持しないが、各 fragment の
    /// `ContentSource` は構築時点でメモリを消費している場合がある:
    ///
    /// - **Range Request 経路** (`is_in_memory_resident() == false`): source
    ///   自体は URL とメタデータだけで常駐ゼロ。active reader が
    ///   `peak_memory_hint()` (典型 `min_req_size` = 4 MiB) のバッファを
    ///   1 つ持つだけ。FragmentedReader が同時にロードするのは 1 fragment
    ///   なので、peak への寄与は max(Range fragments の peak) のみ。
    /// - **In-memory 経路** (`is_in_memory_resident() == true`、`InMemorySource`
    ///   や fetch_streaming の Range 非対応 fallback): `Arc<[u8]>` を source 内に
    ///   保持しており、source が drop されるまで size 分が常駐する。peak への
    ///   寄与は合計 (Σ in-memory sizes)。
    ///
    /// この計算により、Range 非対応サーバーで全 fragments が in-memory fallback
    /// しても `peak = init + Σ all_sizes` で実 RAM を正確に申告できる。
    /// `is_in_memory_resident()` の明示判定を使うことで「peak と size が偶然
    /// 一致する Range source」での誤判定 (heuristic 脆さ) を構造的に防ぐ。
    fn peak_memory_hint(&self) -> Option<u64> {
        let mut in_memory_resident = 0u64;
        let mut active_reader_buf = 0u64;
        for entry in &*self.fragments {
            if entry.source.is_in_memory_resident() {
                in_memory_resident = in_memory_resident.saturating_add(entry.size);
            } else {
                let peak = entry.source.peak_memory_hint().unwrap_or(entry.size);
                active_reader_buf = active_reader_buf.max(peak);
            }
        }
        Some(self.init.len() as u64 + in_memory_resident + active_reader_buf)
    }

    /// `FragmentedSource` 自体は init を `Arc<[u8]>` で常駐させ、各 fragment は
    /// `is_in_memory_resident()` がそれぞれの常駐性を持つ。全 fragment が
    /// 常駐 (= in-memory) かつ init も常駐 (常に true) ならこの source 全体も
    /// 常駐扱い。1 つでも Range fragment があれば streaming 経路扱い。
    fn is_in_memory_resident(&self) -> bool {
        self.fragments
            .iter()
            .all(|e| e.source.is_in_memory_resident())
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
    /// orchestrator (`fetch_fragmented` の漸進予約) が `peak_memory_hint` 確定
    /// 値まで ticket を予約済みなので、内部 swap は ticket と通信せず Vec drop
    /// だけで完結する (peak バウンド内で動作)。
    current_fragment: Option<(usize, Vec<u8>)>,
    pos: u64,
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
    /// orchestrator が事前に `peak_memory_hint` (init + in-memory 常駐分 +
    /// active Range バッファ最大) で
    /// ticket を予約しているため、内部 swap は ticket と通信しない。古い Vec を
    /// drop してから新 Vec を確保すれば、瞬間ピークは `init + max_fragment` で
    /// 安定する (`Vec` のヒープ解放は drop 時点)。
    fn ensure_fragment_loaded(&mut self, fragment_idx: usize) -> std::io::Result<()> {
        if let Some((cur_idx, _)) = &self.current_fragment {
            if *cur_idx == fragment_idx {
                return Ok(()); // 既にロード済み
            }
        }

        // 古い fragment を drop してから新規ロード (peak メモリ抑制)。
        self.current_fragment = None;

        let entry = &self.fragments[fragment_idx];
        let mut reader = entry.source.open()?;
        let mut bytes = Vec::with_capacity(entry.size as usize);
        reader.read_to_end(&mut bytes)?;

        // 宣言サイズと実取得サイズの check (Range Request 経由で size が変動
        // した場合を検出)。
        if (bytes.len() as u64) != entry.size {
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
    /// `std::io::Seek` 規約: negative offset への seek は `InvalidInput` で
    /// reject する。`SeekFrom::End` / `SeekFrom::Current` の計算で overflow や
    /// 負位置になる場合は silent な巨大値設定にせず明示エラー。
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let new_pos = match pos {
            SeekFrom::Start(p) => p,
            SeekFrom::End(p) => add_signed_offset(self.total_size, p)?,
            SeekFrom::Current(p) => add_signed_offset(self.pos, p)?,
        };
        self.pos = new_pos;
        Ok(new_pos)
    }
}

/// `base + offset` を u64 で計算する。負結果は `InvalidInput`、overflow も同様。
/// `std::io::Cursor::seek` と同じセマンティクス。
fn add_signed_offset(base: u64, offset: i64) -> std::io::Result<u64> {
    let result = if offset >= 0 {
        base.checked_add(offset as u64)
    } else {
        let abs = offset.unsigned_abs();
        base.checked_sub(abs)
    };
    result.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("seek to negative or overflow position (base={base}, offset={offset})"),
        )
    })
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
    }

    #[test]
    fn peak_memory_hint_with_in_memory_fragments_sums_all_sizes() {
        // build_source は InMemorySource を使う → fragments は in-memory 常駐。
        // peak = init + Σ fragments = 4 + (2 + 12 + 2) = 20
        let src = build_source(b"INIT", &[b"AA", b"BBBBBBBBBBBB", b"CC"]);
        assert_eq!(src.peak_memory_hint(), Some(4 + 2 + 12 + 2));
    }

    /// Mock Range Request source — `is_in_memory_resident = false`、
    /// `peak_memory_hint < size_hint` (= reader 1 個分のバッファのみ常駐)。
    /// FragmentedSource の Range fragment 経路の peak 計算を test するため。
    struct MockRangeSource {
        bytes: Arc<[u8]>,
        peak_buf: u64,
    }

    impl ContentSource for MockRangeSource {
        fn open(&self) -> std::io::Result<Box<dyn ContentStream>> {
            Ok(Box::new(std::io::Cursor::new(Arc::clone(&self.bytes))))
        }
        fn size_hint(&self) -> Option<u64> {
            Some(self.bytes.len() as u64)
        }
        fn peak_memory_hint(&self) -> Option<u64> {
            Some(self.peak_buf)
        }
        fn is_in_memory_resident(&self) -> bool {
            false // Range Request 想定 = 常駐ゼロ
        }
    }

    /// `is_in_memory_resident()` 直接検証: 全 fragment が InMemorySource なら
    /// true。peak_memory_hint と独立に契約を検証することで、合算/Max 分岐の
    /// 入力条件が明示的にテストされる。
    #[test]
    fn is_in_memory_resident_all_in_memory_returns_true() {
        let src = build_source(b"INIT", &[b"AA", b"BB", b"CC"]);
        assert!(src.is_in_memory_resident());
    }

    /// `is_in_memory_resident()` 直接検証: 1 つでも Range fragment が混ざれば
    /// false。混在パターンでも streaming 経路扱いになることを保証。
    #[test]
    fn is_in_memory_resident_with_any_range_fragment_returns_false() {
        let sources: Vec<Box<dyn ContentSource>> = vec![
            Box::new(InMemorySource::new(b"AA".to_vec())),
            Box::new(MockRangeSource {
                bytes: Arc::from(vec![0u8; 16]),
                peak_buf: 8,
            }),
            Box::new(InMemorySource::new(b"CC".to_vec())),
        ];
        let sizes = vec![2u64, 16, 2];
        let src = FragmentedSource::new(b"INIT".to_vec(), sources, sizes);
        assert!(!src.is_in_memory_resident());
    }

    /// Range fragment 経路 (`is_in_memory_resident = false`) では、active 1
    /// reader 分の peak のみが寄与する (合計ではなく max)。
    #[test]
    fn peak_memory_hint_with_range_fragments_takes_active_max() {
        let init = vec![0u8; 100];
        // 3 Range fragments (各 size 10 MB、reader buf 4 MiB)
        let make = |peak_buf: u64| {
            Box::new(MockRangeSource {
                bytes: Arc::from(vec![0u8; 10 * 1024 * 1024]),
                peak_buf,
            }) as Box<dyn ContentSource>
        };
        let sizes = vec![10 * 1024 * 1024u64; 3];
        let sources: Vec<Box<dyn ContentSource>> = vec![
            make(4 * 1024 * 1024), // 4 MiB reader buf
            make(2 * 1024 * 1024), // 2 MiB reader buf
            make(8 * 1024 * 1024), // 8 MiB reader buf — max
        ];

        let src = FragmentedSource::new(init.clone(), sources, sizes);
        // peak = init (100) + 0 in-memory + max(4, 2, 8) MiB = 100 + 8 MiB
        assert_eq!(src.peak_memory_hint(), Some(100 + 8 * 1024 * 1024));
    }

    /// Range と in-memory の混在ケース: in-memory は合計、Range は active 1 個分。
    #[test]
    fn peak_memory_hint_mixed_in_memory_and_range_separates_accounting() {
        let init = vec![0u8; 100];
        let in_mem_data = vec![0u8; 5 * 1024 * 1024]; // 5 MiB in-memory
        let range_buf = 4 * 1024 * 1024u64; // 4 MiB Range buffer

        let sources: Vec<Box<dyn ContentSource>> = vec![
            Box::new(InMemorySource::new(in_mem_data.clone())),
            Box::new(MockRangeSource {
                bytes: Arc::from(vec![0u8; 10 * 1024 * 1024]),
                peak_buf: range_buf,
            }),
            Box::new(InMemorySource::new(in_mem_data.clone())),
        ];
        let sizes = vec![
            5 * 1024 * 1024u64,
            10 * 1024 * 1024u64,
            5 * 1024 * 1024u64,
        ];
        let src = FragmentedSource::new(init.clone(), sources, sizes);
        // peak = init (100) + in_mem_total (5+5 MiB) + range_max (4 MiB)
        assert_eq!(
            src.peak_memory_hint(),
            Some(100 + 10 * 1024 * 1024 + 4 * 1024 * 1024)
        );
    }

    /// is_in_memory_resident heuristic の境界バグ防止: Range source の
    /// `peak == size` (= min_req_size が fragment と同サイズ) で誤判定しない
    /// ことを確認。明示 override (`MockRangeSource::is_in_memory_resident =
    /// false`) で false-positive を構造的に防ぐ。
    #[test]
    fn peak_memory_hint_range_source_with_buf_equal_to_size_not_treated_as_resident() {
        let init = vec![0u8; 10];
        // size = 4 MiB、peak = 4 MiB (ぴったり一致)。default heuristic だと
        // peak >= size で in-memory 扱いされるが、明示 override で false。
        let sources: Vec<Box<dyn ContentSource>> = vec![Box::new(MockRangeSource {
            bytes: Arc::from(vec![0u8; 4 * 1024 * 1024]),
            peak_buf: 4 * 1024 * 1024,
        })];
        let sizes = vec![4 * 1024 * 1024u64];
        let src = FragmentedSource::new(init.clone(), sources, sizes);
        // 期待: init (10) + Range buf (4 MiB)。in-memory 扱いなら init (10) + 4 MiB に
        // なるが、size==peak のときは数値的にも同じなので、この test は
        // 「2 fragment 並べた場合に sum しないこと」で見るのが正確。次のテストで保証。
        assert_eq!(src.peak_memory_hint(), Some(10 + 4 * 1024 * 1024));
    }

    /// false-positive の発火条件: 2 つの Range fragments が共に size == peak ==
    /// 4 MiB。default heuristic だと両方 in-memory 扱いで合計 8 MiB と申告するが、
    /// 明示 override で各々 false → max(4, 4) = 4 MiB だけ active reader 分。
    #[test]
    fn peak_memory_hint_two_range_sources_with_buf_eq_size_take_max_not_sum() {
        let init = vec![0u8; 10];
        let make = || {
            Box::new(MockRangeSource {
                bytes: Arc::from(vec![0u8; 4 * 1024 * 1024]),
                peak_buf: 4 * 1024 * 1024,
            }) as Box<dyn ContentSource>
        };
        let sources: Vec<Box<dyn ContentSource>> = vec![make(), make()];
        let sizes = vec![4 * 1024 * 1024u64, 4 * 1024 * 1024u64];
        let src = FragmentedSource::new(init.clone(), sources, sizes);
        // 明示 override により Range 扱い → init + max(4, 4) MiB = init + 4 MiB
        // (heuristic だと in-memory 扱いで init + 4+4 = init + 8 MiB になっていた)
        assert_eq!(src.peak_memory_hint(), Some(10 + 4 * 1024 * 1024));
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

    /// `std::io::Seek` 規約: 負位置への seek は `InvalidInput` で reject される。
    #[test]
    fn seek_to_negative_position_returns_invalid_input() {
        let src = build_source(b"INIT", &[b"AAAA"]);
        let mut reader = src.open().unwrap();

        // SeekFrom::Current(-100) で pos=0 から -100 → reject
        let err = reader
            .seek(SeekFrom::Current(-100))
            .expect_err("negative current seek must error");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);

        // SeekFrom::End(-100) で total_size=8、8-100=-92 → reject
        let err = reader
            .seek(SeekFrom::End(-100))
            .expect_err("negative end seek must error");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);

        // 正常な負方向 seek (Current(-2) from pos=5) は OK
        reader.seek(SeekFrom::Start(5)).unwrap();
        let new_pos = reader.seek(SeekFrom::Current(-2)).unwrap();
        assert_eq!(new_pos, 3);
    }

    /// SeekFrom::End / Current の add overflow も InvalidInput。
    #[test]
    fn seek_overflow_returns_invalid_input() {
        let src = build_source(b"INIT", &[b"AAAA"]);
        let mut reader = src.open().unwrap();
        reader.seek(SeekFrom::Start(u64::MAX - 1)).unwrap();
        let err = reader
            .seek(SeekFrom::Current(i64::MAX))
            .expect_err("u64 overflow must error");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    /// i64::MIN 境界 — `offset.unsigned_abs()` で 2^63 を取得し、`checked_sub`
    /// で正しく処理されること。base < 2^63 なら負位置 reject、base >= 2^63 なら成功。
    #[test]
    fn seek_with_i64_min_offset_handles_boundary() {
        let src = build_source(b"INIT", &[b"AAAA"]);
        let mut reader = src.open().unwrap();

        // base = 0、i64::MIN を引く → 0 - 2^63 = 負 → InvalidInput
        let err = reader
            .seek(SeekFrom::Current(i64::MIN))
            .expect_err("0 + i64::MIN must be negative");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);

        // base = 2^63 から i64::MIN を引く → 0 (= valid)
        // Seek::Start(2^63) で base 設定して Current(i64::MIN) で 0 へ。
        reader.seek(SeekFrom::Start(1u64 << 63)).unwrap();
        let new_pos = reader
            .seek(SeekFrom::Current(i64::MIN))
            .expect("2^63 + i64::MIN should land at 0");
        assert_eq!(new_pos, 0);

        // i64::MAX も対象に。base = u64::MAX から +1 で overflow。
        reader.seek(SeekFrom::Start(u64::MAX)).unwrap();
        let err = reader
            .seek(SeekFrom::Current(1))
            .expect_err("u64::MAX + 1 must overflow");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
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

    use title_core::Processor;

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
