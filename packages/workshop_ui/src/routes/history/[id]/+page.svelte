<script lang="ts">
  import { page } from '$app/stores';
  import { base } from '$app/paths';
  import { onMount } from 'svelte';
  import {
    selectedConversation,
    isLoading,
    historyError,
    fetchConversation,
    clearSelectedConversation
  } from '$lib/stores/history';
  import { isDesktop } from '$lib/stores/ui';
  import { tasksLoaded, fetchTasks } from '$lib/stores/tasks';
  import NotebookCell from '$lib/components/NotebookCell.svelte';
  import ConversationMinimap from '$lib/components/ConversationMinimap.svelte';
  import VirtualList from '$lib/components/VirtualList.svelte';
  import type { NotebookCell as NotebookCellType, ConversationEntry, ToolCell, EntryAttribution } from '$lib/types';

  const conversationId = $derived($page.params.id);

  // VirtualList reference for scroll control
  let virtualList = $state<VirtualList<NotebookCellType> | undefined>();
  let scrollInfo = $state({ scrollTop: 0, scrollHeight: 0, clientHeight: 0, visibleStart: 0, visibleEnd: 0 });

  // Handle scroll updates from VirtualList (for minimap)
  function handleScroll(
    scrollTop: number,
    scrollHeight: number,
    clientHeight: number,
    visibleStart: number,
    visibleEnd: number
  ) {
    scrollInfo = { scrollTop, scrollHeight, clientHeight, visibleStart, visibleEnd };
  }

  // Handle minimap click to scroll to a cell
  function handleScrollToIndex(index: number) {
    virtualList?.scrollToIndex(index);
  }

  onMount(() => {
    if (conversationId) {
      fetchConversation(conversationId);
    }
    // Load tasks so the task panel can show details when a task badge is clicked
    if (!$tasksLoaded) {
      fetchTasks();
    }
    return () => clearSelectedConversation();
  });

  // Determine cell type from entry
  function getCellType(entry: ConversationEntry): NotebookCellType['type'] {
    const role = entry.role?.toLowerCase();
    const entryType = entry.entry_type?.toLowerCase();

    // Check entry_type first for progress (role is often null for these)
    if (entryType === 'progress') return 'progress';

    // Then check role
    if (role === 'user' || role === 'human') return 'user';
    if (role === 'assistant') return 'assistant';
    if (role === 'system') return 'system';
    if (role === 'progress' || role === 'agentprogress') return 'progress';
    return 'unknown';
  }

  // Extract tools from message content parts
  // Build a map of tool_use_id → { content, is_error } from all tool_result entries.
  // Tool results live in separate "phantom" user entries that follow the assistant
  // entry containing the tool_use.
  function buildToolResultMap(entries: ConversationEntry[]): Map<string, { content: string; is_error: boolean }> {
    const map = new Map<string, { content: string; is_error: boolean }>();
    for (const e of entries) {
      try {
        const raw = JSON.parse(e.raw_json) as Record<string, unknown>;
        const message = raw.message as Record<string, unknown> | undefined;
        const content = message?.content;
        if (!Array.isArray(content)) continue;
        for (const part of content) {
          if (part && typeof part === 'object' && 'type' in part) {
            const p = part as Record<string, unknown>;
            if (p.type === 'tool_result' && typeof p.tool_use_id === 'string') {
              const text = typeof p.content === 'string' ? p.content : '';
              map.set(p.tool_use_id, { content: text, is_error: !!p.is_error });
            }
          }
        }
      } catch {
        // skip unparseable entries
      }
    }
    return map;
  }

  function extractTools(
    raw: Record<string, unknown>,
    entryUuid: string,
    timestamp: string,
    toolResultMap: Map<string, { content: string; is_error: boolean }>
  ): ToolCell[] {
    const tools: ToolCell[] = [];

    // Check for tool_use in message.content array
    const message = raw.message as Record<string, unknown> | undefined;
    const content = message?.content;

    if (Array.isArray(content)) {
      content.forEach((part, index) => {
        if (part && typeof part === 'object' && 'type' in part) {
          const p = part as Record<string, unknown>;
          if (p.type === 'tool_use' && typeof p.name === 'string') {
            const result = typeof p.id === 'string' ? toolResultMap.get(p.id) : undefined;
            tools.push({
              id: `${entryUuid}-tool-${index}`,
              name: p.name,
              input: (p.input as Record<string, unknown>) ?? {},
              output: result?.content,
              is_error: result?.is_error,
              status: 'complete',
              timestamp,
              canRerun: false
            });
          }
        }
      });
    }

    return tools;
  }

  // Extract thinking from message content parts
  function extractThinking(raw: Record<string, unknown>): string | undefined {
    const message = raw.message as Record<string, unknown> | undefined;
    const content = message?.content;

    if (Array.isArray(content)) {
      const thinkingParts: string[] = [];
      for (const part of content) {
        if (part && typeof part === 'object' && 'type' in part) {
          const p = part as Record<string, unknown>;
          if (p.type === 'thinking' && typeof p.thinking === 'string') {
            thinkingParts.push(p.thinking);
          }
        }
      }
      if (thinkingParts.length > 0) {
        return thinkingParts.join('\n\n');
      }
    }

    return undefined;
  }

  // Extract progress content from raw JSON (matches live view's entry_to_turn logic)
  // Note: extra is flattened in serialization, so 'data' is at root level
  // Returns FULL content - truncation happens in display layer
  function extractProgressContent(raw: Record<string, unknown>): string {
    // 'data' is at root level due to #[serde(flatten)] on extra field
    const data = raw.data as Record<string, unknown> | undefined;

    if (data) {
      const progressType = data.type as string | undefined;

      if (progressType === 'hook_progress') {
        // Hook progress - show hook name
        const hookName = data.hookName as string | undefined;
        return hookName || 'hook';
      }

      if (progressType === 'agent_progress') {
        // Agent progress - try to extract meaningful content
        // Structure: data.message.message.content
        const messageData = data.message as Record<string, unknown> | undefined;
        if (messageData) {
          const innerMsg = messageData.message as Record<string, unknown> | undefined;
          if (innerMsg?.content) {
            const content = innerMsg.content;
            if (typeof content === 'string') {
              return content;
            }
            // Handle array of content parts
            if (Array.isArray(content)) {
              const texts: string[] = [];
              for (const part of content) {
                if (part && typeof part === 'object') {
                  const p = part as Record<string, unknown>;
                  if (p.type === 'text' && typeof p.text === 'string') {
                    texts.push(p.text as string);
                  } else if (p.type === 'tool_use' && typeof p.name === 'string') {
                    texts.push(`[${p.name}]`);
                  }
                }
              }
              if (texts.length > 0) {
                return texts.join(' ');
              }
            }
          }

          // Fallback to toolUseResult
          const toolResult = messageData.toolUseResult as string | undefined;
          if (toolResult) {
            return toolResult;
          }
        }

        // Fallback to prompt or agentId
        const prompt = data.prompt as string | undefined;
        if (prompt) {
          return prompt;
        }
        const agentId = data.agentId as string | undefined;
        return agentId ? `agent-${agentId.slice(0, 7)}` : 'agent';
      }
    }

    // Fallback: check for message content directly
    const message = raw.message as Record<string, unknown> | undefined;
    if (message?.content) {
      const content = message.content;
      if (typeof content === 'string') {
        return content;
      }
    }

    return 'progress';
  }

  // Truncate text for preview display
  function truncate(text: string, maxLen: number): string {
    return text.length > maxLen ? text.slice(0, maxLen) + '...' : text;
  }

  // Transform ConversationEntry[] → NotebookCell[] for both rendering and minimap
  // Aggregates consecutive progress entries like the live view does
  function entriesToCells(entries: ConversationEntry[], attrMap?: Map<string, EntryAttribution>): NotebookCellType[] {
    const cells: NotebookCellType[] = [];
    const toolResultMap = buildToolResultMap(entries);

    for (const e of entries) {
      const cellType = getCellType(e);

      // For progress entries, aggregate consecutive ones
      if (cellType === 'progress') {
        // Extract meaningful content from raw_json
        let progressContent = e.content || '';
        try {
          const raw = JSON.parse(e.raw_json) as Record<string, unknown>;
          progressContent = extractProgressContent(raw);
        } catch {
          // Use content as-is if raw_json not parseable
        }

        const prevCell = cells[cells.length - 1];
        if (prevCell && prevCell.type === 'progress') {
          // Aggregate: increment count and add item
          const currentCount = (prevCell.extra?.progressCount as number) ?? 1;
          const items = (prevCell.extra?.progressItems as string[]) ?? [prevCell.content];

          if (progressContent && items[items.length - 1] !== progressContent) {
            items.push(progressContent);
          }

          prevCell.extra = {
            ...prevCell.extra,
            progressCount: currentCount + 1,
            progressItems: items
          };
          prevCell.content = `${currentCount + 1} events`;
          prevCell.timestamp = e.timestamp;
          continue;
        }

        // First progress in a sequence
        cells.push({
          id: e.entry_uuid,
          type: 'progress',
          content: progressContent,
          timestamp: e.timestamp,
          collapsed: false,
          extra: {
            progressCount: 1,
            progressItems: [progressContent]
          }
        });
        continue;
      }

      // Regular cell processing
      const cell: NotebookCellType = {
        id: e.entry_uuid,
        type: cellType,
        content: e.content || '',
        timestamp: e.timestamp,
        collapsed: false
      };

      // For unknown entries, include the entry type
      if (cellType === 'unknown' || cellType === 'system') {
        cell.entryType = e.entry_type;
      }

      // Parse raw_json for tools and thinking
      try {
        const raw = JSON.parse(e.raw_json) as Record<string, unknown>;

        // Extract tools from content parts
        const tools = extractTools(raw, e.entry_uuid, e.timestamp, toolResultMap);
        if (tools.length > 0) {
          cell.toolCells = tools;
        }

        // Extract thinking from content parts
        const thinking = extractThinking(raw);
        if (thinking) {
          cell.thinking = thinking;
        }
      } catch {
        // raw_json not parseable — that's fine
      }

      // Apply attribution from the batch-fetched map (for history view)
      const attr = attrMap?.get(e.entry_uuid);
      if (attr) {
        cell.attributed_to = { user_id: attr.user_id, display_name: attr.display_name };
        if (attr.task_id != null) cell.task_id = attr.task_id;
      }

      cells.push(cell);
    }

    return cells;
  }

  const cells = $derived.by(() => {
    if (!$selectedConversation) return [];
    const attrMap = new Map(($selectedConversation.attributions ?? []).map((a) => [a.entry_uuid, a]));
    return entriesToCells($selectedConversation.entries, attrMap);
  });

  const showMinimap = $derived($isDesktop && cells.length > 0);
