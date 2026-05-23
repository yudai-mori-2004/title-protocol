// SPDX-License-Identifier: Apache-2.0

//! # Two-tier threshold ResourcePool with Ticket-based reservation
//!
//! Spec SS4.1, SS4.2 -- TEE memory management
//!
//! Manages memory usage across concurrent requests in the TEE using a single
//! `AtomicUsize` counter and two thresholds:
//!
//! - **`admission_limit`**: New request admission threshold.
//!   Only admits new requests when `used < admission_limit`.
//!   Reserves headroom for in-progress requests to extend.
//!
//! - **`total_limit`**: Absolute ceiling for all `extend()` calls.
//!   In-progress requests can use memory up to this limit. Beyond this,
//!   `extend()` fails.
//!
//! ```text
//! |<--- new requests OK --->|<--- in-progress only --->|<--- OS/unmanaged --->|
//! 0                   admission_limit            total_limit         TEE memory
//! ```
//!
//! `Ticket` is a RAII handle that tracks reserved bytes:
//! - `extend(n)`: CAS-loop adds `n` bytes (non-blocking, lock-free)
//! - `shrink(n)`: Returns `n` bytes to the pool
//! - `Drop`: Releases all remaining reservation
//!
//! ## Timeout support (SS4.4)
//!
//! Each Ticket records creation time and last activity time:
//! - **Chunk timeout**: Rejects extend if no activity within the chunk timeout
//! - **Global timeout**: Rejects extend if total elapsed exceeds the request's
//!   global timeout
//!
//! ## Design notes (from legacy v0.1.0)
//!
//! The CAS-loop pattern in `extend()` is carried forward from
//! `legacy/v0.1.0/crates/wasm-host/src/resource_pool.rs`.
//! It provides lock-free, non-blocking reservation under contention.

use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::limits::{self, LimitsError};

// ---------------------------------------------------------------------------
// ResourcePool
// ---------------------------------------------------------------------------

/// Two-tier threshold resource pool.
/// Spec SS4.1
#[derive(Debug)]
pub struct ResourcePool {
    /// New request admission threshold (Ticket issuance check).
    admission_limit: usize,
    /// Absolute memory ceiling for all Ticket reservations combined.
    total_limit: usize,
    /// Sum of all Ticket reservations (bytes).
    used: AtomicUsize,
}

impl ResourcePool {
    /// Create a pool with separate admission and total limits.
    ///
    /// - `admission_limit`: `used` ceiling for new request admission
    /// - `total_limit`: Absolute ceiling for all extend() calls
    ///
    /// # Panics
    /// Panics if `admission_limit > total_limit`.
    pub fn new(admission_limit: usize, total_limit: usize) -> Self {
        assert!(
            admission_limit <= total_limit,
            "admission_limit ({admission_limit}) must be <= total_limit ({total_limit})"
        );
        Self {
            admission_limit,
            total_limit,
            used: AtomicUsize::new(0),
        }
    }

    /// Create a pool with a single limit (admission = total).
    /// Convenient for testing.
    pub fn with_single_limit(limit: usize) -> Self {
        Self::new(limit, limit)
    }

    /// Check whether a new request can be admitted.
    /// Returns `true` if `used < admission_limit`.
    /// Spec SS4.1
    pub fn can_admit(&self) -> bool {
        self.used.load(Ordering::Acquire) < self.admission_limit
    }

    /// Try to issue a zero-byte Ticket for a new request.
    /// Returns `None` if the admission limit is exceeded (HTTP 503).
    /// Spec SS4.1
    ///
    /// The returned Ticket has the default global timeout computed from
    /// `data_size_hint`. If the data size is unknown, pass 0.
    pub fn try_admit(self: &Arc<Self>, data_size_hint: u64) -> Option<Ticket> {
        if self.can_admit() {
            let global_timeout = limits::compute_global_timeout(data_size_hint);
            Some(Ticket::new(
                Arc::clone(self),
                global_timeout,
                limits::CHUNK_TIMEOUT,
            ))
        } else {
            None
        }
    }

