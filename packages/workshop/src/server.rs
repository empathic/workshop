use anyhow::{Context, Result, bail};
use axum::{
    Router,
    routing::{delete, get, patch, post},
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tower_http::cors::CorsLayer;
use tower_http::trace::MakeSpan;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};
use uuid::Uuid;

use crate::auth::AuthState;
use crate::config::{
    AuthConfig, FileConfig, Profile, RuntimeOverrides, ServerConfig, WorkshopConfig, load_config,
};

/// Callback for reporting startup progress to a host (e.g. the desktop loading page).
///
/// The string argument is a human-readable status message like
/// "Initializing database..." or "Syncing conversations (3/12 projects)...".
pub type StartupProgress = Arc<dyn Fn(&str) + Send + Sync>;
use crate::db::Database;
use crate::handlers;
use crate::import;
use crate::instance_manager::InstanceManager;
use crate::metrics::ServerMetrics;
use crate::notes;
use crate::persistence::{InstancePersistor, PersistenceService};
use crate::repository::ConversationRepository;
use crate::ws;

/// Options for starting the server without clap dependency.
pub struct ServerOptions {
    pub port: Option<u16>,
    pub host: Option<String>,
    pub profile: Option<Profile>,
    pub instance_base_port: u16,
    pub default_command: Option<String>,
    pub import_from: Option<PathBuf>,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self {
            port: None,
            host: None,
            profile: None,
            instance_base_port: 9000,
            default_command: None,
            import_from: None,
        }
    }
}

/// Custom span maker that adds a unique request ID to each incoming request.
#[derive(Clone)]
struct RequestIdMakeSpan;

impl<B> MakeSpan<B> for RequestIdMakeSpan {
    fn make_span(&mut self, request: &axum::http::Request<B>) -> tracing::Span {
        let request_id = Uuid::new_v4().to_string();
        tracing::info_span!(
            "request",
            method = %request.method(),
            uri = %request.uri(),
            request_id = %request_id,
        )
    }
}

/// Long-lived server state that survives router rebuilds during config reloads.
pub struct ServerCore {
    pub config: Arc<WorkshopConfig>,
    pub db: Arc<Database>,
    pub repository: Arc<ConversationRepository>,
    pub persistence_service: Arc<PersistenceService>,
    pub instance_manager: Arc<InstanceManager>,
    pub notes_storage: Arc<notes::NotesStorage>,
    pub global_state_manager: Arc<ws::GlobalStateManager>,
    pub metrics: Arc<ServerMetrics>,
    pub conversation_watchers:
        Arc<Mutex<HashMap<String, Box<dyn toolpath_convo::ConversationWatcher + Send>>>>,
    pub instance_persistors: Arc<Mutex<HashMap<String, Arc<InstancePersistor>>>>,
    pub restart_tx: Arc<tokio::sync::watch::Sender<()>>,
    pub runtime_overrides: Arc<tokio::sync::RwLock<RuntimeOverrides>>,
}

/// Shared application state passed to route handlers.
#[derive(Clone)]
#[allow(dead_code)]
pub struct AppState {
    pub instance_manager: Arc<InstanceManager>,
    pub conversation_watchers:
        Arc<Mutex<HashMap<String, Box<dyn toolpath_convo::ConversationWatcher + Send>>>>,
    pub config: Arc<WorkshopConfig>,
    pub server_config: Arc<ServerConfig>,
    pub auth_config: Arc<AuthConfig>,
    pub metrics: Arc<ServerMetrics>,
    pub db: Arc<Database>,
    pub repository: Arc<ConversationRepository>,
    pub persistence_service: Arc<PersistenceService>,
    pub instance_persistors: Arc<Mutex<HashMap<String, Arc<InstancePersistor>>>>,
    pub notes_storage: Arc<notes::NotesStorage>,
    pub global_state_manager: Arc<ws::GlobalStateManager>,
    pub runtime_overrides: Arc<tokio::sync::RwLock<RuntimeOverrides>>,
    pub restart_tx: Arc<tokio::sync::watch::Sender<()>>,
}

