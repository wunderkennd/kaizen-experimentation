//! ADR-031 pilot: buffa + ConnectRPC bindings for `experimentation.assignment.v1`.
//!
//! Kept deliberately separate from the prost/tonic [`experimentation_proto`] crate
//! so the pilot can introduce buffa to the workspace without rippling through the
//! existing 56-file / 225-site prost usage.

connectrpc::include_generated!();
