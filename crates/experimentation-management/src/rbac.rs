//! RBAC interceptor for the Management Service — ported from Go (ADR-025).
//!
//! ## Design
//!
//! The Go implementation used a ConnectRPC unary interceptor that had access to
//! `req.Spec().Procedure` — the full RPC method path — and enforced per-procedure
//! role minimums in one place. In tonic 0.12, the `service::interceptor` closure
//! receives `tonic::Request<()>` which does not expose the HTTP/2 `:path`
//! pseudo-header (that lives in the tower/HTTP layer, not the gRPC layer).
//!
//! ### Equivalent Rust pattern (two-phase RBAC)
//! 1. **Interceptor (auth)**: extracts `x-user-email` / `x-user-role` from gRPC
//!    metadata, validates them, injects an `Identity` extension into every request.
//!    If headers are missing or invalid the interceptor returns `UNAUTHENTICATED`.
//! 2. **Handler (authz)**: each RPC handler calls `require_role(extensions, minimum)`
//!    against the `Identity` injected in step 1. This produces an identical access
//!    control matrix to the Go implementation — the only difference is that the
//!    enforcement point is per-handler rather than per-interceptor.
//!
//! The `procedure_min_role` map below documents the full Go permission matrix and
//! is used in unit tests to verify correctness of per-handler role assignments.
//!
//! Headers (set by API gateway, never by the client):
//!   `x-user-email`  — authenticated user email
//!   `x-user-role`   — one of: viewer, analyst, experimenter, admin
//!
//! Role hierarchy: viewer(0) < analyst(1) < experimenter(2) < admin(3)

use std::fmt;
use std::str::FromStr;

use tonic::{Extensions, Request, Status};

// ---------------------------------------------------------------------------
// Role
// ---------------------------------------------------------------------------

/// Authorization role — 4-level hierarchy matching the Go implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Viewer = 0,
    Analyst = 1,
    Experimenter = 2,
    Admin = 3,
}

impl Role {
    /// Returns true if this role meets or exceeds `minimum`.
    pub fn has_at_least(self, minimum: Role) -> bool {
        (self as u8) >= (minimum as u8)
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Role::Viewer => "viewer",
            Role::Analyst => "analyst",
            Role::Experimenter => "experimenter",
            Role::Admin => "admin",
        })
    }
}

