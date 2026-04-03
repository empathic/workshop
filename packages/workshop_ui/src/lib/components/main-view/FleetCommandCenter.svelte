<script lang="ts">
  import type { Instance } from '$lib/types';
  import { getStateInfo } from '$lib/utils/instance-state';
  import { inboxItems, inboxSorted, dismissInboxItem, formatDuration, type InboxItem } from '$lib/stores/inbox';
  import ActionChip from './fleet/ActionChip.svelte';
  import ActionCard from './fleet/ActionCard.svelte';
  import InstanceKindIcon from './fleet/InstanceKindIcon.svelte';

  interface Props {
    instances: Instance[];
    currentInstanceId: string | null;
    paneInstanceIds: Set<string>;
    expanded: boolean;
    onselect: (instanceId: string) => void;
    onexpand: () => void;
    onclose: () => void;
    oncontextmenu: (instance: Instance, anchorRect: DOMRect) => void;
    oncreate: () => void;
  }

  let {
    instances,
    currentInstanceId,
    paneInstanceIds,
    expanded,
    onselect,
    onexpand,
    onclose,
    oncontextmenu,
    oncreate
  }: Props = $props();

  // =========================================================================
  // Live timer — tick every 1s to force duration re-eval
  // =========================================================================

  let tick = $state(0);
  $effect(() => {
    const id = setInterval(() => {
      tick++;
    }, 1000);
    return () => clearInterval(id);
  });

  // =========================================================================
  // Instance lookup
  // =========================================================================

  const instanceMap = $derived(new Map(instances.map((i) => [i.id, i])));

  // =========================================================================
  // Action items (inbox-driven, uses store ordering)
  // =========================================================================

  const actionItems = $derived.by(() => {
    return $inboxSorted
      .map((item) => {
        const instance = instanceMap.get(item.instance_id);
        if (!instance) return null;
        return { item, instance };
      })
      .filter((x): x is { item: InboxItem; instance: Instance } => x !== null);
  });

  // =========================================================================
  // Single-pass fleet partition — one loop, three buckets, O(n)
  // =========================================================================

  const fleet = $derived.by(() => {
    const inboxIds = $inboxItems;
    const active: Instance[] = [];
    const idle: Instance[] = [];

    for (const inst of instances) {
      if (inboxIds.has(inst.id)) continue;
      const s = inst.claude_state;
      if (
        s &&
        (s.type === 'Thinking' || s.type === 'Responding' || s.type === 'ToolExecuting' || s.type === 'Starting')
      ) {
        active.push(inst);
      } else {
        idle.push(inst);
      }
    }

    return { active, idle };
  });

  // =========================================================================
  // Continuous heat encoding — 0s→muted, 120s+→amber via color-mix()
  // =========================================================================

  function heatColor(secs: number): string {
    const t = Math.min(1, secs / 120);
    const pct = Math.round(t * 100);
    return `color-mix(in srgb, var(--chrome-accent-400) ${pct}%, var(--text-muted))`;
  }

  function heatStyle(secs: number): string {
    const weight = secs > 30 ? 700 : 600;
    return `color: ${heatColor(secs)}; font-weight: ${weight}`;
  }

  /** Activity bar width in px — 0s→0px, 120s+→24px */
  function barPx(secs: number): number {
    return Math.round(Math.min(1, secs / 120) * 24);
  }

  /** Whether the heat level warrants a pulse animation */
  function isHot(secs: number): boolean {
    return secs > 60;
  }

  // =========================================================================
  // Structured summary segments — data-level state machine, not template soup
  // =========================================================================

  interface ActivityItem {
    id: string;
    duration: string;
    secs: number;
  }

  type SummarySegment =
    | { kind: 'activity'; label: string; items: ActivityItem[] }
    | { kind: 'count'; label: string; count: number };

  const summarySegments = $derived.by((): SummarySegment[] => {
    void tick;
    const segments: SummarySegment[] = [];

    const thinking: ActivityItem[] = [];
    const executing: ActivityItem[] = [];
    let bootingCount = 0;

    for (const inst of fleet.active) {
      const secs = inst.state_entered_at ? Math.floor(Date.now() / 1000) - inst.state_entered_at : 0;
      const dur = inst.state_entered_at ? formatDuration(inst.state_entered_at) : '';
      const s = inst.claude_state;
      if (!s) continue;

      switch (s.type) {
        case 'Thinking':
        case 'Responding':
          thinking.push({ id: inst.id, duration: dur, secs });
          break;
        case 'ToolExecuting':
          executing.push({ id: inst.id, duration: dur, secs });
          break;
        case 'Starting':
        case 'Initializing':
          bootingCount++;
          break;
      }
    }

    if (thinking.length > 0) segments.push({ kind: 'activity', label: 'thinking', items: thinking });
    if (executing.length > 0) segments.push({ kind: 'activity', label: 'exec', items: executing });
    if (bootingCount > 0) segments.push({ kind: 'count', label: 'starting', count: bootingCount });
    // No idle in strip — noise. Panel only.

    return segments;
  });

  const hasActivity = $derived(summarySegments.length > 0);

  // =========================================================================
  // Fleet composition bar — proportional segments for spatial overview
  // =========================================================================

  const fleetBarSegments = $derived.by(() => {
    const total = instances.length;
    if (total === 0) return [];

    const inboxCount = $inboxItems.size;
    const activeCount = fleet.active.length;
    const idleCount = fleet.idle.length;

    // Only show if there's something interesting (not all idle)
    if (inboxCount === 0 && activeCount === 0) return [];

    const segs: Array<{ type: string; flex: number }> = [];
    if (inboxCount > 0) segs.push({ type: 'attention', flex: inboxCount });
    if (activeCount > 0) segs.push({ type: 'active', flex: activeCount });
    if (idleCount > 0) segs.push({ type: 'idle', flex: idleCount });
    return segs;
  });

  // =========================================================================
  // Enriched active entries for panel — pre-computed view data
  // =========================================================================

  const activeEntries = $derived.by(() => {
    void tick;
    return fleet.active.map((inst) => {
      const secs = inst.state_entered_at ? Math.floor(Date.now() / 1000) - inst.state_entered_at : 0;
      return {
        instance: inst,
        stateInfo: getStateInfo(inst.id, inst.claude_state, inst.claude_state_stale),
        duration: inst.state_entered_at ? formatDuration(inst.state_entered_at) : '',
        secs,
        barPct: Math.min(100, Math.round(Math.min(1, secs / 180) * 100))
      };
    });
  });

  // =========================================================================
  // ResizeObserver for action chip overflow
  // =========================================================================

  let queueEl: HTMLElement | undefined = $state(undefined);
  let queueWidth = $state(0);

  $effect(() => {
    if (!queueEl) return;
    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        queueWidth = entry.contentRect.width;
      }
    });
    ro.observe(queueEl);
    return () => ro.disconnect();
  });

  const CHIP_EST_WIDTH = 180;
  const visibleChipCount = $derived(Math.max(1, Math.floor(queueWidth / CHIP_EST_WIDTH)));
  const overflowCount = $derived(Math.max(0, actionItems.length - visibleChipCount));
  const visibleChips = $derived(actionItems.slice(0, visibleChipCount));

  let showIdleInstances = $state(false);
  const shouldCollapseIdle = $derived(fleet.idle.length > 4);

  // =========================================================================
  // Keyboard navigation
  // =========================================================================

  let focusedIndex = $state(0);
  let panelEl: HTMLElement | undefined = $state(undefined);

  // Build flat list of all navigable rows in expanded panel
  const allRows = $derived.by(() => {
    const rows: Array<{ id: string; type: 'inbox' | 'active' | 'idle' }> = [];
    for (const entry of actionItems) rows.push({ id: entry.instance.id, type: 'inbox' });
    for (const entry of activeEntries) rows.push({ id: entry.instance.id, type: 'active' });
    for (const inst of fleet.idle) rows.push({ id: inst.id, type: 'idle' });
    return rows;
  });

  // Clamp focusedIndex
  $effect(() => {
    if (focusedIndex >= allRows.length) {
      focusedIndex = Math.max(0, allRows.length - 1);
    }
  });

  // Auto-focus panel on open
  $effect(() => {
    if (expanded && panelEl) {
      panelEl.focus();
    }
  });

  // Scroll focused row into view
  $effect(() => {
    if (panelEl && allRows.length > 0) {
      const focusedId = allRows[focusedIndex]?.id;
      if (focusedId) {
        const row = panelEl.querySelector(`[data-instance-id="${focusedId}"]`) as HTMLElement | null;
        row?.scrollIntoView({ block: 'nearest' });
      }
    }
  });

  function handleStripKeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowLeft') {
      e.preventDefault();
      focusedIndex = Math.max(focusedIndex - 1, 0);
    } else if (e.key === 'ArrowRight') {
      e.preventDefault();
      focusedIndex = Math.min(focusedIndex + 1, Math.max(0, actionItems.length - 1));
    } else if (e.key === 'Enter' && actionItems.length > 0 && actionItems[focusedIndex]) {
      e.preventDefault();
      onselect(actionItems[focusedIndex].instance.id);
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      onexpand();
    }
  }

  function handlePanelKeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      focusedIndex = Math.min(focusedIndex + 1, allRows.length - 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      focusedIndex = Math.max(focusedIndex - 1, 0);
    } else if (e.key === 'Enter' && allRows.length > 0 && allRows[focusedIndex]) {
      e.preventDefault();
      onselect(allRows[focusedIndex].id);
      onclose();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      onclose();
    }
  }

  // =========================================================================
  // Context menu + row helpers
  // =========================================================================

  function handleRowContextMenu(instance: Instance, e: MouseEvent) {
    e.preventDefault();
    const target = e.currentTarget as HTMLElement;
    oncontextmenu(instance, target.getBoundingClientRect());
  }

  function handleMenuClick(instance: Instance, e: MouseEvent) {
    e.stopPropagation();
    const target = e.currentTarget as HTMLElement;
    oncontextmenu(instance, target.getBoundingClientRect());
  }

  function getRowIndex(instanceId: string): number {
    return allRows.findIndex((r) => r.id === instanceId);
  }
