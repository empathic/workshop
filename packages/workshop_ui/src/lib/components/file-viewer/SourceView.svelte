<script lang="ts">
  import { renderMarkdown } from '$lib/utils/markdown';
  import hljs from 'highlight.js';

  interface Props {
    content: string;
    language: string;
    lineNumber: number | null;
    isError: boolean;
    isMarkdown: boolean;
  }

  let { content, language, lineNumber, isError, isMarkdown }: Props = $props();

  let codeEl: HTMLElement | undefined = $state();
  let showPreview = $state(false);

  // Highlight code content
  function highlightCode(content: string, language: string): string {
    if (!content) return '';

    try {
      if (language && hljs.getLanguage(language)) {
        const result = hljs.highlight(content, { language });
        return wrapLinesWithNumbers(result.value);
      }
    } catch {
      // Fall back to auto-detection
    }

    try {
      const result = hljs.highlightAuto(content);
      return wrapLinesWithNumbers(result.value);
    } catch {
      return wrapLinesWithNumbers(escapeHtml(content));
    }
  }

  function escapeHtml(str: string): string {
    return str
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#039;');
  }

  function wrapLinesWithNumbers(html: string): string {
    const lines = html.split('\n');
    const result: string[] = [];
    let openTags: { full: string; name: string }[] = [];

    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      const prefix = openTags.map((t) => t.full).join('');

      const tagRegex = /<(\/?)([a-zA-Z][a-zA-Z0-9]*)([^>]*)>/g;
      let match;
      while ((match = tagRegex.exec(line)) !== null) {
        const isClosing = match[1] === '/';
        const tagName = match[2];
        const attrs = match[3];
        const isSelfClosing = attrs.endsWith('/');

        if (isClosing) {
          for (let j = openTags.length - 1; j >= 0; j--) {
            if (openTags[j].name === tagName) {
              openTags.splice(j, 1);
              break;
            }
          }
        } else if (!isSelfClosing) {
          openTags.push({ full: `<${tagName}${attrs}>`, name: tagName });
        }
      }

      const suffix = openTags
        .map((t) => `</${t.name}>`)
        .reverse()
        .join('');
      const lineContent = prefix + (line || ' ') + suffix;
      result.push(`<span class="code-line" data-line="${i + 1}">${lineContent}</span>`);
    }

    return result.join('\n');
  }

  const highlightedContent = $derived(content && language ? highlightCode(content, language) : '');

  const lineCount = $derived(content?.split('\n').length ?? 0);
  const lineDigits = $derived(Math.max(2, String(lineCount).length));
  const gutterWidth = $derived(lineDigits * 0.6 + 1);

  export function getShowPreview(): boolean {
    return showPreview;
  }

  export function togglePreview(): void {
    showPreview = !showPreview;
  }

  export function getLineCount(): number {
    return lineCount;
  }
</script>

