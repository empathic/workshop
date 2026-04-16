//! Per-session scratch directory under `~/.meld/sessions/<id>/`.
//!
//! The session id is also exported to the PTY child as `MELD_SESSION_ID`, so
//! subprocesses can plug in and attribute actions to whoever currently holds
//! the edit turn by reading `active_user`.

use std::path::PathBuf;

fn dir(session_id: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home)
        .join(".meld")
        .join("sessions")
        .join(session_id)
}

/// Atomically publish the name of the user currently driving the session.
pub fn write_active_user(session_id: &str, name: &str) {
    let dir = dir(session_id);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("active_user");
    let tmp = dir.join("active_user.tmp");
    if std::fs::write(&tmp, name).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

pub fn cleanup_session(session_id: &str) {
    let _ = std::fs::remove_dir_all(dir(session_id));
}
