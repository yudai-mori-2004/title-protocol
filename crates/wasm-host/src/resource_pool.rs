// SPDX-License-Identifier: Apache-2.0

//! 二段階閾値によるリソースプール。
//!
//! 仕様書 §7.1 — TEE内の並行リクエスト間でメモリ使用量を管理する。
//! 単一の `AtomicUsize` カウンタと2つの閾値で制御:
//!
//! - **`admission_limit`**: 新規リクエストの受付閾値。
//!   `used < admission_limit` の場合のみ新規リクエストを受理する。
//!   処理中リクエストのextendに必要なヘッドルームを確保する。
//!
//! - **`total_limit`**: 全extend()呼び出しの絶対上限。
//!   処理中リクエストはこの上限までメモリを使用可能。超過時はextend()が失敗する。
//!
//! ```text
//! |← 新規受付可 →|← 処理中のみ →|← OS/非管理 →|
//! 0          admission_limit   total_limit   enclave_max
//! ```
//!
//! `Ticket` はRAIIハンドルで予約量を追跡する:
//! - `extend(n)`: CASループで `n` バイトを追加予約（ノンブロッキング）
//! - `shrink(n)`: `n` バイトをプールに返却
//! - `Drop`: 残余予約を全て解放

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// 二段階閾値によるリソースプール。
/// 仕様書 §7.1
#[derive(Debug)]
pub struct ResourcePool {
    /// 新規リクエスト受付閾値（Ticket発行時に参照）。
    admission_limit: usize,
    /// 全extend()呼び出しの絶対メモリ上限。
    total_limit: usize,
    /// 全Ticketの予約合計バイト数。
    used: AtomicUsize,
}

/// RAII予約ハンドル。Drop時に予約を自動解放する。
/// 仕様書 §7.1
#[derive(Debug)]
pub struct Ticket {
    pool: Arc<ResourcePool>,
    reserved: AtomicUsize,
}

impl ResourcePool {
    /// 受付閾値と絶対上限を指定してプールを作成する。
    ///
    /// - `admission_limit`: 新規リクエスト受付時の`used`上限
    /// - `total_limit`: 全予約の絶対上限
    pub fn new(admission_limit: usize, total_limit: usize) -> Self {
        assert!(admission_limit <= total_limit);
        Self {
            admission_limit,
            total_limit,
            used: AtomicUsize::new(0),
        }
    }

    /// 単一閾値でプールを作成する（admission = total）。
    /// テスト用の簡易コンストラクタ。
    pub fn with_single_limit(limit: usize) -> Self {
        Self::new(limit, limit)
    }

    /// 新規リクエストを受付可能か判定する。
    /// `used < admission_limit` の場合 true。
    pub fn can_admit(&self) -> bool {
        self.used.load(Ordering::Acquire) < self.admission_limit
    }

    /// 新規リクエスト用に0バイトのTicketを発行する。
    /// 受付閾値を超過している場合はNoneを返す。
    pub fn try_ticket(self: &Arc<Self>) -> Option<Ticket> {
        if self.can_admit() {
            Some(Ticket {
                pool: Arc::clone(self),
                reserved: AtomicUsize::new(0),
            })
        } else {
            None
        }
    }

    /// 無条件で0バイトのTicketを発行する（処理中操作用）。
    pub fn ticket(self: &Arc<Self>) -> Ticket {
        Ticket {
            pool: Arc::clone(self),
            reserved: AtomicUsize::new(0),
        }
    }

    /// ワンショット予約: Ticket発行 + extend。total_limit超過時はNone。
    pub fn acquire(self: &Arc<Self>, size: usize) -> Option<Ticket> {
        let ticket = self.ticket();
        if ticket.extend(size) {
            Some(ticket)
        } else {
            None
        }
    }

    /// 現在の合計使用量（監視・テスト用）。
    pub fn total_used(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }
}

impl Ticket {
    /// 追加バイトを予約する。total_limit超過時はfalseを返す。
    /// CASループによるノンブロッキング実装。失敗時も既存予約は保持される。
    pub fn extend(&self, additional: usize) -> bool {
        if additional == 0 {
            return true;
        }
        loop {
            let current = self.pool.used.load(Ordering::Acquire);
            let new_total = match current.checked_add(additional) {
                Some(v) => v,
                None => return false,
            };
            if new_total > self.pool.total_limit {
                return false;
            }
            if self
                .pool
                .used
                .compare_exchange_weak(current, new_total, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                self.reserved.fetch_add(additional, Ordering::AcqRel);
                return true;
            }
        }
    }

