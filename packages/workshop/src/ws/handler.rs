//! WebSocket Handler
//!
//! Main multiplexed WebSocket connection handler.

use axum::extract::ws::{Message, WebSocket};
use futures::{sink::SinkExt, stream::StreamExt};
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::config::ServerConfig;
use crate::instance_manager::InstanceManager;
use crate::metrics::ServerMetrics;
use crate::repository::ConversationRepository;

use crate::virtual_terminal::ClientType;

use super::focus::{handle_focus, send_conversation_since, send_output_history};
use super::protocol::{BackpressureStats, ClientMessage, PresenceUser, ServerMessage, WsUser};
use super::state_manager::{
    GlobalStateManager, InputContext, InputUser, TERMINAL_LOCK_TIMEOUT_SECS,
};

/// Handle a multiplexed WebSocket connection
pub async fn handle_multiplexed_ws(
    socket: WebSocket,
    instance_manager: Arc<InstanceManager>,
    state_manager: Arc<GlobalStateManager>,
    _server_config: Option<Arc<ServerConfig>>,
    server_metrics: Option<Arc<ServerMetrics>>,
    ws_user: Option<WsUser>,
    repository: Option<Arc<ConversationRepository>>,
) {
    info!(
        "New multiplexed WebSocket connection (user: {})",
        ws_user
            .as_ref()
            .map(|u| u.display_name.as_str())
            .unwrap_or("anonymous")
    );

    // Track connection opened
    if let Some(ref m) = server_metrics {
        m.connection_opened();
    }

    // Per-connection backpressure stats
    let stats = Arc::new(BackpressureStats::new());

    // Unique ID for this connection (for presence tracking)
    let connection_id = uuid::Uuid::new_v4().to_string();

    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Channel for sending messages to the WebSocket
    let (tx, mut rx) = mpsc::channel::<ServerMessage>(100);

    // Current focused instance and its cancellation token
    let focused_instance: Arc<RwLock<Option<String>>> = Arc::new(RwLock::new(None));
    let focus_cancel: Arc<RwLock<Option<CancellationToken>>> = Arc::new(RwLock::new(None));
    // Channel for the TerminalVisible handler to ask the focus task to re-send
    // OutputHistory with the correct client_rows (after draining stale broadcasts).
    let focus_refresh: Arc<RwLock<Option<mpsc::Sender<u16>>>> = Arc::new(RwLock::new(None));

    // Channel for session selection (when ambiguous)
    let (session_select_tx, session_select_rx) = mpsc::channel::<String>(1);
    let session_select_rx = Arc::new(tokio::sync::Mutex::new(session_select_rx));

    // Send initial instance list with states (enriched with state_entered_at)
    let mut instances = instance_manager.list().await;
    for inst in &mut instances {
        if let Some(ts) = state_manager.get_state_entered_at(&inst.id).await {
            inst.state_entered_at = Some(ts.timestamp());
        }
    }
    if tx
        .send(ServerMessage::InstanceList { instances })
        .await
        .is_err()
    {
        warn!(conn_id = %connection_id, "Failed to send initial instance list - channel closed");
    }

    // Send initial inbox state
    if let Some(ref repo) = repository {
        match repo.list_inbox().await {
            Ok(items) if !items.is_empty() => {
                if tx.send(ServerMessage::InboxList { items }).await.is_err() {
                    warn!(conn_id = %connection_id, "Failed to send initial inbox list - channel closed");
                }
            }
            Err(e) => {
                warn!(conn_id = %connection_id, "Failed to load inbox: {}", e);
            }
            _ => {} // Empty inbox, nothing to send
        }
    }

    // Subscribe to state broadcasts from all instances
    let mut state_rx = state_manager.subscribe();
    let tx_state = tx.clone();
    let stats_state = stats.clone();
    let state_manager_for_broadcast = state_manager.clone();
    let state_broadcast_task = async move {
        loop {
            match state_rx.recv().await {
                Ok((instance_id, state, stale)) => {
                    stats_state.record_state_send(1); // At least 1 receiver (us)
                    let entered_at = state_manager_for_broadcast
                        .get_state_entered_at(&instance_id)
                        .await
                        .map(|ts| ts.timestamp());
                    if tx_state
                        .send(ServerMessage::StateChange {
                            instance_id,
                            state,
                            stale,
                            entered_at,
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    stats_state.record_lag(n);
                    warn!("State broadcast lagged by {} messages", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    };

    // Subscribe to instance lifecycle broadcasts (created/stopped)
    let mut lifecycle_rx = state_manager.subscribe_lifecycle();
    let tx_lifecycle = tx.clone();
    let lifecycle_task = async move {
        loop {
            match lifecycle_rx.recv().await {
                Ok(msg) => {
                    if tx_lifecycle.send(msg).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    // Task to send messages to WebSocket
    let sender_task = async move {
        while let Some(msg) = rx.recv().await {
            let json = match serde_json::to_string(&msg) {
                Ok(j) => j,
                Err(e) => {
                    error!("Failed to serialize message: {}", e);
                    continue;
                }
            };
            if ws_sender.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    };

    // Task to handle incoming messages
    let tx_input = tx.clone();
    let focused_clone: Arc<RwLock<Option<String>>> = focused_instance.clone();
    let focus_cancel_clone: Arc<RwLock<Option<CancellationToken>>> = focus_cancel.clone();
    let focus_refresh_clone = focus_refresh.clone();
    let state_mgr = state_manager.clone();
    let inst_mgr = instance_manager.clone();
    let session_tx = session_select_tx.clone();
    let session_rx_clone = session_select_rx.clone();
    let ws_user_clone = ws_user.clone();
    let connection_id_clone = connection_id.clone();
    let repository_clone = repository.clone();

    let input_task = async move {
        while let Some(msg) = ws_receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                        match client_msg {
                            ClientMessage::Focus {
                                instance_id,
                                since_uuid,
                            } => {
                                // Cancel previous focus tasks
                                {
                                    let mut guard = focus_cancel_clone.write().await;
                                    if let Some(cancel) = guard.take() {
                                        cancel.cancel();
                                    }
                                }

                                // Create new cancellation token
                                let cancel_token = CancellationToken::new();
                                {
                                    let mut guard = focus_cancel_clone.write().await;
                                    *guard = Some(cancel_token.clone());
                                }
                                // Remove presence from previous instance, add to new one
                                let prev_instance = {
                                    let mut guard = focused_clone.write().await;
                                    let prev = guard.take();
                                    *guard = Some(instance_id.clone());
                                    prev
                                };

                                if let Some(ref user) = ws_user_clone {
                                    // Remove from previous instance
                                    if let Some(ref prev_id) = prev_instance {
                                        let users = state_mgr
                                            .remove_presence_from_instance(
                                                prev_id,
                                                &connection_id_clone,
                                            )
                                            .await;
                                        state_mgr.broadcast_lifecycle(
                                            ServerMessage::PresenceUpdate {
                                                instance_id: prev_id.clone(),
                                                users,
                                            },
                                        );
                                    }
                                    // Add to new instance
                                    let users = state_mgr
                                        .add_presence(&instance_id, &connection_id_clone, user)
                                        .await;
                                    state_mgr.broadcast_lifecycle(ServerMessage::PresenceUpdate {
                                        instance_id: instance_id.clone(),
                                        users,
                                    });

                                    // Reconcile terminal lock (auto-grant to sole user, etc.)
                                    state_mgr
                                        .reconcile_terminal_lock_with_presence(&instance_id)
                                        .await;
                                    // Also reconcile previous instance (user left, sole remaining user may get auto-grant)
                                    if let Some(ref prev_id) = prev_instance {
                                        state_mgr
                                            .reconcile_terminal_lock_with_presence(prev_id)
                                            .await;
                                        broadcast_terminal_lock_update(&state_mgr, prev_id).await;
                                    }
                                    // Send current lock state to the newly focused client
                                    let lock_msg = build_lock_update_message(
                                        &instance_id,
                                        state_mgr.get_terminal_lock(&instance_id).await,
                                    );
                                    let _ = tx_input.send(lock_msg).await;
                                    // Also broadcast to all clients so everyone sees updated lock state
                                    broadcast_terminal_lock_update(&state_mgr, &instance_id).await;
                                }

                                // Start focus handling in background.
                                // The refresh channel lets the TerminalVisible handler
                                // ask the focus task to re-send OutputHistory (with drain)
                                // instead of sending it directly (which causes overlap).
                                let tx_focus = tx_input.clone();
                                let state_mgr_focus = state_mgr.clone();
                                let inst_mgr_focus = inst_mgr.clone();
                                let session_rx = session_rx_clone.clone();
                                let (refresh_tx, refresh_rx) = mpsc::channel::<u16>(4);
                                *focus_refresh_clone.write().await = Some(refresh_tx);

                                tokio::spawn(async move {
                                    handle_focus(
                                        instance_id,
                                        since_uuid,
                                        cancel_token,
                                        state_mgr_focus,
                                        inst_mgr_focus,
                                        tx_focus,
                                        session_rx,
                                        refresh_rx,
                                    )
                                    .await;
                                });
                            }
                            ClientMessage::ConversationSync { since_uuid } => {
                                // Sync conversation without changing focus
                                // This is used when a tab becomes visible again
                                if let Some(id) = focused_clone.read().await.clone()
                                    && let Some(h) = state_mgr.get_handle(&id).await
                                {
                                    let tx_sync = tx_input.clone();
                                    tokio::spawn(async move {
                                        if let Err(e) = send_conversation_since(
                                            &id,
                                            since_uuid.as_deref(),
                                            &h,
                                            &tx_sync,
                                        )
                                        .await
                                        {
                                            error!("Failed to sync conversation: {}", e);
                                        }
                                    });
                                }
                            }
                            ClientMessage::Input {
                                instance_id,
                                data,
                                task_id,
                            } => {
                                let ctx = InputContext {
                                    instance_id: instance_id.clone(),
                                    data,
                                    connection_id: connection_id_clone.clone(),
                                    user: ws_user_clone.as_ref().map(|u| InputUser {
                                        user_id: u.user_id.clone(),
                                        display_name: u.display_name.clone(),
                                    }),
                                    task_id,
                                };
                                if let Err(e) =
                                    state_mgr.handle_input(ctx, repository_clone.as_ref()).await
                                {
                                    error!(instance = %instance_id, "Failed to write to PTY: {}", e);
                                    let _ = tx_input
                                        .send(ServerMessage::Error {
                                            instance_id: Some(instance_id),
                                            message: format!("Failed to send input: {}", e),
                                        })
                                        .await;
                                }
                            }
                            ClientMessage::Resize {
                                instance_id,
                                rows,
                                cols,
                                client_type,
                            } => {
                                let ct = match client_type.as_deref() {
                                    Some("terminal") => ClientType::Terminal,
                                    _ => ClientType::Web,
                                };
                                if let Some(handle) = state_mgr.get_handle(&instance_id).await
                                    && let Err(e) = handle
                                        .update_viewport_and_resize(
                                            &connection_id_clone,
                                            rows,
                                            cols,
                                            ct,
                                        )
                                        .await
                                {
                                    warn!("Failed to resize PTY for {}: {}", instance_id, e);
                                }
                            }
                            ClientMessage::TerminalVisible {
                                instance_id,
                                rows,
                                cols,
                                client_type,
                            } => {
                                let ct = match client_type.as_deref() {
                                    Some("terminal") => ClientType::Terminal,
                                    _ => ClientType::Web,
                                };
                                debug!(
                                    conn = %connection_id_clone,
                                    instance = %instance_id,
                                    rows,
                                    cols,
                                    "TerminalVisible"
                                );
                                if let Some(handle) = state_mgr.get_handle(&instance_id).await {
                                    if let Err(e) = handle
                                        .update_viewport_and_resize(
                                            &connection_id_clone,
                                            rows,
                                            cols,
                                            ct,
                                        )
                                        .await
                                    {
                                        warn!("Failed to resize PTY for {}: {}", instance_id, e);
                                    }

                                    // If this instance has an active focus task, ask
                                    // it to re-send OutputHistory (it drains stale
                                    // broadcasts first, preventing overlap duplication).
                                    // Otherwise send OutputHistory directly — covers
                                    // non-focused pane-bound terminals after page reload
                                    // and split-opened instances.
                                    let is_focused = focused_clone.read().await.as_deref()
                                        == Some(instance_id.as_str());
                                    let sent_via_focus = if is_focused {
                                        if let Some(ref refresh_tx) =
                                            *focus_refresh_clone.read().await
                                        {
                                            refresh_tx.send(rows).await.is_ok()
                                        } else {
                                            false
                                        }
                                    } else {
                                        false
                                    };
                                    if !sent_via_focus {
                                        let _ = send_output_history(
                                            &handle,
                                            &instance_id,
                                            usize::MAX,
                                            rows,
                                            &tx_input,
                                        )
                                        .await;
                                    }
                                }
                            }
                            ClientMessage::TerminalHidden { instance_id } => {
                                debug!(
                                    conn = %connection_id_clone,
                                    instance = %instance_id,
                                    "TerminalHidden"
                                );
                                if let Some(handle) = state_mgr.get_handle(&instance_id).await
                                    && let Err(e) = handle
                                        .set_active_and_resize(&connection_id_clone, false)
                                        .await
                                {
                                    warn!("Failed to resize PTY for {}: {}", instance_id, e);
                                }
                            }
                            ClientMessage::SessionSelect { session_id } => {
                                debug!("Session selected: {}", session_id);
                                if session_tx.send(session_id.clone()).await.is_err() {
                                    warn!(session = %session_id, "Failed to send session selection - receiver dropped");
                                }
                            }
                            ClientMessage::Lobby { channel, payload } => {
                                state_mgr.broadcast_lifecycle(ServerMessage::LobbyBroadcast {
                                    sender_id: connection_id_clone.clone(),
                                    channel,
                                    payload,
                                });
                            }
                            ClientMessage::TerminalLockRequest { instance_id } => {
                                if let Some(ref user) = ws_user_clone {
                                    let acquired = state_mgr
                                        .try_acquire_terminal_lock(
                                            &instance_id,
                                            &connection_id_clone,
                                            user,
                                        )
                                        .await;
                                    if acquired {
                                        broadcast_terminal_lock_update(&state_mgr, &instance_id)
                                            .await;
                                    } else {
                                        // Send current state back so the client knows who holds it
                                        let msg = build_lock_update_message(
                                            &instance_id,
                                            state_mgr.get_terminal_lock(&instance_id).await,
                                        );
                                        let _ = tx_input.send(msg).await;
                                    }
                                }
                            }
                            ClientMessage::TerminalLockRelease { instance_id } => {
                                let released = state_mgr
                                    .release_terminal_lock(&instance_id, &connection_id_clone)
                                    .await;
                                if released {
                                    // Reconcile: may auto-grant to remaining sole user
                                    state_mgr
                                        .reconcile_terminal_lock_with_presence(&instance_id)
                                        .await;
                                    broadcast_terminal_lock_update(&state_mgr, &instance_id).await;
                                }
                            }
                            ClientMessage::ChatSend {
                                scope,
                                content,
                                uuid,
                                topic,
                            } => {
                                if let (Some(user), Some(repo)) =
                                    (&ws_user_clone, &repository_clone)
                                {
                                    let msg = crate::models::ChatMessage {
                                        id: None,
                                        uuid: uuid.clone(),
                                        scope: scope.clone(),
                                        user_id: user.user_id.clone(),
                                        display_name: user.display_name.clone(),
                                        content: content.clone(),
                                        created_at: chrono::Utc::now().timestamp(),
                                        forwarded_from: None,
                                        topic: topic.clone(),
                                    };
                                    let repo = repo.clone();
                                    let state_mgr_chat = state_mgr.clone();
                                    tokio::spawn(async move {
                                        match repo.insert_chat_message(&msg).await {
                                            Ok(id) => {
                                                state_mgr_chat.broadcast_lifecycle(
                                                    ServerMessage::ChatMessage {
                                                        id,
                                                        uuid: msg.uuid,
                                                        scope: msg.scope,
                                                        user_id: msg.user_id,
                                                        display_name: msg.display_name,
                                                        content: msg.content,
                                                        created_at: msg.created_at,
                                                        forwarded_from: None,
                                                        topic: msg.topic,
                                                    },
                                                );
                                            }
                                            Err(e) => {
                                                warn!("Failed to insert chat message: {}", e);
                                            }
                                        }
                                    });
                                }
                            }
                            ClientMessage::ChatHistory {
                                scope,
                                before_id,
                                limit,
                                topic,
                            } => {
                                if let Some(ref repo) = repository_clone {
                                    let repo = repo.clone();
                                    let tx_chat = tx_input.clone();
                                    let scope = scope.clone();
                                    let limit = limit.unwrap_or(50).min(100);
                                    tokio::spawn(async move {
                                        match repo
                                            .get_chat_history(
                                                &scope,
                                                before_id,
                                                limit,
                                                topic.as_deref(),
                                            )
                                            .await
                                        {
                                            Ok((messages, has_more)) => {
                                                let msgs: Vec<serde_json::Value> = messages
                                                    .into_iter()
                                                    .filter_map(|m| serde_json::to_value(&m).ok())
                                                    .collect();
                                                let _ = tx_chat
                                                    .send(ServerMessage::ChatHistoryResponse {
                                                        scope,
                                                        messages: msgs,
                                                        has_more,
                                                    })
                                                    .await;
                                            }
                                            Err(e) => {
                                                warn!("Failed to get chat history: {}", e);
                                            }
                                        }
                                    });
                                }
                            }
                            ClientMessage::ChatForward {
                                message_id,
                                target_scope,
                            } => {
                                if let (Some(user), Some(repo)) =
                                    (&ws_user_clone, &repository_clone)
                                {
                                    let repo = repo.clone();
                                    let state_mgr_chat = state_mgr.clone();
                                    let user = user.clone();
                                    tokio::spawn(async move {
                                        if let Ok(Some(original)) =
                                            repo.get_chat_message_by_id(message_id).await
                                        {
                                            let fwd = crate::models::ChatMessage {
                                                id: None,
                                                uuid: uuid::Uuid::new_v4().to_string(),
                                                scope: target_scope.clone(),
                                                user_id: user.user_id.clone(),
                                                display_name: original.display_name.clone(),
                                                content: original.content.clone(),
                                                created_at: chrono::Utc::now().timestamp(),
                                                forwarded_from: Some(original.scope.clone()),
                                                topic: original.topic.clone(),
                                            };
                                            if let Ok(id) = repo.insert_chat_message(&fwd).await {
                                                state_mgr_chat.broadcast_lifecycle(
                                                    ServerMessage::ChatMessage {
                                                        id,
                                                        uuid: fwd.uuid,
                                                        scope: fwd.scope,
                                                        user_id: fwd.user_id,
                                                        display_name: fwd.display_name,
                                                        content: fwd.content,
                                                        created_at: fwd.created_at,
                                                        forwarded_from: fwd.forwarded_from,
                                                        topic: fwd.topic,
                                                    },
                                                );
                                            }
                                        }
                                    });
                                }
                            }
                            ClientMessage::ChatTopics { scope } => {
                                if let Some(ref repo) = repository_clone {
                                    let repo = repo.clone();
                                    let tx_chat = tx_input.clone();
                                    tokio::spawn(async move {
                                        match repo.get_chat_topics(&scope).await {
                                            Ok(topics) => {
                                                let _ = tx_chat
                                                    .send(ServerMessage::ChatTopicsResponse {
                                                        scope,
                                                        topics,
                                                    })
                                                    .await;
                                            }
                                            Err(e) => {
                                                warn!("Failed to get chat topics: {}", e);
                                            }
                                        }
                                    });
                                }
                            }
                        }
                    }
                }
                Ok(Message::Close(_)) => {
                    debug!("Client closed connection");
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        // Cancel any focus tasks on disconnect
        {
            let mut guard = focus_cancel_clone.write().await;
            if let Some(cancel) = guard.take() {
                cancel.cancel();
            }
        }
    };

    // Run all tasks
    tokio::select! {
        _ = state_broadcast_task => debug!("State broadcast task ended"),
        _ = lifecycle_task => debug!("Lifecycle task ended"),
        _ = sender_task => debug!("Sender task ended"),
        _ = input_task => debug!("Input task ended"),
    }

    // Clean up VirtualTerminal viewports on disconnect — iterate ALL instances,
    // not just the focused one, to remove any ghost viewport registrations left
    // by instance switches.
    for (instance_id, handle) in state_manager.all_handles().await {
        if let Err(e) = handle.remove_client_and_resize(&connection_id).await {
            warn!(
                instance = %instance_id,
                "Failed to clean up viewport on disconnect: {}", e
            );
        }
    }

    // Clean up presence and terminal locks on disconnect
    if ws_user.is_some() {
        let updates = state_manager.remove_presence_all(&connection_id).await;
        for (instance_id, users) in &updates {
            state_manager.broadcast_lifecycle(ServerMessage::PresenceUpdate {
                instance_id: instance_id.clone(),
                users: users.clone(),
            });
        }
        // Release any terminal locks held by this connection and reconcile
        for (instance_id, _) in &updates {
            let released = state_manager
                .release_terminal_lock(instance_id, &connection_id)
                .await;
            if released {
                state_manager
                    .reconcile_terminal_lock_with_presence(instance_id)
                    .await;
            }
            broadcast_terminal_lock_update(&state_manager, instance_id).await;
        }
    }

    // Log backpressure stats on connection close
    let snapshot = stats.snapshot();

    // Track connection closed in server metrics
    if let Some(ref m) = server_metrics {
        m.connection_closed();
        // Record dropped messages in global metrics
        if snapshot.total_lagged_count > 0 {
            for _ in 0..snapshot.total_lagged_count {
                m.message_dropped();
            }
        }
    }
    if snapshot.total_lagged_count > 0 || snapshot.output_messages_lagged > 0 {
        warn!(
            "WebSocket connection closed with backpressure issues: state_broadcasts={}, output_lagged={}, total_dropped={}",
            snapshot.state_broadcasts_sent,
            snapshot.output_messages_lagged,
            snapshot.total_lagged_count
        );
    } else {
        info!(
            "Multiplexed WebSocket connection closed (state_broadcasts={})",
            snapshot.state_broadcasts_sent
        );
    }
}

/// Build a TerminalLockUpdate message from the current lock state.
fn build_lock_update_message(
    instance_id: &str,
    lock: Option<super::state_manager::TerminalLock>,
) -> ServerMessage {
    match lock {
        Some(lock) => {
            let now = chrono::Utc::now();
            let elapsed = (now - lock.last_activity).num_seconds().max(0) as u64;
            let timeout = TERMINAL_LOCK_TIMEOUT_SECS as u64;
            let expires_in = timeout.saturating_sub(elapsed);

            ServerMessage::TerminalLockUpdate {
                instance_id: instance_id.to_string(),
                holder: Some(PresenceUser {
                    user_id: lock.holder_user_id,
                    display_name: lock.holder_display_name,
                }),
                last_activity: Some(lock.last_activity.to_rfc3339()),
                expires_in_secs: Some(expires_in),
            }
        }
        None => ServerMessage::TerminalLockUpdate {
            instance_id: instance_id.to_string(),
            holder: None,
            last_activity: None,
            expires_in_secs: None,
        },
    }
}

/// Broadcast the current terminal lock state for an instance to all connected clients.
async fn broadcast_terminal_lock_update(state_mgr: &Arc<GlobalStateManager>, instance_id: &str) {
    let lock = state_mgr.get_terminal_lock(instance_id).await;
    let msg = build_lock_update_message(instance_id, lock);
    state_mgr.broadcast_lifecycle(msg);
}

#[cfg(test)]
mod tests {
    use super::super::state_manager::TerminalLock;
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn build_lock_update_no_lock() {
        let msg = build_lock_update_message("inst-1", None);
        match msg {
            ServerMessage::TerminalLockUpdate {
                instance_id,
                holder,
                last_activity,
                expires_in_secs,
            } => {
                assert_eq!(instance_id, "inst-1");
                assert!(holder.is_none());
                assert!(last_activity.is_none());
                assert!(expires_in_secs.is_none());
            }
            _ => panic!("Expected TerminalLockUpdate"),
        }
    }

    #[test]
    fn build_lock_update_with_recent_lock() {
        let lock = TerminalLock {
            holder_connection_id: "conn-1".to_string(),
            holder_user_id: "user-1".to_string(),
            holder_display_name: "Alice".to_string(),
            last_activity: Utc::now(),
        };

        let msg = build_lock_update_message("inst-1", Some(lock));
        match msg {
            ServerMessage::TerminalLockUpdate {
                instance_id,
                holder,
                last_activity,
                expires_in_secs,
            } => {
                assert_eq!(instance_id, "inst-1");
                let h = holder.unwrap();
                assert_eq!(h.user_id, "user-1");
                assert_eq!(h.display_name, "Alice");
                assert!(last_activity.is_some());
                // Recent lock should have ~full timeout remaining
                let remaining = expires_in_secs.unwrap();
                assert!(remaining > 0);
                assert!(remaining <= TERMINAL_LOCK_TIMEOUT_SECS as u64);
            }
            _ => panic!("Expected TerminalLockUpdate"),
        }
    }

    #[test]
    fn build_lock_update_expired_lock() {
        let lock = TerminalLock {
            holder_connection_id: "conn-1".to_string(),
            holder_user_id: "user-1".to_string(),
            holder_display_name: "Alice".to_string(),
            // Way in the past — well beyond any timeout
            last_activity: Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap(),
        };

        let msg = build_lock_update_message("inst-1", Some(lock));
        match msg {
            ServerMessage::TerminalLockUpdate {
                expires_in_secs, ..
            } => {
                // Expired lock should have 0 remaining (saturating_sub)
                assert_eq!(expires_in_secs.unwrap(), 0);
            }
            _ => panic!("Expected TerminalLockUpdate"),
        }
    }

    #[test]
    fn build_lock_update_last_activity_is_rfc3339() {
        let lock = TerminalLock {
            holder_connection_id: "conn-1".to_string(),
            holder_user_id: "user-1".to_string(),
            holder_display_name: "Bob".to_string(),
            last_activity: Utc::now(),
        };

        let msg = build_lock_update_message("inst-2", Some(lock));
        match msg {
            ServerMessage::TerminalLockUpdate { last_activity, .. } => {
                let ts = last_activity.unwrap();
                // Should parse as valid RFC 3339
                chrono::DateTime::parse_from_rfc3339(&ts).unwrap();
            }
            _ => panic!("Expected TerminalLockUpdate"),
        }
    }
}
