import { projectHash, projectStorageKey, LAYOUT_META_KEY } from './project-id.js';

describe('projectHash', () => {
  it('produces stable IDs for the same path', () => {
    const id1 = projectHash('/Users/alex/projects/foo');
    const id2 = projectHash('/Users/alex/projects/foo');
    expect(id1).toBe(id2);
  });

  it('produces different IDs for different paths', () => {
    const id1 = projectHash('/Users/alex/projects/foo');
    const id2 = projectHash('/Users/alex/projects/bar');
    expect(id1).not.toBe(id2);
  });

  it('returns a string prefixed with "proj-"', () => {
    const id = projectHash('/some/path');
    expect(id).toMatch(/^proj-/);
  });

  it('handles empty string', () => {
    const id = projectHash('');
    expect(id).toBe('proj-0');
  });

  it('handles single character', () => {
    const id = projectHash('a');
    expect(typeof id).toBe('string');
    expect(id.startsWith('proj-')).toBe(true);
  });
});

describe('projectStorageKey', () => {
  it('returns correct format', () => {
    expect(projectStorageKey('proj-abc')).toBe('workshop_layout:proj-abc');
  });

  it('includes the project ID', () => {
    const key = projectStorageKey('proj-xyz123');
    expect(key).toContain('proj-xyz123');
  });
});

describe('LAYOUT_META_KEY', () => {
  it('is a string constant', () => {
    expect(typeof LAYOUT_META_KEY).toBe('string');
    expect(LAYOUT_META_KEY).toBe('workshop_layout:meta');
  });
});
