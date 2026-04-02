//! TUI settings screen — view and toggle server configuration.

use anyhow::{Context, Result};
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph},
};
use serde::Deserialize;
use std::time::Duration;

use super::daemon::DaemonInfo;
use workshop::config::{MAX_SCROLLBACK_LINES, MIN_SCROLLBACK_LINES};

/// Mirrors the server's GET /api/admin/config response.
#[derive(Deserialize, Clone, Debug)]
struct ConfigState {
    profile: Option<String>,
    host: String,
    port: u16,
    auth_enabled: bool,
    https: bool,
    scrollback_lines: usize,
}

/// Which field is selected.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Profile,
    Host,
    Port,
    Auth,
    Https,
    ScrollbackLines,
}

const FIELDS: [Field; 6] = [
    Field::Profile,
    Field::Host,
    Field::Port,
    Field::Auth,
    Field::Https,
    Field::ScrollbackLines,
];

const PROFILES: [Option<&str>; 4] = [Some("local"), Some("tunnel"), Some("server"), None];

/// Editing state for text fields.
struct EditState {
    field: Field,
    buffer: String,
    cursor: usize,
}

/// Status message shown at top of screen.
struct StatusMessage {
    text: String,
    is_error: bool,
}

pub fn run_settings(terminal: &mut DefaultTerminal, daemon: &DaemonInfo) -> Result<()> {
    let client = reqwest::blocking::Client::new();
    let mut config = fetch_config(&client, daemon)?;
    let mut selected = 0usize;
    let mut edit: Option<EditState> = None;
    let mut status: Option<StatusMessage> = None;
    // Track local modifications (differs from server state)
    let mut dirty = false;
    let mut confirming_exit = false;
    // Local working copy that may diverge from server until apply/save
    let mut local = config.clone();

    loop {
        let field = FIELDS[selected];

        terminal.draw(|frame| {
            let area = frame.area();

            // Layout: optional status bar + main content
            let chunks = if status.is_some() {
                Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area)
            } else {
                Layout::vertical([Constraint::Length(0), Constraint::Min(0)]).split(area)
            };

            // Status bar
            if let Some(ref msg) = status {
                let style = if msg.is_error {
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().add_modifier(Modifier::BOLD)
                };
                let status_line = Paragraph::new(Line::styled(&*msg.text, style));
                frame.render_widget(status_line, chunks[0]);
            }

            let content_area = chunks[1];

            let mut lines: Vec<Line> = Vec::new();
            lines.push(Line::raw(""));

            for (i, f) in FIELDS.iter().enumerate() {
                let is_selected = i == selected;
                let marker = if is_selected { "▸ " } else { "  " };

                let (label, value) = match f {
                    Field::Profile => (
                        "Profile",
                        local.profile.as_deref().unwrap_or("(none)").to_string(),
                    ),
                    Field::Host => (
                        "Host",
                        format_edit_or_value(&edit, Field::Host, &local.host),
                    ),
                    Field::Port => (
                        "Port",
                        format_edit_or_value(&edit, Field::Port, &local.port.to_string()),
                    ),
                    Field::Auth => (
                        "Auth",
                        if local.auth_enabled { "on" } else { "off" }.to_string(),
                    ),
                    Field::Https => ("HTTPS", if local.https { "on" } else { "off" }.to_string()),
                    Field::ScrollbackLines => (
                        "Scrollback",
                        format_edit_or_value(
                            &edit,
                            Field::ScrollbackLines,
                            &format!("{} lines", local.scrollback_lines),
                        ),
                    ),
                };

                let label_style = if is_selected {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let value_style = if is_selected && edit.is_some() {
                    Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                } else if is_selected {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default().add_modifier(Modifier::DIM)
                };

                lines.push(Line::from(vec![
                    Span::raw(marker),
                    Span::styled(format!("{:<14}", label), label_style),
                    Span::styled(value, value_style),
                ]));
            }

            lines.push(Line::raw(""));

            if dirty {
                lines.push(Line::styled(
                    "  (unsaved changes)",
                    Style::default().add_modifier(Modifier::ITALIC),
                ));
            }

            let help_text = if edit.is_some() {
                " type to edit · enter confirm · esc cancel "
            } else {
                " ↑↓ navigate · enter toggle/edit · a apply · s save+apply · esc back "
            };

            let block = Block::default()
                .title(" Server Configuration ")
                .title_bottom(Line::raw(help_text))
                .borders(Borders::ALL)
                .padding(Padding::horizontal(1));

            let paragraph = Paragraph::new(lines).block(block);
            frame.render_widget(paragraph, content_area);
        })?;

        if !event::poll(Duration::from_millis(100))? {
            continue;
        }

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            // Clear status on non-exit keypresses
            if !matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                confirming_exit = false;
            }
            status = None;

            // Handle edit mode
            if let Some(ref mut ed) = edit {
                match key.code {
                    KeyCode::Esc => {
                        edit = None;
                    }
                    KeyCode::Enter => {
                        // Commit the edit
                        match ed.field {
                            Field::Host => {
                                local.host = ed.buffer.clone();
                                update_profile_after_edit(&mut local);
                                dirty = true;
                            }
                            Field::Port => {
                                if let Ok(p) = ed.buffer.parse::<u16>() {
                                    local.port = p;
                                    dirty = true;
                                } else {
                                    status = Some(StatusMessage {
                                        text: "Invalid port number".to_string(),
                                        is_error: true,
                                    });
                                }
                            }
                            Field::ScrollbackLines => match ed.buffer.parse::<usize>() {
                                Ok(n)
                                    if (MIN_SCROLLBACK_LINES..=MAX_SCROLLBACK_LINES)
                                        .contains(&n) =>
                                {
                                    local.scrollback_lines = n;
                                    dirty = true;
                                }
                                Ok(_) => {
                                    status = Some(StatusMessage {
                                        text: format!(
                                            "Must be {}\u{2013}{}",
                                            MIN_SCROLLBACK_LINES, MAX_SCROLLBACK_LINES
                                        ),
                                        is_error: true,
                                    });
                                }
                                Err(_) => {
                                    status = Some(StatusMessage {
                                        text: "Invalid number".to_string(),
                                        is_error: true,
                                    });
                                }
                            },
                            _ => {}
                        }
                        edit = None;
                    }
                    KeyCode::Backspace => {
                        if ed.cursor > 0 {
                            ed.buffer.remove(ed.cursor - 1);
                            ed.cursor -= 1;
                        }
                    }
                    KeyCode::Left => {
                        ed.cursor = ed.cursor.saturating_sub(1);
                    }
                    KeyCode::Right => {
                        if ed.cursor < ed.buffer.len() {
                            ed.cursor += 1;
                        }
                    }
                    KeyCode::Char(c) => {
                        ed.buffer.insert(ed.cursor, c);
                        ed.cursor += 1;
                    }
                    _ => {}
                }
                continue;
            }

            // Normal mode
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    if dirty && !confirming_exit {
                        confirming_exit = true;
                        status = Some(StatusMessage {
                            text: "Unsaved changes. a=apply  s=save+apply  q=discard".to_string(),
                            is_error: false,
                        });
                        continue;
                    }
                    return Ok(());
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    selected = (selected + 1) % FIELDS.len();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    selected = if selected == 0 {
                        FIELDS.len() - 1
                    } else {
                        selected - 1
                    };
                }
                KeyCode::Enter => match field {
                    Field::Profile => {
                        // Cycle through profiles; "custom" is not in the cycle
                        let current_idx = PROFILES
                            .iter()
                            .position(|p| *p == local.profile.as_deref())
                            .unwrap_or(PROFILES.len() - 1);
                        let next_idx = (current_idx + 1) % PROFILES.len();
                        local.profile = PROFILES[next_idx].map(|s| s.to_string());
                        // Apply profile defaults to other fields
                        apply_profile_defaults(&mut local);
                        dirty = true;
                    }
                    Field::Host => {
                        let buf = local.host.clone();
                        let cursor = buf.len();
                        edit = Some(EditState {
                            field: Field::Host,
                            buffer: buf,
                            cursor,
                        });
                    }
                    Field::Port => {
                        let buf = local.port.to_string();
                        let cursor = buf.len();
                        edit = Some(EditState {
                            field: Field::Port,
                            buffer: buf,
                            cursor,
                        });
                    }
                    Field::Auth => {
                        local.auth_enabled = !local.auth_enabled;
                        update_profile_after_edit(&mut local);
                        dirty = true;
                    }
                    Field::Https => {
                        local.https = !local.https;
                        update_profile_after_edit(&mut local);
                        dirty = true;
                    }
                    Field::ScrollbackLines => {
                        let buf = local.scrollback_lines.to_string();
                        let cursor = buf.len();
                        edit = Some(EditState {
                            field: Field::ScrollbackLines,
                            buffer: buf,
                            cursor,
                        });
                    }
                },
                KeyCode::Char('a') => {
                    // Apply ephemerally
                    match apply_config(&client, daemon, &local, &config, false) {
                        Ok(()) => {
                            status = Some(StatusMessage {
                                text: "Applied (ephemeral) — server restarting...".to_string(),
                                is_error: false,
                            });
                            dirty = false;
                            // Brief pause then re-fetch
                            std::thread::sleep(Duration::from_millis(500));
                            if let Ok(new_config) = fetch_config(&client, daemon) {
                                config = new_config;
                                local = config.clone();
                            }
                        }
                        Err(e) => {
                            status = Some(StatusMessage {
                                text: format!("Apply failed: {}", e),
                                is_error: true,
                            });
                        }
                    }
                }
                KeyCode::Char('s') => {
                    // Save to config.toml and apply
                    match apply_config(&client, daemon, &local, &config, true) {
                        Ok(()) => {
                            status = Some(StatusMessage {
                                text: "Saved to config.toml — server restarting...".to_string(),
                                is_error: false,
                            });
                            dirty = false;
                            std::thread::sleep(Duration::from_millis(500));
                            if let Ok(new_config) = fetch_config(&client, daemon) {
                                config = new_config;
                                local = config.clone();
                            }
                        }
                        Err(e) => {
                            status = Some(StatusMessage {
                                text: format!("Save failed: {}", e),
                                is_error: true,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn format_edit_or_value(edit: &Option<EditState>, field: Field, default: &str) -> String {
    match edit {
        Some(ed) if ed.field == field => format!("{}▏", ed.buffer),
        _ => default.to_string(),
    }
}

/// After editing a non-profile field, check if values still match the current profile.
/// If they diverged, show "custom". If they happen to match a known profile, show that.
fn update_profile_after_edit(local: &mut ConfigState) {
    if let Some(detected) = detect_profile(local) {
        local.profile = Some(detected);
    } else if local.profile.is_some() {
        // Had a profile but values no longer match any
        local.profile = Some("custom".to_string());
    }
    // If profile was None, keep it None
}

/// Check if the current host/auth/https values match a known profile.
fn detect_profile(local: &ConfigState) -> Option<String> {
    if local.host == "127.0.0.1" && !local.auth_enabled && !local.https {
        Some("local".to_string())
    } else if local.host == "127.0.0.1" && local.auth_enabled && local.https {
        Some("tunnel".to_string())
    } else if local.host == "0.0.0.0" && local.auth_enabled && local.https {
        Some("server".to_string())
    } else {
        None
    }
}

fn apply_profile_defaults(local: &mut ConfigState) {
    match local.profile.as_deref() {
        Some("local") => {
            local.host = "127.0.0.1".to_string();
            local.auth_enabled = false;
            local.https = false;
        }
        Some("tunnel") => {
            local.host = "127.0.0.1".to_string();
            local.auth_enabled = true;
            local.https = true;
        }
        Some("server") => {
            local.host = "0.0.0.0".to_string();
            local.auth_enabled = true;
            local.https = true;
        }
        _ => {} // none: keep current values
    }
}

fn fetch_config(client: &reqwest::blocking::Client, daemon: &DaemonInfo) -> Result<ConfigState> {
    let url = format!("{}/api/admin/config", daemon.base_url());
    let resp = client
        .get(&url)
        .send()
        .context("Failed to reach daemon config endpoint")?;
    if !resp.status().is_success() {
        anyhow::bail!("GET /api/admin/config returned {}", resp.status());
    }
    resp.json().context("Failed to parse config response")
}

/// Build a PATCH body containing only fields that differ from the server snapshot.
fn apply_config(
    client: &reqwest::blocking::Client,
    daemon: &DaemonInfo,
    local: &ConfigState,
    server: &ConfigState,
    save: bool,
) -> Result<()> {
    let url = format!("{}/api/admin/config", daemon.base_url());
    let mut body = serde_json::Map::new();
    if local.host != server.host {
        body.insert("host".into(), serde_json::json!(local.host));
    }
    if local.port != server.port {
        body.insert("port".into(), serde_json::json!(local.port));
    }
    if local.auth_enabled != server.auth_enabled {
        body.insert("auth_enabled".into(), serde_json::json!(local.auth_enabled));
    }
    if local.https != server.https {
        body.insert("https".into(), serde_json::json!(local.https));
    }
    if local.scrollback_lines != server.scrollback_lines {
        body.insert(
            "scrollback_lines".into(),
            serde_json::json!(local.scrollback_lines),
        );
    }
    body.insert("save".into(), serde_json::json!(save));

    let resp = client
        .patch(&url)
        .json(&serde_json::Value::Object(body))
        .send()
        .context("Failed to reach daemon config endpoint")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        anyhow::bail!("PATCH /api/admin/config failed: {} {}", status, body);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(host: &str, auth: bool, https: bool) -> ConfigState {
        ConfigState {
            profile: None,
            host: host.to_string(),
            port: 9000,
            auth_enabled: auth,
            https,
            scrollback_lines: 10_000,
        }
    }

    // ── detect_profile ────────────────────────────────────────

    #[test]
    fn detect_local_profile() {
        let c = config("127.0.0.1", false, false);
        assert_eq!(detect_profile(&c), Some("local".to_string()));
    }

    #[test]
    fn detect_tunnel_profile() {
        let c = config("127.0.0.1", true, true);
        assert_eq!(detect_profile(&c), Some("tunnel".to_string()));
    }

    #[test]
    fn detect_server_profile() {
        let c = config("0.0.0.0", true, true);
        assert_eq!(detect_profile(&c), Some("server".to_string()));
    }

    #[test]
    fn detect_no_profile_for_custom() {
        let c = config("192.168.1.1", true, false);
        assert_eq!(detect_profile(&c), None);
    }

    #[test]
    fn detect_no_profile_partial_match() {
        // 0.0.0.0 but auth off
        let c = config("0.0.0.0", false, false);
        assert_eq!(detect_profile(&c), None);
    }

    // ── apply_profile_defaults ────────────────────────────────

    #[test]
    fn apply_local_defaults() {
        let mut c = config("0.0.0.0", true, true);
        c.profile = Some("local".to_string());
        apply_profile_defaults(&mut c);
        assert_eq!(c.host, "127.0.0.1");
        assert!(!c.auth_enabled);
        assert!(!c.https);
    }

    #[test]
    fn apply_tunnel_defaults() {
        let mut c = config("0.0.0.0", false, false);
        c.profile = Some("tunnel".to_string());
        apply_profile_defaults(&mut c);
        assert_eq!(c.host, "127.0.0.1");
        assert!(c.auth_enabled);
        assert!(c.https);
    }

    #[test]
    fn apply_server_defaults() {
        let mut c = config("127.0.0.1", false, false);
        c.profile = Some("server".to_string());
        apply_profile_defaults(&mut c);
        assert_eq!(c.host, "0.0.0.0");
        assert!(c.auth_enabled);
        assert!(c.https);
    }

    #[test]
    fn apply_none_profile_keeps_values() {
        let mut c = config("10.0.0.1", true, false);
        c.profile = None;
        apply_profile_defaults(&mut c);
        assert_eq!(c.host, "10.0.0.1");
        assert!(c.auth_enabled);
        assert!(!c.https);
    }

    // ── update_profile_after_edit ─────────────────────────────

    #[test]
    fn update_profile_detects_match() {
        let mut c = config("127.0.0.1", false, false);
        c.profile = Some("custom".to_string());
        update_profile_after_edit(&mut c);
        assert_eq!(c.profile, Some("local".to_string()));
    }

    #[test]
    fn update_profile_sets_custom_when_diverged() {
        let mut c = config("192.168.1.1", true, false);
        c.profile = Some("server".to_string());
        update_profile_after_edit(&mut c);
        assert_eq!(c.profile, Some("custom".to_string()));
    }

    #[test]
    fn update_profile_keeps_none_when_none() {
        let mut c = config("10.0.0.1", false, false);
        c.profile = None;
        update_profile_after_edit(&mut c);
        assert!(c.profile.is_none());
    }

    // ── format_edit_or_value ──────────────────────────────────

    #[test]
    fn format_shows_default_when_no_edit() {
        let result = format_edit_or_value(&None, Field::Host, "127.0.0.1");
        assert_eq!(result, "127.0.0.1");
    }

    #[test]
    fn format_shows_buffer_when_editing() {
        let edit = Some(EditState {
            field: Field::Host,
            buffer: "10.0.0".to_string(),
            cursor: 6,
        });
        let result = format_edit_or_value(&edit, Field::Host, "127.0.0.1");
        assert_eq!(result, "10.0.0▏");
    }

    #[test]
    fn format_shows_default_when_editing_different_field() {
        let edit = Some(EditState {
            field: Field::Port,
            buffer: "8080".to_string(),
            cursor: 4,
        });
        let result = format_edit_or_value(&edit, Field::Host, "127.0.0.1");
        assert_eq!(result, "127.0.0.1");
    }
}
