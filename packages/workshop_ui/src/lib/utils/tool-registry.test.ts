/**
 * S-Tier Tests for tool-registry.ts
 *
 * Tests the tool widget registry: config lookup, badge labels, expanded fields,
 * truncation, and the default fallback for unknown tools. Every registered tool
 * gets its contract verified. Property-based thinking applied where useful.
 */

import { getToolConfig, truncate, truncateField, TOOL_REGISTRY } from './tool-registry.js';
import type { ToolInput } from './tool-registry.js';

// =============================================================================
// Helpers
// =============================================================================

function tool(name: string, input: Record<string, unknown> = {}): ToolInput {
  return { name, input };
}

/** All tool names registered in the registry. */
const REGISTERED_TOOLS = Object.keys(TOOL_REGISTRY);

/** Badge-mode tools (all except card-mode). */
const BADGE_TOOLS = REGISTERED_TOOLS.filter((name) => TOOL_REGISTRY[name]!.renderMode === 'badge');

/** Card-mode tools. */
const CARD_TOOLS = REGISTERED_TOOLS.filter((name) => TOOL_REGISTRY[name]!.renderMode === 'card');

// =============================================================================
// truncate / truncateField
// =============================================================================

describe('truncate', () => {
  it('returns short strings unchanged', () => {
    expect(truncate('hello')).toBe('hello');
  });

  it('returns exactly max-length strings unchanged', () => {
    const s = 'a'.repeat(40);
    expect(truncate(s)).toBe(s);
  });

  it('truncates at 40 chars by default and appends ellipsis', () => {
    const s = 'a'.repeat(41);
    expect(truncate(s)).toBe('a'.repeat(40) + '\u2026');
  });

  it('respects custom max', () => {
    expect(truncate('abcdef', 3)).toBe('abc\u2026');
  });

  it('handles empty string', () => {
    expect(truncate('')).toBe('');
  });
});

describe('truncateField', () => {
  it('returns strings under 500 unchanged', () => {
    const s = 'x'.repeat(500);
    expect(truncateField(s)).toBe(s);
  });

  it('truncates at 500 chars by default', () => {
    const s = 'x'.repeat(501);
    expect(truncateField(s)).toBe('x'.repeat(500) + '\u2026');
  });

  it('respects custom max', () => {
    expect(truncateField('abcdef', 4)).toBe('abcd\u2026');
  });
});

// =============================================================================
// getToolConfig: Lookup and Fallback
// =============================================================================

describe('getToolConfig', () => {
  it.each(REGISTERED_TOOLS)('returns a config for registered tool %s', (name) => {
    const config = getToolConfig(name);
    expect(config.icon).toBeDefined();
    expect(config.renderMode).toMatch(/^(badge|card)$/);
  });

  it('returns default config for unknown tool', () => {
    const config = getToolConfig('SomeUnknownTool');
    expect(config.icon).toBe('\u26A1');
    expect(config.renderMode).toBe('badge');
    expect(config.expandedFields).toBeDefined();
  });

  it('default config shows all input keys as expanded fields', () => {
    const config = getToolConfig('Unknown');
    const fields = config.expandedFields!(tool('Unknown', { foo: 'bar', count: 42 }));
    expect(fields).toEqual([
      { label: 'FOO', value: 'bar' },
      { label: 'COUNT', value: '42' }
    ]);
  });

  it('default config JSON-stringifies non-string values', () => {
    const config = getToolConfig('Unknown');
    const fields = config.expandedFields!(tool('Unknown', { data: { nested: true } }));
    expect(fields[0]!.label).toBe('DATA');
    expect(fields[0]!.value).toContain('"nested": true');
  });

  it('default config truncates long field values at 500 chars', () => {
    const config = getToolConfig('Unknown');
    const longVal = 'x'.repeat(600);
    const fields = config.expandedFields!(tool('Unknown', { big: longVal }));
    expect(fields[0]!.value.length).toBe(501); // 500 + ellipsis
  });
});

// =============================================================================
// Registry Structure Invariants
// =============================================================================

