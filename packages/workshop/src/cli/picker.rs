use anyhow::Result;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
    layout::Alignment,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Padding},
};
use std::sync::mpsc;
use std::time::Duration;

use super::InstanceInfo;

pub enum PickerResult {
    Attach(String),
    NewInstance,
    Rename {
        id: String,
        custom_name: Option<String>,
    },
    Kill(String),
    KillServer,
    Settings,
    Quit,
}

/// Events received from the multiplexed WebSocket.
pub enum PickerEvent {
    Created(InstanceInfo),
    Stopped(String),
    Renamed {
        instance_id: String,
        custom_name: Option<String>,
    },
}

/// Inline rename editing state.
struct RenameState {
    instance_id: String,
    buffer: String,
    cursor: usize,
}

/// Show an interactive TUI picker for instances.
/// Falls back to most-recent or NewInstance when stdin is not a TTY.
/// The caller owns the terminal (shared with settings screen).
pub fn run_picker(
    terminal: &mut Option<DefaultTerminal>,
    base_url: &str,
    instances: Vec<InstanceInfo>,
    events: mpsc::Receiver<PickerEvent>,
    selected_id: Option<&str>,
) -> Result<PickerResult> {
    match terminal {
        Some(term) => picker_loop(term, base_url, instances, events, selected_id),
        None => {
            // No TTY — fall back to most recent instance or new
            Ok(match instances.last() {
                Some(inst) => PickerResult::Attach(inst.id.clone()),
                None => PickerResult::NewInstance,
            })
        }
    }
}

fn picker_loop(
    terminal: &mut DefaultTerminal,
    base_url: &str,
    mut instances: Vec<InstanceInfo>,
    events: mpsc::Receiver<PickerEvent>,
    selected_id: Option<&str>,
) -> Result<PickerResult> {
    let initial = selected_id
        .and_then(|id| instances.iter().position(|i| i.id == id))
        .unwrap_or(0);
    let mut state = ListState::default().with_selected(Some(initial));
    let mut confirming_kill_server = false;
    let mut rename: Option<RenameState> = None;

    loop {
        // Drain any pending live-update events
        while let Ok(ev) = events.try_recv() {
            // Remember the ID of the currently-selected instance so we can
            // restore the selection after mutating the vector.
            let selected_id = state
                .selected()
                .and_then(|i| instances.get(i))
                .map(|inst| inst.id.clone());

            match ev {
                PickerEvent::Created(inst) => {
                    if !instances.iter().any(|i| i.id == inst.id) {
                        instances.push(inst);
                    }
                }
                PickerEvent::Stopped(id) => {
                    instances.retain(|i| i.id != id);
                    // Cancel rename if the renamed instance was removed
                    if let Some(ref r) = rename
                        && r.instance_id == id
                    {
                        rename = None;
                    }
                }
                PickerEvent::Renamed {
                    instance_id,
                    custom_name,
                } => {
                    if let Some(inst) = instances.iter_mut().find(|i| i.id == instance_id) {
                        inst.custom_name = custom_name;
                    }
                }
            }
            // Restore selection by ID (falls back to clamped position)
            let total = instances.len() + 1;
            let new_sel = selected_id
                .and_then(|id| instances.iter().position(|i| i.id == id))
                .unwrap_or_else(|| state.selected().unwrap_or(0).min(total.saturating_sub(1)));
            state.select(Some(new_sel));
        }

        let total = instances.len() + 1;
        let renaming = rename.is_some();

        terminal.draw(|frame| {
            let area = frame.area();
            let items = build_items(&instances, &rename);

            let bottom_bar = if confirming_kill_server {
                let running = instances.iter().filter(|i| i.running).count();
                Line::from(vec![
                    Span::styled(
                        format!(
                            " Kill server and {} session{}? ",
                            running,
                            if running == 1 { "" } else { "s" }
                        ),
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("y to confirm · any key to cancel "),
                ])
            } else if renaming {
                Line::raw(" type to rename · enter confirm · esc cancel · backspace clear name ")
            } else {
                Line::raw(
                    " ↑↓ navigate · enter select · r rename · x kill · s settings · Q kill server · q/esc quit ",
                )
            };

            let list = List::new(items)
                .block(
                    Block::default()
                        .title(" work: select session ")
                        .title(
                            Line::styled(
                                format!(" {} ", base_url),
                                Style::default().add_modifier(Modifier::DIM),
                            )
                            .alignment(Alignment::Right),
                        )
                        .title_bottom(bottom_bar)
                        .borders(Borders::ALL)
                        .padding(Padding::horizontal(1)),
                )
                .highlight_style(
                    Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED),
                )
                .highlight_symbol("▸ ");
            frame.render_stateful_widget(list, area, &mut state);
        })?;

        // Poll with a short timeout so we can process WS events between frames
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            // Handle rename editing mode
            if let Some(ref mut r) = rename {
                match key.code {
                    KeyCode::Esc => {
                        rename = None;
                    }
                    KeyCode::Enter => {
                        let rename_id = r.instance_id.clone();
                        let trimmed = r.buffer.trim().to_string();
                        if let Some(inst) = instances.iter_mut().find(|i| i.id == rename_id) {
                            let custom_name = if trimmed.is_empty() || trimmed == inst.name {
                                None
                            } else {
                                Some(trimmed)
                            };
                            // Optimistic update
                            inst.custom_name = custom_name.clone();
                            drop(rename.take());
                            return Ok(PickerResult::Rename {
                                id: rename_id,
                                custom_name,
                            });
                        }
                        // Instance was removed while renaming
                        rename = None;
                    }
                    KeyCode::Backspace => {
                        if r.cursor > 0 {
                            // Find start of previous character
                            let prev = r.buffer[..r.cursor]
                                .char_indices()
                                .next_back()
                                .map_or(0, |(i, _)| i);
                            r.buffer.remove(prev);
                            r.cursor = prev;
                        }
                    }
                    KeyCode::Left => {
                        if r.cursor > 0 {
                            r.cursor = r.buffer[..r.cursor]
                                .char_indices()
                                .next_back()
                                .map_or(0, |(i, _)| i);
                        }
                    }
                    KeyCode::Right => {
                        if r.cursor < r.buffer.len() {
                            r.cursor = r.buffer.ceil_char_boundary(r.cursor + 1);
                        }
                    }
                    KeyCode::Char(c) => {
                        r.buffer.insert(r.cursor, c);
                        r.cursor += c.len_utf8();
                    }
                    _ => {}
                }
                continue;
            }

            if confirming_kill_server {
                confirming_kill_server = false;
                if key.code == KeyCode::Char('y') {
                    return Ok(PickerResult::KillServer);
                }
                continue;
            }

            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(PickerResult::Quit),
                KeyCode::Char('Q') => {
                    confirming_kill_server = true;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let i = state.selected().unwrap_or(0);
                    state.select(Some((i + 1) % total));
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let i = state.selected().unwrap_or(0);
                    state.select(Some(if i == 0 { total - 1 } else { i - 1 }));
                }
                KeyCode::Enter => {
                    let i = state.selected().unwrap_or(0);
                    return if let Some(inst) = instances.get(i) {
                        Ok(PickerResult::Attach(inst.id.clone()))
                    } else {
                        Ok(PickerResult::NewInstance)
                    };
                }
                KeyCode::Char('r') => {
                    let i = state.selected().unwrap_or(0);
                    if let Some(inst) = instances.get(i) {
                        let buf = inst.display_name().to_string();
                        let cursor = buf.len();
                        rename = Some(RenameState {
                            instance_id: inst.id.clone(),
                            buffer: buf,
                            cursor,
                        });
                    }
                }
                KeyCode::Char('x') => {
                    let i = state.selected().unwrap_or(0);
                    if let Some(inst) = instances.get(i) {
                        return Ok(PickerResult::Kill(inst.id.clone()));
                    }
                }
                KeyCode::Char('s') => {
                    return Ok(PickerResult::Settings);
                }
                _ => {}
            }
        }
    }
}

