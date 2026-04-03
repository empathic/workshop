<script lang="ts" generics="T extends { id: string }">
  /**
   * VirtualList - Windowed/virtualized list rendering
   *
   * Only renders items visible in the viewport plus a buffer.
   * Handles variable-height items by measuring after render.
   *
   * Usage:
   *   <VirtualList items={cells}>
   *     {#snippet children({ item, index })}
   *       <NotebookCell cell={item} />
   *     {/snippet}
   *   </VirtualList>
   */

  import { tick } from 'svelte';
  import {
    calculateTotalHeight,
    calculateVisibleRange,
    calculateOffsetY,
    calculateScrollToIndex,
    isScrolledAwayFromBottom,
    isNearBottom,
    updateHeightCache
  } from '$lib/utils/virtualList';

  import type { Snippet } from 'svelte';

  interface Props {
    items: T[];
    /** Estimated height per item (for initial layout) */
    estimatedHeight?: number;
    /** Number of items to render above/below viewport */
    buffer?: number;
    /** Whether to auto-scroll to bottom on new items */
    autoScroll?: boolean;
    /** Callback when scroll position changes (for minimap sync) */
    onScroll?: (
      scrollTop: number,
      scrollHeight: number,
      clientHeight: number,
      visibleStart: number,
      visibleEnd: number
    ) => void;
    /** Render snippet for each item */
    children: Snippet<[{ item: T; index: number }]>;
    /** Optional footer content that scrolls with the list */
    footer?: Snippet;
  }

  let { items, estimatedHeight = 120, buffer = 3, autoScroll = true, onScroll, children, footer }: Props = $props();

  // Refs and state
  let container: HTMLElement;
  let scrollTop = $state(0);
  let containerHeight = $state(0);
  let footerHeight = $state(0);

  // Track actual measured heights per item index
  let heights = $state<Map<number, number>>(new Map());

  // Track whether user has manually scrolled up (disable auto-scroll)
  let userScrolledUp = $state(false);
  let prevItemCount = $state(0);

  // Use extracted pure functions for calculations
  let itemsHeight = $derived(calculateTotalHeight(items.length, heights, estimatedHeight));
  let totalHeight = $derived(itemsHeight + footerHeight);

  let visibleRange = $derived(
    calculateVisibleRange(items.length, heights, estimatedHeight, { scrollTop, containerHeight }, buffer)
  );

  let offsetY = $derived(calculateOffsetY(visibleRange.start, heights, estimatedHeight));

  // Items to actually render
  let visibleItems = $derived(items.slice(visibleRange.start, visibleRange.end));

  // Handle scroll
  function handleScroll(e: Event) {
    const target = e.target as HTMLElement;
    scrollTop = target.scrollTop;

    // Detect if user scrolled up (away from bottom)
    if (isScrolledAwayFromBottom(target.scrollTop, target.scrollHeight, target.clientHeight)) {
      userScrolledUp = true;
    }
  }

  // Notify parent when scroll state or visible range changes
  $effect(() => {
    if (!container) return;
    // Access derived values to track them
    const start = visibleRange.start;
    const end = visibleRange.end;
    onScroll?.(container.scrollTop, container.scrollHeight, container.clientHeight, start, end);
  });

  // Measure item height after render - Svelte action taking (node, index)
  function measureHeight(node: HTMLElement, index: number) {
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      const height = entry.contentRect.height;
      const updated = updateHeightCache(heights, index, height);
      if (updated !== heights) {
        heights = updated;
      }
    });
    observer.observe(node);
    return {
      destroy: () => observer.disconnect()
    };
  }

  // Auto-scroll to bottom when new items arrive
  $effect(() => {
    const currentCount = items.length;
    if (currentCount > prevItemCount && autoScroll && !userScrolledUp && container) {
      tick().then(() => {
        container.scrollTop = container.scrollHeight;
      });
    }
    prevItemCount = currentCount;
  });

  // Reset userScrolledUp when they scroll back to bottom
  $effect(() => {
    if (container && scrollTop > 0) {
      if (isNearBottom(scrollTop, totalHeight, containerHeight)) {
        userScrolledUp = false;
      }
    }
  });

  // Expose scroll method for external use
  export function scrollToBottom() {
    if (container) {
      userScrolledUp = false;
      container.scrollTop = container.scrollHeight;
    }
  }

  export function scrollToIndex(index: number) {
    if (!container) return;
    container.scrollTop = calculateScrollToIndex(index, heights, estimatedHeight);
  }
</script>

<div
  bind:this={container}
  bind:clientHeight={containerHeight}
  onscroll={handleScroll}
  class="virtual-container"
  role="list"
  aria-label="Conversation messages"
>
  <div class="virtual-spacer" style="height: {totalHeight}px;">
    <div class="virtual-content" style="transform: translateY({offsetY}px);">
      {#each visibleItems as item, i (item.id)}
        <div
          use:measureHeight={visibleRange.start + i}
          role="listitem"
          aria-setsize={items.length}
          aria-posinset={visibleRange.start + i + 1}
        >
          {@render children({ item, index: visibleRange.start + i })}
        </div>
      {/each}
    </div>
    {#if footer}
      <div class="virtual-footer" style="top: {itemsHeight}px;" bind:clientHeight={footerHeight}>
        {@render footer()}
      </div>
    {/if}
  </div>
</div>

<style>
  .virtual-container {
    height: 100%;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .virtual-spacer {
    position: relative;
  }

  .virtual-content {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
  }

  .virtual-footer {
    position: absolute;
    left: 0;
    right: 0;
  }

  /* Inherit scrollbar styles from parent */
  .virtual-container::-webkit-scrollbar {
    width: 8px;
  }

  .virtual-container::-webkit-scrollbar-track {
    background: transparent;
  }

  .virtual-container::-webkit-scrollbar-thumb {
    background: var(--surface-border);
    border-radius: 4px;
  }

  .virtual-container::-webkit-scrollbar-thumb:hover {
    background: var(--chrome-accent-600);
  }
</style>
