/**
 * S-Tier Tests for fuzzy.ts
 *
 * Tests the three public functions: fuzzyMatch, fuzzyScore, highlightMatches.
 * Validates matching correctness, scoring ordering invariants, and highlight
 * segment rendering.
 */

import { fuzzyMatch, fuzzyScore, highlightMatches } from './fuzzy.js';

// =============================================================================
// fuzzyMatch
// =============================================================================

describe('fuzzyMatch', () => {
  it('empty pattern matches everything (returns [])', () => {
    expect(fuzzyMatch('', 'anything')).toEqual([]);
  });

  it('exact match returns sequential indices', () => {
    expect(fuzzyMatch('abc', 'abc')).toEqual([0, 1, 2]);
  });

  it('prefix match', () => {
    expect(fuzzyMatch('ab', 'abcdef')).toEqual([0, 1]);
  });

  it('scattered match', () => {
    // 'fz' in 'fuzzy' → f at 0, z at 2
    const result = fuzzyMatch('fz', 'fuzzy');
    expect(result).toEqual([0, 2]);
  });

  it('no match returns null', () => {
    expect(fuzzyMatch('xyz', 'abc')).toBeNull();
  });

  it('partial pattern match returns null', () => {
    // Only first char matches, but not all pattern chars
    expect(fuzzyMatch('az', 'abc')).toBeNull();
  });

  it('case insensitive matching', () => {
    expect(fuzzyMatch('ABC', 'abcdef')).toEqual([0, 1, 2]);
    expect(fuzzyMatch('abc', 'ABCDEF')).toEqual([0, 1, 2]);
  });

  it('pattern longer than string returns null', () => {
    expect(fuzzyMatch('abcdef', 'abc')).toBeNull();
  });

  it('matching with gaps', () => {
    // 'so' in 'src/routes' → greedy leftmost: s at 0, o at 5
    const result = fuzzyMatch('so', 'src/routes');
    expect(result).not.toBeNull();
    expect(result![0]).toBe(0); // s
    expect(result![1]).toBe(5); // o
  });

  it('greedy leftmost matching', () => {
    // 'aa' in 'abac' → should match indices 0 and 2 (first 'a', then next 'a')
    const result = fuzzyMatch('aa', 'abac');
    expect(result).toEqual([0, 2]);
  });

  it('single character match', () => {
    expect(fuzzyMatch('b', 'abc')).toEqual([1]);
  });

  it('unicode characters', () => {
    expect(fuzzyMatch('é', 'café')).toEqual([3]);
  });
});

// =============================================================================
// fuzzyScore
// =============================================================================

describe('fuzzyScore', () => {
  it('empty indices returns 0', () => {
    expect(fuzzyScore('', 'anything', [])).toBe(0);
  });

  it('start-of-string bonus (lower score)', () => {
    const atStart = fuzzyScore('a', 'abc', [0]);
    const notAtStart = fuzzyScore('b', 'abc', [1]);
    expect(atStart).toBeLessThan(notAtStart);
  });

  it('consecutive matches score lower than scattered', () => {
    // 'ab' consecutive in 'abc' vs scattered in 'axbxc'
    const consecutive = fuzzyScore('ab', 'abc', [0, 1]);
    const scattered = fuzzyScore('ab', 'axbxc', [0, 2]);
    expect(consecutive).toBeLessThan(scattered);
  });

  it('shorter strings preferred', () => {
    // Same pattern and indices structure, but different string lengths
    const short = fuzzyScore('a', 'ab', [0]);
    const long = fuzzyScore('a', 'abcdefghij', [0]);
    expect(short).toBeLessThan(long);
  });

  it('exact case matches score lower', () => {
    // 'A' matches 'A' (exact case) vs 'a' (different case)
    const exactCase = fuzzyScore('A', 'Abc', [0]);
    const wrongCase = fuzzyScore('a', 'Abc', [0]); // pattern 'a', string has 'A'
    expect(exactCase).toBeLessThan(wrongCase);
  });

  it('ordering property: better matches rank first', () => {
    // Sort candidates by score — exact match should rank above scattered
    const candidates = ['src/main.ts', 'some/random/module.ts', 'smt'];
    const scores = candidates.map((c) => {
      const indices = fuzzyMatch('smt', c);
      if (!indices) return Infinity;
      return fuzzyScore('smt', c, indices);
    });

    // 'smt' (exact) should score better than 'src/main.ts' (scattered)
    expect(scores[2]).toBeLessThan(scores[0]!);
  });
});

// =============================================================================
// highlightMatches
// =============================================================================

describe('highlightMatches', () => {
  it('no indices returns single unhighlighted span', () => {
    expect(highlightMatches('hello', [])).toEqual([{ text: 'hello', highlight: false }]);
  });

  it('all indices returns single highlighted span', () => {
    expect(highlightMatches('abc', [0, 1, 2])).toEqual([{ text: 'abc', highlight: true }]);
  });

  it('prefix highlight', () => {
    expect(highlightMatches('abcdef', [0, 1])).toEqual([
      { text: 'ab', highlight: true },
      { text: 'cdef', highlight: false }
    ]);
  });

  it('suffix highlight', () => {
    expect(highlightMatches('abcdef', [4, 5])).toEqual([
      { text: 'abcd', highlight: false },
      { text: 'ef', highlight: true }
    ]);
  });

  it('middle highlight', () => {
    expect(highlightMatches('abcdef', [2, 3])).toEqual([
      { text: 'ab', highlight: false },
      { text: 'cd', highlight: true },
      { text: 'ef', highlight: false }
    ]);
  });

  it('alternating highlights', () => {
    // Indices: 0, 2 → a highlighted, b not, c highlighted, def not
    expect(highlightMatches('abcdef', [0, 2])).toEqual([
      { text: 'a', highlight: true },
      { text: 'b', highlight: false },
      { text: 'c', highlight: true },
      { text: 'def', highlight: false }
    ]);
  });

  it('adjacent indices merged into one segment', () => {
    const parts = highlightMatches('abcdef', [1, 2, 3]);
    // Should be: "a" (not), "bcd" (highlighted), "ef" (not)
    expect(parts).toEqual([
      { text: 'a', highlight: false },
      { text: 'bcd', highlight: true },
      { text: 'ef', highlight: false }
    ]);
  });

  it('single character name', () => {
    expect(highlightMatches('x', [0])).toEqual([{ text: 'x', highlight: true }]);
  });

  it('empty name returns single empty span', () => {
    expect(highlightMatches('', [])).toEqual([{ text: '', highlight: false }]);
  });
});
