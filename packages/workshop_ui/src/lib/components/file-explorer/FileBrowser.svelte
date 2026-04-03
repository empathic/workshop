<script lang="ts">
  import {
    fileExplorerState,
    currentExplorerPath,
    rootDirectory,
    navigateToDirectory,
    navigateUp,
    loadDirectory,
    fetchFileContent,
    getFileIcon,
    formatFileSize,
    pendingSearchQuery,
    filePickerRequest,
    closeExplorer,
    type FileEntry,
    openFileFromTool
  } from '$lib/stores/files';
  import { currentInstance } from '$lib/stores/instances';
  import { apiGet } from '$lib/utils/api';
  import { gitFileStatuses, directoryVcsStatuses } from '$lib/stores/git';
  import { fuzzyMatch, fuzzyScore, highlightMatches } from '$lib/utils/fuzzy';

  // Search state
  let searchQuery = $state('');
  let selectedIndex = $state(0);
  let searchInputEl: HTMLInputElement | undefined = $state();

  // Recursive search state
  interface SearchResult {
    name: string;
    path: string;
    relativePath: string;
    isDirectory: boolean;
    score: number;
  }
  let recursiveResults = $state<SearchResult[]>([]);
  let isSearching = $state(false);
  let searchTruncated = $state(false);
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  const RECURSIVE_SEARCH_THRESHOLD = 2;

  // "/" to focus search
  function handleKeydown(e: KeyboardEvent) {
    if (e.key === '/' && document.activeElement !== searchInputEl) {
      e.preventDefault();
      searchInputEl?.focus();
    }
  }

  // Search input keyboard handler
  function handleSearchKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      if (searchQuery) {
        e.stopImmediatePropagation();
        searchQuery = '';
        selectedIndex = 0;
      }
      searchInputEl?.blur();
      return;
    }

    const listLength = isRecursiveMode ? recursiveResults.length : filteredEntries.length;

    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault();
        selectedIndex = Math.min(selectedIndex + 1, listLength - 1);
        scrollSelectedIntoView();
        break;
      case 'ArrowUp':
        e.preventDefault();
        selectedIndex = Math.max(selectedIndex - 1, 0);
        scrollSelectedIntoView();
        break;
      case 'Enter':
        e.preventDefault();
        if (isRecursiveMode) {
          if (recursiveResults[selectedIndex]) {
            handleFileClick(recursiveResults[selectedIndex]);
            if (!recursiveResults[selectedIndex].isDirectory) {
              searchQuery = '';
            }
          }
        } else {
          if (filteredEntries[selectedIndex]) {
            handleFileClick(filteredEntries[selectedIndex].entry);
            if (!filteredEntries[selectedIndex].entry.isDirectory) {
              searchQuery = '';
            }
          }
        }
        break;
      case 'Tab':
        e.preventDefault();
        if (e.shiftKey) {
          selectedIndex = Math.max(selectedIndex - 1, 0);
        } else {
          selectedIndex = Math.min(selectedIndex + 1, listLength - 1);
        }
        scrollSelectedIntoView();
        break;
    }
  }

  function scrollSelectedIntoView() {
    requestAnimationFrame(() => {
      const selectedEl = document.querySelector('.file-entry.selected');
      selectedEl?.scrollIntoView({ block: 'nearest' });
    });
  }

  // Reset selection when search changes
  $effect(() => {
    searchQuery;
    selectedIndex = 0;
  });

  // Debounced recursive search
  $effect(() => {
    const query = searchQuery.trim();
    const instance = $currentInstance;

    if (debounceTimer) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }

    if (query.length < RECURSIVE_SEARCH_THRESHOLD) {
      recursiveResults = [];
      isSearching = false;
      searchTruncated = false;
      return;
    }

    isSearching = true;
    debounceTimer = setTimeout(async () => {
      if (!instance) {
        isSearching = false;
        return;
      }

      try {
        const response = await apiGet<{
          query: string;
          results: SearchResult[];
          truncated: boolean;
        }>(`/api/instances/${instance.id}/files/search?q=${encodeURIComponent(query)}&limit=100`);

        if (searchQuery.trim() === query) {
          recursiveResults = response.results;
          searchTruncated = response.truncated;
          isSearching = false;
        }
      } catch (error) {
        console.error('Search failed:', error);
        if (searchQuery.trim() === query) {
          isSearching = false;
        }
      }
    }, 200);

    return () => {
      if (debounceTimer) {
        clearTimeout(debounceTimer);
      }
    };
  });

  // Pick up pending search queries from the store (e.g. from file link fallback)
  $effect(() => {
    const pending = $pendingSearchQuery;
    if (pending) {
      searchQuery = pending;
      pendingSearchQuery.set('');
      // Focus the search input after the DOM updates
      requestAnimationFrame(() => searchInputEl?.focus());
    }
  });

  // Breadcrumb helpers
  function getBreadcrumbs(currentPath: string, root: string): Array<{ name: string; path: string }> {
    if (!currentPath || currentPath === root) {
      return [{ name: getDirectoryName(root), path: root }];
    }

    const rootParts = root.split('/').filter(Boolean);
    const currentParts = currentPath.split('/').filter(Boolean);

    const crumbs: Array<{ name: string; path: string }> = [{ name: getDirectoryName(root), path: root }];

    let buildPath = '';
    for (let i = rootParts.length; i < currentParts.length; i++) {
      buildPath = '/' + currentParts.slice(0, i + 1).join('/');
      crumbs.push({ name: currentParts[i], path: buildPath });
    }

    return crumbs;
  }

  function getDirectoryName(path: string): string {
    const parts = path.split('/').filter(Boolean);
    return parts[parts.length - 1] || '/';
  }

  function formatFileError(error: unknown): string {
    const errorMsg = String(error);

    if (errorMsg.includes('symlink') && errorMsg.includes('outside')) {
      return `Security restriction: This symlink points to a location outside the project directory.

For security reasons, the file viewer only allows access to files within the project's working directory. Symlinks that resolve to external paths are blocked to prevent accidental exposure of sensitive files on your system.

If you need to view this file, you can:
- Access it directly through your system's file manager
- Use a terminal to view its contents`;
    }

    if (errorMsg.includes('403') || errorMsg.includes('permission') || errorMsg.includes('denied')) {
      return `Access denied: Unable to read this file.

This may be due to file permissions or security restrictions.`;
    }

    return `Failed to load file: ${errorMsg}`;
  }

  // Handle clicking on a file or search result
  async function handleFileClick(entry: FileEntry | SearchResult) {
    if (entry.isDirectory) {
      navigateToDirectory(entry.path);
      searchQuery = '';
    } else {
      const picker = $filePickerRequest;
      if (picker) {
        // Picker mode — invoke the caller's callback, then close
        picker.onSelect(entry.path);
        closeExplorer();
      } else {
        try {
          const content = await fetchFileContent(entry.path);
          openFileFromTool(entry.path, content);
        } catch (error) {
          console.error('Failed to fetch file:', error);
          openFileFromTool(entry.path, formatFileError(error));
        }
      }
    }
  }

  // Get the relative path of a file entry for git status lookup
  function getRelativePath(entry: FileEntry): string {
    const root = $rootDirectory;
    if (entry.path.startsWith(root)) {
      const rel = entry.path.slice(root.length);
      return rel.startsWith('/') ? rel.slice(1) : rel;
    }
    return entry.path;
  }

  // Derived values
  const isRecursiveMode = $derived(searchQuery.trim().length >= RECURSIVE_SEARCH_THRESHOLD);
  const currentListing = $derived($fileExplorerState.listings.get($currentExplorerPath));
  const isLoadingFiles = $derived($fileExplorerState.loading.has($currentExplorerPath));
  const breadcrumbs = $derived(getBreadcrumbs($currentExplorerPath, $rootDirectory));

  const sortedEntries = $derived(
    currentListing?.entries
      ? [...currentListing.entries].sort((a, b) => {
          if (a.isDirectory && !b.isDirectory) return -1;
          if (!a.isDirectory && b.isDirectory) return 1;
          return a.name.localeCompare(b.name);
        })
      : []
  );

  const filteredEntries = $derived.by(() => {
    if (!searchQuery.trim()) {
      return sortedEntries.map((entry) => ({ entry, indices: [] as number[], score: 0 }));
    }

    const results: Array<{ entry: FileEntry; indices: number[]; score: number }> = [];

    for (const entry of sortedEntries) {
      const indices = fuzzyMatch(searchQuery, entry.name);
      if (indices !== null) {
        const score = fuzzyScore(searchQuery, entry.name, indices);
        results.push({ entry, indices, score });
      }
    }

    return results.sort((a, b) => {
      if (a.score !== b.score) return a.score - b.score;
      if (a.entry.isDirectory && !b.entry.isDirectory) return -1;
      if (!a.entry.isDirectory && b.entry.isDirectory) return 1;
      return a.entry.name.localeCompare(b.entry.name);
    });
  });
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- Breadcrumb navigation -->
<nav class="breadcrumbs">
  {#each breadcrumbs as crumb, i}
    {#if i > 0}
      <span class="breadcrumb-sep">/</span>
    {/if}
    <button
      class="breadcrumb"
      class:current={i === breadcrumbs.length - 1}
      onclick={() => navigateToDirectory(crumb.path)}
      disabled={i === breadcrumbs.length - 1}
    >
      {crumb.name}
    </button>
  {/each}
</nav>

<!-- Toolbar with search -->
<div class="toolbar">
  <button
    class="toolbar-btn"
    onclick={navigateUp}
    disabled={$currentExplorerPath === $rootDirectory}
    title="Go up"
    aria-label="Navigate to parent directory"
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <path d="M17 11l-5-5-5 5M12 6v12" />
    </svg>
  </button>
  <div class="search-wrapper">
    <svg class="search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <circle cx="11" cy="11" r="8"></circle>
      <path d="m21 21-4.35-4.35"></path>
    </svg>
    <input
      bind:this={searchInputEl}
      bind:value={searchQuery}
      onkeydown={handleSearchKeydown}
      type="text"
      class="search-input"
      class:recursive={isRecursiveMode}
      placeholder="Search files... (/)"
      aria-label="Search files"
    />
    {#if searchQuery}
      <button
        class="search-clear"
        onclick={() => {
          searchQuery = '';
          searchInputEl?.focus();
        }}
        aria-label="Clear search"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <line x1="18" y1="6" x2="6" y2="18"></line>
          <line x1="6" y1="6" x2="18" y2="18"></line>
        </svg>
      </button>
    {/if}
  </div>
  <button
    class="toolbar-btn"
    onclick={() => loadDirectory($currentExplorerPath)}
    title="Refresh"
    aria-label="Refresh directory"
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <path
        d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
      />
    </svg>
  </button>
</div>

<!-- File list -->
<div class="file-list">
  {#if isRecursiveMode}
    <!-- Recursive search mode -->
    {#if isSearching}
      <div class="loading-state">
        <div class="spinner"></div>
        <span>Searching...</span>
      </div>
    {:else if recursiveResults.length === 0}
      <div class="empty-state">
        <span class="empty-text">No files matching "{searchQuery}"</span>
      </div>
    {:else}
      {#each recursiveResults as result, i (result.path)}
        <button
          class="file-entry"
          class:directory={result.isDirectory}
          class:selected={i === selectedIndex}
          onclick={() => {
            selectedIndex = i;
            handleFileClick(result);
          }}
          ondblclick={() => result.isDirectory && navigateToDirectory(result.path)}
          onmouseenter={() => (selectedIndex = i)}
        >
          <span class="entry-icon">{result.isDirectory ? '📁' : '📄'}</span>
          <div class="entry-info">
            <span class="entry-name">
              {#each highlightMatches(result.name, fuzzyMatch(searchQuery, result.name) ?? []) as part}
                {#if part.highlight}
                  <mark class="match">{part.text}</mark>
                {:else}
                  {part.text}
                {/if}
              {/each}
            </span>
            <span class="entry-path">{result.relativePath}</span>
          </div>
        </button>
      {/each}
    {/if}
  {:else}
    <!-- Local filter mode -->
    {#if isLoadingFiles}
      <div class="loading-state">
        <div class="spinner"></div>
        <span>Loading...</span>
      </div>
    {:else if currentListing?.error}
      <div class="error-state">
        <span class="error-icon">&#x26A0;&#xFE0F;</span>
        <span class="error-text">{currentListing.error}</span>
        <button class="retry-btn" onclick={() => loadDirectory($currentExplorerPath)}> Retry </button>
      </div>
    {:else if sortedEntries.length === 0}
      <div class="empty-state">
        <span class="empty-text">Empty directory</span>
      </div>
    {:else if filteredEntries.length === 0}
      <div class="empty-state">
        <span class="empty-text">No matches for "{searchQuery}"</span>
      </div>
    {:else}
      {#each filteredEntries as { entry, indices }, i (entry.path)}
        <button
          class="file-entry"
          class:directory={entry.isDirectory}
          class:symlink={entry.isSymlink}
          class:selected={i === selectedIndex}
          onclick={() => {
            selectedIndex = i;
            handleFileClick(entry);
          }}
          ondblclick={() => entry.isDirectory && navigateToDirectory(entry.path)}
          onmouseenter={() => (selectedIndex = i)}
          title={entry.isSymlink && entry.symlinkTarget ? `→ ${entry.symlinkTarget}` : undefined}
        >
          <span class="entry-icon">{getFileIcon(entry)}</span>
          <div class="entry-info">
            <span class="entry-name">
              {#each highlightMatches(entry.name, indices) as part}
                {#if part.highlight}
                  <mark class="match">{part.text}</mark>
                {:else}
                  {part.text}
                {/if}
              {/each}
            </span>
            {#if entry.isSymlink && entry.symlinkTarget}
              <span class="symlink-target">→ {entry.symlinkTarget}</span>
            {/if}
          </div>
          {#if entry.isDirectory}
            {@const relPath = getRelativePath(entry)}
            {@const dirCount = $directoryVcsStatuses.get(relPath)}
            {#if dirCount}
              <span class="dir-vcs-badge">{dirCount}</span>
            {/if}
          {:else}
            {@const relPath = getRelativePath(entry)}
            {@const gitSt = $gitFileStatuses.get(relPath)}
            {#if gitSt}
              <span class="git-badge" data-status={gitSt}>{gitSt}</span>
            {/if}
            {#if entry.size != null}
              <span class="entry-size">{formatFileSize(entry.size)}</span>
            {/if}
          {/if}
        </button>
      {/each}
    {/if}
  {/if}
</div>

<!-- Match count / search status -->
{#if isRecursiveMode && recursiveResults.length > 0}
  <div class="match-count" class:truncated={searchTruncated}>
    {recursiveResults.length}{searchTruncated ? '+' : ''} files found
  </div>
{:else if searchQuery && !isRecursiveMode && filteredEntries.length > 0}
  <div class="match-count">
    {filteredEntries.length} / {sortedEntries.length}
  </div>
{/if}

<style>
  /* Breadcrumbs */
  .breadcrumbs {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 8px 14px;
    background: var(--surface-800);
    border-bottom: 1px solid var(--surface-border);
    overflow-x: auto;
    flex-shrink: 0;
  }

  .breadcrumbs::-webkit-scrollbar {
    height: 4px;
  }

  .breadcrumb {
    background: none;
    border: none;
    padding: 2px 6px;
    font-size: 11px;
    font-family: inherit;
    color: var(--text-secondary);
    cursor: pointer;
    border-radius: 3px;
    transition: all 0.15s ease;
    white-space: nowrap;
  }

  .breadcrumb:hover:not(:disabled) {
    background: var(--surface-600);
    color: var(--chrome-accent-400);
  }

  .breadcrumb.current {
    color: var(--chrome-accent-400);
    font-weight: 600;
    cursor: default;
  }

  .breadcrumb-sep {
    color: var(--text-muted);
    font-size: 10px;
  }

  /* Toolbar */
  .toolbar {
    display: flex;
    gap: 6px;
    padding: 6px 14px;
    background: var(--surface-800);
    border-bottom: 1px solid var(--surface-border);
    flex-shrink: 0;
  }

  .toolbar-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.15s ease;
    flex-shrink: 0;
  }

  .toolbar-btn:hover:not(:disabled) {
    background: var(--surface-600);
    border-color: var(--chrome-accent-600);
    color: var(--chrome-accent-400);
  }

  .toolbar-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .toolbar-btn svg {
    width: 14px;
    height: 14px;
  }

  /* Search input */
  .search-wrapper {
    flex: 1;
    position: relative;
    display: flex;
    align-items: center;
  }

  .search-icon {
    position: absolute;
    left: 8px;
    width: 14px;
    height: 14px;
    color: var(--text-muted);
    pointer-events: none;
  }

  .search-input {
    width: 100%;
    height: 28px;
    padding: 0 28px;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    font-family: inherit;
    font-size: 11px;
    color: var(--text-primary);
    outline: none;
    transition: all 0.15s ease;
  }

  .search-input::placeholder {
    color: var(--text-muted);
  }

  .search-input:focus {
    border-color: var(--chrome-accent-600);
    box-shadow: 0 0 0 2px var(--tint-focus);
  }

  .search-input.recursive {
    border-color: var(--chrome-accent-600);
    background: var(--tint-hover);
  }

  .search-clear {
    position: absolute;
    right: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 3px;
    transition: all 0.15s ease;
  }

  .search-clear:hover {
    background: var(--surface-600);
    color: var(--text-primary);
  }

  .search-clear svg {
    width: 12px;
    height: 12px;
  }

  /* File list */
  .file-list {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 4px 0;
  }

  .file-entry {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 14px;
    background: none;
    border: none;
    font-family: inherit;
    font-size: 12px;
    color: var(--text-primary);
    cursor: pointer;
    text-align: left;
    transition: all 0.1s ease;
    border-left: 2px solid transparent;
  }

  .file-entry:hover {
    background: var(--tint-hover);
  }

  .file-entry.selected {
    background: var(--tint-active-strong);
    border-left-color: var(--chrome-accent-500);
  }

  .file-entry.directory {
    font-weight: 500;
  }

  .file-entry.symlink {
    opacity: 0.85;
  }

  .file-entry.symlink .entry-name {
    font-style: italic;
  }

  .entry-icon {
    font-size: 14px;
    flex-shrink: 0;
  }

  .entry-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .entry-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .entry-name :global(mark.match) {
    background: var(--tint-selection);
    color: var(--chrome-accent-300);
    border-radius: 2px;
    padding: 0 1px;
    font-weight: 600;
  }

  .entry-size {
    font-size: 10px;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .symlink-target {
    font-size: 9px;
    color: var(--text-muted);
    font-style: normal;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    opacity: 0.7;
  }

  .entry-path {
    font-size: 9px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    opacity: 0.7;
  }

  /* Git badges on file entries */
  .git-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    font-size: 8px;
    font-weight: 700;
    border-radius: 2px;
    flex-shrink: 0;
  }

  .git-badge[data-status='M'] {
    background: var(--tint-focus);
    color: var(--chrome-accent-400);
  }
  .git-badge[data-status='A'] {
    background: var(--status-green-border);
    color: var(--status-green-text);
  }
  .git-badge[data-status='D'] {
    background: var(--status-red-strong);
    color: var(--status-red-text);
  }
  .git-badge[data-status='R'] {
    background: var(--status-blue-tint);
    color: var(--status-blue-text);
  }
  .git-badge[data-status='?'] {
    background: var(--surface-600);
    color: var(--text-muted);
  }

  .dir-vcs-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    font-size: 9px;
    font-weight: 700;
    line-height: 1;
    border-radius: 8px;
    background: var(--tint-focus);
    color: var(--chrome-accent-400);
    flex-shrink: 0;
  }

  /* States */
  .loading-state,
  .error-state,
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

  .error-icon {
    font-size: 24px;
    opacity: 0.5;
  }

  .error-text,
  .empty-text {
    font-size: 11px;
    text-align: center;
  }

  .retry-btn {
    padding: 4px 12px;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--chrome-accent-400);
    font-family: inherit;
    font-size: 10px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .retry-btn:hover {
    background: var(--surface-600);
    border-color: var(--chrome-accent-600);
  }

  /* Match count indicator */
  .match-count {
    padding: 6px 14px;
    background: var(--surface-800);
    border-top: 1px solid var(--surface-border);
    font-size: 10px;
    font-weight: 600;
    color: var(--text-muted);
    text-align: right;
    flex-shrink: 0;
  }

  .match-count.truncated {
    color: var(--chrome-accent-400);
  }

  /* Scrollbar */
  .file-list::-webkit-scrollbar {
    width: 6px;
  }

  .file-list::-webkit-scrollbar-track {
    background: var(--surface-900);
  }

  .file-list::-webkit-scrollbar-thumb {
    background: var(--surface-400);
    border-radius: 3px;
  }

  .file-list::-webkit-scrollbar-thumb:hover {
    background: var(--chrome-accent-600);
  }

  /* Mobile */
  @media (max-width: 639px) {
    .breadcrumbs {
      padding: 10px 16px;
    }

    .breadcrumb {
      padding: 6px 10px;
      font-size: 12px;
      min-height: 32px;
    }

    .toolbar {
      padding: 8px 16px;
      gap: 8px;
    }

    .toolbar-btn {
      width: 40px;
      height: 40px;
    }

    .toolbar-btn svg {
      width: 18px;
      height: 18px;
    }

    .search-input {
      height: 40px;
      font-size: 14px;
      padding: 0 36px;
    }

    .search-icon {
      left: 12px;
      width: 16px;
      height: 16px;
    }

    .search-clear {
      right: 8px;
      width: 28px;
      height: 28px;
    }

    .search-clear svg {
      width: 16px;
      height: 16px;
    }

    .match-count {
      padding: 8px 16px;
      font-size: 11px;
    }

    .file-entry {
      padding: 12px 16px;
      min-height: 48px;
    }

    .entry-icon {
      font-size: 18px;
    }

    .entry-name {
      font-size: 14px;
    }

    .entry-size {
      font-size: 11px;
    }

    .loading-state,
    .error-state,
    .empty-state {
      padding: 40px 20px;
    }

    .retry-btn {
      padding: 8px 16px;
      font-size: 12px;
      min-height: 36px;
    }
  }
</style>
