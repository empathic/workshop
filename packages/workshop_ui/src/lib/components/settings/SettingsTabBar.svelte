<script lang="ts">
  interface Tab {
    id: string;
    label: string;
  }

  interface Props {
    tabs: Tab[];
    activeTab: string;
    onchange: (id: string) => void;
  }

  let { tabs, activeTab, onchange }: Props = $props();

  function handleKeydown(e: KeyboardEvent) {
    const idx = tabs.findIndex((t) => t.id === activeTab);
    if (idx < 0) return;

    if (e.key === 'ArrowRight') {
      e.preventDefault();
      const next = (idx + 1) % tabs.length;
      onchange(tabs[next].id);
    } else if (e.key === 'ArrowLeft') {
      e.preventDefault();
      const prev = (idx - 1 + tabs.length) % tabs.length;
      onchange(tabs[prev].id);
    }
  }
</script>

<!-- svelte-ignore a11y_interactive_supports_focus -->
<div class="tab-bar" role="tablist" onkeydown={handleKeydown}>
  {#each tabs as tab}
    <button
      class="tab-pill"
      class:active={activeTab === tab.id}
      role="tab"
      aria-selected={activeTab === tab.id}
      tabindex={activeTab === tab.id ? 0 : -1}
      onclick={() => onchange(tab.id)}
    >
      {tab.label}
    </button>
  {/each}
</div>

<style>
  .tab-bar {
    display: flex;
    gap: 4px;
    padding: 8px 24px;
    background: var(--surface-800);
    border-bottom: 1px solid var(--surface-border);
    box-shadow: var(--recess-border);
    flex-shrink: 0;
    position: sticky;
    top: 0;
    z-index: 1;
  }

  .tab-pill {
    padding: 4px 10px;
    font-family: inherit;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-muted);
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .tab-pill:hover {
    color: var(--text-secondary);
  }

  .tab-pill.active {
    color: var(--chrome-accent-400);
    border-bottom-color: var(--chrome-accent-500);
    text-shadow: var(--emphasis);
  }
</style>