/// Initialize the long-lived server core (DB, instance manager, etc.).
pub async fn init_server_core(
    config: Arc<WorkshopConfig>,
    options: &ServerOptions,
    progress: Option<&StartupProgress>,
) -> Result<ServerCore> {
    // Initialize database
    if let Some(p) = &progress {
        p("Initializing database...");
    }
    info!("Initializing database...");
    let db = Arc::new(Database::new(&config).await?);

    // Create repository and persistence service
    let repository = Arc::new(ConversationRepository::new(db.pool.clone()));
    let persistence_service = Arc::new(PersistenceService::new(repository.clone()));

    // Start persistence service background task
    persistence_service.clone().start().await;

    // Handle import
    {
        if let Some(p) = &progress {
            p("Syncing conversation history...");
        }
        let importer =
            import::ConversationImporter::new(repository.as_ref().clone(), progress.cloned());

        let stats = if let Some(project_path) = &options.import_from {
            info!("Importing from project: {}", project_path.display());
            importer.import_from_project(project_path).await?
        } else {
            info!("Syncing conversations from Claude Code...");
            importer.import_all_projects().await?
        };

        if stats.imported > 0 || stats.updated > 0 || stats.failed > 0 {
            info!("Import complete!");
            info!("   Imported: {} conversations", stats.imported);
            info!("   Updated:  {} (re-imported changed files)", stats.updated);
            info!("   Skipped:  {} (already imported)", stats.skipped);
            info!("   Failed:   {}", stats.failed);
            info!("   Total:    {} sessions processed", stats.total());
        } else if stats.skipped > 0 {
            info!("Database in sync ({} conversations)", stats.skipped);
        }
    }

    // Starting services phase
    if let Some(p) = &progress {
        p("Starting services...");
    }

    // Determine default command
    let default_command = options.default_command.clone().unwrap_or_else(|| {
        info!("Detecting available commands...");
        let claude_check = std::process::Command::new("which").arg("claude").output();

        if let Ok(output) = claude_check
            && output.status.success()
        {
            let claude_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !claude_path.is_empty() {
                info!("Found 'claude' command at: {}", claude_path);
                return claude_path;
            }
        }

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        info!("'claude' not found or inaccessible, using shell: {}", shell);
        shell
    });

    info!("Default command configured: {}", default_command);

    // Create instance manager
    let fc_initial: FileConfig = load_config(&config.data_dir, options.profile.as_ref())
        .extract()
        .unwrap_or_default();
    let initial_server_config = ServerConfig::from_file(&fc_initial.server);

    let instance_manager = Arc::new(InstanceManager::new(
        default_command,
        options.instance_base_port,
        initial_server_config.instance.max_buffer_bytes,
        initial_server_config.instance.scrollback_lines,
        initial_server_config.instance.vt_record_dir.clone(),
    ));

    // Initialize notes storage
    let notes_storage = Arc::new(notes::NotesStorage::new(&config.data_dir)?);

    // Initialize global state manager for multiplexed WebSocket
    let state_broadcast = ws::create_state_broadcast();
    let global_state_manager = Arc::new(ws::GlobalStateManager::new(state_broadcast));
    global_state_manager.start_inbox_watcher(repository.clone());

    // Initialize metrics
    let metrics = Arc::new(ServerMetrics::new());

    // Shared mutable state across restarts
    let conversation_watchers = Arc::new(Mutex::new(HashMap::new()));
    let instance_persistors = Arc::new(Mutex::new(HashMap::new()));

    // Restart channel
    let (restart_tx, _restart_rx) = tokio::sync::watch::channel(());
    let restart_tx = Arc::new(restart_tx);

    // Runtime overrides (ephemeral, from TUI/API)
    let runtime_overrides = Arc::new(tokio::sync::RwLock::new(RuntimeOverrides::default()));

    Ok(ServerCore {
        config,
        db,
        repository,
        persistence_service,
        instance_manager,
        notes_storage,
        global_state_manager,
        metrics,
        conversation_watchers,
        instance_persistors,
        restart_tx,
        runtime_overrides,
    })
}

/// Build an AppState from the ServerCore and current config iteration.
pub fn build_app_state(
    core: &ServerCore,
    server_config: Arc<ServerConfig>,
    auth_config: Arc<AuthConfig>,
) -> AppState {
    AppState {
        instance_manager: core.instance_manager.clone(),
        conversation_watchers: core.conversation_watchers.clone(),
        config: core.config.clone(),
        server_config,
        auth_config,
        metrics: core.metrics.clone(),
        db: core.db.clone(),
        repository: core.repository.clone(),
        persistence_service: core.persistence_service.clone(),
        instance_persistors: core.instance_persistors.clone(),
        notes_storage: core.notes_storage.clone(),
        global_state_manager: core.global_state_manager.clone(),
        runtime_overrides: core.runtime_overrides.clone(),
        restart_tx: core.restart_tx.clone(),
    }
}

