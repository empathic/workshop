use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use tokio::sync::RwLock;
use toolpath_convo::{Role, ToolCategory, Turn};
use tracing::warn;

use crate::models::{attribution_content_matches, normalize_attribution_content};
use crate::repository;
use crate::ws::PendingAttribution;

/// Normalize whitespace: collapse consecutive blank lines to one, strip trailing blanks,
/// preserve indentation within lines.
fn normalize_whitespace(s: &str) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let mut result = Vec::new();
    let mut last_was_empty = true;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !last_was_empty {
                result.push("");
                last_was_empty = true;
            }
        } else {
            result.push(line);
            last_was_empty = false;
        }
    }
    while result.last().map(|s| s.trim().is_empty()).unwrap_or(false) {
        result.pop();
    }
    result.join("\n")
}

fn category_str(c: &ToolCategory) -> &'static str {
    match c {
        ToolCategory::FileRead => "file_read",
        ToolCategory::FileWrite => "file_write",
        ToolCategory::FileSearch => "file_search",
        ToolCategory::Shell => "shell",
        ToolCategory::Network => "network",
        ToolCategory::Delegation => "delegation",
    }
}

/// Format a Turn (provider-agnostic) for the frontend.
/// Handles User, Assistant, and System messages.
pub fn format_turn(turn: &Turn) -> serde_json::Value {
    let role_str = match &turn.role {
        Role::User => "User",
        Role::Assistant => "Assistant",
        Role::System => "System",
        Role::Other(s) => s.as_str(),
    };

    let tool_names: Vec<&str> = turn.tool_uses.iter().map(|t| t.name.as_str()).collect();

    let tool_details: Vec<serde_json::Value> = turn
        .tool_uses
        .iter()
        .map(|t| {
            let mut detail = serde_json::json!({
                "name": t.name,
                "input": t.input,
            });
            if let Some(c) = &t.category {
                detail["category"] = serde_json::Value::String(category_str(c).to_string());
            }
            if let Some(r) = &t.result {
                detail["result"] = serde_json::Value::String(r.content.clone());
                if r.is_error {
                    detail["is_error"] = serde_json::Value::Bool(true);
                }
            }
            detail
        })
        .collect();

    let mut json = serde_json::json!({
        "uuid": turn.id,
        "role": role_str,
        "content": normalize_whitespace(&turn.text),
        "timestamp": turn.timestamp,
        "tools": tool_names,
        "tool_details": tool_details,
    });

    // Thinking (if present)
    if let Some(thinking) = &turn.thinking {
        json["thinking"] = serde_json::Value::String(thinking.clone());
    }

    // Enrichment: model
    if let Some(model) = &turn.model {
        json["model"] = serde_json::Value::String(model.clone());
    }

    // Enrichment: tool categories
    let categories: HashMap<&str, &str> = turn
        .tool_uses
        .iter()
        .filter_map(|t| {
            t.category
                .as_ref()
                .map(|c| (t.name.as_str(), category_str(c)))
        })
        .collect();
    if !categories.is_empty() {
        json["tool_categories"] = serde_json::json!(categories);
    }

    // Enrichment: environment snapshot
    if let Some(env) = &turn.environment {
        json["environment"] = serde_json::json!(env);
    }

    // Enrichment: delegations
    if !turn.delegations.is_empty() {
        json["delegations"] = serde_json::json!(
            turn.delegations
                .iter()
                .map(|d| {
                    serde_json::json!({
                        "agent_id": d.agent_id,
                        "prompt": if d.prompt.len() > 200 {
                            format!("{}...", &d.prompt[..d.prompt.floor_char_boundary(200)])
                        } else {
                            d.prompt.clone()
                        },
                        "result": d.result,
                    })
                })
                .collect::<Vec<_>>()
        );
    }

    // System messages: include role as entry_type for backward compat
    if turn.role == Role::System {
        json["entry_type"] = serde_json::Value::String("system".into());
    }

    json
}

