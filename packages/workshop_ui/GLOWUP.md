# Workshop → S-Tier: The Glowup Plan

*Deepen the metaphor. Don't broaden the palette.*

---

## Guiding Principles

1. **Every change must make the control room feel more real**, not more "designed"
2. **The conversation view is the product** — 90% of time is spent here, 90% of effort goes here
3. **State transitions are the soul** — Claude thinking vs responding vs executing should feel like different *modes of operation*, not different colored labels
4. **Restraint is conviction** — if a change could belong to any app, it doesn't belong here
5. **Performance is aesthetic** — a 60fps glow is beautiful; a janky one breaks the spell

---

## Phase 1: Phosphor Persistence

*Make the conversation feel like a living CRT display, not a message list.*

### 1.1 Temporal Fade on Messages

Real phosphor decays. Recent messages should burn bright; older ones should cool toward `--text-secondary`. This creates a natural visual hierarchy without adding any UI chrome.

**Implementation:**
- In `NotebookCell.svelte`, accept an `age` prop (index distance from newest message)
- Apply a CSS custom property `--cell-brightness` that decreases with age
- Map it to `opacity` on the cell content (not the whole cell — keep structure visible)
- Range: `1.0` (newest) → `0.7` (oldest visible), with a floor so nothing disappears
- The active/streaming message always burns at full brightness
- When the user scrolls up to older messages, those messages "warm up" slightly on viewport entry (a 0.3s transition to a slightly higher brightness) — like the phosphor re-exciting when the beam passes over it

**Files:** `NotebookCell.svelte`, `ConversationView.svelte` (pass age), `VirtualList.svelte` (index tracking)

**CSS sketch:**
```css
.cell {
  --cell-brightness: 1;
  opacity: var(--cell-brightness);
  transition: opacity 0.4s ease;
}
```

### 1.2 Code Blocks as Embedded Screens

Code blocks currently look like styled `<pre>` tags. They should feel like a *second monitor* embedded in the conversation — a terminal-within-a-terminal.

**Implementation:**
- Add a subtle inner scanline overlay to `<pre>` blocks (lighter than the global one, maybe 1px repeat with 0.04 opacity)
- Add a very faint inner glow: `box-shadow: inset 0 0 30px rgba(251, 146, 60, 0.04)`
- The existing line numbers become part of the embedded screen's "chrome"
- On hover, the inner glow intensifies slightly (0.04 → 0.08) — the screen "brightens" when you look at it
- Add a tiny bezel effect: 1px solid dark border with 1px inset highlight at top

**Files:** `+layout.svelte` (global pre/code styles), `NotebookCell.svelte`

### 1.3 User Messages: The Input Card

User messages currently have a muted left border. They should feel like *data entry cards* — the thing you typed into the terminal. Slightly different texture from assistant messages.

**Implementation:**
- Replace the left border with a subtle top-left corner marker (like `> ` on a terminal prompt)
- Add a very faint horizontal rule below user messages (a 1px `--surface-border` line spanning 60% width, centered)
- The user cell background gets a barely-there noise texture (CSS `background-image` with a tiny inline SVG or a repeated 2x2 pixel pattern) — like the slight grain on a different part of the screen

**Files:** `NotebookCell.svelte`

---

## Phase 2: Ambient State

*The whole room should change when Claude's mode changes.*

### 2.1 Global Ambient Color Shift

When Claude enters thinking mode, the entire UI should drift to a cooler, subtly purple-tinted ambient. When responding, warm amber. When executing tools, a slightly more "alert" tone. This is the single highest-impact visual change possible.

**Implementation:**
- Add CSS custom properties for ambient state on `:root`:
  ```css
  --ambient-glow: var(--amber-glow);
  --ambient-accent: var(--amber-500);
  --ambient-tint: rgba(251, 146, 60, 0.02);
  ```
- In `+layout.svelte`, bind Claude's state to a `data-claude-state` attribute on `<body>` (or a top-level wrapper)
- Define state-specific overrides:
  - `[data-claude-state="thinking"]` → purple ambient vars
  - `[data-claude-state="responding"]` → warm amber (default)
  - `[data-claude-state="tool_executing"]` → slightly brighter amber, maybe a hint of yellow
  - `[data-claude-state="idle"]` → dimmer, cooler amber