</script>

<div class="fleet-command-center">
  <!-- ===== COLLAPSED STRIP ===== -->
  <div class="strip" role="toolbar" tabindex="0" onkeydown={handleStripKeydown}>
    <!-- Action queue -->
    <div class="action-queue" bind:this={queueEl}>
      {#if actionItems.length > 0}
        {#each visibleChips as { item, instance } (item.instance_id)}
          <ActionChip
            {item}
            {instance}
            onclick={() => onselect(instance.id)}
            ondismiss={item.event_type === 'completed_turn' ? () => dismissInboxItem(instance.id) : undefined}
          />
        {/each}
        {#if overflowCount > 0}
          <button class="overflow-badge" onclick={onexpand} title="Show all inbox items">
            +{overflowCount} more
          </button>
        {/if}
      {/if}
    </div>

    <!-- Situation summary — structured segments with inline activity bars -->
    {#if hasActivity}
      <button class="situation-summary" onclick={onexpand} title="Show fleet details">
        {#each summarySegments as seg, si}
          {#if si > 0}<span class="summary-sep"> &middot; </span>{/if}
          {#if seg.kind === 'activity'}
            {#each seg.items as item, ii}
              {#if ii > 0}<span class="summary-sep">, </span>{/if}
              <span class="summary-duration" style={heatStyle(item.secs)}>{item.duration}</span><!--
							--><span
                class="activity-bar"
                class:hot={isHot(item.secs)}
                style="width: {barPx(item.secs)}px; background: {heatColor(item.secs)}"
              ></span>
            {/each}
            <span class="summary-label"> {seg.label}</span>
          {:else}
            <span class="summary-count">{seg.count}</span>
            <span class="summary-label"> {seg.label}</span>
          {/if}
        {/each}
      </button>
    {/if}

    <!-- Fleet composition bar — proportional spatial overview -->
    {#if fleetBarSegments.length > 0}
      <button class="fleet-bar" onclick={onexpand} title="Fleet composition">
        {#each fleetBarSegments as seg (seg.type)}
          <span class="bar-seg {seg.type}" style="flex: {seg.flex}"></span>
        {/each}
      </button>
    {/if}

    <!-- Expand toggle -->
    {#if instances.length > 0}
      <button
        class="strip-expand"
        onclick={onexpand}
        title={expanded ? 'Collapse fleet panel' : 'Expand fleet panel'}
        aria-expanded={expanded}
      >
        {#if actionItems.length > 0 && !expanded}
          <span class="expand-badge">{actionItems.length}</span>
        {/if}
        <svg
          class="expand-chevron"
          class:open={expanded}
          viewBox="0 0 16 16"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
        >
          <polyline points="4,6 8,10 12,6" />
        </svg>
      </button>
    {/if}
  </div>

  <!-- ===== EXPANDED PANEL ===== -->
  {#if expanded}
    <button class="panel-backdrop" onclick={onclose} aria-label="Close fleet panel"></button>

    <div
      class="fleet-panel"
      role="listbox"
      aria-label="Fleet instances"
      bind:this={panelEl}
      onkeydown={handlePanelKeydown}
      tabindex="-1"
    >
      <div class="panel-body">
        <!-- TIER 1: ACTIONS (inbox items) -->
        {#if actionItems.length > 0}
          <div class="tier-label">
            <span class="tier-name inbox-label">Actions</span>
            <span class="tier-count inbox-count">{actionItems.length}</span>
          </div>
          {#each actionItems as { item, instance } (item.instance_id)}
            {@const rowIdx = getRowIndex(instance.id)}
            <div
              role="group"
              data-instance-id={instance.id}
              oncontextmenu={(e) => handleRowContextMenu(instance, e)}
              onmouseenter={() => (focusedIndex = rowIdx)}
            >
              <ActionCard
                {item}
                {instance}
                {tick}
                highlighted={rowIdx === focusedIndex}
                onprimary={() => {
                  onselect(instance.id);
                  onclose();
                }}
                ondismiss={item.event_type !== 'needs_input' ? () => dismissInboxItem(instance.id) : undefined}
              />
            </div>
          {/each}
        {/if}

        <!-- TIER 2: ACTIVITY (active non-inbox instances with timeline bars) -->
        {#if activeEntries.length > 0}
          <div class="tier-label">
            <span class="tier-name">Activity</span>
            <span class="tier-count">{activeEntries.length}</span>
          </div>
          {#each activeEntries as entry (entry.instance.id)}
            {@const inst = entry.instance}
            {@const rowIdx = getRowIndex(inst.id)}
            <div
              role="button"
              tabindex="0"
              class="panel-row activity-row"
              class:highlighted={rowIdx === focusedIndex}
              class:current={currentInstanceId === inst.id}
              class:in-pane={paneInstanceIds.has(inst.id)}
              data-instance-id={inst.id}
              onclick={() => {
                onselect(inst.id);
                onclose();
              }}
              onkeydown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  onselect(inst.id);
                  onclose();
                }
              }}
              oncontextmenu={(e) => handleRowContextMenu(inst, e)}
              onmouseenter={() => (focusedIndex = rowIdx)}
            >
              <!-- Timeline bar — fills behind the row content -->
              <span
                class="row-timeline"
                class:hot={isHot(entry.secs)}
                style="width: {entry.barPct}%; background: {heatColor(entry.secs)}"
              ></span>
              <span class="row-kind">
                <InstanceKindIcon kind={inst.kind} />
              </span>
              <span class="row-led" style="background: {entry.stateInfo.color}" class:pulse={entry.stateInfo.animate}
              ></span>
              <span class="row-name">{inst.custom_name ?? inst.name}</span>
              {#if entry.stateInfo.label}
                <span class="row-state">{entry.stateInfo.label}</span>
              {/if}
              {#if entry.duration}
                <span class="row-duration" style={heatStyle(entry.secs)}>{entry.duration}</span>
              {/if}
              <button
                class="row-menu"
                onclick={(e) => handleMenuClick(inst, e)}
                title="Instance actions"
                aria-label="Instance actions"
              >
                <svg viewBox="0 0 16 16" fill="currentColor"
                  ><circle cx="8" cy="3" r="1.5" /><circle cx="8" cy="8" r="1.5" /><circle
                    cx="8"
                    cy="13"
                    r="1.5"
                  /></svg
                >
              </button>
            </div>
          {/each}
        {/if}

        <!-- TIER 3: READY (idle instances) -->
        {#if fleet.idle.length > 0}
          <div class="tier-label">
            <span class="tier-name">Ready</span>
            <span class="tier-count">{fleet.idle.length}</span>
            {#if shouldCollapseIdle}
              <button class="tier-toggle" onclick={() => (showIdleInstances = !showIdleInstances)}>
                {showIdleInstances ? 'collapse' : 'show all'}
              </button>
            {/if}
          </div>
          {#if !shouldCollapseIdle || showIdleInstances}
            {#each fleet.idle as inst (inst.id)}
              {@const stateInfo = getStateInfo(inst.id, inst.claude_state, inst.claude_state_stale)}
              {@const rowIdx = getRowIndex(inst.id)}
              <div
                role="button"
                tabindex="0"
                class="panel-row idle-row"
                class:highlighted={rowIdx === focusedIndex}
                class:current={currentInstanceId === inst.id}
                class:in-pane={paneInstanceIds.has(inst.id)}
                data-instance-id={inst.id}
                onclick={() => {
                  onselect(inst.id);
                  onclose();
                }}
                onkeydown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault();
                    onselect(inst.id);
                    onclose();
                  }
                }}
                oncontextmenu={(e) => handleRowContextMenu(inst, e)}
                onmouseenter={() => (focusedIndex = rowIdx)}
              >
                <span class="row-kind">
                  <InstanceKindIcon kind={inst.kind} />
                </span>
                <span class="row-led" style="background: {stateInfo.color}"></span>
                <span class="row-name">{inst.custom_name ?? inst.name}</span>
                {#if stateInfo.label}
                  <span class="row-state">{stateInfo.label}</span>
                {/if}
                <button
                  class="row-menu"
                  onclick={(e) => handleMenuClick(inst, e)}
                  title="Instance actions"
                  aria-label="Instance actions"
                >
                  <svg viewBox="0 0 16 16" fill="currentColor"
                    ><circle cx="8" cy="3" r="1.5" /><circle cx="8" cy="8" r="1.5" /><circle
                      cx="8"
                      cy="13"
                      r="1.5"
                    /></svg
                  >
                </button>
              </div>
            {/each}
          {:else}
            <!-- Collapsed idle: compact grid -->
            <div class="idle-grid">
              {#each fleet.idle as inst (inst.id)}
                <button
                  class="idle-chip"
                  onclick={() => {
                    onselect(inst.id);
                    onclose();
                  }}
                  oncontextmenu={(e) => handleRowContextMenu(inst, e)}
                  title={inst.custom_name ?? inst.name}
                >
                  <span class="row-kind mini">
                    <InstanceKindIcon kind={inst.kind} />
                  </span>
                  <span class="chip-name">{inst.custom_name ?? inst.name}</span>
                </button>
              {/each}
            </div>
          {/if}
        {/if}

        <!-- New Instance ghost button -->
        <div class="tier-footer">
          <button class="new-instance-btn" onclick={oncreate}>
            <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2">
              <line x1="8" y1="3" x2="8" y2="13" />
              <line x1="3" y1="8" x2="13" y2="8" />
            </svg>
            New Instance
          </button>
        </div>

        {#if instances.length === 0}
          <div class="panel-empty">No instances</div>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .fleet-command-center {
    display: flex;
    align-items: center;
    flex: 1;
    min-width: 0;
  }

  /* ====== STRIP ====== */
  .strip {
    display: flex;
    align-items: center;
    gap: 4px;
    flex: 1;
    min-width: 0;
  }

  .action-queue {
    display: flex;
    align-items: center;
    gap: 3px;
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
  }

  .overflow-badge {
    display: flex;
    align-items: center;
    padding: 3px 8px;
    background: var(--surface-600);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    color: var(--chrome-accent-400);
    font-size: 9px;
    font-weight: 700;
    font-family: inherit;
    letter-spacing: 0.03em;
    cursor: pointer;
    flex-shrink: 0;
    transition: all 0.1s ease;
    min-height: 26px;
    white-space: nowrap;
  }

  .overflow-badge:hover {
    background: var(--surface-500);
    border-color: var(--chrome-accent-600);
  }

  /* ====== Situation summary ====== */
  .situation-summary {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 2px 6px;
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 9px;
    font-weight: 600;
    font-family: inherit;
    letter-spacing: 0.03em;
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-left: auto;
    transition: color 0.1s ease;
  }

  .situation-summary:hover {
    color: var(--text-secondary);
  }

  .summary-label {
    color: var(--text-muted);
  }

  .summary-sep {
    color: var(--text-muted);
    opacity: 0.5;
  }

  .summary-count {
    font-weight: 700;
  }

  .summary-duration {
    font-weight: 600;
  }

  /* Inline activity bar — the "shape of time" */
  .activity-bar {
    display: inline-block;
    height: 2px;
    border-radius: 1px;
    vertical-align: middle;
    margin-left: 2px;
    max-width: 24px;
    transition:
      width 1s linear,
      background 1s linear;
  }

  .activity-bar.hot {
    animation: bar-glow 2s ease-in-out infinite;
  }

  @keyframes bar-glow {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.6;
    }
  }

  /* ====== Fleet composition bar ====== */
  .fleet-bar {
    display: flex;
    height: 6px;
    min-width: 32px;
    max-width: 48px;
    flex-shrink: 0;
    border-radius: 1px;
    overflow: hidden;
    gap: 1px;
    cursor: pointer;
    background: transparent;
    border: none;
    padding: 0;
    margin-left: 4px;
    opacity: 0.8;
    transition: opacity 0.1s ease;
  }

  .fleet-bar:hover {
    opacity: 1;
  }

  .bar-seg {
    min-width: 2px;
    border-radius: 0.5px;
  }

  .bar-seg.attention {
    background: var(--status-red);
  }
  .bar-seg.active {
    background: var(--thinking-500);
  }
  .bar-seg.idle {
    background: var(--status-green);
    opacity: 0.4;
  }

  /* ====== Expand toggle ====== */
  .strip-expand {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    flex-shrink: 0;
    position: relative;
    transition: color 0.12s ease;
  }

  .strip-expand:hover {
    color: var(--text-primary);
  }

  .expand-chevron {
    width: 12px;
    height: 12px;
    transition: transform 0.15s ease;
    opacity: 0.6;
  }

  .expand-chevron.open {
    transform: rotate(180deg);
  }

  .expand-badge {
    position: absolute;
    top: 0;
    right: 0;
    min-width: 12px;
    height: 12px;
    padding: 0 2px;
    font-size: 7px;
    font-weight: 700;
    line-height: 12px;
    text-align: center;
    border-radius: 6px;
    background: var(--chrome-accent-500);
    color: var(--surface-900);
  }

  /* ====== PANEL ====== */
  .panel-backdrop {
    position: fixed;
    inset: 0;
    z-index: 49;
    background: transparent;
    border: none;
    padding: 0;
    margin: 0;
    cursor: default;
  }

  .fleet-panel {
    position: fixed;
    top: 40px;
    left: 48px;
    right: 0;
    z-index: 50;
    background: var(--surface-700);
    border-bottom: 1px solid var(--surface-border);
    box-shadow: var(--shadow-dropdown);
    animation: panel-slide 0.15s ease-out;
    outline: none;
  }

  .fleet-panel:focus-visible {
    /* Suppress browser focus ring — keyboard nav is visual via .highlighted */
    outline: none;
  }

  @keyframes panel-slide {
    from {
      opacity: 0;
      transform: translateY(-8px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .panel-body {
    overflow-y: auto;
    max-height: min(60vh, 480px);
  }

  /* ====== Tier labels ====== */
  .tier-label {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 10px 4px;
    border-top: 1px solid var(--surface-border);
  }

  .tier-label:first-child {
    border-top: none;
  }

  .tier-name {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .tier-name.inbox-label {
    color: var(--chrome-accent-400);
  }

  .tier-count {
    font-size: 8px;
    font-weight: 700;
    color: var(--text-muted);
    background: var(--surface-600);
    padding: 1px 5px;
    border-radius: 2px;
    line-height: 1.2;
  }

  .tier-count.inbox-count {
    background: color-mix(in srgb, var(--chrome-accent-500) 20%, var(--surface-600));
    color: var(--chrome-accent-400);
  }

  .tier-toggle {
    font-size: 8px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--chrome-accent-500);
    background: transparent;
    border: none;
    cursor: pointer;
    padding: 0;
    margin-left: auto;
  }

  .tier-toggle:hover {
    color: var(--chrome-accent-400);
  }

  /* ====== Panel rows ====== */
  .panel-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-height: 32px;
    padding: 0 10px;
    background: transparent;
    border: none;
    border-left: 2px solid transparent;
    color: var(--text-secondary);
    font-size: 11px;
    font-weight: 600;
    font-family: inherit;
    letter-spacing: 0.03em;
    cursor: pointer;
    transition: background 0.08s ease;
    text-align: left;
    position: relative;
    overflow: hidden;
  }

  .panel-row:hover,
  .panel-row.highlighted {
    background: var(--tint-active-strong);
  }

  .panel-row.in-pane {
    border-left-color: var(--chrome-accent-700);
  }

  .panel-row.current {
    color: var(--chrome-accent-400);
    border-left-color: var(--chrome-accent-500);
  }

  .panel-row.highlighted.current {
    background: var(--tint-focus);
  }

  /* Timeline bar — fills behind activity row content */
  .row-timeline {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    opacity: 0.08;
    transition:
      width 1s linear,
      background 1s linear;
    pointer-events: none;
  }

  .row-timeline.hot {
    opacity: 0.12;
    animation: timeline-pulse 2s ease-in-out infinite;
  }

  @keyframes timeline-pulse {
    0%,
    100% {
      opacity: 0.12;
    }
    50% {
      opacity: 0.06;
    }
  }

  .row-kind {
    width: 12px;
    height: 12px;
    flex-shrink: 0;
    color: var(--text-muted);
    opacity: 0.6;
    position: relative;
  }

  .row-kind :global(svg) {
    width: 12px;
    height: 12px;
    display: block;
  }

  .row-kind.mini {
    width: 10px;
    height: 10px;
  }

  .row-kind.mini :global(svg) {
    width: 10px;
    height: 10px;
  }

  .row-led {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    position: relative;
  }

  .row-led.pulse {
    animation: led-pulse 0.8s ease-in-out infinite;
  }

  .row-name {
    text-transform: uppercase;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
    position: relative;
  }

  .row-state {
    font-size: 9px;
    color: var(--text-muted);
    opacity: 0.8;
    flex-shrink: 0;
    white-space: nowrap;
    position: relative;
  }

  .row-duration {
    font-size: 9px;
    flex-shrink: 0;
    position: relative;
  }

  /* ====== Kebab menu button ====== */
  .row-menu {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 2px;
    color: var(--text-muted);
    cursor: pointer;
    flex-shrink: 0;
    opacity: 0;
    transition: all 0.1s ease;
    position: relative;
  }

  .row-menu svg {
    width: 12px;
    height: 12px;
  }

  .panel-row:hover .row-menu,
  .panel-row.highlighted .row-menu,
  .row-menu:focus-visible {
    opacity: 0.5;
  }

  .row-menu:hover {
    opacity: 1;
    background: var(--surface-500);
    border-color: var(--surface-border-light);
    color: var(--text-primary);
  }

  /* ====== Idle grid (collapsed) ====== */
  .idle-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    padding: 4px 10px 8px;
  }

  .idle-chip {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 3px 8px;
    background: var(--surface-600);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    color: var(--text-muted);
    font-size: 9px;
    font-weight: 600;
    font-family: inherit;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    cursor: pointer;
    transition: all 0.1s ease;
  }

  .idle-chip:hover {
    background: var(--surface-500);
    color: var(--text-secondary);
    border-color: var(--surface-border-light);
  }

  .chip-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 80px;
  }

  /* ====== Tier footer ====== */
  .tier-footer {
    padding: 8px 10px;
    border-top: 1px solid var(--surface-border);
  }

  .new-instance-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 6px 10px;
    background: transparent;
    border: 1px dashed var(--surface-border);
    border-radius: 3px;
    color: var(--text-muted);
    font-size: 10px;
    font-weight: 600;
    font-family: inherit;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .new-instance-btn svg {
    width: 10px;
    height: 10px;
  }

  .new-instance-btn:hover {
    border-color: var(--chrome-accent-600);
    color: var(--chrome-accent-400);
    background: var(--tint-active);
  }

  .panel-empty {
    padding: 16px 10px;
    text-align: center;
    color: var(--text-muted);
    font-size: 10px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  @keyframes led-pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.4;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .row-led.pulse {
      animation: none;
    }
    .fleet-panel {
      animation: none;
    }
    .activity-bar.hot {
      animation: none;
    }
    .row-timeline.hot {
      animation: none;
    }
  }

  /* ====== Analog theme ====== */
  :global([data-theme='analog']) .fleet-panel {
    background-color: var(--surface-700);
    background-image: var(--grain-fine), var(--grain-coarse);
    background-blend-mode: multiply, multiply;
  }

  :global([data-theme='analog']) .overflow-badge {
    background-color: var(--surface-600);
    background-image: var(--grain-fine);
    background-blend-mode: multiply;
  }

  :global([data-theme='analog']) .idle-chip {
    background-color: var(--surface-600);
    background-image: var(--grain-fine);
    background-blend-mode: multiply;
  }

  /* ====== Mobile: full-width panel ====== */
  @media (max-width: 639px) {
    .fleet-panel {
      left: 0;
    }
  }
</style>
