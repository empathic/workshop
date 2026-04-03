<script lang="ts">
  import { base } from '$app/paths';
  import TopoAvatar from '$lib/components/TopoAvatar.svelte';

  interface Match {
    role: string | null;
    snippet: string;
  }

  interface Props {
    id: string;
    title: string;
    timestamp: string;
    messageCount: number;
    matchCount?: number;
    matches?: Match[];
  }

  let { id, title, timestamp, messageCount, matchCount, matches }: Props = $props();

  const isSearch = $derived(matchCount !== undefined);

  function getRoleLabel(role: string | null): string {
    if (role === 'user') return 'You';
    if (role === 'assistant') return 'Claude';
    return 'System';
  }
</script>

<a href="{base}/history/{id}" class="conversation-card" class:search-result-card={isSearch}>
  <div class="card-avatar">
    <TopoAvatar identity={id} type="agent" variant="assistant" size={32} />
  </div>
  <div class="card-content">
    {#if isSearch}
      <div class="card-title-row">
        <h3 class="card-title">{title}</h3>
        <span class="match-badge">{matchCount} match{matchCount !== 1 ? 'es' : ''}</span>
      </div>
    {:else}
      <h3 class="card-title">{title}</h3>
    {/if}
    <div class="card-meta">
      <span class="meta-item">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M8 7V3m8 4V3m-9 8h10M5 21h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z" />
        </svg>
        {timestamp}
      </span>
      <span class="meta-item">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path
            d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z"
          />
        </svg>
        {messageCount} messages
      </span>
    </div>
    {#if matches && matches.length > 0}
      <div class="snippet-list">
        {#each matches as match}
          <div class="snippet-row">
            <span class="snippet-role">{getRoleLabel(match.role)}</span>
            <span class="snippet-text">{@html match.snippet}</span>
          </div>
        {/each}
      </div>
    {/if}
  </div>
  <div class="card-arrow">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <path d="M9 5l7 7-7 7" />
    </svg>
  </div>
</a>

<style>
  .conversation-card {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 14px 16px;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    text-decoration: none;
    transition: all 0.15s ease;
  }

  .conversation-card:hover {
    background: linear-gradient(180deg, var(--tint-active) 0%, var(--tint-hover) 100%);
    border-color: var(--chrome-accent-600);
    box-shadow: 0 0 15px var(--tint-active);
  }

  .search-result-card {
    align-items: flex-start;
  }

  .search-result-card .card-avatar {
    margin-top: 2px;
  }

  .card-avatar {
    flex-shrink: 0;
  }

  .card-content {
    flex: 1;
    min-width: 0;
  }

  .card-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 6px;
  }

  .card-title {
    margin: 0 0 6px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .card-title-row .card-title {
    margin: 0;
  }

  .conversation-card:hover .card-title {
    color: var(--chrome-accent-400);
  }

  .match-badge {
    flex-shrink: 0;
    padding: 2px 8px;
    background: var(--tint-active-strong);
    border: 1px solid var(--tint-selection);
    border-radius: 10px;
    font-size: 10px;
    font-weight: 600;
    color: var(--chrome-accent-400);
    white-space: nowrap;
  }

  .card-meta {
    display: flex;
    gap: 16px;
  }

  .meta-item {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: var(--text-muted);
  }

  .meta-item svg {
    width: 12px;
    height: 12px;
  }

  .snippet-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-top: 8px;
  }

  .snippet-row {
    display: flex;
    align-items: baseline;
    gap: 8px;
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-muted);
  }

  .snippet-role {
    flex-shrink: 0;
    font-weight: 600;
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-secondary);
  }

  .snippet-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .snippet-text :global(mark) {
    background: var(--tint-focus);
    color: var(--chrome-accent-300);
    border-radius: 2px;
    padding: 0 2px;
    box-shadow: 0 0 6px var(--tint-selection);
  }

  .card-arrow {
    flex-shrink: 0;
    color: var(--text-muted);
    opacity: 0;
    transition: all 0.15s ease;
  }

  .card-arrow svg {
    width: 16px;
    height: 16px;
  }

  .conversation-card:hover .card-arrow {
    opacity: 1;
    color: var(--chrome-accent-400);
  }

  @media (max-width: 639px) {
    .conversation-card {
      padding: 12px;
      gap: 12px;
    }

    .card-title {
      font-size: 12px;
    }

    .card-meta {
      flex-direction: column;
      gap: 4px;
    }

    .meta-item {
      font-size: 10px;
    }

    .card-arrow {
      display: none;
    }

    .snippet-list {
      display: none;
    }
  }
</style>
