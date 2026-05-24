// SPDX-License-Identifier: Apache-2.0

//! # Data Size Limits and Timeout Configuration
//!
//! Spec §4.4 -- Attack defense parameters
//!
//! Defines constants and utilities for enforcing data size limits,
//! chunk timeouts, and global request timeouts.

use std::time::Duration;

// ---------------------------------------------------------------------------
// Data size limits (§4.4)
// ---------------------------------------------------------------------------

/// Maximum number of fragments in a fragmented input.
/// Spec §4.4 -- 2-second segments x 100,000 ~= 55 hours of video.
pub const MAX_FRAGMENT_COUNT: usize = 100_000;

/// Maximum size of a single fragment in bytes.
/// Spec §4.4 -- 100 MB covers 10-second high-resolution video segments.
pub const MAX_FRAGMENT_SIZE: usize = 100 * 1024 * 1024; // 100 MB

// ---------------------------------------------------------------------------
// Timeout configuration (§4.4)
// ---------------------------------------------------------------------------

/// Chunk timeout: maximum time between consecutive data arrivals.
/// Spec §4.4 -- 60 seconds. Prevents slow-loris style resource occupation.
pub const CHUNK_TIMEOUT: Duration = Duration::from_secs(60);

/// Maximum global timeout for a single request.
/// Spec §4.4 -- 30 minutes.
pub const MAX_GLOBAL_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Base timeout for small requests.
/// Used in adaptive timeout calculation.
pub const BASE_TIMEOUT: Duration = Duration::from_secs(60);

/// Minimum transfer speed assumption for timeout calculation.
/// Spec §4.4 -- bytes per second. Used to compute size-adaptive timeouts.
/// 64 KB/s is conservative enough to accommodate slow connections while
/// still bounding processing time.
pub const MIN_TRANSFER_SPEED: u64 = 64 * 1024; // 64 KB/s

// ---------------------------------------------------------------------------
// Timeout calculation
// ---------------------------------------------------------------------------

/// Compute the adaptive global timeout for a request.
/// Spec §4.4 — `timeout = min(MAX_GLOBAL_TIMEOUT, BASE_TIMEOUT + size / MIN_SPEED)`.
///
/// `data_size_hint` is `Some(bytes)` when the request payload reveals a
/// size up front (sum of fragment sizes for `Fragmented`, header size for
/// `Single`). For streaming requests with unknown size, pass `None` and the
/// function returns `MAX_GLOBAL_TIMEOUT` so the request gets the full
/// 30-minute budget rather than collapsing onto `BASE_TIMEOUT`.
pub fn compute_global_timeout(data_size_hint: Option<u64>) -> Duration {
    let Some(bytes) = data_size_hint else {
        return MAX_GLOBAL_TIMEOUT;
    };
    let transfer_secs = bytes / MIN_TRANSFER_SPEED;
    let total_secs = BASE_TIMEOUT.as_secs().saturating_add(transfer_secs);
    Duration::from_secs(total_secs.min(MAX_GLOBAL_TIMEOUT.as_secs()))
}

// ---------------------------------------------------------------------------
// Validation utilities
// ---------------------------------------------------------------------------

/// Validate fragment count against the maximum limit.
/// Spec §4.4 -- rejects requests exceeding MAX_FRAGMENT_COUNT.
///
/// # Returns
/// `Ok(())` if within limits, `Err` with a descriptive message otherwise.
pub fn validate_fragment_count(count: usize) -> Result<(), LimitsError> {
    if count > MAX_FRAGMENT_COUNT {
        Err(LimitsError::FragmentCountExceeded {
            count,
            max: MAX_FRAGMENT_COUNT,
        })
    } else {
        Ok(())
    }
}

/// Validate a single fragment size against the maximum limit.
/// Spec §4.4 -- rejects fragments exceeding MAX_FRAGMENT_SIZE.
///
/// # Returns
/// `Ok(())` if within limits, `Err` with a descriptive message otherwise.
pub fn validate_fragment_size(size: usize) -> Result<(), LimitsError> {
    if size > MAX_FRAGMENT_SIZE {
        Err(LimitsError::FragmentSizeExceeded {
            size,
            max: MAX_FRAGMENT_SIZE,
        })
    } else {
        Ok(())
    }
}

