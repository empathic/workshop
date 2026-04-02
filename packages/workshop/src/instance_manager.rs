use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

use tracing::{debug, info, warn};

use crate::inference::ClaudeState;
use crate::instance_actor::{InstanceHandle, SpawnOptions, create_instance};
use crate::process_driver::ProcessDriver;
use crate::repository::ConversationRepository;
use crate::ws::{FirstInputData, PendingAttribution, StateBroadcast};

/// Whether an instance is a structured conversation provider (e.g. Claude, Codex)
/// or an unstructured terminal session (e.g. bash, zsh).
///
/// Computed by the backend at creation time and sent in the wire protocol.
/// Frontend never guesses — it reads `kind` from the instance payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum InstanceKind {
    Structured { provider: String },
    Unstructured { label: Option<String> },
}

impl InstanceKind {
    pub fn is_structured(&self) -> bool {
        matches!(self, InstanceKind::Structured { .. })
    }

    pub fn infer(command: &str) -> Self {
        if command.contains("claude") {
            InstanceKind::Structured {
                provider: "claude".into(),
            }
        } else {
            let basename = command
                .split_whitespace()
                .next()
                .and_then(|w| std::path::Path::new(w).file_name())
                .and_then(|f| f.to_str())
                .unwrap_or(command);
            InstanceKind::Unstructured {
                label: Some(basename.into()),
            }
        }
    }
}

// This is for API compatibility - we'll remove the fake "port" concept later
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeInstance {
    pub id: String,
    pub name: String,
    /// User-set display name. Falls back to `name` if None.
    pub custom_name: Option<String>,
    pub wrapper_port: u16, // This is fake now - just for UI compatibility
    pub working_dir: String,
    pub command: String,
    pub kind: InstanceKind,
    pub running: bool,
    pub created_at: String,
    /// The Claude conversation session ID (detected after instance starts)
    pub session_id: Option<String>,
    /// Current Claude state (for status indicator in sidebar)
    pub claude_state: Option<ClaudeState>,
    /// Unix timestamp (seconds) when the current state was entered
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_entered_at: Option<i64>,
}

pub struct InstanceManager {
    instances: RwLock<HashMap<String, InstanceHandle>>,
    claude_path: String,
    used_names: RwLock<HashSet<String>>,
    base_directory: String,
    max_buffer_bytes: usize,
    scrollback_lines: usize,
    vt_record_dir: Option<std::path::PathBuf>,
}

impl InstanceManager {
    pub fn new(
        claude_path: String,
        _base_port: u16,
        max_buffer_bytes: usize,
        scrollback_lines: usize,
        vt_record_dir: Option<std::path::PathBuf>,
    ) -> Self {
        let base_directory = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
            .to_string_lossy()
            .to_string();

        Self {
            instances: RwLock::new(HashMap::new()),
            claude_path,
            used_names: RwLock::new(HashSet::new()),
            base_directory,
            max_buffer_bytes,
            scrollback_lines,
            vt_record_dir,
        }
    }

    /// The default command (path to claude binary or shell).
    pub fn default_command(&self) -> &str {
        &self.claude_path
    }

