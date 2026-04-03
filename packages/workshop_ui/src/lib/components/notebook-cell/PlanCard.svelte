<script lang="ts">
  import type { ToolCell } from '$lib/types';
  import { allToolCells } from '$lib/stores/conversation';
  import { renderMarkdown } from '$lib/utils/markdown';
  import { getPlanContent, parseAllowedPrompts, parseStatusText } from '$lib/utils/plan';
  import { getContext } from 'svelte';
  import { setPaneViewMode } from '$lib/stores/layout';
  import PlanModal from './PlanModal.svelte';

  const paneCtx = getContext<{ readonly id: string }>('paneId');

  interface Props {
    tool: ToolCell;
  }

  let { tool }: Props = $props();

  let showRaw = $state(false);
  let planExpanded = $state(true);
  let selectedVersion: number | null = $state(null);
  let modalOpen = $state(false);

  // ── Status ──────────────────────────────────────────────────────────
  const isResolved = $derived(!!tool.output);
  const isPending = $derived(!tool.output && !tool.is_error);
  const isError = $derived(!!tool.is_error);

  const planContent = $derived(getPlanContent(tool));
  const renderedPlan = $derived(planContent ? renderMarkdown(planContent) : null);
  const statusText = $derived(parseStatusText(tool.output));

  // ── Version history ─────────────────────────────────────────────────
  const allVersions: ToolCell[] = $derived(
    ($allToolCells ?? [])
      .filter((t: ToolCell) => t.name === 'ExitPlanMode')
      .sort((a: ToolCell, b: ToolCell) => a.timestamp.localeCompare(b.timestamp))
  );

  const currentVersionIndex = $derived(allVersions.findIndex((v: ToolCell) => v.id === tool.id));

  const totalVersions = $derived(allVersions.length);
  const hasMultipleVersions = $derived(totalVersions > 1);
  const isLatestVersion = $derived(currentVersionIndex === totalVersions - 1);

  // Version-tab state — only meaningful for the latest (full) card.
  // Collapsed cards short-circuit to avoid renderMarkdown and redundant derivations.
  const activeVersion = $derived(selectedVersion ?? currentVersionIndex);

  const activeToolCell: ToolCell = $derived.by(() => {
    if (!isLatestVersion) return tool;
    return activeVersion >= 0 && activeVersion < allVersions.length ? allVersions[activeVersion] : tool;
  });

  const activePlanContent: string | null = $derived.by(() => {
    if (!isLatestVersion) return null;
    return activeVersion === currentVersionIndex ? planContent : getPlanContent(activeToolCell);
  });

  const activeRenderedPlan: string | null = $derived.by(() => {
    if (!isLatestVersion) return null;
    return activePlanContent ? renderMarkdown(activePlanContent) : null;
  });

  const activeAllowedPrompts = $derived.by(() => {
    if (!isLatestVersion) return [];
    return parseAllowedPrompts(activeToolCell.input.allowedPrompts);
  });

  // Collapse plan content for resolved cards by default
  $effect(() => {
    if (isResolved) {
      planExpanded = false;
    }
  });
</script>

