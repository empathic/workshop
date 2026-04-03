<script lang="ts">
  import { notebookCells, isWaiting, toolStats } from '$lib/stores/conversation';
  import { drawerWidth, setDrawerWidth } from '$lib/stores/settings';
  import NotebookCell from './NotebookCell.svelte';

  export let onClose: () => void;

  let drawerEl: HTMLElement;
  let contentEl: HTMLElement;
  let isResizing = false;
  let startX = 0;
  let startWidth = 0;

  // Auto-scroll to bottom when new cells arrive
  $: if ($notebookCells.length > 0 && contentEl) {
    // Use tick to wait for DOM update
    setTimeout(() => {
      contentEl.scrollTop = contentEl.scrollHeight;
    }, 0);
  }

  function startResize(event: MouseEvent) {
    isResizing = true;
    startX = event.clientX;
    startWidth = $drawerWidth;
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
  }

  function handleMouseMove(event: MouseEvent) {
    if (!isResizing) return;
    const deltaX = startX - event.clientX;
    setDrawerWidth(startWidth + deltaX);
  }

  function stopResize() {
    if (isResizing) {
      isResizing = false;
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    }
  }

  // Format tool stats
  $: topTools = Array.from($toolStats.entries())
    .sort((a, b) => b[1] - a[1])
    .slice(0, 5);
</script>

<svelte:window on:mousemove={handleMouseMove} on:mouseup={stopResize} />

<aside class="drawer" bind:this={drawerEl} style="width: {$drawerWidth}px">
  <!-- Resize handle -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="resize-handle" on:mousedown={startResize}></div>

  <!-- Header -->
  <header class="drawer-header">
    <h3>Conversation</h3>
    <button class="close-btn" on:click={onClose} aria-label="Close drawer">&times;</button>
  </header>

  <!-- Stats bar -->
  {#if topTools.length > 0}
    <div class="stats-bar">
      {#each topTools as [name, count]}
        <span class="stat-item" title="{name}: {count}">
          {name}
          <span class="stat-count">{count}</span>
        </span>
      {/each}
    </div>
  {/if}

  <!-- Content -->
  <div class="drawer-content" bind:this={contentEl}>
    {#if $isWaiting}
      <div class="empty-state">
        <div class="spinner"></div>
        <p>Waiting for conversation...</p>
      </div>
    {:else if $notebookCells.length === 0}
      <div class="empty-state">
        <p>No messages yet</p>
      </div>
    {:else}
      <div class="notebook">
        {#each $notebookCells as cell (cell.id)}
          <NotebookCell {cell} />
        {/each}
      </div>
    {/if}
  </div>
</aside>

<style>
  .drawer {
    display: flex;
    flex-direction: column;
    background: var(--surface-900);
    border-left: 1px solid var(--surface-border);
    height: 100%;
    position: relative;
    min-width: 250px;
    max-width: 60vw;
  }

  .resize-handle {
    position: absolute;
    left: -4px;
    top: 0;
    bottom: 0;
    width: 8px;
    cursor: col-resize;
    background: transparent;
    transition: background 0.15s ease;
    z-index: 10;
  }

  .resize-handle:hover {
    background: var(--tint-selection);
  }

  .drawer-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid var(--surface-border);
    flex-shrink: 0;
  }

  .drawer-header h3 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--text-secondary);
    font-size: 24px;
    cursor: pointer;
    padding: 0;
    line-height: 1;
    transition: color 0.15s ease;
  }

  .close-btn:hover {
    color: var(--text-primary);
  }

  .stats-bar {
    display: flex;
    gap: 8px;
    padding: 8px 16px;
    border-bottom: 1px solid var(--surface-border);
    overflow-x: auto;
    flex-shrink: 0;
  }

  .stat-item {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 8px;
    background: var(--surface-700);
    border-radius: 4px;
    font-size: 11px;
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .stat-count {
    background: var(--surface-500);
    padding: 1px 5px;
    border-radius: 3px;
    font-weight: 600;
    color: var(--chrome-accent-400);
  }

  .drawer-content {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-muted);
    gap: 12px;
  }

  .spinner {
    width: 24px;
    height: 24px;
    border: 2px solid var(--surface-border);
    border-top-color: var(--chrome-accent-400);
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .notebook {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
  }

  /* Custom scrollbar */
  .drawer-content::-webkit-scrollbar {
    width: 6px;
  }

  .drawer-content::-webkit-scrollbar-track {
    background: var(--surface-700);
  }

  .drawer-content::-webkit-scrollbar-thumb {
    background: var(--surface-border);
    border-radius: 3px;
  }

  .drawer-content::-webkit-scrollbar-thumb:hover {
    background: var(--chrome-accent-600);
  }
</style>
