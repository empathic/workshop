<script lang="ts">
  import type { NotebookCell } from '$lib/types';
  import { openTaskForEdit } from '$lib/stores/tasks';
  import TopoAvatar from './TopoAvatar.svelte';
  import ProgressCell from './notebook-cell/ProgressCell.svelte';
  import ToolBadges from './notebook-cell/ToolBadges.svelte';
  import CellContent from './notebook-cell/CellContent.svelte';

  interface Props {
    cell: NotebookCell;
    /** Instance ID for consistent avatar identity across sidebar and conversation */
    instanceId?: string | null;
    /** Distance from the newest message (0 = newest). Used for phosphor persistence. */
    age?: number;
    /** Whether this cell is keyboard-focused */
    focused?: boolean;
  }

  let { cell, instanceId = null, age = 0, focused = false }: Props = $props();

  // Phosphor persistence: recent messages burn bright, older ones cool down.
  const cellBrightness = $derived(Math.max(0.7, 1 - age * 0.015));

  const isUser = $derived(cell.type === 'user');
  const isSystem = $derived(cell.type === 'system');
  const isUnknown = $derived(cell.type === 'unknown');
  const isAssistant = $derived(cell.type === 'assistant');
  const isLongMessage = $derived(isAssistant && cell.content.length > 500);
  const isAgent = $derived(cell.type === 'agent');
  const isProgress = $derived(cell.type === 'progress');

  // Avatar properties
  const avatarType = $derived(isUser ? 'human' : 'agent');
  const avatarVariant = $derived(
    isUser ? 'user' : isSystem ? 'assistant' : isUnknown ? 'thinking' : isAgent ? 'thinking' : 'assistant'
  ) as 'user' | 'assistant' | 'thinking';
  const avatarIdentity = $derived(
    isUser
      ? (cell.attributed_to?.user_id ?? 'user')
      : isSystem
        ? 'system'
        : isUnknown
          ? 'unknown'
          : isAgent
            ? (cell.agentId ?? 'agent')
            : (instanceId ?? 'claude')
  );

  const hasTools = $derived(cell.toolCells && cell.toolCells.length > 0);

  let showRaw = $state(false);

  // Role display text
  const roleLabel = $derived(
    isUser
      ? (cell.attributed_to?.display_name ?? 'You')
      : isSystem
        ? 'System'
        : isUnknown
          ? (cell.entryType ?? 'Unknown')
          : isAgent
            ? `Agent ${cell.agentId?.slice(0, 7) ?? ''}`
            : 'Claude'
  );

  function formatTimestamp(isoString: string): string {
    try {
      const date = new Date(isoString);
      const now = new Date();
      const isToday = date.toDateString() === now.toDateString();

      const timeStr = date.toLocaleTimeString('en-US', {
        hour: '2-digit',
        minute: '2-digit',
        hour12: false
      });

      if (isToday) {
        return timeStr;
      }

      const dateStr = date.toLocaleDateString('en-US', {
        month: 'short',
        day: 'numeric'
      });
      return `${dateStr} ${timeStr}`;
    } catch {
      return '';
    }
  }
</script>

