use std::collections::HashMap;

use axum::{Json, extract::State, http::StatusCode};

use crate::AppState;
use crate::auth::MaybeAuthUser;
use crate::ws;

/// GET /api/user/settings — returns all settings for the authenticated user
pub async fn get_user_settings_handler(
    State(state): State<AppState>,
    maybe_user: MaybeAuthUser,
) -> Result<Json<HashMap<String, String>>, (StatusCode, String)> {
    let user_id = match &maybe_user {
        MaybeAuthUser(Some(user)) => user.user_id.clone(),
        _ => return Ok(Json(HashMap::new())),
    };

    let settings = state
        .repository
        .get_user_settings(&user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(settings))
}

/// PATCH /api/user/settings — merge-updates settings, broadcasts, returns full snapshot
pub async fn update_user_settings_handler(
    State(state): State<AppState>,
    maybe_user: MaybeAuthUser,
    Json(updates): Json<HashMap<String, String>>,
) -> Result<Json<HashMap<String, String>>, (StatusCode, String)> {
    let user_id = match &maybe_user {
        MaybeAuthUser(Some(user)) => user.user_id.clone(),
        _ => {
            return Err((
                StatusCode::UNAUTHORIZED,
                "Authentication required to update settings".to_string(),
            ));
        }
    };

    state
        .repository
        .set_user_settings(&user_id, &updates)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let settings = state
        .repository
        .get_user_settings(&user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Broadcast to all clients — frontend handles idempotent merge
    state
        .global_state_manager
        .broadcast_lifecycle(ws::ServerMessage::UserSettingsUpdate {
            user_id: user_id.clone(),
            settings: serde_json::to_value(&settings).unwrap_or_default(),
        });

    Ok(Json(settings))
}
