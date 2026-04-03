<script lang="ts">
  import type { Instance } from '$lib/types';
  import { setCustomName } from '$lib/stores/instances';
  import TopoAvatar from '../TopoAvatar.svelte';
  import BaudMeter from '../BaudMeter.svelte';

  interface StateInfo {
    label: string;
    color: string;
    animate: boolean;
    stale: boolean;
  }

  interface Props {
    instance: Instance;
    isActive: boolean;
    stateInfo: StateInfo;
    activityLevel: number;
    presenceCount: number;
    presenceNames: string;
    queueCount: number;
    onselect: () => void;
    ondelete: (e: MouseEvent) => void;
  }

  let {
    instance,
    isActive,
    stateInfo,
    activityLevel,
    presenceCount,
    presenceNames,
    queueCount,
    onselect,
    ondelete
  }: Props = $props();

  let editing = $state(false);
  let editValue = $state('');

  function startEditing(event: MouseEvent) {
    event.stopPropagation();
    editValue = instance.custom_name ?? instance.name;
    editing = true;
  }

  function commitEdit() {
    if (!editing) return;
    const trimmed = editValue.trim();
    const newName = !trimmed || trimmed === instance.name ? null : trimmed;
    setCustomName(instance.id, newName);
    editing = false;
  }

  function handleEditKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      event.preventDefault();
      commitEdit();
    } else if (event.key === 'Escape') {
      event.preventDefault();
      editing = false;
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="instance-item" class:active={isActive} onclick={onselect}>
  <div class="instance-avatar">
    <TopoAvatar
      identity={instance.id}
      type="agent"
      variant="assistant"
      size={24}
      animated={instance.running && isActive}
    />
  </div>
  <div class="instance-content">
    <div class="instance-info">
      {#if editing}
        <!-- svelte-ignore a11y_autofocus -->
        <input
          class="instance-name-input"
          type="text"
          bind:value={editValue}
          onblur={commitEdit}
          onkeydown={handleEditKeydown}
          onclick={(e) => e.stopPropagation()}
          autofocus
        />
      {:else}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <span class="instance-name" ondblclick={startEditing}>{instance.custom_name ?? instance.name}</span>
      {/if}
      {#if instance.running}
        <BaudMeter
          level={isActive ? activityLevel : 0}
          active={stateInfo.animate}
          color={stateInfo.color}
          stale={stateInfo.stale}
        />
      {:else}
        <span class="instance-status-dot stopped"></span>
      {/if}
    </div>
    <div class="instance-meta">
      {#if instance.custom_name}
        <span class="instance-unique-name">{instance.name}</span>
      {:else if stateInfo.label && !isActive}
        <span class="instance-state" class:stale={stateInfo.stale} style="color: {stateInfo.color}"
          >{stateInfo.label}</span
        >
      {:else}
        <span class="instance-command">{instance.command.split('/').pop()}</span>
      {/if}
      {#if presenceCount > 1}
        <span class="presence-count" title={presenceNames}>
          {presenceCount}
        </span>
      {/if}
      {#if queueCount > 0}
        <span class="queue-badge" title="{queueCount} queued">
          {queueCount}
        </span>
      {/if}
    </div>
  </div>
  <button class="delete-btn" onclick={ondelete} aria-label="Delete instance"> &times; </button>
</div>

<style>
  .instance-item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 10px 12px;
    margin-bottom: 4px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    cursor: pointer;
    transition: all 0.15s ease;
    position: relative;
  }

  .instance-avatar {
    flex-shrink: 0;
  }

  .instance-content {
    flex: 1;
    min-width: 0;
  }

  .instance-item:hover {
    background: var(--tint-hover);
    border-color: var(--surface-border);
  }

  .instance-item.active {
    background: linear-gradient(180deg, var(--tint-active) 0%, var(--tint-hover) 100%);
    border: var(--active-border);
    border-left-width: calc(1px + var(--active-accent-width));
    box-shadow: var(--depth-up);
  }

  .instance-info {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .instance-name {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    font-family: var(--font-display);
  }

  .instance-item.active .instance-name {
    color: var(--chrome-accent-400);
    text-shadow: var(--emphasis);
  }

  .instance-name-input {
    font-size: 12px;
    font-weight: 600;
    font-family: var(--font-mono);
    color: var(--text-primary);
    background: var(--surface-600);
    border: 1px solid var(--chrome-accent-600);
    border-radius: 2px;
    padding: 1px 4px;
    outline: none;
    width: 100%;
    min-width: 0;
  }

  .instance-unique-name {
    font-size: 10px;
    color: var(--text-muted);
    font-family: inherit;
    opacity: 0.7;
  }

  .instance-status-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--text-muted);
    transition: all 0.3s ease;
  }

  .instance-status-dot.stopped {
    background: var(--surface-border);
    opacity: 0.5;
  }

  .instance-meta {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-top: 2px;
  }

  .instance-command {
    font-size: 10px;
    color: var(--text-muted);
    font-family: var(--font-mono);
  }

  .instance-state {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    font-family: var(--font-mono);
  }

  .instance-state.stale {
    opacity: 0.5;
    font-style: italic;
  }

  .delete-btn {
    position: absolute;
    right: 8px;
    top: 50%;
    transform: translateY(-50%);
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 16px;
    font-family: inherit;
    cursor: pointer;
    opacity: 0;
    transition: all 0.15s ease;
    padding: 4px 8px;
  }

  .instance-item:hover .delete-btn {
    opacity: 1;
  }

  .delete-btn:hover {
    color: var(--status-red);
    text-shadow: var(--emphasis);
  }

  .presence-count {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    margin-left: 6px;
    background: var(--tint-presence);
    border: 1px solid var(--tint-presence-border);
    border-radius: 8px;
    font-size: 9px;
    font-weight: 700;
    color: var(--thinking-400);
    transition: all 0.15s ease;
  }

  .queue-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    margin-left: 6px;
    background: var(--tint-active-strong);
    border: 1px solid var(--chrome-accent-600);
    border-radius: 8px;
    font-size: 9px;
    font-weight: 700;
    color: var(--chrome-accent-400);
    font-variant-numeric: tabular-nums;
    transition: all 0.15s ease;
  }

  /* Active state: expand badges from circles to labeled pills */
  .instance-item.active .presence-count {
    border-radius: 4px;
    padding: 1px 6px;
    margin-left: 0;
    font-size: 9px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .instance-item.active .presence-count::before {
    content: 'VIEWERS ';
    font-weight: 600;
    opacity: 0.7;
  }

  .instance-item.active .queue-badge {
    border-radius: 4px;
    padding: 1px 6px;
    margin-left: 0;
    font-size: 9px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .instance-item.active .queue-badge::before {
    content: 'QUEUED ';
    font-weight: 600;
    opacity: 0.7;
  }

  /* Analog theme */
  :global([data-theme='analog']) .instance-item.active {
    background-color: var(--tint-active-strong);
    background-image: var(--grain-fine), var(--ink-wash);
    background-blend-mode: multiply, normal;
    border-width: 2px;
    border-left-width: calc(2px + var(--active-accent-width));
  }
</style>
