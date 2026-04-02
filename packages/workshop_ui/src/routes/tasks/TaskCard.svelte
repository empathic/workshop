<script lang="ts">
  import { goto } from '$app/navigation';
  import { base } from '$app/paths';
  import { updateTask, deleteTask, stageTask } from '$lib/stores/tasks';
  import { selectInstance } from '$lib/stores/instances';
  import type { Task, Instance } from '$lib/types';

  interface Props {
    task: Task;
    instanceList: Instance[];
  }

  let { task, instanceList }: Props = $props();

  let editing = $state(false);
  let editTitle = $state('');
  let editBody = $state('');
  let sentTextExpanded = $state(false);

  function startEdit() {
    editing = true;
    editTitle = task.title;
    editBody = task.body ?? '';
  }

  async function commitEdit() {
    if (!editing) return;
    await updateTask(task.id, {
      title: editTitle.trim(),
      body: editBody.trim() || undefined
    });
    editing = false;
  }

  function handleEditKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      commitEdit();
    } else if (e.key === 'Escape') {
      editing = false;
    }
  }

  function getInstanceName(instanceId: string | null): string {
    if (!instanceId) return 'Unassigned';
    const inst = instanceList.find((i) => i.id === instanceId);
    return inst?.custom_name ?? inst?.name ?? instanceId.slice(0, 8);
  }

  function formatTime(ts: number): string {
    const d = new Date(ts * 1000);
    const now = new Date();
    const diffMs = now.getTime() - d.getTime();
    const diffMins = Math.floor(diffMs / 60000);
    if (diffMins < 1) return 'just now';
    if (diffMins < 60) return `${diffMins}m ago`;
    const diffHours = Math.floor(diffMins / 60);
    if (diffHours < 24) return `${diffHours}h ago`;
    const diffDays = Math.floor(diffHours / 24);
    if (diffDays < 7) return `${diffDays}d ago`;
    return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
  }

  function priorityLabel(p: number): string {
    switch (p) {
      case 3:
        return 'high';
      case 2:
        return 'med';
      case 1:
        return 'low';
      default:
        return '';
    }
  }

  function statusIcon(status: string): string {
    switch (status) {
      case 'pending':
        return '\u25CB';
      case 'in_progress':
        return '\u25B6';
      case 'completed':
        return '\u2713';
      case 'cancelled':
        return '\u2715';
      default:
        return '\u25CB';
    }
  }

  function handleSend() {
    stageTask(task.id);
    goto(`${base}/`);
  }

  function goToInstance() {
    if (task.instance_id) {
      selectInstance(task.instance_id);
      goto(`${base}/`);
    }
  }
</script>

<div
  class="task-card"
  class:editing
  class:priority-high={task.priority === 3}
  class:priority-med={task.priority === 2}
  class:status-in-progress={task.status === 'in_progress'}
  class:status-completed={task.status === 'completed'}
  class:status-cancelled={task.status === 'cancelled'}
