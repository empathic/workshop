# Workshop UI Brand Book

## Design System: Amber Phosphor CRT

---

## Philosophy

Workshop's visual identity draws from vintage amber phosphor CRT monitors—the warm, glowing displays that powered early computing. This aesthetic evokes mission control rooms, mainframe terminals, and the tactile reality of hardware before everything became flat screens.

The design balances nostalgia with function: every glow, every scanline serves a purpose, creating an interface that feels alive and responsive while remaining highly usable.

---

## Global Design Tokens

### Color Palette

```css
:root {
  /* Amber phosphor palette */
  --amber-600: #d97706;
  --amber-500: #fb923c;
  --amber-400: #fdba74;
  --amber-300: #fed7aa;
  --amber-glow: rgba(251, 146, 60, 0.5);

  /* Purple (thinking state) */
  --purple-500: #8b5cf6;
  --purple-400: #a78bfa;
  --purple-glow: rgba(139, 92, 246, 0.5);

  /* Surfaces - warm dark browns */
  --surface-900: #0a0806;
  --surface-800: #0f0c0a;
  --surface-700: #15110d;
  --surface-600: #1a1510;
  --surface-500: #201a14;
  --surface-400: #2a231a;
  --surface-border: #3a2a1a;
  --surface-border-light: #4a3a2a;

  /* Text hierarchy */
  --text-primary: #fdba74;
  --text-secondary: #a08060;
  --text-muted: #6a5040;

  /* Status colors */
  --status-green: #22c55e;
  --status-red: #ef4444;
  --status-yellow: #fbbf24;
}
```

### Typography

**Font Stack**
```css
font-family: 'JetBrains Mono', 'SF Mono', 'Consolas', 'Monaco', monospace;
```

All text uses monospace fonts to reinforce the terminal aesthetic.

**Text Styles**

| Style | Size | Weight | Letter-spacing | Transform |
|-------|------|--------|----------------|-----------|
| Header | 16px | 700 | 0.1em | uppercase |
| Label | 11px | 700 | 0.1em | uppercase |
| Body | 13px | 400 | normal | none |
| Caption | 10px | 600 | 0.05em | uppercase |
| Code | 12px | 400 | normal | none |

### Global Effects

**CRT Scanlines**
Applied to the entire application via `body::after`:
```css
background: repeating-linear-gradient(
  0deg,
  transparent,
  transparent 2px,
  rgba(0, 0, 0, 0.08) 2px,
  rgba(0, 0, 0, 0.08) 4px
);
```

**Text Glow**
Primary text elements use amber glow:
```css
text-shadow: 0 0 10px rgba(251, 146, 60, 0.5);
```

**Selection Color**
```css
::selection {
  background: rgba(251, 146, 60, 0.3);
  color: var(--amber-300);
}
```

---

## Baud Meter Design System

### Design Philosophy

**Retro-Futuristic Terminal Aesthetic**

The baud meter draws inspiration from vintage CRT monitors, specifically the warm amber phosphor displays common in early computing and industrial terminals. It evokes the feeling of mission control rooms, mainframe operators, and the golden age of computing—when screens glowed with purpose.

The design balances nostalgia with modern functionality, creating a component that feels like real hardware while serving as an effective activity indicator.

---

### Color Palette

#### Primary: Amber Phosphor

| Token | Hex | Usage |
|-------|-----|-------|
| `amber-500` | `#fb923c` | Primary glow, LED, labels, active bars |
| `amber-400` | `#fdba74` | Bright numeric readout, highlights |
| `amber-glow` | `rgba(251, 146, 60, 0.5)` | Text shadows, outer glow effects |

#### Secondary: Purple (Thinking State)

| Token | Hex | Usage |
|-------|-----|-------|
| `purple-500` | `#8b5cf6` | LED, active bars when thinking |
| `purple-400` | `#a78bfa` | Labels when thinking |
| `purple-300` | `#c4b5fd` | Numeric readout when thinking |
| `purple-glow` | `rgba(139, 92, 246, 0.6)` | Glow effects when thinking |

#### Meter Bar Gradient

| Level | Color | Hex | Condition |
|-------|-------|-----|-----------|
| Normal | Amber | `#fb923c` | 0-40% activity |
| Warning | Yellow | `#fbbf24` | 40-70% activity |
| Hot | Red | `#ef4444` | 70-100% activity |

#### Surface Colors

