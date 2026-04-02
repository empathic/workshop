<script lang="ts">
  import {
    gitBranches,
    gitLoading,
    gitInstanceBranches,
    selectedBranchName,
    branchLogPreview,
    branchDiff,
    branchDiffLoading,
    branchDiffMode,
    branchDiffBase,
    selectBranch,
    changeBranchDiffBase,
    refetchBranchDiff,
    viewBranchLog,
    type GitDiffFile
  } from '$lib/stores/git';

  interface Props {
    onBranchDiffFileClick: (file: GitDiffFile) => void;
    relativeTime: (unixSeconds: number) => string;
  }

  let { onBranchDiffFileClick, relativeTime }: Props = $props();

  let showBasePicker = $state(false);
  let showOrigin = $state(false);

  const localBranches = $derived($gitBranches.filter((b) => !b.remote));
  const originBranches = $derived($gitBranches.filter((b) => b.remote));

  function toggleBranchDiffMode() {
    branchDiffMode.update((m) => (m === 'threedot' ? 'twodot' : 'threedot'));
    refetchBranchDiff();
  }
</script>

{#if $gitLoading && $gitBranches.length === 0}
  <div class="loading-state">
    <div class="spinner"></div>
    <span>Loading branches...</span>
  </div>
{:else if localBranches.length === 0}
  <div class="empty-state">
    <span class="empty-text">No branches found</span>
  </div>
{:else}
  {#each localBranches as branch (branch.name)}
    <button
      class="git-entry branch-entry"
      class:current={branch.current}
      class:expanded={$selectedBranchName === branch.name}
      onclick={() => selectBranch(branch.name)}
    >
      <div class="branch-row">
        {#if branch.current}<span class="branch-indicator">&#x25CF;</span>{/if}
        <span class="branch-name">{branch.name}</span>
        {#if branch.ahead > 0 || branch.behind > 0}
          <span class="branch-sync">
            {#if branch.ahead > 0}<span class="sync-ahead">&#x2191;{branch.ahead}</span>{/if}
            {#if branch.behind > 0}<span class="sync-behind">&#x2193;{branch.behind}</span>{/if}
          </span>
        {/if}
        {#each $gitInstanceBranches.filter((ib) => ib.branch === branch.name) as ib}
          <span class="instance-dot" title={ib.instance_name}>&#x25CF;</span>
        {/each}
      </div>
      <div class="branch-meta">
        <span class="branch-commit-msg">{branch.lastCommitMessage}</span>
        <span class="branch-date">{relativeTime(branch.lastCommitDate)}</span>
      </div>
    </button>
    {#if $selectedBranchName === branch.name}
      <div class="branch-expansion">
        {#if $branchDiffLoading}
          <div class="loading-state" style="padding: 16px;">
            <div class="spinner"></div>
            <span>Loading...</span>
          </div>
        {:else}
          {#if $branchLogPreview.length > 0}
            <div class="branch-section-header">Recent commits</div>
            <div class="branch-log-preview">
              {#each $branchLogPreview as commit (commit.hash)}
                <div class="preview-commit">
                  <span class="commit-hash">{commit.shortHash}</span>
                  <span class="preview-commit-msg">{commit.message}</span>
                  <span class="commit-time">{relativeTime(commit.date)}</span>
                </div>
              {/each}
            </div>
          {/if}
          {#if !branch.current}
            <div class="branch-section-header">
              <span
                >Changes vs <!-- svelte-ignore a11y_click_events_have_key_events --><span
                  class="base-branch-name"
                  role="button"
                  tabindex="0"
                  onclick={(e) => {
                    e.stopPropagation();
                    showBasePicker = !showBasePicker;
                  }}
                  title="Click to change comparison branch">{$branchDiffBase}</span
                ></span
              >
              <span class="diff-header-controls">
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <span
                  class="diff-mode-toggle"
                  role="button"
                  tabindex="0"
                  onclick={(e) => {
                    e.stopPropagation();
                    toggleBranchDiffMode();
                  }}
                  title={$branchDiffMode === 'threedot' ? 'Three-dot (merge-base)' : 'Two-dot (direct)'}
                >
                  {$branchDiffMode === 'threedot' ? '...' : '..'}
                </span>
              </span>
            </div>
            {#if showBasePicker}
              <div class="base-picker">
                {#each $gitBranches.filter((b) => b.name !== branch.name) as b (b.name)}
                  <button
                    class="base-picker-option"
                    class:active={b.name === $branchDiffBase}
                    onclick={(e) => {
                      e.stopPropagation();
                      showBasePicker = false;
                      changeBranchDiffBase(b.name);
                    }}
                  >
                    {b.name}
                    {#if b.current}<span class="base-picker-tag">current</span>{/if}
                  </button>
                {/each}
              </div>
            {/if}
            {#if $branchDiff}
              <div class="diff-stats-mini">
                <span class="stat-add">+{$branchDiff.stats.additions}</span>
                <span class="stat-del">-{$branchDiff.stats.deletions}</span>
                <span class="stat-files">{$branchDiff.stats.filesChanged} files</span>
              </div>
              {#each $branchDiff.files as file}
                <button
                  class="diff-file-entry"
                  onclick={(e) => {
                    e.stopPropagation();
                    onBranchDiffFileClick(file);
                  }}
                >
                  <span class="diff-file-status" data-status={file.status}>{file.status[0].toUpperCase()}</span>
                  <span class="diff-file-path">{file.path}</span>
                  <span class="diff-file-nums">
                    {#if file.additions > 0}<span class="num-add">+{file.additions}</span>{/if}
                    {#if file.deletions > 0}<span class="num-del">-{file.deletions}</span>{/if}
                  </span>
                </button>
              {/each}
            {:else if $branchDiffBase === branch.name}
              <div class="branch-current-note">Same as comparison branch</div>
            {/if}
          {:else}
            <div class="branch-current-note">This is the current branch</div>
          {/if}
          <div class="branch-actions">
            <button
              class="branch-action-btn"
              onclick={(e) => {
                e.stopPropagation();
                viewBranchLog(branch.name);
              }}
            >
              View full log
            </button>
          </div>
        {/if}
      </div>
    {/if}
  {/each}
  {#if originBranches.length > 0}
    <button class="remotes-toggle" onclick={() => (showOrigin = !showOrigin)}>
      <span class="toggle-arrow" class:open={showOrigin}>▶</span> Remote tracking ({originBranches.length})
    </button>
    {#if showOrigin}
      {#each originBranches as branch (branch.name)}
        <div class="git-entry branch-entry remote">
          <div class="branch-row">
            <span class="branch-name">{branch.name}</span>
          </div>
          <div class="branch-meta">
            <span class="branch-commit-msg">{branch.lastCommitMessage}</span>
            <span class="branch-date">{relativeTime(branch.lastCommitDate)}</span>
          </div>
        </div>
      {/each}
    {/if}
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

  .branch-entry {
    cursor: pointer;
  }

  .branch-entry.expanded {
    background: var(--tint-active);
    border-left-color: var(--chrome-accent-500);
  }

  .branch-entry.current {
    border-left-color: var(--chrome-accent-500);
    background: var(--tint-hover);
  }

  .branch-entry.remote {
    opacity: 0.7;
    cursor: default;
  }

  .branch-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .branch-indicator {
    color: var(--chrome-accent-400);
    font-size: 8px;
    flex-shrink: 0;
  }

  .branch-name {
    font-weight: 600;
    font-size: 11px;
    color: var(--text-primary);
  }

  .branch-entry.current .branch-name {
    color: var(--chrome-accent-400);
  }

  .branch-sync {
    display: flex;
    gap: 4px;
    font-size: 9px;
    font-weight: 600;
    flex-shrink: 0;
  }

  .sync-ahead {
    color: var(--chrome-accent-400);
  }
  .sync-behind {
    color: var(--text-muted);
  }

  .instance-dot {
    font-size: 7px;
    color: var(--chrome-accent-500);
  }

  .branch-meta {
    display: flex;
    align-items: baseline;
    gap: 8px;
    margin-top: 2px;
  }

  .branch-commit-msg {
    flex: 1;
    font-size: 10px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .branch-date {
    font-size: 9px;
    color: var(--text-muted);
    opacity: 0.7;
    flex-shrink: 0;
  }

  .remotes-toggle {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 8px 14px;
    margin-top: 4px;
    background: var(--surface-800);
    border: none;
    border-top: 1px solid var(--surface-border);
    border-bottom: 1px solid var(--surface-border);
    font-family: inherit;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-muted);
    cursor: pointer;
    text-align: left;
    transition: all 0.15s ease;
  }

  .remotes-toggle:hover {
    background: var(--surface-700);
    color: var(--text-secondary);
  }

  .toggle-arrow {
    font-size: 8px;
    transition: transform 0.15s ease;
  }

  .toggle-arrow.open {
    transform: rotate(90deg);
  }

  .branch-expansion {
    padding: 4px 0;
    background: var(--surface-800);
    border-left: 2px solid var(--chrome-accent-600);
    margin-bottom: 2px;
  }

  .branch-section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 14px 4px;
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .branch-log-preview {
    padding: 0 0 4px;
  }

  .preview-commit {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 3px 14px;
    font-size: 10px;
    color: var(--text-secondary);
  }

  .commit-hash {
    font-family: inherit;
    font-size: 11px;
    font-weight: 700;
    color: var(--chrome-accent-400);
    flex-shrink: 0;
  }

  .commit-time {
    font-size: 9px;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .preview-commit-msg {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-secondary);
    font-size: 10px;
  }

  .branch-current-note {
    padding: 8px 14px;
    font-size: 10px;
    font-style: italic;
    color: var(--text-muted);
  }

  .diff-mode-toggle {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 1px 6px;
    font-size: 10px;
    font-weight: 700;
    font-family: monospace;
    letter-spacing: 0.1em;
    border-radius: 3px;
    background: var(--surface-600);
    color: var(--chrome-accent-400);
    cursor: pointer;
    transition: all 0.15s ease;
    text-transform: none;
  }

  .diff-mode-toggle:hover {
    background: var(--surface-500);
  }

  .diff-header-controls {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
  }

  .base-branch-name {
    color: var(--chrome-accent-400);
    cursor: pointer;
    border-bottom: 1px dashed var(--chrome-accent-600);
    font-weight: 700;
    text-transform: none;
    letter-spacing: 0;
    font-size: 10px;
  }

  .base-branch-name:hover {
    color: var(--chrome-accent-300);
    border-bottom-color: var(--chrome-accent-400);
  }

  .base-picker {
    max-height: 150px;
    overflow-y: auto;
    background: var(--surface-700);
    border-top: 1px solid var(--surface-border);
    border-bottom: 1px solid var(--surface-border);
  }

  .base-picker::-webkit-scrollbar {
    width: 4px;
  }

  .base-picker::-webkit-scrollbar-thumb {
    background: var(--surface-400);
    border-radius: 2px;
  }

  .base-picker-option {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 4px 14px;
    background: none;
    border: none;
    font-family: inherit;
    font-size: 10px;
    color: var(--text-secondary);
    cursor: pointer;
    text-align: left;
    transition: all 0.1s ease;
  }

  .base-picker-option:hover {
    background: var(--tint-active);
    color: var(--text-primary);
  }

  .base-picker-option.active {
    color: var(--chrome-accent-400);
    font-weight: 600;
  }

  .base-picker-tag {
    font-size: 8px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-muted);
    background: var(--surface-600);
    padding: 1px 4px;
    border-radius: 2px;
  }

  .branch-actions {
    padding: 6px 14px 4px;
    border-top: 1px solid var(--surface-border);
    margin-top: 4px;
  }

  .branch-action-btn {
    display: inline-block;
    padding: 3px 10px;
    background: none;
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    font-family: inherit;
    font-size: 10px;
    font-weight: 600;
    color: var(--chrome-accent-400);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .branch-action-btn:hover {
    background: var(--tint-active);
    border-color: var(--chrome-accent-600);
  }

  .diff-stats-mini {
    display: flex;
    gap: 10px;
    padding: 4px 14px 6px;
    font-size: 10px;
    font-weight: 600;
  }

  .stat-add {
    color: var(--chrome-accent-400);
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
    color: var(--chrome-accent-400);
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
    color: var(--chrome-accent-400);
  }
  .num-del {
    color: var(--text-muted);
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
    border-top-color: var(--chrome-accent-400);
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
