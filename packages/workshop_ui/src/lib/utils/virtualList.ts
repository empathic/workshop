/**
 * VirtualList Logic - Pure functions for virtualized list rendering
 *
 * Extracted from VirtualList.svelte for testability.
 * These functions handle the core calculations without DOM dependencies.
 */

// =============================================================================
// Types
// =============================================================================

export interface VisibleRange {
  start: number;
  end: number;
}

export interface ScrollState {
  scrollTop: number;
  containerHeight: number;
}

// =============================================================================
// Core Calculations
// =============================================================================

/**
 * Calculate total height of all items.
 * Uses measured heights where available, falls back to estimate.
 */
export function calculateTotalHeight(itemCount: number, heights: Map<number, number>, estimatedHeight: number): number {
  let total = 0;
  for (let i = 0; i < itemCount; i++) {
    total += heights.get(i) ?? estimatedHeight;
  }
  return total;
}

/**
 * Calculate which items should be visible based on scroll position.
 *
 * Buffer of 3 items above/below viewport because:
 * 1. Prevents flicker during 60fps scrolling
 * 2. Gives ResizeObserver time to measure before item enters view
 * 3. Empirically tested: 2 causes flicker, 4+ wastes memory
 */
export function calculateVisibleRange(
  itemCount: number,
  heights: Map<number, number>,
  estimatedHeight: number,
  scrollState: ScrollState,
  buffer: number = 3
): VisibleRange {
  const { scrollTop, containerHeight } = scrollState;

  // Edge case: no items
  if (itemCount === 0) {
    return { start: 0, end: 0 };
  }

  // Edge case: no container height yet (initial render)
  if (containerHeight === 0) {
    return { start: 0, end: Math.min(10, itemCount) };
  }

  // Find start index: first item whose bottom edge is below scrollTop
  let accumulatedHeight = 0;
  let startIndex = 0;

  for (let i = 0; i < itemCount; i++) {
    const h = heights.get(i) ?? estimatedHeight;
    if (accumulatedHeight + h >= scrollTop) {
      startIndex = Math.max(0, i - buffer);
      break;
    }
    accumulatedHeight += h;
    // If we reach the end without finding, start from last items
    if (i === itemCount - 1) {
      startIndex = Math.max(0, itemCount - buffer);
    }
  }

  // Calculate accumulated height up to startIndex
  let heightToStart = 0;
  for (let i = 0; i < startIndex; i++) {
    heightToStart += heights.get(i) ?? estimatedHeight;
  }

  // Find end index: first item whose top edge is below viewport + buffer
  const viewportBottom = scrollTop + containerHeight;
  const bufferHeight = buffer * estimatedHeight;
  let endIndex = itemCount;
  accumulatedHeight = heightToStart;

  for (let i = startIndex; i < itemCount; i++) {
    accumulatedHeight += heights.get(i) ?? estimatedHeight;
    if (accumulatedHeight >= viewportBottom + bufferHeight) {
      endIndex = i + 1;
      break;
    }
  }

  return {
    start: startIndex,
    end: Math.min(endIndex, itemCount)
  };
}

/**
 * Calculate Y offset for positioning visible items.
 */
export function calculateOffsetY(startIndex: number, heights: Map<number, number>, estimatedHeight: number): number {
  let offset = 0;
  for (let i = 0; i < startIndex; i++) {
    offset += heights.get(i) ?? estimatedHeight;
  }
  return offset;
}

/**
 * Calculate scroll position to bring an item into view.
 */
export function calculateScrollToIndex(index: number, heights: Map<number, number>, estimatedHeight: number): number {
  let offset = 0;
  for (let i = 0; i < index; i++) {
    offset += heights.get(i) ?? estimatedHeight;
  }
  return offset;
}

/**
 * Determine if user has scrolled away from bottom.
 * Used to disable auto-scroll when user is reading earlier messages.
 */
export function isScrolledAwayFromBottom(
  scrollTop: number,
  scrollHeight: number,
  clientHeight: number,
  threshold: number = 100
): boolean {
  const distanceFromBottom = scrollHeight - scrollTop - clientHeight;
  return distanceFromBottom > threshold;
}

/**
 * Determine if scroll position is near bottom.
 * Used to re-enable auto-scroll when user scrolls back down.
 */
export function isNearBottom(
  scrollTop: number,
  totalHeight: number,
  clientHeight: number,
  threshold: number = 50
): boolean {
  const distanceFromBottom = totalHeight - scrollTop - clientHeight;
  return distanceFromBottom < threshold;
}

// =============================================================================
// Threshold Logic
// =============================================================================

/**
 * Threshold of 20 items before virtualization because:
 * 1. Below 20, virtualization overhead exceeds benefit
 * 2. Height estimation is less accurate with few samples
 * 3. User perception: <20 items feels "instant" without virtualization
 */
export const VIRTUALIZATION_THRESHOLD = 20;

/**
 * Determine if virtualization should be used.
 */
export function shouldVirtualize(itemCount: number): boolean {
  return itemCount >= VIRTUALIZATION_THRESHOLD;
}

// =============================================================================
// Height Cache Management
// =============================================================================

/**
 * Update height cache with a new measurement.
 * Returns new Map if height changed, same Map if unchanged.
 */
export function updateHeightCache(
  heights: Map<number, number>,
  index: number,
  measuredHeight: number
): Map<number, number> {
  if (measuredHeight <= 0) {
    return heights; // Ignore invalid measurements
  }

  const currentHeight = heights.get(index);
  if (currentHeight === measuredHeight) {
    return heights; // No change
  }

  const newHeights = new Map(heights);
  newHeights.set(index, measuredHeight);
  return newHeights;
}

/**
 * Shift height indices when items are added/removed.
 * Used when items are inserted/removed from the list.
 */
export function shiftHeights(heights: Map<number, number>, fromIndex: number, shift: number): Map<number, number> {
  if (shift === 0) return heights;

  const newHeights = new Map<number, number>();

  for (const [index, height] of heights) {
    if (index < fromIndex) {
      // Items before insertion point stay the same
      newHeights.set(index, height);
    } else {
      // Items at or after shift by the delta
      newHeights.set(index + shift, height);
    }
  }

  return newHeights;
}
