<script lang="ts">
  import {
    isTaskPanelOpen,
    closeTaskPanel,
    focusedTaskId,
    tasks,
    pendingTasks,
    createTask,
    updateTask,
    deleteTask,
    stageTask,
    fetchTasks
  } from '$lib/stores/tasks';
  import { instanceList, currentInstanceId, selectInstance } from '$lib/stores/instances';
  import { isDesktop } from '$lib/stores/ui';
  import type { Task } from '$lib/types';
  import TaskCard from './task-panel/TaskCard.svelte';
  import InProgressCard from './task-panel/InProgressCard.svelte';
  import InsetButton from './InsetButton.svelte';

  interface Props {
    embedded?: boolean;
    oninset?: () => void;
  }

  let { embedded = false, oninset }: Props = $props();

  const isVisible = $derived(embedded || $isTaskPanelOpen);

  // --- Sent task expand ---
  let expandedSentId = $state<number | null>(null);

  function toggleSentExpand(id: number) {
    expandedSentId = expandedSentId === id ? null : id;
  }

  function navigateToInstance(instanceId: string) {
    selectInstance(instanceId);
    closeTaskPanel();
  }

  // --- View filter ---
  type ViewMode = 'instance' | 'all' | 'unassigned';
  let viewMode = $state<ViewMode>('instance');

  // --- Quick-add ---
  let addInput = $state('');
  let addBodyExpanded = $state(false);
  let addBody = $state('');
  let isAdding = $state(false);

  // --- Inline edit ---
  let editingId = $state<number | null>(null);
  let editTitle = $state('');
  let editBody = $state('');

  // --- Expand detail ---
  let expandedId = $state<number | null>(null);

  // --- Auto-expand and edit focused task from external navigation ---
  $effect(() => {
    const id = $focusedTaskId;
    if (id !== null) {
      const task = $tasks.find((t) => t.id === id);
      if (task) {
        startEdit(task);
      }
    }
  });

  // --- Filtered list ---
  const visibleTasks = $derived(() => {
    let result: Task[];
    if (viewMode === 'instance' && $currentInstanceId) {
      result = $tasks.filter((t) => t.instance_id === $currentInstanceId && t.status === 'pending');
    } else if (viewMode === 'unassigned') {
      result = $tasks.filter((t) => !t.instance_id && t.status === 'pending');
    } else {
      result = $tasks.filter((t) => t.status === 'pending');
    }
    return result.sort((a, b) => a.sort_order - b.sort_order);
  });

  const inProgressTasks = $derived(() => {
    if (viewMode === 'instance' && $currentInstanceId) {
      return $tasks.filter((t) => t.instance_id === $currentInstanceId && t.status === 'in_progress');
    }
    return $tasks.filter((t) => t.status === 'in_progress');
  });

  // --- Instance name helper ---
  function instanceName(id: string | null): string {
    if (!id) return 'none';
    const inst = $instanceList.find((i) => i.id === id);
    return inst?.custom_name ?? inst?.name ?? id.slice(0, 8);
  }

  // --- Quick-add handler ---
  async function handleAdd() {
    const title = addInput.trim();
    if (!title || isAdding) return;
    isAdding = true;
    await createTask({
      title,
      body: addBody.trim() || undefined,
      instance_id: viewMode === 'instance' && $currentInstanceId ? $currentInstanceId : undefined
    });
    addInput = '';
    addBody = '';
    addBodyExpanded = false;
    isAdding = false;
  }

  function handleAddKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleAdd();
    }
  }

  // --- Edit ---
  function startEdit(task: Task) {
    editingId = task.id;
    editTitle = task.title;
    editBody = task.body ?? '';
    expandedId = task.id;
  }

  async function commitEdit() {
    if (editingId === null) return;
    await updateTask(editingId, {
      title: editTitle.trim(),
      body: editBody.trim() || undefined
    });
    editingId = null;
  }

  function cancelEdit() {
    editingId = null;
  }

  function handleEditKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      commitEdit();
    } else if (e.key === 'Escape') {
      cancelEdit();
    }
  }

  // --- Expand/collapse ---
  function toggleExpand(id: number) {
    expandedId = expandedId === id ? null : id;
  }

  // --- Actions ---
  function handleSend(taskId: number) {
    stageTask(taskId);
    closeTaskPanel();
  }

  async function handleAssign(taskId: number, instanceId: string) {
    await updateTask(taskId, { instance_id: instanceId || undefined });
  }

  async function handleComplete(taskId: number) {
    await updateTask(taskId, { status: 'completed' });
  }

  async function handleDelete(taskId: number) {
    await deleteTask(taskId);
  }

  function formatTime(ts: number): string {
    const d = new Date(ts * 1000);
    const now = new Date();
    const diffMs = now.getTime() - d.getTime();
    const diffMins = Math.floor(diffMs / 60000);
    if (diffMins < 1) return 'now';
    if (diffMins < 60) return `${diffMins}m`;
    const diffHours = Math.floor(diffMins / 60);
    if (diffHours < 24) return `${diffHours}h`;
    return `${Math.floor(diffHours / 24)}d`;
  }
