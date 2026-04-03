/**
 * Inbox Store
 *
 * Server-side inbox model: one item per instance, surfacing state transitions
 * that need attention (completed turns, waiting for input, errors).
 *
 * Items arrive via WebSocket (InboxUpdate, InboxList) and are dismissed via
 * HTTP POST. The server is the single source of truth — clients never create
 * inbox items locally.
 */

import { writable, derived, get } from 'svelte/store';
import type { Instance } from '$lib/types';
import { currentInstanceId, instances } from './instances';
import { userSettings } from './settings';
import { addToast } from './toasts';
import { api } from '$lib/utils/api';

// =============================================================================
// Types (match Rust wire format — snake_case field names)
// =============================================================================

export interface InboxItem {
  instance_id: string;
  event_type: 'completed_turn' | 'needs_input' | 'error';
  turn_count: number;
  created_at: number;
  updated_at: number;
  metadata_json?: string | null;
}

export type AttentionLevel = 'critical' | 'warning' | 'active' | 'idle' | 'booting';

// =============================================================================
// Stores
// =============================================================================

/** All active inbox items, keyed by instance_id (one per instance) */
export const inboxItems = writable<Map<string, InboxItem>>(new Map());

/** Total count of inbox items */
export const inboxCount = derived(inboxItems, ($items) => $items.size);

/** Sorted inbox items: needs_input first, then error, then completed_turn; oldest first within tier */
export const inboxSorted = derived(inboxItems, ($items) => {
  const priorityOrder: Record<string, number> = {
    needs_input: 0,
    error: 1,
    completed_turn: 2
  };

  return Array.from($items.values()).sort((a, b) => {
    const pa = priorityOrder[a.event_type] ?? 3;
    const pb = priorityOrder[b.event_type] ?? 3;
    if (pa !== pb) return pa - pb;
    return a.updated_at - b.updated_at;
  });
});

// =============================================================================
// WebSocket Handlers (called from ws-handlers.ts)
// =============================================================================

/** Handle an InboxUpdate message — upsert or delete a single item */
export function handleInboxUpdate(instanceId: string, item: InboxItem | null): void {
  inboxItems.update((map) => {
    if (item) {
      map.set(instanceId, item);
    } else {
      map.delete(instanceId);
    }
    return new Map(map);
  });

  // Browser notification for actionable inbox events
  if (item?.event_type === 'needs_input' || item?.event_type === 'completed_turn') {
    const focusedId = get(currentInstanceId);
    const isHidden = typeof document !== 'undefined' && document.visibilityState === 'hidden';
    // Notify if the instance isn't focused, or the tab is hidden (user in another app)
    if (instanceId !== focusedId || isHidden) {
      notifyInboxEvent(instanceId, item);
    }
  }
}

/** Handle an InboxList message — replace the entire map (initial load on connect) */
export function handleInboxList(items: InboxItem[]): void {
  const map = new Map<string, InboxItem>();
  for (const item of items) {
    map.set(item.instance_id, item);
  }
  inboxItems.set(map);
}

// =============================================================================
// API Actions
// =============================================================================

/** Dismiss an inbox item via HTTP POST */
export async function dismissInboxItem(instanceId: string): Promise<boolean> {
  try {
    const response = await api(`/api/inbox/${instanceId}/dismiss`, {
      method: 'POST'
    });
    if (response.ok) {
      // Optimistic update — broadcast will also arrive via WS
      inboxItems.update((map) => {
        map.delete(instanceId);
        return new Map(map);
      });
      return true;
    }
    return false;
  } catch (error) {
    console.error('[Inbox] Failed to dismiss:', error);
    return false;
  }
}

// =============================================================================
// Pure Utilities
// =============================================================================

/** Compute attention level from instance state + inbox item */
export function getAttentionLevel(instance: Instance, inboxItem?: InboxItem): AttentionLevel {
  if (inboxItem) {
    if (inboxItem.event_type === 'needs_input' || inboxItem.event_type === 'error') {
      return 'critical';
    }
    if (inboxItem.event_type === 'completed_turn') {
      return 'warning';
    }
  }

  const state = instance.claude_state;
  if (state) {
    if (state.type === 'Thinking' || state.type === 'Responding' || state.type === 'ToolExecuting') {
      return 'active';
    }
    if (state.type === 'Initializing' || state.type === 'Starting') {
      return 'booting';
    }
  }

  return 'idle';
}

/** Format elapsed duration from a unix timestamp (seconds) to a human string */
export function formatDuration(enteredAtSecs: number): string {
  const elapsed = Math.floor(Date.now() / 1000) - enteredAtSecs;
  if (elapsed < 5) return 'just now';
  if (elapsed < 60) return `${elapsed}s`;
  if (elapsed < 3600) return `${Math.floor(elapsed / 60)}m`;
  if (elapsed < 86400) return `${Math.floor(elapsed / 3600)}h`;
  return `${Math.floor(elapsed / 86400)}d`;
}

// =============================================================================
// Browser Notifications (moved from Sidebar.svelte)
// =============================================================================

/**
 * Request notification permission — must be called from a user gesture (click)
 * for Chrome to show the permission dialog. Returns the resulting permission.
 */
export async function requestNotificationPermission(): Promise<NotificationPermission | null> {
  if (!('Notification' in window)) return null;
  if (Notification.permission !== 'default') return Notification.permission;
  return Notification.requestPermission();
}

let permissionNudgeShown = false;

function notifyInboxEvent(instanceId: string, item: InboxItem): void {
  if (!('Notification' in window)) return;

  // If permission was never requested, nudge the user once
  if (Notification.permission === 'default' && !permissionNudgeShown) {
    permissionNudgeShown = true;
    addToast('Enable desktop notifications in Settings', 'info', 5000);
    return;
  }

  if (Notification.permission !== 'granted') return;

  // Respect user setting
  if (!get(userSettings).showNotifications) return;

  // Look up instance display name from the live instances store
  const inst = get(instances).get(instanceId);
  const name = inst?.custom_name ?? inst?.name;
  const displayName = name ?? instanceId;

  let title: string;
  let body: string;

  if (item.event_type === 'needs_input') {
    title = `${displayName} needs input`;
    body = 'Waiting for input';
    if (item.metadata_json) {
      try {
        const metadata = JSON.parse(item.metadata_json);
        if (metadata?.prompt) body = metadata.prompt;
      } catch {
        /* ignore parse errors */
      }
    }
  } else {
    // completed_turn
    title = `${displayName} finished`;
    body = item.turn_count > 1 ? `Completed ${item.turn_count} turns` : 'Turn complete';
  }

  new Notification(title, {
    body,
    icon: '/favicon.png',
    tag: `inbox-${instanceId}`,
    silent: false
  });
}
