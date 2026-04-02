import { writable, get } from 'svelte/store';
import { browser } from '$app/environment';
import { onInstanceDelete } from './instances';
import { deserializeDrafts, serializeDrafts, getDraft as _getDraft, setDraft as _setDraft } from '$lib/utils/draft-map';

const STORAGE_KEY = 'workshop_drafts';

/**
 * Per-instance draft messages, persisted to localStorage.
 *
 * Drafts survive instance switches (component remount) and full page
 * reloads. They're cleared when a message is sent or an instance is
 * deleted.
 *
 * Pure logic lives in `$lib/utils/draft-map` (tested independently).
 * This module wires it to Svelte stores and localStorage.
 */

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

const drafts = writable(browser ? deserializeDrafts(localStorage.getItem(STORAGE_KEY)) : new Map<string, string>());

// Debounce localStorage writes — in-memory state updates immediately, but we
// only flush to disk after 500ms of inactivity. On unload we flush synchronously
// so nothing is lost if the tab closes mid-keystroke.
let persistTimer: ReturnType<typeof setTimeout> | undefined;

function persist(d: Map<string, string>): void {
  localStorage.setItem(STORAGE_KEY, serializeDrafts(d));
}

if (browser) {
  drafts.subscribe((d) => {
    clearTimeout(persistTimer);
    persistTimer = setTimeout(() => persist(d), 500);
  });
  window.addEventListener('beforeunload', () => {
    clearTimeout(persistTimer);
    persist(get(drafts));
  });
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/** Read the draft for an instance (empty string if none). */
export function getDraft(instanceId: string): string {
  return _getDraft(get(drafts), instanceId);
}

/** Update the draft for an instance. Pass empty string to clear. */
export function setDraft(instanceId: string, text: string): void {
  drafts.update((d) => _setDraft(d, instanceId, text));
}

/** Clear the draft for an instance. */
export function clearDraft(instanceId: string): void {
  setDraft(instanceId, '');
}

// ---------------------------------------------------------------------------
// Cleanup on instance deletion
// ---------------------------------------------------------------------------

onInstanceDelete((instanceId) => {
  clearDraft(instanceId);
});