    fn generate_unique_name(&self) -> String {
        use rand::Rng;

        // Word lists for generating random names
        const ADJECTIVES: &[&str] = &[
            "swift", "bright", "calm", "eager", "gentle", "happy", "keen", "lively", "noble",
            "proud", "quiet", "rapid", "sharp", "smooth", "bold", "brave", "clever", "daring",
            "fearless", "graceful", "honest", "jolly", "kind", "merry", "patient", "polite",
            "steady", "trusty", "wise", "zealous", "cosmic", "stellar", "quantum", "cyber",
            "digital", "virtual", "binary",
        ];

        const COLORS: &[&str] = &[
            "amber", "azure", "crimson", "coral", "emerald", "golden", "indigo", "jade", "lilac",
            "magenta", "maroon", "olive", "pearl", "ruby", "sapphire", "scarlet", "silver", "teal",
            "violet", "bronze", "cobalt", "copper", "ivory", "onyx", "opal", "rose", "sage",
            "slate", "topaz", "winter",
        ];

        const NOUNS: &[&str] = &[
            "falcon", "tiger", "eagle", "wolf", "fox", "bear", "lion", "hawk", "raven", "phoenix",
            "dragon", "griffin", "sphinx", "pegasus", "kraken", "comet", "nebula", "quasar",
            "pulsar", "galaxy", "nova", "cosmos", "cipher", "matrix", "nexus", "prism", "beacon",
            "forge", "oracle", "mute",
        ];

        let mut rng = rand::thread_rng();
        let adj_idx = rng.gen_range(0..ADJECTIVES.len());
        let color_idx = rng.gen_range(0..COLORS.len());
        let noun_idx = rng.gen_range(0..NOUNS.len());

        format!(
            "{}-{}-{}",
            ADJECTIVES[adj_idx], COLORS[color_idx], NOUNS[noun_idx]
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        name: Option<String>,
        working_dir: Option<String>,
        command: Option<String>,
        driver: Box<dyn ProcessDriver>,
        state_broadcast_tx: Option<StateBroadcast>,
        lifecycle_tx: Option<broadcast::Sender<crate::ws::ServerMessage>>,
        claimed_sessions: Arc<RwLock<HashMap<String, String>>>,
        first_input_data: Arc<RwLock<HashMap<String, FirstInputData>>>,
        pending_attributions: Arc<RwLock<HashMap<String, VecDeque<PendingAttribution>>>>,
        repository: Option<Arc<ConversationRepository>>,
        kind: Option<InstanceKind>,
    ) -> Result<ClaudeInstance> {
        // Generate unique name if not provided
        let name = if let Some(provided_name) = name {
            // Add provided name to used names
            self.used_names.write().await.insert(provided_name.clone());
            provided_name
        } else {
            // Keep trying until we get a unique name
            loop {
                let generated_name = self.generate_unique_name();
                let mut used = self.used_names.write().await;
                if !used.contains(&generated_name) {
                    used.insert(generated_name.clone());
                    break generated_name;
                }
            }
        };

        // Use provided working_dir or fall back to base_directory
        let working_dir = working_dir.unwrap_or_else(|| self.base_directory.clone());

        // Use provided command or fall back to claude_path
        let command_line = command.unwrap_or_else(|| self.claude_path.clone());

        // For complex commands, use shell to handle them
        // This allows things like "pnpm run claude" or "npm exec claude" to work
        let (program, args) = if command_line.contains(' ') {
            // Complex command - run through shell
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
            (shell, vec!["-c".to_string(), command_line.clone()])
        } else {
            // Simple command - try to resolve its full path
            let resolved_path = if command_line.starts_with('/') {
                // Already an absolute path
                command_line.clone()
            } else {
                // Try to resolve using which
                match std::process::Command::new("which")
                    .arg(&command_line)
                    .output()
                {
                    Ok(output) if output.status.success() => {
                        String::from_utf8_lossy(&output.stdout).trim().to_string()
                    }
                    _ => command_line.clone(), // Use as-is if which fails
                }
            };
            (resolved_path, Vec::new())
        };

        let kind = kind.unwrap_or_else(|| InstanceKind::infer(&command_line));

        info!(
            "Creating instance '{}' with command '{}' (program: '{}' args: {:?})",
            name, command_line, program, args
        );

        // Create the instance actor - pass both display command and actual command
        let handle = create_instance(SpawnOptions {
            name: name.clone(),
            display_command: command_line.clone(),
            actual_command: program,
            args,
            working_dir: working_dir.clone(),
            kind: kind.clone(),
            max_buffer_bytes: self.max_buffer_bytes,
            scrollback_lines: self.scrollback_lines,
            vt_record_dir: self.vt_record_dir.clone(),
            driver,
            state_broadcast_tx,
            lifecycle_tx,
            claimed_sessions,
            first_input_data,
            pending_attributions,
            repository,
        })
        .await?;

        let info = handle.get_info().await;
        let id = info.id.clone();
        let created_at = info.created_at.clone();

        // Store the handle
        let mut instances = self.instances.write().await;
        instances.insert(id.clone(), handle);

        debug!("Instance '{}' created successfully", name);

        Ok(ClaudeInstance {
            id: id.clone(),
            name: name.clone(),
            custom_name: None,
            wrapper_port: 0, // Fake port for backward compatibility
            working_dir,
            command: command_line,
            kind,
            running: true,
            created_at,
            session_id: None, // Will be detected when conversation is accessed
            claude_state: None,
            state_entered_at: None,
        })
    }

    pub async fn list(&self) -> Vec<ClaudeInstance> {
        let instances = self.instances.read().await;
        let mut list = Vec::new();

        for handle in instances.values() {
            let info = handle.get_info().await;
            list.push(ClaudeInstance {
                id: info.id,
                name: info.name,
                custom_name: info.custom_name,
                wrapper_port: 0, // Fake port
                working_dir: info.working_dir,
                command: info.command,
                kind: info.kind,
                running: info.running,
                created_at: info.created_at,
                session_id: info.session_id,
                claude_state: info.claude_state,
                state_entered_at: None, // Populated by handler from GlobalStateManager
            });
        }

        list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        list
    }

    pub async fn get(&self, id: &str) -> Option<ClaudeInstance> {
        let instances = self.instances.read().await;
        if let Some(handle) = instances.get(id) {
            let info = handle.get_info().await;
            Some(ClaudeInstance {
                id: info.id,
                name: info.name,
                custom_name: info.custom_name,
                wrapper_port: 0, // Fake port
                working_dir: info.working_dir,
                command: info.command,
                kind: info.kind,
                running: info.running,
                created_at: info.created_at,
                session_id: info.session_id,
                claude_state: info.claude_state,
                state_entered_at: None, // Populated by handler from GlobalStateManager
            })
        } else {
            None
        }
    }

    pub async fn get_handle(&self, id: &str) -> Option<InstanceHandle> {
        let instances = self.instances.read().await;
        instances.get(id).cloned()
    }

    pub async fn set_custom_name(&self, id: &str, name: Option<String>) -> Result<()> {
        let instances = self.instances.read().await;
        let handle = instances
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("Instance not found"))?;
        handle.set_custom_name(name).await
    }

