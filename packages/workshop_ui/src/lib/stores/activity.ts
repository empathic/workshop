/**
 * Activity Visualization Store
 *
 * Self-managing activity tracking that automatically starts/stops
 * based on subscriber count. No manual lifecycle management needed.
 */

import { derived, writable, get } from 'svelte/store';
import { claudeState } from './claude';
import { currentInstanceId } from './instances';
import { randomVerb } from '$lib/activityVerbs';

// =============================================================================
// Per-Instance Activity Verb
// =============================================================================

/** Cached verbs per instance+stateType, so each instance gets a stable verb */
const verbCache = new Map<string, string>();

/**
 * Get or create a stable verb for a specific instance and state type.
 * Used by Sidebar (for all instances) and internally by currentVerb (for focused instance).
 */
export function getInstanceVerb(instanceId: string, stateType: string): string {
  const key = `${instanceId}-${stateType}`;
  if (!verbCache.has(key)) {
    verbCache.set(key, randomVerb());
  }
  return verbCache.get(key)!;
}

/** Clear cached verbs for an instance (call when a session completes) */
export function clearInstanceVerbs(instanceId: string): void {
  verbCache.delete(`${instanceId}-Thinking`);
  verbCache.delete(`${instanceId}-Responding`);
  verbCache.delete(`${instanceId}-ToolExecuting`);
}

/**
 * Current display verb for the focused instance.
 * Derived from current instance ID and Claude state.
 * Cached per-instance so switching between instances shows stable verbs.
 */
export const currentVerb = derived([currentInstanceId, claudeState], ([$instanceId, $state]) => {
  if (!$instanceId) return 'Thinking';
  if ($state.type === 'Initializing') return 'Init';
  if ($state.type === 'Starting') return 'Boot';
  if ($state.type === 'Thinking' || $state.type === 'Responding' || $state.type === 'ToolExecuting') {
    return getInstanceVerb($instanceId, $state.type);
  }
  return 'Thinking';
});

// Auto-clear verbs for focused instance on active→inactive transitions
let lastActiveState: string | null = null;
claudeState.subscribe(($state) => {
  const instanceId = get(currentInstanceId);
  if (!instanceId) return;

  const isNowActive = $state.type === 'Thinking' || $state.type === 'Responding' || $state.type === 'ToolExecuting';

  if (!isNowActive && lastActiveState) {
    clearInstanceVerbs(instanceId);
  }

  lastActiveState = isNowActive ? $state.type : null;
});

// =============================================================================
// Baud Rate (Output Velocity) - Self-Managing Store
// =============================================================================

// Sample window determines how "smoothed" the rate appears
// 1s window gives responsive feedback without being jittery
const SAMPLE_WINDOW_MS = 1000;

// Max rate for normalization (5000 chars/sec is fast typing or code output)
const MAX_BAUD_RATE = 5000;

// Decay interval: 100ms gives smooth animation at 10fps
// while keeping CPU usage low
const DECAY_INTERVAL_MS = 100;

/** Rolling window of output samples for rate calculation */
const outputSamples: { time: number; bytes: number }[] = [];

/** Internal writable for current rate */
const _baudRate = writable<number>(0);

/** Internal writable for activity level */
const _activityLevel = writable<number>(0);

/**
 * Self-managing baud rate store.
 *
 * Subscription-aware store pattern: interval only runs when someone
 * is subscribed. This prevents memory leaks if component unmounts
 * without cleanup, and avoids unnecessary work when no UI is visible.
 * The alternative (manual start/stop) requires every consumer to
 * remember cleanup, which historically caused leaks.
 */
function createActivityStore() {
  let subscriberCount = 0;
  let decayInterval: ReturnType<typeof setInterval> | null = null;

  function startDecay() {
    if (decayInterval) return;
    decayInterval = setInterval(() => {
      const now = Date.now();
      const cutoff = now - SAMPLE_WINDOW_MS;

      // Remove stale samples
      while (outputSamples.length > 0 && outputSamples[0].time < cutoff) {
        outputSamples.shift();
      }

      // Calculate and update rate
      if (outputSamples.length === 0) {
        _baudRate.set(0);
        _activityLevel.set(0);
      } else {
        const totalBytes = outputSamples.reduce((sum, s) => sum + s.bytes, 0);
        const windowMs = now - outputSamples[0].time;
        const rate = windowMs > 0 ? (totalBytes / windowMs) * 1000 : 0;
        _baudRate.set(Math.round(rate));
        _activityLevel.set(Math.min(1, rate / MAX_BAUD_RATE));
      }
    }, DECAY_INTERVAL_MS);
  }

  function stopDecay() {
    if (decayInterval) {
      clearInterval(decayInterval);
      decayInterval = null;
    }
  }

  // Wrap the internal store with subscription tracking
  return {
    subscribe: (handler: (value: number) => void) => {
      subscriberCount++;

      // Start decay on first subscriber
      if (subscriberCount === 1) {
        startDecay();
      }

      const unsubscribe = _baudRate.subscribe(handler);

      // Return cleanup function
      return () => {
        unsubscribe();
        subscriberCount--;

        // Stop decay on last unsubscribe
        if (subscriberCount === 0) {
          stopDecay();
        }
      };
    }
  };
}

/** Current output rate (characters per second) - self-managing */
export const baudRate = createActivityStore();

/** Normalized activity level (0-1) for visual indicators */
export const activityLevel = {
  subscribe: (handler: (value: number) => void) => {
    // Activity level piggybacks on baudRate's lifecycle management
    // by ensuring baudRate is subscribed when activityLevel is
    const unsubBaud = baudRate.subscribe(() => {});
    const unsubLevel = _activityLevel.subscribe(handler);

    return () => {
      unsubLevel();
      unsubBaud();
    };
  }
};

/** Record output bytes for rate calculation */
export function trackOutput(bytes: number): void {
  const now = Date.now();
  outputSamples.push({ time: now, bytes });

  // Remove samples outside the window
  const cutoff = now - SAMPLE_WINDOW_MS;
  while (outputSamples.length > 0 && outputSamples[0].time < cutoff) {
    outputSamples.shift();
  }

  // Immediate update for responsiveness
  const totalBytes = outputSamples.reduce((sum, s) => sum + s.bytes, 0);
  const windowMs =
    outputSamples.length > 1 ? outputSamples[outputSamples.length - 1].time - outputSamples[0].time : SAMPLE_WINDOW_MS;
  const rate = windowMs > 0 ? (totalBytes / windowMs) * 1000 : 0;

  _baudRate.set(Math.round(rate));
  _activityLevel.set(Math.min(1, rate / MAX_BAUD_RATE));
}

/** Clear output samples (call when switching instances) */
export function clearOutputSamples(): void {
  outputSamples.length = 0;
  _baudRate.set(0);
  _activityLevel.set(0);
}

// Clear samples when switching instances to prevent phantom activity
let lastInstanceId: string | null = null;
currentInstanceId.subscribe(($id) => {
  if ($id !== lastInstanceId) {
    lastInstanceId = $id;
    clearOutputSamples();
  }
});
