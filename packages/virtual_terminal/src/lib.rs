//! Virtual Terminal
//!
//! Sits between clients and a PTY. Maintains a screen buffer via `vt100`,
//! generates keyframe snapshots, stores deltas (raw PTY output since last
//! keyframe), and negotiates dimensions across multiple clients.

pub use vt100;
pub mod recorder;
pub use recorder::{VtEvent, VtRecorder, VtRecording, VtRecordingHeader};

use std::collections::HashMap;

/// Attributes of a single visible cell, extracted from vt100.
///
/// Provides a single source of truth for cell iteration: all three rendering
/// paths (plain text, SGR bytes, ratatui buffer) consume this struct instead
/// of independently querying vt100 cell attributes.
#[derive(Debug, Clone)]
pub struct CellInfo<'a> {
    /// Column index in the row (0-indexed).
    pub col: u16,
    /// Text content of the cell (empty cells are represented as `" "`).
    pub contents: &'a str,
    pub fg: vt100::Color,
    pub bg: vt100::Color,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

/// Iterate visible cells in a row, skipping wide-continuation cells.
///
/// This is the canonical way to read cells from a `vt100::Screen`.  All
/// rendering paths (plain text extraction, SGR byte emission, ratatui widget)
/// should use this iterator to stay in sync when vt100 adds new attributes
/// or changes cell semantics.
pub fn walk_row(screen: &vt100::Screen, row: u16, cols: u16) -> impl Iterator<Item = CellInfo<'_>> {
    (0..cols).filter_map(move |col| {
        let cell = screen.cell(row, col)?;
        if cell.is_wide_continuation() {
            return None;
        }
        let contents = cell.contents();
        Some(CellInfo {
            col,
            contents: if contents.is_empty() { " " } else { contents },
            fg: cell.fgcolor(),
            bg: cell.bgcolor(),
            bold: cell.bold(),
            italic: cell.italic(),
            underline: cell.underline(),
            inverse: cell.inverse(),
        })
    })
}

/// Type of client connection.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientType {
    Web,
    Terminal,
}

/// Per-client viewport tracking.
#[derive(Debug, Clone)]
pub struct ClientViewport {
    pub rows: u16,
    pub cols: u16,
    pub client_type: ClientType,
    pub is_active: bool,
}

/// A virtual terminal that sits between clients and a PTY.
/// Maintains a screen buffer, negotiates dimensions, and provides
/// keyframe + delta replay for efficient client attach.
pub struct VirtualTerminal {
    /// VT100 terminal emulator — processes PTY output, maintains screen state
    parser: vt100::Parser,

    /// Per-client viewport tracking
    client_viewports: HashMap<String, ClientViewport>,

    /// Current effective dimensions (min of all active viewports)
    effective_dims: (u16, u16),

    /// Last keyframe: screen state rendered as ANSI escape sequences
    keyframe: Option<Vec<u8>>,

    /// Raw PTY output accumulated since last keyframe
    deltas: Vec<u8>,

    /// Max delta buffer size before auto-compaction
    max_delta_bytes: usize,

    /// Scrollback capacity (lines) — stored for parser recreation on resize
    scrollback_capacity: usize,
}

/// Diagnostic snapshot of VT state for debugging.
#[derive(Debug)]
pub struct VtDebugState {
    pub effective_dims: (u16, u16),
    pub screen_size: (u16, u16),
    pub alternate_screen: bool,
    pub scrollback_depth: usize,
    pub cursor_position: (u16, u16),
    pub active_viewports: usize,
    pub total_viewports: usize,
    pub visible_rows: Vec<String>,
}

impl VirtualTerminal {
    /// Create a new virtual terminal with the given initial dimensions.
    pub fn new(rows: u16, cols: u16, max_delta_bytes: usize, scrollback_lines: usize) -> Self {
        Self {
            parser: vt100::Parser::new(rows, cols, scrollback_lines),
            client_viewports: HashMap::new(),
            effective_dims: (rows, cols),
            keyframe: None,
            deltas: Vec::new(),
            max_delta_bytes,
            scrollback_capacity: scrollback_lines,
        }
    }

    /// Access the underlying vt100 screen for cell-level reads.
    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    /// Mutable access to the vt100 screen.
    pub fn screen_mut(&mut self) -> &mut vt100::Screen {
        self.parser.screen_mut()
    }

    /// Current cursor position (row, col) — 0-indexed.
    pub fn cursor_position(&self) -> (u16, u16) {
        self.parser.screen().cursor_position()
    }

    /// Process PTY output — feed to vt100 parser, append to deltas,
    /// auto-compact if deltas exceed threshold.
    pub fn process_output(&mut self, data: &[u8]) {
        self.parser.process(data);
        self.deltas.extend_from_slice(data);

        // Auto-compact when deltas get large
        if self.deltas.len() > self.max_delta_bytes {
            self.compact();
        }
    }

    /// Generate a keyframe: snapshot the screen as ANSI escape sequences.
    /// Clears the delta buffer.
    pub fn compact(&mut self) {
        let screen = self.parser.screen();
        let mut kf = Vec::new();
        // Reset terminal state, move home, clear screen, reset attributes
        kf.extend_from_slice(b"\x1b[H\x1b[2J\x1b[0m");
        kf.extend_from_slice(&screen.contents_formatted());
        // Restore cursor position
        let (row, col) = screen.cursor_position();
        kf.extend_from_slice(format!("\x1b[{};{}H", row + 1, col + 1).as_bytes());
        self.keyframe = Some(kf);
        self.deltas.clear();
    }

    /// Get the aggregated terminal state for a new client.
    ///
    /// Returns scrollback content (from the vt100 parser's buffer) followed
    /// by a keyframe (visible screen snapshot). Reads directly from the
    /// parser's screen state — no compaction or cache mutation.
    ///
    /// The only mutation is the temporary `set_scrollback()` viewport shift
    /// used to walk scrollback pages; this is restored to 0 before return.
    pub fn replay(&mut self, client_rows: u16) -> Vec<u8> {
        let screen = self.parser.screen();
        let (eff_rows, eff_cols) = screen.size();
        let alt_screen = screen.alternate_screen();

        let sb_depth = scrollback_depth(&mut self.parser);

        let mut result = Vec::new();

        let scrollback_lines = render_scrollback(&mut self.parser, &mut result);
        let scrollback_bytes = result.len();

        if scrollback_lines > 0 {
            append_scrollback_flush(&mut result, scrollback_lines, client_rows);
        }

        render_visible_screen(self.parser.screen(), &mut result);

        tracing::debug!(
            client_rows,
            eff_rows,
            eff_cols,
            alt_screen,
            sb_depth,
            scrollback_lines,
            scrollback_bytes,
            total_bytes = result.len(),
            "VT replay generated"
        );

        result
    }

    /// Update a client's viewport. Returns new effective dims if changed.
    pub fn update_viewport(
        &mut self,
        connection_id: &str,
        rows: u16,
        cols: u16,
        client_type: ClientType,
    ) -> Option<(u16, u16)> {
        self.client_viewports.insert(
            connection_id.to_string(),
            ClientViewport {
                rows,
                cols,
                client_type,
                is_active: true,
            },
        );
        self.recalculate_effective_dims()
    }

    /// Set a client's terminal visibility. Returns new effective dims if changed.
    pub fn set_active(&mut self, connection_id: &str, active: bool) -> Option<(u16, u16)> {
        if let Some(viewport) = self.client_viewports.get_mut(connection_id) {
            viewport.is_active = active;
        }
        self.recalculate_effective_dims()
    }

    /// Remove a client (disconnect). Returns new effective dims if changed.
    pub fn remove_client(&mut self, connection_id: &str) -> Option<(u16, u16)> {
        self.client_viewports.remove(connection_id);
        self.recalculate_effective_dims()
    }

    /// Apply a resize to the vt100 parser (called when effective dims change).
    /// Resize the terminal: saves visible screen, creates a fresh parser at the
    /// new dimensions (clearing scrollback), and restores the visible content.
    /// SIGWINCH will cause the PTY program to redraw, naturally rebuilding
    /// scrollback at the new width.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let content = self.parser.screen().contents_formatted();
        let cursor = self.parser.screen().cursor_position();
        self.parser = vt100::Parser::new(rows, cols, self.scrollback_capacity);
        self.parser.process(&content);
        self.parser
            .process(format!("\x1b[{};{}H", cursor.0 + 1, cursor.1 + 1).as_bytes());
    }

    /// Current effective dimensions.
    pub fn effective_dims(&self) -> (u16, u16) {
        self.effective_dims
    }

    /// Whether the terminal is in alternate screen mode.
    pub fn alternate_screen(&self) -> bool {
        self.parser.screen().alternate_screen()
    }

    /// Return all content as plain-text lines: scrollback (oldest first)
    /// then visible screen rows. Reads cells directly from the vt100 parser
    /// — same data the TUI renders.
    pub fn lines(&mut self) -> Vec<String> {
        let (_, cols) = self.parser.screen().size();

        // Scrollback (oldest first)
        let depth = scrollback_depth(&mut self.parser);
        let mut out = Vec::with_capacity(depth);
        for offset in (1..=depth).rev() {
            self.parser.screen_mut().set_scrollback(offset);
            out.push(read_row_text(self.parser.screen(), 0, cols));
        }
        self.parser.screen_mut().set_scrollback(0);

        // Visible screen
        let (rows, cols) = self.parser.screen().size();
        for r in 0..rows {
            out.push(read_row_text(self.parser.screen(), r, cols));
        }
        out
    }

    /// Diagnostic dump of VT state for debugging replay/corruption issues.
    pub fn debug_state(&mut self) -> VtDebugState {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let alt = screen.alternate_screen();
        let cursor = screen.cursor_position();

        let sb_depth = scrollback_depth(&mut self.parser);

        let visible_rows = (0..rows)
            .map(|r| read_row_text(self.parser.screen(), r, cols))
            .collect();

        VtDebugState {
            effective_dims: self.effective_dims,
            screen_size: (rows, cols),
            alternate_screen: alt,
            scrollback_depth: sb_depth,
            cursor_position: cursor,
            active_viewports: self
                .client_viewports
                .iter()
                .filter(|(_, v)| v.is_active)
                .count(),
            total_viewports: self.client_viewports.len(),
            visible_rows,
        }
    }

    /// Recalculate effective dims from active viewports.
    /// Returns `Some((rows, cols))` if dims changed, `None` otherwise.
    fn recalculate_effective_dims(&mut self) -> Option<(u16, u16)> {
        let new_dims = calculate_effective_dims(&self.client_viewports);
        if let Some((rows, cols)) = new_dims
            && (rows, cols) != self.effective_dims
        {
            self.effective_dims = (rows, cols);
            self.resize(rows, cols);
            self.keyframe = None;
            self.deltas.clear();
            return Some((rows, cols));
        }
        // If no active viewports, keep current dims (don't shrink to nothing)
        None
    }
}

// =============================================================================
// Scrollback lens — safe temporary viewport shifts
//
// vt100's `set_scrollback()` is a mutable viewport lens: it shifts row 0
// into the scrollback buffer.  Every caller must restore it to 0 before
// returning.  These helpers enforce that invariant structurally.
// =============================================================================

/// Measure the total scrollback depth (lines above the visible screen).
///
/// Temporarily sets scrollback to `usize::MAX` (vt100 clamps to actual depth),
/// reads the clamped value, and restores scrollback to 0.
fn scrollback_depth(parser: &mut vt100::Parser) -> usize {
    parser.screen_mut().set_scrollback(usize::MAX);
    let depth = parser.screen().scrollback();
    parser.screen_mut().set_scrollback(0);
    depth
}

/// Read a single row as plain text, trimming trailing whitespace.
///
/// Shared by `VirtualTerminal::lines()`, `debug_state()`, and test helpers.
pub fn read_row_text(screen: &vt100::Screen, row: u16, cols: u16) -> String {
    let s: String = walk_row(screen, row, cols)
        .map(|cell| cell.contents)
        .collect();
    s.trim_end().to_string()
}

// =============================================================================
// Functional Shell — pure(ish) rendering functions
//
// These produce terminal bytes from screen state. The only mutation is
// `set_scrollback()` which is a temporary viewport lens, restored before
// return. No keyframe cache, no delta buffer, no compaction.
// =============================================================================

/// Walk scrollback pages oldest-to-newest, emitting SGR-styled rows.
///
/// Each row is output as styled text + `\r\n`, so it scrolls naturally
/// through the receiving terminal. **No CUP sequences** — `rows_formatted()`
/// emits absolute cursor-position sequences for rows with leading default
/// cells, which stomp earlier pages when scrollback is emitted page-by-page.
///
/// Temporarily shifts the parser's scrollback viewport (restored to 0 on return).
/// Render scrollback content as plain rows (no CUP). Returns the number of
/// scrollback lines emitted (0 means no scrollback).
fn render_scrollback(parser: &mut vt100::Parser, out: &mut Vec<u8>) -> usize {
    let total_scrollback = scrollback_depth(parser);

    if total_scrollback == 0 {
        return 0;
    }

    let (rows, cols) = parser.screen().size();
    let page_size = rows as usize;

    let mut remaining = total_scrollback;
    while remaining > 0 {
        parser.screen_mut().set_scrollback(remaining);
        let page_rows = remaining.min(page_size);

        for i in 0..page_rows {
            format_row_no_cup(parser.screen(), i as u16, cols, out);
            out.extend_from_slice(b"\x1b[0m\r\n");
        }

        remaining = remaining.saturating_sub(page_size);
    }

    parser.screen_mut().set_scrollback(0);
    total_scrollback
}

/// Push scrollback content off the client's visible screen into its
/// scrollback buffer. After emitting `scrollback_lines` rows of text (each
/// ending with `\r\n`), some of those lines remain on the client's visible
/// screen. We need exactly that many newlines from the bottom to scroll
/// them into the client's scrollback before the keyframe's `\x1b[2J` clears
/// the display.
///
/// After writing N lines into a `client_rows`-tall terminal:
///   - If N < client_rows: all N lines are on-screen (cursor at row N+1).
///   - If N >= client_rows: `client_rows - 1` lines remain on-screen
///     (the rest already scrolled from the `\r\n` at the end of each line).
///
/// So the flush count is `min(N, client_rows - 1)`.
fn append_scrollback_flush(out: &mut Vec<u8>, scrollback_lines: usize, client_rows: u16) {
    let flush_count = scrollback_lines.min(client_rows.saturating_sub(1) as usize);
    out.extend_from_slice(format!("\x1b[{};1H", client_rows).as_bytes());
    for _ in 0..flush_count {
        out.push(b'\n');
    }
}

/// Render the visible screen as a keyframe: clear + contents + cursor restore.
fn render_visible_screen(screen: &vt100::Screen, out: &mut Vec<u8>) {
    out.extend_from_slice(b"\x1b[H\x1b[2J\x1b[0m");
    out.extend_from_slice(&screen.contents_formatted());
    let (row, col) = screen.cursor_position();
    out.extend_from_slice(format!("\x1b[{};{}H", row + 1, col + 1).as_bytes());
}

