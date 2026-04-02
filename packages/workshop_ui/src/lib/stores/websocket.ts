/**
 * WebSocket Management - Multiplexed Version
 *
 * Single WebSocket connection that:
 * - Receives state changes from ALL instances (for sidebar status)
 * - Receives terminal output/conversation from focused instance only
 * - Handles focus switching with automatic history replay
 *
 * Message handling is delegated to ws-handlers.ts.
 */

import { get, writable, derived } from 'svelte/store';
import type { WsMessage, PresenceUser } from '$lib/types';
import { currentInstanceId, addPendingInput, flushPendingInput, getLastConversationUuid } from './instances';
import { recordWebSocketMessage, recordWebSocketReconnect } from './metrics';
import { setLoadingHistory } from './chat';
import { createMessageHandler, type MuxClientMessage, type MuxServerMessage } from './ws-handlers';
import { fetchServerSettings } from './settings';
import { fetchServerConfig } from './server-config';
import { checkAuth } from './auth';

// =============================================================================
// Connection State (formerly connection.ts)
// =============================================================================

export type ConnectionStatus = 'disconnected' | 'connecting' | 'connected' | 'reconnecting' | 'error' | 'server_gone';

export type ConnectionState =
  | { status: 'disconnected' }
  | { status: 'connecting'; instanceId: string }
  | { status: 'connected'; instanceId: string }
  | { status: 'reconnecting'; instanceId: string }
  | { status: 'error'; instanceId: string; error?: string }
  | { status: 'server_gone'; instanceId: string; reason?: string };

export const connectionState = writable<ConnectionState>({ status: 'disconnected' });

export const connectionStatus = derived(connectionState, ($state) => $state.status);

export const connectedInstanceId = derived(connectionState, ($state) =>
  $state.status === 'disconnected' ? null : $state.instanceId
);

export const isConnectionActive = derived(connectionState, ($state) => $state.status !== 'disconnected');

/** Reason from the server's Shutdown message, if any */
export const shutdownReason = writable<string | null>(null);

function setConnecting(instanceId: string): void {
  connectionState.set({ status: 'connecting', instanceId });
}

function setConnected(instanceId: string): void {
  connectionState.set({ status: 'connected', instanceId });
}

function setReconnecting(instanceId: string): void {
  connectionState.set({ status: 'reconnecting', instanceId });
}

function setError(instanceId: string, error?: string): void {
  connectionState.set({ status: 'error', instanceId, error });
}

function setServerGone(instanceId: string, reason?: string): void {
  connectionState.set({ status: 'server_gone', instanceId, reason });
}

function setDisconnected(): void {
  connectionState.set({ status: 'disconnected' });
}

// =============================================================================
// Presence Store
// =============================================================================

/** Per-instance presence: which users are viewing each instance */
export const instancePresence = writable<Map<string, PresenceUser[]>>(new Map());

// =============================================================================
// Internal State
// =============================================================================

let socket: WebSocket | null = null;
let reconnectTimeout: ReturnType<typeof setTimeout> | null = null;
let reconnectAttempt = 0;
let currentFocusedId: string | null = null;
let sessionPickerCallback: ((msg: WsMessage & { type: 'SessionAmbiguous' }) => void) | null = null;
const lobbyHandlers = new Map<string, (senderId: string, payload: unknown) => void>();
let lastMessageTime: number = Date.now();
let visibilityHandler: (() => void) | null = null;
let heartbeatInterval: ReturnType<typeof setInterval> | null = null;
let conversationSyncTimeout: ReturnType<typeof setTimeout> | null = null;

const STALE_THRESHOLD_MS = 60_000;
const HEARTBEAT_INTERVAL_MS = 30_000;
const CONVERSATION_SYNC_TIMEOUT_MS = 10_000;

/** After this many consecutive failed reconnects, escalate to server_gone */
const SERVER_GONE_THRESHOLD = 3;

// =============================================================================
// Message Handler (from ws-handlers.ts)
// =============================================================================

