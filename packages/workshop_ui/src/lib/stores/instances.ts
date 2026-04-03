import { writable, derived, get, readonly } from 'svelte/store';
import type { Readable } from 'svelte/store';
import type { Instance, CreateInstanceRequest, CreateInstanceResponse, ConversationTurn } from '$lib/types';
import { api } from '$lib/utils/api';
import { updateUrl } from '$lib/utils/url';

// =============================================================================
// Cleanup Hooks — avoids circular imports from sibling stores
// =============================================================================

type CleanupFn = (instanceId: string) => void;
const deleteHooks: CleanupFn[] = [];

/** Register a hook to run when an instance is deleted. */
export function onInstanceDelete(fn: CleanupFn): void {
  deleteHooks.push(fn);
}

type InstanceListFn = (validIds: Set<string>) => void;
const instanceListHooks: InstanceListFn[] = [];

/** Register a hook to run when the authoritative instance list arrives. */
export function onInstanceListReceived(fn: InstanceListFn): void {
  instanceListHooks.push(fn);
}

/** Fire instance-list hooks (called from ws-handlers on InstanceList). */
export function fireInstanceListReceived(validIds: Set<string>): void {
  for (const hook of instanceListHooks) {
    hook(validIds);
  }
}

// =============================================================================
// Types
// =============================================================================

/** Per-instance state that persists across instance switches */
export interface InstanceState {
  conversation: ConversationTurn[];
  pending: string[];
  isWaiting: boolean;
}

function createEmptyInstanceState(): InstanceState {
  return {
    conversation: [],
    pending: [],
    isWaiting: true
  };
}

// Re-export updateUrl for existing consumers (canonical source: $lib/utils/url)
export { updateUrl } from '$lib/utils/url';

// =============================================================================
// Core Stores
// =============================================================================

/** All known instances (metadata from backend) */
export const instances = writable<Map<string, Instance>>(new Map());

/**
 * Currently selected instance ID — driven one-way by focusedPaneInstanceId.
 *
 * The writable is private; only driveCurrentInstanceId() writes to it.
 * All external consumers get the Readable export (no .set() available).
 * To change the current instance, use setFocusedInstance() from layout.ts
 * or selectInstance() from this module.
 */
const _currentInstanceId = writable<string | null>(null);
export const currentInstanceId: Readable<string | null> = readonly(_currentInstanceId);

/**
 * Connect currentInstanceId to the focused pane's instance ID.
 * Called once from layout.ts during setupLayoutSync(). The source store
 * pushes values into _currentInstanceId (one-way: focusedPaneInstanceId → _currentInstanceId).
 */
export function driveCurrentInstanceId(source: { subscribe: (fn: (val: string | null) => void) => () => void }): void {
  source.subscribe(($id) => {
    if ($id !== get(_currentInstanceId)) {
      _currentInstanceId.set($id);
    }
  });
}

/** Per-instance state (conversation, pending input, etc.) */
export const instanceStates = writable<Map<string, InstanceState>>(new Map());

// =============================================================================
// Derived Stores
// =============================================================================

/** Get the currently selected instance */
export const currentInstance = derived([instances, currentInstanceId], ([$instances, $currentInstanceId]) => {
  if (!$currentInstanceId) return null;
  return $instances.get($currentInstanceId) ?? null;
});

/** Get instances as sorted array (newest first) */
export const instanceList = derived(instances, ($instances) => {
  // ISO 8601 strings sort lexicographically, no need to create Date objects
  return Array.from($instances.values()).sort((a, b) =>
    b.created_at > a.created_at ? 1 : b.created_at < a.created_at ? -1 : 0
  );
});

/** Check if current instance is a structured (conversation-capable) instance */
export const isClaudeInstance = derived(currentInstance, ($instance) => {
  return $instance?.kind.type === 'Structured' || false;
});

/** Get current instance's state */
export const currentInstanceState = derived([instanceStates, currentInstanceId], ([$states, $id]) => {
  if (!$id) return null;
  return $states.get($id) ?? createEmptyInstanceState();
});

