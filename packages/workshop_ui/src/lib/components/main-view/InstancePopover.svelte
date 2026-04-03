<script lang="ts">
  import type { Instance } from '$lib/types';
  import type { StateInfo } from '$lib/utils/instance-state';
  import { setCustomName, deleteInstance, selectInstance } from '$lib/stores/instances';
  import type { PaneContent } from '$lib/stores/layout';
  import { layoutState, focusPane, splitPane, getPaneInstanceId } from '$lib/stores/layout';

  interface Props {
    instance: Instance;
    stateInfo: StateInfo;
    anchorRect: DOMRect;
    onclose: () => void;
  }

  let { instance, stateInfo, anchorRect, onclose }: Props = $props();

  // Inline rename state
  let isRenaming = $state(false);
  let renameValue = $state('');

  // Delete confirmation state
  let confirmDelete = $state(false);

  function startRename() {
    isRenaming = true;
    renameValue = instance.custom_name ?? instance.name;
  }

  async function commitRename() {
    const trimmed = renameValue.trim();
    if (trimmed && trimmed !== instance.name) {
      await setCustomName(instance.id, trimmed);
    } else if (trimmed === instance.name) {
      // Clear custom name if it matches the default
      await setCustomName(instance.id, null);
    }
    isRenaming = false;
  }

  function cancelRename() {
    isRenaming = false;
  }

  function handleRenameKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      commitRename();
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      cancelRename();
    }
  }

  // Find panes showing this instance
  const panesWithInstance = $derived.by(() => {
    const result: { paneId: string; label: string }[] = [];
    const state = $layoutState;
    let idx = 0;
    for (const [paneId, pane] of state.panes) {
      idx++;
      if (getPaneInstanceId(pane.content) === instance.id) {
        result.push({ paneId, label: `Pane ${idx}` });
      }
    }
    return result;
  });

  function handleFocusPane(paneId: string) {
    focusPane(paneId);
    onclose();
  }

  function handleOpenInFocused() {
    selectInstance(instance.id, false);
    onclose();
  }

  function handleOpenInSplit() {
    const focusedId = $layoutState.focusedPaneId;
    const content: PaneContent =
      instance.kind.type === 'Structured'
        ? { kind: 'conversation', instanceId: instance.id, viewMode: 'structured' as const }
        : { kind: 'terminal', instanceId: instance.id };
    splitPane(focusedId, 'vertical', content);
    onclose();
  }

  function handleDelete() {
    if (!confirmDelete) {
      confirmDelete = true;
      return;
    }
    deleteInstance(instance.id);
    onclose();
  }

  // Position: below anchor, left-aligned, clamped to viewport
  const popoverStyle = $derived.by(() => {
    const top = anchorRect.bottom + 4;
    let left = anchorRect.left;
    // Clamp to viewport (assume popover ~200px wide)
    if (typeof window !== 'undefined' && left + 200 > window.innerWidth) {
      left = window.innerWidth - 210;
    }
    return `top: ${top}px; left: ${left}px`;
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="popover-backdrop" onclick={onclose} onkeydown={(e) => e.key === 'Escape' && onclose()}></div>
<div class="instance-popover" style={popoverStyle}>
  <!-- Instance name / rename -->
  <div class="popover-header">
    {#if isRenaming}
      <input
        class="rename-input"
        type="text"
        bind:value={renameValue}
        onkeydown={handleRenameKeydown}
        onblur={commitRename}
      />
    {:else}
      <button class="popover-name" onclick={startRename} title="Click to rename">
        <span class="name-text">{instance.custom_name ?? instance.name}</span>
        <svg class="edit-icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5">
          <path d="M11.5 1.5l3 3-9 9H2.5v-3l9-9z" />
        </svg>
      </button>
    {/if}
    {#if stateInfo.label}
      <span class="popover-state" style="color: {stateInfo.color}">{stateInfo.label}</span>
    {/if}
  </div>

  <div class="popover-divider"></div>

  <!-- Focus pane(s) showing this instance -->
  {#if panesWithInstance.length > 0}
    {#each panesWithInstance as { paneId, label }}
      <button class="popover-item" onclick={() => handleFocusPane(paneId)}>
        Focus {label}
      </button>
    {/each}
    <div class="popover-divider"></div>
  {/if}

  <!-- Open actions -->
  <button class="popover-item" onclick={handleOpenInFocused}> Open in focused pane </button>
  <button class="popover-item" onclick={handleOpenInSplit}> Open in new split </button>

  <div class="popover-divider"></div>

  <!-- Delete -->
  <button class="popover-item danger" onclick={handleDelete}>
    {confirmDelete ? 'Confirm delete?' : 'Delete instance'}
  </button>
</div>

<style>
  .popover-backdrop {
    position: fixed;
    inset: 0;
    z-index: 69;
  }

  .instance-popover {
    position: fixed;
    z-index: 70;
    min-width: 190px;
    background: var(--surface-600);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    box-shadow: var(--shadow-dropdown);
    padding: 4px 0;
    animation: popover-pop 0.12s ease-out;
  }

  @keyframes popover-pop {
    from {
      opacity: 0;
      transform: scale(0.95) translateY(-4px);
    }
    to {
      opacity: 1;
      transform: scale(1) translateY(0);
    }
  }

  .popover-header {
    padding: 6px 10px;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .popover-name {
    display: flex;
    align-items: center;
    gap: 4px;
    background: transparent;
    border: none;
    color: var(--text-primary);
    font-size: 11px;
    font-weight: 700;
    font-family: inherit;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    cursor: pointer;
    padding: 0;
  }

  .popover-name:hover {
    color: var(--chrome-accent-400);
  }

  .edit-icon {
    width: 10px;
    height: 10px;
    opacity: 0.4;
  }

  .popover-name:hover .edit-icon {
    opacity: 1;
  }

  .name-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 140px;
  }

  .rename-input {
    flex: 1;
    background: var(--surface-700);
    border: 1px solid var(--chrome-accent-600);
    border-radius: 2px;
    color: var(--text-primary);
    font-size: 11px;
    font-weight: 700;
    font-family: inherit;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    padding: 2px 6px;
    outline: none;
  }

  .popover-state {
    font-size: 9px;
    font-weight: 600;
    letter-spacing: 0.03em;
    opacity: 0.8;
    margin-left: auto;
  }

  .popover-divider {
    height: 1px;
    margin: 3px 0;
    background: var(--surface-border);
  }

  .popover-item {
    display: block;
    width: 100%;
    padding: 5px 10px;
    background: transparent;
    border: none;
    color: var(--text-secondary);
    font-size: 11px;
    font-weight: 600;
    font-family: inherit;
    letter-spacing: 0.03em;
    cursor: pointer;
    transition: all 0.1s ease;
    text-align: left;
  }

  .popover-item:hover {
    background: var(--tint-active-strong);
    color: var(--chrome-accent-400);
  }

  .popover-item.danger {
    color: var(--status-red);
  }

  .popover-item.danger:hover {
    background: var(--status-red-tint);
    color: var(--status-red);
  }
</style>
