use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::info;

// =============================================================================
// Unified config (figment-deserialized from defaults / config.toml / env vars)
// =============================================================================
//
// Three equivalent ways to configure:
//
//   config.toml:     [auth]
//                    enabled = true
//
//   env var:         WORKSHOP_AUTH__ENABLED=true   (double underscore = nesting)
//
//   (single underscore stays within field names: WORKSHOP_AUTH__SESSION_TTL_SECS)

/// Named configuration presets.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    /// host=127.0.0.1, auth=off, https=off
    Local,
    /// host=127.0.0.1, auth=on, https=on
    Tunnel,
    /// host=0.0.0.0, auth=on, https=on
    Server,
}

/// Top-level tunable configuration, deserialized by figment.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FileConfig {
    #[serde(default)]
    pub profile: Option<Profile>,
    #[serde(default)]
    pub auth: AuthFileConfig,
    #[serde(default)]
    pub server: ServerFileConfig,
}

/// Auth-related tunables (lives under `[auth]` in config.toml).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthFileConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_session_ttl")]
    pub session_ttl_secs: u64,
    #[serde(default = "default_allow_registration")]
    pub allow_registration: bool,
    #[serde(default)]
    pub https: bool,
}

impl Default for AuthFileConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            session_ttl_secs: default_session_ttl(),
            allow_registration: default_allow_registration(),
            https: false,
        }
    }
}

/// Server tuning knobs (lives under `[server]` in config.toml).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerFileConfig {
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default = "default_max_buffer_mb")]
    pub max_buffer_mb: usize,
    #[serde(default = "default_max_history_kb")]
    pub max_history_kb: usize,
    #[serde(default = "default_hang_timeout_secs")]
    pub hang_timeout_secs: u64,
    #[serde(default = "default_scrollback_lines")]
    pub scrollback_lines: usize,
    /// Directory to write VT session recordings (`.vtr` files) for golden tests.
    /// When set, every instance captures PTY output/input/resize events.
    #[serde(default)]
    pub vt_record_dir: Option<String>,
}

impl Default for ServerFileConfig {
    fn default() -> Self {
        Self {
            host: None,
            port: None,
            max_buffer_mb: default_max_buffer_mb(),
            max_history_kb: default_max_history_kb(),
            hang_timeout_secs: default_hang_timeout_secs(),
            scrollback_lines: default_scrollback_lines(),
            vt_record_dir: None,
        }
    }
}

fn default_session_ttl() -> u64 {
    604800
}
fn default_allow_registration() -> bool {
    true
}
fn default_max_buffer_mb() -> usize {
    25
}
fn default_max_history_kb() -> usize {
    64
}
fn default_hang_timeout_secs() -> u64 {
    300
}
fn default_scrollback_lines() -> usize {
    10_000
}

/// Minimum scrollback lines (fewer than ~3 screens is useless).
pub const MIN_SCROLLBACK_LINES: usize = 100;
/// Maximum scrollback lines (~400MB worst-case at 80 cols).
pub const MAX_SCROLLBACK_LINES: usize = 100_000;

/// Build a figment that layers: defaults → profile defaults → config.toml → WORKSHOP_* env vars.
///
/// Profile defaults sit above struct defaults but below config.toml/env.
/// The CLI profile takes priority over the config file profile.
///
/// Env vars use double-underscore for nesting into sections:
///   `WORKSHOP_AUTH__ENABLED=true`  →  `auth.enabled = true`
///   `WORKSHOP_SERVER__MAX_BUFFER_MB=50`  →  `server.max_buffer_mb = 50`
pub fn load_config(data_dir: &Path, cli_profile: Option<&Profile>) -> figment::Figment {
    use figment::{
        Figment,
        providers::{Env, Format, Serialized, Toml},
    };

    // Pass 1: peek at profile from config.toml/env (CLI overrides file)
    let base = Figment::from(Serialized::defaults(FileConfig::default()))
        .merge(Toml::file(data_dir.join("config.toml")))
        .merge(Env::prefixed("WORKSHOP_").split("__"));

    let profile: Option<Profile> = cli_profile
        .cloned()
        .or_else(|| base.extract_inner("profile").ok());

    // Pass 2: rebuild with profile defaults as a layer between defaults and config.toml
    let profile_layer = profile_to_file_config(profile.as_ref());

    Figment::from(Serialized::defaults(FileConfig::default()))
        .merge(Serialized::defaults(profile_layer))
        .merge(Toml::file(data_dir.join("config.toml")))
        .merge(Env::prefixed("WORKSHOP_").split("__"))
}

