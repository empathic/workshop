pub const VERSION: &str = env!("CARGO_PKG_VERSION");

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};
use tracing_subscriber::prelude::*;

mod cli;

use workshop::config::{
    AuthConfig, FileConfig, Profile, ServerConfig, WorkshopConfig, load_config,
};
use workshop::server;

#[derive(Parser)]
#[command(name = "work")]
#[command(version = VERSION)]
#[command(about = "Terminal multiplexer for Claude Code instances")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Custom data directory (defaults to ~/.workshop)
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the daemon server in the foreground
    Server(ServerArgs),

    /// Attach to an existing instance
    Attach(AttachArgs),

    /// List running instances
    List(ListArgs),

    /// Kill a specific session
    Kill(KillArgs),

    /// Stop the daemon and all sessions
    KillServer(KillServerArgs),

    /// Manage authentication
    Auth(AuthArgs),
}

#[derive(Parser)]
struct ServerArgs {
    /// Port for the web server (0 = auto-select; overrides profile/config)
    #[arg(short, long)]
    port: Option<u16>,

    /// Host to bind to (overrides profile/config)
    #[arg(short = 'b', long)]
    host: Option<String>,

    /// Configuration profile (sets defaults for host/auth/https)
    #[arg(long, value_enum)]
    profile: Option<Profile>,

    /// Base port for Claude instances (will increment for each instance)
    #[arg(long, default_value = "9000")]
    instance_base_port: u16,

    /// Default command to run for new instances
    #[arg(long)]
    default_command: Option<String>,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,

    /// Clean start - reset database (prompt for confirmation)
    #[arg(long)]
    reset_db: bool,

    /// Reset the admin account password
    #[arg(long)]
    reset_admin: bool,

    /// Import all existing Claude conversations from the system
    #[arg(long)]
    import_all: bool,

    /// Import conversations from a specific project directory
    #[arg(long)]
    import_from: Option<PathBuf>,
}

#[derive(Parser)]
struct AttachArgs {
    /// Instance name, ID, or ID prefix to attach to (default: most recent)
    target: Option<String>,
}

#[derive(Parser)]
struct ListArgs {
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Parser)]
struct KillArgs {
    /// Instance name, ID, or ID prefix to kill
    target: String,
}

#[derive(Parser)]
struct KillServerArgs {
    /// Skip confirmation prompt
    #[arg(short, long)]
    force: bool,
}

#[derive(Parser)]
struct AuthArgs {
    #[command(subcommand)]
    command: AuthCommands,
}

#[derive(Subcommand)]
enum AuthCommands {
    /// Enable authentication (writes config, restarts server, prompts for admin if needed)
    Enable,
    /// Disable authentication
    Disable,
    /// Show current auth status
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let config = WorkshopConfig::new(cli.data_dir.clone())?;

    // Install crash diagnostics (terminal restore + crash report file)
    install_panic_hook(config.logs_dir.clone());

    // Initialize CLI-side file logging (server sets up its own in run_server)
    if !matches!(cli.command, Some(Commands::Server(_))) {
        init_cli_tracing(&config.logs_dir);
    }

    match cli.command {
        None => {
            // Bare `work`: create new instance in cwd and attach
            cli::default_command(&config).await
        }
        Some(Commands::Attach(args)) => cli::attach_command(&config, args.target).await,
        Some(Commands::List(args)) => cli::list_command(&config, args.json).await,
        Some(Commands::Kill(args)) => cli::kill_command(&config, &args.target).await,
        Some(Commands::KillServer(args)) => cli::kill_server_command(&config, args.force).await,
        Some(Commands::Auth(args)) => match args.command {
            AuthCommands::Enable => cli::auth::enable_command(&config).await,
            AuthCommands::Disable => cli::auth::disable_command(&config).await,
            AuthCommands::Status => cli::auth::status_command(&config).await,
        },
        Some(Commands::Server(args)) => run_server(args, config).await,
    }
}

