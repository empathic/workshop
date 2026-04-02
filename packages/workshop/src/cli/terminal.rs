use anyhow::Result;
use nix::libc;

/// Get the current terminal size (rows, cols).
#[cfg(unix)]
pub fn get_terminal_size() -> Result<(u16, u16)> {
    let mut ws = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let ret = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) };
    if ret == -1 {
        anyhow::bail!("ioctl TIOCGWINSZ failed");
    }
    Ok((ws.ws_row, ws.ws_col))
}
