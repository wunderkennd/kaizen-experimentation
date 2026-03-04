//! Fail-fast numerical validation. Used by every statistical method.

/// Check that a floating-point value is finite (not NaN, not Infinity).
/// Panics with context if the value is invalid.
#[inline]
pub fn assert_finite(value: f64, context: &str) {
    assert!(
        value.is_finite(),
        "numerical error: {context} produced non-finite value: {value}"
    );
}

/// Check that a value is in the range [0, 1].
#[inline]
pub fn assert_probability(value: f64, context: &str) {
    assert_finite(value, context);
    assert!(
        (0.0..=1.0).contains(&value),
        "probability error: {context} = {value}, expected [0, 1]"
    );
}

/// Check that a slice is non-empty.
#[inline]
pub fn assert_non_empty(data: &[f64], context: &str) {
    assert!(
        !data.is_empty(),
        "empty data: {context} received empty slice"
    );
}
