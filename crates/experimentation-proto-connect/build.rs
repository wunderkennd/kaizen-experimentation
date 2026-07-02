//! ADR-031 pilot codegen: buffa types + ConnectRPC service trait for the
//! `assignment/v1` package only. The prost/tonic crate at
//! `crates/experimentation-proto` is unaffected.
//!
//! Scope: assignment/v1 plus all of common/v1 (assignment's message types reach
//! transitively through `common` for experiments, interleaving, bandits,
//! lifecycle, etc.). Other proto packages — analysis, metrics, pipeline,
//! flags, management — stay out of this crate during the pilot.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_root = "../../proto";

    let mut protos: Vec<String> = walkdir::WalkDir::new(format!("{proto_root}/experimentation/common/v1"))
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "proto"))
        .map(|e| e.path().display().to_string())
        .collect();

    protos.push(format!(
        "{proto_root}/experimentation/assignment/v1/assignment_service.proto"
    ));

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={proto_root}/experimentation/assignment/v1");
    println!("cargo:rerun-if-changed={proto_root}/experimentation/common/v1");

    connectrpc_build::Config::new()
        .files(&protos)
        .includes(&[proto_root])
        .include_file("_connectrpc.rs")
        .compile()?;

    Ok(())
}
