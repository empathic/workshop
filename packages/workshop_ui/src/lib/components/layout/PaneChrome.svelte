<script lang="ts">
  import type { PaneState, PaneContentKind, PaneContent } from '$lib/stores/layout';
  import {
    paneCount,
    splitPane,
    closePane,
    setPaneContent,
    getPaneInstanceId,
    getPaneWorkingDir,
    defaultContentForKind
  } from '$lib/stores/layout';
  import {
    SELECTABLE_KINDS,
    INSTANCE_BOUND_KINDS,
    DIR_BOUND_KINDS
  } from '$lib/utils/pane-content';
  import {
    instances,
    instanceList,
    selectInstance,
    createInstance,
    deleteInstance
  } from '$lib/stores/instances';
  import { sendRefresh } from '$lib/stores/websocket';
  import { openExplorerPicker } from '$lib/stores/files';
  import { userSettings, theme } from '$lib/stores/settings';
  import { activityLevel } from '$lib/stores/activity';
  import CreateInstanceModal from '../CreateInstanceModal.svelte';

  interface Props {
    pane: PaneState;
  }

  let { pane }: Props = $props();

  const canClose = $derived($paneCount > 1);

  // Whether this pane is instance-bound (terminal/conversation) vs directory-bound
  const isInstanceBound = $derived(INSTANCE_BOUND_KINDS.has(pane.content.kind));
  const isDirBound = $derived(DIR_BOUND_KINDS.has(pane.content.kind) && pane.content.kind !== 'file-viewer');

  // Project label for directory-bound panes
  const dirLabel = $derived.by(() => {
    if (!isDirBound || !('workingDir' in pane.content)) return null;
    const wd = pane.content.workingDir;
    if (!wd) return 'No project';
    return wd.replace(/\/+$/, '').split('/').pop() ?? wd;
  });

  const paneInstanceId = $derived(getPaneInstanceId(pane.content));

  // All selectable content kinds are always available — switching to terminal auto-creates
  // a shell, switching to conversation shows the instance picker if no structured instance
  // is bound. Derived from the single-source-of-truth registry.

  // Terminal panes show only shell instances; conversation panes show only structured instances
  const filteredInstances = $derived(
    isInstanceBound
      ? $instanceList.filter((inst) =>
          pane.content.kind === 'terminal' ? inst.kind.type === 'Unstructured' : inst.kind.type === 'Structured'
        )
      : []
  );

  // Instance status indicator for terminal/conversation panes
  const instanceStatus = $derived.by((): 'thinking' | 'responding' | 'tool' | 'idle' | null => {
    if (!paneInstanceId) return null;
    const kind = pane.content.kind;
    if (kind !== 'terminal' && kind !== 'conversation') return null;
    const inst = $instances.get(paneInstanceId);
    if (!inst) return null;
    const cs = inst.claude_state;
    if (!cs) return 'idle';
    if (cs.type === 'Thinking') return 'thinking';
    if (cs.type === 'Responding') return 'responding';
    if (cs.type === 'ToolExecuting') return 'tool';
    return 'idle';
  });

  const statusLabel = $derived.by(() => {
    if (instanceStatus === 'thinking') return 'Claude is thinking';
    if (instanceStatus === 'responding') return 'Claude is responding';
    if (instanceStatus === 'tool') return 'Claude is executing a tool';
    return null;
  });

  // Activity meter: visible whenever the instance is producing output
  const showActivityFill = $derived(pane.content.kind === 'conversation' && $activityLevel > 0);

  const BLOCK_COUNT = 80;
  // √x curve: 25% baud → half bar, compresses asymptotically toward full.
  // Keeps the visual action in the middle where oscillation is most readable.
  const litBlocks = $derived(Math.max(1, Math.ceil(Math.sqrt($activityLevel) * BLOCK_COUNT)));

  // Read accent ramp endpoints from CSS variables so it follows the active theme.
  // Falls back to amber if variables are unavailable.
  function buildMeterColors(): string[] {
    const style = typeof document !== 'undefined' ? getComputedStyle(document.body) : null;
    const lo = style?.getPropertyValue('--accent-600').trim() || '#d97706';
    const hi = style?.getPropertyValue('--accent-400').trim() || '#fdba74';
    const parse = (hex: string) => {
      const h = hex.replace('#', '');
      return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
    };
    const [r0, g0, b0] = parse(lo);
    const [r1, g1, b1] = parse(hi);
    return Array.from({ length: BLOCK_COUNT }, (_, i) => {
      const t = i / (BLOCK_COUNT - 1);
      return `rgb(${Math.round(r0 + t * (r1 - r0))},${Math.round(g0 + t * (g1 - g0))},${Math.round(b0 + t * (b1 - b0))})`;
    });
  }

  let meterColors = $state(buildMeterColors());

  // Rebuild palette when theme changes
  $effect(() => {
    // Subscribe to theme store to trigger rebuild
    void $theme;
    // Tick delay so CSS variables are applied before we read them
    requestAnimationFrame(() => {
      meterColors = buildMeterColors();
    });
  });

  function blockColor(i: number): string {
    return meterColors[i];
  }

  // File name for file-viewer chrome
  const fileViewerLabel = $derived.by(() => {
    if (pane.content.kind !== 'file-viewer') return null;
    const fp = pane.content.filePath;
    if (!fp) return 'No file';
    const name = fp.split('/').pop() ?? fp;
    return name.length > 20 ? name.slice(0, 20) + '\u2026' : name;
  });

  // Scope label for chat chrome
  const chatScopeLabel = $derived.by(() => {
    if (pane.content.kind !== 'chat') return null;
    return pane.content.scope === 'global' ? 'Global' : 'Instance';
  });

  // -- Split popover state --
  let splitPopover = $state<{ direction: 'vertical' | 'horizontal'; x: number; y: number } | null>(null);
  let hoveredKind = $state<PaneContentKind | null>(null);
  /** True once the pointer has entered the popover during this drag */
  let enteredPopover = $state(false);

  const POPOVER_WIDTH = 148; // min-width + padding + border

  function handleSplitPointerDown(direction: 'vertical' | 'horizontal', e: PointerEvent) {
    const btn = e.currentTarget as HTMLElement;
    const rect = btn.getBoundingClientRect();
    // Clamp horizontally so popover stays within viewport
    const x = Math.min(rect.left, window.innerWidth - POPOVER_WIDTH - 4);
    splitPopover = { direction, x, y: rect.bottom + 2 };
    hoveredKind = null;
    enteredPopover = false;
    btn.setPointerCapture(e.pointerId);
  }

  function handleSplitPointerMove(e: PointerEvent) {
    if (!splitPopover) return;
    const el = document.elementFromPoint(e.clientX, e.clientY);
    if (!el) {
      hoveredKind = null;
      return;
    }
    const item = el.closest('[data-split-kind]') as HTMLElement | null;
    const kind = item ? (item.dataset.splitKind as PaneContentKind) : null;
    if (kind) enteredPopover = true;
    hoveredKind = kind;
  }

  function handleSplitPointerUp(e: PointerEvent) {
    if (!splitPopover) return;
    const dir = splitPopover.direction;
    const kind = hoveredKind;
    const didEnter = enteredPopover;
    splitPopover = null;
    hoveredKind = null;
    enteredPopover = false;
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);

    if (kind) {
      // Released on a popover item → split with that kind
      if (kind === 'terminal') {
        // Auto-create a shell for the new pane using configured command
        const wd = getPaneWorkingDir(pane.content, $instances);
        createInstance({ command: $userSettings.shellCommand || 'bash', working_dir: wd ?? undefined }).then(
          (result) => {
            if (result) {
              splitPane(pane.id, dir, { kind: 'terminal', instanceId: result.id });
            }
          }
        );
        return;
      }
      const workingDir = getPaneWorkingDir(pane.content, $instances);
      splitPane(pane.id, dir, defaultContentForKind(kind, workingDir));
    } else if (!didEnter) {
      // Plain click (never entered popover) → split with picker
      splitPane(pane.id, dir);
    }
    // Dragged into popover then off → cancel
  }

  function handleClose() {
    closePane(pane.id);
  }

  async function handleContentChange(e: Event) {
    const newKind = (e.target as HTMLSelectElement).value as PaneContentKind;
    // Switching pane type is always a fresh open — never carry an instance binding
    // across incompatible kinds. Only working directory context is preserved.
    const workingDir = getPaneWorkingDir(pane.content, $instances);
    if (newKind === 'terminal') {
      const result = await createInstance({
        command: $userSettings.shellCommand || 'bash',
        working_dir: workingDir ?? undefined
      });
      if (result) {
        setPaneContent(pane.id, { kind: 'terminal', instanceId: result.id });
      }
      return;
    }
    setPaneContent(pane.id, defaultContentForKind(newKind, workingDir));
  }

  let showCreateModal = $state(false);

  function handleInstanceChange(e: Event) {
    const select = e.target as HTMLSelectElement;
    const value = select.value;

    if (value === '__new__') {
      // Reset select to current value and open modal
      select.value = paneInstanceId ?? '';
      showCreateModal = true;
      return;
    }

    const newId = value || null;
    if (pane.content.kind === 'terminal' || pane.content.kind === 'conversation') {
      setPaneContent(pane.id, { ...pane.content, instanceId: newId });
    }
  }

  function handleCreated(instanceId: string) {
    if (pane.content.kind === 'terminal' || pane.content.kind === 'conversation') {
      setPaneContent(pane.id, { ...pane.content, instanceId });
      selectInstance(instanceId);
    }
  }

  // -- Terminal command editing --
  const isTerminal = $derived(pane.content.kind === 'terminal');
  const currentInstance = $derived(paneInstanceId ? $instances.get(paneInstanceId) : null);
  let termCommand = $state('');
  let termCommandDirty = $state(false);

  // Sync command from instance when instance changes
  $effect(() => {
    if (currentInstance) {
      termCommand = currentInstance.command;
      termCommandDirty = false;
    }
  });

  function handleCommandInput(e: Event) {
    termCommand = (e.target as HTMLInputElement).value;
    termCommandDirty = termCommand !== (currentInstance?.command ?? '');
  }

  async function handleRestart() {
    const cmd = termCommand.trim() || $userSettings.shellCommand || 'bash';
    const oldId = paneInstanceId;
    const result = await createInstance({
      command: cmd,
      working_dir: currentInstance?.working_dir ?? undefined
    });
    if (result) {
      setPaneContent(pane.id, { kind: 'terminal', instanceId: result.id });
      if (oldId) deleteInstance(oldId);
    }
  }

  function handleCommandKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      handleRestart();
    }
  }

  // -- Responsive: hide split buttons when pane is too narrow --
  let chromeEl: HTMLElement | undefined = $state();
  let chromeWidth = $state(Infinity);

  $effect(() => {
    if (!chromeEl) return;
    const ro = new ResizeObserver(([entry]) => {
      chromeWidth = entry.contentRect.width;
    });
    ro.observe(chromeEl);
    return () => ro.disconnect();
  });

  const showSplitButtons = $derived(chromeWidth > 180);