- All ambient variables transition with `transition: all 0.8s ease` — the *drift*, not a hard cut
- Elements that already use `--amber-glow` for text-shadow pick up the ambient shift for free
- The CRT scanline overlay could shift opacity very slightly with state (0.08 idle → 0.06 thinking → 0.10 active) — the screen literally brightens and dims

**Where the ambient shows up:**
- Text shadows on headings and labels (they shift color)
- The sidebar's edge glow
- The body background (barely perceptible tint)
- The message input border glow
- The scrollbar thumb hover color

**Files:** `+layout.svelte`, `+page.svelte` (or whichever component knows Claude state), stores that expose state

### 2.2 Activity Indicator Upgrade

The current activity indicator in the conversation view is functional but could be more *theatrical*. When Claude starts thinking, there should be a moment of drama.

**Implementation:**
- Add a brief "power-on" flicker when transitioning from idle → thinking (CSS keyframe: opacity 0 → 1 → 0.7 → 1 over 0.3s)
- The activity indicator's glow should pulse in sync with the baud meter's LED — creating visual coherence between header and content area
- The verb text (`analyzing...`, `planning...`) should have a typewriter-reveal effect: characters appear one at a time, left to right, with a block cursor. 4-5 chars at a time, fast. This is pure CRT flavor.
- When transitioning between verbs, the old text should "dissolve" (rapid opacity flicker) before the new text types in

**Files:** `ConversationView.svelte` (activity indicator), `activity.ts` (verb store)

---

## Phase 3: Spatial Composition

*Give the conversation room to breathe and structure to communicate.*

### 3.1 Tool Execution Nesting

When Claude executes tools, the tool badges currently sit as flat pills below the message. They should feel like *sub-operations* — indented, grouped, with a visual connection to the parent message.

**Implementation:**
- Tool cells get a left-margin indent (16px) with a thin vertical connector line from the parent message
- The connector line is `1px solid var(--surface-border)` with a small dot at the junction point
- Multiple sequential tool calls on the same turn are grouped under a single connector
- File operation tools (Read, Edit, Write) get a slightly different treatment: they show the filename (not just the tool name) as a truncated label, since that's the information you actually care about
- Collapsed by default when there are more than 4 tools in a turn — show "N tool operations" with expand toggle

**Files:** `NotebookCell.svelte`

### 3.2 Conversation Rhythm

Long conversations currently feel like a wall of uniform cells. Add visual rhythm through subtle size and spacing variation.

**Implementation:**
- User messages get slightly more top padding (24px instead of 16px) — they're "new thoughts," they deserve a breath
- After a long assistant message (>500 chars), add extra bottom spacing (8px) — let it land
- System messages and progress entries are visually de-emphasized: smaller, tighter, more like log lines than messages
- Add a faint timestamp divider when messages span different hours: a thin centered line with the time in the middle, styled like the stats bar labels (10px, uppercase, muted)

**Files:** `NotebookCell.svelte`, `ConversationView.svelte`

### 3.3 Thinking Section: The Purple Zone

The thinking section currently looks like a collapsible box. It should feel like peering into Claude's *internal display* — a different screen entirely.

**Implementation:**
- When expanded, the thinking section gets its own scanline treatment (purple-tinted, slightly denser than the main overlay)
- The background shifts to a deeper, cooler tone — almost like looking at a different monitor
- The text renders in a slightly smaller size (10px instead of 11px) and tighter line-height — it's *internal*, dense, stream-of-consciousness
- Add a subtle CRT curvature effect on the corners (very slight `border-radius` with an inner shadow that suggests barrel distortion — 2-3px radius with `inset 0 0 20px rgba(0,0,0,0.3)`)
- The expand/collapse animation should feel like a screen powering on/off: quick brightness flash, then content fades in

**Files:** `NotebookCell.svelte`

---

## Phase 4: The Control Room Details

*These are the details that separate "good" from "someone clearly cared."*

### 4.1 Instance Switching: Channel Change

When you switch between instances, there should be a brief "channel change" effect — a 100-200ms flash of static/noise, then the new instance's conversation resolves.

**Implementation:**
- A full-viewport overlay that triggers on instance switch
- Frame 1 (0-50ms): Random noise pattern (CSS background with tiny SVG noise)
- Frame 2 (50-100ms): Horizontal lines (like a CRT losing signal)
- Frame 3 (100-200ms): Fade to the new content
- Keep it fast. This is flavor, not friction. If it takes longer than 200ms, cut it.
- Respect `prefers-reduced-motion` — just crossfade for those users

