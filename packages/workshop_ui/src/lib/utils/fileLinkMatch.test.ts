/**
 * S-Tier Tests for fileLinkMatch.ts
 *
 * Tests file path extraction from text and extension validation.
 * These catch regex matching regressions, line number parsing bugs,
 * and extension filtering errors.
 */

import { extractFilePaths, hasValidExtension } from './fileLinkMatch.js';

// =============================================================================
// hasValidExtension
// =============================================================================

describe('hasValidExtension', () => {
  it.each([
    'ts',
    'tsx',
    'js',
    'jsx',
    'rs',
    'go',
    'py',
    'svelte',
    'html',
    'css',
    'json',
    'yaml',
    'toml',
    'sh',
    'md',
    'sql',
    'c',
    'cpp',
    'java'
  ])('accepts known extension .%s', (ext) => {
    expect(hasValidExtension(`file.${ext}`)).toBe(true);
  });

  it('rejects unknown extensions', () => {
    expect(hasValidExtension('file.xyz')).toBe(false);
    expect(hasValidExtension('file.bmp')).toBe(false);
    expect(hasValidExtension('file.png')).toBe(false);
  });

  it('no extension returns false', () => {
    expect(hasValidExtension('Makefile')).toBe(false);
  });

  it('case insensitive', () => {
    expect(hasValidExtension('file.RS')).toBe(true);
    expect(hasValidExtension('file.Py')).toBe(true);
  });

  it('double extension uses last part', () => {
    expect(hasValidExtension('file.test.ts')).toBe(true);
    expect(hasValidExtension('file.backup.xyz')).toBe(false);
  });
});

// =============================================================================
// extractFilePaths
// =============================================================================

describe('extractFilePaths', () => {
  it('absolute path', () => {
    const results = extractFilePaths('See /home/user/src/main.rs for details');
    expect(results).toHaveLength(1);
    expect(results[0]!.path).toBe('/home/user/src/main.rs');
  });

  it('relative path', () => {
    const results = extractFilePaths('Check src/lib/utils/fuzzy.ts');
    expect(results).toHaveLength(1);
    expect(results[0]!.path).toBe('src/lib/utils/fuzzy.ts');
  });

  it('dot-relative path', () => {
    const results = extractFilePaths('Open ./src/main.rs now');
    expect(results).toHaveLength(1);
    expect(results[0]!.path).toBe('./src/main.rs');
  });

  it('parent-relative path', () => {
    const results = extractFilePaths('From ../lib/utils.ts');
    expect(results).toHaveLength(1);
    expect(results[0]!.path).toBe('../lib/utils.ts');
  });

  it('path with line number', () => {
    const results = extractFilePaths('Error at src/main.rs:42');
    expect(results).toHaveLength(1);
    expect(results[0]!.path).toBe('src/main.rs');
    expect(results[0]!.line).toBe(42);
  });

  it('path without line number has undefined line', () => {
    const results = extractFilePaths('In src/main.rs file');
    expect(results[0]!.line).toBeUndefined();
  });

  it('multiple paths in one string', () => {
    const results = extractFilePaths('Changed src/lib/foo.ts and src/lib/bar.ts');
    expect(results).toHaveLength(2);
    expect(results[0]!.path).toBe('src/lib/foo.ts');
    expect(results[1]!.path).toBe('src/lib/bar.ts');
  });

  it('no paths in text', () => {
    const results = extractFilePaths('Just some regular text here');
    expect(results).toHaveLength(0);
  });

  it('path in parentheses', () => {
    const results = extractFilePaths('(src/lib/utils.ts)');
    expect(results).toHaveLength(1);
    expect(results[0]!.path).toBe('src/lib/utils.ts');
  });

  it('path in quotes', () => {
    const results = extractFilePaths('"src/lib/utils.ts"');
    expect(results).toHaveLength(1);
    expect(results[0]!.path).toBe('src/lib/utils.ts');
  });

  it('path in backticks', () => {
    const results = extractFilePaths('`src/lib/utils.ts`');
    expect(results).toHaveLength(1);
    expect(results[0]!.path).toBe('src/lib/utils.ts');
  });

  it('filters out unknown extensions', () => {
    const results = extractFilePaths('See src/images/logo.png for the logo');
    expect(results).toHaveLength(0);
  });

  it('position tracking (start/end)', () => {
    const text = 'Open src/main.rs now';
    const results = extractFilePaths(text);
    expect(results).toHaveLength(1);
    const extracted = text.slice(results[0]!.start, results[0]!.end);
    expect(extracted).toBe('src/main.rs');
  });

  it('position tracking with line number', () => {
    const text = 'Error at src/main.rs:42 here';
    const results = extractFilePaths(text);
    const extracted = text.slice(results[0]!.start, results[0]!.end);
    expect(extracted).toBe('src/main.rs:42');
  });

  it('path at start of text', () => {
    const results = extractFilePaths('src/main.rs contains the entry point');
    expect(results).toHaveLength(1);
    expect(results[0]!.path).toBe('src/main.rs');
  });

  it('nested directory path', () => {
    const results = extractFilePaths('See src/a/b/c/d/file.ts');
    expect(results).toHaveLength(1);
    expect(results[0]!.path).toBe('src/a/b/c/d/file.ts');
  });

  it('path with dots in directory names', () => {
    const results = extractFilePaths('In node_modules/types/node/index.d.ts');
    expect(results).toHaveLength(1);
    expect(results[0]!.path).toBe('node_modules/types/node/index.d.ts');
  });

  it('svelte component path', () => {
    const results = extractFilePaths('Edit src/components/Header.svelte');
    expect(results).toHaveLength(1);
    expect(results[0]!.path).toBe('src/components/Header.svelte');
  });
});