{#if isError}
  <div class="error-content">
    <div class="error-icon">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <circle cx="12" cy="12" r="10"></circle>
        <line x1="12" y1="8" x2="12" y2="12"></line>
        <line x1="12" y1="16" x2="12.01" y2="16"></line>
      </svg>
    </div>
    <div class="error-message">
      {#each content.split('\n\n') as paragraph}
        <p>{paragraph}</p>
      {/each}
    </div>
  </div>
{:else if isMarkdown && showPreview}
  <div class="markdown-preview">
    {@html renderMarkdown(content)}
  </div>
{:else}
  <pre class="code-block" bind:this={codeEl} style="--gutter-width: {gutterWidth}em"><code class="hljs"
      >{@html highlightedContent}</code
    ></pre>
{/if}

<style>
  .code-block {
    margin: 0;
    padding: 8px 16px;
    padding-left: calc(var(--gutter-width, 2.5em) + 0.5em);
    background: transparent;
    font-family: inherit;
    font-size: 12px;
    line-height: 1;
    tab-size: 4;
    white-space: pre-wrap;
    overflow-wrap: break-word;
  }

  .code-block code {
    display: block;
    color: var(--text-primary);
    counter-reset: line;
  }

  /* Line highlighting */
  :global(.code-line) {
    display: block;
    position: relative;
    padding-left: 4px;
    margin-left: -4px;
    border-left: 2px solid transparent;
  }

  :global(.code-line:hover) {
    background: var(--tint-subtle);
    border-left-color: var(--surface-border);
  }

  :global(.code-line.highlight-line) {
    background: var(--tint-active-strong);
    border-left-color: var(--accent-500);
    animation: pulse 0.5s ease-out;
  }

  @keyframes pulse {
    0% {
      background: var(--tint-selection);
    }
    100% {
      background: var(--tint-active-strong);
    }
  }

  /* Error content */
  .error-content {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 48px 32px;
    text-align: center;
    height: 100%;
  }

  .error-icon {
    width: 48px;
    height: 48px;
    margin-bottom: 20px;
    color: var(--status-yellow);
  }

  .error-icon svg {
    width: 100%;
    height: 100%;
  }

  .error-message {
    max-width: 420px;
  }

  .error-message p {
    margin: 0 0 16px 0;
    font-size: 13px;
    line-height: 1.6;
    color: var(--text-secondary);
  }

  .error-message p:first-child {
    font-size: 14px;
    font-weight: 600;
    color: var(--status-yellow);
    text-shadow: var(--emphasis);
  }

  .error-message p:last-child {
    margin-bottom: 0;
  }

  /* Markdown Preview */
  .markdown-preview {
    padding: 20px 24px;
    color: var(--text-primary);
    font-size: 14px;
    line-height: 1.6;
  }

  .markdown-preview :global(h1),
  .markdown-preview :global(h2),
  .markdown-preview :global(h3),
  .markdown-preview :global(h4) {
    color: var(--accent-400);
    margin: 1.5em 0 0.5em 0;
    font-weight: 700;
    text-shadow: var(--emphasis);
  }

  .markdown-preview :global(h1:first-child),
  .markdown-preview :global(h2:first-child),
  .markdown-preview :global(h3:first-child) {
    margin-top: 0;
  }

  .markdown-preview :global(h1) {
    font-size: 1.6em;
  }
  .markdown-preview :global(h2) {
    font-size: 1.3em;
  }
  .markdown-preview :global(h3) {
    font-size: 1.1em;
  }

  .markdown-preview :global(p) {
    margin: 0 0 1em 0;
  }

  .markdown-preview :global(a) {
    color: var(--accent-400);
    text-decoration: none;
    border-bottom: 1px solid transparent;
  }

  .markdown-preview :global(a:hover) {
    border-bottom-color: var(--accent-400);
  }

  .markdown-preview :global(code) {
    background: var(--surface-700);
    padding: 2px 6px;
    border-radius: 3px;
    font-size: 0.9em;
    color: var(--accent-400);
  }

  .markdown-preview :global(pre) {
    background: var(--surface-700);
    padding: 12px 16px;
    border-radius: 4px;
    overflow-x: auto;
    margin: 1em 0;
  }

  .markdown-preview :global(pre code) {
    background: none;
    padding: 0;
    color: var(--text-primary);
  }

  .markdown-preview :global(ul),
  .markdown-preview :global(ol) {
    margin: 0.5em 0 1em 0;
    padding-left: 1.5em;
  }

  .markdown-preview :global(li) {
    margin: 0.3em 0;
  }

  .markdown-preview :global(blockquote) {
    border-left: 3px solid var(--accent-500);
    margin: 1em 0;
    padding: 0.5em 1em;
    background: var(--tint-hover);
    color: var(--text-secondary);
  }

  .markdown-preview :global(hr) {
    border: none;
    border-top: 1px solid var(--surface-border);
    margin: 1.5em 0;
  }

  .markdown-preview :global(table) {
    border-collapse: collapse;
    width: 100%;
    margin: 1em 0;
  }

  .markdown-preview :global(th),
  .markdown-preview :global(td) {
    border: 1px solid var(--surface-border);
    padding: 8px 12px;
    text-align: left;
  }

  .markdown-preview :global(th) {
    background: var(--surface-700);
    font-weight: 600;
    color: var(--accent-400);
  }

  .markdown-preview :global(img) {
    max-width: 100%;
    height: auto;
    border-radius: 4px;
  }

  /* Mobile */
  @media (max-width: 639px) {
    .code-block {
      padding: 8px 14px;
      padding-left: calc(var(--gutter-width, 2.5em) + 0.5em);
      font-size: 12px;
      line-height: 1;
    }

    :global(.code-line) {
      padding-left: 4px;
      margin-left: -4px;
    }
  }
</style>
