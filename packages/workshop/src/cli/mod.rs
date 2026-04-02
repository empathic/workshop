pub mod attach;
pub mod auth;
pub mod daemon;
pub mod picker;
pub mod settings;
pub mod terminal;

use anyhow::{Context, Result};
use futures::StreamExt;
use serde::Deserialize;
use tokio_tungstenite::tungstenite;
use tracing::{debug, error, info, warn};

use attach::AttachOutcome;
use daemon::{DaemonError, DaemonInfo};
use picker::{PickerEvent, PickerResult};
use workshop::config::WorkshopConfig;

/// Default command: ensure daemon, show picker if instances exist, else create new.
/// After detaching from a session, returns to the picker.
pub async fn default_command(config: &WorkshopConfig) -> Result<()> {
    let daemon = daemon::ensure_daemon(config).await?;

    // First run: if no instances at all, create one directly
    let instances = match fetch_instances(&daemon).await {
        Ok(inst) => inst,
        Err(DaemonError::Unavailable) => {
            eprintln!("[work: server stopped]");
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };
    if instances.is_empty() {
        let cwd = std::env::current_dir()
            .context("Failed to get current directory")?
            .to_string_lossy()
            .to_string();
        let instance = match create_instance(&daemon, None, Some(&cwd)).await {
            Ok(inst) => inst,
            Err(DaemonError::Unavailable) => {
                eprintln!("[work: server stopped]");
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };
        match attach::attach(&daemon, &instance.id).await {
            Ok(AttachOutcome::Detached) => {}
            Ok(AttachOutcome::Exited) => {
                delete_instance(&daemon, &instance.id).await;
            }
            Err(DaemonError::Unavailable) => {
                eprintln!("[work: server stopped]");
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        }
    }

    session_loop(config, daemon).await
}

/// Attach to an existing instance (by name, ID, or prefix). No target: show picker.
/// After detaching from a session, returns to the picker.
pub async fn attach_command(config: &WorkshopConfig, target: Option<String>) -> Result<()> {
    let daemon = daemon::require_running_daemon(config).await?;

    if let Some(t) = target {
        let instance_id = resolve_instance(&daemon, &t).await?;
        match attach::attach(&daemon, &instance_id).await {
            Ok(AttachOutcome::Detached) => return Ok(()),
            Ok(AttachOutcome::Exited) => {
                delete_instance(&daemon, &instance_id).await;
                if should_stop_daemon(&daemon).await {
                    daemon::stop_daemon(&daemon);
                }
                return Ok(());
            }
            Err(DaemonError::Unavailable) => {
                eprintln!("[work: server stopped]");
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        }
    }

    session_loop(config, daemon).await
}

/// Picker → attach → detach → picker loop. Exits on Quit or when no instances remain.
/// Owns the ratatui terminal so picker and settings can share it.
async fn session_loop(config: &WorkshopConfig, daemon: DaemonInfo) -> Result<()> {
    use std::io::IsTerminal;

    let has_tty = std::io::stdin().is_terminal();

    // Initialise ratatui terminal once; picker + settings share it.
    // attach() manages its own ratatui terminal internally, so we restore
    // the picker's terminal before attaching and re-init afterwards.
    let mut terminal = if has_tty { Some(ratatui::init()) } else { None };

    let result = session_loop_inner(&mut terminal, config, daemon).await;

    if terminal.is_some() {
        ratatui::restore();
    }
    result
}

async fn session_loop_inner(
    terminal: &mut Option<ratatui::DefaultTerminal>,
    config: &WorkshopConfig,
    mut daemon: DaemonInfo,
) -> Result<()> {
    /// Try to rediscover the daemon after an Unavailable error.
    /// Returns `true` if a new daemon was found and `daemon` was updated.
    async fn try_rediscover(config: &WorkshopConfig, daemon: &mut DaemonInfo) -> bool {
        if let Some(new) = daemon::rediscover_daemon(config).await {
            *daemon = new;
            true
        } else {
            false
        }
    }

    let mut last_attached: Option<String> = None;

    loop {
        debug!("session_loop: fetching instances");
        let instances = match fetch_instances(&daemon).await {
            Ok(inst) => {
                debug!(count = inst.len(), "session_loop: got instances");
                inst
            }
            Err(DaemonError::Unavailable) => {
                warn!("session_loop: daemon unavailable, attempting rediscovery");
                if try_rediscover(config, &mut daemon).await {
                    continue;
                }
                eprintln!("[work: server stopped]");
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };

        let (instance_id, outcome) =
            match run_live_picker(terminal, &daemon, instances, last_attached.as_deref()).await? {
                PickerResult::Attach(id) => {
                    info!(instance_id = %id, "attaching to instance");
                    // Restore terminal before attaching (attach manages its own ratatui terminal)
                    if terminal.is_some() {
                        ratatui::restore();
                    }
                    let outcome = match attach::attach(&daemon, &id).await {
                        Ok(o) => o,
                        Err(DaemonError::Unavailable) => {
                            if terminal.is_some() {
                                *terminal = Some(ratatui::init());
                            }
                            if try_rediscover(config, &mut daemon).await {
                                continue;
                            }
                            eprintln!("[work: server stopped]");
                            return Ok(());
                        }
                        Err(e) => {
                            if terminal.is_some() {
                                *terminal = Some(ratatui::init());
                            }
                            return Err(e.into());
                        }
                    };
                    // Re-init terminal for the next picker iteration
                    if terminal.is_some() {
                        *terminal = Some(ratatui::init());
                    }
                    (id, outcome)
                }
                PickerResult::NewInstance => {
                    if terminal.is_some() {
                        ratatui::restore();
                    }
                    let cwd = std::env::current_dir()
                        .context("Failed to get current directory")?
                        .to_string_lossy()
                        .to_string();
                    let instance = match create_instance(&daemon, None, Some(&cwd)).await {
                        Ok(inst) => inst,
                        Err(DaemonError::Unavailable) => {
                            if terminal.is_some() {
                                *terminal = Some(ratatui::init());
                            }
                            if try_rediscover(config, &mut daemon).await {
                                continue;
                            }
                            eprintln!("[work: server stopped]");
                            return Ok(());
                        }
                        Err(e) => {
                            if terminal.is_some() {
                                *terminal = Some(ratatui::init());
                            }
                            return Err(e.into());
                        }
                    };
                    let outcome = match attach::attach(&daemon, &instance.id).await {
                        Ok(o) => o,
                        Err(DaemonError::Unavailable) => {
                            if terminal.is_some() {
                                *terminal = Some(ratatui::init());
                            }
                            if try_rediscover(config, &mut daemon).await {
                                continue;
                            }
                            eprintln!("[work: server stopped]");
                            return Ok(());
                        }
                        Err(e) => {
                            if terminal.is_some() {
                                *terminal = Some(ratatui::init());
                            }
                            return Err(e.into());
                        }
                    };
                    if terminal.is_some() {
                        *terminal = Some(ratatui::init());
                    }
                    (instance.id, outcome)
                }
                PickerResult::Rename { id, custom_name } => {
                    info!(instance_id = %id, name = ?custom_name, "renaming instance");
                    rename_instance(&daemon, &id, custom_name.as_deref()).await;
                    continue;
                }
                PickerResult::Kill(id) => {
                    info!(instance_id = %id, "killing instance");
                    delete_instance(&daemon, &id).await;
                    continue;
                }
                PickerResult::KillServer => {
                    daemon::stop_daemon(&daemon);
                    return Ok(());
                }
                PickerResult::Settings => {
                    if let Some(term) = terminal {
                        // run_settings uses reqwest::blocking which creates its own
                        // tokio runtime. block_in_place lets it run without panicking
                        // from inside our existing runtime.
                        tokio::task::block_in_place(|| settings::run_settings(term, &daemon))?;
                    }
                    continue;
                }
                PickerResult::Quit => return Ok(()),
            };

        match outcome {
            AttachOutcome::Detached => {
                info!(instance_id = %instance_id, "detached from instance");
                last_attached = Some(instance_id);
            }
            AttachOutcome::Exited => {
                info!(instance_id = %instance_id, "instance exited, cleaning up");
                delete_instance(&daemon, &instance_id).await;
                last_attached = None;
            }
        }
    }
}

/// Connect to the mux WebSocket for live updates and run the picker.
async fn run_live_picker(
    terminal: &mut Option<ratatui::DefaultTerminal>,
    daemon: &DaemonInfo,
    instances: Vec<InstanceInfo>,
    selected_id: Option<&str>,
) -> Result<PickerResult> {
    let (tx, rx) = std::sync::mpsc::channel();

    // Best-effort WS connection for live updates; picker works fine without it
    if let Ok((ws, _)) = tokio_tungstenite::connect_async(daemon.mux_ws_url()).await {
        debug!("picker: mux WebSocket connected");
        let (_, mut ws_read) = ws.split();
        tokio::spawn(async move {
            while let Some(Ok(tungstenite::Message::Text(text))) = ws_read.next().await {
                if let Ok(ev) = serde_json::from_str::<WsLifecycleEvent>(&text) {
                    let picker_ev = match ev {
                        WsLifecycleEvent::Created { instance } => PickerEvent::Created(instance),
                        WsLifecycleEvent::Stopped { instance_id } => {
                            PickerEvent::Stopped(instance_id)
                        }
                        WsLifecycleEvent::Renamed {
                            instance_id,
                            custom_name,
                        } => PickerEvent::Renamed {
                            instance_id,
                            custom_name,
                        },
                    };
                    if tx.send(picker_ev).is_err() {
                        break; // picker dropped the receiver
                    }
                }
                // Silently ignore all other message types
            }
        });
    }

    picker::run_picker(terminal, &daemon.base_url(), instances, rx, selected_id)
}

/// Subset of server messages we care about in the CLI picker.
#[derive(Deserialize)]
#[serde(tag = "type")]
enum WsLifecycleEvent {
    #[serde(rename = "InstanceCreated")]
    Created { instance: InstanceInfo },
    #[serde(rename = "InstanceStopped")]
    Stopped { instance_id: String },
    #[serde(rename = "InstanceRenamed")]
    Renamed {
        instance_id: String,
        custom_name: Option<String>,
    },
}

/// Kill a specific session by name, ID, or prefix.
pub async fn kill_command(config: &WorkshopConfig, target: &str) -> Result<()> {
    let daemon = daemon::require_running_daemon(config).await?;
    let instance_id = resolve_instance(&daemon, target).await?;
    delete_instance(&daemon, &instance_id).await;
    eprintln!(
        "Killed session {}",
        &instance_id[..8.min(instance_id.len())]
    );
    if should_stop_daemon(&daemon).await {
        daemon::stop_daemon(&daemon);
        eprintln!("No sessions remaining, daemon stopped.");
    }
    Ok(())
}

/// Stop the daemon and all sessions.
pub async fn kill_server_command(config: &WorkshopConfig, force: bool) -> Result<()> {
    let daemon = daemon::require_running_daemon(config).await?;

    if !force {
        let instances = fetch_instances(&daemon).await?;
        let running = instances.iter().filter(|i| i.running).count();
        if running > 0 {
            eprint!(
                "Kill daemon and {} running session{}? (y/N) ",
                running,
                if running == 1 { "" } else { "s" }
            );
            use std::io::Write;
            std::io::stderr().flush()?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if !input.trim().eq_ignore_ascii_case("y") {
                eprintln!("Cancelled.");
                return Ok(());
            }
        }
    }

    daemon::stop_daemon(&daemon);
    eprintln!("Daemon stopped.");
    Ok(())
}

/// List running instances.
pub async fn list_command(config: &WorkshopConfig, json: bool) -> Result<()> {
    let daemon = daemon::ensure_daemon(config).await?;

    let instances = fetch_instances(&daemon).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&instances)?);
    } else if instances.is_empty() {
        println!("No running instances.");
    } else {
        // Table header
        println!("{:<38} {:<20} {:<8} WORKING DIR", "ID", "NAME", "STATUS");
        println!("{}", "-".repeat(100));
        for inst in &instances {
            let status = if inst.running { "running" } else { "stopped" };
            // Show short ID (first 8 chars)
            let short_id = if inst.id.len() > 8 {
                &inst.id[..8]
            } else {
                &inst.id
            };
            println!(
                "{:<38} {:<20} {:<8} {}",
                short_id,
                inst.display_name(),
                status,
                inst.working_dir
            );
        }
        println!("\n{} instance(s)", instances.len());
    }

    Ok(())
}

#[derive(Deserialize, serde::Serialize, Clone)]
pub struct InstanceInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub custom_name: Option<String>,
    pub running: bool,
    pub working_dir: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub created_at: String,
}

impl InstanceInfo {
    /// Display name: custom_name if set, otherwise auto-generated name.
    pub fn display_name(&self) -> &str {
        self.custom_name.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct CreateInstanceResponse {
    id: String,
    name: String,
}

/// Delete a stopped instance from the daemon. Best-effort (logs errors).
async fn delete_instance(daemon: &DaemonInfo, instance_id: &str) {
    let url = format!("{}/api/instances/{}", daemon.base_url(), instance_id);
    if let Err(e) = reqwest::Client::new().delete(&url).send().await {
        error!(instance_id, error = %e, "failed to delete instance");
    }
}

/// Set or clear the custom name for an instance. Best-effort (logs errors).
async fn rename_instance(daemon: &DaemonInfo, instance_id: &str, custom_name: Option<&str>) {
    let url = format!("{}/api/instances/{}/name", daemon.base_url(), instance_id);
    if let Err(e) = reqwest::Client::new()
        .patch(&url)
        .json(&serde_json::json!({ "custom_name": custom_name }))
        .send()
        .await
    {
        error!(instance_id, name = ?custom_name, error = %e, "failed to rename instance");
    }
}

/// Check if the daemon has no remaining running instances.
async fn should_stop_daemon(daemon: &DaemonInfo) -> bool {
    match fetch_instances(daemon).await {
        Ok(instances) => instances.is_empty() || instances.iter().all(|i| !i.running),
        Err(_) => false,
    }
}

async fn fetch_instances(daemon: &DaemonInfo) -> Result<Vec<InstanceInfo>, DaemonError> {
    let url = format!("{}/api/instances", daemon.base_url());
    let resp = reqwest::get(&url)
        .await
        .map_err(DaemonError::from_reqwest)?;
    resp.json().await.map_err(DaemonError::from_reqwest)
}

async fn create_instance(
    daemon: &DaemonInfo,
    name: Option<&str>,
    working_dir: Option<&str>,
) -> Result<CreateInstanceResponse, DaemonError> {
    let url = format!("{}/api/instances", daemon.base_url());
    let body = serde_json::json!({
        "name": name,
        "working_dir": working_dir,
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(DaemonError::from_reqwest)?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Failed to create instance: {} {}", status, text).into());
    }

    resp.json().await.map_err(DaemonError::from_reqwest)
}

async fn resolve_instance(daemon: &DaemonInfo, target: &str) -> Result<String> {
    let instances = fetch_instances(daemon).await?;
    match_instance(&instances, target)
}

/// Pure matching logic: resolve an instance target against a list of instances.
/// Tries exact ID match, then name match, then ID prefix match.
fn match_instance(instances: &[InstanceInfo], target: &str) -> Result<String> {
    if instances.is_empty() {
        anyhow::bail!("No running instances. Use `work` to create one.");
    }

    // Try exact ID match
    if let Some(inst) = instances.iter().find(|i| i.id == target) {
        return Ok(inst.id.clone());
    }
    // Try custom_name match
    if let Some(inst) = instances
        .iter()
        .find(|i| i.custom_name.as_deref() == Some(target))
    {
        return Ok(inst.id.clone());
    }
    // Try name match
    if let Some(inst) = instances.iter().find(|i| i.name == target) {
        return Ok(inst.id.clone());
    }
    // Try ID prefix match
    let prefix_matches: Vec<_> = instances
        .iter()
        .filter(|i| i.id.starts_with(target))
        .collect();
    match prefix_matches.len() {
        0 => anyhow::bail!("No instance found matching '{}'", target),
        1 => Ok(prefix_matches[0].id.clone()),
        n => anyhow::bail!(
            "Ambiguous: '{}' matches {} instances. Be more specific.",
            target,
            n
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inst(id: &str, name: &str) -> InstanceInfo {
        InstanceInfo {
            id: id.to_string(),
            name: name.to_string(),
            custom_name: None,
            running: true,
            working_dir: "/tmp".to_string(),
            command: "echo".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn match_instance_empty_list() {
        let result = match_instance(&[], "anything");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No running instances")
        );
    }

    #[test]
    fn match_instance_exact_id() {
        let instances = vec![
            inst("abc-123-def", "swift-azure-falcon"),
            inst("xyz-456-ghi", "calm-ruby-dragon"),
        ];
        let result = match_instance(&instances, "abc-123-def").unwrap();
        assert_eq!(result, "abc-123-def");
    }

    #[test]
    fn match_instance_by_name() {
        let instances = vec![
            inst("abc-123-def", "swift-azure-falcon"),
            inst("xyz-456-ghi", "calm-ruby-dragon"),
        ];
        let result = match_instance(&instances, "calm-ruby-dragon").unwrap();
        assert_eq!(result, "xyz-456-ghi");
    }

    #[test]
    fn match_instance_id_prefix() {
        let instances = vec![inst("abc-123-def", "falcon"), inst("xyz-456-ghi", "dragon")];
        let result = match_instance(&instances, "abc").unwrap();
        assert_eq!(result, "abc-123-def");
    }

    #[test]
    fn match_instance_ambiguous_prefix() {
        let instances = vec![inst("abc-111", "falcon"), inst("abc-222", "dragon")];
        let result = match_instance(&instances, "abc");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Ambiguous"));
    }

    #[test]
    fn match_instance_no_match() {
        let instances = vec![inst("abc-123", "falcon")];
        let result = match_instance(&instances, "zzz");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No instance found")
        );
    }

    #[test]
    fn match_instance_id_takes_priority_over_name() {
        // If target matches both an ID and a name, ID wins
        let instances = vec![inst("falcon", "name-a"), inst("other-id", "falcon")];
        let result = match_instance(&instances, "falcon").unwrap();
        // Should match the first instance by exact ID, not the second by name
        assert_eq!(result, "falcon");
    }

    #[test]
    fn instance_info_serde_roundtrip() {
        let info = inst("id-1", "name-1");
        let json = serde_json::to_string(&info).unwrap();
        let parsed: InstanceInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "id-1");
        assert_eq!(parsed.name, "name-1");
        assert!(parsed.running);
    }

    #[test]
    fn instance_info_defaults() {
        // command and created_at have #[serde(default)]
        let json = r#"{"id":"x","name":"y","running":false,"working_dir":"/tmp"}"#;
        let info: InstanceInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.command, "");
        assert_eq!(info.created_at, "");
    }

    #[test]
    fn ws_lifecycle_event_deserialization() {
        // Uses #[serde(tag = "type")] so "type" field is inline
        let json = r#"{"type":"InstanceStopped","instance_id":"inst-1"}"#;
        let event: WsLifecycleEvent = serde_json::from_str(json).unwrap();
        match event {
            WsLifecycleEvent::Stopped { instance_id } => {
                assert_eq!(instance_id, "inst-1");
            }
            _ => panic!("Expected InstanceStopped"),
        }
    }
}
