<script lang="ts">
  import { onMount } from 'svelte';
  import { base } from '$app/paths';
  import { tasks, tasksLoaded, fetchTasks } from '$lib/stores/tasks';
  import { instanceList } from '$lib/stores/instances';
  import TaskCard from './TaskCard.svelte';
  import TaskForm from './TaskForm.svelte';

  type StatusFilter = 'all' | 'pending' | 'in_progress' | 'completed' | 'cancelled';
  let statusFilter = $state<StatusFilter>('pending');
  let instanceFilter = $state<string | 'all' | 'unassigned'>('all');
  let searchQuery = $state('');
  let showCreateForm = $state(false);

  const filteredTasks = $derived(() => {
    let result = $tasks;
    if (statusFilter !== 'all') {
      result = result.filter((t) => t.status === statusFilter);
    }
    if (instanceFilter === 'unassigned') {
      result = result.filter((t) => !t.instance_id);
    } else if (instanceFilter !== 'all') {
      result = result.filter((t) => t.instance_id === instanceFilter);
    }
    if (searchQuery.trim()) {
      const q = searchQuery.toLowerCase();
      result = result.filter((t) => t.title.toLowerCase().includes(q) || (t.body && t.body.toLowerCase().includes(q)));
    }
    return result.sort((a, b) => a.sort_order - b.sort_order);
  });

  const statusCounts = $derived(() => {
    const counts = { all: 0, pending: 0, in_progress: 0, completed: 0, cancelled: 0 };
    for (const t of $tasks) {
      counts.all++;
      if (t.status in counts) {
        counts[t.status as keyof typeof counts]++;
      }
    }
    return counts;
  });

  onMount(() => {
    if (!$tasksLoaded) {
      fetchTasks();
    }
  });
</script>

