use std::path::PathBuf;

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub name: String,
}

fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".meld").join("config.toml")
}

impl Config {
    pub fn load() -> Option<Self> {
        let content = std::fs::read_to_string(config_path()).ok()?;
        let config: Config = toml::from_str(&content).ok()?;
        if config.name.is_empty() {
            None
        } else {
            Some(config)
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, toml::to_string_pretty(self)?)?;
        Ok(())
    }
}

pub fn ensure_name(terminal: &mut ratatui::DefaultTerminal) -> Result<String> {
    let default = Config::load()
        .map(|c| c.name)
        .unwrap_or_else(|| std::env::var("USER").unwrap_or_default());
    let mut input = default;
    let mut cursor = input.len();

    loop {
        let prompt = format!("enter your name: {}", &input);
        terminal.draw(|frame| {
            let area = frame.area();
            let status_area =
                ratatui::prelude::Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1);
            frame.render_widget(
                ratatui::widgets::Paragraph::new(crate::status_line(&prompt)),
                status_area,
            );
            let cursor_x = status_area.x
                + "(meld) ".len() as u16
                + "enter your name: ".len() as u16
                + cursor as u16;
            frame.set_cursor_position((cursor_x, status_area.y));
        })?;

        match event::read()? {
            Event::Key(key) => match key.code {
                KeyCode::Enter if !input.is_empty() => {
                    let config = Config {
                        name: input.clone(),
                    };
                    config.save()?;
                    return Ok(input);
                }
                KeyCode::Char(c) => {
                    input.insert(cursor, c);
                    cursor += 1;
                }
                KeyCode::Backspace if cursor > 0 => {
                    cursor -= 1;
                    input.remove(cursor);
                }
                KeyCode::Left if cursor > 0 => cursor -= 1,
                KeyCode::Right if cursor < input.len() => cursor += 1,
                _ => {}
            },
            _ => {}
        }
    }
}
