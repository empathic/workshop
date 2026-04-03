<script lang="ts">
  import type { NotebookCell } from '$lib/types';

  interface Props {
    cells: NotebookCell[];
    /** Current scroll position (from VirtualList) */
    scrollTop?: number;
    /** Total scrollable height */
    scrollHeight?: number;
    /** Visible client height */
    clientHeight?: number;
    /** First visible item index (from VirtualList) */
    visibleStart?: number;
    /** Last visible item index (from VirtualList) */
    visibleEnd?: number;
    /** Callback to scroll to a cell index */
    onScrollToIndex?: (index: number) => void;
  }

  let {
    cells,
    scrollTop = 0,
    scrollHeight = 1,
    clientHeight = 1,
    visibleStart = 0,
    visibleEnd = 0,
    onScrollToIndex
  }: Props = $props();

  // Container reference for measuring available height
  let containerEl: HTMLDivElement;
  let availableHeight = $state(300);

  // Constants
  const MINIMAP_WIDTH = 40;
  const MIN_HEIGHT = 150;
  const MIN_SEGMENT_HEIGHT = 8; // Minimum height for a readable segment
  const SEGMENT_GAP = 2;
  const INDICATOR_HEIGHT = 20; // Height reserved for overflow indicators

  // Update available height on mount and resize
  $effect(() => {
    if (!containerEl) return;

    const updateHeight = () => {
      const parent = containerEl.parentElement;
      if (parent) {
        availableHeight = Math.max(MIN_HEIGHT, parent.clientHeight - 32);
      }
    };

    updateHeight();
    const observer = new ResizeObserver(updateHeight);
    observer.observe(containerEl.parentElement!);
    return () => observer.disconnect();
  });

  // Filter to displayable cells and track original indices
  // Include user, assistant, system, and unknown types (everything except tool)
  const filteredCells = $derived(
    cells.map((cell, index) => ({ cell, index })).filter(({ cell }) => cell.type !== 'tool')
  );

  // Calculate how many segments we can show
  const maxVisibleSegments = $derived(
    Math.floor((availableHeight - INDICATOR_HEIGHT * 2) / (MIN_SEGMENT_HEIGHT + SEGMENT_GAP))
  );

  // Current scroll position as ratio (0-1)
  const scrollRatio = $derived(scrollHeight > clientHeight ? scrollTop / (scrollHeight - clientHeight) : 0);

  // Calculate which segments to show (window around current position)
  const windowInfo = $derived.by(() => {
    const total = filteredCells.length;
    if (total <= maxVisibleSegments) {
      // Show all - no windowing needed
      return { start: 0, end: total, hasAbove: false, hasBelow: false };
    }

    // Window size
    const windowSize = maxVisibleSegments;

    // Center the window on the current scroll position
    const centerIndex = Math.floor(scrollRatio * (total - 1));
    let start = Math.max(0, centerIndex - Math.floor(windowSize / 2));
    let end = start + windowSize;

    // Adjust if we hit the bounds
    if (end > total) {
      end = total;
      start = Math.max(0, end - windowSize);
    }

    return {
      start,
      end,
      hasAbove: start > 0,
      hasBelow: end < total,
      aboveCount: start,
      belowCount: total - end
    };
  });

  // Visible segments with positions
  const visibleSegments = $derived.by(() => {
    const { start, end, hasAbove, hasBelow } = windowInfo;
    const segmentCount = end - start;
    if (segmentCount === 0) return [];

    // Available height for segments (minus indicators if needed)
    const topOffset = hasAbove ? INDICATOR_HEIGHT : 4;
    const bottomOffset = hasBelow ? INDICATOR_HEIGHT : 4;
    const segmentAreaHeight = availableHeight - topOffset - bottomOffset;

    const segmentHeight = Math.max(MIN_SEGMENT_HEIGHT, segmentAreaHeight / segmentCount - SEGMENT_GAP);

    return filteredCells.slice(start, end).map(({ cell, index }, i) => ({
      id: cell.id,
      cellIndex: index,
      type: cell.type as 'user' | 'assistant' | 'system' | 'unknown',
      hasTools: (cell.toolCells?.length ?? 0) > 0,
      hasThinking: Boolean(cell.thinking),
      isAttributed: Boolean(cell.attributed_to),
      isUnknown: cell.type === 'unknown' || cell.type === 'system',
      y: topOffset + i * (segmentHeight + SEGMENT_GAP),
      height: segmentHeight
    }));
  });

  // Viewport indicator - simple scroll-ratio based positioning
  // Maps directly from scroll position to minimap position for intuitive 1:1 correspondence
  const viewportIndicator = $derived.by(() => {
    const { hasAbove, hasBelow } = windowInfo;

    // Calculate the segment area (where segments are drawn)
    const topOffset = hasAbove ? INDICATOR_HEIGHT : 4;
    const bottomOffset = hasBelow ? INDICATOR_HEIGHT : 4;
    const segmentAreaHeight = availableHeight - topOffset - bottomOffset;

    // If no scrollable content, show viewport covering everything
    if (scrollHeight <= clientHeight || scrollHeight === 0) {
      return { y: topOffset, height: segmentAreaHeight };
    }

    // Calculate viewport position as ratio of scroll position
    const scrollRatio = scrollTop / (scrollHeight - clientHeight);
    const viewportHeightRatio = clientHeight / scrollHeight;

    // Map to pixel positions within segment area
    const viewportHeight = Math.max(20, viewportHeightRatio * segmentAreaHeight);
    const maxY = segmentAreaHeight - viewportHeight;
    const y = topOffset + scrollRatio * maxY;

    return { y, height: viewportHeight };
  });

  // Handle click on segment
  function handleSegmentClick(cellIndex: number) {
    onScrollToIndex?.(cellIndex);
  }

  // Handle click on overflow indicator
  function handleOverflowClick(direction: 'up' | 'down') {
    const { start, end } = windowInfo;
    const jumpAmount = Math.floor(maxVisibleSegments / 2);

    if (direction === 'up' && start > 0) {
      const targetIndex = Math.max(0, start - jumpAmount);
      onScrollToIndex?.(filteredCells[targetIndex].index);
    } else if (direction === 'down' && end < filteredCells.length) {
      const targetIndex = Math.min(filteredCells.length - 1, end + jumpAmount - 1);
      onScrollToIndex?.(filteredCells[targetIndex].index);
    }
  }
