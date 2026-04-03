/**
 * Svelte action to add copy-to-clipboard buttons to code blocks and blockquotes
 */

export function copyable(node: HTMLElement) {
  function addCopyButtons() {
    // Find all pre (code blocks) and blockquote elements
    const copyableElements = node.querySelectorAll('pre, blockquote');

    copyableElements.forEach((element) => {
      // Skip if already has a copy button
      if (element.parentElement?.classList.contains('copyable-wrapper')) {
        return;
      }

      // Create wrapper
      const wrapper = document.createElement('div');
      wrapper.className = 'copyable-wrapper';

      // Create copy button
      const button = document.createElement('button');
      button.className = 'copy-btn';
      button.setAttribute('aria-label', 'Copy to clipboard');
      button.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path></svg>`;

      button.addEventListener('click', async (e) => {
        e.preventDefault();
        e.stopPropagation();

        // Get text content — use innerText so display:block line
        // wrappers produce proper newlines in the copied text
        // Cast to HTMLElement since innerText is not on Element
        const htmlElement = element as HTMLElement;
        let text: string;
        if (element.tagName === 'PRE') {
          const code = element.querySelector('code') as HTMLElement | null;
          text = code?.innerText ?? htmlElement.innerText ?? '';
        } else {
          text = htmlElement.innerText ?? '';
        }

        try {
          await navigator.clipboard.writeText(text.trim());

          // Show success state
          button.classList.add('copied');
          button.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"></polyline></svg>`;

          setTimeout(() => {
            button.classList.remove('copied');
            button.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path></svg>`;
          }, 2000);
        } catch (err) {
          console.error('Failed to copy:', err);
        }
      });

      // Wrap the element
      element.parentNode?.insertBefore(wrapper, element);
      wrapper.appendChild(element);
      wrapper.appendChild(button);
    });
  }

  // Initial setup
  addCopyButtons();

  // Re-run when content changes (for dynamic updates)
  const observer = new MutationObserver(() => {
    addCopyButtons();
  });

  observer.observe(node, { childList: true, subtree: true });

  return {
    destroy() {
      observer.disconnect();
    }
  };
}