/// Emit a single row as SGR-styled text — no CUP or cursor movement.
/// Iterates cells left-to-right, emitting SGR diffs for attribute changes
/// and cell contents (space for empty/default cells). Wide-continuation
/// cells are skipped.
fn format_row_no_cup(screen: &vt100::Screen, row: u16, cols: u16, out: &mut Vec<u8>) {
    let mut cur_fg = vt100::Color::Default;
    let mut cur_bg = vt100::Color::Default;
    let mut cur_bold = false;
    let mut cur_italic = false;
    let mut cur_underline = false;
    let mut cur_inverse = false;

    for cell in walk_row(screen, row, cols) {
        let attrs_differ = cell.fg != cur_fg
            || cell.bg != cur_bg
            || cell.bold != cur_bold
            || cell.italic != cur_italic
            || cell.underline != cur_underline
            || cell.inverse != cur_inverse;

        if attrs_differ {
            // Check if any attribute was *removed* — needs a full reset
            let needs_reset = (cur_bold && !cell.bold)
                || (cur_italic && !cell.italic)
                || (cur_underline && !cell.underline)
                || (cur_inverse && !cell.inverse)
                || (cur_fg != vt100::Color::Default && cell.fg == vt100::Color::Default)
                || (cur_bg != vt100::Color::Default && cell.bg == vt100::Color::Default);

            if needs_reset {
                out.extend_from_slice(b"\x1b[0m");
                if cell.bold {
                    out.extend_from_slice(b"\x1b[1m");
                }
                if cell.italic {
                    out.extend_from_slice(b"\x1b[3m");
                }
                if cell.underline {
                    out.extend_from_slice(b"\x1b[4m");
                }
                if cell.inverse {
                    out.extend_from_slice(b"\x1b[7m");
                }
                emit_sgr_fg(out, cell.fg);
                emit_sgr_bg(out, cell.bg);
            } else {
                if !cur_bold && cell.bold {
                    out.extend_from_slice(b"\x1b[1m");
                }
                if !cur_italic && cell.italic {
                    out.extend_from_slice(b"\x1b[3m");
                }
                if !cur_underline && cell.underline {
                    out.extend_from_slice(b"\x1b[4m");
                }
                if !cur_inverse && cell.inverse {
                    out.extend_from_slice(b"\x1b[7m");
                }
                if cur_fg != cell.fg {
                    emit_sgr_fg(out, cell.fg);
                }
                if cur_bg != cell.bg {
                    emit_sgr_bg(out, cell.bg);
                }
            }

            cur_fg = cell.fg;
            cur_bg = cell.bg;
            cur_bold = cell.bold;
            cur_italic = cell.italic;
            cur_underline = cell.underline;
            cur_inverse = cell.inverse;
        }

        out.extend_from_slice(cell.contents.as_bytes());
    }
}

fn emit_sgr_fg(out: &mut Vec<u8>, color: vt100::Color) {
    match color {
        vt100::Color::Default => out.extend_from_slice(b"\x1b[39m"),
        vt100::Color::Idx(n) => {
            out.extend_from_slice(format!("\x1b[38;5;{}m", n).as_bytes());
        }
        vt100::Color::Rgb(r, g, b) => {
            out.extend_from_slice(format!("\x1b[38;2;{};{};{}m", r, g, b).as_bytes());
        }
    }
}

fn emit_sgr_bg(out: &mut Vec<u8>, color: vt100::Color) {
    match color {
        vt100::Color::Default => out.extend_from_slice(b"\x1b[49m"),
        vt100::Color::Idx(n) => {
            out.extend_from_slice(format!("\x1b[48;5;{}m", n).as_bytes());
        }
        vt100::Color::Rgb(r, g, b) => {
            out.extend_from_slice(format!("\x1b[48;2;{};{};{}m", r, g, b).as_bytes());
        }
    }
}