/// Convert a profile into a `FileConfig` with the profile's default values filled in.
/// Fields not set by the profile remain at their struct defaults so figment
/// does not override explicit user values from config.toml / env.
fn profile_to_file_config(profile: Option<&Profile>) -> FileConfig {
    match profile {
        Some(Profile::Local) => FileConfig {
            profile: Some(Profile::Local),
            auth: AuthFileConfig {
                enabled: false,
                https: false,
                ..Default::default()
            },
            server: ServerFileConfig {
                host: Some("127.0.0.1".to_string()),
                ..Default::default()
            },
        },
        Some(Profile::Tunnel) => FileConfig {
            profile: Some(Profile::Tunnel),
            auth: AuthFileConfig {
                enabled: true,
                https: true,
                ..Default::default()
            },
            server: ServerFileConfig {
                host: Some("127.0.0.1".to_string()),
                ..Default::default()
            },
        },
        Some(Profile::Server) => FileConfig {
            profile: Some(Profile::Server),
            auth: AuthFileConfig {
                enabled: true,
                https: true,
                ..Default::default()
            },
            server: ServerFileConfig {
                host: Some("0.0.0.0".to_string()),
                ..Default::default()
            },
        },
        None => FileConfig::default(),
    }
}

/// Ephemeral config changes from TUI/API. Lost on daemon shutdown.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RuntimeOverrides {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub auth_enabled: Option<bool>,
    pub https: Option<bool>,
    pub scrollback_lines: Option<usize>,
}

// =============================================================================
// Runtime config structs (derived from FileConfig, used throughout the server)
// =============================================================================

/// Authentication configuration (runtime view).
#[derive(Clone, Debug)]
pub struct AuthConfig {
    /// Whether authentication is enabled
    pub enabled: bool,
    /// Session time-to-live in seconds (default: 7 days)
    pub session_ttl_secs: u64,
    /// Whether new user registration is open (default: true)
    pub allow_registration: bool,
    /// Whether to set Secure flag on cookies
    pub https: bool,
}

impl AuthConfig {
    pub fn from_file(fc: &AuthFileConfig) -> Self {
        Self {
            enabled: fc.enabled,
            session_ttl_secs: fc.session_ttl_secs,
            allow_registration: fc.allow_registration,
            https: fc.https,
        }
    }
}

/// Server configuration for runtime behavior.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// Instance-related settings
    pub instance: InstanceConfig,
    /// WebSocket-related settings
    pub websocket: WebSocketConfig,
    /// State detection settings
    #[allow(dead_code)]
    pub state: StateConfig,
}

