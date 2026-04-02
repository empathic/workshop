/**
 * Claude State Store
 *
 * The authoritative state of what Claude is doing.
 * Derived from the per-instance state in the instances map.
 */

import { derived, writable } from 'svelte/store';
import type { ClaudeState } from '$lib/types';
import { currentInstanceId, instances } from './instances';
import type { Readable } from 'svelte/store';

// =============================================================================
// Core State - Derived from per-instance state with memoization
// =============================================================================

/**
 * Memoized Claude state for the focused instance.
 *
 * Uses a two-stage approach to prevent unnecessary re-renders:
 * 1. Raw derived store computes state from instances map
 * 2. Memoization layer only emits when state actually changes
 *
 * Without memoization, every StateChange for ANY instance would trigger
 * subscribers (since instances map reference changes), causing scroll
 * rubberbanding and unnecessary re-renders.
 */
const _claudeStateRaw = derived([currentInstanceId, instances], ([$instanceId, $instances]): ClaudeState => {
  if (!$instanceId) return { type: 'Idle' };
  const instance = $instances.get($instanceId);
  return instance?.claude_state ?? { type: 'Idle' };
});

// Memoized store that only updates when state meaningfully changes
const _claudeStateMemo = writable<ClaudeState>({ type: 'Idle' });
let _lastStateKey: string | null = null;

_claudeStateRaw.subscribe(($state) => {
  // Create a stable key for comparison (type + tool name if applicable)
  const key = $state.type === 'ToolExecuting' ? `${$state.type}:${$state.tool}` : $state.type;

  if (key !== _lastStateKey) {
    _lastStateKey = key;
    _claudeStateMemo.set($state);
  }
});

/** Current Claude state for the focused instance (memoized) */
export const claudeState = { subscribe: _claudeStateMemo.subscribe };

// =============================================================================
// Derived State
// =============================================================================

/** Is Claude currently active (thinking, responding, or executing)? */
export const isActive = derived(
  claudeState,
  ($state) => $state.type === 'Thinking' || $state.type === 'Responding' || $state.type === 'ToolExecuting'
);

/** Is Claude still booting (PTY spawned, not yet at prompt)? Covers both Initializing and Starting. */
export const isStarting = derived(
  claudeState,
  ($state) => $state.type === 'Initializing' || $state.type === 'Starting'
);

/** Is Claude in the thinking phase? */
export const isThinking = derived(claudeState, ($state) => $state.type === 'Thinking');

/** Is Claude executing a tool? */
export const isToolExecuting = derived(claudeState, ($state) => $state.type === 'ToolExecuting');

/** Current tool name if executing, null otherwise */
export const currentTool = derived(claudeState, ($state) => ($state.type === 'ToolExecuting' ? $state.tool : null));

// =============================================================================
// Per-Instance Derived Stores (for pane-bound components)
// =============================================================================

/** Create a claude state store for a specific instance (with memoization) */
export function claudeStateForInstance(instanceId: string): Readable<ClaudeState> {
  const raw = derived(instances, ($instances): ClaudeState => {
    const instance = $instances.get(instanceId);
    return instance?.claude_state ?? { type: 'Idle' };
  });

  const memo = writable<ClaudeState>({ type: 'Idle' });
  let lastKey: string | null = null;

  raw.subscribe(($state) => {
    const key = $state.type === 'ToolExecuting' ? `${$state.type}:${$state.tool}` : $state.type;
    if (key !== lastKey) {
      lastKey = key;
      memo.set($state);
    }
  });

  return { subscribe: memo.subscribe };
}

/** Derive isActive for a specific instance */
export function isActiveForInstance(instanceId: string): Readable<boolean> {
  const state = claudeStateForInstance(instanceId);
  return derived(state, ($s) => $s.type === 'Thinking' || $s.type === 'Responding' || $s.type === 'ToolExecuting');
}

/** Derive isStarting for a specific instance */
export function isStartingForInstance(instanceId: string): Readable<boolean> {
  const state = claudeStateForInstance(instanceId);
  return derived(state, ($s) => $s.type === 'Initializing' || $s.type === 'Starting');
}

// =============================================================================
// State Updates
// =============================================================================

// Note: setClaudeState and resetClaudeState are removed.
// State is now derived from the instances map, which is updated
// via StateChange broadcasts from the server. No manual syncing needed.