{#if isProgress}
  <ProgressCell {cell} />
{:else}
  <div
    class="cell"
    class:user-cell={isUser}
    class:assistant-cell={isAssistant}
    class:system-cell={isSystem}
    class:unknown-cell={isUnknown}
    class:agent-cell={isAgent}
    class:long-message={isLongMessage}
    class:focused
    data-cell-id={cell.id}
    style="--cell-brightness: {cellBrightness}"
  >
    <div class="cell-avatar">
      <TopoAvatar identity={avatarIdentity} type={avatarType} variant={avatarVariant} size={28} />
    </div>

    <div class="cell-body">
      <div class="cell-header">
        <span class="cell-role">{roleLabel}</span>
        <span class="cell-time">{formatTimestamp(cell.timestamp)}</span>
        {#if cell.task_id != null}
          <button class="task-link" onclick={() => openTaskForEdit(cell.task_id!)} title="Task #{cell.task_id}">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="11" height="11">
              <path
                d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2"
              />
            </svg>
            <span class="task-link-title">Task #{cell.task_id}</span>
          </button>
        {/if}
        <button class="toggle-raw" onclick={() => (showRaw = !showRaw)} title={showRaw ? 'Show rendered' : 'Show raw'}>
          {showRaw ? '◆' : '◇'}
        </button>
      </div>

      <CellContent {cell} {showRaw} cellType={cell.type} />

      {#if hasTools && cell.toolCells}
        <ToolBadges toolCells={cell.toolCells} agentLog={cell.agentLog} />
      {/if}
    </div>
  </div>
{/if}

<style>
  .cell {
    display: flex;
    gap: 12px;
    padding: 16px 20px;
    --cell-brightness: 1;
    --cell-rhythm-top: 0px;
    --cell-rhythm-bottom: 0px;
    padding-top: calc(16px + var(--cell-rhythm-top));
    padding-bottom: calc(16px + var(--cell-rhythm-bottom));
  }

  /* Conversation rhythm: user messages are "new thoughts" — they breathe */
  .cell.user-cell {
    --cell-rhythm-top: 8px;
  }

  /* Long assistant messages: let them land */
  .cell.long-message {
    --cell-rhythm-bottom: 8px;
  }

  /* Phosphor persistence: cell content fades with age */
  .cell .cell-body {
    opacity: var(--cell-brightness);
    transition: opacity 0.4s ease;
  }

  /* Phosphor re-excitation: warm up slightly on hover */
  .cell:hover .cell-body {
    opacity: calc(min(1, var(--cell-brightness) + 0.1));
  }

  .cell:hover {
    background: var(--tint-subtle);
  }

  /* Keyboard focus ring */
  .cell.focused {
    outline: 1px solid var(--accent-500);
    outline-offset: -1px;
    box-shadow: var(--elevation-low);
    background: var(--tint-subtle);
  }

  .cell-avatar {
    flex-shrink: 0;
  }

  .cell-body {
    flex: 1;
    min-width: 0;
  }

  .cell-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }

  .cell-role {
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-secondary);
  }

  .assistant-cell .cell-role {
    color: var(--accent-400);
    text-shadow: var(--emphasis);
    transition: text-shadow 0.8s ease;
  }

  .cell-time {
    font-size: 10px;
    color: var(--text-muted);
    font-family: var(--font-mono);
    letter-spacing: 0.05em;
    font-variant-numeric: tabular-nums;
  }

  .toggle-raw {
    margin-left: auto;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
    padding: 2px 6px;
    border-radius: 3px;
    opacity: 0.4;
    transition: all 0.15s ease;
  }

  .cell:hover .toggle-raw {
    opacity: 0.8;
  }

  .toggle-raw:hover {
    background: var(--surface-500);
    color: var(--accent-400);
  }

  /* Task link badge in cell header */
  .task-link {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 1px 7px 1px 5px;
    background: var(--tint-active);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-size: 10px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.15s ease;
    max-width: 160px;
    overflow: hidden;
    white-space: nowrap;
  }

  .task-link:hover {
    border-color: var(--accent-600);
    color: var(--accent-400);
    background: var(--tint-active-strong);
  }

  .task-link svg {
    flex-shrink: 0;
    opacity: 0.7;
  }

  .task-link:hover svg {
    opacity: 1;
  }

  .task-link-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* System cell styles */
  .system-cell .cell-role {
    color: var(--text-muted);
  }

  /* Unknown cell styles */
  .unknown-cell {
    opacity: 0.85;
  }

  .unknown-cell .cell-role {
    color: var(--text-muted);
    font-style: italic;
  }

  /* Agent cell styles */
  .agent-cell {
    opacity: 0.9;
  }

  .agent-cell .cell-role {
    color: var(--thinking-400);
    font-size: 10px;
  }

  /* Faint horizontal rule below user messages */
  .user-cell .cell-body::after {
    content: '';
    display: block;
    width: 60%;
    height: 1px;
    background: var(--surface-border);
    margin: 12px auto 0;
    opacity: 0.3;
  }

  /* Mobile responsive */
  @media (max-width: 639px) {
    .cell {
      gap: 10px;
      padding: 12px 14px;
    }

    .cell-avatar {
      transform: scale(0.85);
      transform-origin: top left;
    }

    .cell-header {
      gap: 8px;
      margin-bottom: 6px;
    }

    .cell-role {
      font-size: 10px;
    }

    .cell-time {
      font-size: 9px;
    }

    .toggle-raw {
      opacity: 0.6;
      padding: 4px 8px;
      font-size: 14px;
    }
  }

  /* Analog theme */
  :global([data-theme='analog']) .task-link {
    font-family: 'Source Serif 4', Georgia, serif;
    border-width: 1.5px;
  }
</style>
