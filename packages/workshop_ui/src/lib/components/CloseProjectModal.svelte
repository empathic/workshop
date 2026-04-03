<script lang="ts">
  import { deleteInstance, selectInstance } from '$lib/stores/instances';
  import { getStateInfo } from '$lib/utils/instance-state';
  import { addToast } from '$lib/stores/toasts';
  import type { Project } from '$lib/stores/projects';

  interface Props {
    project: Project;
    onclose: () => void;
  }

  let { project, onclose }: Props = $props();

  let isClosing = $state(false);
  let deletedCount = $state(0);
  let currentlyDeleting = $state<string | null>(null);
  let errors = $state<string[]>([]);

  let activeInstances = $derived(
    project.instances.filter((i) => {
      const t = i.claude_state?.type;
      return t === 'Thinking' || t === 'Responding' || t === 'ToolExecuting';
    })
  );

  let summaryText = $derived.by(() => {
    const a = activeInstances.length;
    const total = project.instances.length;
    if (a === 0) return `${total} instance${total !== 1 ? 's' : ''}, all idle`;
    return `${a} active, ${total - a} idle`;
  });

  let totalToDelete = $state(0);

  async function handleCloseProject() {
    isClosing = true;
    errors = [];
    deletedCount = 0;
    const ids = project.instances.map((i) => i.id);
    totalToDelete = ids.length;
    for (const id of ids) {
      currentlyDeleting = id;
      const ok = await deleteInstance(id);
      if (ok) deletedCount++;
      else errors.push(id);
    }
    currentlyDeleting = null;
    if (errors.length === 0) {
      addToast(`Closed project "${project.name}"`, 'info');
      onclose();
    } else {
      addToast(`Failed to close ${errors.length} instance(s)`, 'error');
      isClosing = false;
    }
  }

  function handleJumpToInstance(instanceId: string) {
    selectInstance(instanceId);
    onclose();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && !isClosing) {
      onclose();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="backdrop" onclick={() => !isClosing && onclose()}>
  <div class="modal" onclick={(e) => e.stopPropagation()}>
    <div class="title">CLOSE PROJECT</div>

    <div class="project-info">
      <span class="project-name">{project.name}</span>
      <span class="project-dir">{project.workingDir}</span>
    </div>

    <div class="instance-list">
      {#each project.instances as instance (instance.id)}
        {@const stateInfo = getStateInfo(instance.id, instance.claude_state)}
        {@const isDeleting = currentlyDeleting === instance.id}
        <button
          class="instance-row"
          class:active={stateInfo.animate}
          class:deleting={isDeleting}
          disabled={isClosing}
          onclick={() => handleJumpToInstance(instance.id)}
        >
          {#if isDeleting}
            <span class="row-spinner"></span>
          {:else}
            <span class="row-led" class:pulse={stateInfo.animate} style="background: {stateInfo.color}"></span>
          {/if}
          <span class="row-name">
            {instance.custom_name ?? instance.name}
          </span>
          <span class="row-state" class:stale={stateInfo.stale}>
            {#if isDeleting}
              closing...
            {:else}
              {stateInfo.label || 'idle'}
            {/if}
          </span>
          {#if !isClosing}
            <span class="row-arrow">&rarr;</span>
          {/if}
        </button>
      {/each}
    </div>

    <div class="summary">{summaryText}</div>

    {#if isClosing && totalToDelete > 0}
      <div class="progress-track">
        <div class="progress-fill" style="width: {(deletedCount / totalToDelete) * 100}%"></div>
      </div>
    {/if}

    {#if errors.length > 0}
      <div class="error-msg">
        Failed to delete {errors.length} instance{errors.length !== 1 ? 's' : ''}. Check server logs.
      </div>
    {/if}

    <div class="actions">
      <button class="cancel-btn" disabled={isClosing} onclick={onclose}> CANCEL </button>
      <button class="close-btn" disabled={isClosing && currentlyDeleting === null} onclick={handleCloseProject}>
        {#if isClosing}
          CLOSING {deletedCount}/{totalToDelete}
        {:else}
          CLOSE PROJECT
        {/if}
      </button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 200;
    background: var(--backdrop);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    animation: fade-in 0.2s ease;
  }

  @keyframes fade-in {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }

  .modal {
    max-width: 440px;
    width: 90%;
    padding: 1.5rem;
    border: 1px solid var(--surface-border-light);
    border-radius: 6px;
    background: var(--surface-700);
    box-shadow: var(--shadow-dropdown);
    animation: modal-in 0.2s ease;
  }

  @keyframes modal-in {
    from {
      opacity: 0;
      transform: scale(0.95);
    }
    to {
      opacity: 1;
      transform: scale(1);
    }
  }

  .title {
    font-size: 0.8rem;
    font-weight: 700;
    color: var(--chrome-accent-400);
    letter-spacing: 0.15em;
    margin-bottom: 1rem;
  }

  .project-info {
    display: flex;
    align-items: baseline;
    gap: 0.75rem;
    margin-bottom: 1rem;
    overflow: hidden;
  }

  .project-name {
    font-size: 0.75rem;
    font-weight: 700;
    color: var(--text-primary);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    flex-shrink: 0;
  }

  .project-dir {
    font-size: 0.62rem;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .instance-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 0.75rem;
    max-height: 240px;
    overflow-y: auto;
  }

  .instance-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    cursor: pointer;
    transition: all 0.15s ease;
    width: 100%;
    text-align: left;
  }

  .instance-row:hover:not(:disabled) {
    border-color: var(--chrome-accent-600);
    background: var(--tint-hover);
  }

  .instance-row:disabled {
    cursor: default;
    opacity: 0.7;
  }

  .instance-row.active {
    border-left: 2px solid var(--chrome-accent-500);
  }

  .instance-row.deleting {
    opacity: 0.5;
  }

  .row-led {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .row-led.pulse {
    animation: led-pulse 0.8s ease-in-out infinite;
  }

  @keyframes led-pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.4;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .row-led.pulse {
      animation: none;
    }
  }

  .row-spinner {
    width: 8px;
    height: 8px;
    border: 1.5px solid var(--text-muted);
    border-top-color: var(--chrome-accent-400);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    flex-shrink: 0;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .row-name {
    font-size: 0.68rem;
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
  }

  .instance-row.active .row-name {
    color: var(--text-primary);
  }

  .row-state {
    font-size: 0.62rem;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .row-state.stale {
    font-style: italic;
  }

  .row-arrow {
    font-size: 0.7rem;
    color: var(--text-muted);
    opacity: 0;
    transition: opacity 0.15s ease;
    flex-shrink: 0;
  }

  .instance-row:hover:not(:disabled) .row-arrow {
    opacity: 1;
  }

  .summary {
    font-size: 0.62rem;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.1em;
    margin-bottom: 1rem;
  }

  .progress-track {
    height: 3px;
    background: var(--surface-800);
    border-radius: 2px;
    margin-bottom: 1rem;
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: var(--status-red);
    border-radius: 2px;
    transition: width 0.3s ease;
  }

  .error-msg {
    font-size: 0.62rem;
    color: var(--status-red);
    margin-bottom: 0.75rem;
  }

  .actions {
    display: flex;
    gap: 0.75rem;
    justify-content: flex-end;
  }

  .cancel-btn,
  .close-btn {
    padding: 6px 16px;
    font-size: 0.68rem;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    border-radius: 3px;
    cursor: pointer;
    transition: all 0.15s ease;
    font-family: inherit;
  }

  .cancel-btn {
    background: transparent;
    border: 1px solid var(--surface-border-light);
    color: var(--text-secondary);
  }

  .cancel-btn:hover:not(:disabled) {
    background: var(--tint-hover);
    border-color: var(--text-muted);
  }

  .cancel-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .close-btn {
    background: transparent;
    border: 1px solid var(--status-red);
    color: var(--status-red);
  }

  .close-btn:hover:not(:disabled) {
    background: var(--status-red);
    color: var(--surface-900);
  }

  .close-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>
