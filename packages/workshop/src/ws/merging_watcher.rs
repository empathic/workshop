//! Cross-poll tool result merging wrapper for ConversationWatcher.
//!
//! The upstream `toolpath-claude` ConversationWatcher trait impl only merges
//! tool results within a single poll batch. If a tool_result entry arrives in
//! poll N+1 but the tool_use was emitted in poll N, the result is silently
//! dropped.
//!
//! This wrapper uses the inherent `poll()` to get raw `ConversationEntry`
//! objects, performs its own Turn conversion via `toolpath_claude::provider::to_turn`,
//! and tracks pending tool_uses across poll boundaries to emit `TurnUpdated`
//! when late-arriving results are merged.

use std::collections::HashMap;
use toolpath_claude::{ConversationEntry, MessageRole};
use toolpath_convo::{ConvoError, ToolResult, Turn, WatcherEvent};

/// A ConversationWatcher wrapper that properly merges tool results across
/// poll boundaries.
///
/// Internally tracks all emitted turns and pending tool_use IDs so that
/// tool_result entries arriving in a later poll can be merged into the
/// correct assistant turn, emitting `WatcherEvent::TurnUpdated`.
pub struct MergingWatcher {
    inner: toolpath_claude::ConversationWatcher,
    /// All turns emitted so far, keyed by turn ID.
    /// Used for cross-poll tool result merging.
    emitted_turns: HashMap<String, Turn>,
    /// Maps tool_use_id → turn_id for tool uses still awaiting results.
    pending_tool_uses: HashMap<String, String>,
}

impl MergingWatcher {
    pub fn new(manager: toolpath_claude::ClaudeConvo, project: String, session_id: String) -> Self {
        Self {
            inner: toolpath_claude::ConversationWatcher::new(manager, project, session_id),
            emitted_turns: HashMap::new(),
            pending_tool_uses: HashMap::new(),
        }
    }

    pub fn session_id(&self) -> &str {
        self.inner.session_id()
    }

    pub fn project(&self) -> &str {
        self.inner.project()
    }

    /// Drain any rotation events detected during the last poll.
    /// Returns Vec<(from_session_id, to_session_id)>.
    pub fn take_pending_rotations(&mut self) -> Vec<(String, String)> {
        self.inner.take_pending_rotations()
    }

    /// Check if an entry is a tool-result-only user message
    /// (no human-authored text, only tool_result parts).
    fn is_tool_result_only(entry: &ConversationEntry) -> bool {
        let Some(msg) = &entry.message else {
            return false;
        };
        msg.role == MessageRole::User && msg.text().is_empty() && !msg.tool_results().is_empty()
    }

    /// Merge a single tool result into in-flight events (same-batch).
    /// Returns true if the result was merged.
    fn merge_into_batch(
        &mut self,
        events: &mut [WatcherEvent],
        tool_use_id: &str,
        result: ToolResult,
    ) -> bool {
        for event in events.iter_mut().rev() {
            if let WatcherEvent::Turn(turn) | WatcherEvent::TurnUpdated(turn) = event
                && let Some(inv) = turn
                    .tool_uses
                    .iter_mut()
                    .find(|tu| tu.id == tool_use_id && tu.result.is_none())
            {
                inv.result = Some(result.clone());
                // Keep emitted_turns in sync
                if let Some(stored) = self.emitted_turns.get_mut(&turn.id)
                    && let Some(sinv) = stored.tool_uses.iter_mut().find(|tu| tu.id == tool_use_id)
                {
                    sinv.result = Some(result);
                }
                self.pending_tool_uses.remove(tool_use_id);
                return true;
            }
        }
        false
    }

    /// Re-derive delegation results for a turn after tool results were merged.
    fn update_delegation_results(turn: &mut Turn) {
        for delegation in &mut turn.delegations {
            if delegation.result.is_none()
                && let Some(tu) = turn
                    .tool_uses
                    .iter()
                    .find(|tu| tu.id == delegation.agent_id)
            {
                delegation.result = tu.result.as_ref().map(|r| r.content.clone());
            }
        }
    }
}

