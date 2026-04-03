<script lang="ts">
  import { toasts, removeToast } from '$lib/stores/toasts';
</script>

{#if $toasts.length > 0}
  <div class="toast-stack" aria-live="polite" role="status">
    {#each $toasts as toast (toast.id)}
      <div class="toast {toast.type}">
        <span class="toast-message">{toast.message}</span>
        <button class="toast-dismiss" onclick={() => removeToast(toast.id)} aria-label="Dismiss">&times;</button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .toast-stack {
    position: fixed;
    bottom: 16px;
    right: 16px;
    z-index: 150;
    display: flex;
    flex-direction: column;
    gap: 8px;
    pointer-events: none;
  }

  .toast {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 14px;
    background: var(--surface-600);
    border: 1px solid var(--surface-border);
    border-left: 3px solid var(--chrome-accent-500);
    border-radius: 4px;
    box-shadow: var(--shadow-dropdown);
    pointer-events: auto;
    animation: toast-slide-up 200ms ease-out;
    max-width: 320px;
  }

  .toast.warn {
    border-left-color: var(--status-yellow);
  }

  .toast.error {
    border-left-color: var(--status-red);
  }

  .toast-message {
    font-size: 12px;
    font-family: inherit;
    color: var(--text-primary);
    line-height: 1.4;
  }

  .toast-dismiss {
    flex-shrink: 0;
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 16px;
    line-height: 1;
    padding: 0 2px;
    font-family: inherit;
  }

  .toast-dismiss:hover {
    color: var(--text-secondary);
  }

  @keyframes toast-slide-up {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .toast {
      animation: none;
    }
  }
</style>
