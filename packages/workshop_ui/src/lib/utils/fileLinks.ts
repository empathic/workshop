/**
 * File Links Utility
 *
 * Detects file paths in text content and makes them interactive.
 * Supports patterns like:
 * - /path/to/file.ts:123 (with line number)
 * - /path/to/file.ts
 * - src/components/Foo.svelte
 * - Read tool outputs with file content
 */

import { openFileFromTool, openFilePath, navigateExplorerToFile } from '$lib/stores/files';

// Re-export pure functions from fileLinkMatch (the testable core)
export { extractFilePaths } from './fileLinkMatch.js';
import { extractFilePaths } from './fileLinkMatch.js';

/**
 * Handle clicking on a file path.
 *
 * If content is already available (from tool output), opens the viewer directly
 * and syncs the explorer to the file's parent directory.
 *
 * If no content, delegates to openFilePath which fetches from the API —
 * on success it opens the viewer + syncs explorer, on 404 it falls back
 * to opening the file explorer with a fuzzy search pre-seeded.
 */
export function handleFilePathClick(filePath: string, lineNumber?: number, content?: string): void {
  if (content) {
    openFileFromTool(filePath, content, lineNumber);
    navigateExplorerToFile(filePath);
  } else {
    openFilePath(filePath, lineNumber);
  }
}

/**
 * Svelte action to make file paths in an element clickable.
 * Wraps detected file paths in clickable spans.
 */
export function makeFilePathsClickable(node: HTMLElement, options?: { content?: string }): { destroy: () => void } {
  // Find all text nodes
  function processNode() {
    const walker = document.createTreeWalker(node, NodeFilter.SHOW_TEXT, null);
    const textNodes: Text[] = [];
    let textNode: Text | null;

    while ((textNode = walker.nextNode() as Text | null)) {
      // Skip if parent is already a link or our file-link
      const parent = textNode.parentElement;
      if (parent?.tagName === 'A' || parent?.classList.contains('file-link')) {
        continue;
      }
      textNodes.push(textNode);
    }

    // Process each text node
    for (const text of textNodes) {
      const content = text.textContent ?? '';
      const paths = extractFilePaths(content);

      if (paths.length === 0) continue;

      // Build replacement content
      const fragment = document.createDocumentFragment();
      let lastEnd = 0;

      for (const { path, line, start, end } of paths) {
        // Add text before this path
        if (start > lastEnd) {
          fragment.appendChild(document.createTextNode(content.slice(lastEnd, start)));
        }

        // Create clickable span for the path
        const span = document.createElement('span');
        span.className = 'file-link';
        span.textContent = content.slice(start, end);
        span.title = `Open ${path}${line ? ` at line ${line}` : ''}`;
        span.setAttribute('role', 'button');
        span.setAttribute('tabindex', '0');

        // Click handler
        const clickHandler = () => handleFilePathClick(path, line, options?.content);
        span.addEventListener('click', clickHandler);
        span.addEventListener('keydown', (e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault();
            clickHandler();
          }
        });

        fragment.appendChild(span);
        lastEnd = end;
      }

      // Add remaining text
      if (lastEnd < content.length) {
        fragment.appendChild(document.createTextNode(content.slice(lastEnd)));
      }

      // Replace the text node
      text.parentNode?.replaceChild(fragment, text);
    }
  }

  // Process on mount
  // Use requestAnimationFrame to ensure DOM is ready
  requestAnimationFrame(processNode);

  return {
    destroy() {
      // Cleanup is handled automatically when element is removed
    }
  };
}
