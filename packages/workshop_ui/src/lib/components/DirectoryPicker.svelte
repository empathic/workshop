<script lang="ts">
  import { untrack } from 'svelte';
  import { onMount } from 'svelte';
  import { apiGet, apiPost } from '$lib/utils/api';
  import { projects } from '$lib/stores/projects';

  interface BrowseEntry {
    name: string;
    path: string;
    hasChildren: boolean;
  }

  interface WorktreeInfo {
    path: string;
    branch: string;
    isMain: boolean;
  }

  interface GitRepoInfo {
    repoRoot: string;
    currentBranch: string;
    worktrees: WorktreeInfo[];
    localBranches: string[];
  }

  interface BrowseResponse {
    path: string;
    entries: BrowseEntry[];
    git: GitRepoInfo | null;
  }

  interface GitDetailedInfo {
    repoRoot: string;
    currentBranch: string;
    headSha: string;
    lastCommitSubject: string;
    lastCommitDate: string;
    remotes: { name: string; url: string }[];
    upstream: { name: string; ahead: number; behind: number } | null;
    changes: { staged: number; modified: number; untracked: number };
    stashCount: number;
    recentBranches: string[];
  }

  interface Column {
    path: string;
    entries: BrowseEntry[];
    selectedName: string | null;
    loading: boolean;
    error: string;
    git: GitRepoInfo | null;
  }

  interface Props {
    value: string;
    onselect: (path: string) => void;
  }

  let { value = $bindable(), onselect }: Props = $props();

  let columns = $state<Column[]>([]);
  let columnWidths = $state<number[]>([]);
  let draggingHandle = $state<number | null>(null);
  const DEFAULT_COL_WIDTH = 220;
  const MIN_COL_WIDTH = 120;
  let initialLoading = $state(true);
  let editingPath = $state(false);
  let pathInput = $state('');
  let showWorktreeForm = $state(false);
  let worktreeBranch = $state('');
  let worktreePath = $state('');
  let creatingWorktree = $state(false);
  let creatingDir = $state(false);
  let newDirName = $state('');
  let showNewDirInput = $state<number | null>(null); // column index where input is active
  let newDirInputEl: HTMLInputElement | undefined = $state();

  let showGitDetail = $state(false);
  let gitDetail = $state<GitDetailedInfo | null>(null);
  let gitDetailLoading = $state(false);
  let lastGitRepoRoot = $state<string | null>(null);

  let hoveredPath = $state<string | null>(null);
  let hoveredIsPrefix = $state(false); // true when hover source is project/worktree (prefix match)
  let copied = $state(false);

  async function copyPath() {
    const path = currentPath();
    if (!path) return;
    try {
      await navigator.clipboard.writeText(path);
      copied = true;
      setTimeout(() => {
        copied = false;
      }, 1500);
    } catch {
      /* clipboard not available */
    }
  }

  let columnsEl: HTMLDivElement | undefined = $state();
  let pathInputEl: HTMLInputElement | undefined = $state();

  // The deepest selected path — either the selected entry in the last column,
  // or the last column's own path if nothing is selected there.
  let currentPath = $derived(() => {
    if (columns.length === 0) return '';
    const last = columns[columns.length - 1];
    if (last.selectedName) {
      const entry = last.entries.find((e) => e.name === last.selectedName);
      if (entry) return entry.path;
    }
    return last.path;
  });

  let gitInfo = $derived(() => {
    // Use git info from the deepest column that has it
    for (let i = columns.length - 1; i >= 0; i--) {
      if (columns[i].git) return columns[i].git;
    }
    return null;
  });

  let pathSegments = $derived(() => {
    const p = currentPath();
    if (!p) return [];
    const parts = p.split('/').filter(Boolean);
    return parts.map((part, i) => ({
      name: part,
      path: '/' + parts.slice(0, i + 1).join('/')
    }));
  });

  // Browse on mount — one-shot, doesn't re-trigger on value changes
  onMount(() => {
    expandToPath(untrack(() => value) || '');
  });

  async function fetchColumn(path: string): Promise<Column> {
    const col: Column = { path: '', entries: [], selectedName: null, loading: true, error: '', git: null };
    try {
      const params = path ? `?path=${encodeURIComponent(path)}` : '';
      const resp = await apiGet<BrowseResponse>(`/api/browse${params}`);
      col.path = resp.path;
      col.entries = resp.entries;
      col.git = resp.git;
    } catch (e) {
      col.path = path;
      col.error = e instanceof Error ? e.message : 'Failed to browse';
    }
    col.loading = false;
    return col;
  }

  async function expandToPath(targetPath: string) {
    let normalized = targetPath;
    if (!normalized) {
      // No path — let the server pick its default (server working directory)
      const defaultCol = await fetchColumn('');
      normalized = defaultCol.path;
    }
    // Split into segments: "/Users/alex/code" → ["/", "/Users", "/Users/alex", "/Users/alex/code"]
    const parts = normalized.split('/').filter(Boolean);
    const paths = parts.length > 0 ? parts.map((_, i) => '/' + parts.slice(0, i + 1).join('/')) : ['/'];

    // Find how much of the existing column chain matches
    let matchLen = 0;
    for (let i = 0; i < Math.min(columns.length, paths.length); i++) {
      if (columns[i].path === paths[i]) matchLen = i + 1;
      else break;
    }

    // Trim to matching prefix, preserving existing widths
    columns = columns.slice(0, matchLen);
    columnWidths = columnWidths.slice(0, matchLen);

    // If no match at all, start from root
    if (matchLen === 0) {
      const root = await fetchColumn(paths[0]);
      columns = [root];
      columnWidths = [DEFAULT_COL_WIDTH];
      matchLen = 1;
    }

    // Load remaining segments
    for (let i = matchLen; i < paths.length; i++) {
      // Mark selection in parent column
      let parentCol = columns[i - 1];
      let entry = parentCol.entries.find((e) => e.path === paths[i]);

      // If entry not found, parent is stale (e.g. dir was just created) — re-fetch it
      if (!entry) {
        const refreshed = await fetchColumn(parentCol.path);
        columns[i - 1] = refreshed;
        columns = [...columns]; // trigger reactivity
        parentCol = columns[i - 1];
        entry = parentCol.entries.find((e) => e.path === paths[i]);
      }
      if (entry) parentCol.selectedName = entry.name;

      const col = await fetchColumn(paths[i]);
      columns = [...columns, col];
      columnWidths = [...columnWidths, DEFAULT_COL_WIDTH];
    }

    // Select the final directory
    value = columns.length > 0 ? columns[columns.length - 1].path : normalized;
    onselect(value);
    initialLoading = false;
    scrollColumnsToEnd();
  }

  async function selectEntry(colIndex: number, entry: BrowseEntry) {
    // Mark this entry as selected in its column
    columns[colIndex].selectedName = entry.name;
    // Trim all columns after this one
    columns = columns.slice(0, colIndex + 1);
    columnWidths = columnWidths.slice(0, colIndex + 1);
    // Update selection
    value = entry.path;
    onselect(entry.path);

    // If it has children, load the next column
    if (entry.hasChildren) {
      const nextCol = await fetchColumn(entry.path);
      // Check we haven't navigated away while loading
      if (columns.length === colIndex + 1 && columns[colIndex].selectedName === entry.name) {
        columns = [...columns, nextCol];
        columnWidths = [...columnWidths, DEFAULT_COL_WIDTH];
        scrollColumnsToEnd();
      }
    }
  }

  function navigateToPath(path: string) {
    expandToPath(path);
  }

  function startEditingPath() {
    pathInput = currentPath();
    editingPath = true;
    requestAnimationFrame(() => pathInputEl?.focus());
  }

  function submitPathEdit() {
    editingPath = false;
    if (pathInput.trim()) {
      expandToPath(pathInput.trim());
    }
  }

  function cancelPathEdit() {
    editingPath = false;
  }

  function handlePathKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      submitPathEdit();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      cancelPathEdit();
    }
  }

  function handleColumnKeydown(e: KeyboardEvent, colIndex: number) {
    const col = columns[colIndex];
    const entries = col.entries;
    const selIdx = col.selectedName ? entries.findIndex((en) => en.name === col.selectedName) : -1;

    if (e.key === 'ArrowDown') {
      e.preventDefault();
      const next = Math.min(selIdx + 1, entries.length - 1);
      if (entries[next]) selectEntry(colIndex, entries[next]);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      const prev = Math.max(selIdx - 1, 0);
      if (entries[prev]) selectEntry(colIndex, entries[prev]);
    } else if (e.key === 'ArrowRight') {
      e.preventDefault();
      // Focus next column if it exists
      const nextCol = columnsEl?.querySelectorAll('.miller-column')[colIndex + 1] as HTMLElement;
      nextCol?.focus();
    } else if (e.key === 'ArrowLeft') {
      e.preventDefault();
      if (colIndex > 0) {
        const prevCol = columnsEl?.querySelectorAll('.miller-column')[colIndex - 1] as HTMLElement;
        prevCol?.focus();
      }
    }
  }

  function startNewDir(colIndex: number) {
    newDirName = '';
    showNewDirInput = colIndex;
    requestAnimationFrame(() => newDirInputEl?.focus());
  }

  function cancelNewDir() {
    showNewDirInput = null;
    newDirName = '';
  }

  async function submitNewDir(colIndex: number) {
    const name = newDirName.trim();
    if (!name || creatingDir) return;
    const parentPath = columns[colIndex].path;
    const fullPath = parentPath === '/' ? `/${name}` : `${parentPath}/${name}`;
    creatingDir = true;
    try {
      const resp = await apiPost<{ path: string }>('/api/browse/mkdir', { path: fullPath });
      showNewDirInput = null;
      newDirName = '';
      // Re-fetch the column to pick up the new entry
      const refreshed = await fetchColumn(parentPath);
      columns[colIndex] = refreshed;
      columns = [...columns]; // trigger reactivity
      // Select the new directory and open it
      const newEntry = refreshed.entries.find((e) => e.path === resp.path);
      if (newEntry) {
        await selectEntry(colIndex, newEntry);
      }
    } catch (e) {
      if (columns.length > 0) {
        columns[colIndex].error = e instanceof Error ? e.message : 'Failed to create directory';
      }
    } finally {
      creatingDir = false;
    }
  }

  function handleNewDirKeydown(e: KeyboardEvent, colIndex: number) {
    if (e.key === 'Enter') {
      e.preventDefault();
      submitNewDir(colIndex);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      cancelNewDir();
    }
  }

  function startColResize(e: PointerEvent, handleIndex: number) {
    e.preventDefault();
    draggingHandle = handleIndex;
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }

  function doColResize(e: PointerEvent, handleIndex: number) {
    if (draggingHandle !== handleIndex) return;
    const left = handleIndex - 1;
    const right = handleIndex;
    const dx = e.movementX;
    let lw = (columnWidths[left] ?? DEFAULT_COL_WIDTH) + dx;
    let rw = (columnWidths[right] ?? DEFAULT_COL_WIDTH) - dx;
    if (lw < MIN_COL_WIDTH) {
      rw += lw - MIN_COL_WIDTH;
      lw = MIN_COL_WIDTH;
    }
    if (rw < MIN_COL_WIDTH) {
      lw += rw - MIN_COL_WIDTH;
      rw = MIN_COL_WIDTH;
    }
    columnWidths[left] = lw;
    columnWidths[right] = rw;
  }

  function stopColResize() {
    draggingHandle = null;
  }

  function resetColWidth(handleIndex: number) {
    columnWidths[handleIndex - 1] = DEFAULT_COL_WIDTH;
    columnWidths[handleIndex] = DEFAULT_COL_WIDTH;
  }

  function scrollColumnsToEnd() {
    requestAnimationFrame(() => {
      if (columnsEl) {
        columnsEl.scrollLeft = columnsEl.scrollWidth;
      }
    });
  }

  async function handleCreateWorktree() {
    const git = gitInfo();
    if (!git || !worktreeBranch || !worktreePath) return;
    creatingWorktree = true;
    try {
      const isNew = !git.localBranches.includes(worktreeBranch);
      const resp = await apiPost<{ path: string }>('/api/browse/worktree', {
        repo_path: git.repoRoot,
        branch: worktreeBranch,
        target_path: worktreePath,
        new_branch: isNew
      });
      await expandToPath(resp.path);
      showWorktreeForm = false;
    } catch (e) {
      // Show error in the last column
      if (columns.length > 0) {
        columns[columns.length - 1].error = e instanceof Error ? e.message : 'Failed to create worktree';
      }
    } finally {
      creatingWorktree = false;
    }
  }

  function suggestWorktreePath(branch: string) {
    const git = gitInfo();
    if (!git) return '';
    const safeBranch = branch.replace(/\//g, '-');
    const parentDir = git.repoRoot.replace(/\/[^/]*$/, '') || '/';
    return `${parentDir}/${safeBranch}`;
  }

  // Re-fetch git detail when repo changes (keep panel open)
  $effect(() => {
    const git = gitInfo();
    const root = git?.repoRoot ?? null;
    if (root !== lastGitRepoRoot) {
      lastGitRepoRoot = root;
      gitDetail = null;
      if (showGitDetail && root) loadGitDetail();
    }
  });

  async function toggleGitDetail() {
    showGitDetail = !showGitDetail;
    if (showGitDetail && !gitDetail) loadGitDetail();
  }

  async function loadGitDetail() {
    const git = gitInfo();
    if (!git) return;
    gitDetailLoading = true;
    try {
      gitDetail = await apiGet<GitDetailedInfo>(`/api/browse/git-info?path=${encodeURIComponent(git.repoRoot)}`);
    } catch {
      /* swallow — panel just stays empty */
    } finally {
      gitDetailLoading = false;
    }
  }
</script>

<div class="directory-picker">
  <!-- Quick links: existing projects -->
  {#if $projects.length > 0}
    <div class="quick-links">
      <span class="section-label">PROJECTS</span>
      <div class="project-chips">
        {#each $projects as project}
          <button
            class="project-chip"
            class:active={value === project.workingDir}
            class:chip-hover={hoveredPath !== null &&
              (hoveredPath === project.workingDir || hoveredPath.startsWith(project.workingDir + '/'))}
            onclick={() => expandToPath(project.workingDir)}
            onmouseenter={() => {
              hoveredPath = project.workingDir;
              hoveredIsPrefix = true;
            }}
            onmouseleave={() => {
              if (hoveredPath === project.workingDir) hoveredPath = null;
            }}
            title={project.workingDir}
          >
            {project.name}
          </button>
        {/each}
      </div>
    </div>
  {/if}

  <!-- Path bar -->
  <div class="path-bar">
    <button class="copy-path-btn" class:copied onclick={copyPath} title={copied ? 'Copied!' : 'Copy path'}>
      {#if copied}
        <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2">
          <polyline points="3 8 7 12 13 4" />
        </svg>
      {:else}
        <svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor">
          <path
            d="M4 2a2 2 0 0 1 2-2h6a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V2zm2-1a1 1 0 0 0-1 1v8a1 1 0 0 0 1 1h6a1 1 0 0 0 1-1V2a1 1 0 0 0-1-1H6z"
          />
          <path d="M2 4a1 1 0 0 0-1 1v9a1 1 0 0 0 1 1h6a1 1 0 0 0 1-1v-1h-1v1H2V5h1V4H2z" />
        </svg>
      {/if}
    </button>
    {#if editingPath}
      <input
        bind:this={pathInputEl}
        class="path-input"
        bind:value={pathInput}
        onkeydown={handlePathKeydown}
        onblur={cancelPathEdit}
        spellcheck="false"
      />
    {:else}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="breadcrumbs" ondblclick={startEditingPath}>
        <button class="crumb" onclick={() => navigateToPath('/')}>/</button>
        {#each pathSegments() as seg, i}
          <span class="crumb-sep">/</span>
          <button
            class="crumb"
            class:crumb-hover={hoveredPath !== null &&
              (seg.path === hoveredPath || (hoveredIsPrefix && seg.path.startsWith(hoveredPath + '/')))}
            onmouseenter={() => {
              hoveredPath = seg.path;
              hoveredIsPrefix = false;
            }}
            onmouseleave={() => {
              if (hoveredPath === seg.path) hoveredPath = null;
            }}
            onclick={() => navigateToPath(seg.path)}>{seg.name}</button
          >
        {/each}
      </div>
      <button class="edit-path-btn" onclick={startEditingPath} title="Type path directly">
        <svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor">
          <path
            d="M12.146.854a.5.5 0 0 1 .708 0l2.292 2.292a.5.5 0 0 1 0 .708l-9.5 9.5a.5.5 0 0 1-.168.11l-5 2a.5.5 0 0 1-.65-.65l2-5a.5.5 0 0 1 .11-.168l9.5-9.5zM11.207 2.5 13.5 4.793 14.793 3.5 12.5 1.207 11.207 2.5zm1.586 3-2.293-2.293L3 10.707V11h.5a.5.5 0 0 1 .5.5v.5h.5a.5.5 0 0 1 .5.5v.5h.293l7.5-7.5z"
          />
        </svg>
      </button>
    {/if}
  </div>

  <!-- Miller columns -->
  <div class="miller-columns" bind:this={columnsEl}>
    {#if initialLoading}
      <div class="miller-column">
        <div class="col-status">Loading…</div>
      </div>
    {:else}
      {#each columns as col, colIndex}
        {#if colIndex > 0}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="col-handle"
            class:dragging={draggingHandle === colIndex}
            role="separator"
            onpointerdown={(e) => startColResize(e, colIndex)}
            onpointermove={(e) => doColResize(e, colIndex)}
            onpointerup={stopColResize}
            onpointercancel={stopColResize}
            ondblclick={() => resetColWidth(colIndex)}
          ></div>
        {/if}
        <div
          class="miller-column"
          class:last={colIndex === columns.length - 1}
          class:col-hover={hoveredPath !== null &&
            (col.path === hoveredPath || (hoveredIsPrefix && col.path.startsWith(hoveredPath + '/')))}
          style:width="{columnWidths[colIndex] ?? DEFAULT_COL_WIDTH}px"
          style:flex="none"
          onkeydown={(e) => handleColumnKeydown(e, colIndex)}
          onmouseenter={() => {
            hoveredPath = col.path;
            hoveredIsPrefix = false;
          }}
          onmouseleave={() => {
            if (hoveredPath === col.path) hoveredPath = null;
          }}
          tabindex="0"
          role="listbox"
        >
          <div class="col-header">{col.path.split('/').pop() || '/'}</div>
          {#if col.loading}
            <div class="col-status">Loading…</div>
          {:else if col.error}
            <div class="col-status col-error">{col.error}</div>
          {:else}
            {#if col.entries.length === 0}
              <div class="col-status col-empty">No subdirectories</div>
            {:else}
              {#each col.entries as entry}
                <button
                  class="entry"
                  class:selected={col.selectedName === entry.name}
                  class:leaf={!entry.hasChildren}
                  class:entry-hover={hoveredPath !== null && entry.path === hoveredPath}
                  onclick={() => selectEntry(colIndex, entry)}
                  onmouseenter={() => {
                    hoveredPath = entry.path;
                    hoveredIsPrefix = false;
                  }}
                  onmouseleave={() => {
                    if (hoveredPath === entry.path) {
                      hoveredPath = col.path;
                      hoveredIsPrefix = false;
                    }
                  }}
                  role="option"
                  aria-selected={col.selectedName === entry.name}
                >
                  <span class="entry-name">{entry.name}</span>
                  {#if entry.hasChildren}
                    <span class="entry-arrow">&#9654;</span>
                  {/if}
                </button>
              {/each}
            {/if}
            <div class="new-dir-action">
              {#if showNewDirInput === colIndex}
                <div class="new-dir-form">
                  <input
                    bind:this={newDirInputEl}
                    class="new-dir-input"
                    bind:value={newDirName}
                    onkeydown={(e) => handleNewDirKeydown(e, colIndex)}
                    onblur={cancelNewDir}
                    placeholder="folder name"
                    spellcheck="false"
                    disabled={creatingDir}
                  />
                </div>
              {:else}
                <button class="new-dir-btn" onclick={() => startNewDir(colIndex)}> + NEW FOLDER </button>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
    {/if}
  </div>

  <!-- Git context panel -->
  {#if gitInfo()}
    {@const git = gitInfo()!}
    <div class="git-panel">
      <div class="git-header">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor" style="opacity: 0.7">
          <path
            d="M15.698 7.287 8.712.302a1.03 1.03 0 0 0-1.457 0l-1.45 1.45 1.84 1.84a1.223 1.223 0 0 1 1.55 1.56l1.773 1.774a1.224 1.224 0 1 1-.733.693L8.535 5.92v4.738a1.224 1.224 0 1 1-1.007-.019V5.86a1.224 1.224 0 0 1-.664-1.605L5.04 2.43.302 7.168a1.03 1.03 0 0 0 0 1.457l6.986 6.986a1.03 1.03 0 0 0 1.457 0l6.953-6.953a1.03 1.03 0 0 0 0-1.37z"
          />
        </svg>
        <span class="git-branch">{git.currentBranch}</span>
        <button class="git-info-btn" class:active={showGitDetail} onclick={toggleGitDetail} title="Repository details">
          <svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor">
            <path
              d="M8 1a7 7 0 1 0 0 14A7 7 0 0 0 8 1zm0 12.5a5.5 5.5 0 1 1 0-11 5.5 5.5 0 0 1 0 11zM8 6a.75.75 0 0 1 .75.75v4.5a.75.75 0 0 1-1.5 0v-4.5A.75.75 0 0 1 8 6zm0-2.5a.875.875 0 1 1 0 1.75.875.875 0 0 1 0-1.75z"
            />
          </svg>
        </button>
        <span class="git-repo-name">{git.repoRoot.split('/').pop()}</span>
      </div>

      {#if showGitDetail}
        <div class="git-detail">
          {#if gitDetailLoading}
            <div class="git-detail-row"><span class="git-detail-value">Loading…</span></div>
          {:else if gitDetail}
            {#each gitDetail.remotes as remote}
              <div class="git-detail-row">
                <span class="git-detail-label">{remote.name.toUpperCase()}</span>
                <span class="git-detail-value">{remote.url}</span>
              </div>
            {/each}
            <div class="git-detail-row">
              <span class="git-detail-label">HEAD</span>
              <span class="git-detail-value">
                <span class="git-detail-sha">{gitDetail.headSha.slice(0, 7)}</span>
                {gitDetail.lastCommitSubject}
                {#if gitDetail.lastCommitDate}
                  <span class="git-detail-date">({gitDetail.lastCommitDate.split(' ').slice(0, 1).join('')})</span>
                {/if}
              </span>
            </div>
            {#if gitDetail.upstream}
              <div class="git-detail-row">
                <span class="git-detail-label">UPSTREAM</span>
                <span class="git-detail-value">
                  {gitDetail.upstream.name}
                  <span class="git-detail-counts"
                    >{@html `\u2191${gitDetail.upstream.ahead} \u2193${gitDetail.upstream.behind}`}</span
                  >
                </span>
              </div>
            {/if}
            <div class="git-detail-row">
              <span class="git-detail-label">CHANGES</span>
              <span class="git-detail-value">
                {#if gitDetail.changes.modified === 0 && gitDetail.changes.staged === 0 && gitDetail.changes.untracked === 0}
                  clean
                {:else}
                  {[
                    gitDetail.changes.modified > 0 ? `${gitDetail.changes.modified} modified` : '',
                    gitDetail.changes.staged > 0 ? `${gitDetail.changes.staged} staged` : '',
                    gitDetail.changes.untracked > 0 ? `${gitDetail.changes.untracked} untracked` : ''
                  ]
                    .filter(Boolean)
                    .join(' \u00b7 ')}
                {/if}
              </span>
            </div>
            {#if gitDetail.stashCount > 0}
              <div class="git-detail-row">
                <span class="git-detail-label">STASHES</span>
                <span class="git-detail-value">{gitDetail.stashCount}</span>
              </div>
            {/if}
            {#if gitDetail.recentBranches.length > 0}
              <div class="git-detail-row">
                <span class="git-detail-label">BRANCHES</span>
                <span class="git-detail-value">{gitDetail.recentBranches.join(', ')}</span>
              </div>
            {/if}
          {/if}
        </div>
      {/if}

      {#if git.worktrees.length > 1}
        <div class="worktree-section">
          <span class="section-label">WORKTREES</span>
          {#each git.worktrees as wt}
            <button
              class="worktree-entry"
              class:active={value === wt.path}
              class:wt-hover={hoveredPath !== null &&
                (hoveredPath === wt.path || hoveredPath.startsWith(wt.path + '/'))}
              onclick={() => expandToPath(wt.path)}
              onmouseenter={() => {
                hoveredPath = wt.path;
                hoveredIsPrefix = true;
              }}
              onmouseleave={() => {
                if (hoveredPath === wt.path) hoveredPath = null;
              }}
            >
              <span class="wt-branch">{wt.branch || '(detached)'}</span>
              {#if wt.isMain}
                <span class="wt-badge">main</span>
              {/if}
              <span class="wt-path">{wt.path.split('/').pop()}</span>
            </button>
          {/each}
        </div>
      {/if}

      <div class="worktree-new">
        {#if showWorktreeForm}
          <div class="wt-form">
            <label class="wt-field">
              <span class="wt-label">BRANCH</span>
              <input
                class="wt-input"
                bind:value={worktreeBranch}
                oninput={() => {
                  worktreePath = suggestWorktreePath(worktreeBranch);
                }}
                placeholder="existing or new branch name"
                spellcheck="false"
                autocomplete="off"
                list="wt-branch-list"
              />
              <datalist id="wt-branch-list">
                {#each git.localBranches as branch}
                  <option value={branch}></option>
                {/each}
              </datalist>
            </label>
            <label class="wt-field">
              <span class="wt-label">PATH</span>
              <input class="wt-input" bind:value={worktreePath} placeholder="/path/to/worktree" spellcheck="false" />
            </label>
            <div class="wt-actions">
              <button
                class="wt-cancel"
                onclick={() => {
                  showWorktreeForm = false;
                }}
                disabled={creatingWorktree}
              >
                CANCEL
              </button>
              <button
                class="wt-create"
                onclick={handleCreateWorktree}
                disabled={!worktreeBranch || !worktreePath || creatingWorktree}
              >
                {creatingWorktree ? 'CREATING...' : 'CREATE'}
              </button>
            </div>
          </div>
        {:else}
          <button
            class="wt-add-btn"
            onclick={() => {
              showWorktreeForm = true;
            }}
          >
            + NEW WORKTREE
          </button>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .directory-picker {
    display: flex;
    flex-direction: column;
    gap: 0;
    border: 1px solid var(--surface-border);
    background: var(--surface-800);
    font-size: 0.72rem;
  }

  /* Path bar */
  .path-bar {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 6px 8px;
    border-bottom: 1px solid var(--surface-border);
    background: var(--surface-700);
    min-height: 28px;
    flex-shrink: 0;
  }

  .breadcrumbs {
    display: flex;
    align-items: center;
    gap: 1px;
    flex: 1;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .breadcrumbs::-webkit-scrollbar {
    display: none;
  }

  .crumb {
    font-family: inherit;
    font-size: 0.68rem;
    padding: 1px 4px;
    background: none;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    white-space: nowrap;
    border-radius: 2px;
  }

  .crumb:hover,
  .crumb.crumb-hover {
    color: var(--text-primary);
    background: var(--surface-600);
  }

  .crumb-sep {
    color: var(--text-muted);
    font-size: 0.65rem;
    opacity: 0.5;
  }

  .edit-path-btn {
    display: flex;
    align-items: center;
    padding: 3px;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    opacity: 0.6;
    flex-shrink: 0;
  }

  .edit-path-btn:hover {
    opacity: 1;
    color: var(--text-secondary);
  }

  .copy-path-btn {
    display: flex;
    align-items: center;
    padding: 3px;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    opacity: 0.6;
    flex-shrink: 0;
    transition: all 0.15s ease;
  }

  .copy-path-btn:hover {
    opacity: 1;
    color: var(--text-secondary);
  }
  .copy-path-btn.copied {
    opacity: 1;
    color: var(--chrome-accent-400);
  }

  .path-input {
    flex: 1;
    font-family: inherit;
    font-size: 0.68rem;
    padding: 2px 4px;
    background: var(--surface-800);
    border: 1px solid var(--chrome-accent-600);
    color: var(--text-primary);
    outline: none;
  }

  /* Quick links */
  .quick-links {
    padding: 6px 8px;
    border-bottom: 1px solid var(--surface-border);
    flex-shrink: 0;
  }

  .section-label {
    font-size: 0.58rem;
    font-weight: 600;
    letter-spacing: 0.1em;
    color: var(--text-muted);
    display: block;
    margin-bottom: 4px;
  }

  .project-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .project-chip {
    font-family: inherit;
    font-size: 0.62rem;
    padding: 2px 8px;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    color: var(--text-secondary);
    cursor: pointer;
    white-space: nowrap;
    transition: all 0.1s;
  }

  .project-chip:hover {
    border-color: var(--surface-border-light);
    color: var(--text-primary);
  }

  .project-chip.active {
    border-color: var(--chrome-accent-600);
    color: var(--chrome-accent-400);
  }

  .project-chip.chip-hover {
    border-color: var(--surface-border-light);
    color: var(--text-primary);
    background: var(--surface-600);
  }

  /* Miller columns */
  .miller-columns {
    display: flex;
    flex: 1 1 0;
    min-height: 120px;
    overflow-x: auto;
    overflow-y: hidden;
    scroll-behavior: smooth;
  }

  .col-handle {
    width: 5px;
    flex-shrink: 0;
    cursor: col-resize;
    background: var(--surface-border);
    position: relative;
    transition: background 0.1s;
  }

  .col-handle::before {
    content: '';
    position: absolute;
    inset: 0 -3px;
  }

  .col-handle:hover,
  .col-handle.dragging {
    background: var(--chrome-accent-600);
  }

  .miller-column {
    overflow-y: auto;
    outline: none;
  }

  .col-header {
    position: sticky;
    top: 0;
    z-index: 1;
    padding: 2px 8px;
    font-size: 0.55rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    color: var(--text-muted);
    background: var(--surface-700);
    border-bottom: 1px solid var(--surface-border);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .miller-column:focus-within,
  .miller-column.col-hover {
    background: var(--surface-800);
  }

  .miller-column.col-hover .col-header {
    color: var(--text-primary);
    background: var(--surface-600);
  }

  .col-status {
    padding: 12px 8px;
    text-align: center;
    color: var(--text-muted);
    font-size: 0.65rem;
  }

  .col-error {
    color: var(--status-red);
  }

  .col-empty {
    font-style: italic;
  }

  .entry {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    padding: 3px 8px;
    background: none;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.68rem;
    text-align: left;
    transition: background 0.05s;
  }

  .entry:hover,
  .entry.entry-hover {
    background: var(--surface-700);
    color: var(--text-primary);
  }

  .entry.selected {
    background: var(--surface-600);
    color: var(--text-primary);
  }

  .entry.leaf {
    opacity: 0.5;
  }

  .entry.leaf:hover {
    opacity: 0.75;
  }

  .entry-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  .entry-arrow {
    font-size: 0.5rem;
    color: var(--text-muted);
    flex-shrink: 0;
    opacity: 0.5;
  }

  /* New directory */
  .new-dir-action {
    margin-top: 2px;
    padding: 2px 4px 4px;
    border-top: 1px solid var(--surface-border);
  }

  .new-dir-btn {
    font-family: inherit;
    font-size: 0.58rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    padding: 2px 4px;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    width: 100%;
    text-align: left;
    opacity: 0.6;
    transition: opacity 0.1s;
  }

  .new-dir-btn:hover {
    opacity: 1;
    color: var(--text-secondary);
  }

  .new-dir-form {
    display: flex;
  }

  .new-dir-input {
    flex: 1;
    font-family: inherit;
    font-size: 0.65rem;
    padding: 2px 4px;
    background: var(--surface-800);
    border: 1px solid var(--chrome-accent-600);
    color: var(--text-primary);
    outline: none;
    width: 100%;
  }

  /* Git panel */
  .git-panel {
    border-top: 1px solid var(--surface-border);
    border-left: 2px solid var(--chrome-accent-600);
    background: var(--surface-800);
    flex-shrink: 0;
  }

  .git-header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    border-bottom: 1px solid var(--surface-border);
  }

  .git-branch {
    font-weight: 600;
    color: var(--chrome-accent-400);
    font-size: 0.68rem;
  }

  .git-repo-name {
    font-size: 0.62rem;
    color: var(--text-muted);
    margin-left: auto;
  }

  .git-info-btn {
    display: flex;
    align-items: center;
    padding: 3px;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    opacity: 0.6;
    flex-shrink: 0;
  }

  .git-info-btn:hover,
  .git-info-btn.active {
    opacity: 1;
    color: var(--text-secondary);
  }

  .git-detail {
    padding: 6px 8px;
    border-top: 1px solid var(--surface-border);
    font-size: 0.62rem;
  }

  .git-detail-row {
    display: flex;
    gap: 8px;
    padding: 2px 0;
  }

  .git-detail-label {
    font-size: 0.55rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    color: var(--text-muted);
    min-width: 64px;
    flex-shrink: 0;
  }

  .git-detail-value {
    color: var(--text-secondary);
    word-break: break-all;
  }

  .git-detail-sha {
    color: var(--chrome-accent-400);
    margin-right: 4px;
  }

  .git-detail-date {
    color: var(--text-muted);
    margin-left: 4px;
  }

  .git-detail-counts {
    margin-left: 6px;
    color: var(--text-muted);
  }

  .worktree-section {
    padding: 6px 8px;
  }

  .worktree-entry {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 3px 6px;
    background: none;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.65rem;
    text-align: left;
    transition: background 0.05s;
  }

  .worktree-entry:hover {
    background: var(--surface-700);
    color: var(--text-primary);
  }

  .worktree-entry.active {
    color: var(--chrome-accent-400);
  }

  .worktree-entry.wt-hover {
    background: var(--surface-700);
    color: var(--text-primary);
  }

  .wt-branch {
    font-weight: 600;
  }

  .wt-badge {
    font-size: 0.55rem;
    padding: 0 4px;
    border: 1px solid var(--surface-border);
    color: var(--text-muted);
    letter-spacing: 0.05em;
  }

  .wt-path {
    margin-left: auto;
    color: var(--text-muted);
    font-size: 0.6rem;
  }

  /* New worktree */
  .worktree-new {
    padding: 4px 8px 6px;
    border-top: 1px solid var(--surface-border);
  }

  .wt-add-btn {
    font-family: inherit;
    font-size: 0.6rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    padding: 3px 8px;
    background: none;
    border: 1px dashed var(--surface-border);
    color: var(--text-muted);
    cursor: pointer;
    width: 100%;
    transition: all 0.1s;
  }

  .wt-add-btn:hover {
    border-color: var(--surface-border-light);
    color: var(--text-secondary);
  }

  .wt-form {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .wt-field {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .wt-label {
    font-size: 0.55rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    color: var(--text-muted);
  }

  .wt-input {
    font-family: inherit;
    font-size: 0.65rem;
    padding: 3px 6px;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    color: var(--text-primary);
    outline: none;
  }

  .wt-input:focus {
    border-color: var(--chrome-accent-600);
  }

  .wt-actions {
    display: flex;
    gap: 4px;
    margin-top: 2px;
  }

  .wt-cancel,
  .wt-create {
    flex: 1;
    font-family: inherit;
    font-size: 0.6rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    padding: 3px 8px;
    border: 1px solid var(--surface-border);
    cursor: pointer;
  }

  .wt-cancel {
    background: var(--surface-700);
    color: var(--text-secondary);
  }

  .wt-cancel:hover:not(:disabled) {
    background: var(--surface-600);
  }

  .wt-create {
    background: var(--chrome-accent-600);
    border-color: var(--chrome-accent-600);
    color: var(--surface-900);
  }

  .wt-create:hover:not(:disabled) {
    filter: brightness(1.1);
  }

  .wt-create:disabled,
  .wt-cancel:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
