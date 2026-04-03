use anyhow::{Context, Result};
use tokio_tungstenite::tungstenite;

use workshop::config::WorkshopConfig;

#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("server is unavailable")]
    Unavailable,

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl DaemonError {
    pub fn from_reqwest(err: reqwest::Error) -> Self {
        if err.is_connect() {
            Self::Unavailable
        } else {
            Self::Other(err.into())
        }
    }

    pub fn from_tungstenite(err: tungstenite::Error) -> Self {
        let is_connect = match &err {
            tungstenite::Error::Io(io_err) => matches!(
                io_err.kind(),
                std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
            ),
            _ => false,
        };
        if is_connect {
            Self::Unavailable
        } else {
            Self::Other(err.into())
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DaemonInfo {
    pub pid: u32,
    pub port: u16,
    pub host: String,
}

impl DaemonInfo {
    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    pub fn mux_ws_url(&self) -> String {
        format!("ws://{}:{}/api/ws", self.host, self.port)
    }
}

/// Check if a daemon is already running by reading PID/port files and verifying the process.
pub fn check_daemon(config: &WorkshopConfig) -> Option<DaemonInfo> {
    let pid_path = config.daemon_pid_path();
    let port_path = config.daemon_port_path();

    // Read PID file
    let pid_str = std::fs::read_to_string(&pid_path).ok()?;
    let pid: u32 = pid_str.trim().parse().ok()?;

    // Verify process is alive (kill with signal 0 = check existence)
    #[cfg(unix)]
    {
        use nix::sys::signal;
        use nix::unistd::Pid;
        if signal::kill(Pid::from_raw(pid as i32), None).is_err() {
            // Process is dead, clean up stale files
            let _ = std::fs::remove_file(&pid_path);
            let _ = std::fs::remove_file(&port_path);
            return None;
        }
    }

    // Read port file
    let port_str = std::fs::read_to_string(&port_path).ok()?;
    let port: u16 = port_str.trim().parse().ok()?;

    Some(DaemonInfo {
        pid,
        port,
        host: "127.0.0.1".to_string(),
    })
}

/// Start a new daemon process in the background.
pub fn start_daemon(config: &WorkshopConfig) -> Result<()> {
    let exe = std::env::current_exe().context("Failed to determine current executable")?;

    // Ensure log directory exists
    std::fs::create_dir_all(&config.logs_dir)?;

    let log_file = std::fs::File::create(config.daemon_log_path())
        .context("Failed to create daemon log file")?;
    let err_file = std::fs::File::create(config.daemon_err_path())
        .context("Failed to create daemon error log file")?;

    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("server")
        .arg("--port")
        .arg("0") // auto-select port
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log_file))
        .stderr(std::process::Stdio::from(err_file));

    // Pass data-dir if non-default
    let default_data_dir = dirs::home_dir()
        .expect("Could not find home directory")
        .join(".workshop");
    if config.data_dir != default_data_dir {
        cmd.arg("--data-dir").arg(&config.data_dir);
    }

    // On Unix, create a new session so the daemon doesn't die with the terminal
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                nix::libc::setsid();
                Ok(())
            });
        }
    }

    cmd.spawn().context("Failed to spawn daemon process")?;

    Ok(())
}

/// Require a daemon to already be running. Returns an error if none is found.
pub async fn require_running_daemon(config: &WorkshopConfig) -> Result<DaemonInfo> {
    if let Some(info) = check_daemon(config)
        && health_check(&info).await
    {
        return Ok(info);
    }
    anyhow::bail!("No work daemon is running. Start one with `work` or `work server`.")
}

/// Ensure a daemon is running. Start one if needed, then wait for it to be healthy.
pub async fn ensure_daemon(config: &WorkshopConfig) -> Result<DaemonInfo> {
    // First check if already running
    if let Some(info) = check_daemon(config) {
        // Verify it's actually healthy
        if health_check(&info).await {
            return Ok(info);
        }
        // Process exists but not healthy - clean up and restart
        let _ = std::fs::remove_file(config.daemon_pid_path());
        let _ = std::fs::remove_file(config.daemon_port_path());
    }

    eprintln!("Starting work daemon...");
    start_daemon(config)?;

    // Poll for the port file to appear (daemon writes it after binding)
    let port_path = config.daemon_port_path();
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(10);
    let poll_interval = std::time::Duration::from_millis(100);

    loop {
        if start.elapsed() > timeout {
            anyhow::bail!(
                "Timed out waiting for daemon to start. Check logs at: {}",
                config.daemon_err_path().display()
            );
        }

        tokio::time::sleep(poll_interval).await;

        if let Some(info) = check_daemon(config)
            && health_check(&info).await
        {
            eprintln!("Daemon running on port {}", info.port);
            return Ok(info);
        }

        // Also check if port file exists even if PID check fails (race condition)
        if port_path.exists()
            && let Some(info) = check_daemon(config)
            && health_check(&info).await
        {
            eprintln!("Daemon running on port {}", info.port);
            return Ok(info);
        }
    }
}