**Files:** `+page.svelte` (or a new `ChannelChange.svelte` overlay component)

### 4.2 Boot Sequence

On first load (not on navigation, just on cold start), show a brief "boot" sequence instead of a loading spinner.

**Implementation:**
- 1-2 seconds max, non-blocking (content loads behind it)
- Text appearing line by line, fast:
  ```
  WORKSHOP v1.0
  INITIALIZING PHOSPHOR DISPLAY...
  CONNECTING TO INSTANCE MANAGER...
  WEBSOCKET LINK ESTABLISHED
  READY.
  ```
- Each line appears with a slight delay (80-120ms) and the cursor blinks at the end
- The "READY." line triggers the overlay to fade out
- Store a flag in `sessionStorage` so it only plays once per browser session
- If the WebSocket connects before the sequence finishes, the sequence speeds up to catch up

**Files:** New `BootSequence.svelte` component, `+layout.svelte` or `+page.svelte`

### 4.3 Sound Design (Optional, Off by Default)

A subtle audio layer for users who opt in. This is the nuclear charm option.

**Implementation:**
- Web Audio API, no external files — generate tones programmatically
- **Keystroke:** Barely audible click on message input (sine wave, 2000Hz, 10ms, very low gain)
- **Send:** Short ascending two-tone beep (like confirming a command)
- **Response start:** Soft low hum/tone that fades in (100Hz sine, very low, held for duration of response)
- **Response end:** Gentle double-chime (two sine waves, 800Hz then 1000Hz, 50ms each)
- **Thinking start:** Lower, slower pulse (60Hz, pulsing at 0.5Hz) — feels like machinery spinning up
- **Error:** Single descending tone
- All sounds respect system volume. All are < 100ms except the sustained response hum.
- Toggle in account settings or a persistent UI control
- Sound engine lives in a store/utility: `$lib/utils/audio.ts`

**Files:** New `$lib/utils/audio.ts`, account settings UI

### 4.4 Keyboard Navigation

Power users should be able to navigate entirely by keyboard — this is a *terminal app*, after all.

**Implementation:**
- `j`/`k` to move between messages (vim-style), with a visible focus ring (amber glow outline)
- `Enter` on a focused message expands its thinking section or tool details
- `Escape` to unfocus / close overlays
- `1-9` to switch between instances (when not focused on input)
- `/` to focus the message input (like vim command mode)
- `f` to toggle file explorer
- `t` to toggle terminal
- All shortcuts disabled when message input is focused (so you can actually type)
- Show a keyboard shortcut hint on first visit (dismissable, stored in localStorage)

**Files:** `+page.svelte` (global keydown handler), `ConversationView.svelte` (message focus), `VirtualList.svelte` (scroll-to-focused)

---

## Phase 5: Polish & Performance

*The boring stuff that makes everything else feel good.*

### 5.1 Scroll Behavior

- Auto-scroll should be *smooth* when near the bottom (within 200px) and *instant* when jumping to bottom from far away
- When the user scrolls up, show a subtle "new messages below" indicator at the bottom — a small amber bar with a down arrow, pulsing gently
- Clicking the indicator smooth-scrolls to bottom
- The indicator should count new messages: "3 new" with the count glowing

**Files:** `VirtualList.svelte`, `ConversationView.svelte`

### 5.2 Error & Disconnect States

When the WebSocket disconnects, instead of a banner, the whole UI should feel like it's "losing signal":
- Scanline intensity increases (0.08 → 0.15)
- Text glow dims
- A horizontal noise bar sweeps across the screen periodically
- "SIGNAL LOST" label appears in the baud meter position
- When reconnecting, the signal comes back: scanlines normalize, a brief flash of full brightness, "LINK RESTORED"

**Files:** `MainView.svelte`, `+layout.svelte`, `websocket.ts`

### 5.3 Minimap: Make It Earn Its Space

The D3 contour minimap is cool but needs to be *useful*. Currently it's decorative.

**Options (pick one):**
1. **Make it interactive:** Click to scroll, show current viewport as a highlight band, color-code user vs assistant vs tool cells. Basically a scrollbar with personality.
2. **Replace with a phase bar:** A thin vertical strip showing conversation phases (user input → thinking → response → tools → user input) as colored segments. Clickable. More information-dense than the contour map.
3. **Kill it.** If it's not pulling its weight, remove it. An empty space is better than a confusing decoration.