describe('registry structure', () => {
  it.each(BADGE_TOOLS)('%s has renderMode "badge"', (name) => {
    expect(getToolConfig(name).renderMode).toBe('badge');
  });

  it.each(CARD_TOOLS)('%s has renderMode "card"', (name) => {
    expect(getToolConfig(name).renderMode).toBe('card');
  });

  it('card-mode tools are Task, AskUserQuestion, and ExitPlanMode', () => {
    expect(CARD_TOOLS.sort()).toEqual(['AskUserQuestion', 'ExitPlanMode', 'Task']);
  });

  it.each(BADGE_TOOLS)('%s has expandedFields', (name) => {
    expect(getToolConfig(name).expandedFields).toBeDefined();
  });

  it('card-mode tools have no expandedFields (rendered by widget)', () => {
    for (const name of CARD_TOOLS) {
      expect(getToolConfig(name).expandedFields).toBeUndefined();
    }
  });
});

// =============================================================================
// badgeLabel: File-Operation Tools (Read, Write, Edit)
// =============================================================================

describe.each(['Read', 'Write', 'Edit'])('%s badgeLabel', (name) => {
  const config = getToolConfig(name);

  it('returns filename with file style for file_path', () => {
    const label = config.badgeLabel!({ file_path: '/src/lib/utils/foo.ts' });
    expect(label).toEqual({
      text: 'foo.ts',
      style: 'file',
      title: '/src/lib/utils/foo.ts'
    });
  });

  it('extracts last path segment', () => {
    const label = config.badgeLabel!({ file_path: '/a/b/c/deep.rs' });
    expect(label!.text).toBe('deep.rs');
  });

  it('handles root-level file', () => {
    const label = config.badgeLabel!({ file_path: 'README.md' });
    expect(label!.text).toBe('README.md');
  });

  it('returns null when file_path is missing', () => {
    expect(config.badgeLabel!({})).toBeNull();
  });

  it('returns null when file_path is not a string', () => {
    expect(config.badgeLabel!({ file_path: 123 })).toBeNull();
  });
});

// =============================================================================
// badgeLabel: Glob (file-style accent)
// =============================================================================

describe('Glob badgeLabel', () => {
  const config = getToolConfig('Glob');

  it('returns pattern with file style', () => {
    const label = config.badgeLabel!({ pattern: '**/*.ts' });
    expect(label).toEqual({
      text: '**/*.ts',
      style: 'file',
      title: '**/*.ts'
    });
  });

  it('truncates long patterns at 40 chars', () => {
    const longPat = 'src/components/' + 'a'.repeat(40) + '/*.svelte';
    const label = config.badgeLabel!({ pattern: longPat });
    expect(label!.text.length).toBe(41); // 40 + ellipsis
    expect(label!.title).toBe(longPat); // title is NOT truncated
  });

  it('returns null when pattern is missing', () => {
    expect(config.badgeLabel!({})).toBeNull();
  });
});

// =============================================================================
// badgeLabel: Detail-Style Tools
// =============================================================================

describe.each([
  ['Bash', 'command', 'ls -la'],
  ['Grep', 'pattern', 'function\\s+\\w+'],
  ['WebFetch', 'url', 'https://example.com/api'],
  ['WebSearch', 'query', 'svelte 5 runes migration']
] as const)('%s badgeLabel', (name, key, sample) => {
  const config = getToolConfig(name);

  it(`extracts "${key}" with detail style`, () => {
    const label = config.badgeLabel!({ [key]: sample });
    expect(label).toEqual({
      text: sample,
      style: 'detail',
      title: sample
    });
  });

  it('truncates long values at 40 chars', () => {
    const longVal = 'x'.repeat(50);
    const label = config.badgeLabel!({ [key]: longVal });
    expect(label!.text).toBe('x'.repeat(40) + '\u2026');
    expect(label!.title).toBe(longVal);
  });

  it('returns null when key is missing', () => {
    expect(config.badgeLabel!({})).toBeNull();
  });

  it('returns null when key is not a string', () => {
    expect(config.badgeLabel!({ [key]: 42 })).toBeNull();
  });
});

// =============================================================================
// expandedFields: File-Operation Tools
// =============================================================================

describe.each(['Read', 'Write', 'Edit'])('%s expandedFields', (name) => {
  const config = getToolConfig(name);
  const expand = config.expandedFields!;

  it('includes FILE as clickable when file_path present', () => {
    const fields = expand(tool(name, { file_path: '/foo/bar.ts' }));
    expect(fields).toContainEqual({
      label: 'FILE',
      value: '/foo/bar.ts',
      clickable: 'file'
    });
  });

  it('returns empty for empty input', () => {
    expect(expand(tool(name, {}))).toEqual([]);
  });
});

