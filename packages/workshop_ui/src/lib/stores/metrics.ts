/**
 * Performance Metrics Store
 *
 * Collects runtime metrics for observability and debugging.
 * Toggle with Ctrl+Shift+D to show debug panel.
 */

import { writable, derived, get } from 'svelte/store';

// =============================================================================
// Types
// =============================================================================

export interface VirtualListMetrics {
  totalItems: number;
  renderedItems: number;
  heightCacheSize: number;
  lastRenderMs: number;
}

export interface TerminalMetrics {
  bufferCount: number;
  totalChunks: number;
  nearCapacityCount: number;
}

export interface AvatarMetrics {
  cacheSize: number;
  cacheHits: number;
  cacheMisses: number;
}

export interface WebSocketMetrics {
  messagesReceived: number;
  messagesPerSecond: number;
  reconnectCount: number;
  lastLatencyMs: number;
}

export interface VoiceMetrics {
  backend: 'hybrid' | 'prompt-api' | 'web-speech' | 'none';
  state: 'idle' | 'listening' | 'transcribing';
  transcriptionCount: number;
  errorCount: number;
  errors: string[];
  lastTranscribeMs: number;
}

export interface Metrics {
  virtualList: VirtualListMetrics;
  terminal: TerminalMetrics;
  avatar: AvatarMetrics;
  websocket: WebSocketMetrics;
  voice: VoiceMetrics;
}

// =============================================================================
// Initial State
// =============================================================================

function initialMetrics(): Metrics {
  return {
    virtualList: {
      totalItems: 0,
      renderedItems: 0,
      heightCacheSize: 0,
      lastRenderMs: 0
    },
    terminal: {
      bufferCount: 0,
      totalChunks: 0,
      nearCapacityCount: 0
    },
    avatar: {
      cacheSize: 0,
      cacheHits: 0,
      cacheMisses: 0
    },
    websocket: {
      messagesReceived: 0,
      messagesPerSecond: 0,
      reconnectCount: 0,
      lastLatencyMs: 0
    },
    voice: {
      backend: 'none',
      state: 'idle',
      transcriptionCount: 0,
      errorCount: 0,
      errors: [],
      lastTranscribeMs: 0
    }
  };
}

// =============================================================================
// Store
// =============================================================================

const _metrics = writable<Metrics>(initialMetrics());

// Track message timestamps for rate calculation
const messageTimestamps: number[] = [];
const RATE_WINDOW_MS = 5000;

// =============================================================================
// Update Functions
// =============================================================================

export function updateVirtualListMetrics(data: Partial<VirtualListMetrics>): void {
  _metrics.update((m) => ({
    ...m,
    virtualList: { ...m.virtualList, ...data }
  }));
}

export function updateTerminalMetrics(data: Partial<TerminalMetrics>): void {
  _metrics.update((m) => ({
    ...m,
    terminal: { ...m.terminal, ...data }
  }));
}

export function updateAvatarMetrics(data: Partial<AvatarMetrics>): void {
  _metrics.update((m) => ({
    ...m,
    avatar: { ...m.avatar, ...data }
  }));
}

export function recordAvatarCacheHit(): void {
  _metrics.update((m) => ({
    ...m,
    avatar: { ...m.avatar, cacheHits: m.avatar.cacheHits + 1 }
  }));
}

export function recordAvatarCacheMiss(): void {
  _metrics.update((m) => ({
    ...m,
    avatar: { ...m.avatar, cacheMisses: m.avatar.cacheMisses + 1 }
  }));
}

export function recordWebSocketMessage(): void {
  const now = Date.now();
  messageTimestamps.push(now);

  // Remove old timestamps outside window
  const cutoff = now - RATE_WINDOW_MS;
  while (messageTimestamps.length > 0 && messageTimestamps[0]! < cutoff) {
    messageTimestamps.shift();
  }

  // Calculate rate
  const rate = (messageTimestamps.length / RATE_WINDOW_MS) * 1000;

  _metrics.update((m) => ({
    ...m,
    websocket: {
      ...m.websocket,
      messagesReceived: m.websocket.messagesReceived + 1,
      messagesPerSecond: rate
    }
  }));
}

export function recordWebSocketReconnect(): void {
  _metrics.update((m) => ({
    ...m,
    websocket: {
      ...m.websocket,
      reconnectCount: m.websocket.reconnectCount + 1
    }
  }));
}

export function recordWebSocketLatency(latencyMs: number): void {
  _metrics.update((m) => ({
    ...m,
    websocket: { ...m.websocket, lastLatencyMs: latencyMs }
  }));
}

export function updateVoiceMetrics(data: Partial<VoiceMetrics>): void {
  _metrics.update((m) => ({
    ...m,
    voice: { ...m.voice, ...data }
  }));
}

export function recordVoiceTranscription(latencyMs?: number): void {
  _metrics.update((m) => ({
    ...m,
    voice: {
      ...m.voice,
      transcriptionCount: m.voice.transcriptionCount + 1,
      ...(latencyMs !== undefined ? { lastTranscribeMs: latencyMs } : {})
    }
  }));
}

export function recordVoiceError(message: string): void {
  _metrics.update((m) => ({
    ...m,
    voice: {
      ...m.voice,
      state: 'idle' as const,
      errorCount: m.voice.errorCount + 1,
      errors: [...m.voice.errors, message].slice(-20)
    }
  }));
}

// =============================================================================
// Derived Values
// =============================================================================

export const metrics = {
  subscribe: _metrics.subscribe
};

export const avatarHitRate = derived(_metrics, ($m) => {
  const total = $m.avatar.cacheHits + $m.avatar.cacheMisses;
  return total > 0 ? $m.avatar.cacheHits / total : 0;
});

// =============================================================================
// Debug Panel Toggle
// =============================================================================

export const debugPanelVisible = writable(false);

export function toggleDebugPanel(): void {
  debugPanelVisible.update((v) => !v);
}

// =============================================================================
// Voice Backend Override
// =============================================================================

/** When set, overrides auto-detection in detectVoiceBackend(). null = auto. */
export const voiceBackendOverride = writable<'hybrid' | 'prompt-api' | 'web-speech' | null>(null);

// =============================================================================
// Reset
// =============================================================================

export function resetMetrics(): void {
  _metrics.set(initialMetrics());
  messageTimestamps.length = 0;
}
