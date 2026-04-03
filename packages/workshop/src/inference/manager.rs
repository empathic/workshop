//! Unified State Manager
//!
//! Receives signals from multiple sources (terminal, conversation watcher)
//! and maintains a single source of truth for Claude's state.
//!
//! ## State Detection Strategy
//!
//! State detection uses multiple signal sources, prioritized as follows:
//!
//! 1. **Conversation JSONL** (authoritative):
//!    - `turn_duration` system entry: Definitive turn completion
//!    - `end_turn` stop_reason: Assistant finished responding
//!    - `user` entry type: User sent message → Thinking
//!
//! 2. **Terminal output patterns** (heuristic):
//!    - Tool invocation patterns like "Read(", "Bash(" etc.
//!    - Used to detect tool execution during response
//!    - May have false positives if patterns appear in user messages
//!
//! 3. **Timeout fallback** (safety net):
//!    - 10-second idle timeout as last resort
//!    - Only used when authoritative signals are missed
//!
//! ## Pattern Versioning
//!
//! Terminal patterns may need updating when Claude CLI changes output format.
//! The current patterns are based on Claude Code CLI v1.x spinner output format.

use std::time::{Duration, Instant};
use tracing::debug;

use super::state::{ClaudeState, StateSignal};

/// Tool patterns for detecting tool execution from terminal output.
///
/// Format: (pattern_to_match, tool_name)
///
/// These patterns match Claude CLI's spinner output format, e.g.:
/// - "⠋ Read(src/main.rs)" during file read
/// - "⠙ Bash(ls -la)" during command execution
///
/// **Note:** These are heuristic - conversation JSONL is the authoritative source
/// for state transitions. Terminal patterns provide faster feedback during execution.
///
/// **Important:** More specific patterns must come before less specific ones!
/// E.g., "NotebookEdit(" must come before "Edit(" to match correctly.
///
/// Version: 1.0 (Claude Code CLI v1.x format)
/// Last updated: 2026-02-04
const TOOL_PATTERNS_V1: &[(&str, &str)] = &[
    // Notebook operations (before Edit - more specific)
    ("NotebookEdit(", "NotebookEdit"),
    // Todo operations (before Read/Write - more specific)
    ("TodoRead(", "TodoRead"),
    ("TodoWrite(", "TodoWrite"),
    // Web operations (before Search - more specific)
    ("WebFetch(", "WebFetch"),
    ("WebSearch(", "WebSearch"),
    // Agent/task operations
    ("AskUserQuestion(", "AskUserQuestion"),
    ("EnterPlanMode(", "EnterPlanMode"),
    ("ExitPlanMode(", "ExitPlanMode"),
    ("Task(", "Task"),
    // File operations (general patterns last)
    ("Read(", "Read"),
    ("Write(", "Write"),
    ("Edit(", "Edit"),
    ("Glob(", "Glob"),
    ("Grep(", "Grep"),
    // System operations
    ("Bash(", "Bash"),
];

/// Currently active tool patterns
const TOOL_PATTERNS: &[(&str, &str)] = TOOL_PATTERNS_V1;

/// Configuration for the state manager
pub struct StateManagerConfig {
    /// How long after last activity before considering idle
    pub idle_timeout: Duration,
}

impl Default for StateManagerConfig {
    fn default() -> Self {
        Self {
            // Used for terminal staleness tracking (not state transitions).
            // Authoritative signals (end_turn, turn_duration, tool_use) drive state.
            idle_timeout: Duration::from_secs(10),
        }
    }
}

/// Unified state manager that processes signals from multiple sources
pub struct StateManager {
    state: ClaudeState,
    config: StateManagerConfig,
    /// Last conversation entry time (for idle detection)
    last_convo_activity: Instant,
    /// Last terminal output time (for staleness indicator - separate from state)
    last_terminal_activity: Instant,
    current_tool: Option<String>,
    /// Track if we've already sent idle for this quiet period
    sent_idle: bool,
    /// Last entry role from conversation (for idle detection)
    last_convo_role: Option<String>,
    /// Whether WaitingForInput was set by a definitive signal (turn_duration).
    /// Definitive idle is "sticky" — terminal heuristics cannot override it.
    /// Tentative idle (from assistant entries) IS overridable, allowing
    /// non-interactive tools to recover to ToolExecuting via heuristics.
    definitive_idle: bool,
}

impl StateManager {
    pub fn new(config: StateManagerConfig) -> Self {
        Self {
            state: ClaudeState::Initializing,
            config,
            last_convo_activity: Instant::now(),
            last_terminal_activity: Instant::now(),
            current_tool: None,
            sent_idle: false,
            last_convo_role: None,
            definitive_idle: false,
        }
    }

