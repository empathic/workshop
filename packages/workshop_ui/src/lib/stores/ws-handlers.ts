/**
 * WebSocket Message Handlers
 *
 * Owns the dispatch switch and per-message-type handlers for multiplexed
 * WebSocket messages. Extracted from websocket.ts to separate message handling
 * from connection lifecycle.
 */

import { get } from 'svelte/store';
import type { WsMessage, ClaudeState, Instance, PresenceUser, Task } from '$lib/types';
import { instances, fireInstanceListReceived } from './instances';
import { setConversation, appendTurns } from './conversation';
import { trackOutput } from './activity';
import { writeTerminalOutput, writeTerminalHistory } from './terminal';
import { handleTerminalLockUpdate } from './terminalLock';
import {
  handleChatMessage,
  handleChatHistory,
  handleChatTopics,
  type ChatMessageData,
  type ChatTopicSummary
} from './chat';
import { handleTaskUpdate, handleTaskDeleted } from './tasks';
import { handleUserSettingsUpdate } from './settings';
import { handleInboxUpdate, handleInboxList, type InboxItem } from './inbox';

// =============================================================================
// Multiplexed Message Types
// =============================================================================

export interface MuxClientMessage {
  type:
    | 'Focus'
    | 'Input'
    | 'Resize'
    | 'SessionSelect'
    | 'ConversationSync'
    | 'Lobby'
    | 'TerminalLockRequest'
    | 'TerminalLockRelease'
    | 'ChatSend'
    | 'ChatHistory'
    | 'ChatForward'
    | 'ChatTopics'
    | 'TerminalVisible'
    | 'TerminalHidden';
  instance_id?: string;
  since_uuid?: string;
  data?: string;
  rows?: number;
  cols?: number;
  session_id?: string;
  channel?: string;
  payload?: unknown;
  task_id?: number;
  scope?: string;
  content?: string;
  uuid?: string;
  before_id?: number;
  limit?: number;
  message_id?: number;
  target_scope?: string;
  topic?: string | null;
}

interface SessionCandidate {
  session_id: string;
  started_at?: string;
  message_count: number;
  preview?: string;
}