</script>

<div class="minimap-container" bind:this={containerEl}>
  <svg class="minimap" viewBox="0 0 {MINIMAP_WIDTH} {availableHeight}" width={MINIMAP_WIDTH} height={availableHeight}>
    <!-- Track background -->
    <rect class="track" x="0" y="0" width={MINIMAP_WIDTH} height={availableHeight} rx="4" />

    <!-- Upper overflow indicator -->
    {#if windowInfo.hasAbove}
      <g
        class="overflow-indicator"
        onclick={() => handleOverflowClick('up')}
        role="button"
        tabindex="0"
        onkeydown={(e) => e.key === 'Enter' && handleOverflowClick('up')}
      >
        <rect x="0" y="0" width={MINIMAP_WIDTH} height={INDICATOR_HEIGHT} fill="transparent" />
        <text x={MINIMAP_WIDTH / 2} y="12" class="overflow-text">
          ↑{windowInfo.aboveCount}
        </text>
      </g>
    {/if}

    <!-- Message segments -->
    {#each visibleSegments as segment (segment.id)}
      <g class="segment-group">
        <rect
          class="segment-bar"
          class:user={segment.type === 'user'}
          class:assistant={segment.type === 'assistant'}
          class:system={segment.type === 'system'}
          class:unknown={segment.type === 'unknown'}
          class:has-thinking={segment.hasThinking}
          x="4"
          y={segment.y}
          width="32"
          height={segment.height}
          rx="2"
          role="button"
          tabindex="0"
          onclick={() => handleSegmentClick(segment.cellIndex)}
          onkeydown={(e) => e.key === 'Enter' && handleSegmentClick(segment.cellIndex)}
        />
        <!-- Thinking indicator (left edge) -->
        {#if segment.hasThinking}
          <rect class="thinking-marker" x="2" y={segment.y} width="2" height={segment.height} rx="1" />
        {/if}
        <!-- Tool indicator (right side) -->
        {#if segment.hasTools}
          <circle class="tool-dot" cx="37" cy={segment.y + segment.height / 2} r="2" />
        {/if}
      </g>
    {/each}

    <!-- Lower overflow indicator -->
    {#if windowInfo.hasBelow}
      <g
        class="overflow-indicator"
        onclick={() => handleOverflowClick('down')}
        role="button"
        tabindex="0"
        onkeydown={(e) => e.key === 'Enter' && handleOverflowClick('down')}
      >
        <rect
          x="0"
          y={availableHeight - INDICATOR_HEIGHT}
          width={MINIMAP_WIDTH}
          height={INDICATOR_HEIGHT}
          fill="transparent"
        />
        <text x={MINIMAP_WIDTH / 2} y={availableHeight - 6} class="overflow-text">
          ↓{windowInfo.belowCount}
        </text>
      </g>
    {/if}

    <!-- Viewport indicator -->
    <rect
      class="viewport"
      x="0"
      y={viewportIndicator.y}
      width={MINIMAP_WIDTH}
      height={viewportIndicator.height}
      rx="3"
    />
  </svg>
</div>

<style>
  .minimap-container {
    position: absolute;
    right: 8px;
    top: 50%;
    transform: translateY(-50%);
    z-index: 20;
  }

  .minimap {
    display: block;
    border-radius: 4px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    box-shadow:
      var(--elevation-high),
      var(--recess);
    overflow: hidden;
  }

  .track {
    fill: var(--surface-700);
  }

  .segment-bar {
    cursor: pointer;
    transition: filter 0.15s ease;
  }

  .segment-bar:hover {
    filter: brightness(1.4);
  }

  .segment-bar:focus {
    outline: none;
    filter: brightness(1.5);
  }

  .segment-bar.user {
    fill: var(--minimap-user);
    filter: drop-shadow(0 0 2px color-mix(in srgb, var(--minimap-user) 40%, transparent));
  }

  .segment-bar.user:hover {
    filter: drop-shadow(0 0 4px color-mix(in srgb, var(--minimap-user) 60%, transparent)) brightness(1.2);
  }

  .segment-bar.assistant {
    fill: var(--minimap-assistant);
    filter: drop-shadow(0 0 2px color-mix(in srgb, var(--minimap-assistant) 40%, transparent));
  }

  .segment-bar.assistant:hover {
    filter: drop-shadow(0 0 4px color-mix(in srgb, var(--minimap-assistant) 60%, transparent)) brightness(1.2);
  }

  .segment-bar.system {
    fill: var(--minimap-system);
    opacity: 0.7;
  }

  .segment-bar.system:hover {
    filter: brightness(1.2);
  }

  .segment-bar.unknown {
    fill: var(--minimap-system);
    stroke: color-mix(in srgb, var(--minimap-system) 60%, transparent);
    stroke-width: 1;
    stroke-dasharray: 2 2;
    opacity: 0.8;
  }

  .segment-bar.unknown:hover {
    filter: brightness(1.2);
  }

  .tool-dot {
    fill: var(--minimap-tool);
    filter: drop-shadow(0 0 2px color-mix(in srgb, var(--minimap-tool) 40%, transparent));
    pointer-events: none;
  }

  .thinking-marker {
    fill: var(--minimap-thinking);
    filter: drop-shadow(0 0 3px var(--tint-thinking-strong));
    pointer-events: none;
  }

  .segment-bar.has-thinking {
    filter: drop-shadow(0 0 2px var(--tint-thinking-strong));
  }

  .viewport {
    fill: var(--minimap-viewport-fill);
    stroke: var(--minimap-viewport-stroke);
    stroke-width: 1.5;
    pointer-events: none;
  }

  .overflow-indicator {
    cursor: pointer;
  }

  .overflow-indicator:hover .overflow-text {
    fill: var(--accent-400);
  }

  .overflow-text {
    font-size: 9px;
    font-weight: 600;
    fill: var(--text-muted);
    text-anchor: middle;
    pointer-events: none;
    font-family: var(--font-mono, monospace);
  }

  /* Subtle scanline effect */
  .minimap-container::after {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: 4px;
    background: repeating-linear-gradient(
      0deg,
      transparent,
      transparent 2px,
      var(--scanline-color) 2px,
      var(--scanline-color) 4px
    );
    pointer-events: none;
  }
</style>