/// Calculate effective dimensions from active viewports.
/// Returns min(rows) x min(cols) across all active clients.
fn calculate_effective_dims(viewports: &HashMap<String, ClientViewport>) -> Option<(u16, u16)> {
    let active: Vec<_> = viewports.values().filter(|v| v.is_active).collect();
    if active.is_empty() {
        return None;
    }
    let rows = active.iter().map(|v| v.rows).min().unwrap();
    let cols = active.iter().map(|v| v.cols).min().unwrap();
    Some((rows, cols))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_initial_dims() {
        let vt = VirtualTerminal::new(24, 80, 4096, 0);
        assert_eq!(vt.effective_dims(), (24, 80));
    }

    #[test]
    fn test_screen_access() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 0);
        vt.process_output(b"Hello");
        let screen = vt.screen();
        let (rows, cols) = screen.size();
        assert_eq!(rows, 24);
        assert_eq!(cols, 80);
        // Screen should contain "Hello"
        assert!(screen.contents().contains("Hello"));
    }

    #[test]
    fn test_single_client_viewport() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 0);
        let result = vt.update_viewport("client-1", 40, 120, ClientType::Web);
        // Effective dims changed from (24, 80) to (40, 120)
        assert_eq!(result, Some((40, 120)));
        assert_eq!(vt.effective_dims(), (40, 120));
    }

    #[test]
    fn test_two_clients_min_dims() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 0);
        vt.update_viewport("web", 40, 120, ClientType::Web);
        let result = vt.update_viewport("cli", 24, 80, ClientType::Terminal);
        // min(40,24)=24, min(120,80)=80
        assert_eq!(result, Some((24, 80)));
        assert_eq!(vt.effective_dims(), (24, 80));
    }

    #[test]
    fn test_inactive_client_excluded() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 0);
        vt.update_viewport("web", 40, 120, ClientType::Web);
        vt.update_viewport("cli", 24, 80, ClientType::Terminal);
        assert_eq!(vt.effective_dims(), (24, 80));

        // Deactivate the smaller client
        let result = vt.set_active("cli", false);
        assert_eq!(result, Some((40, 120)));
        assert_eq!(vt.effective_dims(), (40, 120));
    }

    #[test]
    fn test_reactivate_client() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 0);
        vt.update_viewport("web", 40, 120, ClientType::Web);
        vt.update_viewport("cli", 24, 80, ClientType::Terminal);
        vt.set_active("cli", false);
        assert_eq!(vt.effective_dims(), (40, 120));

        // Reactivate
        let result = vt.set_active("cli", true);
        assert_eq!(result, Some((24, 80)));
    }

    #[test]
    fn test_client_removal_recalculates() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 0);
        vt.update_viewport("web", 40, 120, ClientType::Web);
        vt.update_viewport("cli", 24, 80, ClientType::Terminal);
        assert_eq!(vt.effective_dims(), (24, 80));

        let result = vt.remove_client("cli");
        assert_eq!(result, Some((40, 120)));
    }

    #[test]
    fn test_remove_last_client_keeps_dims() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 0);
        vt.update_viewport("cli", 30, 100, ClientType::Terminal);
        assert_eq!(vt.effective_dims(), (30, 100));

        // Remove the only client — dims stay at (30, 100)
        let result = vt.remove_client("cli");
        assert!(result.is_none());
        assert_eq!(vt.effective_dims(), (30, 100));
    }

    #[test]
    fn test_process_output_and_compact_replay() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 0);
        vt.process_output(b"Hello, world!");
        vt.process_output(b"\r\nLine 2");

        let replay = vt.replay(24);
        // Replay should contain the keyframe + any deltas
        assert!(!replay.is_empty());

        // The replay should contain "Hello, world!" and "Line 2" when
        // rendered through a terminal
        let replay_str = String::from_utf8_lossy(&replay);
        assert!(replay_str.contains("Hello, world!"));
        assert!(replay_str.contains("Line 2"));
    }

    #[test]
    fn test_auto_compaction() {
        let mut vt = VirtualTerminal::new(24, 80, 100, 0); // Small threshold
        // Write more than 100 bytes
        let data = vec![b'A'; 150];
        vt.process_output(&data);

        // After auto-compaction, deltas should be empty
        assert!(vt.deltas.is_empty());
        // And keyframe should exist
        assert!(vt.keyframe.is_some());
    }

    #[test]
    fn test_resize_invalidates_keyframe() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 0);
        vt.process_output(b"Some output");
        vt.compact();
        assert!(vt.keyframe.is_some());

        // Resize via a new client viewport
        vt.update_viewport("cli", 30, 100, ClientType::Terminal);
        // Keyframe should be invalidated
        assert!(vt.keyframe.is_none());
    }

    #[test]
    fn test_no_dims_change_returns_none() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 0);
        vt.update_viewport("cli", 30, 100, ClientType::Terminal);
        assert_eq!(vt.effective_dims(), (30, 100));

        // Same dims again — no change
        let result = vt.update_viewport("cli", 30, 100, ClientType::Terminal);
        assert!(result.is_none());
    }

    #[test]
    fn test_replay_does_not_compact() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 0);
        vt.process_output(b"Hello");
        assert!(vt.keyframe.is_none());
        assert!(!vt.deltas.is_empty());

        let replay = vt.replay(24);
        // replay() is read-only — no compaction side effect
        assert!(vt.keyframe.is_none());
        assert!(!vt.deltas.is_empty());
        assert!(!replay.is_empty());
    }

    #[test]
    fn test_calculate_effective_dims_empty() {
        let viewports = HashMap::new();
        assert_eq!(calculate_effective_dims(&viewports), None);
    }

    #[test]
    fn test_calculate_effective_dims_all_inactive() {
        let mut viewports = HashMap::new();
        viewports.insert(
            "a".to_string(),
            ClientViewport {
                rows: 24,
                cols: 80,
                client_type: ClientType::Web,
                is_active: false,
            },
        );
        assert_eq!(calculate_effective_dims(&viewports), None);
    }

    #[test]
    fn test_update_viewport_implies_active() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 0);
        // update_viewport always sets is_active=true
        vt.update_viewport("cli", 30, 100, ClientType::Terminal);
        let vp = vt.client_viewports.get("cli").unwrap();
        assert!(vp.is_active);
    }

    #[test]
    fn test_keyframe_plus_deltas_replay() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 0);
        // Phase 1: write output, then compact to create a keyframe
        vt.process_output(b"Before keyframe");
        vt.compact();
        assert!(vt.keyframe.is_some());
        assert!(vt.deltas.is_empty());

        // Phase 2: write more output — goes into deltas
        vt.process_output(b"\r\nAfter keyframe");
        assert!(!vt.deltas.is_empty());

        // Replay should contain content from both phases
        let replay = vt.replay(24);
        let replay_str = String::from_utf8_lossy(&replay);
        assert!(replay_str.contains("Before keyframe"));
        assert!(replay_str.contains("After keyframe"));
    }

    #[test]
    fn test_auto_compact_then_more_output() {
        let mut vt = VirtualTerminal::new(24, 80, 64, 0); // Small threshold
        // Write enough to trigger auto-compaction
        vt.process_output(
            b"AAAA repeating data that exceeds the sixty-four byte delta threshold!!",
        );
        assert!(vt.keyframe.is_some());
        assert!(vt.deltas.is_empty());

        // Write more output after auto-compaction
        vt.process_output(b"\r\nPost-compact line");
        assert!(!vt.deltas.is_empty());

        // Replay should contain everything
        let replay = vt.replay(24);
        let replay_str = String::from_utf8_lossy(&replay);
        assert!(replay_str.contains("AAAA"));
        assert!(replay_str.contains("Post-compact line"));
    }

    #[test]
    fn test_empty_terminal_replay() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 0);
        // No output at all — replay on a fresh terminal
        let replay = vt.replay(24);
        // Should not be empty — the keyframe is the empty screen reset sequence
        assert!(!replay.is_empty());
        // Should contain the reset/clear sequence
        let replay_str = String::from_utf8_lossy(&replay);
        assert!(replay_str.contains("\x1b[H\x1b[2J\x1b[0m"));
    }

    #[test]
    fn test_replay_idempotency() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 0);
        vt.process_output(b"Hello, world!\r\nLine two");

        let replay1 = vt.replay(24);
        let replay2 = vt.replay(24);
        assert_eq!(replay1, replay2);
    }

    #[test]
    fn test_cursor_position() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 0);
        assert_eq!(vt.cursor_position(), (0, 0));

        vt.process_output(b"Hello");
        assert_eq!(vt.cursor_position(), (0, 5));

        vt.process_output(b"\r\nLine 2");
        assert_eq!(vt.cursor_position(), (1, 6));
    }

    #[test]
    fn test_set_active_unknown_connection_id() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 0);
        // Should return None and not panic
        let result = vt.set_active("nonexistent", false);
        assert!(result.is_none());
    }

    #[test]
    fn test_remove_unknown_connection_id() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 0);
        // Should return None and not panic
        let result = vt.remove_client("nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn test_resize_direct() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 0);
        vt.process_output(b"hello");
        vt.compact();
        assert!(vt.keyframe.is_some());

        // Direct resize (used by instance_actor after effective dims change)
        vt.resize(40, 120);

        // Replay should still work — content rendered at new size
        let replay = vt.replay(24);
        let replay_str = String::from_utf8_lossy(&replay);
        assert!(replay_str.contains("hello"));
    }

    #[test]
    fn test_resize_then_replay() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 0);
        vt.process_output(b"Content at 24x80");
        vt.compact();
        assert!(vt.keyframe.is_some());

        // Resize via update_viewport — uses set_size, preserving content
        let dims_changed = vt.update_viewport("cli", 40, 120, ClientType::Web);
        assert_eq!(dims_changed, Some((40, 120)));
        assert!(vt.keyframe.is_none());
        assert!(vt.deltas.is_empty());

        // Replay preserves old content (set_size keeps screen state)
        let replay = vt.replay(24);
        assert!(!replay.is_empty());
        // replay() is read-only — keyframe remains None (cleared by resize)
        assert!(vt.keyframe.is_none());
        assert_eq!(vt.effective_dims(), (40, 120));
        let replay_str = String::from_utf8_lossy(&replay);
        assert!(replay_str.contains("Content at 24x80"));

        // New output at the correct width IS captured
        vt.process_output(b"Content at 40x120");
        let replay = vt.replay(24);
        let replay_str = String::from_utf8_lossy(&replay);
        assert!(replay_str.contains("Content at 40x120"));
    }

    // ── Scrollback tests ─────────────────────────────────────────────

    #[test]
    fn test_scrollback_in_replay() {
        // 4-row screen with 100 lines of scrollback
        let mut vt = VirtualTerminal::new(4, 40, 4096, 100);

        // Write 8 lines — 4 will scroll off into the scrollback buffer
        for i in 0..8 {
            vt.process_output(format!("Line {}\r\n", i).as_bytes());
        }

        let replay = vt.replay(24);
        let replay_str = String::from_utf8_lossy(&replay);

        // Scrollback lines (0-3) should be in the replay
        assert!(replay_str.contains("Line 0"), "scrollback line 0 missing");
        assert!(replay_str.contains("Line 3"), "scrollback line 3 missing");
        // Visible screen lines (4-7) should also be present
        assert!(replay_str.contains("Line 4"), "visible line 4 missing");
        assert!(replay_str.contains("Line 7"), "visible line 7 missing");
    }

    #[test]
    fn test_scrollback_empty_when_no_overflow() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 100);
        vt.process_output(b"Only one line");

        let replay = vt.replay(24);
        let replay_str = String::from_utf8_lossy(&replay);
        assert!(replay_str.contains("Only one line"));
    }

    #[test]
    fn test_scrollback_zero_means_no_scrollback() {
        // scrollback_lines=0 — same as before
        let mut vt = VirtualTerminal::new(4, 40, 4096, 0);
        for i in 0..8 {
            vt.process_output(format!("Line {}\r\n", i).as_bytes());
        }

        let replay = vt.replay(24);
        let replay_str = String::from_utf8_lossy(&replay);
        // With zero scrollback, lines 0-4 are lost
        assert!(
            !replay_str.contains("Line 0"),
            "line 0 should be lost with zero scrollback"
        );
        assert!(
            !replay_str.contains("Line 4"),
            "line 4 should be lost with zero scrollback"
        );
        // Visible lines should be present (lines 5-7 remain on the 4-row screen
        // because each line ends with \r\n, scrolling the cursor down)
        assert!(replay_str.contains("Line 5"), "line 5 should be visible");
        assert!(replay_str.contains("Line 7"), "line 7 should be visible");
    }

    #[test]
    fn test_scrollback_preserves_formatting() {
        let mut vt = VirtualTerminal::new(4, 40, 4096, 100);

        // Write colored output that scrolls into scrollback
        for i in 0..8 {
            // \x1b[31m = red foreground
            vt.process_output(format!("\x1b[31mRed {}\x1b[0m\r\n", i).as_bytes());
        }

        let replay = vt.replay(24);
        let replay_str = String::from_utf8_lossy(&replay);

        // Scrollback should contain the color escape for old lines
        assert!(
            replay_str.contains("Red 0"),
            "scrollback should contain text"
        );
        // The replay should contain SGR sequences (red = 31)
        assert!(
            replay_str.contains("\x1b["),
            "scrollback should contain ANSI formatting"
        );
    }

    #[test]
    fn test_scrollback_survives_compaction() {
        let mut vt = VirtualTerminal::new(4, 40, 4096, 100);

        for i in 0..8 {
            vt.process_output(format!("Line {}\r\n", i).as_bytes());
        }
        vt.compact();

        // After compaction, replay should still include scrollback
        let replay = vt.replay(24);
        let replay_str = String::from_utf8_lossy(&replay);
        assert!(
            replay_str.contains("Line 0"),
            "scrollback line should survive compaction"
        );
        assert!(
            replay_str.contains("Line 7"),
            "visible line should survive compaction"
        );
    }

    #[test]
    fn test_scrollback_capped_at_limit() {
        // Only 5 lines of scrollback
        let mut vt = VirtualTerminal::new(4, 40, 4096, 5);

        // Write 20 lines — 16 scroll off, but only 5 are retained
        for i in 0..20 {
            vt.process_output(format!("Line {:02}\r\n", i).as_bytes());
        }

        let replay = vt.replay(24);
        let replay_str = String::from_utf8_lossy(&replay);

        // Lines 0-10 should be gone (exceeded scrollback capacity)
        assert!(
            !replay_str.contains("Line 00"),
            "oldest lines should be evicted"
        );
        // Recent scrollback lines should be present
        assert!(
            replay_str.contains("Line 15"),
            "recent scrollback should be present"
        );
        // Visible screen lines should be present
        assert!(
            replay_str.contains("Line 19"),
            "visible lines should be present"
        );
    }

    #[test]
    fn test_resize_preserves_scrollback_buffer() {
        let mut vt = VirtualTerminal::new(4, 40, 4096, 500);
        // Fill some scrollback
        for i in 0..10 {
            vt.process_output(format!("Line {}\r\n", i).as_bytes());
        }
        // Resize via viewport change
        vt.update_viewport("cli", 30, 100, ClientType::Terminal);

        // Simulate SIGWINCH redraw: real programs clear + home before redrawing
        vt.process_output(b"\x1b[2J\x1b[H");
        for i in 0..10 {
            vt.process_output(format!("Line {}\r\n", i).as_bytes());
        }

        // Scrollback should contain the redrawn content
        let replay = vt.replay(24);
        let replay_str = String::from_utf8_lossy(&replay);
        assert!(
            replay_str.contains("Line 0"),
            "scrollback should survive resize after SIGWINCH redraw"
        );
    }

    /// Viewport-driven dimension changes must preserve scrollback history.
    #[test]
    fn test_viewport_change_preserves_scrollback() {
        let mut vt = VirtualTerminal::new(4, 40, 4096, 100);

        // Build up scrollback
        for i in 0..20 {
            vt.process_output(format!("Line {:02}\r\n", i).as_bytes());
        }

        // Verify scrollback exists before dimension change
        let replay_before = vt.replay(24);
        let before_str = String::from_utf8_lossy(&replay_before);
        assert!(
            before_str.contains("Line 00"),
            "scrollback should exist before resize"
        );

        // Simulate a web client connecting with different dimensions —
        // this triggers recalculate_effective_dims
        vt.update_viewport("web-client", 30, 100, ClientType::Web);
        assert_eq!(vt.effective_dims(), (30, 100));

        // Simulate SIGWINCH redraw: clear + home + rewrite (like real programs)
        vt.process_output(b"\x1b[2J\x1b[H");
        for i in 0..20 {
            vt.process_output(format!("Line {:02}\r\n", i).as_bytes());
        }

        // Scrollback must contain the redrawn content
        let replay_after = vt.replay(24);
        let after_str = String::from_utf8_lossy(&replay_after);
        assert!(
            after_str.contains("Line 00"),
            "scrollback must survive viewport dimension change"
        );
        assert!(
            after_str.contains("Line 10"),
            "scrollback must survive viewport dimension change"
        );
    }

    /// Simulate the full client-side round-trip: server generates replay bytes,
    /// client processes them through a fresh vt100 parser, then check whether
    /// scrollback is accessible via set_scrollback/scrollback.
    #[test]
    fn test_replay_roundtrip_preserves_client_scrollback() {
        // Server side: 4-row screen, 100 lines of scrollback, lots of output
        let mut server_vt = VirtualTerminal::new(4, 40, 4096, 100);
        for i in 0..20 {
            server_vt.process_output(format!("Line {:02}\r\n", i).as_bytes());
        }

        // Server generates replay — client_rows=4 matches the client parser below
        let replay = server_vt.replay(4);
        assert!(!replay.is_empty());

        // Simulate JSON round-trip: server encodes as String, client decodes
        let replay_string = String::from_utf8_lossy(&replay).to_string();
        let replay_bytes = replay_string.as_bytes();

        // Client side: fresh parser with scrollback, same screen size
        let mut client_parser = vt100::Parser::new(4, 40, 100);
        client_parser.process(replay_bytes);

        // Client should have scrollback available
        client_parser.screen_mut().set_scrollback(usize::MAX);
        let client_scrollback = client_parser.screen().scrollback();
        assert!(
            client_scrollback > 0,
            "client should have scrollback after processing replay, got 0"
        );

        // Check oldest scrollback line is accessible
        let cell = client_parser.screen().cell(0, 0).unwrap();
        let ch = cell.contents();
        assert!(!ch.is_empty(), "oldest scrollback row should have content");

        // Reset to live screen
        client_parser.screen_mut().set_scrollback(0);
    }

    /// Regression: when the server has fewer scrollback lines than the client's
    /// screen height, the old replay format lost ALL scrollback (the lines
    /// stayed on the visible screen and were erased by \x1b[2J).
    #[test]
    fn test_replay_roundtrip_small_scrollback() {
        // Server: 24-row screen, only 5 lines of scrollback overflow
        let mut server_vt = VirtualTerminal::new(24, 80, 4096, 100);
        // Write 28 lines → 4 scroll off into scrollback (28 - 24 = 4)
        for i in 0..28 {
            server_vt.process_output(format!("SLine {:02}\r\n", i).as_bytes());
        }

        let replay = server_vt.replay(24);

        // Client: same screen size, processes the replay
        let mut client = vt100::Parser::new(24, 80, 100);
        client.process(&replay);

        // Even though only 4 lines were in scrollback (fewer than 24 rows),
        // the client should still have them.
        client.screen_mut().set_scrollback(usize::MAX);
        let sb = client.screen().scrollback();
        assert!(
            sb >= 4,
            "client should have at least 4 scrollback lines, got {}",
            sb,
        );
    }

    /// Verify that \x1b[2J (Erase in Display) does NOT clear the scrollback
    /// buffer in vt100. If this fails, the keyframe in our replay format
    /// would destroy scrollback on the client side.
    #[test]
    fn test_ed2_does_not_clear_scrollback() {
        let mut parser = vt100::Parser::new(4, 40, 100);

        // Fill screen and overflow into scrollback
        for i in 0..10 {
            parser.process(format!("Line {}\r\n", i).as_bytes());
        }

        // Verify scrollback exists
        parser.screen_mut().set_scrollback(usize::MAX);
        let before = parser.screen().scrollback();
        assert!(before > 0, "should have scrollback before ED2");
        parser.screen_mut().set_scrollback(0);

        // Clear screen with ED2 (the escape used in our keyframe)
        parser.process(b"\x1b[H\x1b[2J\x1b[0m");

        // Scrollback should still be there
        parser.screen_mut().set_scrollback(usize::MAX);
        let after = parser.screen().scrollback();
        assert!(
            after > 0,
            "scrollback should survive ED2 (\\x1b[2J), before={} after={}",
            before,
            after
        );
    }

    // ── Replay round-trip helpers ────────────────────────────────────

    /// Feed replay bytes into a fresh client parser, return it.
    fn replay_roundtrip(
        server_vt: &mut VirtualTerminal,
        client_rows: u16,
        client_cols: u16,
    ) -> vt100::Parser {
        let replay = server_vt.replay(client_rows);
        let mut client = vt100::Parser::new(client_rows, client_cols, 1000);
        client.process(&replay);
        client
    }

    /// Extract visible lines from a screen (strips trailing whitespace).
    fn visible_lines(screen: &vt100::Screen) -> Vec<String> {
        let (rows, cols) = screen.size();
        (0..rows).map(|r| read_row_text(screen, r, cols)).collect()
    }

    /// Extract all scrollback lines from a parser (oldest first).
    fn scrollback_lines(parser: &mut vt100::Parser, cols: u16) -> Vec<String> {
        let depth = scrollback_depth(parser);
        let mut lines = Vec::new();
        for offset in (1..=depth).rev() {
            parser.screen_mut().set_scrollback(offset);
            lines.push(read_row_text(parser.screen(), 0, cols));
        }
        parser.screen_mut().set_scrollback(0);
        lines
    }

    // ── Replay rendering TDD tests ──────────────────────────────────

    /// Cell-level round-trip verification.
    #[test]
    fn test_replay_cell_content_exact() {
        let mut vt = VirtualTerminal::new(4, 40, 4096, 100);
        for i in 0..20 {
            vt.process_output(format!("Line {:02}\r\n", i).as_bytes());
        }

        let mut client = replay_roundtrip(&mut vt, 4, 40);
        let vis = visible_lines(client.screen());
        // Last \r\n scrolls cursor to row 3 → visible = [Line 17, Line 18, Line 19, ""]
        assert_eq!(vis[0], "Line 17", "visible row 0");
        assert_eq!(vis[1], "Line 18", "visible row 1");
        assert_eq!(vis[2], "Line 19", "visible row 2");
        assert_eq!(vis[3], "", "visible row 3 (empty after last \\r\\n)");

        let sb = scrollback_lines(&mut client, 40);
        // No blank lines should appear in scrollback — flush should be exact
        let blank_count = sb.iter().filter(|l| l.is_empty()).count();
        assert_eq!(
            blank_count,
            0,
            "scrollback should have no blank lines, got {} blank out of {} total",
            blank_count,
            sb.len()
        );
        // Scrollback should contain Line 00..Line 16 in order
        assert_eq!(
            sb.len(),
            17,
            "expected exactly 17 scrollback lines (Line 00..16), got {}",
            sb.len()
        );
        assert!(
            sb.first().unwrap().contains("Line 00"),
            "first scrollback line should be Line 00, got {:?}",
            sb.first()
        );
        assert!(
            sb.last().unwrap().contains("Line 16"),
            "last content scrollback line should be Line 16, got {:?}",
            sb.last()
        );
    }

    /// Regression test: no CUP sequences in the scrollback portion of replay.
    #[test]
    fn test_replay_no_cup_in_scrollback() {
        let mut vt = VirtualTerminal::new(4, 40, 4096, 100);
        for i in 0..20 {
            vt.process_output(format!("Line {:02}\r\n", i).as_bytes());
        }

        let replay = vt.replay(4);

        // Find the keyframe boundary: \x1b[H\x1b[2J marks the start of the
        // visible screen section. Everything before it is scrollback + the
        // scroll-push section. The scroll-push section starts with a single
        // CUP \x1b[N;1H followed by newlines — exclude it by finding the
        // last \r\n before the keyframe marker.
        let replay_str = String::from_utf8_lossy(&replay);
        let keyframe_marker = "\x1b[H\x1b[2J";
        let keyframe_pos = replay_str
            .find(keyframe_marker)
            .expect("replay should contain keyframe marker");
        // The scrollback rows all end with \r\n. Find the last one before the keyframe.
        let scrollback_end = replay_str[..keyframe_pos].rfind("\r\n").unwrap_or(0);
        let scrollback_portion = &replay_str[..scrollback_end];

        // Scan for CUP sequences: \x1b[ <digits> ; <digits> H
        // Manual scan avoids a regex dependency.
        let sb_bytes = scrollback_portion.as_bytes();
        let mut cups_found = Vec::new();
        let mut i = 0;
        while i < sb_bytes.len().saturating_sub(4) {
            if sb_bytes[i] == b'\x1b' && sb_bytes[i + 1] == b'[' {
                let start = i;
                let mut j = i + 2;
                // digits
                while j < sb_bytes.len() && sb_bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j < sb_bytes.len() && sb_bytes[j] == b';' {
                    j += 1;
                    while j < sb_bytes.len() && sb_bytes[j].is_ascii_digit() {
                        j += 1;
                    }
                    if j < sb_bytes.len() && sb_bytes[j] == b'H' {
                        cups_found.push(String::from_utf8_lossy(&sb_bytes[start..=j]).to_string());
                    }
                }
            }
            i += 1;
        }
        assert!(
            cups_found.is_empty(),
            "scrollback portion should contain no CUP sequences, found: {:?}",
            cups_found
        );
    }

    /// The specific trigger for the CUP bug: lines with leading spaces.
    #[test]
    fn test_replay_scrollback_with_indented_lines() {
        let mut vt = VirtualTerminal::new(4, 40, 4096, 100);
        for i in 0..12 {
            vt.process_output(format!("    indented line {}\r\n", i).as_bytes());
        }

        let mut client = replay_roundtrip(&mut vt, 4, 40);
        let sb = scrollback_lines(&mut client, 40);

        let blank_count = sb.iter().filter(|l| l.is_empty()).count();
        assert_eq!(blank_count, 0, "scrollback should have no blank lines");

        assert_eq!(
            sb.len(),
            9,
            "expected exactly 9 scrollback lines (indented line 0..8), got {}",
            sb.len()
        );

        // All scrollback lines should have the indentation and correct content
        for (idx, line) in sb.iter().enumerate() {
            assert!(
                line.starts_with("    indented line"),
                "scrollback line {} should start with '    indented line', got {:?}",
                idx,
                line
            );
        }

        // No line should contain content from another line (no overwrite artifacts)
        for line in &sb {
            let matches: Vec<_> = sb
                .iter()
                .filter(|other| line.contains(other.as_str()))
                .collect();
            assert!(
                matches.len() <= 1,
                "line {:?} should not contain content from other lines",
                line
            );
        }
    }

    /// Rows with trailing empty cells (short lines in a wide terminal).
    #[test]
    fn test_replay_scrollback_with_partial_lines() {
        let mut vt = VirtualTerminal::new(4, 80, 4096, 100);
        for i in 0..12 {
            vt.process_output(format!("Hi {}\r\n", i).as_bytes());
        }

        let mut client = replay_roundtrip(&mut vt, 4, 80);
        let sb = scrollback_lines(&mut client, 80);

        let blank_count = sb.iter().filter(|l| l.is_empty()).count();
        assert_eq!(blank_count, 0, "scrollback should have no blank lines");

        assert_eq!(
            sb.len(),
            9,
            "expected exactly 9 scrollback lines (Hi 0..8), got {}",
            sb.len()
        );

        for (idx, line) in sb.iter().enumerate() {
            assert!(
                line.starts_with("Hi "),
                "scrollback line {} should start with 'Hi ', got {:?}",
                idx,
                line
            );
        }
    }

    /// Client bigger than server — content should still be correct.
    #[test]
    fn test_replay_mismatched_dims() {
        let mut vt = VirtualTerminal::new(4, 40, 4096, 100);
        for i in 0..20 {
            vt.process_output(format!("Line {:02}\r\n", i).as_bytes());
        }

        let mut client = replay_roundtrip(&mut vt, 24, 80);
        let vis = visible_lines(client.screen());

        // The visible screen on the client should contain the server's visible content
        let vis_text = vis.join("\n");
        assert!(
            vis_text.contains("Line 17"),
            "client should show server's visible content"
        );
        assert!(
            vis_text.contains("Line 19"),
            "client should show server's visible content"
        );

        let sb = scrollback_lines(&mut client, 80);
        // Earlier lines should be in scrollback
        let sb_text = sb.join("\n");
        assert!(
            sb_text.contains("Line 00"),
            "earlier lines should be in client scrollback"
        );
    }

    /// SGR round-trip: colors survive replay.
    #[test]
    fn test_replay_scrollback_colors_preserved() {
        let mut vt = VirtualTerminal::new(4, 40, 4096, 100);
        for i in 0..8 {
            vt.process_output(format!("\x1b[31mRed {}\x1b[0m\r\n", i).as_bytes());
        }

        let mut client = replay_roundtrip(&mut vt, 4, 40);

        // Check scrollback cells for red foreground
        client.screen_mut().set_scrollback(usize::MAX);
        let depth = client.screen().scrollback();
        assert!(depth > 0, "client should have scrollback");

        // Check the first scrollback row (oldest)
        client.screen_mut().set_scrollback(depth);
        let cell = client.screen().cell(0, 0).unwrap();
        assert_eq!(
            cell.fgcolor(),
            vt100::Color::Idx(1),
            "scrollback cell should have red foreground, got {:?}",
            cell.fgcolor()
        );
        client.screen_mut().set_scrollback(0);
    }

    /// Ordering check across multiple pages — no repeats or gaps.
    #[test]
    fn test_replay_scrollback_order_preserved() {
        let mut vt = VirtualTerminal::new(4, 40, 4096, 500);
        for i in 0..100 {
            vt.process_output(format!("Line {:04}\r\n", i).as_bytes());
        }

        let mut client = replay_roundtrip(&mut vt, 4, 40);
        let sb = scrollback_lines(&mut client, 40);

        // Extract line numbers from scrollback and verify ascending order
        let mut prev_num: Option<u32> = None;
        for line in &sb {
            if let Some(pos) = line.find("Line ") {
                let num_str = &line[pos + 5..pos + 9];
                if let Ok(num) = num_str.trim().parse::<u32>() {
                    if let Some(p) = prev_num {
                        assert!(
                            num > p,
                            "scrollback lines should be in ascending order: {} followed by {}",
                            p,
                            num
                        );
                    }
                    prev_num = Some(num);
                }
            }
        }
        assert!(
            prev_num.is_some(),
            "should have found numbered lines in scrollback"
        );
    }

    /// Scrollback survives dimension change after SIGWINCH redraw.
    #[test]
    fn test_replay_after_resize() {
        let mut vt = VirtualTerminal::new(4, 40, 4096, 100);
        for i in 0..20 {
            vt.process_output(format!("Line {:02}\r\n", i).as_bytes());
        }

        // Resize to 8×60
        vt.update_viewport("cli", 8, 60, ClientType::Terminal);

        // Simulate SIGWINCH redraw
        vt.process_output(b"\x1b[2J\x1b[H");
        for i in 0..20 {
            vt.process_output(format!("Line {:02}\r\n", i).as_bytes());
        }

        let mut client = replay_roundtrip(&mut vt, 8, 60);
        let sb = scrollback_lines(&mut client, 60);

        let sb_text = sb.join("\n");
        assert!(
            sb_text.contains("Line 00"),
            "scrollback should survive resize"
        );
        assert!(
            sb_text.contains("Line 10"),
            "mid-range scrollback should survive resize"
        );
    }

    /// CJK / wide characters in scrollback.
    #[test]
    fn test_replay_wide_chars_in_scrollback() {
        let mut vt = VirtualTerminal::new(4, 40, 4096, 100);
        for i in 0..8 {
            vt.process_output(format!("行{} test\r\n", i).as_bytes());
        }

        let mut client = replay_roundtrip(&mut vt, 4, 40);
        let sb = scrollback_lines(&mut client, 40);

        let blank_count = sb.iter().filter(|l| l.is_empty()).count();
        assert_eq!(blank_count, 0, "scrollback should have no blank lines");

        assert_eq!(
            sb.len(),
            5,
            "expected exactly 5 CJK scrollback lines, got {}",
            sb.len()
        );

        for (idx, line) in sb.iter().enumerate() {
            assert!(
                line.contains("行"),
                "scrollback line {} should contain CJK char, got {:?}",
                idx,
                line
            );
            assert!(
                line.contains("test"),
                "scrollback line {} should contain 'test', got {:?}",
                idx,
                line
            );
        }
    }

    /// Resize + SIGWINCH redraw: scrollback survives and content is accessible.
    /// Simulates: TUI at 53×209, web connects at 45×156, PTY redraws.
    #[test]
    fn test_resize_sigwinch_preserves_scrollback() {
        let mut vt = VirtualTerminal::new(53, 209, 4096, 10000);

        // Fill terminal: 89 lines → 36 scrollback + 53 visible
        for i in 0..89 {
            vt.process_output(format!("Line {:03}\r\n", i).as_bytes());
        }

        // Web client connects → resize to 45×156
        vt.resize(45, 156);

        // Simulate SIGWINCH redraw: PTY clears screen and re-outputs content
        vt.process_output(b"\x1b[H\x1b[2J");
        for i in 0..89 {
            vt.process_output(format!("Line {:03}\r\n", i).as_bytes());
        }

        // Replay for a 45-row client
        let mut client = replay_roundtrip(&mut vt, 45, 156);
        let sb = scrollback_lines(&mut client, 156);

        // All lines should be reachable in scrollback
        assert!(
            sb.iter().any(|l| l.contains("Line 000")),
            "earliest line should be in scrollback"
        );
        assert!(
            sb.iter().any(|l| l.contains("Line 044")),
            "mid-range line should be in scrollback"
        );
    }

    /// Regression: small scrollback (< client_rows) must not inject blank
    /// lines into the client's scrollback buffer during flush.
    #[test]
    fn test_replay_flush_no_blank_gap() {
        // Simulate the exact production scenario: 53-row client, few scrollback lines
        let mut vt = VirtualTerminal::new(53, 209, 4096, 10000);
        // 58 lines in 53 rows → 6 scrollback lines (Line 00..05)
        for i in 0..58 {
            vt.process_output(format!("Line {:02}\r\n", i).as_bytes());
        }

        let mut client = replay_roundtrip(&mut vt, 53, 209);
        let sb = scrollback_lines(&mut client, 209);

        // Every scrollback line must have content — no blank gap
        for (idx, line) in sb.iter().enumerate() {
            assert!(
                !line.is_empty(),
                "scrollback line {} is blank (flush injected empty rows)",
                idx,
            );
        }

        assert_eq!(sb.len(), 6, "expected 6 scrollback lines, got {}", sb.len());
        assert!(sb[0].contains("Line 00"));
        assert!(sb[5].contains("Line 05"));
    }

    // ── View-switch test harness ────────────────────────────────────
    //
    // Simulates the full server-side flow for web terminal view switches:
    // TerminalVisible → resize → SIGWINCH redraw → replay()
    // TerminalHidden → set_active(false) → possible resize
    //
    // The harness tracks server VT state across multiple cycles and checks
    // for scrollback corruption / duplication.

    /// Extract ALL content from a VT: scrollback + visible, in order.
    /// Returns (scrollback_lines, visible_lines) — both oldest-first.
    fn server_all_content(vt: &mut VirtualTerminal) -> (Vec<String>, Vec<String>) {
        let (_rows, cols) = vt.parser.screen().size();

        let depth = scrollback_depth(&mut vt.parser);
        let mut sb = Vec::new();
        for offset in (1..=depth).rev() {
            vt.parser.screen_mut().set_scrollback(offset);
            sb.push(read_row_text(vt.parser.screen(), 0, cols));
        }
        vt.parser.screen_mut().set_scrollback(0);

        let vis = visible_lines(vt.parser.screen());

        (sb, vis)
    }

    /// Extract ALL content from a client parser: scrollback + visible.
    fn client_all_content(parser: &mut vt100::Parser) -> (Vec<String>, Vec<String>) {
        let (_rows, cols) = parser.screen().size();
        let sb = scrollback_lines(parser, cols);
        let vis = visible_lines(parser.screen());
        (sb, vis)
    }

    /// Count occurrences of a needle across both scrollback and visible lines.
    fn count_in_all(sb: &[String], vis: &[String], needle: &str) -> usize {
        sb.iter().filter(|l| l.contains(needle)).count()
            + vis.iter().filter(|l| l.contains(needle)).count()
    }

    /// Assert no line from a known set appears more than once across
    /// scrollback + visible content. Searches for exact lines that start
    /// with `prefix` followed by the zero-padded number with `width` digits.
    fn assert_no_duplicates(
        sb: &[String],
        vis: &[String],
        prefix: &str,
        count: usize,
        width: usize,
        context: &str,
    ) {
        let mut dups = Vec::new();
        for i in 0..count {
            let needle = format!("{}{:0>width$}", prefix, i);
            let n = count_in_all(sb, vis, &needle);
            if n > 1 {
                dups.push((needle, n));
            }
        }
        assert!(
            dups.is_empty(),
            "{}: found {} duplicated lines.\n  dups: {:?}\n  scrollback[..20]: {:?}\n  visible: {:?}",
            context,
            dups.len(),
            &dups[..dups.len().min(10)],
            &sb[..sb.len().min(20)],
            vis
        );
    }

    /// Simulate a SIGWINCH redraw: the child process clears and redraws
    /// content at the current terminal dimensions.
    fn simulate_sigwinch_redraw(vt: &mut VirtualTerminal, label: &str) {
        let (rows, cols) = vt.effective_dims();
        let mut redraw = Vec::new();
        redraw.extend_from_slice(b"\x1b[H\x1b[2J");
        for r in 0..rows {
            redraw
                .extend_from_slice(format!("{} r{:02} {}x{}\r\n", label, r, rows, cols).as_bytes());
        }
        vt.process_output(&redraw);
    }

    /// Simulate TerminalVisible: set active, possibly resize, SIGWINCH, replay.
    /// Returns the replay bytes.
    fn simulate_terminal_visible(
        vt: &mut VirtualTerminal,
        conn_id: &str,
        rows: u16,
        cols: u16,
        client_type: ClientType,
    ) -> Vec<u8> {
        // TerminalVisible in handler.rs:
        // 1. update_viewport (marks active, may change dims)
        // 2. resize PTY if dims changed
        // 3. send replay
        if let Some((new_rows, new_cols)) =
            vt.update_viewport(conn_id, rows, cols, client_type.clone())
        {
            vt.resize(new_rows, new_cols);
            // In reality, SIGWINCH response arrives asynchronously.
            // We simulate it synchronously for determinism.
            simulate_sigwinch_redraw(vt, "SIGWINCH");
        }
        vt.replay(rows)
    }

    /// Simulate TerminalHidden: set inactive, possibly resize + SIGWINCH.
    fn simulate_terminal_hidden(vt: &mut VirtualTerminal, conn_id: &str) {
        if let Some((new_rows, new_cols)) = vt.set_active(conn_id, false) {
            vt.resize(new_rows, new_cols);
            simulate_sigwinch_redraw(vt, "SIGWINCH-hidden");
        }
    }

    // ── View-switch scenario tests ───────────────────────────────────

    /// Single web client, no TUI. View-switch cycles should NOT corrupt
    /// server scrollback since dims don't change.
    #[test]
    fn test_harness_single_client_view_switch_no_corruption() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 500);
        vt.update_viewport("web", 24, 80, ClientType::Web);

        // Fill with identifiable content
        for i in 0..60 {
            vt.process_output(format!("Orig {:03}\r\n", i).as_bytes());
        }

        // Baseline: check server state
        let (sb_before, vis_before) = server_all_content(&mut vt);
        let _content_before = sb_before
            .iter()
            .chain(vis_before.iter())
            .filter(|l| !l.is_empty())
            .cloned()
            .collect::<Vec<_>>();

        // 10 rapid view-switch cycles
        for cycle in 0..10 {
            simulate_terminal_hidden(&mut vt, "web");
            let replay = simulate_terminal_visible(&mut vt, "web", 24, 80, ClientType::Web);

            // Check replay produces correct client state
            let mut client = vt100::Parser::new(24, 80, 1000);
            client.process(&replay);
            let (csb, cvis) = client_all_content(&mut client);
            assert_no_duplicates(
                &csb,
                &cvis,
                "Orig ",
                60,
                3,
                &format!("cycle {} client replay", cycle),
            );
        }

        // Check server state after all cycles
        let (sb_after, vis_after) = server_all_content(&mut vt);
        assert_no_duplicates(
            &sb_after,
            &vis_after,
            "Orig ",
            60,
            3,
            "server after 10 single-client cycles",
        );
    }

    /// Web + TUI at different dims. View-switch cycles cause resize,
    /// which triggers SIGWINCH redraws. Server scrollback should not
    /// duplicate original content.
    #[test]
    fn test_harness_dual_client_resize_cycle_no_corruption() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 500);
        vt.update_viewport("web", 24, 80, ClientType::Web);
        vt.update_viewport("tui", 40, 120, ClientType::Terminal);
        // Effective = min(24,40) x min(80,120) = 24x80

        for i in 0..60 {
            vt.process_output(format!("Orig {:03}\r\n", i).as_bytes());
        }

        for cycle in 0..5 {
            // Web hidden → dims expand to TUI's 40×120
            simulate_terminal_hidden(&mut vt, "web");

            // Web visible → dims shrink back to 24×80
            let replay = simulate_terminal_visible(&mut vt, "web", 24, 80, ClientType::Web);

            let mut client = vt100::Parser::new(24, 80, 1000);
            client.process(&replay);
            let (csb, cvis) = client_all_content(&mut client);
            assert_no_duplicates(
                &csb,
                &cvis,
                "Orig ",
                60,
                3,
                &format!("cycle {} client replay", cycle),
            );
        }

        let (sb_after, vis_after) = server_all_content(&mut vt);
        assert_no_duplicates(
            &sb_after,
            &vis_after,
            "Orig ",
            60,
            3,
            "server after 5 dual-client resize cycles",
        );
    }

    /// View switches with ACTIVE PTY output between each switch.
    /// New lines arrive while the terminal is hidden.
    #[test]
    fn test_harness_view_switch_with_interleaved_output() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 500);
        vt.update_viewport("web", 24, 80, ClientType::Web);

        for i in 0..40 {
            vt.process_output(format!("Orig {:03}\r\n", i).as_bytes());
        }

        for cycle in 0..5 {
            simulate_terminal_hidden(&mut vt, "web");

            // Output arrives while terminal is hidden (Claude is working)
            for j in 0..10 {
                vt.process_output(format!("Hidden c{}j{:02}\r\n", cycle, j).as_bytes());
            }

            let replay = simulate_terminal_visible(&mut vt, "web", 24, 80, ClientType::Web);

            let mut client = vt100::Parser::new(24, 80, 1000);
            client.process(&replay);
            let (csb, cvis) = client_all_content(&mut client);

            // Original lines: no dups
            assert_no_duplicates(
                &csb,
                &cvis,
                "Orig ",
                40,
                3,
                &format!("cycle {} client originals", cycle),
            );

            // Hidden-phase lines from THIS cycle should appear exactly once
            for j in 0..10 {
                let needle = format!("Hidden c{}j{:02}", cycle, j);
                let n = count_in_all(&csb, &cvis, &needle);
                assert!(
                    n <= 1,
                    "cycle {}: '{}' appears {} times in client replay",
                    cycle,
                    needle,
                    n
                );
            }
        }

        let (sb_after, vis_after) = server_all_content(&mut vt);
        assert_no_duplicates(
            &sb_after,
            &vis_after,
            "Orig ",
            40,
            3,
            "server after interleaved output cycles",
        );
    }

    /// Multiple replays generated WITHOUT client-side clear between them.
    /// This simulates the scenario where a second replay is sent before
    /// the client processes the first (e.g., rapid Focus + TerminalVisible).
    #[test]
    fn test_harness_consecutive_replays_to_same_client() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 500);
        vt.update_viewport("web", 24, 80, ClientType::Web);

        for i in 0..60 {
            vt.process_output(format!("Orig {:03}\r\n", i).as_bytes());
        }

        let replay1 = vt.replay(24);
        let replay2 = vt.replay(24);
        let replay3 = vt.replay(24);

        // KEY TEST: replay() should be idempotent — calling it multiple
        // times without intervening mutations must produce identical output.
        assert_eq!(
            replay1,
            replay2,
            "replay() is NOT idempotent! Second call differs from first.\n  \
             replay1 len={}, replay2 len={}",
            replay1.len(),
            replay2.len()
        );
        assert_eq!(
            replay2, replay3,
            "replay() is NOT idempotent! Third call differs from second."
        );

        // Single replay: clean
        let mut single = vt100::Parser::new(24, 80, 1000);
        single.process(&replay1);
        let (ssb, svis) = client_all_content(&mut single);
        assert_no_duplicates(&ssb, &svis, "Orig ", 60, 3, "single replay");

        // Double replay WITHOUT clear: does it corrupt?
        let mut double = vt100::Parser::new(24, 80, 1000);
        double.process(&replay1);
        double.process(&replay2);
        let (dsb, dvis) = client_all_content(&mut double);

        // Count how many lines are duplicated
        let mut dup_count = 0;
        for i in 0..60 {
            let needle = format!("Orig {:03}", i);
            let n = count_in_all(&dsb, &dvis, &needle);
            if n > 1 {
                dup_count += 1;
            }
        }

        // Document the result — double replay without clear causes dups.
        // This is expected: ED2 clears the visible screen but NOT the
        // client's scrollback buffer, so the second replay's scrollback
        // section writes on top of the first's.
        assert!(
            dup_count > 0,
            "double replay without clear should produce duplicates (documenting the bug)"
        );

        // With a fresh client (what actually happens — each attach/view-switch
        // either creates a new xterm.js terminal or calls terminal.clear()
        // which resets scrollback):
        let mut fresh = vt100::Parser::new(24, 80, 1000);
        fresh.process(&replay2);
        let (fsb, fvis) = client_all_content(&mut fresh);
        assert_no_duplicates(
            &fsb,
            &fvis,
            "Orig ",
            60,
            3,
            "fresh client for second replay",
        );
    }

    /// Replay with different client_rows each time (simulates browser
    /// window resize between view switches).
    #[test]
    fn test_harness_replay_varying_client_rows() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 500);
        vt.update_viewport("web", 24, 80, ClientType::Web);

        for i in 0..60 {
            vt.process_output(format!("Orig {:03}\r\n", i).as_bytes());
        }

        let client_heights = [24, 30, 20, 35, 24];
        for (idx, &rows) in client_heights.iter().enumerate() {
            let replay = vt.replay(rows);
            let mut client = vt100::Parser::new(rows, 80, 1000);
            client.process(&replay);
            let (csb, cvis) = client_all_content(&mut client);
            assert_no_duplicates(
                &csb,
                &cvis,
                "Orig ",
                60,
                3,
                &format!("varying rows: height={} (iteration {})", rows, idx),
            );
        }
    }

    /// Stress test: rapid alternation with output, varying dims.
    #[test]
    fn test_harness_stress_view_switch() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 500);
        vt.update_viewport("web", 24, 80, ClientType::Web);
        vt.update_viewport("tui", 36, 100, ClientType::Terminal);

        for i in 0..100 {
            vt.process_output(format!("Init {:04}\r\n", i).as_bytes());
        }

        for cycle in 0..20 {
            // Vary what happens each cycle
            if cycle % 3 == 0 {
                // Output while visible
                vt.process_output(format!("Active {:04}\r\n", cycle).as_bytes());
            }

            simulate_terminal_hidden(&mut vt, "web");

            if cycle % 2 == 0 {
                // Output while hidden
                vt.process_output(format!("Background {:04}\r\n", cycle).as_bytes());
            }

            let rows = if cycle % 4 == 0 { 30 } else { 24 };
            let replay = simulate_terminal_visible(&mut vt, "web", rows, 80, ClientType::Web);

            let mut client = vt100::Parser::new(rows, 80, 1000);
            client.process(&replay);
            let (csb, cvis) = client_all_content(&mut client);
            assert_no_duplicates(
                &csb,
                &cvis,
                "Init ",
                100,
                4,
                &format!("stress cycle {}", cycle),
            );
        }

        let (sb_final, vis_final) = server_all_content(&mut vt);
        assert_no_duplicates(
            &sb_final,
            &vis_final,
            "Init ",
            100,
            4,
            "server after 20 stress cycles",
        );
    }

    // ── Realistic output tests ──────────────────────────────────────

    /// Simulate realistic Claude output: SGR codes, cursor movement,
    /// ED2 redraws (like Claude Code's TUI), and progressive scrolling.
    /// Then replay and check for duplicates.
    #[test]
    fn test_harness_realistic_claude_output_replay() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 500);
        vt.update_viewport("web", 24, 80, ClientType::Web);

        // Phase 1: Claude thinking (streaming text with SGR)
        for i in 0..30 {
            let line = format!("\x1b[1m\x1b[36m⠿\x1b[0m Thinking... step {}\r\n", i);
            vt.process_output(line.as_bytes());
        }

        // Phase 2: Claude outputs code with syntax highlighting
        vt.process_output(b"\x1b[32m// Generated code:\x1b[0m\r\n");
        for i in 0..20 {
            let line = format!(
                "\x1b[33mfn\x1b[0m \x1b[34mfunction_{:02}\x1b[0m() {{\r\n    \x1b[32mprintln!\x1b[0m(\"hello {}\");\r\n}}\r\n",
                i, i
            );
            vt.process_output(line.as_bytes());
        }

        // Phase 3: ED2 full-screen redraw (like Claude Code's status bar update)
        let mut redraw = Vec::new();
        redraw.extend_from_slice(b"\x1b[H\x1b[2J");
        for r in 0..24 {
            redraw.extend_from_slice(
                format!("\x1b[{};1H\x1b[36mStatus line {:02}\x1b[0m\r\n", r + 1, r).as_bytes(),
            );
        }
        vt.process_output(&redraw);

        // Phase 4: More progressive output after the redraw
        for i in 0..15 {
            vt.process_output(format!("Post-redraw line {:02}\r\n", i).as_bytes());
        }

        // First replay: baseline
        let replay1 = vt.replay(24);
        let mut client1 = vt100::Parser::new(24, 80, 1000);
        client1.process(&replay1);
        let (sb1, vis1) = client_all_content(&mut client1);

        // Check for duplicate "function_" lines in scrollback
        for i in 0..20 {
            let needle = format!("function_{:02}", i);
            let n = count_in_all(&sb1, &vis1, &needle);
            assert!(
                n <= 1,
                "realistic output: '{}' appears {} times after first replay.\n  \
                 scrollback[..10]: {:?}",
                needle,
                n,
                &sb1[..sb1.len().min(10)]
            );
        }

        // Simulate view switch: hidden then visible (no resize for single client)
        simulate_terminal_hidden(&mut vt, "web");
        let replay2 = simulate_terminal_visible(&mut vt, "web", 24, 80, ClientType::Web);

        // Replay to fresh client
        let mut client2 = vt100::Parser::new(24, 80, 1000);
        client2.process(&replay2);
        let (sb2, vis2) = client_all_content(&mut client2);

        for i in 0..20 {
            let needle = format!("function_{:02}", i);
            let n = count_in_all(&sb2, &vis2, &needle);
            assert!(
                n <= 1,
                "realistic output: '{}' appears {} times after view-switch.\n  \
                 scrollback[..10]: {:?}",
                needle,
                n,
                &sb2[..sb2.len().min(10)]
            );
        }
    }

    /// Test with REAL full-screen application behavior: cursor save/restore,
    /// alternate screen, scrolling regions.
    #[test]
    fn test_harness_fullscreen_app_scrollback() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 500);
        vt.update_viewport("web", 24, 80, ClientType::Web);

        // Phase 1: Normal scrolling output
        for i in 0..50 {
            vt.process_output(format!("Normal line {:03}\r\n", i).as_bytes());
        }

        // Phase 2: Enter alternate screen (like vim/less)
        vt.process_output(b"\x1b[?1049h"); // Save cursor + switch to alt screen
        vt.process_output(b"\x1b[H\x1b[2J"); // Clear alt screen
        for r in 0..24 {
            vt.process_output(format!("\x1b[{};1HAlt screen row {}\r\n", r + 1, r).as_bytes());
        }

        // Phase 3: Exit alternate screen (back to normal)
        vt.process_output(b"\x1b[?1049l"); // Restore cursor + switch back

        // Phase 4: More normal output
        for i in 50..70 {
            vt.process_output(format!("Normal line {:03}\r\n", i).as_bytes());
        }

        let replay = vt.replay(24);
        let mut client = vt100::Parser::new(24, 80, 1000);
        client.process(&replay);
        let (csb, cvis) = client_all_content(&mut client);

        // Check that Normal lines aren't duplicated
        for i in 0..70 {
            let needle = format!("Normal line {:03}", i);
            let n = count_in_all(&csb, &cvis, &needle);
            assert!(
                n <= 1,
                "fullscreen app: '{}' appears {} times.\n  scrollback[..10]: {:?}",
                needle,
                n,
                &csb[..csb.len().min(10)]
            );
        }

        // Alt screen content should NOT be in scrollback
        let alt_in_sb = csb.iter().filter(|l| l.contains("Alt screen")).count();
        eprintln!(
            "Alt screen lines in scrollback: {} (should be 0)",
            alt_in_sb
        );
    }

    /// The actual user scenario: web-only, open terminal → close → open.
    /// Between open/close, Claude is actively producing output.
    /// Check both server VT state and replay output.
    #[test]
    fn test_harness_open_close_open_with_active_output() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 500);
        vt.update_viewport("web", 24, 80, ClientType::Web);

        // Claude running — initial output
        for i in 0..40 {
            vt.process_output(format!("Claude {:03}\r\n", i).as_bytes());
        }

        // OPEN terminal — TerminalVisible → replay sent
        let replay_open1 = vt.replay(24);
        let mut c1 = vt100::Parser::new(24, 80, 1000);
        c1.process(&replay_open1);
        let (sb1, vis1) = client_all_content(&mut c1);
        let sb1_count = sb1.iter().filter(|l| !l.is_empty()).count();
        eprintln!(
            "OPEN 1: scrollback={}, visible non-empty={}",
            sb1_count,
            vis1.iter().filter(|l| !l.is_empty()).count()
        );

        // Claude continues while terminal is open
        for i in 40..60 {
            vt.process_output(format!("Claude {:03}\r\n", i).as_bytes());
        }

        // CLOSE terminal — TerminalHidden
        simulate_terminal_hidden(&mut vt, "web");

        // Claude continues while terminal is hidden
        for i in 60..80 {
            vt.process_output(format!("Claude {:03}\r\n", i).as_bytes());
        }

        // OPEN terminal — TerminalVisible → replay sent
        let replay_open2 = vt.replay(24);

        // Check server VT state
        let (server_sb, server_vis) = server_all_content(&mut vt);
        eprintln!(
            "SERVER: scrollback={}, visible non-empty={}",
            server_sb.iter().filter(|l| !l.is_empty()).count(),
            server_vis.iter().filter(|l| !l.is_empty()).count()
        );
        assert_no_duplicates(
            &server_sb,
            &server_vis,
            "Claude ",
            80,
            3,
            "server VT after open/close/open",
        );

        // Check replay to fresh client
        let mut c2 = vt100::Parser::new(24, 80, 1000);
        c2.process(&replay_open2);
        let (sb2, vis2) = client_all_content(&mut c2);
        let sb2_count = sb2.iter().filter(|l| !l.is_empty()).count();
        eprintln!(
            "OPEN 2: scrollback={}, visible non-empty={}",
            sb2_count,
            vis2.iter().filter(|l| !l.is_empty()).count()
        );
        assert_no_duplicates(&sb2, &vis2, "Claude ", 80, 3, "client after second open");

        // The key check: second open should have MORE content but no dups
        assert!(
            sb2_count > sb1_count,
            "Second open should have more scrollback ({}) than first ({})",
            sb2_count,
            sb1_count
        );
    }

    /// THE USER'S BUG: after replay, scrollback should not have a
    /// "screen's height of empty lines" gap between content sections.
    /// This tests that the flush step doesn't inject visible empty gaps.
    #[test]
    fn test_harness_no_empty_gap_in_client_scrollback() {
        // Vary content amount and screen dimensions
        for &(rows, lines) in &[(24, 50), (24, 100), (30, 60), (40, 200)] {
            let mut vt = VirtualTerminal::new(rows, 80, 4096, 500);
            vt.update_viewport("web", rows, 80, ClientType::Web);

            for i in 0..lines {
                vt.process_output(format!("Line {:04}\r\n", i).as_bytes());
            }

            let replay = vt.replay(rows);
            let mut client = vt100::Parser::new(rows, 80, 1000);
            client.process(&replay);

            // Collect ALL client scrollback lines
            let sb = scrollback_lines(&mut client, 80);
            let _vis = visible_lines(client.screen());

            // Find runs of empty lines in scrollback
            let mut empty_run = 0;
            let mut max_empty_run = 0;
            let mut max_empty_run_at = 0;
            for (i, line) in sb.iter().enumerate() {
                if line.is_empty() {
                    empty_run += 1;
                    if empty_run > max_empty_run {
                        max_empty_run = empty_run;
                        max_empty_run_at = i - empty_run + 1;
                    }
                } else {
                    empty_run = 0;
                }
            }

            // A gap of more than 1 empty line is suspicious.
            // A gap of `rows` empty lines is the flush bug.
            assert!(
                max_empty_run <= 1,
                "{}x80 with {} lines: found {} consecutive empty lines \
                 in client scrollback starting at index {} \
                 (total scrollback: {} lines)\n  \
                 lines around gap: {:?}",
                rows,
                lines,
                max_empty_run,
                max_empty_run_at,
                sb.len(),
                &sb[max_empty_run_at.saturating_sub(3)
                    ..(max_empty_run_at + max_empty_run + 3).min(sb.len())]
            );
        }
    }

    /// Dump raw replay structure to understand exactly what's in it.
    /// This test doesn't assert — it prints diagnostic info.
    #[test]
    fn test_diag_replay_bytes_structure() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 500);
        vt.update_viewport("web", 24, 80, ClientType::Web);

        // Write enough content to overflow into scrollback
        for i in 0..50 {
            vt.process_output(format!("Line {:03}\r\n", i).as_bytes());
        }

        let replay = vt.replay(24);
        let replay_str = String::from_utf8_lossy(&replay);

        // Find the ED2 boundary that separates scrollback from keyframe
        let ed2_positions: Vec<usize> = replay_str
            .match_indices("\x1b[H\x1b[2J")
            .map(|(pos, _)| pos)
            .collect();
        eprintln!("Replay total bytes: {}", replay.len());
        eprintln!("ED2 (\\x1b[H\\x1b[2J) positions: {:?}", ed2_positions);

        // The replay structure should be:
        // [scrollback lines \r\n ...] [CUP last row] [\n * client_rows] [ED2] [keyframe]
        if let Some(&ed2_pos) = ed2_positions.last() {
            let scrollback_section = &replay_str[..ed2_pos];
            let keyframe_section = &replay_str[ed2_pos..];

            // Count lines in scrollback section
            let scrollback_lines: Vec<&str> = scrollback_section
                .split("\r\n")
                .filter(|l| !l.is_empty() && !l.starts_with('\x1b'))
                .collect();
            eprintln!(
                "Scrollback section: {} bytes, ~{} content lines",
                scrollback_section.len(),
                scrollback_lines.len()
            );
            if !scrollback_lines.is_empty() {
                eprintln!(
                    "  first 3: {:?}",
                    &scrollback_lines[..scrollback_lines.len().min(3)]
                );
                eprintln!(
                    "  last 3: {:?}",
                    &scrollback_lines[scrollback_lines.len().saturating_sub(3)..]
                );
            }

            eprintln!("Keyframe section: {} bytes", keyframe_section.len());
        }

        // Now check: does the server VT report the same scrollback depth
        // as we emitted?
        vt.parser.screen_mut().set_scrollback(usize::MAX);
        let server_sb_depth = vt.parser.screen().scrollback();
        vt.parser.screen_mut().set_scrollback(0);
        eprintln!("Server VT scrollback depth: {}", server_sb_depth);

        // Verify round-trip: client should have same scrollback depth
        let mut client = vt100::Parser::new(24, 80, 1000);
        client.process(&replay);
        client.screen_mut().set_scrollback(usize::MAX);
        let client_sb_depth = client.screen().scrollback();
        client.screen_mut().set_scrollback(0);
        eprintln!("Client scrollback depth after replay: {}", client_sb_depth);

        // Check what's in the extra scrollback line(s)
        if client_sb_depth != server_sb_depth {
            eprintln!(
                "MISMATCH: server={} client={}",
                server_sb_depth, client_sb_depth
            );

            // Dump the extra client scrollback lines
            let server_sb = scrollback_lines(&mut vt.parser, 80);
            let client_sb = scrollback_lines(&mut client, 80);
            for (i, line) in client_sb.iter().enumerate() {
                let in_server = i < server_sb.len() && server_sb[i] == *line;
                if !in_server {
                    eprintln!(
                        "  CLIENT EXTRA line [{}]: {:?}",
                        i,
                        &line[..line.len().min(40)]
                    );
                }
            }

            // Also check: are the matching lines in the same order?
            let mut mismatches = 0;
            for i in 0..server_sb.len().min(client_sb.len()) {
                if server_sb[i] != client_sb[i] {
                    if mismatches < 5 {
                        eprintln!(
                            "  MISMATCH at [{}]: server={:?} client={:?}",
                            i,
                            &server_sb[i][..server_sb[i].len().min(30)],
                            &client_sb[i][..client_sb[i].len().min(30)]
                        );
                    }
                    mismatches += 1;
                }
            }
            if mismatches > 0 {
                eprintln!("  Total mismatches: {}", mismatches);
            }
        }

        // Check for actual content duplication (the real bug)
        let client_sb_full = scrollback_lines(&mut client, 80);
        let client_vis = visible_lines(client.screen());
        for i in 0..50 {
            let needle = format!("Line {:03}", i);
            let n = count_in_all(&client_sb_full, &client_vis, &needle);
            assert!(
                n <= 1,
                "After replay: '{}' appears {} times (server_sb={}, client_sb={})",
                needle,
                n,
                server_sb_depth,
                client_sb_depth
            );
        }
    }

    // ── Diagnostic: trace server scrollback through resize cycle ─────

    /// Trace scrollback contents through each step of a resize cycle
    /// to determine WHERE duplication enters the server VT.
    #[test]
    fn test_diag_resize_cycle_scrollback_trace() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 500);
        vt.update_viewport("web", 24, 80, ClientType::Web);
        vt.update_viewport("tui", 36, 100, ClientType::Terminal);

        for i in 0..100 {
            vt.process_output(format!("Init {:04}\r\n", i).as_bytes());
        }

        // Step 0: baseline
        let (sb0, vis0) = server_all_content(&mut vt);
        let sb0_nonempty: Vec<_> = sb0.iter().filter(|l| !l.is_empty()).collect();
        eprintln!("=== STEP 0: after initial output ===");
        eprintln!("  server dims: {:?}", vt.effective_dims());
        eprintln!("  scrollback depth: {}", sb0_nonempty.len());
        eprintln!(
            "  scrollback[..5]: {:?}",
            &sb0_nonempty[..sb0_nonempty.len().min(5)]
        );
        eprintln!(
            "  scrollback[-5..]: {:?}",
            &sb0_nonempty[sb0_nonempty.len().saturating_sub(5)..]
        );
        let vis0_ne: Vec<_> = vis0.iter().filter(|l| !l.is_empty()).collect();
        eprintln!("  visible non-empty: {}", vis0_ne.len());
        eprintln!("  visible[..3]: {:?}", &vis0_ne[..vis0_ne.len().min(3)]);

        // Check for Init dups in server baseline
        let mut baseline_dups = Vec::new();
        for i in 0..100 {
            let needle = format!("Init {:04}", i);
            let n = count_in_all(&sb0, &vis0, &needle);
            if n > 1 {
                baseline_dups.push((needle, n));
            }
        }
        eprintln!(
            "  baseline dups: {:?}",
            &baseline_dups[..baseline_dups.len().min(5)]
        );

        // Step 0.5: "Active" output (like stress test cycle 0)
        vt.process_output(b"Active 0000\r\n");

        // Step 1: web hidden → TUI-only → dims change to 36×100
        vt.set_active("web", false);
        // Manual recalc shows new dims
        let dims_after_hide = vt.effective_dims();
        eprintln!("\n=== STEP 1: web hidden (before SIGWINCH) ===");
        eprintln!("  server dims: {:?}", dims_after_hide);
        let (sb1, vis1) = server_all_content(&mut vt);
        let sb1_ne: Vec<_> = sb1.iter().filter(|l| !l.is_empty()).collect();
        eprintln!("  scrollback depth: {}", sb1_ne.len());
        eprintln!("  scrollback[..5]: {:?}", &sb1_ne[..sb1_ne.len().min(5)]);
        eprintln!(
            "  scrollback[-5..]: {:?}",
            &sb1_ne[sb1_ne.len().saturating_sub(5)..]
        );
        let vis1_ne: Vec<_> = vis1.iter().filter(|l| !l.is_empty()).collect();
        eprintln!("  visible non-empty: {}", vis1_ne.len());

        // Check Init dups after resize
        let mut step1_dups = Vec::new();
        for i in 0..100 {
            let needle = format!("Init {:04}", i);
            let n = count_in_all(&sb1, &vis1, &needle);
            if n > 1 {
                step1_dups.push((needle, n));
            }
        }
        eprintln!("  step1 dups: {:?}", &step1_dups[..step1_dups.len().min(5)]);

        // Step 2: SIGWINCH at new dims (36×100)
        simulate_sigwinch_redraw(&mut vt, "SIGWINCH-hidden");
        eprintln!(
            "\n=== STEP 2: after SIGWINCH at {:?} ===",
            vt.effective_dims()
        );
        let (sb2, vis2) = server_all_content(&mut vt);
        let sb2_ne: Vec<_> = sb2.iter().filter(|l| !l.is_empty()).collect();
        eprintln!("  scrollback depth: {}", sb2_ne.len());
        eprintln!("  scrollback[..5]: {:?}", &sb2_ne[..sb2_ne.len().min(5)]);
        eprintln!(
            "  scrollback[-5..]: {:?}",
            &sb2_ne[sb2_ne.len().saturating_sub(5)..]
        );

        let mut step2_dups = Vec::new();
        for i in 0..100 {
            let needle = format!("Init {:04}", i);
            let n = count_in_all(&sb2, &vis2, &needle);
            if n > 1 {
                step2_dups.push((needle, n));
            }
        }
        eprintln!("  step2 dups: {:?}", &step2_dups[..step2_dups.len().min(5)]);

        // Step 2.5: "Background" output (like stress test cycle 0)
        vt.process_output(b"Background 0000\r\n");

        // Step 3: web visible at 30×80 (stress test uses rows=30 on cycle 0)
        // This matches: simulate_terminal_visible("web", 30, 80, Web)
        if let Some((new_rows, new_cols)) = vt.update_viewport("web", 30, 80, ClientType::Web) {
            eprintln!(
                "\n=== STEP 3: web visible → resize to {}x{} ===",
                new_rows, new_cols
            );
            vt.resize(new_rows, new_cols);
        }
        let dims_after_show = vt.effective_dims();
        eprintln!("  server dims after show: {:?}", dims_after_show);
        let (sb3, vis3) = server_all_content(&mut vt);
        let sb3_ne: Vec<_> = sb3.iter().filter(|l| !l.is_empty()).collect();
        eprintln!("  scrollback depth: {}", sb3_ne.len());
        let mut step3_dups = Vec::new();
        for i in 0..100 {
            let needle = format!("Init {:04}", i);
            let n = count_in_all(&sb3, &vis3, &needle);
            if n > 1 {
                step3_dups.push((needle, n));
            }
        }
        eprintln!("  step3 dups: {:?}", &step3_dups[..step3_dups.len().min(5)]);

        // Step 4: SIGWINCH at new dims (30×80)
        simulate_sigwinch_redraw(&mut vt, "SIGWINCH-show");
        eprintln!(
            "\n=== STEP 4: after SIGWINCH at {:?} ===",
            vt.effective_dims()
        );
        let (sb4, vis4) = server_all_content(&mut vt);
        let sb4_ne: Vec<_> = sb4.iter().filter(|l| !l.is_empty()).collect();
        eprintln!("  scrollback depth: {}", sb4_ne.len());
        let mut step4_dups = Vec::new();
        for i in 0..100 {
            let needle = format!("Init {:04}", i);
            let n = count_in_all(&sb4, &vis4, &needle);
            if n > 1 {
                step4_dups.push((needle, n));
            }
        }
        eprintln!("  step4 dups: {:?}", &step4_dups[..step4_dups.len().min(5)]);

        // Step 5: generate replay with client_rows=30 (matching stress test)
        let replay = vt.replay(30);
        let mut client = vt100::Parser::new(30, 80, 1000);
        client.process(&replay);
        let (csb, cvis) = client_all_content(&mut client);
        let csb_ne: Vec<_> = csb.iter().filter(|l| !l.is_empty()).collect();
        eprintln!("\n=== STEP 5: client after replay ===");
        eprintln!("  client scrollback depth: {}", csb_ne.len());
        eprintln!(
            "  client scrollback[..5]: {:?}",
            &csb_ne[..csb_ne.len().min(5)]
        );
        eprintln!(
            "  client scrollback[-5..]: {:?}",
            &csb_ne[csb_ne.len().saturating_sub(5)..]
        );
        let mut client_dups = Vec::new();
        for i in 0..100 {
            let needle = format!("Init {:04}", i);
            let n = count_in_all(&csb, &cvis, &needle);
            if n > 1 {
                client_dups.push((needle, n));
            }
        }
        eprintln!(
            "  client dups: {:?}",
            &client_dups[..client_dups.len().min(10)]
        );

        // Final assertions
        assert!(
            step4_dups.is_empty(),
            "SERVER has Init dups after one resize cycle: {:?}",
            &step4_dups[..step4_dups.len().min(10)]
        );
        assert!(
            client_dups.is_empty(),
            "CLIENT has Init dups after replay: {:?}",
            &client_dups[..client_dups.len().min(10)]
        );
    }

    // ── Broadcast overlap / attach-cycle tests ────────────────────────

    /// Simulate the broadcast overlap scenario: PTY output that arrives
    /// between subscribe_output and get_recent_output is included in BOTH
    /// the replay (VT state) and the broadcast queue.  The client therefore
    /// processes the same bytes twice — once baked into the replay, once
    /// as live output.  If that output causes scrolling, visible content
    /// from the keyframe is pushed into the client's scrollback, creating
    /// "copied chunks" that shouldn't be there.
    #[test]
    fn test_broadcast_overlap_causes_scrollback_duplication() {
        let mut vt = VirtualTerminal::new(4, 40, 4096, 100);

        // Build up history
        for i in 0..16 {
            vt.process_output(format!("Line {:02}\r\n", i).as_bytes());
        }

        // "Late output" — arrives between subscribe and replay generation.
        // Represents Claude actively producing output while the client attaches.
        // Use more lines than screen height so some end up in the server's
        // scrollback (not just on the visible screen).
        let late_output = b"Late 00\r\nLate 01\r\nLate 02\r\nLate 03\r\nLate 04\r\nLate 05\r\n";
        vt.process_output(late_output);

        // Server state: scrollback has Lines 00-15 + Late 00-02,
        // visible = [Late 03, Late 04, Late 05, ""]
        let replay = vt.replay(4);

        // ── Clean client: processes ONLY the replay (post-drain scenario) ──
        let mut client_clean = vt100::Parser::new(4, 40, 1000);
        client_clean.process(&replay);
        let sb_clean = scrollback_lines(&mut client_clean, 40);
        let clean_content: Vec<_> = sb_clean.iter().filter(|l| !l.is_empty()).collect();

        // ── Buggy client: processes replay THEN the duplicate late output ──
        let mut client_buggy = vt100::Parser::new(4, 40, 1000);
        client_buggy.process(&replay);
        client_buggy.process(late_output); // broadcast overlap
        let sb_buggy = scrollback_lines(&mut client_buggy, 40);
        let buggy_content: Vec<_> = sb_buggy.iter().filter(|l| !l.is_empty()).collect();

        // Clean client: "Late 00" appears exactly once in scrollback
        // (it's in the server's scrollback, included in the replay)
        let clean_count = clean_content
            .iter()
            .filter(|l| l.contains("Late 00"))
            .count();
        assert_eq!(
            clean_count, 1,
            "clean client: 'Late 00' once in scrollback, got {}.\n  scrollback: {:?}",
            clean_count, clean_content
        );

        // Buggy client: "Late 00" appears TWICE — once from the replay's
        // scrollback section, once pushed in when the duplicate output
        // scrolls the keyframe's visible content off screen.
        let buggy_count = buggy_content
            .iter()
            .filter(|l| l.contains("Late 00"))
            .count();
        assert!(
            buggy_count > 1,
            "buggy client should show duplicate 'Late 00' (broadcast overlap), \
             got count={}.\n  scrollback: {:?}",
            buggy_count,
            buggy_content
        );

        // The buggy client has more scrollback entries than the clean one
        assert!(
            buggy_content.len() > clean_content.len(),
            "buggy client should have more scrollback ({}) than clean ({})",
            buggy_content.len(),
            clean_content.len()
        );
    }

    /// After a detach/reattach cycle that changes effective dimensions
    /// (e.g. TUI disconnects, web-only dims are larger, TUI reconnects),
    /// the scrollback should still contain correct, non-duplicated content.
    #[test]
    fn test_replay_after_resize_cycle_no_duplicates() {
        let mut vt = VirtualTerminal::new(4, 40, 4096, 100);

        for i in 0..20 {
            vt.process_output(format!("Line {:02}\r\n", i).as_bytes());
        }

        // TUI connects at 4×40
        vt.update_viewport("tui", 4, 40, ClientType::Terminal);

        // TUI detaches → web-only → dims grow
        vt.remove_client("tui");
        vt.update_viewport("web", 24, 80, ClientType::Web);

        // Some output at the larger size
        for i in 20..25 {
            vt.process_output(format!("Line {:02}\r\n", i).as_bytes());
        }

        // TUI reattaches → dims shrink back
        vt.update_viewport("tui2", 4, 40, ClientType::Terminal);

        // Generate replay for TUI
        let mut client = replay_roundtrip(&mut vt, 4, 40);
        let sb = scrollback_lines(&mut client, 40);
        let content: Vec<_> = sb.iter().filter(|l| !l.is_empty()).collect();

        // Every numbered line should appear at most once
        for i in 0..25 {
            let needle = format!("Line {:02}", i);
            let count = content.iter().filter(|l| l.contains(&needle)).count();
            assert!(
                count <= 1,
                "'{}' appears {} times in scrollback (expected 0 or 1).\n  scrollback: {:?}",
                needle,
                count,
                content
            );
        }

        // Lines should be in ascending order
        let mut prev: Option<u32> = None;
        for line in &content {
            if let Some(pos) = line.find("Line ")
                && let Ok(num) = line[pos + 5..pos + 7].trim().parse::<u32>()
            {
                if let Some(p) = prev {
                    assert!(num > p, "order broken: {} then {}", p, num);
                }
                prev = Some(num);
            }
        }
    }

    /// Repeated replay() calls on the same VT should be idempotent —
    /// each replay should produce identical output (no accumulation).
    #[test]
    fn test_repeated_replay_no_accumulation() {
        let mut vt = VirtualTerminal::new(4, 40, 4096, 100);

        for i in 0..20 {
            vt.process_output(format!("Line {:02}\r\n", i).as_bytes());
        }

        let replay1 = vt.replay(4);
        let replay2 = vt.replay(4);
        let replay3 = vt.replay(4);

        // All three replays should be byte-identical
        assert_eq!(replay1, replay2, "replay 1 and 2 should be identical");
        assert_eq!(replay2, replay3, "replay 2 and 3 should be identical");
    }

    /// Simulate rapid TUI detach/reattach with a SIGWINCH-triggered
    /// screen redraw in between.  The redraw output (which uses \x1b[H\x1b[2J)
    /// should NOT cause duplication because ED2 doesn't scroll.
    #[test]
    fn test_sigwinch_redraw_no_scrollback_pollution() {
        let mut vt = VirtualTerminal::new(4, 40, 4096, 100);

        for i in 0..20 {
            vt.process_output(format!("Line {:02}\r\n", i).as_bytes());
        }

        // Simulate a clean SIGWINCH response (home + clear + redraw)
        let sigwinch_response = b"\x1b[H\x1b[2JLine 17\r\nLine 18\r\nLine 19";
        vt.process_output(sigwinch_response);

        // Generate replay
        let replay = vt.replay(4);

        // Client processes replay then gets the same SIGWINCH response from broadcast
        let mut client = vt100::Parser::new(4, 40, 1000);
        client.process(&replay);
        client.process(sigwinch_response); // duplicate from broadcast

        // The visible screen should be correct
        let vis = visible_lines(client.screen());
        assert_eq!(vis[0], "Line 17");
        assert_eq!(vis[1], "Line 18");
        assert_eq!(vis[2], "Line 19");

        // Check scrollback for duplicates
        let sb = scrollback_lines(&mut client, 40);
        let content: Vec<_> = sb.iter().filter(|l| !l.is_empty()).collect();

        // "Line 17" should appear at most once in scrollback
        // (it's on the visible screen, not in scrollback)
        let line_17 = content.iter().filter(|l| l.contains("Line 17")).count();
        assert!(
            line_17 <= 1,
            "clean SIGWINCH redraw should not duplicate 'Line 17' in scrollback, \
             got count={}.\n  scrollback: {:?}",
            line_17,
            content
        );
    }

    /// Simulate a "dirty" SIGWINCH response that scrolls before clearing.
    /// Some programs output newlines to push old content into scrollback
    /// before redrawing.  If this output is in both the replay and the
    /// broadcast, the client gets double-scrolled content.
    #[test]
    fn test_dirty_sigwinch_broadcast_overlap() {
        let mut vt = VirtualTerminal::new(4, 40, 4096, 100);

        for i in 0..16 {
            vt.process_output(format!("Line {:02}\r\n", i).as_bytes());
        }

        // Dirty SIGWINCH: program scrolls down, then clears and redraws
        let dirty_response = b"\x1b[4;1H\n\n\n\n\x1b[H\x1b[2JVisible A\r\nVisible B\r\nVisible C";
        vt.process_output(dirty_response);

        let replay = vt.replay(4);

        // Clean client (no overlap)
        let mut clean = vt100::Parser::new(4, 40, 1000);
        clean.process(&replay);
        let sb_clean = scrollback_lines(&mut clean, 40);
        let clean_content: Vec<_> = sb_clean.iter().filter(|l| !l.is_empty()).collect();

        // Buggy client (replay + duplicate dirty response)
        let mut buggy = vt100::Parser::new(4, 40, 1000);
        buggy.process(&replay);
        buggy.process(dirty_response);
        let sb_buggy = scrollback_lines(&mut buggy, 40);
        let buggy_content: Vec<_> = sb_buggy.iter().filter(|l| !l.is_empty()).collect();

        // Document the difference
        let clean_visible_a = clean_content
            .iter()
            .filter(|l| l.contains("Visible A"))
            .count();
        let buggy_visible_a = buggy_content
            .iter()
            .filter(|l| l.contains("Visible A"))
            .count();

        // Clean should have 0 or 1 occurrences of "Visible A" in scrollback
        assert!(
            clean_visible_a <= 1,
            "clean: 'Visible A' in scrollback {} times.\n  {:?}",
            clean_visible_a,
            clean_content
        );

        // Buggy may have more due to the dirty response pushing content around
        // This documents the broadcast overlap effect
        eprintln!(
            "Dirty SIGWINCH overlap test:\n  clean scrollback 'Visible A' count: {}\n  buggy scrollback 'Visible A' count: {}\n  clean scrollback len: {}\n  buggy scrollback len: {}",
            clean_visible_a,
            buggy_visible_a,
            clean_content.len(),
            buggy_content.len()
        );
    }

    /// Simulate repeated web terminal visible/hidden cycles.
    /// Each cycle: set_active(false) → dims may change → SIGWINCH redraw →
    ///             set_active(true) → dims change back → SIGWINCH redraw.
    /// The SERVER VT's scrollback must not accumulate duplicates — because
    /// that would persist across browser refreshes.
    #[test]
    fn test_view_switch_cycle_no_server_scrollback_growth() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 500);

        // Initial output: fill up visible + scrollback
        for i in 0..40 {
            vt.process_output(format!("Line {:03}\r\n", i).as_bytes());
        }

        // Register both web and TUI clients (different dims trigger resize)
        vt.update_viewport("web", 24, 80, ClientType::Web);
        vt.update_viewport("tui", 40, 120, ClientType::Terminal);
        // Effective dims = min(24,40) x min(80,120) = 24x80

        // Take a baseline of the server scrollback
        let baseline_replay = vt.replay(24);
        let mut baseline_client = vt100::Parser::new(24, 80, 1000);
        baseline_client.process(&baseline_replay);
        let baseline_sb = scrollback_lines(&mut baseline_client, 80);
        let baseline_count = baseline_sb.iter().filter(|l| !l.is_empty()).count();

        // Simulate 5 web terminal visible/hidden cycles
        for _cycle in 0..5 {
            // Web terminal hidden → dims change to TUI-only (40x120)
            if let Some((rows, cols)) = vt.set_active("web", false) {
                vt.resize(rows, cols);
                // Simulate PTY SIGWINCH response at new dims
                let mut sigwinch = Vec::new();
                sigwinch.extend_from_slice(b"\x1b[H\x1b[2J");
                for r in 0..rows {
                    sigwinch.extend_from_slice(
                        format!("Redraw {:03} at {}x{}\r\n", r, rows, cols).as_bytes(),
                    );
                }
                vt.process_output(&sigwinch);
            }

            // Web terminal visible again → dims back to 24x80
            if let Some((rows, cols)) = vt.set_active("web", true) {
                vt.resize(rows, cols);
                // Simulate PTY SIGWINCH response at restored dims
                let mut sigwinch = Vec::new();
                sigwinch.extend_from_slice(b"\x1b[H\x1b[2J");
                for r in 0..rows {
                    sigwinch.extend_from_slice(
                        format!("Redraw {:03} at {}x{}\r\n", r, rows, cols).as_bytes(),
                    );
                }
                vt.process_output(&sigwinch);
            }
        }

        // Check server VT scrollback after cycles
        let post_replay = vt.replay(24);
        let mut post_client = vt100::Parser::new(24, 80, 1000);
        post_client.process(&post_replay);
        let post_sb = scrollback_lines(&mut post_client, 80);
        let post_count = post_sb.iter().filter(|l| !l.is_empty()).count();

        // The scrollback may grow (SIGWINCH redraws add content), but check
        // for DUPLICATE lines — no original "Line NNN" should appear twice.
        for i in 0..40 {
            let needle = format!("Line {:03}", i);
            let count = post_sb.iter().filter(|l| l.contains(&needle)).count();
            assert!(
                count <= 1,
                "After 5 view-switch cycles, '{}' appears {} times in replay scrollback \
                 (expected 0 or 1).\n  scrollback ({} lines): {:?}",
                needle,
                count,
                post_count,
                &post_sb[..post_sb.len().min(20)]
            );
        }

        eprintln!(
            "View-switch cycle test: baseline scrollback={}, post-cycle scrollback={}",
            baseline_count, post_count
        );
    }

    /// Regression test: sending two replays with different client_rows to the
    /// same terminal causes duplicate scrollback — ED2 clears the visible
    /// screen but NOT the scrollback buffer, so the second replay's scrollback
    /// section writes on top of the first's.
    ///
    /// The fix is to send only ONE replay (from TerminalVisible with correct
    /// dimensions), not two (Focus + TerminalVisible).
    #[test]
    fn test_double_replay_causes_duplicate_scrollback() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 500);

        for i in 0..60 {
            vt.process_output(format!("Line {:03}\r\n", i).as_bytes());
        }

        // Single replay (correct rows) — the only replay we should send.
        let replay = vt.replay(30);
        let mut single = vt100::Parser::new(30, 80, 1000);
        single.process(&replay);
        let sb_single = scrollback_lines(&mut single, 80);
        let single_content: Vec<_> = sb_single.iter().filter(|l| !l.is_empty()).collect();

        // Double replay (first with wrong rows, then correct) — the old bug.
        // ED2 between them does NOT clear scrollback.
        let replay1 = vt.replay(24);
        let replay2 = vt.replay(30);
        let mut double = vt100::Parser::new(30, 80, 1000);
        double.process(&replay1);
        double.process(b"\x1b[H\x1b[2J"); // clear visible only
        double.process(&replay2);
        let sb_double = scrollback_lines(&mut double, 80);
        let double_content: Vec<_> = sb_double.iter().filter(|l| !l.is_empty()).collect();

        // Single replay: no duplicates
        for i in 0..60 {
            let needle = format!("Line {:03}", i);
            let count = single_content
                .iter()
                .filter(|l| l.contains(&needle))
                .count();
            assert!(
                count <= 1,
                "single replay: '{}' appears {} times",
                needle,
                count
            );
        }

        // Double replay: duplicates exist (this documents why we removed the
        // preliminary Focus replay).
        let dup_count = (0..60)
            .filter(|i| {
                let needle = format!("Line {:03}", i);
                double_content
                    .iter()
                    .filter(|l| l.contains(&needle))
                    .count()
                    > 1
            })
            .count();
        assert!(
            dup_count > 0,
            "double replay should produce duplicates (documenting the bug)"
        );
    }

    /// Same as above but with a single web client (no TUI).
    /// set_active(false) with no other active viewports keeps current dims,
    /// so no resize should happen. Scrollback must not change at all.
    #[test]
    fn test_view_switch_single_client_no_scrollback_change() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 500);

        for i in 0..40 {
            vt.process_output(format!("Line {:03}\r\n", i).as_bytes());
        }

        vt.update_viewport("web", 24, 80, ClientType::Web);

        let replay_before = vt.replay(24);

        // 5 cycles of hide/show — single client, so no dim change
        for _ in 0..5 {
            let changed = vt.set_active("web", false);
            assert!(
                changed.is_none(),
                "single client hide should not change dims"
            );

            let changed = vt.set_active("web", true);
            assert!(
                changed.is_none(),
                "single client show should not change dims"
            );
        }

        let replay_after = vt.replay(24);
        assert_eq!(
            replay_before, replay_after,
            "single-client view-switch cycles should not alter the replay"
        );
    }

    // ── Resize race diagnostic tests ────────────────────────────────
    //
    // Between UpdateViewport (set_size on VT) and Resize (SIGWINCH to
    // child), PTY output arrives formatted for the OLD dimensions but
    // is processed by a VT at the NEW dimensions.  These tests verify
    // whether that transient mismatch corrupts scrollback.

    /// Helper: generate CUP-positioned output that a shell/TUI would
    /// produce at the given dimensions.  Each row gets `\x1b[R;1H` + text.
    fn cup_output(rows: u16, cols: u16, label: &str) -> Vec<u8> {
        let mut out = Vec::new();
        for r in 0..rows {
            out.extend_from_slice(format!("\x1b[{};1H", r + 1).as_bytes());
            let text = format!("{} r{:02} {}x{}", label, r, rows, cols);
            out.extend_from_slice(text.as_bytes());
            // Pad remaining cols with spaces to simulate full-width writes
            let remaining = cols as usize - text.len().min(cols as usize);
            out.extend(std::iter::repeat_n(b' ', remaining));
        }
        out
    }

    /// Simulate the resize race: VT resized to new dims, then output
    /// at OLD dims arrives before SIGWINCH.  Check scrollback integrity.
    #[test]
    fn test_resize_race_output_at_old_dims() {
        let old_rows: u16 = 24;
        let old_cols: u16 = 80;
        let new_rows: u16 = 52;
        let new_cols: u16 = 193;

        let mut vt = VirtualTerminal::new(old_rows, old_cols, 4096, 500);

        // Phase 1: initial output at old dims — fills scrollback
        for i in 0..60 {
            vt.process_output(format!("Init {:03}\r\n", i).as_bytes());
        }

        // Capture scrollback baseline
        let (sb_before, vis_before) = server_all_content(&mut vt);
        let init_count_before = (0..60)
            .filter(|i| {
                let needle = format!("Init {:03}", i);
                count_in_all(&sb_before, &vis_before, &needle) == 1
            })
            .count();
        assert_eq!(
            init_count_before, 60,
            "all Init lines should be present once before resize"
        );

        // Phase 2: web client connects — VT resizes to new dims
        // (simulates UpdateViewport actor command)
        vt.update_viewport("web", new_rows, new_cols, ClientType::Web);

        // Phase 3: output at OLD dims arrives BEFORE SIGWINCH
        // (PTY reader processes output during the race window)
        // This simulates a few lines scrolling at the bottom of a 24-row screen
        // being processed by a VT that's now 52 rows.
        for i in 0..5 {
            vt.process_output(format!("Race {:02}\r\n", i).as_bytes());
        }

        // Phase 4: SIGWINCH arrives — child redraws at new dims
        let mut redraw = Vec::new();
        redraw.extend_from_slice(b"\x1b[H\x1b[2J");
        for r in 0..new_rows {
            redraw.extend_from_slice(
                format!("New r{:02} {}x{}\r\n", r, new_rows, new_cols).as_bytes(),
            );
        }
        vt.process_output(&redraw);

        // Phase 5: check server VT for Init duplicates
        let (sb_after, vis_after) = server_all_content(&mut vt);
        let mut init_dups = Vec::new();
        for i in 0..60 {
            let needle = format!("Init {:03}", i);
            let n = count_in_all(&sb_after, &vis_after, &needle);
            if n > 1 {
                init_dups.push((needle, n));
            }
        }

        // Phase 6: replay to client and check
        let replay = vt.replay(new_rows);
        let mut client = vt100::Parser::new(new_rows, new_cols, 1000);
        client.process(&replay);
        let (csb, cvis) = client_all_content(&mut client);
        let mut client_dups = Vec::new();
        for i in 0..60 {
            let needle = format!("Init {:03}", i);
            let n = count_in_all(&csb, &cvis, &needle);
            if n > 1 {
                client_dups.push((needle, n));
            }
        }

        if !init_dups.is_empty() || !client_dups.is_empty() {
            eprintln!("=== RESIZE RACE DIAGNOSTIC ===");
            eprintln!("server dups: {:?}", &init_dups[..init_dups.len().min(5)]);
            eprintln!(
                "client dups: {:?}",
                &client_dups[..client_dups.len().min(5)]
            );
            let sb_ne: Vec<_> = sb_after.iter().filter(|l| !l.is_empty()).collect();
            eprintln!(
                "server scrollback[..10]: {:?}",
                &sb_ne[..sb_ne.len().min(10)]
            );
        }

        assert!(
            init_dups.is_empty(),
            "resize race: server has {} Init dups: {:?}",
            init_dups.len(),
            &init_dups[..init_dups.len().min(10)]
        );
        assert!(
            client_dups.is_empty(),
            "resize race: client has {} Init dups: {:?}",
            client_dups.len(),
            &client_dups[..client_dups.len().min(10)]
        );
    }

    /// Same resize race but with CUP-positioned content (full-screen TUI).
    #[test]
    fn test_resize_race_cup_content() {
        let old_rows: u16 = 24;
        let old_cols: u16 = 80;

        let mut vt = VirtualTerminal::new(old_rows, old_cols, 4096, 500);

        // Fill with scrolling content, then a CUP-positioned redraw
        for i in 0..40 {
            vt.process_output(format!("Scroll {:03}\r\n", i).as_bytes());
        }
        // Full-screen CUP redraw at 24×80
        vt.process_output(&cup_output(old_rows, old_cols, "CUP"));

        // Web client with different dims
        vt.update_viewport("web", 52, 193, ClientType::Web);

        // CUP output at OLD dims during race window
        vt.process_output(&cup_output(old_rows, old_cols, "RACE"));

        // SIGWINCH redraw at new dims
        let mut redraw = Vec::new();
        redraw.extend_from_slice(b"\x1b[H\x1b[2J");
        redraw.extend_from_slice(&cup_output(52, 193, "NEW"));
        vt.process_output(&redraw);

        // Replay should be valid
        let replay = vt.replay(52);
        let mut client = vt100::Parser::new(52, 193, 1000);
        client.process(&replay);
        let vis = visible_lines(client.screen());
        let vis_ne: Vec<_> = vis.iter().filter(|l| !l.is_empty()).collect();

        // Visible screen should show "NEW" content, not "RACE" or "CUP"
        for line in &vis_ne {
            assert!(
                !line.contains("RACE") && !line.contains("CUP"),
                "visible screen should only contain NEW content after SIGWINCH, got: {:?}",
                line
            );
        }
    }

    /// Test that from_utf8_lossy round-trip doesn't corrupt replay bytes.
    /// This simulates the server's GetRecentOutput conversion.
    #[test]
    fn test_replay_utf8_lossy_roundtrip() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 500);
        // Content with UTF-8 characters
        for i in 0..50 {
            vt.process_output(format!("├── Line {:03} ─────┤\r\n", i).as_bytes());
        }

        let replay = vt.replay(24);

        // Simulate server's conversion: bytes → String via from_utf8_lossy → bytes
        let as_string = String::from_utf8_lossy(&replay).to_string();
        let roundtripped = as_string.as_bytes();

        // Bytes should be identical (no lossy replacement)
        assert_eq!(
            replay.len(),
            roundtripped.len(),
            "from_utf8_lossy changed replay byte length: {} → {}",
            replay.len(),
            roundtripped.len()
        );
        assert_eq!(
            &replay, roundtripped,
            "from_utf8_lossy modified replay bytes"
        );

        // Client should see correct content through either path
        let mut client_direct = vt100::Parser::new(24, 80, 1000);
        client_direct.process(&replay);
        let vis_direct = visible_lines(client_direct.screen());

        let mut client_lossy = vt100::Parser::new(24, 80, 1000);
        client_lossy.process(roundtripped);
        let vis_lossy = visible_lines(client_lossy.screen());

        assert_eq!(
            vis_direct, vis_lossy,
            "visible lines differ after from_utf8_lossy round-trip"
        );
    }

    /// Cell-by-cell comparison: server VT visible screen vs client after replay.
    /// If these differ, the replay format has a rendering bug.
    #[test]
    fn test_replay_cell_by_cell_server_vs_client() {
        let mut vt = VirtualTerminal::new(24, 80, 4096, 500);

        // Complex content: SGR colors, CUP positioning, scrolling
        for i in 0..30 {
            vt.process_output(format!("\x1b[32mGreen {:03}\x1b[0m\r\n", i).as_bytes());
        }
        // CUP-positioned status line
        vt.process_output(b"\x1b[24;1H\x1b[7mStatus: OK\x1b[0m");

        let replay = vt.replay(24);
        let mut client = vt100::Parser::new(24, 80, 1000);
        client.process(&replay);

        // Compare each cell on the visible screen
        let (rows, cols) = vt.screen().size();
        let mut mismatches = Vec::new();
        for r in 0..rows {
            for c in 0..cols {
                let server_cell = vt.screen().cell(r, c);
                let client_cell = client.screen().cell(r, c);
                match (server_cell, client_cell) {
                    (Some(sc), Some(cc)) => {
                        if sc.contents() != cc.contents() {
                            mismatches.push(format!(
                                "({},{}) content: server={:?} client={:?}",
                                r,
                                c,
                                sc.contents(),
                                cc.contents()
                            ));
                        }
                        if sc.fgcolor() != cc.fgcolor() {
                            mismatches.push(format!(
                                "({},{}) fgcolor: server={:?} client={:?}",
                                r,
                                c,
                                sc.fgcolor(),
                                cc.fgcolor()
                            ));
                        }
                    }
                    (None, Some(_)) | (Some(_), None) => {
                        mismatches.push(format!(
                            "({},{}) cell existence mismatch: server={} client={}",
                            r,
                            c,
                            server_cell.is_some(),
                            client_cell.is_some()
                        ));
                    }
                    (None, None) => {}
                }
            }
        }

        if !mismatches.is_empty() {
            eprintln!("=== CELL MISMATCH DIAGNOSTIC ===");
            for m in &mismatches[..mismatches.len().min(20)] {
                eprintln!("  {}", m);
            }
        }

        assert!(
            mismatches.is_empty(),
            "found {} cell mismatches between server VT and client replay.\n  \
             first 5: {:?}",
            mismatches.len(),
            &mismatches[..mismatches.len().min(5)]
        );
    }

    /// Test set_size row-shrink behavior: do excess rows go to scrollback?
    /// This is critical for understanding corruption from resize cycles.
    #[test]
    fn test_set_size_shrink_scrollback_behavior() {
        let mut parser = vt100::Parser::new(52, 193, 500);

        // Fill all 52 rows with content
        for r in 0..52 {
            parser.process(format!("\x1b[{};1HRow {:02} at 52x193", r + 1, r).as_bytes());
        }

        // Verify content before shrink
        let before_vis = visible_lines(parser.screen());
        let before_ne: Vec<_> = before_vis.iter().filter(|l| !l.is_empty()).collect();
        assert_eq!(before_ne.len(), 52, "should have 52 non-empty rows");

        // Check scrollback before shrink
        parser.screen_mut().set_scrollback(usize::MAX);
        let sb_before = parser.screen().scrollback();
        parser.screen_mut().set_scrollback(0);

        // Shrink from 52 to 24 rows
        parser.screen_mut().set_size(24, 80);

        let after_vis = visible_lines(parser.screen());
        let after_ne: Vec<_> = after_vis.iter().filter(|l| !l.is_empty()).collect();

        // Check scrollback after shrink
        parser.screen_mut().set_scrollback(usize::MAX);
        let sb_after = parser.screen().scrollback();
        parser.screen_mut().set_scrollback(0);

        eprintln!("=== SET_SIZE SHRINK BEHAVIOR ===");
        eprintln!("Before: 52 rows, scrollback={}", sb_before);
        eprintln!(
            "After: 24 rows, scrollback={}, visible non-empty={}",
            sb_after,
            after_ne.len()
        );
        eprintln!("Visible[..5]: {:?}", &after_ne[..after_ne.len().min(5)]);

        if sb_after > sb_before {
            let sb_lines = scrollback_lines(&mut parser, 80);
            let sb_ne: Vec<_> = sb_lines.iter().filter(|l| !l.is_empty()).collect();
            eprintln!(
                "New scrollback content[..10]: {:?}",
                &sb_ne[..sb_ne.len().min(10)]
            );
        }

        // The key question: did rows 24-51 go to scrollback?
        // If yes, scrollback grew by ~28 rows.
        eprintln!(
            "Scrollback grew by: {} rows",
            sb_after as i64 - sb_before as i64
        );
    }

    /// Reproduce the EXACT user scenario with resize cycles and check
    /// that scrollback doesn't accumulate garbage across cycles.
    #[test]
    fn test_resize_cycle_scrollback_accumulation() {
        // Start at typical TUI-like dims
        let mut vt = VirtualTerminal::new(24, 80, 4096, 500);
        vt.update_viewport("web", 24, 80, ClientType::Web);

        // Fill with content
        for i in 0..60 {
            vt.process_output(format!("Line {:03}\r\n", i).as_bytes());
        }

        // Baseline: count scrollback lines
        vt.parser.screen_mut().set_scrollback(usize::MAX);
        let baseline_sb = vt.parser.screen().scrollback();
        vt.parser.screen_mut().set_scrollback(0);
        let _baseline_replay = vt.replay(24);

        // TUI connects with larger dims
        vt.update_viewport("tui", 52, 193, ClientType::Terminal);
        // Effective = min(24, 52) x min(80, 193) = 24x80

        // Now simulate 5 web close/open cycles (TUI stays connected)
        for cycle in 0..5 {
            // Web closes → effective dims become TUI-only (52x193)
            if let Some((r, c)) = vt.set_active("web", false) {
                vt.resize(r, c);
                // SIGWINCH redraw at larger dims
                let mut redraw = Vec::new();
                redraw.extend_from_slice(b"\x1b[H\x1b[2J");
                for row in 0..r {
                    redraw
                        .extend_from_slice(format!("Redraw c{} r{:02}\r\n", cycle, row).as_bytes());
                }
                vt.process_output(&redraw);
            }

            // Check scrollback growth
            vt.parser.screen_mut().set_scrollback(usize::MAX);
            let mid_sb = vt.parser.screen().scrollback();
            vt.parser.screen_mut().set_scrollback(0);

            // Web opens → effective dims shrink back to 24x80
            if let Some((r, c)) = vt.update_viewport("web", 24, 80, ClientType::Web) {
                vt.resize(r, c);
                // SIGWINCH redraw at smaller dims
                let mut redraw = Vec::new();
                redraw.extend_from_slice(b"\x1b[H\x1b[2J");
                for row in 0..r {
                    redraw.extend_from_slice(format!("Show c{} r{:02}\r\n", cycle, row).as_bytes());
                }
                vt.process_output(&redraw);
            }

            vt.parser.screen_mut().set_scrollback(usize::MAX);
            let end_sb = vt.parser.screen().scrollback();
            vt.parser.screen_mut().set_scrollback(0);

            eprintln!(
                "Cycle {}: mid_sb={} end_sb={} (baseline={})",
                cycle, mid_sb, end_sb, baseline_sb
            );
        }

        // After 5 cycles: check for Line duplicates
        let (sb_final, vis_final) = server_all_content(&mut vt);
        let mut dups = Vec::new();
        for i in 0..60 {
            let needle = format!("Line {:03}", i);
            let n = count_in_all(&sb_final, &vis_final, &needle);
            if n > 1 {
                dups.push((needle.clone(), n));
            }
        }

        if !dups.is_empty() {
            eprintln!("=== RESIZE CYCLE ACCUMULATION ===");
            eprintln!("Found {} duplicated Line entries", dups.len());
            eprintln!("Dups: {:?}", &dups[..dups.len().min(10)]);
        }

        // Replay to client and check
        let post_replay = vt.replay(24);
        let mut client = vt100::Parser::new(24, 80, 1000);
        client.process(&post_replay);
        let (csb, cvis) = client_all_content(&mut client);
        let mut client_dups = Vec::new();
        for i in 0..60 {
            let needle = format!("Line {:03}", i);
            let n = count_in_all(&csb, &cvis, &needle);
            if n > 1 {
                client_dups.push((needle, n));
            }
        }

        // This test is DIAGNOSTIC — we want to see the output even if it passes.
        // The actual assertion:
        assert!(
            dups.is_empty(),
            "server has {} Line dups after 5 resize cycles: {:?}",
            dups.len(),
            &dups[..dups.len().min(10)]
        );
        assert!(
            client_dups.is_empty(),
            "client has {} Line dups after 5 resize cycles: {:?}",
            client_dups.len(),
            &client_dups[..client_dups.len().min(10)]
        );
    }

    /// Edge case: no scrollback overflow → replay should not contain scroll-clear section.
    #[test]
    fn test_replay_empty_scrollback_no_scroll_clear() {
        let mut vt = VirtualTerminal::new(4, 40, 4096, 100);
        // Only 2 lines — no scrollback overflow
        vt.process_output(b"Line A\r\nLine B\r\n");

        let replay = vt.replay(4);
        let replay_str = String::from_utf8_lossy(&replay);

        // Should NOT contain the scroll-clear newline sequence that pushes
        // scrollback down — there's nothing to push.
        // The replay should start directly with the keyframe.
        assert!(
            replay_str.starts_with("\x1b[H\x1b[2J"),
            "replay with no scrollback should start with keyframe, got: {:?}",
            &replay_str[..replay_str.len().min(40)]
        );

        // Visible lines should be correct
        let mut client = vt100::Parser::new(4, 40, 100);
        client.process(&replay);
        let vis = visible_lines(client.screen());
        assert_eq!(vis[0], "Line A");
        assert_eq!(vis[1], "Line B");
    }

    /// Replay a real captured session (13 resize events) through the same
    /// path the TUI and web terminal use: `replay()` → client vt100 parser
    /// → read cells.
    #[test]
    fn test_resize_dedup_golden_recording() {
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/resize_dedup.vtr");
        let recording = VtRecording::from_file(&fixture).unwrap();
        let mut vt = recording.replay(64 * 1024);

        // Use the real client rendering path: replay() bytes → client parser
        let state = vt.debug_state();
        let (server_rows, server_cols) = state.screen_size;
        let mut client = vt100::Parser::new(server_rows, server_cols, 10_000);
        client.process(&vt.replay(server_rows));

        // Read all content exactly as the TUI/web terminal would see it
        let sb = scrollback_lines(&mut client, server_cols);
        let vis = visible_lines(client.screen());
        let all_lines: Vec<&str> = sb.iter().chain(vis.iter()).map(|s| s.as_str()).collect();

        // Should contain the Claude Code banner exactly once
        let banner_count = all_lines
            .iter()
            .filter(|l| l.contains("Claude Code"))
            .count();
        assert_eq!(banner_count, 1, "banner should appear exactly once");

        // Snapshot the full rendered output for regression detection
        insta::assert_snapshot!(all_lines.join("\n"));
    }
}
