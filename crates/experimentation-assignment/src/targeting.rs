//! Targeting rule evaluation.
//!
//! Evaluates predicate trees against user attributes from the request.
//! Logic: AND across groups, OR within each group.
//! Missing attribute key → predicate does not match (fail-closed).

use std::collections::HashMap;

use crate::config::{TargetingGroup, TargetingPredicate, TargetingRule};

/// Evaluate a targeting rule against user attributes.
///
/// Returns `true` if the user matches (eligible for the experiment).
/// An empty rule (no groups) matches all users.
pub fn evaluate(rule: &TargetingRule, attributes: &HashMap<String, String>) -> bool {
    if rule.groups.is_empty() {
        return true;
    }
    // AND across groups: all must match.
    rule.groups.iter().all(|g| evaluate_group(g, attributes))
}

/// OR within group: at least one predicate must match.
fn evaluate_group(group: &TargetingGroup, attributes: &HashMap<String, String>) -> bool {
    if group.predicates.is_empty() {
        return true;
    }
    group
        .predicates
        .iter()
        .any(|p| evaluate_predicate(p, attributes))
}

/// Evaluate a single predicate against the attribute map.
/// Missing attribute → false (safe default, fail-closed).
fn evaluate_predicate(
    pred: &TargetingPredicate,
    attributes: &HashMap<String, String>,
) -> bool {
    let attr_value = match attributes.get(&pred.attribute_key) {
        Some(v) => v,
        None => return false, // Missing attribute → no match.
    };

    match pred.operator.as_str() {
        "EQUALS" => pred.values.first().is_some_and(|v| v == attr_value),
        "NOT_EQUALS" => pred.values.first().is_some_and(|v| v != attr_value),
        "IN" => pred.values.iter().any(|v| v == attr_value),
        "NOT_IN" => !pred.values.iter().any(|v| v == attr_value),
        "CONTAINS" => pred.values.first().is_some_and(|v| attr_value.contains(v.as_str())),
        "GT" | "GREATER_THAN" => compare_numeric(attr_value, &pred.values, |a, b| a > b),
        "LT" | "LESS_THAN" => compare_numeric(attr_value, &pred.values, |a, b| a < b),
        "GTE" => compare_numeric(attr_value, &pred.values, |a, b| a >= b),
        "LTE" => compare_numeric(attr_value, &pred.values, |a, b| a <= b),
        _ => false, // Unknown operator → no match.
    }
}

