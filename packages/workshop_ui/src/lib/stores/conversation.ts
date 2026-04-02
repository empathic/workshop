/**
 * Conversation State
 *
 * Derives the current conversation from instanceStates based on currentInstanceId.
 * Each instance maintains its own conversation history.
 */

import { derived, get } from 'svelte/store';
import type { ConversationTurn, NotebookCell, ToolCell } from '$lib/types';
import {
  currentInstanceId,
  currentInstanceState,
  instanceStates,
  setInstanceConversation,
  appendInstanceTurns,
  setInstanceWaiting
} from './instances';

// =============================================================================
// Derived Stores (from per-instance state)
// =============================================================================

/** Current instance's conversation turns */
export const conversationTurns = derived(currentInstanceState, ($state): ConversationTurn[] => {
  return $state?.conversation ?? [];
});

/** Whether current instance is waiting for first message */
export const isWaiting = derived(currentInstanceState, ($state): boolean => {
  return $state?.isWaiting ?? true;
});

/** Map role to cell type */
function roleToCellType(role: string): NotebookCell['type'] {
  switch (role) {
    case 'User':
      return 'user';
    case 'Assistant':
      return 'assistant';
    case 'System':
      return 'system';
    case 'AgentProgress':
      return 'agent';
    case 'Progress':
      return 'progress';
    default:
      return 'unknown';
  }
}

/** Transform turns into notebook cells — extracted so both derived stores and per-instance helpers can use it */
function turnsToNotebookCells($turns: ConversationTurn[]): NotebookCell[] {
  const t0 = performance.now();
  const cells: NotebookCell[] = [];

  // Filter out skip entries, then process
  const filteredTurns = $turns.filter((turn) => !turn.skip && turn.role !== 'Skip');

  // Forward accumulation: hold a pending tool-only cell group and extend it
  // as long as we see tool-only assistant turns. Anything that isn't a
  // meaningful conversation boundary (user, system, assistant-with-content)
  // gets absorbed into the group. No backward scanning, no cell-type denylist.
  let pendingToolCell: NotebookCell | null = null;

  for (let i = 0; i < filteredTurns.length; i++) {
    const turn = filteredTurns[i];

    // Progress entries: collect agent progress inside a tool group,
    // otherwise aggregate as before
    if (turn.role === 'Progress' || turn.role === 'AgentProgress') {
      if (pendingToolCell) {
        if (turn.role === 'AgentProgress' && turn.content) {
          if (!pendingToolCell.agentLog) pendingToolCell.agentLog = [];
          pendingToolCell.agentLog.push({
            content: turn.content,
            agentId: turn.agent_id,
            role: turn.agent_msg_role,
            tools: turn.agent_tools
          });
        }
        continue;
      }

      // Check if previous cell is also progress - if so, merge
      const prevCell = cells[cells.length - 1];
      if (prevCell && prevCell.type === 'progress') {
        const currentCount = (prevCell.extra?.progressCount as number) ?? 1;
        const items = (prevCell.extra?.progressItems as string[]) ?? [prevCell.content];

        const newItem = turn.content;
        if (items[items.length - 1] !== newItem) {
          items.push(newItem);
        }

        prevCell.extra = {
          ...prevCell.extra,
          progressCount: currentCount + 1,
          progressItems: items
        };
        prevCell.content = `${currentCount + 1} events`;
        prevCell.timestamp = turn.timestamp;
        continue;
      }

      const cell: NotebookCell = {
        id: turn.uuid ?? `turn-${i}`,
        type: 'progress',
        content: turn.content,
        timestamp: turn.timestamp,
        collapsed: false,
        extra: {
          progressCount: 1,
          progressItems: [turn.content],
          progressType: turn.progress_type ?? (turn.agent_id ? 'agent' : 'hook')
        }
      };

      if (turn.agent_id) cell.agentId = turn.agent_id;
      if (turn.agent_prompt) cell.agentPrompt = turn.agent_prompt;
      if (turn.hook_event) cell.hookEvent = turn.hook_event;

      cells.push(cell);
      continue;
    }

    // Build the cell
    const cell: NotebookCell = {
      id: turn.uuid ?? `turn-${i}`,
      type: roleToCellType(turn.role),
      content: turn.content,
      timestamp: turn.timestamp,
      collapsed: false
    };

    if (turn.role === 'Assistant' && turn.tools.length > 0) {
      cell.toolCells = turn.tools.map(
        (toolName, toolIndex): ToolCell => ({
          id: `${cell.id}-tool-${toolIndex}`,
          name: toolName,
          input: turn.tool_details?.[toolIndex]?.input ?? {},
          output: turn.tool_details?.[toolIndex]?.result,
          is_error: turn.tool_details?.[toolIndex]?.is_error,
          status: 'complete',
          timestamp: turn.timestamp,
          canRerun: isRerunnable(toolName)
        })
      );
    }

    if (turn.thinking) cell.thinking = turn.thinking;
    if (turn.attributed_to) cell.attributed_to = turn.attributed_to;
    if (turn.task_id != null) cell.task_id = turn.task_id;
    if (turn.entry_type) cell.entryType = turn.entry_type;
    if (turn.extra) cell.extra = turn.extra;
    if (turn.agent_id) cell.agentId = turn.agent_id;
    if (turn.agent_prompt) cell.agentPrompt = turn.agent_prompt;
    if (turn.agent_msg_role) cell.agentMsgRole = turn.agent_msg_role;

    // Is this a tool-only assistant turn?
    const isToolOnly =
      cell.type === 'assistant' &&
      cell.toolCells &&
      cell.toolCells.length > 0 &&
      !cell.content.trim() &&
      !cell.thinking;

    if (isToolOnly) {
      if (pendingToolCell) {
        // Extend the group
        pendingToolCell.toolCells!.push(...cell.toolCells!);
        pendingToolCell.timestamp = cell.timestamp;
      } else {
        // Start a new group
        pendingToolCell = cell;
      }
      continue;
    }

    // Non-tool-only cell: does it represent a real conversation boundary?
    const breaksGroup =
      cell.type === 'user' ||
      cell.type === 'system' ||
      (cell.type === 'assistant' && (cell.content.trim() || cell.thinking));

    if (breaksGroup && pendingToolCell) {
      cells.push(pendingToolCell);
      pendingToolCell = null;
    }

    // Outside a tool group, push normally. Inside one, absorb noise.
    if (!pendingToolCell) {
      cells.push(cell);
    }
  }

  // Flush any trailing tool group
  if (pendingToolCell) {
    cells.push(pendingToolCell);
  }

  const ms = performance.now() - t0;
  if (ms > 10) {
    console.warn(`[Conversation] notebookCells derivation (${cells.length} cells) took ${ms.toFixed(1)}ms`);
  }
  return cells;
}