/// Consume a matching pending attribution from the shared map.
///
/// Searches the queue for the given instance for an entry whose content prefix
/// matches the entry content. The first match is consumed (FIFO). Stale entries
/// (older than 60 seconds) are pruned if no match is found.
pub async fn consume_pending_attribution(
    pending_attributions: &RwLock<HashMap<String, VecDeque<PendingAttribution>>>,
    instance_id: &str,
    entry_content: &str,
) -> Option<PendingAttribution> {
    if normalize_attribution_content(entry_content).is_empty() {
        return None;
    }

    let mut map = pending_attributions.write().await;
    let queue = map.get_mut(instance_id)?;

    if let Some(idx) = queue
        .iter()
        .position(|attr| attribution_content_matches(&attr.content_prefix, entry_content))
    {
        Some(queue.remove(idx).unwrap())
    } else {
        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(60);
        queue.retain(|attr| attr.timestamp > cutoff);
        None
    }
}

/// Format a Turn with user attribution.
///
/// Attribution sources (checked in order):
/// 1. In-process pending attribution queue (content-matched, for live sessions)
/// 2. DB-based InputAttribution (for historical entries / conversation reload)
/// 3. "Terminal" fallback (auth enabled but no attribution — external input)
///
/// If no auth/repository configured, leaves unattributed (frontend shows "You").
pub async fn format_turn_with_attribution(
    turn: &Turn,
    instance_id: &str,
    repo: Option<&Arc<repository::ConversationRepository>>,
    pending_attributions: Option<&RwLock<HashMap<String, VecDeque<PendingAttribution>>>>,
) -> serde_json::Value {
    let mut json = format_turn(turn);

    if turn.role != Role::User {
        return json;
    }

    let content_text = if turn.text.is_empty() {
        None
    } else {
        Some(turn.text.as_str())
    };

    // 1. Try in-process content-matched attribution (fast path, no DB)
    if let (Some(content), Some(attrs)) = (content_text, pending_attributions)
        && let Some(attr) = consume_pending_attribution(attrs, instance_id, content).await
    {
        if let Some(obj) = json.as_object_mut() {
            obj.insert(
                "attributed_to".to_string(),
                serde_json::json!({
                    "user_id": attr.user_id,
                    "display_name": attr.display_name,
                }),
            );
            if let Some(task_id) = attr.task_id {
                obj.insert("task_id".to_string(), serde_json::json!(task_id));
            }
        }
        // Persist entry_uuid link to DB so historical queries find it
        if let Some(repo) = repo {
            let repo = Arc::clone(repo);
            let instance_id = instance_id.to_string();
            let entry_uuid = turn.id.clone();
            let entry_content = content.to_string();
            let unix_ts = chrono::DateTime::parse_from_rfc3339(&turn.timestamp)
                .map(|dt| dt.timestamp())
                .unwrap_or(0);
            tokio::spawn(async move {
                let _ = repo
                    .correlate_attribution(&instance_id, &entry_uuid, unix_ts, Some(&entry_content))
                    .await;
            });
        }
        return json;
    }

    // 2. Fall back to DB-based attribution
    if let Some(repo) = repo {
        let unix_ts = chrono::DateTime::parse_from_rfc3339(&turn.timestamp)
            .map(|dt| dt.timestamp())
            .unwrap_or(0);

        match repo
            .get_or_correlate_attribution(instance_id, &turn.id, unix_ts, content_text)
            .await
        {
            Ok(Some(attr)) => {
                if let Some(obj) = json.as_object_mut() {
                    obj.insert(
                        "attributed_to".to_string(),
                        serde_json::json!({
                            "user_id": attr.user_id,
                            "display_name": attr.display_name,
                        }),
                    );
                    if let Some(task_id) = attr.task_id {
                        obj.insert("task_id".to_string(), serde_json::json!(task_id));
                    }
                }
                return json;
            }
            Ok(None) => {
                if let Some(obj) = json.as_object_mut() {
                    obj.insert(
                        "attributed_to".to_string(),
                        serde_json::json!({
                            "user_id": "terminal",
                            "display_name": "Terminal",
                        }),
                    );
                }
            }
            Err(e) => {
                warn!("Failed to look up attribution for turn {}: {}", turn.id, e);
            }
        }
    }

    json
}