</script>

{#if isVisible}
  <!-- Mobile backdrop -->
  {#if !embedded && !$isDesktop}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="task-backdrop" onclick={closeTaskPanel}></div>
  {/if}

  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <aside class="task-panel" class:embedded onkeydown={(e) => !embedded && e.key === 'Escape' && closeTaskPanel()}>
    <!-- Header -->
    <header class="panel-header">
      <h2>Tasks</h2>
      <div class="header-actions">
        <div class="view-tabs">
          {#if $currentInstanceId}
            <button
              class="view-tab"
              class:active={viewMode === 'instance'}
              onclick={() => (viewMode = 'instance')}
              title="Current instance tasks"
            >
              This
            </button>
          {/if}
          <button
            class="view-tab"
            class:active={viewMode === 'all'}
            onclick={() => (viewMode = 'all')}
            title="All pending tasks"
          >
            All
          </button>
          <button
            class="view-tab"
            class:active={viewMode === 'unassigned'}
            onclick={() => (viewMode = 'unassigned')}
            title="Unassigned tasks"
          >
            Free
          </button>
        </div>
        {#if !embedded && oninset}
          <InsetButton onclick={oninset} />
        {/if}
        {#if !embedded}
          <button class="close-btn" onclick={closeTaskPanel} aria-label="Close task panel">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        {/if}
      </div>
    </header>

    <!-- Quick-add -->
    <div class="quick-add">
      <div class="add-row">
        <input
          type="text"
          class="add-input"
          placeholder="Add task..."
          bind:value={addInput}
          onkeydown={handleAddKeydown}
        />
        {#if addInput.trim()}
          <button
            class="expand-body-btn"
            class:active={addBodyExpanded}
            onclick={() => (addBodyExpanded = !addBodyExpanded)}
            title="Add body text"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14">
              <path d="M4 6h16M4 12h16M4 18h10" />
            </svg>
          </button>
          <button class="add-btn" onclick={handleAdd} disabled={isAdding}>
            {isAdding ? '...' : '+'}
          </button>
        {/if}
      </div>
      {#if addBodyExpanded && addInput.trim()}
        <textarea class="add-body" placeholder="Full prompt body..." bind:value={addBody} rows="3"></textarea>
      {/if}
    </div>

    <!-- Task list -->
    <div class="task-list">
      {#if visibleTasks().length === 0 && inProgressTasks().length === 0}
        <div class="empty-state">
          <p>No pending tasks</p>
          {#if viewMode === 'instance' && $currentInstanceId}
            <p class="empty-hint">Add one above, or switch to "All"</p>
          {/if}
        </div>
      {/if}

      {#each visibleTasks() as task (task.id)}
        <TaskCard
          {task}
          expanded={expandedId === task.id}
          editing={editingId === task.id}
          {editTitle}
          {editBody}
          instanceList={$instanceList}
          ontoggle={() => toggleExpand(task.id)}
          oncomplete={() => handleComplete(task.id)}
          onsend={() => handleSend(task.id)}
          onedit={() => startEdit(task)}
          ondelete={() => handleDelete(task.id)}
          onassign={(instanceId) => handleAssign(task.id, instanceId)}
          onedittitlechange={(v) => (editTitle = v)}
          oneditbodychange={(v) => (editBody = v)}
          oneditkeydown={handleEditKeydown}
          oneditcommit={commitEdit}
          oneditcancel={cancelEdit}
          {formatTime}
          {instanceName}
        />
      {/each}

      <!-- In-progress tasks -->
      {#if inProgressTasks().length > 0}
        <div class="section-divider">
          <span class="section-label">In Progress ({inProgressTasks().length})</span>
        </div>
        {#each inProgressTasks() as task (task.id)}
          <InProgressCard
            {task}
            expanded={expandedSentId === task.id}
            ontoggle={() => toggleSentExpand(task.id)}
            onnavigate={navigateToInstance}
            onsend={() => handleSend(task.id)}
            oncomplete={() => handleComplete(task.id)}
            onedit={() => startEdit(task)}
            {formatTime}
            {instanceName}
          />
        {/each}
      {/if}
    </div>
  </aside>
{/if}

<style>
  /* === Backdrop (mobile) === */
  .task-backdrop {
    position: fixed;
    inset: 0;
    background: var(--backdrop);
    backdrop-filter: blur(2px);
    z-index: 55;
    animation: task-fade-in 0.2s ease;
  }

  @keyframes task-fade-in {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }

  /* === Panel === */
  .task-panel {
    position: fixed;
    top: 0;
    left: 48px;
    bottom: 0;
    width: 340px;
    z-index: 56;
    display: flex;
    flex-direction: column;
    background: linear-gradient(180deg, var(--surface-700) 0%, var(--surface-800) 100%);
    border-right: 1px solid var(--surface-border);
    box-shadow:
      var(--shadow-panel),
      1px 0 0 var(--tint-active);
    animation: task-slide-in 0.25s cubic-bezier(0.4, 0, 0.2, 1);
  }

  .task-panel.embedded {
    position: relative;
    top: auto;
    left: auto;
    bottom: auto;
    width: 100%;
    height: 100%;
    z-index: auto;
    box-shadow: none;
    animation: none;
    border-right: none;
  }

  @keyframes task-slide-in {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(0);
    }
  }

  /* === Header === */
  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 16px;
    border-bottom: 1px solid var(--surface-border);
    background: var(--panel-inset);
    flex-shrink: 0;
  }

  .panel-header h2 {
    margin: 0;
    font-size: 13px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--chrome-accent-400);
    text-shadow: var(--emphasis);
    font-family: var(--font-display);
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .view-tabs {
    display: flex;
    gap: 2px;
  }

  .view-tab {
    padding: 3px 8px;
    background: none;
    border: 1px solid transparent;
    border-radius: 3px;
    color: var(--text-muted);
    font-size: 10px;
    font-weight: 600;
    font-family: inherit;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .view-tab:hover {
    color: var(--text-secondary);
    background: var(--tint-hover);
  }

  .view-tab.active {
    color: var(--chrome-accent-400);
    background: var(--tint-active);
    border-color: var(--surface-border);
  }

  .close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: none;
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .close-btn:hover {
    border-color: var(--chrome-accent-600);
    color: var(--chrome-accent-400);
  }

  .close-btn svg {
    width: 14px;
    height: 14px;
  }

  /* === Quick-add === */
  .quick-add {
    padding: 10px 14px;
    border-bottom: 1px solid var(--surface-border);
    flex-shrink: 0;
  }

  .add-row {
    display: flex;
    gap: 6px;
    align-items: center;
  }

  .add-input {
    flex: 1;
    padding: 7px 10px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
    outline: none;
    transition: border-color 0.15s ease;
  }

  .add-input::placeholder {
    color: var(--text-muted);
  }

  .add-input:focus {
    border-color: var(--chrome-accent-600);
  }

  .expand-body-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: none;
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 0.15s ease;
    flex-shrink: 0;
  }

  .expand-body-btn:hover,
  .expand-body-btn.active {
    border-color: var(--chrome-accent-600);
    color: var(--chrome-accent-400);
  }

  .add-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: var(--btn-primary-bg);
    border: none;
    border-radius: 3px;
    color: var(--btn-primary-text);
    font-size: 16px;
    font-weight: 700;
    font-family: inherit;
    cursor: pointer;
    flex-shrink: 0;
    transition: all 0.15s ease;
  }

  .add-btn:hover:not(:disabled) {
    box-shadow: var(--elevation-low);
  }

  .add-btn:disabled {
    opacity: 0.5;
  }

  .add-body {
    width: 100%;
    margin-top: 8px;
    padding: 6px 10px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 11px;
    font-family: var(--font-mono);
    outline: none;
    resize: vertical;
    min-height: 36px;
    transition: border-color 0.15s ease;
  }

  .add-body::placeholder {
    color: var(--text-muted);
  }

  .add-body:focus {
    border-color: var(--chrome-accent-600);
  }

  /* === Task list === */
  .task-list {
    flex: 1;
    overflow-y: auto;
    padding: 6px 0;
  }

  .empty-state {
    padding: 32px 16px;
    text-align: center;
    color: var(--text-muted);
  }

  .empty-state p {
    margin: 0;
    font-size: 11px;
    letter-spacing: 0.05em;
  }

  .empty-hint {
    margin-top: 6px !important;
    font-size: 10px !important;
    opacity: 0.6;
  }

  /* === Section divider === */
  .section-divider {
    display: flex;
    align-items: center;
    padding: 8px 14px 4px;
  }

  .section-label {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  /* === Scrollbar === */
  .task-list::-webkit-scrollbar {
    width: 5px;
  }

  .task-list::-webkit-scrollbar-track {
    background: transparent;
  }

  .task-list::-webkit-scrollbar-thumb {
    background: var(--surface-border);
    border-radius: 3px;
  }

  .task-list::-webkit-scrollbar-thumb:hover {
    background: var(--chrome-accent-600);
  }

  /* === Responsive === */
  @media (max-width: 1023px) {
    .task-panel {
      left: 0;
      width: min(85vw, 380px);
      min-width: 280px;
    }
  }

  @media (max-width: 639px) {
    .task-panel {
      left: 0;
      width: 100%;
    }
  }

  /* ============================================
	   ANALOG THEME
	   ============================================ */

  :global([data-theme='analog']) .task-panel {
    background-color: var(--surface-800);
    background-image: var(--grain-fine), var(--grain-coarse);
    background-blend-mode: multiply, multiply;
    border-right-width: 2px;
    box-shadow: var(--shadow-panel);
  }

  :global([data-theme='analog']) .panel-header {
    background-color: var(--surface-800);
    background-image: var(--grain-coarse);
    background-blend-mode: multiply;
    border-bottom-width: 2px;
  }

  :global([data-theme='analog']) .panel-header h2 {
    font-family: 'Newsreader', Georgia, serif;
    text-transform: none;
    font-size: 16px;
    letter-spacing: 0;
  }

  :global([data-theme='analog']) .view-tab {
    font-family: 'Source Serif 4', Georgia, serif;
    text-transform: none;
    letter-spacing: 0;
  }

  :global([data-theme='analog']) .quick-add {
    background-color: var(--surface-700);
    background-image: var(--grain-fine);
    background-blend-mode: multiply;
    border-bottom-width: 2px;
  }
</style>
