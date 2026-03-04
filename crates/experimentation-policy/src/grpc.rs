//! Stub gRPC server for the policy service.
//!
//! Full implementation pending proto code generation.
//! Currently provides a health check endpoint only.

use tracing::info;

/// Start the gRPC server (stub — health check only).
pub async fn serve_grpc(addr: String) -> Result<(), String> {
    info!(%addr, "gRPC server stub started (health check only, awaiting proto codegen)");
    Ok(())
}
