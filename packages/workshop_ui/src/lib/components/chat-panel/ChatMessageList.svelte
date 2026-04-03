<script lang="ts">
  import { tick } from 'svelte';
  import TopoAvatar from '../TopoAvatar.svelte';
  import type { ChatMessageData } from '$lib/stores/chat';

  interface Props {
    messages: ChatMessageData[];
    loadingHistory: boolean;
    hasMore: boolean;
    selectionMode: boolean;
    selectedMessageIds: Set<string>;
    activeTopic: string | null;
    currentTopics: Array<{ topic: string; message_count: number }>;
    groupedByTopic: Map<string, ChatMessageData[]>;
    currentScopeLabel: string;
    instanceScopes: Array<{ id: string; label: string }>;
    onscroll: (el: HTMLDivElement) => void;
    oncontextmenu: (e: MouseEvent, msg: ChatMessageData) => void;
    ontoggleselection: (uuid: string) => void;
    onselectalltopic: (topic: string) => void;
    ontopicselect: (topic: string | null) => void;
    formatTime: (ts: number) => string;
  }

  let {
    messages,
    loadingHistory,
    hasMore,
    selectionMode,
    selectedMessageIds,
    activeTopic,
    currentTopics,
    groupedByTopic,
    currentScopeLabel,
    instanceScopes,
    onscroll,
    oncontextmenu,
    ontoggleselection,
    onselectalltopic,
    ontopicselect,
    formatTime
  }: Props = $props();

  let messageListEl: HTMLDivElement | undefined = $state();

  function handleScroll() {
    if (messageListEl) {
      onscroll(messageListEl);
    }
  }

  export function scrollToBottom() {
    if (messageListEl) {
      messageListEl.scrollTop = messageListEl.scrollHeight;
    }
  }

  export function getElement(): HTMLDivElement | undefined {
    return messageListEl;
  }
</script>

