<script lang="ts">
  import {
    gitCommits,
    gitDiff,
    gitHasMore,
    gitLoading,
    selectedCommitHash,
    logBranchFilter,
    selectCommit,
    clearLogBranchFilter,
    type GitDiffFile
  } from '$lib/stores/git';

  interface Props {
    onCommitFileClick: (file: GitDiffFile) => void;
    onLoadMore: () => void;
    relativeTime: (unixSeconds: number) => string;
  }

  let { onCommitFileClick, onLoadMore, relativeTime }: Props = $props();
</script>

{#if $logBranchFilter}
  <div class="log-filter-chip">
    <span class="filter-label">Branch:</span>
    <span class="filter-value">{$logBranchFilter}</span>
    <button class="filter-clear" aria-label="Clear branch filter" onclick={() => clearLogBranchFilter()}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12">
        <line x1="18" y1="6" x2="6" y2="18"></line>
        <line x1="6" y1="6" x2="18" y2="18"></line>
      </svg>
    </button>
  </div>
{/if}
{#if $gitLoading && $gitCommits.length === 0}
  <div class="loading-state">
    <div class="spinner"></div>
    <span>Loading commits...</span>
  </div>
{:else if $gitCommits.length === 0}
  <div class="empty-state">
    <span class="empty-text">No commits found</span>
  </div>
{:else}
  {#each $gitCommits as commit (commit.hash)}
    <button
      class="git-entry commit-entry"
      class:expanded={$selectedCommitHash === commit.hash}
      onclick={() => selectCommit($selectedCommitHash === commit.hash ? null : commit.hash)}
    >
      <div class="commit-row">
        <span class="commit-hash">{commit.shortHash}</span>
        <span class="commit-message">{commit.message}</span>
        <span class="commit-time">{relativeTime(commit.date)}</span>
      </div>
      {#if commit.refs.length > 0}
        <div class="commit-refs">
          {#each commit.refs as ref}
            <span class="ref-pill" class:branch={!ref.startsWith('tag:')}>{ref}</span>
          {/each}
        </div>
      {/if}
      <div class="commit-author">{commit.authorName}</div>
    </button>
    {#if $selectedCommitHash === commit.hash}
      <div class="commit-diff-summary">
        {#if $gitLoading && !$gitDiff}
          <div class="loading-state" style="padding: 12px;">
            <div class="spinner"></div>
            <span>Loading diff...</span>
          </div>
        {:else if $gitDiff}
          <div class="diff-stats-mini">
            <span class="stat-add">+{$gitDiff.stats.additions}</span>
            <span class="stat-del">-{$gitDiff.stats.deletions}</span>
            <span class="stat-files">{$gitDiff.stats.filesChanged} files</span>
          </div>
          {#each $gitDiff.files as file}
            <button class="diff-file-entry" onclick={() => onCommitFileClick(file)}>
              <span class="diff-file-status" data-status={file.status}>{file.status[0].toUpperCase()}</span>
              <span class="diff-file-path">{file.path}</span>
              <span class="diff-file-nums">
                {#if file.additions > 0}<span class="num-add">+{file.additions}</span>{/if}
                {#if file.deletions > 0}<span class="num-del">-{file.deletions}</span>{/if}
              </span>
            </button>
          {/each}
        {/if}
      </div>
    {/if}
  {/each}
  {#if $gitHasMore}
    <button class="load-more-btn" onclick={onLoadMore} disabled={$gitLoading}>
      {$gitLoading ? 'Loading...' : 'Load more'}
    </button>
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

  .commit-entry.expanded {
    background: var(--tint-active);
    border-left-color: var(--accent-500);
  }

  .commit-row {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .commit-hash {
    font-family: inherit;
    font-size: 11px;
    font-weight: 700;
    color: var(--accent-400);
    flex-shrink: 0;
  }

  .commit-message {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11px;
    color: var(--text-primary);
  }

  .commit-time {
    font-size: 9px;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .commit-refs {
    display: flex;
    gap: 4px;
    margin-top: 3px;
    flex-wrap: wrap;
  }

  .ref-pill {
    display: inline-block;
    padding: 1px 6px;
    font-size: 9px;
    font-weight: 600;
    border-radius: 3px;
    background: var(--surface-600);
    color: var(--text-muted);
  }

  .ref-pill.branch {
    background: var(--tint-focus);
    color: var(--accent-400);
    border: 1px solid var(--tint-selection);
  }

  .commit-author {
    font-size: 9px;
    color: var(--text-muted);
    margin-top: 2px;
  }

  .commit-diff-summary {
    padding: 4px 0;
    background: var(--surface-800);
    border-left: 2px solid var(--accent-600);
    margin-bottom: 2px;
  }

  .diff-stats-mini {
    display: flex;
    gap: 10px;
    padding: 4px 14px 6px;
    font-size: 10px;
    font-weight: 600;
  }

  .stat-add {
    color: var(--accent-400);
  }
  .stat-del {
    color: var(--text-muted);
  }
  .stat-files {
    color: var(--text-muted);
  }

  .diff-file-entry {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 4px 14px;
    background: none;
    border: none;
    font-family: inherit;
    font-size: 11px;
    color: var(--text-secondary);
    cursor: pointer;
    text-align: left;
    transition: all 0.1s ease;
  }

  .diff-file-entry:hover {
    background: var(--tint-active);
    color: var(--text-primary);
  }

  .diff-file-status {
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

  .diff-file-status[data-status='modified'] {
    background: var(--tint-focus);
    color: var(--accent-400);
  }
  .diff-file-status[data-status='added'] {
    background: var(--status-green-border);
    color: var(--status-green-text);
  }
  .diff-file-status[data-status='deleted'] {
    background: var(--status-red-strong);
    color: var(--status-red-text);
  }
  .diff-file-status[data-status='renamed'] {
    background: var(--status-blue-tint);
    color: var(--status-blue-text);
  }

  .diff-file-path {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .diff-file-nums {
    display: flex;
    gap: 4px;
    font-size: 10px;
    flex-shrink: 0;
  }

  .num-add {
    color: var(--accent-400);
  }
  .num-del {
    color: var(--text-muted);
  }

  .load-more-btn {
    display: block;
    width: 100%;
    padding: 10px 14px;
    background: none;
    border: none;
    border-top: 1px solid var(--surface-border);
    font-family: inherit;
    font-size: 11px;
    font-weight: 600;
    color: var(--accent-400);
    cursor: pointer;
    transition: all 0.15s ease;
    text-align: center;
  }

  .load-more-btn:hover:not(:disabled) {
    background: var(--tint-active);
  }

  .load-more-btn:disabled {
    color: var(--text-muted);
    cursor: default;
  }

  .log-filter-chip {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 14px;
    background: var(--surface-800);
    border-bottom: 1px solid var(--surface-border);
  }

  .filter-label {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .filter-value {
    font-size: 11px;
    font-weight: 600;
    color: var(--accent-400);
    padding: 1px 8px;
    background: var(--tint-active-strong);
    border-radius: 3px;
  }

  .filter-clear {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 3px;
    transition: all 0.15s ease;
    margin-left: auto;
  }

  .filter-clear:hover {
    background: var(--surface-600);
    color: var(--text-primary);
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