/** Transform turns into Observable-style notebook cells, aggregating consecutive progress entries */
export const notebookCells = derived(conversationTurns, turnsToNotebookCells);

/** Just the tool cells for quick access */
export const allToolCells = derived(notebookCells, ($cells): ToolCell[] => {
  return $cells.flatMap((cell) => cell.toolCells ?? []);
});

/** Group by tool type for stats */
export const toolStats = derived(allToolCells, ($tools) => {
  const counts = new Map<string, number>();
  $tools.forEach((tool) => {
    counts.set(tool.name, (counts.get(tool.name) ?? 0) + 1);
  });
  return counts;
});

// =============================================================================
// Helpers
// =============================================================================

function isRerunnable(toolName: string): boolean {
  const rerunnableTools = ['Read', 'Glob', 'Grep', 'Bash', 'WebFetch', 'WebSearch'];
  return rerunnableTools.includes(toolName);
}

// =============================================================================
// Actions (require instanceId - websocket.ts passes this)
// =============================================================================

/** Set conversation for a specific instance */
export function setConversation(instanceId: string, turns: ConversationTurn[]): void {
  const t0 = performance.now();
  setInstanceConversation(instanceId, turns);
  const ms = performance.now() - t0;
  if (ms > 20) {
    console.warn(`[Conversation] setConversation(${turns.length} turns) took ${ms.toFixed(1)}ms`);
  }
}

/** Append turns to a specific instance's conversation */
export function appendTurns(instanceId: string, newTurns: ConversationTurn[]): void {
  if (newTurns.length === 0) return;
  const t0 = performance.now();
  appendInstanceTurns(instanceId, newTurns);
  const ms = performance.now() - t0;
  if (ms > 20) {
    console.warn(`[Conversation] appendTurns(${newTurns.length} turns) took ${ms.toFixed(1)}ms`);
  }
}

/** Set waiting state for a specific instance */
export function setWaiting(instanceId: string, waiting: boolean): void {
  setInstanceWaiting(instanceId, waiting);
}

// Note: clearConversation is removed - no longer needed.
// Switching instances automatically shows that instance's preserved conversation.
// To clear an instance's conversation, call setConversation(instanceId, []).

// =============================================================================
// Per-Instance Derived Stores (for pane-bound components)
// =============================================================================

/**
 * Create a derived store of conversation turns for a specific instance.
 * Unlike `conversationTurns` which follows `currentInstanceId`, this is
 * bound to a fixed instance.
 */
export function conversationTurnsForInstance(instanceId: string) {
  return derived(instanceStates, ($states): ConversationTurn[] => {
    return $states.get(instanceId)?.conversation ?? [];
  });
}

/**
 * Create a derived store of notebook cells for a specific instance.
 */
export function notebookCellsForInstance(instanceId: string) {
  const turns = conversationTurnsForInstance(instanceId);
  return derived(turns, turnsToNotebookCells);
}

/**
 * Create a derived store of waiting state for a specific instance.
 */
export function isWaitingForInstance(instanceId: string) {
  return derived(instanceStates, ($states): boolean => {
    return $states.get(instanceId)?.isWaiting ?? true;
  });
}
