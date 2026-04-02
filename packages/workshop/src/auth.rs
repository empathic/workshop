//! Authentication module: password ops, session management, extractors, middleware, handlers.

use anyhow::Result;
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use axum::{
    Json, Router,
    body::Body,
    extract::{Query, Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn};

use crate::config::AuthConfig;
use crate::models::{Session, User, UserInfo};
use crate::repository::ConversationRepository;

// =============================================================================
// Password Operations
// =============================================================================

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("Failed to hash password: {}", e))?;
    Ok(hash.to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

// =============================================================================
// Token Generation
// =============================================================================

pub fn generate_session_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.r#gen()).collect();
    hex::encode(&bytes)
}

pub fn generate_csrf_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.r#gen()).collect();
    hex::encode(&bytes)
}

// Inline hex encoding to avoid extra dependency
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

// =============================================================================
// AuthUser Extractor
// =============================================================================

/// Authenticated user, extracted from the session cookie.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AuthUser {
    pub user_id: String,
    pub display_name: String,
    pub is_admin: bool,
    pub session_token: String,
    pub csrf_token: String,
}

/// Optional auth user (for endpoints that work with or without auth).
#[derive(Debug, Clone)]
pub struct MaybeAuthUser(pub Option<AuthUser>);

// =============================================================================
// Auth State (subset of AppState needed for auth)
// =============================================================================

#[derive(Clone)]
pub struct AuthState {
    pub repository: Arc<ConversationRepository>,
    pub auth_config: Arc<AuthConfig>,
}

// =============================================================================
// Cookie Helpers
// =============================================================================

fn extract_session_token(headers: &HeaderMap) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some(value) = cookie.strip_prefix("workshop_session=") {
            return Some(value.to_string());
        }
    }
    None
}

fn make_session_cookie(token: &str, auth_config: &AuthConfig) -> String {
    let mut cookie = format!(
        "workshop_session={}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}",
        token, auth_config.session_ttl_secs
    );
    if auth_config.https {
        cookie.push_str("; Secure");
    }
    cookie
}

fn make_clear_cookie(auth_config: &AuthConfig) -> String {
    let mut cookie = "workshop_session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0".to_string();
    if auth_config.https {
        cookie.push_str("; Secure");
    }
    cookie
}

// =============================================================================
// Auth Middleware
// =============================================================================

/// Synthetic admin identity for trusted loopback connections without a real session.
/// Used when auth is disabled (local-only mode) or when a loopback request has no
/// valid session cookie. This lets admin endpoints (invites, config) work for
/// local CLI/TUI/browser clients without requiring login.
fn synthetic_loopback_admin() -> AuthUser {
    AuthUser {
        user_id: "loopback".to_string(),
        display_name: "Local Admin".to_string(),
        is_admin: true,
        session_token: String::new(),
        csrf_token: String::new(),
    }
}

/// Auth middleware that:
/// 1. Exempts public routes (auth endpoints, health, metrics, share, static)
/// 2. Validates session cookie for protected routes
/// 3. Verifies CSRF token on mutating requests (POST/PUT/DELETE)
pub async fn auth_middleware(
    State(auth_state): State<AuthState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let path = request.uri().path().to_string();
    let method = request.method().clone();

    // Exempt public routes
    // All SPA pages are served under /spa/ and don't need auth middleware
    // (the SPA handles its own auth redirects client-side).
    // API auth endpoints, health checks, and static assets are also exempt.
    if path.starts_with("/api/auth/")
        || path == "/health"
        || path.starts_with("/health/")
        || path == "/metrics"
        || path.starts_with("/api/share/")
        || path.starts_with("/spa/")
        || path == "/"
        || path.ends_with(".js")
        || path.ends_with(".css")
        || path.ends_with(".png")
        || path.ends_with(".ico")
        || path.ends_with(".svg")
        || path.ends_with(".woff2")
    {
        return next.run(request).await;
    }

    // Check if this is a loopback connection (CLI clients discover daemon via local
    // PID/port files, so reaching the server on localhost already proves local trust).
    let is_loopback = request
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .is_some_and(|ci| ci.0.ip().is_loopback());

    // When auth is disabled, inject a synthetic admin for loopback connections
    // so that admin endpoints (invites, etc.) work for local users.
    if !auth_state.auth_config.enabled {
        if is_loopback {
            request.extensions_mut().insert(synthetic_loopback_admin());
        }
        return next.run(request).await;
    }

    // Extract session token from cookie
    let token = match extract_session_token(request.headers()) {
        Some(t) => t,
        None if is_loopback => {
            // Loopback without a session cookie — inject synthetic admin
            request.extensions_mut().insert(synthetic_loopback_admin());
            return next.run(request).await;
        }
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": "Authentication required"
                })),
            )
                .into_response();
        }
    };

    // Look up session
    let (session, user) = match auth_state.repository.get_session_with_user(&token).await {
        Ok(Some(pair)) => pair,
        Ok(None) if is_loopback => {
            // Loopback with an invalid/expired session — inject synthetic admin
            request.extensions_mut().insert(synthetic_loopback_admin());
            return next.run(request).await;
        }
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": "Invalid or expired session"
                })),
            )
                .into_response();
        }
        Err(e) => {
            warn!("Session lookup error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Internal server error"
                })),
            )
                .into_response();
        }
    };

    // CSRF check for mutating methods
    if method == "POST" || method == "PUT" || method == "DELETE" || method == "PATCH" {
        // WebSocket upgrade requests don't need CSRF
        let is_ws = request
            .headers()
            .get("upgrade")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.eq_ignore_ascii_case("websocket"))
            .unwrap_or(false);

        if !is_ws {
            let csrf_header = request
                .headers()
                .get("X-CSRF-Token")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");

            if csrf_header != session.csrf_token {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({
                        "error": "Invalid CSRF token"
                    })),
                )
                    .into_response();
            }
        }
    }

    // Touch session (update last_active_at) - fire and forget
    let repo = auth_state.repository.clone();
    let token_clone = token.clone();
    tokio::spawn(async move {
        if let Err(e) = repo.touch_session(&token_clone).await {
            warn!("Failed to touch session: {}", e);
        }
    });

    // Inject AuthUser into request extensions
    let auth_user = AuthUser {
        user_id: user.id,
        display_name: user.display_name,
        is_admin: user.is_admin,
        session_token: session.token,
        csrf_token: session.csrf_token,
    };
    request.extensions_mut().insert(auth_user);

    next.run(request).await
}

