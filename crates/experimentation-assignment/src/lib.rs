pub mod bandit_client;
pub mod config;
pub mod config_cache;
#[cfg(feature = "connectrpc")]
pub mod connect_server;
// ADR-031 #644 — the hand-rolled JSON shim is retired on the connectrpc
// feature path. When Connect is on, its `application/connect+json` handler
// serves the same 3 unary routes (GetAssignment, GetAssignments,
// GetSlateAssignment) plus GetInterleavedList (new) and StreamConfigUpdates,
// so http_json is dead weight in that build. Default (non-connectrpc)
// builds still expose the shim until the pilot flips default.
#[cfg(not(feature = "connectrpc"))]
pub mod http_json;
pub mod service;
pub mod stream_client;
pub mod switchback;
pub mod targeting;