#[derive(Clone, Debug)]
pub struct InstanceConfig {
    /// Maximum output buffer per instance in bytes
    pub max_buffer_bytes: usize,
    /// Number of scrollback lines the server-side vt100 parser retains
    pub scrollback_lines: usize,
    /// Consider instance hung after this duration without output (None = disabled)
    #[allow(dead_code)]
    pub hang_timeout: Option<Duration>,
    /// Number of PTY spawn retries
    #[allow(dead_code)]
    pub spawn_retries: usize,
    /// Directory to write VT session recordings. None = recording disabled.
    pub vt_record_dir: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct WebSocketConfig {
    /// Channel capacity for messages to client
    #[allow(dead_code)]
    pub send_channel_capacity: usize,
    /// Broadcast channel capacity for state updates
    #[allow(dead_code)]
    pub state_broadcast_capacity: usize,
    /// Maximum history bytes to send on focus switch
    pub max_history_replay_bytes: usize,
}

#[derive(Clone, Debug)]
pub struct StateConfig {
    /// Idle timeout for staleness detection
    #[allow(dead_code)]
    pub idle_timeout: Duration,
    /// Conversation poll interval
    #[allow(dead_code)]
    pub poll_interval: Duration,
}

impl ServerConfig {
    pub fn from_file(fc: &ServerFileConfig) -> Self {
        Self {
            instance: InstanceConfig {
                max_buffer_bytes: fc.max_buffer_mb * 1024 * 1024,
                scrollback_lines: fc
                    .scrollback_lines
                    .clamp(MIN_SCROLLBACK_LINES, MAX_SCROLLBACK_LINES),
                hang_timeout: if fc.hang_timeout_secs == 0 {
                    None
                } else {
                    Some(Duration::from_secs(fc.hang_timeout_secs))
                },
                spawn_retries: 2,
                vt_record_dir: fc.vt_record_dir.as_deref().map(PathBuf::from),
            },
            websocket: WebSocketConfig {
                send_channel_capacity: 100,
                state_broadcast_capacity: 256,
                max_history_replay_bytes: fc.max_history_kb * 1024,
            },
            state: StateConfig {
                idle_timeout: Duration::from_secs(10),
                poll_interval: Duration::from_millis(500),
            },
        }
    }
}

// =============================================================================
// Directory layout config (not tunable via figment — derived from --data-dir)
// =============================================================================

#[derive(Clone, Debug)]
pub struct WorkshopConfig {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    #[allow(dead_code)]
    pub exports_dir: PathBuf,
    pub logs_dir: PathBuf,
}

impl WorkshopConfig {
    pub fn new(custom_dir: Option<PathBuf>) -> Result<Self> {
        let data_dir = custom_dir.unwrap_or_else(|| {
            dirs::home_dir()
                .expect("Could not find home directory")
                .join(".workshop")
        });

        std::fs::create_dir_all(&data_dir)
            .with_context(|| format!("Failed to create data directory: {:?}", data_dir))?;

        let exports_dir = data_dir.join("exports");
        std::fs::create_dir_all(&exports_dir)
            .with_context(|| format!("Failed to create exports directory: {:?}", exports_dir))?;

        let logs_dir = data_dir.join("logs");
        std::fs::create_dir_all(&logs_dir)
            .with_context(|| format!("Failed to create logs directory: {:?}", logs_dir))?;

        let state_dir = data_dir.join("state");
        std::fs::create_dir_all(&state_dir)
            .with_context(|| format!("Failed to create state directory: {:?}", state_dir))?;

        let db_path = data_dir.join("workshop.db");

        info!("Data directory: {}", data_dir.display());

        Ok(Self {
            data_dir,
            db_path,
            exports_dir,
            logs_dir,
        })
    }

    pub fn db_url(&self) -> String {
        format!("sqlite://{}?mode=rwc", self.db_path.display())
    }

    pub fn reset_database(&self) -> Result<()> {
        if self.db_path.exists() {
            std::fs::remove_file(&self.db_path)
                .with_context(|| format!("Failed to delete database: {:?}", self.db_path))?;
            info!("Database reset: {:?}", self.db_path);

            let wal_path = self.db_path.with_extension("db-wal");
            if wal_path.exists() {
                std::fs::remove_file(&wal_path)?;
            }
            let shm_path = self.db_path.with_extension("db-shm");
            if shm_path.exists() {
                std::fs::remove_file(&shm_path)?;
            }
        }
        Ok(())
    }

    pub fn state_dir(&self) -> PathBuf {
        self.data_dir.join("state")
    }

    pub fn daemon_pid_path(&self) -> PathBuf {
        self.state_dir().join("daemon.pid")
    }

    pub fn daemon_port_path(&self) -> PathBuf {
        self.state_dir().join("daemon.port")
    }

    pub fn daemon_lock_path(&self) -> PathBuf {
        self.state_dir().join("daemon.lock")
    }

    pub fn daemon_log_path(&self) -> PathBuf {
        self.logs_dir.join("daemon.log")
    }

    pub fn daemon_err_path(&self) -> PathBuf {
        self.logs_dir.join("daemon.err")
    }

