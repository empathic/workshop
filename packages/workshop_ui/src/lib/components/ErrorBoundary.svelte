<script lang="ts">
  /**
   * ErrorBoundary - Graceful error handling for component subtrees
   *
   * Catches errors during rendering and displays a fallback UI.
   * Prevents entire app from crashing when a single component fails.
   *
   * Note: Svelte 5 doesn't have built-in try/catch for rendering.
   * This uses $effect error handling and manual error state.
   */

  import type { Snippet } from 'svelte';

  interface Props {
    children: Snippet;
    fallback?: Snippet<[Error, () => void]>;
    onError?: (error: Error) => void;
  }

  let { children, fallback, onError }: Props = $props();

  let error = $state<Error | null>(null);

  /**
   * Manually capture an error (for use by child components).
   * Call this from catch blocks in child $effects.
   */
  export function captureError(e: unknown): void {
    const err = e instanceof Error ? e : new Error(String(e));
    error = err;
    onError?.(err);
    console.error('[ErrorBoundary]', err);
  }

  /**
   * Reset error state to retry rendering.
   */
  export function reset(): void {
    error = null;
  }
</script>

{#if error}
  {#if fallback}
    {@render fallback(error, reset)}
  {:else}
    <div class="error-boundary-fallback">
      <span class="error-icon">!</span>
      <span class="error-message">Something went wrong</span>
      <button class="error-retry" onclick={reset}>Retry</button>
    </div>
  {/if}
{:else}
  {@render children()}
{/if}

<style>
  .error-boundary-fallback {
    padding: 1rem;
    background: var(--surface-800);
    border: 1px solid var(--status-red);
    border-radius: 4px;
    color: var(--status-red-text);
    display: flex;
    align-items: center;
    gap: 0.75rem;
    font-size: 12px;
  }

  .error-icon {
    width: 20px;
    height: 20px;
    background: var(--status-red);
    color: var(--on-status);
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: bold;
    flex-shrink: 0;
  }

  .error-message {
    flex: 1;
  }

  .error-retry {
    padding: 4px 8px;
    background: var(--surface-600);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    color: var(--text-secondary);
    font-size: 11px;
    cursor: pointer;
    transition: background 0.15s;
  }

  .error-retry:hover {
    background: var(--surface-500);
  }
</style>
