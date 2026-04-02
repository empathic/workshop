# workshop — CLAUDE.md

The main server, CLI client, and TUI picker. This is a lib+bin crate: the library (`src/lib.rs`) contains the server core, config, handlers, and WebSocket subsystem; the binary (`src/main.rs`) adds the CLI and TUI. The `workshop_desktop` Tauri app depends on the library crate.

## Module Map

```
src/
├── lib.rs               Library root — public module declarations, re-exports AppState
├── main.rs              CLI entry point (clap), server loop (uses library server functions)
├── server.rs            Shared server init, router builder, EmbeddedServer, daemon file helpers
├── config.rs            Figment-based layered config (WorkshopConfig, ServerConfig, AuthConfig)
├── db.rs                SQLite init + embedded migrations
├── models.rs            Shared data types (Instance, Message, Task, User, etc.)
├── auth.rs              JWT/session auth, registration, middleware (loopback bypass)
│
├── cli/                 CLI subcommands (attach, list, kill, auth, daemon, picker)
├── handlers/            HTTP route handlers (instances, tasks, conversations, admin, notes, browse, inbox)
├── repository/          Database query layer (conversations, tasks, auth, chat, search, inbox)
├── ws/                  WebSocket subsystem
│   ├── protocol.rs      ServerMessage enum — the wire format
│   ├── state_manager.rs Broadcast lifecycle, presence, terminal lock, inbox, state_entered_at
│   ├── handler.rs       Message dispatch (client → server)
│   ├── conversation_watcher.rs  Tail JSONL logs for live conversation updates
│   ├── focus.rs         User focus tracking
│   └── session_discovery.rs
│
├── inference/           Claude state detection
│   ├── engine.rs        Structured state from conversation logs
│   ├── manager.rs       Heuristic fallback from terminal output
│   └── state.rs         ClaudeState enum
│
├── files/               File browsing (tree, content, search)
├── git/                 Git operations (diff, status, log, branches)
├── views/               Maud HTML templates (fallback when no embedded UI)
├── instance_actor.rs    Per-instance tokio task (PTY + virtual terminal + clients)
├── instance_manager.rs  Create/list/stop instances
├── websocket_proxy.rs   Fan-out from 1 PTY to N WebSocket clients
├── import.rs            Claude conversation JSONL → SQLite importer
├── persistence.rs       Instance state snapshots (periodic + on-demand)
├── metrics.rs           Prometheus-style counters/gauges
├── notes.rs             Markdown note storage
├── onboarding.rs        First-run setup (admin account creation)
├── terminal.rs          Terminal emulation helpers
└── embedded_ui.rs       Conditional SPA serving (behind `embedded-ui` feature)
```

## Key Patterns

### Adding a New HTTP Endpoint

1. Write the handler in `handlers/<module>.rs`
2. Add the route in `server.rs` (in `build_router()`)
3. If it mutates shared state, follow the broadcast pattern (mutate DB → broadcast `ServerMessage` → return HTTP response)
4. Re-export the handler in `handlers/mod.rs`

### Adding a New WebSocket Message

1. Add a variant to `ServerMessage` in `ws/protocol.rs`
2. Handle incoming client messages in `ws/handler.rs`
3. Add the corresponding handler in the frontend's `stores/ws-handlers.ts`
4. Update the relevant frontend store to process the message

### Database Queries

All database access goes through `repository/`. Each file is a focused domain:
- `repository/tasks.rs` — task CRUD + tag operations
- `repository/conversations.rs` — full-text search, retrieval
- `repository/auth.rs` — user/session management
- `repository/chat.rs` — broadcast chat messages

Use `sqlx::query!` / `sqlx::query_as!` for compile-time checked queries.

### Instance Lifecycle

Instances flow through: Created → Running → Stopped. The actor model (`instance_actor.rs`) owns the PTY handle and virtual terminal for each instance. The `instance_manager.rs` coordinates creation and teardown.

Each instance carries an `InstanceKind` enum (`Structured { provider }` or `Unstructured { label }`), computed at creation via `InstanceKind::infer()`. This replaces all `command.contains("claude")` checks. The kind is stored in `InstanceInfo`, sent in the `ClaudeInstance` wire format, and used by `state_manager.rs` to gate conversation watching and state tracking.

## Testing

```sh
cargo test -p workshop
```

Test helpers are in `test_helpers.rs`. Tests that need a database use `tempfile` for isolated SQLite instances.