impl FromStr for Role {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "viewer" => Ok(Role::Viewer),
            "analyst" => Ok(Role::Analyst),
            "experimenter" => Ok(Role::Experimenter),
            "admin" => Ok(Role::Admin),
            other => Err(format!(
                "invalid role {:?}: must be one of viewer, analyst, experimenter, admin",
                other
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Identity (stored in request extensions)
// ---------------------------------------------------------------------------

/// Authenticated identity extracted from API gateway headers.
#[derive(Debug, Clone)]
pub struct Identity {
    pub email: String,
    pub role: Role,
}

// ---------------------------------------------------------------------------
// Procedure → minimum role mapping (documents the full Go permission matrix)
// ---------------------------------------------------------------------------

/// Returns the minimum role required for the given gRPC procedure path.
/// Path format: `/experimentation.management.v1.ExperimentManagementService/MethodName`
///
/// This function is the authoritative reference for the permission matrix.
/// Each RPC handler calls `require_role` with the value returned here.
/// Unknown procedures default to `admin` (fail-safe, matching Go behaviour).
pub fn procedure_min_role(path: &str) -> Role {
    let method = path.rsplit('/').next().unwrap_or(path);
    match method {
        // Read-only — viewer
        "GetExperiment"
        | "ListExperiments"
        | "GetMetricDefinition"
        | "ListMetricDefinitions"
        | "GetLayer"
        | "GetLayerAllocations"
        | "ListSurrogateModels"
        | "GetSurrogateCalibration" => Role::Viewer,

        // Analyst operations
        "CreateMetricDefinition"
        | "CreateTargetingRule"
        | "CreateSurrogateModel"
        | "TriggerSurrogateRecalibration" => Role::Analyst,

        // Experimenter operations
        "CreateExperiment"
        | "UpdateExperiment"
        | "StartExperiment"
        | "PauseExperiment"
        | "ResumeExperiment"
        | "ConcludeExperiment" => Role::Experimenter,

        // Admin operations
        "ArchiveExperiment" | "CreateLayer" => Role::Admin,

        // Unknown — fail-safe: require admin.
        unknown => {
            tracing::warn!(
                procedure = %unknown,
                "unknown procedure in RBAC check — defaulting to admin"
            );
            Role::Admin
        }
    }
}

// ---------------------------------------------------------------------------
// tonic interceptor (phase 1: authentication only)
// ---------------------------------------------------------------------------

/// tonic interceptor that authenticates every gRPC request.
///
/// Extracts `x-user-email` and `x-user-role` from gRPC metadata, validates the
/// role string, and injects an `Identity` extension into the request for use by
/// individual RPC handlers.
///
/// Authorization (role vs. procedure minimum) is enforced per-handler via
/// `require_role`. See module-level doc for rationale.
pub fn rbac_interceptor(mut req: Request<()>) -> Result<Request<()>, Status> {
    let email = req
        .metadata()
        .get("x-user-email")
        .ok_or_else(|| Status::unauthenticated("missing x-user-email metadata"))?
        .to_str()
        .map_err(|_| Status::unauthenticated("x-user-email contains non-ASCII characters"))?
        .to_string();

    if email.is_empty() {
        return Err(Status::unauthenticated("x-user-email must not be empty"));
    }

    let role_str = req
        .metadata()
        .get("x-user-role")
        .ok_or_else(|| Status::unauthenticated("missing x-user-role metadata"))?
        .to_str()
        .map_err(|_| Status::unauthenticated("x-user-role contains non-ASCII characters"))?;

    let role: Role = role_str
        .parse()
        .map_err(|e: String| Status::unauthenticated(e))?;

    req.extensions_mut().insert(Identity { email, role });
    Ok(req)
}

// ---------------------------------------------------------------------------
// Per-handler authorization helpers
// ---------------------------------------------------------------------------

/// Extract the injected `Identity` from request extensions.
///
/// Returns `UNAUTHENTICATED` if the interceptor did not inject an identity
/// (should not happen in production; indicates interceptor bypass in tests).
pub fn extract_identity(extensions: &Extensions) -> Result<&Identity, Status> {
    extensions
        .get::<Identity>()
        .ok_or_else(|| Status::unauthenticated("no identity in request context"))
}

/// Verify that the injected `Identity` has at least `minimum` role.
///
/// Returns `PERMISSION_DENIED` if the role is insufficient.
pub fn require_role(extensions: &Extensions, minimum: Role) -> Result<&Identity, Status> {
    let identity = extract_identity(extensions)?;
    if !identity.role.has_at_least(minimum) {
        return Err(Status::permission_denied(format!(
            "role {:?} is insufficient (requires {:?})",
            identity.role, minimum
        )));
    }
    Ok(identity)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_ordering() {
        assert!(Role::Admin.has_at_least(Role::Viewer));
        assert!(Role::Admin.has_at_least(Role::Analyst));
        assert!(Role::Admin.has_at_least(Role::Experimenter));
        assert!(Role::Admin.has_at_least(Role::Admin));

        assert!(Role::Experimenter.has_at_least(Role::Viewer));
        assert!(Role::Experimenter.has_at_least(Role::Analyst));
        assert!(Role::Experimenter.has_at_least(Role::Experimenter));
        assert!(!Role::Experimenter.has_at_least(Role::Admin));

        assert!(Role::Analyst.has_at_least(Role::Viewer));
        assert!(Role::Analyst.has_at_least(Role::Analyst));
        assert!(!Role::Analyst.has_at_least(Role::Experimenter));

        assert!(Role::Viewer.has_at_least(Role::Viewer));
        assert!(!Role::Viewer.has_at_least(Role::Analyst));
    }

    #[test]
    fn role_parse() {
        assert_eq!("viewer".parse::<Role>().unwrap(), Role::Viewer);
        assert_eq!("analyst".parse::<Role>().unwrap(), Role::Analyst);
        assert_eq!("experimenter".parse::<Role>().unwrap(), Role::Experimenter);
        assert_eq!("admin".parse::<Role>().unwrap(), Role::Admin);
        assert!("superuser".parse::<Role>().is_err());
        assert!("ADMIN".parse::<Role>().is_err()); // case-sensitive
    }

    #[test]
    fn procedure_min_roles() {
        assert_eq!(
            procedure_min_role("/experimentation.management.v1.ExperimentManagementService/GetExperiment"),
            Role::Viewer
        );
        assert_eq!(
            procedure_min_role("/experimentation.management.v1.ExperimentManagementService/CreateExperiment"),
            Role::Experimenter
        );
        assert_eq!(
            procedure_min_role("/experimentation.management.v1.ExperimentManagementService/ArchiveExperiment"),
            Role::Admin
        );
        assert_eq!(
            procedure_min_role("/experimentation.management.v1.ExperimentManagementService/CreateMetricDefinition"),
            Role::Analyst
        );
        // Unknown procedure defaults to admin.
        assert_eq!(
            procedure_min_role("/experimentation.management.v1.ExperimentManagementService/UndocumentedRpc"),
            Role::Admin
        );
    }

    #[test]
    fn role_display() {
        assert_eq!(Role::Viewer.to_string(), "viewer");
        assert_eq!(Role::Admin.to_string(), "admin");
    }

    #[test]
    fn require_role_no_identity_returns_unauthenticated() {
        let ext = Extensions::default();
        let err = require_role(&ext, Role::Viewer).unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
    }

    #[test]
    fn require_role_insufficient_returns_permission_denied() {
        let mut ext = Extensions::default();
        ext.insert(Identity {
            email: "user@example.com".to_string(),
            role: Role::Viewer,
        });
        let err = require_role(&ext, Role::Experimenter).unwrap_err();
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }

    #[test]
    fn require_role_sufficient_returns_identity() {
        let mut ext = Extensions::default();
        ext.insert(Identity {
            email: "admin@example.com".to_string(),
            role: Role::Admin,
        });
        let identity = require_role(&ext, Role::Experimenter).unwrap();
        assert_eq!(identity.email, "admin@example.com");
    }
}
