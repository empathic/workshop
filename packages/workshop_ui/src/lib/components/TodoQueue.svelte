<script lang="ts">
  import {
    currentInstanceTasks,
    currentInstanceTaskCount,
    deleteTask,
    reorderTask,
    stageTask,
    clearInstanceTasks
  } from '$lib/stores/tasks';
  import { currentInstanceId } from '$lib/stores/instances';
  import { openTaskForEdit } from '$lib/stores/tasks';

  let expanded = $state(false);

  function handleSendNext() {
    if (!$currentInstanceId) return;
    const next = $currentInstanceTasks[0];
    if (next) stageTask(next.id);
  }

  function handleRemove(taskId: number) {
    deleteTask(taskId);
  }

  function handleClear() {
    if (!$currentInstanceId) return;
    clearInstanceTasks($currentInstanceId);
  }

  // --- Drag and drop reorder ---
  let dragId = $state<number | null>(null);
  let dragOverId = $state<number | null>(null);

  function handleDragStart(e: DragEvent, taskId: number) {
    dragId = taskId;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = 'move';
    }
  }

  function handleDragOver(e: DragEvent, taskId: number) {
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
    dragOverId = taskId;
  }

  function handleDrop(e: DragEvent, targetId: number) {
    e.preventDefault();
    if (!dragId || !$currentInstanceId || dragId === targetId) {
      dragId = null;
      dragOverId = null;
      return;
    }
    const targetIndex = $currentInstanceTasks.findIndex((i) => i.id === targetId);
    if (targetIndex !== -1) {
      reorderTask($currentInstanceId, dragId, targetIndex);
    }
    dragId = null;
    dragOverId = null;
  }

  function handleDragEnd() {
    dragId = null;
    dragOverId = null;
  }
</script>

