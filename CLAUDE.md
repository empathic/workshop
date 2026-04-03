# CLAUDE.md

## Documentation

When changing code, **update the associated docs in the same change**. Key files: `docs/architecture.md` (system design), `docs/web-terminal.md` (client terminal), `docs/configuration.md` (config), `packages/workshop_ui/CLAUDE.md` (frontend conventions), this file (build/architecture notes). Stale docs are worse than no docs.

## Build System

This is a monorepo with Bazel, Cargo, and TS/JS build systems. Everything must stay in sync.

### Crate Features

When adding a crate feature (e.g. `reqwest`'s `blocking`), update **both**:
1. `Cargo.toml` — the package's `[dependencies]` features list
2. `MODULE.bazel` — the corresponding `crate_index.spec()` features list

### Formatting

Always use `bazel run //tools/format` to format code. Do not run `rustfmt` directly.

### Git

All commits **must be GPG-signed**. Never use `--no-gpg-sign`. If signing agent
errors occur, ask the user to unlock the signing agent or fix the issue rather
than bypassing signing.

### Rust Edition 2024

All Rust code uses edition 2024. Cargo defaults to edition 2021 for `cargo check`/`cargo test`, so some edition 2024 errors only surface in Bazel. Known gotcha: `ref mut` in match/if-let patterns is disallowed when the default binding mode is already `ref mut` (e.g. matching on `&mut Option<T>` — use `Some(x)` not `Some(ref mut x)`).

### Build Commands

- `cargo check -p workshop` — quick compile check
- `cargo test -p workshop` — run unit tests for the server
- `cargo test -p <package>` — run unit tests for any workspace crate
- `bazel test //...` — full CI-equivalent (includes format check, edition 2024)
- `WORKSHOP_UI_PATH=packages/workshop_ui/build cargo build -p workshop_ui` — build embedded UI crate

### Desktop App (Tauri)

- `cargo check -p workshop_desktop` — quick compile check
- `cargo test -p workshop_desktop` — run unit tests
- `cd packages/workshop_desktop && cargo tauri dev --config tauri.dev.conf.json` — launch desktop app with embedded server (auto-starts Vite dev server)
- `bazel build //packages/workshop_desktop:macos_app` — build macOS `.app` bundle (debug)
- `bazel build --config=opt //packages/workshop_desktop:macos_app` — build optimized `.app` bundle

**Dev workflow** (single terminal): `cd packages/workshop_desktop && cargo tauri dev --config tauri.dev.conf.json` — the Tauri app starts an embedded server in-process, and Vite's dev proxy discovers it automatically via the `daemon.port` file. The `--config` flag merges `tauri.dev.conf.json` (devUrl + beforeDevCommand) into the base config. The base `tauri.conf.json` has no dev URL — production builds never reference external dev servers.

**Custom data directory**: `workshop_desktop --data-dir /path/to/data` (defaults to `~/.workshop`).

Note: `workshop_desktop` is in workspace `members` but NOT in `default-members` (requires Tauri system deps). The desktop app depends on the `workshop` library crate (with `embedded-ui` feature) — no separate daemon process.

### Frontend (SvelteKit)

- `cd packages/workshop_ui && pnpm install && pnpm build` — build the web UI
- `cd packages/workshop_ui && pnpm dev` — dev server with hot reload
- `cd packages/workshop_ui && pnpm test` — run Jest tests
- `cd packages/workshop_ui && pnpm format` — format TS/Svelte with Prettier (also runs via `bazel run //tools/format`)

## TUI Styling

The terminal theme is solarized. Hardcoded ANSI colors are invisible or clash:
- **Avoid**: `Color::DarkGray`, `Color::White`, `Color::Cyan`, `Color::Black`, `Color::Green`, `Color::Yellow`
- **Use modifiers**: `BOLD`, `DIM`, `REVERSED`, `ITALIC`, `UNDERLINED`
- **Exception**: `Color::Red` is safe (universally visible, use for errors)

## Architecture Notes

### Package Dependency Graph

```
workshop (lib: server core, config, handlers, WS | bin "work": CLI + TUI)
├── claude_convo      (conversation log reader)
├── pty_manager        (PTY lifecycle)
└── virtual_terminal   (screen buffer + viewport negotiation)

tty_wrapper            (standalone HTTP-controlled PTY — not depended on by workshop)
workshop_ui           (SvelteKit frontend — embedded via rust-embed feature flag)
workshop_desktop      (Tauri native desktop app — embeds workshop server in-process)
  └── workshop (lib, with embedded-ui feature)
```

### Daemon Lifecycle

One server per data directory, enforced by advisory file lock (`daemon.lock`). The lock uses `flock(2)` — automatically released on process crash.

- **`try_acquire_daemon_lock()`** — non-blocking exclusive lock attempt; returns `None` if another server holds it
- **`check_existing_server()`** — reads `daemon.pid`/`daemon.port`, verifies process alive via `kill(pid, 0)`, then health-checks `GET /health`
- **`release_daemon_files()`** — PID-aware cleanup: only deletes state files if `daemon.pid` matches current process
- **`DaemonLock`** — RAII guard; `Drop` calls `release_daemon_files()`, then releases the flock
- Both `work server` and `EmbeddedServer::start()` acquire the lock before initializing
- Desktop app calls `check_existing_server()` first — connects to existing daemon if healthy, otherwise starts embedded

### Server Internals

- **Auth middleware** has a loopback bypass — CLI/TUI requests to `127.0.0.1` work without credentials
- **Server loop** supports hot restart via `restart_tx` watch channel (config reload without process restart)
- **Config layering**: struct defaults < profile defaults < config.toml < env vars < CLI flags < runtime overrides
- **Instance actor model**: each Claude instance runs in a dedicated `tokio::task` with its own PTY handle, virtual terminal, and client registry (`instance_actor.rs`)
- **`InstanceKind`** (`instance_manager.rs`): `Structured { provider }` (conversation-capable, e.g. Claude) or `Unstructured { label }` (terminal-only, e.g. bash). Computed at creation by `InstanceKind::infer()`, stored in `InstanceInfo`, and sent in the wire protocol. Backend uses `kind.is_structured()` instead of `command.contains("claude")`; frontend checks `inst.kind.type === 'Structured'`
- **Database**: SQLite via sqlx with embedded migrations (`db.rs`). Schema covers conversations, messages, tasks, users, sessions, chat, and instance snapshots

### Real-time Broadcast Pattern

All multi-user features (chat, presence, terminal lock, tasks, instance lifecycle) push updates to WebSocket clients via `state_manager.broadcast_lifecycle(ServerMessage::...)`. When adding a new mutation endpoint:

1. Mutate the DB
2. Broadcast a `ServerMessage` variant with a full snapshot (not a diff)
3. Return the HTTP response

Helpers in `handlers/tasks.rs` (`broadcast_task`, `broadcast_task_by_id`) and `repository::get_task_with_tags` show the pattern. Client-side stores must handle broadcasts idempotently (upsert by ID, not blind append) since the originating client receives both the HTTP response and its own broadcast echo.

### WebSocket Protocol

Two WebSocket endpoints:
- `/api/instances/{id}/ws` — single-instance terminal connection
- `/api/ws` — multiplexed connection (all instances, chat, presence, tasks)

The multiplexed protocol uses `ServerMessage` (defined in `ws/protocol.rs`) — a tagged enum serialized as JSON. When adding new real-time features, add a variant to `ServerMessage` and handle it in `ws/handler.rs` (server) and `stores/ws-handlers.ts` (client).

On graceful shutdown the server broadcasts `ServerMessage::Shutdown` to all connected clients. The frontend connection state machine has 6 states: `disconnected → connecting → connected → reconnecting → server_gone` (plus `error`). After 3 failed reconnect attempts the client escalates from `reconnecting` to `server_gone`, showing "Server Offline" instead of "Reconnecting...". Background retry continues indefinitely.

### Inbox System

Server-side attention model tracking instance state transitions that need user action. One `instance_inbox` row per instance (DB schema v11). Events:
- `completed_turn` — instance finished work (Active→Idle), turn_count accumulates
- `needs_input` — instance is `WaitingForInput`, auto-clears when user responds
- `error` — instance stopped unexpectedly

**Server flow:** State forwarding task in `ws/state_manager.rs` detects transitions via `prev_state`, upserts/clears inbox via `repository/inbox.rs`, broadcasts `ServerMessage::InboxUpdate`. On WS connect, server sends `InboxList` with all active items.

**HTTP endpoints:** `GET /api/inbox` (list), `POST /api/inbox/{id}/dismiss` (clear + broadcast)

**Frontend:** `stores/inbox.ts` — `inboxItems` store (Map by instance_id), `inboxSorted` (priority-sorted), `getAttentionLevel()` (critical/warning/active/idle/booting). Browser notifications for `needs_input` events (moved from Sidebar.svelte).

### Inference Engine

Claude's state (idle/thinking/tool-use/streaming) is detected by two systems:
- **Conversation watcher** (`ws/conversation_watcher.rs`) — tails the JSONL log file for structured state
- **Heuristic manager** (`inference/manager.rs`) — analyzes terminal output patterns as a fallback

State is exposed as `ClaudeState` in `inference/state.rs` and broadcast to clients.

### Terminal Multiplexing

Multiple clients share a single PTY per instance:
- `virtual_terminal` maintains the screen buffer, negotiates dimensions as min(all active viewports), and owns a server-side scrollback buffer (configurable via `scrollback_lines` in `[server]` config, default 10,000 lines). On resize, the visible screen is saved, a fresh parser is created at the new dimensions (clearing scrollback), and visible content is restored — the PTY program's SIGWINCH redraw rebuilds scrollback at the correct width
- `websocket_proxy.rs` manages the fan-out from one PTY to N WebSocket clients

The server's `vt100` parser aggregates terminal state — clients receive compacted snapshots (scrollback + visible screen keyframe), never raw PTY byte replay. This means intermediate cursor throbs, partial rewrites, and animation frames are collapsed into final line content. On focus switch (web) or attach (TUI), clients get the full aggregated state via `replay()`.
