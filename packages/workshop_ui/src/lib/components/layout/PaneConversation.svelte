<script lang="ts">
  import { setContext } from 'svelte';
  import ConversationView from '../ConversationView.svelte';
  import Terminal from '../Terminal.svelte';
  import ErrorBoundary from '../ErrorBoundary.svelte';
  import { togglePaneViewMode } from '$lib/stores/layout';

  interface Props {
    instanceId: string;
    viewMode: 'structured' | 'raw';
    paneId: string;
  }

  let { instanceId, viewMode, paneId }: Props = $props();

  // Expose paneId via context so deeply-nested components (QuestionCard, PlanCard) can reach it.
  // Use a getter to avoid Svelte's state_referenced_locally warning — paneId is reactive via $props().
  setContext('paneId', {
    get id() {
      return paneId;
    }
  });
</script>

<div class="pane-conversation">
  <div class="view-toggle-bar">
    <button
      class="view-toggle"
      role="switch"
      aria-checked={viewMode === 'raw'}
      aria-label="Toggle raw view"
      onclick={() => togglePaneViewMode(paneId)}
    >
      <span class="toggle-label" class:active={viewMode === 'structured'}>Structured</span>
      <span class="toggle-track">
        <span class="toggle-thumb" class:on={viewMode === 'raw'}></span>
      </span>
      <span class="toggle-label" class:active={viewMode === 'raw'}>Raw</span>
    </button>
  </div>

  <div class="pane-content-inner">
    <ErrorBoundary>
      {#snippet children()}
        {#if viewMode === 'structured'}
          {#key 'conversation-' + instanceId}
            <ConversationView {instanceId} />
          {/key}
        {:else}
          <div class="pane-terminal">
            {#key 'terminal-' + instanceId}
              <Terminal {instanceId} {paneId} />
            {/key}
          </div>
        {/if}
      {/snippet}
    </ErrorBoundary>
  </div>
</div>

<style>
  .pane-conversation {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }

  .view-toggle-bar {
    display: flex;
    align-items: center;
    justify-content: flex-start;
    height: 24px;
    padding-left: 8px;
    background: var(--surface-800);
    border-bottom: 1px solid var(--surface-border);
    flex-shrink: 0;
  }

  .view-toggle {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: transparent;
    border: none;
    cursor: pointer;
    padding: 0;
    font-family: inherit;
  }

  .toggle-label {
    font-size: 9px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-muted);
    transition: color 0.15s ease;
  }

  .toggle-label.active {
    color: var(--chrome-accent-400);
  }

  .toggle-track {
    position: relative;
    width: 24px;
    height: 12px;
    background: var(--surface-600);
    border: 1px solid var(--surface-border);
    border-radius: 6px;
  }

  .toggle-thumb {
    position: absolute;
    top: 1px;
    left: 1px;
    width: 8px;
    height: 8px;
    background: var(--text-muted);
    border-radius: 50%;
    transition:
      transform 0.15s ease,
      background 0.15s ease;
  }

  .toggle-thumb.on {
    transform: translateX(12px);
    background: var(--chrome-accent-400);
  }

  .pane-content-inner {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .pane-terminal {
    flex: 1;
    min-height: 0;
  }
</style>
