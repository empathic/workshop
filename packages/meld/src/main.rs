pub mod config;
pub mod host;
pub mod protocol;
pub mod session;
pub mod session_dir;
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

/// xterm modifier encoding: 1 + shift + alt*2 + ctrl*4 (range 1..=8, where 1 = no mods).
fn modifier_code(m: KeyModifiers) -> u8 {
    let mut code = 1;
    if m.contains(KeyModifiers::SHIFT) {
        code += 1;
    }
    if m.contains(KeyModifiers::ALT) {
        code += 2;
    }
    if m.contains(KeyModifiers::CONTROL) {
        code += 4;
    }
    code
}

/// CSI-encoded sequence for keys that support xterm modifier encoding
/// (arrows, Home/End, page/insert/delete, F1-F12). Returns `None` for
/// other keycodes so the caller can handle them (chars, ctrl-letters, etc).
fn csi_key(code: KeyCode, mod_code: u8) -> Option<Vec<u8>> {
    if let Some(letter) = match code {
        KeyCode::Up => Some('A'),
        KeyCode::Down => Some('B'),
        KeyCode::Right => Some('C'),
        KeyCode::Left => Some('D'),
        KeyCode::Home => Some('H'),
        KeyCode::End => Some('F'),
        _ => None,
    } {
        return Some(if mod_code == 1 {
            format!("\x1b[{letter}").into_bytes()
        } else {
            format!("\x1b[1;{mod_code}{letter}").into_bytes()
        });
    }

    if let Some(letter) = match code {
        KeyCode::F(1) => Some('P'),
        KeyCode::F(2) => Some('Q'),
        KeyCode::F(3) => Some('R'),
        KeyCode::F(4) => Some('S'),
        _ => None,
    } {
        return Some(if mod_code == 1 {
            format!("\x1bO{letter}").into_bytes()
        } else {
            format!("\x1b[1;{mod_code}{letter}").into_bytes()
        });
    }

    let num: Option<u8> = match code {
        KeyCode::Insert => Some(2),
        KeyCode::Delete => Some(3),
        KeyCode::PageUp => Some(5),
        KeyCode::PageDown => Some(6),
        KeyCode::F(5) => Some(15),
        KeyCode::F(6) => Some(17),
        KeyCode::F(7) => Some(18),
        KeyCode::F(8) => Some(19),
        KeyCode::F(9) => Some(20),
        KeyCode::F(10) => Some(21),
        KeyCode::F(11) => Some(23),
        KeyCode::F(12) => Some(24),
        _ => None,
    };
    num.map(|n| {
        if mod_code == 1 {
            format!("\x1b[{n}~").into_bytes()
        } else {
            format!("\x1b[{n};{mod_code}~").into_bytes()
        }
    })
}