    pub fn config_toml_path(&self) -> PathBuf {
        self.data_dir.join("config.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── profile_to_file_config ──────────────────────────────────────────

    #[test]
    fn test_local_profile() {
        let fc = profile_to_file_config(Some(&Profile::Local));
        assert_eq!(fc.profile, Some(Profile::Local));
        assert!(!fc.auth.enabled);
        assert!(!fc.auth.https);
        assert_eq!(fc.server.host.as_deref(), Some("127.0.0.1"));
    }

    #[test]
    fn test_tunnel_profile() {
        let fc = profile_to_file_config(Some(&Profile::Tunnel));
        assert_eq!(fc.profile, Some(Profile::Tunnel));
        assert!(fc.auth.enabled);
        assert!(fc.auth.https);
        assert_eq!(fc.server.host.as_deref(), Some("127.0.0.1"));
    }

    #[test]
    fn test_server_profile() {
        let fc = profile_to_file_config(Some(&Profile::Server));
        assert_eq!(fc.profile, Some(Profile::Server));
        assert!(fc.auth.enabled);
        assert!(fc.auth.https);
        assert_eq!(fc.server.host.as_deref(), Some("0.0.0.0"));
    }

    #[test]
    fn test_no_profile() {
        let fc = profile_to_file_config(None);
        assert!(fc.profile.is_none());
        assert!(!fc.auth.enabled);
        assert!(fc.server.host.is_none());
    }

    // ── defaults ────────────────────────────────────────────────────────

    #[test]
    fn test_auth_file_config_defaults() {
        let d = AuthFileConfig::default();
        assert!(!d.enabled);
        assert_eq!(d.session_ttl_secs, 604800); // 7 days
        assert!(d.allow_registration);
        assert!(!d.https);
    }

    #[test]
    fn test_server_file_config_defaults() {
        let d = ServerFileConfig::default();
        assert!(d.host.is_none());
        assert!(d.port.is_none());
        assert_eq!(d.max_buffer_mb, 25);
        assert_eq!(d.max_history_kb, 64);
        assert_eq!(d.hang_timeout_secs, 300);
        assert_eq!(d.scrollback_lines, 10_000);
    }

    // ── AuthConfig::from_file ───────────────────────────────────────────

    #[test]
    fn test_auth_config_from_file() {
        let fc = AuthFileConfig {
            enabled: true,
            session_ttl_secs: 3600,
            allow_registration: false,
            https: true,
        };
        let ac = AuthConfig::from_file(&fc);
        assert!(ac.enabled);
        assert_eq!(ac.session_ttl_secs, 3600);
        assert!(!ac.allow_registration);
        assert!(ac.https);
    }

    // ── ServerConfig::from_file ─────────────────────────────────────────

    #[test]
    fn test_server_config_from_file_defaults() {
        let fc = ServerFileConfig::default();
        let sc = ServerConfig::from_file(&fc);
        assert_eq!(sc.instance.max_buffer_bytes, 25 * 1024 * 1024);
        assert_eq!(sc.websocket.max_history_replay_bytes, 64 * 1024);
        assert!(sc.instance.hang_timeout.is_some());
        assert_eq!(sc.instance.hang_timeout.unwrap().as_secs(), 300);
    }

    #[test]
    fn test_server_config_hang_timeout_zero_disables() {
        let fc = ServerFileConfig {
            hang_timeout_secs: 0,
            ..Default::default()
        };
        let sc = ServerConfig::from_file(&fc);
        assert!(sc.instance.hang_timeout.is_none());
    }

    #[test]
    fn test_server_config_custom_values() {
        let fc = ServerFileConfig {
            max_buffer_mb: 100,
            max_history_kb: 256,
            hang_timeout_secs: 600,
            ..Default::default()
        };
        let sc = ServerConfig::from_file(&fc);
        assert_eq!(sc.instance.max_buffer_bytes, 100 * 1024 * 1024);
        assert_eq!(sc.websocket.max_history_replay_bytes, 256 * 1024);
        assert_eq!(sc.instance.hang_timeout.unwrap().as_secs(), 600);
    }

    // ── WorkshopConfig ──────────────────────────────────────────────────

    #[test]
    fn test_workshop_config_with_custom_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let config = WorkshopConfig::new(Some(tmp.path().to_path_buf())).unwrap();

        assert_eq!(config.data_dir, tmp.path());
        assert_eq!(config.db_path, tmp.path().join("workshop.db"));
        assert_eq!(config.exports_dir, tmp.path().join("exports"));
        assert_eq!(config.logs_dir, tmp.path().join("logs"));
        assert!(tmp.path().join("exports").exists());
        assert!(tmp.path().join("logs").exists());
        assert!(tmp.path().join("state").exists());
    }

    #[test]
    fn test_db_url() {
        let tmp = tempfile::tempdir().unwrap();
        let config = WorkshopConfig::new(Some(tmp.path().to_path_buf())).unwrap();
        let url = config.db_url();
        assert!(url.starts_with("sqlite://"));
        assert!(url.contains("workshop.db"));
        assert!(url.ends_with("?mode=rwc"));
    }

    #[test]
    fn test_path_helpers() {
        let tmp = tempfile::tempdir().unwrap();
        let config = WorkshopConfig::new(Some(tmp.path().to_path_buf())).unwrap();

        assert_eq!(config.state_dir(), tmp.path().join("state"));
        assert_eq!(
            config.daemon_pid_path(),
            tmp.path().join("state/daemon.pid")
        );
        assert_eq!(
            config.daemon_port_path(),
            tmp.path().join("state/daemon.port")
        );
        assert_eq!(config.daemon_log_path(), tmp.path().join("logs/daemon.log"));
        assert_eq!(config.daemon_err_path(), tmp.path().join("logs/daemon.err"));
        assert_eq!(config.config_toml_path(), tmp.path().join("config.toml"));
    }

    #[test]
    fn test_reset_database() {
        let tmp = tempfile::tempdir().unwrap();
        let config = WorkshopConfig::new(Some(tmp.path().to_path_buf())).unwrap();

        // Create fake db files
        std::fs::write(&config.db_path, "fake db").unwrap();
        let wal = config.db_path.with_extension("db-wal");
        std::fs::write(&wal, "wal").unwrap();
        let shm = config.db_path.with_extension("db-shm");
        std::fs::write(&shm, "shm").unwrap();

        config.reset_database().unwrap();

        assert!(!config.db_path.exists());
        assert!(!wal.exists());
        assert!(!shm.exists());
    }

    #[test]
    fn test_reset_database_no_file() {
        let tmp = tempfile::tempdir().unwrap();
        let config = WorkshopConfig::new(Some(tmp.path().to_path_buf())).unwrap();
        // Should not error when file doesn't exist
        config.reset_database().unwrap();
    }

    // ── load_config ─────────────────────────────────────────────────────

    #[test]
    fn test_load_config_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let fc: FileConfig = load_config(tmp.path(), None).extract().unwrap();
        assert!(!fc.auth.enabled);
        assert!(fc.profile.is_none());
        assert!(fc.server.host.is_none());
    }

