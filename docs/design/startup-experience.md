# Startup Experience: The Arrival

## Problem

When Claude starts up slowly, Workshop looks broken. The TUI shows a blank terminal. The web UI shows an empty conversation view. There's no indication that anything is happening, no hint about what to do, and no way to productively use the wait time. Messages typed during startup are silently lost or ignored.

The root cause is a state gap: the `ClaudeState` enum jumps straight to `Idle`, but "Idle" means "ready and waiting for input." During startup, the instance is *not* idle — it's booting. This state is real but unrepresentable in the model, so it's invisible in every view.

## Design Principle

S-tier startup doesn't feel like waiting. It feels like arriving.

Every piece of this design says the same thing: **the system is in control, and so are you.** The progress phases say "we know what's happening." The hot input says "your time matters." The task queue says "your intent is preserved." The time-aware messaging says "we're honest about what's slow." The seamless transition says "this was always one experience, not two."

## Architecture

### Layer 1: Model the State

Add a `Starting` variant to `ClaudeState`:

```rust
// packages/workshop/src/inference/state.rs
pub enum ClaudeState {
    Starting,  // NEW: PTY spawned, Claude not yet at prompt
    Idle,
    Thinking,
    Responding,
    ToolExecuting { tool: String },
    WaitingForInput { prompt: Option<String> },
}
```

**Why a new variant, not a boolean?** Because `ClaudeState` is the single source of truth for "what is Claude doing right now?" A boolean `is_starting` would invite impossible states (`is_starting: true, state: Thinking`). The enum makes illegal states unrepresentable.

**Lifecycle:**
- Instance creation sets `claude_state: Some(ClaudeState::Starting)` (not `None`)
- First Claude prompt detection OR first `ConversationEntry` triggers transition to `Idle`
- Everything else follows the existing state machine

**Transition trigger:** The inference manager already watches terminal output via `StateSignal::TerminalOutput`. Add a heuristic: if the instance has `Starting` state and we see Claude prompt patterns or an assistant conversation entry, transition to `Idle`. The existing signal path handles this naturally.

**Files:**
- `packages/workshop/src/inference/state.rs` — add variant, update `is_active()`, `Default` (keep as `Idle`, set `Starting` explicitly at creation)
- `packages/workshop/src/instance_actor.rs` — set initial `claude_state: Some(ClaudeState::Starting)`
- `packages/workshop/src/inference/manager.rs` — add `Starting` → `Idle` transition on first prompt/output
- `packages/workshop/src/ws/protocol.rs` — serde tests for new variant
- `packages/workshop_ui/src/lib/types.ts` — add `{ type: 'Starting' }` to `ClaudeState` union

### Layer 2: Phased Progress (Web)

The frontend infers boot phases from existing signals — no enum bloat needed:

```
[✓] PROCESS STARTED          ← instance.running === true
[·] WAITING FOR CLAUDE...     ← running + Starting + no terminal output
[ ] SESSION LINKED            ← session_id still undefined
[ ] READY                     ← transition to Idle
```

Each line appears as its milestone is reached. The visual shifts from "is it broken?" to "I can see where it is."

**ConversationView** (`packages/workshop_ui/src/lib/components/ConversationView.svelte`): Replace the empty-state "Start a conversation" message with a boot progress panel when `claude_state?.type === 'Starting'`. Styled per the CRT brand book — amber phosphor, uppercase monospace, adapting the aesthetic that `BootSequence.svelte` already established. But this one is *real*, not cosmetic. Each checkpoint lights up as it's reached.

**Sidebar** (`packages/workshop_ui/src/lib/components/sidebar/InstanceItem.svelte`): The `stateInfo` label shows `STARTING` with a slow rhythmic pulse (distinct from the fast flicker of active work). Other team members see "this instance is starting up" — ambient awareness, no action needed.

**MainHeader** (`packages/workshop_ui/src/lib/components/main-view/MainHeader.svelte`): The baud panel shows `BOOT` verb during `Starting` state with the baud meter in a slow pulse pattern.

### Layer 3: The Input Bar Stays Hot

**Never disable the input.** The moment you gray out a text field, you've told the user "you can't do anything."

- Input stays fully active during `Starting`
- A subtle banner above the input: `CLAUDE IS STARTING UP — YOUR MESSAGE WILL BE SENT WHEN READY`
- As you type and submit, messages appear in a **queued section** — visible, editable, cancellable
- When Claude reaches `Idle`, queued messages flush automatically
- If you submit multiple messages during startup, they send in order

This turns dead wait time into **planning time**. You're not waiting for Claude — you're *briefing* Claude.

**File:** `packages/workshop_ui/src/lib/components/MessageInput.svelte`

### Layer 4: Messages Queue as Tasks

