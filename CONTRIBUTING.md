# Contributing to Workshop

## Prerequisites

- **Rust 1.91+** — `rustup update` to get the latest. Edition 2024 is required.
- **Bazel 8+** — for full CI builds and format checks. Install via [Bazelisk](https://github.com/bazelbuild/bazelisk).
- **pnpm** — for the SvelteKit frontend (`npm install -g pnpm`)
- **Claude Code CLI** — for testing instance creation (`npm install -g @anthropic-ai/claude-code`)

## Clone and Build

```sh
git clone https://github.com/anthropics/workshop && cd workshop

# Cargo for development (fast iteration)
cargo build -p workshop

# Bazel for CI-equivalent builds (includes format checks, edition 2024 enforcement)
bazel build //packages/workshop:work
```

## Running the Server

```sh
# Start server + TUI picker
cargo run -p workshop

# Start server only (for web UI development)
cargo run -p workshop -- server --debug
```

The server starts on a random port on `127.0.0.1`. The port is printed on startup.

## Web UI Development

The frontend is a SvelteKit app in `packages/workshop_ui/`.

```sh
# Install dependencies
cd packages/workshop_ui
pnpm install

# Start dev server with hot reload
pnpm dev

# Build for production
pnpm build

# Run tests
pnpm test
```

### Embedded UI feature flag

In development, the server runs without the embedded UI. To build a binary with the UI baked in:

```sh
WORKSHOP_UI_PATH=packages/workshop_ui/build cargo build -p workshop --features embedded-ui
```

Or use Bazel, which handles the frontend build automatically:

```sh
bazel build //packages/workshop:work
```

## Testing

```sh
# Unit tests for any package
cargo test -p workshop
cargo test -p virtual_terminal
cargo test -p <package>

# Full CI suite (all packages, format check, edition 2024)
bazel test //...

# Frontend tests
cd packages/workshop_ui && pnpm test
```

## Formatting

Always use:

```sh
bazel run //tools/format
```

**Never run `rustfmt` directly.** The Bazel format tool handles both Rust and frontend code with the project's configuration.

## Rust Edition 2024

All Rust code uses edition 2024. Cargo defaults to edition 2021 for `cargo check`/`cargo test`, so some edition 2024 errors only surface in Bazel.

**Known gotcha:** `ref mut` in match/if-let patterns is disallowed when the default binding mode is already `ref mut`. For example, when matching on `&mut Option<T>`:

```rust
// Wrong — rejected by edition 2024
if let Some(ref mut x) = opt { ... }

// Right
if let Some(x) = opt { ... }
```

Run `bazel test //...` before submitting to catch these.

## Crate Features

When adding a crate feature (e.g. `reqwest`'s `blocking`), update **both**:

1. `Cargo.toml` — the package's `[dependencies]` features list
2. `MODULE.bazel` — the corresponding `crate_index.spec()` features list

These must stay in sync.

## Project Layout

See the [architecture doc](docs/architecture.md) for system design details, or the CLAUDE.md files in each package for module-level maps and patterns:

- [`CLAUDE.md`](CLAUDE.md) — top-level build system and architecture notes
- [`packages/workshop/CLAUDE.md`](packages/workshop/CLAUDE.md) — server module map, key patterns, testing

## Documentation

- [Configuration Reference](docs/configuration.md) — CLI options, profiles, config.toml, env vars
- [Architecture](docs/architecture.md) — system design, WebSocket protocol, state detection
- [Operations](docs/operations.md) — endpoints, metrics, troubleshooting, database management
