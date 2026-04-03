use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};

use crate::AppState;
use crate::models::InboxItem;
use crate::ws::ServerMessage;

/// GET /api/inbox — list all active inbox items
pub async fn list_inbox_handler(
    State(state): State<AppState>,
) -> Result<Json<Vec<InboxItem>>, StatusCode> {
    match state.repository.list_inbox().await {
        Ok(items) => Ok(Json(items)),
        Err(e) => {
            tracing::error!("Failed to list inbox: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// POST /api/inbox/{instance_id}/dismiss — clear an inbox item
pub async fn dismiss_inbox_handler(
    State(state): State<AppState>,
    Path(instance_id): Path<String>,
) -> impl IntoResponse {
    match state.repository.dismiss_inbox_item(&instance_id).await {
        Ok(true) => {
            // Broadcast the dismissal to all clients
            state
                .global_state_manager
                .broadcast_lifecycle(ServerMessage::InboxUpdate {
                    instance_id,
                    item: None,
                });
            StatusCode::NO_CONTENT
        }
        Ok(false) => StatusCode::NOT_FOUND,
        Err(e) => {
            tracing::error!("Failed to dismiss inbox item: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}
