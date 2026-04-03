<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import {
    conversationPage,
    sortedConversations,
    isLoading,
    historyError,
    fetchConversations,
    formatTimestamp,
    getDisplayTitle,
    syncConversations,
    searchQuery,
    searchPills,
    searchResults,
    isSearching,
    isSearchActive,
    searchConversations,
    clearSearch,
    addPill,
    removePill,
    buildQuery
  } from '$lib/stores/history';
  import ConversationCard from '../../routes/history/ConversationCard.svelte';
  import FullscreenHeader from './FullscreenHeader.svelte';

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();

  let isSyncing = $state(false);
  let searchInput = $state('');
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  onMount(() => {
    fetchConversations();
  });

  function handleSearchInput(e: Event) {
    const value = (e.target as HTMLInputElement).value;
    searchInput = value;

    if (debounceTimer) clearTimeout(debounceTimer);

    const trimmed = value.trim();
    const pills = $searchPills;
    const combined = buildQuery(pills, trimmed);

    if (!combined) {
      clearSearch();
      return;
    }

    searchQuery.set(combined);
    isSearchActive.set(true);

    debounceTimer = setTimeout(() => {
      searchConversations(combined);
    }, 150);
  }

  function handleSearchKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      const trimmed = searchInput.trim();
      if (trimmed) {
        if (debounceTimer) clearTimeout(debounceTimer);
        addPill(trimmed);
        searchInput = '';
      }
    }
  }

  function handleClearSearch() {
    searchInput = '';
    clearSearch();
  }

  function handleRemovePill(index: number) {
    removePill(index);
  }

  async function handleSync() {
    if (isSyncing) return;
    isSyncing = true;
    await syncConversations();
    isSyncing = false;
  }

  function goToListPage(pageNum: number) {
    fetchConversations(pageNum, $conversationPage.per_page);
  }

  function goToSearchPage(pageNum: number) {
    searchConversations(get(searchQuery), pageNum);
  }

  function pageNumbers(totalPages: number, currentPage: number): (number | '...')[] {
    if (totalPages <= 7) {
      return Array.from({ length: totalPages }, (_, i) => i + 1);
    }
    const pages: (number | '...')[] = [1];
    if (currentPage > 3) pages.push('...');
    for (let i = Math.max(2, currentPage - 1); i <= Math.min(totalPages - 1, currentPage + 1); i++) {
      pages.push(i);
    }
    if (currentPage < totalPages - 2) pages.push('...');
    pages.push(totalPages);
    return pages;
  }
</script>

