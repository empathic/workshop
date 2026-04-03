/**
 * Terminal Lock Store
 *
 * Tracks which user holds the terminal lock for each instance.
 * Lock only matters when 2+ users are present — solo users always have implicit control.
 * Server is the source of truth; this store just mirrors TerminalLockUpdate messages.
 */

import { derived, writable, get } from 'svelte/store';
import { currentInstanceId } from './instances';
import { currentUser } from './auth';
import type { PresenceUser } from '$lib/types';

// =============================================================================
// Types
// =============================================================================

export interface TerminalLockState {
  holder: PresenceUser | null;
  lastActivity: string | null;
  expiresInSecs: number | null;
}

// =============================================================================
// Stores
// =============================================================================

/** Per-instance terminal lock state */
export const instanceTerminalLock = writable<Map<string, TerminalLockState>>(new Map());

/** Terminal lock state for the currently focused instance */
export const currentTerminalLock = derived([instanceTerminalLock, currentInstanceId], ([$locks, $instanceId]) => {
  if (!$instanceId) return null;
  return $locks.get($instanceId) ?? null;
});

/** Whether the current user holds the lock for the focused instance */
export const iHoldLock = derived([currentTerminalLock, currentUser], ([$lock, $user]) => {
  if (!$lock?.holder || !$user) return false;
  return $lock.holder.user_id === $user.id;
});

/** Whether another user holds the lock (and it's not me) */
export const isLockedByOther = derived([currentTerminalLock, currentUser], ([$lock, $user]) => {
  if (!$lock?.holder) return false;
  if (!$user) return true; // Not authenticated, someone else has it
  return $lock.holder.user_id !== $user.id;
});

// =============================================================================
// Actions
// =============================================================================

/** Handle a TerminalLockUpdate message from the server */
export function handleTerminalLockUpdate(
  instanceId: string,
  holder: PresenceUser | null,
  lastActivity: string | null,
  expiresInSecs: number | null
): void {
  instanceTerminalLock.update((map) => {
    if (!holder) {
      map.delete(instanceId);
    } else {
      map.set(instanceId, { holder, lastActivity, expiresInSecs });
    }
    return new Map(map);
  });
}