const handleMultiplexedMessage = createMessageHandler({
  getFocusedId: () => currentFocusedId,
  getSessionPickerCallback: () => sessionPickerCallback,
  getLobbyHandlers: () => lobbyHandlers,
  getConversationSyncTimeout: () => conversationSyncTimeout,
  setConversationSyncTimeout: (t) => {
    conversationSyncTimeout = t;
  },
  updatePresence: (instanceId, users) => {
    instancePresence.update((map) => {
      if (users.length === 0) {
        map.delete(instanceId);
      } else {
        map.set(instanceId, users);
      }
      return new Map(map);
    });
  },
  setConnected,
  setError,
  setServerGone: (reason?: string) => {
    shutdownReason.set(reason ?? null);
    const selectedId = get(currentInstanceId);
    if (selectedId) {
      setServerGone(selectedId, reason);
    }
  }
});

// =============================================================================
// Public API
// =============================================================================

export function setSessionPickerCallback(cb: ((msg: WsMessage & { type: 'SessionAmbiguous' }) => void) | null): void {
  sessionPickerCallback = cb;
}

/**
 * Initialize the multiplexed WebSocket connection.
 * Should be called once on app mount.
 */
export function initMultiplexedConnection(): void {
  if (socket && socket.readyState === WebSocket.OPEN) {
    return;
  }

  connectMultiplexed();
  setupVisibilityHandler();
  setupHeartbeat();
}

/**
 * Connect/focus to an instance.
 * If already connected to multiplexed WS, just sends Focus message.
 */
export function connect(instanceId: string): void {
  if (!socket || socket.readyState !== WebSocket.OPEN) {
    setConnecting(instanceId);
    connectMultiplexed(() => {
      sendFocus(instanceId);
    });
    return;
  }

  sendFocus(instanceId);
}

/** Disconnect - just clear focus, keep multiplexed connection alive. */
export function disconnect(): void {
  currentFocusedId = null;
  setDisconnected();
}

/** Full disconnect - close the multiplexed WebSocket. */
export function disconnectAll(): void {
  if (reconnectTimeout) {
    clearTimeout(reconnectTimeout);
    reconnectTimeout = null;
  }
  if (heartbeatInterval) {
    clearInterval(heartbeatInterval);
    heartbeatInterval = null;
  }
  if (conversationSyncTimeout) {
    clearTimeout(conversationSyncTimeout);
    conversationSyncTimeout = null;
  }
  if (visibilityHandler) {
    document.removeEventListener('visibilitychange', visibilityHandler);
    visibilityHandler = null;
  }
  if (socket) {
    socket.close();
    socket = null;
  }
  currentFocusedId = null;
  setDisconnected();
}

/**
 * Send a complete message (text + Enter).
 * The delay before sending Enter is proportional to message length,
 * since Claude needs time to process longer text input.
 */
export function sendMessage(message: string, taskId?: number): void {
  sendRaw(message, taskId);

  const delay = Math.min(750, 50 + message.length * 0.5);
  setTimeout(() => {
    sendRaw('\r');
  }, delay);
}

/**
 * Send raw terminal input.
 * If not connected, queues to the current instance's pending buffer.
 */
export function sendRaw(data: string, taskId?: number): void {
  const instanceId = get(currentInstanceId);
  if (!instanceId) return;

  if (socket?.readyState === WebSocket.OPEN) {
    const msg: MuxClientMessage = { type: 'Input', instance_id: instanceId, data };
    if (taskId != null) msg.task_id = taskId;
    socket.send(JSON.stringify(msg));
    return;
  }

  addPendingInput(instanceId, data);
}

/** Send terminal resize notification. Uses explicit instanceId to avoid races during instance switching. */
export function sendResize(rows: number, cols: number, explicitInstanceId?: string): void {
  const instanceId = explicitInstanceId ?? get(currentInstanceId);
  if (socket?.readyState !== WebSocket.OPEN || !instanceId) return;
  socket.send(JSON.stringify({ type: 'Resize', instance_id: instanceId, rows, cols } as MuxClientMessage));
}