    /// Issue a zero-byte Ticket unconditionally.
    /// For internal/in-progress operations that bypass admission checks.
    pub fn ticket(self: &Arc<Self>, data_size_hint: u64) -> Ticket {
        let global_timeout = limits::compute_global_timeout(data_size_hint);
        Ticket::new(Arc::clone(self), global_timeout, limits::CHUNK_TIMEOUT)
    }

    /// Issue a Ticket with custom timeouts.
    /// For testing or requests with special timeout requirements.
    pub fn ticket_with_timeouts(
        self: &Arc<Self>,
        global_timeout: Duration,
        chunk_timeout: Duration,
    ) -> Ticket {
        Ticket::new(Arc::clone(self), global_timeout, chunk_timeout)
    }

    /// One-shot: issue Ticket + extend. Returns `None` if total_limit exceeded.
    pub fn acquire(self: &Arc<Self>, size: usize) -> Option<Ticket> {
        let ticket = self.ticket(0);
        if ticket.extend(size).is_ok() {
            Some(ticket)
        } else {
            None
        }
    }

    /// Current total memory usage (bytes). For monitoring and tests.
    pub fn total_used(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }

    /// The total_limit of this pool. For decoded-size validation.
    pub fn total_limit(&self) -> usize {
        self.total_limit
    }

    /// The admission_limit of this pool. For monitoring.
    pub fn admission_limit(&self) -> usize {
        self.admission_limit
    }
}

// ---------------------------------------------------------------------------
// Ticket
// ---------------------------------------------------------------------------

/// RAII reservation handle. Releases all reserved bytes on Drop.
/// Spec SS4.2
///
/// A Ticket belongs to a single request thread and must not be shared
/// across threads (it is `Send` but not `Sync` due to `Cell<Instant>`).
pub struct Ticket {
    pool: Arc<ResourcePool>,
    reserved: AtomicUsize,
    /// When this Ticket was created.
    created_at: Instant,
    /// Last time data was received (extend was called).
    /// Uses `Cell` for interior mutability -- safe because Ticket is
    /// accessed from a single request thread.
    last_activity: Cell<Instant>,
    /// Global timeout for this request.
    /// Spec SS4.4 -- adaptive: min(max, base + size/speed)
    global_timeout: Duration,
    /// Chunk timeout -- max time between consecutive data arrivals.
    /// Spec SS4.4 -- default 60s.
    chunk_timeout: Duration,
}

/// Error returned when a Ticket operation fails.
#[derive(Debug, thiserror::Error)]
pub enum TicketError {
    /// The extend would exceed total_limit.
    #[error("Memory reservation would exceed total_limit ({requested} bytes requested, {available} available)")]
    TotalLimitExceeded {
        /// Bytes requested.
        requested: usize,
        /// Bytes available before hitting total_limit.
        available: usize,
    },

    /// Chunk timeout exceeded -- no data received within the window.
    #[error("Chunk timeout exceeded: {0}")]
    ChunkTimeout(LimitsError),

    /// Global timeout exceeded.
    #[error("Global timeout exceeded")]
    GlobalTimeout(Duration),

    /// Integer overflow in reservation calculation.
    #[error("Integer overflow in memory reservation")]
    Overflow,
}

impl Ticket {
    /// Create a new Ticket with explicit timeouts.
    fn new(pool: Arc<ResourcePool>, global_timeout: Duration, chunk_timeout: Duration) -> Self {
        let now = Instant::now();
        Self {
            pool,
            reserved: AtomicUsize::new(0),
            created_at: now,
            last_activity: Cell::new(now),
            global_timeout,
            chunk_timeout,
        }
    }