    pub async fn stop(&self, id: &str) -> bool {
        debug!("Stopping instance {}", id);

        let mut instances = self.instances.write().await;
        if let Some(handle) = instances.remove(id) {
            let info = handle.get_info().await;

            // Remove name from used names
            self.used_names.write().await.remove(&info.name);

            // Stop the actor
            if let Err(e) = handle.stop().await {
                warn!("Error stopping instance: {}", e);
            }

            true
        } else {
            false
        }
    }

    #[allow(dead_code)]
    pub async fn cleanup(&self) {
        debug!("Cleaning up all instances");

        let instances = self.instances.read().await;
        let ids: Vec<String> = instances.keys().cloned().collect();
        drop(instances);

        for id in ids {
            self.stop(&id).await;
        }
    }
}

impl Drop for InstanceManager {
    fn drop(&mut self) {
        debug!("InstanceManager dropping - cleanup may be required for running instances");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn test_manager() -> InstanceManager {
        InstanceManager::new("claude".to_string(), 0, 1024 * 1024, 0, None)
    }

    #[test]
    fn generate_unique_name_format() {
        let mgr = test_manager();
        let name = mgr.generate_unique_name();
        let parts: Vec<&str> = name.split('-').collect();
        assert_eq!(
            parts.len(),
            3,
            "Name should be adj-color-noun, got: {}",
            name
        );
    }

    #[test]
    fn generate_unique_name_lowercase_ascii() {
        let mgr = test_manager();
        for _ in 0..50 {
            let name = mgr.generate_unique_name();
            for c in name.chars() {
                assert!(
                    c.is_ascii_lowercase() || c == '-',
                    "Expected lowercase ASCII or hyphen, got '{}' in '{}'",
                    c,
                    name
                );
            }
        }
    }

    #[test]
    fn generate_unique_name_variety() {
        let mgr = test_manager();
        let names: HashSet<String> = (0..100).map(|_| mgr.generate_unique_name()).collect();
        assert!(
            names.len() > 10,
            "Expected >10 unique names from 100 calls, got {}",
            names.len()
        );
    }

    #[test]
    fn generate_unique_name_components_from_word_lists() {
        let adjectives: HashSet<&str> = [
            "swift", "bright", "calm", "eager", "gentle", "happy", "keen", "lively", "noble",
            "proud", "quiet", "rapid", "sharp", "smooth", "bold", "brave", "clever", "daring",
            "fearless", "graceful", "honest", "jolly", "kind", "merry", "patient", "polite",
            "steady", "trusty", "wise", "zealous", "cosmic", "stellar", "quantum", "cyber",
            "digital", "virtual", "binary",
        ]
        .into_iter()
        .collect();
        let colors: HashSet<&str> = [
            "amber", "azure", "crimson", "coral", "emerald", "golden", "indigo", "jade", "lilac",
            "magenta", "maroon", "olive", "pearl", "ruby", "sapphire", "scarlet", "silver", "teal",
            "violet", "bronze", "cobalt", "copper", "ivory", "onyx", "opal", "rose", "sage",
            "slate", "topaz", "winter",
        ]
        .into_iter()
        .collect();
        let nouns: HashSet<&str> = [
            "falcon", "tiger", "eagle", "wolf", "fox", "bear", "lion", "hawk", "raven", "phoenix",
            "dragon", "griffin", "sphinx", "pegasus", "kraken", "comet", "nebula", "quasar",
            "pulsar", "galaxy", "nova", "cosmos", "cipher", "matrix", "nexus", "prism", "beacon",
            "forge", "oracle", "mute",
        ]
        .into_iter()
        .collect();

        let mgr = test_manager();
        for _ in 0..50 {
            let name = mgr.generate_unique_name();
            let parts: Vec<&str> = name.split('-').collect();
            assert!(
                adjectives.contains(parts[0]),
                "Unknown adjective: {}",
                parts[0]
            );
            assert!(colors.contains(parts[1]), "Unknown color: {}", parts[1]);
            assert!(nouns.contains(parts[2]), "Unknown noun: {}", parts[2]);
        }
    }

    #[test]
    fn instance_kind_infer_claude() {
        let kind = InstanceKind::infer("claude");
        assert!(kind.is_structured());
        assert_eq!(
            kind,
            InstanceKind::Structured {
                provider: "claude".into()
            }
        );
    }

    #[test]
    fn instance_kind_infer_shell() {
        let kind = InstanceKind::infer("bash");
        assert!(!kind.is_structured());
        assert_eq!(
            kind,
            InstanceKind::Unstructured {
                label: Some("bash".into())
            }
        );
    }

    #[test]
    fn instance_kind_infer_complex_command() {
        let kind = InstanceKind::infer("/usr/bin/zsh -l");
        assert!(!kind.is_structured());
        assert_eq!(
            kind,
            InstanceKind::Unstructured {
                label: Some("zsh".into())
            }
        );
    }

    #[test]
    fn instance_kind_infer_claude_in_path() {
        let kind = InstanceKind::infer("pnpm run claude");
        assert!(kind.is_structured());
    }

    #[test]
    fn instance_kind_serde_roundtrip() {
        let structured = InstanceKind::Structured {
            provider: "claude".into(),
        };
        let json = serde_json::to_value(&structured).unwrap();
        assert_eq!(json["type"], "Structured");
        assert_eq!(json["provider"], "claude");
        let rt: InstanceKind = serde_json::from_value(json).unwrap();
        assert_eq!(rt, structured);

        let unstructured = InstanceKind::Unstructured {
            label: Some("bash".into()),
        };
        let json = serde_json::to_value(&unstructured).unwrap();
        assert_eq!(json["type"], "Unstructured");
        assert_eq!(json["label"], "bash");
        let rt: InstanceKind = serde_json::from_value(json).unwrap();
        assert_eq!(rt, unstructured);
    }

    #[test]
    fn claude_instance_serde() {
        let inst = ClaudeInstance {
            id: "inst-1".to_string(),
            name: "swift-azure-falcon".to_string(),
            custom_name: Some("My Instance".to_string()),
            wrapper_port: 0,
            working_dir: "/tmp".to_string(),
            command: "claude".to_string(),
            kind: InstanceKind::Structured {
                provider: "claude".into(),
            },
            running: true,
            created_at: "2025-01-01T00:00:00Z".to_string(),
            session_id: Some("sess-abc".to_string()),
            claude_state: None,
            state_entered_at: None,
        };
        let json = serde_json::to_value(&inst).unwrap();
        assert_eq!(json["id"], "inst-1");
        assert_eq!(json["name"], "swift-azure-falcon");
        assert_eq!(json["custom_name"], "My Instance");
        assert_eq!(json["running"], true);
        assert_eq!(json["session_id"], "sess-abc");
        let rt: ClaudeInstance = serde_json::from_value(json).unwrap();
        assert_eq!(rt.id, "inst-1");
        assert_eq!(rt.custom_name, Some("My Instance".to_string()));
    }

    #[test]
    fn claude_instance_none_fields() {
        let inst = ClaudeInstance {
            id: "i".to_string(),
            name: "n".to_string(),
            custom_name: None,
            wrapper_port: 0,
            working_dir: "/tmp".to_string(),
            command: "echo".to_string(),
            kind: InstanceKind::Unstructured {
                label: Some("echo".into()),
            },
            running: false,
            created_at: "2025-01-01T00:00:00Z".to_string(),
            session_id: None,
            claude_state: None,
            state_entered_at: None,
        };
        let json = serde_json::to_value(&inst).unwrap();
        assert!(json["custom_name"].is_null());
        assert!(json["session_id"].is_null());
        assert!(json["claude_state"].is_null());
        assert_eq!(json["running"], false);
    }

    #[test]
    fn generate_unique_name_nonempty_parts() {
        let mgr = test_manager();
        let name = mgr.generate_unique_name();
        for part in name.split('-') {
            assert!(
                !part.is_empty(),
                "Name part should not be empty in '{}'",
                name
            );
        }
    }
}