    /// 指定バイト数をプールに返却する。
    pub fn shrink(&self, amount: usize) {
        if amount == 0 {
            return;
        }
        self.reserved.fetch_sub(amount, Ordering::AcqRel);
        self.pool.used.fetch_sub(amount, Ordering::AcqRel);
    }

    /// このTicketの現在の予約量。
    pub fn reserved(&self) -> usize {
        self.reserved.load(Ordering::Acquire)
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        let r = self.reserved.load(Ordering::Acquire);
        if r > 0 {
            self.pool.used.fetch_sub(r, Ordering::AcqRel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_acquire_release() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        {
            let ticket = pool.acquire(500).unwrap();
            assert_eq!(pool.total_used(), 500);
            assert_eq!(ticket.reserved(), 500);
        }
        assert_eq!(pool.total_used(), 0);
    }

    #[test]
    fn test_acquire_exceeds_limit() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        let _t1 = pool.acquire(600).unwrap();
        assert!(pool.acquire(500).is_none());
        assert_eq!(pool.total_used(), 600);
    }

    #[test]
    fn test_ticket_extend_pattern() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        let ticket = pool.ticket();
        assert!(ticket.extend(300));
        assert!(ticket.extend(200));
        assert_eq!(ticket.reserved(), 500);
        assert_eq!(pool.total_used(), 500);
    }

    #[test]
    fn test_extend_exceeds_limit() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        let ticket = pool.ticket();
        assert!(ticket.extend(800));
        assert!(!ticket.extend(300));
        assert_eq!(ticket.reserved(), 800);
    }

    #[test]
    fn test_shrink_partial_release() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        let ticket = pool.ticket();
        assert!(ticket.extend(800));
        ticket.shrink(500);
        assert_eq!(pool.total_used(), 300);
        assert_eq!(ticket.reserved(), 300);
        assert!(pool.acquire(600).is_some());
    }

    #[test]
    fn test_shrink_then_drop() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        {
            let ticket = pool.ticket();
            assert!(ticket.extend(800));
            ticket.shrink(300);
            assert_eq!(pool.total_used(), 500);
        }
        assert_eq!(pool.total_used(), 0);
    }

    #[test]
    fn test_two_tier_admission() {
        // admission_limit=600, total_limit=1000
        let pool = Arc::new(ResourcePool::new(600, 1000));

        // New request when used=0: admitted
        let t1 = pool.try_ticket().expect("should admit when empty");
        assert!(t1.extend(500));
        assert_eq!(pool.total_used(), 500);

        // New request when used=500 < 600: still admitted
        let t2 = pool.try_ticket().expect("should admit under admission_limit");
        assert!(t2.extend(100));
        assert_eq!(pool.total_used(), 600);

        // New request when used=600 >= 600: rejected
        assert!(pool.try_ticket().is_none(), "should reject at admission_limit");

        // But in-progress ticket can still extend up to total_limit
        assert!(t1.extend(300)); // 600 + 300 = 900 < 1000
        assert_eq!(pool.total_used(), 900);

        // In-progress extend beyond total_limit fails
        assert!(!t2.extend(200)); // 900 + 200 = 1100 > 1000
    }

    #[test]
    fn test_multiple_tickets_share_pool() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        let t1 = pool.acquire(400).unwrap();
        let t2 = pool.acquire(400).unwrap();
        assert_eq!(pool.total_used(), 800);
        assert!(pool.acquire(300).is_none());
        drop(t1);
        assert_eq!(pool.total_used(), 400);
        drop(t2);
        assert_eq!(pool.total_used(), 0);
    }

    #[test]
    fn test_drop_releases_reservation() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        {
            let ticket = pool.ticket();
            assert!(ticket.extend(500));
            assert!(ticket.extend(300));
            assert_eq!(pool.total_used(), 800);
        }
        assert_eq!(pool.total_used(), 0);
    }
}
