/**
 * Markdown rendering with syntax highlighting and line numbers
 */

import { Marked } from 'marked';
import { markedHighlight } from 'marked-highlight';
import hljs from 'highlight.js';
import rust from 'highlight.js/lib/languages/rust';
import { wrapLines } from './wrapLines.js';

// Prevent warnings about unescaped HTML in code blocks
hljs.configure({ ignoreUnescapedHTML: true });

// Patch highlight.js Rust grammar: the char-literal escape pattern is missing \\
// (escaped backslash). Without this, '\\'  eats the closing quote and everything
// after it highlights as a string.  https://github.com/highlightjs/highlight.js/issues/...
hljs.registerLanguage('rust', (hljs) => {
  const lang = rust(hljs);
  patchCharEscapes(lang);
  return lang;
});

function patchCharEscapes(obj: unknown): void {
  if (!obj || typeof obj !== 'object') return;
  const rec = obj as Record<string, unknown>;
  if (rec.scope === 'char.escape' && rec.match instanceof RegExp) {
    const src = rec.match.source; // e.g.  \\('|\w|x\w{2}|...)
    if (src.includes("('|") && !src.includes('\\\\|')) {
      rec.match = new RegExp(src.replace("('|", "('|\\\\|"), rec.match.flags);
    }
    return;
  }
  for (const v of Object.values(rec)) {
    if (Array.isArray(v)) v.forEach((i) => patchCharEscapes(i));
    else patchCharEscapes(v);
  }
}

// Create a configured marked instance with syntax highlighting
export const marked = new Marked(
  markedHighlight({
    emptyLangClass: 'hljs',
    langPrefix: 'hljs language-',
    highlight(code, lang) {
      try {
        const language = hljs.getLanguage(lang) ? lang : 'plaintext';
        return wrapLines(hljs.highlight(code, { language, ignoreIllegals: true }).value);
      } catch {
        // Fallback: auto-detect, then plain text
        try {
          return wrapLines(hljs.highlightAuto(code).value);
        } catch {
          return wrapLines(code);
        }
      }
    }
  }),
  {
    breaks: true,
    gfm: true
  }
);

/**
 * Render markdown to HTML with syntax highlighting
 */
export function renderMarkdown(content: string): string {
  if (!content) return '';
  return marked.parse(content) as string;
}
