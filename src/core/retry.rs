//! Retry configuration with increasing intervals.

use std::time::Duration;

/// Configuration for retry behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryConfig {
    /// Maximum number of retry attempts.
    pub max_attempts: u32,
    /// Base interval between retries in seconds.
    pub base_interval_secs: u64,
    /// Amount to increase interval after each failure.
    pub interval_increment_secs: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 100,
            base_interval_secs: 10,
            interval_increment_secs: 10,
        }
    }
}

impl RetryConfig {
    /// Creates a new retry configuration with custom values.
    ///
    /// # Arguments
    ///
    /// * `max_attempts` - Maximum number of retry attempts
    /// * `base_interval_secs` - Initial wait time in seconds
    /// * `interval_increment_secs` - Amount to add after each failure
    #[must_use]
    pub const fn new(
        max_attempts: u32,
        base_interval_secs: u64,
        interval_increment_secs: u64,
    ) -> Self {
        Self {
            max_attempts,
            base_interval_secs,
            interval_increment_secs,
        }
    }

    /// Calculates the wait duration for a given attempt number.
    ///
    /// Uses linear backoff: base + (attempt * increment)
    ///
    /// # Examples
    ///
    /// ```
    /// use mcgravity::core::RetryConfig;
    /// use std::time::Duration;
    ///
    /// let config = RetryConfig::new(10, 5, 2);
    /// assert_eq!(config.wait_duration(0), Duration::from_secs(5));  // 5 + 0*2
    /// assert_eq!(config.wait_duration(1), Duration::from_secs(7));  // 5 + 1*2
    /// assert_eq!(config.wait_duration(5), Duration::from_secs(15)); // 5 + 5*2
    /// ```
    #[must_use]
    pub const fn wait_duration(&self, attempt: u32) -> Duration {
        let secs = self.base_interval_secs + (attempt as u64 * self.interval_increment_secs);
        Duration::from_secs(secs)
    }

    /// Returns true if the given attempt number is within the allowed limit.
    ///
    /// # Arguments
    ///
    /// * `attempt` - The current attempt number (1-indexed)
    #[must_use]
    pub const fn has_attempts_remaining(&self, attempt: u32) -> bool {
        attempt < self.max_attempts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that default configuration has expected values.
    #[test]
    fn default_has_expected_values() {
        let config = RetryConfig::default();

        assert_eq!(config.max_attempts, 100);
        assert_eq!(config.base_interval_secs, 10);
        assert_eq!(config.interval_increment_secs, 10);
    }

    /// Tests creating a custom configuration.
    #[test]
    fn new_sets_custom_values() {
        let config = RetryConfig::new(5, 2, 3);

        assert_eq!(config.max_attempts, 5);
        assert_eq!(config.base_interval_secs, 2);
        assert_eq!(config.interval_increment_secs, 3);
    }

    /// Tests wait duration for first attempt (attempt 0).
    #[test]
    fn wait_duration_first_attempt_returns_base() {
        let config = RetryConfig::new(10, 5, 10);

        assert_eq!(config.wait_duration(0), Duration::from_secs(5));
    }

    /// Tests wait duration uses linear backoff correctly.
    #[test]
    fn wait_duration_linear_backoff() {
        let config = RetryConfig::new(10, 10, 10);

        assert_eq!(config.wait_duration(0), Duration::from_secs(10)); // 10 + 0*10
        assert_eq!(config.wait_duration(1), Duration::from_secs(20)); // 10 + 1*10
        assert_eq!(config.wait_duration(2), Duration::from_secs(30)); // 10 + 2*10
        assert_eq!(config.wait_duration(5), Duration::from_secs(60)); // 10 + 5*10
        assert_eq!(config.wait_duration(10), Duration::from_secs(110)); // 10 + 10*10
    }

    /// Tests wait duration with zero increment (constant backoff).
    #[test]
    fn wait_duration_zero_increment_is_constant() {
        let config = RetryConfig::new(10, 5, 0);

        assert_eq!(config.wait_duration(0), Duration::from_secs(5));
        assert_eq!(config.wait_duration(1), Duration::from_secs(5));
        assert_eq!(config.wait_duration(100), Duration::from_secs(5));
    }

    /// Tests wait duration with zero base (starts from zero).
    #[test]
    fn wait_duration_zero_base() {
        let config = RetryConfig::new(10, 0, 5);

        assert_eq!(config.wait_duration(0), Duration::from_secs(0));
        assert_eq!(config.wait_duration(1), Duration::from_secs(5));
        assert_eq!(config.wait_duration(2), Duration::from_secs(10));
    }

    /// Tests `has_attempts_remaining` returns true when attempts remain.
    #[test]
    fn has_attempts_remaining_true_when_under_limit() {
        let config = RetryConfig::new(5, 10, 10);

        assert!(config.has_attempts_remaining(0));
        assert!(config.has_attempts_remaining(1));
        assert!(config.has_attempts_remaining(4));
    }

    /// Tests `has_attempts_remaining` returns false at limit.
    #[test]
    fn has_attempts_remaining_false_at_limit() {
        let config = RetryConfig::new(5, 10, 10);

        assert!(!config.has_attempts_remaining(5));
    }

    /// Tests `has_attempts_remaining` returns false over limit.
    #[test]
    fn has_attempts_remaining_false_over_limit() {
        let config = RetryConfig::new(5, 10, 10);

        assert!(!config.has_attempts_remaining(6));
        assert!(!config.has_attempts_remaining(100));
    }

    /// Tests that `RetryConfig` can be cloned.
    #[test]
    fn clone_creates_equal_copy() {
        let original = RetryConfig::new(5, 10, 15);
        let cloned = original.clone();

        assert_eq!(original, cloned);
    }

    /// Tests equality comparison.
    #[test]
    fn eq_same_values_are_equal() {
        let a = RetryConfig::new(5, 10, 15);
        let b = RetryConfig::new(5, 10, 15);

        assert_eq!(a, b);
    }

    /// Tests inequality when values differ.
    #[test]
    fn eq_different_values_are_not_equal() {
        let a = RetryConfig::new(5, 10, 15);
        let b = RetryConfig::new(5, 10, 20);

        assert_ne!(a, b);
    }

    /// Tests Debug trait implementation.
    #[test]
    fn debug_format_is_readable() {
        let config = RetryConfig::new(5, 10, 15);
        let debug_str = format!("{config:?}");

        assert!(debug_str.contains("RetryConfig"));
        assert!(debug_str.contains("max_attempts: 5"));
    }
}