/// Format a progress event from a `WatcherEvent::Progress { kind, data }`.
///
/// For `agent_progress` events, extracts structured sub-agent data (content,
/// tool uses, agent ID) so the frontend can populate the Task card's activity
/// timeline. Other progress kinds produce a minimal placeholder entry.
pub fn format_progress_event(kind: &str, data: &serde_json::Value) -> serde_json::Value {
    let uuid = data.get("uuid").and_then(|v| v.as_str()).unwrap_or("");
    let timestamp = data.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
    let empty_tools: Vec<String> = vec![];

    // Detect agent_progress: the nested "data" object carries the real payload.
    let inner = data.get("data");
    let is_agent_progress =
        inner.and_then(|d| d.get("type")).and_then(|v| v.as_str()) == Some("agent_progress");

    if !is_agent_progress {
        return serde_json::json!({
            "uuid": uuid,
            "role": "Unknown",
            "content": "",
            "timestamp": timestamp,
            "tools": empty_tools,
            "entry_type": kind,
        });
    }

    let inner = inner.unwrap();
    let agent_id = inner.get("agentId").and_then(|v| v.as_str()).unwrap_or("");

    // The message lives at data.message.message (outer is the wrapper, inner is the API message).
    let msg = inner.get("message");
    let inner_msg = msg.and_then(|m| m.get("message")).or(msg);

    let agent_msg_role = inner_msg
        .and_then(|m| m.get("role"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Extract content: string or array of content parts
    let raw_content = inner_msg.and_then(|m| m.get("content"));

    let (content_text, agent_tools) = match raw_content {
        Some(serde_json::Value::String(s)) => (s.clone(), vec![]),
        Some(serde_json::Value::Array(parts)) => {
            let mut texts = Vec::new();
            let mut tools: Vec<serde_json::Value> = Vec::new();
            for part in parts {
                match part.get("type").and_then(|v| v.as_str()) {
                    Some("text") => {
                        if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                            texts.push(t.to_string());
                        }
                    }
                    Some("tool_use") => {
                        let name = part
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let mut tool_obj = serde_json::json!({ "name": name });
                        if let Some(input) = part.get("input") {
                            tool_obj["input"] = input.clone();
                        }
                        tools.push(tool_obj);
                    }
                    _ => {}
                }
            }
            (texts.join("\n"), tools)
        }
        // Fallback chain: toolUseResult → prompt → empty
        None => {
            let fallback = inner
                .get("toolUseResult")
                .and_then(|v| v.as_str())
                .or_else(|| inner.get("prompt").and_then(|v| v.as_str()))
                .unwrap_or("");
            (fallback.to_string(), vec![])
        }
        _ => (String::new(), vec![]),
    };

    let mut result = serde_json::json!({
        "uuid": uuid,
        "role": "AgentProgress",
        "content": normalize_whitespace(&content_text),
        "timestamp": timestamp,
        "tools": empty_tools,
        "entry_type": "agent_progress",
        "agent_id": agent_id,
        "agent_msg_role": format!("agent_{}", agent_msg_role),
    });

    if !agent_tools.is_empty() {
        result["agent_tools"] = serde_json::Value::Array(agent_tools);
    }

    result
}

/// Extract title from a Turn's text, truncated to `limit` chars.
/// Returns None if the turn is not a user message or has no text.
pub fn extract_title_from_turn(turn: &Turn, limit: usize) -> Option<String> {
    if turn.role != Role::User || turn.text.is_empty() {
        return None;
    }
    Some(turn.text.chars().take(limit).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Whitespace normalization ──────────────────────────────────

    #[test]
    fn whitespace_normalization_collapses_blanks() {
        assert_eq!(
            normalize_whitespace("Hello\n\n\n\nWorld\n\n\n"),
            "Hello\n\nWorld"
        );
    }

    #[test]
    fn whitespace_preserves_indentation() {
        assert_eq!(
            normalize_whitespace("def foo():\n    return 42"),
            "def foo():\n    return 42"
        );
    }

    // ── format_progress_event ─────────────────────────────────────

    #[test]
    fn progress_event_extracts_uuid_and_timestamp() {
        let data = json!({"uuid": "p1", "timestamp": "2024-06-01T12:00:00Z"});
        let result = format_progress_event("hook_progress", &data);

        assert_eq!(result["uuid"], "p1");
        assert_eq!(result["timestamp"], "2024-06-01T12:00:00Z");
        assert_eq!(result["entry_type"], "hook_progress");
        assert_eq!(result["role"], "Unknown");
        assert_eq!(result["content"], "");
        assert_eq!(result["tools"], json!([]));
    }

    #[test]
    fn progress_event_missing_fields_defaults_empty() {
        let data = json!({});
        let result = format_progress_event("some_progress", &data);

        assert_eq!(result["uuid"], "");
        assert_eq!(result["timestamp"], "");
        assert_eq!(result["entry_type"], "some_progress");
        assert_eq!(result["role"], "Unknown");
    }

    #[test]
    fn agent_progress_string_content() {
        let data = json!({
            "uuid": "p1",
            "timestamp": "2024-06-01T12:00:00Z",
            "agentId": "agent-1",
            "data": {
                "type": "agent_progress",
                "agentId": "agent-1",
                "message": {
                    "role": "assistant",
                    "content": "Reading the file now."
                }
            }
        });
        let result = format_progress_event("agent_progress", &data);

        assert_eq!(result["role"], "AgentProgress");
        assert_eq!(result["content"], "Reading the file now.");
        assert_eq!(result["agent_id"], "agent-1");
        assert_eq!(result["agent_msg_role"], "agent_assistant");
        assert_eq!(result["entry_type"], "agent_progress");
        assert!(result.get("agent_tools").is_none());
    }

    #[test]
    fn agent_progress_array_content_with_tool_uses() {
        let data = json!({
            "uuid": "p2",
            "timestamp": "2024-06-01T12:00:01Z",
            "data": {
                "type": "agent_progress",
                "agentId": "agent-2",
                "message": {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "Checking files."},
                        {"type": "tool_use", "id": "t1", "name": "Read", "input": {"path": "/foo"}},
                        {"type": "tool_use", "id": "t2", "name": "Grep", "input": {"pattern": "TODO"}}
                    ]
                }
            }
        });
        let result = format_progress_event("agent_progress", &data);

        assert_eq!(result["role"], "AgentProgress");
        assert_eq!(result["content"], "Checking files.");
        assert_eq!(result["agent_id"], "agent-2");
        let tools = result["agent_tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["name"], "Read");
        assert_eq!(tools[0]["input"]["path"], "/foo");
        assert_eq!(tools[1]["name"], "Grep");
    }

    #[test]
    fn agent_progress_fallback_chain() {
        // No message.content → falls back to toolUseResult
        let data = json!({
            "uuid": "p3",
            "timestamp": "2024-06-01T12:00:02Z",
            "data": {
                "type": "agent_progress",
                "agentId": "agent-3",
                "toolUseResult": "File contents here"
            }
        });
        let result = format_progress_event("agent_progress", &data);

        assert_eq!(result["role"], "AgentProgress");
        assert_eq!(result["content"], "File contents here");

        // No toolUseResult → falls back to prompt
        let data2 = json!({
            "uuid": "p4",
            "timestamp": "2024-06-01T12:00:03Z",
            "data": {
                "type": "agent_progress",
                "agentId": "agent-4",
                "prompt": "Find the bug"
            }
        });
        let result2 = format_progress_event("agent_progress", &data2);
        assert_eq!(result2["content"], "Find the bug");
    }

    #[test]
    fn non_agent_progress_unchanged() {
        let data = json!({
            "uuid": "h1",
            "timestamp": "2024-06-01T12:00:00Z",
            "data": {
                "type": "hook_progress",
                "hookId": "pre-commit"
            }
        });
        let result = format_progress_event("hook_progress", &data);

        assert_eq!(result["role"], "Unknown");
        assert_eq!(result["content"], "");
        assert_eq!(result["entry_type"], "hook_progress");
    }

    #[test]
    fn agent_progress_nested_message_message() {
        // Some progress events wrap message inside message.message
        let data = json!({
            "uuid": "p5",
            "timestamp": "2024-06-01T12:00:00Z",
            "data": {
                "type": "agent_progress",
                "agentId": "agent-5",
                "message": {
                    "message": {
                        "role": "assistant",
                        "content": "Nested content"
                    }
                }
            }
        });
        let result = format_progress_event("agent_progress", &data);

        assert_eq!(result["role"], "AgentProgress");
        assert_eq!(result["content"], "Nested content");
        assert_eq!(result["agent_msg_role"], "agent_assistant");
    }

    // ── extract_title_from_turn ───────────────────────────────────

    #[test]
    fn extract_title_user_message() {
        let turn = Turn {
            id: "u1".to_string(),
            parent_id: None,
            role: Role::User,
            timestamp: "2024-06-01T12:00:00Z".to_string(),
            text: "Fix the authentication bug in login.rs".to_string(),
            thinking: None,
            tool_uses: vec![],
            model: None,
            stop_reason: None,
            token_usage: None,
            environment: None,
            delegations: vec![],
            extra: HashMap::new(),
        };

        assert_eq!(
            extract_title_from_turn(&turn, 20),
            Some("Fix the authenticati".to_string())
        );
    }

    #[test]
    fn extract_title_assistant_returns_none() {
        let turn = Turn {
            id: "a1".to_string(),
            parent_id: None,
            role: Role::Assistant,
            timestamp: "2024-06-01T12:00:00Z".to_string(),
            text: "Sure, I can help".to_string(),
            thinking: None,
            tool_uses: vec![],
            model: None,
            stop_reason: None,
            token_usage: None,
            environment: None,
            delegations: vec![],
            extra: HashMap::new(),
        };

        assert_eq!(extract_title_from_turn(&turn, 100), None);
    }

    #[test]
    fn extract_title_empty_text_returns_none() {
        let turn = Turn {
            id: "u1".to_string(),
            parent_id: None,
            role: Role::User,
            timestamp: "2024-06-01T12:00:00Z".to_string(),
            text: "".to_string(),
            thinking: None,
            tool_uses: vec![],
            model: None,
            stop_reason: None,
            token_usage: None,
            environment: None,
            delegations: vec![],
            extra: HashMap::new(),
        };

        assert_eq!(extract_title_from_turn(&turn, 100), None);
    }
}

/// Tests for Turn-based formatting (`format_turn` and `format_turn_with_attribution`).
#[cfg(test)]
mod turn_format_tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;
    use toolpath_convo::{
        DelegatedWork, EnvironmentSnapshot, Role, TokenUsage, ToolCategory, ToolInvocation, Turn,
    };

    use crate::ws::{GlobalStateManager, create_state_broadcast};

    fn make_turn(id: &str, role: Role, text: &str) -> Turn {
        Turn {
            id: id.to_string(),
            parent_id: None,
            role,
            timestamp: "2024-06-01T12:00:00Z".to_string(),
            text: text.to_string(),
            thinking: None,
            tool_uses: vec![],
            model: None,
            stop_reason: None,
            token_usage: None,
            environment: None,
            delegations: vec![],
            extra: HashMap::new(),
        }
    }

    // ── Basic user message ─────────────────────────────────────────

    #[test]
    fn format_turn_user_message() {
        let turn = make_turn("u1", Role::User, "Hello world");
        let result = format_turn(&turn);

        assert_eq!(result["uuid"], "u1");
        assert_eq!(result["role"], "User");
        assert_eq!(result["content"], "Hello world");
        assert_eq!(result["tools"], json!([]));
        assert!(result.get("thinking").is_none());
        assert!(result.get("entry_type").is_none());
    }

    // ── Assistant with thinking and tools ───────────────────────────

    #[test]
    fn format_turn_assistant_with_thinking_and_tools() {
        let mut turn = make_turn("a1", Role::Assistant, "Here is my answer");
        turn.thinking = Some("Let me think...".to_string());
        turn.model = Some("claude-opus-4-6".to_string());
        turn.tool_uses = vec![
            ToolInvocation {
                id: "tu1".to_string(),
                name: "Read".to_string(),
                input: json!({"path": "/foo"}),
                result: Some(toolpath_convo::ToolResult {
                    content: "file contents here".to_string(),
                    is_error: false,
                }),
                category: Some(ToolCategory::FileRead),
            },
            ToolInvocation {
                id: "tu2".to_string(),
                name: "Bash".to_string(),
                input: json!({"command": "ls"}),
                result: Some(toolpath_convo::ToolResult {
                    content: "No such file".to_string(),
                    is_error: true,
                }),
                category: Some(ToolCategory::Shell),
            },
        ];

        let result = format_turn(&turn);

        assert_eq!(result["role"], "Assistant");
        assert_eq!(result["content"], "Here is my answer");
        assert_eq!(result["thinking"], "Let me think...");
        assert_eq!(result["tools"], json!(["Read", "Bash"]));
        assert_eq!(result["model"], "claude-opus-4-6");
        assert_eq!(result["tool_categories"]["Read"], "file_read");
        assert_eq!(result["tool_categories"]["Bash"], "shell");

        // tool_details carries name, input, category, and result
        let details = result["tool_details"].as_array().unwrap();
        assert_eq!(details.len(), 2);
        assert_eq!(details[0]["name"], "Read");
        assert_eq!(details[0]["input"]["path"], "/foo");
        assert_eq!(details[0]["category"], "file_read");
        assert_eq!(details[0]["result"], "file contents here");
        assert!(
            details[0].get("is_error").is_none(),
            "is_error should be omitted when false"
        );
        assert_eq!(details[1]["name"], "Bash");
        assert_eq!(details[1]["input"]["command"], "ls");
        assert_eq!(details[1]["category"], "shell");
        assert_eq!(details[1]["result"], "No such file");
        assert_eq!(details[1]["is_error"], true);
    }

    // ── System message includes entry_type ──────────────────────────

    #[test]
    fn format_turn_system_message() {
        let turn = make_turn("s1", Role::System, "System prompt");
        let result = format_turn(&turn);

        assert_eq!(result["role"], "System");
        assert_eq!(result["content"], "System prompt");
        assert_eq!(result["entry_type"], "system");
        assert_eq!(result["tools"], json!([]));
    }

    // ── Environment snapshot ────────────────────────────────────────

    #[test]
    fn format_turn_with_environment() {
        let mut turn = make_turn("u1", Role::User, "Hello");
        turn.environment = Some(EnvironmentSnapshot {
            working_dir: Some("/project/path".to_string()),
            vcs_branch: Some("feat/auth".to_string()),
            vcs_revision: None,
        });

        let result = format_turn(&turn);

        let env = &result["environment"];
        assert_eq!(env["working_dir"], "/project/path");
        assert_eq!(env["vcs_branch"], "feat/auth");
        assert!(env["vcs_revision"].is_null());
    }

    // ── Delegated work ──────────────────────────────────────────────

    #[test]
    fn format_turn_with_delegations() {
        let mut turn = make_turn("a1", Role::Assistant, "Delegating...");
        turn.delegations = vec![DelegatedWork {
            agent_id: "task-1".to_string(),
            prompt: "Find the bug".to_string(),
            turns: vec![],
            result: Some("Found it in auth.rs".to_string()),
        }];

        let result = format_turn(&turn);

        let delegations = result["delegations"].as_array().unwrap();
        assert_eq!(delegations.len(), 1);
        assert_eq!(delegations[0]["agent_id"], "task-1");
        assert_eq!(delegations[0]["prompt"], "Find the bug");
        assert_eq!(delegations[0]["result"], "Found it in auth.rs");
    }

    // ── Delegation prompt truncation ────────────────────────────────

    #[test]
    fn format_turn_delegation_prompt_truncated() {
        let long_prompt = "p".repeat(300);
        let mut turn = make_turn("a1", Role::Assistant, "Delegating...");
        turn.delegations = vec![DelegatedWork {
            agent_id: "task-1".to_string(),
            prompt: long_prompt,
            turns: vec![],
            result: None,
        }];

        let result = format_turn(&turn);

        let prompt = result["delegations"][0]["prompt"].as_str().unwrap();
        assert!(prompt.ends_with("..."));
        assert_eq!(prompt.len(), 203); // 200 + "..."
    }

    // ── Whitespace normalization ─────────────────────────────────────

    #[test]
    fn format_turn_whitespace_normalization() {
        let turn = make_turn("u1", Role::User, "Hello\n\n\n\nWorld\n\n\n");
        let result = format_turn(&turn);

        assert_eq!(result["content"], "Hello\n\nWorld");
    }

    // ── Other role ──────────────────────────────────────────────────

    #[test]
    fn format_turn_other_role() {
        let turn = make_turn("x1", Role::Other("CustomRole".into()), "Custom content");
        let result = format_turn(&turn);

        assert_eq!(result["role"], "CustomRole");
    }

    // ── Token usage preserved in turn ────────────────────────────────

    #[test]
    fn format_turn_no_token_usage_in_output() {
        let mut turn = make_turn("a1", Role::Assistant, "Hello");
        turn.token_usage = Some(TokenUsage {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: Some(200),
            cache_write_tokens: None,
        });

        let result = format_turn(&turn);
        assert_eq!(result["role"], "Assistant");
    }

    // ── Attribution: queue match works with Turn ────────────────────

    #[tokio::test]
    async fn format_turn_with_attribution_queue_match() {
        let sm = Arc::new(GlobalStateManager::new(create_state_broadcast()));
        sm.push_pending_attribution("i1", "u1".into(), "Alice".into(), "fix the bug", None)
            .await;

        let turn = make_turn("e1", Role::User, "fix the bug");
        let result =
            format_turn_with_attribution(&turn, "i1", None, Some(sm.pending_attributions_lock()))
                .await;

        assert_eq!(result["attributed_to"]["user_id"], "u1");
        assert_eq!(result["attributed_to"]["display_name"], "Alice");
    }

    // ── Attribution: assistant turns are skipped ─────────────────────

    #[tokio::test]
    async fn format_turn_with_attribution_assistant_skips() {
        let sm = Arc::new(GlobalStateManager::new(create_state_broadcast()));
        sm.push_pending_attribution("i1", "u1".into(), "Alice".into(), "anything", None)
            .await;

        let turn = make_turn("a1", Role::Assistant, "Sure, I can help");
        let result =
            format_turn_with_attribution(&turn, "i1", None, Some(sm.pending_attributions_lock()))
                .await;

        assert!(result.get("attributed_to").is_none());
    }

    // ── Attribution: no repo/state → unattributed ────────────────────

    #[tokio::test]
    async fn format_turn_with_attribution_no_repo_no_state() {
        let turn = make_turn("e1", Role::User, "hello");
        let result = format_turn_with_attribution(&turn, "i1", None, None).await;

        assert!(result.get("attributed_to").is_none());
    }

    // ── Attribution: task_id propagated ──────────────────────────────

    #[tokio::test]
    async fn format_turn_with_attribution_task_id_propagated() {
        let sm = Arc::new(GlobalStateManager::new(create_state_broadcast()));
        sm.push_pending_attribution("i1", "u1".into(), "Alice".into(), "do the thing", Some(42))
            .await;

        let turn = make_turn("e1", Role::User, "do the thing");
        let result =
            format_turn_with_attribution(&turn, "i1", None, Some(sm.pending_attributions_lock()))
                .await;

        assert_eq!(result["attributed_to"]["user_id"], "u1");
        assert_eq!(result["task_id"], 42);
    }
}
