//! Bloom filter for event deduplication with hourly rotation.
//!
//! Sized for ~100M events/day at 0.1% false positive rate.
//! With hourly rotation, each filter handles ~4.17M events (100M / 24).
//!
//! Rotation strategy: maintain current + previous filter. Events are checked
//! against both (union) but only inserted into the current filter. Every hour
//! the current becomes previous and a fresh filter is allocated.
//! On restart (crash-only), both filters reset — brief dedup gap accepted per design doc.

use bloomfilter::Bloom;
use prometheus::{IntCounter, IntGauge};
use std::time::Instant;
use tracing::info;

/// Metrics for Bloom filter observability.
pub struct DedupMetrics {
    /// Estimated number of items in the current filter.
    pub items_current: IntGauge,
    /// Estimated number of items in the previous filter.
    pub items_previous: IntGauge,
    /// Number of filter rotations performed since startup.
    pub rotations_total: IntCounter,
    /// Number of duplicates detected.
    pub duplicates_total: IntCounter,
}

impl DedupMetrics {
    pub fn new(registry: &prometheus::Registry) -> Self {
        let items_current = IntGauge::new(
            "bloom_filter_items_current",
            "Estimated items in the current Bloom filter",
        )
        .unwrap();
        let items_previous = IntGauge::new(
            "bloom_filter_items_previous",
            "Estimated items in the previous Bloom filter",
        )
        .unwrap();
        let rotations_total = IntCounter::new(
            "bloom_filter_rotations_total",
            "Total number of Bloom filter rotations",
        )
        .unwrap();
        let duplicates_total = IntCounter::new(
            "bloom_filter_duplicates_total",
            "Total duplicate events detected by Bloom filter",
        )
        .unwrap();

        registry.register(Box::new(items_current.clone())).unwrap();
        registry
            .register(Box::new(items_previous.clone()))
            .unwrap();
        registry
            .register(Box::new(rotations_total.clone()))
            .unwrap();
        registry
            .register(Box::new(duplicates_total.clone()))
            .unwrap();

        Self {
            items_current,
            items_previous,
            rotations_total,
            duplicates_total,
        }
    }

    /// Create a no-op metrics instance (for testing without a registry).
    pub fn noop() -> Self {
        Self {
            items_current: IntGauge::new("noop_items_current", "noop").unwrap(),
            items_previous: IntGauge::new("noop_items_previous", "noop").unwrap(),
            rotations_total: IntCounter::new("noop_rotations_total", "noop").unwrap(),
            duplicates_total: IntCounter::new("noop_duplicates_total", "noop").unwrap(),
        }
    }
}

/// Configuration for the dedup filter.
pub struct DedupConfig {
    /// Expected events per rotation interval (e.g., 100M/day / 24 hours ~ 4.17M per hour).
    pub items_per_interval: usize,
    /// Target false positive rate per filter (e.g., 0.001 for 0.1%).
    pub fp_rate: f64,
    /// Rotation interval in seconds (default: 3600 = 1 hour).
    pub rotation_interval_secs: u64,
}

impl Default for DedupConfig {
    fn default() -> Self {
        Self {
            // 100M events/day / 24 hours ~ 4,166,667 per hour
            items_per_interval: 4_166_667,
            fp_rate: 0.001,
            rotation_interval_secs: 3600,
        }
    }
}

impl DedupConfig {
    /// Create a config from daily event volume and FPR target.
    pub fn from_daily(expected_daily: usize, fp_rate: f64) -> Self {
        Self {
            items_per_interval: expected_daily / 24,
            fp_rate,
            rotation_interval_secs: 3600,
        }
    }

    /// Compute the optimal Bloom filter bit count for configured parameters.
    /// Formula: m = -(n * ln(p)) / (ln(2)^2)
    pub fn optimal_bits(&self) -> u64 {
        let n = self.items_per_interval as f64;
        let p = self.fp_rate;
        let m = -(n * p.ln()) / (2.0_f64.ln().powi(2));
        m.ceil() as u64
    }

