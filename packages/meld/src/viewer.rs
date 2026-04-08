use anyhow::{Context, Result};
use iroh::Endpoint;
use iroh::endpoint::presets;
use iroh_tickets::endpoint::EndpointTicket;
use ratatui::crossterm::event::{self, Event, KeyCode, MouseEventKind};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use tokio::sync::mpsc;
use tracing::info;

use crate::host::ALPN;

const STATUS: &str = "(meld) viewing [readonly] press q to exit";
const SCROLLBACK: usize = 10_000;

pub async fn run(ticket_str: &str) -> Result<()> {
    let ticket: EndpointTicket = ticket_str
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid ticket: {e}"))?;

    info!("connecting...");

    let endpoint = Endpoint::bind(presets::N0)
        .await
        .context("failed to bind iroh endpoint")?;

    let conn = endpoint
        .connect(ticket.endpoint_addr().clone(), ALPN)
        .await
        .context("failed to connect to host")?;

    info!("connected");

    let (mut send, mut recv) = conn.open_bi().await?;

    let (mut cols, mut rows) = ratatui::crossterm::terminal::size()?;
    let mut pty_rows = rows.saturating_sub(1).max(1);

    send_viewport(&mut send, pty_rows, cols).await?;

    let mut vt_parser = vt100::Parser::new(pty_rows, cols, SCROLLBACK);
    let mut scroll_offset: usize = 0;

    let mut terminal = ratatui::init();
    ratatui::crossterm::execute!(
        std::io::stdout(),
        ratatui::crossterm::event::EnableMouseCapture
    )?;

    // Crossterm event reader thread
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

    let mut buf = vec![0u8; 4096];

    loop {
        if scroll_offset > 0 {
            vt_parser.screen_mut().set_scrollback(scroll_offset);
            scroll_offset = vt_parser.screen().scrollback();
        }

        let status = if scroll_offset > 0 {
            format!("(meld) ↑ {} lines — scroll down to return", scroll_offset)
        } else {
            STATUS.to_string()
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

        if scroll_offset > 0 {
            vt_parser.screen_mut().set_scrollback(0);
        }

        tokio::select! {
            result = recv.read(&mut buf) => {
                match result {
                    Ok(Some(n)) => {
                        vt_parser.process(&buf[..n]);
                        scroll_offset = 0;
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
            Some(ev) = event_rx.recv() => {
                match ev {
                    Event::Key(key) => match key.code {
                        KeyCode::Char('q') | KeyCode::Char('Q') => break,
                        KeyCode::PageUp => {
                            scroll_offset += pty_rows as usize / 2;
                        }
                        KeyCode::PageDown => {
                            scroll_offset = scroll_offset.saturating_sub(pty_rows as usize / 2);
                        }
                        _ => {}
                    },
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
                        let _ = send_viewport(&mut send, pty_rows, cols).await;
                        scroll_offset = 0;
                    }
                    _ => {}
                }
            }
        }
    }

    ratatui::crossterm::execute!(
        std::io::stdout(),
        ratatui::crossterm::event::DisableMouseCapture
    )?;
    ratatui::restore();
    conn.close(0u32.into(), b"done");
    Ok(())
}

async fn send_viewport(send: &mut iroh::endpoint::SendStream, rows: u16, cols: u16) -> Result<()> {
    let buf = [
        (rows >> 8) as u8,
        rows as u8,
        (cols >> 8) as u8,
        cols as u8,
    ];
    send.write_all(&buf).await?;
    Ok(())
}
