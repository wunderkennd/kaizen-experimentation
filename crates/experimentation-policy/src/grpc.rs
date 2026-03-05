//! Stub gRPC server for the policy service.
//!
//! Full implementation pending proto code generation.
//! Currently does not expose any gRPC endpoints; logs startup and returns immediately.

use tracing::info;

/// Start the stub gRPC server (logs startup and returns immediately; no endpoints active).
pub async fn serve_grpc(addr: String) -> Result<(), String> {
    info!(%addr, "gRPC server stub started (no gRPC endpoints active; awaiting proto codegen)");
    Ok(())
}
