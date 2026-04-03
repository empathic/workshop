<script lang="ts">
  import type { Task, Instance } from '$lib/types';

  interface Props {
    task: Task;
    expanded: boolean;
    editing: boolean;
    editTitle: string;
    editBody: string;
    instanceList: Instance[];
    ontoggle: () => void;
    oncomplete: () => void;
    onsend: () => void;
    onedit: () => void;
    ondelete: () => void;
    onassign: (instanceId: string) => void;
    onedittitlechange: (value: string) => void;
    oneditbodychange: (value: string) => void;
    oneditkeydown: (e: KeyboardEvent) => void;
    oneditcommit: () => void;
    oneditcancel: () => void;
    formatTime: (ts: number) => string;
    instanceName: (id: string | null) => string;
  }

  let {
    task,
    expanded,
    editing,
    editTitle,
    editBody,
    instanceList,
    ontoggle,
    oncomplete,
    onsend,
    onedit,
    ondelete,
    onassign,
    onedittitlechange,
    oneditbodychange,
    oneditkeydown,
    oneditcommit,
    oneditcancel,
    formatTime,
    instanceName
  }: Props = $props();
</script>

<div class="task-item" class:expanded>
  <!-- Main row -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="task-row" onclick={ontoggle}>
    <button
      class="check-btn"
      onclick={(e) => {
        e.stopPropagation();
        oncomplete();
      }}
      title="Mark complete"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14">
        <circle cx="12" cy="12" r="10" />
      </svg>
    </button>
    <div class="task-text">
      {#if editing}
        <!-- svelte-ignore a11y_autofocus -->
        <input
          type="text"
          class="edit-input"
          value={editTitle}
          oninput={(e) => onedittitlechange((e.target as HTMLInputElement).value)}
          onkeydown={oneditkeydown}
          onblur={oneditcommit}
          onclick={(e) => e.stopPropagation()}
          autofocus
        />
      {:else}
        <span class="task-title">{task.title}</span>
      {/if}
      {#if task.body && !expanded}
        <span class="has-body-dot" title="Has body text"></span>
      {/if}
    </div>
    <span class="task-age">{formatTime(task.created_at)}</span>
  </div>

  <!-- Expanded detail -->
  {#if expanded}
    <div class="task-detail">
      {#if editing}
        <textarea
          class="edit-body"
          value={editBody}
          oninput={(e) => oneditbodychange((e.target as HTMLTextAreaElement).value)}
          rows="3"
          placeholder="Body..."
        ></textarea>
        <div class="edit-actions">
          <button class="detail-btn save-btn" onclick={oneditcommit}>Save</button>
          <button class="detail-btn" onclick={oneditcancel}>Cancel</button>
        </div>
      {:else}
        {#if task.body}
          <pre class="task-body-text">{task.body}</pre>
        {/if}
        <div class="detail-row">
          <select
            class="assign-select"
            value={task.instance_id ?? ''}
            onchange={(e) => onassign((e.target as HTMLSelectElement).value)}
          >
            <option value="">Unassigned</option>
            {#each instanceList as inst}
              <option value={inst.id}>{inst.custom_name ?? inst.name}</option>
            {/each}
          </select>
          {#if task.instance_id}
            <button class="detail-btn send-btn" onclick={onsend} title="Send to {instanceName(task.instance_id)}">
              Send &rarr;
            </button>
          {/if}
          <button class="detail-btn edit-btn" onclick={onedit}> Edit </button>
          <button class="detail-btn delete-btn" onclick={ondelete}> Delete </button>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  /* === Task item === */
  .task-item {
    border-bottom: 1px solid var(--surface-border);
    transition: background 0.1s ease;
  }

  .task-item:last-child {
    border-bottom: none;
  }

  .task-item:hover {
    background: var(--tint-hover);
  }

  .task-item.expanded {
    background: var(--tint-active);
  }

  .task-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 14px;
    cursor: pointer;
    user-select: none;
  }

  .check-btn {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 0;
    transition: color 0.15s ease;
  }

  .check-btn:hover {
    color: var(--status-green);
  }

  .task-text {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .task-title {
    font-size: 12px;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .has-body-dot {
    display: inline-block;
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: var(--accent-500);
    flex-shrink: 0;
    opacity: 0.5;
  }

  .task-age {
    font-size: 10px;
    color: var(--text-muted);
    flex-shrink: 0;
    font-variant-numeric: tabular-nums;
  }

  /* === Edit inline === */
  .edit-input {
    width: 100%;
    padding: 2px 6px;
    background: var(--surface-800);
    border: 1px solid var(--accent-600);
    border-radius: 3px;
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
    outline: none;
  }

  /* === Expanded detail === */
  .task-detail {
    padding: 4px 14px 10px 42px;
    animation: detail-expand 0.12s ease-out;
  }

  @keyframes detail-expand {
    from {
      opacity: 0;
      max-height: 0;
    }
    to {
      opacity: 1;
      max-height: 200px;
    }
  }

  .task-body-text {
    margin: 0 0 8px;
    padding: 6px 8px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    font-size: 11px;
    font-family: var(--font-mono);
    color: var(--text-secondary);
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 80px;
    overflow-y: auto;
  }

  .edit-body {
    width: 100%;
    margin-bottom: 6px;
    padding: 6px 8px;
    background: var(--surface-800);
    border: 1px solid var(--accent-600);
    border-radius: 3px;
    color: var(--text-primary);
    font-size: 11px;
    font-family: var(--font-mono);
    outline: none;
    resize: vertical;
    min-height: 36px;
  }

  .edit-actions {
    display: flex;
    gap: 6px;
    margin-bottom: 6px;
  }

  .detail-row {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
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
  }

  .assign-select:focus {
    border-color: var(--accent-600);
  }

  .detail-btn {
    padding: 3px 8px;
    background: none;
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    color: var(--text-muted);
    font-size: 10px;
    font-weight: 600;
    font-family: inherit;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .detail-btn:hover {
    border-color: var(--surface-border-light);
    color: var(--text-secondary);
  }

  .detail-btn.send-btn {
    border-color: var(--accent-600);
    color: var(--accent-400);
  }

  .detail-btn.send-btn:hover {
    background: var(--tint-active);
    color: var(--accent-300);
  }

  .detail-btn.save-btn {
    border-color: var(--accent-600);
    color: var(--accent-400);
  }

  .detail-btn.delete-btn:hover {
    border-color: var(--status-red-border);
    color: var(--status-red);
  }

  /* === Analog theme === */
  :global([data-theme='analog']) .task-item.expanded {
    background-color: var(--tint-active);
    background-image: var(--grain-fine), var(--ink-wash);
    background-blend-mode: multiply, normal;
  }

  :global([data-theme='analog']) .task-title {
    font-family: 'Source Serif 4', Georgia, serif;
  }

  :global([data-theme='analog']) .detail-btn {
    font-family: 'Source Serif 4', Georgia, serif;
    text-transform: none;
    letter-spacing: 0;
  }
</style>
