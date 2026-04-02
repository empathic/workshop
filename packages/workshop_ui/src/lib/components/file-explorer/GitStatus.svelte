<script lang="ts">
  import { gitStatus, gitLoading, statusCounts, type GitFileStatus } from '$lib/stores/git';

  interface Props {
    onStatusFileClick: (file: GitFileStatus) => void;
  }

  let { onStatusFileClick }: Props = $props();
</script>

{#if $gitLoading && !$gitStatus}
  <div class="loading-state">
    <div class="spinner"></div>
    <span>Loading status...</span>
  </div>
{:else if !$gitStatus || $statusCounts.total === 0}
  <div class="empty-state">
    <span class="empty-text">Working tree clean</span>
  </div>
{:else}
  <div class="status-summary">
    {#if $statusCounts.staged > 0}<span class="status-count staged">{$statusCounts.staged} staged</span>{/if}
    {#if $statusCounts.modified > 0}<span class="status-count modified">{$statusCounts.modified} modified</span>{/if}
    {#if $statusCounts.untracked > 0}<span class="status-count untracked">{$statusCounts.untracked} untracked</span
      >{/if}
  </div>
  {#if $gitStatus.staged.length > 0}
    <div class="status-section-header">Staged</div>
    {#each $gitStatus.staged as file}
      <button class="git-entry status-file-entry" onclick={() => onStatusFileClick(file)}>
        <span class="git-status-indicator" data-status="staged">{file.status[0].toUpperCase()}</span>
        <span class="status-file-path">{file.path}</span>
      </button>
    {/each}
  {/if}
  {#if $gitStatus.unstaged.length > 0}
    <div class="status-section-header">Modified</div>
    {#each $gitStatus.unstaged as file}
      <button class="git-entry status-file-entry" onclick={() => onStatusFileClick(file)}>
        <span class="git-status-indicator" data-status="modified">{file.status[0].toUpperCase()}</span>
        <span class="status-file-path">{file.path}</span>
      </button>
    {/each}
  {/if}
  {#if $gitStatus.untracked.length > 0}
    <div class="status-section-header">Untracked</div>
    {#each $gitStatus.untracked as file}
      <button class="git-entry status-file-entry" onclick={() => onStatusFileClick(file)}>
        <span class="git-status-indicator" data-status="untracked">?</span>
        <span class="status-file-path">{file.path}</span>
      </button>
    {/each}
  {/if}
{/if}

<style>
  .git-entry {
    display: block;
    width: 100%;
    padding: 8px 14px;
    background: none;
    border: none;
    border-left: 2px solid transparent;
    font-family: inherit;
    font-size: 12px;
    color: var(--text-primary);
    text-align: left;
    cursor: pointer;
    transition: all 0.1s ease;
  }

  .git-entry:hover {
    background: var(--tint-hover);
  }

  .status-summary {
    display: flex;
    gap: 8px;
    padding: 8px 14px;
    background: var(--surface-800);
    border-bottom: 1px solid var(--surface-border);
    flex-wrap: wrap;
  }

  .status-count {
    font-size: 10px;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: 3px;
  }

  .status-count.staged {
    background: var(--status-green-tint);
    color: var(--status-green-text);
  }

  .status-count.modified {
    background: var(--tint-active-strong);
    color: var(--accent-400);
  }

  .status-count.untracked {
    background: var(--surface-700);
    color: var(--text-muted);
  }

  .status-section-header {
    padding: 6px 14px 4px;
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-muted);
    background: var(--surface-800);
    border-bottom: 1px solid var(--surface-border);
  }

  .status-file-entry {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .git-status-indicator {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    font-size: 9px;
    font-weight: 700;
    border-radius: 2px;
    flex-shrink: 0;
  }

  .git-status-indicator[data-status='staged'] {
    background: var(--status-green-border);
    color: var(--status-green-text);
  }

  .git-status-indicator[data-status='modified'] {
    background: var(--tint-focus);
    color: var(--accent-400);
  }

  .git-status-indicator[data-status='untracked'] {
    background: var(--surface-600);
    color: var(--text-muted);
  }

  .status-file-path {
    flex: 1;
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .loading-state,
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 32px 16px;
    gap: 12px;
    color: var(--text-muted);
  }

  .spinner {
    width: 20px;
    height: 20px;
    border: 2px solid var(--surface-border);
    border-top-color: var(--accent-400);
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .empty-text {
    font-size: 11px;
    text-align: center;
  }
</style>
