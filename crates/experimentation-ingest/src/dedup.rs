//! Bloom filter for event deduplication.
//!
//! Sized for ~100M events/day at 0.1% false positive rate.

use bloomfilter::Bloom;

pub struct EventDedup {
    filter: Bloom<str>,
}

impl EventDedup {
    /// Create a new dedup filter sized for `expected_items` with `fp_rate` false positive rate.
    pub fn new(expected_items: usize, fp_rate: f64) -> Self {
        Self {
            filter: Bloom::new_for_fp_rate(expected_items, fp_rate),
        }
    }

    /// Check if an event_id has been seen. Returns true if likely duplicate.
    pub fn is_duplicate(&mut self, event_id: &str) -> bool {
        if self.filter.check(event_id) {
            true  // Probable duplicate (may be false positive)
        } else {
            self.filter.set(event_id);
            false
        }
    }
}