    /// Compute the optimal number of hash functions.
    /// Formula: k = (m/n) * ln(2)
    pub fn optimal_hashes(&self) -> u32 {
        let m = self.optimal_bits() as f64;
        let n = self.items_per_interval as f64;
        let k = (m / n) * 2.0_f64.ln();
        k.round().max(1.0) as u32
    }

    /// Compute the memory usage per filter in bytes.
    pub fn filter_size_bytes(&self) -> u64 {
        self.optimal_bits().div_ceil(8)
    }
}

/// Rotating Bloom filter deduplicator.
///
/// Maintains two filters: current and previous. On rotation, the current filter
/// becomes previous, and a fresh filter is allocated. Events are checked against
/// both filters (union) but only inserted into the current one.
pub struct EventDedup {
    current: Bloom<str>,
    previous: Option<Bloom<str>>,
    items_per_interval: usize,
    fp_rate: f64,
    rotation_interval_secs: u64,
    last_rotation: Instant,
    current_count: usize,
    previous_count: usize,
    metrics: DedupMetrics,
}

impl EventDedup {
    /// Create a new dedup filter with the given config and Prometheus metrics.
    pub fn with_config(config: DedupConfig, metrics: DedupMetrics) -> Self {
        info!(
            items_per_interval = config.items_per_interval,
            fp_rate = config.fp_rate,
            rotation_interval_secs = config.rotation_interval_secs,
            optimal_bits = config.optimal_bits(),
            optimal_hashes = config.optimal_hashes(),
            filter_size_mb = config.filter_size_bytes() as f64 / (1024.0 * 1024.0),
            "Initializing rotating Bloom filter"
        );

        Self {
            current: Bloom::new_for_fp_rate(config.items_per_interval, config.fp_rate),
            previous: None,
            items_per_interval: config.items_per_interval,
            fp_rate: config.fp_rate,
            rotation_interval_secs: config.rotation_interval_secs,
            last_rotation: Instant::now(),
            current_count: 0,
            previous_count: 0,
            metrics,
        }
    }

    /// Create a new dedup filter sized for `expected_items` with `fp_rate` false positive rate.
    /// This is the simple non-rotating constructor (backwards compatible).
    pub fn new(expected_items: usize, fp_rate: f64) -> Self {
        Self {
            current: Bloom::new_for_fp_rate(expected_items, fp_rate),
            previous: None,
            items_per_interval: expected_items,
            fp_rate,
            rotation_interval_secs: u64::MAX, // Never rotate
            last_rotation: Instant::now(),
            current_count: 0,
            previous_count: 0,
            metrics: DedupMetrics::noop(),
        }
    }

    /// Check if rotation is needed and perform it.
    fn maybe_rotate(&mut self) {
        if self.last_rotation.elapsed().as_secs() >= self.rotation_interval_secs {
            self.rotate();
        }
    }

    /// Force a rotation: current -> previous, allocate fresh current.
    pub fn rotate(&mut self) {
        let prev_count = self.current_count;
        self.previous = Some(std::mem::replace(
            &mut self.current,
            Bloom::new_for_fp_rate(self.items_per_interval, self.fp_rate),
        ));
        self.previous_count = prev_count;
        self.current_count = 0;
        self.last_rotation = Instant::now();

        self.metrics.rotations_total.inc();
        self.metrics.items_current.set(0);
        self.metrics
            .items_previous
            .set(self.previous_count as i64);

        info!(
            previous_items = prev_count,
            "Bloom filter rotated"
        );
    }

    /// Check if an event_id has been seen. Returns true if likely duplicate.
    /// Checks both current and previous filters (union).
    pub fn is_duplicate(&mut self, event_id: &str) -> bool {
        self.maybe_rotate();

        // Check current filter
        if self.current.check(event_id) {
            self.metrics.duplicates_total.inc();
            return true;
        }

        // Check previous filter (if exists)
        if let Some(ref prev) = self.previous {
            if prev.check(event_id) {
                // Seen in previous window — still a duplicate, but also insert
                // into current so it survives the next rotation
                self.current.set(event_id);
                self.current_count += 1;
                self.metrics.items_current.set(self.current_count as i64);
                self.metrics.duplicates_total.inc();
                return true;
            }
        }

        // New event: insert into current filter
        self.current.set(event_id);
        self.current_count += 1;
        self.metrics.items_current.set(self.current_count as i64);
        false
    }