| Token | Hex | Usage |
|-------|-----|-------|
| `surface-dark` | `#15100a` | Panel background (bottom) |
| `surface-mid` | `#1a120a` | Panel background (top) |
| `surface-bar` | `#1f150f` | Inactive bar background |
| `border` | `#3a2a1a` | Panel border |
| `border-bar` | `#2a1f1a` | Inactive bar border |

---

### Typography

**Font Stack**
```css
font-family: 'SF Mono', 'Consolas', 'Monaco', monospace;
```

**Label Style**
- Size: 10px
- Weight: 700
- Letter-spacing: 0.08em
- Text-shadow: `0 0 8px rgba(251, 146, 60, 0.5)`

**Numeric Readout**
- Size: 12px
- Weight: 700
- Variant: `tabular-nums` (fixed-width digits)
- Text-shadow: `0 0 8px rgba(253, 186, 116, 0.5)`

---

### Visual Effects

#### CRT Scanlines

Horizontal scanline overlay creates the characteristic CRT texture:

```css
background: repeating-linear-gradient(
  0deg,
  transparent,
  transparent 2px,
  rgba(0, 0, 0, 0.1) 2px,
  rgba(0, 0, 0, 0.1) 4px
);
```

#### Animated Scanline Sweep

A glowing line sweeps down the panel to simulate CRT refresh:

```css
.panel-scanline {
  height: 2px;
  background: linear-gradient(
    90deg,
    transparent,
    rgba(251, 146, 60, 0.5),
    transparent
  );
  animation: panel-scan 1.5s linear infinite;
}

@keyframes panel-scan {
  0% { top: 0; opacity: 1; }
  100% { top: 100%; opacity: 0.3; }
}
```

#### LED Glow

The status LED pulses to indicate activity:

```css
.panel-led {
  width: 6px;
  height: 6px;
  background: #fb923c;
  border-radius: 50%;
  box-shadow:
    0 0 6px #fb923c,
    0 0 12px rgba(251, 146, 60, 0.6);
}

@keyframes led-glow {
  0% { opacity: 0.5; }
  100% {
    opacity: 1;
    box-shadow:
      0 0 8px currentColor,
      0 0 16px currentColor;
  }
}
```

#### Hot Bar Flicker

At high activity levels, the red bars flicker like overdriven hardware:

```css
@keyframes bar-flash {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.7; }
}
```

---

### Component Anatomy

```
┌─────────────────────────────────────────────────────────┐
│ ┌─scanline sweep──────────────────────────────────────┐ │
│ │                                                     │ │
│ ●  LABEL    ▮▮▮▮▮▮▯▯▯▯    0000                       │
│ LED         └─meter bars─┘    └─CPS readout          │
│                                                       │
└─────────────────────────────────────────────────────────┘
  └─panel background with scanline texture
```

**Elements:**
1. **LED** - 6x6px pulsing indicator
2. **Label** - Current state (PROC, ACTIVE, tool name)
3. **Meter Bars** - 10 segments, 6x16px each, 2px gap
4. **CPS Readout** - 4-digit zero-padded rate display

---

### States

| State | Label | LED Color | Bar Color | Background |
|-------|-------|-----------|-----------|------------|
| Idle | — | — | — | Not visible |
| Active/Streaming | `ACTIVE` | Amber | Amber gradient | Warm brown |
| Thinking | `PROC` | Purple | Purple | Cool purple-brown |
| Tool Executing | Tool name | Amber | Amber gradient | Warm brown |

---

### Spacing & Dimensions

| Property | Value |
|----------|-------|
| Panel padding | `6px 14px` |
| Element gap | `10px` |
| Panel border-radius | `4px` |
| Bar dimensions | `6px × 16px` |
| Bar gap | `2px` |
| Bar border-radius | `1px` |
| LED size | `6px` |
| Min label width | `50px` |
| Min rate width | `36px` |

---

### Box Shadow

The panel has a subtle glow and inset highlight:

```css
box-shadow:
  0 0 15px rgba(251, 146, 60, 0.15),  /* outer glow */
  inset 0 1px 0 rgba(251, 146, 60, 0.1);  /* top highlight */
```

---

### Animation Timing

| Animation | Duration | Easing |
|-----------|----------|--------|
| Scanline sweep | 1.5s | linear |
| LED pulse | 0.4s | ease-in-out |
| Bar transition | 0.06s | ease |
| Hot bar flicker | 0.1s | ease |

---

### Usage Guidelines

**Do:**
- Use the baud meter to indicate active AI processing
- Let it pulse and glow—it should feel alive
- Keep it in the header for persistent visibility
- Use the thinking state (purple) for waiting/processing
- Use the tool state for specific operations