impl toolpath_convo::ConversationWatcher for MergingWatcher {
    fn poll(&mut self) -> toolpath_convo::Result<Vec<WatcherEvent>> {
        // Get raw entries from the inherent poll (re-reads file, returns unseen)
        let entries = self
            .inner
            .poll()
            .map_err(|e| ConvoError::Provider(e.to_string()))?;

        let mut events: Vec<WatcherEvent> = Vec::new();

        for entry in &entries {
            // No message → progress event (preserve extra fields)
            if entry.message.is_none() {
                let mut data = serde_json::json!({});
                if let Some(obj) = data.as_object_mut() {
                    for (key, value) in &entry.extra {
                        obj.insert(key.clone(), value.clone());
                    }
                    obj.insert("uuid".to_string(), serde_json::json!(entry.uuid));
                    obj.insert("timestamp".to_string(), serde_json::json!(entry.timestamp));
                }
                events.push(WatcherEvent::Progress {
                    kind: entry.entry_type.clone(),
                    data,
                });
                continue;
            }

            // Tool-result-only entries get merged, not emitted as turns.
            // Cross-poll merges emit TurnUpdated HERE (at the natural position
            // of the tool_result entry) rather than at the end of the batch.
            // This preserves correct ordering: the user's answer signal must
            // arrive BEFORE the subsequent turn_duration signal.
            if Self::is_tool_result_only(entry) {
                let msg = entry.message.as_ref().unwrap();
                let mut updated_this_entry: Vec<String> = Vec::new();

                for tr in msg.tool_results() {
                    let tool_use_id = tr.tool_use_id.to_string();
                    let result = ToolResult {
                        content: tr.content.text(),
                        is_error: tr.is_error,
                    };

                    // First try same-batch merge
                    if self.merge_into_batch(&mut events, &tool_use_id, result.clone()) {
                        continue;
                    }

                    // Cross-poll merge: find the turn that had this tool_use
                    if let Some(turn_id) = self.pending_tool_uses.remove(&tool_use_id)
                        && let Some(stored) = self.emitted_turns.get_mut(&turn_id)
                    {
                        if let Some(inv) =
                            stored.tool_uses.iter_mut().find(|tu| tu.id == tool_use_id)
                        {
                            inv.result = Some(result);
                        }
                        Self::update_delegation_results(stored);
                        if !updated_this_entry.contains(&turn_id) {
                            updated_this_entry.push(turn_id);
                        }
                    }
                    // If no match found anywhere, silently drop
                }

                // Emit TurnUpdated at this position in the event stream
                for turn_id in updated_this_entry {
                    if let Some(turn) = self.emitted_turns.get(&turn_id) {
                        events.push(WatcherEvent::TurnUpdated(Box::new(turn.clone())));
                    }
                }
                continue;
            }

            // Regular entry → convert to Turn
            match toolpath_claude::provider::to_turn(entry) {
                Some(turn) => {
                    // Track pending tool uses
                    for tu in &turn.tool_uses {
                        if tu.result.is_none() {
                            self.pending_tool_uses
                                .insert(tu.id.clone(), turn.id.clone());
                        }
                    }
                    // Store for cross-poll merging
                    self.emitted_turns.insert(turn.id.clone(), turn.clone());
                    events.push(WatcherEvent::Turn(Box::new(turn)));
                }
                None => {
                    // Entry has message but conversion failed (shouldn't happen)
                    let mut data = serde_json::json!({});
                    if let Some(obj) = data.as_object_mut() {
                        for (key, value) in &entry.extra {
                            obj.insert(key.clone(), value.clone());
                        }
                        obj.insert("uuid".to_string(), serde_json::json!(entry.uuid));
                        obj.insert("timestamp".to_string(), serde_json::json!(entry.timestamp));
                    }
                    events.push(WatcherEvent::Progress {
                        kind: entry.entry_type.clone(),
                        data,
                    });
                }
            }
        }

        Ok(events)
    }

