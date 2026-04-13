use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result};
use iroh::Endpoint;
use iroh::endpoint::presets;
use iroh_tickets::endpoint::EndpointTicket;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyModifiers, MouseEventKind};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tracing::{info, warn};

use crate::protocol::{self, host as htag, viewer as vtag};
use crate::session::{Output, Session};

pub const ALPN: &[u8] = b"meld/term/0";
const SCROLLBACK: usize = 10_000;

/// Turn state broadcast from host main loop to viewer tasks.
#[derive(Clone, Default)]
struct TurnState {
    holder: Option<String>,
    requester: Option<String>,
}

/// Events from viewer tasks to host main loop.
enum TurnEvent {
    ViewerConnected,
    ViewerDisconnected,
    TurnRequested { conn_id: String },
    TurnReleased { conn_id: String },
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

    crate::draw_message(&mut terminal, "(meld) starting...")?;

    let endpoint = Endpoint::builder(presets::N0)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .context("failed to bind iroh endpoint")?;
    endpoint.online().await;

    let ticket = EndpointTicket::new(endpoint.addr());
    let view_cmd = format!("meld view {ticket}");
    let copied = arboard::Clipboard::new()
        .and_then(|mut cb| cb.set_text(&view_cmd))
        .is_ok();

    let (mut cols, mut rows) = ratatui::crossterm::terminal::size()?;
    let mut pty_rows = rows.saturating_sub(1).max(1);

    let session = Session::spawn(&cmd, &args, &cwd, pty_rows, cols, SCROLLBACK)
        .context("failed to spawn session")?;

    let mut output_rx = session.subscribe_output().await?;
    session
        .update_viewport_and_resize("host", pty_rows, cols)
        .await?;

    let mut vt_parser = vt100::Parser::new(pty_rows, cols, SCROLLBACK);
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
    let (event_notify_tx, mut event_notify_rx) = mpsc::channel::<TurnEvent>(16);
    let vc = viewer_count.clone();
    let etx = event_notify_tx.clone();
    let viewer_session = session.clone();
    let viewer_endpoint = endpoint.clone();
    let viewer_turn_rx = turn_rx.clone();
    tokio::spawn(async move {
        accept_viewers(viewer_endpoint, viewer_session, vc, etx, viewer_turn_rx).await;
    });
    drop(event_notify_tx);

    // PTY exit detection
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
    let exit_session = session.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if exit_session.get_pid().await.is_none() {
                let _ = shutdown_tx.send(());
                return;
            }
        }
    });

    let mut turn_state = TurnState::default();
    let banner_until = if copied {
        Some(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
    } else {
        None
    };

    loop {
        if scroll_offset > 0 {
            vt_parser.screen_mut().set_scrollback(scroll_offset);
            scroll_offset = vt_parser.screen().scrollback();
        }

        let n = viewer_count.load(Ordering::Relaxed);
        let status = build_status(n, &turn_state, scroll_offset);
        let show_banner = banner_until.is_some_and(|t| tokio::time::Instant::now() < t);
        let screen = vt_parser.screen();
        let cursor_pos = screen.cursor_position();
        let hide_cursor = screen.hide_cursor();

        terminal.draw(|frame| {
            let [content, status_area] =
                Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());

            frame.render_widget(crate::PtyWidget { screen }, content);
            frame.render_widget(
                Paragraph::new(status.as_str()).style(Style::default().dim()),
                status_area,
            );

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
            vt_parser.screen_mut().set_scrollback(0);
        }

        tokio::select! {
            result = output_rx.recv() => {
                match result {
                    Ok(output) => {
                        vt_parser.process(&output.data);
                        drain_output(&mut output_rx, &mut vt_parser);
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
                            if turn_state.requester.is_some() {
                                turn_state.holder = turn_state.requester.take();
                                let _ = turn_tx.send(turn_state.clone());
                            } else if turn_state.holder.is_some() {
                                turn_state.holder = None;
                                let _ = turn_tx.send(turn_state.clone());
                            }
                        } else if key.code == KeyCode::F(10) {
                            if turn_state.requester.is_some() {
                                turn_state.requester = None;
                                let _ = turn_tx.send(turn_state.clone());
                            }
                        } else if let Some(bytes) = crate::key_to_bytes(&key) {
                            let text = String::from_utf8_lossy(&bytes);
                            let _ = session.write_input(&text).await;
                            scroll_offset = 0;
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
                        vt_parser = vt100::Parser::new(pty_rows, cols, SCROLLBACK);
                        let _ = session.update_viewport_and_resize("host", pty_rows, cols).await;
                        scroll_offset = 0;
                    }
                    _ => {}
                }
            }
            Some(event) = event_notify_rx.recv() => {
                match event {
                    TurnEvent::ViewerConnected | TurnEvent::ViewerDisconnected => {}
                    TurnEvent::TurnRequested { conn_id } => {
                        if turn_state.holder.is_none() && turn_state.requester.is_none() {
                            turn_state.requester = Some(conn_id);
                        }
                    }
                    TurnEvent::TurnReleased { conn_id } => {
                        if turn_state.holder.as_deref() == Some(&conn_id) {
                            turn_state.holder = None;
                            let _ = turn_tx.send(turn_state.clone());
                        }
                        if turn_state.requester.as_deref() == Some(&conn_id) {
                            turn_state.requester = None;
                        }
                    }
                }
            }
            _ = &mut shutdown_rx => break,
        }
    }

    cleanup();
    let _ = session.stop().await;
    endpoint.close().await;
    Ok(())
}

