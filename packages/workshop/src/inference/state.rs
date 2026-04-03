//! Claude State Types
//!
//! Defines the state machine for tracking Claude's activity.
//! Uses a unified state manager that receives signals from multiple sources.

use serde::{Deserialize, Serialize};

/// The current state of a Claude instance
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
#[derive(Default)]
pub enum ClaudeState {
    /// PTY spawned, waiting for first byte of output
    Initializing,

    /// First output received, waiting for Claude Code banner/prompt
    Starting,

    /// Claude is waiting for user input (prompt visible)
    #[default]
    Idle,

    /// User sent input, Claude is processing but no output yet
    Thinking,

    /// Claude is actively streaming a response
    Responding,

    /// Claude is executing a tool
    ToolExecuting { tool: String },

    /// Claude is waiting for user confirmation or additional input
    WaitingForInput { prompt: Option<String> },
}

/// Events emitted during state transitions (for external consumers)
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
pub enum StateEvent {
    /// State has changed
    StateChanged { state: ClaudeState },

    /// A tool execution has started
    ToolStarted { tool: String },

    /// A tool execution has completed
    ToolCompleted { tool: String },
}

/// Input signals to the unified state manager (from multiple sources)
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum StateSignal {
    /// Terminal output received (for tool detection)
    TerminalOutput { data: String },

    /// User sent input to terminal
    TerminalInput { data: String },

    /// Conversation entry from JSONL watcher
    ConversationEntry {
        entry_type: String,
        subtype: Option<String>,
        stop_reason: Option<String>,
        /// Tool names from assistant entry tool_uses (empty for non-assistant entries)
        tool_names: Vec<String>,
    },

    /// Periodic tick for timeout detection (fallback)
    Tick,
}

impl ClaudeState {
    /// Returns true if Claude is actively working (not waiting for input)
    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            ClaudeState::Thinking | ClaudeState::Responding | ClaudeState::ToolExecuting { .. }
        )
    }

    /// Returns the tool name if currently executing a tool
    #[allow(dead_code)]
    pub fn current_tool(&self) -> Option<&str> {
        match self {
            ClaudeState::ToolExecuting { tool } => Some(tool),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializing_is_not_active() {
        assert!(!ClaudeState::Initializing.is_active());
    }

    #[test]
    fn starting_is_not_active() {
        assert!(!ClaudeState::Starting.is_active());
    }

    #[test]
    fn idle_is_not_active() {
        assert!(!ClaudeState::Idle.is_active());
    }

    #[test]
    fn thinking_is_active() {
        assert!(ClaudeState::Thinking.is_active());
    }

    #[test]
    fn responding_is_active() {
        assert!(ClaudeState::Responding.is_active());
    }

    #[test]
    fn tool_executing_is_active() {
        let state = ClaudeState::ToolExecuting {
            tool: "Read".to_string(),
        };
        assert!(state.is_active());
    }

    #[test]
    fn waiting_for_input_is_not_active() {
        let state = ClaudeState::WaitingForInput {
            prompt: Some("Continue?".into()),
        };
        assert!(!state.is_active());
    }

    #[test]
    fn current_tool_when_executing() {
        let state = ClaudeState::ToolExecuting {
            tool: "Bash".to_string(),
        };
        assert_eq!(state.current_tool(), Some("Bash"));
    }

    #[test]
    fn current_tool_none_for_other_states() {
        assert!(ClaudeState::Idle.current_tool().is_none());
        assert!(ClaudeState::Thinking.current_tool().is_none());
        assert!(ClaudeState::Responding.current_tool().is_none());
        assert!(
            ClaudeState::WaitingForInput { prompt: None }
                .current_tool()
                .is_none()
        );
    }

    #[test]
    fn default_is_idle() {
        assert_eq!(ClaudeState::default(), ClaudeState::Idle);
    }

    #[test]
    fn serde_roundtrip_initializing() {
        let state = ClaudeState::Initializing;
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"type\":\"Initializing\""));
        let parsed: ClaudeState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ClaudeState::Initializing);
    }

    #[test]
    fn serde_roundtrip_starting() {
        let state = ClaudeState::Starting;
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"type\":\"Starting\""));
        let parsed: ClaudeState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ClaudeState::Starting);
    }

    #[test]
    fn serde_roundtrip_idle() {
        let state = ClaudeState::Idle;
        let json = serde_json::to_string(&state).unwrap();
        let parsed: ClaudeState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ClaudeState::Idle);
    }

    #[test]
    fn serde_roundtrip_tool_executing() {
        let state = ClaudeState::ToolExecuting {
            tool: "Write".to_string(),
        };
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"type\":\"ToolExecuting\""));
        assert!(json.contains("\"tool\":\"Write\""));
        let parsed: ClaudeState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, state);
    }

    #[test]
    fn serde_roundtrip_waiting_for_input() {
        let state = ClaudeState::WaitingForInput {
            prompt: Some("Allow?".into()),
        };
        let json = serde_json::to_string(&state).unwrap();
        let parsed: ClaudeState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, state);
    }

    #[test]
    fn serde_state_event() {
        let event = StateEvent::ToolStarted {
            tool: "Grep".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"ToolStarted\""));
        assert!(json.contains("\"tool\":\"Grep\""));
    }
}
