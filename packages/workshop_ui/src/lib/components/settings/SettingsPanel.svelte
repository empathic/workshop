<script lang="ts">
  import { onMount } from 'svelte';
  import { currentUser, authEnabled } from '$lib/stores/auth';
  import FullscreenHeader from '../FullscreenHeader.svelte';
  import SettingsTabBar from './SettingsTabBar.svelte';
  import PreferencesTab from './PreferencesTab.svelte';
  import AccountTab from './AccountTab.svelte';
  import ServerConfigTab from './ServerConfigTab.svelte';
  import AdminTab from './AdminTab.svelte';

  interface Props {
    embedded?: boolean;
    onback?: () => void;
  }

  let { embedded = false, onback }: Props = $props();

  let activeTab = $state('preferences');
  let layout = $state<'tabs' | 'columns'>('tabs');
  let panelEl = $state<HTMLDivElement | null>(null);

  const COLUMNS_THRESHOLD = 700;

  // When auth is disabled, the user is effectively a local admin (loopback bypass).
  let isAdmin = $derived($currentUser?.is_admin || !$authEnabled);

  interface TabDef {
    id: string;
    label: string;
    requiresAuth?: boolean;
    requiresAdmin?: boolean;
  }

  const allTabs: TabDef[] = [
    { id: 'preferences', label: 'Preferences' },
    { id: 'account', label: 'Account', requiresAuth: true },
    { id: 'server', label: 'Server', requiresAdmin: true },
    { id: 'admin', label: 'Admin', requiresAdmin: true },
  ];

  let visibleTabs = $derived(
    allTabs.filter((tab) => {
      if (tab.requiresAdmin) return isAdmin;
      if (tab.requiresAuth) return !!$currentUser;
      return true;
    })
  );

  onMount(() => {
    if (!panelEl) return;
    const ro = new ResizeObserver((entries) => {
      const width = entries[0]?.contentRect.width ?? 0;
      layout = width >= COLUMNS_THRESHOLD ? 'columns' : 'tabs';
    });
    ro.observe(panelEl);
    return () => ro.disconnect();
  });
</script>

<div class="settings-panel" class:embedded bind:this={panelEl}>
  {#if onback}
    <FullscreenHeader title="Settings" onclose={onback} />
  {/if}

  {#if layout === 'tabs'}
    <SettingsTabBar tabs={visibleTabs} {activeTab} onchange={(id) => (activeTab = id)} />
    <div class="settings-scroll">
      {#if activeTab === 'preferences'}
        <PreferencesTab />
      {:else if activeTab === 'account'}
        <AccountTab />
      {:else if activeTab === 'server'}
        <ServerConfigTab onTabChange={(id) => (activeTab = id)} />
      {:else if activeTab === 'admin'}
        <AdminTab />
      {/if}
    </div>
  {:else}
    <div class="columns-scroll">
      <div class="columns-grid">
        {#each visibleTabs as tab (tab.id)}
          <div class="settings-column">
            <h2 class="column-header">{tab.label}</h2>
            {#if tab.id === 'preferences'}
              <PreferencesTab />
            {:else if tab.id === 'account'}
              <AccountTab />
            {:else if tab.id === 'server'}
              <ServerConfigTab onTabChange={(id) => (activeTab = id)} />
            {:else if tab.id === 'admin'}
              <AdminTab />
            {/if}
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .settings-panel {
    width: 100%;
    height: 100%;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    background: var(--surface-800);
  }

  .settings-panel.embedded {
    border: none;
  }

  /* --- Tab mode (narrow) --- */

  .settings-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 24px;
    max-width: 560px;
  }

  .settings-scroll::-webkit-scrollbar {
    width: 4px;
  }

  .settings-scroll::-webkit-scrollbar-thumb {
    background: var(--surface-border);
    border-radius: 2px;
  }

  /* --- Column mode (wide) --- */

  .columns-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 24px 24px 24px 24px;
  }

  .columns-scroll::-webkit-scrollbar {
    width: 4px;
  }

  .columns-scroll::-webkit-scrollbar-thumb {
    background: var(--surface-border);
    border-radius: 2px;
  }

  .columns-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 32px;
    align-items: flex-start;
  }

  .settings-column {
    flex: 1;
    min-width: 260px;
    max-width: 420px;
  }

  .column-header {
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-primary);
    text-shadow: var(--emphasis-strong);
    margin: 0 0 16px 0;
    padding-bottom: 8px;
    border-bottom: 2px solid var(--chrome-accent-500);
  }
</style>
