//! Build script: compile .proto files into Rust types via tonic-build.
//! Proto source directory: ../../proto/

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_root = "../../proto";

    // Collect all .proto files
    let protos: Vec<String> = walkdir::WalkDir::new(proto_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "proto"))
        .map(|e| e.path().display().to_string())
        .collect();

    if protos.is_empty() {
        // During initial scaffolding, proto files may not exist yet.
        // Generate an empty module so the crate compiles.
        println!("cargo:warning=No .proto files found in {proto_root}, generating empty module.");
        return Ok(());
    }

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&protos, &[proto_root])?;

    Ok(())
}
