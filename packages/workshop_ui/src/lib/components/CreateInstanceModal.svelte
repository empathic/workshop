<script lang="ts">
  import { untrack } from 'svelte';
  import { createInstance, selectInstance } from '$lib/stores/instances';
  import { defaultCommand } from '$lib/stores/settings';
  import { currentProject } from '$lib/stores/projects';
  import DirectoryPicker from './DirectoryPicker.svelte';

  interface Props {
    onclose: () => void;
    oncreated?: (instanceId: string) => void;
    /** 'project' = new working dir (sidebar); 'instance' = within current project */
    mode?: 'project' | 'instance';
  }

  let { onclose, oncreated, mode = 'instance' }: Props = $props();

  let workingDir = $state(untrack(() => mode) === 'project' ? '' : ($currentProject?.workingDir ?? ''));
  let command = $state($defaultCommand);
  let instanceName = $state('');
  let isCreating = $state(false);
  let error = $state('');

  function handleKeydown(e: KeyboardEvent) {
    // Only Escape-to-close in modal mode (instance), not fullscreen (project)
    if (e.key === 'Escape' && mode !== 'project') {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    }
  }

  async function handleSubmit(e: Event) {
    e.preventDefault();
    if (isCreating) return;

    if (mode === 'project') {
      const trimmed = workingDir.trim();
      if (!trimmed) {
        error = 'Working directory is required for a new project';
        return;
      }
    }

    isCreating = true;
    error = '';

    const result = await createInstance({
      command: command || undefined,
      working_dir: workingDir.trim() || undefined,
      name: instanceName || undefined
    });

    if (result) {
      selectInstance(result.id);
      if (oncreated) oncreated(result.id);
      onclose();
    } else {
      error = 'Failed to create instance';
      isCreating = false;
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if mode === 'project'}
  <!-- Full-screen layout for project creation -->
  <div class="fullscreen">
    <header class="project-header">
      <button class="back-chip" onclick={onclose} aria-label="Back">
        <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5">
          <path d="M10 3L5 8l5 5" />
        </svg>
      </button>
      <h1 class="header-title">New Project</h1>
    </header>
    <div class="fullscreen-panel">
      <div class="picker-area">
        <DirectoryPicker bind:value={workingDir} onselect={(path) => (workingDir = path)} />
      </div>

      <form class="bottom-form" onsubmit={handleSubmit}>
        <div class="bottom-fields">
          <label class="field compact">
            <span class="field-label">COMMAND</span>
            <input
              type="text"
              class="field-input"
              bind:value={command}
              placeholder="claude"
              spellcheck="false"
              autocomplete="off"
            />
          </label>

          <label class="field compact">
            <span class="field-label">NAME <span class="optional">(optional)</span></span>
            <input
              type="text"
              class="field-input"
              bind:value={instanceName}
              placeholder="auto-generated"
              spellcheck="false"
              autocomplete="off"
            />
          </label>
        </div>

        {#if error}
          <p class="error">{error}</p>
        {/if}

        <button type="submit" class="create-btn fullwidth" disabled={isCreating}>
          {#if isCreating}
            <span class="spinner"></span>
            CREATING…
          {:else}
            CREATE
          {/if}
        </button>
      </form>
    </div>
  </div>
{:else}
  <!-- Modal layout for instance creation -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="backdrop" onclick={onclose}>
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="modal" onclick={(e) => e.stopPropagation()}>
      <h2 class="title">NEW INSTANCE</h2>

      <form onsubmit={handleSubmit}>
        <label class="field">
          <span class="field-label">WORKING DIRECTORY <span class="optional">(optional)</span></span>
          <!-- svelte-ignore a11y_autofocus -->
          <input
            type="text"
            class="field-input"
            bind:value={workingDir}
            placeholder="/path/to/project"
            spellcheck="false"
            autocomplete="off"
          />
        </label>

        <label class="field">
          <span class="field-label">COMMAND</span>
          <input
            type="text"
            class="field-input"
            bind:value={command}
            placeholder="claude"
            spellcheck="false"
            autocomplete="off"
          />
        </label>

        <label class="field">
          <span class="field-label">NAME <span class="optional">(optional)</span></span>
          <input
            type="text"
            class="field-input"
            bind:value={instanceName}
            placeholder="auto-generated"
            spellcheck="false"
            autocomplete="off"
          />
        </label>

        {#if error}
          <p class="error">{error}</p>
        {/if}

        <div class="actions">
          <button type="button" class="cancel-btn" onclick={onclose} disabled={isCreating}> CANCEL </button>
          <button type="submit" class="create-btn" disabled={isCreating}>
            {#if isCreating}
              <span class="spinner"></span>
              CREATING…
            {:else}
              CREATE
            {/if}
          </button>
        </div>
      </form>
    </div>
  </div>
{/if}

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
    max-width: 400px;
    width: 90%;
    padding: 1.5rem;
    border: 1px solid var(--surface-border-light);
    background: var(--surface-700);
    box-shadow: var(--depth-up);
    font-family: var(--font-mono);
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
    letter-spacing: 0.15em;
    color: var(--chrome-accent-400);
    text-shadow: var(--emphasis, 0 0 4px currentColor);
    margin: 0 0 1.25rem;
    text-align: center;
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .field-label {
    font-size: 0.65rem;
    font-weight: 600;
    letter-spacing: 0.1em;
    color: var(--text-secondary);
  }

  .optional {
    font-weight: 400;
    color: var(--text-muted);
    letter-spacing: 0.05em;
    text-transform: lowercase;
  }

  .field-input {
    font-family: inherit;
    font-size: 0.75rem;
    padding: 0.5rem 0.6rem;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    color: var(--text-primary);
    outline: none;
    transition: border-color 0.15s;
  }

  .field-input::placeholder {
    color: var(--text-muted);
    opacity: 0.6;
  }

  .field-input:focus {
    border-color: var(--chrome-accent-600);
  }

  .error {
    font-size: 0.7rem;
    color: var(--status-red);
    margin: 0;
    padding: 0.4rem 0.6rem;
    background: var(--status-red-tint);
    border: 1px solid var(--status-red-border);
  }

  .actions {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.5rem;
  }

  .cancel-btn {
    flex: 1;
    font-family: inherit;
    font-size: 0.7rem;
    font-weight: 600;
    letter-spacing: 0.1em;
    padding: 0.5rem 1rem;
    border: 1px solid var(--surface-border);
    background: var(--surface-600);
    color: var(--text-secondary);
    cursor: pointer;
    transition:
      background 0.15s,
      border-color 0.15s;
  }

  .cancel-btn:hover:not(:disabled) {
    background: var(--surface-500);
    border-color: var(--surface-border-light);
  }

  .cancel-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .create-btn {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    font-family: inherit;
    font-size: 0.7rem;
    font-weight: 600;
    letter-spacing: 0.1em;
    padding: 0.5rem 1rem;
    border: 1px solid var(--chrome-accent-600);
    background: var(
      --btn-primary-bg,
      linear-gradient(180deg, var(--chrome-accent-600) 0%, var(--chrome-accent-700, var(--chrome-accent-600)) 100%)
    );
    color: var(--surface-900);
    cursor: pointer;
    transition: all 0.15s;
  }

  .create-btn:hover:not(:disabled) {
    filter: brightness(1.1);
  }

  .create-btn.fullwidth {
    width: 100%;
    flex: none;
  }

  .create-btn:disabled {
    opacity: 0.7;
    cursor: not-allowed;
  }

  .spinner {
    width: 10px;
    height: 10px;
    border: 1.5px solid var(--surface-border);
    border-top-color: var(--surface-900);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .project-header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 16px;
    background: linear-gradient(180deg, var(--surface-600) 0%, var(--surface-700) 100%);
    border-bottom: 1px solid var(--surface-border);
    flex-shrink: 0;
  }

  .back-chip {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    font-family: inherit;
    color: var(--text-muted);
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .back-chip:hover {
    color: var(--text-secondary);
    border-color: var(--surface-border-light);
    background: var(--surface-700);
  }

  .back-chip svg {
    width: 14px;
    height: 14px;
  }

  .project-header .header-title {
    flex: 1;
    margin: 0;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--chrome-accent-500);
  }

  /* Full-screen project layout (inline — parent handles placement) */
  .fullscreen {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    background: var(--surface-900);
    font-family: var(--font-mono);
  }

  .fullscreen-panel {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .picker-area {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    padding: 0 1.5rem;
    display: flex;
    flex-direction: column;
  }

  .picker-area :global(.directory-picker) {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .picker-area :global(.miller-columns) {
    flex: 1;
    min-height: 0;
  }

  .bottom-form {
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    padding: 0.75rem 1.5rem 1.25rem;
    border-top: 1px solid var(--surface-border);
    background: var(--surface-800);
  }

  .bottom-fields {
    display: flex;
    gap: 0.75rem;
  }

  .field.compact {
    flex: 1;
  }
</style>