describe('Edit expandedFields', () => {
  const expand = getToolConfig('Edit').expandedFields!;

  it('includes OLD and NEW strings', () => {
    const fields = expand(
      tool('Edit', {
        file_path: '/f.ts',
        old_string: 'foo',
        new_string: 'bar'
      })
    );
    const labels = fields.map((f) => f.label);
    expect(labels).toEqual(['FILE', 'OLD', 'NEW']);
    expect(fields[1]!.value).toBe('foo');
    expect(fields[2]!.value).toBe('bar');
  });

  it('includes empty old_string (not filtered by truthiness)', () => {
    const fields = expand(tool('Edit', { old_string: '' }));
    expect(fields).toContainEqual({ label: 'OLD', value: '' });
  });
});

describe('Write expandedFields', () => {
  const expand = getToolConfig('Write').expandedFields!;

  it('truncates content at 500 chars', () => {
    const longContent = 'c'.repeat(600);
    const fields = expand(tool('Write', { file_path: '/f.ts', content: longContent }));
    const contentField = fields.find((f) => f.label === 'CONTENT');
    expect(contentField!.value.length).toBe(501); // 500 + ellipsis
  });

  it('preserves short content verbatim', () => {
    const fields = expand(tool('Write', { content: 'hello' }));
    expect(fields[0]!.value).toBe('hello');
  });
});

// =============================================================================
// expandedFields: Bash
// =============================================================================

describe('Bash expandedFields', () => {
  const expand = getToolConfig('Bash').expandedFields!;

  it('shows COMMAND and DESCRIPTION', () => {
    const fields = expand(tool('Bash', { command: 'npm test', description: 'Run tests' }));
    expect(fields).toEqual([
      { label: 'COMMAND', value: 'npm test' },
      { label: 'DESCRIPTION', value: 'Run tests' }
    ]);
  });

  it('omits missing fields', () => {
    const fields = expand(tool('Bash', { command: 'ls' }));
    expect(fields).toEqual([{ label: 'COMMAND', value: 'ls' }]);
  });

  it('returns empty for empty input', () => {
    expect(expand(tool('Bash', {}))).toEqual([]);
  });
});

// =============================================================================
// expandedFields: Glob
// =============================================================================

describe('Glob expandedFields', () => {
  const expand = getToolConfig('Glob').expandedFields!;

  it('PATTERN is clickable as glob, PATH is clickable as file', () => {
    const fields = expand(tool('Glob', { pattern: '**/*.ts', path: '/src' }));
    expect(fields).toEqual([
      { label: 'PATTERN', value: '**/*.ts', clickable: 'glob' },
      { label: 'PATH', value: '/src', clickable: 'file' }
    ]);
  });

  it('omits path when missing', () => {
    const fields = expand(tool('Glob', { pattern: '*.rs' }));
    expect(fields).toHaveLength(1);
    expect(fields[0]!.clickable).toBe('glob');
  });
});

// =============================================================================
// expandedFields: Grep
// =============================================================================

describe('Grep expandedFields', () => {
  const expand = getToolConfig('Grep').expandedFields!;

  it('shows PATTERN, PATH (clickable), and GLOB', () => {
    const fields = expand(
      tool('Grep', {
        pattern: 'TODO',
        path: '/src',
        glob: '*.ts'
      })
    );
    expect(fields).toEqual([
      { label: 'PATTERN', value: 'TODO' },
      { label: 'PATH', value: '/src', clickable: 'file' },
      { label: 'GLOB', value: '*.ts' }
    ]);
  });

  it('PATTERN is not clickable', () => {
    const fields = expand(tool('Grep', { pattern: 'foo' }));
    expect(fields[0]).toEqual({ label: 'PATTERN', value: 'foo' });
    expect(fields[0]).not.toHaveProperty('clickable');
  });
});

// =============================================================================
// expandedFields: WebFetch
// =============================================================================

describe('WebFetch expandedFields', () => {
  const expand = getToolConfig('WebFetch').expandedFields!;

  it('shows URL and PROMPT', () => {
    const fields = expand(
      tool('WebFetch', {
        url: 'https://example.com',
        prompt: 'Summarize'
      })
    );
    expect(fields).toEqual([
      { label: 'URL', value: 'https://example.com' },
      { label: 'PROMPT', value: 'Summarize' }
    ]);
  });
});

// =============================================================================
// expandedFields: WebSearch
// =============================================================================

