use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{error, info};

use crate::AppState;
use crate::auth::AuthUser;

pub async fn get_database_stats(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, StatusCode> {
    match state.db.get_stats().await {
        Ok(stats) => Ok(Json(stats)),
        Err(e) => {
            error!("Failed to get database stats: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Deserialize)]
pub struct ImportRequest {
    project_path: Option<String>,
    import_all: bool,
}

#[derive(Serialize)]
pub struct ImportResponse {
    imported: usize,
    updated: usize,
    skipped: usize,
    failed: usize,
    total: usize,
}

pub async fn trigger_import(
    State(state): State<AppState>,
    Json(request): Json<ImportRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let importer =
        crate::import::ConversationImporter::new(state.repository.as_ref().clone(), None);

    let stats = if request.import_all {
        info!("API: Starting full system import...");
        match importer.import_all_projects().await {
            Ok(stats) => stats,
            Err(e) => {
                error!("Import failed: {}", e);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    } else if let Some(project_path) = request.project_path {
        info!("API: Importing from {}", project_path);
        match importer
            .import_from_project(&PathBuf::from(project_path))
            .await
        {
            Ok(stats) => stats,
            Err(e) => {
                error!("Import failed: {}", e);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    } else {
        return Err(StatusCode::BAD_REQUEST);
    };

    Ok(Json(ImportResponse {
        imported: stats.imported,
        updated: stats.updated,
        skipped: stats.skipped,
        failed: stats.failed,
        total: stats.total(),
    }))
}

// Server invite admin handlers

#[derive(Deserialize)]
pub struct CreateServerInviteRequest {
    label: Option<String>,
    max_uses: Option<i32>,
    expires_in_hours: Option<i64>,
}

pub async fn create_server_invite_handler(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(req): Json<CreateServerInviteRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    if !auth_user.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    let now = chrono::Utc::now().timestamp();
    let invite = crate::models::ServerInvite {
        token: uuid::Uuid::new_v4().to_string(),
        created_by: auth_user.user_id,
        label: req.label,
        max_uses: req.max_uses,
        use_count: 0,
        expires_at: req.expires_in_hours.map(|h| now + h * 3600),
        revoked: false,
        created_at: now,
    };

    state
        .repository
        .create_server_invite(&invite)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "token": invite.token,
        "label": invite.label,
        "max_uses": invite.max_uses,
        "expires_at": invite.expires_at,
        "created_at": invite.created_at,
    })))
}

pub async fn list_server_invites_handler(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<impl IntoResponse, StatusCode> {
    if !auth_user.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    match state.repository.list_server_invites().await {
        Ok(invites) => Ok(Json(invites)),
        Err(e) => {
            error!("Failed to list server invites: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn revoke_server_invite_handler(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(token): Path<String>,
) -> StatusCode {
    if !auth_user.is_admin {
        return StatusCode::FORBIDDEN;
    }

    match state.repository.revoke_server_invite(&token).await {
        Ok(true) => StatusCode::NO_CONTENT,
        Ok(false) => StatusCode::NOT_FOUND,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

// =============================================================================
// User management handlers
// =============================================================================

#[derive(Serialize)]
pub struct AdminUserInfo {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub is_admin: bool,
    pub is_disabled: bool,
    pub created_at: i64,
}

pub async fn list_users_handler(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<impl IntoResponse, StatusCode> {
    if !auth_user.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    match state.repository.list_users().await {
        Ok(users) => {
            let infos: Vec<AdminUserInfo> = users
                .into_iter()
                .map(|u| AdminUserInfo {
                    id: u.id,
                    username: u.username,
                    display_name: u.display_name,
                    is_admin: u.is_admin,
                    is_disabled: u.is_disabled,
                    created_at: u.created_at,
                })
                .collect();
            Ok(Json(infos))
        }
        Err(e) => {
            error!("Failed to list users: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Deserialize)]
pub struct CreateUserRequest {
    username: String,
    display_name: Option<String>,
    password: String,
    is_admin: Option<bool>,
}

pub async fn create_user_handler(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(req): Json<CreateUserRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    if !auth_user.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    let username = req.username.trim().to_string();
    if username.len() < 2 || username.len() > 64 {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Username must be 2-64 characters" })),
        )
            .into_response());
    }

    if req.password.len() < 8 {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Password must be at least 8 characters" })),
        )
            .into_response());
    }

    // Check for duplicate
    if state
        .repository
        .get_user_by_username(&username)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .is_some()
    {
        return Ok((
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "Username already taken" })),
        )
            .into_response());
    }

    let password_hash =
        crate::auth::hash_password(&req.password).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let now = chrono::Utc::now().timestamp();
    let user = crate::models::User {
        id: uuid::Uuid::new_v4().to_string(),
        username: username.clone(),
        display_name: req.display_name.unwrap_or_else(|| username.clone()),
        password_hash,
        is_admin: req.is_admin.unwrap_or(false),
        is_disabled: false,
        created_at: now,
        updated_at: now,
    };

    state
        .repository
        .create_user(&user)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(AdminUserInfo {
        id: user.id,
        username: user.username,
        display_name: user.display_name,
        is_admin: user.is_admin,
        is_disabled: user.is_disabled,
        created_at: user.created_at,
    })
    .into_response())
}

#[derive(Deserialize)]
pub struct UpdateUserRequest {
    display_name: Option<String>,
    is_admin: Option<bool>,
    is_disabled: Option<bool>,
}

pub async fn update_user_handler(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(user_id): Path<String>,
    Json(req): Json<UpdateUserRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    if !auth_user.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    // Prevent admins from demoting or disabling themselves
    if user_id == auth_user.user_id {
        if req.is_admin == Some(false) {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Cannot remove your own admin role" })),
            )
                .into_response());
        }
        if req.is_disabled == Some(true) {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Cannot disable your own account" })),
            )
                .into_response());
        }
    }

    if let Some(is_admin) = req.is_admin {
        // Prevent removing the last active admin
        if !is_admin {
            let admin_count = state
                .repository
                .count_active_admins()
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            if admin_count <= 1 {
                // Check the target is actually an active admin (not already non-admin)
                if let Ok(users) = state.repository.list_users().await
                    && let Some(target) = users.iter().find(|u| u.id == user_id)
                    && target.is_admin
                    && !target.is_disabled
                {
                    return Ok((
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({ "error": "Cannot remove the last admin" })),
                    )
                        .into_response());
                }
            }
        }
        state
            .repository
            .set_user_admin(&user_id, is_admin)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    if let Some(is_disabled) = req.is_disabled {
        state
            .repository
            .set_user_disabled(&user_id, is_disabled)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    if let Some(ref display_name) = req.display_name {
        state
            .repository
            .update_user_display_name(&user_id, display_name)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    Ok(Json(serde_json::json!({ "ok": true })).into_response())
}

pub async fn delete_user_handler(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(user_id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    if !auth_user.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    // Prevent admins from deleting themselves
    if user_id == auth_user.user_id {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Cannot delete your own account" })),
        )
            .into_response());
    }

    // Prevent deleting the last active admin
    if let Ok(users) = state.repository.list_users().await
        && let Some(target) = users.iter().find(|u| u.id == user_id)
        && target.is_admin
        && !target.is_disabled
    {
        let admin_count = state
            .repository
            .count_active_admins()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if admin_count <= 1 {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Cannot delete the last admin" })),
            )
                .into_response());
        }
    }

    match state.repository.delete_user(&user_id).await {
        Ok(true) => Ok(StatusCode::NO_CONTENT.into_response()),
        Ok(false) => Ok(StatusCode::NOT_FOUND.into_response()),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// Trigger an HTTP server restart to reload configuration.
/// Accessible from loopback without auth (the CLI calls this after writing config.toml).
pub async fn restart_handler(State(state): State<AppState>) -> impl IntoResponse {
    info!("Admin restart requested — signalling server loop to reload config");
    let _ = state.restart_tx.send(());
    Json(serde_json::json!({ "ok": true, "message": "Server restarting with new config" }))
}

// =============================================================================
// Config read/patch endpoints
// =============================================================================

/// Response shape for GET /api/admin/config
#[derive(Serialize)]
pub struct ConfigResponse {
    pub profile: Option<String>,
    pub host: String,
    pub port: u16,
    pub auth_enabled: bool,
    pub https: bool,
    pub scrollback_lines: usize,
    /// Which values are overridden at runtime (ephemeral)
    pub overrides: OverrideState,
}

#[derive(Serialize)]
pub struct OverrideState {
    pub host: bool,
    pub port: bool,
    pub auth_enabled: bool,
    pub https: bool,
    pub scrollback_lines: bool,
}

/// GET /api/admin/config — return the effective config + which fields are overridden.
pub async fn get_config_handler(State(state): State<AppState>) -> impl IntoResponse {
    let fc: crate::config::FileConfig = crate::config::load_config(&state.config.data_dir, None)
        .extract()
        .unwrap_or_default();
    let overrides = state.runtime_overrides.read().await.clone();

    let effective_host = overrides
        .host
        .as_deref()
        .or(fc.server.host.as_deref())
        .unwrap_or("127.0.0.1")
        .to_string();
    let effective_port = overrides.port.or(fc.server.port).unwrap_or(0);
    let effective_auth = overrides.auth_enabled.unwrap_or(fc.auth.enabled);
    let effective_https = overrides.https.unwrap_or(fc.auth.https);
    let effective_scrollback = overrides
        .scrollback_lines
        .unwrap_or(fc.server.scrollback_lines);

    let profile_name = fc.profile.as_ref().map(|p| match p {
        crate::config::Profile::Local => "local",
        crate::config::Profile::Tunnel => "tunnel",
        crate::config::Profile::Server => "server",
    });

    Json(ConfigResponse {
        profile: profile_name.map(|s| s.to_string()),
        host: effective_host,
        port: effective_port,
        auth_enabled: effective_auth,
        https: effective_https,
        scrollback_lines: effective_scrollback,
        overrides: OverrideState {
            host: overrides.host.is_some(),
            port: overrides.port.is_some(),
            auth_enabled: overrides.auth_enabled.is_some(),
            https: overrides.https.is_some(),
            scrollback_lines: overrides.scrollback_lines.is_some(),
        },
    })
}

/// Request body for PATCH /api/admin/config
#[derive(Deserialize)]
pub struct ConfigPatchRequest {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub auth_enabled: Option<bool>,
    pub https: Option<bool>,
    pub scrollback_lines: Option<usize>,
    /// If true, persist changes to config.toml (in addition to applying at runtime)
    #[serde(default)]
    pub save: bool,
}

/// PATCH /api/admin/config — apply runtime overrides and optionally save to config.toml.
pub async fn patch_config_handler(
    State(state): State<AppState>,
    Json(req): Json<ConfigPatchRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    // Guard: prevent enabling auth when no admin users exist
    if req.auth_enabled == Some(true) {
        let admin_count = state
            .repository
            .count_active_admins()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if admin_count == 0 {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Cannot enable auth: no admin accounts exist. Create an admin user first."
                })),
            )
                .into_response());
        }
    }

    // If save=true, persist to config.toml first
    if req.save {
        if let Err(e) = save_overrides_to_config(&state.config, &req) {
            error!("Failed to save config: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
        info!(
            "Config changes saved to {}",
            state.config.config_toml_path().display()
        );
    }

    // Apply runtime overrides
    {
        let mut overrides = state.runtime_overrides.write().await;
        if let Some(ref host) = req.host {
            overrides.host = Some(host.clone());
        }
        if let Some(port) = req.port {
            overrides.port = Some(port);
        }
        if let Some(auth) = req.auth_enabled {
            overrides.auth_enabled = Some(auth);
        }
        if let Some(https) = req.https {
            overrides.https = Some(https);
        }
        if let Some(scrollback) = req.scrollback_lines {
            overrides.scrollback_lines = Some(scrollback);
        }

        // If saving, clear the runtime overrides for saved fields (they're now in config.toml)
        if req.save {
            if req.host.is_some() {
                overrides.host = None;
            }
            if req.port.is_some() {
                overrides.port = None;
            }
            if req.auth_enabled.is_some() {
                overrides.auth_enabled = None;
            }
            if req.https.is_some() {
                overrides.https = None;
            }
            if req.scrollback_lines.is_some() {
                overrides.scrollback_lines = None;
            }
        }
    }

    // Trigger server restart
    info!("Config patch applied — triggering server restart");
    let _ = state.restart_tx.send(());

    Ok(Json(serde_json::json!({
        "ok": true,
        "saved": req.save,
    }))
    .into_response())
}

/// Read-modify-write config.toml to persist the given overrides.
fn save_overrides_to_config(
    config: &crate::config::WorkshopConfig,
    req: &ConfigPatchRequest,
) -> anyhow::Result<()> {
    use anyhow::Context;

    let path = config.config_toml_path();
    let mut doc = if path.exists() {
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        contents
            .parse::<toml::Table>()
            .with_context(|| format!("Failed to parse {}", path.display()))?
    } else {
        toml::Table::new()
    };

    if let Some(ref host) = req.host {
        let server = doc
            .entry("server")
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .context("[server] is not a table")?;
        server.insert("host".to_string(), toml::Value::String(host.clone()));
    }

    if let Some(port) = req.port {
        let server = doc
            .entry("server")
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .context("[server] is not a table")?;
        server.insert("port".to_string(), toml::Value::Integer(port as i64));
    }

    if let Some(auth_enabled) = req.auth_enabled {
        let auth = doc
            .entry("auth")
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .context("[auth] is not a table")?;
        auth.insert("enabled".to_string(), toml::Value::Boolean(auth_enabled));
    }

    if let Some(https) = req.https {
        let auth = doc
            .entry("auth")
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .context("[auth] is not a table")?;
        auth.insert("https".to_string(), toml::Value::Boolean(https));
    }

    if let Some(scrollback_lines) = req.scrollback_lines {
        let server = doc
            .entry("server")
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .context("[server] is not a table")?;
        server.insert(
            "scrollback_lines".to_string(),
            toml::Value::Integer(scrollback_lines as i64),
        );
    }

    let serialized = toml::to_string_pretty(&doc).context("Failed to serialize config.toml")?;
    std::fs::write(&path, serialized)
        .with_context(|| format!("Failed to write {}", path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WorkshopConfig;

    fn make_config(tmp: &std::path::Path) -> WorkshopConfig {
        WorkshopConfig::new(Some(tmp.to_path_buf())).unwrap()
    }

    #[test]
    fn test_save_host_to_new_config() {
        let tmp = tempfile::tempdir().unwrap();
        let config = make_config(tmp.path());

        let req = ConfigPatchRequest {
            host: Some("0.0.0.0".into()),
            port: None,
            auth_enabled: None,
            https: None,
            scrollback_lines: None,
            save: true,
        };

        save_overrides_to_config(&config, &req).unwrap();

        let contents = std::fs::read_to_string(config.config_toml_path()).unwrap();
        let doc: toml::Table = contents.parse().unwrap();
        let server = doc["server"].as_table().unwrap();
        assert_eq!(server["host"].as_str().unwrap(), "0.0.0.0");
    }

    #[test]
    fn test_save_port() {
        let tmp = tempfile::tempdir().unwrap();
        let config = make_config(tmp.path());

        let req = ConfigPatchRequest {
            host: None,
            port: Some(8080),
            auth_enabled: None,
            https: None,
            scrollback_lines: None,
            save: true,
        };

        save_overrides_to_config(&config, &req).unwrap();

        let contents = std::fs::read_to_string(config.config_toml_path()).unwrap();
        let doc: toml::Table = contents.parse().unwrap();
        assert_eq!(doc["server"]["port"].as_integer().unwrap(), 8080);
    }

    #[test]
    fn test_save_auth_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let config = make_config(tmp.path());

        let req = ConfigPatchRequest {
            host: None,
            port: None,
            auth_enabled: Some(true),
            https: Some(true),
            scrollback_lines: None,
            save: true,
        };

        save_overrides_to_config(&config, &req).unwrap();

        let contents = std::fs::read_to_string(config.config_toml_path()).unwrap();
        let doc: toml::Table = contents.parse().unwrap();
        let auth = doc["auth"].as_table().unwrap();
        assert!(auth["enabled"].as_bool().unwrap());
        assert!(auth["https"].as_bool().unwrap());
    }

    #[test]
    fn test_save_preserves_existing_config() {
        let tmp = tempfile::tempdir().unwrap();
        let config = make_config(tmp.path());

        // Write existing config
        std::fs::write(
            config.config_toml_path(),
            "[server]\nhost = \"127.0.0.1\"\n\n[auth]\nsession_ttl_secs = 3600\n",
        )
        .unwrap();

        // Patch only port
        let req = ConfigPatchRequest {
            host: None,
            port: Some(9090),
            auth_enabled: None,
            https: None,
            scrollback_lines: None,
            save: true,
        };

        save_overrides_to_config(&config, &req).unwrap();

        let contents = std::fs::read_to_string(config.config_toml_path()).unwrap();
        let doc: toml::Table = contents.parse().unwrap();
        // Existing values preserved
        assert_eq!(doc["server"]["host"].as_str().unwrap(), "127.0.0.1");
        assert_eq!(doc["auth"]["session_ttl_secs"].as_integer().unwrap(), 3600);
        // New value added
        assert_eq!(doc["server"]["port"].as_integer().unwrap(), 9090);
    }

    #[test]
    fn test_save_all_fields_at_once() {
        let tmp = tempfile::tempdir().unwrap();
        let config = make_config(tmp.path());

        let req = ConfigPatchRequest {
            host: Some("192.168.1.1".into()),
            port: Some(3000),
            auth_enabled: Some(true),
            https: Some(false),
            scrollback_lines: None,
            save: true,
        };

        save_overrides_to_config(&config, &req).unwrap();

        let contents = std::fs::read_to_string(config.config_toml_path()).unwrap();
        let doc: toml::Table = contents.parse().unwrap();
        assert_eq!(doc["server"]["host"].as_str().unwrap(), "192.168.1.1");
        assert_eq!(doc["server"]["port"].as_integer().unwrap(), 3000);
        assert!(doc["auth"]["enabled"].as_bool().unwrap());
        assert!(!doc["auth"]["https"].as_bool().unwrap());
    }

    #[test]
    fn test_save_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let config = make_config(tmp.path());

        let req = ConfigPatchRequest {
            host: None,
            port: None,
            auth_enabled: None,
            https: None,
            scrollback_lines: None,
            save: true,
        };

        save_overrides_to_config(&config, &req).unwrap();
        // Should create a valid (empty) config file
        let contents = std::fs::read_to_string(config.config_toml_path()).unwrap();
        let _doc: toml::Table = contents.parse().unwrap();
    }

    #[test]
    fn test_save_scrollback_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let config = make_config(tmp.path());

        let req = ConfigPatchRequest {
            host: None,
            port: None,
            auth_enabled: None,
            https: None,
            scrollback_lines: Some(500),
            save: true,
        };

        save_overrides_to_config(&config, &req).unwrap();

        let contents = std::fs::read_to_string(config.config_toml_path()).unwrap();
        let doc: toml::Table = contents.parse().unwrap();
        assert_eq!(doc["server"]["scrollback_lines"].as_integer().unwrap(), 500);
    }

    // =========================================================================
    // Handler tests
    // =========================================================================

    use axum::{Router, body::Body, http::Request, routing::get, routing::post};
    use tower::ServiceExt;

    /// Helper to build a router with admin routes using test app state.
    async fn test_admin_router() -> (Router, AppState, tempfile::TempDir) {
        let (state, tmp) = crate::test_helpers::test_app_state().await;
        let router = Router::new()
            .route("/admin/config", get(get_config_handler))
            .route("/admin/restart", post(restart_handler))
            .route("/admin/stats", get(get_database_stats))
            .with_state(state.clone());
        (router, state, tmp)
    }

    #[tokio::test]
    async fn test_get_config_handler_returns_defaults() {
        let (router, _state, _tmp) = test_admin_router().await;

        let req = Request::builder()
            .uri("/admin/config")
            .body(Body::empty())
            .unwrap();

        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 10_000)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // Should have the expected fields
        assert!(json.get("host").is_some());
        assert!(json.get("port").is_some());
        assert!(json.get("auth_enabled").is_some());
        assert!(json.get("https").is_some());
        assert!(json.get("scrollback_lines").is_some());
        assert!(json.get("overrides").is_some());
        // No overrides active by default
        let overrides = &json["overrides"];
        assert!(!overrides["host"].as_bool().unwrap());
        assert!(!overrides["port"].as_bool().unwrap());
        assert!(!overrides["scrollback_lines"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_restart_handler_returns_ok() {
        let (router, _state, _tmp) = test_admin_router().await;

        let req = Request::builder()
            .method("POST")
            .uri("/admin/restart")
            .body(Body::empty())
            .unwrap();

        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 10_000)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["ok"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_get_database_stats_returns_ok() {
        let (router, _state, _tmp) = test_admin_router().await;

        let req = Request::builder()
            .uri("/admin/stats")
            .body(Body::empty())
            .unwrap();

        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_patch_config_applies_runtime_overrides() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let router = Router::new()
            .route("/admin/config", axum::routing::patch(patch_config_handler))
            .with_state(state.clone());

        let req = Request::builder()
            .method("PATCH")
            .uri("/admin/config")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"host":"0.0.0.0","port":9090,"save":false}"#))
            .unwrap();

        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Check that runtime overrides were applied
        let overrides = state.runtime_overrides.read().await;
        assert_eq!(overrides.host.as_deref(), Some("0.0.0.0"));
        assert_eq!(overrides.port, Some(9090));
    }

    #[tokio::test]
    async fn test_patch_config_save_clears_overrides() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let router = Router::new()
            .route("/admin/config", axum::routing::patch(patch_config_handler))
            .with_state(state.clone());

        let req = Request::builder()
            .method("PATCH")
            .uri("/admin/config")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"port":8080,"save":true}"#))
            .unwrap();

        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 10_000)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["saved"].as_bool().unwrap());

        // Runtime override should be cleared (saved to config.toml)
        let overrides = state.runtime_overrides.read().await;
        assert!(overrides.port.is_none());
    }

    #[tokio::test]
    async fn test_patch_config_rejects_auth_enable_without_admins() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let router = Router::new()
            .route("/admin/config", axum::routing::patch(patch_config_handler))
            .with_state(state.clone());

        // No users exist at all — enabling auth should be rejected
        let req = Request::builder()
            .method("PATCH")
            .uri("/admin/config")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"auth_enabled":true,"save":false}"#))
            .unwrap();

        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let body = axum::body::to_bytes(resp.into_body(), 10_000)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(
            json["error"]
                .as_str()
                .unwrap()
                .contains("no admin accounts exist")
        );
    }

    #[tokio::test]
    async fn test_patch_config_allows_auth_enable_with_admin() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;

        // Create an active admin user
        let admin = crate::models::User {
            id: "u-admin".to_string(),
            username: "admin".to_string(),
            display_name: "Admin".to_string(),
            password_hash: "hashed".to_string(),
            is_admin: true,
            is_disabled: false,
            created_at: chrono::Utc::now().timestamp(),
            updated_at: chrono::Utc::now().timestamp(),
        };
        state.repository.create_user(&admin).await.unwrap();

        let router = Router::new()
            .route("/admin/config", axum::routing::patch(patch_config_handler))
            .with_state(state.clone());

        let req = Request::builder()
            .method("PATCH")
            .uri("/admin/config")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"auth_enabled":true,"save":false}"#))
            .unwrap();

        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // =========================================================================
    // Serialization tests for response types
    // =========================================================================

    #[test]
    fn test_config_response_serialization() {
        let resp = ConfigResponse {
            profile: Some("local".to_string()),
            host: "127.0.0.1".to_string(),
            port: 3000,
            auth_enabled: false,
            https: false,
            scrollback_lines: 10_000,
            overrides: OverrideState {
                host: false,
                port: true,
                auth_enabled: false,
                https: false,
                scrollback_lines: false,
            },
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["profile"], "local");
        assert_eq!(json["host"], "127.0.0.1");
        assert_eq!(json["port"], 3000);
        assert!(!json["auth_enabled"].as_bool().unwrap());
        assert!(json["overrides"]["port"].as_bool().unwrap());
        assert_eq!(json["scrollback_lines"], 10_000);
    }

    #[test]
    fn test_import_response_serialization() {
        let resp = ImportResponse {
            imported: 5,
            updated: 3,
            skipped: 10,
            failed: 1,
            total: 19,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["imported"], 5);
        assert_eq!(json["total"], 19);
    }

    #[test]
    fn test_import_request_deserialization() {
        let req: ImportRequest = serde_json::from_str(r#"{"import_all": true}"#).unwrap();
        assert!(req.import_all);
        assert!(req.project_path.is_none());

        let req: ImportRequest =
            serde_json::from_str(r#"{"import_all": false, "project_path": "/tmp/foo"}"#).unwrap();
        assert!(!req.import_all);
        assert_eq!(req.project_path.as_deref(), Some("/tmp/foo"));
    }

    #[test]
    fn test_create_server_invite_request_deserialization() {
        let req: CreateServerInviteRequest = serde_json::from_str(r#"{}"#).unwrap();
        assert!(req.label.is_none());
        assert!(req.max_uses.is_none());
        assert!(req.expires_in_hours.is_none());

        let req: CreateServerInviteRequest =
            serde_json::from_str(r#"{"label":"test","max_uses":5,"expires_in_hours":24}"#).unwrap();
        assert_eq!(req.label.as_deref(), Some("test"));
        assert_eq!(req.max_uses, Some(5));
        assert_eq!(req.expires_in_hours, Some(24));
    }

    #[test]
    fn test_config_patch_request_defaults() {
        let req: ConfigPatchRequest = serde_json::from_str(r#"{}"#).unwrap();
        assert!(req.host.is_none());
        assert!(req.port.is_none());
        assert!(req.auth_enabled.is_none());
        assert!(req.https.is_none());
        assert!(req.scrollback_lines.is_none());
        assert!(!req.save);
    }

    #[test]
    fn test_override_state_serialization() {
        let state = OverrideState {
            host: true,
            port: false,
            auth_enabled: true,
            https: false,
            scrollback_lines: false,
        };
        let json = serde_json::to_value(&state).unwrap();
        assert!(json["host"].as_bool().unwrap());
        assert!(!json["port"].as_bool().unwrap());
        assert!(!json["scrollback_lines"].as_bool().unwrap());
    }
}
