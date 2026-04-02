<script lang="ts">
  import type { ToolCell } from '$lib/types';
  import { openFilePath, openExplorerWithSearch } from '$lib/stores/files';
  import { getToolConfig } from '$lib/utils/tool-registry';
  import QuestionCard from './QuestionCard.svelte';
  import TaskCard from './TaskCard.svelte';
  import PlanCard from './PlanCard.svelte';

  interface Props {
    toolCells: ToolCell[];
    agentLog?: Array<{ content: string; agentId?: string; role?: string }>;
  }

  let { toolCells, agentLog }: Props = $props();

  let toolsExpanded = $state(false);
  let expandedToolId: string | null = $state(null);

  // Widget components for card-mode tools.
  // Add entries here when registering a new card tool in TOOL_REGISTRY.
  const CARD_WIDGETS: Record<string, typeof QuestionCard | typeof TaskCard | typeof PlanCard> = {
    AskUserQuestion: QuestionCard,
    Task: TaskCard,
    ExitPlanMode: PlanCard
  };

  // Single derivation: pair each tool with its registry config once.
  const toolEntries = $derived(toolCells.map((tool) => ({ tool, config: getToolConfig(tool.name) })));
  const cardEntries = $derived(toolEntries.filter((e) => e.config.renderMode === 'card'));
  const badgeEntries = $derived(toolEntries.filter((e) => e.config.renderMode !== 'card'));

  const toolCount = $derived(badgeEntries.length);
  const shouldCollapse = $derived(toolCount > 4);

  const expandedEntry = $derived(
    expandedToolId ? (badgeEntries.find((e) => e.tool.id === expandedToolId) ?? null) : null
  );

  function toggleTool(toolId: string) {
    expandedToolId = expandedToolId === toolId ? null : toolId;
  }

  function handleFieldClick(field: { clickable?: 'file' | 'glob'; value: string }) {
    if (field.clickable === 'file') openFilePath(field.value);
    else if (field.clickable === 'glob') openExplorerWithSearch(field.value);
  }
</script>