fn build_items<'a>(instances: &[InstanceInfo], rename: &Option<RenameState>) -> Vec<ListItem<'a>> {
    let mut items: Vec<ListItem> = instances
        .iter()
        .map(|inst| {
            let status = if inst.running { "running" } else { "stopped" };
            let short_id = if inst.id.len() > 8 {
                &inst.id[..8]
            } else {
                &inst.id
            };

            // Check if this row is being renamed
            let is_renaming = rename.as_ref().is_some_and(|r| r.instance_id == inst.id);

            let mut spans = if is_renaming {
                let r = rename.as_ref().unwrap();
                // Defensive: clamp cursor to a valid char boundary
                let cursor = r.buffer.floor_char_boundary(r.cursor.min(r.buffer.len()));
                let (before, rest) = r.buffer.split_at(cursor);
                let edit_style =
                    Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
                let cursor_style = Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED);
                // Character under cursor shown reversed; space block if at end
                let (cursor_ch, after) = if rest.is_empty() {
                    (" ", "")
                } else {
                    rest.split_at(rest.ceil_char_boundary(1))
                };
                let pad_len = 20usize.saturating_sub(before.len() + cursor_ch.len() + after.len());
                vec![
                    Span::styled(before.to_string(), edit_style),
                    Span::styled(cursor_ch.to_string(), cursor_style),
                    Span::styled(
                        format!("{:pad_len$}", after, pad_len = after.len() + pad_len),
                        edit_style,
                    ),
                ]
            } else {
                let display = inst.display_name();
                vec![Span::styled(
                    format!("{:<20}", display),
                    Style::default().add_modifier(Modifier::BOLD),
                )]
            };

            spans.extend([
                Span::raw(format!(" {:<10}", short_id)),
                Span::styled(
                    format!(" {:<8}", status),
                    if inst.running {
                        Style::default().add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().add_modifier(Modifier::DIM)
                    },
                ),
            ]);

            // Show auto-generated name dimmed when a custom name is set (and not renaming)
            if !is_renaming && inst.custom_name.is_some() {
                spans.push(Span::styled(
                    format!(" ({})", inst.name),
                    Style::default().add_modifier(Modifier::DIM),
                ));
            } else {
                spans.push(Span::raw(format!(" {}", inst.working_dir)));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    items.push(ListItem::new(Line::from(vec![Span::styled(
        "[ + ]  New instance",
        Style::default().add_modifier(Modifier::BOLD),
    )])));

    items
}