<div class="tasks-page">
  <header class="tasks-header">
    <a href="{base}/" class="back-link">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M19 12H5M12 19l-7-7 7-7" />
      </svg>
      Back
    </a>
    <h1>Tasks</h1>
    <div class="header-actions">
      <button class="refresh-btn" onclick={() => fetchTasks()} aria-label="Refresh tasks">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path
            d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
          />
        </svg>
      </button>
      <button class="create-btn" onclick={() => (showCreateForm = !showCreateForm)} class:active={showCreateForm}>
        <span class="create-icon">{showCreateForm ? '\u2212' : '+'}</span>
        New Task
      </button>
    </div>
  </header>

  {#if showCreateForm}
    <TaskForm instanceList={$instanceList} oncreated={() => (showCreateForm = false)} />
  {/if}

  <div class="filter-bar">
    <div class="status-tabs">
      {#each ['pending', 'in_progress', 'completed', 'cancelled', 'all'] as status}
        <button
          class="status-tab"
          class:active={statusFilter === status}
          onclick={() => (statusFilter = status as StatusFilter)}
        >
          {status}
          <span class="tab-count">{statusCounts()[status as keyof ReturnType<typeof statusCounts>]}</span>
        </button>
      {/each}
    </div>
    <div class="filter-controls">
      <select class="filter-select" bind:value={instanceFilter}>
        <option value="all">All instances</option>
        <option value="unassigned">Unassigned</option>
        {#each $instanceList as inst}
          <option value={inst.id}>{inst.custom_name ?? inst.name}</option>
        {/each}
      </select>
      <div class="search-wrap">
        <svg class="search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="11" cy="11" r="8" />
          <path d="m21 21-4.35-4.35" />
        </svg>
        <input type="text" class="search-input" placeholder="Filter..." bind:value={searchQuery} />
      </div>
    </div>
  </div>

  <div class="tasks-content">
    {#if !$tasksLoaded}
      <div class="loading-state">
        <div class="spinner"></div>
        <span>Loading tasks...</span>
      </div>
    {:else if filteredTasks().length === 0}
      <div class="empty-state">
        <div class="empty-icon">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
            <path
              d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2"
            />
            <path d="M9 14l2 2 4-4" />
          </svg>
        </div>
        <h2>No tasks</h2>
        <p>
          {#if statusFilter !== 'all' || instanceFilter !== 'all' || searchQuery}
            No tasks match your filters
          {:else}
            Create a task to queue work for Claude
          {/if}
        </p>
      </div>
    {:else}
      <div class="task-list">
        {#each filteredTasks() as task (task.id)}
          <TaskCard {task} instanceList={$instanceList} />
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .tasks-page {
    display: flex;
    flex-direction: column;
    height: 100vh;
    height: 100dvh;
    background: var(--surface-800);
  }

  .tasks-header {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 16px 20px;
    background: linear-gradient(180deg, var(--surface-600) 0%, var(--surface-700) 100%);
    border-bottom: 1px solid var(--surface-border);
    flex-shrink: 0;
  }

  .back-link {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 12px;
    background: transparent;
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-decoration: none;
    text-transform: uppercase;
    transition: all 0.15s ease;
  }

  .back-link:hover {
    border-color: var(--chrome-accent-600);
    color: var(--chrome-accent-400);
    background: var(--tint-hover);
  }

  .back-link svg {
    width: 14px;
    height: 14px;
  }

  .tasks-header h1 {
    flex: 1;
    margin: 0;
    font-size: 14px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--chrome-accent-400);
    text-shadow: var(--emphasis-strong);
    font-family: var(--font-display);
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .refresh-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    background: transparent;
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .refresh-btn:hover {
    border-color: var(--chrome-accent-600);
    color: var(--chrome-accent-400);
  }

  .refresh-btn svg {
    width: 16px;
    height: 16px;
  }

  .create-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 14px;
    background: linear-gradient(180deg, var(--surface-500) 0%, var(--surface-600) 100%);
    border: 1px solid var(--chrome-accent-600);
    border-radius: 4px;
    color: var(--chrome-accent-400);
    font-size: 12px;
    font-weight: 600;
    font-family: inherit;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    cursor: pointer;
    transition: all 0.15s ease;
    box-shadow: var(--depth-up);
  }

  .create-btn:hover {
    background: linear-gradient(180deg, var(--surface-400) 0%, var(--surface-500) 100%);
    border-color: var(--chrome-accent-500);
    color: var(--chrome-accent-300);
    box-shadow: var(--elevation-high);
  }

  .create-btn.active {
    background: var(--tint-active-strong);
    border-color: var(--chrome-accent-500);
  }

  .create-icon {
    font-size: 16px;
    font-weight: 400;
    line-height: 1;
  }

  .filter-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 20px;
    background: var(--surface-700);
    border-bottom: 1px solid var(--surface-border);
    flex-shrink: 0;
    flex-wrap: wrap;
  }

  .status-tabs {
    display: flex;
    gap: 2px;
  }

  .status-tab {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 5px 10px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 3px;
    color: var(--text-muted);
    font-size: 11px;
    font-weight: 600;
    font-family: inherit;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .status-tab:hover {
    color: var(--text-secondary);
    background: var(--tint-hover);
  }

  .status-tab.active {
    color: var(--chrome-accent-400);
    background: var(--tint-active);
    border-color: var(--surface-border);
  }

  .tab-count {
    font-size: 10px;
    font-weight: 700;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }

  .status-tab.active .tab-count {
    color: var(--chrome-accent-500);
  }

  .filter-controls {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .filter-select {
    padding: 5px 8px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    color: var(--text-primary);
    font-size: 11px;
    font-family: inherit;
    outline: none;
    cursor: pointer;
  }

  .filter-select:focus {
    border-color: var(--chrome-accent-600);
  }

  .search-wrap {
    position: relative;
    display: flex;
    align-items: center;
  }

  .search-icon {
    position: absolute;
    left: 8px;
    width: 13px;
    height: 13px;
    color: var(--text-muted);
    pointer-events: none;
  }

  .search-input {
    width: 140px;
    padding: 5px 8px 5px 26px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    color: var(--text-primary);
    font-size: 11px;
    font-family: inherit;
    outline: none;
    transition: all 0.15s ease;
  }

  .search-input::placeholder {
    color: var(--text-muted);
  }

  .search-input:focus {
    border-color: var(--chrome-accent-600);
    width: 200px;
  }

  .tasks-content {
    flex: 1;
    overflow-y: auto;
    padding: 16px 20px;
  }

  .loading-state,
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-muted);
    text-align: center;
  }

  .spinner {
    width: 24px;
    height: 24px;
    border: 2px solid var(--surface-border);
    border-top-color: var(--chrome-accent-500);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    margin-bottom: 16px;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .empty-icon {
    width: 64px;
    height: 64px;
    margin-bottom: 16px;
    opacity: 0.3;
    color: var(--chrome-accent-500);
  }

  .empty-icon svg {
    width: 100%;
    height: 100%;
  }

  .empty-state h2 {
    margin: 0 0 8px;
    font-size: 14px;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-secondary);
  }

  .empty-state p {
    margin: 0;
    font-size: 12px;
  }

  .task-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .tasks-content::-webkit-scrollbar {
    width: 8px;
  }

  .tasks-content::-webkit-scrollbar-track {
    background: transparent;
  }

  .tasks-content::-webkit-scrollbar-thumb {
    background: var(--surface-border);
    border-radius: 4px;
  }

  .tasks-content::-webkit-scrollbar-thumb:hover {
    background: var(--chrome-accent-600);
  }

  @media (max-width: 639px) {
    .tasks-header {
      padding: 12px 14px;
      gap: 10px;
    }

    .tasks-header h1 {
      font-size: 12px;
    }

    .filter-bar {
      padding: 8px 14px;
      flex-direction: column;
      align-items: stretch;
    }

    .status-tabs {
      overflow-x: auto;
    }

    .filter-controls {
      flex-wrap: wrap;
    }

    .search-input {
      width: 100%;
    }

    .search-input:focus {
      width: 100%;
    }

    .tasks-content {
      padding: 12px 14px;
    }
  }

  :global([data-theme='analog']) .tasks-header {
    background-color: var(--surface-800);
    background-image: var(--grain-fine), var(--grain-coarse);
    background-blend-mode: multiply, multiply;
    border-bottom-width: 2px;
  }

  :global([data-theme='analog']) .tasks-header h1 {
    font-family: 'Newsreader', Georgia, serif;
    text-transform: none;
    font-size: 18px;
    font-weight: 600;
    letter-spacing: 0;
  }

  :global([data-theme='analog']) .filter-bar {
    background-color: var(--surface-800);
    background-image: var(--grain-fine);
    background-blend-mode: multiply;
    border-bottom-width: 2px;
  }

  :global([data-theme='analog']) .status-tab {
    font-family: 'Source Serif 4', Georgia, serif;
    text-transform: none;
    letter-spacing: 0;
  }

  :global([data-theme='analog']) .create-btn {
    font-family: 'Source Serif 4', Georgia, serif;
    text-transform: none;
    letter-spacing: 0;
    border-width: 1.5px;
  }

  :global([data-theme='analog']) .back-link {
    font-family: 'Source Serif 4', Georgia, serif;
    text-transform: none;
    letter-spacing: 0;
  }

  :global([data-theme='analog']) .empty-state h2 {
    font-family: 'Newsreader', Georgia, serif;
    text-transform: none;
    letter-spacing: 0;
  }
</style>
