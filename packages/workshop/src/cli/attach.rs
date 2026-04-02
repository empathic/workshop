use anyhow::Result;
use futures::{SinkExt, StreamExt};
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseEventKind,
};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite;

use crate::cli::daemon::{DaemonError, DaemonInfo};
use crate::cli::terminal::get_terminal_size;
use virtual_terminal::walk_row;
use workshop::config::{MAX_SCROLLBACK_LINES, MIN_SCROLLBACK_LINES};
use workshop::inference::ClaudeState;
use workshop::ws::{ClientMessage, ServerMessage};

/// Default scrollback lines if config fetch fails.
const DEFAULT_SCROLLBACK_LINES: usize = 10_000;

/// Lines scrolled per mouse wheel tick.
const MOUSE_SCROLL_LINES: usize = 3;

/// Detect detach key (Ctrl-]).
///
/// Crossterm 0.28 maps bytes 0x1C-0x1F to Ctrl+'4'-'7' (not the real
/// characters \, ], ^, _). Byte 0x1D (Ctrl-]) therefore arrives as
/// `Char('5') + CONTROL`.  Match both representations for safety.
fn is_detach_key(key: &KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char(']') | KeyCode::Char('5'))
}

/// Build status bar text based on current Claude state and elapsed time.
fn status_bar_text(state: &ClaudeState, started_at: Instant) -> String {
    match state {
        ClaudeState::Initializing => {
            let elapsed = started_at.elapsed().as_secs();
            let msg = if elapsed < 10 {
                "Waiting for first byte..."
            } else if elapsed < 30 {
                "Process is starting \u{2014} no output yet"
            } else {
                "No output received. Ctrl-] to switch instances"
            };
            format!(" INIT \u{2502} {} \u{2502} Ctrl-] switch ", msg)
        }
        ClaudeState::Starting => {
            let elapsed = started_at.elapsed().as_secs();
            let msg = if elapsed < 10 {
                "Loading Claude Code..."
            } else if elapsed < 30 {
                "Claude is taking a moment to initialize..."
            } else {
                "Startup is slower than usual \u{2014} network latency or API load"
            };
            format!(" STARTING \u{2502} {} \u{2502} Ctrl-] switch ", msg)
        }
        ClaudeState::Idle | ClaudeState::WaitingForInput { .. } => {
            " READY \u{2502} Claude is idle \u{2502} Ctrl-] detach ".to_string()
        }
        ClaudeState::Thinking => " ACTIVE \u{2502} Thinking... \u{2502} Ctrl-] detach ".to_string(),
        ClaudeState::Responding => {
            " ACTIVE \u{2502} Responding... \u{2502} Ctrl-] detach ".to_string()
        }
        ClaudeState::ToolExecuting { tool } => {
            format!(
                " ACTIVE \u{2502} Running {}... \u{2502} Ctrl-] detach ",
                tool
            )
        }
    }
}

// ── Color conversion ────────────────────────────────────────────────

fn vt100_color_to_ratatui(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(n) => Color::Indexed(n),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

// ── PtyWidget ───────────────────────────────────────────────────────

/// Renders a `vt100::Screen` buffer into a ratatui `Buffer`.
struct PtyWidget<'a> {
    screen: &'a vt100::Screen,
}

impl Widget for PtyWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let screen = self.screen;
        for row in 0..area.height {
            for cell in walk_row(screen, row, area.width) {
                let mut style = Style::default()
                    .fg(vt100_color_to_ratatui(cell.fg))
                    .bg(vt100_color_to_ratatui(cell.bg));
                let mut modifiers = Modifier::empty();
                if cell.bold {
                    modifiers |= Modifier::BOLD;
                }
                if cell.italic {
                    modifiers |= Modifier::ITALIC;
                }
                if cell.underline {
                    modifiers |= Modifier::UNDERLINED;
                }
                if cell.inverse {
                    modifiers |= Modifier::REVERSED;
                }
                style = style.add_modifier(modifiers);

                let x = area.x + cell.col;
                let y = area.y + row;
                if x < area.right() && y < area.bottom() {
                    buf[(x, y)].set_symbol(cell.contents).set_style(style);
                }
            }
        }
    }
}

// ── StatusBarWidget ─────────────────────────────────────────────────

/// Full-width reversed+bold status bar.
struct StatusBarWidget<'a> {
    text: &'a str,
}

impl Widget for StatusBarWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let style = Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD);
        // Fill entire area with spaces first
        for x in area.left()..area.right() {
            buf[(x, area.y)].set_symbol(" ").set_style(style);
        }
        // Write text characters
        let mut col = area.x;
        for ch in self.text.chars() {
            if col >= area.right() {
                break;
            }
            buf[(col, area.y)]
                .set_symbol(&ch.to_string())
                .set_style(style);
            col += 1;
        }
    }
}

// ── OverlayBadge ────────────────────────────────────────────────────