/** Notify server that terminal panel is visible (include in dimension negotiation). */
export function sendTerminalVisible(rows: number, cols: number, explicitInstanceId?: string): void {
  const instanceId = explicitInstanceId ?? get(currentInstanceId);
  if (socket?.readyState !== WebSocket.OPEN || !instanceId) return;
  socket.send(JSON.stringify({ type: 'TerminalVisible', instance_id: instanceId, rows, cols } as MuxClientMessage));
}

/** Notify server that terminal panel is hidden (exclude from dimension negotiation). */
export function sendTerminalHidden(explicitInstanceId?: string): void {
  const instanceId = explicitInstanceId ?? get(currentInstanceId);
  if (socket?.readyState !== WebSocket.OPEN || !instanceId) return;
  socket.send(JSON.stringify({ type: 'TerminalHidden', instance_id: instanceId } as MuxClientMessage));
}

/** Send session selection (for ambiguous session resolution). */
export function sendSessionSelect(sessionId: string): void {
  if (socket?.readyState !== WebSocket.OPEN) return;
  socket.send(JSON.stringify({ type: 'SessionSelect', session_id: sessionId } as MuxClientMessage));
}

/** Send a message on a lobby channel (broadcast to all clients). */
export function sendLobbyMessage(channel: string, payload: unknown): void {
  if (socket?.readyState !== WebSocket.OPEN) return;
  socket.send(JSON.stringify({ type: 'Lobby', channel, payload } as MuxClientMessage));
}

/** Register a handler for a lobby channel. Returns unsubscribe function. */
export function onLobbyMessage(channel: string, handler: (senderId: string, payload: unknown) => void): () => void {
  lobbyHandlers.set(channel, handler);
  return () => lobbyHandlers.delete(channel);
}

/** Unregister a lobby channel handler. */
export function offLobbyMessage(channel: string): void {
  lobbyHandlers.delete(channel);
}

/** Request terminal lock for the current instance. */
export function requestTerminalLock(): void {
  const instanceId = get(currentInstanceId);
  if (!instanceId || socket?.readyState !== WebSocket.OPEN) return;
  socket.send(JSON.stringify({ type: 'TerminalLockRequest', instance_id: instanceId } as MuxClientMessage));
}

/** Release terminal lock for the current instance. */
export function releaseTerminalLock(): void {
  const instanceId = get(currentInstanceId);
  if (!instanceId || socket?.readyState !== WebSocket.OPEN) return;
  socket.send(JSON.stringify({ type: 'TerminalLockRelease', instance_id: instanceId } as MuxClientMessage));
}

/** Send a chat message to a scope, optionally with a topic. */
export function sendChatMessage(scope: string, content: string, topic?: string | null): void {
  if (socket?.readyState !== WebSocket.OPEN) return;
  const uuid = crypto.randomUUID();
  const msg: MuxClientMessage = { type: 'ChatSend', scope, content, uuid };
  if (topic) msg.topic = topic;
  socket.send(JSON.stringify(msg));
}

/** Request chat history for a scope, optionally filtered by topic. */
export function requestChatHistory(scope: string, beforeId?: number, limit?: number, topic?: string | null): void {
  if (socket?.readyState !== WebSocket.OPEN) return;
  setLoadingHistory(scope);
  const msg: MuxClientMessage = {
    type: 'ChatHistory',
    scope,
    before_id: beforeId,
    limit: limit ?? 50
  };
  if (topic) msg.topic = topic;
  socket.send(JSON.stringify(msg));
}

/** Request list of topics for a scope. */
export function requestChatTopics(scope: string): void {
  if (socket?.readyState !== WebSocket.OPEN) return;
  socket.send(JSON.stringify({ type: 'ChatTopics', scope } as MuxClientMessage));
}

