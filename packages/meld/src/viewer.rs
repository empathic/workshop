use anyhow::{Context, Result};
use iroh::Endpoint;
use iroh::endpoint::presets;
use iroh_tickets::endpoint::EndpointTicket;
use ratatui::crossterm::event::{self, Event, KeyCode, MouseEventKind};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use tokio::sync::mpsc;

use crate::host::ALPN;
use crate::protocol::{self, host as htag, viewer as vtag};

const SCROLLBACK: usize = 10_000;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    ReadOnly,
    Requesting,
    Editing,
}

pub async fn run(ticket_str: &str) -> Result<()> {
    let ticket: EndpointTicket = ticket_str
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid ticket: {e}"))?;

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

    let (event_tx, mut event_rx) = mpsc::channel::<Event>(64);
    std::thread::spawn(move || {
        while let Ok(ev) = event::read() {
            if event_tx.blocking_send(ev).is_err() {
                break;
            }
        }
    });

    draw_message(&mut terminal, "(meld) connecting to host... q to quit")?;

    let endpoint = Endpoint::bind(presets::N0)
        .await
        .context("failed to bind iroh endpoint")?;

    let (conn, mut send, mut recv) = {
        let mut connect_fut = std::pin::pin!(async {
            let conn = endpoint
                .connect(ticket.endpoint_addr().clone(), ALPN)
                .await?;
            let streams = conn.open_bi().await?;
            Ok::<_, anyhow::Error>((conn, streams.0, streams.1))
        });
        loop {
            tokio::select! {
                result = &mut connect_fut => {
                    match result {
                        Ok(val) => break val,
                        Err(e) => { cleanup(); return Err(e); }
                    }
                }
                Some(ev) = event_rx.recv() => {
                    if matches!(ev, Event::Key(key) if matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q'))) {
                        cleanup();
                        return Ok(());
                    }
                }
            }
        }
    };

    let (mut cols, mut rows) = ratatui::crossterm::terminal::size()?;
    let mut pty_rows = rows.saturating_sub(1).max(1);

    let vp = viewport_bytes(pty_rows, cols);
    protocol::write_msg(&mut send, vtag::VIEWPORT, &vp).await?;

    let mut vt_parser = vt100::Parser::new(pty_rows, cols, SCROLLBACK);
    let mut scroll_offset: usize = 0;
    let mut mode = Mode::ReadOnly;
    let mut got_output = false;

    loop {
        if scroll_offset > 0 {
            vt_parser.screen_mut().set_scrollback(scroll_offset);
            scroll_offset = vt_parser.screen().scrollback();
        }

        let status = if !got_output {
            "(meld) connecting to host... q to quit".to_string()
        } else {
            build_status(mode, scroll_offset)
        };
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

            if got_output && scroll_offset == 0 && !hide_cursor {
                let (row, col) = cursor_pos;
                frame.set_cursor_position((content.x + col, content.y + row));
            }
        })?;

        if scroll_offset > 0 {
            vt_parser.screen_mut().set_scrollback(0);
        }

        tokio::select! {
            result = protocol::read_msg(&mut recv) => {
                let (tag, payload) = match result {
                    Ok(msg) => msg,
                    Err(_) => break,
                };
                match tag {
                    htag::OUTPUT => {
                        vt_parser.process(&payload);
                        scroll_offset = 0;
                        got_output = true;
                    }
                    htag::TURN_GRANTED => {
                        mode = Mode::Editing;
                    }
                    htag::TURN_REVOKED | htag::TURN_DENIED => {
                        mode = Mode::ReadOnly;
                    }
                    _ => {}
                }
            }
            Some(ev) = event_rx.recv() => {
                match ev {
                    Event::Key(key) => {
                        match mode {
                            Mode::ReadOnly => match key.code {
                                KeyCode::Char('q') | KeyCode::Char('Q') => break,
                                KeyCode::Char('e') | KeyCode::Char('E') if got_output => {
                                    protocol::write_msg(&mut send, vtag::REQUEST_TURN, &[]).await?;
                                    mode = Mode::Requesting;
                                }
                                KeyCode::PageUp => {
                                    scroll_offset += pty_rows as usize / 2;
                                }
                                KeyCode::PageDown => {
                                    scroll_offset = scroll_offset.saturating_sub(pty_rows as usize / 2);
                                }
                                _ => {}
                            },
                            Mode::Requesting => match key.code {
                                KeyCode::Char('q') | KeyCode::Char('Q') => {
                                    protocol::write_msg(&mut send, vtag::RELEASE_TURN, &[]).await?;
                                    break;
                                }
                                KeyCode::Esc => {
                                    protocol::write_msg(&mut send, vtag::RELEASE_TURN, &[]).await?;
                                    mode = Mode::ReadOnly;
                                }
                                KeyCode::PageUp => {
                                    scroll_offset += pty_rows as usize / 2;
                                }
                                KeyCode::PageDown => {
                                    scroll_offset = scroll_offset.saturating_sub(pty_rows as usize / 2);
                                }
                                _ => {}
                            },
                            Mode::Editing => match key.code {
                                KeyCode::Esc => {
                                    protocol::write_msg(&mut send, vtag::RELEASE_TURN, &[]).await?;
                                    mode = Mode::ReadOnly;
                                }
                                KeyCode::PageUp => {
                                    scroll_offset += pty_rows as usize / 2;
                                }
                                KeyCode::PageDown => {
                                    scroll_offset = scroll_offset.saturating_sub(pty_rows as usize / 2);
                                }
                                _ => {
                                    if let Some(bytes) = crate::key_to_bytes(&key) {
                                        protocol::write_msg(&mut send, vtag::INPUT, &bytes).await?;
                                        scroll_offset = 0;
                                    }
                                }
                            },
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
                        let vp = viewport_bytes(pty_rows, cols);
                        protocol::write_msg(&mut send, vtag::VIEWPORT, &vp).await?;
                        scroll_offset = 0;
                    }
                    _ => {}
                }
            }
        }
    }

    cleanup();
    conn.close(0u32.into(), b"done");
    Ok(())
}

fn draw_message(terminal: &mut ratatui::DefaultTerminal, msg: &str) -> Result<()> {
    terminal.draw(|frame| {
        let area = frame.area();
        frame.render_widget(
            Paragraph::new(msg).style(Style::default().dim()),
            Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1),
        );
    })?;
    Ok(())
}

fn build_status(mode: Mode, scroll_offset: usize) -> String {
    if scroll_offset > 0 {
        return format!("(meld) ↑ {} lines — scroll down to return", scroll_offset);
    }
    match mode {
        Mode::ReadOnly => "(meld) viewing [readonly] e to request edit · q to exit".into(),
        Mode::Requesting => "(meld) requesting edit access...".into(),
        Mode::Editing => "(meld) editing · Esc to release".into(),
    }
}

fn viewport_bytes(rows: u16, cols: u16) -> [u8; 4] {
    [(rows >> 8) as u8, rows as u8, (cols >> 8) as u8, cols as u8]
}