/// Right-aligned reversed badge rendered in the top-right of the given area.
struct OverlayBadge<'a> {
    text: &'a str,
}

impl Widget for OverlayBadge<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let padded = format!(" {} ", self.text);
        let badge_width = padded.len() as u16;
        if badge_width > area.width {
            return;
        }
        let style = Style::default().add_modifier(Modifier::REVERSED);
        let start_col = area.right().saturating_sub(badge_width);
        for (i, ch) in padded.chars().enumerate() {
            let x = start_col + i as u16;
            if x < area.right() {
                buf[(x, area.y)]
                    .set_symbol(&ch.to_string())
                    .set_style(style);
            }
        }
    }
}

// ── key_to_bytes ────────────────────────────────────────────────────

/// Convert a crossterm `KeyEvent` to the byte sequence a PTY expects.
fn key_to_bytes(key: &KeyEvent) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char(c) if ctrl => {
            // Crossterm 0.28 maps control bytes to KeyCode::Char in two ranges:
            //   0x01-0x1A  → Ctrl + 'a'-'z'   (c - 0x01 + b'a')
            //   0x1C-0x1F  → Ctrl + '4'-'7'   (c - 0x1C + b'4')
            // Reverse both mappings to recover the original byte.
            match c {
                'a'..='z' => Some(vec![(c as u8) - b'a' + 1]),
                '4'..='7' => Some(vec![(c as u8) - b'4' + 0x1C]),
                _ => None,
            }
        }
        KeyCode::Char(c) => {
            let mut bytes = [0u8; 4];
            let s = c.encode_utf8(&mut bytes);
            Some(s.as_bytes().to_vec())
        }
        KeyCode::Enter => Some(b"\r".to_vec()),
        KeyCode::Backspace => Some(vec![0x7f]),
        KeyCode::Esc => Some(b"\x1b".to_vec()),
        KeyCode::Tab => Some(b"\t".to_vec()),
        KeyCode::BackTab => Some(b"\x1b[Z".to_vec()),
        KeyCode::Up => Some(b"\x1b[A".to_vec()),
        KeyCode::Down => Some(b"\x1b[B".to_vec()),
        KeyCode::Right => Some(b"\x1b[C".to_vec()),
        KeyCode::Left => Some(b"\x1b[D".to_vec()),
        KeyCode::Home => Some(b"\x1b[H".to_vec()),
        KeyCode::End => Some(b"\x1b[F".to_vec()),
        KeyCode::PageUp => Some(b"\x1b[5~".to_vec()),
        KeyCode::PageDown => Some(b"\x1b[6~".to_vec()),
        KeyCode::Insert => Some(b"\x1b[2~".to_vec()),
        KeyCode::Delete => Some(b"\x1b[3~".to_vec()),
        KeyCode::F(1) => Some(b"\x1bOP".to_vec()),
        KeyCode::F(2) => Some(b"\x1bOQ".to_vec()),
        KeyCode::F(3) => Some(b"\x1bOR".to_vec()),
        KeyCode::F(4) => Some(b"\x1bOS".to_vec()),
        KeyCode::F(5) => Some(b"\x1b[15~".to_vec()),
        KeyCode::F(6) => Some(b"\x1b[17~".to_vec()),
        KeyCode::F(7) => Some(b"\x1b[18~".to_vec()),
        KeyCode::F(8) => Some(b"\x1b[19~".to_vec()),
        KeyCode::F(9) => Some(b"\x1b[20~".to_vec()),
        KeyCode::F(10) => Some(b"\x1b[21~".to_vec()),
        KeyCode::F(11) => Some(b"\x1b[23~".to_vec()),
        KeyCode::F(12) => Some(b"\x1b[24~".to_vec()),
        _ => None,
    }
}

// ── Events from the WS reader task ─────────────────────────────────

enum AttachEvent {
    Output(String),
    StateChange(ClaudeState),
    Closed,
}

/// What happened when an attach session ended.
pub enum AttachOutcome {
    /// User pressed Ctrl-] to detach; instance is still running.
    Detached,
    /// The remote process exited (WebSocket closed).
    Exited,
}

/// Attach to an instance, forwarding terminal I/O over WebSocket.
pub async fn attach(daemon: &DaemonInfo, instance_id: &str) -> Result<AttachOutcome, DaemonError> {
    // 1. Fetch scrollback_lines from server config (best-effort, fall back to default)
    let scrollback_lines = fetch_scrollback_lines(daemon).await;

    // 2. Connect to the multiplexed WebSocket endpoint.
    let ws_url = daemon.mux_ws_url();
    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .map_err(DaemonError::from_tungstenite)?;

    // 3. Session phase — internal anyhow, mapped to Other at boundary
    attach_session(ws_stream, scrollback_lines, instance_id)
        .await
        .map_err(Into::into)
}