{#if hasMultipleVersions && !isLatestVersion}
  <!-- Older version: collapsed one-liner — the latest card has version tabs to access this -->
  <div class="plan-card resolved plan-collapsed">
    <div class="collapsed-row">
      <span class="header-label">PLAN</span>
      <span class="version-indicator">v{currentVersionIndex + 1}</span>
      <span
        class="collapsed-status"
        class:status-rejected={statusText === 'REJECTED'}
        class:status-changes={statusText === 'CHANGES REQUESTED'}
      >
        {statusText ?? (isError ? 'ERROR' : isResolved ? 'RESOLVED' : 'PENDING')}
      </span>
    </div>
  </div>
{:else}
  <div class="plan-card" class:pending={isPending} class:resolved={isResolved} class:error={isError}>
    {#if showRaw}
      <!-- Raw view -->
      <div class="raw-view">
        <div class="raw-header">
          <span class="raw-title">RAW — EXITPLANMODE</span>
          <button class="toggle-raw" onclick={() => (showRaw = false)} title="Show rendered">&#9670;</button>
        </div>
        <div class="raw-field">
          <span class="raw-label">INPUT</span>
          <pre class="raw-value">{JSON.stringify(tool.input, null, 2)}</pre>
        </div>
        <div class="raw-field">
          <span class="raw-label">OUTPUT</span>
          <pre class="raw-value">{tool.output ?? '(none)'}</pre>
        </div>
        <div class="raw-field">
          <span class="raw-label">STATUS</span>
          <pre class="raw-value">{isResolved ? (statusText ?? 'resolved') : isPending ? 'pending' : 'error'}</pre>
        </div>
      </div>
    {:else}
      <!-- Header -->
      <div class="card-header">
        <div class="header-left">
          <span class="header-label">PLAN REVIEW</span>
          {#if hasMultipleVersions}
            <span class="version-indicator">v{currentVersionIndex + 1} of {totalVersions}</span>
            <div class="version-tabs">
              {#each allVersions as _, i}
                <button
                  class="version-tab"
                  class:active={activeVersion === i}
                  class:current={i === currentVersionIndex}
                  class:dimmed={allVersions[i].output && i !== currentVersionIndex}
                  onclick={() => (selectedVersion = selectedVersion === i ? null : i)}
                >
                  {i + 1}
                </button>
              {/each}
            </div>
          {/if}
          <span
            class="status-led"
            class:led-pending={isPending}
            class:led-resolved={isResolved && !isError}
            class:led-error={isError}
          ></span>
        </div>
        <div class="header-actions">
          <button class="expand-btn" onclick={() => (modalOpen = true)} title="Expand plan"><span class="expand-icon"></span></button>
          <button class="toggle-raw" onclick={() => (showRaw = true)} title="Show raw">&#9671;</button>
        </div>
      </div>

      <!-- Plan content area -->
      {#if activeRenderedPlan}
        <div class="plan-section">
          <button class="plan-toggle" onclick={() => (planExpanded = !planExpanded)}>
            <span class="section-label">PLAN</span>
            <span class="toggle-icon">{planExpanded ? '\u25BC' : '\u25B6'}</span>
          </button>
          {#if planExpanded}
            <div class="plan-content markdown-body">
              {@html activeRenderedPlan}
            </div>
          {/if}
        </div>
      {:else if activePlanContent}
        <div class="plan-section">
          <button class="plan-toggle" onclick={() => (planExpanded = !planExpanded)}>
            <span class="section-label">PLAN</span>
            <span class="toggle-icon">{planExpanded ? '\u25BC' : '\u25B6'}</span>
          </button>
          {#if planExpanded}
            <pre class="plan-content-raw">{activePlanContent}</pre>
          {/if}
        </div>
      {:else}
        <div class="plan-fallback">
          <span class="fallback-text">Plan written to file</span>
        </div>
      {/if}

      <!-- Permissions section -->
      {#if activeAllowedPrompts.length > 0}
        <div class="permissions-section">
          <span class="section-label">REQUESTED PERMISSIONS</span>
          <div class="permissions-list">
            {#each activeAllowedPrompts as perm}
              <div class="permission-row">
                <span class="permission-tool">{perm.tool}</span>
                <span class="permission-prompt">{perm.prompt}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      <!-- Result section (when resolved) -->
      {#if isResolved && tool.output}
        <div class="result-section" class:error-result={isError}>
          <span class="result-label">{statusText ?? (isError ? 'ERROR' : 'RESULT')}</span>
          {#if statusText === null}
            <pre class="result-value">{tool.output}</pre>
          {/if}
        </div>
      {/if}

      <!-- Pending banner -->
      {#if isPending}
        <button class="pending-banner" onclick={() => setPaneViewMode(paneCtx.id, 'raw')}>
          <span class="pending-icon">&#9000;</span>
          <span class="pending-text">Switch to Terminal to approve plan</span>
        </button>
      {/if}
    {/if}
  </div>
{/if}

{#if modalOpen}
  <PlanModal
    {tool}
    {allVersions}
    {currentVersionIndex}
    {statusText}
    {isResolved}
    {isPending}
    {isError}
    onclose={() => (modalOpen = false)}
  />
{/if}

<style>
  .plan-card {
    border: 1px solid var(--accent-600);
    border-radius: 4px;
    background: var(--surface-800);
    overflow: hidden;
    animation: card-on 0.3s ease-out;
  }

  .plan-card.pending {
    border-color: var(--accent-500);
    box-shadow: var(--elevation-low);
  }

  .plan-card.resolved {
    border-color: var(--surface-border);
    opacity: 0.85;
  }

  .plan-card.error {
    border-color: var(--status-red);
  }

  /* ── Collapsed (older version) ────────────── */

  .plan-collapsed {
    animation: none;
  }

  .collapsed-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
  }

  .collapsed-status {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .collapsed-status.status-rejected {
    color: var(--status-red);
  }

  .collapsed-status.status-changes {
    color: var(--accent-400);
  }

  /* ── Header ──────────────────────────────── */

  .card-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-bottom: 1px solid var(--surface-border);
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .header-label {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.15em;
    color: var(--accent-400);
    text-shadow: var(--emphasis);
  }

  .version-indicator {
    font-size: 9px;
    font-weight: 600;
    letter-spacing: 0.08em;
    color: var(--text-muted);
    text-transform: uppercase;
  }

  .version-tabs {
    display: flex;
    gap: 2px;
  }

  .version-tab {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    padding: 0;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    font-family: inherit;
    font-size: 9px;
    font-weight: 700;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .version-tab:hover {
    background: var(--surface-600);
    border-color: var(--accent-600);
    color: var(--accent-400);
  }

  .version-tab.active {
    background: var(--tint-active);
    border-color: var(--accent-500);
    color: var(--accent-400);
  }

  .version-tab.dimmed:not(.active) {
    opacity: 0.4;
  }

  .status-led {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .status-led.led-pending {
    background: var(--accent-500);
    box-shadow: 0 0 6px var(--accent-500);
    animation: led-pulse 1s ease-in-out infinite alternate;
  }

  .status-led.led-resolved {
    background: var(--status-green);
    box-shadow: 0 0 4px var(--status-green);
  }

  .status-led.led-error {
    background: var(--status-red);
    box-shadow: 0 0 6px var(--status-red);
  }

  /* ── Header actions ──────────────────────── */

  .header-actions {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .toggle-raw,
  .expand-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 12px;
    padding: 2px 6px;
    border-radius: 3px;
    opacity: 0.3;
    transition: all 0.15s ease;
  }

  .plan-card:hover .toggle-raw,
  .plan-card:hover .expand-btn {
    opacity: 0.8;
  }

  .toggle-raw:hover,
  .expand-btn:hover {
    background: var(--surface-500);
    color: var(--accent-400);
  }

  .expand-icon {
    display: inline-block;
    width: 10px;
    height: 10px;
    border: 1.5px solid currentColor;
    border-radius: 1px;
    position: relative;
  }

  .expand-icon::after {
    content: '';
    position: absolute;
    top: -1px;
    right: -1px;
    width: 0;
    height: 0;
    border-style: solid;
    border-width: 0 5px 5px 0;
    border-color: transparent currentColor transparent transparent;
  }

  /* ── Raw view ────────────────────────────── */

  .raw-view {
    padding: 0;
  }

  .raw-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 10px;
    border-bottom: 1px solid var(--surface-border);
  }

  .raw-title {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.15em;
    color: var(--accent-400);
  }

  .raw-field {
    padding: 6px 10px;
    border-bottom: 1px solid var(--surface-border);
  }

  .raw-field:last-child {
    border-bottom: none;
  }

  .raw-label {
    display: block;
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.15em;
    color: var(--text-muted);
    margin-bottom: 2px;
  }

  .raw-value {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-all;
    font-family: inherit;
    font-size: 10px;
    line-height: 1.5;
    color: var(--text-secondary);
    max-height: 200px;
    overflow-y: auto;
  }

  /* ── Plan content ────────────────────────── */

  .plan-section {
    padding: 8px 12px;
    border-top: 1px solid var(--surface-border);
  }

  .plan-toggle {
    display: flex;
    align-items: center;
    gap: 6px;
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    margin-bottom: 4px;
    font-family: inherit;
  }

  .plan-toggle:hover .section-label {
    color: var(--accent-300);
  }

  .section-label {
    display: block;
    font-size: 9px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.15em;
    color: var(--accent-400);
    margin-bottom: 0;
    cursor: pointer;
  }

  .toggle-icon {
    font-size: 8px;
    color: var(--text-muted);
  }

  .plan-content {
    max-height: 400px;
    overflow-y: auto;
    font-size: 12px;
    line-height: 1.6;
    color: var(--text-primary);
    padding-top: 4px;
  }

  /* Markdown body styles */
  .plan-content :global(h1),
  .plan-content :global(h2),
  .plan-content :global(h3),
  .plan-content :global(h4) {
    color: var(--accent-400);
    margin: 12px 0 6px 0;
    font-weight: 700;
    letter-spacing: 0.02em;
  }

  .plan-content :global(h1) {
    font-size: 14px;
  }
  .plan-content :global(h2) {
    font-size: 13px;
  }
  .plan-content :global(h3) {
    font-size: 12px;
  }
  .plan-content :global(h4) {
    font-size: 11px;
  }

  .plan-content :global(p) {
    margin: 6px 0;
  }

  .plan-content :global(ul),
  .plan-content :global(ol) {
    margin: 6px 0;
    padding-left: 20px;
  }

  .plan-content :global(li) {
    margin: 2px 0;
  }

  .plan-content :global(code) {
    background: var(--surface-700);
    padding: 1px 4px;
    border-radius: 2px;
    font-size: 11px;
  }

  .plan-content :global(pre) {
    background: var(--surface-700);
    padding: 8px 10px;
    border-radius: 3px;
    overflow-x: auto;
    margin: 6px 0;
    border: 1px solid var(--surface-border);
  }

  .plan-content :global(pre code) {
    background: none;
    padding: 0;
  }

  .plan-content :global(blockquote) {
    border-left: 2px solid var(--accent-600);
    padding-left: 10px;
    margin: 6px 0;
    color: var(--text-secondary);
  }

  .plan-content :global(hr) {
    border: none;
    border-top: 1px solid var(--surface-border);
    margin: 10px 0;
  }

  .plan-content-raw {
    max-height: 400px;
    overflow-y: auto;
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
    font-family: inherit;
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-primary);
    padding-top: 4px;
  }

  .plan-fallback {
    padding: 12px 14px;
    border-top: 1px solid var(--surface-border);
  }

  .fallback-text {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-muted);
    font-style: italic;
  }

  /* ── Permissions ─────────────────────────── */

  .permissions-section {
    padding: 8px 12px;
    border-top: 1px solid var(--surface-border);
    background: var(--surface-700);
  }

  .permissions-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-top: 6px;
  }

  .permission-row {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 4px 8px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
  }

  .permission-tool {
    display: inline-block;
    padding: 1px 6px;
    background: var(--tint-active);
    border: 1px solid var(--accent-600);
    border-radius: 3px;
    font-size: 9px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--accent-400);
    flex-shrink: 0;
  }

  .permission-prompt {
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-primary);
    white-space: pre-wrap;
  }

  /* ── Result section ──────────────────────── */

  .result-section {
    padding: 8px 14px;
    border-top: 1px solid var(--surface-border);
    background: var(--surface-700);
    position: relative;
  }

  .result-section.error-result {
    border-top-color: var(--status-red);
  }

  .result-label {
    display: block;
    font-size: 9px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.15em;
    color: var(--status-green-text, var(--status-green));
    margin-bottom: 3px;
  }

  .result-section.error-result .result-label {
    color: var(--status-red-text);
  }

  .result-value {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
    font-family: inherit;
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-primary);
  }

  .result-section.error-result .result-value {
    color: var(--status-red-muted);
  }

  /* Scanline overlay on result */
  .result-section::after {
    content: '';
    position: absolute;
    inset: 0;
    background: repeating-linear-gradient(
      0deg,
      transparent,
      transparent 2px,
      var(--scanline-color) 2px,
      var(--scanline-color) 4px
    );
    pointer-events: none;
    border-radius: 2px;
  }

  /* ── Pending banner ──────────────────────── */

  .pending-banner {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 14px;
    border: none;
    border-top: 1px solid var(--accent-600);
    border-radius: 0;
    background: var(--tint-active);
    width: 100%;
    cursor: pointer;
    font-family: inherit;
    transition: background 0.15s ease;
  }

  .pending-banner:hover {
    background: var(--accent-600);
  }

  .pending-icon {
    font-size: 12px;
    flex-shrink: 0;
  }

  .pending-text {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--accent-400);
  }

  /* ── Animations ──────────────────────────── */

  @keyframes card-on {
    0% {
      opacity: 0;
      filter: brightness(3);
    }
    30% {
      opacity: 0.5;
      filter: brightness(2);
    }
    60% {
      opacity: 0.8;
      filter: brightness(1.2);
    }
    100% {
      opacity: 1;
      filter: brightness(1);
    }
  }

  @keyframes led-pulse {
    0% {
      opacity: 0.6;
    }
    100% {
      opacity: 1;
      box-shadow:
        0 0 8px currentColor,
        0 0 16px currentColor;
    }
  }

  /* ── Mobile ──────────────────────────────── */

  @media (max-width: 639px) {
    .card-header {
      padding: 6px 10px;
    }

    .plan-section {
      padding: 6px 10px;
    }

    .plan-content {
      max-height: 300px;
      font-size: 11px;
    }

    .permissions-section {
      padding: 6px 10px;
    }

    .pending-text {
      font-size: 9px;
    }

    .toggle-raw {
      opacity: 0.6;
      padding: 4px 8px;
      font-size: 14px;
    }
  }

  /* ── Analog theme ────────────────────────── */

  :global([data-theme='analog']) .plan-card {
    background-color: var(--surface-800);
    background-image: var(--grain-fine);
    background-blend-mode: multiply;
    border-color: var(--surface-border);
  }

  :global([data-theme='analog']) .plan-card {
    animation: ink-bleed 0.5s cubic-bezier(0.1, 0.9, 0.2, 1);
  }

  :global([data-theme='analog']) .result-section::after {
    display: none;
  }

  @keyframes ink-bleed {
    0% {
      opacity: 0;
      transform: scaleY(0.95);
    }
    100% {
      opacity: 1;
      transform: scaleY(1);
    }
  }
</style>
