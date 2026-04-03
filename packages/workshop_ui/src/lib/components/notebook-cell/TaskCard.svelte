<script lang="ts">
  import type { ToolCell } from '$lib/types';
  import { getToolConfig } from '$lib/utils/tool-registry';

  interface Props {
    tool: ToolCell;
    agentLog?: Array<{
      content: string;
      agentId?: string;
      role?: string;
      tools?: Array<{ name: string; input?: Record<string, unknown> }>;
    }>;
  }

  let { tool, agentLog }: Props = $props();

  let showRaw = $state(false);
  let promptExpanded = $state(false);
  let resultExpanded = $state(false);

  const status = $derived(tool.is_error ? 'error' : tool.output ? 'complete' : 'running');

  const agentType = $derived(typeof tool.input.subagent_type === 'string' ? tool.input.subagent_type : null);

  const description = $derived(typeof tool.input.description === 'string' ? tool.input.description : null);

  const prompt = $derived(typeof tool.input.prompt === 'string' ? tool.input.prompt : null);

  const resultLong = $derived((tool.output?.length ?? 0) > 500);
</script>

<div
  class="task-card"
  class:running={status === 'running'}
  class:complete={status === 'complete'}
  class:error={status === 'error'}
>
  {#if showRaw}
    <div class="raw-view">
      <div class="raw-header">
        <span class="raw-title">RAW — TASK</span>
        <button class="toggle-raw" onclick={() => (showRaw = false)} title="Show rendered">&#9670;</button>
      </div>
      <div class="raw-field">
        <span class="raw-label">INPUT</span>
        <pre class="raw-value">{JSON.stringify(tool.input, null, 2)}</pre>
      </div>
      <div class="raw-field">
        <span class="raw-label">OUTPUT</span>
        <pre class="raw-value">{tool.output ?? '(none)'}</pre>
      </div>
      {#if agentLog && agentLog.length > 0}
        <div class="raw-field">
          <span class="raw-label">AGENT LOG ({agentLog.length} entries)</span>
          <pre class="raw-value">{JSON.stringify(agentLog, null, 2)}</pre>
        </div>
      {/if}
    </div>
  {:else}
    <!-- Header -->
    <div class="card-header">
      <div class="header-left">
        <span class="header-label">TASK</span>
        {#if agentType}
          <span class="agent-chip">{agentType}</span>
        {/if}
        <span
          class="status-led"
          class:led-running={status === 'running'}
          class:led-complete={status === 'complete'}
          class:led-error={status === 'error'}
        ></span>
      </div>
      <button class="toggle-raw" onclick={() => (showRaw = true)} title="Show raw">&#9671;</button>
    </div>

    <!-- Description -->
    {#if description}
      <div class="section description-section">
        <span class="section-content">{description}</span>
      </div>
    {/if}

    <!-- Prompt (collapsible) -->
    {#if prompt}
      <div class="section prompt-section">
        <button class="section-toggle" onclick={() => (promptExpanded = !promptExpanded)}>
          <span class="section-label">PROMPT</span>
          <span class="toggle-icon">{promptExpanded ? '\u25BC' : '\u25B6'}</span>
        </button>
        {#if promptExpanded}
          <pre class="section-body">{prompt}</pre>
        {/if}
      </div>
    {/if}

    <!-- Activity timeline -->
    {#if agentLog && agentLog.length > 0}
      <div class="section activity-section">
        <span class="section-label">ACTIVITY</span>
        <div class="activity-log">
          {#each agentLog as entry}
            <div class="activity-entry" class:entry-response={entry.role === 'agent_assistant'}>
              <span class="activity-arrow">{entry.role === 'agent_assistant' ? '\u2190' : '\u2192'}</span>
              <div class="activity-body">
                {#if entry.tools && entry.tools.length > 0}
                  <div class="activity-tools">
                    {#each entry.tools as t}
                      <span class="activity-tool-badge" title={t.name}>
                        <span class="atb-icon">{getToolConfig(t.name).icon}</span>
                        <span class="atb-name">{t.name}</span>
                      </span>
                    {/each}
                  </div>
                {/if}
                {#if entry.content}
                  <span class="activity-text">{entry.content}</span>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      </div>
    {:else if status === 'running'}
      <div class="section activity-section">
        <span class="section-label">ACTIVITY</span>
        <div class="activity-waiting">
          <span class="waiting-dots">...</span>
          <span class="waiting-text">Agent working</span>
        </div>
      </div>
    {/if}

    <!-- Result -->
    {#if tool.output}
      <div class="section result-section" class:error-result={tool.is_error}>
        {#if resultLong && !resultExpanded}
          <button class="section-toggle" onclick={() => (resultExpanded = true)}>
            <span class="section-label">{tool.is_error ? 'ERROR' : 'RESULT'}</span>
            <span class="toggle-icon">&#9654;</span>
          </button>
          <pre class="section-body truncated">{tool.output.slice(0, 500)}&#8230;</pre>
        {:else if resultLong}
          <button class="section-toggle" onclick={() => (resultExpanded = false)}>
            <span class="section-label">{tool.is_error ? 'ERROR' : 'RESULT'}</span>
            <span class="toggle-icon">&#9660;</span>
          </button>
          <pre class="section-body">{tool.output}</pre>
        {:else}
          <span class="section-label">{tool.is_error ? 'ERROR' : 'RESULT'}</span>
          <pre class="section-body">{tool.output}</pre>
        {/if}
      </div>
    {/if}
  {/if}
</div>

<style>
  .task-card {
    border: 1px solid var(--accent-600);
    border-radius: 4px;
    background: var(--surface-800);
    overflow: hidden;
    animation: card-on 0.3s ease-out;
  }

  .task-card.running {
    border-color: var(--accent-500);
    box-shadow: var(--elevation-low);
  }

  .task-card.complete {
    border-color: var(--surface-border);
    opacity: 0.85;
  }

  .task-card.error {
    border-color: var(--status-red);
  }

  /* ── Header ──────────────────────────────── */

  .card-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-bottom: 1px solid var(--surface-border);
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .header-label {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.15em;
    color: var(--accent-400);
    text-shadow: var(--emphasis);
  }

  .agent-chip {
    display: inline-block;
    padding: 1px 6px;
    background: var(--tint-active);
    border: 1px solid var(--accent-600);
    border-radius: 3px;
    font-size: 9px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--accent-400);
  }

  .status-led {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .status-led.led-running {
    background: var(--accent-500);
    box-shadow: 0 0 6px var(--accent-500);
    animation: led-pulse 1s ease-in-out infinite alternate;
  }

  .status-led.led-complete {
    background: var(--status-green);
    box-shadow: 0 0 4px var(--status-green);
  }

  .status-led.led-error {
    background: var(--status-red);
    box-shadow: 0 0 6px var(--status-red);
  }

  /* ── Sections ────────────────────────────── */

  .section {
    padding: 8px 12px;
    border-top: 1px solid var(--surface-border);
  }

  .section:first-child {
    border-top: none;
  }

  .section-label {
    display: block;
    font-size: 9px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.15em;
    color: var(--accent-400);
    margin-bottom: 4px;
  }

  .section-content {
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-primary);
  }

  .section-toggle {
    display: flex;
    align-items: center;
    gap: 6px;
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    margin-bottom: 4px;
    font-family: inherit;
  }

  .section-toggle .section-label {
    margin-bottom: 0;
    cursor: pointer;
  }

  .section-toggle:hover .section-label {
    color: var(--accent-300);
  }

  .toggle-icon {
    font-size: 8px;
    color: var(--text-muted);
  }

  .section-body {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
    font-family: inherit;
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-primary);
    max-height: 400px;
    overflow-y: auto;
  }

  .section-body.truncated {
    max-height: none;
  }

  /* ── Description ─────────────────────────── */

  .description-section {
    border-top: none;
  }

  /* ── Prompt ───────────────────────────────── */

  .prompt-section {
    background: var(--surface-700);
  }

  /* ── Activity log ────────────────────────── */

  .activity-section {
    background: var(--surface-700);
  }

  .activity-log {
    max-height: 300px;
    overflow-y: auto;
  }

  .activity-entry {
    display: flex;
    gap: 6px;
    padding: 3px 0;
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-primary);
  }

  .activity-entry + .activity-entry {
    border-top: 1px solid var(--surface-border);
  }

  .activity-arrow {
    flex-shrink: 0;
    color: var(--accent-400);
    font-size: 10px;
    width: 14px;
    text-align: center;
    line-height: 1.5;
  }

  .activity-entry.entry-response .activity-arrow {
    color: var(--text-muted);
  }

  .activity-body {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }

  .activity-tools {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .activity-tool-badge {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 1px 6px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    font-size: 9px;
    color: var(--accent-400);
  }

  .atb-icon {
    font-size: 10px;
  }

  .atb-name {
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.1em;
  }

  .activity-text {
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--text-primary);
  }

  .activity-waiting {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 0;
  }

  .waiting-dots {
    color: var(--accent-400);
    animation: dots-pulse 1.4s ease-in-out infinite;
  }

  .waiting-text {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-muted);
  }

  /* ── Result ───────────────────────────────── */

  .result-section {
    background: var(--surface-700);
    position: relative;
  }

  .result-section.error-result {
    border-top-color: var(--status-red);
  }

  .result-section.error-result .section-label {
    color: var(--status-red-text);
  }

  .result-section.error-result .section-body {
    color: var(--status-red-muted);
  }

  /* Scanline overlay on result */
  .result-section::after {
    content: '';
    position: absolute;
    inset: 0;
    background: repeating-linear-gradient(
      0deg,
      transparent,
      transparent 2px,
      var(--scanline-color) 2px,
      var(--scanline-color) 4px
    );
    pointer-events: none;
    border-radius: 2px;
  }

  /* ── Raw view ────────────────────────────── */

  .raw-view {
    padding: 0;
  }

  .raw-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 10px;
    border-bottom: 1px solid var(--surface-border);
  }

  .raw-title {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.15em;
    color: var(--accent-400);
  }

  .raw-field {
    padding: 6px 10px;
    border-bottom: 1px solid var(--surface-border);
  }

  .raw-field:last-child {
    border-bottom: none;
  }

  .raw-label {
    display: block;
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.15em;
    color: var(--text-muted);
    margin-bottom: 2px;
  }

  .raw-value {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-all;
    font-family: inherit;
    font-size: 10px;
    line-height: 1.5;
    color: var(--text-secondary);
    max-height: 200px;
    overflow-y: auto;
  }

  /* ── Toggle raw button ───────────────────── */

  .toggle-raw {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 12px;
    padding: 2px 6px;
    border-radius: 3px;
    opacity: 0.3;
    transition: all 0.15s ease;
  }

  .task-card:hover .toggle-raw {
    opacity: 0.8;
  }

  .toggle-raw:hover {
    background: var(--surface-500);
    color: var(--accent-400);
  }

  /* ── Animations ──────────────────────────── */

  @keyframes card-on {
    0% {
      opacity: 0;
      filter: brightness(3);
    }
    30% {
      opacity: 0.5;
      filter: brightness(2);
    }
    60% {
      opacity: 0.8;
      filter: brightness(1.2);
    }
    100% {
      opacity: 1;
      filter: brightness(1);
    }
  }

  @keyframes led-pulse {
    0% {
      opacity: 0.6;
    }
    100% {
      opacity: 1;
      box-shadow:
        0 0 8px currentColor,
        0 0 16px currentColor;
    }
  }

  @keyframes dots-pulse {
    0%,
    100% {
      opacity: 0.3;
    }
    50% {
      opacity: 1;
    }
  }

  /* ── Mobile ──────────────────────────────── */

  @media (max-width: 639px) {
    .card-header {
      padding: 6px 10px;
    }

    .section {
      padding: 6px 10px;
    }

    .section-content {
      font-size: 11px;
    }

    .section-body {
      font-size: 10px;
    }

    .toggle-raw {
      opacity: 0.6;
      padding: 4px 8px;
      font-size: 14px;
    }
  }

  /* ── Analog theme ────────────────────────── */

  :global([data-theme='analog']) .task-card {
    background-color: var(--surface-800);
    background-image: var(--grain-fine);
    background-blend-mode: multiply;
    border-color: var(--surface-border);
  }

  :global([data-theme='analog']) .task-card {
    animation: ink-bleed 0.5s cubic-bezier(0.1, 0.9, 0.2, 1);
  }

  :global([data-theme='analog']) .result-section::after {
    display: none;
  }

  @keyframes ink-bleed {
    0% {
      opacity: 0;
      transform: scaleY(0.95);
    }
    100% {
      opacity: 1;
      transform: scaleY(1);
    }
  }
</style>
