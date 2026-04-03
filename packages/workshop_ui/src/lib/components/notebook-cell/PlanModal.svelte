<script lang="ts">
  import type { ToolCell } from '$lib/types';
  import { renderMarkdown } from '$lib/utils/markdown';
  import { getPlanContent, parseAllowedPrompts, parseStatusText } from '$lib/utils/plan';

  interface Props {
    tool: ToolCell;
    allVersions: ToolCell[];
    currentVersionIndex: number;
    statusText: string | null;
    isResolved: boolean;
    isPending: boolean;
    isError: boolean;
    onclose: () => void;
  }

  let {
    tool,
    allVersions,
    currentVersionIndex,
    statusText,
    isResolved,
    isPending,
    isError,
    onclose
  }: Props = $props();

  let selectedVersion: number | null = $state(null);
  let backdropEl: HTMLDivElement | undefined = $state();

  const totalVersions = $derived(allVersions.length);
  const hasMultipleVersions = $derived(totalVersions > 1);
  const activeVersion = $derived(selectedVersion ?? currentVersionIndex);

  const activeToolCell: ToolCell = $derived.by(() => {
    return activeVersion >= 0 && activeVersion < allVersions.length ? allVersions[activeVersion] : tool;
  });

  const activePlanContent = $derived(getPlanContent(activeToolCell));
  const activeRenderedPlan = $derived(activePlanContent ? renderMarkdown(activePlanContent) : null);
  const activeAllowedPrompts = $derived(parseAllowedPrompts(activeToolCell.input.allowedPrompts));
  const activeStatusText = $derived(parseStatusText(activeToolCell.output));

  const FOCUSABLE = 'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])';

  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        node.remove();
      }
    };
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onclose();
      return;
    }

    if (e.key === 'Tab' && backdropEl) {
      const focusable = [...backdropEl.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
        (el) => !el.hasAttribute('disabled')
      );
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="backdrop" use:portal bind:this={backdropEl} onclick={onclose}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="modal" onclick={(e) => e.stopPropagation()}>
    <!-- Header bar -->
    <div class="modal-header">
      <div class="header-left">
        <span class="header-label">PLAN REVIEW</span>
        {#if hasMultipleVersions}
          <span class="version-indicator">v{activeVersion + 1} of {totalVersions}</span>
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
        {#if activeStatusText}
          <span
            class="status-badge"
            class:status-approved={activeStatusText === 'APPROVED'}
            class:status-rejected={activeStatusText === 'REJECTED'}
            class:status-changes={activeStatusText === 'CHANGES REQUESTED'}
          >
            {activeStatusText}
          </span>
        {:else if isPending}
          <span class="status-badge status-pending">AWAITING REVIEW</span>
        {/if}
      </div>
      <button class="close-btn" onclick={onclose} title="Close (Esc)">&#x2715;</button>
    </div>

    <!-- Scrollable body -->
    <div class="modal-body">
      {#if activeRenderedPlan}
        <div class="plan-content markdown-body">
          {@html activeRenderedPlan}
        </div>
      {:else if activePlanContent}
        <pre class="plan-content-raw">{activePlanContent}</pre>
      {:else}
        <div class="plan-fallback">
          <span class="fallback-text">Plan written to file</span>
        </div>
      {/if}
    </div>

    <!-- Footer: permissions + result -->
    {#if activeAllowedPrompts.length > 0 || (activeToolCell.output && activeToolCell.output.length > 0)}
      <div class="modal-footer">
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

        {#if activeToolCell.output}
          <div class="result-section" class:error-result={activeToolCell.is_error}>
            <span class="result-label">{activeStatusText ?? (activeToolCell.is_error ? 'ERROR' : 'RESULT')}</span>
            {#if activeStatusText === null}
              <pre class="result-value">{activeToolCell.output}</pre>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  /* ── Backdrop ─────────────────────────────── */

  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 200;
    background: rgba(0, 0, 0, 0.8);
    backdrop-filter: blur(6px);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    animation: fade-in 0.2s ease;
  }

  @keyframes fade-in {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }

  /* ── Modal ────────────────────────────────── */

  .modal {
    width: 100%;
    max-width: 900px;
    max-height: 100%;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--accent-600);
    background: var(--surface-800);
    box-shadow: var(--elevation-high);
    animation: modal-in 0.25s ease;
    overflow: hidden;
  }

  @keyframes modal-in {
    from {
      opacity: 0;
      transform: scale(0.96);
    }
    to {
      opacity: 1;
      transform: scale(1);
    }
  }

  /* ── Header ───────────────────────────────── */

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid var(--surface-border);
    background: var(--surface-700);
    flex-shrink: 0;
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .header-label {
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.15em;
    color: var(--accent-400);
    text-shadow: var(--emphasis);
  }

  .version-indicator {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.08em;
    color: var(--text-muted);
    text-transform: uppercase;
  }

  .version-tabs {
    display: flex;
    gap: 3px;
  }

  .version-tab {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    font-family: inherit;
    font-size: 10px;
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
    width: 7px;
    height: 7px;
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

  .status-badge {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    padding: 2px 8px;
    border-radius: 3px;
  }

  .status-badge.status-approved {
    color: var(--status-green);
    background: var(--status-green-bg, rgba(34, 197, 94, 0.08));
    border: 1px solid var(--status-green-border, rgba(34, 197, 94, 0.25));
  }

  .status-badge.status-rejected {
    color: var(--status-red);
    background: var(--status-red-bg, rgba(239, 68, 68, 0.08));
    border: 1px solid var(--status-red-border, rgba(239, 68, 68, 0.25));
  }

  .status-badge.status-changes {
    color: var(--accent-400);
    background: var(--tint-active);
    border: 1px solid var(--accent-600);
  }

  .status-badge.status-pending {
    color: var(--accent-500);
    background: var(--tint-active);
    border: 1px solid var(--accent-600);
    animation: badge-pulse 2s ease-in-out infinite;
  }

  .close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    background: none;
    border: 1px solid transparent;
    border-radius: 3px;
    color: var(--text-muted);
    font-size: 14px;
    cursor: pointer;
    transition: all 0.15s ease;
    flex-shrink: 0;
  }

  .close-btn:hover {
    background: var(--surface-600);
    border-color: var(--surface-border-light);
    color: var(--text-primary);
  }

  /* ── Body ─────────────────────────────────── */

  .modal-body {
    flex: 1;
    overflow-y: auto;
    overscroll-behavior: contain;
    padding: 20px 24px;
    min-height: 0;
  }

  .plan-content {
    font-size: 13px;
    line-height: 1.7;
    color: var(--text-primary);
  }

  .plan-content :global(h1),
  .plan-content :global(h2),
  .plan-content :global(h3),
  .plan-content :global(h4) {
    color: var(--accent-400);
    margin: 20px 0 10px 0;
    font-weight: 700;
    letter-spacing: 0.02em;
  }


  .plan-content :global(h1) {
    font-size: 18px;
    border-bottom: 1px solid var(--surface-border);
    padding-bottom: 8px;
  }
  .plan-content :global(h2) {
    font-size: 16px;
  }
  .plan-content :global(h3) {
    font-size: 14px;
  }
  .plan-content :global(h4) {
    font-size: 13px;
  }

  .plan-content :global(p) {
    margin: 10px 0;
  }

  .plan-content :global(ul),
  .plan-content :global(ol) {
    margin: 10px 0;
    padding-left: 24px;
  }

  .plan-content :global(li) {
    margin: 4px 0;
  }

  .plan-content :global(code) {
    background: var(--surface-700);
    padding: 2px 6px;
    border-radius: 3px;
    font-size: 12px;
  }

  .plan-content :global(pre) {
    background: var(--surface-700);
    padding: 12px 16px;
    border-radius: 4px;
    overflow-x: auto;
    margin: 12px 0;
    border: 1px solid var(--surface-border);
  }

  .plan-content :global(pre code) {
    background: none;
    padding: 0;
  }

  .plan-content :global(blockquote) {
    border-left: 3px solid var(--accent-600);
    padding-left: 14px;
    margin: 12px 0;
    color: var(--text-secondary);
  }

  .plan-content :global(hr) {
    border: none;
    border-top: 1px solid var(--surface-border);
    margin: 16px 0;
  }

  .plan-content :global(table) {
    width: 100%;
    border-collapse: collapse;
    margin: 12px 0;
  }

  .plan-content :global(th),
  .plan-content :global(td) {
    padding: 6px 10px;
    border: 1px solid var(--surface-border);
    text-align: left;
    font-size: 12px;
  }

  .plan-content :global(th) {
    background: var(--surface-700);
    color: var(--accent-400);
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    font-size: 10px;
  }

  .plan-content-raw {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
    font-family: inherit;
    font-size: 13px;
    line-height: 1.7;
    color: var(--text-primary);
  }

  .plan-fallback {
    padding: 24px;
    text-align: center;
  }

  .fallback-text {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-muted);
    font-style: italic;
  }

  /* ── Footer ───────────────────────────────── */

  .modal-footer {
    flex-shrink: 0;
    border-top: 1px solid var(--surface-border);
    max-height: 200px;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .permissions-section {
    padding: 12px 16px;
    background: var(--surface-700);
  }

  .section-label {
    display: block;
    font-size: 9px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.15em;
    color: var(--accent-400);
    margin-bottom: 8px;
  }

  .permissions-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .permission-row {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 5px 10px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
  }

  .permission-tool {
    display: inline-block;
    padding: 2px 7px;
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
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-primary);
    white-space: pre-wrap;
  }

  .result-section {
    padding: 10px 16px;
    background: var(--surface-700);
    border-top: 1px solid var(--surface-border);
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
    margin-bottom: 4px;
  }

  .result-section.error-result .result-label {
    color: var(--status-red-text);
  }

  .result-value {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
    font-family: inherit;
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-primary);
  }

  .result-section.error-result .result-value {
    color: var(--status-red-muted);
  }

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
  }

  /* ── Animations ───────────────────────────── */

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

  @keyframes badge-pulse {
    0%,
    100% {
      opacity: 0.8;
    }
    50% {
      opacity: 1;
    }
  }

  /* ── Mobile ───────────────────────────────── */

  @media (max-width: 639px) {
    .backdrop {
      padding: 12px;
    }

    .modal-header {
      padding: 10px 12px;
    }

    .modal-body {
      padding: 16px;
    }

    .plan-content {
      font-size: 12px;
      line-height: 1.6;
    }

    .plan-content :global(h1) {
      font-size: 15px;
    }
    .plan-content :global(h2) {
      font-size: 14px;
    }

    .permissions-section {
      padding: 10px 12px;
    }
  }

  /* ── Analog theme ─────────────────────────── */

  :global([data-theme='analog']) .modal {
    background-color: var(--surface-800);
    background-image: var(--grain-fine);
    background-blend-mode: multiply;
    border-color: var(--surface-border);
  }

  :global([data-theme='analog']) .modal {
    animation: ink-bleed 0.4s cubic-bezier(0.1, 0.9, 0.2, 1);
  }

  :global([data-theme='analog']) .result-section::after {
    display: none;
  }

  @keyframes ink-bleed {
    0% {
      opacity: 0;
      transform: scaleY(0.97);
    }
    100% {
      opacity: 1;
      transform: scaleY(1);
    }
  }
</style>
