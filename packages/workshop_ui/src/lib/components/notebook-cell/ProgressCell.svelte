<script lang="ts">
  import type { NotebookCell } from '$lib/types';

  interface Props {
    cell: NotebookCell;
  }

  let { cell }: Props = $props();

  let progressExpanded = $state(false);

  const allItems = $derived((cell.extra?.progressItems as string[]) ?? [cell.content]);
  const count = $derived((cell.extra?.progressCount as number) ?? 1);
  const previewItems = $derived(allItems.slice(-3));

  function truncateText(text: string, maxLen: number): string {
    return text.length > maxLen ? text.slice(0, maxLen) + '...' : text;
  }
</script>

<!-- Compact progress indicator - expandable -->
<div class="progress-container" class:expanded={progressExpanded} data-cell-id={cell.id}>
  <button
    class="progress-line"
    onclick={() => (progressExpanded = !progressExpanded)}
    aria-expanded={progressExpanded}
    aria-label={progressExpanded ? 'Collapse progress details' : 'Expand progress details'}
  >
    <span class="progress-icon">{progressExpanded ? '▼' : '▶'}</span>
    <span class="progress-items">
      {#each previewItems as item, i}
        {#if i > 0}<span class="progress-sep">→</span>{/if}
        <span class="progress-item" title={item}>{truncateText(item, 50)}</span>
      {/each}
    </span>
    {#if count > 1}
      <span class="progress-count">{count} events</span>
    {/if}
  </button>
  {#if progressExpanded}
    <div class="progress-explorer">
      <div class="progress-explorer-header">
        <span class="progress-explorer-title">Progress Events</span>
        <span class="progress-explorer-count">{count} total</span>
      </div>
      <div class="progress-explorer-list">
        {#each allItems as item, i}
          <div class="progress-explorer-item">
            <span class="progress-explorer-idx">{i + 1}</span>
            <span class="progress-explorer-text">{item}</span>
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .progress-container {
    margin: 2px 0;
  }

  .progress-container.expanded {
    background: var(--surface-700);
    border-radius: 4px;
    margin: 4px 20px 4px 40px;
    border: 1px solid var(--surface-border);
  }

  /* Compact progress line - clickable */
  .progress-line {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 20px 4px 60px;
    font-size: 10px;
    color: var(--text-muted);
    opacity: 0.6;
    transition: all 0.15s ease;
    background: none;
    border: none;
    width: 100%;
    text-align: left;
    cursor: pointer;
    font-family: inherit;
  }

  .progress-container.expanded .progress-line {
    padding: 8px 12px;
    opacity: 1;
    border-bottom: 1px solid var(--surface-border);
  }

  .progress-line:hover {
    opacity: 1;
    background: var(--tint-subtle);
  }

  .progress-icon {
    font-size: 8px;
    color: var(--text-muted);
    opacity: 0.6;
    flex-shrink: 0;
    width: 10px;
  }

  .progress-items {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-wrap: wrap;
    font-family: inherit;
  }

  .progress-item {
    padding: 1px 6px;
    background: var(--surface-700);
    border-radius: 3px;
    font-size: 9px;
    letter-spacing: 0.02em;
    white-space: nowrap;
    max-width: 150px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .progress-container.expanded .progress-item {
    background: var(--surface-600);
  }

  .progress-sep {
    font-size: 8px;
    opacity: 0.4;
  }

  .progress-count {
    margin-left: auto;
    padding: 2px 8px;
    background: var(--surface-600);
    border-radius: 10px;
    font-size: 9px;
    font-weight: 600;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .progress-container.expanded .progress-count {
    background: var(--surface-500);
    color: var(--accent-400);
  }

  /* Progress explorer - expanded view */
  .progress-explorer {
    padding: 0;
  }

  .progress-explorer-header {
    display: none; /* Header info already in the toggle line */
  }

  .progress-explorer-list {
    max-height: 200px;
    overflow-y: auto;
    padding: 4px 0;
  }

  .progress-explorer-item {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 4px 12px;
    font-size: 11px;
    transition: background 0.1s ease;
  }

  .progress-explorer-item:hover {
    background: var(--tint-hover);
  }

  .progress-explorer-idx {
    flex-shrink: 0;
    width: 24px;
    text-align: right;
    font-size: 9px;
    font-weight: 600;
    color: var(--text-muted);
    opacity: 0.5;
    padding-top: 1px;
    font-family: inherit;
  }

  .progress-explorer-text {
    color: var(--text-secondary);
    line-height: 1.4;
    word-break: break-word;
  }
</style>
