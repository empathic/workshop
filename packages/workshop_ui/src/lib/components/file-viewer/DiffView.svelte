<script lang="ts">
  import type { InlineHighlight, GitDiffFile } from '$lib/stores/git';

  interface DiffLine {
    type: string;
    oldNum?: number | null;
    newNum?: number | null;
    content: string;
    highlights?: InlineHighlight[];
  }

  interface DiffHunk {
    header: string;
    lines: DiffLine[];
  }

  interface DiffData {
    additions: number;
    deletions: number;
    hunks: DiffHunk[];
  }

  interface GapInfo {
    key: string;
    newStart: number;
    newEnd: number;
    count: number;
  }

  interface GapLayout {
    before: Map<number, GapInfo>;
    after: GapInfo | null;
  }

  interface Props {
    diffData: DiffData;
    diffEngine: string;
    actualEngine?: string;
    refreshStatus: 'idle' | 'loading' | 'done';
    showRefreshOverlay: boolean;
    onengineToggle: () => void;
    onfetchContent?: () => Promise<string>;
    allFiles?: GitDiffFile[];
    currentFileIndex?: number;
    onfileSelect?: (index: number) => void;
  }

  let {
    diffData,
    diffEngine,
    actualEngine,
    refreshStatus,
    showRefreshOverlay,
    onengineToggle,
    onfetchContent,
    allFiles = [],
    currentFileIndex = 0,
    onfileSelect
  }: Props = $props();

  const hasMultipleFiles = $derived(allFiles.length > 1);
  let fileListExpanded = $state(false);

  let diffViewEl: HTMLElement | undefined = $state();

  // --- Expandable context state ---
  let fileLines: string[] | null = $state(null);
  let expandedGaps: Set<string> = $state(new Set());
  let expandLoading: string | null = $state(null);

  // Reset when the actual diff content changes (new file or engine toggle).
  // We fingerprint by hunk headers — immune to Svelte's Proxy re-wrapping.
  const diffFingerprint = $derived(diffData.hunks.map((h) => h.header).join('\n'));
  let lastFingerprint = '';
  $effect(() => {
    if (diffFingerprint !== lastFingerprint) {
      lastFingerprint = diffFingerprint;
      fileLines = null;
      expandedGaps = new Set();
      expandLoading = null;
    }
  });

  // Parse @@ -oldStart,oldCount +newStart,newCount @@
  function parseHunkHeader(header: string): { oldStart: number; oldCount: number; newStart: number; newCount: number } {
    const m = header.match(/@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/);
    if (!m) return { oldStart: 1, oldCount: 0, newStart: 1, newCount: 0 };
    return {
      oldStart: parseInt(m[1], 10),
      oldCount: m[2] !== undefined ? parseInt(m[2], 10) : 1,
      newStart: parseInt(m[3], 10),
      newCount: m[4] !== undefined ? parseInt(m[4], 10) : 1
    };
  }

  function hunkNewEnd(hunk: DiffHunk): number {
    const parsed = parseHunkHeader(hunk.header);
    return parsed.newStart + parsed.newCount - 1;
  }

  // Single-pass gap computation: produces the Map<hunkIdx, GapInfo> and trailing gap
  const gapLayout = $derived.by((): GapLayout => {
    const hunks = diffData.hunks;
    const before = new Map<number, GapInfo>();
    let after: GapInfo | null = null;

    if (hunks.length === 0) return { before, after };

    // Gap before first hunk
    const first = parseHunkHeader(hunks[0].header);
    if (first.newStart > 1) {
      before.set(0, { key: 'gap-0', newStart: 1, newEnd: first.newStart - 1, count: first.newStart - 1 });
    }

    // Gaps between consecutive hunks
    for (let i = 1; i < hunks.length; i++) {
      const prevEnd = hunkNewEnd(hunks[i - 1]);
      const nextStart = parseHunkHeader(hunks[i].header).newStart;
      if (nextStart > prevEnd + 1) {
        before.set(i, {
          key: `gap-${i}`,
          newStart: prevEnd + 1,
          newEnd: nextStart - 1,
          count: nextStart - prevEnd - 1
        });
      }
    }

    // Gap after last hunk (only when we know total line count)
    if (fileLines) {
      const lastEnd = hunkNewEnd(hunks[hunks.length - 1]);
      if (lastEnd < fileLines.length) {
        after = {
          key: `gap-${hunks.length}`,
          newStart: lastEnd + 1,
          newEnd: fileLines.length,
          count: fileLines.length - lastEnd
        };
      }
    }

    return { before, after };
  });

  const canExpand = $derived(!!onfetchContent);

  async function toggleGap(gap: GapInfo) {
    if (expandedGaps.has(gap.key)) {
      const next = new Set(expandedGaps);
      next.delete(gap.key);
      expandedGaps = next;
      return;
    }

    // Fetch file content on first expand
    if (!fileLines && onfetchContent) {
      expandLoading = gap.key;
      try {
        const content = await onfetchContent();
        fileLines = content.split('\n');
      } catch {
        expandLoading = null;
        return;
      }
      expandLoading = null;
    }

    const next = new Set(expandedGaps);
    next.add(gap.key);
    expandedGaps = next;
  }

  function selectFile(index: number) {
    onfileSelect?.(index);
    fileListExpanded = false;
    requestAnimationFrame(() => diffViewEl?.focus());
  }

  function statusLetter(status: string): string {
    return status === 'added' ? 'A' : status === 'deleted' ? 'D' : status === 'renamed' ? 'R' : 'M';
  }

  function getStatusColor(status: string): string {
    switch (status) {
      case 'added':
        return 'var(--amber-400)';
      case 'deleted':
        return 'var(--status-red-text)';
      case 'renamed':
        return 'var(--purple-400)';
      default:
        return 'var(--text-secondary)';
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!hasMultipleFiles || !onfileSelect) return;
    if (e.key === '[' || (e.key === 'k' && e.altKey)) {
      e.preventDefault();
      if (currentFileIndex > 0) onfileSelect(currentFileIndex - 1);
    } else if (e.key === ']' || (e.key === 'j' && e.altKey)) {
      e.preventDefault();
      if (currentFileIndex < allFiles.length - 1) onfileSelect(currentFileIndex + 1);
    }
  }

  let fileListEntriesEl: HTMLElement | undefined = $state();

  // Scroll the active file entry into view
  $effect(() => {
    if (hasMultipleFiles && fileListEntriesEl && fileListExpanded) {
      const activeEntry = fileListEntriesEl.querySelector('.file-list-entry.active');
      activeEntry?.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    }
  });

  // Auto-focus for keyboard shortcuts when multi-file navigation is available
  $effect(() => {
    if (diffViewEl && hasMultipleFiles) {
      diffViewEl.focus();
    }
  });

  function splitByHighlights(
    content: string,
    highlights?: InlineHighlight[]
  ): Array<{ text: string; highlighted: boolean }> {
    if (!highlights || highlights.length === 0) {
      return [{ text: content, highlighted: false }];
    }
    const segments: Array<{ text: string; highlighted: boolean }> = [];
    let pos = 0;
    for (const hl of highlights) {
      if (hl.start > pos) {
        segments.push({ text: content.slice(pos, hl.start), highlighted: false });
      }
      segments.push({ text: content.slice(hl.start, hl.end), highlighted: true });
      pos = hl.end;
    }
    if (pos < content.length) {
      segments.push({ text: content.slice(pos), highlighted: false });
    }
    return segments;
  }
