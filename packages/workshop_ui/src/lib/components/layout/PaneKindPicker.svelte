<script lang="ts">
  import type { PaneContent, PaneContentKind } from '$lib/stores/layout';
  import { setPaneContent, defaultContentForKind, closePane, paneCount } from '$lib/stores/layout';
  import { SELECTABLE_KINDS } from '$lib/utils/pane-content';
  import { createInstance } from '$lib/stores/instances';
  import { userSettings } from '$lib/stores/settings';

  const canClose = $derived($paneCount > 1);

  interface Props {
    paneId: string;
    content: PaneContent & { kind: 'picker' };
  }

  let { paneId, content }: Props = $props();

  const options = SELECTABLE_KINDS;

  async function handleSelect(kind: PaneContentKind) {
    const workingDir = content.sourceWorkingDir ?? null;
    if (kind === 'terminal') {
      const result = await createInstance({
        command: $userSettings.shellCommand || 'bash',
        working_dir: workingDir ?? undefined
      });
      if (result) {
        setPaneContent(paneId, { kind: 'terminal', instanceId: result.id });
      }
      return;
    }
    setPaneContent(paneId, defaultContentForKind(kind, workingDir));
  }

  // -- Keyboard navigation --
  let focusedIndex = $state(0);
  let gridEl: HTMLElement | undefined = $state();

  function handleKeydown(e: KeyboardEvent) {
    const cols = columns;
    const count = options.length;
    let next = focusedIndex;

    if (e.key === 'ArrowRight') {
      e.preventDefault();
      next = Math.min(focusedIndex + 1, count - 1);
    } else if (e.key === 'ArrowLeft') {
      e.preventDefault();
      next = Math.max(focusedIndex - 1, 0);
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      next = Math.min(focusedIndex + cols, count - 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      next = Math.max(focusedIndex - cols, 0);
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      handleSelect(options[focusedIndex].kind);
      return;
    } else if (e.key === 'Escape' && canClose) {
      e.preventDefault();
      closePane(paneId);
      return;
    }

    if (next !== focusedIndex) {
      focusedIndex = next;
      // Move DOM focus to the button
      const btns = gridEl?.querySelectorAll<HTMLButtonElement>('.kind-card');
      btns?.[focusedIndex]?.focus();
    }
  }

  // Auto-focus the grid on mount
  $effect(() => {
    if (gridEl) {
      const first = gridEl.querySelector<HTMLButtonElement>('.kind-card');
      first?.focus();
    }
  });

  // -- Responsive columns via ResizeObserver --
  let pickerEl: HTMLElement | undefined = $state();
  let pickerWidth = $state(400);

  $effect(() => {
    if (!pickerEl) return;
    const ro = new ResizeObserver(([entry]) => {
      pickerWidth = entry.contentRect.width;
    });
    ro.observe(pickerEl);
    return () => ro.disconnect();
  });

  // 2 columns above 260px, 1 column below
  const columns = $derived(pickerWidth > 260 ? 2 : 1);
  // Compact mode: hide descriptions, shrink padding
  const compact = $derived(pickerWidth < 200);
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="picker" bind:this={pickerEl} onkeydown={handleKeydown}>
  {#if canClose}
    <button class="picker-close" onclick={() => closePane(paneId)} title="Close pane" aria-label="Close pane">
      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5">
        <line x1="4" y1="4" x2="12" y2="12" />
        <line x1="12" y1="4" x2="4" y2="12" />
      </svg>
    </button>
  {/if}
  <div class="picker-inner" class:compact>
    <div
      class="kind-grid"
      class:single-col={columns === 1}
      bind:this={gridEl}
      role="grid"
      aria-label="Pane type selection"
    >
      {#each options as opt, i}
        <button
          class="kind-card"
          class:focused={focusedIndex === i}
          style="--i: {i}"
          tabindex={focusedIndex === i ? 0 : -1}
          onclick={() => handleSelect(opt.kind)}
          onfocus={() => (focusedIndex = i)}
          role="gridcell"
          aria-label={opt.label}
        >
          <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" class="kind-icon">
            {@html opt.pickerIcon}
          </svg>
          <span class="kind-label">{opt.label}</span>
          {#if !compact}
            <span class="kind-desc">{opt.desc}</span>
          {/if}
        </button>
      {/each}
    </div>
  </div>
</div>

<style>
  .picker {
    display: flex;
    align-items: safe center;
    justify-content: center;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    position: relative;
  }

  .picker-close {
    position: absolute;
    top: 4px;
    right: 6px;
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
    z-index: 1;
  }

  .picker-close svg {
    width: 12px;
    height: 12px;
  }

  .picker-close:hover {
    background: var(--status-red-tint);
    color: var(--status-red);
  }

  .picker-inner {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    max-width: 360px;
    width: 100%;
    padding: 24px;
  }

  .picker-inner.compact {
    padding: 12px;
    gap: 6px;
  }

  .kind-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
    width: 100%;
    outline: none;
  }

  .kind-grid.single-col {
    grid-template-columns: 1fr;
  }

  .kind-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    padding: 12px 8px 10px;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-secondary);
    font-family: inherit;
    cursor: pointer;
    outline: none;
    transition:
      border-color 0.1s ease,
      background 0.1s ease,
      box-shadow 0.1s ease;

    /* Staggered entrance */
    animation: card-in 0.12s ease-out both;
    animation-delay: calc(var(--i) * 35ms);
  }

  .compact .kind-card {
    flex-direction: row;
    justify-content: flex-start;
    gap: 8px;
    padding: 8px 10px;
  }

  .kind-card:hover,
  .kind-card.focused {
    border-color: var(--chrome-accent-600);
    background: var(--tint-hover);
    box-shadow: var(--elevation-low);
  }

  .kind-card:hover .kind-icon,
  .kind-card.focused .kind-icon {
    color: var(--chrome-accent-400);
  }

  .kind-card:hover .kind-label,
  .kind-card.focused .kind-label {
    color: var(--chrome-accent-400);
    text-shadow: var(--emphasis);
  }

  .kind-card:hover .kind-desc,
  .kind-card.focused .kind-desc {
    color: var(--text-secondary);
  }

  @keyframes card-in {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .kind-icon {
    width: 20px;
    height: 20px;
    flex-shrink: 0;
    color: var(--text-muted);
    transition:
      color 0.1s ease,
      filter 0.1s ease;
  }

  .kind-label {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-secondary);
    transition:
      color 0.1s ease,
      text-shadow 0.1s ease;
  }

  .kind-desc {
    font-size: 9px;
    letter-spacing: 0.03em;
    color: var(--text-muted);
    transition: color 0.1s ease;
  }

  @media (prefers-reduced-motion: reduce) {
    .kind-card {
      animation: none;
    }
  }
</style>
