<script lang="ts">
  /**
   * DebugPanel - Performance metrics overlay
   *
   * Toggle with Ctrl+Shift+D (or Cmd+Shift+D on Mac).
   * Shows real-time metrics for debugging performance issues.
   * Drag by title bar to reposition.
   */

  import { metrics, debugPanelVisible, avatarHitRate, voiceBackendOverride } from '$lib/stores/metrics';
  import { availableVoiceBackends, type VoiceBackend } from '$lib/utils/voice';
  import { onMount } from 'svelte';

  let panelEl: HTMLDivElement | undefined = $state();
  let position: { x: number; y: number } | null = $state<{ x: number; y: number } | null>(null);
  let dragging = $state(false);
  let dragOffset = { x: 0, y: 0 };
  let errorsExpanded = $state(false);

  // Available backends for the switcher
  let backends = $state<VoiceBackend[]>([]);
  onMount(() => {
    availableVoiceBackends().then((b) => {
      backends = b;
    });
  });

  function setBackendOverride(backend: VoiceBackend | null) {
    voiceBackendOverride.set(backend === 'none' ? null : backend);
  }

  function onPointerDown(e: PointerEvent) {
    if (!panelEl) return;
    dragging = true;
    const rect = panelEl.getBoundingClientRect();
    dragOffset = { x: e.clientX - rect.left, y: e.clientY - rect.top };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onPointerMove(e: PointerEvent) {
    if (!dragging || !panelEl) return;
    const maxX = window.innerWidth - panelEl.offsetWidth;
    const maxY = window.innerHeight - panelEl.offsetHeight;
    position = {
      x: Math.max(0, Math.min(e.clientX - dragOffset.x, maxX)),
      y: Math.max(0, Math.min(e.clientY - dragOffset.y, maxY))
    };
  }

  function onPointerUp() {
    dragging = false;
  }

  const panelStyle = $derived(position ? `left: ${position.x}px; top: ${position.y}px;` : '');
</script>

{#if $debugPanelVisible}
  <div class="debug-panel" class:dragged={position !== null} style={panelStyle} bind:this={panelEl}>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <h4 class:grabbing={dragging} onpointerdown={onPointerDown} onpointermove={onPointerMove} onpointerup={onPointerUp}>
      Performance Metrics
    </h4>

    <section>
      <h5>VirtualList</h5>
      <div class="metric">
        <span class="label">Items</span>
        <span class="value">{$metrics.virtualList.renderedItems}/{$metrics.virtualList.totalItems}</span>
      </div>
      <div class="metric">
        <span class="label">Height cache</span>
        <span class="value">{$metrics.virtualList.heightCacheSize}</span>
      </div>
      <div class="metric">
        <span class="label">Last render</span>
        <span class="value">{$metrics.virtualList.lastRenderMs.toFixed(1)}ms</span>
      </div>
    </section>

    <section>
      <h5>Avatar Cache</h5>
      <div class="metric">
        <span class="label">Size</span>
        <span class="value">{$metrics.avatar.cacheSize}/100</span>
      </div>
      <div class="metric">
        <span class="label">Hit rate</span>
        <span class="value">{($avatarHitRate * 100).toFixed(1)}%</span>
      </div>
    </section>

    <section>
      <h5>Terminal Buffers</h5>
      <div class="metric">
        <span class="label">Instances</span>
        <span class="value">{$metrics.terminal.bufferCount}</span>
      </div>
      <div class="metric">
        <span class="label">Total chunks</span>
        <span class="value">{$metrics.terminal.totalChunks}</span>
      </div>
      <div class="metric">
        <span class="label">Near capacity</span>
        <span class="value" class:warning={$metrics.terminal.nearCapacityCount > 0}>
          {$metrics.terminal.nearCapacityCount}
        </span>
      </div>
    </section>

    <section>
      <h5>WebSocket</h5>
      <div class="metric">
        <span class="label">Messages/sec</span>
        <span class="value">{$metrics.websocket.messagesPerSecond.toFixed(1)}</span>
      </div>
      <div class="metric">
        <span class="label">Total received</span>
        <span class="value">{$metrics.websocket.messagesReceived}</span>
      </div>
      <div class="metric">
        <span class="label">Reconnects</span>
        <span class="value" class:warning={$metrics.websocket.reconnectCount > 0}>
          {$metrics.websocket.reconnectCount}
        </span>
      </div>
    </section>

    <section>
      <h5>Voice Input</h5>
      <div class="metric">
        <span class="label">Backend</span>
        <span class="value">{$metrics.voice.backend}</span>
      </div>
      {#if backends.length > 0}
        <div class="backend-switcher">
          <button
            class="backend-btn"
            class:active={$voiceBackendOverride === null}
            onclick={() => setBackendOverride(null)}>auto</button
          >
          {#each backends as b}
            <button class="backend-btn" class:active={$voiceBackendOverride === b} onclick={() => setBackendOverride(b)}
              >{b}</button
            >
          {/each}
        </div>
      {/if}
      <div class="metric">
        <span class="label">State</span>
        <span class="value">{$metrics.voice.state}</span>
      </div>
      <div class="metric">
        <span class="label">Transcriptions</span>
        <span class="value">{$metrics.voice.transcriptionCount}</span>
      </div>
      <div class="metric">
        <span class="label">Errors</span>
        {#if $metrics.voice.errorCount > 0}
          <button class="error-toggle" onclick={() => (errorsExpanded = !errorsExpanded)}>
            {$metrics.voice.errorCount}
            {errorsExpanded ? '▾' : '▸'}
          </button>
        {:else}
          <span class="value">0</span>
        {/if}
      </div>
      {#if errorsExpanded && $metrics.voice.errors.length > 0}
        <ul class="error-log">
          {#each $metrics.voice.errors as error}
            <li>{error}</li>
          {/each}
        </ul>
      {/if}
      {#if ($metrics.voice.backend === 'prompt-api' || $metrics.voice.backend === 'hybrid') && $metrics.voice.lastTranscribeMs > 0}
        <div class="metric">
          <span class="label">Last transcribe</span>
          <span class="value">{$metrics.voice.lastTranscribeMs}ms</span>
        </div>
      {/if}
    </section>

    <div class="hint">Press Ctrl+Shift+D to close</div>
  </div>
{/if}

<style>
  .debug-panel {
    position: fixed;
    bottom: 1rem;
    right: 1rem;
    background: var(--surface-900);
    border: 1px solid var(--chrome-accent-600);
    border-radius: 4px;
    padding: 12px;
    font-size: 11px;
    font-family: var(--font-mono);
    z-index: 9999;
    min-width: 200px;
    max-width: 280px;
    box-shadow: var(--shadow-dropdown);
  }

  .debug-panel.dragged {
    bottom: auto;
    right: auto;
  }

  h4 {
    margin: 0 0 8px;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--chrome-accent-400);
    cursor: grab;
    user-select: none;
    touch-action: none;
  }

  h4.grabbing {
    cursor: grabbing;
  }

  section {
    margin-bottom: 10px;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--surface-border);
  }

  section:last-of-type {
    border-bottom: none;
    margin-bottom: 8px;
  }

  h5 {
    margin: 0 0 4px;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--chrome-accent-500);
  }

  .metric {
    display: flex;
    justify-content: space-between;
    padding: 2px 0;
  }

  .label {
    color: var(--text-muted);
  }

  .value {
    color: var(--text-secondary);
    font-weight: 500;
  }

  .value.warning {
    color: var(--status-red-text);
  }

  .error-toggle {
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    color: var(--status-red-text);
    font: inherit;
    font-weight: 500;
  }

  .error-log {
    list-style: none;
    margin: 4px 0 0;
    padding: 0;
    max-height: 6em;
    overflow-y: auto;
    font-size: 9px;
    color: var(--status-red-text);
  }

  .error-log li {
    padding: 1px 0;
    word-break: break-all;
  }

  .backend-switcher {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
    margin: 4px 0 2px;
  }

  .backend-btn {
    background: none;
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    padding: 1px 6px;
    cursor: pointer;
    color: var(--text-muted);
    font: inherit;
    font-size: 9px;
    transition: all 0.1s ease;
  }

  .backend-btn:hover {
    border-color: var(--chrome-accent-600);
    color: var(--text-secondary);
  }

  .backend-btn.active {
    border-color: var(--chrome-accent-500);
    color: var(--chrome-accent-400);
    background: var(--tint-active);
  }

  .hint {
    font-size: 9px;
    color: var(--text-muted);
    text-align: center;
    margin-top: 4px;
  }
</style>