// =============================================================================
// Extractor implementations
// =============================================================================

/// Extract AuthUser from request extensions (set by middleware).
/// Returns 401 if not present.
impl<S> axum::extract::FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, Json<serde_json::Value>);

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        parts.extensions.get::<AuthUser>().cloned().ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Authentication required"})),
            )
        })
    }
}

/// Extract optional AuthUser from request extensions.
impl<S> axum::extract::FromRequestParts<S> for MaybeAuthUser
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        Ok(MaybeAuthUser(parts.extensions.get::<AuthUser>().cloned()))
    }
}

// =============================================================================
// Auth Route Handlers
// =============================================================================

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
    pub display_name: Option<String>,
    pub invite_token: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Serialize)]
struct AuthResponse {
    user: UserInfo,
    csrf_token: String,
}

#[derive(Serialize)]
struct MeResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    user: Option<UserInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    csrf_token: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    needs_setup: bool,
    auth_enabled: bool,
}

/// POST /api/auth/register
async fn register_handler(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(req): Json<RegisterRequest>,
) -> Response {
    if !state.auth_config.enabled {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Auth not enabled"
            })),
        )
            .into_response();
    }

    // Validate input
    let username = req.username.trim();
    if username.len() < 2 || username.len() > 64 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Username must be 2-64 characters"
            })),
        )
            .into_response();
    }

    if req.password.len() < 8 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Password must be at least 8 characters"
            })),
        )
            .into_response();
    }

    // Check if this is the first user (becomes admin)
    let user_count = match state.repository.user_count().await {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to count users: {}", e);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let is_first_user = user_count == 0;

    // Validate server invite token (if provided) before the registration gate
    let valid_invite = if let Some(ref invite_token) = req.invite_token {
        match state.repository.get_server_invite(invite_token).await {
            Ok(Some(invite)) if invite.is_valid() => Some(invite),
            _ => {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({
                        "error": "Invalid or expired invite"
                    })),
                )
                    .into_response();
            }
        }
    } else {
        None
    };

    // If not first user, check registration setting (DB overrides env var)
    if !is_first_user {
        let allow_reg = match state.repository.get_setting("allow_registration").await {
            Ok(Some(v)) => v != "false" && v != "0",
            _ => state.auth_config.allow_registration,
        };

        if !allow_reg && valid_invite.is_none() {
            // Only allow if caller is an authenticated admin
            let caller_is_admin = if let Some(token) = extract_session_token(&headers) {
                match state.repository.get_session_with_user(&token).await {
                    Ok(Some((_session, user))) => user.is_admin,
                    _ => false,
                }
            } else {
                false
            };

            if !caller_is_admin {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({
                        "error": "Registration is disabled. Contact an admin."
                    })),
                )
                    .into_response();
            }
        }
    }

    // Hash password
    let password_hash = match hash_password(&req.password) {
        Ok(h) => h,
        Err(e) => {
            warn!("Password hashing failed: {}", e);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let display_name = req.display_name.unwrap_or_else(|| username.to_string());

    let now = chrono::Utc::now().timestamp();
    let user = User {
        id: uuid::Uuid::new_v4().to_string(),
        username: username.to_string(),
        display_name,
        password_hash,
        is_admin: is_first_user,
        is_disabled: false,
        created_at: now,
        updated_at: now,
    };

    if let Err(e) = state.repository.create_user(&user).await {
        if format!("{:#}", e).contains("UNIQUE") {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "Username already taken"
                })),
            )
                .into_response();
        }
        warn!("Failed to create user: {}", e);
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Record server invite usage if applicable
    if let Some(ref invite) = valid_invite
        && let Err(e) = state
            .repository
            .use_server_invite(&invite.token, &user.id)
            .await
    {
        warn!("Failed to record server invite usage: {}", e);
    }

    info!(
        "User registered: {} (admin: {})",
        user.username, user.is_admin
    );

    // Auto-login: create session
    let session_token = generate_session_token();
    let csrf_token = generate_csrf_token();
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let session = Session {
        token: session_token.clone(),
        user_id: user.id.clone(),
        csrf_token: csrf_token.clone(),
        expires_at: now + state.auth_config.session_ttl_secs as i64,
        last_active_at: now,
        user_agent,
        ip_address: None,
    };

    if let Err(e) = state.repository.create_session(&session).await {
        warn!("Failed to create session: {}", e);
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let cookie = make_session_cookie(&session_token, &state.auth_config);
    let user_info: UserInfo = user.into();

    (
        StatusCode::CREATED,
        [(header::SET_COOKIE, cookie)],
        Json(AuthResponse {
            user: user_info,
            csrf_token,
        }),
    )
        .into_response()
}

