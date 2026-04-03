# tty_wrapper

Standalone HTTP-controlled TTY wrapper. Wraps an interactive program (shell, Claude, etc.) in a PTY and exposes it over HTTP and WebSocket.

This package is **independent** from `workshop` — it can be built and run on its own.

## Usage

```sh
# Run with defaults (bash, 24x80)
cargo run -p tty_wrapper

# Run with a specific command
cargo run -p tty_wrapper -- --command claude --rows 40 --cols 120 --port 8080
```

## HTTP API

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/state` | Current PTY state (running, exit code, dimensions) |
| `POST` | `/input` | Send input bytes to the PTY |
| `GET` | `/output` | Stream PTY output (SSE) |
| `GET` | `/history` | Get buffered output history |
| `POST` | `/resize` | Resize the terminal (`rows`, `cols`) |
| `POST` | `/kill` | Kill the child process |
| `GET` | `/ws` | WebSocket connection for bidirectional I/O |

## Architecture

```
┌──────────────┐     HTTP/WS      ┌──────────────┐
│   Client(s)  │ ◄──────────────► │  tty_wrapper  │
└──────────────┘                  │  (axum)       │
                                  └──────┬───────┘
                                         │ PTY
                                  ┌──────┴───────┐
                                  │  Child proc   │
                                  │  (claude/sh)  │
                                  └──────────────┘
```

- `main.rs` — CLI args (clap), axum server setup, route registration
- `lib.rs` — `PtyManager` + HTTP route handlers
- `pty_actor.rs` — Actor for PTY lifecycle (spawn, I/O, signals)
- `pty_manager.rs` — `OutputEvent` streaming
- `websocket.rs` — WebSocket upgrade and bidirectional relay

## Testing

```sh
cargo test -p tty_wrapper
```
