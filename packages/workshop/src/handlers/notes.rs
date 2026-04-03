use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use tracing::error;

use crate::AppState;

#[derive(Deserialize)]
pub struct CreateNoteRequest {
    content: String,
    entry_id: Option<String>,
}

pub async fn create_note(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(req): Json<CreateNoteRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    match state
        .notes_storage
        .add_note(&session_id, req.content, req.entry_id)
        .await
    {
        Ok(note) => Ok(Json(note)),
        Err(e) => {
            error!("Failed to create note: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn get_notes(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let notes = state.notes_storage.get_notes(&session_id).await;
    Json(notes)
}

#[derive(Deserialize)]
pub struct UpdateNoteRequest {
    content: String,
}

pub async fn update_note(
    State(state): State<AppState>,
    Path((session_id, note_id)): Path<(String, String)>,
    Json(req): Json<UpdateNoteRequest>,
) -> StatusCode {
    match state
        .notes_storage
        .update_note(&session_id, &note_id, req.content)
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(e) => {
            error!("Failed to update note: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

pub async fn delete_note(
    State(state): State<AppState>,
    Path((session_id, note_id)): Path<(String, String)>,
) -> StatusCode {
    match state.notes_storage.delete_note(&session_id, &note_id).await {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(e) => {
            error!("Failed to delete note: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        http::Request,
        routing::{delete, get, post},
    };
    use tower::ServiceExt;

    async fn test_router() -> (Router, tempfile::TempDir) {
        let (state, tmp) = crate::test_helpers::test_app_state().await;
        let router = Router::new()
            .route("/notes/{session_id}", get(get_notes).post(create_note))
            .route(
                "/notes/{session_id}/{note_id}",
                post(update_note).delete(delete_note),
            )
            .with_state(state);
        (router, tmp)
    }

    #[tokio::test]
    async fn test_get_notes_empty() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/notes/session1")
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
    async fn test_create_and_get_note() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let app = Router::new()
            .route("/notes/{session_id}", get(get_notes).post(create_note))
            .with_state(state);

        // Create
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/notes/s1")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"content":"My note"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(created["content"], "My note");

        // Get
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/notes/s1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let notes: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(notes.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_delete_note_handler() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let app = Router::new()
            .route("/notes/{session_id}", post(create_note))
            .route("/notes/{session_id}/{note_id}", delete(delete_note))
            .with_state(state);

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/notes/s1")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"content":"Delete me"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let note_id = created["id"].as_str().unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/notes/s1/{}", note_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_update_note() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let app = Router::new()
            .route("/notes/{session_id}", get(get_notes).post(create_note))
            .route(
                "/notes/{session_id}/{note_id}",
                post(update_note).delete(delete_note),
            )
            .with_state(state);

        // Create
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/notes/s1")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"content":"Original text"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let note_id = created["id"].as_str().unwrap();

        // Update
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/notes/s1/{}", note_id))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"content":"Updated text"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Verify
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/notes/s1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let notes: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(notes[0]["content"], "Updated text");
    }

    #[tokio::test]
    async fn test_create_note_with_entry_id() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/notes/session1")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"content":"Note on entry","entry_id":"entry-uuid-123"}"#,
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
        assert_eq!(json["content"], "Note on entry");
    }
}
