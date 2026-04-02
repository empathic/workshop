<script lang="ts">
  import SnakeTeaser from '../SnakeTeaser.svelte';
  import SnakeGame from '../SnakeGame.svelte';
  import { projects } from '$lib/stores/projects';
  import { openFullscreen } from '$lib/stores/fullscreen';

  // Easter egg: triple-click the monitor icon to launch snake
  let clicks = $state(0);
  let clickTimer: ReturnType<typeof setTimeout> | null = null;
  let showSnake = $state(false);

  let hasProjects = $derived($projects.length > 0);

  function onIconClick() {
    clicks++;
    if (clickTimer) clearTimeout(clickTimer);
    clickTimer = setTimeout(() => {
      clicks = 0;
    }, 2000);
    if (clicks >= 3) {
      clicks = 0;
      showSnake = true;
    }
  }
</script>

{#if showSnake}
  <SnakeGame
    onexit={() => {
      showSnake = false;
    }}
  />
{:else}
  <div class="landing">
    <div class="empty-content">
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="empty-icon"
        onclick={onIconClick}
        style="opacity: {0.3 + clicks * 0.25}; filter: drop-shadow(0 0 {clicks * 8}px var(--accent-500));"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
          <path
            d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z"
          />
        </svg>
        <div class="monitor-screen">
          <SnakeTeaser />
        </div>
      </div>
      {#if hasProjects}
        <h2>No Project Selected</h2>
        <p>Select a project from the sidebar to get started</p>
      {:else}
        <h2>No Projects</h2>
        <p>Create a project to get started</p>
      {/if}
      <button class="new-project-btn" onclick={() => openFullscreen('new-project')}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <line x1="12" y1="5" x2="12" y2="19" />
          <line x1="5" y1="12" x2="19" y2="12" />
        </svg>
        NEW PROJECT
      </button>
    </div>
  </div>
{/if}

<style>
  .landing {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 1;
  }

  .empty-content {
    text-align: center;
    color: var(--text-muted);
  }

  .empty-icon {
    position: relative;
    width: 80px;
    height: 80px;
    margin: 0 auto 20px;
    opacity: 0.3;
    color: var(--chrome-accent-500);
    cursor: pointer;
    transition:
      opacity 0.2s ease,
      filter 0.2s ease;
  }

  .empty-icon svg {
    width: 100%;
    height: 100%;
  }

  .monitor-screen {
    position: absolute;
    left: 10px;
    top: 10px;
    width: 60px;
    height: 33px;
    overflow: hidden;
    border-radius: 1px;
  }

  .empty-content h2 {
    margin: 0 0 12px;
    font-size: 14px;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-secondary);
  }

  .empty-content p {
    margin: 0;
    font-size: 12px;
    letter-spacing: 0.05em;
  }

  .new-project-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-top: 20px;
    padding: 6px 16px;
    font-size: 0.68rem;
    font-weight: 700;
    font-family: inherit;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--chrome-accent-400);
    background: transparent;
    border: 1px solid var(--chrome-accent-600);
    border-radius: 3px;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .new-project-btn:hover {
    background: var(--tint-hover);
    border-color: var(--chrome-accent-400);
  }

  .new-project-btn svg {
    width: 12px;
    height: 12px;
  }
</style>
