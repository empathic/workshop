<script lang="ts">
  import type { Snippet } from 'svelte';
  import { onMount } from 'svelte';

  let { children }: { children: Snippet } = $props();
  import { goto } from '$app/navigation';
  import { base } from '$app/paths';
  import { page } from '$app/stores';
  import { fetchInstances, initFromUrl, selectInstance, clearSelection } from '$lib/stores/instances';
  import { fetchTasks, migrateFromLocalStorage } from '$lib/stores/tasks';
  import { initMultiplexedConnection, disconnectAll } from '$lib/stores/websocket';
  import { viewportWidth } from '$lib/stores/ui';
  import { toggleDebugPanel } from '$lib/stores/metrics';
  import { checkAuth, authChecked, authEnabled, isAuthenticated, needsSetup } from '$lib/stores/auth';
  import { theme } from '$lib/stores/settings';
  import { resolveAuthGuard } from '$lib/utils/authGuard';
  import DebugPanel from '$lib/components/DebugPanel.svelte';

  let appInitialized = $state(false);

  // Handle browser back/forward navigation
  function handlePopState() {
    const instanceId = initFromUrl();
    if (instanceId) {
      // Don't push history when responding to popstate
      selectInstance(instanceId, false);
    } else {
      clearSelection(false);
    }
  }

  // Handle keyboard shortcuts
  function handleKeydown(e: KeyboardEvent) {
    // Ctrl+Shift+D (or Cmd+Shift+D on Mac) toggles debug panel
    if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key === 'D') {
      e.preventDefault();
      toggleDebugPanel();
    }
  }

  // Frame jank detector - warns when main thread is blocked
  function startJankDetector() {
    let lastFrame = performance.now();
    let rafId: number;

    function check() {
      const now = performance.now();
      const delta = now - lastFrame;
      if (delta > 100) {
        console.warn(`[Jank] Frame blocked for ${delta.toFixed(0)}ms`);
      }
      lastFrame = now;
      rafId = requestAnimationFrame(check);
    }

    rafId = requestAnimationFrame(check);
    return () => cancelAnimationFrame(rafId);
  }

  // Ink transition — the UI is ruled by hand when entering analog mode.
  // Structural lines (sidebar edge, header borders) draw themselves in
  // with pen-pressure gradients, followed by faint notebook ruling across
  // the content area. Each line fades to reveal the real border underneath.
  function playInkTransition() {
    const container = document.createElement('div');
    container.className = 'ink-rules-container';

    const rules: { dir: 'h' | 'v'; css: string; light?: boolean }[] = [
      // Structural lines — heavier, drawn first
      // Sidebar right edge: the first margin ruled
      { dir: 'v', css: 'left:259px;top:0;height:100%;--d:0ms;--dur:480ms' },
      // Sidebar header bottom
      { dir: 'h', css: 'left:0;top:69px;width:260px;--d:100ms;--dur:320ms' },
      // Main header bottom rule
      { dir: 'h', css: 'left:0;top:49px;width:100%;--d:180ms;--dur:420ms' },

      // Notebook ruling — lighter, drawn later, like guide lines on the page
      { dir: 'h', css: 'left:280px;top:30%;width:calc(100% - 300px);--d:320ms;--dur:340ms', light: true },
      { dir: 'h', css: 'left:280px;top:50%;width:calc(100% - 300px);--d:400ms;--dur:340ms', light: true },
      { dir: 'h', css: 'left:280px;top:70%;width:calc(100% - 300px);--d:480ms;--dur:340ms', light: true }
    ];

    for (const { dir, css, light } of rules) {
      const el = document.createElement('div');
      el.className = `ink-rule ink-rule-${dir}${light ? ' ink-rule-light' : ''}`;
      el.style.cssText = css;
      container.appendChild(el);
    }

    document.body.appendChild(container);

    // Cleanup after the last line finishes
    setTimeout(() => container.remove(), 1100);
  }

  // -----------------------------------------------------------------------
  // Reactive auth guard + app initialization.
  //
  // Pure decision in resolveAuthGuard(), side effects here.
  // Re-evaluates whenever $authChecked, $authEnabled, $isAuthenticated,
  // $needsSetup, or $page change — covers every path into every state.
  // -----------------------------------------------------------------------
  $effect(() => {
    const action = resolveAuthGuard({
      authChecked: $authChecked,
      authEnabled: $authEnabled,
      isAuthenticated: $isAuthenticated,
      needsSetup: $needsSetup,
      pathname: $page.url.pathname,
      basePath: base,
      appInitialized
    });

    switch (action.kind) {
      case 'wait':
        break;
      case 'redirect':
        goto(action.to);
        break;
      case 'init_app':
        appInitialized = true;
        initMultiplexedConnection();
        fetchInstances().then(() => {
          const instanceId = initFromUrl();
          if (instanceId) selectInstance(instanceId, false);
        });
        fetchTasks().then(() => migrateFromLocalStorage());
        break;
      case 'noop':
        break;
    }
  });

  // -----------------------------------------------------------------------
  // One-time browser setup (event listeners, theme binding, jank detector).
  // Also kicks off the auth check that feeds the $effect above.
  // -----------------------------------------------------------------------
  onMount(() => {
    const stopJankDetector = startJankDetector();

    let prevTheme: string | null = null;
    const unsubTheme = theme.subscribe((t) => {
      const isSwitch = prevTheme !== null && prevTheme !== t;
      document.body.setAttribute('data-theme', t);
      if (isSwitch && t === 'analog') {
        playInkTransition();
      }
      prevTheme = t;
    });

    // Kick off auth — the result flows into stores, the $effect reacts.
    checkAuth();

    window.addEventListener('popstate', handlePopState);
    const handleResize = () => viewportWidth.set(window.innerWidth);
    window.addEventListener('resize', handleResize);
    window.addEventListener('keydown', handleKeydown);

    return () => {
      unsubTheme();
      stopJankDetector();
      disconnectAll();
      window.removeEventListener('popstate', handlePopState);
      window.removeEventListener('resize', handleResize);
      window.removeEventListener('keydown', handleKeydown);
    };
  });
</script>

{@render children()}
<DebugPanel />

