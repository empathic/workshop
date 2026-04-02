<script lang="ts">
  import { onMount } from 'svelte';
  import { userSettings, setTheme, THEME_OPTIONS, type ThemeId } from '$lib/stores/settings';
  import { openFullscreen } from '$lib/stores/fullscreen';

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();

  let popoverEl: HTMLDivElement | undefined = $state();

  function handleOpenSettings() {
    openFullscreen('settings');
    onclose();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      onclose();
    }
  }

  function handleClickOutside(e: MouseEvent) {
    if (popoverEl && !popoverEl.contains(e.target as Node)) {
      onclose();
    }
  }

  onMount(() => {
    // Delay adding click listener to avoid immediate close
    const timer = setTimeout(() => {
      document.addEventListener('click', handleClickOutside, true);
    }, 0);

    document.addEventListener('keydown', handleKeydown);

    return () => {
      clearTimeout(timer);
      document.removeEventListener('click', handleClickOutside, true);
      document.removeEventListener('keydown', handleKeydown);
    };
  });
</script>

<div class="quick-settings" bind:this={popoverEl}>
  <div class="qs-header">QUICK SETTINGS</div>

  <div class="qs-row">
    <span class="qs-label">Theme</span>
    <select
      class="qs-select"
      value={$userSettings.theme}
      onchange={(e) => setTheme((e.target as HTMLSelectElement).value as ThemeId)}
    >
      {#each THEME_OPTIONS as opt (opt.id)}
        <option value={opt.id}>{opt.label}</option>
      {/each}
    </select>
  </div>

  <div class="qs-separator"></div>

  <button class="qs-open-btn" onclick={handleOpenSettings}> Open Settings </button>
</div>

<style>
  .quick-settings {
    position: fixed;
    left: 56px;
    bottom: 80px;
    width: 200px;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 6px;
    padding: 8px;
    z-index: 1000;
    box-shadow: var(--shadow-dropdown);
  }

  .qs-header {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.12em;
    color: var(--text-muted);
    padding: 4px 4px 8px;
  }

  .qs-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px;
  }

  .qs-label {
    font-size: 11px;
    color: var(--text-secondary);
    font-weight: 600;
    letter-spacing: 0.03em;
  }

  .qs-select {
    font-size: 10px;
    font-weight: 600;
    font-family: inherit;
    color: var(--text-secondary);
    background: var(--surface-600);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    padding: 3px 6px;
    cursor: pointer;
    outline: none;
    letter-spacing: 0.05em;
    transition: border-color 0.15s ease;
  }

  .qs-select:hover {
    border-color: var(--chrome-accent-600);
  }

  .qs-select:focus {
    border-color: var(--chrome-accent-500);
  }

  .qs-select option {
    background: var(--surface-600);
    color: var(--text-primary);
  }

  .qs-separator {
    height: 1px;
    background: var(--surface-border);
    margin: 6px 4px;
  }

  .qs-open-btn {
    display: block;
    width: 100%;
    font-size: 11px;
    font-weight: 600;
    font-family: inherit;
    letter-spacing: 0.05em;
    color: var(--text-secondary);
    background: transparent;
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    padding: 5px 8px;
    cursor: pointer;
    text-align: center;
    transition: all 0.15s ease;
  }

  .qs-open-btn:hover {
    background: var(--tint-hover);
    border-color: var(--chrome-accent-600);
    color: var(--chrome-accent-400);
  }
</style>