**Don't:**
- Show when idle (component should be hidden)
- Use for non-activity indicators
- Override the color palette with brand colors
- Remove the CRT effects (they're essential to the aesthetic)
- Display static—it should always animate when visible

---

### Accessibility

- The meter is decorative and supplementary
- Primary status is communicated through text labels
- High contrast between lit/unlit states (>4.5:1)
- Animation can be reduced via `prefers-reduced-motion`

---

### Code Reference

Implementation: `src/lib/components/MainView.svelte`

Styles are scoped to the component using Svelte's built-in CSS scoping.

---

## Component Library

### Sidebar

The sidebar uses a gradient background with a subtle edge glow:

```css
background: linear-gradient(180deg, var(--surface-700) 0%, var(--surface-800) 100%);
border-right: 1px solid var(--surface-border);
```

**Header**: App title in amber with strong glow, tagline in muted text
**Instance List**: Cards with amber border on active state, green LED for running status
**New Instance Button**: Bordered button with amber accent, glows on hover

### Header Bar

Gradient background with subtle bottom shadow:
```css
background: linear-gradient(180deg, var(--surface-600) 0%, var(--surface-700) 100%);
box-shadow: 0 2px 10px rgba(0, 0, 0, 0.3);
```

**Instance Name**: Amber-400 with strong glow
**Action Buttons**: Surface gradient with amber border when active

### Conversation View

**Stats Bar**: Tool usage counts with amber-highlighted numbers
**Message Cells**:
- User messages have muted left border
- Assistant messages have amber role label with glow
- Code blocks use surface-700 background
- Links are amber with hover glow

**Activity Indicator**: Matches baud meter styling (amber for active, purple for thinking)

### Message Input

**Textarea**: Surface-800 background, amber border on focus with subtle glow
**Send Button**: Amber-bordered with gradient background, glows on hover

### Terminal

Amber phosphor theme matching the original CRT aesthetic:
- Background: `#0a0806` (near-black warm)
- Foreground: `#fdba74` (amber-400)
- Cursor: `#fb923c` (amber-500) with glow effect

---

## Interaction States

### Hover

Elements lighten and gain subtle amber glow:
```css
background: var(--surface-500);
box-shadow: 0 0 15px rgba(251, 146, 60, 0.1);
```

### Focus

Inputs gain amber border and glow:
```css
border-color: var(--amber-600);
box-shadow: 0 0 15px rgba(251, 146, 60, 0.1);
```

### Active/Selected

Strong amber accent with visible glow:
```css
border-color: var(--amber-600);
box-shadow: 0 0 15px rgba(251, 146, 60, 0.2);
text-shadow: 0 0 10px var(--amber-glow);
```

### Disabled

Reduced opacity, muted colors:
```css
opacity: 0.5;
color: var(--text-muted);
```

---

## Motion

### Transitions

All interactive elements use 0.15s ease:
```css
transition: all 0.15s ease;
```

### Animations

| Animation | Duration | Easing | Use Case |
|-----------|----------|--------|----------|
| Scanline sweep | 1.5s | linear | Baud meter |
| LED pulse | 0.4s | ease-in-out | Status indicators |
| Spinner | 0.8s | linear | Loading states |
| Dot pulse | 1.4s | ease-in-out | Thinking indicator |

### Reduced Motion

Respect user preferences:
```css
@media (prefers-reduced-motion: reduce) {
  * {
    animation-duration: 0.01ms !important;
    transition-duration: 0.01ms !important;
  }
}
```

---

## File Structure

```
src/
├── routes/
│   └── +layout.svelte      # Global styles, CSS variables
├── lib/
│   └── components/
│       ├── Sidebar.svelte
│       ├── MainView.svelte     # Header + baud meter
│       ├── ConversationView.svelte
│       ├── NotebookCell.svelte
│       ├── MessageInput.svelte
│       └── Terminal.svelte
```

---

## Summary

The Workshop aesthetic is defined by:

1. **Amber phosphor** - Warm orange glow as the primary accent
2. **Monospace typography** - JetBrains Mono throughout
3. **CRT texture** - Subtle scanlines overlay the entire UI
4. **Warm dark surfaces** - Brown-tinted blacks, not blue-grays
5. **Glowing elements** - Text shadows and box shadows create depth
6. **Uppercase labels** - Industrial, technical feel
7. **Purple contrast** - Used for "thinking" states only
8. **Minimal borders** - Let glows define boundaries
