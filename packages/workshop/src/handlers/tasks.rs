use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;
use crate::auth::MaybeAuthUser;
use crate::ws;

/// Broadcast a full task snapshot (with tags + dispatches) to all WS clients.
fn broadcast_task(state: &AppState, task_with_tags: &crate::models::TaskWithTags) {
    state
        .global_state_manager
        .broadcast_lifecycle(ws::ServerMessage::TaskUpdate {
            task: serde_json::to_value(task_with_tags).unwrap_or_default(),
        });
}

/// Fetch a full task snapshot and broadcast it. Silently no-ops if the task is gone.
async fn broadcast_task_by_id(state: &AppState, id: i64) {
    if let Ok(Some(twt)) = state.repository.get_task_with_tags(id).await {
        broadcast_task(state, &twt);
    }
}

pub async fn list_tasks_handler(
    State(state): State<AppState>,
    _maybe_user: MaybeAuthUser,
    Query(filters): Query<crate::models::TaskListFilters>,
) -> Result<Json<Vec<crate::models::TaskWithTags>>, (StatusCode, String)> {
    match state.repository.list_tasks(&filters).await {
        Ok(tasks) => Ok(Json(tasks)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

pub async fn create_task_handler(
    State(state): State<AppState>,
    maybe_user: MaybeAuthUser,
    Json(req): Json<crate::models::CreateTaskRequest>,
) -> Result<Json<crate::models::TaskWithTags>, (StatusCode, String)> {
    let now = chrono::Utc::now().timestamp();
    let sort_order = state
        .repository
        .get_next_sort_order(req.instance_id.as_deref())
        .await
        .unwrap_or(1.0);

    let (creator_id, creator_name) = match &maybe_user {
        MaybeAuthUser(Some(user)) => (Some(user.user_id.clone()), user.display_name.clone()),
        _ => (None, "anonymous".to_string()),
    };

    let task = crate::models::Task {
        id: None,
        uuid: Uuid::new_v4().to_string(),
        title: req.title.clone(),
        body: req.body.clone(),
        status: req.status.unwrap_or_else(|| "pending".to_string()),
        priority: req.priority.unwrap_or(0),
        instance_id: req.instance_id.clone(),
        creator_id,
        creator_name,
        sort_order,
        created_at: now,
        updated_at: now,
        completed_at: None,
        is_deleted: false,
        sent_text: None,
        conversation_id: None,
    };

    let id = state
        .repository
        .create_task(&task)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Add tags if provided
    if let Some(tags) = &req.tags {
        for tag_name in tags {
            let _ = state.repository.add_task_tag(id, tag_name).await;
        }
    }

    // Fetch the full task (with tags + dispatches)
    let task_with_tags = state
        .repository
        .get_task_with_tags(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                "Task not found after creation".to_string(),
            )
        })?;

    broadcast_task(&state, &task_with_tags);

    Ok(Json(task_with_tags))
}

pub async fn get_task_handler(
    State(state): State<AppState>,
    _maybe_user: MaybeAuthUser,
    Path(id): Path<i64>,
) -> Result<Json<crate::models::TaskWithTags>, (StatusCode, String)> {
    let task_with_tags = state
        .repository
        .get_task_with_tags(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Task not found".to_string()))?;

    Ok(Json(task_with_tags))
}

pub async fn update_task_handler(
    State(state): State<AppState>,
    _maybe_user: MaybeAuthUser,
    Path(id): Path<i64>,
    Json(req): Json<crate::models::UpdateTaskRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    state
        .repository
        .update_task(id, &req)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    broadcast_task_by_id(&state, id).await;

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn delete_task_handler(
    State(state): State<AppState>,
    _maybe_user: MaybeAuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    state
        .repository
        .delete_task(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    state
        .global_state_manager
        .broadcast_lifecycle(ws::ServerMessage::TaskDeleted { task_id: id });

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn send_task_handler(
    State(state): State<AppState>,
    _maybe_user: MaybeAuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let task = state
        .repository
        .get_task(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Task not found".to_string()))?;

    let instance_id = task.instance_id.as_ref().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "Task has no assigned instance".to_string(),
        )
    })?;

    let handle = state
        .instance_manager
        .get_handle(instance_id)
        .await
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                "Instance not found or not running".to_string(),
            )
        })?;

    let text = task.body.as_deref().unwrap_or(&task.title);
    handle.write_input(text).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to write to PTY: {}", e),
        )
    })?;

    handle.write_input("\r").await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to send Enter: {}", e),
        )
    })?;

    let _ = state
        .repository
        .create_task_dispatch(id, instance_id, text)
        .await;

    let new_status = if task.status == "pending" {
        let update = crate::models::UpdateTaskRequest {
            status: Some("in_progress".to_string()),
            ..Default::default()
        };
        let _ = state.repository.update_task(id, &update).await;
        "in_progress"
    } else {
        &task.status
    };

    broadcast_task_by_id(&state, id).await;

    Ok(Json(
        serde_json::json!({ "ok": true, "status": new_status }),
    ))
}

