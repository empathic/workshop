use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result};
use iroh::Endpoint;
use iroh::endpoint::presets;
use iroh_tickets::endpoint::EndpointTicket;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyModifiers, MouseEventKind};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use tokio::sync::{broadcast, mpsc, watch};
use tracing::{info, warn};

use crate::protocol::{self, host as htag, viewer as vtag};
use crate::session::Session;

pub const ALPN: &[u8] = b"meld/term/0";
const SCROLLBACK: usize = 10_000;

/// Turn state broadcast from host main loop to viewer tasks.
#[derive(Clone, Default, Debug, PartialEq)]
struct TurnState {
    holder: Option<String>,
    requester: Option<String>,
}

/// Events from viewer tasks to host main loop.
enum TurnEvent {
    ViewerConnected { conn_id: String, name: String },
    ViewerDisconnected { conn_id: String },
    TurnRequested { conn_id: String },
    TurnReleased { conn_id: String },
}

/// Side-effects emitted by `HostState` transitions.
#[derive(Debug, PartialEq)]
enum Effect {
    BroadcastTurn,
    BroadcastDenied(String),
    UpdateActiveUser,
}

/// Authoritative turn + viewer state. All methods are pure: they mutate
/// the struct and return the side-effects the caller must perform.
struct HostState {
    turn: TurnState,
    viewer_names: HashMap<String, String>,
    host_name: String,
    session_id: String,
}

impl HostState {
    fn new(host_name: String, session_id: String) -> Self {
        Self {
            turn: TurnState::default(),
            viewer_names: HashMap::new(),
            host_name,
            session_id,
        }
    }

    /// F9: promote requester → holder, or revoke current holder.
    fn accept_or_revoke(&mut self) -> Vec<Effect> {
        if self.turn.requester.is_some() {
            self.turn.holder = self.turn.requester.take();
            vec![Effect::BroadcastTurn, Effect::UpdateActiveUser]
        } else if self.turn.holder.is_some() {
            self.turn.holder = None;
            vec![Effect::BroadcastTurn, Effect::UpdateActiveUser]
        } else {
            vec![]
        }
    }

    /// F10: deny the current requester.
    fn deny(&mut self) -> Vec<Effect> {
        if let Some(req) = self.turn.requester.take() {
            vec![Effect::BroadcastDenied(req), Effect::BroadcastTurn]
        } else {
            vec![]
        }
    }

    fn viewer_connected(&mut self, conn_id: String, name: String) {
        self.viewer_names.insert(conn_id, name);
    }

    fn viewer_disconnected(&mut self, conn_id: &str) {
        self.viewer_names.remove(conn_id);
    }

    /// A viewer requests the turn. Denied if anyone already holds or is queued.
    fn turn_requested(&mut self, conn_id: String) -> Vec<Effect> {
        if self.turn.holder.is_none() && self.turn.requester.is_none() {
            self.turn.requester = Some(conn_id);
            vec![Effect::BroadcastTurn]
        } else {
            vec![Effect::BroadcastDenied(conn_id)]
        }
    }

    /// A viewer released the turn (explicit release, or connection dropped).
    fn turn_released(&mut self, conn_id: &str) -> Vec<Effect> {
        let mut effects = vec![];
        if self.turn.holder.as_deref() == Some(conn_id) {
            self.turn.holder = None;
            effects.push(Effect::BroadcastTurn);
            effects.push(Effect::UpdateActiveUser);
        }
        if self.turn.requester.as_deref() == Some(conn_id) {
            self.turn.requester = None;
        }
        effects
    }

    /// Name of whoever currently drives the session (for `active_user` and UI).
    fn active_user(&self) -> &str {
        match &self.turn.holder {
            Some(id) => self
                .viewer_names
                .get(id)
                .map(String::as_str)
                .unwrap_or(&self.host_name),
            None => &self.host_name,
        }
    }
}

fn apply_effects(
    state: &HostState,
    effects: Vec<Effect>,
    turn_tx: &watch::Sender<TurnState>,
    denied_tx: &broadcast::Sender<String>,
) {
    for eff in effects {
        match eff {
            Effect::BroadcastTurn => {
                let _ = turn_tx.send(state.turn.clone());
            }
            Effect::BroadcastDenied(id) => {
                let _ = denied_tx.send(id);
            }
            Effect::UpdateActiveUser => {
                crate::session_dir::write_active_user(&state.session_id, state.active_user());
            }
        }
    }
}