/** Check if current instance has pending input */
export const hasPendingInput = derived(currentInstanceState, ($state) => {
  return ($state?.pending.length ?? 0) > 0;
});

// =============================================================================
// Instance State Helpers
// =============================================================================

/** Get or create state for an instance */
export function getInstanceState(instanceId: string): InstanceState {
  const states = get(instanceStates);
  let state = states.get(instanceId);
  if (!state) {
    state = createEmptyInstanceState();
    states.set(instanceId, state);
    instanceStates.set(states);
  }
  return state;
}

/** Update state for a specific instance */
export function updateInstanceState(instanceId: string, updater: (state: InstanceState) => InstanceState): void {
  instanceStates.update((states) => {
    const current = states.get(instanceId) ?? createEmptyInstanceState();
    states.set(instanceId, updater(current));
    return states;
  });
}

/** Set conversation for an instance */
export function setInstanceConversation(instanceId: string, turns: ConversationTurn[]): void {
  updateInstanceState(instanceId, (state) => ({
    ...state,
    conversation: turns,
    isWaiting: turns.length === 0
  }));
}

/** Append turns to an instance's conversation (with deduplication) */
export function appendInstanceTurns(instanceId: string, newTurns: ConversationTurn[]): void {
  updateInstanceState(instanceId, (state) => {
    const existingUuids = new Set(state.conversation.map((t) => t.uuid).filter(Boolean));
    const uniqueNewTurns = newTurns.filter((t) => !t.uuid || !existingUuids.has(t.uuid));

    if (uniqueNewTurns.length === 0) return state;

    return {
      ...state,
      conversation: [...state.conversation, ...uniqueNewTurns],
      isWaiting: false
    };
  });
}

/** Add pending input for an instance */
export function addPendingInput(instanceId: string, input: string): void {
  updateInstanceState(instanceId, (state) => ({
    ...state,
    pending: [...state.pending, input]
  }));
}

/** Get and clear pending input for an instance */
export function flushPendingInput(instanceId: string): string[] {
  let pending: string[] = [];
  updateInstanceState(instanceId, (state) => {
    pending = state.pending;
    return { ...state, pending: [] };
  });
  return pending;
}

/** Set waiting state for an instance */
export function setInstanceWaiting(instanceId: string, isWaiting: boolean): void {
  updateInstanceState(instanceId, (state) => ({
    ...state,
    isWaiting
  }));
}

/** Get the last conversation UUID for an instance (for sync) */
export function getLastConversationUuid(instanceId: string): string | null {
  const states = get(instanceStates);
  const state = states.get(instanceId);
  if (!state || state.conversation.length === 0) return null;

  // Get the last turn's UUID
  const lastTurn = state.conversation[state.conversation.length - 1];
  return lastTurn.uuid ?? null;
}

// =============================================================================
// API Functions
// =============================================================================

const API_BASE = '/api';

export async function fetchInstances(): Promise<void> {
  try {
    const response = await fetch(`${API_BASE}/instances`);
    if (!response.ok) throw new Error('Failed to fetch instances');

    const data: Instance[] = await response.json();
    const map = new Map<string, Instance>();
    data.forEach((inst) => map.set(inst.id, inst));
    instances.set(map);
  } catch (error) {
    console.error('Failed to fetch instances:', error);
  }
}

export async function createInstance(request: CreateInstanceRequest = {}): Promise<CreateInstanceResponse | null> {
  try {
    const response = await api(`${API_BASE}/instances`, {
      method: 'POST',
      body: JSON.stringify(request)
    });

    if (!response.ok) {
      const errorText = await response.text();
      throw new Error(errorText);
    }

    const data: CreateInstanceResponse = await response.json();
    await fetchInstances();
    return data;
  } catch (error) {
    console.error('Failed to create instance:', error);
    return null;
  }
}

