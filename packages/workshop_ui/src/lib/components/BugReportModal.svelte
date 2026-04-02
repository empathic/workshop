<script lang="ts">
  import { apiPost } from '$lib/utils/api';
  import { addToast } from '$lib/stores/toasts';

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();

  let title = $state('');
  let description = $state('');
  let isSubmitting = $state(false);
  let error = $state('');

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    }
  }

  async function handleSubmit(e: Event) {
    e.preventDefault();
    if (isSubmitting) return;

    if (!title.trim()) {
      error = 'Title is required';
      return;
    }

    isSubmitting = true;
    error = '';

    try {
      await apiPost('/api/bug-report', {
        title: title.trim(),
        description: description.trim() || undefined
      });
      addToast('Bug report submitted', 'info');
      onclose();
    } catch (err) {
      console.error(err);
      error = err instanceof Error ? err.message : 'Failed to submit report';
      isSubmitting = false;
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onclose}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="modal" onclick={(e) => e.stopPropagation()}>
    <h2 class="title">BUG REPORT</h2>

    <form onsubmit={handleSubmit}>
      <label class="field">
        <span class="field-label">TITLE</span>
        <!-- svelte-ignore a11y_autofocus -->
        <input
          type="text"
          class="field-input"
          bind:value={title}
          placeholder="Brief summary of the issue"
          spellcheck="false"
          autocomplete="off"
          autofocus
        />
      </label>

      <label class="field">
        <span class="field-label">DESCRIPTION <span class="optional">(optional)</span></span>
        <textarea
          class="field-input"
          bind:value={description}
          placeholder="Steps to reproduce, expected behavior, etc."
          rows="4"
          spellcheck="false"
        ></textarea>
      </label>

      {#if error}
        <p class="error">{error}</p>
      {/if}

      <div class="actions">
        <button type="button" class="cancel-btn" onclick={onclose} disabled={isSubmitting}>CANCEL</button>
        <button type="submit" class="submit-btn" disabled={isSubmitting}>
          {#if isSubmitting}
            <span class="spinner"></span>
            SUBMITTING…
          {:else}
            SUBMIT
          {/if}
        </button>
      </div>
    </form>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 200;
    background: rgba(0, 0, 0, 0.7);
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
    box-shadow: var(--depth-up, 0 4px 24px rgba(0, 0, 0, 0.5));
    font-family: var(--font-mono, 'JetBrains Mono', monospace);
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
    color: var(--amber-400);
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
    border-color: var(--amber-600);
  }

  textarea.field-input {
    resize: vertical;
    min-height: 80px;
    line-height: 1.4;
  }

  .error {
    font-size: 0.7rem;
    color: var(--status-red, #ef4444);
    margin: 0;
    padding: 0.4rem 0.6rem;
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid rgba(239, 68, 68, 0.2);
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

  .submit-btn {
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
    border: 1px solid var(--amber-600);
    background: var(
      --btn-primary-bg,
      linear-gradient(180deg, var(--amber-600) 0%, var(--amber-700, var(--amber-600)) 100%)
    );
    color: var(--surface-900);
    cursor: pointer;
    transition: all 0.15s;
  }

  .submit-btn:hover:not(:disabled) {
    filter: brightness(1.1);
  }

  .submit-btn:disabled {
    opacity: 0.7;
    cursor: not-allowed;
  }

  .spinner {
    width: 10px;
    height: 10px;
    border: 1.5px solid rgba(0, 0, 0, 0.3);
    border-top-color: var(--surface-900);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
