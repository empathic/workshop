<script lang="ts">
  import { createTask } from '$lib/stores/tasks';
  import type { Instance } from '$lib/types';

  interface Props {
    instanceList: Instance[];
    oncreated: () => void;
  }

  let { instanceList, oncreated }: Props = $props();

  let newTitle = $state('');
  let newBody = $state('');
  let newInstanceId = $state<string>('');
  let newPriority = $state(0);
  let isCreating = $state(false);

  async function handleCreate() {
    if (!newTitle.trim() || isCreating) return;
    isCreating = true;
    await createTask({
      title: newTitle.trim(),
      body: newBody.trim() || undefined,
      instance_id: newInstanceId || undefined,
      priority: newPriority
    });
    newTitle = '';
    newBody = '';
    newInstanceId = '';
    newPriority = 0;
    isCreating = false;
    oncreated();
  }
</script>

<div class="create-form">
  <div class="form-row">
    <!-- svelte-ignore a11y_autofocus -->
    <input
      type="text"
      class="form-input title-input"
      placeholder="Task title..."
      bind:value={newTitle}
      onkeydown={(e) => e.key === 'Enter' && handleCreate()}
      autofocus
    />
  </div>
  <div class="form-row">
    <textarea class="form-input body-input" placeholder="Full prompt body (optional)..." bind:value={newBody} rows="3"
    ></textarea>
  </div>
  <div class="form-row form-meta-row">
    <select class="form-select" bind:value={newInstanceId}>
      <option value="">No instance</option>
      {#each instanceList as inst}
        <option value={inst.id}>{inst.custom_name ?? inst.name}</option>
      {/each}
    </select>
    <select class="form-select" bind:value={newPriority}>
      <option value={0}>No priority</option>
      <option value={1}>Low</option>
      <option value={2}>Medium</option>
      <option value={3}>High</option>
    </select>
    <button class="submit-btn" onclick={handleCreate} disabled={!newTitle.trim() || isCreating}>
      {#if isCreating}
        <span class="spinner-small"></span>
      {:else}
        Create
      {/if}
    </button>
  </div>
</div>

<style>
  .create-form {
    padding: 16px 20px;
    background: var(--surface-700);
    border-bottom: 1px solid var(--surface-border);
    display: flex;
    flex-direction: column;
    gap: 10px;
    animation: form-expand 0.15s ease-out;
    flex-shrink: 0;
  }

  @keyframes form-expand {
    from {
      opacity: 0;
      max-height: 0;
    }
    to {
      opacity: 1;
      max-height: 300px;
    }
  }

  .form-row {
    display: flex;
    gap: 8px;
  }

  .form-input {
    width: 100%;
    padding: 8px 12px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 13px;
    font-family: inherit;
    outline: none;
    transition: border-color 0.15s ease;
  }

  .form-input::placeholder {
    color: var(--text-muted);
  }

  .form-input:focus {
    border-color: var(--chrome-accent-600);
  }

  .body-input {
    resize: vertical;
    min-height: 48px;
    font-family: var(--font-mono);
    font-size: 12px;
  }

  .form-meta-row {
    align-items: center;
  }

  .form-select {
    padding: 6px 10px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
    outline: none;
    cursor: pointer;
    min-width: 120px;
  }

  .form-select:focus {
    border-color: var(--chrome-accent-600);
  }

  .submit-btn {
    margin-left: auto;
    padding: 6px 16px;
    background: var(--btn-primary-bg);
    border: none;
    border-radius: 4px;
    color: var(--btn-primary-text);
    font-size: 12px;
    font-weight: 700;
    font-family: inherit;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    text-shadow: var(--btn-primary-text-shadow);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .submit-btn:hover:not(:disabled) {
    box-shadow: var(--elevation-high);
  }

  .submit-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .spinner-small {
    display: inline-block;
    width: 12px;
    height: 12px;
    border: 2px solid var(--spinner-track);
    border-top-color: var(--btn-primary-text);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (max-width: 639px) {
    .create-form {
      padding: 12px 14px;
    }
  }

  :global([data-theme='analog']) .create-form {
    background-color: var(--surface-700);
    background-image: var(--grain-fine), var(--ink-wash);
    background-blend-mode: multiply, normal;
  }
</style>
