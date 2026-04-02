/**
 * Terminal Output Store
 *
 * Instance-scoped terminal I/O that replaces global window events.
 * Each instance has its own output buffer - no crosstalk between terminals.
 */

import { writable, derived, get } from 'svelte/store';
import { currentInstanceId } from './instances';

// =============================================================================
// Types
// =============================================================================

interface TerminalBuffer {
  /** Pending output chunks to be written to terminal */
  chunks: string[];
  /** Whether terminal should be cleared before next write */
  shouldClear: boolean;
  /** When true, Output messages are silently dropped while waiting
   *  for OutputHistory (full replay). Set by markAwaitingReplay(),
   *  cleared by writeTerminalHistory(). Prevents stale Output from
   *  entering xterm's async write queue before the replay arrives. */
  awaitingReplay: boolean;
}

// =============================================================================
// Internal State
// =============================================================================

/** Map of instanceId -> output buffer */
const terminalBuffers = writable<Map<string, TerminalBuffer>>(new Map());

/**
 * Maximum chunks to buffer per instance.
 *
 * MAX_BUFFER_SIZE of 10000 because:
 * 1. Average chunk is 50-100 bytes
 * 2. 10000 * 100 = 1MB max per terminal (acceptable)
 * 3. Prevents unbounded growth if output never consumed
 * 4. FIFO eviction loses old output (not ideal but better than OOM)
 */
const MAX_BUFFER_SIZE = 10000;

// =============================================================================
// Write Functions (called by WebSocket)
// =============================================================================

/**
 * Write output to a specific instance's terminal buffer.
 * Called by WebSocket handler when receiving terminal output.
 */
export function writeTerminalOutput(instanceId: string, data: string): void {
  terminalBuffers.update((buffers) => {
    let buffer = buffers.get(instanceId);
    if (!buffer) {
      buffer = { chunks: [], shouldClear: false, awaitingReplay: false };
    }

    // If a full replay (OutputHistory) is pending or we're waiting for
    // one (awaitingReplay), skip appending.  The replay will contain
    // the complete terminal state — any Output arriving now is either
    // already baked into the upcoming replay (duplicate) or a SIGWINCH
    // redraw that will harmlessly overwrite the visible screen.
    //
    // Without this guard, accumulated Output enters xterm.js's async
    // write queue before the replay, and terminal.clear() (which is
    // synchronous) can't remove queued-but-unprocessed writes.  The
    // result is duplicated scrollback content.
    if (buffer.shouldClear || buffer.awaitingReplay) {
      return buffers;
    }

    buffer.chunks.push(data);

    // Prevent unbounded growth
    if (buffer.chunks.length > MAX_BUFFER_SIZE) {
      buffer.chunks = buffer.chunks.slice(-MAX_BUFFER_SIZE);
    }

    buffers.set(instanceId, buffer);
    return new Map(buffers);
  });
}

/**
 * Write history output (on focus switch) - clears terminal first.
 * Clears the awaitingReplay flag so the subscription consumes the replay.
 */
export function writeTerminalHistory(instanceId: string, data: string): void {
  terminalBuffers.update((buffers) => {
    buffers.set(instanceId, {
      chunks: [data],
      shouldClear: true,
      awaitingReplay: false
    });
    return new Map(buffers);
  });
}

/**
 * Mark a terminal as awaiting a full replay (OutputHistory).
 *
 * Discards any accumulated Output chunks and blocks new Output from
 * entering the buffer until writeTerminalHistory() arrives.  Call this
 * BEFORE setting up the output subscription and sending TerminalVisible
 * to prevent stale Output from racing into xterm.js's async write queue.
 */
export function markAwaitingReplay(instanceId: string): void {
  terminalBuffers.update((buffers) => {
    buffers.set(instanceId, {
      chunks: [],
      shouldClear: false,
      awaitingReplay: true
    });
    return new Map(buffers);
  });
}

// =============================================================================
// Read Functions (called by Terminal component)
// =============================================================================

/**
 * Get pending output for the current instance.
 * Returns { chunks, shouldClear } and marks as consumed.
 */
export function consumeTerminalOutput(instanceId: string): TerminalBuffer {
  const buffers = get(terminalBuffers);
  const buffer = buffers.get(instanceId);

  if (!buffer || (buffer.chunks.length === 0 && !buffer.shouldClear)) {
    return { chunks: [], shouldClear: false, awaitingReplay: false };
  }

  // Clear the buffer after consumption
  terminalBuffers.update((b) => {
    b.set(instanceId, { chunks: [], shouldClear: false, awaitingReplay: false });
    return new Map(b);
  });

  return buffer;
}

/**
 * Check if there's pending output for an instance.
 */
export function hasPendingOutput(instanceId: string): boolean {
  const buffers = get(terminalBuffers);
  const buffer = buffers.get(instanceId);
  if (!buffer) return false;
  if (buffer.awaitingReplay && buffer.chunks.length === 0 && !buffer.shouldClear) return false;
  return buffer.chunks.length > 0 || buffer.shouldClear;
}

// =============================================================================
// Derived Stores
// =============================================================================

/**
 * Reactive store that signals when current instance has pending output.
 * Terminal component can subscribe to this to know when to consume.
 */
export const currentTerminalHasOutput = derived([terminalBuffers, currentInstanceId], ([$buffers, $instanceId]) => {
  if (!$instanceId) return false;
  const buffer = $buffers.get($instanceId);
  if (!buffer) return false;
  // Don't signal when only awaitingReplay is set — the subscription
  // would consume an empty buffer and reset the flag prematurely.
  // Wait for writeTerminalHistory() which provides actual data.
  if (buffer.awaitingReplay && buffer.chunks.length === 0 && !buffer.shouldClear) {
    return false;
  }
  return buffer.chunks.length > 0 || buffer.shouldClear;
});

/**
 * Create a derived store that signals when a specific instance has pending output.
 * Used by pane-bound terminals that target a specific instanceId (not currentInstanceId).
 */
export function terminalHasOutputForInstance(instanceId: string) {
  return derived(terminalBuffers, ($buffers) => {
    const buffer = $buffers.get(instanceId);
    if (!buffer) return false;
    if (buffer.awaitingReplay && buffer.chunks.length === 0 && !buffer.shouldClear) {
      return false;
    }
    return buffer.chunks.length > 0 || buffer.shouldClear;
  });
}

// =============================================================================
// Cleanup
// =============================================================================

/**
 * Clear buffer for an instance (when instance is deleted).
 */
export function deleteTerminalBuffer(instanceId: string): void {
  terminalBuffers.update((buffers) => {
    buffers.delete(instanceId);
    return new Map(buffers);
  });
}