</script>

<div class="conversation-page">
  <header class="conversation-header">
    <a href="{base}/history" class="back-link">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M19 12H5M12 19l-7-7 7-7" />
      </svg>
      History
    </a>
    {#if $selectedConversation}
      <h1>
        {$selectedConversation.conversation.title ||
          `Conversation ${$selectedConversation.conversation.id.slice(0, 8)}...`}
      </h1>
    {:else}
      <h1>Loading...</h1>
    {/if}
  </header>

  {#if $historyError}
    <div class="error-banner">
      <span>{$historyError}</span>
      <a href="{base}/history" class="error-link">Return to history</a>
    </div>
  {/if}

  <div class="conversation-content" class:with-minimap={showMinimap}>
    {#if $isLoading}
      <div class="loading-state">
        <div class="spinner"></div>
        <span>Loading conversation...</span>
      </div>
    {:else if $selectedConversation && cells.length > 0}
      <div class="messages-wrapper">
        <VirtualList
          bind:this={virtualList}
          items={cells}
          estimatedHeight={120}
          buffer={3}
          autoScroll={false}
          onScroll={handleScroll}
        >
          {#snippet children({ item: cell, index })}
            <div class="notebook-cell-wrapper">
              <NotebookCell {cell} />
            </div>
          {/snippet}
        </VirtualList>

        {#if showMinimap}
          <ConversationMinimap
            {cells}
            scrollTop={scrollInfo.scrollTop}
            scrollHeight={scrollInfo.scrollHeight}
            clientHeight={scrollInfo.clientHeight}
            visibleStart={scrollInfo.visibleStart}
            visibleEnd={scrollInfo.visibleEnd}
            onScrollToIndex={handleScrollToIndex}
          />
        {/if}
      </div>
    {:else if !$historyError}
      <div class="empty-state">
        <p>Conversation not found</p>
      </div>
    {/if}
  </div>
</div>

<style>
  .conversation-page {
    display: flex;
    flex-direction: column;
    height: 100vh;
    height: 100dvh;
    background: var(--surface-800);
  }

  .conversation-header {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 16px 20px;
    background: linear-gradient(180deg, var(--surface-600) 0%, var(--surface-700) 100%);
    border-bottom: 1px solid var(--surface-border);
  }

  .back-link {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 12px;
    background: transparent;
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-decoration: none;
    text-transform: uppercase;
    transition: all 0.15s ease;
    flex-shrink: 0;
  }

  .back-link:hover {
    border-color: var(--chrome-accent-600);
    color: var(--chrome-accent-400);
    background: var(--tint-active);
  }

  .back-link svg {
    width: 14px;
    height: 14px;
  }

  .conversation-header h1 {
    flex: 1;
    margin: 0;
    font-size: 13px;
    font-weight: 600;
    letter-spacing: 0.05em;
    color: var(--chrome-accent-400);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .error-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 20px;
    background: var(--status-red-tint);
    border-bottom: 1px solid var(--status-red-border);
    color: var(--status-red-text);
    font-size: 12px;
    font-weight: 600;
  }

  .error-link {
    color: var(--chrome-accent-400);
    text-decoration: none;
  }

  .error-link:hover {
    text-decoration: underline;
  }

  .conversation-content {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .messages-wrapper {
    flex: 1;
    position: relative;
    overflow: hidden;
  }

  /* Add right padding when minimap is visible */
  .conversation-content.with-minimap :global(.virtual-container) {
    padding-right: 56px;
  }

  .notebook-cell-wrapper {
    padding: 0 16px;
  }

  .loading-state,
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-muted);
  }

  .spinner {
    width: 24px;
    height: 24px;
    border: 2px solid var(--surface-border);
    border-top-color: var(--chrome-accent-500);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    margin-bottom: 16px;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  /* Mobile responsive */
  @media (max-width: 639px) {
    .conversation-header {
      padding: 12px 14px;
      gap: 12px;
    }

    .conversation-header h1 {
      font-size: 12px;
    }

    .back-link {
      padding: 6px 10px;
      font-size: 11px;
    }
  }
</style>