<style>
  @import url('https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;500;600;700&display=swap');
  @import url('https://fonts.googleapis.com/css2?family=Source+Serif+4:ital,opsz,wght@0,8..60,300;0,8..60,400;0,8..60,500;0,8..60,600;0,8..60,700;1,8..60,400;1,8..60,500&display=swap');
  @import url('https://fonts.googleapis.com/css2?family=Newsreader:ital,opsz,wght@0,6..72,400;0,6..72,500;0,6..72,600;0,6..72,700;1,6..72,400;1,6..72,500&display=swap');

  :global(*) {
    box-sizing: border-box;
  }

  :global(:root) {
    /* Amber phosphor palette */
    --accent-700: #b45309;
    --accent-600: #d97706;
    --accent-500: #fb923c;
    --accent-400: #fdba74;
    --accent-300: #fed7aa;

    /*
     * Chrome accent vs content accent:
     *
     * --chrome-accent-*  UI chrome: sidebar, headers, tabs, settings panels,
     *                    status indicators, fleet controls. Themes may tint
     *                    chrome independently (e.g. Solarized uses teal chrome).
     *
     * --accent-*         Content areas: conversation messages, notebook cells,
     *                    diffs, code highlights, game canvases.
     *
     * Defaults: chrome-accent inherits from accent unless a theme overrides it.
     * Rule of thumb: if the element is part of the application frame, use
     * --chrome-accent-*; if it lives inside user content, use --accent-*.
     */
    --chrome-accent-700: var(--accent-700);
    --chrome-accent-600: var(--accent-600);
    --chrome-accent-500: var(--accent-500);
    --chrome-accent-400: var(--accent-400);
    --chrome-accent-300: var(--accent-300);

    /* Purple (thinking state) */
    --thinking-500: #8b5cf6;
    --thinking-400: #a78bfa;
    --thinking-300: #c4b5fd;

    /* Surfaces */
    --surface-900: #0a0806;
    --surface-800: #0f0c0a;
    --surface-700: #15110d;
    --surface-600: #1a1510;
    --surface-500: #201a14;
    --surface-400: #2a231a;
    --surface-border: #3a2a1a;
    --surface-border-light: #4a3a2a;

    /* Text */
    --text-primary: #fdba74;
    --text-secondary: #a08060;
    --text-muted: #6a5040;

    /* Status */
    --status-green: #22c55e;
    --status-red: #ef4444;
    --status-yellow: #fbbf24;

    /* Responsive spacing */
    --spacing-xs: 4px;
    --spacing-sm: 8px;
    --spacing-md: 12px;
    --spacing-lg: 16px;
    --spacing-xl: 20px;

    /* Sidebar width */
    --sidebar-width: 260px;

    /* Touch targets */
    --touch-target-min: 44px;

    /* Ambient state - default is idle (dimmer, cooler amber) */
    --ambient-glow: rgba(251, 146, 60, 0.35);
    --ambient-accent: #d97706;
    --ambient-tint: rgba(251, 146, 60, 0.01);
    --ambient-scanline-opacity: 0.08;

    /* =======================================================
		   SEMANTIC TOKENS — intent, not appearance.
		   Components use THESE. Themes set THESE.
		   ======================================================= */

    /* Typography stacks */
    --font-body: 'JetBrains Mono', 'SF Mono', 'Consolas', 'Monaco', monospace;
    --font-display: 'JetBrains Mono', 'SF Mono', 'Consolas', 'Monaco', monospace;
    --font-mono: 'JetBrains Mono', 'SF Mono', 'Consolas', 'Monaco', monospace;

    /* Interactive tints — background color on state changes */
    --tint-hover: rgba(251, 146, 60, 0.05);
    --tint-active: rgba(251, 146, 60, 0.1);
    --tint-active-strong: rgba(251, 146, 60, 0.15);
    --tint-focus: rgba(251, 146, 60, 0.2);
    --tint-subtle: rgba(251, 146, 60, 0.02);
    --tint-thinking: rgba(139, 92, 246, 0.06);
    --tint-thinking-strong: rgba(139, 92, 246, 0.1);
    --tint-selection: rgba(251, 146, 60, 0.3);

    /* Effect: emphasis — text-shadow for glowing/prominent text */
    --emphasis: 0 0 10px var(--ambient-glow);
    --emphasis-strong: 0 0 15px var(--ambient-glow);

    /* Effect: elevation — box-shadow for raised elements */
    --elevation-low: 0 0 10px rgba(251, 146, 60, 0.1);
    --elevation-high: 0 0 20px rgba(251, 146, 60, 0.15);

    /* Effect: recess — box-shadow for inset/sunken elements */
    --recess: inset 0 0 30px rgba(251, 146, 60, 0.04);
    --recess-border: inset 0 1px 0 rgba(251, 146, 60, 0.06);

    /* Effect: depth — combined for cards/panels */
    --depth-up: 0 0 15px rgba(251, 146, 60, 0.1), inset 0 1px 0 rgba(251, 146, 60, 0.1);
    --depth-down: inset 0 0 20px rgba(0, 0, 0, 0.15);

    /* Overlay textures */
    --texture-overlay: repeating-linear-gradient(0deg, transparent, transparent 2px, black 2px, black 4px);
    --texture-opacity: var(--ambient-scanline-opacity, 0.08);
    --vignette: var(--ambient-tint);

    /* Multi-user presence indicators */
    --tint-presence: rgba(139, 92, 246, 0.2);
    --tint-presence-border: rgba(139, 92, 246, 0.3);

    /* Panel header/footer — recessed panel background */
    --panel-inset: rgba(0, 0, 0, 0.2);

    /* Spinner track */
    --spinner-track: rgba(251, 146, 60, 0.3);

    /* Backdrops & overlays */
    --backdrop: rgba(0, 0, 0, 0.7);
    --shadow-panel: -4px 0 20px rgba(0, 0, 0, 0.4);
    --shadow-dropdown: 0 4px 16px rgba(0, 0, 0, 0.5);
    --scanline-color: rgba(0, 0, 0, 0.04);

    /* Primary action buttons */
    --btn-primary-bg: linear-gradient(180deg, var(--accent-600) 0%, #b45309 100%);
    --btn-primary-text: #000;
    --btn-primary-text-shadow: 0 1px 0 rgba(255, 255, 255, 0.1);

    /* Status tints — functional color at alpha */
    --status-red-tint: rgba(239, 68, 68, 0.1);
    --status-red-border: rgba(239, 68, 68, 0.2);
    --status-red-strong: rgba(239, 68, 68, 0.2);
    --status-red-text: #f87171;
    --status-red-muted: #fca5a5;
    --status-green-tint: rgba(34, 197, 94, 0.1);
    --status-green-border: rgba(34, 197, 94, 0.2);
    --status-green-text: #6ee7b7;
    --status-blue: #60a5fa;
    --status-blue-tint: rgba(96, 165, 250, 0.2);
    --status-blue-text: #93c5fd;

    /* Text that contrasts with solid status-color backgrounds */
    --on-status: #ffffff;

    /* Active indicator — how "this is selected" is expressed */
    --active-border: 1px solid var(--accent-600);
    --active-accent-width: 0px;

    /* Minimap segment colors — muted indicators, tuned per theme */
    --minimap-user: #5a9a5a;
    --minimap-assistant: #d4944a;
    --minimap-system: #666666;
    --minimap-tool: #9a7ab0;
    --minimap-thinking: #8b5cf6;
    --minimap-viewport-fill: rgba(212, 148, 74, 0.2);
    --minimap-viewport-stroke: rgba(212, 148, 74, 0.6);
  }

  /* Ambient state overrides - the whole room shifts */
  :global([data-claude-state='thinking']) {
    --ambient-glow: rgba(139, 92, 246, 0.5);
    --ambient-accent: #8b5cf6;
    --ambient-tint: rgba(139, 92, 246, 0.02);
    --ambient-scanline-opacity: 0.06;
  }

  :global([data-claude-state='responding']) {
    --ambient-glow: rgba(251, 146, 60, 0.5);
    --ambient-accent: #fb923c;
    --ambient-tint: rgba(251, 146, 60, 0.025);
    --ambient-scanline-opacity: 0.08;
  }

  :global([data-claude-state='tool_executing']) {
    --ambient-glow: rgba(251, 191, 36, 0.5);
    --ambient-accent: #fbbf24;
    --ambient-tint: rgba(251, 191, 36, 0.02);
    --ambient-scanline-opacity: 0.1;
  }

  :global([data-claude-state='active']) {
    --ambient-glow: rgba(251, 146, 60, 0.5);
    --ambient-accent: #fb923c;
    --ambient-tint: rgba(251, 146, 60, 0.02);
    --ambient-scanline-opacity: 0.08;
  }

  /* Signal lost — the whole UI loses signal when disconnected */
  :global([data-connection='error']),
  :global([data-connection='disconnected']) {
    --ambient-scanline-opacity: 0.15;
    --ambient-glow: rgba(251, 146, 60, 0.2);
    --ambient-tint: rgba(0, 0, 0, 0.03);
  }

  :global([data-connection='reconnecting']) {
    --ambient-scanline-opacity: 0.12;
    --ambient-glow: rgba(251, 191, 36, 0.4);
    --ambient-tint: rgba(251, 191, 36, 0.01);
  }

  /* Server offline — more muted than transient disconnection */
  :global([data-connection='server_gone']) {
    --ambient-scanline-opacity: 0.18;
    --ambient-glow: rgba(251, 146, 60, 0.1);
    --ambient-tint: rgba(0, 0, 0, 0.05);
  }

  /* ==========================================================================
	   ANALOG THEME — Ink on Paper
	   Material shift: phosphor screen → printed page
	   ========================================================================== */

  :global([data-theme='analog']) {
    /* === INK — fountain pen, real pigment, pools and bleeds === */
    /* Iron gall ink — the classic. Near-black with blue-violet undertone. */
    --accent-700: #1a1410; /* deep pool */
    --accent-600: #2a1f18; /* concentrated iron gall — nearly black */
    --accent-500: #4a3528; /* mid-stroke density */
    --accent-400: #3a2a1e; /* nib-lift, slightly less saturated */
    --accent-300: #1a1410; /* full saturation pool */
    /* Thinking — Pilot Iroshizuku Kon-peki (deep cerulean) */
    --thinking-500: #1a4a7a;
    --thinking-400: #2a5a8a;
    --thinking-300: #3a6a9a;

    /* === PAPER — pure white stock === */
    --surface-900: #ffffff; /* top sheet — white */
    --surface-800: #fafafa; /* underlayer */
    --surface-700: #f3f3f1; /* inset/recessed */
    --surface-600: #eaeae7; /* deeper inset */
    --surface-500: #dadadb; /* pressed/debossed */
    --surface-400: #c4c4c0; /* heavy impression */
    --surface-border: #9a9080; /* ruled line — drawn with a straightedge, real graphite */
    --surface-border-light: #b5aa96;

    /* === TEXT — fountain pen ink density === */
    --text-primary: #141210; /* like fresh Noodler's Heart of Darkness */
    --text-secondary: #3a3630; /* second pass, nib running dry */
    --text-muted: #6a645c; /* pencil annotation, HB graphite */

    /* Status — real pigments, from the watercolor tray */
    --status-green: #1a5e2a; /* Hooker's green deep */
    --status-red: #8a2020; /* alizarin crimson */
    --status-yellow: #6a5510; /* yellow ochre concentrate */

    /* Ambient — ink bleeding into paper fibers */
    --ambient-glow: rgba(42, 31, 24, 0.06);
    --ambient-accent: #2a1f18;
    --ambient-tint: rgba(42, 31, 24, 0.008);
    --ambient-scanline-opacity: 0;

    /* === SEMANTIC TOKENS — drafting table material === */

    /* Typography — pen-friendly faces */
    --font-body: 'Source Serif 4', 'Georgia', 'Times New Roman', serif;
    --font-display: 'Newsreader', 'Georgia', serif;
    --font-mono: 'JetBrains Mono', 'SF Mono', 'Consolas', monospace;

    /* Tints — ink wash spreading through wet paper */
    --tint-hover: rgba(42, 31, 24, 0.04);
    --tint-active: rgba(42, 31, 24, 0.08);
    --tint-active-strong: rgba(42, 31, 24, 0.14);
    --tint-focus: rgba(42, 31, 24, 0.12);
    --tint-subtle: rgba(42, 31, 24, 0.02);
    --tint-thinking: rgba(26, 74, 122, 0.06);
    --tint-thinking-strong: rgba(26, 74, 122, 0.12);
    --tint-selection: rgba(42, 31, 24, 0.12);

    /* Emphasis — ink bleed, not glow. Slight feathering at edges. */
    --emphasis: 0 0 2px rgba(42, 31, 24, 0.25);
    --emphasis-strong: 0 0 3px rgba(42, 31, 24, 0.35), 0 0 6px rgba(42, 31, 24, 0.08);

    /* Elevation — paper cockle, not floating */
    --elevation-low: 0 1px 2px rgba(20, 18, 16, 0.08), 0 0 1px rgba(20, 18, 16, 0.12);
    --elevation-high: 0 2px 4px rgba(20, 18, 16, 0.1), 0 0 1px rgba(20, 18, 16, 0.15);

    /* Recess — pressed into the page, ink pooling in the valley */
    --recess: inset 0 1px 4px rgba(20, 18, 16, 0.06), inset 0 0 1px rgba(20, 18, 16, 0.08);
    --recess-border: inset 0 1px 0 rgba(20, 18, 16, 0.05);

    /* Depth — card stock stacking */
    --depth-up: 0 1px 3px rgba(20, 18, 16, 0.08), 0 0 1px rgba(20, 18, 16, 0.12);
    --depth-down: inset 0 1px 4px rgba(20, 18, 16, 0.06);

    /* Overlay — heavy paper grain (cold-press watercolor stock) */
    --texture-overlay: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='400' height='400'%3E%3Cfilter id='f'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.85' numOctaves='8' stitchTiles='stitch'/%3E%3CfeColorMatrix type='saturate' values='0'/%3E%3C/filter%3E%3Crect width='400' height='400' filter='url(%23f)' opacity='0.18'/%3E%3C/svg%3E");
    --texture-opacity: 1;
    --vignette: radial-gradient(
      ellipse at 50% 40%,
      transparent 0%,
      transparent 40%,
      rgba(120, 110, 90, 0.04) 70%,
      rgba(80, 70, 55, 0.1) 100%
    );

    /* Multi-user presence — cerulean ink */
    --tint-presence: rgba(26, 74, 122, 0.08);
    --tint-presence-border: rgba(26, 74, 122, 0.2);

    /* Panel header/footer — paper shadow */
    --panel-inset: rgba(20, 18, 16, 0.04);

    /* Spinner track — graphite */
    --spinner-track: var(--surface-border);

    /* Backdrops & overlays — vellum over the page */
    --backdrop: rgba(254, 254, 254, 0.75);
    --shadow-panel: -2px 0 6px rgba(20, 18, 16, 0.1);
    --shadow-dropdown: 0 2px 6px rgba(20, 18, 16, 0.12), 0 0 1px rgba(20, 18, 16, 0.15);
    --scanline-color: rgba(0, 0, 0, 0.03);

    /* Primary action buttons — ink stamp, heavy impression */
    --btn-primary-bg: linear-gradient(180deg, #3a2a1e 0%, #1a1410 100%);
    --btn-primary-text: var(--surface-900);
    --btn-primary-text-shadow: none;

    /* Status tints — watercolor pigment dropped on wet paper */
    --status-red-tint: rgba(138, 32, 32, 0.08);
    --status-red-border: rgba(138, 32, 32, 0.25);
    --status-red-strong: rgba(138, 32, 32, 0.14);
    --status-red-text: #8a2020;
    --status-red-muted: #a54040;
    --status-green-tint: rgba(26, 94, 42, 0.08);
    --status-green-border: rgba(26, 94, 42, 0.25);
    --status-green-text: #1a5e2a;
    --status-blue: #1a4a7a;
    --status-blue-tint: rgba(26, 74, 122, 0.1);
    --status-blue-text: #1a4a7a;

    --on-status: #ffffff;

    /* Active indicator — heavy pen stroke down the left margin */
    --active-border: 2px solid var(--accent-600);
    --active-accent-width: 3px;

    /* Minimap — ink-wash muted tones */
    --minimap-user: #3a6a4a;
    --minimap-assistant: #5a4538;
    --minimap-system: #7a7470;
    --minimap-tool: #4a5a7a;
    --minimap-thinking: #1a4a7a;
    --minimap-viewport-fill: rgba(42, 31, 24, 0.12);
    --minimap-viewport-stroke: rgba(42, 31, 24, 0.4);

    /* === SCROLLING GRAIN — applied directly to element backgrounds === */
    /* Fine fiber — sharp micro-detail for small elements (buttons, badges, inline code) */
    --grain-fine: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='200' height='200'%3E%3Cfilter id='f'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='8' stitchTiles='stitch'/%3E%3CfeColorMatrix type='saturate' values='0'/%3E%3C/filter%3E%3Crect width='200' height='200' filter='url(%23f)' opacity='0.14'/%3E%3C/svg%3E");
    /* Coarse pulp — large-scale paper texture for panels and surfaces */
    --grain-coarse: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='150' height='150'%3E%3Cfilter id='g'%3E%3CfeTurbulence type='turbulence' baseFrequency='0.35' numOctaves='2' stitchTiles='stitch'/%3E%3CfeColorMatrix type='saturate' values='0'/%3E%3C/filter%3E%3Crect width='150' height='150' filter='url(%23g)' opacity='0.08'/%3E%3C/svg%3E");
    /* Ink wash — asymmetric radial gradient like diluted ink pooling */
    --ink-wash: radial-gradient(ellipse at 30% 20%, rgba(42, 31, 24, 0.03) 0%, transparent 70%);
  }

  /* Analog ambient overrides — ink changes character under pressure */
  :global([data-theme='analog'][data-claude-state='thinking']) {
    --ambient-glow: rgba(26, 74, 122, 0.06);
    --ambient-accent: #1a4a7a;
    --ambient-tint: rgba(26, 74, 122, 0.015);
  }

  :global([data-theme='analog'][data-claude-state='responding']) {
    --ambient-glow: rgba(42, 31, 24, 0.06);
    --ambient-accent: #4a3528;
    --ambient-tint: rgba(42, 31, 24, 0.02);
  }

  :global([data-theme='analog'][data-claude-state='tool_executing']) {
    --ambient-glow: rgba(106, 85, 16, 0.06);
    --ambient-accent: #6a5510;
    --ambient-tint: rgba(106, 85, 16, 0.015);
  }

  :global([data-theme='analog'][data-claude-state='active']) {
    --ambient-glow: rgba(42, 31, 24, 0.06);
    --ambient-accent: #4a3528;
    --ambient-tint: rgba(42, 31, 24, 0.015);
  }

  :global([data-theme='analog'][data-connection='error']),
  :global([data-theme='analog'][data-connection='disconnected']) {
    --ambient-glow: rgba(42, 31, 24, 0.03);
    --ambient-tint: rgba(20, 18, 16, 0.02);
  }

  :global([data-theme='analog'][data-connection='reconnecting']) {
    --ambient-glow: rgba(106, 85, 16, 0.04);
    --ambient-tint: rgba(106, 85, 16, 0.01);
  }

  :global([data-theme='analog'][data-connection='server_gone']) {
    --ambient-glow: rgba(42, 31, 24, 0.02);
    --ambient-tint: rgba(20, 18, 16, 0.03);
  }

  /* ==========================================================================
     SOLARIZED DARK THEME
     Ethan Schoonover's Solarized palette — dark variant.
     Precision-engineered color relationships with L*a*b* symmetry.
     ========================================================================== */

  :global([data-theme='solarized-dark']) {
    /* Primary accent — solarized blue (content: notebook cells, diffs, code) */
    --accent-700: #1a6ba0;
    --accent-600: #1e7ab8;
    --accent-500: #268bd2;
    --accent-400: #4a9cd8;
    --accent-300: #6eafde;
    /* Chrome accent — muted blue-gray for UI chrome (base01/base00 blended ~35% toward blue) */
    --chrome-accent-700: #345c6f;
    --chrome-accent-600: #466f84;
    --chrome-accent-500: #568095;
    --chrome-accent-400: #6791a8;
    --chrome-accent-300: #7ba0b4;
    /* Thinking — solarized violet */
    --thinking-500: #6c71c4;
    --thinking-400: #8085cc;
    --thinking-300: #9a9ed6;

    /* Solarized base03..base01 surfaces (interpolated with desaturation) */
    --surface-900: #002b36;
    --surface-800: #04313c;
    --surface-700: #073642;
    --surface-600: #0f3c47;
    --surface-500: #17414c;
    --surface-400: #1f4751;
    --surface-border: #274c56;
    --surface-border-light: #30525c;

    /* Solarized base0/base1 text */
    --text-primary: #839496;
    --text-secondary: #657b83;
    --text-muted: #586e75;

    /* Status */
    --status-green: #859900;
    --status-red: #dc322f;
    --status-yellow: #b58900;

    /* Ambient — yellow for emphasis */
    --ambient-glow: rgba(181, 137, 0, 0.08);
    --ambient-accent: #b58900;
    --ambient-tint: rgba(181, 137, 0, 0.008);
    --ambient-scanline-opacity: 0;

    /* Tints — base01 neutral shifts (warm gray, not accent-colored) */
    --tint-hover: rgba(88, 110, 117, 0.06);
    --tint-active: rgba(88, 110, 117, 0.10);
    --tint-active-strong: rgba(88, 110, 117, 0.15);
    --tint-focus: rgba(88, 110, 117, 0.12);
    --tint-subtle: rgba(88, 110, 117, 0.02);
    --tint-thinking: rgba(108, 113, 196, 0.06);
    --tint-thinking-strong: rgba(108, 113, 196, 0.1);
    --tint-selection: rgba(88, 110, 117, 0.30);

    /* Emphasis — subtle blue glow */
    --emphasis: 0 0 6px rgba(38, 139, 210, 0.12);
    --emphasis-strong: 0 0 10px rgba(38, 139, 210, 0.2);

    /* Elevation */
    --elevation-low: 0 0 8px rgba(0, 0, 0, 0.3);
    --elevation-high: 0 0 16px rgba(0, 0, 0, 0.4);

    /* Recess */
    --recess: inset 0 0 20px rgba(0, 0, 0, 0.15);
    --recess-border: inset 0 1px 0 rgba(0, 0, 0, 0.1);

    /* Depth */
    --depth-up: 0 0 12px rgba(0, 0, 0, 0.3), inset 0 1px 0 rgba(0, 0, 0, 0.1);
    --depth-down: inset 0 0 16px rgba(0, 0, 0, 0.2);

    /* No scanlines, no grain */
    --texture-overlay: none;
    --texture-opacity: 0;
    --vignette: none;

    /* Presence */
    --tint-presence: rgba(108, 113, 196, 0.2);
    --tint-presence-border: rgba(108, 113, 196, 0.3);

    --panel-inset: rgba(0, 0, 0, 0.2);
    --spinner-track: rgba(88, 110, 117, 0.4);

    /* Backdrops */
    --backdrop: rgba(0, 43, 54, 0.8);
    --shadow-panel: -4px 0 20px rgba(0, 0, 0, 0.4);
    --shadow-dropdown: 0 4px 16px rgba(0, 0, 0, 0.5);
    --scanline-color: rgba(0, 0, 0, 0.04);

    /* Primary buttons — solarized yellow (emphasis element) */
    --btn-primary-bg: linear-gradient(180deg, #b58900 0%, #8a6800 100%);
    --btn-primary-text: #002b36;
    --btn-primary-text-shadow: none;

    /* Status tints */
    --status-red-tint: rgba(220, 50, 47, 0.1);
    --status-red-border: rgba(220, 50, 47, 0.2);
    --status-red-strong: rgba(220, 50, 47, 0.2);
    --status-red-text: #dc322f;
    --status-red-muted: #e06060;
    --status-green-tint: rgba(133, 153, 0, 0.1);
    --status-green-border: rgba(133, 153, 0, 0.2);
    --status-green-text: #859900;
    --status-blue: #268bd2;
    --status-blue-tint: rgba(38, 139, 210, 0.2);
    --status-blue-text: #268bd2;

    --on-status: #ffffff;

    --active-border: 1px solid #b58900;
    --active-accent-width: 0px;

    /* Minimap — solarized muted tones */
    --minimap-user: #5a7a30;
    --minimap-assistant: #4a7aa0;
    --minimap-system: #586e75;
    --minimap-tool: #7a70a0;
    --minimap-thinking: #6c71c4;
    --minimap-viewport-fill: rgba(38, 139, 210, 0.15);
    --minimap-viewport-stroke: rgba(38, 139, 210, 0.5);
  }

  /* Solarized Dark ambient overrides */
  :global([data-theme='solarized-dark'][data-claude-state='thinking']) {
    --ambient-glow: rgba(108, 113, 196, 0.08);
    --ambient-accent: #6c71c4;
    --ambient-tint: rgba(108, 113, 196, 0.02);
  }

  :global([data-theme='solarized-dark'][data-claude-state='responding']) {
    --ambient-glow: rgba(181, 137, 0, 0.08);
    --ambient-accent: #b58900;
    --ambient-tint: rgba(181, 137, 0, 0.008);
  }

  :global([data-theme='solarized-dark'][data-claude-state='tool_executing']) {
    --ambient-glow: rgba(42, 161, 152, 0.08);
    --ambient-accent: #2aa198;
    --ambient-tint: rgba(42, 161, 152, 0.02);
  }

  :global([data-theme='solarized-dark'][data-connection='error']),
  :global([data-theme='solarized-dark'][data-connection='disconnected']) {
    --ambient-glow: rgba(88, 110, 117, 0.04);
    --ambient-tint: rgba(0, 0, 0, 0.03);
  }

  /* Solarized Dark syntax */
  :global([data-theme='solarized-dark'] .hljs) {
    background: var(--surface-700);
    color: #839496;
  }
  :global([data-theme='solarized-dark'] .hljs-comment),
  :global([data-theme='solarized-dark'] .hljs-quote) { color: #586e75; font-style: italic; }
  :global([data-theme='solarized-dark'] .hljs-keyword),
  :global([data-theme='solarized-dark'] .hljs-selector-tag) { color: #859900; font-weight: 500; }
  :global([data-theme='solarized-dark'] .hljs-string),
  :global([data-theme='solarized-dark'] .hljs-addition) { color: #2aa198; }
  :global([data-theme='solarized-dark'] .hljs-number),
  :global([data-theme='solarized-dark'] .hljs-literal) { color: #d33682; }
  :global([data-theme='solarized-dark'] .hljs-built_in),
  :global([data-theme='solarized-dark'] .hljs-type) { color: #b58900; }
  :global([data-theme='solarized-dark'] .hljs-variable) { color: #cb4b16; }
  :global([data-theme='solarized-dark'] .hljs-attr) { color: #b58900; }
  :global([data-theme='solarized-dark'] .hljs-title),
  :global([data-theme='solarized-dark'] .hljs-title.function_),
  :global([data-theme='solarized-dark'] .hljs-section) { color: #268bd2; font-weight: 500; }
  :global([data-theme='solarized-dark'] .hljs-title.class_) { color: #b58900; font-weight: 600; }
  :global([data-theme='solarized-dark'] .hljs-regexp),
  :global([data-theme='solarized-dark'] .hljs-symbol) { color: #d33682; }
  :global([data-theme='solarized-dark'] .hljs-deletion) { color: #dc322f; background: rgba(220, 50, 47, 0.1); }
  :global([data-theme='solarized-dark'] .hljs-meta) { color: #2aa198; }
  :global([data-theme='solarized-dark'] .hljs-operator) { color: #93a1a1; }
  :global([data-theme='solarized-dark'] .hljs-property) { color: #268bd2; }
  :global([data-theme='solarized-dark'] .hljs-punctuation) { color: #586e75; }
  :global([data-theme='solarized-dark'] .hljs-tag) { color: #268bd2; }
  :global([data-theme='solarized-dark'] .hljs-selector-class),
  :global([data-theme='solarized-dark'] .hljs-selector-id) { color: #268bd2; font-weight: 600; }

  /* ==========================================================================
     SOLARIZED LIGHT THEME
     Solarized palette — light variant. Same hues, swapped base tones.
     ========================================================================== */

  :global([data-theme='solarized-light']) {
    --accent-700: #1a6ba0;
    --accent-600: #1e7ab8;
    --accent-500: #268bd2;
    --accent-400: #4a9cd8;
    --accent-300: #6eafde;
    /* Chrome accent — muted blue-gray for UI chrome (darker for light bg) */
    --chrome-accent-700: #285066;
    --chrome-accent-600: #346078;
    --chrome-accent-500: #3c6e88;
    --chrome-accent-400: #457998;
    --chrome-accent-300: #5f8da8;

    --thinking-500: #6c71c4;
    --thinking-400: #5a5fba;
    --thinking-300: #8085cc;

    /* Solarized base3..base00 surfaces (light) */
    --surface-900: #fdf6e3;
    --surface-800: #f5efd6;
    --surface-700: #eee8d5;
    --surface-600: #e6e0cb;
    --surface-500: #ddd8c2;
    --surface-400: #d0cab5;
    --surface-border: #b8b09a;
    --surface-border-light: #c8c0a8;

    --text-primary: #586e75;
    --text-secondary: #657b83;
    --text-muted: #93a1a1;

    --status-green: #859900;
    --status-red: #dc322f;
    --status-yellow: #b58900;

    --ambient-glow: rgba(181, 137, 0, 0.08);
    --ambient-accent: #b58900;
    --ambient-tint: rgba(181, 137, 0, 0.008);
    --ambient-scanline-opacity: 0;

    --tint-hover: rgba(147, 161, 161, 0.08);
    --tint-active: rgba(147, 161, 161, 0.12);
    --tint-active-strong: rgba(147, 161, 161, 0.18);
    --tint-focus: rgba(147, 161, 161, 0.15);
    --tint-subtle: rgba(147, 161, 161, 0.03);
    --tint-thinking: rgba(108, 113, 196, 0.06);
    --tint-thinking-strong: rgba(108, 113, 196, 0.12);
    --tint-selection: rgba(147, 161, 161, 0.25);

    --emphasis: none;
    --emphasis-strong: 0 0 2px rgba(38, 139, 210, 0.15);

    --elevation-low: 0 1px 3px rgba(0, 0, 0, 0.08);
    --elevation-high: 0 2px 6px rgba(0, 0, 0, 0.12);

    --recess: inset 0 1px 4px rgba(0, 0, 0, 0.06);
    --recess-border: inset 0 1px 0 rgba(0, 0, 0, 0.04);

    --depth-up: 0 1px 3px rgba(0, 0, 0, 0.08), inset 0 1px 0 rgba(0, 0, 0, 0.04);
    --depth-down: inset 0 1px 4px rgba(0, 0, 0, 0.06);

    --texture-overlay: none;
    --texture-opacity: 0;
    --vignette: none;

    --tint-presence: rgba(108, 113, 196, 0.1);
    --tint-presence-border: rgba(108, 113, 196, 0.2);

    --panel-inset: rgba(0, 0, 0, 0.04);
    --spinner-track: rgba(88, 110, 117, 0.4);

    --backdrop: rgba(253, 246, 227, 0.8);
    --shadow-panel: -2px 0 8px rgba(0, 0, 0, 0.08);
    --shadow-dropdown: 0 2px 8px rgba(0, 0, 0, 0.12);
    --scanline-color: rgba(0, 0, 0, 0.03);

    --btn-primary-bg: linear-gradient(180deg, #b58900 0%, #8a6800 100%);
    --btn-primary-text: #fdf6e3;
    --btn-primary-text-shadow: none;

    --status-red-tint: rgba(220, 50, 47, 0.08);
    --status-red-border: rgba(220, 50, 47, 0.2);
    --status-red-strong: rgba(220, 50, 47, 0.14);
    --status-red-text: #dc322f;
    --status-red-muted: #c44040;
    --status-green-tint: rgba(133, 153, 0, 0.08);
    --status-green-border: rgba(133, 153, 0, 0.2);
    --status-green-text: #859900;
    --status-blue: #268bd2;
    --status-blue-tint: rgba(38, 139, 210, 0.1);
    --status-blue-text: #268bd2;

    --on-status: #ffffff;

    --active-border: 2px solid #b58900;
    --active-accent-width: 2px;

    /* Minimap — solarized light muted tones */
    --minimap-user: #6a8a40;
    --minimap-assistant: #5a8ab0;
    --minimap-system: #93a1a1;
    --minimap-tool: #8a80b0;
    --minimap-thinking: #6c71c4;
    --minimap-viewport-fill: rgba(38, 139, 210, 0.12);
    --minimap-viewport-stroke: rgba(38, 139, 210, 0.4);
  }

  /* Solarized Light ambient overrides */
  :global([data-theme='solarized-light'][data-claude-state='thinking']) {
    --ambient-glow: rgba(108, 113, 196, 0.12);
    --ambient-accent: #6c71c4;
    --ambient-tint: rgba(108, 113, 196, 0.015);
  }

  :global([data-theme='solarized-light'][data-claude-state='responding']) {
    --ambient-glow: rgba(181, 137, 0, 0.08);
    --ambient-accent: #b58900;
    --ambient-tint: rgba(181, 137, 0, 0.008);
  }

  :global([data-theme='solarized-light'][data-claude-state='tool_executing']) {
    --ambient-glow: rgba(42, 161, 152, 0.12);
    --ambient-accent: #2aa198;
    --ambient-tint: rgba(42, 161, 152, 0.015);
  }

  :global([data-theme='solarized-light'][data-connection='error']),
  :global([data-theme='solarized-light'][data-connection='disconnected']) {
    --ambient-glow: rgba(88, 110, 117, 0.04);
    --ambient-tint: rgba(0, 0, 0, 0.02);
  }

  /* Solarized Light syntax */
  :global([data-theme='solarized-light'] .hljs) {
    background: var(--surface-700);
    color: #586e75;
  }
  :global([data-theme='solarized-light'] .hljs-comment),
  :global([data-theme='solarized-light'] .hljs-quote) { color: #93a1a1; font-style: italic; }
  :global([data-theme='solarized-light'] .hljs-keyword),
  :global([data-theme='solarized-light'] .hljs-selector-tag) { color: #859900; font-weight: 500; }
  :global([data-theme='solarized-light'] .hljs-string),
  :global([data-theme='solarized-light'] .hljs-addition) { color: #2aa198; }
  :global([data-theme='solarized-light'] .hljs-number),
  :global([data-theme='solarized-light'] .hljs-literal) { color: #d33682; }
  :global([data-theme='solarized-light'] .hljs-built_in),
  :global([data-theme='solarized-light'] .hljs-type) { color: #b58900; }
  :global([data-theme='solarized-light'] .hljs-variable) { color: #cb4b16; }
  :global([data-theme='solarized-light'] .hljs-attr) { color: #b58900; }
  :global([data-theme='solarized-light'] .hljs-title),
  :global([data-theme='solarized-light'] .hljs-title.function_),
  :global([data-theme='solarized-light'] .hljs-section) { color: #268bd2; font-weight: 500; }
  :global([data-theme='solarized-light'] .hljs-title.class_) { color: #b58900; font-weight: 600; }
  :global([data-theme='solarized-light'] .hljs-regexp),
  :global([data-theme='solarized-light'] .hljs-symbol) { color: #d33682; }
  :global([data-theme='solarized-light'] .hljs-deletion) { color: #dc322f; background: rgba(220, 50, 47, 0.08); }
  :global([data-theme='solarized-light'] .hljs-meta) { color: #2aa198; }
  :global([data-theme='solarized-light'] .hljs-operator) { color: #657b83; }
  :global([data-theme='solarized-light'] .hljs-property) { color: #268bd2; }
  :global([data-theme='solarized-light'] .hljs-punctuation) { color: #93a1a1; }
  :global([data-theme='solarized-light'] .hljs-tag) { color: #268bd2; }
  :global([data-theme='solarized-light'] .hljs-selector-class),
  :global([data-theme='solarized-light'] .hljs-selector-id) { color: #268bd2; font-weight: 600; }

  /* ==========================================================================
     DARCULA THEME (IntelliJ Dark)
     JetBrains Darcula palette — warm dark with blue-gray undertones.
     ========================================================================== */

  :global([data-theme='darcula']) {
    --accent-700: #365880;
    --accent-600: #4a6da7;
    --accent-500: #5a82ba;
    --accent-400: #7096c8;
    --accent-300: #88aad4;

    --thinking-500: #9876aa;
    --thinking-400: #ab8cbf;
    --thinking-300: #bfa0d0;

    /* Darcula surfaces */
    --surface-900: #2b2b2b;
    --surface-800: #2f2f2f;
    --surface-700: #3c3f41;
    --surface-600: #45484a;
    --surface-500: #4e5254;
    --surface-400: #5a5d5f;
    --surface-border: #616365;
    --surface-border-light: #6e7072;

    --text-primary: #a9b7c6;
    --text-secondary: #808890;
    --text-muted: #606870;

    --status-green: #6a8759;
    --status-red: #cf6a4c;
    --status-yellow: #bbb529;

    --ambient-glow: rgba(74, 109, 167, 0.08);
    --ambient-accent: #4a6da7;
    --ambient-tint: rgba(74, 109, 167, 0.01);
    --ambient-scanline-opacity: 0;

    --tint-hover: rgba(74, 109, 167, 0.06);
    --tint-active: rgba(74, 109, 167, 0.1);
    --tint-active-strong: rgba(74, 109, 167, 0.15);
    --tint-focus: rgba(74, 109, 167, 0.2);
    --tint-subtle: rgba(74, 109, 167, 0.02);
    --tint-thinking: rgba(152, 118, 170, 0.06);
    --tint-thinking-strong: rgba(152, 118, 170, 0.1);
    --tint-selection: rgba(33, 66, 131, 0.45);

    --emphasis: 0 0 4px rgba(74, 109, 167, 0.10);
    --emphasis-strong: 0 0 6px rgba(74, 109, 167, 0.18);

    --elevation-low: 0 0 8px rgba(0, 0, 0, 0.3);
    --elevation-high: 0 0 16px rgba(0, 0, 0, 0.4);

    --recess: inset 0 0 20px rgba(0, 0, 0, 0.15);
    --recess-border: inset 0 1px 0 rgba(74, 109, 167, 0.06);

    --depth-up: 0 0 12px rgba(0, 0, 0, 0.3), inset 0 1px 0 rgba(74, 109, 167, 0.08);
    --depth-down: inset 0 0 16px rgba(0, 0, 0, 0.2);

    --texture-overlay: none;
    --texture-opacity: 0;
    --vignette: none;

    --tint-presence: rgba(152, 118, 170, 0.2);
    --tint-presence-border: rgba(152, 118, 170, 0.3);

    --panel-inset: rgba(0, 0, 0, 0.15);
    --spinner-track: rgba(74, 109, 167, 0.3);

    --backdrop: rgba(43, 43, 43, 0.85);
    --shadow-panel: -4px 0 20px rgba(0, 0, 0, 0.4);
    --shadow-dropdown: 0 4px 16px rgba(0, 0, 0, 0.5);
    --scanline-color: rgba(0, 0, 0, 0.04);

    --btn-primary-bg: linear-gradient(180deg, #4a6da7 0%, #365880 100%);
    --btn-primary-text: #bbc7d4;
    --btn-primary-text-shadow: none;

    --status-red-tint: rgba(207, 106, 76, 0.1);
    --status-red-border: rgba(207, 106, 76, 0.2);
    --status-red-strong: rgba(207, 106, 76, 0.2);
    --status-red-text: #cf6a4c;
    --status-red-muted: #d48070;
    --status-green-tint: rgba(106, 135, 89, 0.1);
    --status-green-border: rgba(106, 135, 89, 0.2);
    --status-green-text: #6a8759;
    --status-blue: #6897bb;
    --status-blue-tint: rgba(104, 151, 187, 0.2);
    --status-blue-text: #6897bb;

    --on-status: #ffffff;

    --active-border: 1px solid #4a6da7;
    --active-accent-width: 0px;

    /* Minimap — darcula muted tones */
    --minimap-user: #5a7a50;
    --minimap-assistant: #a07050;
    --minimap-system: #606366;
    --minimap-tool: #806a90;
    --minimap-thinking: #9876aa;
    --minimap-viewport-fill: rgba(204, 120, 50, 0.15);
    --minimap-viewport-stroke: rgba(204, 120, 50, 0.5);
  }

  /* Darcula ambient overrides */
  :global([data-theme='darcula'][data-claude-state='thinking']) {
    --ambient-glow: rgba(152, 118, 170, 0.08);
    --ambient-accent: #9876aa;
    --ambient-tint: rgba(152, 118, 170, 0.02);
  }

  :global([data-theme='darcula'][data-claude-state='responding']) {
    --ambient-glow: rgba(74, 109, 167, 0.12);
    --ambient-accent: #4a6da7;
    --ambient-tint: rgba(74, 109, 167, 0.02);
  }

  :global([data-theme='darcula'][data-claude-state='tool_executing']) {
    --ambient-glow: rgba(104, 151, 187, 0.12);
    --ambient-accent: #6897bb;
    --ambient-tint: rgba(104, 151, 187, 0.015);
  }

  :global([data-theme='darcula'][data-connection='error']),
  :global([data-theme='darcula'][data-connection='disconnected']) {
    --ambient-glow: rgba(74, 109, 167, 0.04);
    --ambient-tint: rgba(0, 0, 0, 0.03);
  }

  /* Darcula syntax */
  :global([data-theme='darcula'] .hljs) {
    background: var(--surface-700);
    color: #a9b7c6;
  }
  :global([data-theme='darcula'] .hljs-comment),
  :global([data-theme='darcula'] .hljs-quote) { color: #808080; font-style: italic; }
  :global([data-theme='darcula'] .hljs-keyword),
  :global([data-theme='darcula'] .hljs-selector-tag) { color: #cc7832; font-weight: 500; }
  :global([data-theme='darcula'] .hljs-string),
  :global([data-theme='darcula'] .hljs-addition) { color: #6a8759; }
  :global([data-theme='darcula'] .hljs-number),
  :global([data-theme='darcula'] .hljs-literal) { color: #6897bb; }
  :global([data-theme='darcula'] .hljs-built_in),
  :global([data-theme='darcula'] .hljs-type) { color: #ffc66d; }
  :global([data-theme='darcula'] .hljs-variable) { color: #9876aa; }
  :global([data-theme='darcula'] .hljs-attr) { color: #bababa; }
  :global([data-theme='darcula'] .hljs-title),
  :global([data-theme='darcula'] .hljs-title.function_),
  :global([data-theme='darcula'] .hljs-section) { color: #ffc66d; font-weight: 500; }
  :global([data-theme='darcula'] .hljs-title.class_) { color: #a9b7c6; font-weight: 600; text-decoration: underline; }
  :global([data-theme='darcula'] .hljs-regexp),
  :global([data-theme='darcula'] .hljs-symbol) { color: #e0c46c; }
  :global([data-theme='darcula'] .hljs-deletion) { color: #cf6a4c; background: rgba(207, 106, 76, 0.1); }
  :global([data-theme='darcula'] .hljs-meta) { color: #bbb529; }
  :global([data-theme='darcula'] .hljs-operator) { color: #a9b7c6; }
  :global([data-theme='darcula'] .hljs-property) { color: #9876aa; }
  :global([data-theme='darcula'] .hljs-punctuation) { color: #a9b7c6; }
  :global([data-theme='darcula'] .hljs-tag) { color: #e8bf6a; }
  :global([data-theme='darcula'] .hljs-selector-class),
  :global([data-theme='darcula'] .hljs-selector-id) { color: #ffc66d; font-weight: 600; }

  /* ==========================================================================
     INTELLIJ LIGHT THEME
     JetBrains IntelliJ Light — clean, neutral, professional.
     ========================================================================== */

  :global([data-theme='intellij-light']) {
    --accent-700: #144a88;
    --accent-600: #1a5da8;
    --accent-500: #2470c0;
    --accent-400: #4a8ad0;
    --accent-300: #6ea0dc;

    --thinking-500: #871094;
    --thinking-400: #9c1aab;
    --thinking-300: #b030c0;

    /* IntelliJ light surfaces */
    --surface-900: #ffffff;
    --surface-800: #f7f8fa;
    --surface-700: #f0f0f0;
    --surface-600: #e8e8e8;
    --surface-500: #d8d8d8;
    --surface-400: #c0c0c0;
    --surface-border: #b0b0b0;
    --surface-border-light: #c8c8c8;

    --text-primary: #080808;
    --text-secondary: #3b3b3b;
    --text-muted: #787878;

    --status-green: #067d17;
    --status-red: #c7222d;
    --status-yellow: #9e880d;

    --ambient-glow: rgba(36, 112, 192, 0.08);
    --ambient-accent: #2470c0;
    --ambient-tint: rgba(36, 112, 192, 0.008);
    --ambient-scanline-opacity: 0;

    --tint-hover: rgba(36, 112, 192, 0.06);
    --tint-active: rgba(36, 112, 192, 0.1);
    --tint-active-strong: rgba(36, 112, 192, 0.15);
    --tint-focus: rgba(36, 112, 192, 0.12);
    --tint-subtle: rgba(36, 112, 192, 0.02);
    --tint-thinking: rgba(135, 16, 148, 0.06);
    --tint-thinking-strong: rgba(135, 16, 148, 0.12);
    --tint-selection: rgba(36, 112, 192, 0.18);

    --emphasis: none;
    --emphasis-strong: 0 0 2px rgba(36, 112, 192, 0.15);

    --elevation-low: 0 1px 3px rgba(0, 0, 0, 0.06);
    --elevation-high: 0 2px 6px rgba(0, 0, 0, 0.1);

    --recess: inset 0 1px 3px rgba(0, 0, 0, 0.04);
    --recess-border: inset 0 1px 0 rgba(0, 0, 0, 0.03);

    --depth-up: 0 1px 3px rgba(0, 0, 0, 0.06);
    --depth-down: inset 0 1px 3px rgba(0, 0, 0, 0.04);

    --texture-overlay: none;
    --texture-opacity: 0;
    --vignette: none;

    --tint-presence: rgba(135, 16, 148, 0.08);
    --tint-presence-border: rgba(135, 16, 148, 0.2);

    --panel-inset: rgba(0, 0, 0, 0.03);
    --spinner-track: rgba(36, 112, 192, 0.3);

    --backdrop: rgba(255, 255, 255, 0.8);
    --shadow-panel: -2px 0 6px rgba(0, 0, 0, 0.06);
    --shadow-dropdown: 0 2px 6px rgba(0, 0, 0, 0.1);
    --scanline-color: rgba(0, 0, 0, 0.03);

    --btn-primary-bg: linear-gradient(180deg, #4a86c8 0%, #3574b8 100%);
    --btn-primary-text: #ffffff;
    --btn-primary-text-shadow: none;

    --status-red-tint: rgba(199, 34, 45, 0.08);
    --status-red-border: rgba(199, 34, 45, 0.2);
    --status-red-strong: rgba(199, 34, 45, 0.14);
    --status-red-text: #c7222d;
    --status-red-muted: #d04050;
    --status-green-tint: rgba(6, 125, 23, 0.08);
    --status-green-border: rgba(6, 125, 23, 0.2);
    --status-green-text: #067d17;
    --status-blue: #2470c0;
    --status-blue-tint: rgba(36, 112, 192, 0.1);
    --status-blue-text: #2470c0;

    --on-status: #ffffff;

    --active-border: 2px solid #2470c0;
    --active-accent-width: 2px;

    /* Minimap — IntelliJ light muted tones */
    --minimap-user: #4a8a4a;
    --minimap-assistant: #5a80b0;
    --minimap-system: #8c8c8c;
    --minimap-tool: #8a6aa0;
    --minimap-thinking: #871094;
    --minimap-viewport-fill: rgba(36, 112, 192, 0.1);
    --minimap-viewport-stroke: rgba(36, 112, 192, 0.4);
  }

  /* IntelliJ Light ambient overrides */
  :global([data-theme='intellij-light'][data-claude-state='thinking']) {
    --ambient-glow: rgba(135, 16, 148, 0.08);
    --ambient-accent: #871094;
    --ambient-tint: rgba(135, 16, 148, 0.015);
  }

  :global([data-theme='intellij-light'][data-claude-state='responding']) {
    --ambient-glow: rgba(36, 112, 192, 0.08);
    --ambient-accent: #2470c0;
    --ambient-tint: rgba(36, 112, 192, 0.02);
  }

  :global([data-theme='intellij-light'][data-claude-state='tool_executing']) {
    --ambient-glow: rgba(158, 136, 13, 0.08);
    --ambient-accent: #9e880d;
    --ambient-tint: rgba(158, 136, 13, 0.015);
  }

  :global([data-theme='intellij-light'][data-connection='error']),
  :global([data-theme='intellij-light'][data-connection='disconnected']) {
    --ambient-glow: rgba(36, 112, 192, 0.04);
    --ambient-tint: rgba(0, 0, 0, 0.02);
  }

  /* IntelliJ Light syntax */
  :global([data-theme='intellij-light'] .hljs) {
    background: var(--surface-700);
    color: #080808;
  }
  :global([data-theme='intellij-light'] .hljs-comment),
  :global([data-theme='intellij-light'] .hljs-quote) { color: #8c8c8c; font-style: italic; }
  :global([data-theme='intellij-light'] .hljs-keyword),
  :global([data-theme='intellij-light'] .hljs-selector-tag) { color: #0033b3; font-weight: 700; }
  :global([data-theme='intellij-light'] .hljs-string),
  :global([data-theme='intellij-light'] .hljs-addition) { color: #067d17; }
  :global([data-theme='intellij-light'] .hljs-number),
  :global([data-theme='intellij-light'] .hljs-literal) { color: #1750eb; }
  :global([data-theme='intellij-light'] .hljs-built_in),
  :global([data-theme='intellij-light'] .hljs-type) { color: #0033b3; }
  :global([data-theme='intellij-light'] .hljs-variable) { color: #871094; }
  :global([data-theme='intellij-light'] .hljs-attr) { color: #174ad4; }
  :global([data-theme='intellij-light'] .hljs-title),
  :global([data-theme='intellij-light'] .hljs-title.function_),
  :global([data-theme='intellij-light'] .hljs-section) { color: #00627a; font-weight: 500; }
  :global([data-theme='intellij-light'] .hljs-title.class_) { color: #0033b3; font-weight: 600; }
  :global([data-theme='intellij-light'] .hljs-regexp),
  :global([data-theme='intellij-light'] .hljs-symbol) { color: #067d17; }
  :global([data-theme='intellij-light'] .hljs-deletion) { color: #c7222d; background: rgba(199, 34, 45, 0.06); }
  :global([data-theme='intellij-light'] .hljs-meta) { color: #9e880d; }
  :global([data-theme='intellij-light'] .hljs-operator) { color: #080808; }
  :global([data-theme='intellij-light'] .hljs-property) { color: #871094; }
  :global([data-theme='intellij-light'] .hljs-punctuation) { color: #080808; }
  :global([data-theme='intellij-light'] .hljs-tag) { color: #0033b3; }
  :global([data-theme='intellij-light'] .hljs-selector-class),
  :global([data-theme='intellij-light'] .hljs-selector-id) { color: #00627a; font-weight: 600; }

  /* ==========================================================================
     LIGHT THEME SHARED OVERRIDES
     Common adjustments for all light themes (solarized-light, intellij-light).
     These handle body background, scrollbar, selection, and pseudo-element
     cleanup that dark themes don't need.
     ========================================================================== */

  :global([data-theme='solarized-light'] html),
  :global([data-theme='solarized-light'] body),
  :global([data-theme='intellij-light'] html),
  :global([data-theme='intellij-light'] body) {
    background: var(--surface-900);
    color: var(--text-primary);
  }

  /* Kill the vignette/tint overlay on light themes */
  :global([data-theme='solarized-light'] body::before),
  :global([data-theme='intellij-light'] body::before) {
    background: none;
  }

  /* Kill the texture overlay on light themes */
  :global([data-theme='solarized-light'] body::after),
  :global([data-theme='intellij-light'] body::after) {
    display: none;
  }

  /* Light scrollbars */
  :global([data-theme='solarized-light'] ::-webkit-scrollbar-track),
  :global([data-theme='intellij-light'] ::-webkit-scrollbar-track) {
    background: var(--surface-800);
  }

  :global([data-theme='solarized-light'] ::-webkit-scrollbar-thumb),
  :global([data-theme='intellij-light'] ::-webkit-scrollbar-thumb) {
    background: var(--surface-400);
    border: 1px solid var(--surface-border-light);
  }

  :global([data-theme='solarized-light'] ::-webkit-scrollbar-thumb:hover),
  :global([data-theme='intellij-light'] ::-webkit-scrollbar-thumb:hover) {
    background: var(--text-muted);
  }

  /* Light selection */
  :global([data-theme='solarized-light'] ::selection) {
    background: rgba(147, 161, 161, 0.25);
    color: var(--text-primary);
  }

  :global([data-theme='intellij-light'] ::selection) {
    background: rgba(36, 112, 192, 0.2);
    color: var(--text-primary);
  }

  /* Solarized Dark — kill overlays (clean dark, not CRT) */
  :global([data-theme='solarized-dark'] body::before) {
    background: none;
  }
  :global([data-theme='solarized-dark'] body::after) {
    display: none;
  }

  /* Darcula — kill overlays (clean dark, not CRT) */
  :global([data-theme='darcula'] body::before) {
    background: none;
  }
  :global([data-theme='darcula'] body::after) {
    display: none;
  }

  /* Analog: font family swap — serif for body, keep mono for code */
  :global([data-theme='analog'] html),
  :global([data-theme='analog'] body) {
    font-family: 'Source Serif 4', 'Georgia', 'Times New Roman', serif;
    background: var(--surface-900);
    color: var(--text-primary);
  }

  /* Ink-stained edges — like a well-used drawing pad.
	   Darker vignette, more intentional. Ink fingerprints on the margins. */
  :global([data-theme='analog'] body::before) {
    background:
			/* Main vignette — tighter, more dramatic */
      radial-gradient(
        ellipse at 50% 45%,
        transparent 0%,
        transparent 35%,
        rgba(100, 90, 70, 0.05) 60%,
        rgba(70, 60, 45, 0.12) 85%,
        rgba(40, 35, 25, 0.18) 100%
      ),
      /* Ink blot — top left corner, like a pen was rested there */
      radial-gradient(circle at 8% 6%, rgba(42, 31, 24, 0.06) 0%, rgba(42, 31, 24, 0.02) 40%, transparent 60%),
      /* Ink blot — bottom right, accidental touch */
      radial-gradient(circle at 92% 88%, rgba(42, 31, 24, 0.04) 0%, rgba(42, 31, 24, 0.01) 30%, transparent 50%);
    transition: background 1.2s ease;
  }

  /* Paper grain is now on the elements themselves (via --grain-fine / --grain-coarse
	   background-image), so kill the fixed overlay — it doesn't scroll and flattens
	   the per-element texture. */
  :global([data-theme='analog'] body::after) {
    display: none;
  }

  /* Scrollbar — slim, understated, like a page edge */
  :global([data-theme='analog'] ::-webkit-scrollbar) {
    width: 6px;
  }

  :global([data-theme='analog'] ::-webkit-scrollbar-track) {
    background: transparent;
  }

  :global([data-theme='analog'] ::-webkit-scrollbar-thumb) {
    background: var(--surface-400);
    border-radius: 3px;
    border: none;
  }

  :global([data-theme='analog'] ::-webkit-scrollbar-thumb:hover) {
    background: var(--text-muted);
  }

  /* Selection — ink wash, like watercolor dragged across the page */
  :global([data-theme='analog'] ::selection) {
    background: rgba(42, 31, 24, 0.15);
    color: var(--text-primary);
  }

  /* File links — underline drawn with a ruling pen, ink bleeds on hover */
  :global([data-theme='analog'] .file-link) {
    color: var(--text-primary);
    border-bottom: 1.5px solid var(--accent-600);
    transition:
      border-color 0.2s ease,
      text-shadow 0.3s ease;
  }

  :global([data-theme='analog'] .file-link:hover) {
    background: transparent;
    border-bottom-width: 2px;
    border-bottom-color: var(--text-primary);
    text-shadow: 0 0 3px rgba(42, 31, 24, 0.2);
  }

  :global([data-theme='analog'] .file-link:focus) {
    background: rgba(42, 31, 24, 0.06);
    box-shadow: none;
  }

  /* ==========================================================================
	   Analog Syntax Highlighting — Ink on Paper
	   Restrained, editorial color palette
	   ========================================================================== */

  /* Code blocks: like a plate inset on the page, ruled left margin,
	   ink-bleed shadow instead of clean edges. Scrolling grain. */
  :global([data-theme='analog'] .hljs) {
    background-color: var(--surface-700);
    background-image: var(--grain-fine), var(--grain-coarse);
    background-blend-mode: multiply, multiply;
    color: #201c18;
    border-left: 3px solid var(--accent-600);
    box-shadow: inset 2px 0 4px rgba(42, 31, 24, 0.06);
  }

  /* Comments — light pencil, graphite gray. The drafter's notes. */
  :global([data-theme='analog'] .hljs-comment),
  :global([data-theme='analog'] .hljs-quote) {
    color: #8a8478;
    font-style: italic;
  }

  /* Keywords — heavy nib pressure, dense ink, the backbone of the text */
  :global([data-theme='analog'] .hljs-keyword),
  :global([data-theme='analog'] .hljs-selector-tag) {
    color: #141210;
    font-weight: 700;
  }

  :global([data-theme='analog'] .hljs-tag) {
    color: #3a2a1e;
  }

  /* Strings — Diamine Sherwood Green, classic fountain pen ink */
  :global([data-theme='analog'] .hljs-string),
  :global([data-theme='analog'] .hljs-addition) {
    color: #155e28;
  }

  :global([data-theme='analog'] .hljs-template-tag),
  :global([data-theme='analog'] .hljs-template-variable) {
    color: #105020;
  }

  /* Numbers — Diamine Ancient Copper */
  :global([data-theme='analog'] .hljs-number),
  :global([data-theme='analog'] .hljs-literal) {
    color: #8a3a08;
  }

  /* Built-ins — raw umber, earthier */
  :global([data-theme='analog'] .hljs-built_in),
  :global([data-theme='analog'] .hljs-type) {
    color: #5a3e10;
  }

  :global([data-theme='analog'] .hljs-variable) {
    color: #2a2420;
  }

  /* Attributes — Kon-peki blue-black */
  :global([data-theme='analog'] .hljs-attr) {
    color: #1a4a7a;
  }

  /* Functions — iron gall ink, heavy stroke, the important words */
  :global([data-theme='analog'] .hljs-title),
  :global([data-theme='analog'] .hljs-title.function_),
  :global([data-theme='analog'] .hljs-section) {
    color: #2a1f18;
    font-weight: 700;
  }

  /* Classes — Diamine Damson, plum with weight */
  :global([data-theme='analog'] .hljs-title.class_),
  :global([data-theme='analog'] .hljs-class .hljs-title) {
    color: #4a2050;
    font-weight: 600;
  }

  /* Regex — alizarin, like a correction mark */
  :global([data-theme='analog'] .hljs-regexp),
  :global([data-theme='analog'] .hljs-symbol) {
    color: #701830;
  }

  :global([data-theme='analog'] .hljs-deletion) {
    color: #701818;
    background: rgba(112, 24, 24, 0.08);
    text-decoration: line-through;
    text-decoration-color: rgba(112, 24, 24, 0.3);
  }

  :global([data-theme='analog'] .hljs-addition) {
    background: rgba(21, 94, 40, 0.08);
  }

  :global([data-theme='analog'] .hljs-meta) {
    color: #1a4a4a;
  }

  :global([data-theme='analog'] .hljs-meta .hljs-keyword) {
    color: #1a4a4a;
    font-weight: 700;
  }

  :global([data-theme='analog'] .hljs-meta .hljs-string) {
    color: #155e28;
  }

  /* Punctuation — nib barely touching paper, light and delicate */
  :global([data-theme='analog'] .hljs-punctuation) {
    color: #8a8478;
  }

  :global([data-theme='analog'] .hljs-operator) {
    color: #141210;
    font-weight: 500;
  }

  :global([data-theme='analog'] .hljs-property) {
    color: #3a2a1e;
  }

  :global([data-theme='analog'] .hljs-params) {
    color: #4a4540;
  }

  :global([data-theme='analog'] .hljs-strong) {
    font-weight: 800;
    color: #141210;
  }

  :global([data-theme='analog'] .hljs-emphasis) {
    font-style: italic;
    color: #2a2520;
  }

  :global([data-theme='analog'] .hljs-link) {
    color: #1a4a7a;
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  :global([data-theme='analog'] .hljs-selector-class),
  :global([data-theme='analog'] .hljs-selector-id) {
    color: #2a1f18;
    font-weight: 600;
  }

  :global([data-theme='analog'] .hljs-selector-pseudo) {
    color: #4a2050;
  }

  :global([data-theme='analog'] .hljs-namespace) {
    color: #4a2050;
    opacity: 0.9;
  }

  /* Hover: ink bleed shadow, not phosphor glow */
  :global([data-theme='analog'] pre code.hljs:hover) {
    box-shadow: inset 3px 0 8px rgba(42, 31, 24, 0.08);
  }

  /* Line numbers — pencil guides, barely there */
  :global([data-theme='analog'] .code-line::before) {
    color: rgba(20, 18, 16, 0.12);
  }

  /* ==========================================================================
	   Analog Theme — Global Fallbacks
	   Typography overrides for generic HTML elements.
	   Component-specific overrides live in their own .svelte files.
	   ========================================================================== */

  /* Buttons: ink stamp impression — dark textured background with grain */
  :global([data-theme='analog'] button) {
    text-shadow: none;
  }

  /* Blockquotes: marginalia with ink wash and visible paper */
  :global([data-theme='analog'] blockquote) {
    background-color: var(--surface-800);
    background-image: var(--grain-fine), var(--ink-wash);
    background-blend-mode: multiply, normal;
    border-left: 3px solid var(--accent-600);
  }

  /* Keep monospace for code elements (global HTML, not component-scoped) */
  :global([data-theme='analog'] code),
  :global([data-theme='analog'] pre) {
    font-family: 'JetBrains Mono', 'SF Mono', 'Consolas', monospace;
  }

  /* Headings: use the display serif */
  :global([data-theme='analog'] h1),
  :global([data-theme='analog'] h2),
  :global([data-theme='analog'] h3) {
    font-family: 'Newsreader', Georgia, serif;
  }

  /* Mobile-first responsive adjustments */
  @media (max-width: 639px) {
    :global(:root) {
      --spacing-md: 10px;
      --spacing-lg: 12px;
      --spacing-xl: 16px;
    }
  }

  :global(html, body) {
    margin: 0;
    padding: 0;
    height: 100%;
    overflow: hidden;
    background: var(--surface-900);
    color: var(--text-primary);
    font-family: var(--font-body);
  }

  /* Vignette/tint overlay */
  :global(body::before) {
    content: '';
    position: fixed;
    inset: 0;
    background: var(--vignette);
    pointer-events: none;
    z-index: 9998;
    transition: background 0.8s ease;
  }

  /* Texture overlay — scanlines or paper grain */
  :global(body::after) {
    content: '';
    position: fixed;
    inset: 0;
    background: var(--texture-overlay);
    opacity: var(--texture-opacity);
    pointer-events: none;
    z-index: 9999;
    transition: opacity 0.8s ease;
  }

  /* Custom scrollbar styles - amber themed */
  :global(::-webkit-scrollbar) {
    width: 8px;
    height: 8px;
  }

  :global(::-webkit-scrollbar-track) {
    background: var(--surface-800);
  }

  :global(::-webkit-scrollbar-thumb) {
    background: var(--surface-400);
    border-radius: 4px;
    border: 1px solid var(--surface-border);
  }

  :global(::-webkit-scrollbar-thumb:hover) {
    background: var(--ambient-accent, var(--chrome-accent-600));
    border-color: var(--ambient-accent, var(--chrome-accent-500));
    transition:
      background 0.8s ease,
      border-color 0.8s ease;
  }

  /* Selection color */
  :global(::selection) {
    background: var(--tint-selection);
    color: var(--accent-300);
  }

  /* ==========================================================================
	   Syntax Highlighting - High Contrast CRT Theme
	   Distinct colors for readability while maintaining retro aesthetic
	   ========================================================================== */

  :global(.hljs) {
    background: var(--surface-700);
    color: #e8dcc8;
  }

  /* Comments - visible but subdued olive */
  :global(.hljs-comment),
  :global(.hljs-quote) {
    color: #7a8a6a;
    font-style: italic;
  }

  /* Keywords - electric cyan (stands out clearly) */
  :global(.hljs-keyword),
  :global(.hljs-selector-tag) {
    color: #5ccfe6;
    font-weight: 500;
  }

  /* HTML/XML tags - coral */
  :global(.hljs-tag) {
    color: #f29e74;
  }

  /* Strings - bright lime green */
  :global(.hljs-string),
  :global(.hljs-addition) {
    color: #a5e075;
  }

  /* Template strings */
  :global(.hljs-template-tag),
  :global(.hljs-template-variable) {
    color: #95d865;
  }

  /* Numbers, booleans - vivid orange */
  :global(.hljs-number),
  :global(.hljs-literal) {
    color: #ffae57;
  }

  /* Built-in types, primitives - gold */
  :global(.hljs-built_in),
  :global(.hljs-type) {
    color: #ffd580;
  }

  /* Variables - light amber */
  :global(.hljs-variable) {
    color: #f5d9a8;
  }

  /* Attributes - peach */
  :global(.hljs-attr) {
    color: #f0b090;
  }

  /* Functions - bright yellow (very prominent) */
  :global(.hljs-title),
  :global(.hljs-title.function_),
  :global(.hljs-section) {
    color: #ffd866;
    font-weight: 500;
  }

  /* Classes, types - soft purple */
  :global(.hljs-title.class_),
  :global(.hljs-class .hljs-title) {
    color: #d4a5ff;
  }

  /* Regex, special chars - pink/magenta */
  :global(.hljs-regexp),
  :global(.hljs-symbol) {
    color: #ff80bf;
  }

  /* Deletion - bright red */
  :global(.hljs-deletion) {
    color: #ff6b6b;
    background: rgba(255, 107, 107, 0.1);
  }

  /* Addition background */
  :global(.hljs-addition) {
    background: rgba(165, 224, 117, 0.1);
  }

  /* Meta, preprocessor - teal */
  :global(.hljs-meta) {
    color: #80cbc4;
  }

  :global(.hljs-meta .hljs-keyword) {
    color: #80cbc4;
    font-weight: 500;
  }

  :global(.hljs-meta .hljs-string) {
    color: #a5e075;
  }

  /* Punctuation - subtle but visible */
  :global(.hljs-punctuation) {
    color: #b0a090;
  }

  /* Operators - brighter for visibility */
  :global(.hljs-operator) {
    color: #ff9d6f;
  }

  /* Property names - warm tan */
  :global(.hljs-property) {
    color: #e8c090;
  }

  /* Parameters - distinct from variables */
  :global(.hljs-params) {
    color: #c4b8a8;
  }

  /* Bold and emphasis */
  :global(.hljs-strong) {
    font-weight: 700;
    color: #ffe4a0;
  }

  :global(.hljs-emphasis) {
    font-style: italic;
    color: #c8d8c8;
  }

  /* Links - blue that pops */
  :global(.hljs-link) {
    color: #73d0ff;
    text-decoration: underline;
  }

  /* Language-specific: JSON keys */
  :global(.hljs-attr) {
    color: #5ccfe6;
  }

  /* Language-specific: CSS selectors */
  :global(.hljs-selector-class),
  :global(.hljs-selector-id) {
    color: #ffd866;
  }

  :global(.hljs-selector-pseudo) {
    color: #d4a5ff;
  }

  /* Namespace */
  :global(.hljs-namespace) {
    color: #d4a5ff;
    opacity: 0.9;
  }

  /* Subtle glow on hover for interactive feel */
  :global(pre code.hljs:hover) {
    box-shadow: inset 0 0 30px rgba(251, 146, 60, 0.03);
  }

  /* ==========================================================================
	   Line Numbers
	   ========================================================================== */

  :global(code.hljs) {
    counter-reset: line;
  }

  :global(.code-line) {
    display: block;
    position: relative;
    counter-increment: line;
  }

  :global(.code-line::before) {
    content: counter(line);
    position: absolute;
    left: calc(-1 * var(--gutter-width, 2.5em) - 0.5em);
    width: var(--gutter-width, 2.5em);
    text-align: right;
    color: rgba(232, 220, 200, 0.2);
    font-size: inherit;
    line-height: inherit;
    user-select: none;
    pointer-events: none;
  }

  /* ==========================================================================
	   File Links - Interactive file path styling
	   ========================================================================== */

  :global(.file-link) {
    color: var(--accent-400);
    text-decoration: none;
    border-bottom: 1px dashed var(--accent-600);
    cursor: pointer;
    transition: all 0.15s ease;
    padding: 0 2px;
    margin: 0 -2px;
    border-radius: 2px;
  }

  :global(.file-link:hover) {
    background: rgba(251, 146, 60, 0.15);
    border-bottom-color: var(--accent-400);
  }

  :global(.file-link:focus) {
    outline: none;
    background: rgba(251, 146, 60, 0.2);
    box-shadow: 0 0 0 2px rgba(251, 146, 60, 0.3);
  }

  :global(.file-link:active) {
    background: rgba(251, 146, 60, 0.25);
  }

  /* ==========================================================================
	   Ink Transition — ruled-line drawing
	   The UI's structural lines draw themselves in like a ruling pen on
	   fresh paper. Notebook guide lines follow. Each fades to reveal the
	   real border underneath.
	   ========================================================================== */

  :global(.ink-rules-container) {
    position: fixed;
    inset: 0;
    z-index: 100000;
    pointer-events: none;
  }

  /* Base rule — positioned by JS, animated by direction class */
  :global(.ink-rule) {
    position: absolute;
    pointer-events: none;
  }

  /* Horizontal rule — pen-pressure gradient: heavier at the nib start */
  :global(.ink-rule-h) {
    height: 1.5px;
    background: linear-gradient(
      90deg,
      rgba(42, 31, 24, 0.5) 0%,
      rgba(42, 31, 24, 0.35) 50%,
      rgba(42, 31, 24, 0.12) 100%
    );
    transform-origin: left center;
    transform: scaleX(0) rotate(0.12deg);
    animation: rule-draw-h var(--dur, 420ms) cubic-bezier(0.25, 0.8, 0.25, 1) var(--d, 0ms) forwards;
  }

  /* Vertical rule — pressure fades toward the bottom */
  :global(.ink-rule-v) {
    width: 2px;
    background: linear-gradient(
      180deg,
      rgba(42, 31, 24, 0.5) 0%,
      rgba(42, 31, 24, 0.35) 50%,
      rgba(42, 31, 24, 0.12) 100%
    );
    transform-origin: center top;
    transform: scaleY(0) rotate(-0.08deg);
    animation: rule-draw-v var(--dur, 480ms) cubic-bezier(0.25, 0.8, 0.25, 1) var(--d, 0ms) forwards;
  }

  /* Light ruling lines — notebook guide lines, much fainter */
  :global(.ink-rule-light.ink-rule-h) {
    height: 1px;
    background: linear-gradient(
      90deg,
      rgba(42, 31, 24, 0.15) 0%,
      rgba(42, 31, 24, 0.12) 50%,
      rgba(42, 31, 24, 0.04) 100%
    );
  }

  @keyframes rule-draw-h {
    0% {
      transform: scaleX(0) rotate(0.12deg);
      opacity: 1;
    }
    55% {
      transform: scaleX(1) rotate(0.12deg);
      opacity: 0.8;
    }
    75% {
      transform: scaleX(1) rotate(0.12deg);
      opacity: 0.5;
    }
    100% {
      transform: scaleX(1) rotate(0.12deg);
      opacity: 0;
    }
  }

  @keyframes rule-draw-v {
    0% {
      transform: scaleY(0) rotate(-0.08deg);
      opacity: 1;
    }
    55% {
      transform: scaleY(1) rotate(-0.08deg);
      opacity: 0.8;
    }
    75% {
      transform: scaleY(1) rotate(-0.08deg);
      opacity: 0.5;
    }
    100% {
      transform: scaleY(1) rotate(-0.08deg);
      opacity: 0;
    }
  }
</style>
