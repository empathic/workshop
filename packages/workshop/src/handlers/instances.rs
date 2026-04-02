use crate::instance_manager::ClaudeInstance;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;
use crate::auth::MaybeAuthUser;
use crate::claude_driver::ClaudeDriver;
use crate::persistence::InstancePersistor;
use crate::process_driver::{ProcessDriver, ShellDriver};
use crate::ws;

#[derive(Serialize)]
pub struct CreateInstanceResponse {
    id: String,
    name: String,
    wrapper_port: u16,
}

pub async fn list_instances(
    State(state): State<AppState>,
    maybe_user: MaybeAuthUser,
) -> Json<Vec<ClaudeInstance>> {
    let all_instances = state.instance_manager.list().await;

    if state.auth_config.enabled
        && let MaybeAuthUser(Some(ref user)) = maybe_user
    {
        if user.is_admin {
            return Json(all_instances);
        }
        let permitted = state
            .repository
            .list_user_instance_ids(&user.user_id)
            .await
            .unwrap_or_default();
        let filtered = all_instances
            .into_iter()
            .filter(|i| permitted.contains(&i.id))
            .collect();
        return Json(filtered);
    }

    Json(all_instances)
}

#[derive(Deserialize)]
pub struct CreateInstanceRequest {
    name: Option<String>,
    working_dir: Option<String>,
    command: Option<String>,
}

pub async fn create_instance(
    State(state): State<AppState>,
    maybe_user: MaybeAuthUser,
    Json(req): Json<CreateInstanceRequest>,
) -> Result<Json<CreateInstanceResponse>, (StatusCode, String)> {
    // Determine if the command will be Claude and create the appropriate driver.
    let command_str = req
        .command
        .as_deref()
        .unwrap_or(state.instance_manager.default_command());
    let is_claude = command_str.contains("claude");
    let driver: Box<dyn ProcessDriver> = if is_claude {
        Box::new(ClaudeDriver::new())
    } else {
        Box::new(ShellDriver)
    };

    let gsm = &state.global_state_manager;
    match state
        .instance_manager
        .create(
            req.name,
            req.working_dir,
            req.command,
            driver,
            Some(gsm.broadcast_tx().clone()),
            Some(gsm.lifecycle_tx().clone()),
            gsm.claimed_sessions_arc(),
            gsm.first_input_data_arc(),
            gsm.pending_attributions_arc(),
            Some(state.repository.clone()),
            None,
        )
        .await
    {
        Ok(instance) => {
            state.metrics.instance_created();

            let created_at: DateTime<Utc> =
                instance.created_at.parse().unwrap_or_else(|_| Utc::now());

            if let Some(handle) = state.instance_manager.get_handle(&instance.id).await {
                state
                    .global_state_manager
                    .register_instance(
                        instance.id.clone(),
                        handle,
                        instance.working_dir.clone(),
                        created_at,
                        instance.kind.is_structured(),
                    )
                    .await;
            }

            if state.auth_config.enabled
                && let MaybeAuthUser(Some(ref user)) = maybe_user
            {
                let perm = crate::models::InstancePermission {
                    instance_id: instance.id.clone(),
                    user_id: user.user_id.clone(),
                    role: "owner".to_string(),
                    granted_at: chrono::Utc::now().timestamp(),
                    granted_by: None,
                };
                if let Err(e) = state.repository.create_instance_permission(&perm).await {
                    tracing::warn!("Failed to set instance owner: {}", e);
                }
            }

            if instance.kind.is_structured() {
                let persistor = Arc::new(InstancePersistor::new(
                    instance.id.clone(),
                    instance.working_dir.clone(),
                    state.persistence_service.clone(),
                ));
                persistor.clone().start_monitoring().await;
                state
                    .instance_persistors
                    .lock()
                    .await
                    .insert(instance.id.clone(), persistor);
            }

            state
                .global_state_manager
                .broadcast_lifecycle(ws::ServerMessage::InstanceCreated {
                    instance: instance.clone(),
                });

            Ok(Json(CreateInstanceResponse {
                id: instance.id.clone(),
                name: instance.name.clone(),
                wrapper_port: instance.wrapper_port,
            }))
        }
        Err(e) => {
            tracing::error!("Failed to create instance: {}", e);
            state.metrics.pty_error();
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create instance: {}", e),
            ))
        }
    }
}

