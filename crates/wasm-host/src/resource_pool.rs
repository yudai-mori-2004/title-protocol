// SPDX-License-Identifier: Apache-2.0

//! Resource pool with two-tier admission control.
//!
//! Manages concurrent memory usage across all requests with a single
//! `AtomicUsize` counter and two thresholds:
//!
//! - **`admission_limit`**: New requests are accepted only when
//!   `used < admission_limit`. This reserves headroom for in-progress
//!   requests to extend without being starved by new arrivals.
//!
//! - **`total_limit`**: Absolute ceiling for all extend() calls.
//!   In-progress requests can use memory up to this limit. Beyond this,
//!   extend() fails and the specific operation returns an error.
//!
//! ```text
//! |← new requests OK →|← in-progress only →|← OS/unmanaged →|
//! 0              admission_limit        total_limit      enclave_max
//! ```
//!
//! `Ticket` is a RAII handle that tracks a reservation. It supports:
//! - `extend(n)`: atomically reserve `n` more bytes (CAS loop, non-blocking)
//! - `shrink(n)`: release `n` bytes back to the pool
//! - `Drop`: release all remaining reservation

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Resource pool with two-tier admission control.
#[derive(Debug)]
pub struct ResourcePool {
    /// Threshold for accepting new requests (new ticket creation).
    admission_limit: usize,
    /// Absolute memory ceiling for all extend() calls.
    total_limit: usize,
    /// Total bytes currently reserved across all tickets.
    used: AtomicUsize,
}

/// RAII reservation handle. Released automatically on drop.
#[derive(Debug)]
pub struct Ticket {
    pool: Arc<ResourcePool>,
    reserved: AtomicUsize,
}

impl ResourcePool {
    /// Create a pool with separate admission and total limits.
    ///
    /// - `admission_limit`: max `used` for accepting new requests
    /// - `total_limit`: absolute ceiling for all reservations
    pub fn new(admission_limit: usize, total_limit: usize) -> Self {
        assert!(admission_limit <= total_limit);
        Self {
            admission_limit,
            total_limit,
            used: AtomicUsize::new(0),
        }
    }

    /// Create a pool with a single limit (admission = total).
    /// Convenience for tests where two-tier control is not needed.
    pub fn with_single_limit(limit: usize) -> Self {
        Self::new(limit, limit)
    }

    /// Check if the pool can accept a new request.
    /// Returns true if `used < admission_limit`.
    pub fn can_admit(&self) -> bool {
        self.used.load(Ordering::Acquire) < self.admission_limit
    }

    /// Issue a new 0-byte ticket for a new request.
    /// Fails if the pool has exceeded the admission limit.
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

    /// Issue a 0-byte ticket unconditionally (for in-progress operations).
    pub fn ticket(self: &Arc<Self>) -> Ticket {
        Ticket {
            pool: Arc::clone(self),
            reserved: AtomicUsize::new(0),
        }
    }

    /// One-shot reserve: issue ticket + extend. Fails if over total_limit.
    pub fn acquire(self: &Arc<Self>, size: usize) -> Option<Ticket> {
        let ticket = self.ticket();
        if ticket.extend(size) {
            Some(ticket)
        } else {
            None
        }
    }

    /// Current total usage (for monitoring/tests).
    pub fn total_used(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }
}

impl Ticket {
    /// Reserve additional bytes. Fails if total_limit would be exceeded.
    /// CAS loop, non-blocking. Existing reservation is preserved on failure.
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

    /// Release bytes back to the pool.
    pub fn shrink(&self, amount: usize) {
        if amount == 0 {
            return;
        }
        self.reserved.fetch_sub(amount, Ordering::AcqRel);
        self.pool.used.fetch_sub(amount, Ordering::AcqRel);
    }

    /// Current reservation of this ticket.
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
