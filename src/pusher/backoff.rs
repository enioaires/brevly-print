//! Exponential backoff with jitter for the Pusher reconnect loop.
//!
//! D-06: initial 1 s, doubles each attempt, 60 s cap, ±25% jitter.
//! Jitter does NOT extend the result beyond the 60 s cap.
//!
//! Portable — no `#[cfg(windows)]` gate.

use std::time::Duration;

/// Compute the delay before the next Pusher reconnect attempt.
///
/// # Algorithm (D-06)
///
/// 1. Base = `1000ms * 2^attempt`, where `attempt` is clamped at 6 (so base ≤ 64 s).
/// 2. Base is capped at 60 000 ms before jitter.
/// 3. Jitter: multiply base by a random factor in `[0.75, 1.25]`.
/// 4. Final result is also capped at 60 000 ms — jitter never exceeds the ceiling.
///
/// # Example
///
/// ```
/// use std::time::Duration;
/// use brevly_print::pusher::backoff::backoff_delay;
///
/// let delay = backoff_delay(0);
/// // attempt 0: base = 1 s; jittered result is between 750 ms and 1 250 ms
/// assert!(delay.as_millis() <= 60_000);
/// ```
pub fn backoff_delay(attempt: u32) -> Duration {
    // Base: 1 s * 2^attempt, clamped at attempt=6 (64 s base) before cap
    let base_ms: u64 = 1000u64.saturating_mul(1u64 << attempt.min(6));
    // Cap base at 60 s before applying jitter
    let capped_ms = base_ms.min(60_000);
    // Jitter: random factor in [0.75, 1.25] — ±25% of the capped base
    let jitter_factor = 0.75 + rand::random::<f64>() * 0.50;
    let jittered_ms = (capped_ms as f64 * jitter_factor) as u64;
    // Final cap: jitter never extends beyond 60 s
    Duration::from_millis(jittered_ms.min(60_000))
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_never_exceeds_60s_cap() {
        // Run many attempts including high attempt numbers; none may exceed 60 s
        for attempt in 0..=20 {
            let delay = backoff_delay(attempt);
            assert!(
                delay.as_millis() <= 60_000,
                "attempt {attempt} returned {}ms — expected <= 60 000ms",
                delay.as_millis()
            );
        }
    }

    #[test]
    fn backoff_attempt_0_is_within_jitter_range() {
        // attempt 0: base = 1 s; with ±25% jitter → [750ms, 1250ms]
        // Run 20 times to get statistical coverage; all must be in range.
        for _ in 0..20 {
            let delay = backoff_delay(0);
            let ms = delay.as_millis();
            assert!(
                ms >= 750 && ms <= 1_250,
                "attempt 0 delay {ms}ms out of expected [750, 1250] range"
            );
        }
    }
}