/// Convert a crossterm KeyEvent to the byte sequence a PTY expects.
pub fn key_to_bytes(key: &KeyEvent) -> Option<Vec<u8>> {
    let mod_code = modifier_code(key.modifiers);
    if let Some(bytes) = csi_key(key.code, mod_code) {
        return Some(bytes);
    }

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let mut bytes = match key.code {
        KeyCode::Char(' ') if ctrl => vec![0x00],
        KeyCode::Char(c) if ctrl => match c {
            'a'..='z' => vec![(c as u8) - b'a' + 1],
            '4'..='7' => vec![(c as u8) - b'4' + 0x1C],
            _ => return None,
        },
        KeyCode::Char(c) => {
            let mut buf = [0u8; 4];
            c.encode_utf8(&mut buf).as_bytes().to_vec()
        }
        KeyCode::Enter => b"\r".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => b"\x1b".to_vec(),
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        _ => return None,
    };
    if alt {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyEventKind;

    fn k(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new_with_kind(code, modifiers, KeyEventKind::Press)
    }

    #[test]
    fn plain_ascii() {
        assert_eq!(
            key_to_bytes(&k(KeyCode::Char('a'), KeyModifiers::NONE)),
            Some(b"a".to_vec())
        );
        assert_eq!(
            key_to_bytes(&k(KeyCode::Char('Z'), KeyModifiers::SHIFT)),
            Some(b"Z".to_vec())
        );
    }

    #[test]
    fn ctrl_letters() {
        for (c, expected) in [('a', 0x01), ('m', 0x0d), ('z', 0x1a)] {
            assert_eq!(
                key_to_bytes(&k(KeyCode::Char(c), KeyModifiers::CONTROL)),
                Some(vec![expected]),
            );
        }
    }

    #[test]
    fn ctrl_space_is_nul() {
        assert_eq!(
            key_to_bytes(&k(KeyCode::Char(' '), KeyModifiers::CONTROL)),
            Some(vec![0x00]),
        );
    }

    #[test]
    fn ctrl_digit_range() {
        // 4/5/6/7 → 0x1C..0x1F (FS/GS/RS/US, the xterm mapping for C-\ C-] C-^ C-_).
        assert_eq!(
            key_to_bytes(&k(KeyCode::Char('4'), KeyModifiers::CONTROL)),
            Some(vec![0x1C])
        );
        assert_eq!(
            key_to_bytes(&k(KeyCode::Char('7'), KeyModifiers::CONTROL)),
            Some(vec![0x1F])
        );
    }

    #[test]
    fn alt_prepends_escape() {
        assert_eq!(
            key_to_bytes(&k(KeyCode::Char('a'), KeyModifiers::ALT)),
            Some(vec![0x1b, b'a']),
        );
    }

    #[test]
    fn ctrl_alt_composes() {
        assert_eq!(
            key_to_bytes(&k(
                KeyCode::Char('a'),
                KeyModifiers::CONTROL | KeyModifiers::ALT
            )),
            Some(vec![0x1b, 0x01]),
        );
    }

    #[test]
    fn arrows() {
        assert_eq!(
            key_to_bytes(&k(KeyCode::Up, KeyModifiers::NONE)),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            key_to_bytes(&k(KeyCode::Down, KeyModifiers::NONE)),
            Some(b"\x1b[B".to_vec())
        );
        assert_eq!(
            key_to_bytes(&k(KeyCode::Right, KeyModifiers::NONE)),
            Some(b"\x1b[C".to_vec())
        );
        assert_eq!(
            key_to_bytes(&k(KeyCode::Left, KeyModifiers::NONE)),
            Some(b"\x1b[D".to_vec())
        );
    }

    #[test]
    fn function_keys() {
        assert_eq!(
            key_to_bytes(&k(KeyCode::F(1), KeyModifiers::NONE)),
            Some(b"\x1bOP".to_vec())
        );
        assert_eq!(
            key_to_bytes(&k(KeyCode::F(5), KeyModifiers::NONE)),
            Some(b"\x1b[15~".to_vec())
        );
        assert_eq!(
            key_to_bytes(&k(KeyCode::F(12), KeyModifiers::NONE)),
            Some(b"\x1b[24~".to_vec())
        );
    }

    #[test]
    fn enter_backspace_esc_tab() {
        assert_eq!(
            key_to_bytes(&k(KeyCode::Enter, KeyModifiers::NONE)),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            key_to_bytes(&k(KeyCode::Backspace, KeyModifiers::NONE)),
            Some(vec![0x7f])
        );
        assert_eq!(
            key_to_bytes(&k(KeyCode::Esc, KeyModifiers::NONE)),
            Some(b"\x1b".to_vec())
        );
        assert_eq!(
            key_to_bytes(&k(KeyCode::Tab, KeyModifiers::NONE)),
            Some(b"\t".to_vec())
        );
    }

    #[test]
    fn modifier_code_table() {
        assert_eq!(modifier_code(KeyModifiers::NONE), 1);
        assert_eq!(modifier_code(KeyModifiers::SHIFT), 2);
        assert_eq!(modifier_code(KeyModifiers::ALT), 3);
        assert_eq!(modifier_code(KeyModifiers::SHIFT | KeyModifiers::ALT), 4);
        assert_eq!(modifier_code(KeyModifiers::CONTROL), 5);
        assert_eq!(
            modifier_code(KeyModifiers::CONTROL | KeyModifiers::SHIFT),
            6
        );
        assert_eq!(modifier_code(KeyModifiers::CONTROL | KeyModifiers::ALT), 7);
        assert_eq!(
            modifier_code(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT),
            8
        );
    }

    #[test]
    fn alt_arrow() {
        assert_eq!(
            key_to_bytes(&k(KeyCode::Up, KeyModifiers::ALT)),
            Some(b"\x1b[1;3A".to_vec()),
        );
    }

    #[test]
    fn ctrl_arrow() {
        assert_eq!(
            key_to_bytes(&k(KeyCode::Right, KeyModifiers::CONTROL)),
            Some(b"\x1b[1;5C".to_vec()),
        );
    }

    #[test]
    fn shift_arrow() {
        assert_eq!(
            key_to_bytes(&k(KeyCode::Left, KeyModifiers::SHIFT)),
            Some(b"\x1b[1;2D".to_vec()),
        );
    }

    #[test]
    fn ctrl_alt_arrow() {
        assert_eq!(
            key_to_bytes(&k(KeyCode::Down, KeyModifiers::CONTROL | KeyModifiers::ALT)),
            Some(b"\x1b[1;7B".to_vec()),
        );
    }

    #[test]
    fn modified_home_end() {
        assert_eq!(
            key_to_bytes(&k(KeyCode::Home, KeyModifiers::ALT)),
            Some(b"\x1b[1;3H".to_vec()),
        );
        assert_eq!(
            key_to_bytes(&k(KeyCode::End, KeyModifiers::SHIFT)),
            Some(b"\x1b[1;2F".to_vec()),
        );
    }

    #[test]
    fn modified_f1_through_f4() {
        assert_eq!(
            key_to_bytes(&k(KeyCode::F(1), KeyModifiers::SHIFT)),
            Some(b"\x1b[1;2P".to_vec()),
        );
        assert_eq!(
            key_to_bytes(&k(KeyCode::F(4), KeyModifiers::CONTROL)),
            Some(b"\x1b[1;5S".to_vec()),
        );
    }

    #[test]
    fn shift_f5() {
        assert_eq!(
            key_to_bytes(&k(KeyCode::F(5), KeyModifiers::SHIFT)),
            Some(b"\x1b[15;2~".to_vec()),
        );
    }

    #[test]
    fn ctrl_shift_f12() {
        assert_eq!(
            key_to_bytes(&k(
                KeyCode::F(12),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            )),
            Some(b"\x1b[24;6~".to_vec()),
        );
    }

    #[test]
    fn modified_pageup_delete() {
        assert_eq!(
            key_to_bytes(&k(KeyCode::PageUp, KeyModifiers::CONTROL)),
            Some(b"\x1b[5;5~".to_vec()),
        );
        assert_eq!(
            key_to_bytes(&k(KeyCode::Delete, KeyModifiers::ALT)),
            Some(b"\x1b[3;3~".to_vec()),
        );
    }
}
