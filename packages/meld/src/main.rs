pub mod host;
pub mod protocol;
pub mod session;
mod viewer;

use anyhow::Result;
use clap::{Parser, Subcommand};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::Widget;
use virtual_terminal::{vt100, walk_row};

#[derive(Parser)]
#[command(name = "meld", about = "P2P terminal sharing over iroh")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Share a terminal session
    Host {
        /// Command to run (default: $SHELL or /bin/bash)
        #[arg(trailing_var_arg = true)]
        command: Vec<String>,
    },
    /// View a shared terminal session (readonly)
    View {
        /// iroh ticket from the host
        ticket: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "off".parse().unwrap()),
        )
        .with_writer(std::io::stderr)
        .init();

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        ratatui::restore();
        original_hook(info);
    }));

    let cli = Cli::parse();
    match cli.command {
        Command::Host { command } => host::run(command).await,
        Command::View { ticket } => viewer::run(&ticket).await,
    }
}

// --- Shared widgets ---

pub struct PtyWidget<'a> {
    pub screen: &'a vt100::Screen,
}

impl Widget for PtyWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for row in 0..area.height {
            for cell in walk_row(self.screen, row, area.width) {
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

fn vt100_color_to_ratatui(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(n) => Color::Indexed(n),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

pub fn status_line(rest: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled("(meld) ", Style::default().fg(Color::Magenta)),
        Span::styled(rest.to_string(), Style::default().dim()),
    ])
}

pub fn draw_message(terminal: &mut ratatui::DefaultTerminal, msg: &str) -> anyhow::Result<()> {
    use ratatui::widgets::Paragraph;
    terminal.draw(|frame| {
        let area = frame.area();
        frame.render_widget(
            Paragraph::new(status_line(msg)),
            Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1),
        );
    })?;
    Ok(())
}

/// Convert a crossterm KeyEvent to the byte sequence a PTY expects.
pub fn key_to_bytes(key: &KeyEvent) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char(c) if ctrl => match c {
            'a'..='z' => Some(vec![(c as u8) - b'a' + 1]),
            '4'..='7' => Some(vec![(c as u8) - b'4' + 0x1C]),
            _ => None,
        },
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
        KeyCode::F(n @ 1..=12) => {
            let seq: &[u8] = match n {
                1 => b"\x1bOP",
                2 => b"\x1bOQ",
                3 => b"\x1bOR",
                4 => b"\x1bOS",
                5 => b"\x1b[15~",
                6 => b"\x1b[17~",
                7 => b"\x1b[18~",
                8 => b"\x1b[19~",
                9 => b"\x1b[20~",
                10 => b"\x1b[21~",
                11 => b"\x1b[23~",
                12 => b"\x1b[24~",
                _ => return None,
            };
            Some(seq.to_vec())
        }
        _ => None,
    }
}