/// Build the full router with all routes and middleware.
pub fn build_router(
    app_state: AppState,
    auth_config: Arc<AuthConfig>,
    repository: Arc<ConversationRepository>,
) -> Router {
    let auth_state = AuthState {
        repository,
        auth_config,
    };

    let mut app = Router::new()
        // Instance routes
        .route("/api/instances", get(handlers::list_instances))
        .route("/api/instances", post(handlers::create_instance))
        .route("/api/instances/{id}", get(handlers::get_instance))
        .route("/api/instances/{id}", delete(handlers::delete_instance))
        .route("/api/instances/{id}/name", patch(handlers::set_custom_name))
        .route("/api/ws", get(handlers::multiplexed_websocket_handler))
        .route(
            "/api/instances/{id}/output",
            get(handlers::get_instance_output),
        )
        // File routes
        .route(
            "/api/instances/{id}/files",
            get(crate::files::list_instance_files),
        )
        .route(
            "/api/instances/{id}/files/search",
            get(crate::files::search_instance_files),
        )
        .route(
            "/api/instances/{id}/files/content",
            get(crate::files::get_instance_file_content),
        )
        // Git routes
        .route("/api/instances/{id}/git/log", get(crate::git::get_git_log))
        .route(
            "/api/instances/{id}/git/branches",
            get(crate::git::get_git_branches),
        )
        .route(
            "/api/instances/{id}/git/status",
            get(crate::git::get_git_status),
        )
        .route(
            "/api/instances/{id}/git/diff",
            get(crate::git::get_git_diff),
        )
        // Live conversation routes
        .route(
            "/api/instances/{id}/conversation",
            get(handlers::get_conversation),
        )
        .route(
            "/api/instances/{id}/conversation/poll",
            get(handlers::poll_conversation),
        )
        // Instance permission / invitation endpoints
        .route(
            "/api/instances/{id}/invite",
            post(handlers::create_invitation),
        )
        .route(
            "/api/invitations/{token}/accept",
            post(handlers::accept_invitation),
        )
        .route(
            "/api/instances/{id}/collaborators/{user_id}",
            delete(handlers::remove_collaborator),
        )
        // Database conversation endpoints
        .route("/api/conversations", get(handlers::list_conversations))
        .route(
            "/api/conversations/search",
            get(handlers::search_conversations_handler),
        )
        .route(
            "/api/conversations/{id}",
            get(handlers::get_conversation_by_id),
        )
        .route(
            "/api/conversations/{id}/comments",
            post(handlers::add_comment).get(handlers::get_comments),
        )
        .route(
            "/api/conversations/{id}/share",
            post(handlers::create_share),
        )
        .route("/api/share/{token}", get(handlers::get_shared_conversation))
        // Notes endpoints
        .route(
            "/api/notes/{session_id}",
            get(handlers::get_notes).post(handlers::create_note),
        )
        .route(
            "/api/notes/{session_id}/{note_id}",
            post(handlers::update_note).delete(handlers::delete_note),
        )
        // Task endpoints
        .route(
            "/api/tasks",
            get(handlers::list_tasks_handler).post(handlers::create_task_handler),
        )
        .route("/api/tasks/migrate", post(handlers::migrate_tasks_handler))
        .route(
            "/api/tasks/{id}",
            get(handlers::get_task_handler)
                .patch(handlers::update_task_handler)
                .delete(handlers::delete_task_handler),
        )
        .route("/api/tasks/{id}/send", post(handlers::send_task_handler))
        .route(
            "/api/tasks/{id}/dispatch",
            post(handlers::create_dispatch_handler),
        )
        .route("/api/tasks/{id}/tags", post(handlers::add_task_tag_handler))
        .route(
            "/api/tasks/{id}/tags/{tag_id}",
            delete(handlers::remove_task_tag_handler),
        )
        // User settings
        .route(
            "/api/user/settings",
            get(handlers::get_user_settings_handler).patch(handlers::update_user_settings_handler),
        )
        // Admin endpoints
        .route("/api/admin/stats", get(handlers::get_database_stats))
        .route("/api/admin/import", post(handlers::trigger_import))
        .route("/api/admin/restart", post(handlers::restart_handler))
        .route(
            "/api/admin/config",
            get(handlers::get_config_handler).patch(handlers::patch_config_handler),
        )
        .route(
            "/api/admin/invites",
            post(handlers::create_server_invite_handler).get(handlers::list_server_invites_handler),
        )
        .route(
            "/api/admin/invites/{token}",
            delete(handlers::revoke_server_invite_handler),
        )
        .route(
            "/api/admin/users",
            get(handlers::list_users_handler).post(handlers::create_user_handler),
        )
        .route(
            "/api/admin/users/{id}",
            patch(handlers::update_user_handler).delete(handlers::delete_user_handler),
        )
        // Browse endpoints
        .route("/api/browse", get(handlers::browse_directory))
        .route("/api/browse/worktree", post(handlers::create_worktree))
        .route("/api/browse/mkdir", post(handlers::create_directory))
        .route("/api/browse/git-info", get(handlers::git_detailed_info))
        // Bug report endpoint
        .route("/api/bug-report", post(handlers::create_bug_report))
        // Inbox endpoints
        .route("/api/inbox", get(handlers::list_inbox_handler))
        .route(
            "/api/inbox/{instance_id}/dismiss",
            post(handlers::dismiss_inbox_handler),
        )
        // Health endpoints
        .route("/health", get(handlers::health_handler))
        .route("/health/live", get(handlers::health_live_handler))
        .route("/health/ready", get(handlers::health_ready_handler))
        .route("/metrics", get(handlers::metrics_handler));

    // Merge auth routes
    app = app.merge(crate::auth::auth_routes().with_state(auth_state.clone()));

    // Apply auth middleware
    app = app.layer(axum::middleware::from_fn_with_state(
        auth_state,
        crate::auth::auth_middleware,
    ));

    let app = app
        .layer(TraceLayer::new_for_http().make_span_with(RequestIdMakeSpan))
        .layer(CorsLayer::permissive())
        .with_state(app_state);

    // Serve SPA when embedded
    #[cfg(feature = "embedded-ui")]
    let app = app.fallback_service(crate::embedded_ui::spa_router());

    app
}

