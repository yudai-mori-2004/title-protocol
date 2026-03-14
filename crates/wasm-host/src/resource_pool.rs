// SPDX-License-Identifier: Apache-2.0

//! # ResourcePool（統合セマフォ）
//!
//! 仕様書 §7.1
//!
//! raw binary ダウンロードとデコード済みデータのメモリ予算を
//! 単一の `AtomicUsize` で CAS 管理する。
//!
//! ## 設計
//!
//! `ResourcePool` は合計使用量 `used` を単一の AtomicUsize で管理する。
//! `Ticket` は Drop で自動解放される予約チケットで、`extend` による漸進的予約をサポートする。
//! これにより、従来の `tokio::Semaphore`（Semaphore A）と `MemoryPool`（Semaphore B）を
//! 単一のリソースプールに統合し、TOCTOU 競合を完全に排除する。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// ResourcePool: 全リソース予約の合計使用量を total_limit 以下に保つ。
/// 仕様書 §7.1
#[derive(Debug)]
pub struct ResourcePool {
    /// 全体のメモリ予算（バイト）
    total_limit: usize,
    /// 全 Ticket の合計使用量（CAS で排他制御する唯一の判定値）
    used: AtomicUsize,
}

/// Drop で自動解放される予約チケット。
/// 仕様書 §7.1
#[derive(Debug)]
pub struct Ticket {
    /// 所属プール
    pool: Arc<ResourcePool>,
    /// このチケットが予約しているバイト数（extend で追加可能）
    reserved: AtomicUsize,
}

impl ResourcePool {
    /// 新しい ResourcePool を作成する。
    /// 仕様書 §7.1
    pub fn new(total_limit: usize) -> Self {
        Self {
            total_limit,
            used: AtomicUsize::new(0),
        }
    }

    /// 一括予約。失敗時 None。
    /// `ticket()` + `extend(size)` の糖衣構文。
    /// 仕様書 §7.1
    pub fn acquire(self: &Arc<Self>, size: usize) -> Option<Ticket> {
        let ticket = self.ticket();
        if ticket.extend(size) {
            Some(ticket)
        } else {
            None
        }
    }

    /// 漸進予約用の 0 バイトチケットを発行する。
    /// 仕様書 §7.1
    pub fn ticket(self: &Arc<Self>) -> Ticket {
        Ticket {
            pool: Arc::clone(self),
            reserved: AtomicUsize::new(0),
        }
    }

    /// 現在の合計使用量を返す（テスト・モニタリング用）。
    pub fn total_used(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }
}

impl Ticket {
    /// 追加予約。失敗時 false（既存予約は保持される）。
    /// CAS ループで非ブロッキング。
    /// 仕様書 §7.1
    pub fn extend(&self, additional: usize) -> bool {
        if additional == 0 {
            return true;
        }
        loop {
            let current = self.pool.used.load(Ordering::Acquire);
            let new_total = match current.checked_add(additional) {
                Some(v) => v,
                None => return false, // オーバーフロー
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

    /// このチケットが予約しているバイト数を返す。
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
        let pool = Arc::new(ResourcePool::new(1000));
        {
            let ticket = pool.acquire(500).expect("500バイト予約に成功するべき");
            assert_eq!(pool.total_used(), 500);
            assert_eq!(ticket.reserved(), 500);
        }
        // Drop で解放
        assert_eq!(pool.total_used(), 0);
    }

    #[test]
    fn test_acquire_exceeds_limit() {
        let pool = Arc::new(ResourcePool::new(1000));
        let _t1 = pool.acquire(600).expect("600バイト予約に成功するべき");
        // 600 + 500 = 1100 > 1000
        assert!(pool.acquire(500).is_none());
        assert_eq!(pool.total_used(), 600);
    }

    #[test]
    fn test_ticket_extend_pattern() {
        let pool = Arc::new(ResourcePool::new(1000));
        let ticket = pool.ticket();
        assert_eq!(ticket.reserved(), 0);
        assert!(ticket.extend(300));
        assert_eq!(ticket.reserved(), 300);
        assert!(ticket.extend(200));
        assert_eq!(ticket.reserved(), 500);
        assert_eq!(pool.total_used(), 500);
    }

    #[test]
    fn test_extend_exceeds_limit() {
        let pool = Arc::new(ResourcePool::new(1000));
        let ticket = pool.ticket();
        assert!(ticket.extend(800));
        // 800 + 300 = 1100 > 1000
        assert!(!ticket.extend(300));
        // 既存予約は保持
        assert_eq!(ticket.reserved(), 800);
        assert_eq!(pool.total_used(), 800);
    }

    #[test]
    fn test_multiple_tickets_share_pool() {
        let pool = Arc::new(ResourcePool::new(1000));
        let t1 = pool.acquire(400).expect("400バイト予約に成功するべき");
        let t2 = pool.acquire(400).expect("400バイト予約に成功するべき");
        assert_eq!(pool.total_used(), 800);
        // 800 + 300 = 1100 > 1000
        assert!(pool.acquire(300).is_none());
        drop(t1);
        assert_eq!(pool.total_used(), 400);
        drop(t2);
        assert_eq!(pool.total_used(), 0);
    }

    #[test]
    fn test_drop_releases_reservation() {
        let pool = Arc::new(ResourcePool::new(1000));
        {
            let ticket = pool.ticket();
            assert!(ticket.extend(500));
            assert!(ticket.extend(300));
            assert_eq!(pool.total_used(), 800);
            // ticket がスコープを離脱 → Drop で解放
        }
        assert_eq!(pool.total_used(), 0);
    }
}
