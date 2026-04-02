<script lang="ts">
  import type { SplitNode } from '$lib/stores/layout';
  import { setSplitRatio, isResizing } from '$lib/stores/layout';

  interface Props {
    splitNode: SplitNode;
  }

  let { splitNode }: Props = $props();

  let dragging = $state(false);
  let containerEl: HTMLDivElement | undefined = $state();

  function handlePointerDown(e: PointerEvent) {
    e.preventDefault();
    dragging = true;
    isResizing.set(true);
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }

  function handlePointerMove(e: PointerEvent) {
    if (!dragging || !containerEl) return;

    const parent = containerEl.parentElement;
    if (!parent) return;

    const rect = parent.getBoundingClientRect();
    let ratio: number;

    if (splitNode.direction === 'vertical') {
      ratio = (e.clientX - rect.left) / rect.width;
    } else {
      ratio = (e.clientY - rect.top) / rect.height;
    }

    // Use requestAnimationFrame for smooth updates
    requestAnimationFrame(() => {
      setSplitRatio(splitNode.id, ratio);
    });
  }

  function handlePointerUp() {
    dragging = false;
    isResizing.set(false);
  }

  function handleKeydown(e: KeyboardEvent) {
    const STEP = 0.05;
    const COARSE = 0.15;
    const vert = splitNode.direction === 'vertical';
    let delta = 0;

    if (e.key === 'Home') {
      e.preventDefault();
      setSplitRatio(splitNode.id, 0.5);
      return;
    }

    if (vert) {
      if (e.key === 'ArrowLeft') delta = -(e.shiftKey ? COARSE : STEP);
      else if (e.key === 'ArrowRight') delta = e.shiftKey ? COARSE : STEP;
    } else {
      if (e.key === 'ArrowUp') delta = -(e.shiftKey ? COARSE : STEP);
      else if (e.key === 'ArrowDown') delta = e.shiftKey ? COARSE : STEP;
    }

    if (delta !== 0) {
      e.preventDefault();
      setSplitRatio(splitNode.id, splitNode.ratio + delta);
    }
  }

  const isVertical = $derived(splitNode.direction === 'vertical');
  const ratioPercent = $derived(Math.round(splitNode.ratio * 100));
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  bind:this={containerEl}
  class="split-handle"
  class:vertical={isVertical}
  class:horizontal={!isVertical}
  class:dragging
  role="separator"
  tabindex="0"
  aria-orientation={isVertical ? 'vertical' : 'horizontal'}
  aria-valuenow={ratioPercent}
  aria-valuemin={15}
  aria-valuemax={85}
  aria-label="Resize panes"
  onpointerdown={handlePointerDown}
  onpointermove={handlePointerMove}
  onpointerup={handlePointerUp}
  onpointercancel={handlePointerUp}
  onkeydown={handleKeydown}
>
  <div class="handle-bar"></div>
</div>

<style>
  .split-handle {
    position: relative;
    flex-shrink: 0;
    z-index: 5;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .split-handle.vertical {
    width: 5px;
    cursor: col-resize;
  }

  .split-handle.horizontal {
    height: 5px;
    cursor: row-resize;
  }

  .handle-bar {
    background: var(--surface-border);
    border-radius: 1px;
    transition: background 0.15s ease;
  }

  .split-handle.vertical .handle-bar {
    width: 1px;
    height: 100%;
  }

  .split-handle.horizontal .handle-bar {
    height: 1px;
    width: 100%;
  }

  .split-handle:hover .handle-bar,
  .split-handle.dragging .handle-bar {
    background: var(--chrome-accent-500);
  }

  .split-handle:focus-visible .handle-bar {
    background: var(--chrome-accent-500);
  }

  /* Wider hover target — always ±4px, ±8px during drag */
  .split-handle.vertical::before {
    content: '';
    position: absolute;
    top: 0;
    bottom: 0;
    left: -4px;
    right: -4px;
  }

  .split-handle.horizontal::before {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    top: -4px;
    bottom: -4px;
  }

  .split-handle.vertical.dragging::before {
    left: -8px;
    right: -8px;
  }

  .split-handle.horizontal.dragging::before {
    top: -8px;
    bottom: -8px;
  }
</style>