    fn seen_count(&self) -> usize {
        self.inner.seen_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use toolpath_claude::{ClaudeConvo, PathResolver};
    use toolpath_convo::Role;

    fn create_test_jsonl(claude_dir: &std::path::Path, session_id: &str, entries: &[&str]) {
        let project_dir = claude_dir.join("projects/-test-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            project_dir.join(format!("{session_id}.jsonl")),
            entries.join("\n") + "\n",
        )
        .unwrap();
    }

    fn test_manager(claude_dir: &std::path::Path) -> ClaudeConvo {
        let resolver = PathResolver::new().with_claude_dir(claude_dir);
        ClaudeConvo::with_resolver(resolver)
    }

    #[test]
    fn same_batch_tool_results_merged_into_turn() {
        let temp = tempfile::TempDir::new().unwrap();
        let claude_dir = temp.path().join(".claude");

        let entries = vec![
            r#"{"uuid":"u1","type":"user","timestamp":"2024-01-01T00:00:00Z","message":{"role":"user","content":"Read file"}}"#,
            r#"{"uuid":"u2","type":"assistant","timestamp":"2024-01-01T00:00:01Z","message":{"role":"assistant","content":[{"type":"text","text":"Reading..."},{"type":"tool_use","id":"t1","name":"Read","input":{"path":"test.rs"}}]}}"#,
            r#"{"uuid":"u3","type":"user","timestamp":"2024-01-01T00:00:02Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"fn main() {}","is_error":false}]}}"#,
        ];
        create_test_jsonl(&claude_dir, "s1", &entries);

        let mut watcher = MergingWatcher::new(
            test_manager(&claude_dir),
            "/test/project".into(),
            "s1".into(),
        );

        let events = toolpath_convo::ConversationWatcher::poll(&mut watcher).unwrap();

        // user Turn + assistant Turn (with result merged in-place)
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], WatcherEvent::Turn(t) if t.role == Role::User));

        if let WatcherEvent::Turn(t) = &events[1] {
            assert_eq!(t.role, Role::Assistant);
            assert_eq!(t.tool_uses.len(), 1);
            let result = t.tool_uses[0]
                .result
                .as_ref()
                .expect("result should be merged");
            assert_eq!(result.content, "fn main() {}");
            assert!(!result.is_error);
        } else {
            panic!("Expected Turn, got {:?}", events[1]);
        }
    }

    #[test]
    fn cross_poll_tool_results_emit_turn_updated() {
        let temp = tempfile::TempDir::new().unwrap();
        let claude_dir = temp.path().join(".claude");

        // Phase 1: user + assistant with tool_use (no result yet)
        let entries_phase1 = vec![
            r#"{"uuid":"u1","type":"user","timestamp":"2024-01-01T00:00:00Z","message":{"role":"user","content":"Read file"}}"#,
            r#"{"uuid":"u2","type":"assistant","timestamp":"2024-01-01T00:00:01Z","message":{"role":"assistant","content":[{"type":"text","text":"Reading..."},{"type":"tool_use","id":"t1","name":"Read","input":{"path":"test.rs"}}]}}"#,
        ];
        create_test_jsonl(&claude_dir, "s1", &entries_phase1);

        let mut watcher = MergingWatcher::new(
            test_manager(&claude_dir),
            "/test/project".into(),
            "s1".into(),
        );

        // First poll: 2 turns, assistant has no result yet
        let events1 = toolpath_convo::ConversationWatcher::poll(&mut watcher).unwrap();
        assert_eq!(events1.len(), 2);
        if let WatcherEvent::Turn(t) = &events1[1] {
            assert!(
                t.tool_uses[0].result.is_none(),
                "result should be None initially"
            );
        } else {
            panic!("Expected Turn");
        }

        // Phase 2: append tool result to the file
        use std::io::Write;
        let project_dir = claude_dir.join("projects/-test-project");
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(project_dir.join("s1.jsonl"))
            .unwrap();
        writeln!(file, r#"{{"uuid":"u3","type":"user","timestamp":"2024-01-01T00:00:02Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t1","content":"fn main() {{}}","is_error":false}}]}}}}"#).unwrap();

        // Second poll: should get TurnUpdated with the result merged
        let events2 = toolpath_convo::ConversationWatcher::poll(&mut watcher).unwrap();
        assert_eq!(events2.len(), 1, "Expected exactly one TurnUpdated event");

        match &events2[0] {
            WatcherEvent::TurnUpdated(turn) => {
                assert_eq!(turn.id, "u2");
                assert_eq!(turn.tool_uses.len(), 1);
                let result = turn.tool_uses[0]
                    .result
                    .as_ref()
                    .expect("result should be merged cross-poll");
                assert_eq!(result.content, "fn main() {}");
                assert!(!result.is_error);
            }
            other => panic!("Expected TurnUpdated, got {:?}", other),
        }
    }

    #[test]
    fn cross_poll_multiple_results_single_turn_updated() {
        let temp = tempfile::TempDir::new().unwrap();
        let claude_dir = temp.path().join(".claude");

        // Phase 1: assistant with two tool_uses
        let entries_phase1 = vec![
            r#"{"uuid":"u1","type":"user","timestamp":"2024-01-01T00:00:00Z","message":{"role":"user","content":"Check files"}}"#,
            r#"{"uuid":"u2","type":"assistant","timestamp":"2024-01-01T00:00:01Z","message":{"role":"assistant","content":[{"type":"text","text":"Checking..."},{"type":"tool_use","id":"t1","name":"Read","input":{"path":"a.rs"}},{"type":"tool_use","id":"t2","name":"Read","input":{"path":"b.rs"}}]}}"#,
        ];
        create_test_jsonl(&claude_dir, "s1", &entries_phase1);

        let mut watcher = MergingWatcher::new(
            test_manager(&claude_dir),
            "/test/project".into(),
            "s1".into(),
        );

        let events1 = toolpath_convo::ConversationWatcher::poll(&mut watcher).unwrap();
        assert_eq!(events1.len(), 2);

        // Phase 2: both results arrive in one entry
        use std::io::Write;
        let project_dir = claude_dir.join("projects/-test-project");
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(project_dir.join("s1.jsonl"))
            .unwrap();
        writeln!(file, r#"{{"uuid":"u3","type":"user","timestamp":"2024-01-01T00:00:02Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t1","content":"file a","is_error":false}},{{"type":"tool_result","tool_use_id":"t2","content":"file b","is_error":false}}]}}}}"#).unwrap();

        let events2 = toolpath_convo::ConversationWatcher::poll(&mut watcher).unwrap();
        // Should get exactly one TurnUpdated (both results merged into same turn)
        assert_eq!(events2.len(), 1);

        if let WatcherEvent::TurnUpdated(turn) = &events2[0] {
            assert_eq!(turn.id, "u2");
            assert_eq!(turn.tool_uses[0].result.as_ref().unwrap().content, "file a");
            assert_eq!(turn.tool_uses[1].result.as_ref().unwrap().content, "file b");
        } else {
            panic!("Expected TurnUpdated");
        }
    }

    #[test]
    fn progress_entries_emitted_as_progress() {
        let temp = tempfile::TempDir::new().unwrap();
        let claude_dir = temp.path().join(".claude");

        let entries = vec![
            r#"{"uuid":"u1","type":"user","timestamp":"2024-01-01T00:00:00Z","message":{"role":"user","content":"Hello"}}"#,
            r#"{"uuid":"p1","type":"progress","timestamp":"2024-01-01T00:00:01Z"}"#,
        ];
        create_test_jsonl(&claude_dir, "s1", &entries);

        let mut watcher = MergingWatcher::new(
            test_manager(&claude_dir),
            "/test/project".into(),
            "s1".into(),
        );

        let events = toolpath_convo::ConversationWatcher::poll(&mut watcher).unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], WatcherEvent::Turn(_)));
        assert!(matches!(&events[1], WatcherEvent::Progress { .. }));
    }

    #[test]
    fn progress_entries_preserve_extra_data() {
        let temp = tempfile::TempDir::new().unwrap();
        let claude_dir = temp.path().join(".claude");

        // agent_progress entry with extra fields (agentId, data, etc.)
        let entries = vec![
            r#"{"uuid":"u1","type":"user","timestamp":"2024-01-01T00:00:00Z","message":{"role":"user","content":"Hello"}}"#,
            r#"{"uuid":"p1","type":"agent_progress","timestamp":"2024-01-01T00:00:01Z","agentId":"agent-123","data":{"type":"agent_progress","agentId":"agent-123","message":{"role":"assistant","content":"Working..."}}}"#,
        ];
        create_test_jsonl(&claude_dir, "s1", &entries);

        let mut watcher = MergingWatcher::new(
            test_manager(&claude_dir),
            "/test/project".into(),
            "s1".into(),
        );

        let events = toolpath_convo::ConversationWatcher::poll(&mut watcher).unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], WatcherEvent::Turn(_)));

        if let WatcherEvent::Progress { kind, data } = &events[1] {
            assert_eq!(kind, "agent_progress");
            assert_eq!(data["uuid"], "p1");
            assert_eq!(data["timestamp"], "2024-01-01T00:00:01Z");
            // Extra fields should be preserved
            assert_eq!(data["agentId"], "agent-123");
            assert_eq!(data["data"]["type"], "agent_progress");
            assert_eq!(data["data"]["agentId"], "agent-123");
            assert_eq!(data["data"]["message"]["content"], "Working...");
        } else {
            panic!("Expected Progress, got {:?}", events[1]);
        }
    }

    #[test]
    fn seen_count_tracks_entries() {
        let temp = tempfile::TempDir::new().unwrap();
        let claude_dir = temp.path().join(".claude");

        let entries = vec![
            r#"{"uuid":"u1","type":"user","timestamp":"2024-01-01T00:00:00Z","message":{"role":"user","content":"Hello"}}"#,
            r#"{"uuid":"u2","type":"assistant","timestamp":"2024-01-01T00:00:01Z","message":{"role":"assistant","content":"Hi"}}"#,
        ];
        create_test_jsonl(&claude_dir, "s1", &entries);

        let mut watcher = MergingWatcher::new(
            test_manager(&claude_dir),
            "/test/project".into(),
            "s1".into(),
        );

        assert_eq!(toolpath_convo::ConversationWatcher::seen_count(&watcher), 0);
        let _ = toolpath_convo::ConversationWatcher::poll(&mut watcher).unwrap();
        assert_eq!(toolpath_convo::ConversationWatcher::seen_count(&watcher), 2);
    }

    #[test]
    fn accessors_delegate_to_inner() {
        let temp = tempfile::TempDir::new().unwrap();
        let claude_dir = temp.path().join(".claude");
        create_test_jsonl(&claude_dir, "s1", &[]);

        let watcher = MergingWatcher::new(
            test_manager(&claude_dir),
            "/test/project".into(),
            "s1".into(),
        );

        assert_eq!(watcher.project(), "/test/project");
        assert_eq!(watcher.session_id(), "s1");
    }

    #[test]
    fn error_result_preserved_cross_poll() {
        let temp = tempfile::TempDir::new().unwrap();
        let claude_dir = temp.path().join(".claude");

        let entries_phase1 = vec![
            r#"{"uuid":"u1","type":"user","timestamp":"2024-01-01T00:00:00Z","message":{"role":"user","content":"Read"}}"#,
            r#"{"uuid":"u2","type":"assistant","timestamp":"2024-01-01T00:00:01Z","message":{"role":"assistant","content":[{"type":"text","text":"Reading..."},{"type":"tool_use","id":"t1","name":"Read","input":{"path":"missing.rs"}}]}}"#,
        ];
        create_test_jsonl(&claude_dir, "s1", &entries_phase1);

        let mut watcher = MergingWatcher::new(
            test_manager(&claude_dir),
            "/test/project".into(),
            "s1".into(),
        );
        let _ = toolpath_convo::ConversationWatcher::poll(&mut watcher).unwrap();

        // Append error result
        use std::io::Write;
        let project_dir = claude_dir.join("projects/-test-project");
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(project_dir.join("s1.jsonl"))
            .unwrap();
        writeln!(file, r#"{{"uuid":"u3","type":"user","timestamp":"2024-01-01T00:00:02Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t1","content":"File not found","is_error":true}}]}}}}"#).unwrap();

        let events2 = toolpath_convo::ConversationWatcher::poll(&mut watcher).unwrap();
        assert_eq!(events2.len(), 1);

        if let WatcherEvent::TurnUpdated(turn) = &events2[0] {
            let result = turn.tool_uses[0].result.as_ref().unwrap();
            assert!(result.is_error);
            assert_eq!(result.content, "File not found");
        } else {
            panic!("Expected TurnUpdated");
        }
    }

    /// Verify that the REAL Claude Code JSONL format (stop_reason: null) produces
    /// a Turn with stop_reason: None.  The downstream watcher_event_to_signal
    /// infers end_turn/tool_use from the Turn's tool_uses field.
    #[test]
    fn real_jsonl_null_stop_reason_produces_none() {
        let temp = tempfile::TempDir::new().unwrap();
        let claude_dir = temp.path().join(".claude");

        // Real Claude Code format: stop_reason is always null in the message
        let entries = vec![
            r#"{"uuid":"u1","type":"user","timestamp":"2024-01-01T00:00:00Z","message":{"role":"user","content":"Hello"}}"#,
            r#"{"uuid":"a1","type":"assistant","timestamp":"2024-01-01T00:00:01Z","message":{"role":"assistant","content":"Hi there!","stop_reason":null,"stop_sequence":null}}"#,
        ];
        create_test_jsonl(&claude_dir, "s1", &entries);

        let mut watcher = MergingWatcher::new(
            test_manager(&claude_dir),
            "/test/project".into(),
            "s1".into(),
        );

        let events = toolpath_convo::ConversationWatcher::poll(&mut watcher).unwrap();
        let assistant_turn = events.iter().find_map(|e| match e {
            WatcherEvent::Turn(t) if t.role == Role::Assistant => Some(t),
            _ => None,
        });

        let turn = assistant_turn.expect("Expected an assistant Turn");
        assert!(
            turn.stop_reason.is_none(),
            "Real JSONL null stop_reason must produce None (not Some(\"null\"))"
        );
        assert!(
            turn.tool_uses.is_empty(),
            "Text-only assistant entry should have no tool_uses"
        );
    }

    /// Verify that stop_reason from assistant entries makes it through to_turn().
    #[test]
    fn assistant_turn_carries_stop_reason() {
        let temp = tempfile::TempDir::new().unwrap();
        let claude_dir = temp.path().join(".claude");

        let entries = vec![
            r#"{"uuid":"u1","type":"user","timestamp":"2024-01-01T00:00:00Z","message":{"role":"user","content":"Hello"}}"#,
            r#"{"uuid":"a1","type":"assistant","timestamp":"2024-01-01T00:00:01Z","message":{"role":"assistant","content":"Hi there!","stop_reason":"end_turn"}}"#,
        ];
        create_test_jsonl(&claude_dir, "s1", &entries);

        let mut watcher = MergingWatcher::new(
            test_manager(&claude_dir),
            "/test/project".into(),
            "s1".into(),
        );

        let events = toolpath_convo::ConversationWatcher::poll(&mut watcher).unwrap();
        let assistant_turn = events.iter().find_map(|e| match e {
            WatcherEvent::Turn(t) if t.role == Role::Assistant => Some(t),
            _ => None,
        });

        let turn = assistant_turn.expect("Expected an assistant Turn");
        assert_eq!(
            turn.stop_reason.as_deref(),
            Some("end_turn"),
            "stop_reason must propagate through to_turn() into turn.stop_reason"
        );
    }

    /// Regression test: cross-poll TurnUpdated must come BEFORE subsequent events
    /// in the same batch (like turn_duration). Without this, the signal ordering
    /// is [assistant, turn_duration, TurnUpdated] which causes the "user" signal
    /// from TurnUpdated to override the definitive WaitingForInput from turn_duration,
    /// leaving the state stuck at Thinking forever.
    #[test]
    fn cross_poll_turn_updated_comes_before_subsequent_events() {
        let temp = tempfile::TempDir::new().unwrap();
        let claude_dir = temp.path().join(".claude");

        // Phase 1: user asks question, assistant uses AskUserQuestion
        let entries_phase1 = vec![
            r#"{"uuid":"u1","type":"user","timestamp":"2024-01-01T00:00:00Z","message":{"role":"user","content":"Do the thing"}}"#,
            r#"{"uuid":"a1","type":"assistant","timestamp":"2024-01-01T00:00:01Z","message":{"role":"assistant","content":[{"type":"text","text":"Which option?"},{"type":"tool_use","id":"t1","name":"AskUserQuestion","input":{"question":"Which?"}}]}}"#,
        ];
        create_test_jsonl(&claude_dir, "s1", &entries_phase1);

        let mut watcher = MergingWatcher::new(
            test_manager(&claude_dir),
            "/test/project".into(),
            "s1".into(),
        );
        let _ = toolpath_convo::ConversationWatcher::poll(&mut watcher).unwrap();

        // Phase 2: user answers + Claude responds + turn_duration, all in one batch.
        // This is the real scenario: tool_result, new assistant entry, and turn_duration
        // all arrive in the same poll.
        use std::io::Write;
        let project_dir = claude_dir.join("projects/-test-project");
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(project_dir.join("s1.jsonl"))
            .unwrap();
        // tool_result for AskUserQuestion
        writeln!(file, r#"{{"uuid":"u2","type":"user","timestamp":"2024-01-01T00:00:02Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t1","content":"Option A","is_error":false}}]}}}}"#).unwrap();
        // Claude's response after getting the answer
        writeln!(file, r#"{{"uuid":"a2","type":"assistant","timestamp":"2024-01-01T00:00:03Z","message":{{"role":"assistant","content":"OK, doing option A."}}}}"#).unwrap();
        // Turn duration (end of turn)
        writeln!(file, r#"{{"uuid":"td1","type":"system","subtype":"turn_duration","timestamp":"2024-01-01T00:00:04Z","durationMs":4000}}"#).unwrap();

        let events2 = toolpath_convo::ConversationWatcher::poll(&mut watcher).unwrap();

        // Critical ordering: TurnUpdated must come BEFORE the assistant Turn and
        // turn_duration Progress. This ensures the state signal ordering is:
        //   user(Thinking) → assistant(no change) → turn_duration(WaitingForInput)
        // NOT:
        //   assistant(no change) → turn_duration(WaitingForInput) → user(Thinking!) ← BUG
        assert_eq!(events2.len(), 3, "Expected TurnUpdated + Turn + Progress");
        assert!(
            matches!(&events2[0], WatcherEvent::TurnUpdated(_)),
            "First event must be TurnUpdated, got {:?}",
            std::mem::discriminant(&events2[0])
        );
        assert!(
            matches!(&events2[1], WatcherEvent::Turn(_)),
            "Second event must be Turn (assistant response)"
        );
        assert!(
            matches!(&events2[2], WatcherEvent::Progress { kind, .. } if kind == "system"),
            "Third event must be Progress (turn_duration)"
        );
    }

    /// Verify that system/turn_duration JSONL entries produce a Progress event
    /// with "subtype" in the data bag (not "type", which is consumed during parsing).
    #[test]
    fn system_turn_duration_progress_data_shape() {
        let temp = tempfile::TempDir::new().unwrap();
        let claude_dir = temp.path().join(".claude");

        let entries = vec![
            r#"{"uuid":"u1","type":"user","timestamp":"2024-01-01T00:00:00Z","message":{"role":"user","content":"Hello"}}"#,
            r#"{"uuid":"a1","type":"assistant","timestamp":"2024-01-01T00:00:01Z","message":{"role":"assistant","content":"Hi","stop_reason":"end_turn"}}"#,
            r#"{"uuid":"td1","type":"system","subtype":"turn_duration","timestamp":"2024-01-01T00:00:02Z","durationMs":1234,"costUSD":0.05}"#,
        ];
        create_test_jsonl(&claude_dir, "s1", &entries);

        let mut watcher = MergingWatcher::new(
            test_manager(&claude_dir),
            "/test/project".into(),
            "s1".into(),
        );

        let events = toolpath_convo::ConversationWatcher::poll(&mut watcher).unwrap();

        let system_progress = events
            .iter()
            .find(|e| matches!(e, WatcherEvent::Progress { kind, .. } if kind == "system"));

        assert!(
            system_progress.is_some(),
            "Expected a Progress event with kind='system'"
        );

        if let WatcherEvent::Progress { kind, data } = system_progress.unwrap() {
            assert_eq!(kind, "system");
            // "subtype" must be in the data bag (not "type" — consumed during parsing)
            assert_eq!(
                data.get("subtype").and_then(|v| v.as_str()),
                Some("turn_duration"),
                "subtype must be extractable from Progress data"
            );
            assert!(
                data.get("type").is_none(),
                "'type' key should NOT be in Progress data (consumed into entry_type)"
            );
        }
    }
}