/// POST /api/auth/login
async fn login_handler(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Response {
    if !state.auth_config.enabled {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Auth not enabled"
            })),
        )
            .into_response();
    }

    let user = match state.repository.get_user_by_username(&req.username).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            // Prevent timing attacks: still hash something
            let _ = hash_password("dummy_password_for_timing");
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": "Invalid username or password"
                })),
            )
                .into_response();
        }
        Err(e) => {
            warn!("User lookup failed: {}", e);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if user.is_disabled {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "Account is disabled"
            })),
        )
            .into_response();
    }

    if !verify_password(&req.password, &user.password_hash) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "Invalid username or password"
            })),
        )
            .into_response();
    }

    let now = chrono::Utc::now().timestamp();
    let session_token = generate_session_token();
    let csrf_token = generate_csrf_token();
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let session = Session {
        token: session_token.clone(),
        user_id: user.id.clone(),
        csrf_token: csrf_token.clone(),
        expires_at: now + state.auth_config.session_ttl_secs as i64,
        last_active_at: now,
        user_agent,
        ip_address: None,
    };

    if let Err(e) = state.repository.create_session(&session).await {
        warn!("Failed to create session: {}", e);
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    info!("User logged in: {}", user.username);

    let cookie = make_session_cookie(&session_token, &state.auth_config);
    let user_info: UserInfo = user.into();

    (
        [(header::SET_COOKIE, cookie)],
        Json(AuthResponse {
            user: user_info,
            csrf_token,
        }),
    )
        .into_response()
}

/// POST /api/auth/logout
async fn logout_handler(State(state): State<AuthState>, headers: HeaderMap) -> Response {
    if let Some(token) = extract_session_token(&headers)
        && let Err(e) = state.repository.delete_session(&token).await
    {
        warn!("Failed to delete session on logout: {}", e);
    }

    let cookie = make_clear_cookie(&state.auth_config);
    (
        [(header::SET_COOKIE, cookie)],
        Json(serde_json::json!({"ok": true})),
    )
        .into_response()
}

/// GET /api/auth/me
async fn me_handler(State(state): State<AuthState>, headers: HeaderMap) -> Response {
    if !state.auth_config.enabled {
        return Json(MeResponse {
            user: None,
            csrf_token: None,
            needs_setup: false,
            auth_enabled: false,
        })
        .into_response();
    }

    // Check if any users exist
    let user_count = state.repository.user_count().await.unwrap_or(0);
    if user_count == 0 {
        return Json(MeResponse {
            user: None,
            csrf_token: None,
            needs_setup: true,
            auth_enabled: true,
        })
        .into_response();
    }

    // Try to get current user from session
    if let Some(token) = extract_session_token(&headers)
        && let Ok(Some((session, user))) = state.repository.get_session_with_user(&token).await
    {
        return Json(MeResponse {
            user: Some(user.into()),
            csrf_token: Some(session.csrf_token),
            needs_setup: false,
            auth_enabled: true,
        })
        .into_response();
    }

    Json(MeResponse {
        user: None,
        csrf_token: None,
        needs_setup: false,
        auth_enabled: true,
    })
    .into_response()
}