/// Fetch scrollback_lines from the server config endpoint.
/// Clamped to 100–100,000 to prevent degenerate allocations.
async fn fetch_scrollback_lines(daemon: &DaemonInfo) -> usize {
    let url = format!("{}/api/admin/config", daemon.base_url());
    let result: Option<usize> = async {
        let resp = reqwest::get(&url).await.ok()?;
        let json: serde_json::Value = resp.json().await.ok()?;
        json.get("scrollback_lines")?.as_u64().map(|v| v as usize)
    }
    .await;
    result
        .unwrap_or(DEFAULT_SCROLLBACK_LINES)
        .clamp(MIN_SCROLLBACK_LINES, MAX_SCROLLBACK_LINES)
}

/// Run the attach session after WebSocket is connected.
async fn attach_session(
    ws_stream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    scrollback_lines: usize,
    instance_id: &str,
) -> Result<AttachOutcome> {
    let (mut ws_write, mut ws_read) = ws_stream.split();

    // Size the PTY: terminal height minus 1 row for status bar
    let (term_rows, term_cols) = get_terminal_size().unwrap_or((24, 80));
    let pty_rows = term_rows.saturating_sub(1).max(1);

    let mut vt_parser = vt100::Parser::new(pty_rows, term_cols, scrollback_lines);

    // Channels: WS reader → main loop, main loop → WS writer
    let (ws_read_tx, ws_read_rx) = std::sync::mpsc::channel::<AttachEvent>();
    let (ws_write_tx, ws_write_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // Send Focus to subscribe to the instance
    let focus_msg = ClientMessage::Focus {
        instance_id: instance_id.to_string(),
        since_uuid: None,
    };
    let json = serde_json::to_string(&focus_msg)?;
    ws_write.send(tungstenite::Message::Text(json)).await?;

    // Send TerminalVisible to trigger resize + OutputHistory replay
    let visible_msg = ClientMessage::TerminalVisible {
        instance_id: instance_id.to_string(),
        rows: pty_rows,
        cols: term_cols,
        client_type: Some("terminal".to_string()),
    };
    let json = serde_json::to_string(&visible_msg)?;
    ws_write.send(tungstenite::Message::Text(json)).await?;

    // Spawn async WS reader task
    let read_tx = ws_read_tx.clone();
    let filter_instance_id = instance_id.to_string();
    tokio::spawn(async move {
        while let Some(msg) = ws_read.next().await {
            match msg {
                Ok(tungstenite::Message::Text(text)) => {
                    match serde_json::from_str::<ServerMessage>(&text) {
                        // Terminal output (live)
                        Ok(ServerMessage::Output {
                            instance_id: ref iid,
                            data,
                            ..
                        }) if iid == &filter_instance_id => {
                            if read_tx.send(AttachEvent::Output(data)).is_err() {
                                break;
                            }
                        }
                        // Terminal replay on focus/TerminalVisible
                        Ok(ServerMessage::OutputHistory {
                            instance_id: ref iid,
                            data,
                        }) if iid == &filter_instance_id => {
                            if read_tx.send(AttachEvent::Output(data)).is_err() {
                                break;
                            }
                        }
                        // State change
                        Ok(ServerMessage::StateChange {
                            instance_id: ref iid,
                            state,
                            ..
                        }) if iid == &filter_instance_id => {
                            if read_tx.send(AttachEvent::StateChange(state)).is_err() {
                                break;
                            }
                        }
                        // Initial state from focus acknowledgment
                        Ok(ServerMessage::FocusAck {
                            instance_id: ref iid,
                            claude_state: Some(state),
                        }) if iid == &filter_instance_id => {
                            if read_tx.send(AttachEvent::StateChange(state)).is_err() {
                                break;
                            }
                        }
                        // Ignore everything else (InstanceList, PresenceUpdate, Chat, etc.)
                        _ => {}
                    }
                }
                Ok(tungstenite::Message::Close(_)) | Err(_) => {
                    let _ = read_tx.send(AttachEvent::Closed);
                    break;
                }
                _ => {}
            }
        }
    });

    // Spawn async WS writer task
    let mut ws_write_rx = ws_write_rx;
    tokio::spawn(async move {
        while let Some(json) = ws_write_rx.recv().await {
            if ws_write
                .send(tungstenite::Message::Text(json))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // Enter the blocking ratatui render loop (with mouse capture for scroll wheel)
    let mut terminal = ratatui::init();
    let _ = ratatui::crossterm::execute!(std::io::stdout(), EnableMouseCapture);
    let outcome = tokio::task::block_in_place(|| {
        run_event_loop(
            &mut terminal,
            &mut vt_parser,
            &ws_read_rx,
            &ws_write_tx,
            scrollback_lines,
            instance_id,
        )
    });
    let _ = ratatui::crossterm::execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();

    outcome
}

// =============================================================================
// Event loop types
// =============================================================================

/// Mutable state for a TUI attach session.
struct AttachState {
    claude_state: ClaudeState,
    scroll_offset: usize,
    badge_until: Instant,
    attach_time: Instant,
}

/// What the input handler decided — pure classification, no side effects.
#[derive(Debug, PartialEq)]
enum InputAction {
    Detach,
    ScrollUp(usize),
    ScrollDown(usize),
    SendBytes(Vec<u8>),
    Resize { rows: u16, cols: u16 },
}

// =============================================================================
// Event loop phases
// =============================================================================

/// Drain all pending WS events into the parser and state.
/// Returns `true` if the connection closed.
fn drain_ws(
    ws_read_rx: &std::sync::mpsc::Receiver<AttachEvent>,
    vt_parser: &mut vt100::Parser,
    state: &mut AttachState,
) -> bool {
    while let Ok(ev) = ws_read_rx.try_recv() {
        match ev {
            AttachEvent::Output(data) => vt_parser.process(data.as_bytes()),
            AttachEvent::StateChange(new_state) => state.claude_state = new_state,
            AttachEvent::Closed => return true,
        }
    }
    false
}

/// Classify a crossterm event into an InputAction.
/// Pure function — no state mutation, no I/O.
fn classify_input(ev: &Event, page_size: usize) -> Option<InputAction> {
    match ev {
        // Shift+scroll keys
        Event::Key(key)
            if key.kind == KeyEventKind::Press && key.modifiers.contains(KeyModifiers::SHIFT) =>
        {
            match key.code {
                KeyCode::PageUp => Some(InputAction::ScrollUp(page_size)),
                KeyCode::PageDown => Some(InputAction::ScrollDown(page_size)),
                KeyCode::Up => Some(InputAction::ScrollUp(1)),
                KeyCode::Down => Some(InputAction::ScrollDown(1)),
                _ => {
                    // Any other Shift+key: forward as input (apply_action snaps to bottom)
                    key_to_bytes(key).map(InputAction::SendBytes)
                }
            }
        }

        // Mouse wheel
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::ScrollUp => Some(InputAction::ScrollUp(MOUSE_SCROLL_LINES)),
            MouseEventKind::ScrollDown => Some(InputAction::ScrollDown(MOUSE_SCROLL_LINES)),
            _ => None,
        },

        // Regular key presses
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            if is_detach_key(key) {
                Some(InputAction::Detach)
            } else {
                key_to_bytes(key).map(InputAction::SendBytes)
            }
        }

        Event::Resize(cols, rows) => Some(InputAction::Resize {
            rows: *rows,
            cols: *cols,
        }),

        _ => None,
    }
}

/// Apply a classified action to session state. Returns `Some(outcome)` to exit the loop.
fn apply_action(
    action: InputAction,
    vt_parser: &mut vt100::Parser,
    state: &mut AttachState,
    ws_write_tx: &tokio::sync::mpsc::UnboundedSender<String>,
    scrollback_capacity: usize,
    instance_id: &str,
) -> Option<AttachOutcome> {
    match action {
        InputAction::Detach => {
            eprintln!("\r\n[work: detached]");
            return Some(AttachOutcome::Detached);
        }
        InputAction::ScrollUp(n) => {
            state.scroll_offset = state.scroll_offset.saturating_add(n);
        }
        InputAction::ScrollDown(n) => {
            state.scroll_offset = state.scroll_offset.saturating_sub(n);
        }
        InputAction::SendBytes(bytes) => {
            state.scroll_offset = 0;
            let msg = ClientMessage::Input {
                instance_id: instance_id.to_string(),
                data: String::from_utf8_lossy(&bytes).to_string(),
                task_id: None,
            };
            if let Ok(json) = serde_json::to_string(&msg) {
                let _ = ws_write_tx.send(json);
            }
        }
        InputAction::Resize { rows, cols } => {
            let pty_rows = rows.saturating_sub(1).max(1);
            // Save visible screen, recreate parser at new dims (clears
            // scrollback — SIGWINCH redraw will rebuild it correctly).
            let content = vt_parser.screen().contents_formatted();
            let cursor = vt_parser.screen().cursor_position();
            *vt_parser = vt100::Parser::new(pty_rows, cols, scrollback_capacity);
            vt_parser.process(&content);
            vt_parser.process(format!("\x1b[{};{}H", cursor.0 + 1, cursor.1 + 1).as_bytes());
            // Use TerminalVisible to trigger both resize AND OutputHistory replay
            let msg = ClientMessage::TerminalVisible {
                instance_id: instance_id.to_string(),
                rows: pty_rows,
                cols,
                client_type: Some("terminal".to_string()),
            };
            if let Ok(json) = serde_json::to_string(&msg) {
                let _ = ws_write_tx.send(json);
            }
        }
    }
    None
}

/// Render the current frame: PTY content, status bar, optional badge.
fn render_frame(
    terminal: &mut ratatui::DefaultTerminal,
    vt_parser: &vt100::Parser,
    state: &AttachState,
) -> Result<()> {
    let screen = vt_parser.screen();
    let bar_text = if state.scroll_offset > 0 {
        format!(
            " SCROLL \u{2502} {} lines up \u{2502} Shift-PgDn or scroll to return ",
            state.scroll_offset
        )
    } else {
        status_bar_text(&state.claude_state, state.attach_time)
    };
    let show_badge = Instant::now() < state.badge_until;
    let cursor_pos = screen.cursor_position();
    let hide_cursor = screen.hide_cursor();
    let at_bottom = state.scroll_offset == 0;

    terminal.draw(|frame| {
        let [content, status] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());

        frame.render_widget(PtyWidget { screen }, content);
        frame.render_widget(StatusBarWidget { text: &bar_text }, status);

        if show_badge && at_bottom {
            const OVERLAY_TEXT: &str = "attached -- Ctrl-] to detach";
            frame.render_widget(OverlayBadge { text: OVERLAY_TEXT }, content);
        }

        if at_bottom && !hide_cursor {
            let (row, col) = cursor_pos;
            frame.set_cursor_position((content.x + col, content.y + row));
        }
    })?;
    Ok(())
}

// =============================================================================
// Main event loop
// =============================================================================

/// Blocking event loop: drain → clamp scroll → render → poll → classify → apply.
fn run_event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    vt_parser: &mut vt100::Parser,
    ws_read_rx: &std::sync::mpsc::Receiver<AttachEvent>,
    ws_write_tx: &tokio::sync::mpsc::UnboundedSender<String>,
    scrollback_capacity: usize,
    instance_id: &str,
) -> Result<AttachOutcome> {
    let mut state = AttachState {
        claude_state: ClaudeState::Initializing,
        scroll_offset: 0,
        badge_until: Instant::now() + Duration::from_secs(5),
        attach_time: Instant::now(),
    };

    loop {
        // 1. Drain all pending WS messages
        if drain_ws(ws_read_rx, vt_parser, &mut state) {
            eprintln!("\r\n[work: exited]");
            return Ok(AttachOutcome::Exited);
        }

        // 2. Clamp scroll offset to actual scrollback depth
        vt_parser.screen_mut().set_scrollback(state.scroll_offset);
        state.scroll_offset = vt_parser.screen().scrollback();

        // 3. Render
        render_frame(terminal, vt_parser, &state)?;

        // 4. Poll, classify, apply
        if event::poll(Duration::from_millis(16))? {
            let ev = event::read()?;
            let page_size = terminal
                .size()
                .map_or(23, |s| s.height.saturating_sub(1).max(1) as usize);

            if let Some(action) = classify_input(&ev, page_size)
                && let Some(outcome) = apply_action(
                    action,
                    vt_parser,
                    &mut state,
                    ws_write_tx,
                    scrollback_capacity,
                    instance_id,
                )
            {
                return Ok(outcome);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Color conversion ────────────────────────────────────────────

    #[test]
    fn color_conversion_default() {
        assert_eq!(vt100_color_to_ratatui(vt100::Color::Default), Color::Reset);
    }

    #[test]
    fn color_conversion_indexed() {
        assert_eq!(
            vt100_color_to_ratatui(vt100::Color::Idx(42)),
            Color::Indexed(42)
        );
    }

    #[test]
    fn color_conversion_rgb() {
        assert_eq!(
            vt100_color_to_ratatui(vt100::Color::Rgb(10, 20, 30)),
            Color::Rgb(10, 20, 30)
        );
    }

    // ── PtyWidget ───────────────────────────────────────────────────

    #[test]
    fn pty_widget_renders_text() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"Hello");

        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        PtyWidget {
            screen: parser.screen(),
        }
        .render(area, &mut buf);

        assert_eq!(buf[(0, 0)].symbol(), "H");
        assert_eq!(buf[(1, 0)].symbol(), "e");
        assert_eq!(buf[(2, 0)].symbol(), "l");
        assert_eq!(buf[(3, 0)].symbol(), "l");
        assert_eq!(buf[(4, 0)].symbol(), "o");
    }

    #[test]
    fn pty_widget_renders_colors() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"\x1b[31mRed");

        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        PtyWidget {
            screen: parser.screen(),
        }
        .render(area, &mut buf);

        assert_eq!(buf[(0, 0)].symbol(), "R");
        assert_eq!(buf[(0, 0)].fg, Color::Indexed(1));
    }

    #[test]
    fn pty_widget_wide_chars() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process("漢字".as_bytes());

        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        PtyWidget {
            screen: parser.screen(),
        }
        .render(area, &mut buf);

        // First char occupies cols 0-1, second occupies cols 2-3
        assert_eq!(buf[(0, 0)].symbol(), "漢");
        // Col 1 is wide continuation — should still be space (skipped)
        assert_eq!(buf[(2, 0)].symbol(), "字");
    }

    // ── key_to_bytes ────────────────────────────────────────────────

    #[test]
    fn key_to_bytes_printable() {
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&key), Some(vec![0x61]));
    }

    #[test]
    fn key_to_bytes_ctrl_c() {
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(key_to_bytes(&key), Some(vec![0x03]));
    }

    #[test]
    fn key_to_bytes_arrows() {
        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        let left = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
        let right = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&up), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_to_bytes(&down), Some(b"\x1b[B".to_vec()));
        assert_eq!(key_to_bytes(&left), Some(b"\x1b[D".to_vec()));
        assert_eq!(key_to_bytes(&right), Some(b"\x1b[C".to_vec()));
    }

    #[test]
    fn key_to_bytes_enter_backspace() {
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let backspace = KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(key_to_bytes(&enter), Some(b"\r".to_vec()));
        assert_eq!(key_to_bytes(&backspace), Some(vec![0x7f]));
    }

    #[test]
    fn key_to_bytes_crossterm_ctrl_bracket_range() {
        // Crossterm 0.28 maps bytes 0x1C-0x1F to Ctrl+'4'-'7'
        let ctrl4 = KeyEvent::new(KeyCode::Char('4'), KeyModifiers::CONTROL);
        let ctrl5 = KeyEvent::new(KeyCode::Char('5'), KeyModifiers::CONTROL);
        let ctrl6 = KeyEvent::new(KeyCode::Char('6'), KeyModifiers::CONTROL);
        let ctrl7 = KeyEvent::new(KeyCode::Char('7'), KeyModifiers::CONTROL);
        assert_eq!(key_to_bytes(&ctrl4), Some(vec![0x1C])); // Ctrl-\
        assert_eq!(key_to_bytes(&ctrl5), Some(vec![0x1D])); // Ctrl-]
        assert_eq!(key_to_bytes(&ctrl6), Some(vec![0x1E])); // Ctrl-^
        assert_eq!(key_to_bytes(&ctrl7), Some(vec![0x1F])); // Ctrl-_
    }

    #[test]
    fn detach_key_matches_crossterm_representation() {
        // Crossterm reports Ctrl-] as Char('5') + CONTROL
        let crossterm_ctrl_bracket = KeyEvent::new(KeyCode::Char('5'), KeyModifiers::CONTROL);
        assert!(is_detach_key(&crossterm_ctrl_bracket));
        // Also match the "ideal" representation in case a future crossterm fixes this
        let ideal_ctrl_bracket = KeyEvent::new(KeyCode::Char(']'), KeyModifiers::CONTROL);
        assert!(is_detach_key(&ideal_ctrl_bracket));
        // Regular keys should not match
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(!is_detach_key(&ctrl_c));
        let plain_5 = KeyEvent::new(KeyCode::Char('5'), KeyModifiers::NONE);
        assert!(!is_detach_key(&plain_5));
    }

    // ── Scrollback ───────────────────────────────────────────────────

    #[test]
    fn scrollback_preserves_history() {
        let mut parser = vt100::Parser::new(3, 10, 100);
        // Write 6 lines into a 3-row terminal → 3 lines should be in scrollback
        for i in 0..6 {
            parser.process(format!("line {}\r\n", i).as_bytes());
        }
        // At offset 0 (bottom), visible screen shows the most recent rows
        assert_eq!(parser.screen().scrollback(), 0);

        // Scroll up to see history
        parser.screen_mut().set_scrollback(3);
        let offset = parser.screen().scrollback();
        assert_eq!(offset, 3);

        // Row 0 of the viewport is now a scrollback row
        let cell = parser.screen().cell(0, 0).unwrap();
        let ch = cell.contents();
        // Should be one of the earlier lines (the exact content depends on
        // newline wrapping, but it must not be empty/blank)
        assert!(!ch.is_empty(), "scrollback row should have content");
    }

    #[test]
    fn scrollback_offset_clamps_to_max() {
        let mut parser = vt100::Parser::new(3, 10, 100);
        // Only 2 lines of output → at most ~0-1 scrollback rows
        parser.process(b"hello\r\nworld");
        parser.screen_mut().set_scrollback(9999);
        // Should clamp to the actual scrollback length
        let offset = parser.screen().scrollback();
        assert!(offset <= 2, "offset should be clamped, got {}", offset);
    }

    // ── StatusBarWidget ─────────────────────────────────────────────

    #[test]
    fn status_bar_renders() {
        let states = vec![
            ClaudeState::Initializing,
            ClaudeState::Starting,
            ClaudeState::Idle,
            ClaudeState::Thinking,
            ClaudeState::Responding,
            ClaudeState::ToolExecuting {
                tool: "Read".to_string(),
            },
            ClaudeState::WaitingForInput { prompt: None },
        ];
        let started = Instant::now();
        for state in states {
            let text = status_bar_text(&state, started);
            assert!(!text.is_empty(), "status bar text for {:?} is empty", state);

            let area = Rect::new(0, 0, 80, 1);
            let mut buf = Buffer::empty(area);
            StatusBarWidget { text: &text }.render(area, &mut buf);

            // Every cell should be reversed+bold
            let cell = &buf[(0, 0)];
            assert!(
                cell.modifier.contains(Modifier::REVERSED),
                "status bar cell should be reversed for {:?}",
                state
            );
            assert!(
                cell.modifier.contains(Modifier::BOLD),
                "status bar cell should be bold for {:?}",
                state
            );
        }
    }

    // ── classify_input ──────────────────────────────────────────────

    fn key_press(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new_with_kind(
            code,
            modifiers,
            KeyEventKind::Press,
        ))
    }

    fn mouse_event(kind: MouseEventKind) -> Event {
        use ratatui::crossterm::event::MouseEvent;
        Event::Mouse(MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        })
    }

    #[test]
    fn classify_shift_page_up_scrolls_up_by_page() {
        let ev = key_press(KeyCode::PageUp, KeyModifiers::SHIFT);
        assert_eq!(classify_input(&ev, 24), Some(InputAction::ScrollUp(24)));
    }

    #[test]
    fn classify_shift_page_down_scrolls_down_by_page() {
        let ev = key_press(KeyCode::PageDown, KeyModifiers::SHIFT);
        assert_eq!(classify_input(&ev, 24), Some(InputAction::ScrollDown(24)));
    }

    #[test]
    fn classify_shift_up_scrolls_up_by_one() {
        let ev = key_press(KeyCode::Up, KeyModifiers::SHIFT);
        assert_eq!(classify_input(&ev, 24), Some(InputAction::ScrollUp(1)));
    }

    #[test]
    fn classify_shift_down_scrolls_down_by_one() {
        let ev = key_press(KeyCode::Down, KeyModifiers::SHIFT);
        assert_eq!(classify_input(&ev, 24), Some(InputAction::ScrollDown(1)));
    }

    #[test]
    fn classify_shift_other_key_sends_bytes() {
        // Shift+A should forward as input, not scroll
        let ev = key_press(KeyCode::Char('A'), KeyModifiers::SHIFT);
        assert_eq!(
            classify_input(&ev, 24),
            Some(InputAction::SendBytes(b"A".to_vec()))
        );
    }

    #[test]
    fn classify_mouse_scroll_up() {
        let ev = mouse_event(MouseEventKind::ScrollUp);
        assert_eq!(
            classify_input(&ev, 24),
            Some(InputAction::ScrollUp(MOUSE_SCROLL_LINES))
        );
    }

    #[test]
    fn classify_mouse_scroll_down() {
        let ev = mouse_event(MouseEventKind::ScrollDown);
        assert_eq!(
            classify_input(&ev, 24),
            Some(InputAction::ScrollDown(MOUSE_SCROLL_LINES))
        );
    }

    #[test]
    fn classify_mouse_move_ignored() {
        let ev = mouse_event(MouseEventKind::Moved);
        assert_eq!(classify_input(&ev, 24), None);
    }

    #[test]
    fn classify_detach_key() {
        // Crossterm represents Ctrl-] as Char('5') + CONTROL
        let ev = key_press(KeyCode::Char('5'), KeyModifiers::CONTROL);
        assert_eq!(classify_input(&ev, 24), Some(InputAction::Detach));
    }

    #[test]
    fn classify_regular_char_sends_bytes() {
        let ev = key_press(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(
            classify_input(&ev, 24),
            Some(InputAction::SendBytes(vec![0x61]))
        );
    }

    #[test]
    fn classify_resize_event() {
        let ev = Event::Resize(120, 40);
        assert_eq!(
            classify_input(&ev, 24),
            Some(InputAction::Resize {
                rows: 40,
                cols: 120
            })
        );
    }

    #[test]
    fn classify_key_release_ignored() {
        let ev = Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        assert_eq!(classify_input(&ev, 24), None);
    }

    #[test]
    fn classify_page_size_propagates() {
        // Page size should be exactly what's passed in
        let ev = key_press(KeyCode::PageUp, KeyModifiers::SHIFT);
        assert_eq!(classify_input(&ev, 53), Some(InputAction::ScrollUp(53)));
        assert_eq!(classify_input(&ev, 1), Some(InputAction::ScrollUp(1)));
    }

    // ── apply_action ────────────────────────────────────────────────

    fn make_state() -> AttachState {
        AttachState {
            claude_state: ClaudeState::Idle,
            scroll_offset: 0,
            badge_until: Instant::now(),
            attach_time: Instant::now(),
        }
    }

    #[test]
    fn apply_detach_returns_detached() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        let mut state = make_state();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let result = apply_action(
            InputAction::Detach,
            &mut parser,
            &mut state,
            &tx,
            100,
            "test",
        );
        assert!(matches!(result, Some(AttachOutcome::Detached)));
    }

    #[test]
    fn apply_scroll_up_increases_offset() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        let mut state = make_state();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        apply_action(
            InputAction::ScrollUp(10),
            &mut parser,
            &mut state,
            &tx,
            100,
            "test",
        );
        assert_eq!(state.scroll_offset, 10);
    }

    #[test]
    fn apply_scroll_down_decreases_offset() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        let mut state = make_state();
        state.scroll_offset = 15;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        apply_action(
            InputAction::ScrollDown(5),
            &mut parser,
            &mut state,
            &tx,
            100,
            "test",
        );
        assert_eq!(state.scroll_offset, 10);
    }

    #[test]
    fn apply_scroll_down_saturates_at_zero() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        let mut state = make_state();
        state.scroll_offset = 3;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        apply_action(
            InputAction::ScrollDown(100),
            &mut parser,
            &mut state,
            &tx,
            100,
            "test",
        );
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn apply_send_bytes_snaps_to_bottom_and_sends() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        let mut state = make_state();
        state.scroll_offset = 42;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let result = apply_action(
            InputAction::SendBytes(b"hello".to_vec()),
            &mut parser,
            &mut state,
            &tx,
            100,
            "test",
        );
        assert!(result.is_none());
        assert_eq!(state.scroll_offset, 0, "should snap to bottom on input");
        let msg = rx.try_recv().expect("should have sent a message");
        assert!(msg.contains("hello"), "message should contain input data");
    }

    #[test]
    fn apply_resize_recreates_parser_and_sends_resize() {
        let mut parser = vt100::Parser::new(24, 80, 100);
        parser.process(b"visible content");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut state = make_state();
        let result = apply_action(
            InputAction::Resize {
                rows: 40,
                cols: 120,
            },
            &mut parser,
            &mut state,
            &tx,
            100,
            "test",
        );
        assert!(result.is_none());
        // Parser should be at new dims (rows - 1 for status bar)
        let (rows, cols) = parser.screen().size();
        assert_eq!(rows, 39);
        assert_eq!(cols, 120);
        // Content should be preserved
        let vis = virtual_terminal::read_row_text(parser.screen(), 0, cols);
        assert!(vis.contains("visible content"));
        // Resize message should have been sent
        let msg = rx.try_recv().expect("should have sent resize message");
        assert!(msg.contains("\"rows\":39"));
        assert!(msg.contains("\"cols\":120"));
    }

    #[test]
    fn apply_resize_minimum_one_row() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut state = make_state();
        // rows=1 means pty_rows = max(1-1, 1) = 1
        apply_action(
            InputAction::Resize { rows: 1, cols: 80 },
            &mut parser,
            &mut state,
            &tx,
            100,
            "test",
        );
        let (rows, _) = parser.screen().size();
        assert_eq!(rows, 1);
    }

    // ── drain_ws ────────────────────────────────────────────────────

    #[test]
    fn drain_ws_processes_output() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut parser = vt100::Parser::new(24, 80, 0);
        let mut state = make_state();
        tx.send(AttachEvent::Output("Hello".to_string())).unwrap();
        let closed = drain_ws(&rx, &mut parser, &mut state);
        assert!(!closed);
        let vis = virtual_terminal::read_row_text(parser.screen(), 0, 80);
        assert!(vis.contains("Hello"));
    }

    #[test]
    fn drain_ws_updates_claude_state() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut parser = vt100::Parser::new(24, 80, 0);
        let mut state = make_state();
        assert!(matches!(state.claude_state, ClaudeState::Idle));
        tx.send(AttachEvent::StateChange(ClaudeState::Thinking))
            .unwrap();
        let closed = drain_ws(&rx, &mut parser, &mut state);
        assert!(!closed);
        assert!(matches!(state.claude_state, ClaudeState::Thinking));
    }

    #[test]
    fn drain_ws_returns_true_on_close() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut parser = vt100::Parser::new(24, 80, 0);
        let mut state = make_state();
        tx.send(AttachEvent::Closed).unwrap();
        assert!(drain_ws(&rx, &mut parser, &mut state));
    }

    #[test]
    fn drain_ws_processes_all_pending() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut parser = vt100::Parser::new(24, 80, 0);
        let mut state = make_state();
        tx.send(AttachEvent::Output("A".to_string())).unwrap();
        tx.send(AttachEvent::Output("B".to_string())).unwrap();
        tx.send(AttachEvent::StateChange(ClaudeState::Responding))
            .unwrap();
        let closed = drain_ws(&rx, &mut parser, &mut state);
        assert!(!closed);
        let vis = virtual_terminal::read_row_text(parser.screen(), 0, 80);
        assert!(vis.contains("AB"));
        assert!(matches!(state.claude_state, ClaudeState::Responding));
    }

    #[test]
    fn drain_ws_empty_channel_is_noop() {
        let (_tx, rx) = std::sync::mpsc::channel();
        let mut parser = vt100::Parser::new(24, 80, 0);
        let mut state = make_state();
        let closed = drain_ws(&rx, &mut parser, &mut state);
        assert!(!closed);
    }
}