/// Write daemon PID and port files after the server binds.
pub fn write_daemon_files(config: &WorkshopConfig, pid: u32, port: u16) -> Result<()> {
    use anyhow::Context;
    std::fs::write(config.daemon_pid_path(), pid.to_string())
        .context("Failed to write daemon PID file")?;
    std::fs::write(config.daemon_port_path(), port.to_string())
        .context("Failed to write daemon port file")?;
    Ok(())
}

/// Clean up daemon PID, port, and lock files — only if we own them.
///
/// Reads `daemon.pid` and only deletes state files if the PID matches
/// `std::process::id()`. This prevents server A from clobbering server B's
/// files when both target the same data directory.
pub fn release_daemon_files(config: &WorkshopConfig) {
    let dominated = match std::fs::read_to_string(config.daemon_pid_path()) {
        Ok(contents) => contents.trim().parse::<u32>().ok() == Some(std::process::id()),
        Err(_) => true, // file gone — nothing to protect
    };
    if dominated {
        let _ = std::fs::remove_file(config.daemon_pid_path());
        let _ = std::fs::remove_file(config.daemon_port_path());
        let _ = std::fs::remove_file(config.daemon_lock_path());
    }
}

/// RAII guard holding an exclusive advisory lock on `daemon.lock`.
///
/// The kernel releases the flock automatically if the process crashes.
/// On `Drop`, PID-aware file cleanup runs so stale files don't linger.
pub struct DaemonLock {
    config: Arc<WorkshopConfig>,
    /// Held for its `Drop` impl which calls `flock(LOCK_UN)`.
    _flock: nix::fcntl::Flock<std::fs::File>,
}

impl Drop for DaemonLock {
    fn drop(&mut self) {
        release_daemon_files(&self.config);
        // _flock drops here, releasing the advisory lock
    }
}