export async function deleteInstance(id: string): Promise<boolean> {
  try {
    const response = await api(`${API_BASE}/instances/${id}`, {
      method: 'DELETE'
    });

    if (response.ok) {
      const wasSelected = get(_currentInstanceId) === id;
      if (wasSelected) {
        // Find the best replacement: prefer same project, fall back to any instance
        const allInstances = get(instances);
        const deleted = allInstances.get(id);
        let replacement: string | null = null;
        if (deleted) {
          // Same working_dir first (same project)
          for (const inst of allInstances.values()) {
            if (inst.id !== id && inst.working_dir === deleted.working_dir) {
              replacement = inst.id;
              break;
            }
          }
        }
        // Fall back to any other instance
        if (!replacement) {
          for (const inst of allInstances.values()) {
            if (inst.id !== id) {
              replacement = inst.id;
              break;
            }
          }
        }
        _focusInstance?.(replacement);
        updateUrl({ instance: replacement });
      }
      // Clean up instance state
      instanceStates.update((states) => {
        states.delete(id);
        return states;
      });
      // Run registered cleanup hooks (e.g. todo queue)
      for (const hook of deleteHooks) {
        hook(id);
      }
      await fetchInstances();
      return true;
    }
    return false;
  } catch (error) {
    console.error('Failed to delete instance:', error);
    return false;
  }
}

export async function setCustomName(id: string, name: string | null): Promise<boolean> {
  // Optimistic update
  instances.update((map) => {
    const inst = map.get(id);
    if (inst) {
      map.set(id, { ...inst, custom_name: name });
    }
    return new Map(map);
  });

  try {
    const response = await api(`${API_BASE}/instances/${id}/name`, {
      method: 'PATCH',
      body: JSON.stringify({ custom_name: name })
    });
    return response.ok;
  } catch (error) {
    console.error('Failed to set custom name:', error);
    return false;
  }
}

// =============================================================================
// Instance Focus — delegates to layout.ts via callback to avoid circular imports
// =============================================================================

type FocusInstanceFn = (id: string | null) => void;
let _focusInstance: FocusInstanceFn | null = null;

/**
 * Register the layout system's setFocusedInstance function.
 * Called once from layout.ts during setupLayoutSync().
 */
export function registerFocusInstance(fn: FocusInstanceFn): void {
  _focusInstance = fn;
}

type ProjectSwitchFn = (workingDir: string, instanceId?: string | null) => void;
let _switchProject: ProjectSwitchFn | null = null;

/**
 * Register the layout system's switchProject function.
 * Called once from layout.ts during setupLayoutSync().
 */
export function registerProjectSwitch(fn: ProjectSwitchFn): void {
  _switchProject = fn;
}

export function selectInstance(id: string, updateHistory = true): void {
  const inst = get(instances).get(id);
  // Cross-project guard: if the instance is in a different project, switch first.
  // switchProject loads the saved layout but does NOT focus — the user explicitly
  // selected this instance, so we always call _focusInstance afterward.
  if (inst) {
    _switchProject?.(inst.working_dir, id);
  }
  _focusInstance?.(id);

  if (updateHistory) {
    // Clear terminal param when switching instances (default to conversation view)
    updateUrl({ instance: id, terminal: null });
  }
}

export function clearSelection(updateHistory = true): void {
  _focusInstance?.(null);
  if (updateHistory) {
    updateUrl({ instance: null });
  }
}

export function initFromUrl(): string | null {
  const url = new URL(window.location.href);
  return url.searchParams.get('instance');
}

/** Read view-state params from URL (call once on init) */
export function initViewStateFromUrl(): {
  explorer?: 'files' | 'git';
  file?: string;
  line?: number;
  view?: 'diff';
  commit?: string;
} {
  const url = new URL(window.location.href);
  const result: ReturnType<typeof initViewStateFromUrl> = {};

  const explorer = url.searchParams.get('explorer');
  if (explorer === 'files' || explorer === 'git') {
    result.explorer = explorer;
  }

  const file = url.searchParams.get('file');
  if (file) {
    result.file = file;
  }

  const line = url.searchParams.get('line');
  if (line) {
    const n = parseInt(line, 10);
    if (n > 0) result.line = n;
  }

  const view = url.searchParams.get('view');
  if (view === 'diff') {
    result.view = 'diff';
  }

  const commit = url.searchParams.get('commit');
  if (commit) {
    result.commit = commit;
  }

  return result;
}
