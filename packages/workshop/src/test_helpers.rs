use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::AppState;
use crate::config::{AuthConfig, RuntimeOverrides, ServerConfig, ServerFileConfig, WorkshopConfig};
use crate::db::Database;
use crate::instance_manager::{ClaudeInstance, InstanceManager};
use crate::metrics::ServerMetrics;
use crate::persistence::PersistenceService;
use crate::process_driver::ShellDriver;
use crate::repository::ConversationRepository;

/// Build a fully-wired `AppState` backed by an in-memory SQLite database.
/// Suitable for handler tests that exercise real SQL queries without I/O.
///
/// Returns `(AppState, TempDir)` — callers **must** hold the `TempDir` for
/// the lifetime of the test so that file-backed services (e.g. `NotesStorage`)
/// continue to have a valid directory.
pub async fn test_app_state() -> (AppState, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = WorkshopConfig::new(Some(tmp.path().to_path_buf())).expect("config");

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory sqlite");

    crate::db::run_migrations(&pool).await.expect("migrations");

    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .expect("pragma");

    let db = Arc::new(Database { pool: pool.clone() });
    let repository = Arc::new(ConversationRepository::new(pool));
    let persistence_service = Arc::new(PersistenceService::new(repository.clone()));
    let notes_storage = Arc::new(crate::notes::NotesStorage::new(&config.data_dir).expect("notes"));
    let state_broadcast = crate::ws::create_state_broadcast();
    let global_state_manager = Arc::new(crate::ws::GlobalStateManager::new(state_broadcast));
    let (restart_tx, _restart_rx) = tokio::sync::watch::channel(());

    let state = AppState {
        instance_manager: Arc::new(InstanceManager::new(
            "echo".into(),
            9000,
            25 * 1024 * 1024,
            0,
            None,
        )),
        conversation_watchers: Arc::new(Mutex::new(HashMap::new())),
        config: Arc::new(config),
        server_config: Arc::new(ServerConfig::from_file(&ServerFileConfig::default())),
        auth_config: Arc::new(AuthConfig {
            enabled: false,
            session_ttl_secs: 3600,
            allow_registration: true,
            https: false,
        }),
        metrics: Arc::new(ServerMetrics::new()),
        db,
        repository,
        persistence_service,
        instance_persistors: Arc::new(Mutex::new(HashMap::new())),
        notes_storage,
        global_state_manager,
        runtime_overrides: Arc::new(tokio::sync::RwLock::new(RuntimeOverrides::default())),
        restart_tx: Arc::new(restart_tx),
    };

    (state, tmp)
}

/// Like `test_app_state`, but with auth **enabled** and a pre-created admin user.
///
/// Returns `(AppState, TempDir, AuthUser)` where `AuthUser` can be inserted into
/// request extensions to simulate an authenticated caller.
pub async fn test_app_state_with_auth() -> (AppState, tempfile::TempDir, crate::auth::AuthUser) {
    let (mut state, tmp) = test_app_state().await;

    // Enable auth
    state.auth_config = Arc::new(AuthConfig {
        enabled: true,
        session_ttl_secs: 3600,
        allow_registration: true,
        https: false,
    });

    // Create an admin user in the database
    let password_hash = crate::auth::hash_password("admin123").unwrap();
    let user = crate::models::User {
        id: "admin-user".to_string(),
        username: "admin".to_string(),
        display_name: "Admin".to_string(),
        password_hash,
        is_admin: true,
        is_disabled: false,
        created_at: chrono::Utc::now().timestamp(),
        updated_at: chrono::Utc::now().timestamp(),
    };
    state.repository.create_user(&user).await.unwrap();

    let auth_user = crate::auth::AuthUser {
        user_id: "admin-user".to_string(),
        display_name: "Admin".to_string(),
        is_admin: true,
        session_token: "test-session-token".to_string(),
        csrf_token: "test-csrf-token".to_string(),
    };

    (state, tmp, auth_user)
}

/// Create a non-admin `AuthUser` and insert the backing user into the database.
pub async fn create_test_user(
    repository: &ConversationRepository,
    user_id: &str,
    username: &str,
    display_name: &str,
) -> crate::auth::AuthUser {
    let password_hash = crate::auth::hash_password("password123").unwrap();
    let user = crate::models::User {
        id: user_id.to_string(),
        username: username.to_string(),
        display_name: display_name.to_string(),
        password_hash,
        is_admin: false,
        is_disabled: false,
        created_at: chrono::Utc::now().timestamp(),
        updated_at: chrono::Utc::now().timestamp(),
    };
    repository.create_user(&user).await.unwrap();

    crate::auth::AuthUser {
        user_id: user_id.to_string(),
        display_name: display_name.to_string(),
        is_admin: false,
        session_token: format!("session-{}", user_id),
        csrf_token: format!("csrf-{}", user_id),
    }
}

/// Create a test instance with default driver/shared-state parameters.
/// Convenience wrapper around `InstanceManager::create()` for tests.
pub async fn create_test_instance(
    manager: &InstanceManager,
    name: Option<String>,
    working_dir: Option<String>,
    command: Option<String>,
) -> anyhow::Result<ClaudeInstance> {
    manager
        .create(
            name,
            working_dir,
            command,
            Box::new(ShellDriver),
            None,
            None,
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
            None,
            None,
        )
        .await
}