</script>

<div class="pane-chrome" bind:this={chromeEl}>
  {#if showActivityFill}
    <span class="chrome-activity-meter">
      {#each Array(BLOCK_COUNT) as _, i}
        <span
          class="chrome-activity-block"
          style={i < litBlocks ? `background-color: ${blockColor(i)}` : ''}
        ></span>
      {/each}
    </span>
  {/if}
  {#if instanceStatus && instanceStatus !== 'idle'}
    <span
      class="status-dot"
      class:thinking={instanceStatus === 'thinking'}
      class:responding={instanceStatus === 'responding'}
      class:tool={instanceStatus === 'tool'}
      title={statusLabel}
      role="status"
      aria-label={statusLabel}
    ></span>
  {/if}
  <select
    class="pane-type-select"
    value={pane.content.kind}
    onchange={handleContentChange}
    aria-label="Pane content type"
  >
    {#each SELECTABLE_KINDS as def}
      <option value={def.kind}>{def.label}</option>
    {/each}
  </select>
  {#if isTerminal && paneInstanceId}
    <span class="chrome-sep">/</span>
    <input
      class="command-input"
      type="text"
      value={termCommand}
      oninput={handleCommandInput}
      onkeydown={handleCommandKeydown}
      aria-label="Shell command"
      spellcheck="false"
    />
    {#if termCommandDirty}
      <button
        class="chrome-btn restart"
        onclick={handleRestart}
        title="Restart with new command"
        aria-label="Restart shell"
      >
        <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5">
          <path d="M2 8a6 6 0 0 1 10.2-4.3" />
          <path d="M14 8a6 6 0 0 1-10.2 4.3" />
          <polyline points="12 2 12.5 4 10.5 4.5" />
          <polyline points="4 12 3.5 12 3.5 14" />
        </svg>
      </button>
    {/if}
  {:else if isInstanceBound}
    <span class="chrome-sep">/</span>
    <select class="instance-select" value={paneInstanceId ?? ''} onchange={handleInstanceChange} aria-label="Instance">
      <option value="">None</option>
      {#each filteredInstances as inst}
        <option value={inst.id}>{inst.custom_name ?? inst.name}</option>
      {/each}
      <option value="__new__">+ New</option>
    </select>
  {:else if isDirBound}
    <span class="chrome-sep">/</span>
    <span class="chrome-label">{dirLabel}</span>
  {:else if pane.content.kind === 'file-viewer'}
    <span class="chrome-sep">/</span>
    <button
      class="chrome-label chrome-label-btn"
      onclick={() => {
        const wd = getPaneWorkingDir(pane.content, $instances);
        openExplorerPicker((path) => {
          setPaneContent(pane.id, { kind: 'file-viewer', filePath: path, workingDir: wd });
        });
      }}
      title="Browse files"
    >
      {fileViewerLabel}
    </button>
  {:else if pane.content.kind === 'chat'}
    <span class="chrome-sep">/</span>
    <span class="chrome-label">{chatScopeLabel}</span>
  {/if}
  <div class="pane-spacer"></div>
  {#if pane.content.kind !== 'landing'}
    <div class="pane-actions">
      {#if canClose}
        <button class="chrome-btn close" onclick={handleClose} title="Close pane (Cmd+W)" aria-label="Close pane">
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5">
            <line x1="4" y1="4" x2="12" y2="12" />
            <line x1="12" y1="4" x2="4" y2="12" />
          </svg>
        </button>
      {/if}
      {#if showSplitButtons}
        <button
          class="chrome-btn"
          onpointerdown={(e) => handleSplitPointerDown('vertical', e)}
          onpointermove={handleSplitPointerMove}
          onpointerup={handleSplitPointerUp}
          title="Split vertical (Cmd+\)"
          aria-label="Split pane vertically"
        >
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5">
            <rect x="1" y="1" width="14" height="14" rx="1" />
            <line x1="8" y1="1" x2="8" y2="15" />
          </svg>
        </button>
        <button
          class="chrome-btn"
          onpointerdown={(e) => handleSplitPointerDown('horizontal', e)}
          onpointermove={handleSplitPointerMove}
          onpointerup={handleSplitPointerUp}
          title="Split horizontal"
          aria-label="Split pane horizontally"
        >
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5">
            <rect x="1" y="1" width="14" height="14" rx="1" />
            <line x1="1" y1="8" x2="15" y2="8" />
          </svg>
        </button>
      {/if}
      {#if isTerminal && paneInstanceId}
        <button
          class="chrome-btn"
          onclick={() => sendRefresh(paneInstanceId!)}
          title="Refresh terminal"
          aria-label="Refresh terminal"
        >
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5">
            <path d="M2.5 8a5.5 5.5 0 0 1 9.3-4" />
            <path d="M13.5 8a5.5 5.5 0 0 1-9.3 4" />
            <polyline points="11.5 2 12 4.2 9.8 4.5" />
            <polyline points="4.5 14 4 11.8 6.2 11.5" />
          </svg>
        </button>
      {/if}
    </div>
  {/if}
</div>

{#if showCreateModal}
  <CreateInstanceModal onclose={() => (showCreateModal = false)} oncreated={handleCreated} />
{/if}

{#if splitPopover}
  <div class="split-popover" style="left: {splitPopover.x}px; top: {splitPopover.y}px;">
    {#each SELECTABLE_KINDS as def}
      <div class="split-popover-item" class:hovered={hoveredKind === def.kind} data-split-kind={def.kind}>
        <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.2" class="split-popover-icon">
          {@html def.chromeIcon}
        </svg>
        <span class="split-popover-label">{def.label}</span>
      </div>
    {/each}
  </div>
{/if}

<style>
  .pane-chrome {
    position: relative;
    overflow: hidden;
    display: flex;
    align-items: center;
    height: 24px;
    padding: 0 8px;
    background: var(--surface-700);
    border-bottom: 1px solid var(--surface-border);
    flex-shrink: 0;
    gap: 4px;
  }

  .chrome-activity-meter {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    right: 0;
    display: flex;
    gap: 1px;
    padding: 5px 0;
    opacity: 0.25;
    pointer-events: none;
  }

  .chrome-activity-block {
    flex: 1;
    border-radius: 1px;
    transition: background-color 0.15s ease;
  }

  .status-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    animation: dot-pulse 0.8s ease-in-out infinite;
  }

  .status-dot.thinking {
    background: var(--thinking-500);
  }

  .status-dot.responding,
  .status-dot.tool {
    background: var(--chrome-accent-500);
  }

  @keyframes dot-pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.5;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .status-dot {
      animation: none;
    }
  }

  .pane-type-select {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-muted);
    background: transparent;
    border: none;
    cursor: pointer;
    font-family: inherit;
    padding: 0;
    outline: none;
    appearance: none;
    -webkit-appearance: none;
  }

  .pane-type-select:hover {
    color: var(--text-secondary);
  }

  .pane-type-select option {
    background: var(--surface-600);
    color: var(--text-primary);
    text-transform: none;
    letter-spacing: normal;
  }

  .command-input {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.05em;
    color: var(--text-secondary);
    background: transparent;
    border: none;
    border-bottom: 1px solid transparent;
    font-family: inherit;
    padding: 0 2px;
    outline: none;
    max-width: 140px;
    min-width: 40px;
  }

  .command-input:focus {
    border-bottom-color: var(--chrome-accent-600);
    color: var(--text-primary);
  }

  .chrome-btn.restart:hover {
    background: var(--tint-hover);
    color: var(--chrome-accent-400);
  }

  .chrome-sep {
    color: var(--text-muted);
    opacity: 0.3;
    font-size: 10px;
    flex-shrink: 0;
  }

  .instance-select {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.05em;
    color: var(--text-secondary);
    background: transparent;
    border: none;
    cursor: pointer;
    font-family: inherit;
    padding: 0;
    outline: none;
    appearance: none;
    -webkit-appearance: none;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .instance-select:hover {
    color: var(--chrome-accent-400);
  }

  .instance-select option {
    background: var(--surface-600);
    color: var(--text-primary);
    letter-spacing: normal;
  }

  .chrome-label {
    font-size: 10px;
    font-weight: 600;
    color: var(--text-muted);
    letter-spacing: 0.05em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 120px;
  }

  .chrome-label-btn {
    background: none;
    border: none;
    padding: 1px 4px;
    margin: -1px -4px;
    border-radius: 3px;
    cursor: pointer;
    font-family: inherit;
    text-align: left;
    transition: all 0.15s ease;
  }

  .chrome-label-btn:hover {
    background: var(--tint-hover);
    color: var(--chrome-accent-400);
  }

  .pane-spacer {
    flex: 1;
  }

  .pane-actions {
    position: relative;
    display: flex;
    gap: 2px;
    flex-shrink: 0;
    padding: 0 4px;
    margin-right: -8px;
    padding-right: 8px;
    background: color-mix(in srgb, var(--surface-700) 80%, transparent);
    border-radius: 3px;
  }

  .chrome-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: 2px;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 0.1s ease;
  }

  .chrome-btn:hover {
    background: var(--tint-hover);
    color: var(--text-secondary);
  }

  .chrome-btn.close:hover {
    background: var(--status-red-tint);
    color: var(--status-red);
  }

  .chrome-btn svg {
    width: 12px;
    height: 12px;
  }

  .split-popover {
    position: fixed;
    z-index: 100;
    display: flex;
    flex-direction: column;
    min-width: 140px;
    padding: 4px;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    box-shadow: var(--shadow-dropdown);
    pointer-events: none;
  }

  .split-popover-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 8px;
    border-radius: 2px;
    pointer-events: auto;
  }

  .split-popover-item.hovered {
    background: var(--tint-hover);
  }

  .split-popover-item.hovered .split-popover-icon {
    color: var(--chrome-accent-400);
  }

  .split-popover-item.hovered .split-popover-label {
    color: var(--chrome-accent-400);
  }

  .split-popover-icon {
    width: 12px;
    height: 12px;
    flex-shrink: 0;
    color: var(--text-muted);
  }

  .split-popover-label {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-secondary);
    white-space: nowrap;
  }
</style>
