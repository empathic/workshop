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
use tokio::sync::{RwLock, broadcast, mpsc, oneshot};
use tracing::{info, warn};

use workshop::instance_actor::{InstanceHandle, SpawnOptions, create_instance};
use workshop::instance_manager::InstanceKind;
use workshop::process_driver::ShellDriver;
use workshop::virtual_terminal::ClientType;

pub const ALPN: &[u8] = b"meld/term/0";
const SCROLLBACK: usize = 10_000;

pub async fn run(command: Vec<String>) -> Result<()> {
    let (cmd, args) = resolve_command(command);
    let cwd = std::env::current_dir()?.to_string_lossy().to_string();

    let handle = create_instance(SpawnOptions {
        name: "meld".into(),
        display_command: cmd.clone(),
        actual_command: cmd.clone(),
        args: args.clone(),
        working_dir: cwd,
        kind: InstanceKind::Unstructured {
            label: Some("meld".into()),
        },
        max_buffer_bytes: 1024 * 1024,
        scrollback_lines: SCROLLBACK,
        vt_record_dir: None,
        driver: Box::new(ShellDriver),
        state_broadcast_tx: None,
        lifecycle_tx: None,
        claimed_sessions: Arc::new(RwLock::new(HashMap::new())),
        first_input_data: Arc::new(RwLock::new(HashMap::new())),
        pending_attributions: Arc::new(RwLock::new(HashMap::new())),
        repository: None,
    })
    .await
    .context("failed to spawn instance")?;

    let mut output_rx = handle.subscribe_output().await?;

    let endpoint = Endpoint::builder(presets::N0)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .context("failed to bind iroh endpoint")?;
    endpoint.online().await;

    let ticket = EndpointTicket::new(endpoint.addr());
    eprintln!();
    eprintln!("  viewers can connect with:");
    eprintln!();
    eprintln!("  meld view {ticket}");
    eprintln!();
    eprint!("  press enter to start session...");
    let _ = std::io::stdin().read_line(&mut String::new());

    let (mut cols, mut rows) = ratatui::crossterm::terminal::size()?;
    let mut pty_rows = rows.saturating_sub(1).max(1);

    handle
        .update_viewport_and_resize("host", pty_rows, cols, ClientType::Terminal)
        .await?;

    let mut vt_parser = vt100::Parser::new(pty_rows, cols, SCROLLBACK);
    let mut scroll_offset: usize = 0;

    let mut terminal = ratatui::init();
    ratatui::crossterm::execute!(
        std::io::stdout(),
        ratatui::crossterm::event::EnableMouseCapture
    )?;

    // Crossterm event reader thread — replaces stdin thread + SIGWINCH
    let (event_tx, mut event_rx) = mpsc::channel::<Event>(64);
    std::thread::spawn(move || {
        loop {
            match event::read() {
                Ok(ev) => {
                    if event_tx.blocking_send(ev).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Viewer accept loop
    let viewer_count = Arc::new(AtomicUsize::new(0));
    let (viewer_changed_tx, mut viewer_changed_rx) = mpsc::channel::<()>(4);
    let vc = viewer_count.clone();
    let vtx = viewer_changed_tx.clone();
    let viewer_handle = handle.clone();
    let viewer_endpoint = endpoint.clone();
    tokio::spawn(async move {
        accept_viewers(viewer_endpoint, viewer_handle, vc, vtx).await;
    });
    drop(viewer_changed_tx);

    // PTY exit detection
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
    let exit_handle = handle.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if exit_handle.get_pid().await.is_none() {
                let _ = shutdown_tx.send(());
                return;
            }
        }
    });

    loop {
        // Set scrollback viewport for rendering
        if scroll_offset > 0 {
            vt_parser.screen_mut().set_scrollback(scroll_offset);
            scroll_offset = vt_parser.screen().scrollback();
        }

        let n = viewer_count.load(Ordering::Relaxed);
        let status = if scroll_offset > 0 {
            format!("(meld) ↑ {} lines — type to return", scroll_offset)
        } else {
            host_status(n)
        };
        let screen = vt_parser.screen();
        let cursor_pos = screen.cursor_position();
        let hide_cursor = screen.hide_cursor();

        terminal.draw(|frame| {
            let [content, status_area] =
                Layout::vertical([Constraint::Min(1), Constraint::Length(1)])
                    .areas(frame.area());

            frame.render_widget(crate::PtyWidget { screen }, content);
            frame.render_widget(
                Paragraph::new(status.as_str()).style(Style::default().dim()),
                status_area,
            );

            if scroll_offset == 0 && !hide_cursor {
                let (row, col) = cursor_pos;
                frame.set_cursor_position((content.x + col, content.y + row));
            }
        })?;

        // Restore scrollback viewport
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
                        } else if let Some(bytes) = crate::key_to_bytes(&key) {
                            let text = String::from_utf8_lossy(&bytes);
                            let _ = handle.write_input(&text).await;
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
                        let _ = handle.update_viewport_and_resize(
                            "host", pty_rows, cols, ClientType::Terminal,
                        ).await;
                        scroll_offset = 0;
                    }
                    _ => {}
                }
            }
            _ = viewer_changed_rx.recv() => {}
            _ = &mut shutdown_rx => break,
        }
    }

    ratatui::crossterm::execute!(
        std::io::stdout(),
        ratatui::crossterm::event::DisableMouseCapture
    )?;
    ratatui::restore();
    let _ = handle.stop().await;
    endpoint.close().await;
    Ok(())
}