</script>

<!-- Snippet: single diff line (hunk lines and expanded context lines) -->
{#snippet diffLine(type: string, oldNum: number | string, newNum: number | string, content: string, highlights?: InlineHighlight[])}
  <div class="diff-line {type}">
    <span class="line-num old">{oldNum}</span>
    <span class="line-num new">{newNum}</span>
    <span class="line-marker">{type === 'add' ? '+' : type === 'del' ? '-' : ' '}</span>
    <span class="line-content"
      >{#each splitByHighlights(content, highlights) as seg}{#if seg.highlighted}<mark class="inline-hl"
            >{seg.text}</mark
          >{:else}{seg.text}{/if}{/each}</span
    >
  </div>
{/snippet}

<!-- Snippet: gap bar (collapsed or expanded) -->
{#snippet gapBar(gap: GapInfo)}
  {#if expandedGaps.has(gap.key) && fileLines}
    <button class="diff-gap-bar expanded" onclick={() => toggleGap(gap)}>
      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><polyline points="4 10 8 6 12 10"/></svg>
      <span>Collapse {gap.count} lines</span>
    </button>
    {#each fileLines.slice(gap.newStart - 1, gap.newEnd) as ctxLine, ctxIdx}
      {@render diffLine('ctx', gap.newStart + ctxIdx, gap.newStart + ctxIdx, ctxLine)}
    {/each}
  {:else}
    <button
      class="diff-gap-bar"
      class:loading={expandLoading === gap.key}
      onclick={() => toggleGap(gap)}
      disabled={!canExpand || expandLoading !== null}
    >
      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><polyline points="4 6 8 10 12 6"/></svg>
      <span>{expandLoading === gap.key ? 'Loading...' : `${gap.count} hidden lines`}</span>
    </button>
  {/if}
{/snippet}

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="diff-view" class:refreshing={showRefreshOverlay} onkeydown={handleKeydown} tabindex="-1" bind:this={diffViewEl}>
  {#if showRefreshOverlay}
    <div class="diff-refresh-bar"></div>
  {/if}
  <div class="diff-stats-bar">
    <span class="diff-stat additions">+{diffData.additions}</span>
    <span class="diff-stat deletions">-{diffData.deletions}</span>
    {#if hasMultipleFiles}
      <span class="file-position">File {currentFileIndex + 1} of {allFiles.length}</span>
    {/if}
    <button
      class="engine-toggle"
      class:active={diffEngine !== 'standard'}
      onclick={onengineToggle}
      disabled={refreshStatus === 'loading'}
      title={diffEngine === 'structural'
        ? 'Using structural diff (syntax-aware)'
        : diffEngine === 'patience'
          ? 'Using patience diff (word-level)'
          : 'Using standard diff'}
    >
      {diffEngine}
    </button>
    {#if diffEngine === 'structural' && actualEngine === 'patience'}
      <span class="engine-fallback" title="Structural diff unavailable for this file — using patience"
        >fell back to patience</span
      >
    {/if}
    <span class="refresh-indicator" class:loading={refreshStatus === 'loading'} class:done={refreshStatus === 'done'}
    ></span>
  </div>
  {#if hasMultipleFiles}
    <div class="file-list-panel">
      <button class="file-list-toggle" onclick={() => { fileListExpanded = !fileListExpanded; }}>
        <span class="file-list-summary">{allFiles.length} files changed</span>
        <svg class="file-list-chevron" class:expanded={fileListExpanded} viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2"><polyline points="4 6 8 10 12 6"/></svg>
      </button>
      {#if fileListExpanded}
        <div class="file-list-entries" bind:this={fileListEntriesEl}>
          {#each allFiles as file, i}
            <button
              class="file-list-entry"
              class:active={i === currentFileIndex}
              onclick={() => selectFile(i)}
              title="{file.path} (+{file.additions} -{file.deletions})"
            >
              <span class="entry-status" style="color: {getStatusColor(file.status)}">{statusLetter(file.status)}</span>
              <span class="entry-path">{file.path}</span>
              <span class="entry-stats">
                <span class="entry-add">+{file.additions}</span>
                <span class="entry-del">-{file.deletions}</span>
              </span>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
  {#each diffData.hunks as hunk, hunkIdx}
    {#if gapLayout.before.has(hunkIdx)}
      {@render gapBar(gapLayout.before.get(hunkIdx)!)}
    {/if}
    <div class="diff-hunk-header">{hunk.header}</div>
    {#each hunk.lines as line}
      {@render diffLine(line.type, line.oldNum ?? '', line.newNum ?? '', line.content, line.highlights)}
    {/each}
  {/each}
  {#if gapLayout.after}
    {@render gapBar(gapLayout.after)}
  {/if}
</div>

<style>
  .diff-view {
    font-family: inherit;
    font-size: 12px;
    line-height: 1.5;
    position: relative;
    transition: opacity 0.2s ease;
    outline: none;
  }

  .diff-view.refreshing {
    opacity: 0.5;
    pointer-events: none;
  }

  .diff-refresh-bar {
    position: sticky;
    top: 0;
    left: 0;
    right: 0;
    height: 2px;
    z-index: 2;
    background: linear-gradient(90deg, transparent 0%, var(--accent-400) 40%, var(--accent-400) 60%, transparent 100%);
    background-size: 200% 100%;
    animation: shimmer 1.2s ease-in-out infinite;
  }

  @keyframes shimmer {
    0% {
      background-position: 100% 0;
    }
    100% {
      background-position: -100% 0;
    }
  }

  .diff-stats-bar {
    display: flex;
    gap: 12px;
    padding: 8px 16px;
    background: var(--surface-700);
    border-bottom: 1px solid var(--surface-border);
    font-size: 11px;
    font-weight: 600;
    font-family: inherit;
  }

  .diff-stat.additions {
    color: var(--accent-400);
    text-shadow: var(--emphasis);
  }

  .diff-stat.deletions {
    color: var(--text-muted);
  }

  .diff-hunk-header {
    padding: 6px 16px;
    background: var(--tint-hover);
    border-top: 1px solid var(--surface-border);
    border-bottom: 1px solid var(--surface-border);
    color: var(--accent-600);
    font-style: italic;
    font-size: 11px;
    user-select: none;
  }

  .diff-line {
    display: flex;
    align-items: stretch;
    min-height: 20px;
    border-left: 2px solid transparent;
  }

  .diff-line.add {
    background: var(--tint-active);
    border-left-color: var(--accent-500);
  }

  .diff-line.add .line-content {
    color: var(--accent-400);
  }

  .diff-line.del {
    background: var(--status-red-tint);
    border-left-color: var(--status-red-border);
  }

  .diff-line.del .line-content {
    color: var(--text-muted);
    text-decoration: line-through;
    text-decoration-color: var(--status-red-border);
  }

  .diff-line.ctx .line-content {
    color: var(--text-secondary);
  }

  .diff-line .line-num {
    display: inline-block;
    width: 4ch;
    padding: 0 4px;
    text-align: right;
    font-size: 10px;
    color: var(--text-muted);
    opacity: 0.5;
    user-select: none;
    flex-shrink: 0;
    font-variant-numeric: tabular-nums;
  }

  .diff-line .line-marker {
    display: inline-block;
    width: 2ch;
    text-align: center;
    flex-shrink: 0;
    font-weight: 700;
    user-select: none;
  }

  .diff-line.add .line-marker {
    color: var(--accent-400);
  }

  .diff-line.del .line-marker {
    color: var(--status-red-text);
  }

  .diff-line.ctx .line-marker {
    color: var(--text-muted);
    opacity: 0.3;
  }

  .diff-line .line-content {
    flex: 1;
    white-space: pre-wrap;
    word-break: break-all;
    padding-right: 16px;
  }

  /* Inline word-level highlights */
  .diff-line .line-content :global(.inline-hl) {
    background: none;
    border-radius: 2px;
    padding: 0 1px;
  }

  .diff-line.add .line-content :global(.inline-hl) {
    background: var(--tint-selection);
    color: var(--accent-300);
  }

  .diff-line.del .line-content :global(.inline-hl) {
    background: var(--status-red-strong);
    color: var(--status-red-text);
    text-decoration: line-through;
    text-decoration-color: var(--status-red-border);
  }

  .file-position {
    font-size: 10px;
    color: var(--text-muted);
    letter-spacing: 0.05em;
  }

  /* Engine toggle */
  .engine-toggle {
    margin-left: auto;
    padding: 2px 8px;
    background: var(--surface-600);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    font-family: inherit;
    font-size: 9px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .engine-toggle:hover {
    background: var(--surface-500);
    color: var(--text-secondary);
    border-color: var(--accent-600);
  }

  .engine-toggle.active {
    color: var(--accent-400);
    border-color: var(--tint-selection);
  }

  .engine-fallback {
    font-size: 9px;
    color: var(--accent-400);
    opacity: 0.7;
    letter-spacing: 0.03em;
  }

  .refresh-indicator {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    opacity: 0;
    transform: scale(0);
    transition:
      opacity 0.15s ease,
      transform 0.15s ease,
      background-color 0.2s ease;
  }

  .refresh-indicator.loading {
    opacity: 1;
    transform: scale(1);
    background: var(--accent-400);
    animation: indicator-pulse 0.6s ease-in-out infinite alternate;
  }

  .refresh-indicator.done {
    opacity: 1;
    transform: scale(1);
    background: var(--status-green-text);
    animation: indicator-fade 0.6s ease-out forwards;
  }

  @keyframes indicator-pulse {
    from {
      opacity: 0.4;
    }
    to {
      opacity: 1;
    }
  }

  @keyframes indicator-fade {
    0% {
      opacity: 1;
      transform: scale(1);
    }
    70% {
      opacity: 1;
      transform: scale(1);
    }
    100% {
      opacity: 0;
      transform: scale(0);
    }
  }

  /* Expandable gap bars */
  .diff-gap-bar {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 4px 16px;
    background: var(--surface-700);
    border: none;
    border-top: 1px solid var(--surface-border);
    border-bottom: 1px solid var(--surface-border);
    color: var(--text-muted);
    font-family: inherit;
    font-size: 10px;
    letter-spacing: 0.05em;
    cursor: pointer;
    transition: all 0.1s ease;
    user-select: none;
  }

  .diff-gap-bar:hover:not(:disabled) {
    background: var(--surface-600);
    color: var(--text-secondary);
  }

  .diff-gap-bar:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .diff-gap-bar.expanded {
    color: var(--amber-400);
    background: var(--tint-hover);
  }

  .diff-gap-bar.loading {
    opacity: 0.7;
    cursor: wait;
  }

  .diff-gap-bar svg {
    width: 12px;
    height: 12px;
    flex-shrink: 0;
  }

  /* Vertical file list panel */
  .file-list-panel {
    background: var(--surface-800);
    border-bottom: 1px solid var(--surface-border);
  }

  .file-list-toggle {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 5px 12px;
    background: var(--surface-700);
    border: none;
    border-bottom: 1px solid var(--surface-border);
    color: var(--text-secondary);
    font-family: inherit;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    cursor: pointer;
    transition: all 0.1s ease;
  }

  .file-list-toggle:hover {
    background: var(--surface-600);
    color: var(--amber-400);
  }

  .file-list-chevron {
    width: 12px;
    height: 12px;
    transition: transform 0.15s ease;
    flex-shrink: 0;
  }

  .file-list-chevron.expanded {
    transform: rotate(180deg);
  }

  .file-list-entries {
    max-height: 200px;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--surface-400) transparent;
  }

  .file-list-entries::-webkit-scrollbar {
    width: 5px;
  }

  .file-list-entries::-webkit-scrollbar-track {
    background: transparent;
  }

  .file-list-entries::-webkit-scrollbar-thumb {
    background: var(--surface-400);
    border-radius: 3px;
  }

  .file-list-entry {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 3px 12px;
    background: none;
    border: none;
    border-left: 2px solid transparent;
    color: var(--text-muted);
    font-family: inherit;
    font-size: 10px;
    cursor: pointer;
    transition: all 0.1s ease;
    text-align: left;
  }

  .file-list-entry:hover {
    background: var(--tint-hover);
    color: var(--text-secondary);
  }

  .file-list-entry.active {
    border-left-color: var(--amber-500);
    background: var(--tint-active);
    color: var(--amber-400);
  }

  .entry-status {
    font-weight: 700;
    font-size: 9px;
    width: 1.2ch;
    flex-shrink: 0;
  }

  .entry-path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .entry-stats {
    display: flex;
    gap: 4px;
    font-size: 9px;
    opacity: 0.7;
    flex-shrink: 0;
  }

  .entry-add {
    color: var(--amber-400);
  }

  .entry-del {
    color: var(--text-muted);
  }
</style>
