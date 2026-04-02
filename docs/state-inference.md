# Claude State Inference System

How workshop detects whether Claude is idle, thinking, responding, executing a
tool, or waiting for user input — and presents that to connected clients.

## Table of Contents

- [Overview](#overview)
- [ClaudeState Enum](#claudestate-enum)
- [Signal Pipeline](#signal-pipeline)
- [Signal Sources](#signal-sources)
- [State Transitions](#state-transitions)
- [Interactive vs Non-Interactive Tools](#interactive-vs-non-interactive-tools)
- [Definitive vs Tentative Idle](#definitive-vs-tentative-idle)
- [Terminal Tool Pattern Detection](#terminal-tool-pattern-detection)
- [Staleness Tracking](#staleness-tracking)
- [Design Decisions](#design-decisions)
- [Known Limitations](#known-limitations)
- [Module Map](#module-map)

---

## Overview

Each Claude instance runs in a PTY. We need to know what it's doing so the UI
can show an activity indicator ("verbing..." / "ready"). Two signal sources feed
a unified state manager:

1. **Conversation JSONL** — Claude Code writes structured entries to a JSONL log
   file on disk. These are the authoritative signals: they tell us exactly when a
   user message was sent, when the model finished a turn, and which tools were
   invoked. Latency: up to 500ms (poll interval).

2. **Terminal output** — The raw PTY output contains spinner patterns like
   `⠋ Read(src/main.rs)` that indicate tool execution. These are heuristic
   signals: faster (~immediate) but can produce false positives. They provide
   sub-100ms feedback between slower JSONL polls.

The state manager merges both into a single `ClaudeState` that is broadcast to
all connected WebSocket clients.

## ClaudeState Enum

Defined in `inference/state.rs`:

```rust
pub enum ClaudeState {
    Idle,                                    // Waiting for user input
    Thinking,                                // User sent input, no output yet
    Responding,                              // Claude is streaming a response
    ToolExecuting { tool: String },          // Claude is running a tool
    WaitingForInput { prompt: Option<String> }, // Claude needs user input
}
```

From the UI's perspective:
- `Idle` and `WaitingForInput` → "ready" (Claude is done, user can act)
- `Thinking`, `Responding`, `ToolExecuting` → "verbing..." (Claude is working)

`WaitingForInput` is distinct from `Idle` because it carries an optional prompt
string and semantically means Claude paused mid-turn (e.g. for a permission
confirmation), whereas `Idle` means no turn is in progress.

## Signal Pipeline

Signals flow through a chain of components before reaching the state manager:

```
JSONL file on disk
  │
  ▼
toolpath-claude ConversationWatcher (polls every 500ms)
  │
  ▼
MergingWatcher (cross-poll tool_result merging)
  │
  ├── WatcherEvent::Turn(turn)
  ├── WatcherEvent::TurnUpdated(turn)
  └── WatcherEvent::Progress { kind, data }
       │
       ▼
  watcher_event_to_signal()          [conversation_watcher.rs:23]
       │
       ▼
  StateSignal::ConversationEntry { entry_type, subtype, stop_reason, tool_names }
       │                                             ┌──────────────────────┐
       │                                             │ StateSignal::        │
       │  PTY raw output ──────────────────────────→ │ TerminalOutput{data} │
       │  PTY raw input  ──────────────────────────→ │ TerminalInput{data}  │
       │  500ms timer    ──────────────────────────→ │ Tick                 │
       │                                             └──────────┬───────────┘
       ▼                                                        ▼
  ┌─────────────────────────────────────────────────────────────────┐
  │  StateManager::process(signal) → Option<ClaudeState>           │
  │                                                [manager.rs:127]│
  └──────────────────────────┬──────────────────────────────────────┘
                             ▼
                   StateUpdate { state, terminal_stale }
                             │
                             ▼
                   mpsc channel → broadcast to WebSocket clients
```

### MergingWatcher

The upstream `toolpath-claude` `ConversationWatcher` only merges tool results
that arrive within a single poll batch. If a `tool_result_only` user entry
appears in a later poll than the assistant entry it belongs to, the upstream
watcher drops it silently. `MergingWatcher` wraps the upstream watcher and
handles cross-poll merges, emitting `TurnUpdated` events when a tool result is
retroactively merged into a previously-seen assistant turn.

This is still necessary as of toolpath-claude 0.6.

### watcher_event_to_signal

This function (`conversation_watcher.rs:23`) converts `WatcherEvent` variants
into `StateSignal::ConversationEntry`:

| WatcherEvent | StateSignal entry_type | Notes |
|---|---|---|
| `Turn(role=User)` | `"user"` | User sent a message |
| `Turn(role=Assistant)` | `"assistant"` | Model completed an API call |
| `Turn(role=System)` | `"system"` | System metadata (e.g. `turn_duration`) |
| `TurnUpdated(_)` | `"user"` | Tool result merged → user answered a prompt |
| `Progress { kind, .. }` | `kind` (passthrough) | Real-time progress entries |

**Key detail — stop_reason inference:** Claude Code JSONL always writes
`stop_reason: null` (the streaming API field is never populated at write time).
The function infers `"end_turn"` for all assistant entries since every assistant
entry represents a completed API call.

**Key detail — tool_names extraction:** For assistant turns, the function
extracts tool names from `turn.tool_uses` into `tool_names: Vec<String>`. This
lets the state manager distinguish interactive tools (AskUserQuestion) from
non-interactive ones (Read, Bash, etc.).

## Signal Sources

### StateSignal Variants

Defined in `inference/state.rs`:

```rust
pub enum StateSignal {
    TerminalOutput { data: String },
    TerminalInput { data: String },
    ConversationEntry {
        entry_type: String,          // "user", "assistant", "system"
        subtype: Option<String>,     // e.g. "turn_duration"
        stop_reason: Option<String>, // e.g. "end_turn" (inferred)
        tool_names: Vec<String>,     // e.g. ["AskUserQuestion"]
    },
    Tick,
}
```

### What Each Signal Does

| Signal | Triggers State Change? | Purpose |
|---|---|---|
| `ConversationEntry(user)` | **Yes** → `Thinking` | Authoritative: user submitted a message |
| `ConversationEntry(system, turn_duration)` | **Yes** → `WaitingForInput` (definitive) | Authoritative: turn is over |
| `ConversationEntry(assistant, no tools)` | **Deferred** → `WaitingForInput` (non-definitive) | Text-only — held one poll cycle; suppressed if more entries follow |
| `ConversationEntry(assistant, interactive tool)` | **Yes** → `WaitingForInput` (non-definitive) | Claude is asking the user something |
| `ConversationEntry(assistant, non-interactive tools)` | **No** | Mid-turn — prevents flickering between tool calls |
| `TerminalOutput` (with tool pattern) | **Yes** → `ToolExecuting` | Fast heuristic feedback (unless definitive idle) |
| `TerminalOutput` (plain text, from Thinking) | **Yes** → `Responding` | First output after user message |
| `TerminalOutput` (plain text, other states) | **No** | Already responding or executing |
| `TerminalInput` | **No** | Fires on every keystroke — never changes state |
| `Tick` | **No** | Staleness tracking only |

## State Transitions

### State Machine Diagram

```
                        user ConversationEntry
   Idle ──────────────────────────────────────────→ Thinking
     ↑                                                │
     │                                                │ TerminalOutput (plain text)
     │                                                ↓
     │  turn_duration               ┌──────────── Responding
     │  (definitive)                │                 │
     │        │                     │                 │ TerminalOutput (tool pattern)
     │        ▼                     │                 ▼
     │  WaitingForInput ◄───────────┤          ToolExecuting
     │  (definitive — sticky)       │                 │
     │                              │                 │ TerminalOutput (different tool)
     │                              │                 ▼
     │  WaitingForInput ◄───────────┘          ToolExecuting (new tool)
     │  (non-definitive,
     │   from interactive tool)
     │        │
     │        │ TerminalOutput (tool pattern) ← CAN override non-definitive
     │        ▼
     │  ToolExecuting
     │
     └──── turn_duration from ANY state
```

### Transition Rules

**ConversationEntry signals (authoritative):**

1. `entry_type == "system"` + `subtype == "turn_duration"`:
   - From ANY state → `WaitingForInput { prompt: None }`
   - Sets `definitive_idle = true`
   - Clears `current_tool`
   - This is the only definitive end-of-turn signal

2. `entry_type == "user"`:
   - From ANY state → `Thinking`
   - Sets `definitive_idle = false`
   - Indicates the user submitted a message (or answered a tool prompt)

3. `entry_type == "assistant"` + interactive tool (AskUserQuestion, etc.):
   - → `WaitingForInput { prompt: None }` (tentative, not definitive)
   - Sets `definitive_idle = false`
   - Clears `current_tool`

4. `entry_type == "assistant"` + no tool uses (`tool_names` empty):
   - **Deferred** — the signal is held for one poll cycle by the conversation
     watcher. If the next poll brings more entries, the signal is suppressed
     (it was mid-turn text). If the next poll is empty, the signal is sent →
     `WaitingForInput` (tentative). See [mid-turn text handling](#why-text-only-assistant-signals-are-deferred).

5. `entry_type == "assistant"` + non-interactive tools only:
   - **No state change** (this is critical — see [flickering prevention](#why-non-interactive-tool-entries-dont-change-state))

**TerminalOutput signals (heuristic):**

5. Tool pattern detected (e.g. `Read(`, `Bash(`):
   - If `definitive_idle` → **ignored** (false positive protection)
   - Otherwise → `ToolExecuting { tool }`
   - Sets `definitive_idle = false`

6. Plain text (no tool pattern), current state is `Thinking`:
   - → `Responding` (first output after user message)

7. Plain text, any other state:
   - **No state change**

**TerminalInput signals:**

8. Always: **No state change**
   - Only sets `sent_idle = false` (bookkeeping)
   - Fires on every keystroke — arrow keys, menu navigation, typing

**Tick signals:**

9. Always: **No state change**
   - Used only for terminal staleness tracking

## Interactive vs Non-Interactive Tools

The state manager distinguishes tools that require user input from tools that
execute automatically:

```rust
fn is_interactive_tool(name: &str) -> bool {
    matches!(name, "AskUserQuestion" | "EnterPlanMode" | "ExitPlanMode")
}
```

**Interactive tools** pause Claude's execution and wait for the user to respond.
When the state manager sees an assistant entry with an interactive tool, it
transitions to `WaitingForInput` so the UI shows "ready."

**Non-interactive tools** (Read, Write, Edit, Bash, Glob, Grep, Task, etc.)
execute automatically without user involvement. When an assistant entry contains
only non-interactive tool uses, the state manager does NOT change state — this is
a mid-turn signal (tool results will follow, then another API call). Terminal
heuristics handle the `ToolExecuting` display.

**No tools** (text-only assistant entries) can mean the model finished
responding OR can be mid-turn explanatory text (see [claude-jsonl-protocol.md
§3](claude-jsonl-protocol.md#3-assistant-entries-are-written-per-content-block-not-per-api-call)).
The signal is **deferred** for one poll cycle before being applied.

### Why This Three-Way Split Matters

- **AskUserQuestion** → `WaitingForInput`: Claude is waiting for the user to
  select an option. Without this, the UI would show "verbing..." while idle.
- **Read/Bash/etc.** → no state change: The agentic loop continues. Without
  this, the UI would briefly flash "ready" between each tool call in a
  10-tool sequence — a distracting flickering effect.
- **Text-only** → deferred `WaitingForInput`: May be turn-ending OR mid-turn.
  Held for one poll cycle to prevent green flashes from mid-turn text. If no
  more entries arrive, applied as tentative WaitingForInput.

## Definitive vs Tentative Idle

The `definitive_idle` flag on `StateManager` controls whether terminal heuristics
can override a `WaitingForInput` state.

### Definitive Idle

Set by: `turn_duration` system entry (the only source). Note: `turn_duration` is
not always present — see [claude-jsonl-protocol.md
§2](claude-jsonl-protocol.md#2-turn_duration-is-not-always-present).

Behavior: Terminal output with tool patterns (e.g. `Read(`) is **ignored**. This
prevents false positives when Claude's response text mentions tool names after
the turn is over (e.g. "I used Read(file.rs) to check the contents").

Cleared by: A new `user` ConversationEntry (the next turn starts).

### Non-Definitive Idle

Set by: Assistant entry with an interactive tool (e.g. `AskUserQuestion`).

Behavior: Terminal output with tool patterns **can** override it back to
`ToolExecuting`. This shouldn't happen in practice for interactive tools, but
the non-definitive flag keeps the system flexible.

### Why Not Just Use One Flag?

The turn might not be over when Claude asks a question. Consider:

1. Claude calls `AskUserQuestion` → `WaitingForInput` (non-definitive)
2. User answers → `user` signal → `Thinking`
3. Claude continues working → tool patterns → `ToolExecuting`
4. Claude finishes → `turn_duration` → `WaitingForInput` (definitive)

If step 1 set `definitive_idle = true`, step 3's tool patterns would be
suppressed, and the UI would show "ready" while Claude is actively working.

## Terminal Tool Pattern Detection

The state manager matches terminal output against a list of patterns:

```rust
const TOOL_PATTERNS_V1: &[(&str, &str)] = &[
    ("NotebookEdit(", "NotebookEdit"),  // Before Edit (more specific)
    ("TodoRead(", "TodoRead"),          // Before Read (more specific)
    ("TodoWrite(", "TodoWrite"),        // Before Write (more specific)
    ("WebFetch(", "WebFetch"),          // Before generic patterns
    ("WebSearch(", "WebSearch"),
    ("AskUserQuestion(", "AskUserQuestion"),
    ("Task(", "Task"),
    ("Read(", "Read"),
    ("Write(", "Write"),
    ("Edit(", "Edit"),
    ("Glob(", "Glob"),
    ("Grep(", "Grep"),
    ("Bash(", "Bash"),
];
```

**Ordering matters.** More specific patterns come first so that `NotebookEdit(`
matches before `Edit(`, and `TodoRead(` matches before `Read(`.

**Detection is substring-based** — `output.contains(pattern)`. This means
patterns can match inside regular text (known false positive). The definitive
idle mechanism prevents this from causing incorrect state transitions after a
turn is complete.

**Patterns match Claude CLI's spinner format:** During tool execution, the CLI
outputs lines like `⠋ Read(src/main.rs)` with a rotating spinner character.

## Staleness Tracking

Separate from state, the manager tracks whether its signals are "fresh":

- **Terminal staleness** (`is_terminal_stale()`): True when no `TerminalOutput`
  signal has arrived within `idle_timeout` (default 10s). This indicates the PTY
  might be inactive or the instance might have exited.

- **Conversation staleness** (`is_conversation_stale()`): True when no
  `ConversationEntry` signal has arrived within `idle_timeout`.

The `StateUpdate` struct includes a `terminal_stale: bool` flag alongside the
state itself. The UI can use this to indicate lower confidence in the displayed
state (e.g. during extended thinking where Claude produces no terminal output
but is still working).

Staleness does NOT trigger state transitions. Only authoritative JSONL signals
and terminal heuristics change the state.

## Design Decisions

### Why TerminalInput Never Changes State

Terminal input fires on **every keystroke**: typing characters, pressing arrow
keys, navigating tool confirmation menus, scrolling. It's impossible to
distinguish "user submitted a message" from "user pressed the down arrow" at the
PTY level.

Previous iterations tried using `TerminalInput` for `Idle → Thinking` but this
caused the UI to show "verbing..." whenever the user moved the cursor or browsed
a selection menu.

The authoritative signal for "user submitted a message" is the JSONL `user`
entry, which only appears after Claude actually receives and logs the message.

### Why Non-Interactive Tool Entries Don't Change State

During an agentic turn, Claude makes many API calls in sequence:

```
assistant (Read file1) → user (tool_result) → assistant (Read file2) → user (tool_result) → ...
```

Each assistant entry with tool uses represents one completed API call where more
work is expected (tool results will come back, then another API call). If each
one set `WaitingForInput`, the UI would rapidly flicker between "ready" and
"verbing..." as terminal heuristics detect the next tool:

```
ToolExecuting(Read) → WaitingForInput → ToolExecuting(Read) → WaitingForInput → ...
                      ^^^ 50ms flicker                        ^^^ 50ms flicker
```

The fix: assistant entries with **only non-interactive tools** cause **no state
change**. The state stays at whatever the terminal heuristics set it to (usually
`ToolExecuting` or `Responding`).

Assistant entries with **no tool uses** (text-only) are handled separately — see
the next section.

### Why Text-Only Assistant Signals Are Deferred

Claude Code writes separate JSONL entries for thinking, text, and tool_use
content blocks within a single API response. This means text-only assistant
entries can appear **mid-turn** as explanatory text between tool batches:

```
assistant  |                              ← thinking (empty, text-only)
assistant  | Let me find some files...    ← explanatory text (text-only)
assistant  tools=['Glob'] |               ← tool_use (has tools)
...
assistant  | Now let me read the files... ← explanatory text (text-only)
assistant  tools=['Read'] |               ← tool_use
...
assistant  | Here are the results...      ← final response (text-only)
```

If text-only entries immediately set `WaitingForInput`, the UI would flash
green between tool batches — every time Claude writes an explanatory message.

However, text-only entries are also the primary end-of-turn signal when
`turn_duration` is absent (which is common in short sessions). So we can't
ignore them entirely.

The fix: the **conversation watcher defers** text-only assistant signals by
one poll cycle (500ms). If the next poll brings more entries, the text was
mid-turn — the held signal is discarded. If the next poll is empty, the text
was turn-ending — the signal is sent to the state manager.

This adds at most 500ms delay to showing "ready" at end of turn, which is
imperceptible to users and eliminates green flashes entirely.

### Why TurnUpdated Emits a User Signal

When Claude calls a tool that needs user input (like a Bash command needing
permission), the flow is:

1. Assistant entry (tool_use) → JSONL
2. User types "y" or selects an option → terminal input
3. A `tool_result_only` user entry appears in JSONL

The upstream watcher may deliver the user entry in a later poll than the
assistant entry. `MergingWatcher` merges it into the assistant turn and emits
`TurnUpdated`. Without converting this to a `user` signal, the state manager
would never see the "user answered" event, and the state would stay at
`WaitingForInput` until the next assistant entry.

### Why Tick Doesn't Change State

Earlier designs used tick-based timeouts to fall back to `Idle` after N seconds
of no activity. This caused problems during extended thinking (Claude working
for 30+ seconds without terminal output) — the UI would incorrectly show "ready"
while Claude was still processing.

Since `turn_duration` reliably signals turn completion, tick-based transitions
were removed entirely. Tick now only updates staleness tracking.

## Known Limitations

1. **Tool pattern false positives:** If Claude's response text mentions a tool
   name followed by `(` (e.g. "I used Read(file) to check"), the terminal
   heuristic will detect it as tool execution. Mitigated by definitive idle —
   after `turn_duration`, these false positives are ignored.

2. **500ms JSONL latency:** State transitions from conversation signals can lag
   up to 500ms behind real-time. Terminal heuristics partially bridge this gap
   but only for tool detection, not for turn completion.

3. **Spinner format dependency:** Tool patterns depend on Claude CLI's specific
   output format (`ToolName(args)`). If the CLI changes its spinner format,
   patterns need updating. The `TOOL_PATTERNS_V1` constant is versioned for
   this reason.

4. **No pattern for Skill/Invoke tools:** Custom MCP tools and skills don't
   have hardcoded patterns. They'll show as `Responding` rather than
   `ToolExecuting` unless the CLI uses a matching format.

5. **Extended thinking appearance:** During extended thinking (Claude processing
   for 30+ seconds), there's no terminal output. The state stays at `Thinking`
   or `Responding`, which is correct but the `terminal_stale` flag will be set,
   indicating lower confidence.

## Module Map

```
inference/
├── mod.rs       Re-exports: ClaudeState, StateSignal, StateUpdate,
│                spawn_state_manager, StateManagerConfig
│
├── state.rs     Type definitions:
│                  - ClaudeState enum (Idle/Thinking/Responding/ToolExecuting/WaitingForInput)
│                  - StateSignal enum (TerminalInput/TerminalOutput/ConversationEntry/Tick)
│                  - StateUpdate struct (state + terminal_stale)
│                  - StateEvent enum (for the legacy engine)
│
├── manager.rs   The unified state manager:
│                  - StateManager struct (processes signals, maintains state)
│                  - TOOL_PATTERNS_V1 (terminal pattern matching)
│                  - spawn_state_manager() (tokio task: signal_rx → state_tx)
│                  - is_interactive_tool() (AskUserQuestion/EnterPlanMode/ExitPlanMode)
│
└── engine.rs    Legacy standalone inferrer (dead code, kept for reference):
│                  - StateInferrer: input/output/tick-based state machine
│                  - Operates on raw PTY I/O only, no JSONL integration
│                  - Predates the unified state manager design
```

### Related Files

- `ws/conversation_watcher.rs` — `watcher_event_to_signal()` converts
  `WatcherEvent` from the JSONL poller into `StateSignal::ConversationEntry`
- `ws/merging_watcher.rs` — Cross-poll tool result merging wrapper
- `instance_actor.rs` — Spawns the state manager and feeds terminal I/O signals
- `docs/claude-jsonl-protocol.md` — JSONL format reference (covers the raw
  protocol that feeds into this system)