    /// Process an incoming signal and return new state if changed
    pub fn process(&mut self, signal: StateSignal) -> Option<ClaudeState> {
        let old_state = self.state.clone();

        match &signal {
            StateSignal::TerminalInput { .. } => {
                // Terminal input fires on every keystroke (arrow keys, typing,
                // navigating menus), not just message submission.  It must
                // NEVER cause state transitions — the authoritative signal
                // for Idle/WaitingForInput → Thinking is the JSONL `user`
                // ConversationEntry.  Boot-phase transitions are driven
                // by terminal output patterns (banner detection), not input.
                self.sent_idle = false;
            }

            StateSignal::TerminalOutput { data } => {
                // Track terminal activity to prevent false idle during extended thinking
                self.last_terminal_activity = Instant::now();
                self.sent_idle = false;

                // Initializing → Starting on first terminal output (first byte received)
                if matches!(self.state, ClaudeState::Initializing) {
                    self.state = ClaudeState::Starting;
                }

                // Starting → Idle when the Claude Code banner appears in output.
                // Early startup noise (before Claude is loaded) stays in Starting;
                // only the "Claude Code" banner means the process is at its prompt.
                if matches!(self.state, ClaudeState::Starting) && data.contains("Claude Code") {
                    self.state = ClaudeState::Idle;
                }

                // WaitingForInput has two modes:
                // - Definitive (from turn_duration): truly sticky, ignores all heuristics.
                //   This prevents false positives from tool patterns in Claude's text
                //   (e.g. "I used Read(file) to check") after the turn is over.
                // - Tentative (from assistant entry): overridable by tool patterns.
                //   This allows non-interactive tools (Read, Bash) to show ToolExecuting
                //   after the assistant entry signals the API call completed.
                if matches!(self.state, ClaudeState::WaitingForInput { .. }) && self.definitive_idle
                {
                    // Definitive idle — ignore terminal heuristics
                } else if let Some(tool) = self.detect_tool(data) {
                    self.current_tool = Some(tool.clone());
                    self.definitive_idle = false;
                    self.state = ClaudeState::ToolExecuting { tool };
                } else if matches!(self.state, ClaudeState::Thinking) {
                    // First output after thinking -> responding
                    self.state = ClaudeState::Responding;
                }
            }

            StateSignal::ConversationEntry {
                entry_type,
                subtype,
                stop_reason,
                tool_names,
            } => {
                debug!(
                    "ConversationEntry signal: type={}, subtype={:?}, stop_reason={:?}, tools={:?}",
                    entry_type, subtype, stop_reason, tool_names
                );
                // Only conversation entries reset the idle timer
                self.last_convo_activity = Instant::now();
                self.sent_idle = false;

                // Check for definitive turn completion signal
                if entry_type == "system" && subtype.as_deref() == Some("turn_duration") {
                    debug!("Got turn_duration -> WaitingForInput (definitive)");
                    self.current_tool = None;
                    self.definitive_idle = true;
                    self.state = ClaudeState::WaitingForInput { prompt: None };
                    return Some(self.state.clone()); // Early return - this is authoritative
                }

                self.last_convo_role = Some(entry_type.clone());

                // Any conversation entry while still booting means Claude is alive
                if matches!(
                    self.state,
                    ClaudeState::Initializing | ClaudeState::Starting
                ) {
                    self.state = ClaudeState::Idle;
                }

                if entry_type == "user" {
                    self.definitive_idle = false;
                    self.state = ClaudeState::Thinking;
                } else if entry_type == "tool_result" {
                    // Non-interactive tool result merged (TurnUpdated from mid-chain).
                    // If already in an active state (Thinking/Responding/ToolExecuting),
                    // don't change — avoids a Thinking flash between tool calls.
                    // If in Idle or tentative WaitingForInput, transition to Thinking
                    // so the state reflects that Claude is processing input.
                    if !self.definitive_idle
                        && matches!(
                            self.state,
                            ClaudeState::Idle | ClaudeState::WaitingForInput { .. }
                        )
                    {
                        debug!("Got tool_result from inactive state -> Thinking");
                        self.definitive_idle = false;
                        self.state = ClaudeState::Thinking;
                    } else {
                        debug!("Got tool_result while active -> no state change");
                    }
                } else if entry_type == "assistant" {
                    // Assistant entries with ONLY non-interactive tool uses are
                    // mid-turn (the agentic loop will continue with tool results
                    // and another API call). Don't change state or we'd flicker
                    // between sequential tool calls.
                    //
                    // All other assistant entries mean the model stopped generating:
                    //   - No tool uses (text-only) → turn is ending
                    //   - Interactive tool (AskUserQuestion) → needs user input
                    // In both cases → WaitingForInput (tentative, not definitive).
                    let mid_turn = !tool_names.is_empty()
                        && !tool_names
                            .iter()
                            .any(|name| Self::is_interactive_tool(name));

                    if mid_turn {
                        debug!(
                            "Got assistant entry (tools={:?}) -> no state change (mid-turn)",
                            tool_names
                        );
                    } else {
                        self.current_tool = None;
                        // Don't downgrade definitive idle to tentative.
                        // The text-only assistant deferral can deliver this
                        // signal one poll cycle AFTER turn_duration already
                        // set definitive WaitingForInput.  Clearing
                        // definitive_idle here lets terminal heuristic false
                        // positives (e.g. "Read(" in printed response text)
                        // override WaitingForInput → stuck at ToolExecuting.
                        if !self.definitive_idle {
                            debug!(
                                "Got assistant entry (tools={:?}) -> WaitingForInput (tentative)",
                                tool_names
                            );
                            self.state = ClaudeState::WaitingForInput { prompt: None };
                        } else {
                            debug!(
                                "Got assistant entry (tools={:?}) -> already definitive idle, ignoring",
                                tool_names
                            );
                        }
                    }
                }
            }

            StateSignal::Tick => {
                // Tick is only used for staleness tracking, not state transitions.
                // State transitions rely on authoritative signals:
                // - end_turn: assistant finished responding
                // - turn_duration: definitive turn completion
                // - tool_use: assistant paused for tool execution (interactive or not)
            }
        }

        if self.state != old_state {
            debug!("State changed: {:?} -> {:?}", old_state, self.state);
            Some(self.state.clone())
        } else {
            None
        }
    }

    /// Get current state
    #[allow(dead_code)]
    pub fn state(&self) -> &ClaudeState {
        &self.state
    }