/// Estimate decoded memory size from image header information.
/// Spec §4.4 — decode memory protection.
pub fn estimate_decoded_size(width: u32, height: u32, channels: u32, bit_depth: u32) -> u64 {
    let bytes_per_pixel = (channels * bit_depth).div_ceil(8);
    u64::from(width) * u64::from(height) * u64::from(bytes_per_pixel)
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Error type for data size limit violations.
/// Spec §4.4
#[derive(Debug, thiserror::Error)]
pub enum LimitsError {
    /// Fragment count exceeds the maximum.
    #[error("Fragment count {count} exceeds maximum {max}")]
    FragmentCountExceeded {
        /// Actual fragment count.
        count: usize,
        /// Maximum allowed.
        max: usize,
    },

    /// Single fragment size exceeds the maximum.
    #[error("Fragment size {size} bytes exceeds maximum {max} bytes")]
    FragmentSizeExceeded {
        /// Actual size in bytes.
        size: usize,
        /// Maximum allowed in bytes.
        max: usize,
    },

    /// Estimated decoded size exceeds the memory limit.
    #[error("Estimated decoded size {estimated} bytes exceeds total_limit {limit} bytes")]
    DecodedSizeExceeded {
        /// Estimated decoded size in bytes.
        estimated: u64,
        /// total_limit in bytes.
        limit: usize,
    },

    /// Chunk timeout exceeded (no data arrived within the window).
    #[error("No data received within chunk timeout of {timeout:?}")]
    ChunkTimeoutExceeded {
        /// The chunk timeout that was exceeded.
        timeout: Duration,
    },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_timeout_small_file() {
        // 1 MB file: 60s base + 1MB/64KB/s = 60 + 16 = 76s
        let timeout = compute_global_timeout(Some(1_024 * 1_024));
        assert_eq!(timeout, Duration::from_secs(76));
    }

    #[test]
    fn global_timeout_large_file() {
        // 10 GB clamped to MAX_GLOBAL_TIMEOUT
        let timeout = compute_global_timeout(Some(10 * 1024 * 1024 * 1024));
        assert_eq!(timeout, MAX_GLOBAL_TIMEOUT);
    }

    #[test]
    fn global_timeout_zero_size_hint_is_base() {
        let timeout = compute_global_timeout(Some(0));
        assert_eq!(timeout, BASE_TIMEOUT);
    }

    #[test]
    fn global_timeout_unknown_uses_max() {
        let timeout = compute_global_timeout(None);
        assert_eq!(timeout, MAX_GLOBAL_TIMEOUT);
    }

    #[test]
    fn validate_fragment_count_within_limit() {
        assert!(validate_fragment_count(1).is_ok());
        assert!(validate_fragment_count(100_000).is_ok());
    }

    #[test]
    fn validate_fragment_count_exceeds_limit() {
        let err = validate_fragment_count(100_001).unwrap_err();
        assert!(matches!(
            err,
            LimitsError::FragmentCountExceeded {
                count: 100_001,
                max: 100_000
            }
        ));
    }

    #[test]
    fn validate_fragment_size_within_limit() {
        assert!(validate_fragment_size(0).is_ok());
        assert!(validate_fragment_size(100 * 1024 * 1024).is_ok());
    }

    #[test]
    fn validate_fragment_size_exceeds_limit() {
        let err = validate_fragment_size(100 * 1024 * 1024 + 1).unwrap_err();
        assert!(matches!(err, LimitsError::FragmentSizeExceeded { .. }));
    }

    #[test]
    fn estimate_decoded_size_rgb8() {
        // 1920x1080 RGB 8-bit = 1920 * 1080 * 3 = 6,220,800 bytes
        let size = estimate_decoded_size(1920, 1080, 3, 8);
        assert_eq!(size, 6_220_800);
    }

    #[test]
    fn estimate_decoded_size_rgba16() {
        // 4096x4096 RGBA 16-bit = 4096 * 4096 * 8 = 134,217,728 bytes (128 MB)
        let size = estimate_decoded_size(4096, 4096, 4, 16);
        assert_eq!(size, 134_217_728);
    }

    #[test]
    fn estimate_decoded_size_handles_large_dimensions() {
        // Very large image: 65535x65535 RGBA 8-bit
        // Should not overflow due to u64 arithmetic
        let size = estimate_decoded_size(65535, 65535, 4, 8);
        assert_eq!(size, u64::from(65535u32) * u64::from(65535u32) * 4);
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn constants_are_consistent() {
        // Asserts on consts so a future tweak that swaps the ordering
        // (e.g. setting CHUNK_TIMEOUT > MAX_GLOBAL_TIMEOUT) is caught.
        // Duration's `<` isn't const, so this stays a runtime test.
        assert!(CHUNK_TIMEOUT < MAX_GLOBAL_TIMEOUT);
        assert!(BASE_TIMEOUT <= MAX_GLOBAL_TIMEOUT);
        assert!(MIN_TRANSFER_SPEED > 0);
    }
}
