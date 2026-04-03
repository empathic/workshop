//! ProcessDriver Trait
//!
//! Each instance actor owns a driver that handles process-type-specific
//! behavior: state detection, conversation tracking, session discovery.
//! Adding a new process type = implementing ProcessDriver.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::{RwLock, broadcast, mpsc};

use crate::repository::ConversationRepository;
use crate::ws::ConversationEvent;
use crate::ws::{FirstInputData, PendingAttribution};

/// Signals from a driver's background work (e.g. conversation watcher).
#[derive(Clone, Debug, PartialEq)]
pub enum DriverSignal {
    /// A conversation entry was observed in the JSONL log.
    ConversationEntry {
        entry_type: String,
        subtype: Option<String>,
        stop_reason: Option<String>,
        tool_names: Vec<String>,
    },
    /// A new Claude session was discovered.
    SessionDiscovered(String),
    /// Full conversation snapshot (initial load or after update).
    ConversationSnapshot(Vec<serde_json::Value>),
    /// Incremental conversation turns.
    ConversationDelta(Vec<serde_json::Value>),
}

/// Effects returned by a driver after processing a signal.
#[derive(Debug, PartialEq)]
pub struct DriverEffect {
    pub state_change: Option<ProcessState>,
    pub session_id: Option<String>,
}

impl DriverEffect {
    pub fn none() -> Self {
        Self {
            state_change: None,
            session_id: None,
        }
    }
}

/// Process state — generic across all process types.
#[derive(Clone, Debug, PartialEq)]
pub enum ProcessState {
    Initializing,
    Starting,
    Idle,
    Working {
        detail: Option<String>,
    },
    WaitingForInput {
        prompt: Option<String>,
    },
    #[allow(dead_code)]
    Exited,
}

/// Context passed to a driver's `start()` method.
pub struct DriverContext {
    pub working_dir: String,
    pub instance_id: String,
    pub instance_created_at: DateTime<Utc>,
    pub claimed_sessions: Arc<RwLock<HashMap<String, String>>>,
    pub first_input_data: Arc<RwLock<HashMap<String, FirstInputData>>>,
    pub pending_attributions: Arc<RwLock<HashMap<String, VecDeque<PendingAttribution>>>>,
    pub repository: Option<Arc<ConversationRepository>>,
}

/// Trait for process-type-specific behavior inside an instance actor.
pub trait ProcessDriver: Send + 'static {
    /// Feed raw PTY output. Returns state change if any.
    fn on_output(&mut self, data: &[u8]) -> Option<ProcessState>;

    /// Feed user input. Returns state change if any.
    fn on_input(&mut self, data: &str) -> Option<ProcessState>;

    /// Periodic tick (~500ms). Returns state change if any.
    fn tick(&mut self) -> Option<ProcessState>;

    /// Start background work (file watchers, pollers).
    /// Called once after actor is running. Returns a receiver for signals
    /// from background tasks, or None if no background work is needed.
    fn start(&mut self, ctx: DriverContext) -> Option<mpsc::Receiver<DriverSignal>>;

    /// Process a signal from background work.
    fn on_signal(&mut self, signal: DriverSignal) -> DriverEffect;

    /// Current state.
    #[allow(dead_code)]
    fn state(&self) -> ProcessState;

    /// Get the Claude-specific state if this is a Claude driver.
    /// Used by the actor for backward-compatible state broadcasts.
    fn claude_state(&self) -> Option<&crate::inference::ClaudeState> {
        None
    }

    /// Whether terminal output is stale (no recent activity).
    fn is_terminal_stale(&self) -> bool {
        false
    }

    /// Current conversation snapshot (empty for non-conversation drivers).
    fn conversation_snapshot(&self) -> &[serde_json::Value];

    /// Subscribe to conversation events. Returns None for non-conversation drivers.
    fn subscribe_conversation(&self) -> Option<broadcast::Receiver<ConversationEvent>>;
}

/// A no-op driver for raw shell processes.
pub struct ShellDriver;

impl ProcessDriver for ShellDriver {
    fn on_output(&mut self, _: &[u8]) -> Option<ProcessState> {
        None
    }
    fn on_input(&mut self, _: &str) -> Option<ProcessState> {
        None
    }
    fn tick(&mut self) -> Option<ProcessState> {
        None
    }
    fn start(&mut self, _: DriverContext) -> Option<mpsc::Receiver<DriverSignal>> {
        None
    }
    fn on_signal(&mut self, _: DriverSignal) -> DriverEffect {
        DriverEffect::none()
    }
    fn state(&self) -> ProcessState {
        ProcessState::Idle
    }
    fn conversation_snapshot(&self) -> &[serde_json::Value] {
        &[]
    }
    fn subscribe_conversation(&self) -> Option<broadcast::Receiver<ConversationEvent>> {
        None
    }
}