/// Parse both sides as f64 and apply the comparator.
/// Returns false if either side is not a valid number.
fn compare_numeric(attr_value: &str, values: &[String], cmp: fn(f64, f64) -> bool) -> bool {
    let a = match attr_value.parse::<f64>() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = match values.first().and_then(|v| v.parse::<f64>().ok()) {
        Some(v) => v,
        None => return false,
    };
    cmp(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{TargetingGroup, TargetingPredicate, TargetingRule};

    fn attrs(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn pred(key: &str, op: &str, values: &[&str]) -> TargetingPredicate {
        TargetingPredicate {
            attribute_key: key.to_string(),
            operator: op.to_string(),
            values: values.iter().map(|v| v.to_string()).collect(),
        }
    }

    #[test]
    fn empty_rule_matches_all() {
        let rule = TargetingRule { groups: vec![] };
        assert!(evaluate(&rule, &attrs(&[("country", "US")])));
        assert!(evaluate(&rule, &HashMap::new()));
    }

    #[test]
    fn equals_match() {
        let rule = TargetingRule {
            groups: vec![TargetingGroup {
                predicates: vec![pred("country", "EQUALS", &["US"])],
            }],
        };
        assert!(evaluate(&rule, &attrs(&[("country", "US")])));
        assert!(!evaluate(&rule, &attrs(&[("country", "FR")])));
    }

    #[test]
    fn not_equals() {
        let rule = TargetingRule {
            groups: vec![TargetingGroup {
                predicates: vec![pred("country", "NOT_EQUALS", &["CN"])],
            }],
        };
        assert!(evaluate(&rule, &attrs(&[("country", "US")])));
        assert!(!evaluate(&rule, &attrs(&[("country", "CN")])));
    }

    #[test]
    fn in_operator() {
        let rule = TargetingRule {
            groups: vec![TargetingGroup {
                predicates: vec![pred("country", "IN", &["US", "UK", "CA"])],
            }],
        };
        assert!(evaluate(&rule, &attrs(&[("country", "US")])));
        assert!(evaluate(&rule, &attrs(&[("country", "UK")])));
        assert!(!evaluate(&rule, &attrs(&[("country", "FR")])));
    }

    #[test]
    fn not_in_operator() {
        let rule = TargetingRule {
            groups: vec![TargetingGroup {
                predicates: vec![pred("platform", "NOT_IN", &["web", "legacy"])],
            }],
        };
        assert!(evaluate(&rule, &attrs(&[("platform", "ios")])));
        assert!(!evaluate(&rule, &attrs(&[("platform", "web")])));
    }

    #[test]
    fn contains_operator() {
        let rule = TargetingRule {
            groups: vec![TargetingGroup {
                predicates: vec![pred("email", "CONTAINS", &["@company.com"])],
            }],
        };
        assert!(evaluate(&rule, &attrs(&[("email", "alice@company.com")])));
        assert!(!evaluate(&rule, &attrs(&[("email", "bob@other.com")])));
    }

    #[test]
    fn greater_than_numeric() {
        let rule = TargetingRule {
            groups: vec![TargetingGroup {
                predicates: vec![pred("age", "GT", &["18"])],
            }],
        };
        assert!(evaluate(&rule, &attrs(&[("age", "25")])));
        assert!(!evaluate(&rule, &attrs(&[("age", "18")])));
        assert!(!evaluate(&rule, &attrs(&[("age", "15")])));
    }

    #[test]
    fn less_than_numeric() {
        let rule = TargetingRule {
            groups: vec![TargetingGroup {
                predicates: vec![pred("score", "LT", &["100"])],
            }],
        };
        assert!(evaluate(&rule, &attrs(&[("score", "50")])));
        assert!(!evaluate(&rule, &attrs(&[("score", "100")])));
    }

    #[test]
    fn gte_lte() {
        let gte_rule = TargetingRule {
            groups: vec![TargetingGroup {
                predicates: vec![pred("version", "GTE", &["3.0"])],
            }],
        };
        assert!(evaluate(&gte_rule, &attrs(&[("version", "3.0")])));
        assert!(evaluate(&gte_rule, &attrs(&[("version", "4.5")])));
        assert!(!evaluate(&gte_rule, &attrs(&[("version", "2.9")])));

        let lte_rule = TargetingRule {
            groups: vec![TargetingGroup {
                predicates: vec![pred("version", "LTE", &["5.0"])],
            }],
        };
        assert!(evaluate(&lte_rule, &attrs(&[("version", "5.0")])));
        assert!(!evaluate(&lte_rule, &attrs(&[("version", "5.1")])));
    }

    #[test]
    fn missing_attribute_no_match() {
        let rule = TargetingRule {
            groups: vec![TargetingGroup {
                predicates: vec![pred("country", "EQUALS", &["US"])],
            }],
        };
        // No "country" in attributes → no match.
        assert!(!evaluate(&rule, &attrs(&[("platform", "ios")])));
        assert!(!evaluate(&rule, &HashMap::new()));
    }

    #[test]
    fn non_numeric_gt_no_match() {
        let rule = TargetingRule {
            groups: vec![TargetingGroup {
                predicates: vec![pred("age", "GT", &["18"])],
            }],
        };
        assert!(!evaluate(&rule, &attrs(&[("age", "not_a_number")])));
    }

    #[test]
    fn or_within_group() {
        // country = US OR country = UK
        let rule = TargetingRule {
            groups: vec![TargetingGroup {
                predicates: vec![
                    pred("country", "EQUALS", &["US"]),
                    pred("country", "EQUALS", &["UK"]),
                ],
            }],
        };
        assert!(evaluate(&rule, &attrs(&[("country", "US")])));
        assert!(evaluate(&rule, &attrs(&[("country", "UK")])));
        assert!(!evaluate(&rule, &attrs(&[("country", "FR")])));
    }

    #[test]
    fn and_across_groups() {
        // (country IN [US, UK]) AND (platform = ios)
        let rule = TargetingRule {
            groups: vec![
                TargetingGroup {
                    predicates: vec![pred("country", "IN", &["US", "UK"])],
                },
                TargetingGroup {
                    predicates: vec![pred("platform", "EQUALS", &["ios"])],
                },
            ],
        };
        assert!(evaluate(
            &rule,
            &attrs(&[("country", "US"), ("platform", "ios")])
        ));
        assert!(!evaluate(
            &rule,
            &attrs(&[("country", "US"), ("platform", "android")])
        ));
        assert!(!evaluate(
            &rule,
            &attrs(&[("country", "FR"), ("platform", "ios")])
        ));
    }

    #[test]
    fn compound_or_and() {
        // (country=US OR country=UK) AND (tier=premium OR tier=platinum)
        let rule = TargetingRule {
            groups: vec![
                TargetingGroup {
                    predicates: vec![
                        pred("country", "EQUALS", &["US"]),
                        pred("country", "EQUALS", &["UK"]),
                    ],
                },
                TargetingGroup {
                    predicates: vec![pred("tier", "IN", &["premium", "platinum"])],
                },
            ],
        };
        assert!(evaluate(
            &rule,
            &attrs(&[("country", "US"), ("tier", "premium")])
        ));
        assert!(evaluate(
            &rule,
            &attrs(&[("country", "UK"), ("tier", "platinum")])
        ));
        assert!(!evaluate(
            &rule,
            &attrs(&[("country", "US"), ("tier", "free")])
        ));
        assert!(!evaluate(
            &rule,
            &attrs(&[("country", "FR"), ("tier", "premium")])
        ));
    }
}