/// Try to rediscover a running daemon from PID/port files on disk.
/// Used when the current DaemonInfo is stale (e.g. server restarted on a new port).
pub async fn rediscover_daemon(config: &WorkshopConfig) -> Option<DaemonInfo> {
    let info = check_daemon(config)?;
    if health_check(&info).await {
        Some(info)
    } else {
        None
    }
}

/// Public health check (used by `cli::auth`).
pub async fn health_check_pub(info: &DaemonInfo) -> bool {
    health_check(info).await
}

/// Health check the daemon via GET /health.
async fn health_check(info: &DaemonInfo) -> bool {
    let url = format!("{}/health", info.base_url());
    match reqwest::get(&url).await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// Send SIGTERM to the daemon process for a graceful shutdown.
pub fn stop_daemon(info: &DaemonInfo) {
    #[cfg(unix)]
    {
        use nix::sys::signal::{self, Signal};
        use nix::unistd::Pid;
        let _ = signal::kill(Pid::from_raw(info.pid as i32), Signal::SIGTERM);
    }
}

// write_daemon_files / cleanup_daemon_files are now in workshop::server

#[cfg(test)]
mod tests {
    use super::*;
    use workshop::server::write_daemon_files;

    /// Build a throwaway `WorkshopConfig` rooted in a temp directory.
    /// Returns (config, _tempdir_guard) — keep the guard alive or the dir disappears.
    fn temp_config() -> (WorkshopConfig, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().to_path_buf();
        let logs_dir = data_dir.join("logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        std::fs::create_dir_all(data_dir.join("state")).unwrap();
        let config = WorkshopConfig {
            data_dir: data_dir.clone(),
            db_path: data_dir.join("workshop.db"),
            exports_dir: data_dir.join("exports"),
            logs_dir,
        };
        (config, tmp)
    }

    /// Spawn a minimal HTTP server that responds 200 on /health.
    /// Returns the port it bound to and a shutdown handle.
    async fn spawn_health_server() -> (u16, tokio::sync::oneshot::Sender<()>) {
        use axum::{Router, routing::get};

        let app = Router::new().route("/health", get(|| async { "ok" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await
                .unwrap();
        });
        (port, tx)
    }

    // -- DaemonError display --

    #[test]
    fn unavailable_display() {
        let err = DaemonError::Unavailable;
        assert_eq!(err.to_string(), "server is unavailable");
    }

    #[test]
    fn other_display_is_transparent() {
        let inner = anyhow::anyhow!("something broke");
        let err = DaemonError::Other(inner);
        assert_eq!(err.to_string(), "something broke");
    }

    // -- From<anyhow::Error> --

    #[test]
    fn from_anyhow() {
        let inner = anyhow::anyhow!("boom");
        let err: DaemonError = inner.into();
        assert!(matches!(err, DaemonError::Other(_)));
        assert_eq!(err.to_string(), "boom");
    }

    // -- from_reqwest --

    #[tokio::test]
    async fn from_reqwest_connect_error_yields_unavailable() {
        // Port 1 is reserved and nothing listens on it → guaranteed ConnectionRefused
        let err = reqwest::get("http://127.0.0.1:1/nope").await.unwrap_err();
        assert!(err.is_connect(), "expected a connect error, got: {err}");
        assert!(matches!(
            DaemonError::from_reqwest(err),
            DaemonError::Unavailable
        ));
    }

    #[test]
    fn from_reqwest_non_connect_error_yields_other() {
        // A builder error (invalid URL scheme) is not a connect error
        let err = reqwest::blocking::Client::new()
            .get("htp://[bad")
            .build()
            .unwrap_err();
        assert!(!err.is_connect());
        assert!(matches!(
            DaemonError::from_reqwest(err),
            DaemonError::Other(_)
        ));
    }

    // -- from_tungstenite: IO connection errors → Unavailable --

    #[test]
    fn from_tungstenite_connection_refused() {
        let io = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        let err = tungstenite::Error::Io(io);
        assert!(matches!(
            DaemonError::from_tungstenite(err),
            DaemonError::Unavailable
        ));
    }

    #[test]
    fn from_tungstenite_connection_reset() {
        let io = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "reset");
        let err = tungstenite::Error::Io(io);
        assert!(matches!(
            DaemonError::from_tungstenite(err),
            DaemonError::Unavailable
        ));
    }

    #[test]
    fn from_tungstenite_connection_aborted() {
        let io = std::io::Error::new(std::io::ErrorKind::ConnectionAborted, "aborted");
        let err = tungstenite::Error::Io(io);
        assert!(matches!(
            DaemonError::from_tungstenite(err),
            DaemonError::Unavailable
        ));
    }

    // -- from_tungstenite: IO non-connection error → Other --

    #[test]
    fn from_tungstenite_io_other_kind() {
        let io = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe");
        let err = tungstenite::Error::Io(io);
        assert!(matches!(
            DaemonError::from_tungstenite(err),
            DaemonError::Other(_)
        ));
    }

    // -- from_tungstenite: non-IO variant → Other --

    #[test]
    fn from_tungstenite_non_io_variant() {
        let err = tungstenite::Error::ConnectionClosed;
        assert!(matches!(
            DaemonError::from_tungstenite(err),
            DaemonError::Other(_)
        ));
    }

    // -- DaemonInfo URL builders --

    fn make_daemon_info(host: &str, port: u16) -> DaemonInfo {
        DaemonInfo {
            pid: 12345,
            port,
            host: host.to_string(),
        }
    }

    #[test]
    fn base_url_formats_correctly() {
        let info = make_daemon_info("127.0.0.1", 9000);
        assert_eq!(info.base_url(), "http://127.0.0.1:9000");
    }

    #[test]
    fn base_url_custom_host() {
        let info = make_daemon_info("0.0.0.0", 8080);
        assert_eq!(info.base_url(), "http://0.0.0.0:8080");
    }

    #[test]
    fn mux_ws_url_formats_correctly() {
        let info = make_daemon_info("192.168.1.1", 9000);
        assert_eq!(info.mux_ws_url(), "ws://192.168.1.1:9000/api/ws");
    }

    // -- rediscover_daemon --

    #[tokio::test]
    async fn rediscover_finds_healthy_daemon() {
        let (config, _tmp) = temp_config();
        let (port, _shutdown) = spawn_health_server().await;

        // Write daemon files pointing at the healthy server (use our own PID so kill(0) succeeds)
        let pid = std::process::id();
        write_daemon_files(&config, pid, port).unwrap();

        let info = rediscover_daemon(&config).await;
        assert!(info.is_some(), "should discover the healthy daemon");
        assert_eq!(info.unwrap().port, port);
    }

    #[tokio::test]
    async fn rediscover_returns_none_when_no_files() {
        let (config, _tmp) = temp_config();
        // No daemon files written
        assert!(rediscover_daemon(&config).await.is_none());
    }

    #[tokio::test]
    async fn rediscover_returns_none_when_server_dead() {
        let (config, _tmp) = temp_config();
        let pid = std::process::id();
        // Port 1 is reserved — nothing listens there
        write_daemon_files(&config, pid, 1).unwrap();

        assert!(
            rediscover_daemon(&config).await.is_none(),
            "should not discover a daemon whose health check fails"
        );
    }

    #[tokio::test]
    async fn rediscover_picks_up_new_port_after_restart() {
        let (config, _tmp) = temp_config();
        let pid = std::process::id();

        // Start server on first port
        let (port1, shutdown1) = spawn_health_server().await;
        write_daemon_files(&config, pid, port1).unwrap();

        let info1 = rediscover_daemon(&config).await.unwrap();
        assert_eq!(info1.port, port1);

        // "Restart": shut down old server, start new one on a different port
        let _ = shutdown1.send(());
        // Give the old listener time to close
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let (port2, _shutdown2) = spawn_health_server().await;
        assert_ne!(
            port1, port2,
            "sanity: new server should bind a different port"
        );
        write_daemon_files(&config, pid, port2).unwrap();

        let info2 = rediscover_daemon(&config).await.unwrap();
        assert_eq!(
            info2.port, port2,
            "should discover the new port after restart"
        );
    }
}