describe('WebSearch expandedFields', () => {
  const expand = getToolConfig('WebSearch').expandedFields!;

  it('shows QUERY', () => {
    const fields = expand(tool('WebSearch', { query: 'rust async' }));
    expect(fields).toEqual([{ label: 'QUERY', value: 'rust async' }]);
  });
});

// =============================================================================
// Task: card-mode (no badgeLabel or expandedFields)
// =============================================================================

describe('Task card mode', () => {
  it('is card renderMode', () => {
    expect(getToolConfig('Task').renderMode).toBe('card');
  });

  it('has no badgeLabel', () => {
    expect(getToolConfig('Task').badgeLabel).toBeUndefined();
  });

  it('has no expandedFields', () => {
    expect(getToolConfig('Task').expandedFields).toBeUndefined();
  });
});

// =============================================================================
// EnterPlanMode: badge-mode (no badgeLabel, empty expandedFields)
// =============================================================================

describe('EnterPlanMode badge mode', () => {
  const config = getToolConfig('EnterPlanMode');

  it('is badge renderMode', () => {
    expect(config.renderMode).toBe('badge');
  });

  it('has no badgeLabel', () => {
    expect(config.badgeLabel).toBeUndefined();
  });

  it('has expandedFields that returns empty array', () => {
    expect(config.expandedFields).toBeDefined();
    expect(config.expandedFields!(tool('EnterPlanMode', {}))).toEqual([]);
  });

  it('returns empty array regardless of input', () => {
    expect(config.expandedFields!(tool('EnterPlanMode', { foo: 'bar' }))).toEqual([]);
  });
});

// =============================================================================
// ExitPlanMode: card-mode (no badgeLabel or expandedFields)
// =============================================================================

describe('ExitPlanMode card mode', () => {
  it('is card renderMode', () => {
    expect(getToolConfig('ExitPlanMode').renderMode).toBe('card');
  });

  it('has no badgeLabel', () => {
    expect(getToolConfig('ExitPlanMode').badgeLabel).toBeUndefined();
  });

  it('has no expandedFields', () => {
    expect(getToolConfig('ExitPlanMode').expandedFields).toBeUndefined();
  });
});

// =============================================================================
// Icon Consistency
// =============================================================================

describe('icons', () => {
  const EXPECTED_ICONS: Record<string, string> = {
    Read: '\u{1F4D6}',
    Write: '\u270F\uFE0F',
    Edit: '\u{1F527}',
    Bash: '\u{1F4BB}',
    Glob: '\u{1F50D}',
    Grep: '\u{1F50E}',
    WebFetch: '\u{1F310}',
    WebSearch: '\u{1F50D}',
    Task: '\u{1F4CB}',
    AskUserQuestion: '\u2753',
    EnterPlanMode: '\u{1F4D0}',
    ExitPlanMode: '\u{1F4D0}'
  };

  it.each(Object.entries(EXPECTED_ICONS))('%s has icon %s', (name, icon) => {
    expect(getToolConfig(name).icon).toBe(icon);
  });

  it('unknown tool gets lightning bolt', () => {
    expect(getToolConfig('Nope').icon).toBe('\u26A1');
  });
});

// =============================================================================
// Cross-Cutting Properties
// =============================================================================

describe('cross-cutting properties', () => {
  it.each(BADGE_TOOLS)('%s badgeLabel returns null for empty input', (name) => {
    const config = getToolConfig(name);
    if (config.badgeLabel) {
      expect(config.badgeLabel({})).toBeNull();
    }
  });

  it.each(BADGE_TOOLS)('%s expandedFields returns array for empty input', (name) => {
    const config = getToolConfig(name);
    const fields = config.expandedFields!(tool(name, {}));
    expect(Array.isArray(fields)).toBe(true);
    expect(fields.length).toBe(0);
  });

  it('no badge-mode tool is missing both badgeLabel and expandedFields', () => {
    for (const name of BADGE_TOOLS) {
      const config = getToolConfig(name);
      const hasSomething = config.badgeLabel != null || config.expandedFields != null;
      expect(hasSomething).toBe(true);
    }
  });

  it('getToolConfig is referentially stable for same name', () => {
    // The registry returns the same object, not a copy
    const a = getToolConfig('Bash');
    const b = getToolConfig('Bash');
    expect(a).toBe(b);
  });

  it('getToolConfig returns same default for all unknown names', () => {
    const a = getToolConfig('FakeTool1');
    const b = getToolConfig('FakeTool2');
    expect(a).toBe(b);
  });
});