    /// Get the current item count across both filters.
    pub fn total_items(&self) -> usize {
        self.current_count + self.previous_count
    }

    /// Get the current filter's item count.
    pub fn current_items(&self) -> usize {
        self.current_count
    }

    /// Get the previous filter's item count.
    pub fn previous_items(&self) -> usize {
        self.previous_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_first_seen_not_duplicate() {
        let mut dedup = EventDedup::new(1000, 0.01);
        assert!(!dedup.is_duplicate("evt-1"));
    }

    #[test]
    fn test_second_seen_is_duplicate() {
        let mut dedup = EventDedup::new(1000, 0.01);
        assert!(!dedup.is_duplicate("evt-1"));
        assert!(dedup.is_duplicate("evt-1"));
    }

    #[test]
    fn test_different_ids_not_duplicates() {
        let mut dedup = EventDedup::new(1000, 0.01);
        assert!(!dedup.is_duplicate("evt-1"));
        assert!(!dedup.is_duplicate("evt-2"));
        assert!(!dedup.is_duplicate("evt-3"));
    }

    #[test]
    fn test_false_positive_rate_within_bounds() {
        let n = 100_000;
        let target_fpr = 0.01;
        let mut dedup = EventDedup::new(n, target_fpr);

        // Insert n items
        for i in 0..n {
            dedup.is_duplicate(&format!("insert-{i}"));
        }

        // Check items that were never inserted
        let check_n = 50_000;
        let mut false_positives = 0;
        for i in 0..check_n {
            if dedup.is_duplicate(&format!("check-{i}")) {
                false_positives += 1;
            }
        }

        let observed_fpr = false_positives as f64 / check_n as f64;
        // Allow 10x margin — this is a smoke test, not a statistical guarantee
        assert!(
            observed_fpr < target_fpr * 10.0,
            "FPR too high: {observed_fpr:.4} (target: {target_fpr})"
        );
    }

    #[test]
    fn test_rotation_moves_current_to_previous() {
        let config = DedupConfig {
            items_per_interval: 1000,
            fp_rate: 0.01,
            rotation_interval_secs: 3600,
        };
        let mut dedup = EventDedup::with_config(config, DedupMetrics::noop());

        // Insert into current
        assert!(!dedup.is_duplicate("evt-1"));
        assert!(!dedup.is_duplicate("evt-2"));
        assert_eq!(dedup.current_items(), 2);

        // Rotate
        dedup.rotate();
        assert_eq!(dedup.current_items(), 0);
        assert_eq!(dedup.previous_items(), 2);

        // Previous items are still detected as duplicates
        assert!(dedup.is_duplicate("evt-1"));
        assert!(dedup.is_duplicate("evt-2"));
    }

    #[test]
    fn test_rotation_clears_old_previous() {
        let config = DedupConfig {
            items_per_interval: 1000,
            fp_rate: 0.01,
            rotation_interval_secs: 3600,
        };
        let mut dedup = EventDedup::with_config(config, DedupMetrics::noop());

        // Insert and rotate twice — first batch should be gone
        assert!(!dedup.is_duplicate("batch1-evt"));
        dedup.rotate();
        assert!(!dedup.is_duplicate("batch2-evt"));
        dedup.rotate();

        // batch2-evt should be in previous
        assert!(dedup.is_duplicate("batch2-evt"));
        // batch1-evt: gone from both filters
        assert!(!dedup.is_duplicate("batch1-evt"));
    }

    #[test]
    fn test_new_events_after_rotation_go_to_current() {
        let config = DedupConfig {
            items_per_interval: 1000,
            fp_rate: 0.01,
            rotation_interval_secs: 3600,
        };
        let mut dedup = EventDedup::with_config(config, DedupMetrics::noop());

        dedup.rotate(); // Empty rotation

        assert!(!dedup.is_duplicate("new-evt"));
        assert_eq!(dedup.current_items(), 1);
        assert!(dedup.is_duplicate("new-evt")); // Found in current
    }

    #[test]
    fn test_duplicate_from_previous_also_inserted_into_current() {
        let config = DedupConfig {
            items_per_interval: 1000,
            fp_rate: 0.01,
            rotation_interval_secs: 3600,
        };
        let mut dedup = EventDedup::with_config(config, DedupMetrics::noop());

        // Insert into current, then rotate
        assert!(!dedup.is_duplicate("evt-1"));
        dedup.rotate();
        assert_eq!(dedup.current_items(), 0);

        // Check evt-1 — found in previous, duplicate AND inserted into current
        assert!(dedup.is_duplicate("evt-1"));
        assert_eq!(dedup.current_items(), 1);

        // After another rotation, evt-1 should still be detectable
        dedup.rotate();
        assert!(dedup.is_duplicate("evt-1"));
    }

    #[test]
    fn test_config_optimal_bits() {
        let config = DedupConfig {
            items_per_interval: 4_166_667,
            fp_rate: 0.001,
            rotation_interval_secs: 3600,
        };

        // m = -(n * ln(p)) / (ln(2)^2) ~ 60M bits ~ 7.5 MB
        let bits = config.optimal_bits();
        assert!(bits > 50_000_000, "Expected >50M bits, got {bits}");
        assert!(bits < 70_000_000, "Expected <70M bits, got {bits}");

        let size_mb = config.filter_size_bytes() as f64 / (1024.0 * 1024.0);
        assert!(size_mb > 5.0, "Expected >5 MB, got {size_mb:.2} MB");
        assert!(size_mb < 10.0, "Expected <10 MB, got {size_mb:.2} MB");

        let hashes = config.optimal_hashes();
        // k = (m/n) * ln(2) ~ 10
        assert!(
            hashes >= 8 && hashes <= 12,
            "Expected 8-12 hashes, got {hashes}"
        );
    }

    #[test]
    fn test_config_from_daily() {
        let config = DedupConfig::from_daily(100_000_000, 0.001);
        assert_eq!(config.items_per_interval, 4_166_666); // 100M / 24
        assert_eq!(config.rotation_interval_secs, 3600);
    }

    #[test]
    fn test_total_items() {
        let config = DedupConfig {
            items_per_interval: 1000,
            fp_rate: 0.01,
            rotation_interval_secs: 3600,
        };
        let mut dedup = EventDedup::with_config(config, DedupMetrics::noop());

        assert!(!dedup.is_duplicate("a"));
        assert!(!dedup.is_duplicate("b"));
        assert_eq!(dedup.total_items(), 2);

        dedup.rotate();
        assert!(!dedup.is_duplicate("c"));
        assert_eq!(dedup.total_items(), 3); // 2 in previous + 1 in current
    }

    #[test]
    fn test_prometheus_metrics_update() {
        let registry = prometheus::Registry::new();
        let metrics = DedupMetrics::new(&registry);

        let config = DedupConfig {
            items_per_interval: 1000,
            fp_rate: 0.01,
            rotation_interval_secs: 3600,
        };
        let mut dedup = EventDedup::with_config(config, metrics);

        // Insert an event
        assert!(!dedup.is_duplicate("evt-1"));

        // Gather and verify
        let metric_families = registry.gather();
        let items_current = metric_families
            .iter()
            .find(|mf| mf.get_name() == "bloom_filter_items_current")
            .unwrap();
        assert_eq!(
            items_current.get_metric()[0].get_gauge().get_value(),
            1.0
        );

        // Insert duplicate
        assert!(dedup.is_duplicate("evt-1"));
        let dupes = metric_families
            .iter()
            .find(|mf| mf.get_name() == "bloom_filter_duplicates_total");
        assert!(dupes.is_some());
    }
}
