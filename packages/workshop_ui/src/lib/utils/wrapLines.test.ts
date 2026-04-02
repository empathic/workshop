/**
 * S-Tier Tests for wrapLines.ts
 *
 * Tests the code line wrapping function that handles multi-line spans
 * from syntax highlighting. Each line must be wrapped in a .code-line
 * span with properly closed and reopened HTML tags.
 */

import { wrapLines } from './wrapLines.js';

describe('wrapLines', () => {
  it('single line with no spans', () => {
    expect(wrapLines('hello')).toBe('<span class="code-line">hello</span>');
  });

  it('multi-line with no spans', () => {
    const result = wrapLines('a\nb');
    expect(result).toBe('<span class="code-line">a</span>' + '<span class="code-line">b</span>');
  });

  it('trailing newline stripped', () => {
    const result = wrapLines('a\nb\n');
    // Should be 2 lines, not 3 (trailing empty line removed)
    expect(result).toBe('<span class="code-line">a</span>' + '<span class="code-line">b</span>');
  });

  it('empty line gets space character', () => {
    const result = wrapLines('a\n\nb');
    // Middle empty line should get a space (not empty)
    expect(result).toContain('<span class="code-line"> </span>');
  });

  it('span within a single line preserved', () => {
    const html = '<span class="kw">if</span> true';
    const result = wrapLines(html);
    expect(result).toBe(`<span class="code-line">${html}</span>`);
  });

  it('span crossing line boundary closed and reopened', () => {
    // A span that starts on line 1 and continues on line 2
    const html = '<span class="kw">if\nthen</span>';
    const result = wrapLines(html);

    // Line 1: open kw span, text "if", close kw span
    // Line 2: reopen kw span, text "then</span>", close kw span
    expect(result).toBe(
      '<span class="code-line"><span class="kw">if</span></span>' +
        '<span class="code-line"><span class="kw">then</span></span>'
    );
  });

  it('nested spans tracked correctly', () => {
    // Outer span wraps entire content, inner span on first line only
    const html = '<span class="outer"><span class="inner">a</span>\nb</span>';
    const result = wrapLines(html);

    // Line 1: outer opens, inner opens, "a", inner closes → outer still open at EOL
    // Line 2: reopen outer, "b", outer closes
    expect(result).toBe(
      '<span class="code-line"><span class="outer"><span class="inner">a</span></span></span>' +
        '<span class="code-line"><span class="outer">b</span></span>'
    );
  });

  it('no spans — plain text wrapped correctly', () => {
    const result = wrapLines('function main() {\n  return 0;\n}');
    expect(result).toBe(
      '<span class="code-line">function main() {</span>' +
        '<span class="code-line">  return 0;</span>' +
        '<span class="code-line">}</span>'
    );
  });

  it('single empty string gets space', () => {
    const result = wrapLines('');
    expect(result).toBe('<span class="code-line"> </span>');
  });
});
