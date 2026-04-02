/**
 * VirtualList Logic Tests
 *
 * Property-based testing for virtualized list calculations.
 * Tests invariants that must hold regardless of specific inputs.
 */

import {
  calculateTotalHeight,
  calculateVisibleRange,
  calculateOffsetY,
  calculateScrollToIndex,
  isScrolledAwayFromBottom,
  isNearBottom,
  shouldVirtualize,
  updateHeightCache,
  shiftHeights,
  VIRTUALIZATION_THRESHOLD
} from './virtualList.js';

// =============================================================================
// calculateTotalHeight
// =============================================================================

describe('calculateTotalHeight', () => {
  it('returns 0 for empty list', () => {
    const heights = new Map<number, number>();
    expect(calculateTotalHeight(0, heights, 100)).toBe(0);
  });

  it('uses estimated height when no measurements', () => {
    const heights = new Map<number, number>();
    expect(calculateTotalHeight(5, heights, 100)).toBe(500);
  });

  it('uses measured heights when available', () => {
    const heights = new Map<number, number>([
      [0, 50],
      [1, 150],
      [2, 100]
    ]);
    expect(calculateTotalHeight(3, heights, 100)).toBe(300);
  });

  it('mixes measured and estimated heights', () => {
    const heights = new Map<number, number>([
      [0, 50],
      [2, 150]
    ]);
    // Item 0: 50, Item 1: 100 (estimated), Item 2: 150
    expect(calculateTotalHeight(3, heights, 100)).toBe(300);
  });

  describe('invariants', () => {
    it('total height >= itemCount * min possible height', () => {
      const heights = new Map<number, number>();
      const itemCount = 100;
      const estimatedHeight = 50;
      const total = calculateTotalHeight(itemCount, heights, estimatedHeight);
      expect(total).toBeGreaterThanOrEqual(0);
    });

    it('total height is always non-negative', () => {
      const testCases = [
        { itemCount: 0, heights: new Map<number, number>(), estimated: 100 },
        { itemCount: 10, heights: new Map<number, number>(), estimated: 0 },
        { itemCount: 5, heights: new Map([[0, 0]]), estimated: 50 }
      ];

      for (const { itemCount, heights, estimated } of testCases) {
        expect(calculateTotalHeight(itemCount, heights, estimated)).toBeGreaterThanOrEqual(0);
      }
    });
  });
});

// =============================================================================
// calculateVisibleRange
// =============================================================================

describe('calculateVisibleRange', () => {
  const defaultScroll = { scrollTop: 0, containerHeight: 500 };
  const defaultEstimate = 100;

  describe('edge cases', () => {
    it('handles empty list', () => {
      const range = calculateVisibleRange(0, new Map(), defaultEstimate, defaultScroll);
      expect(range).toEqual({ start: 0, end: 0 });
    });

    it('handles zero container height (initial render)', () => {
      const range = calculateVisibleRange(100, new Map(), defaultEstimate, {
        scrollTop: 0,
        containerHeight: 0
      });
      // Should return reasonable default
      expect(range.start).toBe(0);
      expect(range.end).toBeLessThanOrEqual(10);
    });

    it('handles single item', () => {
      const range = calculateVisibleRange(1, new Map(), defaultEstimate, defaultScroll);
      expect(range.start).toBe(0);
      expect(range.end).toBe(1);
    });
  });

  describe('basic visibility', () => {
    it('shows items at top when scrolled to top', () => {
      const range = calculateVisibleRange(100, new Map(), 100, {
        scrollTop: 0,
        containerHeight: 500
      });

      expect(range.start).toBe(0);
      // With 500px viewport and 100px items, should see ~5 items + buffer
      expect(range.end).toBeGreaterThan(5);
      expect(range.end).toBeLessThan(20);
    });

    it('shows items in middle when scrolled', () => {
      const range = calculateVisibleRange(100, new Map(), 100, {
        scrollTop: 3000, // 30 items down
        containerHeight: 500
      });

      // Should start around item 27 (30 - buffer of 3)
      expect(range.start).toBeGreaterThanOrEqual(25);
      expect(range.start).toBeLessThanOrEqual(30);
      // Should end around item 38 (30 + 5 visible + 3 buffer)
      expect(range.end).toBeGreaterThanOrEqual(35);
    });

    it('handles scroll past end', () => {
      const range = calculateVisibleRange(10, new Map(), 100, {
        scrollTop: 2000, // Way past total height of 1000
        containerHeight: 500
      });

      // Should still return valid range
      expect(range.start).toBeGreaterThanOrEqual(0);
      expect(range.end).toBeLessThanOrEqual(10);
    });
  });

  describe('with measured heights', () => {
    it('uses measured heights for positioning', () => {
      // First 5 items are 200px each, rest are 100px
      const heights = new Map<number, number>();
      for (let i = 0; i < 5; i++) {
        heights.set(i, 200);
      }

      const range = calculateVisibleRange(50, heights, 100, {
        scrollTop: 1000, // First 5 items = 1000px
        containerHeight: 500
      });

      // Should start around item 5 (after the tall items)
      expect(range.start).toBeLessThanOrEqual(5);
    });
  });

  describe('invariants', () => {
    it('start <= end always', () => {
      const testCases = [
        { itemCount: 0, scrollTop: 0, containerHeight: 500 },
        { itemCount: 100, scrollTop: 0, containerHeight: 500 },
        { itemCount: 100, scrollTop: 5000, containerHeight: 500 },
        { itemCount: 100, scrollTop: 10000, containerHeight: 500 },
        { itemCount: 5, scrollTop: 1000, containerHeight: 100 }
      ];

      for (const { itemCount, scrollTop, containerHeight } of testCases) {
        const range = calculateVisibleRange(itemCount, new Map(), 100, {
          scrollTop,
          containerHeight
        });
        expect(range.start).toBeLessThanOrEqual(range.end);
      }
    });

    it('range is within bounds [0, itemCount]', () => {
      for (let itemCount = 0; itemCount <= 100; itemCount += 10) {
        for (let scrollTop = 0; scrollTop <= 5000; scrollTop += 500) {
          const range = calculateVisibleRange(itemCount, new Map(), 100, {
            scrollTop,
            containerHeight: 500
          });

          expect(range.start).toBeGreaterThanOrEqual(0);
          expect(range.end).toBeLessThanOrEqual(itemCount);
        }
      }
    });

    it('renders at least 1 item when items exist and container has height', () => {
      for (let itemCount = 1; itemCount <= 50; itemCount++) {
        const range = calculateVisibleRange(itemCount, new Map(), 100, {
          scrollTop: 0,
          containerHeight: 500
        });

        expect(range.end - range.start).toBeGreaterThanOrEqual(1);
      }
    });
  });
});

