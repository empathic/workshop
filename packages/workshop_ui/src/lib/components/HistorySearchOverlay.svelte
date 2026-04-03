<script lang="ts">
  import type { SearchResultConversation } from '$lib/types';
  import { formatTimestamp } from '$lib/stores/history';

  interface Props {
    results: SearchResultConversation[];
    loading: boolean;
    selectedIndex: number;
    hasQuery: boolean;
    onselect: (index: number) => void;
  }

  let { results, loading, selectedIndex, hasQuery, onselect }: Props = $props();

  let resultsEl = $state<HTMLDivElement>();

  // Scroll selected item into view when selection changes
  $effect(() => {
    const idx = selectedIndex;
    const el = resultsEl;
    if (!el || results.length === 0) return;
    requestAnimationFrame(() => {
      const items = el.querySelectorAll('.hs-result');
      items[idx]?.scrollIntoView({ block: 'nearest' });
    });
  });

  function getRoleLabel(role: string | null): string {
    if (role === 'user') return 'You';
    if (role === 'assistant') return 'Claude';
    return 'System';
  }

  function getUserMatches(
    matches: Array<{ entry_uuid: string; role: string | null; snippet: string; timestamp: string }>
  ): typeof matches {
    return matches.filter((m) => m.role === 'user');
  }

  /** Whitelist sanitizer: only allow <mark> and </mark> tags, escape everything else. */
  function sanitizeSnippet(html: string): string {
    // Replace allowed tags with placeholders, escape the rest, restore placeholders
    return html
      .replace(/<mark>/g, '\x00MARK_OPEN\x00')
      .replace(/<\/mark>/g, '\x00MARK_CLOSE\x00')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/\x00MARK_OPEN\x00/g, '<mark>')
      .replace(/\x00MARK_CLOSE\x00/g, '</mark>');
  }
</script>

<div class="hs-overlay">
  {#if results.length > 0}
    <div class="hs-results" bind:this={resultsEl} id="hs-listbox" role="listbox" aria-label="Search results">
      {#each results as result, i (result.id)}
        {@const userMatches = getUserMatches(result.matches)}
        {@const firstMatch = userMatches[0] ?? result.matches[0]}
        {@const extraCount = userMatches.length > 1 ? userMatches.length - 1 : 0}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          id="hs-result-{i}"
          class="hs-result"
          class:selected={i === selectedIndex}
          role="option"
          tabindex="-1"
          aria-selected={i === selectedIndex}
          onclick={() => onselect(i)}
        >
          <div class="hs-result-header">
            <span class="hs-result-title"
              >{result.title ?? `Conversation ${result.id.slice(0, 8)}...`}</span
            >
            <span class="hs-result-time">{formatTimestamp(result.updated_at)}</span>
          </div>
          {#if firstMatch}
            {@const plainText = firstMatch.snippet.replace(/<[^>]*>/g, '')}
            {@const isTruncated = plainText.endsWith('...') || plainText.startsWith('...')}
            <div class="hs-result-snippet">
              <span class="hs-snippet-role">{getRoleLabel(firstMatch.role)}</span>
              <span class="hs-snippet-text">{@html sanitizeSnippet(firstMatch.snippet)}</span>
              {#if isTruncated && i === selectedIndex}
                <span class="hs-truncated">truncated</span>
              {/if}
            </div>
          {/if}
          {#if extraCount > 0}
            <div class="hs-result-more">
              +{extraCount} more match{extraCount !== 1 ? 'es' : ''}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {:else if hasQuery && !loading}
    <div class="hs-status">
      <span class="hs-empty">No matches</span>
    </div>
  {:else if !hasQuery}
    <div class="hs-status">
      <span class="hs-hint">Type to search your message history</span>
    </div>
  {/if}

  <div class="hs-footer">
    <svg class="hs-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <circle cx="11" cy="11" r="8" />
      <path d="m21 21-4.35-4.35" />
    </svg>
    <span class="hs-label">reverse-i-search</span>
    {#if loading}
      <div class="hs-spinner"></div>
    {/if}
    {#if results.length > 0}
      <span class="hs-position">{selectedIndex + 1} of {results.length}</span>
    {/if}
    <div class="hs-hints">
      <kbd>↑↓</kbd>
      <kbd>ENTER</kbd>
      <kbd>ESC</kbd>
    </div>
  </div>
</div>

<style>
  .hs-overlay {
    position: absolute;
    bottom: 100%;
    left: 0;
    right: 0;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-bottom: none;
    border-radius: 4px 4px 0 0;
    box-shadow: var(--shadow-dropdown);
    z-index: 100;
    max-height: 400px;
    display: flex;
    flex-direction: column;
    animation: hs-slide-up 0.15s ease-out;
  }

  @keyframes hs-slide-up {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .hs-results {
    display: flex;
    flex-direction: column-reverse;
    overflow-y: auto;
    flex: 1;
    min-height: 0;
  }

  .hs-result {
    padding: 10px 14px;
    cursor: pointer;
    border-bottom: 1px solid var(--surface-border);
    transition: background 0.1s ease;
  }

  .hs-result:first-child {
    border-bottom: none;
  }

  .hs-result:hover {
    background: var(--tint-subtle);
  }

  .hs-result.selected {
    background: var(--tint-active);
    border-left: 2px solid var(--chrome-accent-500);
    padding-left: 12px;
  }

  .hs-result-header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 4px;
  }

  .hs-result-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 1;
    min-width: 0;
  }

  .hs-result.selected .hs-result-title {
    color: var(--chrome-accent-400);
  }

  .hs-result-time {
    font-size: 10px;
    color: var(--text-muted);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .hs-result-snippet {
    display: flex;
    align-items: baseline;
    gap: 8px;
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-muted);
  }

  .hs-snippet-role {
    flex-shrink: 0;
    font-weight: 600;
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-secondary);
  }

  .hs-snippet-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .hs-snippet-text :global(mark) {
    background: var(--tint-selection);
    color: var(--chrome-accent-300);
    border-radius: 2px;
    padding: 0 2px;
    box-shadow: var(--emphasis);
  }

  .hs-truncated {
    flex-shrink: 0;
    font-size: 9px;
    font-style: italic;
    color: var(--text-muted);
    opacity: 0.7;
  }

  .hs-result-more {
    margin-top: 2px;
    font-size: 10px;
    font-style: italic;
    color: var(--text-muted);
  }

  .hs-status {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 14px;
    font-size: 11px;
    color: var(--text-muted);
  }

  .hs-empty {
    font-style: italic;
  }

  .hs-hint {
    font-style: italic;
    opacity: 0.7;
  }

  .hs-footer {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 14px;
    border-top: 1px solid var(--surface-border);
    flex-shrink: 0;
  }

  .hs-icon {
    width: 14px;
    height: 14px;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .hs-label {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.05em;
    color: var(--text-muted);
  }

  .hs-spinner {
    width: 12px;
    height: 12px;
    border: 2px solid var(--surface-border);
    border-top-color: var(--chrome-accent-500);
    border-radius: 50%;
    animation: hs-spin 0.8s linear infinite;
  }

  @keyframes hs-spin {
    to {
      transform: rotate(360deg);
    }
  }

  .hs-position {
    font-size: 10px;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }

  .hs-hints {
    display: flex;
    gap: 4px;
    margin-left: auto;
  }

  .hs-hints kbd {
    padding: 1px 5px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    font-size: 9px;
    font-weight: 700;
    font-family: inherit;
    letter-spacing: 0.06em;
    color: var(--text-muted);
  }
</style>