{#snippet paginationControls(totalPages: number, currentPage: number, goTo: (p: number) => void)}
  {#if totalPages > 1}
    <div class="pagination">
      <button
        class="page-btn"
        disabled={currentPage <= 1}
        onclick={() => goTo(currentPage - 1)}
        aria-label="Previous page">&lt;</button
      >
      {#each pageNumbers(totalPages, currentPage) as p}
        {#if p === '...'}
          <span class="page-ellipsis">...</span>
        {:else}
          <button class="page-btn" class:active={p === currentPage} onclick={() => goTo(p as number)}>{p}</button>
        {/if}
      {/each}
      <button
        class="page-btn"
        disabled={currentPage >= totalPages}
        onclick={() => goTo(currentPage + 1)}
        aria-label="Next page">&gt;</button
      >
    </div>
  {/if}
{/snippet}

<div class="history-page">
  <FullscreenHeader title="History" {onclose}>
    {#snippet actions()}
      <button
        class="refresh-btn"
        onclick={() => fetchConversations()}
        disabled={$isLoading}
        aria-label="Refresh conversations"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class:spinning={$isLoading}>
          <path
            d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
          />
        </svg>
      </button>
    {/snippet}
  </FullscreenHeader>

  <div class="search-bar">
    <div class="search-input-wrap">
      <svg class="search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <circle cx="11" cy="11" r="8" />
        <path d="m21 21-4.35-4.35" />
      </svg>
      <input
        type="text"
        class="search-input"
        class:has-pills={$searchPills.length > 0}
        placeholder={$searchPills.length > 0 ? 'Narrow further...' : 'Search conversations...'}
        bind:value={searchInput}
        oninput={handleSearchInput}
        onkeydown={handleSearchKeydown}
      />
      {#if $isSearching}
        <div class="search-spinner"></div>
      {/if}
      {#if searchInput || $searchPills.length > 0}
        <button class="search-clear" onclick={handleClearSearch} aria-label="Clear search">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M18 6 6 18M6 6l12 12" />
          </svg>
        </button>
      {/if}
    </div>
    {#if $searchPills.length > 0}
      <div class="pill-row">
        {#each $searchPills as pill, i}
          <span class="search-pill">
            {pill}
            <button class="pill-dismiss" onclick={() => handleRemovePill(i)} aria-label="Remove '{pill}'">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
                <path d="M18 6 6 18M6 6l12 12" />
              </svg>
            </button>
          </span>
        {/each}
      </div>
    {/if}
  </div>

  {#if $historyError}
    <div class="error-banner">
      <span>{$historyError}</span>
    </div>
  {/if}

  <div class="history-content">
    {#if $isSearchActive}
      <div class="results-header">
        {$searchResults.total} conversation{$searchResults.total !== 1 ? 's' : ''} match{$searchResults.total === 1
          ? 'es'
          : ''}
        {#each $searchPills as pill, i}'{pill}'{#if i < $searchPills.length - 1 || searchInput.trim()},
          {/if}{/each}{#if searchInput.trim()}'{searchInput.trim()}'{/if}
      </div>

      {#if $searchResults.items.length === 0 && !$isSearching}
        <div class="empty-state">
          <div class="empty-icon">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
              <circle cx="11" cy="11" r="8" />
              <path d="m21 21-4.35-4.35" />
            </svg>
          </div>
          <h2>No matches found</h2>
          <p>Try a different search term</p>
        </div>
      {:else}
        <div class="conversation-list">
          {#each $searchResults.items as result (result.id)}
            <ConversationCard
              id={result.id}
              title={getDisplayTitle(result)}
              timestamp={formatTimestamp(result.created_at)}
              messageCount={result.entry_count}
              matchCount={result.match_count}
              matches={result.matches}
            />
          {/each}
        </div>
        {@render paginationControls($searchResults.total_pages, $searchResults.page, goToSearchPage)}
      {/if}
    {:else if $isLoading && $sortedConversations.length === 0}
      <div class="loading-state">
        <div class="spinner"></div>
        <span>Loading conversations...</span>
      </div>
    {:else if $sortedConversations.length === 0}
      <div class="empty-state">
        <div class="empty-icon">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
            <path
              d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10"
            />
          </svg>
        </div>
        <h2>No conversations found</h2>
        <p>Sync from Claude Code to import your conversation history</p>
        <button class="sync-btn" onclick={handleSync} disabled={isSyncing}>
          {#if isSyncing}
            <span class="spinner-small"></span>
            Syncing...
          {:else}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path
                d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
              />
            </svg>
            Sync from Claude
          {/if}
        </button>
      </div>
    {:else}
      <div class="results-header">
        Showing {($conversationPage.page - 1) * $conversationPage.per_page + 1}-{Math.min(
          $conversationPage.page * $conversationPage.per_page,
          $conversationPage.total
        )} of {$conversationPage.total}
      </div>

      <div class="conversation-list">
        {#each $sortedConversations as convo (convo.id)}
          <ConversationCard
            id={convo.id}
            title={getDisplayTitle(convo)}
            timestamp={formatTimestamp(convo.created_at)}
            messageCount={convo.entry_count}
          />
        {/each}
      </div>
      {@render paginationControls($conversationPage.total_pages, $conversationPage.page, goToListPage)}
    {/if}
  </div>
</div>

<style>
  .history-page {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    background: var(--surface-800);
  }

  .refresh-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    background: transparent;
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .refresh-btn:hover:not(:disabled) {
    border-color: var(--chrome-accent-600);
    color: var(--chrome-accent-400);
  }

  .refresh-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .refresh-btn svg {
    width: 16px;
    height: 16px;
  }

  .refresh-btn svg.spinning {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .search-bar {
    padding: 12px 20px;
    background: var(--surface-700);
    border-bottom: 1px solid var(--surface-border);
  }

  .search-input-wrap {
    position: relative;
    display: flex;
    align-items: center;
  }

  .search-icon {
    position: absolute;
    left: 10px;
    width: 16px;
    height: 16px;
    color: var(--text-muted);
    pointer-events: none;
  }

  .search-input {
    width: 100%;
    padding: 8px 36px 8px 34px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 13px;
    font-family: inherit;
    outline: none;
    transition: border-color 0.15s ease;
  }

  .search-input::placeholder {
    color: var(--text-muted);
  }

  .search-input:focus {
    border-color: var(--chrome-accent-600);
  }

  .pill-row {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 8px;
  }

  .search-pill {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px 8px 3px 10px;
    background: var(--tint-active-strong);
    border: 1px solid var(--tint-selection);
    border-radius: 12px;
    font-size: 11px;
    font-weight: 600;
    color: var(--chrome-accent-400);
    white-space: nowrap;
  }

  .pill-dismiss {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: 50%;
    color: var(--chrome-accent-400);
    cursor: pointer;
    opacity: 0.6;
    transition: opacity 0.15s ease;
  }

  .pill-dismiss:hover {
    opacity: 1;
  }

  .pill-dismiss svg {
    width: 10px;
    height: 10px;
  }

  .search-spinner {
    position: absolute;
    right: 36px;
    width: 14px;
    height: 14px;
    border: 2px solid var(--surface-border);
    border-top-color: var(--chrome-accent-500);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  .search-clear {
    position: absolute;
    right: 6px;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
    transition: color 0.15s ease;
  }

  .search-clear:hover {
    color: var(--text-primary);
  }

  .search-clear svg {
    width: 14px;
    height: 14px;
  }

  .error-banner {
    padding: 12px 20px;
    background: var(--status-red-tint);
    border-bottom: 1px solid var(--status-red-border);
    color: var(--status-red-text);
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.05em;
  }

  .history-content {
    flex: 1;
    overflow-y: auto;
    padding: 20px;
  }

  .results-header {
    margin-bottom: 16px;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .loading-state,
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-muted);
    text-align: center;
  }

  .spinner {
    width: 24px;
    height: 24px;
    border: 2px solid var(--surface-border);
    border-top-color: var(--chrome-accent-500);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    margin-bottom: 16px;
  }

  .empty-icon {
    width: 64px;
    height: 64px;
    margin-bottom: 16px;
    opacity: 0.3;
    color: var(--chrome-accent-500);
  }

  .empty-icon svg {
    width: 100%;
    height: 100%;
  }

  .empty-state h2 {
    margin: 0 0 8px;
    font-size: 14px;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-secondary);
  }

  .empty-state p {
    margin: 0 0 20px;
    font-size: 12px;
  }

  .sync-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 12px 20px;
    background: linear-gradient(180deg, var(--surface-500) 0%, var(--surface-600) 100%);
    border: 1px solid var(--chrome-accent-600);
    border-radius: 4px;
    color: var(--chrome-accent-400);
    font-size: 12px;
    font-weight: 600;
    font-family: inherit;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    cursor: pointer;
    transition: all 0.15s ease;
    box-shadow:
      0 0 10px var(--tint-active),
      inset 0 1px 0 var(--tint-active);
  }

  .sync-btn:hover:not(:disabled) {
    background: linear-gradient(180deg, var(--surface-400) 0%, var(--surface-500) 100%);
    border-color: var(--chrome-accent-500);
    color: var(--chrome-accent-300);
    box-shadow:
      0 0 20px var(--tint-focus),
      inset 0 1px 0 var(--tint-focus);
  }

  .sync-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .sync-btn svg {
    width: 16px;
    height: 16px;
  }

  .spinner-small {
    width: 14px;
    height: 14px;
    border: 2px solid var(--tint-selection);
    border-top-color: var(--chrome-accent-500);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  .conversation-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .pagination {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    margin-top: 20px;
    padding-top: 16px;
    border-top: 1px solid var(--surface-border);
  }

  .page-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 32px;
    height: 32px;
    padding: 0 8px;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
    font-family: inherit;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .page-btn:hover:not(:disabled):not(.active) {
    border-color: var(--chrome-accent-600);
    color: var(--chrome-accent-400);
  }

  .page-btn.active {
    background: var(--tint-active-strong);
    border-color: var(--chrome-accent-500);
    color: var(--chrome-accent-400);
    box-shadow: 0 0 10px var(--tint-focus);
  }

  .page-btn:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }

  .page-ellipsis {
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 32px;
    height: 32px;
    color: var(--text-muted);
    font-size: 12px;
  }

  @media (max-width: 639px) {
    .search-bar {
      padding: 10px 14px;
    }

    .history-content {
      padding: 14px;
    }
  }
</style>