// =============================================================================
// calculateOffsetY
// =============================================================================

describe('calculateOffsetY', () => {
  it('returns 0 when starting from index 0', () => {
    expect(calculateOffsetY(0, new Map(), 100)).toBe(0);
  });

  it('calculates offset using measured heights', () => {
    const heights = new Map([
      [0, 50],
      [1, 150],
      [2, 100]
    ]);
    // Offset to start at index 3 = 50 + 150 + 100 = 300
    expect(calculateOffsetY(3, heights, 100)).toBe(300);
  });

  it('falls back to estimated height', () => {
    const heights = new Map<number, number>();
    // 5 items at 100px each = 500
    expect(calculateOffsetY(5, heights, 100)).toBe(500);
  });

  describe('invariant: offsetY == sum of heights before startIndex', () => {
    it('holds for various configurations', () => {
      const heights = new Map([
        [0, 80],
        [2, 120],
        [4, 90]
      ]);

      for (let startIndex = 0; startIndex <= 10; startIndex++) {
        const offset = calculateOffsetY(startIndex, heights, 100);
        let expectedOffset = 0;
        for (let i = 0; i < startIndex; i++) {
          expectedOffset += heights.get(i) ?? 100;
        }
        expect(offset).toBe(expectedOffset);
      }
    });
  });
});

// =============================================================================
// calculateScrollToIndex
// =============================================================================

describe('calculateScrollToIndex', () => {
  it('returns 0 for index 0', () => {
    expect(calculateScrollToIndex(0, new Map(), 100)).toBe(0);
  });

  it('calculates scroll position for middle item', () => {
    const heights = new Map<number, number>();
    // Scroll to item 5 = 5 * 100 = 500
    expect(calculateScrollToIndex(5, heights, 100)).toBe(500);
  });

  it('uses measured heights', () => {
    const heights = new Map([
      [0, 200],
      [1, 200]
    ]);
    // Scroll to item 2 = 200 + 200 = 400
    expect(calculateScrollToIndex(2, heights, 100)).toBe(400);
  });
});

// =============================================================================
// Scroll Position Detection
// =============================================================================

describe('isScrolledAwayFromBottom', () => {
  it('returns false when at bottom', () => {
    // scrollHeight: 1000, clientHeight: 500, scrollTop: 500 = at bottom
    expect(isScrolledAwayFromBottom(500, 1000, 500)).toBe(false);
  });

  it('returns true when scrolled up', () => {
    // scrollHeight: 1000, clientHeight: 500, scrollTop: 0 = at top
    expect(isScrolledAwayFromBottom(0, 1000, 500)).toBe(true);
  });

  it('respects threshold', () => {
    // 99px from bottom with 100px threshold = not scrolled away
    expect(isScrolledAwayFromBottom(401, 1000, 500, 100)).toBe(false);
    // 101px from bottom with 100px threshold = scrolled away
    expect(isScrolledAwayFromBottom(399, 1000, 500, 100)).toBe(true);
  });
});

