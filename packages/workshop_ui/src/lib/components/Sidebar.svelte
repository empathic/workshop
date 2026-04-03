<script lang="ts">
  import { projects, currentProject, reorderProjects } from '$lib/stores/projects';
  import { isGapValid } from '$lib/utils/project-order';
  import { switchProject } from '$lib/stores/layout';
  import { currentUser, isAuthenticated } from '$lib/stores/auth';
  import { fullscreenView, openFullscreen, closeFullscreen } from '$lib/stores/fullscreen';
  import CloseProjectModal from './CloseProjectModal.svelte';
  import type { Project } from '$lib/stores/projects';

  let closeProjectTarget = $state<Project | null>(null);

  // Drag-and-drop state — tracks insertion gap, not hovered item.
  // For N items there are N+1 gaps (0=before first, N=after last).
  let dragId = $state<string | null>(null);
  let dropGap = $state<number | null>(null);

  function handleDragStart(e: DragEvent, projectId: string) {
    dragId = projectId;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = 'move';
    }
  }

  function handleDragOver(e: DragEvent, itemIndex: number) {
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const gap = e.clientY < rect.top + rect.height / 2 ? itemIndex : itemIndex + 1;
    const dragIdx = $projects.findIndex((p) => p.id === dragId);
    dropGap = isGapValid(dragIdx, gap) ? gap : null;
  }

  function handleDrop(e: DragEvent) {
    e.preventDefault();
    if (dragId && dropGap !== null) {
      reorderProjects(dragId, dropGap);
    }
    dragId = null;
    dropGap = null;
  }

  function handleDragEnd() {
    dragId = null;
    dropGap = null;
  }

  function handleSelectProject(workingDir: string) {
    const project = $projects.find((p) => p.workingDir === workingDir);
    if (project && project.instances.length > 0) {
      switchProject(workingDir, project.instances[0].id);
    }
  }

  /** Get 2-letter abbreviation for a project name */
  function getProjectAbbr(name: string): string {
    const words = name
      .replace(/[^a-zA-Z0-9\s]/g, '')
      .split(/[\s_-]+/)
      .filter(Boolean);
    if (words.length >= 2) {
      return (words[0][0] + words[1][0]).toUpperCase();
    }
    return name.slice(0, 2).toUpperCase();
  }

  /** Color by index for project icons */
  const projectColors = ['var(--chrome-accent-500)', 'var(--thinking-400)', 'var(--status-green)', 'var(--status-red)'];
</script>