    #[test]
    fn test_load_config_with_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let fc: FileConfig = load_config(tmp.path(), Some(&Profile::Server))
            .extract()
            .unwrap();
        assert!(fc.auth.enabled);
        assert!(fc.auth.https);
        assert_eq!(fc.server.host.as_deref(), Some("0.0.0.0"));
    }

    #[test]
    fn test_load_config_toml_overrides_profile() {
        let tmp = tempfile::tempdir().unwrap();
        // Server profile defaults auth.enabled=true, but config.toml says false
        std::fs::write(tmp.path().join("config.toml"), "[auth]\nenabled = false\n").unwrap();
        let fc: FileConfig = load_config(tmp.path(), Some(&Profile::Server))
            .extract()
            .unwrap();
        assert!(!fc.auth.enabled);
    }

    #[test]
    fn test_load_config_toml_sets_values() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("config.toml"),
            "[server]\nhost = \"192.168.1.1\"\nport = 8080\nmax_buffer_mb = 50\nscrollback_lines = 500\n",
        )
        .unwrap();
        let fc: FileConfig = load_config(tmp.path(), None).extract().unwrap();
        assert_eq!(fc.server.host.as_deref(), Some("192.168.1.1"));
        assert_eq!(fc.server.port, Some(8080));
        assert_eq!(fc.server.max_buffer_mb, 50);
        assert_eq!(fc.server.scrollback_lines, 500);
    }

    #[test]
    fn test_load_config_scrollback_lines_defaults_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("config.toml"), "[server]\nport = 3000\n").unwrap();
        let fc: FileConfig = load_config(tmp.path(), None).extract().unwrap();
        assert_eq!(fc.server.scrollback_lines, 10_000);
    }
}
