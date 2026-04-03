<script lang="ts">
  import { tick } from 'svelte';
  import ComposeForClaude from './ComposeForClaude.svelte';
  import ChatMessageList from './chat-panel/ChatMessageList.svelte';
  import TopicSelector from './chat-panel/TopicSelector.svelte';
  import {
    isChatOpen,
    chatScope,
    currentChatMessages,
    currentHasMore,
    currentLoadingHistory,
    totalUnread,
    unreadCounts,
    closeChat,
    switchScope,
    getOldestMessageId,
    activeTopic,
    currentTopics,
    setActiveTopic,
    selectionMode,
    selectedMessageIds,
    toggleSelectionMode,
    toggleMessageSelection,
    selectAllInTopic,
    exitSelectionMode,
    openCompose,
    composeOpen,
    groupedByTopic,
    type ChatMessageData
  } from '$lib/stores/chat';
  import { sendChatMessage, requestChatHistory, requestChatTopics, forwardChatMessage } from '$lib/stores/websocket';
  import { currentInstanceId, currentInstance, instances } from '$lib/stores/instances';
  import { isDesktop } from '$lib/stores/ui';
  import InsetButton from './InsetButton.svelte';

  interface Props {
    embedded?: boolean;
    oninset?: () => void;
  }

  let { embedded = false, oninset }: Props = $props();

  const isVisible = $derived(embedded || $isChatOpen);

  let messageListRef: ChatMessageList | undefined = $state();
  let inputValue = $state('');
  let inputEl: HTMLTextAreaElement | undefined = $state();
  let contextMenuMsg = $state<ChatMessageData | null>(null);
  let contextMenuPos = $state({ x: 0, y: 0 });
  let wasAtBottom = $state(true);
  let historyLoaded = $state<Set<string>>(new Set());
  let panelWidth = $state(360);

  // Load initial history when scope changes
  $effect(() => {
    const scope = $chatScope;
    if (isVisible && !historyLoaded.has(scope)) {
      historyLoaded.add(scope);
      requestChatHistory(scope);
      requestChatTopics(scope);
    }
  });

  // Also load when panel opens
  $effect(() => {
    if (isVisible) {
      const scope = $chatScope;
      if (!historyLoaded.has(scope)) {
        historyLoaded.add(scope);
        requestChatHistory(scope);
        requestChatTopics(scope);
      }
      tick().then(() => {
        messageListRef?.scrollToBottom();
        inputEl?.focus();
      });
    }
  });

  // Refresh topics when topic filter changes
  $effect(() => {
    const _topic = $activeTopic;
    tick().then(() => messageListRef?.scrollToBottom());
  });

  // Auto-scroll on new messages if already at bottom
  $effect(() => {
    const msgs = $currentChatMessages;
    if (msgs.length > 0 && wasAtBottom) {
      tick().then(() => messageListRef?.scrollToBottom());
    }
  });

  function handleMessageListScroll(el: HTMLDivElement) {
    const { scrollTop, scrollHeight, clientHeight } = el;
    wasAtBottom = scrollHeight - scrollTop - clientHeight < 40;

    // Infinite scroll up for history
    if (scrollTop < 60 && $currentHasMore && !$currentLoadingHistory) {
      const oldest = getOldestMessageId($chatScope);
      if (oldest) {
        const prevScrollHeight = el.scrollHeight;
        requestChatHistory($chatScope, oldest, undefined, $activeTopic);
        tick().then(() => {
          el.scrollTop = el.scrollHeight - prevScrollHeight;
        });
      }
    }
  }

  function handleSend() {
    const content = inputValue.trim();
    if (!content) return;
    sendChatMessage($chatScope, content, $activeTopic);
    inputValue = '';
    wasAtBottom = true;
    tick().then(() => messageListRef?.scrollToBottom());
  }

  function handleInputKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }

  function handleContextMenu(e: MouseEvent, msg: ChatMessageData) {
    e.preventDefault();
    contextMenuMsg = msg;
    contextMenuPos = { x: e.clientX, y: e.clientY };
  }

  function handleForward(targetScope: string) {
    if (contextMenuMsg) {
      forwardChatMessage(contextMenuMsg.id, targetScope);
    }
    contextMenuMsg = null;
  }

  function dismissContextMenu() {
    contextMenuMsg = null;
  }

  function handleTopicSelect(topic: string | null) {
    setActiveTopic(topic);
  }

  function handleComposeClick() {
    openCompose();
  }

  function formatTime(ts: number): string {
    const d = new Date(ts * 1000);
    const now = new Date();
    const diff = (now.getTime() - d.getTime()) / 1000;

    if (diff < 60) return 'now';
    if (diff < 3600) return `${Math.floor(diff / 60)}m`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h`;

    if (d.getFullYear() === now.getFullYear()) {
      return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
    }
    return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: '2-digit' });
  }

  // Instance list for scope tabs
  const instanceScopes = $derived(
    Array.from($instances.values()).map((inst) => ({
      id: inst.id,
      label: inst.custom_name ?? inst.name
    }))
  );

  const currentScopeLabel = $derived(
    $chatScope === 'global'
      ? 'Global'
      : (instanceScopes.find((s) => s.id === $chatScope)?.label ?? $chatScope.slice(0, 8))
  );

  const globalUnread = $derived($unreadCounts.get('global') ?? 0);
  const instanceUnread = $derived($currentInstanceId ? ($unreadCounts.get($currentInstanceId) ?? 0) : 0);

  const selectionCount = $derived($selectedMessageIds.size);
</script>

{#if isVisible}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  {#if !embedded && !$isDesktop}
    <div class="chat-backdrop" onclick={closeChat}></div>
  {/if}

  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <aside
    class="chat-panel"
    class:embedded
    style="width: {!embedded && $isDesktop ? panelWidth : undefined}px"
    onclick={dismissContextMenu}
  >
    <!-- Header -->
    <header class="chat-header">
      <div class="header-left">
        <svg class="header-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z" />
        </svg>
        <span class="header-title">COMMS</span>
      </div>
      <div class="header-actions">
        {#if !$selectionMode}
          <button class="header-btn" onclick={toggleSelectionMode} title="Select messages">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <rect x="3" y="3" width="18" height="18" rx="2" />
              <path d="M9 12l2 2 4-4" />
            </svg>
          </button>
        {/if}
        {#if !embedded && oninset}
          <InsetButton onclick={oninset} />
        {/if}
        {#if !embedded}
          <button class="close-btn" onclick={closeChat} aria-label="Close chat">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </button>
        {/if}
      </div>
    </header>

    <!-- Scope tabs -->
    <div class="scope-tabs">
      <button class="scope-tab" class:active={$chatScope === 'global'} onclick={() => switchScope('global')}>
        <span class="tab-label">Global</span>
        {#if globalUnread > 0}
          <span class="tab-badge">{globalUnread}</span>
        {/if}
      </button>
      {#if $currentInstanceId}
        <button
          class="scope-tab"
          class:active={$chatScope === $currentInstanceId}
          onclick={() => switchScope($currentInstanceId ?? '')}
        >
          <span class="tab-label">{$currentInstance?.custom_name ?? $currentInstance?.name ?? 'Instance'}</span>
          {#if instanceUnread > 0}
            <span class="tab-badge">{instanceUnread}</span>
          {/if}
        </button>
      {/if}
    </div>

    <!-- Topic bar -->
    <TopicSelector activeTopic={$activeTopic} currentTopics={$currentTopics} ontopicselect={handleTopicSelect} />

    <!-- Message list (extracted component) -->
    <ChatMessageList
      bind:this={messageListRef}
      messages={$currentChatMessages}
      loadingHistory={$currentLoadingHistory}
      hasMore={$currentHasMore}
      selectionMode={$selectionMode}
      selectedMessageIds={$selectedMessageIds}
      activeTopic={$activeTopic}
      currentTopics={$currentTopics}
      groupedByTopic={$groupedByTopic}
      {currentScopeLabel}
      {instanceScopes}
      onscroll={handleMessageListScroll}
      oncontextmenu={handleContextMenu}
      ontoggleselection={toggleMessageSelection}
      onselectalltopic={selectAllInTopic}
      ontopicselect={handleTopicSelect}
      {formatTime}
    />

    <!-- Selection action bar -->
    {#if $selectionMode}
      <div class="selection-bar">
        <span class="selection-count">{selectionCount} selected</span>
        <div class="selection-actions">
          {#if selectionCount > 0}
            <button class="compose-btn" onclick={handleComposeClick}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14">
                <path d="M12 19l9 2-9-18-9 18 9-2zm0 0v-8" />
              </svg>
              Compose for Claude
            </button>
          {/if}
          <button class="cancel-selection-btn" onclick={exitSelectionMode}>Cancel</button>
        </div>
      </div>
    {/if}

    <!-- Input -->
    {#if !$selectionMode}
      <div class="chat-input-area">
        {#if $activeTopic}
          <div class="input-topic-badge"># {$activeTopic}</div>
        {/if}
        <div class="input-row">
          <textarea
            bind:this={inputEl}
            bind:value={inputValue}
            onkeydown={handleInputKeydown}
            placeholder="Message {currentScopeLabel}{$activeTopic ? ` / #${$activeTopic}` : ''}..."
            rows="1"
            class="chat-input"
          ></textarea>
          <button class="send-btn" onclick={handleSend} disabled={!inputValue.trim()} aria-label="Send message">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M22 2L11 13M22 2l-7 20-4-9-9-4 20-7z" />
            </svg>
          </button>
        </div>
      </div>
    {/if}
  </aside>

  <!-- Context menu for forwarding -->
  {#if contextMenuMsg}
    <div class="context-menu" style="left: {contextMenuPos.x}px; top: {contextMenuPos.y}px">
      {#if $chatScope !== 'global'}
        <button class="context-item" onclick={() => handleForward('global')}> Forward to Global </button>
      {/if}
      {#each instanceScopes.filter((s) => s.id !== $chatScope) as scope}
        <button class="context-item" onclick={() => handleForward(scope.id)}>
          Forward to {scope.label}
        </button>
      {/each}
    </div>
  {/if}

  <!-- Compose overlay -->
  {#if $composeOpen}
    <ComposeForClaude />
  {/if}
{/if}

<style>
  /* ============================================
	   CHAT PANEL - Amber CRT comms terminal
	   ============================================ */

  .chat-backdrop {
    position: fixed;
    inset: 0;
    background: var(--backdrop);
    backdrop-filter: blur(2px);
    z-index: 55;
    animation: chat-fade-in 0.2s ease;
  }

  @keyframes chat-fade-in {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }

  .chat-panel {
    position: fixed;
    top: 0;
    right: 0;
    bottom: 0;
    width: 360px;
    z-index: 56;
    display: flex;
    flex-direction: column;
    background: linear-gradient(180deg, var(--surface-700) 0%, var(--surface-800) 100%);
    border-left: 1px solid var(--surface-border);
    box-shadow:
      var(--shadow-panel),
      -1px 0 0 var(--tint-active);
    animation: chat-slide-in 0.25s cubic-bezier(0.4, 0, 0.2, 1);
  }

  .chat-panel.embedded {
    position: relative;
    top: auto;
    right: auto;
    bottom: auto;
    width: 100% !important;
    height: 100%;
    z-index: auto;
    box-shadow: none;
    animation: none;
    border-left: none;
  }

  @keyframes chat-slide-in {
    from {
      transform: translateX(100%);
    }
    to {
      transform: translateX(0);
    }
  }

  /* Header */
  .chat-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 14px;
    background: linear-gradient(180deg, var(--surface-600) 0%, var(--surface-700) 100%);
    border-bottom: 1px solid var(--surface-border);
    flex-shrink: 0;
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .header-icon {
    width: 16px;
    height: 16px;
    color: var(--chrome-accent-400);
  }

  .header-title {
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.12em;
    color: var(--chrome-accent-400);
    text-shadow: var(--emphasis);
  }

  .header-btn {
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

  .header-btn:hover {
    border-color: var(--surface-border);
    color: var(--chrome-accent-400);
    background: var(--tint-active);
  }

  .header-btn svg {
    width: 14px;
    height: 14px;
  }

  .close-btn {
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

  .close-btn:hover {
    border-color: var(--surface-border);
    color: var(--text-primary);
    background: var(--tint-active);
  }

  .close-btn svg {
    width: 14px;
    height: 14px;
  }

  /* Scope tabs */
  .scope-tabs {
    display: flex;
    border-bottom: 1px solid var(--surface-border);
    flex-shrink: 0;
    background: var(--surface-700);
  }

  .scope-tab {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 8px 12px;
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--text-muted);
    font-size: 10px;
    font-weight: 600;
    font-family: inherit;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .scope-tab:hover {
    color: var(--text-secondary);
    background: var(--tint-hover);
  }

  .scope-tab.active {
    color: var(--chrome-accent-400);
    border-bottom-color: var(--chrome-accent-500);
    text-shadow: var(--emphasis);
  }

  .tab-label {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 120px;
  }

  .tab-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    font-size: 9px;
    font-weight: 700;
    border-radius: 8px;
    background: var(--chrome-accent-500);
    color: var(--btn-primary-text);
    line-height: 1;
  }

  /* Selection bar */
  .selection-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 14px;
    border-top: 1px solid var(--surface-border);
    background: linear-gradient(180deg, var(--surface-600) 0%, var(--surface-700) 100%);
    flex-shrink: 0;
    animation: selection-slide-up 0.2s ease;
  }

  @keyframes selection-slide-up {
    from {
      transform: translateY(100%);
      opacity: 0;
    }
    to {
      transform: translateY(0);
      opacity: 1;
    }
  }

  .selection-count {
    font-size: 10px;
    font-weight: 600;
    color: var(--chrome-accent-400);
    letter-spacing: 0.04em;
  }

  .selection-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .compose-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 5px 10px;
    background: var(--btn-primary-bg);
    border: 1px solid var(--chrome-accent-500);
    border-radius: 3px;
    color: var(--btn-primary-text);
    font-size: 10px;
    font-weight: 700;
    font-family: inherit;
    letter-spacing: 0.04em;
    cursor: pointer;
    transition: all 0.15s ease;
    text-shadow: var(--btn-primary-text-shadow);
  }

  .compose-btn:hover {
    background: linear-gradient(180deg, var(--chrome-accent-500) 0%, var(--chrome-accent-600) 100%);
  }

  .cancel-selection-btn {
    padding: 5px 10px;
    background: transparent;
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    color: var(--text-muted);
    font-size: 10px;
    font-family: inherit;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .cancel-selection-btn:hover {
    color: var(--text-secondary);
    border-color: var(--surface-border-light);
  }

  /* Input area */
  .chat-input-area {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 10px 14px;
    border-top: 1px solid var(--surface-border);
    background: var(--surface-700);
    flex-shrink: 0;
  }

  .input-topic-badge {
    font-size: 9px;
    color: var(--chrome-accent-600);
    letter-spacing: 0.04em;
    padding: 0 2px;
  }

  .input-row {
    display: flex;
    align-items: flex-end;
    gap: 8px;
  }

  .chat-input {
    flex: 1;
    padding: 8px 10px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
    line-height: 1.4;
    resize: none;
    outline: none;
    transition: border-color 0.15s ease;
    max-height: 100px;
    overflow-y: auto;
  }

  .chat-input::placeholder {
    color: var(--text-muted);
    font-size: 11px;
    letter-spacing: 0.02em;
  }

  .chat-input:focus {
    border-color: var(--chrome-accent-600);
    box-shadow: var(--elevation-low);
  }

  .send-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    background: linear-gradient(180deg, var(--surface-500) 0%, var(--surface-600) 100%);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.15s ease;
    flex-shrink: 0;
  }

  .send-btn:hover:not(:disabled) {
    background: linear-gradient(180deg, var(--tint-focus) 0%, var(--tint-active-strong) 100%);
    border-color: var(--chrome-accent-600);
    color: var(--chrome-accent-400);
  }

  .send-btn:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }

  .send-btn svg {
    width: 14px;
    height: 14px;
  }

  /* Context menu */
  .context-menu {
    position: fixed;
    z-index: 100;
    background: var(--surface-600);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    box-shadow: var(--shadow-dropdown);
    min-width: 160px;
    padding: 4px 0;
    animation: ctx-pop 0.12s ease-out;
  }

  @keyframes ctx-pop {
    from {
      opacity: 0;
      transform: scale(0.95);
    }
    to {
      opacity: 1;
      transform: scale(1);
    }
  }

  .context-item {
    display: block;
    width: 100%;
    padding: 6px 14px;
    background: transparent;
    border: none;
    color: var(--text-secondary);
    font-size: 11px;
    font-family: inherit;
    letter-spacing: 0.03em;
    text-align: left;
    cursor: pointer;
    transition: all 0.1s ease;
  }

  .context-item:hover {
    background: var(--tint-active-strong);
    color: var(--chrome-accent-400);
  }

  /* Responsive */
  @media (max-width: 1023px) {
    .chat-panel {
      width: 85vw !important;
      min-width: 280px;
      max-width: 420px;
    }
  }

  @media (max-width: 639px) {
    .chat-panel {
      width: 100% !important;
      min-width: 100%;
      max-width: 100%;
    }
  }
</style>