export type MuxServerMessage =
  | { type: 'Output'; instance_id: string; data: string }
  | { type: 'OutputHistory'; instance_id: string; data: string }
  | { type: 'ConversationFull'; instance_id: string; turns: unknown[] }
  | { type: 'ConversationUpdate'; instance_id: string; turns: unknown[] }
  | { type: 'SessionAmbiguous'; instance_id: string; candidates: SessionCandidate[] }
  | { type: 'StateChange'; instance_id: string; state: ClaudeState; stale?: boolean; entered_at?: number }
  | { type: 'InstanceCreated'; instance: Instance }
  | { type: 'InstanceStopped'; instance_id: string }
  | { type: 'InstanceRenamed'; instance_id: string; custom_name: string | null }
  | { type: 'InstanceList'; instances: Instance[] }
  | { type: 'FocusAck'; instance_id: string; claude_state?: ClaudeState }
  | { type: 'Error'; instance_id?: string; message: string }
  | { type: 'PresenceUpdate'; instance_id: string; users: PresenceUser[] }
  | { type: 'OutputLagged'; instance_id: string; dropped_count: number }
  | { type: 'LobbyBroadcast'; sender_id: string; channel: string; payload: unknown }
  | {
      type: 'TerminalLockUpdate';
      instance_id: string;
      holder?: PresenceUser | null;
      last_activity?: string | null;
      expires_in_secs?: number | null;
    }
  | {
      type: 'ChatMessage';
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
  | { type: 'ChatHistoryResponse'; scope: string; messages: ChatMessageData[]; has_more: boolean }
  | { type: 'ChatTopicsResponse'; scope: string; topics: ChatTopicSummary[] }
  | { type: 'TaskUpdate'; task: Task }
  | { type: 'TaskDeleted'; task_id: number }
  | { type: 'UserSettingsUpdate'; user_id: string; settings: Record<string, string> }
  | { type: 'InboxUpdate'; instance_id: string; item: InboxItem | null }
  | { type: 'InboxList'; items: InboxItem[] }
  | { type: 'Shutdown'; reason: string };

// =============================================================================
// Handler Context — mutable state owned by websocket.ts
// =============================================================================

export interface HandlerContext {
  /** Get the currently focused instance ID */
  getFocusedId: () => string | null;
  /** Get the session picker callback */
  getSessionPickerCallback: () => ((msg: WsMessage & { type: 'SessionAmbiguous' }) => void) | null;
  /** Get lobby handlers map */
  getLobbyHandlers: () => Map<string, (senderId: string, payload: unknown) => void>;
  /** Get/set the conversation sync timeout */
  getConversationSyncTimeout: () => ReturnType<typeof setTimeout> | null;
  setConversationSyncTimeout: (t: ReturnType<typeof setTimeout> | null) => void;
  /** Presence store setter */
  updatePresence: (instanceId: string, users: PresenceUser[]) => void;
  /** Connection state setters */
  setConnected: (instanceId: string) => void;
  setError: (instanceId: string, error?: string) => void;
  /** Server shutdown notification */
  setServerGone: (reason?: string) => void;
}

// =============================================================================
// Validation
// =============================================================================

function validateInstanceId(instanceId: string | undefined, msgType: string): instanceId is string {
  if (!instanceId) {
    console.error(`[WebSocket] ${msgType} missing instance_id - message dropped`);
    return false;
  }

  const knownInstances = get(instances);
  if (!knownInstances.has(instanceId)) {
    console.warn(`[WebSocket] ${msgType} for unknown instance ${instanceId}`);
  }

  return true;
}

// =============================================================================
// Handler Factory
// =============================================================================

export function createMessageHandler(ctx: HandlerContext): (msg: MuxServerMessage) => void {
  return (msg: MuxServerMessage) => {
    switch (msg.type) {
      case 'InstanceList':
        console.log('[WebSocket] Received instance list:', msg.instances.length, 'instances');
        updateInstancesWithStates(msg.instances);
        fireInstanceListReceived(new Set(msg.instances.map((i: Instance) => i.id)));
        break;

      case 'StateChange':
        instances.update((map) => {
          const instance = map.get(msg.instance_id);
          if (instance) {
            map.set(msg.instance_id, {
              ...instance,
              claude_state: msg.state,
              claude_state_stale: msg.stale ?? false,
              state_entered_at: msg.entered_at ?? instance.state_entered_at
            });
          }
          return new Map(map);
        });
        break;

      case 'InstanceCreated':
        console.log('[WebSocket] Instance created:', msg.instance.id);
        instances.update((map) => {
          map.set(msg.instance.id, msg.instance);
          return new Map(map);
        });
        break;

      case 'InstanceStopped':
        console.log('[WebSocket] Instance stopped:', msg.instance_id);
        instances.update((map) => {
          map.delete(msg.instance_id);
          return new Map(map);
        });
        break;

      case 'InstanceRenamed':
        instances.update((map) => {
          const instance = map.get(msg.instance_id);
          if (instance) {
            map.set(msg.instance_id, { ...instance, custom_name: msg.custom_name });
          }
          return new Map(map);
        });
        break;

      case 'FocusAck':
        console.log('[WebSocket] Focus acknowledged:', msg.instance_id);
        ctx.setConnected(msg.instance_id);
        if (msg.claude_state) {
          instances.update((map) => {
            const instance = map.get(msg.instance_id);
            if (instance) {
              map.set(msg.instance_id, {
                ...instance,
                claude_state: msg.claude_state
              });
            }
            return new Map(map);
          });
        }
        break;

      case 'OutputHistory': {
        if (!validateInstanceId(msg.instance_id, 'OutputHistory')) break;
        writeTerminalHistory(msg.instance_id, msg.data);
        break;
      }

      case 'Output': {
        if (!validateInstanceId(msg.instance_id, 'Output')) break;
        writeTerminalOutput(msg.instance_id, msg.data);
        if (msg.instance_id === ctx.getFocusedId()) {
          trackOutput(msg.data.length);
        }
        break;
      }

      case 'ConversationFull': {
        if (!validateInstanceId(msg.instance_id, 'ConversationFull')) break;
        console.log('[WebSocket] ConversationFull for', msg.instance_id, 'turns:', msg.turns.length);
        setConversation(msg.instance_id, msg.turns as never[]);
        clearConversationSyncTimeout(ctx);
        break;
      }

      case 'ConversationUpdate': {
        if (!validateInstanceId(msg.instance_id, 'ConversationUpdate')) break;
        console.log('[WebSocket] ConversationUpdate for', msg.instance_id, 'turns:', msg.turns.length);
        appendTurns(msg.instance_id, msg.turns as never[]);
        clearConversationSyncTimeout(ctx);
        break;
      }

      case 'SessionAmbiguous':
        if (!validateInstanceId(msg.instance_id, 'SessionAmbiguous')) break;
        ctx.getSessionPickerCallback()?.({
          type: 'SessionAmbiguous',
          instance_id: msg.instance_id,
          candidates: msg.candidates
        } as WsMessage & { type: 'SessionAmbiguous' });
        break;

      case 'Error': {
        console.error('[WebSocket] Server error:', msg.message, 'instance:', msg.instance_id);
        const errorInstanceId = msg.instance_id ?? ctx.getFocusedId();
        if (errorInstanceId) {
          ctx.setError(errorInstanceId);
        }
        break;
      }

      case 'PresenceUpdate':
        ctx.updatePresence(msg.instance_id, msg.users);
        break;

      case 'OutputLagged':
        console.warn(`[WebSocket] Output lagged for ${msg.instance_id}: ${msg.dropped_count} messages dropped`);
        break;

      case 'LobbyBroadcast': {
        const handler = ctx.getLobbyHandlers().get(msg.channel);
        if (handler) {
          handler(msg.sender_id, msg.payload);
        }
        break;
      }

      case 'TerminalLockUpdate':
        handleTerminalLockUpdate(
          msg.instance_id,
          msg.holder ?? null,
          msg.last_activity ?? null,
          msg.expires_in_secs ?? null
        );
        break;

      case 'ChatMessage':
        handleChatMessage({
          id: msg.id,
          uuid: msg.uuid,
          scope: msg.scope,
          user_id: msg.user_id,
          display_name: msg.display_name,
          content: msg.content,
          created_at: msg.created_at,
          forwarded_from: msg.forwarded_from,
          topic: msg.topic
        });
        break;

      case 'ChatHistoryResponse':
        handleChatHistory(msg.scope, msg.messages, msg.has_more);
        break;

      case 'ChatTopicsResponse':
        handleChatTopics(msg.scope, msg.topics);
        break;

      case 'TaskUpdate':
        handleTaskUpdate(msg.task);
        break;

      case 'TaskDeleted':
        handleTaskDeleted(msg.task_id);
        break;

      case 'UserSettingsUpdate':
        handleUserSettingsUpdate(msg.settings);
        break;

      case 'InboxUpdate':
        handleInboxUpdate(msg.instance_id, msg.item);
        break;

      case 'InboxList':
        handleInboxList(msg.items);
        break;

      case 'Shutdown':
        console.warn('[WebSocket] Server shutting down:', msg.reason);
        ctx.setServerGone(msg.reason);
        break;
    }
  };
}

// =============================================================================
// Helpers
// =============================================================================

function clearConversationSyncTimeout(ctx: HandlerContext): void {
  const timeout = ctx.getConversationSyncTimeout();
  if (timeout) {
    clearTimeout(timeout);
    ctx.setConversationSyncTimeout(null);
  }
}

function updateInstancesWithStates(serverInstances: Instance[]): void {
  instances.update((map) => {
    const newMap = new Map<string, Instance>();
    for (const instance of serverInstances) {
      const existing = map.get(instance.id);
      newMap.set(instance.id, {
        ...instance,
        claude_state: instance.claude_state ?? existing?.claude_state
      });
    }
    return newMap;
  });
}