pub async fn get_instance(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.instance_manager.get(&id).await {
        Some(instance) => Json(instance).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub async fn delete_instance(
    State(state): State<AppState>,
    maybe_user: MaybeAuthUser,
    Path(id): Path<String>,
) -> StatusCode {
    if state.auth_config.enabled
        && let MaybeAuthUser(Some(ref user)) = maybe_user
        && !user.is_admin
    {
        match state
            .repository
            .check_instance_permission(&id, &user.user_id)
            .await
        {
            Ok(Some(perm)) if perm.role == "owner" => {}
            _ => return StatusCode::FORBIDDEN,
        }
    }

    state.instance_persistors.lock().await.remove(&id);
    state.global_state_manager.unregister_instance(&id).await;

    if state.instance_manager.stop(&id).await {
        state.metrics.instance_stopped();

        state
            .global_state_manager
            .broadcast_lifecycle(ws::ServerMessage::InstanceStopped { instance_id: id });

        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

#[derive(Deserialize)]
pub struct SetCustomNameRequest {
    custom_name: Option<String>,
}

pub async fn set_custom_name(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<SetCustomNameRequest>,
) -> Result<StatusCode, StatusCode> {
    let custom_name = req.custom_name.filter(|n| !n.trim().is_empty());

    state
        .instance_manager
        .set_custom_name(&id, custom_name.clone())
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    state
        .global_state_manager
        .broadcast_lifecycle(ws::ServerMessage::InstanceRenamed {
            instance_id: id,
            custom_name,
        });

    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_instance_output(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    if let Some(handle) = state.instance_manager.get_handle(&id).await {
        let max_bytes = state.server_config.websocket.max_history_replay_bytes;
        let output = handle.get_recent_output(max_bytes, 24).await;
        Json(serde_json::json!({ "lines": output })).into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

// --- Instance invitation handlers ---

#[derive(Deserialize)]
pub struct CreateInvitationRequest {
    role: Option<String>,
    max_uses: Option<i32>,
    expires_in_hours: Option<i64>,
}

pub async fn create_invitation(
    State(state): State<AppState>,
    maybe_user: MaybeAuthUser,
    Path(instance_id): Path<String>,
    Json(req): Json<CreateInvitationRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let user = match maybe_user.0 {
        Some(u) => u,
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    if !user.is_admin {
        match state
            .repository
            .check_instance_permission(&instance_id, &user.user_id)
            .await
        {
            Ok(Some(perm)) if perm.role == "owner" => {}
            _ => return Err(StatusCode::FORBIDDEN),
        }
    }

    let now = chrono::Utc::now().timestamp();
    let invite = crate::models::InstanceInvitation {
        invite_token: uuid::Uuid::new_v4().to_string(),
        instance_id,
        created_by: user.user_id,
        role: req.role.unwrap_or_else(|| "collaborator".to_string()),
        max_uses: req.max_uses,
        use_count: 0,
        expires_at: req.expires_in_hours.map(|h| now + h * 3600),
        created_at: now,
    };

    state
        .repository
        .create_invitation(&invite)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "invite_token": invite.invite_token,
    })))
}

pub async fn accept_invitation(
    State(state): State<AppState>,
    maybe_user: MaybeAuthUser,
    Path(token): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let user = match maybe_user.0 {
        Some(u) => u,
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    let invite = match state.repository.get_invitation(&token).await {
        Ok(Some(i)) => i,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    if invite.is_expired() || invite.is_used_up() {
        return Err(StatusCode::GONE);
    }

    let perm = crate::models::InstancePermission {
        instance_id: invite.instance_id.clone(),
        user_id: user.user_id,
        role: invite.role.clone(),
        granted_at: chrono::Utc::now().timestamp(),
        granted_by: Some(invite.created_by.clone()),
    };

    state
        .repository
        .create_instance_permission(&perm)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    state
        .repository
        .accept_invitation(&token)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "instance_id": invite.instance_id,
        "role": invite.role,
    })))
}

pub async fn remove_collaborator(
    State(state): State<AppState>,
    maybe_user: MaybeAuthUser,
    Path((instance_id, target_user_id)): Path<(String, String)>,
) -> StatusCode {
    let user = match maybe_user.0 {
        Some(u) => u,
        None => return StatusCode::UNAUTHORIZED,
    };

    if !user.is_admin {
        match state
            .repository
            .check_instance_permission(&instance_id, &user.user_id)
            .await
        {
            Ok(Some(perm)) if perm.role == "owner" => {}
            _ => return StatusCode::FORBIDDEN,
        }
    }

    match state
        .repository
        .delete_instance_permission(&instance_id, &target_user_id)
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        http::Request,
        routing::{delete, get, patch, post},
    };
    use tower::ServiceExt;

    async fn test_router() -> (Router, tempfile::TempDir) {
        let (state, tmp) = crate::test_helpers::test_app_state().await;
        let router = Router::new()
            .route("/instances", get(list_instances))
            .route("/instances/{id}", get(get_instance))
            .route("/instances/{id}", delete(delete_instance))
            .route("/instances/{id}/name", patch(set_custom_name))
            .route("/instances/{id}/output", get(get_instance_output))
            .route("/instances/{id}/invitations", post(create_invitation))
            .route("/invitations/{token}/accept", post(accept_invitation))
            .with_state(state);
        (router, tmp)
    }

    #[tokio::test]
    async fn test_list_instances_empty() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/instances")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let instances: Vec<ClaudeInstance> = serde_json::from_slice(&body).unwrap();
        assert!(instances.is_empty());
    }

    #[tokio::test]
    async fn test_get_instance_not_found() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/instances/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_delete_instance_not_found() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/instances/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_instance_output_not_found() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/instances/nonexistent/output")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_accept_invitation_not_found() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/invitations/nonexistent-token/accept")
                    .header("content-type", "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Without auth, should get UNAUTHORIZED (no session cookie)
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_create_instance_request_deserialization() {
        let json = r#"{"name": "test", "working_dir": "/tmp", "command": "echo hello"}"#;
        let req: CreateInstanceRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name.as_deref(), Some("test"));
        assert_eq!(req.working_dir.as_deref(), Some("/tmp"));
        assert_eq!(req.command.as_deref(), Some("echo hello"));
    }

    #[tokio::test]
    async fn test_create_instance_request_all_optional() {
        let json = r#"{}"#;
        let req: CreateInstanceRequest = serde_json::from_str(json).unwrap();
        assert!(req.name.is_none());
        assert!(req.working_dir.is_none());
        assert!(req.command.is_none());
    }

    #[tokio::test]
    async fn test_set_custom_name_request_deserialization() {
        let json = r#"{"custom_name": "My Instance"}"#;
        let req: SetCustomNameRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.custom_name.as_deref(), Some("My Instance"));

        let json_null = r#"{"custom_name": null}"#;
        let req2: SetCustomNameRequest = serde_json::from_str(json_null).unwrap();
        assert!(req2.custom_name.is_none());
    }

    #[tokio::test]
    async fn test_create_invitation_request_deserialization() {
        let json = r#"{"role": "collaborator", "max_uses": 5, "expires_in_hours": 24}"#;
        let req: CreateInvitationRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.role.as_deref(), Some("collaborator"));
        assert_eq!(req.max_uses, Some(5));
        assert_eq!(req.expires_in_hours, Some(24));
    }

    #[tokio::test]
    async fn test_create_invitation_request_defaults() {
        let json = r#"{}"#;
        let req: CreateInvitationRequest = serde_json::from_str(json).unwrap();
        assert!(req.role.is_none());
        assert!(req.max_uses.is_none());
        assert!(req.expires_in_hours.is_none());
    }

    #[tokio::test]
    async fn test_create_and_get_instance() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let router = Router::new()
            .route("/instances", get(list_instances).post(create_instance))
            .route("/instances/{id}", get(get_instance))
            .route("/instances/{id}", delete(delete_instance))
            .route("/instances/{id}/name", patch(set_custom_name))
            .route("/instances/{id}/output", get(get_instance_output))
            .with_state(state);

        // Create an instance with a short-lived command
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/instances")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"command":"echo hello","working_dir":"/tmp"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let instance_id = created["id"].as_str().unwrap().to_string();
        assert!(!instance_id.is_empty());

        // Get instance by ID
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/instances/{}", instance_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let instance: ClaudeInstance = serde_json::from_slice(&body).unwrap();
        assert_eq!(instance.id, instance_id);
        assert_eq!(instance.command, "echo hello");

        // List instances - should have at least one
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/instances")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let instances: Vec<ClaudeInstance> = serde_json::from_slice(&body).unwrap();
        assert!(!instances.is_empty());
        assert!(instances.iter().any(|i| i.id == instance_id));

        // Get instance output
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/instances/{}/output", instance_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Set custom name
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/instances/{}/name", instance_id))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"custom_name":"My Echo"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Delete instance
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/instances/{}", instance_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Verify it's gone
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/instances/{}", instance_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_create_instance_with_custom_name() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let router = Router::new()
            .route("/instances", post(create_instance))
            .with_state(state);

        // Create with custom name, default command (echo)
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/instances")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"my-instance","working_dir":"/tmp"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(created["name"], "my-instance");
    }

    #[tokio::test]
    async fn test_create_instance_response_serialization() {
        let resp = CreateInstanceResponse {
            id: "inst-1".to_string(),
            name: "swift-azure-falcon".to_string(),
            wrapper_port: 9001,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["id"], "inst-1");
        assert_eq!(json["name"], "swift-azure-falcon");
        assert_eq!(json["wrapper_port"], 9001);
    }

    // === Auth-gated handler tests ===

    /// Helper to inject an AuthUser into request extensions.
    fn inject_auth(req: &mut Request<Body>, auth_user: &crate::auth::AuthUser) {
        req.extensions_mut().insert(auth_user.clone());
    }

    /// Router with auth-enabled state for invitation/collaboration tests.
    async fn auth_test_router() -> (Router, tempfile::TempDir, crate::auth::AuthUser, AppState) {
        let (state, tmp, admin_user) = crate::test_helpers::test_app_state_with_auth().await;
        let router = Router::new()
            .route("/instances", get(list_instances).post(create_instance))
            .route("/instances/{id}", get(get_instance).delete(delete_instance))
            .route("/instances/{id}/invitations", post(create_invitation))
            .route("/invitations/{token}/accept", post(accept_invitation))
            .route(
                "/instances/{id}/collaborators/{user_id}",
                delete(remove_collaborator),
            )
            .with_state(state.clone());
        (router, tmp, admin_user, state)
    }

    #[tokio::test]
    async fn test_create_invitation_as_admin() {
        let (app, _tmp, admin_user, _state) = auth_test_router().await;

        // Create an instance first (non-claude command so no InstancePersistor)
        let mut req = Request::builder()
            .method("POST")
            .uri("/instances")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"command":"echo test","working_dir":"/tmp"}"#,
            ))
            .unwrap();
        inject_auth(&mut req, &admin_user);
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let inst_id = created["id"].as_str().unwrap().to_string();

        // Admin creates invitation
        let mut req = Request::builder()
            .method("POST")
            .uri(format!("/instances/{}/invitations", inst_id))
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"role":"collaborator","max_uses":5,"expires_in_hours":24}"#,
            ))
            .unwrap();
        inject_auth(&mut req, &admin_user);
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["invite_token"].as_str().is_some());

        // Clean up
        let mut req = Request::builder()
            .method("DELETE")
            .uri(format!("/instances/{}", inst_id))
            .body(Body::empty())
            .unwrap();
        inject_auth(&mut req, &admin_user);
        let _ = app.oneshot(req).await;
    }

    #[tokio::test]
    async fn test_create_invitation_unauthorized() {
        let (app, _tmp, _admin_user, _state) = auth_test_router().await;

        // No auth user injected → UNAUTHORIZED
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/instances/some-inst/invitations")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_create_invitation_forbidden_non_owner() {
        let (app, _tmp, _admin, state) = auth_test_router().await;

        // Create a regular user (not admin, not owner)
        let regular_user = crate::test_helpers::create_test_user(
            &state.repository,
            "regular-1",
            "regular",
            "Regular User",
        )
        .await;

        let mut req = Request::builder()
            .method("POST")
            .uri("/instances/some-inst/invitations")
            .header("content-type", "application/json")
            .body(Body::from(r#"{}"#))
            .unwrap();
        inject_auth(&mut req, &regular_user);
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_accept_invitation_success() {
        let (app, _tmp, admin_user, state) = auth_test_router().await;

        // Create invitation directly in DB
        let invite = crate::models::InstanceInvitation {
            invite_token: "test-invite-token".to_string(),
            instance_id: "inst-1".to_string(),
            created_by: admin_user.user_id.clone(),
            role: "collaborator".to_string(),
            max_uses: Some(5),
            use_count: 0,
            expires_at: Some(chrono::Utc::now().timestamp() + 86400),
            created_at: chrono::Utc::now().timestamp(),
        };
        state.repository.create_invitation(&invite).await.unwrap();

        // Create a user to accept the invitation
        let accepting_user = crate::test_helpers::create_test_user(
            &state.repository,
            "accepter-1",
            "accepter",
            "Accepter",
        )
        .await;

        let mut req = Request::builder()
            .method("POST")
            .uri("/invitations/test-invite-token/accept")
            .header("content-type", "application/json")
            .body(Body::empty())
            .unwrap();
        inject_auth(&mut req, &accepting_user);
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["instance_id"], "inst-1");
        assert_eq!(json["role"], "collaborator");
    }

    #[tokio::test]
    async fn test_accept_invitation_expired() {
        let (app, _tmp, _admin, state) = auth_test_router().await;

        // Create an expired invitation
        let invite = crate::models::InstanceInvitation {
            invite_token: "expired-token".to_string(),
            instance_id: "inst-1".to_string(),
            created_by: "admin-user".to_string(),
            role: "collaborator".to_string(),
            max_uses: None,
            use_count: 0,
            expires_at: Some(0), // epoch = definitely past
            created_at: 0,
        };
        state.repository.create_invitation(&invite).await.unwrap();

        let user =
            crate::test_helpers::create_test_user(&state.repository, "u-1", "user1", "User 1")
                .await;

        let mut req = Request::builder()
            .method("POST")
            .uri("/invitations/expired-token/accept")
            .header("content-type", "application/json")
            .body(Body::empty())
            .unwrap();
        inject_auth(&mut req, &user);
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::GONE);
    }

    #[tokio::test]
    async fn test_accept_invitation_used_up() {
        let (app, _tmp, _admin, state) = auth_test_router().await;

        let invite = crate::models::InstanceInvitation {
            invite_token: "used-up-token".to_string(),
            instance_id: "inst-1".to_string(),
            created_by: "admin-user".to_string(),
            role: "collaborator".to_string(),
            max_uses: Some(1),
            use_count: 1, // already fully used
            expires_at: None,
            created_at: chrono::Utc::now().timestamp(),
        };
        state.repository.create_invitation(&invite).await.unwrap();

        let user =
            crate::test_helpers::create_test_user(&state.repository, "u-2", "user2", "User 2")
                .await;

        let mut req = Request::builder()
            .method("POST")
            .uri("/invitations/used-up-token/accept")
            .header("content-type", "application/json")
            .body(Body::empty())
            .unwrap();
        inject_auth(&mut req, &user);
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::GONE);
    }

    #[tokio::test]
    async fn test_remove_collaborator_as_admin() {
        let (app, _tmp, admin_user, state) = auth_test_router().await;

        // Create the collaborator user first (FK constraint)
        let _ = crate::test_helpers::create_test_user(
            &state.repository,
            "collab-user",
            "collab",
            "Collab User",
        )
        .await;

        // Create a collaborator permission
        let perm = crate::models::InstancePermission {
            instance_id: "inst-1".to_string(),
            user_id: "collab-user".to_string(),
            role: "collaborator".to_string(),
            granted_at: chrono::Utc::now().timestamp(),
            granted_by: Some("admin-user".to_string()),
        };
        state
            .repository
            .create_instance_permission(&perm)
            .await
            .unwrap();

        let mut req = Request::builder()
            .method("DELETE")
            .uri("/instances/inst-1/collaborators/collab-user")
            .body(Body::empty())
            .unwrap();
        inject_auth(&mut req, &admin_user);
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_remove_collaborator_forbidden() {
        let (app, _tmp, _admin, state) = auth_test_router().await;

        // Create a regular user (not admin, not owner)
        let regular_user = crate::test_helpers::create_test_user(
            &state.repository,
            "regular-2",
            "regular2",
            "Regular 2",
        )
        .await;

        let mut req = Request::builder()
            .method("DELETE")
            .uri("/instances/inst-1/collaborators/someone")
            .body(Body::empty())
            .unwrap();
        inject_auth(&mut req, &regular_user);
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_remove_collaborator_unauthorized() {
        let (app, _tmp, _admin, _state) = auth_test_router().await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/instances/inst-1/collaborators/someone")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_delete_instance_forbidden_non_owner() {
        let (app, _tmp, admin_user, state) = auth_test_router().await;

        // Create an instance via the admin
        let mut req = Request::builder()
            .method("POST")
            .uri("/instances")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"command":"echo test","working_dir":"/tmp"}"#,
            ))
            .unwrap();
        inject_auth(&mut req, &admin_user);
        let resp = app.clone().oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let inst_id = created["id"].as_str().unwrap().to_string();

        // Create a non-owner, non-admin user
        let regular_user = crate::test_helpers::create_test_user(
            &state.repository,
            "regular-3",
            "regular3",
            "Regular 3",
        )
        .await;

        // Non-owner tries to delete → FORBIDDEN
        let mut req = Request::builder()
            .method("DELETE")
            .uri(format!("/instances/{}", inst_id))
            .body(Body::empty())
            .unwrap();
        inject_auth(&mut req, &regular_user);
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Clean up: admin deletes it
        let mut req = Request::builder()
            .method("DELETE")
            .uri(format!("/instances/{}", inst_id))
            .body(Body::empty())
            .unwrap();
        inject_auth(&mut req, &admin_user);
        let _ = app.oneshot(req).await;
    }

    #[tokio::test]
    async fn test_list_instances_auth_non_admin_filtered() {
        let (app, _tmp, admin_user, state) = auth_test_router().await;

        // Create two instances via admin
        let mut ids = Vec::new();
        for _ in 0..2 {
            let mut req = Request::builder()
                .method("POST")
                .uri("/instances")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"command":"echo test","working_dir":"/tmp"}"#,
                ))
                .unwrap();
            inject_auth(&mut req, &admin_user);
            let resp = app.clone().oneshot(req).await.unwrap();
            let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
                .await
                .unwrap();
            let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
            ids.push(created["id"].as_str().unwrap().to_string());
        }

        // Create a regular user with permission to only the first instance
        let regular_user = crate::test_helpers::create_test_user(
            &state.repository,
            "regular-4",
            "regular4",
            "Regular 4",
        )
        .await;
        let perm = crate::models::InstancePermission {
            instance_id: ids[0].clone(),
            user_id: "regular-4".to_string(),
            role: "collaborator".to_string(),
            granted_at: chrono::Utc::now().timestamp(),
            granted_by: Some("admin-user".to_string()),
        };
        state
            .repository
            .create_instance_permission(&perm)
            .await
            .unwrap();

        // Regular user lists instances → should see only the one they have permission to
        let mut req = Request::builder()
            .uri("/instances")
            .body(Body::empty())
            .unwrap();
        inject_auth(&mut req, &regular_user);
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let instances: Vec<ClaudeInstance> = serde_json::from_slice(&body).unwrap();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].id, ids[0]);

        // Admin lists all → should see both
        let mut req = Request::builder()
            .uri("/instances")
            .body(Body::empty())
            .unwrap();
        inject_auth(&mut req, &admin_user);
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let instances: Vec<ClaudeInstance> = serde_json::from_slice(&body).unwrap();
        assert!(instances.len() >= 2);

        // Clean up
        for id in &ids {
            let mut req = Request::builder()
                .method("DELETE")
                .uri(format!("/instances/{}", id))
                .body(Body::empty())
                .unwrap();
            inject_auth(&mut req, &admin_user);
            let _ = app.clone().oneshot(req).await;
        }
    }

    #[tokio::test]
    async fn test_create_invitation_as_owner_non_admin() {
        let (app, _tmp, _admin, state) = auth_test_router().await;

        // Create a regular user who is the owner of an instance
        let owner_user = crate::test_helpers::create_test_user(
            &state.repository,
            "owner-1",
            "owner",
            "Instance Owner",
        )
        .await;

        // Grant owner permission
        let perm = crate::models::InstancePermission {
            instance_id: "owned-inst".to_string(),
            user_id: "owner-1".to_string(),
            role: "owner".to_string(),
            granted_at: chrono::Utc::now().timestamp(),
            granted_by: None,
        };
        state
            .repository
            .create_instance_permission(&perm)
            .await
            .unwrap();

        // Owner creates invitation
        let mut req = Request::builder()
            .method("POST")
            .uri("/instances/owned-inst/invitations")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"role":"collaborator"}"#))
            .unwrap();
        inject_auth(&mut req, &owner_user);
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_remove_collaborator_as_owner() {
        let (app, _tmp, _admin, state) = auth_test_router().await;

        // Create owner user
        let owner_user = crate::test_helpers::create_test_user(
            &state.repository,
            "owner-2",
            "owner2",
            "Owner 2",
        )
        .await;

        // Grant owner permission
        let perm = crate::models::InstancePermission {
            instance_id: "owned-inst-2".to_string(),
            user_id: "owner-2".to_string(),
            role: "owner".to_string(),
            granted_at: chrono::Utc::now().timestamp(),
            granted_by: None,
        };
        state
            .repository
            .create_instance_permission(&perm)
            .await
            .unwrap();

        // Create the collaborator user first (FK constraint)
        let _ = crate::test_helpers::create_test_user(
            &state.repository,
            "collab-to-remove",
            "collabrem",
            "Collab Remove",
        )
        .await;

        // Create collaborator permission to remove
        let collab_perm = crate::models::InstancePermission {
            instance_id: "owned-inst-2".to_string(),
            user_id: "collab-to-remove".to_string(),
            role: "collaborator".to_string(),
            granted_at: chrono::Utc::now().timestamp(),
            granted_by: Some("owner-2".to_string()),
        };
        state
            .repository
            .create_instance_permission(&collab_perm)
            .await
            .unwrap();

        // Owner removes collaborator
        let mut req = Request::builder()
            .method("DELETE")
            .uri("/instances/owned-inst-2/collaborators/collab-to-remove")
            .body(Body::empty())
            .unwrap();
        inject_auth(&mut req, &owner_user);
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
}