My recommendation: **Option 1** with a fallback to **Option 3** if it doesn't feel right after implementation. The contour visualization is interesting but the utility needs to match the ambition.

**Files:** `ConversationMinimap.svelte`

### 5.4 File Viewer: Split Screen, Not Overlay

The file viewer currently appears as an overlay. It should feel like splitting the CRT.

**Implementation:**
- Instead of overlaying, push the conversation view to the left and slide the file viewer in from the right
- An amber divider line between them (2px, glowing) with a drag handle
- The divider should feel like a physical split in the monitor — add a subtle darkening at the edges (inner shadow on both panels)
- The file viewer gets its own mini-header with the filename, line count, and close button
- Keyboard shortcut: `Escape` closes the viewer, `[` / `]` navigate between recently viewed files

**Files:** `+page.svelte` (layout), `FileViewer.svelte`, `fileViewer.ts` (store)

---

## Phase 6: Typography Refinement

*The last 10% that takes it from "well designed" to "someone obsessed over this."*

### 6.1 Weight as Information

Within JetBrains Mono, use weight more deliberately:

| Element | Current | Proposed |
|---------|---------|----------|
| Role labels (CLAUDE, YOU) | 700 | 700 (keep) |
| Timestamps | 400 | 400 (keep, but add `tabular-nums`) |
| Body text (assistant) | 400 | 400 (keep) |
| Body text (user) | 400 | 500 (slightly bolder — it's *your* words) |
| Code inline | 400 | 400 (keep) |
| Tool names | 600 | 700 + 0.15em letter-spacing (more mechanical) |
| Stats bar labels | 600 | 500 (soften — they're secondary info) |
| Thinking text | 400 | 400 but at 11px (keep dense) |
| Progress items | 400 | 400 at 9px (really small, really quiet) |

### 6.2 Spacing Consistency

Audit all spacing values and snap to a 4px grid. Currently there's a mix of arbitrary values (14px, 10px, 6px). Standardize to:

- 4px (micro — within components)
- 8px (small — between related elements)
- 12px (medium — between sections within a component)
- 16px (large — between components)
- 24px (extra — between major sections like user messages)

This doesn't change the look dramatically but creates a subliminal sense of order.

---

## Priority Order

If I had to ship these in waves:

**Wave 1 (Highest Impact, Lowest Risk):**
1. Phase 2.1 — Ambient state shifts (transforms the entire feel)
2. Phase 1.1 — Phosphor persistence (instant character)
3. Phase 1.2 — Code blocks as embedded screens (polish)
4. Phase 6.1 — Typography weight refinement (subtle but cumulative)

**Wave 2 (High Impact, Medium Effort):**
5. Phase 3.3 — Thinking section purple zone
6. Phase 2.2 — Activity indicator upgrade
7. Phase 3.2 — Conversation rhythm
8. Phase 5.1 — Scroll behavior

**Wave 3 (Character & Delight):**
9. Phase 4.1 — Channel change effect
10. Phase 4.2 — Boot sequence
11. Phase 5.2 — Disconnect states
12. Phase 5.4 — File viewer split screen

**Wave 4 (Power Features):**
13. Phase 4.4 — Keyboard navigation
14. Phase 5.3 — Minimap overhaul
15. Phase 3.1 — Tool execution nesting

**Wave 5 (Optional / Experimental):**
16. Phase 4.3 — Sound design
17. Phase 1.3 — User message input card texture

---

## What This Does NOT Include

Things I considered and intentionally excluded:

- **Dark/light mode toggle** — You ARE the mode.
- **Theming / customization** — The aesthetic is the product. Users don't pick themes in a control room.
- **Second font family** — The monospace commitment is sacred.
- **Rounded corners everywhere** — The flat, angular feel is the CRT.
- **Emoji reactions / rich presence** — Wrong metaphor. This is a workstation, not Slack.
- **Drag-and-drop reordering** — Conversations are chronological. Period.
- **AI-generated summaries in UI** — The conversation IS the summary. Don't abstract it.

---

## Success Criteria

You'll know this worked when:

1. Someone sees it for the first time and says "whoa" before they understand what it does
2. The transition from thinking → responding feels like something *happened*, not just a color change
3. Old messages feel like history, not like they're just further up the page
4. The file viewer feels like part of the workspace, not a popup
5. Power users never touch the mouse
6. The boot sequence makes people smile, once
7. Nobody asks "can I change the theme?"
