<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { get } from 'svelte/store';
  import {
    sendInput,
    sendResize,
    sendTerminalVisible,
    sendTerminalHidden,
    hasPendingInput,
    connectionStatus,
    requestTerminalLock,
    releaseTerminalLock,
    instancePresence,
    reconnect,
    shutdownReason
  } from '$lib/stores/websocket';
  import {
    currentTerminalHasOutput,
    consumeTerminalOutput,
    markAwaitingReplay,
    terminalHasOutputForInstance
  } from '$lib/stores/terminal';
  import { currentInstanceId } from '$lib/stores/instances';
  import { consumeTerminalFocus } from '$lib/stores/layout';
  import { currentTerminalLock, iHoldLock, isLockedByOther } from '$lib/stores/terminalLock';
  import { theme, userSettings } from '$lib/stores/settings';

  /** Optional: bind to a specific instance instead of following currentInstanceId */
  interface Props {
    instanceId?: string;
    paneId?: string;
  }

  let { instanceId: propInstanceId, paneId }: Props = $props();

  const xtermThemes: Record<string, Record<string, string>> = {
    phosphor: {
      background: '#0a0806',
      foreground: '#fdba74',
      cursor: '#fb923c',
      cursorAccent: '#0a0806',
      selectionBackground: 'rgba(251, 146, 60, 0.3)',
      black: '#15110d',
      red: '#ef4444',
      green: '#22c55e',
      yellow: '#fbbf24',
      blue: '#60a5fa',
      magenta: '#a78bfa',
      cyan: '#22d3ee',
      white: '#fdba74'
    },
    analog: {
      background: '#faf7f0',
      foreground: '#1a1714',
      cursor: '#6e3b1a',
      cursorAccent: '#faf7f0',
      selectionBackground: 'rgba(220, 190, 100, 0.3)',
      black: '#1a1714',
      red: '#943030',
      green: '#2a6e3a',
      yellow: '#7a6520',
      blue: '#2e4a6e',
      magenta: '#5a3060',
      cyan: '#2a5a5a',
      white: '#faf7f0'
    },
    'solarized-dark': {
      background: '#002b36',
      foreground: '#839496',
      cursor: '#b58900',
      cursorAccent: '#002b36',
      selectionBackground: 'rgba(181, 137, 0, 0.3)',
      black: '#073642',
      red: '#dc322f',
      green: '#859900',
      yellow: '#b58900',
      blue: '#268bd2',
      magenta: '#d33682',
      cyan: '#2aa198',
      white: '#eee8d5'
    },
    'solarized-light': {
      background: '#fdf6e3',
      foreground: '#586e75',
      cursor: '#b58900',
      cursorAccent: '#fdf6e3',
      selectionBackground: 'rgba(181, 137, 0, 0.2)',
      black: '#073642',
      red: '#dc322f',
      green: '#859900',
      yellow: '#b58900',
      blue: '#268bd2',
      magenta: '#d33682',
      cyan: '#2aa198',
      white: '#eee8d5'
    },
    darcula: {
      background: '#2b2b2b',
      foreground: '#a9b7c6',
      cursor: '#cc7832',
      cursorAccent: '#2b2b2b',
      selectionBackground: 'rgba(33, 66, 131, 0.45)',
      black: '#3c3f41',
      red: '#cf6a4c',
      green: '#6a8759',
      yellow: '#bbb529',
      blue: '#6897bb',
      magenta: '#9876aa',
      cyan: '#629755',
      white: '#a9b7c6'
    },
    'intellij-light': {
      background: '#ffffff',
      foreground: '#080808',
      cursor: '#2470c0',
      cursorAccent: '#ffffff',
      selectionBackground: 'rgba(36, 112, 192, 0.2)',
      black: '#080808',
      red: '#c7222d',
      green: '#067d17',
      yellow: '#9e880d',
      blue: '#0033b3',
      magenta: '#871094',
      cyan: '#00627a',
      white: '#f0f0f0'
    }
  };

  let terminalEl: HTMLDivElement;
  let terminal: import('@xterm/xterm').Terminal | null = null;
  let fitAddon: import('@xterm/addon-fit').FitAddon | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let outputUnsubscribe: (() => void) | null = null;
  let isReady = $state(false);
  let error = $state<string | null>(null);
  let scrolledUp = $state(false);

  // Capture instance ID at mount time so onDestroy targets the correct instance
  // (currentInstanceId store may have already changed by the time onDestroy fires)
  let mountedInstanceId: string | null = null;

  // Resolve which instanceId to use: prop or global
  const resolvedInstanceId = $derived(propInstanceId ?? $currentInstanceId);

  // Derived state for showing status banner
  let isDisconnected = $derived($connectionStatus === 'disconnected' || $connectionStatus === 'error');
  let isReconnecting = $derived($connectionStatus === 'connecting' || $connectionStatus === 'reconnecting');
  let isServerGone = $derived($connectionStatus === 'server_gone');
  let showStatusBanner = $derived(isDisconnected || isReconnecting || isServerGone || $hasPendingInput);

  // Multi-user lock state
  let presence = $derived(resolvedInstanceId ? ($instancePresence.get(resolvedInstanceId) ?? []) : []);
  let isMultiUser = $derived(presence.length > 1);
  let showLockBanner = $derived(isMultiUser && ($isLockedByOther || $iHoldLock));

  // When the terminal becomes ready, consume any pending focus request
  $effect(() => {
    if (isReady && paneId && consumeTerminalFocus(paneId)) {
      terminal?.focus();
    }
  });

  // Set up output subscription after terminal is ready
  function setupOutputSubscription() {
    if (outputUnsubscribe) return;

    // Use prop-bound or global output signal
    const outputStore = mountedInstanceId ? terminalHasOutputForInstance(mountedInstanceId) : currentTerminalHasOutput;

    // Subscribe to the derived store that signals when output is available
    outputUnsubscribe = outputStore.subscribe((hasOutput) => {
      if (!hasOutput || !terminal) return;

      const instanceId = mountedInstanceId ?? get(currentInstanceId);
      if (!instanceId) return;

      const buffer = consumeTerminalOutput(instanceId);

      if (buffer.shouldClear) {
        // Full replay incoming — nuke all state so the replay starts
        // from a clean (0,0) cursor.  terminal.clear() alone preserves
        // the cursor row, which garbles the first scrollback line if
        // the cursor wasn't at column 0.
        terminal.clear();
        terminal.write('\x1b[H\x1b[2J');
      }

      // On replay, always pin to bottom — the old buffer was nuked so
      // the previous scroll position is meaningless.
      const pinToBottom = buffer.shouldClear || isAtBottom();

      for (let i = 0; i < buffer.chunks.length; i++) {
        const isLast = i === buffer.chunks.length - 1;
        terminal.write(buffer.chunks[i], isLast && pinToBottom ? () => terminal?.scrollToBottom() : undefined);
      }
    });
  }

  // Check if terminal is scrolled to bottom using xterm's buffer API.
  // The `.xterm-viewport` DOM element doesn't reflect real scroll state
  // in xterm.js 6.0's canvas renderer — use viewportY/baseY instead.
  function isAtBottom(): boolean {
    if (!terminal) return true;
    const buf = terminal.buffer.active;
    return buf.viewportY >= buf.baseY;
  }

  onMount(() => {
    initTerminal();
  });

  async function initTerminal() {
    try {
      // Capture the instance ID NOW — before any async work — so that
      // onDestroy always targets the correct instance even if the user
      // switches instances while this component is still alive.
      mountedInstanceId = propInstanceId ?? get(currentInstanceId);

      // Wait for DOM to be ready - retry a few times as bind:this is async in Svelte 5
      let attempts = 0;
      while (!terminalEl && attempts < 10) {
        await tick();
        await new Promise((resolve) => setTimeout(resolve, 50));
        attempts++;
      }

      if (!terminalEl) {
        throw new Error('Terminal container not available after retries');
      }

      const { Terminal } = await import('@xterm/xterm');
      const { FitAddon } = await import('@xterm/addon-fit');
      const { WebLinksAddon } = await import('@xterm/addon-web-links');
      const { ClipboardAddon } = await import('@xterm/addon-clipboard');
      await import('@xterm/xterm/css/xterm.css');

      const currentTheme = get(theme);
      const settings = get(userSettings);

      terminal = new Terminal({
        cursorBlink: true,
        fontSize: settings.terminalFontSize,
        fontFamily: settings.terminalFontFamily,
        allowProposedApi: true, // Required for clipboard addon
        theme: xtermThemes[currentTheme] ?? xtermThemes.phosphor
      });

      fitAddon = new FitAddon();
      terminal.loadAddon(fitAddon);
      terminal.loadAddon(new WebLinksAddon());
      terminal.loadAddon(new ClipboardAddon());

      terminal.open(terminalEl);

      // Delay fit to ensure container has dimensions
      requestAnimationFrame(() => {
        fitAddon?.fit();
        isReady = true;

        // Discard any Output accumulated while the terminal was hidden
        // and block new Output until the full replay (OutputHistory)
        // arrives.  Without this, stale Output enters xterm.js's async
        // write queue before the replay, and terminal.clear() can't
        // remove queued-but-unprocessed writes — causing duplicated
        // scrollback content on view switch / reload.
        markAwaitingReplay(mountedInstanceId!);

        // Now that terminal is ready, set up output subscription
        setupOutputSubscription();

        // Register this terminal in server-side dimension negotiation
        if (terminal) {
          sendTerminalVisible(terminal.rows, terminal.cols, mountedInstanceId!);
        }
      });

      terminal.onData((data) => {
        // Terminal lock gating: only allow input when appropriate
        if (isMultiUser) {
          if ($isLockedByOther) {
            // Blocked — another user holds the lock
            return;
          }
          if (!$iHoldLock && !$currentTerminalLock?.holder) {
            // Lock unclaimed — auto-acquire on first keystroke
            requestTerminalLock();
          }
        }
        sendInput(data);
        // Scroll to bottom when user types
        terminal?.scrollToBottom();
      });

      terminal.onScroll(() => {
        scrolledUp = !isAtBottom();
      });

      resizeObserver = new ResizeObserver(() => {
        if (fitAddon && terminal && isReady && document.visibilityState === 'visible') {
          fitAddon.fit();
          sendResize(terminal.rows, terminal.cols, mountedInstanceId!);
        }
      });
      resizeObserver.observe(terminalEl);

      // Write welcome message
      terminal.writeln('\x1b[90m--- Terminal connected ---\x1b[0m');
      terminal.writeln('');
    } catch (e) {
      console.error('Failed to initialize terminal:', e);
      error = e instanceof Error ? e.message : 'Failed to load terminal';
    }
  }

  // React to theme changes — swap xterm color palette
  const themeUnsubscribe = theme.subscribe((t) => {
    if (!terminal) return;
    terminal.options.theme = xtermThemes[t] ?? xtermThemes.phosphor;
  });

  // React to font setting changes
  const settingsUnsubscribe = userSettings.subscribe((s) => {
    if (!terminal) return;
    let changed = false;
    if (terminal.options.fontSize !== s.terminalFontSize) {
      terminal.options.fontSize = s.terminalFontSize;
      changed = true;
    }
    if (terminal.options.fontFamily !== s.terminalFontFamily) {
      terminal.options.fontFamily = s.terminalFontFamily;
      changed = true;
    }
    if (changed && fitAddon && isReady) {
      fitAddon.fit();
      sendResize(terminal.rows, terminal.cols, mountedInstanceId!);
    }
  });

  onDestroy(() => {
    // Unregister from server-side dimension negotiation before cleanup.
    // Use the captured mountedInstanceId — NOT the store — because
    // currentInstanceId may have already changed to the next instance.
    if (mountedInstanceId) {
      sendTerminalHidden(mountedInstanceId);
    }

    themeUnsubscribe();
    settingsUnsubscribe();
    outputUnsubscribe?.();
    resizeObserver?.disconnect();
    terminal?.dispose();
    terminal = null;
    fitAddon = null;
  });

  export function clear() {
    terminal?.clear();
  }

  export function write(data: string) {
    terminal?.write(data);
  }
