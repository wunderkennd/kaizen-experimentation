fn main() {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_language(cbindgen::Language::C)
        .with_include_guard("EXPERIMENTATION_FFI_H")
        .generate()
        .expect("Unable to generate C bindings")
        .write_to_file("target/experimentation_ffi.h");
}
