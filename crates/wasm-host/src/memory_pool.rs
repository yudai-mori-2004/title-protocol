// SPDX-License-Identifier: Apache-2.0

//! # メモリプール（セマフォ方式）
//!
//! 仕様書 §7.1
//!
//! 複数の独立したセマフォ（A: raw binary, B: decoded data, C: 将来拡張…）の
//! 合計使用量を `total_limit` 以下に維持する。
//!
//! 各セマフォはライフサイクルが異なるが、物理メモリは同一プールを共有する。
//! CAS ループによるスレッドセーフな acquire/release。

use std::sync::atomic::{AtomicUsize, Ordering};

/// メモリプール: 複数セマフォの合計使用量を total_limit 以下に保つ。
/// 仕様書 §7.1
pub struct MemoryPool {
    /// 全体のメモリ予算（バイト）
    total_limit: usize,
    /// Semaphore A: raw binary 使用量（将来の統合用）
    used_a: AtomicUsize,
    /// Semaphore B: decoded data 使用量
    used_b: AtomicUsize,
}

impl MemoryPool {
    /// 新しい MemoryPool を作成する。
    /// 仕様書 §7.1
    pub fn new(total_limit: usize) -> Self {
        Self {
            total_limit,
            used_a: AtomicUsize::new(0),
            used_b: AtomicUsize::new(0),
        }
    }

    /// Semaphore A のメモリ予約を試みる。
    /// A.used + B.used + size <= total_limit の場合に成功。
    /// 将来の統合用（本タスクでは未使用）。
    pub fn try_acquire_a(&self, size: usize) -> bool {
        loop {
            let current_a = self.used_a.load(Ordering::Acquire);
            let current_b = self.used_b.load(Ordering::Acquire);
            if current_a + current_b + size > self.total_limit {
                return false;
            }
            if self
                .used_a
                .compare_exchange_weak(
                    current_a,
                    current_a + size,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                return true;
            }
        }
    }

    /// Semaphore A の予約を解放する。
    pub fn release_a(&self, size: usize) {
        self.used_a.fetch_sub(size, Ordering::AcqRel);
    }

    /// Semaphore B のメモリ予約を試みる。
    /// A.used + B.used + size <= total_limit の場合に成功。
    /// 仕様書 §7.1 — デコード済みデータのメモリ予算管理。
    pub fn try_acquire_b(&self, size: usize) -> bool {
        loop {
            let current_b = self.used_b.load(Ordering::Acquire);
            let current_a = self.used_a.load(Ordering::Acquire);
            if current_a + current_b + size > self.total_limit {
                return false;
            }
            if self
                .used_b
                .compare_exchange_weak(
                    current_b,
                    current_b + size,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                return true;
            }
        }
    }

    /// Semaphore B の予約を解放する。
    pub fn release_b(&self, size: usize) {
        self.used_b.fetch_sub(size, Ordering::AcqRel);
    }

    /// 現在の合計使用量を返す（テスト・モニタリング用）。
    pub fn total_used(&self) -> usize {
        self.used_a.load(Ordering::Relaxed) + self.used_b.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_acquire_release_b() {
        let pool = MemoryPool::new(1000);
        assert!(pool.try_acquire_b(500));
        assert_eq!(pool.total_used(), 500);
        pool.release_b(500);
        assert_eq!(pool.total_used(), 0);
    }

    #[test]
    fn test_acquire_b_exceeds_limit() {
        let pool = MemoryPool::new(1000);
        assert!(pool.try_acquire_b(600));
        // 600 + 500 = 1100 > 1000
        assert!(!pool.try_acquire_b(500));
        assert_eq!(pool.total_used(), 600);
    }

    #[test]
    fn test_multiple_semaphores_share_pool() {
        let pool = MemoryPool::new(1000);
        assert!(pool.try_acquire_a(400));
        assert!(pool.try_acquire_b(400));
        assert_eq!(pool.total_used(), 800);
        // 400 + 400 + 300 = 1100 > 1000
        assert!(!pool.try_acquire_b(300));
        assert!(!pool.try_acquire_a(300));
    }

    #[test]
    fn test_release_frees_capacity() {
        let pool = MemoryPool::new(1000);
        assert!(pool.try_acquire_b(800));
        assert!(!pool.try_acquire_b(300));
        pool.release_b(800);
        assert!(pool.try_acquire_b(300));
        assert_eq!(pool.total_used(), 300);
    }
}