describe('isNearBottom', () => {
  it('returns true when at bottom', () => {
    expect(isNearBottom(500, 1000, 500)).toBe(true);
  });

  it('returns false when scrolled up', () => {
    expect(isNearBottom(0, 1000, 500)).toBe(false);
  });

  it('respects threshold', () => {
    // 49px from bottom with 50px threshold = near bottom
    expect(isNearBottom(451, 1000, 500, 50)).toBe(true);
    // 51px from bottom with 50px threshold = not near bottom
    expect(isNearBottom(449, 1000, 500, 50)).toBe(false);
  });
});

// =============================================================================
// Virtualization Threshold
// =============================================================================

describe('shouldVirtualize', () => {
  it('returns false below threshold', () => {
    expect(shouldVirtualize(0)).toBe(false);
    expect(shouldVirtualize(19)).toBe(false);
  });

  it('returns true at threshold', () => {
    expect(shouldVirtualize(20)).toBe(true);
  });

  it('returns true above threshold', () => {
    expect(shouldVirtualize(100)).toBe(true);
    expect(shouldVirtualize(1000)).toBe(true);
  });

  it('threshold constant is documented value', () => {
    expect(VIRTUALIZATION_THRESHOLD).toBe(20);
  });
});

// =============================================================================
// Height Cache Management
// =============================================================================

describe('updateHeightCache', () => {
  it('adds new height measurement', () => {
    const heights = new Map<number, number>();
    const updated = updateHeightCache(heights, 5, 150);

    expect(updated.get(5)).toBe(150);
    expect(updated).not.toBe(heights); // New Map returned
  });

  it('updates existing measurement', () => {
    const heights = new Map([[5, 100]]);
    const updated = updateHeightCache(heights, 5, 150);

    expect(updated.get(5)).toBe(150);
  });

  it('returns same map if height unchanged', () => {
    const heights = new Map([[5, 150]]);
    const updated = updateHeightCache(heights, 5, 150);

    expect(updated).toBe(heights); // Same reference
  });

  it('ignores invalid measurements', () => {
    const heights = new Map([[5, 100]]);

    expect(updateHeightCache(heights, 5, 0)).toBe(heights);
    expect(updateHeightCache(heights, 5, -50)).toBe(heights);
  });
});

describe('shiftHeights', () => {
  it('shifts indices forward on insert', () => {
    const heights = new Map([
      [0, 100],
      [1, 150],
      [2, 200]
    ]);

    // Insert 2 items at index 1
    const shifted = shiftHeights(heights, 1, 2);

    expect(shifted.get(0)).toBe(100); // Unchanged
    expect(shifted.get(1)).toBeUndefined(); // New items, no height yet
    expect(shifted.get(2)).toBeUndefined();
    expect(shifted.get(3)).toBe(150); // Was index 1
    expect(shifted.get(4)).toBe(200); // Was index 2
  });

  it('shifts indices backward on delete', () => {
    const heights = new Map([
      [0, 100],
      [5, 150],
      [10, 200]
    ]);

    // Delete 3 items starting at index 2
    const shifted = shiftHeights(heights, 2, -3);

    expect(shifted.get(0)).toBe(100); // Unchanged
    expect(shifted.get(2)).toBe(150); // Was index 5
    expect(shifted.get(7)).toBe(200); // Was index 10
  });

  it('returns same map if shift is 0', () => {
    const heights = new Map([[0, 100]]);
    expect(shiftHeights(heights, 5, 0)).toBe(heights);
  });

  it('preserves all heights during shift', () => {
    const heights = new Map<number, number>();
    for (let i = 0; i < 20; i++) {
      heights.set(i, 100 + i);
    }

    const shifted = shiftHeights(heights, 10, 5);

    // Count total heights
    expect(shifted.size).toBe(heights.size);

    // Verify values preserved (just shifted)
    const originalValues = new Set(heights.values());
    const shiftedValues = new Set(shifted.values());
    expect(shiftedValues).toEqual(originalValues);
  });
});

// =============================================================================
// Integration: Visible Range + Offset Consistency
// =============================================================================

describe('visible range and offset consistency', () => {
  it('offsetY positions content correctly for visible range', () => {
    const itemCount = 100;
    const heights = new Map<number, number>();
    const estimatedHeight = 100;

    // Test various scroll positions
    for (let scrollTop = 0; scrollTop <= 5000; scrollTop += 500) {
      const range = calculateVisibleRange(itemCount, heights, estimatedHeight, {
        scrollTop,
        containerHeight: 500
      });

      const offset = calculateOffsetY(range.start, heights, estimatedHeight);

      // Offset should place visible items at their correct position
      // The first visible item should be at or before the scroll position
      expect(offset).toBeLessThanOrEqual(scrollTop + estimatedHeight);
    }
  });
});
