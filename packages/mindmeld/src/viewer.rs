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

    let viewer_name = crate::config::ensure_name(&mut terminal)?;

    let (event_tx, mut event_rx) = mpsc::channel::<Event>(64);
    std::thread::spawn(move || {
        while let Ok(ev) = event::read() {
            if event_tx.blocking_send(ev).is_err() {
                break;
            }
        }
    });

    crate::draw_message(&mut terminal, "connecting to host... q to quit")?;

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

    protocol::write_msg(&mut send, vtag::HELLO, viewer_name.as_bytes()).await?;
    let vp = viewport_bytes(pty_rows, cols);
    protocol::write_msg(&mut send, vtag::VIEWPORT, &vp).await?;

    let mut eff_rows = pty_rows;
    let mut eff_cols = cols;
    let mut vt =
        virtual_terminal::VirtualTerminal::new(eff_rows, eff_cols, 1024 * 1024, SCROLLBACK);
    let mut scroll_offset: usize = 0;
    let mut mode = Mode::ReadOnly;
    let mut got_output = false;
    let mut denied_until: Option<tokio::time::Instant> = None;

    loop {
        if scroll_offset > 0 {
            vt.screen_mut().set_scrollback(scroll_offset);
            scroll_offset = vt.screen().scrollback();
        }

        let show_denied = denied_until.is_some_and(|t| tokio::time::Instant::now() < t);
        let status = if !got_output {
            crate::status_line("connecting to host... q to quit")
        } else if show_denied {
            Line::from(vec![
                crate::meld_prefix(),
                crate::fg("edit request denied", Color::White),
            ])
        } else {
            build_status(mode, scroll_offset, pty_rows, cols, eff_rows, eff_cols)
        };
        let screen = vt.screen();
        let cursor_pos = screen.cursor_position();
        let hide_cursor = screen.hide_cursor();

        terminal.draw(|frame| {
            let [content, status_area] =
                Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());

            frame.render_widget(crate::PtyWidget { screen }, content);
            frame.render_widget(Paragraph::new(status.clone()), status_area);

            if got_output && scroll_offset == 0 && !hide_cursor {
                let (row, col) = cursor_pos;
                frame.set_cursor_position((content.x + col, content.y + row));
            }
        })?;

        if scroll_offset > 0 {
            vt.screen_mut().set_scrollback(0);
        }

        let banner_deadline = denied_until
            .unwrap_or_else(|| tokio::time::Instant::now() + std::time::Duration::from_secs(86400));

        tokio::select! {
            _ = tokio::time::sleep_until(banner_deadline), if denied_until.is_some() => {
                denied_until = None;
            }
            result = protocol::read_msg(&mut recv) => {
                let (tag, payload) = match result {
                    Ok(msg) => msg,
                    Err(_) => break,
                };
                match tag {
                    htag::OUTPUT => {
                        vt.process_output(&payload);
                        scroll_offset = 0;
                        got_output = true;
                    }
                    htag::TURN_GRANTED => {
                        mode = Mode::Editing;
                    }
                    htag::TURN_REVOKED => {
                        mode = Mode::ReadOnly;
                    }
                    htag::TURN_DENIED => {
                        mode = Mode::ReadOnly;
                        denied_until = Some(
                            tokio::time::Instant::now() + std::time::Duration::from_secs(3),
                        );
                    }
                    htag::DIMS_CHANGED if payload.len() == 4 => {
                        let new_r = u16::from_be_bytes([payload[0], payload[1]]);
                        let new_c = u16::from_be_bytes([payload[2], payload[3]]);
                        if new_r != eff_rows || new_c != eff_cols {
                            eff_rows = new_r;
                            eff_cols = new_c;
                            vt.resize(eff_rows, eff_cols);
                            scroll_offset = 0;
                        }
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
                                KeyCode::F(9) if got_output => {
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
                                KeyCode::F(9) => {
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
                                KeyCode::F(9) => {
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
                        vt.resize(pty_rows, cols);
                        let vp = viewport_bytes(pty_rows, cols);
                        protocol::write_msg(&mut send, vtag::VIEWPORT, &vp).await?;
                        scroll_offset = 0;
                    }
                    _ => {}
                }
            }
        }
    }

    let _ = protocol::write_msg(&mut send, vtag::GOODBYE, &[]).await;
    let _ = send.finish();
    drop(send);
    drop(recv);
    conn.close(0u32.into(), b"done");
    cleanup();
    Ok(())
}

fn build_status(
    mode: Mode,
    scroll_offset: usize,
    local_rows: u16,
    local_cols: u16,
    eff_rows: u16,
    eff_cols: u16,
) -> Line<'static> {
    if scroll_offset > 0 {
        return crate::status_line(&format!(
            "↑ {} lines — scroll down to return",
            scroll_offset
        ));
    }
    let dims_note = if eff_rows != local_rows || eff_cols != local_cols {
        format!(" · {}×{}", eff_cols, eff_rows)
    } else {
        String::new()
    };
    let msg = match mode {
        Mode::ReadOnly => "viewing [readonly] F9 to request edit · q to exit",
        Mode::Requesting => "requesting edit access... F9 to cancel",
        Mode::Editing => "editing · F9 to release",
    };
    crate::status_line(&format!("{msg}{dims_note}"))
}

fn viewport_bytes(rows: u16, cols: u16) -> [u8; 4] {
    [(rows >> 8) as u8, rows as u8, (cols >> 8) as u8, cols as u8]
}