fn build_status(viewers: usize, turn: &TurnState, scroll_offset: usize) -> String {
    if scroll_offset > 0 {
        return format!("(meld) ↑ {} lines — type to return", scroll_offset);
    }
    if let Some(ref id) = turn.requester {
        let short = &id[..8.min(id.len())];
        return format!("(meld) {short} requesting edit · F9 accept · F10 deny");
    }
    if let Some(ref id) = turn.holder {
        let short = &id[..8.min(id.len())];
        return format!("(meld) {short} editing · F9 revoke");
    }
    format!(
        "(meld) hosting [{} viewer{}]",
        viewers,
        if viewers == 1 { "" } else { "s" }
    )
}

fn drain_output(rx: &mut broadcast::Receiver<Output>, parser: &mut vt100::Parser) {
    loop {
        match rx.try_recv() {
            Ok(output) => parser.process(&output.data),
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
        tokio::spawn(async move {
            if let Err(e) = handle_viewer(conn, s, vc, etx, trx).await {
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
) -> Result<()> {
    let conn_id = conn.remote_id().to_string();
    info!("viewer connected: {}", &conn_id[..8]);
    viewer_count.fetch_add(1, Ordering::Relaxed);
    let _ = event_tx.send(TurnEvent::ViewerConnected).await;

    let result = serve_viewer(&conn, &session, &conn_id, &event_tx, turn_rx).await;

    let _ = event_tx
        .send(TurnEvent::TurnReleased {
            conn_id: conn_id.clone(),
        })
        .await;
    viewer_count.fetch_sub(1, Ordering::Relaxed);
    let _ = event_tx.send(TurnEvent::ViewerDisconnected).await;
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
) -> Result<()> {
    let (mut send, mut recv) = conn.accept_bi().await?;

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
    let replay = session.get_recent_output(64 * 1024, rows).await;
    for chunk in &replay {
        protocol::write_msg(&mut send, htag::OUTPUT, chunk.as_bytes()).await?;
    }

    let mut output_rx = session.subscribe_output().await?;

    // Track what this viewer's last-known turn state was, to send grant/revoke only on transitions
    let mut was_holder = false;
    let mut was_denied = false;

    loop {
        tokio::select! {
            result = output_rx.recv() => {
                match result {
                    Ok(output) => {
                        protocol::write_msg(&mut send, htag::OUTPUT, &output.data).await?;
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
                        // Only forward if this viewer holds the turn
                        let state = turn_rx.borrow().clone();
                        if state.holder.as_deref() == Some(conn_id) {
                            let text = String::from_utf8_lossy(&payload);
                            let _ = session.write_input(&text).await;
                        }
                    }
                    vtag::RELEASE_TURN => {
                        let _ = event_tx.send(TurnEvent::TurnReleased {
                            conn_id: conn_id.to_string(),
                        }).await;
                    }
                    _ => {}
                }
            }
            Ok(()) = turn_rx.changed() => {
                let state = turn_rx.borrow().clone();
                let is_holder = state.holder.as_deref() == Some(conn_id);
                let is_requester = state.requester.as_deref() == Some(conn_id);

                if is_holder && !was_holder {
                    protocol::write_msg(&mut send, htag::TURN_GRANTED, &[]).await?;
                    was_holder = true;
                    was_denied = false;
                } else if !is_holder && was_holder {
                    protocol::write_msg(&mut send, htag::TURN_REVOKED, &[]).await?;
                    was_holder = false;
                } else if !is_requester && !is_holder && !was_denied && !was_holder {
                    // Was requesting but now cleared (denied)
                    protocol::write_msg(&mut send, htag::TURN_DENIED, &[]).await?;
                    was_denied = true;
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
