import { isGapValid, applyGapReorder, sortByOrder } from './project-order.js';

describe('isGapValid', () => {
  // For item at index 1 in [A, B, C], gaps 1 and 2 are adjacent (no-ops)
  it('returns false for gap immediately before the item', () => {
    expect(isGapValid(1, 1)).toBe(false);
  });

  it('returns false for gap immediately after the item', () => {
    expect(isGapValid(1, 2)).toBe(false);
  });

  it('returns true for gap two positions before', () => {
    expect(isGapValid(2, 0)).toBe(true);
  });

  it('returns true for gap two positions after', () => {
    expect(isGapValid(0, 2)).toBe(true);
  });

  it('returns false for fromIdx -1 (not found)', () => {
    expect(isGapValid(-1, 0)).toBe(false);
  });

  // First item: gaps 0 and 1 are no-ops
  it('returns false for first item at gap 0', () => {
    expect(isGapValid(0, 0)).toBe(false);
  });

  it('returns false for first item at gap 1', () => {
    expect(isGapValid(0, 1)).toBe(false);
  });

  it('returns true for first item at gap 2', () => {
    expect(isGapValid(0, 2)).toBe(true);
  });

  // Last item (idx 2 in 3-item list): gaps 2 and 3 are no-ops
  it('returns false for last item at its own gap', () => {
    expect(isGapValid(2, 2)).toBe(false);
  });

  it('returns false for last item at gap after it', () => {
    expect(isGapValid(2, 3)).toBe(false);
  });

  it('returns true for last item at gap 0', () => {
    expect(isGapValid(2, 0)).toBe(true);
  });
});

describe('applyGapReorder', () => {
  // For [A, B, C] (indices 0,1,2), gaps are: 0 | A | 1 | B | 2 | C | 3

  it('moves first item to end (gap 3)', () => {
    expect(applyGapReorder(['A', 'B', 'C'], 'A', 3)).toEqual(['B', 'C', 'A']);
  });

  it('moves last item to front (gap 0)', () => {
    expect(applyGapReorder(['A', 'B', 'C'], 'C', 0)).toEqual(['C', 'A', 'B']);
  });

  it('moves middle item to front (gap 0)', () => {
    expect(applyGapReorder(['A', 'B', 'C'], 'B', 0)).toEqual(['B', 'A', 'C']);
  });

  it('moves middle item to end (gap 3)', () => {
    expect(applyGapReorder(['A', 'B', 'C'], 'B', 3)).toEqual(['A', 'C', 'B']);
  });

  it('moves first item to middle (gap 2)', () => {
    // Gap 2 = between B and C
    expect(applyGapReorder(['A', 'B', 'C'], 'A', 2)).toEqual(['B', 'A', 'C']);
  });

  it('moves last item to middle (gap 1)', () => {
    // Gap 1 = between A and B
    expect(applyGapReorder(['A', 'B', 'C'], 'C', 1)).toEqual(['A', 'C', 'B']);
  });

  // No-op cases: gap immediately before or after the source
  it('returns null for gap immediately before source (gap === fromIdx)', () => {
    expect(applyGapReorder(['A', 'B', 'C'], 'B', 1)).toBeNull(); // gap 1 is before B(idx 1)
  });

  it('returns null for gap immediately after source (gap === fromIdx + 1)', () => {
    expect(applyGapReorder(['A', 'B', 'C'], 'B', 2)).toBeNull(); // gap 2 is after B(idx 1)
  });

  it('returns null for first item at gap 0', () => {
    expect(applyGapReorder(['A', 'B', 'C'], 'A', 0)).toBeNull();
  });

  it('returns null for first item at gap 1', () => {
    expect(applyGapReorder(['A', 'B', 'C'], 'A', 1)).toBeNull();
  });

  it('returns null for last item at gap N-1', () => {
    expect(applyGapReorder(['A', 'B', 'C'], 'C', 2)).toBeNull();
  });

  it('returns null for last item at gap N', () => {
    expect(applyGapReorder(['A', 'B', 'C'], 'C', 3)).toBeNull();
  });

  // Edge cases
  it('returns null if fromId is not in the array', () => {
    expect(applyGapReorder(['A', 'B', 'C'], 'X', 0)).toBeNull();
  });

  it('handles two-item list', () => {
    expect(applyGapReorder(['A', 'B'], 'A', 2)).toEqual(['B', 'A']);
    expect(applyGapReorder(['A', 'B'], 'B', 0)).toEqual(['B', 'A']);
  });

  it('returns null for single-item list (all gaps are adjacent)', () => {
    expect(applyGapReorder(['A'], 'A', 0)).toBeNull();
    expect(applyGapReorder(['A'], 'A', 1)).toBeNull();
  });

  it('clamps gap index past end of list', () => {
    expect(applyGapReorder(['A', 'B', 'C'], 'A', 99)).toEqual(['B', 'C', 'A']);
  });

  it('does not mutate the input array', () => {
    const input = ['A', 'B', 'C'];
    applyGapReorder(input, 'A', 3);
    expect(input).toEqual(['A', 'B', 'C']);
  });
});

describe('sortByOrder', () => {
  const items = [
    { id: 'A', name: 'alpha' },
    { id: 'B', name: 'beta' },
    { id: 'C', name: 'gamma' }
  ];

  it('returns items in original order when order array is empty', () => {
    expect(sortByOrder(items, [])).toEqual(items);
  });

  it('sorts items by the given order', () => {
    const result = sortByOrder(items, ['C', 'A', 'B']);
    expect(result.map((i) => i.id)).toEqual(['C', 'A', 'B']);
  });

  it('reverses order', () => {
    const result = sortByOrder(items, ['C', 'B', 'A']);
    expect(result.map((i) => i.id)).toEqual(['C', 'B', 'A']);
  });

  it('puts known IDs first, unknown IDs appended', () => {
    const result = sortByOrder(items, ['C']);
    expect(result.map((i) => i.id)).toEqual(['C', 'A', 'B']);
  });

  it('ignores stale IDs in order that do not match any item', () => {
    const result = sortByOrder(items, ['X', 'C', 'Y', 'A', 'B']);
    expect(result.map((i) => i.id)).toEqual(['C', 'A', 'B']);
  });

  it('does not mutate the input array', () => {
    const input = [...items];
    sortByOrder(input, ['C', 'B', 'A']);
    expect(input.map((i) => i.id)).toEqual(['A', 'B', 'C']);
  });

  it('handles partial order — unordered items preserve relative position', () => {
    const four = [
      { id: 'A', name: 'a' },
      { id: 'B', name: 'b' },
      { id: 'C', name: 'c' },
      { id: 'D', name: 'd' }
    ];
    // Only C is ordered — it goes first, then A, B, D in original order
    const result = sortByOrder(four, ['C']);
    expect(result.map((i) => i.id)).toEqual(['C', 'A', 'B', 'D']);
  });
});
