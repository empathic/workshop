<script lang="ts">
  import type { Instance } from '$lib/types';
  import { currentInstanceId, selectInstance } from '$lib/stores/instances';
  import { connectionStatus } from '$lib/stores/websocket';
  import { currentProject, projects } from '$lib/stores/projects';
  import { getStateInfo } from '$lib/utils/instance-state';

  import { toggleExplorer, isExplorerOpen } from '$lib/stores/files';
  import { toggleChat, isChatOpen, totalUnread } from '$lib/stores/chat';
  import { isTaskPanelOpen, toggleTaskPanel, currentInstanceTaskCount } from '$lib/stores/tasks';
  import { layoutState, getPaneInstanceId } from '$lib/stores/layout';

  import FleetCommandCenter from './FleetCommandCenter.svelte';
  import InstancePopover from './InstancePopover.svelte';
  import CreateInstanceModal from '../CreateInstanceModal.svelte';
  import BugReportModal from '../BugReportModal.svelte';

  function getStatusColor(status: string): string {
    switch (status) {
      case 'connected':
        return 'var(--status-green)';
      case 'connecting':
      case 'reconnecting':
        return 'var(--chrome-accent-500)';
      case 'error':
      case 'server_gone':
        return 'var(--status-red)';
      default:
        return 'var(--text-muted)';
    }
  }

  function getStatusText(status: string): string {
    switch (status) {
      case 'connected':
        return 'Online';
      case 'connecting':
        return 'Connecting';
      case 'reconnecting':
        return 'Reconnecting';
      case 'server_gone':
        return 'Offline';
      case 'error':
        return 'Error';
      default:
        return 'No Signal';
    }
  }

  let showRestored = $state(false);
  let prevConnectionStatus = $state('disconnected');

  $effect(() => {
    const status = $connectionStatus;
    if (
      status === 'connected' &&
      (prevConnectionStatus === 'error' ||
        prevConnectionStatus === 'reconnecting' ||
        prevConnectionStatus === 'server_gone')
    ) {
      showRestored = true;
      setTimeout(() => {
        showRestored = false;
      }, 2000);
    }
    prevConnectionStatus = status;
  });

  // =========================================================================
  // Fleet panel
  // =========================================================================

  let panelOpen = $state(false);
  let showCreateModal = $state(false);
  let showBugReport = $state(false);

  // Instance fleet for the current project (only shown when a project is selected)
  const fleetInstances = $derived($currentProject?.instances ?? []);

  // Set of instance IDs visible in at least one pane (for pane-presence indicator)
  const paneInstanceIds = $derived.by(() => {
    const ids = new Set<string>();
    for (const pane of $layoutState.panes.values()) {
      const id = getPaneInstanceId(pane.content);
      if (id) ids.add(id);
    }
    return ids;
  });

  /** Close all popovers/dropdowns — ensures mutual exclusion */
  function dismissAllPopovers() {
    panelOpen = false;
    popoverTarget = null;
  }

  function togglePanel() {
    const opening = !panelOpen;
    if (opening) dismissAllPopovers();
    panelOpen = opening;
  }

  function handleFleetSelect(instanceId: string) {
    panelOpen = false;
    selectInstance(instanceId);
  }

  function handleFilesClick() {
    toggleExplorer();
  }

  function handleTasksClick() {
    toggleTaskPanel();
  }

  function handleChatClick() {
    toggleChat();
  }

  // =========================================================================
  // Right-click / contextmenu popover
  // =========================================================================

  let popoverTarget = $state<{ instance: Instance; anchorRect: DOMRect } | null>(null);
</script>

