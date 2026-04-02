<script lang="ts">
  import type { NotebookCell } from '$lib/types';
  import { renderMarkdown } from '$lib/utils/markdown';
  import { copyable } from '$lib/utils/copyable';
  import { makeFilePathsClickable } from '$lib/utils/fileLinks';

  interface Props {
    cell: NotebookCell;
    showRaw: boolean;
    cellType: 'user' | 'assistant' | 'system' | 'unknown' | 'agent' | 'tool' | 'progress';
  }

  let { cell, showRaw, cellType }: Props = $props();

  const isUser = $derived(cellType === 'user');
  const isAgent = $derived(cellType === 'agent');
  const isUnknown = $derived(cellType === 'unknown');
  const hasThinking = $derived(!!cell.thinking);

  let showThinking = $state(false);

  const renderedContent = $derived(renderMarkdown(cell.content));
</script>

{#if showRaw}
  <!-- Raw view: show all content including thinking and extra data -->
  <div class="cell-content raw">
    {#if hasThinking}
      <div class="raw-section-label">THINKING:</div>
      <pre>{cell.thinking}</pre>
      <div class="raw-section-label">RESPONSE:</div>
    {/if}
    <pre>{cell.content}</pre>
    {#if isUnknown && cell.extra}
      <div class="raw-section-label">EXTRA DATA:</div>
      <pre>{JSON.stringify(cell.extra, null, 2)}</pre>
    {/if}
  </div>
{:else if isAgent}
  <!-- Agent progress entry - show sub-agent activity -->
  <div class="cell-content agent-content">
    {#if cell.agentPrompt}
      <div class="agent-prompt">
        <span class="agent-prompt-label">Task:</span>
        <span class="agent-prompt-text">{cell.agentPrompt}</span>
      </div>
    {/if}
    {#if cell.content}
      <div
        class="agent-message"
        class:agent-user={cell.agentMsgRole === 'agent_user'}
        class:agent-assistant={cell.agentMsgRole === 'agent_assistant'}
      >
        {#if cell.agentMsgRole === 'agent_user'}
          <span class="agent-msg-role">→</span>
        {:else if cell.agentMsgRole === 'agent_assistant'}
          <span class="agent-msg-role">←</span>
        {/if}
        <span class="agent-msg-content">{cell.content}</span>
      </div>
    {/if}
  </div>
{:else if isUnknown}
  <!-- Unknown entry - show entry type badge and content -->
  <div class="cell-content unknown-content">
    <div class="unknown-type-badge">
      <span class="unknown-icon">?</span>
      <span class="unknown-type">{cell.entryType ?? 'unknown'}</span>
    </div>
    {#if cell.content}
      <div class="unknown-preview">{cell.content}</div>
    {:else}
      <div class="unknown-empty">(no content)</div>
    {/if}
  </div>
{:else}
  <!-- Rendered view: collapsible thinking + rendered markdown -->
  {#if hasThinking}
    <div class="thinking-section" class:expanded={showThinking}>
      <button
        class="thinking-toggle"
        onclick={() => (showThinking = !showThinking)}
        title={showThinking ? 'Hide thinking' : 'Show thinking'}
      >
        <span class="thinking-icon">{showThinking ? '▼' : '▶'}</span>
        <span class="thinking-label">Extended thinking</span>
        <span class="thinking-preview">{showThinking ? '' : cell.thinking?.slice(0, 60) + '...'}</span>
      </button>
      {#if showThinking}
        <div class="thinking-content">
          <pre>{cell.thinking}</pre>
        </div>
      {/if}
    </div>
  {/if}

  <div class="cell-content markdown" class:user-content={isUser} use:copyable use:makeFilePathsClickable>
    {@html renderedContent}
  </div>
{/if}

<style>
  .cell-content {
    color: var(--text-primary);
    font-size: 13px;
    line-height: 1.7;
    word-break: break-word;
  }

  .cell-content.raw pre {
    margin: 0;
    white-space: pre-wrap;
    font-family: inherit;
    font-size: 12px;
    color: var(--text-secondary);
    background: var(--surface-700);
    padding: 12px;
    border-radius: 4px;
    border: 1px solid var(--surface-border);
    overflow-x: auto;
  }

  .raw-section-label {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.15em;
    color: var(--text-muted);
    margin-bottom: 4px;
    margin-top: 12px;
  }

  .raw-section-label:first-child {
    margin-top: 0;
  }

  .cell-content.markdown :global(p) {
    margin: 0 0 1em 0;
  }

  .cell-content.markdown :global(p:last-child) {
    margin-bottom: 0;
  }

  .cell-content.markdown :global(code) {
    background: var(--surface-600);
    padding: 2px 6px;
    border-radius: 3px;
    font-family: inherit;
    font-size: 12px;
    color: var(--accent-400);
    border: 1px solid var(--surface-border);
  }

  .cell-content.markdown :global(pre) {
    background: var(--surface-700);
    padding: 16px 16px 16px 56px;
    border-radius: 3px;
    border: 1px solid var(--surface-border);
    border-top-color: var(--surface-border-light);
    overflow-x: auto;
    margin: 1em 0;
    /* Embedded screen: inner glow + bezel */
    box-shadow: var(--recess), var(--recess-border);
    position: relative;
    transition: box-shadow 0.25s ease;
  }

  /* Inner scanline overlay on code blocks — second monitor effect */
  .cell-content.markdown :global(pre::after) {
    content: '';
    position: absolute;
    inset: 0;
    background: repeating-linear-gradient(
      0deg,
      transparent,
      transparent 1px,
      var(--scanline-color) 1px,
      var(--scanline-color) 2px
    );
    pointer-events: none;
    border-radius: 3px;
  }

  /* Hover: screen brightens when you look at it */
  .cell-content.markdown :global(pre:hover) {
    box-shadow: var(--recess), var(--recess-border), var(--elevation-low);
  }

  /* Copyable wrapper for code blocks and blockquotes */
  .cell-content.markdown :global(.copyable-wrapper) {
    position: relative;
    margin: 1em 0;
  }

  .cell-content.markdown :global(.copyable-wrapper pre),
  .cell-content.markdown :global(.copyable-wrapper blockquote) {
    margin: 0;
  }

  .cell-content.markdown :global(.copy-btn) {
    position: absolute;
    top: 8px;
    right: 8px;
    width: 28px;
    height: 28px;
    padding: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--surface-600);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
    opacity: 0;
    transition: all 0.15s ease;
    z-index: 10;
  }

  .cell-content.markdown :global(.copy-btn svg) {
    width: 14px;
    height: 14px;
  }

  .cell-content.markdown :global(.copyable-wrapper:hover .copy-btn) {
    opacity: 1;
  }

  .cell-content.markdown :global(.copy-btn:hover) {
    background: var(--surface-500);
    border-color: var(--accent-600);
    color: var(--accent-400);
  }

  .cell-content.markdown :global(.copy-btn.copied) {
    background: var(--status-green);
    border-color: var(--status-green);
    color: white;
    opacity: 1;
  }

  .cell-content.markdown :global(pre code) {
    background: none;
    padding: 0;
    border: none;
    color: var(--text-primary);
    font-size: 12px;
    line-height: 1.6;
  }

  .cell-content.markdown :global(strong) {
    font-weight: 700;
    color: var(--accent-300);
  }

  .cell-content.markdown :global(h1),
  .cell-content.markdown :global(h2),
  .cell-content.markdown :global(h3),
  .cell-content.markdown :global(h4) {
    color: var(--accent-400);
    margin: 1.5em 0 0.5em 0;
    font-weight: 700;
    letter-spacing: 0.02em;
    text-shadow: var(--emphasis-strong);
    transition: text-shadow 0.8s ease;
    font-family: var(--font-display);
  }

  .cell-content.markdown :global(h1:first-child),
  .cell-content.markdown :global(h2:first-child),
  .cell-content.markdown :global(h3:first-child) {
    margin-top: 0;
  }

  .cell-content.markdown :global(h1) {
    font-size: 1.4em;
  }
  .cell-content.markdown :global(h2) {
    font-size: 1.2em;
  }
  .cell-content.markdown :global(h3) {
    font-size: 1.05em;
  }

  .cell-content.markdown :global(ul),
  .cell-content.markdown :global(ol) {
    margin: 0.5em 0;
    padding-left: 1.5em;
  }

  .cell-content.markdown :global(li) {
    margin: 0.3em 0;
  }

  .cell-content.markdown :global(li::marker) {
    color: var(--accent-500);
  }

  .cell-content.markdown :global(table) {
    border-collapse: collapse;
    width: 100%;
    margin: 1em 0;
    font-size: 12px;
  }

  .cell-content.markdown :global(th),
  .cell-content.markdown :global(td) {
    border: 1px solid var(--surface-border);
    padding: 8px 12px;
    text-align: left;
  }

  .cell-content.markdown :global(th) {
    background: var(--surface-600);
    font-weight: 700;
    color: var(--accent-400);
    text-transform: uppercase;
    font-size: 10px;
    letter-spacing: 0.1em;
  }

  .cell-content.markdown :global(tr:nth-child(even)) {
    background: var(--tint-subtle);
  }

  .cell-content.markdown :global(blockquote) {
    border-left: 2px solid var(--accent-500);
    margin: 1em 0;
    padding: 0.5em 1em;
    color: var(--text-secondary);
    background: var(--tint-subtle);
    border-radius: 0 4px 4px 0;
  }

  .cell-content.markdown :global(a) {
    color: var(--accent-400);
    text-decoration: none;
    border-bottom: 1px solid transparent;
    transition: all 0.15s ease;
  }

  .cell-content.markdown :global(a:hover) {
    border-bottom-color: var(--accent-400);
    text-shadow: var(--emphasis);
  }

  .cell-content.markdown :global(hr) {
    border: none;
    border-top: 1px solid var(--surface-border);
    margin: 1.5em 0;
  }

  .cell-content.user-content {
    background: var(--surface-700);
    padding: 16px 20px;
    border-radius: 4px;
    border-left: none;
    font-weight: 500;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='4' height='4'%3E%3Crect width='1' height='1' fill='%23fdba74' opacity='0.02'/%3E%3Crect x='2' y='2' width='1' height='1' fill='%23fdba74' opacity='0.015'/%3E%3C/svg%3E");
    background-color: var(--surface-700);
    position: relative;
  }

  /* Terminal prompt marker */
  .cell-content.user-content::before {
    content: '>';
    position: absolute;
    left: -16px;
    top: 12px;
    color: var(--text-muted);
    font-size: 12px;
    font-weight: 700;
    opacity: 0.5;
  }

  .unknown-content {
    background: var(--surface-700);
    padding: 12px 14px;
    border-radius: 4px;
    border-left: 2px dashed var(--surface-border);
  }

  .unknown-type-badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    background: var(--surface-600);
    border: 1px dashed var(--surface-border);
    border-radius: 4px;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.05em;
    color: var(--text-muted);
    margin-bottom: 8px;
  }

  .unknown-icon {
    font-size: 11px;
    opacity: 0.6;
  }

  .unknown-type {
    font-family: inherit;
    text-transform: uppercase;
  }

  .unknown-preview {
    font-size: 12px;
    color: var(--text-secondary);
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .unknown-empty {
    font-size: 11px;
    color: var(--text-muted);
    font-style: italic;
  }

  .agent-content {
    background: linear-gradient(180deg, var(--tint-thinking) 0%, var(--tint-subtle) 100%);
    padding: 10px 12px;
    border-radius: 4px;
    border-left: 2px solid var(--thinking-500);
    font-size: 12px;
  }

  .agent-prompt {
    display: flex;
    gap: 6px;
    margin-bottom: 8px;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--surface-border);
  }

  .agent-prompt-label {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--thinking-400);
    flex-shrink: 0;
  }

  .agent-prompt-text {
    font-size: 11px;
    color: var(--text-secondary);
    line-height: 1.4;
  }

  .agent-message {
    display: flex;
    align-items: flex-start;
    gap: 8px;
  }

  .agent-msg-role {
    font-size: 11px;
    font-weight: 700;
    color: var(--thinking-500);
    flex-shrink: 0;
    width: 12px;
  }

  .agent-message.agent-user .agent-msg-role {
    color: var(--text-muted);
  }

  .agent-message.agent-assistant .agent-msg-role {
    color: var(--thinking-400);
  }

  .agent-msg-content {
    font-size: 11px;
    color: var(--text-secondary);
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .agent-message.agent-assistant .agent-msg-content {
    color: var(--thinking-300);
  }

  .thinking-section {
    margin-bottom: 12px;
    border: 1px solid var(--tint-thinking-strong);
    border-radius: 3px;
    background: var(--tint-thinking);
    overflow: hidden;
  }

  .thinking-toggle {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 8px 12px;
    background: transparent;
    border: none;
    cursor: pointer;
    font-family: inherit;
    font-size: 11px;
    color: var(--thinking-400);
    text-align: left;
    transition: background 0.15s ease;
  }

  .thinking-toggle:hover {
    background: var(--tint-thinking-strong);
  }

  .thinking-icon {
    font-size: 9px;
    opacity: 0.8;
  }

  .thinking-label {
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    text-shadow: var(--emphasis);
  }

  .thinking-preview {
    flex: 1;
    font-size: 10px;
    color: var(--text-muted);
    font-weight: 400;
    text-transform: none;
    letter-spacing: normal;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    opacity: 0.7;
  }

  .thinking-content {
    padding: 12px;
    border-top: 1px solid var(--surface-border);
    background: var(--surface-800);
    position: relative;
    border-radius: 2px;
    box-shadow: var(--depth-down);
  }

  /* Purple scanline overlay — denser than the main display */
  .thinking-content::after {
    content: '';
    position: absolute;
    inset: 0;
    background: var(--texture-overlay);
    opacity: var(--texture-opacity);
    pointer-events: none;
    border-radius: 2px;
  }

  .thinking-content pre {
    margin: 0;
    white-space: pre-wrap;
    font-family: inherit;
    font-size: 10px;
    line-height: 1.5;
    color: var(--thinking-400);
    opacity: 0.9;
    position: relative;
    z-index: 1;
  }

  .thinking-section.expanded {
    border-color: var(--thinking-500);
    box-shadow: var(--elevation-low);
  }

  /* Screen power-on/off animation for expand/collapse */
  .thinking-section.expanded .thinking-content {
    animation: screen-on 0.3s ease-out;
  }

  @keyframes screen-on {
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

  @media (max-width: 639px) {
    .cell-content {
      font-size: 14px;
      line-height: 1.6;
    }

    .cell-content.raw pre {
      font-size: 11px;
      padding: 10px;
    }

    .cell-content.user-content {
      padding: 12px 16px;
    }

    /* Hide prompt marker on mobile — not enough space */
    .cell-content.user-content::before {
      display: none;
    }

    /* Markdown mobile adjustments */
    .cell-content.markdown :global(code) {
      font-size: 11px;
      padding: 1px 4px;
    }

    .cell-content.markdown :global(pre) {
      padding: 10px 12px 10px 48px;
      margin: 0.75em 0;
      max-width: calc(100vw - 76px);
    }

    .cell-content.markdown :global(pre code) {
      font-size: 11px;
    }

    .cell-content.markdown :global(h1) {
      font-size: 1.25em;
    }
    .cell-content.markdown :global(h2) {
      font-size: 1.1em;
    }
    .cell-content.markdown :global(h3) {
      font-size: 1em;
    }

    .cell-content.markdown :global(table) {
      font-size: 11px;
      display: block;
      overflow-x: auto;
      max-width: calc(100vw - 76px);
    }

    .cell-content.markdown :global(th),
    .cell-content.markdown :global(td) {
      padding: 6px 8px;
    }

    .cell-content.markdown :global(th) {
      font-size: 9px;
    }

    .cell-content.markdown :global(blockquote) {
      padding: 0.4em 0.8em;
      margin: 0.75em 0;
    }

    /* Thinking section mobile */
    .thinking-section {
      margin-bottom: 10px;
    }

    .thinking-toggle {
      padding: 6px 10px;
      font-size: 10px;
    }

    .thinking-label {
      font-size: 9px;
    }

    .thinking-preview {
      font-size: 9px;
    }

    .thinking-content {
      padding: 10px;
    }

    .thinking-content pre {
      font-size: 10px;
      max-height: 200px;
      overflow-y: auto;
    }

    /* Copy button mobile - always visible since no hover */
    .cell-content.markdown :global(.copy-btn) {
      opacity: 0.7;
      width: 32px;
      height: 32px;
    }

    .cell-content.markdown :global(.copy-btn svg) {
      width: 16px;
      height: 16px;
    }
  }

  :global([data-theme='analog']) .cell-content.user-content {
    background-color: var(--surface-700);
    background-image: var(--grain-fine), var(--grain-coarse), var(--ink-wash);
    background-blend-mode: multiply, multiply, normal;
    border-left: 2px solid var(--accent-600);
    box-shadow: var(--recess);
  }

  /* Replace the terminal prompt marker with a pen nib mark */
  :global([data-theme='analog']) .cell-content.user-content::before {
    content: '¶';
    font-size: 11px;
    opacity: 0.25;
    color: var(--accent-600);
  }

  /* Code blocks: inset plate, heavy left-margin rule */
  :global([data-theme='analog']) .cell-content.markdown :global(pre) {
    border: none;
    border-left: 3px solid var(--accent-600);
    border-radius: 0;
    background-color: var(--surface-700);
    background-image: var(--grain-fine), var(--grain-coarse);
    background-blend-mode: multiply, multiply;
    box-shadow:
      var(--recess),
      var(--recess);
  }

  :global([data-theme='analog']) .cell-content.markdown :global(pre::after) {
    display: none;
  }

  /* Hover: ink bleed deepens along the left edge */
  :global([data-theme='analog']) .cell-content.markdown :global(pre:hover) {
    box-shadow:
      var(--recess),
      var(--recess);
  }

  /* Inline code: stamp impression with fine grain */
  :global([data-theme='analog']) .cell-content.markdown :global(code) {
    background-color: var(--surface-700);
    background-image: var(--grain-fine);
    background-blend-mode: multiply;
    border-color: var(--surface-border);
    color: var(--text-primary);
    box-shadow: var(--elevation-low);
  }

  /* Agent content: manuscript passage with paper grain */
  :global([data-theme='analog']) .agent-content {
    background-color: var(--surface-800);
    background-image: var(--grain-fine), var(--ink-wash);
    background-blend-mode: multiply, normal;
    border-left: 3px solid var(--accent-500);
    box-shadow: var(--recess);
  }

  :global([data-theme='analog']) .agent-prompt {
    border-bottom: 1.5px solid var(--accent-600);
  }

  /* Thinking section: vellum overlay with fine grain */
  :global([data-theme='analog']) .thinking-content {
    background-color: var(--surface-800);
    background-image: var(--grain-fine);
    background-blend-mode: multiply;
    border-top: 1.5px solid var(--thinking-500);
    box-shadow:
      var(--recess),
      var(--recess);
  }

  :global([data-theme='analog']) .thinking-content::after {
    display: none;
  }

  :global([data-theme='analog']) .thinking-section {
    border-color: var(--surface-border);
    background-color: var(--surface-800);
    background-image: var(--grain-coarse);
    background-blend-mode: multiply;
  }

  /* Blockquote: marginalia with ink wash */
  :global([data-theme='analog']) .cell-content.markdown :global(blockquote) {
    background-color: var(--surface-800);
    background-image: var(--grain-fine), var(--ink-wash);
    background-blend-mode: multiply, normal;
    border-left: 3px solid var(--accent-600);
  }

  /* Ink settling into paper — capillary action */
  :global([data-theme='analog']) .thinking-section.expanded .thinking-content {
    animation: ink-bleed 0.5s cubic-bezier(0.1, 0.9, 0.2, 1);
  }

  @keyframes ink-bleed {
    0% {
      opacity: 0;
      transform: translateY(-1px);
      box-shadow: var(--recess);
    }
    30% {
      opacity: 0.5;
    }
    60% {
      opacity: 0.85;
      box-shadow:
        var(--recess),
        var(--recess);
    }
    100% {
      opacity: 1;
      transform: translateY(0);
      box-shadow:
        var(--recess),
        var(--recess);
    }
  }
</style>