</script>

<div class="terminal-wrapper">
  {#if error}
    <div class="error">
      <span class="error-icon">!</span>
      {error}
    </div>
  {:else if !isReady}
    <div class="loading">
      <span class="spinner"></span>
      Loading terminal...
    </div>
  {/if}
  {#if showStatusBanner && isReady}
    <div
      class="status-banner"
      class:warning={isDisconnected || isServerGone}
      class:info={isReconnecting && !isDisconnected && !isServerGone}
    >
      {#if isServerGone}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M18.364 5.636a9 9 0 11-12.728 0M12 9v4m0 4h.01" />
        </svg>
        <span>{$shutdownReason || 'Server is offline'} — will reconnect automatically when it restarts</span>
        <button class="retry-btn" onclick={() => reconnect()}>Retry Now</button>
      {:else if isDisconnected}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M18.364 5.636a9 9 0 11-12.728 0M12 9v4m0 4h.01" />
        </svg>
        <span>Disconnected</span>
      {:else if isReconnecting}
        <svg class="spinner-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path
            d="M12 2v4m0 12v4m-7.07-3.93l2.83-2.83m8.48 8.48l2.83-2.83M2 12h4m12 0h4M4.93 4.93l2.83 2.83m8.48 8.48l2.83 2.83"
          />
        </svg>
        <span>Reconnecting...</span>
      {/if}
      {#if $hasPendingInput}
        <span class="pending-badge">Input queued</span>
      {/if}
    </div>
  {/if}
  {#if showLockBanner && isReady}
    <div class="lock-banner" class:locked-by-other={$isLockedByOther} class:i-hold={$iHoldLock}>
      {#if $isLockedByOther}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
          <path d="M7 11V7a5 5 0 0110 0v4" />
        </svg>
        <span>Terminal controlled by <strong>{$currentTerminalLock?.holder?.display_name}</strong></span>
        <button class="lock-action-btn" onclick={requestTerminalLock}>Take Control</button>
      {:else if $iHoldLock}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
          <path d="M7 11V7a5 5 0 0110 0v4" />
        </svg>
        <span>You have terminal control</span>
        <button class="lock-action-btn release" onclick={releaseTerminalLock}>Release</button>
      {/if}
    </div>
  {/if}
  <div class="terminal-container" class:hidden={!isReady || error} bind:this={terminalEl}></div>
  {#if scrolledUp && isReady}
    <button class="scroll-bottom-btn" onclick={() => { terminal?.scrollToBottom(); }}>
      &#9660; LATEST
    </button>
  {/if}
</div>

<style>
  .terminal-wrapper {
    width: 100%;
    height: 100%;
    position: relative;
    display: flex;
    flex-direction: column;
    background: var(--surface-900);
  }

  .loading,
  .error {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: var(--text-muted);
    font-size: 12px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  .error {
    color: var(--status-red);
  }

  .error-icon {
    width: 24px;
    height: 24px;
    background: var(--status-red-strong);
    border: 1px solid var(--status-red-border);
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: bold;
    color: var(--status-red);
  }

  .spinner {
    width: 14px;
    height: 14px;
    border: 2px solid var(--surface-border);
    border-top-color: var(--chrome-accent-500);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .terminal-container {
    width: 100%;
    flex: 1;
    min-height: 0;
  }

  .terminal-container.hidden {
    visibility: hidden;
  }

  .status-banner {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 14px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    flex-shrink: 0;
  }

  .status-banner.warning {
    background: var(--status-red-tint);
    border-bottom: 1px solid var(--status-red-border);
    color: var(--status-red-text);
  }

  .status-banner.info {
    background: var(--tint-active-strong);
    border-bottom: 1px solid var(--tint-focus);
    color: var(--chrome-accent-400);
  }

  .status-banner svg {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
  }

  .spinner-icon {
    animation: spin 1s linear infinite;
  }

  .retry-btn {
    margin-left: auto;
    padding: 4px 10px;
    background: var(--tint-focus);
    border: 1px solid var(--status-red-border);
    border-radius: 4px;
    font-size: 10px;
    font-weight: 600;
    font-family: inherit;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--status-red-text);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .retry-btn:hover {
    background: var(--status-red-tint);
    border-color: var(--status-red);
  }

  .pending-badge {
    margin-left: auto;
    padding: 4px 10px;
    background: var(--tint-focus);
    border: 1px solid var(--tint-selection);
    border-radius: 4px;
    font-size: 10px;
    font-weight: 600;
    color: var(--chrome-accent-400);
  }

  .lock-banner {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 14px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.05em;
    flex-shrink: 0;
  }

  .lock-banner.locked-by-other {
    background: var(--status-red-tint);
    border-bottom: 1px solid var(--status-red-border);
    color: var(--status-red-text);
  }

  .lock-banner.i-hold {
    background: var(--status-green-tint);
    border-bottom: 1px solid var(--status-green-border);
    color: var(--status-green-text);
  }

  .lock-banner svg {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
  }

  .lock-action-btn {
    margin-left: auto;
    padding: 4px 10px;
    background: var(--tint-focus);
    border: 1px solid var(--tint-selection);
    border-radius: 4px;
    font-size: 10px;
    font-weight: 600;
    font-family: inherit;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--chrome-accent-400);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .lock-action-btn:hover {
    background: var(--tint-selection);
    border-color: var(--tint-selection);
  }

  .lock-action-btn.release {
    background: var(--status-green-tint);
    border-color: var(--status-green-border);
    color: var(--status-green-text);
  }

  .lock-action-btn.release:hover {
    background: var(--status-green-border);
    border-color: var(--status-green);
  }

  .scroll-bottom-btn {
    position: absolute;
    bottom: 12px;
    right: 20px;
    padding: 4px 12px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    font-size: 10px;
    font-weight: 600;
    font-family: inherit;
    letter-spacing: 0.1em;
    color: var(--chrome-accent-400);
    cursor: pointer;
    z-index: 10;
    transition: all 0.15s ease;
    box-shadow: var(--elevation-low);
  }

  .scroll-bottom-btn:hover {
    background: var(--tint-focus);
    border-color: var(--tint-selection);
    box-shadow: var(--elevation-high);
  }

  .terminal-container :global(.xterm) {
    padding: 10px;
    height: 100%;
  }

  .terminal-container :global(.xterm-viewport) {
    background-color: transparent !important;
  }

  /* Terminal cursor glow */
  .terminal-container :global(.xterm-cursor-block) {
    box-shadow: 0 0 8px var(--chrome-accent-500);
  }

  /* Mobile responsive */
  @media (max-width: 639px) {
    .terminal-container :global(.xterm) {
      padding: 6px;
    }

    .status-banner {
      padding: 8px 12px;
      font-size: 10px;
      flex-wrap: wrap;
    }

    .pending-badge {
      margin-left: 0;
      margin-top: 6px;
      width: 100%;
      text-align: center;
    }

    .loading,
    .error {
      font-size: 11px;
      gap: 10px;
    }
  }
</style>
