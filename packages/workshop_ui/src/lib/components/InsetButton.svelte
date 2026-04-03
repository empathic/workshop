<script lang="ts">
  interface Props {
    onclick: () => void;
  }

  let { onclick }: Props = $props();
</script>

<button class="inset-btn" {onclick} aria-label="Insert into pane" title="Insert into pane">
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
    <rect x="3" y="3" width="18" height="18" rx="2" />
    <line x1="12" y1="3" x2="12" y2="21" />
    <line x1="3" y1="12" x2="12" y2="12" />
  </svg>
</button>

<style>
  .inset-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 3px;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .inset-btn:hover {
    border-color: var(--surface-border);
    color: var(--chrome-accent-400);
    background: var(--tint-active);
  }

  .inset-btn svg {
    width: 14px;
    height: 14px;
  }
</style>
