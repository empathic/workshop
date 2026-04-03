/**
 * History Store
 *
 * Manages conversation history data from the database with pagination and search.
 */

import { writable, derived } from 'svelte/store';
import type {
  ConversationSummary,
  ConversationWithEntries,
  PaginatedResponse,
  SearchResultConversation
} from '$lib/types';

// =============================================================================
// State
// =============================================================================

/** Paginated conversation list */
export const conversationPage = writable<PaginatedResponse<ConversationSummary>>({
  items: [],
  total: 0,
  page: 1,
  per_page: 20,
  total_pages: 1
});

/** Currently selected conversation (for detail view) */
export const selectedConversation = writable<ConversationWithEntries | null>(null);

/** Loading state */
export const isLoading = writable<boolean>(false);

/** Error state */
export const historyError = writable<string | null>(null);

/** Search query text (combined pills + live input, sent to backend) */
export const searchQuery = writable<string>('');

/** Committed search pills */
export const searchPills = writable<string[]>([]);

/** Search results (paginated) */
export const searchResults = writable<PaginatedResponse<SearchResultConversation>>({
  items: [],
  total: 0,
  page: 1,
  per_page: 20,
  total_pages: 1
});

/** Current page of search results */
export const searchPage = writable<number>(1);

/** Whether a search request is in flight */
export const isSearching = writable<boolean>(false);

/** Whether search mode is active (user has typed a query) */
export const isSearchActive = writable<boolean>(false);

// =============================================================================
// Derived State
// =============================================================================

/** Conversations from current page, sorted by updated_at descending */
export const sortedConversations = derived(conversationPage, ($page) =>
  [...$page.items].sort((a, b) => b.updated_at - a.updated_at)
);

/** Total conversation count from pagination */
export const conversationCount = derived(conversationPage, ($page) => $page.total);

// =============================================================================
// Actions
// =============================================================================

/** Fetch paginated conversation summaries */
export async function fetchConversations(page = 1, perPage = 20): Promise<void> {
  isLoading.set(true);
  historyError.set(null);

  try {
    const response = await fetch(`/api/conversations?page=${page}&per_page=${perPage}`);
    if (!response.ok) {
      throw new Error(`Failed to fetch conversations: ${response.statusText}`);
    }
    const data: PaginatedResponse<ConversationSummary> = await response.json();
    conversationPage.set(data);
  } catch (e) {
    const message = e instanceof Error ? e.message : 'Unknown error';
    historyError.set(message);
    console.error('Failed to fetch conversations:', e);
  } finally {
    isLoading.set(false);
  }
}

/** Monotonic counter to discard stale search responses */
let searchSeq = 0;

/** Search conversations via FTS. Stale responses are silently dropped. */
export async function searchConversations(query: string, page = 1, perPage = 20): Promise<void> {
  const seq = ++searchSeq;
  isSearching.set(true);
  historyError.set(null);

  try {
    const response = await fetch(
      `/api/conversations/search?q=${encodeURIComponent(query)}&page=${page}&per_page=${perPage}`
    );
    if (seq !== searchSeq) return; // a newer request was fired; discard
    if (!response.ok) {
      throw new Error(`Search failed: ${response.statusText}`);
    }
    const data: PaginatedResponse<SearchResultConversation> = await response.json();
    if (seq !== searchSeq) return;
    searchResults.set(data);
    searchPage.set(page);
  } catch (e) {
    if (seq !== searchSeq) return;
    const message = e instanceof Error ? e.message : 'Unknown error';
    historyError.set(message);
    console.error('Failed to search conversations:', e);
  } finally {
    if (seq === searchSeq) {
      isSearching.set(false);
    }
  }
}

/** Build the combined query string from pills + optional live text */
export function buildQuery(pills: string[], liveText: string): string {
  return [...pills, liveText]
    .map((s) => s.trim())
    .filter(Boolean)
    .join(' ');
}

/** Add a pill and trigger a search with the combined query */
export function addPill(text: string): void {
  const trimmed = text.trim();
  if (!trimmed) return;
  searchPills.update((p) => [...p, trimmed]);
  // Search with pills only (input is being cleared by caller)
  let pills: string[] = [];
  searchPills.subscribe((v) => (pills = v))();
  const combined = buildQuery(pills, '');
  searchQuery.set(combined);
  isSearchActive.set(true);
  searchConversations(combined);
}

/** Remove a pill by index and trigger a search (or clear if none remain) */
export function removePill(index: number): void {
  let pills: string[] = [];
  searchPills.update((p) => {
    const next = p.filter((_, i) => i !== index);
    pills = next;
    return next;
  });
  const combined = buildQuery(pills, '');
  if (!combined) {
    clearSearch();
  } else {
    searchQuery.set(combined);
    searchConversations(combined);
  }
}

/** Clear search state and return to paginated list */
export function clearSearch(): void {
  searchQuery.set('');
  searchPills.set([]);
  isSearchActive.set(false);
  searchPage.set(1);
  searchResults.set({
    items: [],
    total: 0,
    page: 1,
    per_page: 20,
    total_pages: 1
  });
}

/** Fetch a single conversation with all entries */
export async function fetchConversation(id: string): Promise<ConversationWithEntries | null> {
  isLoading.set(true);
  historyError.set(null);

  try {
    const response = await fetch(`/api/conversations/${id}`);
    if (!response.ok) {
      if (response.status === 404) {
        historyError.set('Conversation not found');
        return null;
      }
      throw new Error(`Failed to fetch conversation: ${response.statusText}`);
    }
    const data: ConversationWithEntries = await response.json();
    selectedConversation.set(data);
    return data;
  } catch (e) {
    const message = e instanceof Error ? e.message : 'Unknown error';
    historyError.set(message);
    console.error('Failed to fetch conversation:', e);
    return null;
  } finally {
    isLoading.set(false);
  }
}

/** Clear the selected conversation */
export function clearSelectedConversation(): void {
  selectedConversation.set(null);
}

/** Sync conversations from Claude Code JSONL files into the database */
export async function syncConversations(): Promise<{
  imported: number;
  skipped: number;
  failed: number;
}> {
  isLoading.set(true);
  historyError.set(null);

  try {
    const response = await fetch('/api/admin/import', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ import_all: true })
    });

    if (!response.ok) {
      throw new Error(`Failed to sync: ${response.statusText}`);
    }

    const result = await response.json();

    // Refresh the conversation list after sync
    await fetchConversations();

    return result;
  } catch (e) {
    const message = e instanceof Error ? e.message : 'Unknown error';
    historyError.set(message);
    console.error('Failed to sync conversations:', e);
    return { imported: 0, skipped: 0, failed: 0 };
  } finally {
    isLoading.set(false);
  }
}

/** Format a Unix timestamp for display */
export function formatTimestamp(timestamp: number): string {
  const date = new Date(timestamp * 1000);
  const now = new Date();
  const isToday = date.toDateString() === now.toDateString();

  const timeStr = date.toLocaleTimeString('en-US', {
    hour: '2-digit',
    minute: '2-digit',
    hour12: false
  });

  if (isToday) {
    return `Today ${timeStr}`;
  }

  const dateStr = date.toLocaleDateString('en-US', {
    month: 'short',
    day: 'numeric'
  });

  return `${dateStr} ${timeStr}`;
}

/** Get a display title for a conversation */
export function getDisplayTitle(convo: ConversationSummary | SearchResultConversation): string {
  if (convo.title) {
    return convo.title.length > 60 ? convo.title.slice(0, 60) + '...' : convo.title;
  }
  return `Conversation ${convo.id.slice(0, 8)}...`;
}