<header class="main-header">
  <!-- Left: Project identity + connection status -->
  <div class="header-project">
    {#if $currentProject}
      <span class="project-name">{$currentProject.name}</span>
      {#if $projects.length > 1}
        <span class="project-count">{$projects.length} projects</span>
      {/if}
    {:else}
      <span class="project-name">Workshop</span>
    {/if}
    <span
      class="connection-dot"
      class:signal-lost={$connectionStatus === 'error' ||
        $connectionStatus === 'reconnecting' ||
        $connectionStatus === 'server_gone'}
      class:signal-restored={showRestored}
      style="background: {getStatusColor($connectionStatus)}"
      title={showRestored ? 'Link Restored' : getStatusText($connectionStatus)}
    ></span>
  </div>

  <!-- Center: Fleet command center -->
  {#if $currentProject}
    <div class="header-fleet">
      <FleetCommandCenter
        instances={fleetInstances}
        currentInstanceId={$currentInstanceId}
        {paneInstanceIds}
        expanded={panelOpen}
        onselect={handleFleetSelect}
        onexpand={togglePanel}
        onclose={() => (panelOpen = false)}
        oncontextmenu={(inst, rect) => {
          popoverTarget = { instance: inst, anchorRect: rect };
        }}
        oncreate={() => {
          panelOpen = false;
          showCreateModal = true;
        }}
      />
    </div>
  {:else}
    <div class="header-fleet"></div>
  {/if}

  <!-- Right: Actions -->
  <div class="header-actions">
    <button
      class="action-btn"
      onclick={() => (showBugReport = true)}
      title="Bug Report"
      aria-label="Submit bug report"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" style="width: 16px; height: 16px;">
        <path d="M8 9h8v4.017a4 4 0 01-8 0V9z" />
        <path d="M10 9V8a2 2 0 014 0v1" />
        <line x1="12" y1="13" x2="12" y2="17" />
        <line x1="5" y1="13" x2="8" y2="13" />
        <line x1="16" y1="13" x2="19" y2="13" />
        <line x1="5.5" y1="7" x2="8" y2="9" />
        <line x1="18.5" y1="7" x2="16" y2="9" />
        <line x1="6.5" y1="17.5" x2="8.6" y2="15.3" />
        <line x1="17.5" y1="17.5" x2="15.4" y2="15.3" />
      </svg>
    </button>
    <button
      class="action-btn"
      class:active={$isExplorerOpen}
      onclick={handleFilesClick}
      title="Files"
      aria-label="Toggle file explorer"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
      </svg>
    </button>
    <button
      class="action-btn tasks-btn"
      class:active={$isTaskPanelOpen}
      onclick={handleTasksClick}
      title="Tasks"
      aria-label="Toggle task panel"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path
          d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2"
        />
      </svg>
      {#if $currentInstanceTaskCount > 0}
        <span class="tasks-badge">{$currentInstanceTaskCount}</span>
      {/if}
    </button>
    <button
      class="action-btn chat-btn"
      class:active={$isChatOpen}
      onclick={handleChatClick}
      title="Chat"
      aria-label="Toggle chat panel"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z" />
      </svg>
      {#if $totalUnread > 0}
        <span class="chat-badge">{$totalUnread > 99 ? '99+' : $totalUnread}</span>
      {/if}
    </button>
  </div>
</header>

{#if popoverTarget}
  {@const stateInfo = getStateInfo(
    popoverTarget.instance.id,
    popoverTarget.instance.claude_state,
    popoverTarget.instance.claude_state_stale
  )}
  <InstancePopover
    instance={popoverTarget.instance}
    {stateInfo}
    anchorRect={popoverTarget.anchorRect}
    onclose={() => (popoverTarget = null)}
  />
{/if}

{#if showCreateModal}
  <CreateInstanceModal onclose={() => (showCreateModal = false)} />
{/if}

{#if showBugReport}
  <BugReportModal onclose={() => (showBugReport = false)} />
{/if}

<style>
  .main-header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px 12px;
    background: linear-gradient(180deg, var(--surface-600) 0%, var(--surface-700) 100%);
    border-bottom: 1px solid var(--surface-border);
    flex-shrink: 0;
    box-shadow: var(--elevation-low);
    min-height: 40px;
  }

  /* Left: Project identity */
  .header-project {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }

  .project-name {
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.08em;
    color: var(--chrome-accent-400);
    text-shadow: var(--emphasis-strong);
    text-transform: uppercase;
    font-family: var(--font-display);
  }

  .project-count {
    font-size: 9px;
    color: var(--text-muted);
    letter-spacing: 0.05em;
  }

  .connection-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .connection-dot.signal-lost:not(.signal-restored) {
    animation: dot-blink 1.5s ease-in-out infinite;
  }

  .connection-dot.signal-restored {
    animation: dot-flash 0.5s ease-out;
  }

  @keyframes dot-blink {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.3;
    }
  }

  @keyframes dot-flash {
    0% {
      box-shadow: 0 0 8px currentColor;
    }
    100% {
      box-shadow: none;
    }
  }

  /* Center: Fleet */
  .header-fleet {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1;
    min-width: 0;
    padding: 2px 0;
  }

  /* Right: Actions */
  .header-actions {
    display: flex;
    gap: 6px;
    flex-shrink: 0;
  }

  .action-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 5px;
    background: linear-gradient(180deg, var(--surface-500) 0%, var(--surface-600) 100%);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.15s ease;
    min-width: 28px;
    min-height: 28px;
  }

  .action-btn:hover {
    background: linear-gradient(180deg, var(--surface-400) 0%, var(--surface-500) 100%);
    border-color: var(--surface-border-light);
    color: var(--text-primary);
  }

  .action-btn.active {
    background: linear-gradient(180deg, var(--tint-focus) 0%, var(--tint-active) 100%);
    border-color: var(--chrome-accent-600);
    color: var(--chrome-accent-400);
    box-shadow: var(--elevation-low);
    text-shadow: var(--emphasis);
  }

  .action-btn svg {
    width: 12px;
    height: 12px;
    flex-shrink: 0;
  }

  .tasks-btn,
  .chat-btn {
    position: relative;
  }

  .tasks-badge,
  .chat-badge {
    position: absolute;
    top: 0;
    right: 0;
    min-width: 14px;
    height: 14px;
    padding: 0 3px;
    font-size: 8px;
    font-weight: 700;
    line-height: 14px;
    text-align: center;
    border-radius: 7px;
    background: var(--chrome-accent-500);
    color: var(--surface-900);
    box-shadow: var(--elevation-low);
  }

  .chat-badge {
    animation: badge-pulse 2s ease-in-out infinite;
  }

  @keyframes badge-pulse {
    0%,
    100% {
      box-shadow: var(--elevation-low);
    }
    50% {
      box-shadow: var(--elevation-high);
    }
  }

  /* Mobile */
  @media (max-width: 639px) {
    .main-header {
      padding: 4px 8px;
      gap: 6px;
    }

    .header-project {
      display: none;
    }

    /* Hide fleet on mobile — bottom tab bar in LayoutTree handles instance switching */
    .header-fleet {
      display: none;
    }

    .header-actions {
      gap: 4px;
    }
  }

  /* Analog theme */
  :global([data-theme='analog']) .main-header {
    background-color: var(--surface-800);
    background-image: var(--grain-fine), var(--grain-coarse), var(--ink-wash);
    background-blend-mode: multiply, multiply, normal;
    border-bottom-width: 2px;
  }

  :global([data-theme='analog']) .action-btn {
    background-color: var(--surface-600);
    background-image: var(--grain-fine);
    background-blend-mode: multiply;
    border-width: 1.5px;
    box-shadow: var(--elevation-low);
  }

  :global([data-theme='analog']) .action-btn:hover {
    background-color: var(--surface-500);
    background-image: var(--grain-fine);
    background-blend-mode: multiply;
  }

  :global([data-theme='analog']) .action-btn.active {
    background-color: var(--tint-active-strong);
    background-image: var(--grain-fine), var(--ink-wash);
    background-blend-mode: multiply, normal;
    border-width: 2px;
  }

  :global([data-theme='analog']) .chat-badge {
    animation: none;
    box-shadow: var(--elevation-low);
  }
</style>