/// Install a panic hook that restores the terminal and saves a crash report.
fn install_panic_hook(logs_dir: std::path::PathBuf) {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // 1. Restore terminal — critical so the user's shell isn't left in raw mode.
        //    Use raw ANSI sequences to avoid depending on ratatui/crossterm state.
        let _ = ratatui::crossterm::terminal::disable_raw_mode();
        let mut stdout = std::io::stdout();
        // Leave alternate screen + show cursor
        let _ = std::io::Write::write_all(&mut stdout, b"\x1b[?1049l\x1b[?25h");
        let _ = std::io::Write::flush(&mut stdout);

        // 2. Capture backtrace (always, regardless of RUST_BACKTRACE)
        let backtrace = std::backtrace::Backtrace::force_capture();

        // 3. Write crash report
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let crash_path = logs_dir.join(format!("crash-{timestamp}.log"));
        if let Ok(mut f) = std::fs::File::create(&crash_path) {
            use std::io::Write;
            let _ = writeln!(f, "=== Workshop Crash Report ===");
            let _ = writeln!(f, "Time: {}", chrono::Local::now());
            let _ = writeln!(f, "Version: {VERSION}");
            let _ = writeln!(f);
            let _ = writeln!(f, "Panic: {info}");
            if let Some(loc) = info.location() {
                let _ = writeln!(
                    f,
                    "Location: {}:{}:{}",
                    loc.file(),
                    loc.line(),
                    loc.column()
                );
            }
            let _ = writeln!(f);
            let _ = writeln!(f, "Backtrace:\n{backtrace}");

            eprintln!("\n[work] crash report saved to {}", crash_path.display());
        }

        // 4. Chain to default hook for the standard panic output
        default_hook(info);
    }));
}

/// Initialize file-based tracing for CLI commands (server has its own in run_server).
fn init_cli_tracing(logs_dir: &std::path::Path) {
    let log_path = logs_dir.join("cli.log");
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    else {
        return; // best-effort — don't block CLI startup
    };

    let writer = std::sync::Mutex::new(file);
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("workshop=debug,warn"));

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(writer)
                .with_ansi(false),
        )
        .with(env_filter)
        .init();
}