Messages submitted during `Starting` create **real `Task` records** via the existing task API, not just volatile `InstanceState.pending` entries:

- Task title = first ~80 chars of the message
- Task body = full message text
- Task status = `pending`
- Task instance_id = the starting instance

This means:
- **They survive page refreshes.** Close the tab, come back, your queued work is still there.
- **They're visible in the task board.** Other team members can see what's queued.
- **They have full task affordances** — edit, cancel, reprioritize, reassign to a different instance.
- When Claude reaches `Idle`, tasks dispatch in `sort_order` using the existing `TaskDispatch` machinery.

**Files:**
- `packages/workshop_ui/src/lib/stores/ws-handlers.ts` — on `StateChange` to `Idle`, flush pending tasks
- `packages/workshop_ui/src/lib/stores/instances.ts` — wire task creation on submit during `Starting`
- Existing task API (`handlers/tasks.rs`, `repository/tasks.rs`) — no changes needed

### Layer 5: Time-Aware Messaging

Static text gets stale. The overlay should be time-aware:

| Elapsed | Message | Why |
|---------|---------|-----|
| 0–5s | `Waiting for Claude to start...` | Normal, expected |
| 5–15s | `Claude is taking a moment to initialize...` | Small nudge |
| 15–30s | `Startup is slower than usual — this may be due to network latency or API load` | Honest about what's happening |
| 30s+ | `Claude is taking unusually long. You can switch to another instance or keep waiting.` | Offer an escape hatch |

This isn't a countdown. It's **progressive disclosure of context**. You only show the network/API explanation when it's actually relevant.

Both the TUI overlay and the web boot panel implement this — driven by `created_at` timestamp vs current time, no extra server signals needed.

### Layer 6: TUI Status Bar

The current TUI attach overlay is a 5-second badge that disappears. For startup status, we need something persistent without being intrusive.

**A single-line status bar at the bottom of the terminal** (compositor bottom-anchor layer):

```
 STARTING │ Waiting for Claude... │ Ctrl-] switch │ Type to queue a message
```

Then when ready:

```
 READY │ Claude is idle │ Ctrl-] detach
```

This is the vim statusline pattern. Always present, always informative, never in the way. It frames whatever's happening in the terminal — the raw Claude boot output above the bar goes from "what is this noise?" to "oh, that's Claude starting up, I can see the status bar says STARTING."

When state transitions to `Idle`, the bar updates with a brief reverse-video flash to draw attention to the change, then settles into the steady `READY` state.

The status bar subscribes to `StateChange` messages via the instance WebSocket — the attach loop already has a `select!` handling WebSocket frames.

**Files:**
- `packages/workshop/src/cli/attach.rs` — add status bar overlay, subscribe to state changes
- `packages/workshop/src/cli/terminal.rs` — extend `TerminalGuard` with status bar support
- Uses existing compositor `Anchor::BottomLeft` + `Attrs` with `REVERSED`/`BOLD` modifiers (solarized-safe)

### Layer 7: The Seamless Transition

The boot overlay shouldn't just disappear. It should **become** the conversation view.

When Claude reaches `Idle` and the first conversation turn arrives:
- The boot progress checkmarks all complete (✓)
- A brief `READY` state holds for ~500ms
- The boot panel fades as the conversation view appears below it
- If messages were queued, they appear as the first user turn, and Claude's response streams in naturally

No jarring state change. No flash of empty content. The boot experience *becomes* the conversation experience through a continuous visual flow.

## Implementation Order

Each layer is independently shippable and improves the experience incrementally:

1. **Model** — Add `Starting` variant to `ClaudeState` (Rust + TypeScript), set on creation, transition on first prompt detection. All existing serde, broadcast, and state derivation works unchanged.
2. **TUI status bar** — Bottom-line overlay in attach mode, state-aware text, Ctrl-] hint.
3. **Web boot panel** — Replace empty ConversationView state with phased progress when `Starting`.
4. **Sidebar + header** — `STARTING` label with pulse, `BOOT` verb in baud panel.
5. **Input queueing** — Keep input hot, banner during `Starting`, task creation for queued messages.
6. **Time-aware messaging** — Progressive disclosure based on elapsed time.
7. **Seamless transition** — Animate boot → conversation handoff.
8. **Tests** — State transition tests, serde roundtrip for `Starting`, frontend store tests.

## What This Avoids

- **No polling/timer-based "are we ready yet" checks.** The state machine transitions on real signals. Timers create race conditions.
- **No separate `is_booting` boolean alongside `ClaudeState`.** That's asking for impossible combinations. One enum, one state, always.
- **No blocking input during startup.** That feels broken in a different way. Let users type, queue it, send when ready. Responsive > correct-but-frozen.
- **No fake progress bars.** Every phase shown reflects a real system milestone. Honest > theatrical.