<div class="cell-tools">
  <!-- Card tools (rendered as specialized widget cards, not badges) -->
  {#each cardEntries as { tool }}
    {@const Widget = CARD_WIDGETS[tool.name]}
    {#if Widget}
      <Widget {tool} {agentLog} />
    {/if}
  {/each}

  <!-- Badge tools -->
  {#if toolCount > 0}
    {#if shouldCollapse && !toolsExpanded}
      <button class="tools-collapsed-toggle" onclick={() => (toolsExpanded = true)}>
        <span class="tools-count">{toolCount} tool operations</span>
        <span class="tools-expand-icon">&#9654;</span>
      </button>
    {:else}
      <div class="badges-row">
        {#each badgeEntries as { tool, config }}
          {@const label = config.badgeLabel ? config.badgeLabel(tool.input) : null}
          {@const isExpanded = expandedToolId === tool.id}
          <button
            class="tool-badge"
            class:file-tool={label?.style === 'file'}
            class:expanded={isExpanded}
            onclick={() => toggleTool(tool.id)}
            title={label?.title ?? tool.name}
          >
            <span class="tool-icon">{config.icon}</span>
            <span class="tool-name">{tool.name}</span>
            {#if label}
              <span class={label.style === 'file' ? 'tool-file' : 'tool-detail'}>{label.text}</span>
            {/if}
            {#if isExpanded}
              <span class="expand-indicator">▼</span>
            {/if}
          </button>
        {/each}
        {#if shouldCollapse}
          <button
            class="tools-collapse-toggle"
            onclick={() => {
              toolsExpanded = false;
              expandedToolId = null;
            }}
          >
            &#9650; Collapse
          </button>
        {/if}
      </div>

      <!-- Expanded tool detail panel -->
      {#if expandedEntry}
        {@const fields = expandedEntry.config.expandedFields
          ? expandedEntry.config.expandedFields(expandedEntry.tool)
          : []}
        <div class="tool-detail-panel" class:error-panel={expandedEntry.tool.is_error}>
          {#if fields.length > 0}
            <div class="detail-section">
              {#each fields as field}
                <div class="detail-field">
                  <span class="detail-label">{field.label}</span>
                  {#if field.clickable}
                    <button class="detail-value clickable" onclick={() => handleFieldClick(field)}>
                      {field.value}
                      <span class="open-arrow">&rarr;</span>
                    </button>
                  {:else}
                    <pre class="detail-value">{field.value}</pre>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}
          {#if expandedEntry.tool.output}
            <div class="detail-section output-section" class:error-output={expandedEntry.tool.is_error}>
              <span class="detail-label">{expandedEntry.tool.is_error ? 'ERROR' : 'OUTPUT'}</span>
              <pre class="detail-output">{expandedEntry.tool.output}</pre>
            </div>
          {/if}
          {#if expandedEntry.tool.name === 'Task' && agentLog && agentLog.length > 0}
            <div class="detail-section agent-log-section">
              <span class="detail-label">AGENT LOG</span>
              <div class="agent-log">
                {#each agentLog as entry}
                  <div class="agent-log-entry" class:agent-response={entry.role === 'agent_assistant'}>
                    <span class="agent-log-arrow">{entry.role === 'agent_assistant' ? '←' : '→'}</span>
                    <span class="agent-log-content">{entry.content}</span>
                  </div>
                {/each}
              </div>
            </div>
          {/if}
        </div>
      {/if}
    {/if}
  {/if}
</div>

<style>
  .cell-tools {
    margin-top: 12px;
    margin-left: 16px;
    padding-left: 12px;
    border-left: 1px solid var(--surface-border);
    display: flex;
    flex-direction: column;
    gap: 6px;
    position: relative;
  }

  /* Junction dot connecting tools to parent message */
  .cell-tools::before {
    content: '';
    position: absolute;
    left: -3px;
    top: 8px;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--surface-border-light);
  }

  .badges-row {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  /* Collapsed tools toggle */
  .tools-collapsed-toggle {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-secondary);
    font-family: inherit;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.05em;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .tools-collapsed-toggle:hover {
    background: var(--surface-600);
    border-color: var(--accent-600);
    color: var(--accent-400);
  }

  .tools-count {
    text-transform: uppercase;
  }

  .tools-expand-icon {
    font-size: 8px;
    opacity: 0.6;
  }

  .tools-collapse-toggle {
    background: none;
    border: none;
    color: var(--text-muted);
    font-family: inherit;
    font-size: 9px;
    cursor: pointer;
    padding: 4px 8px;
    opacity: 0.5;
    transition: opacity 0.15s ease;
    text-align: left;
  }

  .tools-collapse-toggle:hover {
    opacity: 1;
    color: var(--accent-400);
  }

  /* File path label on file operation tools */
  .tool-file,
  .tool-detail {
    font-weight: 400;
    font-size: 9px;
    color: var(--text-muted);
    letter-spacing: 0;
    text-transform: none;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tool-badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 5px 10px;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.05em;
    color: var(--accent-400);
    cursor: pointer;
    transition: all 0.15s ease;
    font-family: inherit;
  }

  .tool-badge:hover {
    background: var(--surface-600);
    border-color: var(--accent-600);
    box-shadow: var(--elevation-low);
  }

  .tool-badge:focus {
    outline: none;
    box-shadow: 0 0 0 2px var(--tint-selection);
  }

  .tool-badge.expanded {
    border-color: var(--accent-500);
    background: var(--surface-600);
    text-shadow: var(--emphasis);
  }

  /* File operation tool badges */
  .tool-badge.file-tool {
    border-left: 2px solid var(--accent-500);
  }

  .tool-badge.file-tool:hover {
    border-left-color: var(--accent-400);
    text-shadow: var(--emphasis);
  }

  .expand-indicator {
    font-size: 8px;
    opacity: 0.7;
    margin-left: 2px;
  }

  .tool-icon {
    font-size: 11px;
  }

  .tool-name {
    font-family: inherit;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.15em;
  }

  /* ── Expanded detail panel ─────────────────────────── */

  .tool-detail-panel {
    border: 1px solid var(--accent-600);
    border-radius: 3px;
    background: var(--surface-800);
    overflow: hidden;
    animation: panel-on 0.3s ease-out;
  }

  .tool-detail-panel.error-panel {
    border-color: var(--status-red);
  }

  .detail-section {
    padding: 10px 12px;
  }

  .detail-section + .detail-section {
    border-top: 1px solid var(--surface-border);
  }

  .detail-field {
    margin-bottom: 6px;
  }

  .detail-field:last-child {
    margin-bottom: 0;
  }

  .detail-label {
    display: block;
    font-size: 9px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.15em;
    color: var(--accent-400);
    margin-bottom: 3px;
  }

  pre.detail-value {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-all;
    font-family: inherit;
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-primary);
  }

  button.detail-value.clickable {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: none;
    border: none;
    padding: 0;
    font-family: inherit;
    font-size: 11px;
    color: var(--accent-400);
    cursor: pointer;
    text-decoration: underline;
    text-decoration-style: dotted;
    text-underline-offset: 3px;
  }

  button.detail-value.clickable:hover {
    color: var(--accent-300);
    text-shadow: var(--emphasis);
  }

  .open-arrow {
    font-size: 10px;
    opacity: 0.6;
  }

  .output-section {
    background: var(--surface-700);
    position: relative;
  }

  .output-section.error-output {
    border-top-color: var(--status-red);
  }

  .output-section.error-output .detail-label {
    color: var(--status-red-text);
  }

  .detail-output {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-all;
    font-family: inherit;
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-primary);
    max-height: 300px;
    overflow-y: auto;
  }

  .output-section.error-output .detail-output {
    color: var(--status-red-muted);
  }

  /* Scanline overlay on output — denser than main display */
  .output-section::after {
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

  /* ── Agent log (sub-agent activity inside Task) ─── */

  .agent-log-section {
    background: var(--surface-700);
  }

  .agent-log {
    max-height: 300px;
    overflow-y: auto;
  }

  .agent-log-entry {
    display: flex;
    gap: 6px;
    padding: 3px 0;
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-primary);
  }

  .agent-log-entry + .agent-log-entry {
    border-top: 1px solid var(--surface-border);
  }

  .agent-log-arrow {
    flex-shrink: 0;
    color: var(--accent-400);
    font-size: 10px;
    width: 14px;
    text-align: center;
  }

  .agent-log-entry.agent-response .agent-log-arrow {
    color: var(--text-muted);
  }

  .agent-log-content {
    white-space: pre-wrap;
    word-break: break-word;
  }

  @keyframes panel-on {
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

  /* Mobile */
  @media (max-width: 639px) {
    .cell-tools {
      margin-top: 10px;
      gap: 4px;
    }

    .badges-row {
      gap: 4px;
    }

    .tool-badge {
      padding: 4px 8px;
      font-size: 9px;
      gap: 4px;
    }

    .tool-icon {
      font-size: 10px;
    }

    .detail-output {
      max-height: 200px;
      font-size: 10px;
    }

    .tool-detail-panel {
      margin-top: 2px;
    }
  }

  /* Analog theme */
  :global([data-theme='analog']) .tool-badge {
    background-color: var(--surface-700);
    background-image: var(--grain-fine);
    background-blend-mode: multiply;
    border-width: 1.5px;
    box-shadow: var(--elevation-low);
  }

  :global([data-theme='analog']) .tool-detail-panel {
    background-color: var(--surface-800);
    background-image: var(--grain-fine);
    background-blend-mode: multiply;
    border-color: var(--surface-border);
  }

  :global([data-theme='analog']) .output-section::after {
    display: none;
  }

  :global([data-theme='analog']) .tool-detail-panel {
    animation: ink-bleed 0.5s cubic-bezier(0.1, 0.9, 0.2, 1);
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
