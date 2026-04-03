/**
 * Chat Store — persistent human-to-human messaging
 *
 * Manages per-scope (global + per-instance) chat messages,
 * unread counts, panel open/close state, Zulip-style topics,
 * message selection, and compose-for-Claude state.
 */

import { writable, derived, get } from 'svelte/store';
import { currentInstanceId } from './instances';

// =============================================================================
// Types
// =============================================================================

export interface ChatMessageData {
  id: number;
  uuid: string;
  scope: string;
  user_id: string;
  display_name: string;
  content: string;
  created_at: number;
  forwarded_from?: string | null;
  topic?: string | null;
}

export interface ChatTopicSummary {
  topic: string;
  message_count: number;
  latest_at: number;
}

// =============================================================================
// State
// =============================================================================

/** All messages keyed by scope */
export const chatMessages = writable<Map<string, ChatMessageData[]>>(new Map());

/** Whether the chat panel is open */
export const isChatOpen = writable<boolean>(false);

/** Active scope in the chat panel: 'global' or an instance_id */
export const chatScope = writable<'global' | string>('global');

/** Unread counts per scope */
export const unreadCounts = writable<Map<string, number>>(new Map());

/** Whether we have more history available per scope (for pagination) */
export const hasMoreHistory = writable<Map<string, boolean>>(new Map());

/** Whether we're currently loading history for a scope */
export const loadingHistory = writable<Map<string, boolean>>(new Map());

// --- Topics ---

/** Active topic filter: null = "all topics" view, string = filtered to one topic */
export const activeTopic = writable<string | null>(null);

/** Per-scope topic lists */
export const topicList = writable<Map<string, ChatTopicSummary[]>>(new Map());

// --- Selection + Compose ---

/** Whether message selection checkboxes are showing */
export const selectionMode = writable<boolean>(false);

/** Selected message UUIDs */
export const selectedMessageIds = writable<Set<string>>(new Set());

/** Whether the compose-for-Claude overlay is open */
export const composeOpen = writable<boolean>(false);

/** Editable compose text */
export const composeContent = writable<string>('');

/** Target instance for compose send */
export const composeTargetInstance = writable<string | null>(null);

// =============================================================================
// Derived
// =============================================================================

/** Messages for the currently active scope, filtered by active topic */
export const currentChatMessages = derived(
  [chatMessages, chatScope, activeTopic],
  ([$chatMessages, $chatScope, $activeTopic]) => {
    const all = $chatMessages.get($chatScope) ?? [];
    if ($activeTopic === null) return all;
    return all.filter((m) => m.topic === $activeTopic);
  }
);

/** Messages for the current scope grouped by topic (for "all topics" view) */
export const groupedByTopic = derived([chatMessages, chatScope], ([$chatMessages, $chatScope]) => {
  const all = $chatMessages.get($chatScope) ?? [];
  const groups = new Map<string, ChatMessageData[]>();
  for (const msg of all) {
    const key = msg.topic ?? '(General)';
    const group = groups.get(key) ?? [];
    group.push(msg);
    groups.set(key, group);
  }
  return groups;
});

/** Full ChatMessageData objects for selected IDs, ordered by created_at */
export const selectedMessages = derived(
  [chatMessages, chatScope, selectedMessageIds],
  ([$chatMessages, $chatScope, $selectedIds]) => {
    if ($selectedIds.size === 0) return [];
    const all = $chatMessages.get($chatScope) ?? [];
    return all.filter((m) => $selectedIds.has(m.uuid)).sort((a, b) => a.created_at - b.created_at);
  }
);

/** Formatted compose draft from selected messages */
export const composeDraft = derived(selectedMessages, ($selectedMessages) => {
  if ($selectedMessages.length === 0) return '';
  const participants = [...new Set($selectedMessages.map((m) => m.display_name))];
  const lines = $selectedMessages.map((m) => `${m.display_name}: ${m.content}`);
  return `[Context from chat discussion between ${participants.join(', ')}]\n\n${lines.join('\n')}\n\nBased on the above discussion, please:\n`;
});

/** Total unread across all scopes */
export const totalUnread = derived(unreadCounts, ($unreadCounts) => {
  let total = 0;
  for (const count of $unreadCounts.values()) {
    total += count;
  }
  return total;
});

/** Whether the current scope has more history to load */
export const currentHasMore = derived(
  [hasMoreHistory, chatScope],
  ([$hasMore, $scope]) => $hasMore.get($scope) ?? true
);

/** Whether the current scope is loading history */
export const currentLoadingHistory = derived(
  [loadingHistory, chatScope],
  ([$loading, $scope]) => $loading.get($scope) ?? false
);

