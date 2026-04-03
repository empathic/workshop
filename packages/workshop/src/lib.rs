pub mod auth;
pub mod claude_driver;
pub mod config;
pub mod db;
#[cfg(feature = "embedded-ui")]
pub mod embedded_ui;
pub mod files;
pub mod git;
pub mod handlers;
pub mod import;
pub mod inference;
pub mod instance_actor;
pub mod instance_manager;
pub mod metrics;
pub mod models;
pub mod notes;
pub mod onboarding;
pub mod persistence;
pub mod process_driver;
pub mod repository;
pub mod server;
pub mod virtual_terminal;
pub mod ws;

#[cfg(test)]
pub(crate) mod test_helpers;

// Re-export AppState at crate root so `use crate::AppState` works in handler modules
pub use server::AppState;