<div class="message-list" bind:this={messageListEl} onscroll={handleScroll}>
  {#if loadingHistory}
    <div class="loading-indicator">
      <div class="loading-dot"></div>
      <div class="loading-dot"></div>
      <div class="loading-dot"></div>
    </div>
  {/if}

  {#if messages.length === 0 && !loadingHistory}
    <div class="empty-chat">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <path
          d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z"
        />
      </svg>
      <span>No messages in {currentScopeLabel}{activeTopic ? ` / #${activeTopic}` : ''}</span>
    </div>
  {/if}

  {#if activeTopic === null && currentTopics.length > 0}
    <!-- Grouped by topic view -->
    {#each [...groupedByTopic.entries()] as [topic, msgs]}
      <div class="topic-group">
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="topic-group-header" onclick={() => ontopicselect(topic === '(General)' ? null : topic)}>
          <span class="topic-group-name">{topic === '(General)' ? topic : `# ${topic}`}</span>
          <span class="topic-group-count">{msgs.length} msg</span>
          {#if selectionMode && topic !== '(General)'}
            <button
              class="select-all-topic"
              onclick={(e) => {
                e.stopPropagation();
                onselectalltopic(topic);
              }}
            >
              Select all
            </button>
          {/if}
        </div>
        {#each msgs.slice(-3) as msg (msg.uuid)}
          {@const isSelected = selectedMessageIds.has(msg.uuid)}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="message grouped-msg" class:selected={isSelected} oncontextmenu={(e) => oncontextmenu(e, msg)}>
            {#if selectionMode}
              <label class="msg-checkbox">
                <input type="checkbox" checked={isSelected} onchange={() => ontoggleselection(msg.uuid)} />
                <span class="checkmark"></span>
              </label>
            {/if}
            <div class="message-inline">
              <TopoAvatar identity={msg.user_id} type="human" variant="user" size={16} />
              <span class="message-author-inline">{msg.display_name}</span>
              <span class="message-content-inline"
                >{msg.content.length > 60 ? msg.content.slice(0, 60) + '...' : msg.content}</span
              >
              <span class="message-time">{formatTime(msg.created_at)}</span>
            </div>
          </div>
        {/each}
        {#if msgs.length > 3}
          <button class="topic-view-all" onclick={() => ontopicselect(topic === '(General)' ? null : topic)}>
            View all {msgs.length} messages
          </button>
        {/if}
      </div>
    {/each}
  {:else}
    <!-- Flat message view (filtered by topic or no topics exist) -->
    {#each messages as msg, i (msg.uuid)}
      {@const prevMsg = i > 0 ? messages[i - 1] : null}
      {@const showAuthor = !prevMsg || prevMsg.user_id !== msg.user_id || msg.created_at - prevMsg.created_at > 120}
      {@const isSelected = selectedMessageIds.has(msg.uuid)}

      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="message"
        class:continued={!showAuthor}
        class:forwarded={!!msg.forwarded_from}
        class:selected={isSelected}
        oncontextmenu={(e) => oncontextmenu(e, msg)}
      >
        {#if selectionMode}
          <label class="msg-checkbox">
            <input type="checkbox" checked={isSelected} onchange={() => ontoggleselection(msg.uuid)} />
            <span class="checkmark"></span>
          </label>
        {/if}
        <div class="message-body">
          {#if showAuthor}
            <div class="message-header">
              <TopoAvatar identity={msg.user_id} type="human" variant="user" size={20} />
              <span class="message-author">{msg.display_name}</span>
              {#if msg.topic && activeTopic === null}
                <span class="message-topic-tag"># {msg.topic}</span>
              {/if}
              <span class="message-time">{formatTime(msg.created_at)}</span>
            </div>
          {/if}
          {#if msg.forwarded_from}
            <div class="forwarded-badge">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="10" height="10">
                <path d="M13 17l5-5-5-5M6 17l5-5-5-5" />
              </svg>
              fwd from {msg.forwarded_from === 'global'
                ? 'Global'
                : (instanceScopes.find((s) => s.id === msg.forwarded_from)?.label ?? msg.forwarded_from?.slice(0, 8))}
            </div>
          {/if}
          <div class="message-content">{msg.content}</div>
        </div>
      </div>
    {/each}
  {/if}
</div>

<style>
  /* Message list */
  .message-list {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 12px 14px;
    scroll-behavior: smooth;
  }

  .message-list::-webkit-scrollbar {
    width: 5px;
  }

  .message-list::-webkit-scrollbar-track {
    background: transparent;
  }

  .message-list::-webkit-scrollbar-thumb {
    background: var(--surface-border);
    border-radius: 3px;
  }

  .message-list::-webkit-scrollbar-thumb:hover {
    background: var(--surface-border-light);
  }

  /* Loading indicator */
  .loading-indicator {
    display: flex;
    justify-content: center;
    gap: 4px;
    padding: 12px;
  }

  .loading-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--chrome-accent-500);
    opacity: 0.4;
    animation: dot-pulse 1.2s ease-in-out infinite;
  }

  .loading-dot:nth-child(2) {
    animation-delay: 0.15s;
  }
  .loading-dot:nth-child(3) {
    animation-delay: 0.3s;
  }

  @keyframes dot-pulse {
    0%,
    80%,
    100% {
      opacity: 0.2;
      transform: scale(0.8);
    }
    40% {
      opacity: 1;
      transform: scale(1.1);
    }
  }

  /* Empty state */
  .empty-chat {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 48px 24px;
    color: var(--text-muted);
  }

  .empty-chat svg {
    width: 36px;
    height: 36px;
    opacity: 0.3;
  }

  .empty-chat span {
    font-size: 11px;
    letter-spacing: 0.04em;
  }

  /* Topic groups */
  .topic-group {
    margin-bottom: 16px;
  }

  .topic-group-header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    margin-bottom: 6px;
    background: var(--tint-hover);
    border: 1px solid var(--tint-active);
    border-radius: 3px;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .topic-group-header:hover {
    background: var(--tint-active);
    border-color: var(--tint-focus);
  }

  .topic-group-name {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.04em;
    color: var(--chrome-accent-400);
    text-transform: uppercase;
  }

  .topic-group-count {
    font-size: 9px;
    color: var(--text-muted);
    margin-left: auto;
  }

  .select-all-topic {
    font-size: 9px;
    color: var(--text-muted);
    background: transparent;
    border: 1px solid var(--surface-border);
    border-radius: 2px;
    padding: 1px 5px;
    cursor: pointer;
    font-family: inherit;
    transition: all 0.15s ease;
  }

  .select-all-topic:hover {
    color: var(--chrome-accent-400);
    border-color: var(--chrome-accent-600);
  }

  .topic-view-all {
    display: block;
    width: 100%;
    padding: 4px 8px;
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 9px;
    font-family: inherit;
    letter-spacing: 0.04em;
    cursor: pointer;
    text-align: left;
    transition: color 0.15s ease;
  }

  .topic-view-all:hover {
    color: var(--chrome-accent-400);
  }

  /* Message inline (for grouped view) */
  .message-inline {
    display: flex;
    align-items: center;
    gap: 6px;
    padding-left: 0;
  }

  .message-author-inline {
    font-size: 10px;
    font-weight: 600;
    color: var(--text-primary);
    flex-shrink: 0;
  }

  .message-content-inline {
    font-size: 11px;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 1;
  }

  /* Messages */
  .message {
    margin-bottom: 12px;
    display: flex;
    gap: 6px;
    align-items: flex-start;
    transition: background 0.15s ease;
    padding: 2px 4px;
    border-radius: 3px;
  }

  .message.selected {
    background: var(--tint-active);
  }

  .message.continued {
    margin-bottom: 4px;
    margin-top: -4px;
  }

  .message-body {
    flex: 1;
    min-width: 0;
  }

  .msg-checkbox {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 18px;
    height: 18px;
    margin-top: 2px;
    cursor: pointer;
    position: relative;
  }

  .msg-checkbox input {
    position: absolute;
    opacity: 0;
    width: 0;
    height: 0;
  }

  .msg-checkbox .checkmark {
    width: 14px;
    height: 14px;
    border: 1px solid var(--surface-border-light);
    border-radius: 2px;
    background: var(--surface-800);
    transition: all 0.15s ease;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .msg-checkbox input:checked + .checkmark {
    background: var(--chrome-accent-600);
    border-color: var(--chrome-accent-500);
  }

  .msg-checkbox input:checked + .checkmark::after {
    content: '';
    width: 6px;
    height: 3px;
    border: solid var(--btn-primary-text);
    border-width: 0 0 1.5px 1.5px;
    transform: rotate(-45deg);
    margin-top: -1px;
  }

  .msg-checkbox:hover .checkmark {
    border-color: var(--chrome-accent-600);
  }

  .message-header {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 4px;
  }

  .message-author {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-primary);
    letter-spacing: 0.03em;
  }

  .message-topic-tag {
    font-size: 9px;
    color: var(--accent-600);
    background: var(--tint-active);
    border: 1px solid var(--tint-active-strong);
    padding: 0 4px;
    border-radius: 2px;
    letter-spacing: 0.02em;
  }

  .message-time {
    font-size: 9px;
    color: var(--text-muted);
    letter-spacing: 0.04em;
    margin-left: auto;
    flex-shrink: 0;
  }

  .forwarded-badge {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 9px;
    color: var(--text-muted);
    letter-spacing: 0.03em;
    padding: 1px 6px;
    background: var(--tint-active);
    border: 1px solid var(--tint-active-strong);
    border-radius: 3px;
    margin-bottom: 3px;
  }

  .message-content {
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-secondary);
    padding-left: 26px;
    word-break: break-word;
    white-space: pre-wrap;
  }

  .message.continued .message-content {
    padding-left: 26px;
  }
</style>