/** Topics for the current scope */
export const currentTopics = derived(
  [topicList, chatScope],
  ([$topicList, $chatScope]) => $topicList.get($chatScope) ?? []
);

// =============================================================================
// Actions
// =============================================================================

/** Handle an incoming ChatMessage from WebSocket */
export function handleChatMessage(msg: ChatMessageData): void {
  chatMessages.update((map) => {
    const existing = map.get(msg.scope) ?? [];
    // Dedup by uuid
    if (existing.some((m) => m.uuid === msg.uuid)) return map;
    map.set(msg.scope, [...existing, msg]);
    return new Map(map);
  });

  // Bump unread if panel is closed or on a different scope
  const open = get(isChatOpen);
  const activeScope = get(chatScope);
  if (!open || activeScope !== msg.scope) {
    unreadCounts.update((map) => {
      map.set(msg.scope, (map.get(msg.scope) ?? 0) + 1);
      return new Map(map);
    });
  }
}

/** Handle a ChatHistoryResponse from WebSocket */
export function handleChatHistory(scope: string, messages: ChatMessageData[], hasMore: boolean): void {
  chatMessages.update((map) => {
    const existing = map.get(scope) ?? [];
    // Prepend older messages, dedup by uuid
    const existingUuids = new Set(existing.map((m) => m.uuid));
    const newMsgs = messages.filter((m) => !existingUuids.has(m.uuid));
    map.set(scope, [...newMsgs, ...existing]);
    return new Map(map);
  });

  hasMoreHistory.update((map) => {
    map.set(scope, hasMore);
    return new Map(map);
  });

  loadingHistory.update((map) => {
    map.set(scope, false);
    return new Map(map);
  });
}

/** Handle a ChatTopicsResponse from WebSocket */
export function handleChatTopics(scope: string, topics: ChatTopicSummary[]): void {
  topicList.update((map) => {
    map.set(scope, topics);
    return new Map(map);
  });
}

/** Mark a scope as loading history */
export function setLoadingHistory(scope: string): void {
  loadingHistory.update((map) => {
    map.set(scope, true);
    return new Map(map);
  });
}

// =============================================================================
// Topic actions
// =============================================================================

export function setActiveTopic(topic: string | null): void {
  activeTopic.set(topic);
}

// =============================================================================
// Selection actions
// =============================================================================

export function toggleSelectionMode(): void {
  const current = get(selectionMode);
  if (current) {
    // Exiting: clear selection
    selectionMode.set(false);
    selectedMessageIds.set(new Set());
  } else {
    selectionMode.set(true);
  }
}

export function exitSelectionMode(): void {
  selectionMode.set(false);
  selectedMessageIds.set(new Set());
}

export function toggleMessageSelection(uuid: string): void {
  selectedMessageIds.update((set) => {
    const next = new Set(set);
    if (next.has(uuid)) {
      next.delete(uuid);
    } else {
      next.add(uuid);
    }
    return next;
  });
}

export function selectAllInTopic(topic: string): void {
  const scope = get(chatScope);
  const msgs = get(chatMessages).get(scope) ?? [];
  const topicMsgs = msgs.filter((m) => (m.topic ?? '(General)') === topic);
  selectedMessageIds.update((set) => {
    const next = new Set(set);
    for (const m of topicMsgs) {
      next.add(m.uuid);
    }
    return next;
  });
}

// =============================================================================
// Compose actions
// =============================================================================

export function openCompose(): void {
  const draft = get(composeDraft);
  composeContent.set(draft);
  composeTargetInstance.set(get(currentInstanceId));
  composeOpen.set(true);
}

export function closeCompose(): void {
  composeOpen.set(false);
}

// =============================================================================
// Panel actions
// =============================================================================

export function openChat(): void {
  isChatOpen.set(true);
  clearUnreadForCurrentScope();
}

export function closeChat(): void {
  isChatOpen.set(false);
}

export function toggleChat(): void {
  const open = get(isChatOpen);
  if (open) {
    closeChat();
  } else {
    openChat();
  }
}

export function switchScope(scope: string): void {
  chatScope.set(scope);
  activeTopic.set(null); // Reset topic filter on scope change
  clearUnread(scope);
}

function clearUnreadForCurrentScope(): void {
  const scope = get(chatScope);
  clearUnread(scope);
}

function clearUnread(scope: string): void {
  unreadCounts.update((map) => {
    map.delete(scope);
    return new Map(map);
  });
}

/** Get the oldest message ID for a scope (for pagination) */
export function getOldestMessageId(scope: string): number | undefined {
  const msgs = get(chatMessages).get(scope);
  if (!msgs || msgs.length === 0) return undefined;
  return msgs[0].id;
}