    /// Reserve additional bytes from the pool.
    /// Spec SS4.2 -- incremental reservation via CAS loop.
    ///
    /// Also checks chunk timeout and global timeout (SS4.4).
    ///
    /// # Returns
    /// `Ok(())` if the reservation succeeded.
    /// `Err(TicketError)` if total_limit exceeded, timed out, or overflow.
    pub fn extend(&self, additional: usize) -> Result<(), TicketError> {
        if additional == 0 {
            return Ok(());
        }

        // Check global timeout
        // Spec SS4.4 -- total request processing time
        if self.created_at.elapsed() > self.global_timeout {
            return Err(TicketError::GlobalTimeout(self.global_timeout));
        }

        // Check chunk timeout
        // Spec SS4.4 -- time since last data arrival
        if self.last_activity.get().elapsed() > self.chunk_timeout {
            return Err(TicketError::ChunkTimeout(
                LimitsError::ChunkTimeoutExceeded {
                    timeout: self.chunk_timeout,
                },
            ));
        }

        self.extend_inner(additional)?;

        // Update last_activity on successful extend
        self.last_activity.set(Instant::now());

        Ok(())
    }

    /// Extend without timeout checks.
    /// For internal use where timeout enforcement is handled at a higher level,
    /// or in concurrent tests where timing is unpredictable.
    pub fn extend_unchecked(&self, additional: usize) -> Result<(), TicketError> {
        if additional == 0 {
            return Ok(());
        }
        self.extend_inner(additional)
    }

    /// Core CAS-loop reservation logic shared by extend() and extend_unchecked().
    fn extend_inner(&self, additional: usize) -> Result<(), TicketError> {
        loop {
            let current = self.pool.used.load(Ordering::Acquire);
            let new_total = current
                .checked_add(additional)
                .ok_or(TicketError::Overflow)?;
            if new_total > self.pool.total_limit {
                return Err(TicketError::TotalLimitExceeded {
                    requested: additional,
                    available: self.pool.total_limit.saturating_sub(current),
                });
            }
            if self
                .pool
                .used
                .compare_exchange_weak(current, new_total, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                self.reserved.fetch_add(additional, Ordering::AcqRel);
                return Ok(());
            }
        }
    }

    /// Return bytes to the pool.
    /// Spec SS4.2
    ///
    /// # Panics
    /// Debug-asserts that `amount <= reserved`.
    pub fn shrink(&self, amount: usize) {
        if amount == 0 {
            return;
        }
        debug_assert!(
            amount <= self.reserved.load(Ordering::Acquire),
            "shrink({amount}) exceeds reserved({})",
            self.reserved.load(Ordering::Acquire)
        );
        self.reserved.fetch_sub(amount, Ordering::AcqRel);
        self.pool.used.fetch_sub(amount, Ordering::AcqRel);
    }

    /// Current reservation for this Ticket (bytes).
    pub fn reserved(&self) -> usize {
        self.reserved.load(Ordering::Acquire)
    }

    /// Check whether the global timeout has been exceeded.
    pub fn is_global_timeout_exceeded(&self) -> bool {
        self.created_at.elapsed() > self.global_timeout
    }

    /// Check whether the chunk timeout has been exceeded.
    pub fn is_chunk_timeout_exceeded(&self) -> bool {
        self.last_activity.get().elapsed() > self.chunk_timeout
    }