/** Send composed text to a specific instance's PTY. */
export function sendToInstance(instanceId: string, content: string): void {
  if (socket?.readyState !== WebSocket.OPEN) return;
  socket.send(JSON.stringify({ type: 'Input', instance_id: instanceId, data: content } as MuxClientMessage));
  const delay = Math.min(750, 50 + content.length * 0.5);
  setTimeout(() => {
    if (socket?.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify({ type: 'Input', instance_id: instanceId, data: '\r' } as MuxClientMessage));
    }
  }, delay);
}

/** Forward a chat message to another scope. */
export function forwardChatMessage(messageId: number, targetScope: string): void {
  if (socket?.readyState !== WebSocket.OPEN) return;
  socket.send(
    JSON.stringify({ type: 'ChatForward', message_id: messageId, target_scope: targetScope } as MuxClientMessage)
  );
}

/** Check if currently connected. */
export function isConnected(): boolean {
  return socket?.readyState === WebSocket.OPEN;
}

/** Refresh an instance's output. Uses current instance if no ID given. */
export async function sendRefresh(id?: string): Promise<void> {
  const instanceId = id ?? get(currentInstanceId);
  if (!instanceId) return;

  if (socket?.readyState === WebSocket.OPEN) {
    sendFocus(instanceId);
  }
}

/** Manually trigger reconnection. */
export function reconnect(): void {
  connectMultiplexed();
}

// Legacy alias for Terminal component
export const sendInput = sendRaw;

// Re-export for backwards compatibility
export { hasPendingInput } from './instances';
export { claudeState } from './claude';
export { baudRate, activityLevel } from './activity';

// =============================================================================
// Internal Functions
// =============================================================================

function connectMultiplexed(onConnected?: () => void): void {
  if (socket && socket.readyState === WebSocket.OPEN) {
    onConnected?.();
    return;
  }

  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const wsUrl = `${protocol}//${window.location.host}/api/ws`;

  console.log('[WebSocket] Connecting to multiplexed endpoint:', wsUrl);

  socket = new WebSocket(wsUrl);

  socket.onopen = () => {
    console.log('[WebSocket] Connected');
    reconnectAttempt = 0;
    shutdownReason.set(null);

    const pendingFocus = get(currentInstanceId);
    if (pendingFocus) {
      sendFocus(pendingFocus);
    }

    // Re-sync all server state on (re)connect. These three fetches are
    // independent and convergent: each writes to its own store, and
    // checkAuth() is the sole authority for auth-enabled state (server-config
    // only does optimistic writes during applyConfig). Order doesn't matter;
    // all three settling in any order produces the same final state.
    fetchServerSettings();
    fetchServerConfig();
    checkAuth();

    onConnected?.();
  };

  socket.onmessage = (event) => {
    lastMessageTime = Date.now();
    recordWebSocketMessage();

    const t0 = performance.now();
    let msg: MuxServerMessage;
    try {
      msg = JSON.parse(event.data);
    } catch {
      console.warn('[WebSocket] Received non-JSON message:', event.data.slice(0, 100));
      return;
    }
    const parseMs = performance.now() - t0;

    try {
      const t1 = performance.now();
      handleMultiplexedMessage(msg);
      const handleMs = performance.now() - t1;
      const totalMs = parseMs + handleMs;
      if (totalMs > 50) {
        console.warn(
          `[WebSocket] Slow message: ${msg.type} parse=${parseMs.toFixed(1)}ms handle=${handleMs.toFixed(1)}ms total=${totalMs.toFixed(1)}ms` +
            (event.data.length > 1024 ? ` size=${(event.data.length / 1024).toFixed(0)}KB` : '')
        );
      }
    } catch (e) {
      console.error('[WebSocket] Handler error:', e, 'Message type:', msg.type);
    }
  };

  socket.onerror = (e) => {
    console.error('[WebSocket] Error:', e);
    if (currentFocusedId) {
      setError(currentFocusedId);
    }
  };

  socket.onclose = () => {
    console.log('[WebSocket] Disconnected');
    socket = null;

    const selectedId = get(currentInstanceId);
    if (selectedId) {
      setReconnecting(selectedId);
      scheduleReconnect();
    } else {
      setDisconnected();
    }
  };
}

