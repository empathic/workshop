<script module lang="ts">
  import type { LayoutNode as LN, PaneState as PS } from '$lib/stores/layout';
  import { getPaneInstanceId } from '$lib/stores/layout';
  import { kindShortLabel } from '$lib/utils/pane-content';
  import type { Instance as Inst } from '$lib/types';

  function collectLeaves(node: LN): string[] {
    if (node.type === 'pane') return [node.id];
    return [...collectLeaves(node.children[0]), ...collectLeaves(node.children[1])];
  }

  function getMobileTabLabel(pane: PS | undefined, instanceMap: Map<string, Inst>): string {
    if (!pane) return 'Pane';
    const content = pane.content;
    const kind = content.kind;
    if ((kind === 'terminal' || kind === 'conversation') && content.instanceId) {
      const inst = instanceMap.get(content.instanceId);
      if (inst) {
        const name = inst.custom_name || inst.name;
        return name.length > 12 ? name.slice(0, 12) + '\u2026' : name;
      }
    }
    if (kind === 'file-viewer' && content.filePath) {
      const filename = content.filePath.split('/').pop() ?? 'File';
      return filename.length > 12 ? filename.slice(0, 12) + '\u2026' : filename;
    }
    return kindShortLabel(kind) ?? 'Pane';
  }

  function getMobileTabStatus(
    pane: PS | undefined,
    instanceMap: Map<string, Inst>
  ): 'thinking' | 'responding' | 'tool' | null {
    if (!pane) return null;
    const content = pane.content;
    if (content.kind !== 'terminal' && content.kind !== 'conversation') return null;
    const id = content.instanceId;
    if (!id) return null;
    const inst = instanceMap.get(id);
    if (!inst) return null;
    const cs = inst.claude_state;
    if (!cs) return null;
    if (cs.type === 'Thinking') return 'thinking';
    if (cs.type === 'Responding') return 'responding';
    if (cs.type === 'ToolExecuting') return 'tool';
    return null;
  }
</script>

<script lang="ts">
  import type { LayoutNode, PaneState } from '$lib/stores/layout';
  import { layoutState, focusPane, closePane, splitPane, paneCount, isResizing } from '$lib/stores/layout';
  import { isMobile } from '$lib/stores/ui';
  import { instances } from '$lib/stores/instances';
  import type { Instance } from '$lib/types';
  import LayoutTree from './LayoutTree.svelte';
  import PaneHost from './PaneHost.svelte';
  import SplitHandle from './SplitHandle.svelte';

  interface Props {
    node: LayoutNode;
    depth?: number;
  }

  let { node, depth = 0 }: Props = $props();

  // On mobile with multiple panes at root level, show only the focused pane
  const mobileMode = $derived(depth === 0 && $isMobile && $paneCount > 1);
</script>

{#if mobileMode}
  <!-- Mobile: render only the focused pane full-screen with a tab bar -->
  {@const leaves = collectLeaves(node)}
  <div class="mobile-layout">
    <PaneHost paneId={$layoutState.focusedPaneId} />
    {#if leaves.length > 1}
      <div class="mobile-tab-bar">
        {#each leaves as leafId}
          {@const pane = $layoutState.panes.get(leafId)}
          {@const tabStatus = getMobileTabStatus(pane, $instances)}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="mobile-tab"
            class:active={leafId === $layoutState.focusedPaneId}
            role="button"
            tabindex="0"
            onclick={() => focusPane(leafId)}
            onkeydown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                focusPane(leafId);
              }
            }}
          >
            {#if tabStatus}
              <span
                class="tab-status-dot"
                class:thinking={tabStatus === 'thinking'}
                class:responding={tabStatus === 'responding'}
                class:tool={tabStatus === 'tool'}
              ></span>
            {/if}
            <span class="tab-label">{getMobileTabLabel(pane, $instances)}</span>
            {#if leaves.length > 1}
              <button
                class="tab-close"
                onclick={(e) => {
                  e.stopPropagation();
                  closePane(leafId);
                }}
                aria-label="Close pane">&times;</button
              >
            {/if}
          </div>
        {/each}
        <button
          class="mobile-tab add-tab"
          onclick={() => splitPane($layoutState.focusedPaneId, 'vertical')}
          aria-label="Add pane">+</button
        >
      </div>
    {/if}
  </div>
{:else if node.type === 'pane'}
  <PaneHost paneId={node.id} />
{:else}
  <div
    class="split-container"
    class:vertical={node.direction === 'vertical'}
    class:horizontal={node.direction === 'horizontal'}
  >
    <div
      class="split-child"
      class:resizing={$isResizing}
      style={node.direction === 'vertical' ? `width: ${node.ratio * 100}%` : `height: ${node.ratio * 100}%`}
    >
      <LayoutTree node={node.children[0]} depth={depth + 1} />
    </div>
    <SplitHandle splitNode={node} />
    <div
      class="split-child"
      class:resizing={$isResizing}
      style={node.direction === 'vertical' ? `width: ${(1 - node.ratio) * 100}%` : `height: ${(1 - node.ratio) * 100}%`}
    >
      <LayoutTree node={node.children[1]} depth={depth + 1} />
    </div>
  </div>
{/if}

<style>
  .split-container {
    display: flex;
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
  }

  .split-container.vertical {
    flex-direction: row;
  }

  .split-container.horizontal {
    flex-direction: column;
  }

  .split-child {
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    transition:
      width 150ms ease-out,
      height 150ms ease-out;
  }

  .split-child.resizing {
    transition: none;
  }

  @media (prefers-reduced-motion: reduce) {
    .split-child {
      transition: none;
    }
  }

  /* Mobile layout */
  .mobile-layout {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
  }

  .mobile-tab-bar {
    display: flex;
    gap: 1px;
    background: var(--surface-800);
    border-top: 1px solid var(--surface-border);
    flex-shrink: 0;
    overflow-x: auto;
  }

  .mobile-tab {
    flex: 1;
    min-width: 0;
    padding: 8px 6px;
    background: var(--surface-700);
    border: none;
    color: var(--text-muted);
    font-size: 10px;
    font-weight: 600;
    font-family: inherit;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    cursor: pointer;
    transition: all 0.15s ease;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
  }

  .mobile-tab:hover {
    color: var(--text-secondary);
  }

  .mobile-tab.active {
    background: var(--surface-600);
    color: var(--chrome-accent-400);
    border-top: 2px solid var(--chrome-accent-500);
    text-shadow: var(--emphasis);
  }

  .mobile-tab.add-tab {
    flex: 0 0 32px;
    font-size: 16px;
    color: var(--text-muted);
  }

  .mobile-tab.add-tab:hover {
    color: var(--chrome-accent-400);
  }

  .tab-label {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .tab-close {
    flex-shrink: 0;
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 14px;
    line-height: 1;
    padding: 0 2px;
    cursor: pointer;
    font-family: inherit;
    opacity: 0.6;
  }

  .tab-close:hover {
    color: var(--status-red);
    opacity: 1;
  }

  .tab-status-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    flex-shrink: 0;
    animation: tab-dot-pulse 0.8s ease-in-out infinite;
  }

  .tab-status-dot.thinking {
    background: var(--thinking-500);
  }

  .tab-status-dot.responding,
  .tab-status-dot.tool {
    background: var(--chrome-accent-500);
  }

  @keyframes tab-dot-pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.5;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .tab-status-dot {
      animation: none;
    }
  }
</style>