    /// Elapsed time since Ticket creation.
    pub fn elapsed(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// The global timeout configured for this Ticket.
    pub fn global_timeout(&self) -> Duration {
        self.global_timeout
    }

    /// Validate that an estimated decoded size fits within total_limit.
    /// Spec SS4.4 -- decode memory protection.
    ///
    /// # Arguments
    /// * `estimated_bytes` -- Estimated memory needed for decoding
    ///
    /// # Returns
    /// `Ok(())` if within limits, `Err` otherwise.
    pub fn validate_decoded_size(&self, estimated_bytes: u64) -> Result<(), LimitsError> {
        if estimated_bytes > self.pool.total_limit as u64 {
            Err(LimitsError::DecodedSizeExceeded {
                estimated: estimated_bytes,
                limit: self.pool.total_limit,
            })
        } else {
            Ok(())
        }
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

impl std::fmt::Debug for Ticket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ticket")
            .field("reserved", &self.reserved.load(Ordering::Relaxed))
            .field("elapsed", &self.created_at.elapsed())
            .field("global_timeout", &self.global_timeout)
            .field("chunk_timeout", &self.chunk_timeout)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- ResourcePool basic operations ----

    #[test]
    fn basic_acquire_release() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        {
            let ticket = pool.acquire(500).unwrap();
            assert_eq!(pool.total_used(), 500);
            assert_eq!(ticket.reserved(), 500);
        }
        assert_eq!(pool.total_used(), 0);
    }

    #[test]
    fn acquire_exceeds_limit() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        let _t1 = pool.acquire(600).unwrap();
        assert!(pool.acquire(500).is_none());
        assert_eq!(pool.total_used(), 600);
    }

    // ---- Ticket extend/shrink ----

