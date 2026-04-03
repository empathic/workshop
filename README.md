# Workshop

Collaborative Claude Code for teams.

Run multiple Claude instances, share them with teammates in real time, and
search across every conversation you've ever had — from the terminal or the
browser.

<!-- TODO: screenshot / demo GIF -->

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/empathic-ai/workshop/main/scripts/install.sh | bash
```

Installs the `work` binary to `~/.local/bin`. Requires
[Claude Code](https://docs.anthropic.com/en/docs/claude-code).

## Get Started

```sh
work
```

A TUI opens where you create and switch between Claude Code sessions. Press
`Ctrl+]` to move between sessions and the overview. A background daemon keeps
everything running — close the TUI and your sessions stay alive.

For the web UI:

```sh
work server
```

Then open the URL it prints. You get live terminals, a conversation viewer,
task board, and fleet overview — all in the browser. The CLI and web UI talk
to the same daemon, so use whichever you prefer.

## Invite Your Team

Workshop is built for collaboration. To share over a tunnel:

```sh
work server --profile tunnel
work auth enable
```

Or on your LAN:

```sh
work server --profile server
work auth enable
```

Share the URL. The first person to register becomes admin. From there:

- **Shared terminals** — watch what Claude is doing on a teammate's instance,
  with a lock to coordinate who's typing
- **Live presence** — see who's connected and which instance they're focused on
- **Chat** — real-time messaging, per-instance or global
- **Task board** — create and assign tasks tied to instances, synced to every
  client

Your local CLI always bypasses auth, so `work attach` keeps working without
credentials.

## Search Everything

Workshop automatically syncs conversation history from Claude Code into a
local SQLite database — every session, every project, fully searchable.

```sh
work server --import-all           # sync all projects
work server --import-from ~/myapp  # sync one project
```

The web UI gives you full-text search with syntax-highlighted code and diffs.
Find that conversation from three weeks ago where Claude solved the exact
problem you're looking at now.

## Configuration

All config lives in `~/.workshop/config.toml`. See
[docs/configuration.md](docs/configuration.md) for the full reference —
profiles, env vars, auth settings, server tuning.

## FAQ

<details>
<summary><strong>What happens when I type <code>work</code>?</strong></summary>

A daemon starts in the background (if one isn't already running) and manages
all your Claude instances. The TUI connects to it. So does the web UI.
Closing the TUI doesn't kill anything — instances keep running until you
`work kill` them or `work kill-server`.
</details>

<details>
<summary><strong>How is this different from tmux?</strong></summary>

Workshop understands Claude's protocol. It detects state (thinking, tool use,
idle), imports conversations into a searchable database, provides multi-user
auth with live presence, and gives you a web dashboard. tmux sees raw bytes.
</details>

<details>
<summary><strong>Does my data leave my machine?</strong></summary>

No. Everything is local: SQLite database, conversation history, config. The
`tunnel` and `server` profiles bind to a network interface so teammates can
connect, but the data stays on your machine.
</details>

<details>
<summary><strong>What platforms are supported?</strong></summary>

Linux (x86_64, aarch64) and macOS (Apple Silicon, Intel).
</details>

## Learn More

- [Configuration](docs/configuration.md) — profiles, env vars, CLI options
- [Architecture](docs/architecture.md) — system design, protocols, state
  detection
- [Operations](docs/operations.md) — endpoints, metrics, troubleshooting
- [Contributing](CONTRIBUTING.md) — building from source, dev setup, testing

## License

Apache 2.0 — Copyright 2025-2026 Empathic, Inc.