function setupVisibilityHandler(): void {
  if (visibilityHandler) return;

  visibilityHandler = () => {
    if (document.visibilityState === 'visible') {
      const timeSinceLastMessage = Date.now() - lastMessageTime;
      console.log(
        `[WebSocket] Tab became visible. Time since last message: ${Math.round(timeSinceLastMessage / 1000)}s`
      );

      if (timeSinceLastMessage > STALE_THRESHOLD_MS) {
        console.log('[WebSocket] Connection may be stale, refreshing state...');
        refreshAfterVisibilityChange();
      }
    }
  };

  document.addEventListener('visibilitychange', visibilityHandler);
}

function refreshAfterVisibilityChange(): void {
  if (!socket || socket.readyState !== WebSocket.OPEN) {
    console.log('[WebSocket] Socket not open, triggering reconnect');
    connectMultiplexed();
    return;
  }

  if (currentFocusedId) {
    const lastUuid = getLastConversationUuid(currentFocusedId);
    console.log('[WebSocket] Syncing conversation for:', currentFocusedId, 'since:', lastUuid);

    if (conversationSyncTimeout) {
      clearTimeout(conversationSyncTimeout);
    }

    socket.send(
      JSON.stringify({
        type: 'ConversationSync',
        since_uuid: lastUuid
      } as MuxClientMessage)
    );

    conversationSyncTimeout = setTimeout(() => {
      console.log('[WebSocket] ConversationSync timeout - no response in 10s, reconnecting');
      if (socket) {
        socket.close();
      }
    }, CONVERSATION_SYNC_TIMEOUT_MS);
  }
}

function setupHeartbeat(): void {
  if (heartbeatInterval) return;

  heartbeatInterval = setInterval(() => {
    if (!socket || socket.readyState !== WebSocket.OPEN) return;

    const timeSinceLastMessage = Date.now() - lastMessageTime;

    if (timeSinceLastMessage > STALE_THRESHOLD_MS * 2 && currentFocusedId) {
      console.log('[WebSocket] No messages for extended period, connection may be dead');
      socket.close();
    }
  }, HEARTBEAT_INTERVAL_MS);
}

function scheduleReconnect(): void {
  if (reconnectTimeout) return;

  const delay = Math.min(1000 * Math.pow(2, reconnectAttempt), 30000);
  reconnectAttempt++;
  recordWebSocketReconnect();

  // After repeated failures, escalate from "reconnecting" to "server gone"
  if (reconnectAttempt >= SERVER_GONE_THRESHOLD) {
    const selectedId = get(currentInstanceId);
    if (selectedId) {
      const currentState = get(connectionState);
      // Only escalate if we haven't already
      if (currentState.status !== 'server_gone') {
        console.log(`[WebSocket] ${reconnectAttempt} failed attempts — server appears offline`);
        setServerGone(selectedId);
      }
    }
  }

  console.log(`[WebSocket] Reconnecting in ${delay}ms (attempt ${reconnectAttempt})...`);

  reconnectTimeout = setTimeout(() => {
    reconnectTimeout = null;
    connectMultiplexed();
  }, delay);
}

function sendFocus(instanceId: string): void {
  if (!socket || socket.readyState !== WebSocket.OPEN) {
    return;
  }

  console.log('[WebSocket] Focusing on instance:', instanceId);
  currentFocusedId = instanceId;

  socket.send(JSON.stringify({ type: 'Focus', instance_id: instanceId } as MuxClientMessage));

  const pending = flushPendingInput(instanceId);
  for (const data of pending) {
    socket.send(JSON.stringify({ type: 'Input', instance_id: instanceId, data } as MuxClientMessage));
  }
}