<aside class="sidebar-rail">
  <!-- Project icons -->
  <nav class="rail-projects">
    {#each $projects as project, i (project.id)}
      {@const isActive = $currentProject?.id === project.id}
      <div
        class="rail-project-slot"
        class:active={isActive}
        class:drag-over={dropGap === i}
        class:dragging={dragId === project.id}
        role="listitem"
        draggable="true"
        ondragstart={(e) => handleDragStart(e, project.id)}
        ondragover={(e) => handleDragOver(e, i)}
        ondrop={handleDrop}
        ondragend={handleDragEnd}
      >
        <button
          class="rail-project"
          class:active={isActive}
          onclick={() => handleSelectProject(project.workingDir)}
          title="{project.name} ({project.instances.length} instances)"
          aria-label="{project.name} project"
          style="--project-color: {projectColors[i % projectColors.length]}"
        >
          <span class="project-abbr">{getProjectAbbr(project.name)}</span>
        </button>
        {#if isActive}
          <button
            class="rail-action"
            onclick={() => {
              closeProjectTarget = project;
            }}
            title="Close project"
            aria-label="Close {project.name}">&times;</button
          >
        {/if}
      </div>
    {/each}
    <!-- Drop zone fills remaining nav space for "move to end" -->
    <div
      class="rail-drop-end"
      class:drag-over={dropGap === $projects.length}
      role="listitem"
      ondragover={(e) => {
        e.preventDefault();
        if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
        if (!dragId) return;
        const dragIdx = $projects.findIndex((p) => p.id === dragId);
        dropGap = isGapValid(dragIdx, $projects.length) ? $projects.length : null;
      }}
      ondrop={handleDrop}
    ></div>
  </nav>

  <!-- Bottom actions -->
  <div class="rail-bottom">
    <button
      class="rail-btn"
      onclick={() => ($fullscreenView === 'new-project' ? closeFullscreen() : openFullscreen('new-project'))}
      title="New project"
      aria-label="Create new project"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <line x1="12" y1="5" x2="12" y2="19" />
        <line x1="5" y1="12" x2="19" y2="12" />
      </svg>
    </button>

    <button
      class="rail-btn"
      onclick={() => ($fullscreenView === 'history' ? closeFullscreen() : openFullscreen('history'))}
      title="History"
      aria-label="Conversation history"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M6 2L6 6M18 2L18 6M6 18L6 22M18 18L18 22" />
        <path d="M6 6C6 6 4 6 4 8V10C4 12 6 12 6 12H18C18 12 20 12 20 10V8C20 6 18 6 18 6" />
        <path d="M6 12C6 12 4 12 4 14V16C4 18 6 18 6 18H18C18 18 20 18 20 16V14C20 12 18 12 18 12" />
      </svg>
    </button>

    <button
      class="rail-btn"
      onclick={() => ($fullscreenView === 'settings' ? closeFullscreen() : openFullscreen('settings'))}
      title="Settings"
      aria-label="Settings"
    >
      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5">
        <circle cx="8" cy="8" r="2.5" />
        <path
          d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.1 3.1l1.4 1.4M11.5 11.5l1.4 1.4M3.1 12.9l1.4-1.4M11.5 4.5l1.4-1.4"
        />
      </svg>
    </button>

    {#if $isAuthenticated && $currentUser}
      <button
        class="rail-btn user-btn"
        title="{$currentUser.display_name} — Account"
        onclick={() => ($fullscreenView === 'settings' ? closeFullscreen() : openFullscreen('settings'))}
        aria-label="User: {$currentUser.display_name}"
      >
        <span class="user-initial">{$currentUser.display_name.charAt(0).toUpperCase()}</span>
      </button>
    {/if}
  </div>
</aside>

{#if closeProjectTarget}
  <CloseProjectModal project={closeProjectTarget} onclose={() => (closeProjectTarget = null)} />
{/if}

<style>
  .sidebar-rail {
    display: flex;
    flex-direction: column;
    width: 48px;
    background: linear-gradient(180deg, var(--surface-700) 0%, var(--surface-800) 100%);
    border-right: 1px solid var(--surface-border);
    height: 100%;
    flex-shrink: 0;
    align-items: center;
    padding: 8px 0;
  }

  .sidebar-rail::after {
    content: '';
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    width: 1px;
    background: linear-gradient(180deg, transparent 0%, var(--tint-active) 50%, transparent 100%);
  }

  .rail-projects {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    flex: 1;
    overflow-y: auto;
    padding: 4px 0;
  }

  .rail-projects::-webkit-scrollbar {
    width: 0;
  }

  .rail-project-slot {
    display: flex;
    flex-direction: column;
    align-items: center;
    flex-shrink: 0;
    border-radius: 50%;
    transition: all 0.15s ease;
  }

  .rail-project-slot.dragging {
    opacity: 0.3;
  }

  .rail-project-slot.drag-over {
    position: relative;
  }

  .rail-project-slot.drag-over::before {
    content: '';
    position: absolute;
    top: -4px;
    left: 4px;
    right: 4px;
    height: 2px;
    background: var(--chrome-accent-500);
    border-radius: 1px;
  }

  .rail-drop-end {
    width: 100%;
    flex: 1;
    min-height: 6px;
  }

  .rail-drop-end.drag-over {
    position: relative;
  }

  .rail-drop-end.drag-over::before {
    content: '';
    position: absolute;
    top: -4px;
    left: 4px;
    right: 4px;
    height: 2px;
    background: var(--chrome-accent-500);
    border-radius: 1px;
  }

  .rail-project-slot.active {
    background: var(--tint-active);
    border: 1px solid var(--chrome-accent-500);
    border-radius: 10px;
    padding: 3px 3px 2px;
  }

  .rail-project-slot.active .rail-project {
    width: 28px;
    height: 28px;
    border: none;
    background: transparent;
  }

  .rail-project-slot.active .rail-project:hover {
    background: var(--tint-hover);
  }

  .rail-project-slot.active .project-abbr {
    color: var(--chrome-accent-400);
  }

  .rail-action {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    height: 16px;
    background: transparent;
    border: none;
    border-top: 1px solid var(--chrome-accent-700);
    color: var(--text-muted);
    font-size: 12px;
    line-height: 1;
    cursor: pointer;
    padding: 0;
    transition: color 0.15s ease;
    border-radius: 0 0 8px 8px;
    margin-top: 1px;
  }

  .rail-action:hover {
    color: var(--status-red);
  }

  .rail-project {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border-radius: 50%;
    background: var(--surface-600);
    border: 2px solid transparent;
    cursor: pointer;
    transition: all 0.15s ease;
    flex-shrink: 0;
  }

  .rail-project:hover {
    background: var(--surface-500);
    border-color: var(--surface-border-light);
  }

  .project-abbr {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.05em;
    color: var(--project-color, var(--text-secondary));
  }

  .rail-bottom {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding-top: 8px;
    border-top: 1px solid var(--surface-border);
  }

  .rail-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 0.15s ease;
    flex-shrink: 0;
    padding: 0;
  }

  .rail-btn:hover {
    background: var(--tint-hover);
    border-color: var(--surface-border);
    color: var(--text-secondary);
  }

  .rail-btn svg {
    width: 14px;
    height: 14px;
  }

  .user-btn {
    border-radius: 50%;
    width: 28px;
    height: 28px;
  }

  .user-initial {
    font-size: 11px;
    font-weight: 700;
    color: var(--text-secondary);
  }

  /* Analog theme */
  :global([data-theme='analog']) .sidebar-rail {
    background-color: var(--surface-800);
    background-image: var(--grain-fine), var(--grain-coarse);
    background-blend-mode: multiply, multiply;
  }

  :global([data-theme='analog']) .sidebar-rail::after {
    background: var(--chrome-accent-600);
    width: 1.5px;
  }
</style>
