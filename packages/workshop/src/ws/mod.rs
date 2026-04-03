//! Multiplexed WebSocket Handler
//!
//! Single WebSocket connection per client that:
//! - Receives state changes from ALL instances (low bandwidth)
//! - Receives terminal output from ONE focused instance (high bandwidth)
//! - Handles focus switching with history replay

mod conversation_watcher;
mod focus;
mod handler;
pub(crate) mod merging_watcher;
pub(crate) mod protocol;
mod session_discovery;
mod state_manager;

// Re-export the main types and functions
pub(crate) use conversation_watcher::run_driver_conversation_watcher;
pub use handler::handle_multiplexed_ws;
pub use protocol::{ClientMessage, ServerMessage, WsUser};
pub use state_manager::{
    ConversationEvent, FirstInputData, GlobalStateManager, PendingAttribution, StateBroadcast,
    create_state_broadcast,
};
// Re-exported for integration tests in instance_actor
#[allow(unused_imports)]
pub use state_manager::InputContext;
