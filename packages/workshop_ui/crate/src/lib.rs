//! Embedded SvelteKit UI assets for Workshop.
//! v5 - with assets
//!
//! This crate embeds the built SvelteKit SPA at compile time.
//!
//! The folder path is set via WORKSHOP_UI_PATH env var:
//! - Cargo builds: WORKSHOP_UI_PATH=../build cargo build
//! - Bazel builds: set via rustc_env in BUILD.bazel

pub use rust_embed::Embed;

/// Embedded UI assets from the SvelteKit build.
#[derive(Embed)]
#[folder = "$WORKSHOP_UI_PATH"]
pub struct Assets;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_assets_not_empty() {
        assert!(
            Assets::iter().count() > 0,
            "rust-embed embedded zero files — the SvelteKit build output is missing"
        );
    }

    #[test]
    fn index_html_present() {
        assert!(
            Assets::get("index.html").is_some(),
            "index.html not found in embedded assets — SPA entry point is missing"
        );
    }
}