/// Try to acquire an exclusive advisory lock on `daemon.lock`.
///
/// Returns `Some(DaemonLock)` on success. Returns `None` if another process
/// already holds the lock (i.e. another server is running on this data dir).
pub fn try_acquire_daemon_lock(config: &Arc<WorkshopConfig>) -> Result<Option<DaemonLock>> {
    use nix::fcntl::{Flock, FlockArg};

    let lock_path = config.daemon_lock_path();
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("Failed to open lock file: {}", lock_path.display()))?;

    match Flock::lock(lock_file, FlockArg::LockExclusiveNonblock) {
        Ok(mut locked) => {
            // Write our PID into the lock file for diagnostics
            use std::io::{Seek, Write};
            let _ = locked.set_len(0);
            let _ = locked.seek(std::io::SeekFrom::Start(0));
            let _ = write!(locked, "{}", std::process::id());

            Ok(Some(DaemonLock {
                config: config.clone(),
                _flock: locked,
            }))
        }
        Err((_, nix::errno::Errno::EWOULDBLOCK)) => Ok(None),
        Err((_, e)) => Err(anyhow::anyhow!(
            "Failed to lock {}: {}",
            lock_path.display(),
            e
        )),
    }
}

/// Check if an existing server is running and healthy on this data directory.
///
/// Returns `Some(port)` if a healthy server is found, `None` otherwise.
pub fn check_existing_server(config: &WorkshopConfig) -> Option<u16> {
    // Read PID file and verify process is alive
    let pid_str = std::fs::read_to_string(config.daemon_pid_path()).ok()?;
    let pid: u32 = pid_str.trim().parse().ok()?;

    #[cfg(unix)]
    {
        use nix::sys::signal::kill;
        use nix::unistd::Pid;
        // kill(pid, 0) checks if process exists without sending a signal
        if kill(Pid::from_raw(pid as i32), None).is_err() {
            return None;
        }
    }

    // Read port file
    let port_str = std::fs::read_to_string(config.daemon_port_path()).ok()?;
    let port: u16 = port_str.trim().parse().ok()?;

    // Health check
    if health_check_port(port) {
        Some(port)
    } else {
        None
    }
}

/// Simple health probe: `GET http://127.0.0.1:{port}/health`.
///
/// Returns `true` on a 2xx response, `false` otherwise.
pub fn health_check_port(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{}/health", port);
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .ok()
        .and_then(|client| client.get(&url).send().ok())
        .is_some_and(|resp| resp.status().is_success())
}

/// An embedded server that can be started and stopped from within a host process
/// (e.g. the Tauri desktop app).
pub struct EmbeddedServer {
    port: u16,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    handle: JoinHandle<Result<()>>,
}

impl EmbeddedServer {
    /// Start the embedded server. Returns immediately with the bound port.
    ///
    /// Acquires an advisory lock on `daemon.lock` to prevent multiple servers
    /// on the same data directory. The lock is held for the server's lifetime.
    ///
    /// If `progress` is provided, it will be called with human-readable status
    /// messages during startup (e.g. "Initializing database...").
    pub async fn start(
        config: WorkshopConfig,
        options: ServerOptions,
        progress: Option<StartupProgress>,
    ) -> Result<Self> {
        let config = Arc::new(config);

        // Acquire exclusive lock — bail if another server is already running
        let daemon_lock = match try_acquire_daemon_lock(&config)? {
            Some(lock) => lock,
            None => {
                let pid_info = std::fs::read_to_string(config.daemon_pid_path())
                    .unwrap_or_else(|_| "unknown".to_string());
                let port_info = std::fs::read_to_string(config.daemon_port_path())
                    .unwrap_or_else(|_| "unknown".to_string());
                bail!(
                    "Another server is already running (PID {}, port {})",
                    pid_info.trim(),
                    port_info.trim()
                );
            }
        };

        let core = init_server_core(config.clone(), &options, progress.as_ref()).await?;

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // Resolve effective host/port
        let fc: FileConfig = load_config(&config.data_dir, options.profile.as_ref())
            .extract()
            .unwrap_or_default();

        let effective_host = options
            .host
            .as_deref()
            .or(fc.server.host.as_deref())
            .unwrap_or("127.0.0.1");
        let effective_port = options.port.or(fc.server.port).unwrap_or(0);

        let addr = format!("{}:{}", effective_host, effective_port).parse::<SocketAddr>()?;
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let actual_addr = listener.local_addr()?;
        let port = actual_addr.port();

        // Write daemon files so CLI clients can discover us
        let pid = std::process::id();
        write_daemon_files(&config, pid, port)?;

        info!("Embedded server listening on http://{}", actual_addr);

        let handle = tokio::spawn(async move {
            run_embedded_server_loop(core, listener, shutdown_rx, &options).await?;

            // DaemonLock drop handles PID-aware file cleanup
            drop(daemon_lock);
            Ok(())
        });

        Ok(Self {
            port,
            shutdown_tx,
            handle,
        })
    }

