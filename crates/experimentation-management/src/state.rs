//! Experiment lifecycle state machine — ported from Go (ADR-025, ADR-005).
//!
//! States and valid transitions:
//!
//!   DRAFT → STARTING        (StartExperiment — begins validation)
//!   STARTING → RUNNING      (Validation succeeded; M1 begins serving assignments)
//!   STARTING → DRAFT        (Validation failed; experiment rolled back)
//!   RUNNING → CONCLUDING    (ConcludeExperiment — triggers final M4a analysis)
//!   CONCLUDING → CONCLUDED  (Analysis complete)
//!   CONCLUDED → ARCHIVED    (ArchiveExperiment — terminal state)
//!
//! Pause/Resume do NOT change ExperimentState. They are modelled as a traffic
//! fraction change within RUNNING (traffic → 0% for pause, restored for resume),
//! consistent with the Go implementation and the proto's ExperimentState enum.
//!
//! TOCTOU Safety: all transitions are enforced in the database via:
//!   `UPDATE experiments SET state = $new WHERE experiment_id = $id AND state = $expected`
//! If rows_affected() != 1, a concurrent transition happened and the caller gets
//! `TransitionError::ConcurrentModification`.

use std::fmt;

// ---------------------------------------------------------------------------
// ExperimentState
// ---------------------------------------------------------------------------

/// All stable and transitional lifecycle states, matching proto ExperimentState enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExperimentState {
    Draft,
    Starting,
    Running,
    Concluding,
    Concluded,
    Archived,
}

impl ExperimentState {
    /// Canonical database string for this state.
    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::Draft => "DRAFT",
            Self::Starting => "STARTING",
            Self::Running => "RUNNING",
            Self::Concluding => "CONCLUDING",
            Self::Concluded => "CONCLUDED",
            Self::Archived => "ARCHIVED",
        }
    }

    /// Parse from database string representation.
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "DRAFT" => Some(Self::Draft),
            "STARTING" => Some(Self::Starting),
            "RUNNING" => Some(Self::Running),
            "CONCLUDING" => Some(Self::Concluding),
            "CONCLUDED" => Some(Self::Concluded),
            "ARCHIVED" => Some(Self::Archived),
            _ => None,
        }
    }

    /// Returns true if transitioning from `self` to `to` is a valid lifecycle step.
    pub fn can_transition_to(self, to: ExperimentState) -> bool {
        matches!(
            (self, to),
            (Self::Draft, Self::Starting)
                | (Self::Starting, Self::Running)
                | (Self::Starting, Self::Draft) // Rollback on validation failure
                | (Self::Running, Self::Concluding)
                | (Self::Concluding, Self::Concluded)
                | (Self::Concluded, Self::Archived)
        )
    }

    /// Returns true if this is a terminal state (no further transitions possible).
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Archived)
    }

    /// Returns true if this is a transitional state (STARTING or CONCLUDING).
    pub fn is_transitional(self) -> bool {
        matches!(self, Self::Starting | Self::Concluding)
    }
}

impl fmt::Display for ExperimentState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_db_str())
    }
}

// ---------------------------------------------------------------------------
// TransitionError
// ---------------------------------------------------------------------------

/// Errors that can occur during a state machine transition.
#[derive(Debug, thiserror::Error)]
pub enum TransitionError {
    /// The requested transition is not valid per the lifecycle graph.
    #[error("invalid state transition: {from} → {to}")]
    InvalidTransition {
        from: ExperimentState,
        to: ExperimentState,
    },

    /// The database row was modified by a concurrent operation before our UPDATE committed.
    /// The experiment is no longer in the expected `from` state.
    #[error(
        "concurrent modification: experiment {experiment_id} is no longer in state {expected}"
    )]
    ConcurrentModification {
        experiment_id: uuid::Uuid,
        expected: ExperimentState,
    },

    /// The experiment was not found.
    #[error("experiment not found: {0}")]
    NotFound(uuid::Uuid),

    /// Underlying database error.
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

// ---------------------------------------------------------------------------
// Transition validation (pure logic, no DB)
// ---------------------------------------------------------------------------

/// Validates that the transition `from → to` is allowed by the lifecycle graph.
/// Returns `Err(TransitionError::InvalidTransition)` if the transition is illegal.
pub fn validate_transition(
    from: ExperimentState,
    to: ExperimentState,
) -> Result<(), TransitionError> {
    if from.can_transition_to(to) {
        Ok(())
    } else {
        Err(TransitionError::InvalidTransition { from, to })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_transitions() {
        assert!(ExperimentState::Draft.can_transition_to(ExperimentState::Starting));
        assert!(ExperimentState::Starting.can_transition_to(ExperimentState::Running));
        assert!(ExperimentState::Starting.can_transition_to(ExperimentState::Draft));
        assert!(ExperimentState::Running.can_transition_to(ExperimentState::Concluding));
        assert!(ExperimentState::Concluding.can_transition_to(ExperimentState::Concluded));
        assert!(ExperimentState::Concluded.can_transition_to(ExperimentState::Archived));
    }

    #[test]
    fn invalid_transitions() {
        // Cannot skip states.
        assert!(!ExperimentState::Draft.can_transition_to(ExperimentState::Running));
        assert!(!ExperimentState::Draft.can_transition_to(ExperimentState::Concluded));
        assert!(!ExperimentState::Running.can_transition_to(ExperimentState::Concluded));
        assert!(!ExperimentState::Running.can_transition_to(ExperimentState::Draft));
        // Cannot go backwards (except Starting → Draft).
        assert!(!ExperimentState::Concluded.can_transition_to(ExperimentState::Running));
        assert!(!ExperimentState::Archived.can_transition_to(ExperimentState::Concluded));
        // Terminal state.
        assert!(!ExperimentState::Archived.can_transition_to(ExperimentState::Archived));
    }

    #[test]
    fn roundtrip_db_str() {
        let states = [
            ExperimentState::Draft,
            ExperimentState::Starting,
            ExperimentState::Running,
            ExperimentState::Concluding,
            ExperimentState::Concluded,
            ExperimentState::Archived,
        ];
        for s in &states {
            assert_eq!(ExperimentState::from_db_str(s.as_db_str()), Some(*s));
        }
    }

    #[test]
    fn unknown_db_str_returns_none() {
        assert_eq!(ExperimentState::from_db_str("PAUSED"), None);
        assert_eq!(ExperimentState::from_db_str(""), None);
        assert_eq!(ExperimentState::from_db_str("draft"), None); // case-sensitive
    }

    #[test]
    fn validate_transition_ok() {
        assert!(
            validate_transition(ExperimentState::Draft, ExperimentState::Starting).is_ok()
        );
    }

    #[test]
    fn validate_transition_err() {
        let err =
            validate_transition(ExperimentState::Draft, ExperimentState::Running).unwrap_err();
        assert!(matches!(err, TransitionError::InvalidTransition { .. }));
    }

    #[test]
    fn terminal_and_transitional() {
        assert!(ExperimentState::Archived.is_terminal());
        assert!(!ExperimentState::Concluded.is_terminal());
        assert!(ExperimentState::Starting.is_transitional());
        assert!(ExperimentState::Concluding.is_transitional());
        assert!(!ExperimentState::Running.is_transitional());
    }
}