#[derive(Deserialize)]
pub struct CreateDispatchRequest {
    pub instance_id: String,
    pub sent_text: String,
}

pub async fn create_dispatch_handler(
    State(state): State<AppState>,
    _maybe_user: MaybeAuthUser,
    Path(id): Path<i64>,
    Json(req): Json<CreateDispatchRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let task = state
        .repository
        .get_task(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Task not found".to_string()))?;

    let dispatch = state
        .repository
        .create_task_dispatch(id, &req.instance_id, &req.sent_text)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if task.status == "pending" {
        let update = crate::models::UpdateTaskRequest {
            status: Some("in_progress".to_string()),
            ..Default::default()
        };
        let _ = state.repository.update_task(id, &update).await;
    }

    broadcast_task_by_id(&state, id).await;

    Ok(Json(serde_json::json!({
        "ok": true,
        "dispatch_id": dispatch.id,
        "status": if task.status == "pending" { "in_progress" } else { &task.status }
    })))
}

#[derive(Deserialize)]
pub struct AddTagRequest {
    pub tag: String,
}

pub async fn add_task_tag_handler(
    State(state): State<AppState>,
    _maybe_user: MaybeAuthUser,
    Path(id): Path<i64>,
    Json(req): Json<AddTagRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    state
        .repository
        .add_task_tag(id, &req.tag)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    broadcast_task_by_id(&state, id).await;

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn remove_task_tag_handler(
    State(state): State<AppState>,
    _maybe_user: MaybeAuthUser,
    Path((id, tag_id)): Path<(i64, i64)>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    state
        .repository
        .remove_task_tag(id, tag_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    broadcast_task_by_id(&state, id).await;

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn migrate_tasks_handler(
    State(state): State<AppState>,
    _maybe_user: MaybeAuthUser,
    Json(items): Json<Vec<crate::models::MigrateTaskItem>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let ids = state
        .repository
        .migrate_tasks(&items)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::json!({ "ok": true, "ids": ids })))
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
            .route("/tasks", get(list_tasks_handler).post(create_task_handler))
            .route("/tasks/migrate", post(migrate_tasks_handler))
            .route(
                "/tasks/{id}",
                get(get_task_handler)
                    .patch(update_task_handler)
                    .delete(delete_task_handler),
            )
            .with_state(state);
        (router, tmp)
    }

    #[tokio::test]
    async fn test_list_tasks_empty() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/tasks")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_create_and_get_task() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let app = Router::new()
            .route("/tasks", post(create_task_handler))
            .route("/tasks/{id}", get(get_task_handler))
            .with_state(state);

        // Create
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":"Test task","body":"Do the thing"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(created["title"], "Test task");
        assert_eq!(created["status"], "pending");

        // Get by ID
        let id = created["id"].as_i64().unwrap();
        let get_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/tasks/{}", id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let fetched: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(fetched["title"], "Test task");
    }

    #[tokio::test]
    async fn test_update_task() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let app = Router::new()
            .route("/tasks", post(create_task_handler))
            .route("/tasks/{id}", patch(update_task_handler))
            .with_state(state);

        // Create
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":"Original"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let id = created["id"].as_i64().unwrap();

        // Update
        let update_resp = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/tasks/{}", id))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":"Updated","status":"in_progress"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(update_resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_delete_task() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let app = Router::new()
            .route("/tasks", post(create_task_handler))
            .route("/tasks/{id}", delete(delete_task_handler))
            .with_state(state);

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":"To delete"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let id = created["id"].as_i64().unwrap();

        let del_resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/tasks/{}", id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_task_not_found() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/tasks/99999")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_create_task_with_priority() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":"High pri","priority":5}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["priority"], 5);
    }

    #[tokio::test]
    async fn test_migrate_tasks() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/tasks/migrate")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"[{"title":"Migrated task 1"},{"title":"Migrated task 2"}]"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["ok"].as_bool().unwrap());
        assert_eq!(json["ids"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_add_and_remove_task_tag() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let app = Router::new()
            .route("/tasks", post(create_task_handler))
            .route("/tasks/{id}", get(get_task_handler))
            .route("/tasks/{id}/tags", post(add_task_tag_handler))
            .route("/tasks/{id}/tags/{tag_id}", delete(remove_task_tag_handler))
            .with_state(state);

        // Create a task
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":"Tag test task"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let id = created["id"].as_i64().unwrap();

        // Add a tag
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/tasks/{}/tags", id))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"tag":"urgent"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Get task to see tags
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/tasks/{}", id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let fetched: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let tags = fetched["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0]["name"], "urgent");
        let tag_id = tags[0]["id"].as_i64().unwrap();

        // Remove the tag
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/tasks/{}/tags/{}", id, tag_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_create_dispatch() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let app = Router::new()
            .route("/tasks", post(create_task_handler))
            .route("/tasks/{id}/dispatch", post(create_dispatch_handler))
            .with_state(state);

        // Create a task
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":"Dispatch test"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let id = created["id"].as_i64().unwrap();

        // Create dispatch
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/tasks/{}/dispatch", id))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"instance_id":"inst-1","sent_text":"do the thing"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["ok"].as_bool().unwrap());
        assert!(json["dispatch_id"].as_i64().is_some());
        // Task should transition from pending to in_progress
        assert_eq!(json["status"], "in_progress");
    }

    #[tokio::test]
    async fn test_create_dispatch_task_not_found() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let app = Router::new()
            .route("/tasks/{id}/dispatch", post(create_dispatch_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/tasks/99999/dispatch")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"instance_id":"inst-1","sent_text":"test"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_create_task_with_tags_returns_tags() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let app = Router::new()
            .route("/tasks", post(create_task_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"title":"Tagged on create","tags":["bug","urgent"]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["title"], "Tagged on create");
        let tags = json["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 2);
        let tag_names: Vec<&str> = tags.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(tag_names.contains(&"bug"));
        assert!(tag_names.contains(&"urgent"));
    }

    #[tokio::test]
    async fn test_create_dispatch_preserves_non_pending_status() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let app = Router::new()
            .route("/tasks", post(create_task_handler))
            .route("/tasks/{id}", patch(update_task_handler))
            .route("/tasks/{id}/dispatch", post(create_dispatch_handler))
            .with_state(state);

        // Create a task
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":"Already running"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let id = created["id"].as_i64().unwrap();

        // Move to in_progress first
        app.clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/tasks/{}", id))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"status":"in_progress"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Create dispatch on an already in_progress task
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/tasks/{}/dispatch", id))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"instance_id":"inst-1","sent_text":"more work"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // Status should remain in_progress, not be overwritten
        assert_eq!(json["status"], "in_progress");
    }

    #[tokio::test]
    async fn test_get_task_includes_tags_and_dispatches() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let app = Router::new()
            .route("/tasks", post(create_task_handler))
            .route("/tasks/{id}", get(get_task_handler))
            .route("/tasks/{id}/tags", post(add_task_tag_handler))
            .route("/tasks/{id}/dispatch", post(create_dispatch_handler))
            .with_state(state);

        // Create
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":"Full task"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let id = created["id"].as_i64().unwrap();

        // Add tag
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/tasks/{}/tags", id))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"tag":"feature"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Add dispatch
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/tasks/{}/dispatch", id))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"instance_id":"inst-1","sent_text":"do it"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Get should include both
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/tasks/{}", id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["tags"].as_array().unwrap().len(), 1);
        assert_eq!(json["tags"][0]["name"], "feature");
        assert_eq!(json["dispatches"].as_array().unwrap().len(), 1);
        assert_eq!(json["dispatches"][0]["sent_text"], "do it");
    }

    #[tokio::test]
    async fn test_list_tasks_with_filters() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let app = Router::new()
            .route("/tasks", get(list_tasks_handler).post(create_task_handler))
            .with_state(state);

        // Create a task with status
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"title":"Filtered task","status":"in_progress"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Filter by status
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/tasks?status=in_progress")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let tasks: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(!tasks.as_array().unwrap().is_empty());
    }
}
