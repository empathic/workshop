<script lang="ts">
  import { shutdownReason } from '$lib/stores/websocket';

  function dismiss() {
    shutdownReason.set(null);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && $shutdownReason !== null) {
      e.preventDefault();
      e.stopPropagation();
      dismiss();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if $shutdownReason !== null}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="backdrop" onclick={dismiss}>
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="modal" onclick={(e) => e.stopPropagation()}>
      <div class="icon">&#x26A0;</div>
      <h2 class="title">SERVER SHUTDOWN</h2>
      <p class="reason">{$shutdownReason}</p>
      <p class="note">Reconnection will happen automatically when the server is back online.</p>
      <button class="dismiss-btn" onclick={dismiss}>DISMISS</button>
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
    max-width: 420px;
    width: 90%;
    padding: 2rem;
    border: 1px solid var(--surface-border-light);
    background: var(--surface-800);
    box-shadow: var(--depth-up);
    text-align: center;
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

  .icon {
    font-size: 2.5rem;
    margin-bottom: 0.75rem;
    color: var(--status-red);
    text-shadow: var(--emphasis-strong, 0 0 8px currentColor);
  }

  .title {
    font-size: 0.85rem;
    font-weight: 700;
    letter-spacing: 0.15em;
    color: var(--text-primary);
    text-shadow: var(--emphasis, 0 0 4px currentColor);
    margin: 0 0 1rem;
  }

  .reason {
    font-size: 0.8rem;
    color: var(--text-secondary);
    line-height: 1.5;
    margin: 0 0 0.75rem;
    padding: 0.75rem;
    background: var(--surface-900);
    border: 1px solid var(--surface-border);
  }

  .note {
    font-size: 0.7rem;
    color: var(--text-muted);
    line-height: 1.4;
    margin: 0 0 1.5rem;
  }

  .dismiss-btn {
    font-family: inherit;
    font-size: 0.75rem;
    font-weight: 600;
    letter-spacing: 0.1em;
    padding: 0.5rem 1.5rem;
    border: 1px solid var(--surface-border-light);
    background: var(--surface-600);
    color: var(--text-primary);
    cursor: pointer;
    transition:
      background 0.15s,
      border-color 0.15s;
  }

  .dismiss-btn:hover {
    background: var(--surface-500);
    border-color: var(--text-secondary);
  }

  .dismiss-btn:active {
    background: var(--surface-400);
  }

  .dismiss-btn:focus-visible {
    outline: 1px solid var(--text-primary);
    outline-offset: 2px;
  }
</style>