/// POST /api/auth/change-password
async fn change_password_handler(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(req): Json<ChangePasswordRequest>,
) -> Response {
    let token = match extract_session_token(&headers) {
        Some(t) => t,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": "Authentication required"
                })),
            )
                .into_response();
        }
    };

    let (_, user) = match state.repository.get_session_with_user(&token).await {
        Ok(Some(pair)) => pair,
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": "Invalid session"
                })),
            )
                .into_response();
        }
    };

    if !verify_password(&req.current_password, &user.password_hash) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "Current password is incorrect"
            })),
        )
            .into_response();
    }

    if req.new_password.len() < 8 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "New password must be at least 8 characters"
            })),
        )
            .into_response();
    }

    let new_hash = match hash_password(&req.new_password) {
        Ok(h) => h,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    if let Err(e) = state
        .repository
        .update_user_password(&user.id, &new_hash)
        .await
    {
        warn!("Failed to update password: {}", e);
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Invalidate all other sessions (keep the current one so the user stays logged in)
    if let Err(e) = state
        .repository
        .delete_user_sessions(&user.id, Some(&token))
        .await
    {
        warn!("Failed to invalidate old sessions: {}", e);
    }

    Json(serde_json::json!({"ok": true})).into_response()
}

// =============================================================================
// Check Invite Handler
// =============================================================================

#[derive(Deserialize)]
pub struct CheckInviteParams {
    pub token: String,
}

#[derive(Serialize)]
struct CheckInviteResponse {
    valid: bool,
    label: Option<String>,
}

/// GET /api/auth/check-invite?token=...
async fn check_invite_handler(
    State(state): State<AuthState>,
    Query(params): Query<CheckInviteParams>,
) -> Json<CheckInviteResponse> {
    match state.repository.get_server_invite(&params.token).await {
        Ok(Some(invite)) if invite.is_valid() => Json(CheckInviteResponse {
            valid: true,
            label: invite.label,
        }),
        _ => Json(CheckInviteResponse {
            valid: false,
            label: None,
        }),
    }
}

// =============================================================================
// Router
// =============================================================================

pub fn auth_routes() -> Router<AuthState> {
    Router::new()
        .route("/api/auth/register", post(register_handler))
        .route("/api/auth/login", post(login_handler))
        .route("/api/auth/logout", post(logout_handler))
        .route("/api/auth/me", get(me_handler))
        .route("/api/auth/change-password", post(change_password_handler))
        .route("/api/auth/check-invite", get(check_invite_handler))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_password_and_verify() {
        let password = "secret123";
        let hash = hash_password(password).unwrap();

        // Hash should be non-empty and different from password
        assert!(!hash.is_empty());
        assert_ne!(hash, password);

        // Verification should work
        assert!(verify_password(password, &hash));
        assert!(!verify_password("wrong_password", &hash));
    }

    #[test]
    fn test_hash_password_produces_unique_hashes() {
        let password = "same_password";
        let hash1 = hash_password(password).unwrap();
        let hash2 = hash_password(password).unwrap();

        // Same password should produce different hashes (due to random salt)
        assert_ne!(hash1, hash2);

        // Both should verify correctly
        assert!(verify_password(password, &hash1));
        assert!(verify_password(password, &hash2));
    }

    #[test]
    fn test_verify_password_invalid_hash() {
        // Invalid hash format should return false, not panic
        assert!(!verify_password("password", "not_a_valid_hash"));
        assert!(!verify_password("password", ""));
    }

    #[test]
    fn test_generate_session_token() {
        let token1 = generate_session_token();
        let token2 = generate_session_token();

        // Tokens should be 64 hex characters (32 bytes)
        assert_eq!(token1.len(), 64);
        assert_eq!(token2.len(), 64);

        // Tokens should be unique
        assert_ne!(token1, token2);

        // Tokens should be valid hex
        assert!(token1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_generate_csrf_token() {
        let token1 = generate_csrf_token();
        let token2 = generate_csrf_token();

        // CSRF tokens should be 64 hex characters
        assert_eq!(token1.len(), 64);

        // Tokens should be unique
        assert_ne!(token1, token2);
    }

    #[test]
    fn test_hex_encode() {
        assert_eq!(hex::encode(&[0x00, 0xff, 0xab]), "00ffab");
        assert_eq!(hex::encode(&[]), "");
        assert_eq!(hex::encode(&[0x12, 0x34]), "1234");
    }

    #[test]
    fn test_make_session_cookie_http() {
        let config = AuthConfig {
            enabled: true,
            session_ttl_secs: 3600,
            https: false,
            allow_registration: true,
        };

        let cookie = make_session_cookie("test_token", &config);

        assert!(cookie.contains("workshop_session=test_token"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Lax"));
        assert!(cookie.contains("Path=/"));
        assert!(cookie.contains("Max-Age=3600"));
        assert!(!cookie.contains("Secure"));
    }

    #[test]
    fn test_make_session_cookie_https() {
        let config = AuthConfig {
            enabled: true,
            session_ttl_secs: 7200,
            https: true,
            allow_registration: true,
        };

        let cookie = make_session_cookie("test_token", &config);

        assert!(cookie.contains("workshop_session=test_token"));
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("Max-Age=7200"));
    }

    #[test]
    fn test_make_clear_cookie() {
        let config = AuthConfig {
            enabled: true,
            session_ttl_secs: 3600,
            https: false,
            allow_registration: true,
        };

        let cookie = make_clear_cookie(&config);

        assert!(cookie.contains("workshop_session="));
        assert!(cookie.contains("Max-Age=0"));
    }

    #[test]
    fn test_extract_session_token() {
        use axum::http::HeaderValue;

        // Test with valid cookie
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("workshop_session=abc123"),
        );
        assert_eq!(extract_session_token(&headers), Some("abc123".to_string()));

        // Test with multiple cookies
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("other=foo; workshop_session=xyz789; another=bar"),
        );
        assert_eq!(extract_session_token(&headers), Some("xyz789".to_string()));

        // Test with no matching cookie
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, HeaderValue::from_static("other=foo"));
        assert!(extract_session_token(&headers).is_none());

        // Test with no cookie header
        let headers = HeaderMap::new();
        assert!(extract_session_token(&headers).is_none());
    }

    // =========================================================================
    // Handler-level tests (axum router + oneshot)
    // =========================================================================

    use axum::{body::Body, http::Request, routing::get};
    use tower::ServiceExt;

    async fn test_auth_state(enabled: bool) -> AuthState {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite");
        crate::db::run_migrations(&pool).await.expect("migrations");
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .expect("pragma");
        AuthState {
            repository: Arc::new(ConversationRepository::new(pool)),
            auth_config: Arc::new(AuthConfig {
                enabled,
                session_ttl_secs: 3600,
                allow_registration: true,
                https: false,
            }),
        }
    }

    fn auth_router(state: AuthState) -> Router {
        auth_routes().with_state(state)
    }

    fn json_body(val: serde_json::Value) -> Body {
        Body::from(serde_json::to_vec(&val).unwrap())
    }

    #[tokio::test]
    async fn handler_register_first_user_becomes_admin() {
        let state = test_auth_state(true).await;
        let app = auth_router(state.clone());

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/register")
                    .header("content-type", "application/json")
                    .body(json_body(serde_json::json!({
                        "username": "admin",
                        "password": "password123"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CREATED);

        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), 10_000)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["user"]["username"], "admin");
        assert_eq!(body["user"]["is_admin"], true);
        assert!(body["csrf_token"].as_str().is_some());
    }

    #[tokio::test]
    async fn handler_register_second_user_not_admin() {
        let state = test_auth_state(true).await;

        // Register first user
        let app = auth_router(state.clone());
        app.oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/register")
                .header("content-type", "application/json")
                .body(json_body(serde_json::json!({
                    "username": "admin",
                    "password": "password123"
                })))
                .unwrap(),
        )
        .await
        .unwrap();

        // Register second user
        let app = auth_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/register")
                    .header("content-type", "application/json")
                    .body(json_body(serde_json::json!({
                        "username": "user2",
                        "password": "password123",
                        "display_name": "User Two"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CREATED);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), 10_000)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["user"]["is_admin"], false);
        assert_eq!(body["user"]["display_name"], "User Two");
    }

    #[tokio::test]
    async fn handler_register_validation_short_username() {
        let state = test_auth_state(true).await;
        let app = auth_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/register")
                    .header("content-type", "application/json")
                    .body(json_body(serde_json::json!({
                        "username": "a",
                        "password": "password123"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn handler_register_validation_short_password() {
        let state = test_auth_state(true).await;
        let app = auth_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/register")
                    .header("content-type", "application/json")
                    .body(json_body(serde_json::json!({
                        "username": "testuser",
                        "password": "short"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn handler_register_duplicate_username() {
        let state = test_auth_state(true).await;

        // Register first user
        let app = auth_router(state.clone());
        app.oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/register")
                .header("content-type", "application/json")
                .body(json_body(serde_json::json!({
                    "username": "alice",
                    "password": "password123"
                })))
                .unwrap(),
        )
        .await
        .unwrap();

        // Try to register same username
        let app = auth_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/register")
                    .header("content-type", "application/json")
                    .body(json_body(serde_json::json!({
                        "username": "alice",
                        "password": "password456"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn handler_register_auth_disabled() {
        let state = test_auth_state(false).await;
        let app = auth_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/register")
                    .header("content-type", "application/json")
                    .body(json_body(serde_json::json!({
                        "username": "testuser",
                        "password": "password123"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn handler_login_success() {
        let state = test_auth_state(true).await;

        // Register a user first
        let app = auth_router(state.clone());
        app.oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/register")
                .header("content-type", "application/json")
                .body(json_body(serde_json::json!({
                    "username": "alice",
                    "password": "password123"
                })))
                .unwrap(),
        )
        .await
        .unwrap();

        // Login
        let app = auth_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/login")
                    .header("content-type", "application/json")
                    .body(json_body(serde_json::json!({
                        "username": "alice",
                        "password": "password123"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let set_cookie = resp.headers().get("set-cookie").unwrap().to_str().unwrap();
        assert!(set_cookie.contains("workshop_session="));

        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), 10_000)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["user"]["username"], "alice");
    }

    #[tokio::test]
    async fn handler_login_wrong_password() {
        let state = test_auth_state(true).await;

        // Register
        let app = auth_router(state.clone());
        app.oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/register")
                .header("content-type", "application/json")
                .body(json_body(serde_json::json!({
                    "username": "alice",
                    "password": "password123"
                })))
                .unwrap(),
        )
        .await
        .unwrap();

        // Login with wrong password
        let app = auth_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/login")
                    .header("content-type", "application/json")
                    .body(json_body(serde_json::json!({
                        "username": "alice",
                        "password": "wrongpassword"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn handler_login_nonexistent_user() {
        let state = test_auth_state(true).await;
        let app = auth_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/login")
                    .header("content-type", "application/json")
                    .body(json_body(serde_json::json!({
                        "username": "nobody",
                        "password": "password123"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn handler_login_auth_disabled() {
        let state = test_auth_state(false).await;
        let app = auth_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/login")
                    .header("content-type", "application/json")
                    .body(json_body(serde_json::json!({
                        "username": "alice",
                        "password": "password123"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn handler_logout() {
        let state = test_auth_state(true).await;
        let app = auth_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/logout")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let set_cookie = resp.headers().get("set-cookie").unwrap().to_str().unwrap();
        assert!(set_cookie.contains("Max-Age=0"));
    }

    #[tokio::test]
    async fn handler_me_auth_disabled() {
        let state = test_auth_state(false).await;
        let app = auth_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/auth/me")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), 10_000)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["auth_enabled"], false);
    }

    #[tokio::test]
    async fn handler_me_needs_setup() {
        let state = test_auth_state(true).await;
        let app = auth_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/auth/me")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), 10_000)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["needs_setup"], true);
        assert_eq!(body["auth_enabled"], true);
    }

    #[tokio::test]
    async fn handler_me_with_valid_session() {
        let state = test_auth_state(true).await;

        // Register to get a session cookie
        let app = auth_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/register")
                    .header("content-type", "application/json")
                    .body(json_body(serde_json::json!({
                        "username": "alice",
                        "password": "password123"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        let set_cookie = resp.headers().get("set-cookie").unwrap().to_str().unwrap();
        // Extract session token from set-cookie header
        let session_token = set_cookie
            .split(';')
            .next()
            .unwrap()
            .strip_prefix("workshop_session=")
            .unwrap();

        // Call /me with the session cookie
        let app = auth_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/auth/me")
                    .header("cookie", format!("workshop_session={}", session_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), 10_000)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["user"]["username"], "alice");
        assert!(body["csrf_token"].as_str().is_some());
        assert_eq!(body["auth_enabled"], true);
    }

    #[tokio::test]
    async fn handler_me_no_session() {
        let state = test_auth_state(true).await;

        // Create a user so needs_setup is false
        let app = auth_router(state.clone());
        app.oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/register")
                .header("content-type", "application/json")
                .body(json_body(serde_json::json!({
                    "username": "alice",
                    "password": "password123"
                })))
                .unwrap(),
        )
        .await
        .unwrap();

        // Call /me without a session cookie
        let app = auth_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/auth/me")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), 10_000)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(body.get("user").is_none() || body["user"].is_null());
        assert_eq!(body["auth_enabled"], true);
        assert!(
            !body
                .get("needs_setup")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        );
    }

    #[tokio::test]
    async fn handler_change_password_success() {
        let state = test_auth_state(true).await;

        // Register a user
        let app = auth_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/register")
                    .header("content-type", "application/json")
                    .body(json_body(serde_json::json!({
                        "username": "alice",
                        "password": "password123"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        let set_cookie = resp.headers().get("set-cookie").unwrap().to_str().unwrap();
        let session_token = set_cookie
            .split(';')
            .next()
            .unwrap()
            .strip_prefix("workshop_session=")
            .unwrap()
            .to_string();

        // Change password
        let app = auth_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/change-password")
                    .header("content-type", "application/json")
                    .header("cookie", format!("workshop_session={}", session_token))
                    .body(json_body(serde_json::json!({
                        "current_password": "password123",
                        "new_password": "newpassword456"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        // Verify new password works for login
        let app = auth_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/login")
                    .header("content-type", "application/json")
                    .body(json_body(serde_json::json!({
                        "username": "alice",
                        "password": "newpassword456"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn handler_change_password_wrong_current() {
        let state = test_auth_state(true).await;

        // Register a user
        let app = auth_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/register")
                    .header("content-type", "application/json")
                    .body(json_body(serde_json::json!({
                        "username": "alice",
                        "password": "password123"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        let set_cookie = resp.headers().get("set-cookie").unwrap().to_str().unwrap();
        let session_token = set_cookie
            .split(';')
            .next()
            .unwrap()
            .strip_prefix("workshop_session=")
            .unwrap()
            .to_string();

        // Try to change with wrong current password
        let app = auth_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/change-password")
                    .header("content-type", "application/json")
                    .header("cookie", format!("workshop_session={}", session_token))
                    .body(json_body(serde_json::json!({
                        "current_password": "wrongpassword",
                        "new_password": "newpassword456"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn handler_change_password_too_short() {
        let state = test_auth_state(true).await;

        // Register a user
        let app = auth_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/register")
                    .header("content-type", "application/json")
                    .body(json_body(serde_json::json!({
                        "username": "alice",
                        "password": "password123"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        let set_cookie = resp.headers().get("set-cookie").unwrap().to_str().unwrap();
        let session_token = set_cookie
            .split(';')
            .next()
            .unwrap()
            .strip_prefix("workshop_session=")
            .unwrap()
            .to_string();

        // Try to change to short password
        let app = auth_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/change-password")
                    .header("content-type", "application/json")
                    .header("cookie", format!("workshop_session={}", session_token))
                    .body(json_body(serde_json::json!({
                        "current_password": "password123",
                        "new_password": "short"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn handler_change_password_no_session() {
        let state = test_auth_state(true).await;
        let app = auth_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/change-password")
                    .header("content-type", "application/json")
                    .body(json_body(serde_json::json!({
                        "current_password": "password123",
                        "new_password": "newpassword456"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn handler_check_invite_valid() {
        let state = test_auth_state(true).await;

        // Create a server invite directly via the repo
        let now = chrono::Utc::now().timestamp();
        let user = crate::models::User {
            id: "admin-id".to_string(),
            username: "admin".to_string(),
            display_name: "Admin".to_string(),
            password_hash: "hashed".to_string(),
            is_admin: true,
            is_disabled: false,
            created_at: now,
            updated_at: now,
        };
        state.repository.create_user(&user).await.unwrap();

        let invite = crate::models::ServerInvite {
            token: "test-invite-token".to_string(),
            created_by: "admin-id".to_string(),
            label: Some("Test Invite".to_string()),
            max_uses: Some(5),
            use_count: 0,
            expires_at: Some(now + 86400),
            revoked: false,
            created_at: now,
        };
        state
            .repository
            .create_server_invite(&invite)
            .await
            .unwrap();

        let app = auth_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/auth/check-invite?token=test-invite-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), 10_000)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["valid"], true);
        assert_eq!(body["label"], "Test Invite");
    }

    #[tokio::test]
    async fn handler_check_invite_invalid() {
        let state = test_auth_state(true).await;
        let app = auth_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/auth/check-invite?token=nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), 10_000)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["valid"], false);
    }

    #[tokio::test]
    async fn handler_register_with_invite() {
        let state = test_auth_state(true).await;

        // Register first user (admin)
        let app = auth_router(state.clone());
        app.oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/register")
                .header("content-type", "application/json")
                .body(json_body(serde_json::json!({
                    "username": "admin",
                    "password": "password123"
                })))
                .unwrap(),
        )
        .await
        .unwrap();

        // Disable registration
        state
            .repository
            .set_setting("allow_registration", "false")
            .await
            .unwrap();

        // Create invite
        let now = chrono::Utc::now().timestamp();
        let admin = state
            .repository
            .get_user_by_username("admin")
            .await
            .unwrap()
            .unwrap();
        let invite = crate::models::ServerInvite {
            token: "invite-tok".to_string(),
            created_by: admin.id,
            label: None,
            max_uses: Some(1),
            use_count: 0,
            expires_at: Some(now + 86400),
            revoked: false,
            created_at: now,
        };
        state
            .repository
            .create_server_invite(&invite)
            .await
            .unwrap();

        // Register with invite
        let app = auth_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/register")
                    .header("content-type", "application/json")
                    .body(json_body(serde_json::json!({
                        "username": "invitee",
                        "password": "password123",
                        "invite_token": "invite-tok"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn handler_login_disabled_user() {
        let state = test_auth_state(true).await;

        // Register a user
        let app = auth_router(state.clone());
        app.oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/register")
                .header("content-type", "application/json")
                .body(json_body(serde_json::json!({
                    "username": "alice",
                    "password": "password123"
                })))
                .unwrap(),
        )
        .await
        .unwrap();

        // Disable the user directly in DB via raw SQL
        let user = state
            .repository
            .get_user_by_username("alice")
            .await
            .unwrap()
            .unwrap();
        sqlx::query("UPDATE users SET is_disabled = 1 WHERE id = ?")
            .bind(&user.id)
            .execute(&state.repository.pool)
            .await
            .unwrap();

        // Try to login with disabled user
        let app = auth_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/login")
                    .header("content-type", "application/json")
                    .body(json_body(serde_json::json!({
                        "username": "alice",
                        "password": "password123"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    // =========================================================================
    // Auth Middleware Tests
    // =========================================================================

    /// Helper: a dummy handler that returns 200 with a JSON body.
    async fn protected_get_handler() -> Json<serde_json::Value> {
        Json(serde_json::json!({"ok": true}))
    }

    async fn protected_post_handler() -> Json<serde_json::Value> {
        Json(serde_json::json!({"ok": true}))
    }

    fn middleware_router(state: AuthState) -> Router {
        Router::new()
            .route("/api/protected", get(protected_get_handler))
            .route("/api/protected", post(protected_post_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            ))
            .with_state(state)
    }

    /// Helper: register a user and return (session_token, csrf_token).
    async fn register_and_get_tokens(state: &AuthState) -> (String, String) {
        let app = auth_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/register")
                    .header("content-type", "application/json")
                    .body(json_body(serde_json::json!({
                        "username": "testuser",
                        "password": "password123"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let set_cookie = resp
            .headers()
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let session_token = set_cookie
            .split(';')
            .next()
            .unwrap()
            .strip_prefix("workshop_session=")
            .unwrap()
            .to_string();

        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), 10_000)
                .await
                .unwrap(),
        )
        .unwrap();
        let csrf_token = body["csrf_token"].as_str().unwrap().to_string();

        (session_token, csrf_token)
    }

    #[tokio::test]
    async fn middleware_no_session_returns_401() {
        let state = test_auth_state(true).await;
        let app = middleware_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/protected")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn middleware_valid_session_passes() {
        let state = test_auth_state(true).await;
        let (session_token, _csrf) = register_and_get_tokens(&state).await;
        let app = middleware_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/protected")
                    .header("cookie", format!("workshop_session={}", session_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn middleware_invalid_session_returns_401() {
        let state = test_auth_state(true).await;
        // Create a user so DB is populated
        register_and_get_tokens(&state).await;
        let app = middleware_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/protected")
                    .header("cookie", "workshop_session=invalid_token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn middleware_post_without_csrf_returns_403() {
        let state = test_auth_state(true).await;
        let (session_token, _csrf) = register_and_get_tokens(&state).await;
        let app = middleware_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/protected")
                    .header("cookie", format!("workshop_session={}", session_token))
                    .header("content-type", "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn middleware_post_with_valid_csrf_passes() {
        let state = test_auth_state(true).await;
        let (session_token, csrf_token) = register_and_get_tokens(&state).await;
        let app = middleware_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/protected")
                    .header("cookie", format!("workshop_session={}", session_token))
                    .header("X-CSRF-Token", csrf_token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn middleware_post_with_wrong_csrf_returns_403() {
        let state = test_auth_state(true).await;
        let (session_token, _csrf) = register_and_get_tokens(&state).await;
        let app = middleware_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/protected")
                    .header("cookie", format!("workshop_session={}", session_token))
                    .header("X-CSRF-Token", "wrong_csrf_token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn handler_register_closed_without_invite() {
        let state = test_auth_state(true).await;

        // Register first user
        let app = auth_router(state.clone());
        app.oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/register")
                .header("content-type", "application/json")
                .body(json_body(serde_json::json!({
                    "username": "admin",
                    "password": "password123"
                })))
                .unwrap(),
        )
        .await
        .unwrap();

        // Disable registration
        state
            .repository
            .set_setting("allow_registration", "false")
            .await
            .unwrap();

        // Try to register without invite
        let app = auth_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/register")
                    .header("content-type", "application/json")
                    .body(json_body(serde_json::json!({
                        "username": "rando",
                        "password": "password123"
                    })))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
