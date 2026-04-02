use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use toolpath_claude::ClaudeConvo;
use toolpath_convo::ConversationProvider;
use tracing::{debug, info};

use super::format::{format_progress_event, format_turn_with_attribution};
use crate::AppState;

pub async fn get_conversation(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let instance = match state.instance_manager.get(&id).await {
        Some(inst) => inst,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    if !instance.kind.is_structured() {
        return Json(serde_json::json!({
            "error": "Not a structured instance",
            "turns": []
        }))
        .into_response();
    }

    // Session discovery is handled by the server-owned conversation watcher.
    // The HTTP handler only reads from an already-discovered session.
    let session_id = match &instance.session_id {
        Some(sid) => sid.clone(),
        None => {
            debug!("No session discovered yet for instance {}", id);
            return Json(serde_json::json!({ "turns": [] })).into_response();
        }
    };

    let convo_manager = ClaudeConvo::new();
    let working_dir = &instance.working_dir;

    match ConversationProvider::load_conversation(&convo_manager, working_dir, &session_id) {
        Ok(view) => {
            let mut turns = Vec::with_capacity(view.turns.len());
            for turn in &view.turns {
                turns.push(
                    format_turn_with_attribution(
                        turn,
                        &id,
                        Some(&state.repository),
                        Some(state.global_state_manager.pending_attributions_lock()),
                    )
                    .await,
                );
            }

            info!(
                "Found conversation {} with {} turns",
                session_id,
                turns.len()
            );
            Json(serde_json::json!({
                "conversation_id": view.id,
                "turns": turns,
                "files_changed": view.files_changed,
                "total_usage": view.total_usage,
                "provider_id": view.provider_id,
            }))
            .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to read conversation {}: {}", session_id, e);
            Json(serde_json::json!({
                "error": format!("Failed to read conversation: {}", e),
                "turns": []
            }))
            .into_response()
        }
    }
}

/// Poll for new conversation entries (uses watcher to return only unseen entries)
pub async fn poll_conversation(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let instance = match state.instance_manager.get(&id).await {
        Some(inst) => inst,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    if !instance.kind.is_structured() {
        return Json(serde_json::json!({
            "error": "Not a structured instance",
            "new_turns": []
        }))
        .into_response();
    }

    let session_id = match &instance.session_id {
        Some(sid) => sid.clone(),
        None => {
            return Json(serde_json::json!({
                "new_turns": [],
                "waiting": true
            }))
            .into_response();
        }
    };

    let mut watchers = state.conversation_watchers.lock().await;
    let watcher = watchers.entry(id.clone()).or_insert_with(|| {
        Box::new(crate::ws::merging_watcher::MergingWatcher::new(
            ClaudeConvo::new(),
            instance.working_dir.clone(),
            session_id.clone(),
        )) as Box<dyn toolpath_convo::ConversationWatcher + Send>
    });

    match toolpath_convo::ConversationWatcher::poll(watcher.as_mut()) {
        Ok(events) => {
            let mut turns = Vec::new();
            for event in &events {
                match event {
                    toolpath_convo::WatcherEvent::Turn(turn) => {
                        turns.push(
                            format_turn_with_attribution(
                                turn,
                                &id,
                                Some(&state.repository),
                                Some(state.global_state_manager.pending_attributions_lock()),
                            )
                            .await,
                        );
                    }
                    toolpath_convo::WatcherEvent::TurnUpdated(turn) => {
                        turns.push(
                            format_turn_with_attribution(
                                turn,
                                &id,
                                Some(&state.repository),
                                Some(state.global_state_manager.pending_attributions_lock()),
                            )
                            .await,
                        );
                    }
                    toolpath_convo::WatcherEvent::Progress { kind, data } => {
                        turns.push(format_progress_event(kind, data));
                    }
                }
            }

            Json(serde_json::json!({
                "new_turns": turns,
                "total_seen": toolpath_convo::ConversationWatcher::seen_count(watcher.as_ref())
            }))
            .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to poll conversation: {}", e);
            Json(serde_json::json!({
                "error": format!("Failed to poll: {}", e),
                "new_turns": []
            }))
            .into_response()
        }
    }
}
