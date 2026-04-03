import { deserializeDrafts, serializeDrafts, getDraft, setDraft, clearDraft } from './draft-map.js';

// =============================================================================
// deserializeDrafts
// =============================================================================

describe('deserializeDrafts', () => {
  it('returns empty map for null', () => {
    expect(deserializeDrafts(null)).toEqual(new Map());
  });

  it('returns empty map for empty string', () => {
    expect(deserializeDrafts('')).toEqual(new Map());
  });

  it('returns empty map for invalid JSON', () => {
    expect(deserializeDrafts('{not json')).toEqual(new Map());
  });

  it('returns empty map for JSON array', () => {
    expect(deserializeDrafts('[1,2,3]')).toEqual(new Map());
  });

  it('returns empty map for JSON primitive', () => {
    expect(deserializeDrafts('"hello"')).toEqual(new Map());
    expect(deserializeDrafts('42')).toEqual(new Map());
  });

  it('parses valid object', () => {
    const json = JSON.stringify({ 'inst-1': 'hello', 'inst-2': 'world' });
    const result = deserializeDrafts(json);
    expect(result.size).toBe(2);
    expect(result.get('inst-1')).toBe('hello');
    expect(result.get('inst-2')).toBe('world');
  });

  it('filters out non-string values', () => {
    // A hand-crafted JSON with bad values — the type cast at runtime would
    // produce these if localStorage was tampered with.
    const json = JSON.stringify({ good: 'ok', bad: 42, ugly: null });
    const result = deserializeDrafts(json);
    expect(result.size).toBe(1);
    expect(result.get('good')).toBe('ok');
  });

  it('filters out empty-string values', () => {
    const json = JSON.stringify({ a: 'draft', b: '' });
    const result = deserializeDrafts(json);
    expect(result.size).toBe(1);
    expect(result.has('b')).toBe(false);
  });
});

// =============================================================================
// serializeDrafts
// =============================================================================

describe('serializeDrafts', () => {
  it('serializes empty map to empty object', () => {
    expect(serializeDrafts(new Map())).toBe('{}');
  });

  it('round-trips through deserialize', () => {
    const original = new Map([
      ['a', 'hello'],
      ['b', 'world']
    ]);
    const json = serializeDrafts(original);
    const restored = deserializeDrafts(json);
    expect(restored).toEqual(original);
  });

  it('preserves unicode content', () => {
    const original = new Map([['x', 'café ☕ 日本語']]);
    const restored = deserializeDrafts(serializeDrafts(original));
    expect(restored.get('x')).toBe('café ☕ 日本語');
  });
});

// =============================================================================
// getDraft
// =============================================================================

describe('getDraft', () => {
  it('returns draft text for existing key', () => {
    const drafts = new Map([['inst-1', 'hello']]);
    expect(getDraft(drafts, 'inst-1')).toBe('hello');
  });

  it('returns empty string for missing key', () => {
    expect(getDraft(new Map(), 'missing')).toBe('');
  });
});

// =============================================================================
// setDraft
// =============================================================================

describe('setDraft', () => {
  it('adds a new draft', () => {
    const result = setDraft(new Map(), 'inst-1', 'hello');
    expect(result.get('inst-1')).toBe('hello');
  });

  it('overwrites an existing draft', () => {
    const before = new Map([['inst-1', 'old']]);
    const after = setDraft(before, 'inst-1', 'new');
    expect(after.get('inst-1')).toBe('new');
  });

  it('deletes entry when text is empty', () => {
    const before = new Map([['inst-1', 'hello']]);
    const after = setDraft(before, 'inst-1', '');
    expect(after.has('inst-1')).toBe(false);
    expect(after.size).toBe(0);
  });

  it('returns a new Map (does not mutate input)', () => {
    const before = new Map([['a', 'x']]);
    const after = setDraft(before, 'b', 'y');
    expect(after).not.toBe(before);
    expect(before.has('b')).toBe(false);
  });

  it('preserves other entries', () => {
    const before = new Map([
      ['a', 'one'],
      ['b', 'two']
    ]);
    const after = setDraft(before, 'a', 'updated');
    expect(after.get('a')).toBe('updated');
    expect(after.get('b')).toBe('two');
  });
});

// =============================================================================
// clearDraft
// =============================================================================

describe('clearDraft', () => {
  it('removes an existing entry', () => {
    const before = new Map([['inst-1', 'hello']]);
    const after = clearDraft(before, 'inst-1');
    expect(after.has('inst-1')).toBe(false);
  });

  it('is a no-op for missing key', () => {
    const before = new Map([['a', 'x']]);
    const after = clearDraft(before, 'missing');
    expect(after.size).toBe(1);
    expect(after.get('a')).toBe('x');
  });

  it('returns a new Map (does not mutate input)', () => {
    const before = new Map([['a', 'x']]);
    const after = clearDraft(before, 'a');
    expect(after).not.toBe(before);
    expect(before.get('a')).toBe('x');
  });
});
