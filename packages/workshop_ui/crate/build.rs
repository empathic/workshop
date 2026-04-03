fn main() {
    // Provide a default WORKSHOP_UI_PATH so `cargo clippy --all` works
    // without requiring the env var. Bazel builds set this explicitly.
    if std::env::var("WORKSHOP_UI_PATH").is_err() {
        let fallback = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../build")
            .canonicalize()
            .unwrap_or_else(|_| {
                // If the build dir doesn't exist, point at the crate dir itself
                // so rust-embed compiles (with no assets) rather than erroring.
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            });
        println!("cargo:rustc-env=WORKSHOP_UI_PATH={}", fallback.display());
    }
}