    #[test]
    fn ticket_incremental_extend() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        let ticket = pool.ticket(0);
        assert!(ticket.extend(300).is_ok());
        assert!(ticket.extend(200).is_ok());
        assert_eq!(ticket.reserved(), 500);
        assert_eq!(pool.total_used(), 500);
    }

    #[test]
    fn extend_exceeds_total_limit() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        let ticket = pool.ticket(0);
        assert!(ticket.extend(800).is_ok());
        let err = ticket.extend(300).unwrap_err();
        assert!(matches!(err, TicketError::TotalLimitExceeded { .. }));
        assert_eq!(ticket.reserved(), 800);
    }

    #[test]
    fn extend_zero_is_noop() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        let ticket = pool.ticket(0);
        assert!(ticket.extend(0).is_ok());
        assert_eq!(ticket.reserved(), 0);
        assert_eq!(pool.total_used(), 0);
    }

    #[test]
    fn shrink_partial_release() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        let ticket = pool.ticket(0);
        ticket.extend(800).unwrap();
        ticket.shrink(500);
        assert_eq!(pool.total_used(), 300);
        assert_eq!(ticket.reserved(), 300);
        // Pool should have room now
        assert!(pool.acquire(600).is_some());
    }

    #[test]
    fn shrink_then_drop() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        {
            let ticket = pool.ticket(0);
            ticket.extend(800).unwrap();
            ticket.shrink(300);
            assert_eq!(pool.total_used(), 500);
        }
        assert_eq!(pool.total_used(), 0);
    }

    #[test]
    fn shrink_zero_is_noop() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        let ticket = pool.ticket(0);
        ticket.extend(100).unwrap();
        ticket.shrink(0);
        assert_eq!(ticket.reserved(), 100);
    }

    // ---- Two-tier admission ----

    #[test]
    fn two_tier_admission() {
        // admission_limit=600, total_limit=1000
        let pool = Arc::new(ResourcePool::new(600, 1000));

        // New request when used=0: admitted
        let t1 = pool.try_admit(0).expect("should admit when empty");
        t1.extend(500).unwrap();
        assert_eq!(pool.total_used(), 500);

        // New request when used=500 < 600: still admitted
        let t2 = pool.try_admit(0).expect("should admit under admission_limit");
        t2.extend(100).unwrap();
        assert_eq!(pool.total_used(), 600);

        // New request when used=600 >= 600: rejected
        assert!(
            pool.try_admit(0).is_none(),
            "should reject at admission_limit"
        );

        // But in-progress ticket can still extend up to total_limit
        t1.extend(300).unwrap(); // 600 + 300 = 900 < 1000
        assert_eq!(pool.total_used(), 900);

        // In-progress extend beyond total_limit fails
        let err = t2.extend(200).unwrap_err(); // 900 + 200 = 1100 > 1000
        assert!(matches!(err, TicketError::TotalLimitExceeded { .. }));
    }

    // ---- Multiple tickets sharing pool ----

    #[test]
    fn multiple_tickets_share_pool() {
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

    // ---- Drop releases reservation ----

    #[test]
    fn drop_releases_all_reservation() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        {
            let ticket = pool.ticket(0);
            ticket.extend(500).unwrap();
            ticket.extend(300).unwrap();
            assert_eq!(pool.total_used(), 800);
        }
        assert_eq!(pool.total_used(), 0);
    }

    // ---- Timeout: config ----

    #[test]
    fn global_timeout_configured_from_data_size() {
        let pool = Arc::new(ResourcePool::with_single_limit(1_000_000));
        // 10 MB hint: base 60s + 10MB/64KB/s = 60 + 160 = 220s
        let ticket = pool.ticket(10 * 1024 * 1024);
        assert_eq!(ticket.global_timeout(), Duration::from_secs(220));
    }

    #[test]
    fn global_timeout_zero_hint_gives_base() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        let ticket = pool.ticket(0);
        assert_eq!(ticket.global_timeout(), limits::BASE_TIMEOUT);
    }

    #[test]
    fn newly_created_ticket_not_timed_out() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        let ticket = pool.ticket(0);
        assert!(!ticket.is_global_timeout_exceeded());
        assert!(!ticket.is_chunk_timeout_exceeded());
    }

    // ---- Timeout: elapsed enforcement ----

    #[test]
    fn extend_rejected_after_global_timeout() {
        let pool = Arc::new(ResourcePool::with_single_limit(1_000_000));
        let ticket = pool.ticket_with_timeouts(
            Duration::from_millis(1), // very short global timeout
            Duration::from_secs(60),
        );
        std::thread::sleep(Duration::from_millis(10));
        let err = ticket.extend(100).unwrap_err();
        assert!(matches!(err, TicketError::GlobalTimeout(_)));
        // Reservation should not have changed
        assert_eq!(ticket.reserved(), 0);
        assert_eq!(pool.total_used(), 0);
    }

    #[test]
    fn extend_rejected_after_chunk_timeout() {
        let pool = Arc::new(ResourcePool::with_single_limit(1_000_000));
        let ticket = pool.ticket_with_timeouts(
            Duration::from_secs(60),
            Duration::from_millis(1), // very short chunk timeout
        );
        // First extend succeeds (created_at == last_activity, elapsed ~0)
        // Need to wait for chunk timeout to expire first
        std::thread::sleep(Duration::from_millis(10));
        let err = ticket.extend(100).unwrap_err();
        assert!(matches!(err, TicketError::ChunkTimeout(_)));
        assert_eq!(ticket.reserved(), 0);
    }

    #[test]
    fn extend_succeeds_within_chunk_timeout_window() {
        let pool = Arc::new(ResourcePool::with_single_limit(1_000_000));
        let ticket = pool.ticket_with_timeouts(
            Duration::from_secs(60),
            Duration::from_secs(1), // 1s chunk timeout -- plenty of time
        );
        // Rapid successive extends should all succeed
        ticket.extend(100).unwrap();
        ticket.extend(200).unwrap();
        ticket.extend(300).unwrap();
        assert_eq!(ticket.reserved(), 600);
    }

    #[test]
    fn extend_updates_last_activity() {
        let pool = Arc::new(ResourcePool::with_single_limit(1_000_000));
        let ticket = pool.ticket_with_timeouts(
            Duration::from_secs(60),
            Duration::from_millis(50), // 50ms chunk timeout
        );
        // Extend, wait 30ms (within window), extend again -- should succeed
        // because the first extend updated last_activity
        ticket.extend(100).unwrap();
        std::thread::sleep(Duration::from_millis(30));
        ticket.extend(100).unwrap(); // Should succeed, only 30ms since last
        assert_eq!(ticket.reserved(), 200);
    }

    // ---- Decoded size validation ----

    #[test]
    fn validate_decoded_size_within_limit() {
        let pool = Arc::new(ResourcePool::with_single_limit(100 * 1024 * 1024)); // 100 MB
        let ticket = pool.ticket(0);
        // 1920x1080 RGB = ~6 MB -- well within limit
        let size = limits::estimate_decoded_size(1920, 1080, 3, 8);
        assert!(ticket.validate_decoded_size(size).is_ok());
    }

    #[test]
    fn validate_decoded_size_exceeds_limit() {
        let pool = Arc::new(ResourcePool::with_single_limit(10 * 1024 * 1024)); // 10 MB
        let ticket = pool.ticket(0);
        // 4096x4096 RGBA = ~64 MB -- exceeds 10 MB limit
        let size = limits::estimate_decoded_size(4096, 4096, 4, 8);
        let err = ticket.validate_decoded_size(size).unwrap_err();
        assert!(matches!(err, LimitsError::DecodedSizeExceeded { .. }));
    }

    // ---- Pool accessors ----

    #[test]
    fn pool_accessors() {
        let pool = ResourcePool::new(500, 1000);
        assert_eq!(pool.admission_limit(), 500);
        assert_eq!(pool.total_limit(), 1000);
        assert_eq!(pool.total_used(), 0);
    }

    // ---- Concurrent access ----

    #[test]
    fn concurrent_extend_shrink_no_data_race() {
        use std::thread;

        let pool = Arc::new(ResourcePool::with_single_limit(1_000_000));
        let mut handles = vec![];

        for _ in 0..8 {
            let pool = Arc::clone(&pool);
            handles.push(thread::spawn(move || {
                // Each thread owns its Ticket -- no cross-thread sharing
                let ticket = pool.ticket(0);
                for _ in 0..100 {
                    ticket.extend_unchecked(100).unwrap();
                    ticket.shrink(100);
                }
                assert_eq!(ticket.reserved(), 0);
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(
            pool.total_used(),
            0,
            "Pool should be empty after all threads complete"
        );
    }

    #[test]
    fn concurrent_ticket_lifecycle() {
        use std::thread;

        let pool = Arc::new(ResourcePool::new(800_000, 1_000_000));
        let mut handles = vec![];

        // Spawn 10 threads, each acquiring and releasing tickets
        for i in 0..10 {
            let pool = Arc::clone(&pool);
            handles.push(thread::spawn(move || {
                if let Some(ticket) = pool.try_admit(0) {
                    let amount = (i + 1) * 1000;
                    if ticket.extend_unchecked(amount).is_ok() {
                        std::hint::black_box(&ticket);
                        ticket.shrink(amount / 2);
                    }
                }
                // Ticket drops here, releasing everything
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(
            pool.total_used(),
            0,
            "Pool must be empty after all tickets dropped"
        );
    }

    // ---- extend_unchecked ----

    #[test]
    fn extend_unchecked_skips_timeout() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        let ticket = pool.ticket(0);
        assert!(ticket.extend_unchecked(500).is_ok());
        assert_eq!(ticket.reserved(), 500);
    }

    #[test]
    fn extend_unchecked_respects_total_limit() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        let ticket = pool.ticket(0);
        ticket.extend_unchecked(800).unwrap();
        let err = ticket.extend_unchecked(300).unwrap_err();
        assert!(matches!(err, TicketError::TotalLimitExceeded { .. }));
    }

    // ---- Debug impl ----

    #[test]
    fn ticket_debug_format() {
        let pool = Arc::new(ResourcePool::with_single_limit(1000));
        let ticket = pool.ticket(0);
        ticket.extend_unchecked(42).unwrap();
        let debug = format!("{ticket:?}");
        assert!(debug.contains("Ticket"));
        assert!(debug.contains("42"));
    }
}
