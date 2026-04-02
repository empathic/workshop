# workshop_ui — CLAUDE.md

## Stack

- **SvelteKit** 2.x with **Svelte 5** (runes, `$state`, `$derived`, `$effect`)
- **TypeScript** 5.7
- **xterm.js** 6.0 for terminal emulation
- **marked** + **highlight.js** for markdown/code rendering
- **d3** for the conversation minimap
- **Jest** for testing
- **pnpm** as package manager

## Conventions

### State Management

All shared state lives in `src/lib/stores/`. Stores use Svelte 5 runes (`$state`, `$derived`). The WebSocket store (`websocket.ts`) is the backbone — it manages the connection and dispatches incoming `ServerMessage` payloads via `ws-handlers.ts` to domain-specific stores.

Stores must handle WebSocket broadcasts **idempotently** (upsert by ID, not blind append) because the originating client receives both its HTTP response and its own broadcast echo.

**localStorage-backed stores** (`settings.ts`, `drafts.ts`) persist client-only preferences and draft input across page reloads. Keep business logic in pure utility modules (`utils/draft-map.ts`) and limit the store to wiring reactivity + persistence. Debounce writes; flush synchronously on `beforeunload`.

**Settings** (`settings.ts`): Layered persistence — localStorage for instant hydration, server (`/api/user/settings`) as source of truth for cross-device sync. On WebSocket connect, client fetches server settings and merges (server wins). Changes write to both localStorage and server (async PATCH), then broadcast to all clients via `UserSettingsUpdate`. UI-only keys (e.g. `drawerWidth`) skip server sync. Quick access via gear icon in sidebar; full settings via `settings` pane kind.

### Layout Architecture

The UI uses a **binary split pane** layout (tmux-inspired). Core data lives in `stores/layout.ts`:

- **`LayoutNode`** = `SplitNode | LeafNode` — recursive tree, immutable updates
- **`PaneContent`** = discriminated union tagged by `kind`, split into **instance-bound** and **directory-bound** variants:
  - Instance-bound (carry `instanceId`):
    - `{ kind: 'terminal'; instanceId: string | null }` — terminal bound to an instance
    - `{ kind: 'conversation'; instanceId: string | null; viewMode: 'structured' | 'raw' }` — conversation view
  - Directory-bound (carry `workingDir`):
    - `{ kind: 'file-explorer'; workingDir: string | null }` — file tree for a project directory
    - `{ kind: 'file-viewer'; filePath: string | null; lineNumber?: number; workingDir: string | null; diffContext?: { commit?, base?, head?, diffMode? } }` — file content/diff viewer scoped to a directory. Optional `diffContext` enables multi-file diff navigation
    - `{ kind: 'tasks'; workingDir: string | null }` — task panel scoped to a directory
    - `{ kind: 'git'; workingDir: string | null }` — git diff/log view for a directory
  - Other:
    - `{ kind: 'chat'; scope: 'global' | string }` — chat panel (global or instance-scoped)
    - `{ kind: 'settings' }` — settings panel (no instanceId)
- **Pane Kind Registry** (`utils/pane-content.ts`): `PANE_KIND_REGISTRY` is the single source of truth for all pane kind metadata (label, shortLabel, desc, binding type, icons, selectable flag). Derived lookups: `PANE_KIND_MAP` (kind→def), `SELECTABLE_KINDS` (chrome dropdown + picker order), `INSTANCE_BOUND_KINDS` (terminal/conversation), `DIR_BOUND_KINDS` (file-explorer/tasks/git/file-viewer), `PERSISTABLE_CONTENT_KINDS` (all except picker), `kindLabel(kind)`, `kindShortLabel(kind)`. Adding a new pane kind = add to `PaneContentKind` union + add registry entry + add `PaneContent` variant + add dispatch case in PaneHost.
- Helpers (defined in `utils/pane-content.ts`, re-exported from `stores/layout.ts`): `getPaneInstanceId(content)` extracts instanceId (terminal/conversation only); `getPaneWorkingDir(content, instanceMap)` resolves workingDir from any variant (directory-bound directly, instance-bound via instance lookup — takes an explicit instances map, no hidden store reads); `defaultContentForKind(kind, workingDir?)` constructs default config; `migratePaneContentV3toV4(content)` handles persistence migration
- Actions: `splitPane`, `closePane`, `focusPane`, `setSplitRatio`, `setPaneContent`, `setPaneViewMode`, `togglePaneViewMode`

Components in `src/lib/components/layout/`:
- **LayoutTree.svelte** — recursive renderer (split → flex + SplitHandle, leaf → PaneHost). CSS transitions on split/close (150ms, disabled during drag). On mobile with multiple panes, renders focused pane with a tab bar (instance names, status dots, close/add buttons).
- **PaneHost.svelte** — dispatches to content component by `kind`, passes explicit props from the discriminated union (no global fallback). Each pane owns its own `instanceId`/`filePath`/`scope`.
- **PaneChrome.svelte** — title bar with content type dropdown, instance selector (for terminal/conversation), project label (for file-explorer/tasks/git), split/close buttons, status dot. Split buttons hide via `ResizeObserver` when pane is <180px wide; close button is always visible. File-viewer panes show filename; chat panes show scope label.
- **SplitHandle.svelte** — drag-to-resize between split children, keyboard accessible (`role="separator"`, arrow keys ±5%/±15%, Home=50%)
- **Pane\*.svelte** — thin wrappers (PaneTerminal, PaneConversation, PaneFileExplorer, PaneChat, PaneTasks, PaneFileViewer, PaneGit, PaneSettings). Each accepts explicit props from its union variant.