async fn run_server(args: ServerArgs, config: WorkshopConfig) -> Result<()> {
    // Setup logging
    let default_directive = if args.debug {
        "workshop=debug,tower_http=debug,info"
    } else {
        "workshop=info,tower_http=info,warn"
    };
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_directive));
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(env_filter)
        .init();

    info!("Starting Workshop - Claude Code Instance Manager");

    let config = Arc::new(config);

    // Handle database reset if requested
    if args.reset_db && config.db_path.exists() {
        println!("This will delete all stored conversations!");
        print!("Are you sure? (yes/no): ");
        use std::io::{self, Write};
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if input.trim() == "yes" {
            config.reset_database()?;
            println!("Database reset.");
        } else {
            println!("Cancelled.");
        }
    }

    // === Acquire daemon lock (one server per data dir) ===
    let _daemon_lock = match server::try_acquire_daemon_lock(&config)? {
        Some(lock) => lock,
        None => {
            let pid_info = std::fs::read_to_string(config.daemon_pid_path())
                .unwrap_or_else(|_| "unknown".to_string());
            anyhow::bail!(
                "Another server is already running (PID {}). Use `work kill-server` to stop it.",
                pid_info.trim()
            );
        }
    };

    // === Init shared server core ===
    let options = server::ServerOptions {
        port: args.port,
        host: args.host.clone(),
        profile: args.profile.clone(),
        instance_base_port: args.instance_base_port,
        default_command: args.default_command,
        import_from: args.import_from,
    };

    let core = server::init_server_core(config.clone(), &options, None).await?;

    // Reset admin password if requested (one-time, interactive — not for embedded)
    let cli_profile = args.profile.clone();
    let fc_initial: FileConfig = load_config(&config.data_dir, cli_profile.as_ref())
        .extract()
        .unwrap_or_default();
    if args.reset_admin {
        if fc_initial.auth.enabled {
            workshop::onboarding::reset_admin(&core.repository).await?;
        } else {
            warn!("--reset-admin ignored: auth is not enabled");
        }
    }

    // CLI --host/--port override everything (applied after figment + runtime overrides)
    let cli_host = args.host.clone();
    let cli_port = args.port;

    /// Tracks the server's TCP bind state across restart iterations.
    struct BindState {
        requested: (String, u16),
        actual_port: u16,
    }

    impl BindState {
        fn resolve_port(&self, effective_host: &str, effective_port: u16) -> u16 {
            if self.requested.0 == effective_host && self.requested.1 == effective_port {
                self.actual_port
            } else {
                effective_port
            }
        }
    }

    let mut bind_state: Option<BindState> = None;
    let mut restart_rx = core.restart_tx.subscribe();
    let mut cleanup_spawned = false;
    let mut first_iteration = true;

    // === Server loop: reload config and rebuild router on each iteration ===
    loop {
        let fc: FileConfig = load_config(&config.data_dir, cli_profile.as_ref())
            .extract()
            .unwrap_or_default();
        let overrides = core.runtime_overrides.read().await.clone();

        let effective_host = overrides
            .host
            .as_deref()
            .or(cli_host.as_deref())
            .or(fc.server.host.as_deref())
            .unwrap_or("127.0.0.1");
        let effective_port = overrides.port.or(cli_port).or(fc.server.port).unwrap_or(0);

        let mut auth_config_raw = AuthConfig::from_file(&fc.auth);
        if let Some(auth_enabled) = overrides.auth_enabled {
            auth_config_raw.enabled = auth_enabled;
        }
        if let Some(https) = overrides.https {
            auth_config_raw.https = https;
        }

        let server_config = Arc::new(ServerConfig::from_file(&fc.server));

        if auth_config_raw.enabled {
            info!(
                "Authentication ENABLED (session TTL: {}s)",
                auth_config_raw.session_ttl_secs
            );
        } else {
            info!("Authentication disabled (use `work auth enable` to enable)");
        }

        info!(
            "Server config: max_history={}KB, max_buffer={}MB",
            server_config.websocket.max_history_replay_bytes / 1024,
            server_config.instance.max_buffer_bytes / (1024 * 1024)
        );

        // Onboarding only on first iteration
        if first_iteration {
            workshop::onboarding::maybe_run_onboarding(&core.repository, &auth_config_raw).await?;
        }

        let auth_config = Arc::new(auth_config_raw);

        let app_state = server::build_app_state(&core, server_config, auth_config.clone());
        let app = server::build_router(app_state, auth_config.clone(), core.repository.clone());

        // Spawn periodic session cleanup
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

        let port = match &bind_state {
            Some(bs) => {
                let resolved = bs.resolve_port(effective_host, effective_port);
                if resolved != bs.actual_port {
                    info!(
                        "Bind address changed: was :{}, now requesting :{}",
                        bs.actual_port, resolved
                    );
                }
                resolved
            }
            None => effective_port,
        };
        let addr = format!("{}:{}", effective_host, port).parse::<SocketAddr>()?;
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let actual_addr = listener.local_addr()?;
        bind_state = Some(BindState {
            requested: (effective_host.to_string(), effective_port),
            actual_port: actual_addr.port(),
        });

        // Write daemon PID and port files
        let pid = std::process::id();
        server::write_daemon_files(&config, pid, actual_addr.port())?;

        if first_iteration {
            info!("Workshop listening on http://{}", actual_addr);
            info!("");
            info!("Web UI: http://{}/", actual_addr);
            info!("API endpoints:");
            info!("  GET    /api/instances       - List all instances");
            info!("  POST   /api/instances       - Create new instance");
            info!("  GET    /api/instances/:id   - Get instance details");
            info!("  DELETE /api/instances/:id   - Stop instance");
            info!("  GET    /api/ws              - Multiplexed WebSocket connection");
        } else {
            info!("Server restarted on http://{}", actual_addr);
        }

        // Shutdown signal handler
        let shutdown_gsm = core.global_state_manager.clone();
        let shutdown_signal = async move {
            let ctrl_c = tokio::signal::ctrl_c();
            #[cfg(unix)]
            {
                let mut sigterm =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("Failed to install SIGTERM handler");
                tokio::select! {
                    _ = ctrl_c => {}
                    _ = sigterm.recv() => {}
                }
            }
            #[cfg(not(unix))]
            {
                ctrl_c.await.expect("Failed to install Ctrl+C handler");
            }
            info!("Received shutdown signal, notifying clients...");
            shutdown_gsm.broadcast_lifecycle(workshop::ws::ServerMessage::Shutdown {
                reason: "Server shutting down".to_string(),
            });
        };

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
                continue;
            }
        }
    }

    // === Cleanup ===
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

    // _daemon_lock drops here — PID-aware file cleanup + lock release

    info!("Shutdown complete");
    Ok(())
}
