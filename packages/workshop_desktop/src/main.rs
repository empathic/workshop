// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU16, Ordering};

use clap::Parser;
use tauri::Manager;
use tauri::menu::{AboutMetadata, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

use workshop::config::WorkshopConfig;
use workshop::server::{self, EmbeddedServer, ServerOptions, StartupProgress};

#[derive(Parser)]
#[command(name = "workshop-desktop")]
struct DesktopArgs {
    /// Custom data directory (defaults to ~/.workshop)
    #[arg(long)]
    data_dir: Option<PathBuf>,
}

/// How the desktop app is connected to the server.
enum ServerMode {
    /// We started the server ourselves — shut it down on exit.
    Embedded(EmbeddedServer),
    /// We connected to a pre-existing daemon — leave it running on exit.
    External { port: u16 },
}

struct AppState {
    server_mode: Mutex<Option<ServerMode>>,
    server_port: AtomicU16,
}

fn main() {
    tracing_subscriber::fmt::init();

    let args = DesktopArgs::parse();

    let data_dir = args.data_dir.clone();

    let state = AppState {
        server_mode: Mutex::new(None),
        server_port: AtomicU16::new(0),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .manage(state)
        .invoke_handler(tauri::generate_handler![retry_server_startup])
        .setup(move |app| {
            setup_menu(app)?;
            setup_tray(app)?;

            if let Some(window) = app.get_webview_window("main") {
                // In `cargo tauri dev`, the webview loads devUrl (Vite) before
                // setup runs. Don't navigate away — Vite proxies to the daemon
                // and handles its own reconnection.
                let has_dev_frontend = window
                    .url()
                    .map(|u| u.scheme() == "http" || u.scheme() == "https")
                    .unwrap_or(false);

                let _ = window.show();

                let handle = app.handle().clone();
                let data_dir_clone = data_dir.clone();
                tauri::async_runtime::spawn(async move {
                    start_or_discover_server(handle, data_dir_clone, has_dev_frontend).await;
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // macOS: hide window on close instead of destroying
                #[cfg(target_os = "macos")]
                {
                    let _ = window.hide();
                    api.prevent_close();
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = api;
                    let _ = window;
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // Handle true app exit (Cmd+Q, tray Quit, etc.)
            if let tauri::RunEvent::Exit = event {
                let state = app_handle.state::<AppState>();
                match state.server_mode.lock().unwrap().take() {
                    Some(ServerMode::Embedded(server)) => {
                        tracing::info!("App exiting — shutting down embedded server");
                        let _ = tauri::async_runtime::block_on(server.shutdown());
                    }
                    Some(ServerMode::External { port }) => {
                        tracing::info!(
                            "App exiting — leaving external server running on port {port}"
                        );
                    }
                    None => {}
                }
            }
        });
}

// =============================================================================
// Server Discovery & Startup
// =============================================================================

/// Discover an existing server or start an embedded one, then navigate the webview.
async fn start_or_discover_server(
    handle: tauri::AppHandle,
    data_dir: Option<PathBuf>,
    has_dev_frontend: bool,
) {
    // Update loading status — the loading page is already showing natively
    // via Tauri's frontendDist embedding (tauri://localhost/)
    if !has_dev_frontend {
        eval_loading(&handle, "setStatus('Checking for running server...')");
    }

    let config = match WorkshopConfig::new(data_dir) {
        Ok(c) => c,
        Err(err) => {
            tracing::error!("Failed to initialize config: {err:#}");
            show_error(&handle, &err.to_string());
            return;
        }
    };

    // Try to discover an existing healthy server first.
    // check_existing_server uses reqwest::blocking internally, which panics
    // inside a tokio spawn task. block_in_place tells tokio we're about to
    // block so it can move other tasks to different threads.
    let existing = tokio::task::block_in_place(|| server::check_existing_server(&config));
    if let Some(port) = existing {
        tracing::info!("Discovered existing server on port {port}");
        let url = format!("http://127.0.0.1:{}", port);

        let state = handle.state::<AppState>();
        state.server_port.store(port, Ordering::Relaxed);
        *state.server_mode.lock().unwrap() = Some(ServerMode::External { port });

        if has_dev_frontend {
            tracing::info!("Dev frontend detected — external server available for Vite proxy");
        } else {
            eval_loading(&handle, "setStatus('Connected to existing server...')");
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;

            if let Some(window) = handle.get_webview_window("main") {
                let navigate_js = format!("fadeOutAndNavigate('{}')", url.replace('\'', "\\'"));
                let _ = window.eval(&navigate_js);
            }
        }

        // Spawn health monitor for external server
        spawn_external_health_monitor(handle.clone(), port);
        return;
    }

    // No existing server — start our own
    let progress: Option<StartupProgress> = if !has_dev_frontend {
        let handle_clone = handle.clone();
        Some(std::sync::Arc::new(move |msg: &str| {
            let escaped = msg.replace('\'', "\\'");
            eval_loading(&handle_clone, &format!("setStatus('{escaped}')"));
        }))
    } else {
        None
    };

    let options = ServerOptions {
        port: Some(0), // auto-select
        ..Default::default()
    };

    match EmbeddedServer::start(config, options, progress).await {
        Ok(server) => {
            let port = server.port();
            let url = format!("http://127.0.0.1:{}", port);
            tracing::info!("Embedded server ready at {url}");

            let state = handle.state::<AppState>();
            state.server_port.store(port, Ordering::Relaxed);
            *state.server_mode.lock().unwrap() = Some(ServerMode::Embedded(server));

            if has_dev_frontend {
                tracing::info!("Dev frontend detected — server available for Vite proxy");
            } else {
                eval_loading(&handle, "setStatus('Connected — loading UI...')");
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;

                if let Some(window) = handle.get_webview_window("main") {
                    let navigate_js = format!("fadeOutAndNavigate('{}')", url.replace('\'', "\\'"));
                    let _ = window.eval(&navigate_js);
                }
            }
        }
        Err(err) => {
            tracing::error!("Failed to start embedded server: {err:#}");
            show_error(&handle, &err.to_string());
        }
    }
}

/// Background task that polls an external server's health every 5 seconds.
///
/// If the external server dies, navigates back to the embedded loading page
/// (`tauri://localhost/`) and shows an error with a retry button. On retry,
/// `start_or_discover_server()` runs again — it may rediscover a restarted
/// daemon or start an embedded server.
fn spawn_external_health_monitor(handle: tauri::AppHandle, port: u16) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        // Skip the first tick (immediate) since we just verified health
        interval.tick().await;

        loop {
            interval.tick().await;

            // Only monitor while we're in External mode
            {
                let state = handle.state::<AppState>();
                let mode = state.server_mode.lock().unwrap();
                match mode.as_ref() {
                    Some(ServerMode::External { .. }) => {}
                    _ => break, // switched to embedded or shutting down
                }
            }

            if !tokio::task::block_in_place(|| server::health_check_port(port)) {
                tracing::warn!("External server on port {port} is no longer responding");

                // Clear the stale server mode
                {
                    let state = handle.state::<AppState>();
                    *state.server_mode.lock().unwrap() = None;
                    state.server_port.store(0, Ordering::Relaxed);
                }

                // Navigate back to the embedded loading page
                if let Some(window) = handle.get_webview_window("main") {
                    let url: tauri::Url = "tauri://localhost/".parse().unwrap();
                    let _ = window.navigate(url);
                    // Brief delay for the page to load before showing error
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                show_error(&handle, "Server disconnected");
                break;
            }
        }
    });
}

fn show_error(handle: &tauri::AppHandle, msg: &str) {
    if let Some(window) = handle.get_webview_window("main") {
        let escaped = msg.replace('\\', "\\\\").replace('\'', "\\'");
        let js = format!(
            "(function _p(){{if(typeof showError==='function'){{showError('{}')}}else{{setTimeout(_p,10)}}}})()",
            escaped
        );
        let _ = window.eval(&js);
    }
}

// =============================================================================
// Native Menu Bar
// =============================================================================

fn setup_menu(app: &tauri::App) -> tauri::Result<()> {
    let settings_item = MenuItemBuilder::new("Settings...")
        .id("settings")
        .accelerator("CmdOrCtrl+,")
        .build(app)?;

    let reload_item = MenuItemBuilder::new("Reload")
        .id("reload")
        .accelerator("CmdOrCtrl+R")
        .build(app)?;

    #[cfg(debug_assertions)]
    let devtools_item = MenuItemBuilder::new("Toggle Developer Tools")
        .id("devtools")
        .accelerator("CmdOrCtrl+Alt+I")
        .build(app)?;

    // App submenu (becomes macOS application menu)
    let app_submenu = SubmenuBuilder::new(app, "Workshop")
        .about(Some(AboutMetadata {
            name: Some("Workshop".into()),
            version: Some(env!("CARGO_PKG_VERSION").into()),
            ..Default::default()
        }))
        .separator()
        .item(&settings_item)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;

    let edit_submenu = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    let view_submenu_builder = SubmenuBuilder::new(app, "View").item(&reload_item);
    #[cfg(debug_assertions)]
    let view_submenu_builder = view_submenu_builder.item(&devtools_item);
    let view_submenu = view_submenu_builder.separator().fullscreen().build()?;

    let window_submenu = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .separator()
        .close_window()
        .build()?;

    let menu = MenuBuilder::new(app)
        .items(&[&app_submenu, &edit_submenu, &view_submenu, &window_submenu])
        .build()?;

    app.set_menu(menu)?;

    app.on_menu_event(move |app_handle, event| match event.id().as_ref() {
        "settings" => {
            navigate_to_fragment(app_handle, "settings");
        }
        "reload" => {
            if let Some(window) = app_handle.get_webview_window("main") {
                let state = app_handle.state::<AppState>();
                let port = state.server_port.load(Ordering::Relaxed);
                if port > 0 {
                    let url: tauri::Url = format!("http://127.0.0.1:{}", port)
                        .parse()
                        .expect("invalid server URL");
                    let _ = window.navigate(url);
                }
            }
        }
        "devtools" =>
        {
            #[cfg(debug_assertions)]
            if let Some(window) = app_handle.get_webview_window("main") {
                if window.is_devtools_open() {
                    window.close_devtools();
                } else {
                    window.open_devtools();
                }
            }
        }
        _ => {}
    });

    Ok(())
}

/// Navigate to a fragment route by evaluating JS in the webview.
fn navigate_to_fragment(app_handle: &tauri::AppHandle, fragment: &str) {
    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
        let js = format!(
            "if (window.location.hostname === 'localhost' && window.location.port !== '5173') {{ window.location.hash = '{}'; }}",
            fragment
        );
        let _ = window.eval(&js);
    }
}

// =============================================================================
// System Tray
// =============================================================================

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let show_item =
        tauri::menu::MenuItem::with_id(app, "tray_show", "Show Window", true, None::<&str>)?;
    let quit_item =
        tauri::menu::MenuItem::with_id(app, "tray_quit", "Quit Workshop", true, None::<&str>)?;
    let menu = tauri::menu::Menu::with_items(app, &[&show_item, &quit_item])?;

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Workshop")
        .on_menu_event(|app_handle, event| match event.id.as_ref() {
            "tray_show" => {
                show_main_window(app_handle);
            }
            "tray_quit" => {
                app_handle.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    // Use app icon if available
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;

    Ok(())
}

fn show_main_window(app_handle: &tauri::AppHandle) {
    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

// =============================================================================
// Tauri IPC Commands
// =============================================================================

/// Called from the loading page when the user clicks "Retry".
#[tauri::command]
async fn retry_server_startup(handle: tauri::AppHandle) -> Result<(), String> {
    start_or_discover_server(handle, None, false).await;
    Ok(())
}

// =============================================================================
// Loading Page Helpers
// =============================================================================

/// Evaluate JS on the loading page, polling until the page is ready.
///
/// The webview loads `tauri://localhost/` asynchronously — the loading page's
/// script may not have executed yet when setup() fires. This wraps the call
/// in a poll that retries every 10ms until `setStatus` is defined.
fn eval_loading(handle: &tauri::AppHandle, js: &str) {
    if let Some(window) = handle.get_webview_window("main") {
        let safe = format!(
            "(function _p(){{if(typeof setStatus==='function'){{{js}}}else{{setTimeout(_p,10)}}}})()"
        );
        let _ = window.eval(&safe);
    }
}