    /// The port the server is listening on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Shut down the server gracefully.
    pub async fn shutdown(self) -> Result<()> {
        let _ = self.shutdown_tx.send(true);
        self.handle.await??;
        Ok(())
    }
}

/// Run the embedded server loop (handles config-reload restarts).
async fn run_embedded_server_loop(
    core: ServerCore,
    initial_listener: tokio::net::TcpListener,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
    options: &ServerOptions,
) -> Result<()> {
    let mut restart_rx = core.restart_tx.subscribe();
    let mut listener = initial_listener;
    let mut cleanup_spawned = false;
    let mut first_iteration = true;

    loop {
        let fc: FileConfig = load_config(&core.config.data_dir, options.profile.as_ref())
            .extract()
            .unwrap_or_default();
        let overrides = core.runtime_overrides.read().await.clone();

        // Resolve effective auth/https
        let mut auth_config_raw = AuthConfig::from_file(&fc.auth);
        if let Some(auth_enabled) = overrides.auth_enabled {
            auth_config_raw.enabled = auth_enabled;
        }
        if let Some(https) = overrides.https {
            auth_config_raw.https = https;
        }

        let server_config = Arc::new(ServerConfig::from_file(&fc.server));
        let auth_config = Arc::new(auth_config_raw);

        if auth_config.enabled {
            info!(
                "Authentication ENABLED (session TTL: {}s)",
                auth_config.session_ttl_secs
            );
        }

        // Skip onboarding for embedded server (no interactive TTY)

        let app_state = build_app_state(&core, server_config, auth_config.clone());
        let app = build_router(app_state, auth_config.clone(), core.repository.clone());

        // Spawn session cleanup if needed
        if !cleanup_spawned && auth_config.enabled {
            cleanup_spawned = true;
            let cleanup_repo = core.repository.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
                loop {
                    interval.tick().await;
                    match cleanup_repo.cleanup_expired_sessions().await {
                        Ok(n) if n > 0 => info!("Cleaned up {} expired sessions", n),
                        _ => {}
                    }
                }
            });
        }

        #[cfg(feature = "embedded-ui")]
        if first_iteration {
            info!("Embedded UI enabled - serving SPA at /");
        }

        if !first_iteration {
            info!("Server restarted on http://{}", listener.local_addr()?);
        }

        let shutdown_gsm = core.global_state_manager.clone();
        let mut shutdown_rx_clone = shutdown_rx.clone();
        let shutdown_signal = async move {
            let _ = shutdown_rx_clone.wait_for(|v| *v).await;
            info!("Embedded server shutting down, notifying clients...");
            shutdown_gsm.broadcast_lifecycle(ws::ServerMessage::Shutdown {
                reason: "Server shutting down".to_string(),
            });
        };

        // Race: serve vs restart vs shutdown
        tokio::select! {
            result = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            ).with_graceful_shutdown(shutdown_signal) => {
                if let Err(e) = result {
                    warn!("Server error: {}", e);
                }
                break;
            }
            _ = restart_rx.changed() => {
                info!("Restarting HTTP server with new config...");
                first_iteration = false;
                // Re-bind on the same port (old listener was moved into axum::serve)
                let port_str = std::fs::read_to_string(core.config.daemon_port_path())
                    .unwrap_or_else(|_| "0".to_string());
                let port: u16 = port_str.trim().parse().unwrap_or(0);
                let effective_host = options
                    .host
                    .as_deref()
                    .unwrap_or("127.0.0.1");
                let addr = format!("{}:{}", effective_host, port).parse::<SocketAddr>()?;
                listener = tokio::net::TcpListener::bind(addr).await?;
                continue;
            }
        }
    }

    // Cleanup
    info!("Flushing persistence buffer...");
    if let Err(e) = core.persistence_service.flush_all().await {
        warn!("Failed to flush persistence buffer during shutdown: {}", e);
    }

    info!("Stopping running instances...");
    let instances = core.instance_manager.list().await;
    for instance in instances.iter().filter(|i| i.running) {
        if !core.instance_manager.stop(&instance.id).await {
            warn!("Failed to stop instance {} during shutdown", instance.id);
        }
    }
    info!("Stopped {} instances", instances.len());

    info!("Shutdown complete");
    Ok(())
}
