use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;

use crate::AppState;
use crate::repository;

#[derive(Deserialize)]
pub struct PaginationParams {
    page: Option<i64>,
    per_page: Option<i64>,
}

pub async fn list_conversations(
    State(state): State<AppState>,
    Query(params): Query<PaginationParams>,
) -> Result<impl IntoResponse, StatusCode> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 100);

    match state
        .repository
        .list_conversations_paginated(page, per_page)
        .await
    {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            tracing::error!("Failed to list conversations: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Deserialize)]
pub struct SearchParams {
    q: String,
    page: Option<i64>,
    per_page: Option<i64>,
    /// Filter by message role (user, assistant)
    role: Option<String>,
    /// Filter entries after this Unix timestamp
    date_from: Option<i64>,
    /// Filter entries before this Unix timestamp
    date_to: Option<i64>,
    /// Only return conversations containing tool use
    has_tools: Option<bool>,
}

pub async fn search_conversations_handler(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<impl IntoResponse, StatusCode> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 100);

    let filters = repository::SearchFilters {
        role: params.role,
        date_from: params.date_from,
        date_to: params.date_to,
        has_tools: params.has_tools,
    };

    match state
        .repository
        .search_conversations(&params.q, page, per_page, 3, &filters)
        .await
    {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            tracing::error!("Failed to search conversations: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn get_conversation_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    match state.repository.get_conversation_with_entries(&id).await {
        Ok(Some(conversation)) => Ok(Json(conversation)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::error!("Failed to get conversation {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Deserialize)]
pub struct CreateCommentRequest {
    author: Option<String>,
    content: String,
    entry_uuid: Option<String>,
}

pub async fn add_comment(
    State(state): State<AppState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<CreateCommentRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let comment =
        crate::models::Comment::new(conversation_id, req.content, req.author, req.entry_uuid);

    match state.repository.add_comment(&comment).await {
        Ok(id) => Ok(Json(serde_json::json!({ "id": id }))),
        Err(e) => {
            tracing::error!("Failed to add comment: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn get_comments(
    State(state): State<AppState>,
    Path(conversation_id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    match state
        .repository
        .get_conversation_comments(&conversation_id)
        .await
    {
        Ok(comments) => Ok(Json(comments)),
        Err(e) => {
            tracing::error!("Failed to get comments: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Deserialize)]
pub struct CreateShareRequest {
    expires_in_days: Option<i32>,
    title: Option<String>,
    description: Option<String>,
}

pub async fn create_share(
    State(state): State<AppState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<CreateShareRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut share = crate::models::ConversationShare::new(conversation_id, req.expires_in_days);

    if let Some(title) = req.title {
        share.title = Some(title);
    }
    if let Some(desc) = req.description {
        share.description = Some(desc);
    }

    match state.repository.create_share(&share).await {
        Ok(token) => Ok(Json(serde_json::json!({
            "share_token": token,
            "url": format!("/api/share/{}", token)
        }))),
        Err(e) => {
            tracing::error!("Failed to create share: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn get_shared_conversation(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let share = match state.repository.get_share(&token).await {
        Ok(Some(s)) => s,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::error!("Failed to get share: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    if share.is_expired() {
        return Err(StatusCode::GONE);
    }

    if share.is_access_limit_reached() {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    if let Err(e) = state.repository.increment_share_access(&token).await {
        tracing::warn!("Failed to increment share access: {}", e);
    }

    match state
        .repository
        .get_conversation_with_entries(&share.conversation_id)
        .await
    {
        Ok(Some(conversation)) => Ok(Json(conversation)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::error!("Failed to get shared conversation: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
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
        routing::{get, post},
    };
    use tower::ServiceExt;

    async fn test_router() -> (Router, tempfile::TempDir) {
        let (state, tmp) = crate::test_helpers::test_app_state().await;
        let router = Router::new()
            .route("/conversations", get(list_conversations))
            .route("/conversations/search", get(search_conversations_handler))
            .route("/conversations/{id}", get(get_conversation_by_id))
            .route(
                "/conversations/{id}/comments",
                post(add_comment).get(get_comments),
            )
            .route("/conversations/{id}/share", post(create_share))
            .route("/share/{token}", get(get_shared_conversation))
            .with_state(state);
        (router, tmp)
    }

    #[tokio::test]
    async fn test_list_conversations_empty() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/conversations")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_list_conversations_pagination() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/conversations?page=1&per_page=10")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_conversation_not_found() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/conversations/nonexistent-id")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_search_conversations() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/conversations/search?q=test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_search_with_filters() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/conversations/search?q=test&role=user&date_from=0&date_to=9999999999")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_comments_empty() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/conversations/some-id/comments")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_shared_conversation_not_found() {
        let (app, _tmp) = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/share/invalid-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_add_comment() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        // Insert a conversation first
        let conv = crate::models::Conversation::new("conv-1".into(), "inst-1".into());
        state.repository.create_conversation(&conv).await.unwrap();

        let app = Router::new()
            .route(
                "/conversations/{id}/comments",
                post(add_comment).get(get_comments),
            )
            .with_state(state);

        // Add comment
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/conversations/conv-1/comments")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"content":"Great insight!","author":"Alice"}"#,
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
        assert!(json["id"].as_i64().is_some());

        // Verify via get_comments
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/conversations/conv-1/comments")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let comments: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(comments.as_array().unwrap().len(), 1);
        assert_eq!(comments[0]["content"], "Great insight!");
    }

    #[tokio::test]
    async fn test_get_conversation_by_id_success() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let mut conv = crate::models::Conversation::new("conv-1".into(), "inst-1".into());
        conv.title = Some("Test Conversation".into());
        state.repository.create_conversation(&conv).await.unwrap();

        let app = Router::new()
            .route("/conversations/{id}", get(get_conversation_by_id))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/conversations/conv-1")
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
        assert_eq!(json["conversation"]["id"], "conv-1");
        assert_eq!(json["conversation"]["title"], "Test Conversation");
    }

    #[tokio::test]
    async fn test_create_share() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let conv = crate::models::Conversation::new("conv-1".into(), "inst-1".into());
        state.repository.create_conversation(&conv).await.unwrap();

        let app = Router::new()
            .route("/conversations/{id}/share", post(create_share))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/conversations/conv-1/share")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"expires_in_days":7,"title":"My Share","description":"A shared convo"}"#,
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
        assert!(json["share_token"].as_str().is_some());
        assert!(json["url"].as_str().unwrap().contains("/api/share/"));
    }

    #[tokio::test]
    async fn test_get_shared_conversation_success() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;

        // Create a conversation with entries
        let conv = crate::models::Conversation::new("conv-1".into(), "inst-1".into());
        state.repository.create_conversation(&conv).await.unwrap();

        // Create a share
        let share = crate::models::ConversationShare::new("conv-1".into(), Some(7));
        let token = state.repository.create_share(&share).await.unwrap();

        let app = Router::new()
            .route("/share/{token}", get(get_shared_conversation))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/share/{}", token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_add_comment_with_entry_uuid() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let conv = crate::models::Conversation::new("conv-2".into(), "inst-1".into());
        state.repository.create_conversation(&conv).await.unwrap();

        let app = Router::new()
            .route("/conversations/{id}/comments", post(add_comment))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/conversations/conv-2/comments")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"content":"Note on entry","entry_uuid":"entry-uuid-123"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_create_share_defaults() {
        let (state, _tmp) = crate::test_helpers::test_app_state().await;
        let conv = crate::models::Conversation::new("conv-3".into(), "inst-1".into());
        state.repository.create_conversation(&conv).await.unwrap();

        let app = Router::new()
            .route("/conversations/{id}/share", post(create_share))
            .with_state(state);

        // No optional fields
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/conversations/conv-3/share")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
