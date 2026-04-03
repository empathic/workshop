# Configuration Reference

Workshop uses layered configuration. Each layer overrides the one below it:

```
CLI flags  >  env vars  >  config.toml  >  profile defaults  >  struct defaults
```

## CLI Options

### `work server`

| Flag | Description | Default |
|------|-------------|---------|
| `--profile <PROFILE>` | Configuration profile: `local`, `tunnel`, or `server` | `local` |
| `-p, --port <PORT>` | Port for the web server (0 = auto-select) | `0` |
| `-b, --host <HOST>` | Host to bind to | `127.0.0.1` |
| `--instance-base-port <PORT>` | Base port for instances | `9000` |
| `--default-command <CMD>` | Default command for new instances | — |
| `-d, --debug` | Enable debug logging | `false` |
| `--data-dir <PATH>` | Custom data directory | `~/.workshop` |
| `--reset-db` | Reset database (with confirmation prompt) | — |
| `--import-all` | Import all existing Claude conversations on startup | — |
| `--import-from <PATH>` | Import conversations from a specific project directory | — |

### Other subcommands

| Command | Description |
|---------|-------------|
| `work` | Start daemon + open TUI picker (default) |
| `work attach <name-or-id>` | Attach to an instance by name or ID prefix |
| `work list [--json]` | List running instances |
| `work kill <name-or-id>` | Stop a specific instance |
| `work kill-server` | Stop the daemon and all instances |
| `work auth enable` | Enable authentication |
| `work auth disable` | Disable authentication |
| `work auth status` | Show current auth status |

## Profiles

Profiles set sensible defaults for common deployment scenarios. Specify with `--profile` or `profile = "..."` in config.toml.

| Profile  | Host        | Auth | Use Case                        |
|----------|-------------|------|---------------------------------|
| `local`  | `127.0.0.1` | off  | Solo development (default)      |
| `tunnel` | `127.0.0.1` | on   | Tunneling (ngrok, cloudflared)  |
| `server` | `0.0.0.0`   | on   | LAN or public deployment        |

### Profile Details

**`local`** — For single-user development on your own machine. Binds to localhost only, no authentication required. CLI commands work without credentials.

**`tunnel`** — For exposing your instance through a tunnel service. Binds to localhost (the tunnel handles external traffic) but enables auth so anyone with the tunnel URL must log in.

**`server`** — For LAN or public deployment. Binds to all interfaces and requires authentication. Use this when running on a shared server.

## Config File

Location: `~/.workshop/config.toml`

Full annotated reference:

```toml
# Configuration profile (local | tunnel | server)
profile = "local"

[auth]
# Enable/disable authentication
enabled = false
# Session lifetime in seconds (default: 7 days)
session_ttl_secs = 604800
# Allow new user registration
allow_registration = true

[server]
# Host to bind to
host = "127.0.0.1"
# Port (0 = auto-select)
port = 0
# Maximum output buffer per instance in MB
max_buffer_mb = 25
# Maximum history bytes sent on focus switch in KB
max_history_kb = 64
# Hang detection timeout in seconds (0 = disabled)
hang_timeout_secs = 300
# Scrollback buffer lines for terminal attach (applies on next attach, 100–100,000)
scrollback_lines = 10000
# Directory to write VT session recordings (.vtr files) for debugging/golden tests.
# When set, every instance captures PTY output/input/resize events with timestamps.
# Omit or leave unset to disable recording.
# vt_record_dir = "/tmp/vt-captures"
```

## Environment Variables

Every config field can be set via environment variable using the `WORKSHOP_` prefix with `__` (double underscore) as the section separator.

| Variable | Config equivalent | Example |
|----------|-------------------|---------|
| `WORKSHOP_PROFILE` | `profile` | `tunnel` |
| `WORKSHOP_AUTH__ENABLED` | `auth.enabled` | `true` |
| `WORKSHOP_AUTH__SESSION_TTL_SECS` | `auth.session_ttl_secs` | `604800` |
| `WORKSHOP_AUTH__ALLOW_REGISTRATION` | `auth.allow_registration` | `true` |
| `WORKSHOP_SERVER__HOST` | `server.host` | `0.0.0.0` |
| `WORKSHOP_SERVER__PORT` | `server.port` | `8080` |
| `WORKSHOP_SERVER__MAX_BUFFER_MB` | `server.max_buffer_mb` | `50` |
| `WORKSHOP_SERVER__MAX_HISTORY_KB` | `server.max_history_kb` | `128` |
| `WORKSHOP_SERVER__HANG_TIMEOUT_SECS` | `server.hang_timeout_secs` | `600` |
| `WORKSHOP_SERVER__SCROLLBACK_LINES` | `server.scrollback_lines` | `10000` |
| `WORKSHOP_SERVER__VT_RECORD_DIR` | `server.vt_record_dir` | — |

Legacy environment variables (still supported):

| Variable | Description | Default |
|----------|-------------|---------|
| `WORKSHOP_MAX_BUFFER_MB` | Maximum output buffer per instance (MB) | `1` |
| `WORKSHOP_MAX_HISTORY_KB` | Maximum history bytes sent on focus switch (KB) | `64` |
| `WORKSHOP_HANG_TIMEOUT_SECS` | Hang detection timeout (0 = disabled) | `300` |

## Conversation Import

Workshop can import Claude conversation logs from `~/.claude/projects/` into its local SQLite database for full-text search and browsing.

### On startup

```sh
# Import all conversations from all projects
work server --import-all

# Import from a specific project directory
work server --import-from /path/to/project
```

### At runtime (via API)

```sh
# Import all conversations
curl -X POST http://localhost:PORT/api/admin/import \
  -H "Content-Type: application/json" \
  -d '{"import_all": true}'
```

The web UI provides a searchable notebook-style conversation viewer with syntax-highlighted diffs and code blocks.

## Data Directory

All runtime data lives in `~/.workshop/` (override with `--data-dir`):

```
~/.workshop/
├── config.toml          Configuration file
├── workshop.db          SQLite database
├── exports/             Exported conversations
└── logs/                Server logs
```

## Web UI Build

The web UI is a SvelteKit application that can be embedded into the Rust binary behind a feature flag. For development, the server runs without the embedded UI.

### Building with embedded UI

```sh
# Build the SvelteKit app
cd packages/workshop_ui
pnpm install
pnpm build
cd ../..

# Build the Rust binary with embedded UI
WORKSHOP_UI_PATH=packages/workshop_ui/build cargo build -p workshop --features embedded-ui
```

Or use Bazel, which handles everything automatically:

```sh
bazel build //packages/workshop:work
```
