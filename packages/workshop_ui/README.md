# Workshop UI

SvelteKit web frontend for Workshop. Connects to the server over HTTP + WebSocket to provide a real-time interface for managing Claude Code instances.

## Quick Start

```sh
pnpm install
pnpm dev        # dev server at http://localhost:5173
```

The dev server proxies API requests to the Workshop server. Start the server first:

```sh
cargo run -p workshop -- server
```

## Build

```sh
pnpm build      # outputs to build/
```

The built assets are embedded into the Rust binary via the `workshop_ui` crate (`crate/`). See the root README for details on building with the embedded UI feature flag.

## Architecture

### Stores

Svelte 5 stores in `src/lib/stores/` manage all client state. Each store is a focused module:

| Store | Purpose |
|-------|---------|
| `websocket.ts` | WebSocket connection lifecycle, reconnect logic |
| `ws-handlers.ts` | Dispatches incoming `ServerMessage` variants to other stores |
| `instances.ts` | Instance list, lifecycle state, active instance selection |
| `terminal.ts` | xterm.js terminal emulator state |
| `conversation.ts` | Live conversation entries for the active instance |
| `tasks.ts` | Task board (CRUD, tags, assignment, dispatch) |
| `chat.ts` | Real-time broadcast chat |
| `activity.ts` | User activity / presence tracking |
| `terminalLock.ts` | Terminal write lock coordination (who can type) |
| `files.ts` | File explorer tree state |
| `git.ts` | Git status, diff, branch, log |
| `auth.ts` | Authentication state and session management |
| `claude.ts` | Claude instance inference state (idle/thinking/tool-use) |
| `history.ts` | Conversation history browser |
| `search.ts` | Search state |
| `metrics.ts` | Server metrics (Prometheus-style) |
| `settings.ts` | User settings |
| `ui.ts` | UI state (active panels, modals, sidebar) |

### Components

Components live in `src/lib/components/`. Major ones:

- **Terminal.svelte** — xterm.js wrapper with fit, clipboard, web-links plugins
- **ConversationView.svelte** — Notebook-style conversation viewer
- **NotebookCell.svelte** — Individual conversation entry (user/assistant/tool)
- **ChatPanel.svelte** — Real-time chat overlay
- **TaskPanel.svelte** — Task board with tags and filtering
- **Sidebar.svelte** — Instance list, navigation, status indicators
- **MainView.svelte** — Layout container with header and baud meter
- **FileExplorer.svelte** / **FileViewer.svelte** — File browsing and code viewing
- **ComposeForClaude.svelte** — Message composer for sending to Claude
- **ConversationMinimap.svelte** — D3 contour visualization of conversation shape

### Routes

```
src/routes/
├── +layout.svelte      Root layout (sidebar, global styles, CSS variables)
├── +page.svelte         Dashboard / active instance view
├── login/               Login form
├── register/            Registration form
├── account/             User account management
├── invite/              Invite token handling
├── history/             Conversation history browser
│   ├── [id]/            Conversation detail
│   └── search/          Search results
└── tasks/
    └── [id]/            Task detail
```

### Utilities

`src/lib/utils/` contains shared helpers:

- `api.ts` — HTTP client wrapper (handles auth, base URL)
- `markdown.ts` — Markdown rendering (marked + highlight.js)
- `fileLinks.ts` / `fileLinkMatch.ts` — Detect and link file references in text
- `fuzzy.ts` — Fuzzy search
- `copyable.ts` — Copy-to-clipboard
- `authGuard.ts` — Auth redirect logic

## Design System

The UI uses an amber phosphor CRT aesthetic. See `BRAND_BOOK.md` for the full design system including color tokens, typography, effects, and component library documentation.

Key design tokens are defined as CSS custom properties in `+layout.svelte`.

## Testing

```sh
pnpm test           # run all tests
pnpm test -- --watch  # watch mode
```

Tests use Jest and live alongside source files as `*.test.ts`.