>
  <button
    class="status-icon"
    class:in-progress={task.status === 'in_progress'}
    class:completed={task.status === 'completed'}
    class:cancelled={task.status === 'cancelled'}
    onclick={() => {
      if (task.status === 'completed') updateTask(task.id, { status: 'pending' });
    }}
    title={task.status === 'completed' ? 'Reopen' : task.status}
  >
    {statusIcon(task.status)}
  </button>

  <div class="task-content">
    {#if editing}
      <!-- svelte-ignore a11y_autofocus -->
      <input
        type="text"
        class="edit-title-input"
        bind:value={editTitle}
        onkeydown={handleEditKeydown}
        onblur={commitEdit}
        autofocus
      />
      <textarea class="edit-body-input" bind:value={editBody} rows="2" placeholder="Body (optional)..."></textarea>
    {:else}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div class="task-title" ondblclick={startEdit}>
        {task.title}
      </div>
      {#if task.body}
        <div class="task-body">{task.body}</div>
      {/if}
    {/if}

    {#if task.status === 'in_progress' && task.dispatches?.length}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="sent-text-toggle" onclick={() => (sentTextExpanded = !sentTextExpanded)}>
        <span class="sent-text-label"
          >Dispatches ({task.dispatches.length}) {sentTextExpanded ? '\u25B4' : '\u25BE'}</span
        >
      </div>
      {#if sentTextExpanded}
        {#each task.dispatches as dispatch}
          <pre class="sent-text-body">{dispatch.sent_text}</pre>
        {/each}
      {/if}
    {/if}

    <div class="task-meta">
      {#if task.priority > 0}
        <span
          class="priority-pill"
          class:high={task.priority === 3}
          class:med={task.priority === 2}
          class:low={task.priority === 1}
        >
          {priorityLabel(task.priority)}
        </span>
      {/if}
      {#if task.instance_id && task.status === 'in_progress'}
        <button
          class="instance-badge instance-link"
          title="Go to {getInstanceName(task.instance_id)}"
          onclick={goToInstance}
        >
          {getInstanceName(task.instance_id)}
        </button>
      {:else if task.instance_id}
        <span class="instance-badge" title={task.instance_id}>
          {getInstanceName(task.instance_id)}
        </span>
      {:else}
        <span class="instance-badge unassigned">unassigned</span>
      {/if}
      {#each task.tags as tag}
        <span
          class="tag-pill"
          style:background={tag.color ? `${tag.color}20` : undefined}
          style:border-color={tag.color ?? undefined}
          style:color={tag.color ?? undefined}
        >
          {tag.name}
        </span>
      {/each}
      <span class="task-time">{formatTime(task.created_at)}</span>
    </div>
  </div>

  <div class="task-actions">
    {#if (task.status === 'pending' || task.status === 'in_progress') && task.instance_id}
      <button class="action-btn send-btn" onclick={handleSend} title="Send to instance">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M5 12h14M12 5l7 7-7 7" />
        </svg>
      </button>
    {/if}
    {#if task.status === 'pending' || task.status === 'in_progress'}
      <button
        class="action-btn complete-btn"
        onclick={() => updateTask(task.id, { status: 'completed' })}
        title="Mark complete"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M20 6L9 17l-5-5" />
        </svg>
      </button>
    {/if}
    {#if task.status === 'pending'}
      <select
        class="assign-select"
        value={task.instance_id ?? ''}
        onchange={(e) => updateTask(task.id, { instance_id: (e.target as HTMLSelectElement).value || undefined })}
        title="Assign to instance"
      >
        <option value="">none</option>
        {#each instanceList as inst}
          <option value={inst.id}>{inst.custom_name ?? inst.name}</option>
        {/each}
      </select>
    {/if}
    <button class="action-btn delete-btn" onclick={() => deleteTask(task.id)} title="Delete task">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path
          d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"
        />
      </svg>
    </button>
  </div>
</div>

<style>
  .task-card {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 12px 14px;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    transition: all 0.15s ease;
  }

  .task-card:hover {
    background: linear-gradient(180deg, var(--tint-hover) 0%, transparent 100%);
    border-color: var(--surface-border-light);
  }

  .task-card.priority-high {
    border-left: 3px solid var(--status-red);
  }

  .task-card.priority-med {
    border-left: 3px solid var(--status-yellow);
  }

  .task-card.status-in-progress {
    opacity: 0.7;
  }

  .task-card.status-completed {
    opacity: 0.5;
  }

  .task-card.status-cancelled {
    opacity: 0.4;
  }

  .status-icon {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    margin-top: 1px;
    background: none;
    border: 1px solid var(--surface-border);
    border-radius: 50%;
    color: var(--text-muted);
    font-size: 12px;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .status-icon:hover {
    border-color: var(--accent-600);
    color: var(--accent-400);
  }

  .status-icon.in-progress {
    border-color: var(--status-blue);
    color: var(--status-blue);
  }

  .status-icon.completed {
    border-color: var(--status-green);
    color: var(--status-green);
    background: var(--status-green-tint);
  }

  .status-icon.cancelled {
    border-color: var(--status-red);
    color: var(--status-red);
  }

  .task-content {
    flex: 1;
    min-width: 0;
  }

  .task-title {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    line-height: 1.4;
    cursor: default;
  }

  .task-card.status-completed .task-title,
  .task-card.status-cancelled .task-title {
    text-decoration: line-through;
    text-decoration-color: var(--text-muted);
  }

  .task-body {
    margin-top: 4px;
    font-size: 11px;
    color: var(--text-secondary);
    font-family: var(--font-mono);
    line-height: 1.5;
    white-space: pre-wrap;
    max-height: 60px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .task-meta {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 6px;
    flex-wrap: wrap;
  }

  .priority-pill {
    padding: 1px 6px;
    border-radius: 3px;
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  .priority-pill.high {
    background: var(--status-red-tint);
    border: 1px solid var(--status-red-border);
    color: var(--status-red-text);
  }

  .priority-pill.med {
    background: var(--tint-active);
    border: 1px solid var(--status-yellow-border);
    color: var(--status-yellow);
  }

  .priority-pill.low {
    background: var(--status-green-tint);
    border: 1px solid var(--status-green-border);
    color: var(--status-green-text);
  }

  .instance-badge {
    padding: 1px 6px;
    background: var(--tint-active);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    font-size: 10px;
    font-weight: 600;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 120px;
  }

  .instance-badge.unassigned {
    border-style: dashed;
    color: var(--text-muted);
    font-style: italic;
  }

  .tag-pill {
    padding: 1px 6px;
    background: var(--tint-subtle);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    font-size: 9px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .task-time {
    font-size: 10px;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
    margin-left: auto;
  }

  .sent-text-toggle {
    margin-top: 4px;
    cursor: pointer;
    user-select: none;
  }

  .sent-text-label {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--text-muted);
    transition: color 0.15s ease;
  }

  .sent-text-toggle:hover .sent-text-label {
    color: var(--text-secondary);
  }

  .sent-text-body {
    margin: 4px 0 0;
    padding: 6px 8px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    font-size: 11px;
    font-family: var(--font-mono);
    color: var(--text-secondary);
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 120px;
    overflow-y: auto;
  }

  .instance-link {
    cursor: pointer;
    border-color: var(--accent-600);
    color: var(--accent-400);
    font-family: inherit;
    font-size: 10px;
    font-weight: 600;
    transition: all 0.15s ease;
  }

  .instance-link:hover {
    background: var(--tint-active-strong);
    color: var(--accent-300);
  }

  .edit-title-input,
  .edit-body-input {
    width: 100%;
    padding: 4px 8px;
    background: var(--surface-800);
    border: 1px solid var(--accent-600);
    border-radius: 3px;
    color: var(--text-primary);
    font-size: 13px;
    font-family: inherit;
    outline: none;
  }

  .edit-body-input {
    margin-top: 4px;
    font-size: 11px;
    font-family: var(--font-mono);
    resize: vertical;
    min-height: 36px;
  }

  .task-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
    opacity: 0;
    transition: opacity 0.15s ease;
  }

  .task-card:hover .task-actions {
    opacity: 1;
  }

  .action-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: none;
    border: 1px solid transparent;
    border-radius: 3px;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .action-btn svg {
    width: 14px;
    height: 14px;
  }

  .action-btn.send-btn:hover {
    border-color: var(--accent-600);
    color: var(--accent-400);
    background: var(--tint-active);
  }

  .action-btn.complete-btn:hover {
    border-color: var(--status-green);
    color: var(--status-green);
    background: var(--status-green-tint);
  }

  .action-btn.delete-btn:hover {
    border-color: var(--status-red-border);
    color: var(--status-red);
    background: var(--status-red-tint);
  }

  .assign-select {
    padding: 3px 6px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    color: var(--text-secondary);
    font-size: 10px;
    font-family: inherit;
    outline: none;
    cursor: pointer;
    max-width: 100px;
  }

  @media (max-width: 639px) {
    .task-card {
      padding: 10px 12px;
    }

    .task-actions {
      opacity: 1;
    }
  }

  :global([data-theme='analog']) .task-card {
    background-color: var(--surface-800);
    background-image: var(--grain-fine);
    background-blend-mode: multiply;
    border-width: 1.5px;
  }

  :global([data-theme='analog']) .task-card:hover {
    background-color: var(--tint-hover);
    background-image: var(--grain-fine), var(--ink-wash);
    background-blend-mode: multiply, normal;
  }

  :global([data-theme='analog']) .task-card.priority-high {
    border-left-width: 4px;
  }

  :global([data-theme='analog']) .task-title {
    font-family: 'Source Serif 4', Georgia, serif;
  }
</style>
