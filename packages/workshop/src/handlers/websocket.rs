use axum::{
    extract::{State, WebSocketUpgrade},
    response::Response,
};

use crate::AppState;
use crate::auth::{AuthUser, MaybeAuthUser};
use crate::ws;

/// Resolve a WsUser from an optional AuthUser.
///
/// When the user is authenticated, uses their identity. Otherwise falls back
/// to the system username so that attribution works even without auth.
pub(crate) fn resolve_ws_user(auth_user: Option<AuthUser>) -> ws::WsUser {
    match auth_user {
        Some(u) => ws::WsUser {
            user_id: u.user_id,
            display_name: u.display_name,
        },
        None => {
            let name = std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "local".into());
            ws::WsUser {
                user_id: name.clone(),
                display_name: name,
            }
        }
    }
}

/// Multiplexed WebSocket handler - single connection for all instances
pub async fn multiplexed_websocket_handler(
    State(state): State<AppState>,
    maybe_user: MaybeAuthUser,
    ws: WebSocketUpgrade,
) -> Response {
    let instance_manager = state.instance_manager.clone();
    let global_state_manager = state.global_state_manager.clone();
    let server_config = state.server_config.clone();
    let metrics = state.metrics.clone();
    let repository = state.repository.clone();

    let ws_user = Some(resolve_ws_user(maybe_user.0));

    ws.on_upgrade(move |socket| {
        ws::handle_multiplexed_ws(
            socket,
            instance_manager,
            global_state_manager,
            Some(server_config),
            Some(metrics),
            ws_user,
            Some(repository),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_ws_user_from_auth_user() {
        let auth = AuthUser {
            user_id: "alice-id".into(),
            display_name: "Alice".into(),
            is_admin: false,
            session_token: "tok".into(),
            csrf_token: "csrf".into(),
        };
        let ws = resolve_ws_user(Some(auth));
        assert_eq!(ws.user_id, "alice-id");
        assert_eq!(ws.display_name, "Alice");
    }

    #[test]
    fn resolve_ws_user_fallback_reads_env() {
        // This test validates the env-var cascade. On any Unix system,
        // $USER is set, so we should get the actual username.
        let ws = resolve_ws_user(None);
        // Should never be empty — either $USER, $USERNAME, or "local"
        assert!(!ws.user_id.is_empty());
        assert!(!ws.display_name.is_empty());
        // user_id and display_name should match (both from same source)
        assert_eq!(ws.user_id, ws.display_name);
    }

    #[test]
    fn resolve_ws_user_fallback_not_terminal() {
        // Regression: old code returned None → ws_user was None →
        // everything attributed to "Terminal". Verify we never do that.
        let ws = resolve_ws_user(None);
        assert_ne!(ws.user_id, "terminal");
        assert_ne!(ws.display_name, "Terminal");
    }
}