    /// Check if terminal output is stale (no recent activity)
    /// This is separate from state - indicates confidence in the state
    pub fn is_terminal_stale(&self) -> bool {
        self.last_terminal_activity.elapsed() > self.config.idle_timeout
    }

    /// Check if conversation data is stale (no recent entries)
    #[allow(dead_code)]
    pub fn is_conversation_stale(&self) -> bool {
        self.last_convo_activity.elapsed() > self.config.idle_timeout
    }

    /// Get time since last terminal activity
    #[allow(dead_code)]
    pub fn terminal_idle_duration(&self) -> Duration {
        self.last_terminal_activity.elapsed()
    }

    /// Reset state (e.g., when switching instances)
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.state = ClaudeState::Idle;
        self.last_convo_activity = Instant::now();
        self.last_terminal_activity = Instant::now();
        self.current_tool = None;
        self.sent_idle = false;
        self.last_convo_role = None;
        self.definitive_idle = false;
    }

    /// Tools that require user input (questions, permission, plan mode).
    /// These should transition to WaitingForInput, not stay "verbing."
    fn is_interactive_tool(name: &str) -> bool {
        matches!(name, "AskUserQuestion" | "EnterPlanMode" | "ExitPlanMode")
    }

    fn detect_tool(&self, output: &str) -> Option<String> {
        for (pattern, tool) in TOOL_PATTERNS {
            if output.contains(pattern) {
                return Some(tool.to_string());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_manager() -> StateManager {
        StateManager::new(StateManagerConfig::default())
    }

    #[test]
    fn test_initial_state_is_initializing() {
        let manager = default_manager();
        assert_eq!(*manager.state(), ClaudeState::Initializing);
    }

    /// Create a manager in Starting state (first byte received)
    fn starting_manager() -> StateManager {
        let mut manager = default_manager();
        manager.state = ClaudeState::Starting;
        manager
    }

    /// Create a manager that has already booted (Starting → Idle)
    fn idle_manager() -> StateManager {
        let mut manager = default_manager();
        manager.state = ClaudeState::Idle;
        manager
    }

    // --- Initializing phase tests ---

    #[test]
    fn test_initializing_to_starting_on_first_output() {
        let mut manager = default_manager();
        let result = manager.process(StateSignal::TerminalOutput {
            data: "Loading...".to_string(),
        });
        assert_eq!(result, Some(ClaudeState::Starting));
        assert_eq!(*manager.state(), ClaudeState::Starting);
    }

    #[test]
    fn test_initializing_to_idle_on_claude_code_banner() {
        // If the very first output contains the banner, go straight to Idle
        let mut manager = default_manager();
        let result = manager.process(StateSignal::TerminalOutput {
            data: "Claude Code v2.1.37".to_string(),
        });
        assert_eq!(result, Some(ClaudeState::Idle));
        assert_eq!(*manager.state(), ClaudeState::Idle);
    }

    #[test]
    fn test_initializing_ignores_terminal_input() {
        // Keystrokes must never drive state — only JSONL conversation
        // entries are authoritative for Thinking transitions.
        let mut manager = default_manager();
        let result = manager.process(StateSignal::TerminalInput {
            data: "hello".to_string(),
        });
        assert_eq!(result, None);
        assert_eq!(*manager.state(), ClaudeState::Initializing);
    }

    #[test]
    fn test_initializing_to_waiting_on_text_only_assistant_entry() {
        let mut manager = default_manager();
        // An assistant entry while Initializing → Idle (boot transition),
        // then text-only assistant → WaitingForInput (tentative).
        let result = manager.process(StateSignal::ConversationEntry {
            entry_type: "assistant".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        assert_eq!(result, Some(ClaudeState::WaitingForInput { prompt: None }));
    }

    #[test]
    fn test_initializing_to_thinking_on_user_conversation_entry() {
        let mut manager = default_manager();
        let result = manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        assert_eq!(result, Some(ClaudeState::Thinking));
    }

    // --- Starting phase tests ---

    #[test]
    fn test_starting_stays_starting_on_generic_terminal_output() {
        let mut manager = starting_manager();
        // Generic PTY output (startup noise) should NOT transition out of Starting.
        let result = manager.process(StateSignal::TerminalOutput {
            data: "Loading...".to_string(),
        });
        assert_eq!(result, None);
        assert_eq!(*manager.state(), ClaudeState::Starting);
    }

    #[test]
    fn test_starting_to_idle_on_claude_code_banner() {
        let mut manager = starting_manager();
        // The Claude Code banner signals the process is loaded and at its prompt.
        let result = manager.process(StateSignal::TerminalOutput {
            data: "Claude Code v2.1.37".to_string(),
        });
        assert_eq!(result, Some(ClaudeState::Idle));
        assert_eq!(*manager.state(), ClaudeState::Idle);
    }

    #[test]
    fn test_starting_ignores_terminal_input() {
        // Keystrokes during boot must not cause state transitions.
        // Boot-phase transitions are driven by terminal output (banner).
        let mut manager = starting_manager();
        let result = manager.process(StateSignal::TerminalInput {
            data: "hello".to_string(),
        });
        assert_eq!(result, None);
        assert_eq!(*manager.state(), ClaudeState::Starting);
    }

    #[test]
    fn test_starting_to_waiting_on_text_only_assistant_entry() {
        let mut manager = starting_manager();
        // An assistant entry while Starting → Idle (boot transition),
        // then text-only assistant → WaitingForInput (tentative).
        let result = manager.process(StateSignal::ConversationEntry {
            entry_type: "assistant".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        assert_eq!(result, Some(ClaudeState::WaitingForInput { prompt: None }));
    }

    #[test]
    fn test_starting_to_thinking_on_user_conversation_entry() {
        let mut manager = starting_manager();
        let result = manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        assert_eq!(result, Some(ClaudeState::Thinking));
    }

    #[test]
    fn test_terminal_input_from_idle_does_not_transition() {
        // TerminalInput fires on every keystroke (not just message submission).
        // Idle → Thinking must come from the authoritative JSONL user entry,
        // not from keystrokes — otherwise typing shows false "active" state.
        let mut manager = idle_manager();
        let result = manager.process(StateSignal::TerminalInput {
            data: "hello".to_string(),
        });
        assert_eq!(result, None);
        assert_eq!(*manager.state(), ClaudeState::Idle);
    }

    #[test]
    fn test_terminal_input_during_tool_executing_no_change() {
        // Terminal input (keystrokes) must never change state, even during
        // active tool execution.
        let mut manager = default_manager();
        manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        manager.process(StateSignal::TerminalOutput {
            data: "⠋ Read(file.rs)".to_string(),
        });
        assert!(matches!(manager.state(), ClaudeState::ToolExecuting { .. }));

        let result = manager.process(StateSignal::TerminalInput {
            data: "y".to_string(),
        });
        assert_eq!(
            result, None,
            "TerminalInput must never trigger state transitions"
        );
        assert!(matches!(manager.state(), ClaudeState::ToolExecuting { .. }));
    }

    #[test]
    fn test_terminal_output_transitions_thinking_to_responding() {
        let mut manager = default_manager();
        // JSONL user entry is the authoritative Thinking trigger
        manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        let result = manager.process(StateSignal::TerminalOutput {
            data: "Hi there!".to_string(),
        });
        assert_eq!(result, Some(ClaudeState::Responding));
        assert_eq!(*manager.state(), ClaudeState::Responding);
    }

    #[test]
    fn test_terminal_output_with_tool_pattern() {
        let mut manager = default_manager();
        manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        manager.process(StateSignal::TerminalOutput {
            data: "Starting...".to_string(),
        });
        let result = manager.process(StateSignal::TerminalOutput {
            data: "Read(file.txt)".to_string(),
        });
        assert!(matches!(
            result,
            Some(ClaudeState::ToolExecuting { tool }) if tool == "Read"
        ));
    }

    #[test]
    fn test_conversation_entry_turn_duration_transitions_to_waiting() {
        let mut manager = default_manager();
        // Start in Responding state
        manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        manager.process(StateSignal::TerminalOutput {
            data: "response".to_string(),
        });
        assert_eq!(*manager.state(), ClaudeState::Responding);

        // Send turn_duration signal
        let result = manager.process(StateSignal::ConversationEntry {
            entry_type: "system".to_string(),
            subtype: Some("turn_duration".to_string()),
            stop_reason: None,
            tool_names: vec![],
        });

        assert_eq!(result, Some(ClaudeState::WaitingForInput { prompt: None }));
        assert_eq!(
            *manager.state(),
            ClaudeState::WaitingForInput { prompt: None }
        );
    }

    #[test]
    fn test_text_only_assistant_transitions_to_waiting() {
        // Assistant entries with no tool uses (text-only response) signal the
        // end of the turn → WaitingForInput (tentative).
        let mut manager = default_manager();
        manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        manager.process(StateSignal::TerminalOutput {
            data: "response".to_string(),
        });
        assert_eq!(*manager.state(), ClaudeState::Responding);

        let result = manager.process(StateSignal::ConversationEntry {
            entry_type: "assistant".to_string(),
            subtype: None,
            stop_reason: Some("end_turn".to_string()),
            tool_names: vec![],
        });

        assert_eq!(
            result,
            Some(ClaudeState::WaitingForInput { prompt: None }),
            "Text-only assistant entry must transition to WaitingForInput"
        );
    }

    #[test]
    fn test_assistant_with_non_interactive_tools_does_not_change_state() {
        // Assistant entries with non-interactive tool uses are mid-turn
        // (agentic loop) — don't change state or we'd flicker.
        let mut manager = default_manager();
        manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        manager.process(StateSignal::TerminalOutput {
            data: "response".to_string(),
        });
        assert_eq!(*manager.state(), ClaudeState::Responding);

        let result = manager.process(StateSignal::ConversationEntry {
            entry_type: "assistant".to_string(),
            subtype: None,
            stop_reason: Some("end_turn".to_string()),
            tool_names: vec!["Read".to_string()],
        });

        assert_eq!(
            result, None,
            "Mid-turn assistant entry (non-interactive tools) must not change state"
        );
        assert_eq!(*manager.state(), ClaudeState::Responding);
    }

    #[test]
    fn test_conversation_entry_user_transitions_to_thinking() {
        let mut manager = idle_manager();
        // Start in Idle
        assert_eq!(*manager.state(), ClaudeState::Idle);

        // User entry should transition to Thinking
        let result = manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });

        assert_eq!(result, Some(ClaudeState::Thinking));
    }

    #[test]
    fn test_text_only_assistant_from_thinking_transitions_to_waiting() {
        // Even from Thinking, a text-only assistant entry means the model
        // finished generating — transition to WaitingForInput.
        let mut manager = default_manager();
        manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        assert_eq!(*manager.state(), ClaudeState::Thinking);

        let result = manager.process(StateSignal::ConversationEntry {
            entry_type: "assistant".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });

        assert_eq!(
            result,
            Some(ClaudeState::WaitingForInput { prompt: None }),
            "Text-only assistant must transition to WaitingForInput"
        );
    }

    #[test]
    fn test_turn_duration_from_any_state() {
        // turn_duration should ALWAYS transition to WaitingForInput
        let states_to_test = vec![
            ClaudeState::Starting,
            ClaudeState::Idle,
            ClaudeState::Thinking,
            ClaudeState::Responding,
            ClaudeState::ToolExecuting {
                tool: "Read".to_string(),
            },
        ];

        for initial_state in states_to_test {
            let mut manager = default_manager();

            // Force to initial state via authoritative signals
            match &initial_state {
                ClaudeState::Starting => {
                    manager.state = ClaudeState::Starting;
                }
                ClaudeState::Thinking => {
                    manager.process(StateSignal::ConversationEntry {
                        entry_type: "user".to_string(),
                        subtype: None,
                        stop_reason: None,
                        tool_names: vec![],
                    });
                }
                ClaudeState::Responding => {
                    manager.process(StateSignal::ConversationEntry {
                        entry_type: "user".to_string(),
                        subtype: None,
                        stop_reason: None,
                        tool_names: vec![],
                    });
                    manager.process(StateSignal::TerminalOutput {
                        data: "y".to_string(),
                    });
                }
                ClaudeState::ToolExecuting { .. } => {
                    manager.process(StateSignal::ConversationEntry {
                        entry_type: "user".to_string(),
                        subtype: None,
                        stop_reason: None,
                        tool_names: vec![],
                    });
                    manager.process(StateSignal::TerminalOutput {
                        data: "Read(file)".to_string(),
                    });
                }
                _ => {}
            }

            let result = manager.process(StateSignal::ConversationEntry {
                entry_type: "system".to_string(),
                subtype: Some("turn_duration".to_string()),
                stop_reason: None,
                tool_names: vec![],
            });

            assert_eq!(
                result,
                Some(ClaudeState::WaitingForInput { prompt: None }),
                "turn_duration should transition from {:?} to WaitingForInput",
                initial_state
            );
        }
    }

    #[test]
    fn test_terminal_output_does_not_override_waiting_for_input() {
        let mut manager = default_manager();
        // Get to WaitingForInput via authoritative conversation signal
        manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        manager.process(StateSignal::TerminalOutput {
            data: "response".to_string(),
        });
        manager.process(StateSignal::ConversationEntry {
            entry_type: "system".to_string(),
            subtype: Some("turn_duration".to_string()),
            stop_reason: None,
            tool_names: vec![],
        });
        assert_eq!(
            *manager.state(),
            ClaudeState::WaitingForInput { prompt: None }
        );

        // Terminal output with tool pattern should NOT override WaitingForInput.
        // This is the false-positive scenario: Claude's response text mentions
        // "Read(" but the conversation is already over.
        let result = manager.process(StateSignal::TerminalOutput {
            data: "I used Read(file.rs) to check".to_string(),
        });
        assert_eq!(
            result, None,
            "Tool pattern in terminal must not override WaitingForInput"
        );
        assert_eq!(
            *manager.state(),
            ClaudeState::WaitingForInput { prompt: None },
            "State must remain WaitingForInput after false-positive tool pattern"
        );
    }

    #[test]
    fn test_terminal_input_does_not_exit_definitive_waiting() {
        // When turn is over (definitive WaitingForInput from turn_duration),
        // keystrokes must not exit WaitingForInput — user is just typing
        // their next message. Only the JSONL `user` entry transitions.
        let mut manager = default_manager();
        // Get to definitive WaitingForInput via turn_duration
        manager.process(StateSignal::ConversationEntry {
            entry_type: "system".to_string(),
            subtype: Some("turn_duration".to_string()),
            stop_reason: None,
            tool_names: vec![],
        });
        assert_eq!(
            *manager.state(),
            ClaudeState::WaitingForInput { prompt: None }
        );
        assert!(manager.definitive_idle);

        // User typing should NOT exit definitive WaitingForInput
        let result = manager.process(StateSignal::TerminalInput {
            data: "next question".to_string(),
        });
        assert_eq!(
            result, None,
            "Keystroke must not change definitive WaitingForInput"
        );
        assert_eq!(
            *manager.state(),
            ClaudeState::WaitingForInput { prompt: None }
        );
    }

    #[test]
    fn test_terminal_input_does_not_exit_tentative_waiting() {
        // WaitingForInput from AskUserQuestion (tentative, not definitive)
        // must also ignore keystrokes — user is navigating the prompt.
        let mut manager = default_manager();
        manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        manager.process(StateSignal::ConversationEntry {
            entry_type: "assistant".to_string(),
            subtype: None,
            stop_reason: Some("end_turn".to_string()),
            tool_names: vec!["AskUserQuestion".to_string()],
        });
        assert_eq!(
            *manager.state(),
            ClaudeState::WaitingForInput { prompt: None }
        );
        assert!(!manager.definitive_idle);

        // Arrow keys navigating AskUserQuestion options
        let result = manager.process(StateSignal::TerminalInput {
            data: "\x1b[B".to_string(), // down arrow
        });
        assert_eq!(
            result, None,
            "Keystroke must not change tentative WaitingForInput"
        );
        assert_eq!(
            *manager.state(),
            ClaudeState::WaitingForInput { prompt: None }
        );
    }

    #[test]
    fn test_user_conversation_entry_triggers_thinking() {
        let mut manager = default_manager();
        // User entry from conversation watcher should transition to Thinking
        let result = manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        assert_eq!(
            result,
            Some(ClaudeState::Thinking),
            "entry_type 'user' must trigger Thinking (was broken when watcher sent 'human')"
        );
    }

    #[test]
    fn test_repeated_terminal_output_no_state_change() {
        let mut manager = default_manager();
        manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        manager.process(StateSignal::TerminalOutput {
            data: "response".to_string(),
        });
        assert_eq!(*manager.state(), ClaudeState::Responding);

        // Additional output should NOT emit state change
        let result = manager.process(StateSignal::TerminalOutput {
            data: "more response".to_string(),
        });
        assert_eq!(result, None);
        assert_eq!(*manager.state(), ClaudeState::Responding);
    }

    #[test]
    fn test_terminal_activity_tracked_for_staleness() {
        // Verify that terminal activity is tracked separately from conversation activity
        let mut manager = StateManager::new(StateManagerConfig {
            idle_timeout: Duration::from_millis(50),
        });

        // Initial state - both should be fresh
        assert!(!manager.is_terminal_stale());
        assert!(!manager.is_conversation_stale());

        // Wait for timeout
        std::thread::sleep(Duration::from_millis(100));

        // Both should now be stale
        assert!(manager.is_terminal_stale());
        assert!(manager.is_conversation_stale());

        // Terminal output should refresh terminal staleness
        manager.process(StateSignal::TerminalOutput {
            data: "output".to_string(),
        });
        assert!(!manager.is_terminal_stale());
        assert!(manager.is_conversation_stale()); // Convo still stale

        // Conversation entry should refresh convo staleness
        manager.process(StateSignal::ConversationEntry {
            entry_type: "assistant".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        assert!(!manager.is_conversation_stale());
    }

    // ==========================================
    // Real Claude Output Sample Tests
    // ==========================================
    // These test against actual Claude CLI output patterns to catch
    // regressions when Claude CLI format changes.

    /// Real Claude CLI output samples and expected tool detection results
    /// Format: (terminal_output, expected_tool_name_or_none)
    const REAL_CLAUDE_SAMPLES: &[(&str, Option<&str>)] = &[
        // Tool execution with spinner
        ("⠋ Read(src/main.rs)", Some("Read")),
        ("⠙ Read(/path/to/file.txt)", Some("Read")),
        ("⠹ Write(output.json)", Some("Write")),
        ("⠸ Edit(src/lib.rs)", Some("Edit")),
        ("⠼ Bash(ls -la)", Some("Bash")),
        ("⠴ Bash(cargo build)", Some("Bash")),
        ("⠦ Glob(**/*.rs)", Some("Glob")),
        ("⠧ Grep(TODO)", Some("Grep")),
        ("⠇ WebFetch(https://example.com)", Some("WebFetch")),
        ("⠏ WebSearch(rust async)", Some("WebSearch")),
        ("⠋ Task(explore files)", Some("Task")),
        ("⠙ AskUserQuestion(Which option?)", Some("AskUserQuestion")),
        ("⠋ EnterPlanMode()", Some("EnterPlanMode")),
        ("⠙ ExitPlanMode()", Some("ExitPlanMode")),
        ("⠹ NotebookEdit(notebook.ipynb)", Some("NotebookEdit")),
        // Plain text output (no tool)
        ("Hello! How can I help you today?", None),
        ("Let me analyze this code for you.", None),
        ("I'll help you with that task.", None),
        // Edge cases - tool names in regular text (should NOT match)
        // Note: Current simple pattern matching WILL match these - known limitation
        // The authoritative source (conversation JSONL) prevents false state transitions
        ("I can Read(files) for you", Some("Read")), // Known false positive
        ("Try using Bash(command)", Some("Bash")),   // Known false positive
        // Empty and whitespace
        ("", None),
        ("   ", None),
        ("\n\n", None),
        // Unicode in output
        ("🔍 Searching...", None),
        ("✅ Done!", None),
    ];

    #[test]
    fn test_tool_detection_against_real_samples() {
        let manager = default_manager();

        for (output, expected_tool) in REAL_CLAUDE_SAMPLES {
            let detected = manager.detect_tool(output);
            assert_eq!(
                detected.as_deref(),
                *expected_tool,
                "Tool detection mismatch for output: {:?}\nExpected: {:?}\nGot: {:?}",
                output,
                expected_tool,
                detected
            );
        }
    }

    #[test]
    fn test_all_documented_tools_have_patterns() {
        // List of tools that Claude can use (should have patterns)
        let expected_tools = [
            "Read",
            "Write",
            "Edit",
            "Bash",
            "Glob",
            "Grep",
            "WebFetch",
            "WebSearch",
            "Task",
            "AskUserQuestion",
            "EnterPlanMode",
            "ExitPlanMode",
            "TodoRead",
            "TodoWrite",
            "NotebookEdit",
        ];

        let pattern_tools: std::collections::HashSet<&str> =
            TOOL_PATTERNS.iter().map(|(_, tool)| *tool).collect();

        for tool in expected_tools {
            assert!(
                pattern_tools.contains(tool),
                "Missing pattern for tool: {}",
                tool
            );
        }
    }

    #[test]
    fn test_tool_detection_is_case_sensitive() {
        let manager = default_manager();

        // Should match exact case
        assert_eq!(manager.detect_tool("Read(file)"), Some("Read".to_string()));

        // Should NOT match different case
        assert_eq!(manager.detect_tool("read(file)"), None);
        assert_eq!(manager.detect_tool("READ(file)"), None);
    }

    #[test]
    fn test_tool_detection_requires_open_paren() {
        let manager = default_manager();

        // Should match with open paren
        assert_eq!(manager.detect_tool("Read(file)"), Some("Read".to_string()));

        // Should NOT match without open paren
        assert_eq!(manager.detect_tool("Read file"), None);
        assert_eq!(manager.detect_tool("Reading"), None);
    }

    // ==========================================
    // Tick behavior tests
    // ==========================================

    #[test]
    fn test_assistant_with_non_interactive_tool_use_does_not_change_state() {
        // Mid-turn assistant entries (with non-interactive tool uses) must
        // not change state. Terminal heuristics handle tool detection,
        // TurnUpdated handles "user answered" transitions.
        let mut manager = default_manager();
        manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        manager.process(StateSignal::TerminalOutput {
            data: "response".to_string(),
        });
        assert_eq!(*manager.state(), ClaudeState::Responding);

        let result = manager.process(StateSignal::ConversationEntry {
            entry_type: "assistant".to_string(),
            subtype: None,
            stop_reason: Some("tool_use".to_string()),
            tool_names: vec!["Read".to_string()],
        });

        assert_eq!(
            result, None,
            "Mid-turn assistant entry (non-interactive tools) must not change state"
        );
        assert_eq!(*manager.state(), ClaudeState::Responding);
    }

    #[test]
    fn test_multi_tool_sequence_no_flickering() {
        // Simulate 2 Reads then a final text response: state should stay
        // "active" during tool calls, never briefly showing WaitingForInput
        // between them. Only the final text-only assistant entry shows ready.
        let mut manager = default_manager();

        // User sends message
        manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        assert_eq!(*manager.state(), ClaudeState::Thinking);

        // Terminal shows first tool
        manager.process(StateSignal::TerminalOutput {
            data: "⠋ Read(file1.rs)".to_string(),
        });
        assert!(matches!(
            manager.state(),
            ClaudeState::ToolExecuting { tool } if tool == "Read"
        ));

        // Assistant entry arrives with Read tool — mid-turn, must not flicker
        let result = manager.process(StateSignal::ConversationEntry {
            entry_type: "assistant".to_string(),
            subtype: None,
            stop_reason: Some("end_turn".to_string()),
            tool_names: vec!["Read".to_string()],
        });
        assert_eq!(result, None, "No flickering between tool calls");

        // Tool result merged (TurnUpdated) → user signal → Thinking
        manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        assert_eq!(*manager.state(), ClaudeState::Thinking);

        // Terminal shows second tool
        manager.process(StateSignal::TerminalOutput {
            data: "⠋ Read(file2.rs)".to_string(),
        });
        assert!(matches!(
            manager.state(),
            ClaudeState::ToolExecuting { tool } if tool == "Read"
        ));

        // Another mid-turn assistant entry with tools — still no flickering
        let result = manager.process(StateSignal::ConversationEntry {
            entry_type: "assistant".to_string(),
            subtype: None,
            stop_reason: Some("end_turn".to_string()),
            tool_names: vec!["Read".to_string()],
        });
        assert_eq!(result, None, "No flickering between tool calls");

        // Tool result → user signal → Thinking
        manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });

        // Final text-only assistant entry → WaitingForInput (tentative)
        let result = manager.process(StateSignal::ConversationEntry {
            entry_type: "assistant".to_string(),
            subtype: None,
            stop_reason: Some("end_turn".to_string()),
            tool_names: vec![],
        });
        assert_eq!(
            result,
            Some(ClaudeState::WaitingForInput { prompt: None }),
            "Final text-only assistant entry must show ready"
        );

        // Turn ends (definitive)
        manager.process(StateSignal::ConversationEntry {
            entry_type: "system".to_string(),
            subtype: Some("turn_duration".to_string()),
            stop_reason: None,
            tool_names: vec![],
        });
        assert_eq!(
            *manager.state(),
            ClaudeState::WaitingForInput { prompt: None }
        );
    }

    #[test]
    fn test_interactive_tool_ask_user_question_sets_waiting() {
        // AskUserQuestion requires user input → WaitingForInput
        let mut manager = default_manager();
        manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        manager.process(StateSignal::TerminalOutput {
            data: "response".to_string(),
        });
        assert_eq!(*manager.state(), ClaudeState::Responding);

        let result = manager.process(StateSignal::ConversationEntry {
            entry_type: "assistant".to_string(),
            subtype: None,
            stop_reason: Some("end_turn".to_string()),
            tool_names: vec!["AskUserQuestion".to_string()],
        });
        assert_eq!(
            result,
            Some(ClaudeState::WaitingForInput { prompt: None }),
            "AskUserQuestion must transition to WaitingForInput"
        );
    }

    #[test]
    fn test_non_interactive_tool_does_not_set_waiting() {
        // Read is non-interactive → no state change from assistant entry
        let mut manager = default_manager();
        manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        manager.process(StateSignal::TerminalOutput {
            data: "response".to_string(),
        });

        let result = manager.process(StateSignal::ConversationEntry {
            entry_type: "assistant".to_string(),
            subtype: None,
            stop_reason: Some("end_turn".to_string()),
            tool_names: vec!["Read".to_string()],
        });
        assert_eq!(result, None, "Non-interactive tool must not change state");
        assert_eq!(*manager.state(), ClaudeState::Responding);
    }

    #[test]
    fn test_tick_does_not_transition_state() {
        // Tick is for staleness tracking only — state transitions come from
        // authoritative conversation signals (end_turn, turn_duration, tool_use).
        let mut manager = StateManager::new(StateManagerConfig {
            idle_timeout: Duration::from_millis(50),
        });
        manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        manager.process(StateSignal::TerminalOutput {
            data: "response".to_string(),
        });
        assert_eq!(*manager.state(), ClaudeState::Responding);

        std::thread::sleep(Duration::from_millis(100));

        let result = manager.process(StateSignal::Tick);
        assert_eq!(result, None, "Tick must not cause state transitions");
        assert_eq!(*manager.state(), ClaudeState::Responding);
    }

    // ==========================================
    // tool_result signal tests (non-interactive TurnUpdated)
    // ==========================================

    #[test]
    fn test_tool_result_from_idle_transitions_to_thinking() {
        let mut manager = default_manager();
        let result = manager.process(StateSignal::ConversationEntry {
            entry_type: "tool_result".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        assert_eq!(result, Some(ClaudeState::Thinking));
    }

    #[test]
    fn test_tool_result_from_active_state_no_change() {
        let mut manager = default_manager();
        manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        manager.process(StateSignal::TerminalOutput {
            data: "⠋ Read(file.rs)".to_string(),
        });
        assert!(matches!(manager.state(), ClaudeState::ToolExecuting { .. }));

        let result = manager.process(StateSignal::ConversationEntry {
            entry_type: "tool_result".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        assert_eq!(result, None, "tool_result from active state must not flash");
    }

    #[test]
    fn test_tool_result_from_definitive_waiting_no_change() {
        let mut manager = default_manager();
        manager.process(StateSignal::ConversationEntry {
            entry_type: "system".to_string(),
            subtype: Some("turn_duration".to_string()),
            stop_reason: None,
            tool_names: vec![],
        });
        assert!(manager.definitive_idle);

        let result = manager.process(StateSignal::ConversationEntry {
            entry_type: "tool_result".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        assert_eq!(
            result, None,
            "tool_result must not override definitive idle"
        );
    }

    // ==========================================
    // Definitive vs tentative idle tests
    // ==========================================

    #[test]
    fn test_turn_duration_sets_definitive_idle() {
        let mut manager = default_manager();
        manager.process(StateSignal::ConversationEntry {
            entry_type: "system".to_string(),
            subtype: Some("turn_duration".to_string()),
            stop_reason: None,
            tool_names: vec![],
        });
        assert_eq!(
            *manager.state(),
            ClaudeState::WaitingForInput { prompt: None }
        );
        assert!(
            manager.definitive_idle,
            "turn_duration must set definitive_idle"
        );
    }

    #[test]
    fn test_definitive_idle_not_overridable_by_tool_pattern() {
        // End of turn: turn_duration → definitive WaitingForInput
        // → terminal false positive "Read(" → must NOT override
        let mut manager = default_manager();
        manager.process(StateSignal::ConversationEntry {
            entry_type: "system".to_string(),
            subtype: Some("turn_duration".to_string()),
            stop_reason: None,
            tool_names: vec![],
        });
        assert!(manager.definitive_idle);

        let result = manager.process(StateSignal::TerminalOutput {
            data: "I used Read(file.rs) to check".to_string(),
        });
        assert_eq!(
            result, None,
            "Tool pattern must NOT override definitive WaitingForInput"
        );
        assert_eq!(
            *manager.state(),
            ClaudeState::WaitingForInput { prompt: None }
        );
    }

    #[test]
    fn test_deferred_text_only_assistant_does_not_downgrade_definitive_idle() {
        // Regression: the conversation watcher holds text-only assistant signals
        // for one poll cycle.  When turn_duration arrives first (setting
        // definitive idle), the deferred text-only signal arrives AFTER.
        // It must not downgrade definitive_idle to tentative — otherwise
        // terminal heuristic false positives ("Read(" in response text)
        // override WaitingForInput → stuck at ToolExecuting.
        let mut manager = default_manager();

        // Simulate: turn_duration arrives first
        manager.process(StateSignal::ConversationEntry {
            entry_type: "system".to_string(),
            subtype: Some("turn_duration".to_string()),
            stop_reason: None,
            tool_names: vec![],
        });
        assert_eq!(
            *manager.state(),
            ClaudeState::WaitingForInput { prompt: None }
        );
        assert!(manager.definitive_idle);

        // Then deferred text-only assistant arrives (next poll cycle)
        let result = manager.process(StateSignal::ConversationEntry {
            entry_type: "assistant".to_string(),
            subtype: None,
            stop_reason: Some("end_turn".to_string()),
            tool_names: vec![],
        });
        assert_eq!(
            result, None,
            "Deferred text-only assistant must not emit state change when already definitive idle"
        );
        assert!(
            manager.definitive_idle,
            "definitive_idle must not be downgraded by deferred text-only assistant"
        );

        // Now verify that a false-positive tool pattern is still blocked
        let result = manager.process(StateSignal::TerminalOutput {
            data: "I used Read(file.rs) to check".to_string(),
        });
        assert_eq!(
            result, None,
            "Tool pattern must NOT override WaitingForInput after deferred text-only"
        );
        assert_eq!(
            *manager.state(),
            ClaudeState::WaitingForInput { prompt: None }
        );
    }

    #[test]
    fn test_user_entry_clears_definitive_idle() {
        let mut manager = default_manager();
        // Set definitive idle
        manager.process(StateSignal::ConversationEntry {
            entry_type: "system".to_string(),
            subtype: Some("turn_duration".to_string()),
            stop_reason: None,
            tool_names: vec![],
        });
        assert!(manager.definitive_idle);

        // User types → Thinking, clears definitive
        manager.process(StateSignal::ConversationEntry {
            entry_type: "user".to_string(),
            subtype: None,
            stop_reason: None,
            tool_names: vec![],
        });
        assert_eq!(*manager.state(), ClaudeState::Thinking);
        assert!(!manager.definitive_idle);
    }
}