fn drain_output(
    rx: &mut broadcast::Receiver<workshop::instance_actor::EnrichedOutput>,
    parser: &mut vt100::Parser,
) {
    loop {
        match rx.try_recv() {
            Ok(output) => parser.process(&output.data),
            Err(broadcast::error::TryRecvError::Lagged(_)) => {}
            _ => break,
        }
    }
}

fn host_status(viewers: usize) -> String {
    format!(
        "(meld) hosting [{} viewer{}]",
        viewers,
        if viewers == 1 { "" } else { "s" }
    )
}

async fn accept_viewers(
    endpoint: Endpoint,
    handle: InstanceHandle,
    viewer_count: Arc<AtomicUsize>,
    viewer_changed_tx: mpsc::Sender<()>,
) {
    while let Some(incoming) = endpoint.accept().await {
        let conn = match incoming.await {
            Ok(conn) => conn,
            Err(e) => {
                warn!("failed to accept connection: {e}");
                continue;
            }
        };
        let h = handle.clone();
        let vc = viewer_count.clone();
        let vtx = viewer_changed_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_viewer(conn, h, vc, vtx).await {
                info!("viewer disconnected: {e}");
            }
        });
    }
}

async fn handle_viewer(
    conn: iroh::endpoint::Connection,
    handle: InstanceHandle,
    viewer_count: Arc<AtomicUsize>,
    viewer_changed_tx: mpsc::Sender<()>,
) -> Result<()> {
    let conn_id = conn.remote_id().to_string();
    info!("viewer connected: {}", &conn_id[..8]);
    viewer_count.fetch_add(1, Ordering::Relaxed);
    let _ = viewer_changed_tx.send(()).await;

    let result = serve_viewer(&conn, &handle, &conn_id).await;

    viewer_count.fetch_sub(1, Ordering::Relaxed);
    let _ = viewer_changed_tx.send(()).await;
    let _ = handle.remove_client_and_resize(&conn_id).await;
    info!("viewer disconnected: {}", &conn_id[..8]);
    result
}

async fn serve_viewer(
    conn: &iroh::endpoint::Connection,
    handle: &InstanceHandle,
    conn_id: &str,
) -> Result<()> {
    let (mut send, mut recv) = conn.accept_bi().await?;

    let mut buf = [0u8; 4];
    recv.read_exact(&mut buf).await?;
    let rows = u16::from_be_bytes([buf[0], buf[1]]).max(1);
    let cols = u16::from_be_bytes([buf[2], buf[3]]).max(1);

    handle
        .update_viewport_and_resize(conn_id, rows, cols, ClientType::Terminal)
        .await?;

    let replay = handle.get_recent_output(64 * 1024, rows).await;
    for chunk in &replay {
        send.write_all(chunk.as_bytes()).await?;
    }

    let mut output_rx = handle.subscribe_output().await?;

    loop {
        tokio::select! {
            result = output_rx.recv() => {
                match result {
                    Ok(output) => send.write_all(&output.data).await?,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("viewer {} lagged by {n}", &conn_id[..8]);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            result = recv.read_exact(&mut buf) => {
                match result {
                    Ok(()) => {
                        let rows = u16::from_be_bytes([buf[0], buf[1]]).max(1);
                        let cols = u16::from_be_bytes([buf[2], buf[3]]).max(1);
                        let _ = handle.update_viewport_and_resize(conn_id, rows, cols, ClientType::Terminal).await;
                    }
                    Err(_) => break,
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
