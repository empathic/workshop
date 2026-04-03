/**
 * Wrap highlighted code lines with spans for line numbering.
 * Properly handles multi-line hljs span elements by closing and
 * reopening them at line boundaries so each .code-line is well-formed HTML.
 *
 * Extracted from markdown.ts for testability.
 */
export function wrapLines(html: string): string {
  const lines = html.split('\n');
  // Code blocks typically end with a trailing newline → empty last element
  if (lines.length > 1 && lines[lines.length - 1] === '') {
    lines.pop();
  }

  let openTags: string[] = [];

  return lines
    .map((line) => {
      // Reopen spans that were still open at the end of the previous line
      const prefix = openTags.join('');

      // Walk this line's tags to figure out what's open at line-end
      const newOpenTags = [...openTags];
      const tagRegex = /<(\/?)span([^>]*)>/g;
      let m;
      while ((m = tagRegex.exec(line)) !== null) {
        if (m[1] === '/') {
          newOpenTags.pop();
        } else {
          newOpenTags.push(`<span${m[2]}>`);
        }
      }

      // Close every span that's open at end-of-line
      const closeSuffix = '</span>'.repeat(newOpenTags.length);
      openTags = newOpenTags;

      const inner = prefix + line + closeSuffix;
      return `<span class="code-line">${inner || ' '}</span>`;
    })
    .join(''); // no \n — .code-line uses display:block
}
