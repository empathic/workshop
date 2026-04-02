<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    title: string;
    onclose: () => void;
    actions?: Snippet;
  }

  let { title, onclose, actions }: Props = $props();
</script>

<header class="fullscreen-header">
  <button class="back-chip" onclick={onclose} aria-label="Back">
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5">
      <path d="M10 3L5 8l5 5" />
    </svg>
  </button>
  <h1 class="header-title">{title}</h1>
  {#if actions}
    <div class="header-actions">
      {@render actions()}
    </div>
  {/if}
</header>

<style>
  .fullscreen-header {
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

  .header-title {
    flex: 1;
    margin: 0;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--chrome-accent-500);
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }
</style>