{#if $currentInstanceTaskCount > 0}
  <div class="todo-queue" class:expanded>
    <!-- Collapsed bar -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="queue-bar" onclick={() => (expanded = !expanded)}>
      <div class="queue-bar-left">
        <span class="queue-count">{$currentInstanceTaskCount} queued</span>
        <span class="queue-chevron">{expanded ? '\u25B4' : '\u25BE'}</span>
      </div>
      <div class="queue-bar-right">
        <button
          class="send-next-btn"
          onclick={(e) => {
            e.stopPropagation();
            handleSendNext();
          }}
          title="Load next task into input"
        >
          Load Next &rarr;
        </button>
        <button
          class="clear-btn"
          onclick={(e) => {
            e.stopPropagation();
            handleClear();
          }}
          title="Clear all queued items"
        >
          &times;
        </button>
      </div>
    </div>

    <!-- Expanded list -->
    {#if expanded}
      <div class="queue-list">
        {#each $currentInstanceTasks as item (item.id)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="queue-item"
            class:drag-over={dragOverId === item.id}
            draggable="true"
            ondragstart={(e) => handleDragStart(e, item.id)}
            ondragover={(e) => handleDragOver(e, item.id)}
            ondrop={(e) => handleDrop(e, item.id)}
            ondragend={handleDragEnd}
            onclick={() => openTaskForEdit(item.id)}
            title="Click to edit task"
          >
            <span class="drag-handle" title="Drag to reorder">&#x2630;</span>
            <span class="item-text">{item.title}</span>
            <button
              class="item-stage"
              onclick={(e) => {
                e.stopPropagation();
                stageTask(item.id);
              }}
              title="Load into input"
            >
              &#x25B6;
            </button>
            <button
              class="item-delete"
              onclick={(e) => {
                e.stopPropagation();
                handleRemove(item.id);
              }}
              title="Remove from queue"
            >
              &times;
            </button>
          </div>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .todo-queue {
    flex-shrink: 0;
    border-top: 1px solid var(--surface-border);
    background: linear-gradient(180deg, var(--surface-700) 0%, var(--surface-750, var(--surface-700)) 100%);
    animation: queue-slide-in 0.2s ease-out;
  }

  @keyframes queue-slide-in {
    from {
      opacity: 0;
      max-height: 0;
    }
    to {
      opacity: 1;
      max-height: 200px;
    }
  }

  /* --- Collapsed bar --- */
  .queue-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 16px;
    cursor: pointer;
    user-select: none;
    transition: background 0.15s ease;
  }

  .queue-bar:hover {
    background: var(--tint-hover);
  }

  .queue-bar-left {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .queue-count {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--chrome-accent-400);
    text-shadow: var(--emphasis);
  }

  .queue-chevron {
    font-size: 10px;
    color: var(--text-muted);
  }

  .queue-bar-right {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .send-next-btn {
    padding: 3px 10px;
    background: linear-gradient(180deg, var(--surface-500) 0%, var(--surface-600) 100%);
    border: 1px solid var(--chrome-accent-600);
    border-radius: 3px;
    color: var(--chrome-accent-400);
    font-size: 10px;
    font-weight: 600;
    font-family: inherit;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    cursor: pointer;
    transition: all 0.15s ease;
    white-space: nowrap;
  }

  .send-next-btn:hover {
    background: linear-gradient(180deg, var(--surface-400) 0%, var(--surface-500) 100%);
    border-color: var(--chrome-accent-500);
    color: var(--chrome-accent-300);
    box-shadow: var(--elevation-low);
  }

  .clear-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    background: none;
    border: 1px solid transparent;
    border-radius: 3px;
    color: var(--text-muted);
    font-size: 14px;
    font-family: inherit;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .clear-btn:hover {
    border-color: var(--status-red-border);
    color: var(--status-red);
  }

  /* --- Expanded list --- */
  .queue-list {
    border-top: 1px solid var(--surface-border);
    max-height: 180px;
    overflow-y: auto;
    animation: list-expand 0.15s ease-out;
  }

  @keyframes list-expand {
    from {
      max-height: 0;
      opacity: 0;
    }
    to {
      max-height: 180px;
      opacity: 1;
    }
  }

  .queue-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 16px;
    border-bottom: 1px solid var(--surface-border);
    transition: background 0.1s ease;
    cursor: pointer;
  }

  .queue-item:last-child {
    border-bottom: none;
  }

  .queue-item:hover {
    background: var(--tint-hover);
  }

  .queue-item.drag-over {
    background: var(--tint-active);
    border-top: 2px solid var(--chrome-accent-500);
  }

  .drag-handle {
    cursor: grab;
    color: var(--text-muted);
    font-size: 11px;
    opacity: 0.5;
    transition: opacity 0.15s ease;
    flex-shrink: 0;
    user-select: none;
  }

  .drag-handle:hover {
    opacity: 1;
  }

  .queue-item:active .drag-handle {
    cursor: grabbing;
  }

  .item-text {
    flex: 1;
    font-size: 11px;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
    font-family: var(--font-mono);
  }

  .item-stage {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    background: none;
    border: 1px solid transparent;
    border-radius: 2px;
    color: var(--text-muted);
    font-size: 8px;
    font-family: inherit;
    cursor: pointer;
    flex-shrink: 0;
    opacity: 0;
    transition: all 0.15s ease;
  }

  .queue-item:hover .item-stage {
    opacity: 1;
  }

  .item-stage:hover {
    border-color: var(--chrome-accent-600);
    color: var(--chrome-accent-400);
  }

  .item-delete {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    background: none;
    border: 1px solid transparent;
    border-radius: 2px;
    color: var(--text-muted);
    font-size: 14px;
    font-family: inherit;
    cursor: pointer;
    flex-shrink: 0;
    opacity: 0;
    transition: all 0.15s ease;
  }

  .queue-item:hover .item-delete {
    opacity: 1;
  }

  .item-delete:hover {
    border-color: var(--status-red-border);
    color: var(--status-red);
  }

  /* Scrollbar */
  .queue-list::-webkit-scrollbar {
    width: 4px;
  }

  .queue-list::-webkit-scrollbar-track {
    background: transparent;
  }

  .queue-list::-webkit-scrollbar-thumb {
    background: var(--surface-border);
    border-radius: 2px;
  }

  /* ============================================
	   ANALOG THEME — TodoQueue overrides
	   ============================================ */

  :global([data-theme='analog']) .todo-queue {
    background-color: var(--surface-700);
    background-image: var(--grain-fine);
    background-blend-mode: multiply;
    border-top-width: 2px;
  }

  :global([data-theme='analog']) .queue-count {
    font-family: 'Source Serif 4', Georgia, serif;
    font-style: italic;
    text-transform: none;
    letter-spacing: 0;
  }

  :global([data-theme='analog']) .send-next-btn {
    font-family: 'Source Serif 4', Georgia, serif;
    text-transform: none;
    letter-spacing: 0;
    border-width: 1.5px;
  }

  :global([data-theme='analog']) .item-text {
    font-family: 'Source Serif 4', Georgia, serif;
  }
</style>
