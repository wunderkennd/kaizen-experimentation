//! Lifecycle state machine for experiments (ADR-025, ADR-005).
//!
//! ## States
//!
//! ```text
//! DRAFT ──► STARTING ──► RUNNING ◄──► PAUSED
//!                           │
//!                           ▼
//!                       CONCLUDING ──► CONCLUDED ──► ARCHIVED
//! ```
//!
//! STARTING is a transitional state during which M5 validates config, allocates
//! buckets, and ensures metric availability. On validation failure the experiment
//! reverts to DRAFT.
//!
//! ## TOCTOU safety
//!
//! Every transition executes:
//! ```sql
//! UPDATE experiments
//!    SET state = $new_state, updated_at = NOW()
//!  WHERE experiment_id = $id
//!    AND state = $expected_state
//! ```
//! If `rows_affected() != 1` another concurrent writer won the race and this
//! transition returns `TransitionError::Conflict`.

use sqlx::postgres::PgPool;
use uuid::Uuid;

use crate::store::{experiment_exists, StoreError};

// ---------------------------------------------------------------------------
// State enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExperimentState {
    Draft,
    Starting,
    Running,
    Paused,
    Concluding,
    Concluded,
    Archived,
}

impl ExperimentState {
    /// Parse from the string stored in PostgreSQL.
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "DRAFT" => Some(Self::Draft),
            "STARTING" => Some(Self::Starting),
            "RUNNING" => Some(Self::Running),
            "PAUSED" => Some(Self::Paused),
            "CONCLUDING" => Some(Self::Concluding),
            "CONCLUDED" => Some(Self::Concluded),
            "ARCHIVED" => Some(Self::Archived),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "DRAFT",
            Self::Starting => "STARTING",
            Self::Running => "RUNNING",
            Self::Paused => "PAUSED",
            Self::Concluding => "CONCLUDING",
            Self::Concluded => "CONCLUDED",
            Self::Archived => "ARCHIVED",
        }
    }

    /// Valid successor states from this state.
    pub fn valid_successors(self) -> &'static [ExperimentState] {
        match self {
            Self::Draft => &[Self::Starting],
            // STARTING → RUNNING (success) or STARTING → DRAFT (validation failure rollback)
            Self::Starting => &[Self::Running, Self::Draft],
            // RUNNING can be paused or concluded; cumulative holdouts can only be paused.
            Self::Running => &[Self::Paused, Self::Concluding],
            // PAUSED can resume or be concluded directly.
            Self::Paused => &[Self::Running, Self::Concluding],
            Self::Concluding => &[Self::Concluded],
            Self::Concluded => &[Self::Archived],
            Self::Archived => &[],
        }
    }

    pub fn can_transition_to(self, target: ExperimentState) -> bool {
        self.valid_successors().contains(&target)
    }
}

// ---------------------------------------------------------------------------
// Transition errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum TransitionError {
    /// Another concurrent writer changed the state before us (TOCTOU race).
    #[error("concurrent state transition conflict for experiment {experiment_id}")]
    Conflict { experiment_id: Uuid },
    /// The requested transition is not valid from the current state.
    #[error("invalid transition from {from:?} to {to:?}")]
    Invalid {
        from: ExperimentState,
        to: ExperimentState,
    },
    /// The experiment was not found.
    #[error("experiment {0} not found")]
    NotFound(Uuid),
    /// Underlying store error.
    #[error("store error: {0}")]
    Store(#[from] StoreError),
}

// ---------------------------------------------------------------------------
// State machine executor
// ---------------------------------------------------------------------------

/// Execute a TOCTOU-safe state transition.
///
/// Checks that `from → to` is a valid edge, then issues:
/// ```sql
/// UPDATE experiments SET state=$to WHERE experiment_id=$id AND state=$from
/// ```
/// Returns `TransitionError::Conflict` if no row was updated (concurrent writer won).
pub async fn transition(
    pool: &PgPool,
    experiment_id: Uuid,
    from: ExperimentState,
    to: ExperimentState,
) -> Result<(), TransitionError> {
    if !from.can_transition_to(to) {
        return Err(TransitionError::Invalid { from, to });
    }

    let result = sqlx::query(
        "UPDATE experiments SET state=$2, updated_at=NOW() WHERE experiment_id=$1 AND state=$3",
    )
    .bind(experiment_id)
    .bind(to.as_str())
    .bind(from.as_str())
    .execute(pool)
    .await
    .map_err(StoreError::Db)?;

    if result.rows_affected() == 0 {
        // Could be either NotFound or concurrent-writer conflict.
        // Distinguish by checking existence.
        if !experiment_exists(pool, experiment_id).await? {
            return Err(TransitionError::NotFound(experiment_id));
        }
        return Err(TransitionError::Conflict { experiment_id });
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_to_starting_is_valid() {
        assert!(ExperimentState::Draft.can_transition_to(ExperimentState::Starting));
    }

    #[test]
    fn draft_to_running_is_invalid() {
        assert!(!ExperimentState::Draft.can_transition_to(ExperimentState::Running));
    }

    #[test]
    fn running_to_paused_and_back() {
        assert!(ExperimentState::Running.can_transition_to(ExperimentState::Paused));
        assert!(ExperimentState::Paused.can_transition_to(ExperimentState::Running));
    }

    #[test]
    fn archived_has_no_successors() {
        assert!(ExperimentState::Archived.valid_successors().is_empty());
    }

    #[test]
    fn round_trip_str_parse() {
        for s in &["DRAFT", "STARTING", "RUNNING", "PAUSED", "CONCLUDING", "CONCLUDED", "ARCHIVED"] {
            let state = ExperimentState::from_db_str(s).unwrap();
            assert_eq!(state.as_str(), *s);
        }
    }

    #[test]
    fn concluded_to_archived() {
        assert!(ExperimentState::Concluded.can_transition_to(ExperimentState::Archived));
        assert!(!ExperimentState::Concluded.can_transition_to(ExperimentState::Running));
    }
}
