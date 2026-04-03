<script lang="ts">
  import TopoAvatar from './TopoAvatar.svelte';
  import {
    selectedMessages,
    selectedMessageIds,
    composeContent,
    composeTargetInstance,
    closeCompose,
    exitSelectionMode,
    toggleMessageSelection,
    type ChatMessageData
  } from '$lib/stores/chat';
  import { sendToInstance } from '$lib/stores/websocket';
  import { instances, currentInstanceId } from '$lib/stores/instances';
  import { tick } from 'svelte';

  let textareaEl: HTMLTextAreaElement | undefined = $state();
  let sent = $state(false);
  let sentInstanceName = $state('');
  let instancePickerOpen = $state(false);

  // Focus textarea on mount
  $effect(() => {
    tick().then(() => textareaEl?.focus());
  });

  // Initialize target instance to current instance
  $effect(() => {
    if ($composeTargetInstance === null && $currentInstanceId) {
      composeTargetInstance.set($currentInstanceId);
    }
  });

  // Derive instance list with display names
  const instanceOptions = $derived(
    Array.from($instances.values())
      .filter((inst) => inst.running)
      .map((inst) => ({
        id: inst.id,
        label: inst.custom_name ?? inst.name,
        isClaude: inst.kind.type === 'Structured'
      }))
  );

  const targetInstance = $derived(instanceOptions.find((inst) => inst.id === $composeTargetInstance));

  const targetLabel = $derived(targetInstance?.label ?? 'Select instance');

  // Preview: first 3 lines of compose content
  const previewLines = $derived(() => {
    const text = $composeContent;
    if (!text) return [];
    const lines = text.split('\n').filter((l) => l.trim());
    return lines.slice(0, 3);
  });

  const charCount = $derived($composeContent.length);
  const canSend = $derived($composeContent.trim().length > 0 && $composeTargetInstance !== null);

  function handleSend() {
    if (!canSend || !$composeTargetInstance) return;

    sentInstanceName = targetLabel;
    sendToInstance($composeTargetInstance, $composeContent);

    sent = true;
    setTimeout(() => {
      closeCompose();
      exitSelectionMode();
    }, 1200);
  }

  function handleCancel() {
    closeCompose();
  }

  function handleDeselectMsg(uuid: string) {
    toggleMessageSelection(uuid);
    // If no messages left, close compose
    tick().then(() => {
      if ($selectedMessages.length === 0) {
        closeCompose();
      }
    });
  }

  function selectAll() {
    // Already selected — this is a no-op display
  }

  function handleInstanceSelect(id: string) {
    composeTargetInstance.set(id);
    instancePickerOpen = false;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.stopPropagation();
      handleCancel();
    }
    // Cmd/Ctrl+Enter to send
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter' && canSend) {
      e.preventDefault();
      handleSend();
    }
  }

  function formatTime(ts: number): string {
    const d = new Date(ts * 1000);
    const now = new Date();
    const diff = (now.getTime() - d.getTime()) / 1000;
    if (diff < 60) return 'now';
    if (diff < 3600) return `${Math.floor(diff / 60)}m`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
    return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="compose-overlay" onkeydown={handleKeydown}>
  {#if sent}
    <!-- Sent confirmation -->
    <div class="sent-flash">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M22 2L11 13M22 2l-7 20-4-9-9-4 20-7z" />
      </svg>
      <span>Sent to {sentInstanceName}</span>
    </div>
  {:else}
    <!-- Header -->
    <header class="compose-header">
      <div class="compose-header-left">
        <svg class="compose-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M12 19l9 2-9-18-9 18 9-2zm0 0v-8" />
        </svg>
        <span class="compose-title">COMPOSE FOR CLAUDE</span>
      </div>
      <button class="compose-close" onclick={handleCancel} aria-label="Close compose">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M18 6L6 18M6 6l12 12" />
        </svg>
      </button>
    </header>

    <!-- Section 1: Selected messages -->
    <section class="compose-section messages-section">
      <div class="section-label">
        <span class="section-label-text">SELECTED MESSAGES</span>
        <span class="section-label-count">{$selectedMessages.length}</span>
      </div>
      <div class="messages-scroll">
        {#each $selectedMessages as msg (msg.uuid)}
          <div class="compose-msg">
            <button class="compose-msg-deselect" onclick={() => handleDeselectMsg(msg.uuid)} title="Remove">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="10" height="10">
                <path d="M18 6L6 18M6 6l12 12" />
              </svg>
            </button>
            <TopoAvatar identity={msg.user_id} type="human" variant="user" size={16} />
            <span class="compose-msg-author">{msg.display_name}</span>
            <span class="compose-msg-time">{formatTime(msg.created_at)}</span>
            <div class="compose-msg-content">{msg.content}</div>
          </div>
        {/each}
      </div>
    </section>

    <!-- Section 2: Compose area -->
    <section class="compose-section editor-section">
      <div class="section-label">
        <span class="section-label-text">PROMPT</span>
        <span class="char-count">{charCount} chars</span>
      </div>
      <textarea
        bind:this={textareaEl}
        bind:value={$composeContent}
        class="compose-textarea"
        placeholder="Compose your prompt for Claude..."
        spellcheck="false"
      ></textarea>
    </section>

    <!-- Section 3: Send controls -->
    <section class="compose-section send-section">
      <!-- Instance picker -->
      <div class="instance-picker-area">
        <span class="picker-label">TARGET</span>
        <div class="instance-picker-wrap">
          <button
            class="instance-picker-btn"
            onclick={() => {
              instancePickerOpen = !instancePickerOpen;
            }}
          >
            {#if targetInstance?.isClaude}
              <span class="claude-dot"></span>
            {/if}
            <span>{targetLabel}</span>
            <svg
              class="picker-chevron"
              class:open={instancePickerOpen}
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              width="10"
              height="10"
            >
              <path d="M6 9l6 6 6-6" />
            </svg>
          </button>

          {#if instancePickerOpen}
            <div class="instance-dropdown">
              {#each instanceOptions as inst}
                <button
                  class="instance-option"
                  class:active={inst.id === $composeTargetInstance}
                  onclick={() => handleInstanceSelect(inst.id)}
                >
                  {#if inst.isClaude}
                    <span class="claude-dot"></span>
                  {/if}
                  <span class="inst-label">{inst.label}</span>
                  {#if inst.id === $composeTargetInstance}
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12">
                      <path d="M20 6L9 17l-5-5" />
                    </svg>
                  {/if}
                </button>
              {/each}
              {#if instanceOptions.length === 0}
                <div class="instance-empty">No running instances</div>
              {/if}
            </div>
          {/if}
        </div>
      </div>

      <!-- Preview -->
      {#if previewLines().length > 0}
        <div class="send-preview">
          <span class="preview-label">PREVIEW</span>
          <div class="preview-text">
            {#each previewLines() as line}
              <div class="preview-line">{line.length > 80 ? line.slice(0, 80) + '...' : line}</div>
            {/each}
            {#if $composeContent.split('\n').filter((l) => l.trim()).length > 3}
              <div class="preview-more">
                ... +{$composeContent.split('\n').filter((l) => l.trim()).length - 3} more lines
              </div>
            {/if}
          </div>
        </div>
      {/if}

      <!-- Action buttons -->
      <div class="send-actions">
        <button class="cancel-btn" onclick={handleCancel}> Cancel </button>
        <button
          class="send-btn"
          disabled={!canSend}
          onclick={handleSend}
          title={canSend ? `Send to ${targetLabel} (Cmd+Enter)` : 'Select an instance and compose text'}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14">
            <path d="M22 2L11 13M22 2l-7 20-4-9-9-4 20-7z" />
          </svg>
          Send to {targetLabel}
        </button>
      </div>
    </section>
  {/if}
</div>

<style>
  /* ============================================
	   COMPOSE FOR CLAUDE — full-panel overlay
	   matches chat panel CRT amber aesthetic
	   ============================================ */

  .compose-overlay {
    position: fixed;
    top: 0;
    right: 0;
    bottom: 0;
    width: 420px;
    z-index: 60;
    display: flex;
    flex-direction: column;
    background: linear-gradient(180deg, var(--surface-700) 0%, var(--surface-900) 100%);
    border-left: 1px solid var(--surface-border);
    box-shadow:
      var(--shadow-panel),
      -1px 0 0 var(--tint-active-strong);
    animation: compose-slide-in 0.25s cubic-bezier(0.4, 0, 0.2, 1);
  }

  @keyframes compose-slide-in {
    from {
      transform: translateX(100%);
      opacity: 0.8;
    }
    to {
      transform: translateX(0);
      opacity: 1;
    }
  }

  /* Sent confirmation flash */
  .sent-flash {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 16px;
    flex: 1;
    animation: flash-in 0.3s ease;
  }

  .sent-flash svg {
    width: 36px;
    height: 36px;
    color: var(--chrome-accent-400);
  }

  .sent-flash span {
    font-size: 13px;
    font-weight: 600;
    color: var(--chrome-accent-400);
    letter-spacing: 0.06em;
    text-shadow: var(--emphasis);
  }

  @keyframes flash-in {
    from {
      opacity: 0;
      transform: scale(0.9);
    }
    to {
      opacity: 1;
      transform: scale(1);
    }
  }

  /* Header */
  .compose-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    background: linear-gradient(180deg, var(--surface-600) 0%, var(--surface-700) 100%);
    border-bottom: 1px solid var(--surface-border);
    flex-shrink: 0;
  }

  .compose-header-left {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .compose-icon {
    width: 16px;
    height: 16px;
    color: var(--chrome-accent-400);
  }

  .compose-title {
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.12em;
    color: var(--chrome-accent-400);
    text-shadow: var(--emphasis);
  }

  .compose-close {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 3px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .compose-close:hover {
    border-color: var(--surface-border);
    color: var(--text-primary);
    background: var(--tint-active);
  }

  .compose-close svg {
    width: 14px;
    height: 14px;
  }

  /* Sections */
  .compose-section {
    display: flex;
    flex-direction: column;
    border-bottom: 1px solid var(--surface-border);
  }

  .section-label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 16px 6px;
    flex-shrink: 0;
  }

  .section-label-text {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.1em;
    color: var(--text-muted);
  }

  .section-label-count {
    font-size: 9px;
    font-weight: 600;
    color: var(--chrome-accent-600);
    background: var(--tint-active);
    padding: 1px 6px;
    border-radius: 8px;
  }

  .char-count {
    font-size: 9px;
    color: var(--text-muted);
    letter-spacing: 0.04em;
  }

  /* Messages section */
  .messages-section {
    max-height: 30%;
    min-height: 80px;
  }

  .messages-scroll {
    overflow-y: auto;
    padding: 0 16px 10px;
    flex: 1;
  }

  .messages-scroll::-webkit-scrollbar {
    width: 4px;
  }

  .messages-scroll::-webkit-scrollbar-track {
    background: transparent;
  }

  .messages-scroll::-webkit-scrollbar-thumb {
    background: var(--surface-border);
    border-radius: 2px;
  }

  .compose-msg {
    display: grid;
    grid-template-columns: 18px 16px auto 1fr;
    grid-template-rows: auto auto;
    gap: 0 6px;
    align-items: center;
    padding: 6px 0;
    border-bottom: 1px solid var(--tint-hover);
  }

  .compose-msg:last-child {
    border-bottom: none;
  }

  .compose-msg-deselect {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    background: transparent;
    border: 1px solid var(--surface-border);
    border-radius: 2px;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 0.15s ease;
    grid-row: 1;
    grid-column: 1;
  }

  .compose-msg-deselect:hover {
    color: var(--status-red);
    border-color: var(--status-red);
    background: var(--status-red-tint);
  }

  .compose-msg-author {
    font-size: 10px;
    font-weight: 600;
    color: var(--text-primary);
    grid-row: 1;
    grid-column: 3;
  }

  .compose-msg-time {
    font-size: 9px;
    color: var(--text-muted);
    text-align: right;
    grid-row: 1;
    grid-column: 4;
  }

  .compose-msg-content {
    font-size: 11px;
    color: var(--text-secondary);
    line-height: 1.45;
    grid-row: 2;
    grid-column: 2 / -1;
    padding-top: 2px;
    word-break: break-word;
    white-space: pre-wrap;
  }

  /* Editor section */
  .editor-section {
    flex: 1;
    min-height: 120px;
  }

  .compose-textarea {
    flex: 1;
    margin: 0 16px 10px;
    padding: 10px 12px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 12px;
    font-family: 'Berkeley Mono', 'JetBrains Mono', 'Fira Code', 'SF Mono', monospace;
    line-height: 1.55;
    resize: none;
    outline: none;
    transition: border-color 0.15s ease;
  }

  .compose-textarea::placeholder {
    color: var(--text-muted);
    font-size: 11px;
  }

  .compose-textarea:focus {
    border-color: var(--chrome-accent-600);
    box-shadow: var(--elevation-low);
  }

  .compose-textarea::-webkit-scrollbar {
    width: 4px;
  }

  .compose-textarea::-webkit-scrollbar-track {
    background: transparent;
  }

  .compose-textarea::-webkit-scrollbar-thumb {
    background: var(--surface-border);
    border-radius: 2px;
  }

  /* Send section */
  .send-section {
    flex-shrink: 0;
    border-bottom: none;
    padding-bottom: 16px;
  }

  /* Instance picker */
  .instance-picker-area {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 16px;
  }

  .picker-label {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.1em;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .instance-picker-wrap {
    position: relative;
    flex: 1;
  }

  .instance-picker-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 6px 10px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    color: var(--text-primary);
    font-size: 11px;
    font-family: inherit;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .instance-picker-btn:hover {
    border-color: var(--chrome-accent-600);
    background: var(--tint-hover);
  }

  .picker-chevron {
    margin-left: auto;
    transition: transform 0.15s ease;
    color: var(--text-muted);
  }

  .picker-chevron.open {
    transform: rotate(180deg);
  }

  .claude-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--chrome-accent-500);
    flex-shrink: 0;
  }

  .instance-dropdown {
    position: absolute;
    bottom: 100%;
    left: 0;
    right: 0;
    z-index: 10;
    background: var(--surface-600);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    box-shadow: var(--shadow-dropdown);
    padding: 4px 0;
    margin-bottom: 4px;
    max-height: 180px;
    overflow-y: auto;
    animation: dropdown-pop 0.12s ease-out;
  }

  @keyframes dropdown-pop {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .instance-option {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 6px 12px;
    background: transparent;
    border: none;
    color: var(--text-secondary);
    font-size: 11px;
    font-family: inherit;
    text-align: left;
    cursor: pointer;
    transition: all 0.1s ease;
  }

  .instance-option:hover {
    background: var(--tint-active-strong);
    color: var(--chrome-accent-400);
  }

  .instance-option.active {
    color: var(--chrome-accent-400);
  }

  .inst-label {
    flex: 1;
  }

  .instance-empty {
    padding: 10px 12px;
    font-size: 10px;
    color: var(--text-muted);
    letter-spacing: 0.04em;
  }

  /* Preview */
  .send-preview {
    padding: 0 16px 8px;
  }

  .preview-label {
    display: block;
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.1em;
    color: var(--text-muted);
    margin-bottom: 4px;
  }

  .preview-text {
    padding: 6px 8px;
    background: var(--tint-subtle);
    border: 1px solid var(--tint-active);
    border-radius: 3px;
  }

  .preview-line {
    font-size: 10px;
    font-family: 'Berkeley Mono', 'JetBrains Mono', 'Fira Code', 'SF Mono', monospace;
    color: var(--text-secondary);
    line-height: 1.5;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .preview-more {
    font-size: 9px;
    color: var(--text-muted);
    padding-top: 2px;
    letter-spacing: 0.03em;
  }

  /* Action buttons */
  .send-actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    padding: 0 16px;
  }

  .cancel-btn {
    padding: 7px 14px;
    background: transparent;
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    color: var(--text-muted);
    font-size: 11px;
    font-family: inherit;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .cancel-btn:hover {
    color: var(--text-secondary);
    border-color: var(--surface-border-light);
  }

  .send-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 7px 16px;
    background: var(--btn-primary-bg);
    border: 1px solid var(--chrome-accent-500);
    border-radius: 3px;
    color: var(--btn-primary-text);
    font-size: 11px;
    font-weight: 700;
    font-family: inherit;
    letter-spacing: 0.04em;
    cursor: pointer;
    transition: all 0.15s ease;
    text-shadow: var(--btn-primary-text-shadow);
  }

  .send-btn:hover:not(:disabled) {
    background: linear-gradient(180deg, var(--chrome-accent-500) 0%, var(--chrome-accent-600) 100%);
  }

  .send-btn:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }

  /* Responsive */
  @media (max-width: 1023px) {
    .compose-overlay {
      width: 90vw;
      max-width: 440px;
    }
  }

  @media (max-width: 639px) {
    .compose-overlay {
      width: 100%;
      max-width: 100%;
    }
  }
</style>