**PaneFileViewer** is self-contained — it fetches file content independently via `apiGet`, has its own loading/error/empty states, and does not read global file viewer state. Two file-viewer panes can show different files simultaneously. It is directory-bound (carries `workingDir`) so that `effectiveInstanceId` resolves correctly for its API calls. When `diffContext` is present, PaneFileViewer fetches the full multi-file diff and renders `DiffView` with file navigation (same as the drawer). Supports diff/source toggle and engine cycling. Both PaneFileViewer and the drawer FileViewer pass an `onfetchContent` callback to DiffView for lazy-loading full file content (used by expandable context gaps).

**DiffView** (`file-viewer/DiffView.svelte`): Renders diff hunks with line-level highlighting. Features: (1) **Expandable context gaps** — between hunks, clickable bars show hidden line count; clicking fetches full file content via `onfetchContent` and splices lines as synthetic context. Content is cached per file; `expandedGaps` tracks which gaps are open. (2) **Dual file navigation** — for ≤5 files, horizontal chip strip; for >5 files, collapsible vertical file panel with full paths, status letters, and per-file stats. Both modes support `[`/`]` keyboard navigation.

**Instance-bound vs directory-bound panes**: Binding categories are defined in the pane kind registry (`INSTANCE_BOUND_KINDS`, `DIR_BOUND_KINDS`). Terminal and conversation panes carry `instanceId` and show an instance selector in PaneChrome. File-explorer, file-viewer, tasks, and git panes carry `workingDir` and show a project label instead (file-viewer shows filename rather than project label in chrome). When a directory-bound pane is focused, `currentInstanceId` is resolved by finding any instance in the same `workingDir` (via `effectiveInstanceId` in `setupLayoutSync`). The instance picker only shows for `INSTANCE_BOUND_KINDS`. The pane wrapper components (PaneFileExplorer, PaneTasks, PaneGit) are zero-prop wrappers — they read from global stores (`currentInstance`, `currentInstanceId`) which are correctly set by the layout system's `effectiveInstanceId` derivation. The `workingDir` on `PaneContent` is a layout concern (drives `effectiveInstanceId` and `dirLabel`), not a component concern.

**Embedded panel pattern**: FileExplorer, ChatPanel, TaskPanel, FileViewer accept an `embedded` prop. When `true`, they skip the `position: fixed` overlay chrome (backdrop, close button, resize handle) and render inline. Pane wrappers pass `embedded={true}`. In single-pane mode, overlays still work as before.

**Persistence**: Per-project layout persistence. Each project gets its own `localStorage` key (`workshop_layout:<projectId>`) and `workshop_layout:meta` tracks which project was last active. `switchProject(workingDir, instanceId?)` saves the current layout, loads the target's (or creates a default single-pane), and updates `activeProjectId`. Legacy single-key layouts (`workshop_layout`) are migrated on first project switch. Schema version 4 with debounced 300ms writes, flushed on `beforeunload`. Deserialization migrates legacy flat format (version 1→2), adds `viewMode` (2→3), converts directory-bound kinds to `workingDir` (3→4), and adds `workingDir` to file-viewer (4→5).

**Presets**: `applyPreset('single' | 'dev-split' | 'side-by-side')` — accessible from MainHeader.

**Terminal cap**: Max 6 terminal panes enforced in `splitPane()` and `setPaneContent()`. Hitting the cap shows a toast notification.

**Orphan cleanup**: When an instance is deleted, `pruneInstancePanes()` reassigns any panes referencing that instance to the current global instance (or null → empty state).

**Persistence hardening**: Deserialization validates schema version, content kinds, tree-pane consistency, and clamps split ratios to [0.15, 0.85]. Corrupt state falls back gracefully with `console.warn`.

**Toast notifications**: `stores/toasts.ts` provides `addToast(message, type?, duration?)`. Max 3 visible (FIFO). `ToastStack.svelte` renders fixed bottom-right with slide-up animation.

**Cross-view focus**: `currentInstanceId` is driven one-way from an `effectiveInstanceId` derived in `setupLayoutSync()`. For instance-bound panes (terminal/conversation), this is the pane's `instanceId`. For directory-bound panes (file-explorer/tasks/git), it resolves the pane's `workingDir` to the first matching instance. This ensures file/git/task stores see the correct context when a directory-bound pane is focused. To change the current instance, always use `setFocusedInstance(id)` or `selectInstance(id)` — never write to `currentInstanceId` directly. `setFocusedInstance()` routes through the layout bridge: it finds a pane already showing the instance and focuses it, or binds the focused pane to the new instance (choosing `conversation` vs `terminal` pane kind based on `InstanceKind`). `selectInstance()` has a cross-project guard: if the instance belongs to a different project, it delegates to `switchProject()` (via registered callback) which saves the current layout and loads the target project's layout before focusing. Terminal focus handoff uses per-pane `requestTerminalFocus(paneId)` / `consumeTerminalFocus(paneId)` in `layout.ts`. There is no global `showTerminal` store — `PaneContent` is the single source of truth for what each pane displays, including the `viewMode` on conversation panes.

