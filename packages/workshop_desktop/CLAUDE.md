# workshop_desktop — CLAUDE.md

Native desktop app for Workshop using Tauri 2. Embeds the `workshop` server in-process (no separate daemon) and wraps the SvelteKit web UI in a native window with native OS integration.

## Architecture

The desktop app first checks for an existing healthy server (`check_existing_server()`). If found, it connects as an external client. Otherwise, it starts an `EmbeddedServer` in-process. The webview loads `http://127.0.0.1:{port}` either way.

```
src/
└── main.rs      Tauri setup, server discovery/lifecycle, menus, tray, window lifecycle
```

**Key types**:
- `ServerMode` — enum: `Embedded(EmbeddedServer)` (we own it) or `External { port }` (existing daemon)
- `AppState` — holds `Mutex<Option<ServerMode>>` and `AtomicU16` for the server port
- `EmbeddedServer` (from `workshop::server`) — starts/stops the axum server, writes daemon files for CLI discovery

**Custom data directory**: `--data-dir /path/to/data` via clap (defaults to `~/.workshop`).

## Native OS Integration

- **Menu bar**: App menu (About, Settings `Cmd+,`, Quit `Cmd+Q`), Edit (undo/redo/cut/copy/paste/select_all), View (Reload `Cmd+R`, Toggle DevTools `Cmd+Alt+I`, Fullscreen), Window (minimize/zoom/close)
- **System tray**: Left-click shows window, right-click context menu (Show Window, Quit). Tooltip shows "Workshop"
- **Window state persistence**: `tauri-plugin-window-state` saves/restores position, size, maximized state across launches. Window starts hidden (visible: false) to prevent flash before state restore
- **macOS close behavior**: Closing the window (Cmd+W / red X) hides it — the app stays running in the tray. Cmd+Q or tray Quit actually exits
- **Smooth loading transition**: Loading screen fades out before navigating to the server URL (no white flash)

## Build & Test

```sh
cargo check -p workshop_desktop
cargo test -p workshop_desktop
bazel build //packages/workshop_desktop
bazel test //packages/workshop_desktop:workshop_desktop_test
```

### Bazel + Tauri Compatibility

Tauri's `generate_context!()` proc macro writes cache files to `OUT_DIR` during compilation (icon hashes, plist, frontend assets). In Bazel, `OUT_DIR` is read-only after the build script runs. The workaround is in `build.rs`: pre-create the exact cache files during the build script phase (where `OUT_DIR` IS writable), so the proc macro finds matching content and skips the write. The build-deps `png`, `blake3`, `brotli`, `plist`, and `serde_json` exist solely for this pre-creation.

Frontend assets (`frontendDist`) are brotli-compressed into `$OUT_DIR/tauri-codegen-assets/{blake3_hash}.{ext}`. The `precreate_frontend_cache()` function in `build.rs` replicates this exactly so the proc macro's write is a no-op in Bazel.

Additionally, `ResolvedCommand` has `#[cfg(debug_assertions)]` fields. Bazel compiles proc macros in exec config (default: `opt`, no debug_assertions) but targets in `fastbuild` (with debug_assertions). This mismatch is fixed by a Starlark transition in `tauri_transition.bzl` that forces `host_compilation_mode` to match `compilation_mode`. The `tauri_binary` and `tauri_test` wrapper rules apply this transition — the raw `rust_binary` and `rust_test` targets are tagged `manual` and should not be built directly.

DevTools APIs (`is_devtools_open()`, `open_devtools()`, `close_devtools()`) are only available with `debug_assertions`. The devtools menu item and handler are gated behind `#[cfg(debug_assertions)]`.

## Dev Workflow

Single terminal: `cd packages/workshop_desktop && cargo tauri dev --config tauri.dev.conf.json`

The `--config` flag merges `tauri.dev.conf.json` (which adds `devUrl` and `beforeDevCommand`) into the base config. This launches Vite's dev server, then opens the Tauri window. The embedded server starts in-process and writes `daemon.port`, which Vite's `dynamicBackendProxy` reads to proxy `/api/*` and WebSocket requests.

The base `tauri.conf.json` has no `devUrl` — production builds (Bazel) never reference external dev servers. In dev mode, the webview loads Vite at `http://localhost:5173`. The embedded server still starts (so Vite can proxy to it), but the webview is not navigated away from the Vite URL.

## Key Patterns

### Loading Screen

The loading page (`loading-dist/index.html`) is embedded natively via Tauri's `frontendDist` config. Tauri serves it at `tauri://localhost/` — the webview shows it immediately on launch, before `setup()` runs. No JS injection or blank-page flash.

In `cargo tauri dev`, `generate_context!()` ignores `frontendDist` and uses `devUrl` instead, so Vite hot-reload works normally.

**IPC** with the loading page:
- **Rust → JS**: `window.eval()` calls global functions (`setStatus()`, `showError()`, `fadeOutAndNavigate()`)
- **JS → Rust**: Retry button invokes `retry_server_startup` Tauri command via `__TAURI_INTERNALS__.invoke()`

When the external server dies, the health monitor navigates back to `tauri://localhost/` (the embedded loading page) and shows the error.

### Server Lifecycle

- **Discovery**: `start_or_discover_server()` first calls `check_existing_server()` — if a healthy daemon is found, sets `ServerMode::External` and spawns a health monitor
- **Embedded startup**: if no existing server, calls `EmbeddedServer::start()` (which acquires the daemon lock), sets `ServerMode::Embedded`
- **Health monitor**: for external servers, polls `health_check_port()` every 5s. On failure, navigates back to `tauri://localhost/` and shows "Server disconnected" error with retry button
- **Shutdown**: `RunEvent::Exit` checks `ServerMode` — `Embedded` triggers `server.shutdown()`, `External` leaves the daemon running

### macOS Window Lifecycle

- `CloseRequested` → `window.hide()` + `api.prevent_close()` (keeps app in tray)
- `RunEvent::Exit` → shut down embedded server (true quit via Cmd+Q or tray)

## Release Build (macOS .app bundle)

```sh
bazel build //packages/workshop_desktop:macos_app          # debug
bazel build --config=opt //packages/workshop_desktop:macos_app  # optimized
```

The `tauri_binary` wrapper rule applies a Starlark transition that keeps `host_compilation_mode` in sync with `compilation_mode`, so both `-c opt` and `--config=opt` work correctly.

Produces `Workshop.app` with the Tauri binary (which includes the embedded server) in `Contents/MacOS/`. No sidecar binary needed. The `macos_app` rule in `macos_app.bzl` uses a tree artifact (directory output) to assemble the bundle structure. Code signing and notarization are deferred to when public distribution is needed.