pub async fn run(command: Vec<String>) -> Result<()> {
    let (cmd, args) = resolve_command(command);
    let cwd = std::env::current_dir()?.to_string_lossy().to_string();

    let mut terminal = ratatui::init();
    ratatui::crossterm::execute!(
        std::io::stdout(),
        ratatui::crossterm::event::EnableMouseCapture
    )?;

    let cleanup = || {
        ratatui::crossterm::execute!(
            std::io::stdout(),
            ratatui::crossterm::event::DisableMouseCapture
        )
        .ok();
        ratatui::restore();
    };

    let host_name = crate::config::ensure_name(&mut terminal)?;

    crate::draw_message(&mut terminal, "starting...")?;

    let endpoint = Endpoint::builder(presets::N0)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .context("failed to bind iroh endpoint")?;
    endpoint.online().await;

    let ticket = EndpointTicket::new(endpoint.addr());
    let view_cmd = format!("meld join {ticket}");
    let copied = arboard::Clipboard::new()
        .and_then(|mut cb| cb.set_text(&view_cmd))
        .is_ok();

    let (mut cols, mut rows) = ratatui::crossterm::terminal::size()?;
    let mut pty_rows = rows.saturating_sub(1).max(1);

    let session_id = uuid::Uuid::new_v4().to_string();
    crate::session_dir::write_active_user(&session_id, &host_name);

    let session = Session::spawn(&cmd, &args, &cwd, pty_rows, cols, SCROLLBACK, &session_id)
        .context("failed to spawn session")?;

    let mut output_rx = session.subscribe_output().await?;
    let mut dims_rx = session.subscribe_dims();
    session
        .update_viewport_and_resize("host", pty_rows, cols)
        .await?;

    let (eff_rows, eff_cols) = *dims_rx.borrow();
    let mut vt =
        virtual_terminal::VirtualTerminal::new(eff_rows, eff_cols, 1024 * 1024, SCROLLBACK);
    let mut scroll_offset: usize = 0;

    // Crossterm event reader thread
    let (event_tx, mut event_rx) = mpsc::channel::<Event>(64);
    std::thread::spawn(move || {
        while let Ok(ev) = event::read() {
            if event_tx.blocking_send(ev).is_err() {
                break;
            }
        }
    });

    let viewer_count = Arc::new(AtomicUsize::new(0));
    let (turn_tx, turn_rx) = watch::channel(TurnState::default());
    let (denied_tx, _) = broadcast::channel::<String>(16);
    let (event_notify_tx, mut event_notify_rx) = mpsc::channel::<TurnEvent>(16);
    let vc = viewer_count.clone();
    let etx = event_notify_tx.clone();
    let viewer_session = session.clone();
    let viewer_endpoint = endpoint.clone();
    let viewer_turn_rx = turn_rx.clone();
    let viewer_dims_rx = dims_rx.clone();
    let viewer_denied_tx = denied_tx.clone();
    tokio::spawn(async move {
        accept_viewers(
            viewer_endpoint,
            viewer_session,
            vc,
            etx,
            viewer_turn_rx,
            viewer_dims_rx,
            viewer_denied_tx,
        )
        .await;
    });
    drop(event_notify_tx);

    let mut state = HostState::new(host_name, session_id);
    let banner_until = if copied {
        Some(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
    } else {
        None
    };

    loop {
        if scroll_offset > 0 {
            vt.screen_mut().set_scrollback(scroll_offset);
            scroll_offset = vt.screen().scrollback();
        }

        let n = viewer_count.load(Ordering::Relaxed);
        let (eff_r, eff_c) = *dims_rx.borrow();
        let status = build_status(n, &state, scroll_offset, pty_rows, cols, eff_r, eff_c);
        let show_banner = banner_until.is_some_and(|t| tokio::time::Instant::now() < t);
        let screen = vt.screen();
        let cursor_pos = screen.cursor_position();
        let hide_cursor = screen.hide_cursor();

        terminal.draw(|frame| {
            let [content, status_area] =
                Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());

            frame.render_widget(crate::PtyWidget { screen }, content);
            frame.render_widget(Paragraph::new(status.clone()), status_area);

            if show_banner {
                let msg = " viewer command copied to clipboard ";
                let w = msg.len() as u16;
                let x = content.right().saturating_sub(w);
                let banner_area = Rect::new(x, content.y, w, 1);
                frame.render_widget(
                    Paragraph::new(msg).style(Style::default().dim()),
                    banner_area,
                );
            }

            if scroll_offset == 0 && !hide_cursor {
                let (row, col) = cursor_pos;
                frame.set_cursor_position((content.x + col, content.y + row));
            }
        })?;

        if scroll_offset > 0 {
            vt.screen_mut().set_scrollback(0);
        }

        tokio::select! {
            result = output_rx.recv() => {
                match result {
                    Ok(output) => {
                        vt.process_output(&output);
                        drain_output(&mut output_rx, &mut vt);
                        scroll_offset = 0;
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            Some(ev) = event_rx.recv() => {
                match ev {
                    Event::Key(key) => {
                        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                        if shift && key.code == KeyCode::PageUp {
                            scroll_offset += pty_rows as usize / 2;
                        } else if shift && key.code == KeyCode::PageDown {
                            scroll_offset = scroll_offset.saturating_sub(pty_rows as usize / 2);
                        } else if key.code == KeyCode::F(9) {
                            let effects = state.accept_or_revoke();
                            apply_effects(&state, effects, &turn_tx, &denied_tx);
                        } else if key.code == KeyCode::F(10) {
                            let effects = state.deny();
                            apply_effects(&state, effects, &turn_tx, &denied_tx);
                        } else if state.turn.holder.is_none() {
                            if let Some(bytes) = crate::key_to_bytes(&key) {
                                let text = String::from_utf8_lossy(&bytes);
                                let _ = session.write_input(&text).await;
                                scroll_offset = 0;
                            }
                        }
                    }
                    Event::Mouse(mouse) => match mouse.kind {
                        MouseEventKind::ScrollUp => scroll_offset += 3,
                        MouseEventKind::ScrollDown => {
                            scroll_offset = scroll_offset.saturating_sub(3);
                        }
                        _ => {}
                    },
                    Event::Resize(new_cols, new_rows) => {
                        cols = new_cols;
                        rows = new_rows;
                        pty_rows = rows.saturating_sub(1).max(1);
                        let _ = session.update_viewport_and_resize("host", pty_rows, cols).await;
                        scroll_offset = 0;
                    }
                    _ => {}
                }
            }
            Ok(()) = dims_rx.changed() => {
                let (new_r, new_c) = *dims_rx.borrow();
                let (cur_r, cur_c) = vt.screen().size();
                if new_r != cur_r || new_c != cur_c {
                    vt.resize(new_r, new_c);
                    scroll_offset = 0;
                }
            }
            Some(event) = event_notify_rx.recv() => {
                match event {
                    TurnEvent::ViewerConnected { conn_id, name } => {
                        state.viewer_connected(conn_id, name);
                    }
                    TurnEvent::ViewerDisconnected { conn_id } => {
                        state.viewer_disconnected(&conn_id);
                    }
                    TurnEvent::TurnRequested { conn_id } => {
                        let effects = state.turn_requested(conn_id);
                        apply_effects(&state, effects, &turn_tx, &denied_tx);
                    }
                    TurnEvent::TurnReleased { conn_id } => {
                        let effects = state.turn_released(&conn_id);
                        apply_effects(&state, effects, &turn_tx, &denied_tx);
                    }
                }
            }
        }
    }

    cleanup();
    let _ = session.stop().await;
    endpoint.close().await;
    crate::session_dir::cleanup_session(&state.session_id);
    Ok(())
}

fn build_status(
    viewers: usize,
    state: &HostState,
    scroll_offset: usize,
    local_rows: u16,
    local_cols: u16,
    eff_rows: u16,
    eff_cols: u16,
) -> Line<'static> {
    if scroll_offset > 0 {
        return crate::status_line(&format!("↑ {} lines — type to return", scroll_offset));
    }
    let dims_note = if eff_rows != local_rows || eff_cols != local_cols {
        format!(" · {}×{}", eff_cols, eff_rows)
    } else {
        String::new()
    };
    let display_name = |conn_id: &str| -> String {
        state
            .viewer_names
            .get(conn_id)
            .cloned()
            .unwrap_or_else(|| conn_id[..8.min(conn_id.len())].to_string())
    };
    if let Some(ref id) = state.turn.requester {
        return Line::from(vec![
            crate::meld_prefix(),
            crate::fg(
                format!(
                    "{} requesting edit · F9 accept · F10 deny",
                    display_name(id)
                ),
                Color::White,
            ),
            crate::dim(dims_note.clone()),
        ]);
    }
    let msg = if let Some(ref id) = state.turn.holder {
        format!("{} editing · F9 revoke", display_name(id))
    } else {
        format!(
            "hosting [{} viewer{}]",
            viewers,
            if viewers == 1 { "" } else { "s" }
        )
    };
    crate::status_line(&format!("{msg}{dims_note}"))
}

fn drain_output(rx: &mut broadcast::Receiver<Vec<u8>>, vt: &mut virtual_terminal::VirtualTerminal) {
    loop {
        match rx.try_recv() {
            Ok(output) => vt.process_output(&output),
            Err(broadcast::error::TryRecvError::Lagged(_)) => {}
            _ => break,
        }
    }
}

async fn accept_viewers(
    endpoint: Endpoint,
    session: Session,
    viewer_count: Arc<AtomicUsize>,
    event_tx: mpsc::Sender<TurnEvent>,
    turn_rx: watch::Receiver<TurnState>,
    dims_rx: watch::Receiver<(u16, u16)>,
    denied_tx: broadcast::Sender<String>,
) {
    while let Some(incoming) = endpoint.accept().await {
        let conn = match incoming.await {
            Ok(conn) => conn,
            Err(e) => {
                warn!("failed to accept connection: {e}");
                continue;
            }
        };
        let s = session.clone();
        let vc = viewer_count.clone();
        let etx = event_tx.clone();
        let trx = turn_rx.clone();
        let drx = dims_rx.clone();
        let denied_rx = denied_tx.subscribe();
        tokio::spawn(async move {
            if let Err(e) = handle_viewer(conn, s, vc, etx, trx, drx, denied_rx).await {
                info!("viewer disconnected: {e}");
            }
        });
    }
}

async fn handle_viewer(
    conn: iroh::endpoint::Connection,
    session: Session,
    viewer_count: Arc<AtomicUsize>,
    event_tx: mpsc::Sender<TurnEvent>,
    turn_rx: watch::Receiver<TurnState>,
    dims_rx: watch::Receiver<(u16, u16)>,
    denied_rx: broadcast::Receiver<String>,
) -> Result<()> {
    let conn_id = conn.remote_id().to_string();
    viewer_count.fetch_add(1, Ordering::Relaxed);

    let result = serve_viewer(
        &conn, &session, &conn_id, &event_tx, turn_rx, dims_rx, denied_rx,
    )
    .await;

    let _ = event_tx
        .send(TurnEvent::TurnReleased {
            conn_id: conn_id.clone(),
        })
        .await;
    viewer_count.fetch_sub(1, Ordering::Relaxed);
    let _ = event_tx
        .send(TurnEvent::ViewerDisconnected {
            conn_id: conn_id.clone(),
        })
        .await;
    let _ = session.remove_client_and_resize(&conn_id).await;
    info!("viewer disconnected: {}", &conn_id[..8]);
    result
}

async fn serve_viewer(
    conn: &iroh::endpoint::Connection,
    session: &Session,
    conn_id: &str,
    event_tx: &mpsc::Sender<TurnEvent>,
    mut turn_rx: watch::Receiver<TurnState>,
    mut dims_rx: watch::Receiver<(u16, u16)>,
    mut denied_rx: broadcast::Receiver<String>,
) -> Result<()> {
    let (mut send, mut recv) = conn.accept_bi().await?;

    // Read Hello (viewer name)
    let (tag, payload) = protocol::read_msg(&mut recv).await?;
    anyhow::ensure!(tag == vtag::HELLO, "expected hello");
    let viewer_name = String::from_utf8_lossy(&payload).to_string();
    info!("viewer connected: {viewer_name}");

    let _ = event_tx
        .send(TurnEvent::ViewerConnected {
            conn_id: conn_id.to_string(),
            name: viewer_name,
        })
        .await;

    // Read viewport
    let (tag, payload) = protocol::read_msg(&mut recv).await?;
    anyhow::ensure!(
        tag == vtag::VIEWPORT && payload.len() == 4,
        "expected viewport"
    );
    let rows = u16::from_be_bytes([payload[0], payload[1]]).max(1);
    let cols = u16::from_be_bytes([payload[2], payload[3]]).max(1);

    session
        .update_viewport_and_resize(conn_id, rows, cols)
        .await?;

    // Send replay
    let replay = session.get_replay(rows).await;
    protocol::write_msg(&mut send, htag::OUTPUT, &replay).await?;

    let mut output_rx = session.subscribe_output().await?;
    let mut was_holder = false;

    loop {
        tokio::select! {
            result = output_rx.recv() => {
                match result {
                    Ok(output) => {
                        protocol::write_msg(&mut send, htag::OUTPUT, &output).await?;
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("viewer {} lagged by {n}", &conn_id[..8]);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            result = protocol::read_msg(&mut recv) => {
                let (tag, payload) = result?;
                match tag {
                    vtag::VIEWPORT => {
                        if payload.len() == 4 {
                            let rows = u16::from_be_bytes([payload[0], payload[1]]).max(1);
                            let cols = u16::from_be_bytes([payload[2], payload[3]]).max(1);
                            let _ = session.update_viewport_and_resize(conn_id, rows, cols).await;
                        }
                    }
                    vtag::REQUEST_TURN => {
                        let _ = event_tx.send(TurnEvent::TurnRequested {
                            conn_id: conn_id.to_string(),
                        }).await;
                    }
                    vtag::INPUT => {
                        let state = turn_rx.borrow().clone();
                        if state.holder.as_deref() == Some(conn_id) {
                            let text = String::from_utf8_lossy(&payload);
                            let _ = session.write_input(&text).await;
                        }
                    }
                    vtag::RELEASE_TURN => {
                        let _ = event_tx
                            .send(TurnEvent::TurnReleased {
                                conn_id: conn_id.to_string(),
                            })
                            .await;
                    }
                    vtag::GOODBYE => break,
                    _ => {}
                }
            }
            Ok(()) = dims_rx.changed() => {
                let (r, c) = *dims_rx.borrow();
                let payload = [(r >> 8) as u8, r as u8, (c >> 8) as u8, c as u8];
                protocol::write_msg(&mut send, htag::DIMS_CHANGED, &payload).await?;
            }
            Ok(()) = turn_rx.changed() => {
                let is_holder = turn_rx.borrow().holder.as_deref() == Some(conn_id);
                if is_holder && !was_holder {
                    protocol::write_msg(&mut send, htag::TURN_GRANTED, &[]).await?;
                    was_holder = true;
                } else if !is_holder && was_holder {
                    protocol::write_msg(&mut send, htag::TURN_REVOKED, &[]).await?;
                    was_holder = false;
                }
            }
            Ok(denied_id) = denied_rx.recv() => {
                if denied_id == conn_id {
                    protocol::write_msg(&mut send, htag::TURN_DENIED, &[]).await?;
                }
            }
        }
    }

    Ok(())
}

fn resolve_command(command: Vec<String>) -> (String, Vec<String>) {
    if command.is_empty() {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        (shell, vec![])
    } else {
        let cmd = command[0].clone();
        let args = command[1..].to_vec();
        (cmd, args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> HostState {
        HostState::new("host".into(), "sess".into())
    }

    #[test]
    fn initial_state_is_empty() {
        let s = state();
        assert_eq!(s.turn, TurnState::default());
        assert!(s.viewer_names.is_empty());
        assert_eq!(s.active_user(), "host");
    }

    #[test]
    fn turn_requested_sets_requester_when_idle() {
        let mut s = state();
        let effects = s.turn_requested("alice".into());
        assert_eq!(s.turn.requester.as_deref(), Some("alice"));
        assert_eq!(effects, vec![Effect::BroadcastTurn]);
    }

    #[test]
    fn turn_requested_denied_when_holder_exists() {
        let mut s = state();
        s.turn.holder = Some("bob".into());
        let effects = s.turn_requested("alice".into());
        assert_eq!(s.turn.requester, None);
        assert_eq!(effects, vec![Effect::BroadcastDenied("alice".into())]);
    }

    #[test]
    fn turn_requested_denied_when_requester_already_queued() {
        let mut s = state();
        s.turn.requester = Some("bob".into());
        let effects = s.turn_requested("alice".into());
        assert_eq!(s.turn.requester.as_deref(), Some("bob"));
        assert_eq!(effects, vec![Effect::BroadcastDenied("alice".into())]);
    }

    #[test]
    fn f9_promotes_requester_to_holder() {
        let mut s = state();
        s.viewer_connected("alice".into(), "Alice".into());
        s.turn_requested("alice".into());
        let effects = s.accept_or_revoke();
        assert_eq!(s.turn.holder.as_deref(), Some("alice"));
        assert_eq!(s.turn.requester, None);
        assert_eq!(
            effects,
            vec![Effect::BroadcastTurn, Effect::UpdateActiveUser]
        );
        assert_eq!(s.active_user(), "Alice");
    }

    #[test]
    fn f9_revokes_holder_when_no_requester() {
        let mut s = state();
        s.viewer_connected("alice".into(), "Alice".into());
        s.turn.holder = Some("alice".into());
        let effects = s.accept_or_revoke();
        assert_eq!(s.turn.holder, None);
        assert_eq!(
            effects,
            vec![Effect::BroadcastTurn, Effect::UpdateActiveUser]
        );
        assert_eq!(s.active_user(), "host");
    }

    #[test]
    fn f9_is_noop_when_idle() {
        let mut s = state();
        let effects = s.accept_or_revoke();
        assert!(effects.is_empty());
        assert_eq!(s.turn, TurnState::default());
    }

    #[test]
    fn f10_denies_requester_and_broadcasts_identity() {
        let mut s = state();
        s.turn_requested("alice".into());
        let effects = s.deny();
        assert_eq!(s.turn.requester, None);
        assert_eq!(
            effects,
            vec![
                Effect::BroadcastDenied("alice".into()),
                Effect::BroadcastTurn,
            ],
        );
    }

    #[test]
    fn f10_is_noop_when_no_requester() {
        let mut s = state();
        let effects = s.deny();
        assert!(effects.is_empty());
    }

    #[test]
    fn f10_does_not_affect_holder() {
        let mut s = state();
        s.turn.holder = Some("alice".into());
        let effects = s.deny();
        assert!(effects.is_empty());
        assert_eq!(s.turn.holder.as_deref(), Some("alice"));
    }

    #[test]
    fn turn_released_clears_holder_and_broadcasts() {
        let mut s = state();
        s.viewer_connected("alice".into(), "Alice".into());
        s.turn.holder = Some("alice".into());
        let effects = s.turn_released("alice");
        assert_eq!(s.turn.holder, None);
        assert_eq!(
            effects,
            vec![Effect::BroadcastTurn, Effect::UpdateActiveUser]
        );
    }

    #[test]
    fn turn_released_by_requester_clears_silently() {
        let mut s = state();
        s.turn.requester = Some("alice".into());
        let effects = s.turn_released("alice");
        // Requester change is not broadcast — the next turn_tx send will reflect it,
        // but releasing the queue slot alone is not a turn-state change viewers care about.
        assert_eq!(s.turn.requester, None);
        assert!(effects.is_empty());
    }

    #[test]
    fn turn_released_by_stranger_is_noop() {
        let mut s = state();
        s.turn.holder = Some("alice".into());
        let effects = s.turn_released("eve");
        assert!(effects.is_empty());
        assert_eq!(s.turn.holder.as_deref(), Some("alice"));
    }

    #[test]
    fn turn_released_clears_both_slots_if_same_conn() {
        let mut s = state();
        s.turn.holder = Some("alice".into());
        s.turn.requester = Some("alice".into());
        let effects = s.turn_released("alice");
        assert_eq!(s.turn, TurnState::default());
        assert_eq!(
            effects,
            vec![Effect::BroadcastTurn, Effect::UpdateActiveUser]
        );
    }

    #[test]
    fn viewer_disconnect_removes_name() {
        let mut s = state();
        s.viewer_connected("alice".into(), "Alice".into());
        s.viewer_disconnected("alice");
        assert!(s.viewer_names.is_empty());
    }

    #[test]
    fn active_user_falls_back_to_host_if_holder_name_missing() {
        // If a viewer held the turn and then disconnected without the main
        // loop seeing the release first, the name map might not have them.
        let mut s = state();
        s.turn.holder = Some("ghost".into());
        assert_eq!(s.active_user(), "host");
    }

    #[test]
    fn request_grant_revoke_round_trip() {
        let mut s = state();
        s.viewer_connected("alice".into(), "Alice".into());

        s.turn_requested("alice".into());
        assert_eq!(s.turn.requester.as_deref(), Some("alice"));

        let _ = s.accept_or_revoke();
        assert_eq!(s.turn.holder.as_deref(), Some("alice"));
        assert_eq!(s.turn.requester, None);
        assert_eq!(s.active_user(), "Alice");

        let _ = s.accept_or_revoke();
        assert_eq!(s.turn.holder, None);
        assert_eq!(s.active_user(), "host");
    }
}