### Project & Instance Hierarchy

Instances are grouped into **projects** client-side by `working_dir` (`stores/projects.ts`). A `Project` is purely derived — no server changes, no persistence. Projects appear/disappear as instances are created/destroyed.

- **`projects`** — derived store: groups `instanceList` by `working_dir`
- **`currentProject`** — derived from `[projects, activeProjectId]` → find project matching the active layout's project ID
- **`activeProjectId`** — readable store (from `layout.ts`) tracking which project's layout is loaded. Updated by `switchProject()`
- **`projectHash()`** / **`projectStorageKey()`** — pure utilities in `utils/project-id.ts` (shared by `layout.ts` and `projects.ts` to avoid import cycles)

**Sidebar** (`Sidebar.svelte`): 48px vertical icon rail showing project abbreviations (2-letter circles). Active project has amber highlight. Bottom: new instance, theme toggle, avatar/logout. Project clicks call `switchProject()` directly (explicit project switch, not via `selectInstance`).

**MainHeader** (`main-view/MainHeader.svelte`): Project control center with three zones:
- Left: project name + connection status
- Center: `FleetStrip.svelte` — adaptive fleet visualization using `ResizeObserver`. Rendering modes: `detail` (≥200px/cell: icon, name, state+duration, tool), `compact` (80-199px: icon, truncated name, LED), `column` (30-79px: colored bars with attention pips), `aggregate` (<30px: proportional bar + counts). Inbox summary text right-aligned. Expand chevron opens FleetPanel.
- Right: action buttons (layout presets, files, tasks, chat)

**FleetPanel** (`main-view/FleetPanel.svelte`): Expanded fleet control panel (replaces FleetDrawer). Three tiers sorted by attention:
- **Inbox**: items from `stores/inbox.ts` — `needs_input` (respond button), `completed_turn` (review/dismiss), `error` (dismiss). Auto-sorted by priority.
- **Active**: instances currently Thinking/Responding/ToolExecuting/Starting, with state + duration
- **Idle**: collapsible when >4, compact chip grid when collapsed
Search filter, keyboard nav (arrows/Enter/Escape), right-click → `InstancePopover.svelte`. Hidden on mobile.

**Instance state utility** (`utils/instance-state.ts`): `getStateInfo()` extracts display state (label, color, animation) from instance + claude state. Shared by Sidebar rail, FleetStrip, and FleetPanel. `InstanceKind` drives the kind icon (brain for Structured, terminal prompt for Unstructured).

**Inbox store** (`stores/inbox.ts`): Server-side inbox model — `inboxItems` (Map by instance_id), `inboxSorted` (priority-sorted), `inboxCount`. Pure utilities: `getAttentionLevel()` (critical/warning/active/idle/booting), `formatDuration()`. Browser notifications for `needs_input` events. WS messages: `InboxUpdate` (single upsert/delete), `InboxList` (initial load). HTTP: `POST /api/inbox/{id}/dismiss`.

### API Calls

Use `src/lib/utils/api.ts` for all HTTP requests. It handles auth headers, base URL resolution, and error normalization. Do not use `fetch` directly.

### Types

Domain types are defined in `src/lib/types.ts`. Keep them in sync with the Rust `models.rs` types. The JSON serialization from the server is the contract.

**`InstanceKind`** — discriminated union (`{ type: 'Structured'; provider: string } | { type: 'Unstructured'; label?: string | null }`). Computed by the backend at creation time, sent in the wire protocol on every `Instance` payload. Frontend checks `inst.kind.type === 'Structured'` instead of `command.includes('claude')`. The `isClaudeInstance` derived store wraps this check for the current instance.

### Styling

Follow the amber phosphor CRT design system documented in `BRAND_BOOK.md`:
- Use CSS custom properties (`--amber-*`, `--surface-*`, `--text-*`) — never hardcode colors
- Monospace typography only (JetBrains Mono stack)
- Uppercase labels with letter-spacing for industrial feel
- CRT scanlines and glow effects are part of the aesthetic, not decoration

### File Organization

Components with subcomponents live in directories (e.g. `file-explorer/`, `chat-panel/`, `layout/`). Top-level components in `src/lib/components/` are the main views. The `layout/` directory contains the pane tree renderer and all pane wrapper components.

### Formatting

Prettier formats all TS and Svelte files. Config is in `.prettierrc` (spaces, not tabs; single quotes; 120 print width). Run `pnpm format` or `bazel run //tools/format`. CI checks formatting via `bazel test //tools/format:format_test`.

### Testing

Test files sit alongside source: `foo.test.ts` next to `foo.ts`. Run with `pnpm test`.
