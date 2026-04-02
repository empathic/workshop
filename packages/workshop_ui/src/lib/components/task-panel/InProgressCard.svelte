<script lang="ts">
  import type { Task } from '$lib/types';

  interface Props {
    task: Task;
    expanded: boolean;
    ontoggle: () => void;
    onnavigate: (instanceId: string) => void;
    onsend: () => void;
    oncomplete: () => void;
    onedit: () => void;
    formatTime: (ts: number) => string;
    instanceName: (id: string | null) => string;
  }

  let { task, expanded, ontoggle, onnavigate, onsend, oncomplete, onedit, formatTime, instanceName }: Props = $props();
</script>

<div class="task-item in-progress" class:expanded>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="task-row" onclick={ontoggle}>
    <span class="sent-icon">&triangleright;</span>
    <div class="task-text">
      <span class="task-title">{task.title}</span>
    </div>
    {#if task.instance_id}
      <button
        class="task-instance-link"
        onclick={(e) => {
          e.stopPropagation();
          onnavigate(task.instance_id!);
        }}
        title="Go to {instanceName(task.instance_id)}"
      >
        {instanceName(task.instance_id)}
      </button>
    {:else}
      <span class="task-instance-tag">{instanceName(task.instance_id)}</span>
    {/if}
  </div>
  {#if expanded}
    <div class="sent-detail">
      {#if task.dispatches?.length}
        <div class="dispatch-history">
          {#each task.dispatches as dispatch}
            <div class="dispatch-item">
              <span class="dispatch-instance">{instanceName(dispatch.instance_id)}</span>
              <span class="dispatch-time">{formatTime(dispatch.sent_at)}</span>
            </div>
          {/each}
        </div>
      {/if}
      <div class="detail-row">
        {#if task.instance_id}
          <button class="detail-btn send-btn" onclick={onsend} title="Send again"> Send &rarr; </button>
        {/if}
        <button class="detail-btn" onclick={oncomplete}> Complete </button>
        <button class="detail-btn edit-btn" onclick={onedit}> Edit </button>
      </div>
    </div>
  {/if}
</div>

<style>
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

  .task-item.in-progress {
    opacity: 0.7;
  }

  .task-item.in-progress.expanded {
    opacity: 0.9;
    background: var(--tint-active);
  }

  .task-item.in-progress .task-row {
    cursor: pointer;
  }

  .task-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 14px;
    cursor: pointer;
    user-select: none;
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

  .sent-icon {
    flex-shrink: 0;
    font-size: 10px;
    color: var(--status-blue);
    width: 20px;
    text-align: center;
  }

  .task-instance-link {
    font-size: 10px;
    color: var(--accent-400);
    flex-shrink: 0;
    padding: 1px 5px;
    background: var(--tint-active);
    border: 1px solid var(--accent-600);
    border-radius: 3px;
    cursor: pointer;
    transition: all 0.15s ease;
    font-family: inherit;
  }

  .task-instance-link:hover {
    background: var(--tint-active-strong);
    color: var(--accent-300);
  }

  .task-instance-tag {
    font-size: 10px;
    color: var(--text-muted);
    flex-shrink: 0;
    padding: 1px 5px;
    background: var(--tint-subtle);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
  }

  /* === Sent detail === */
  .sent-detail {
    padding: 4px 14px 8px 42px;
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

  .dispatch-history {
    margin-bottom: 8px;
  }

  .dispatch-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 2px 0;
    font-size: 10px;
    color: var(--text-muted);
  }

  .dispatch-instance {
    color: var(--text-secondary);
    font-weight: 600;
  }

  .dispatch-time {
    font-variant-numeric: tabular-nums;
  }

  .detail-row {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
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

  /* === Analog theme === */
  :global([data-theme='analog']) .task-title {
    font-family: 'Source Serif 4', Georgia, serif;
  }

  :global([data-theme='analog']) .detail-btn {
    font-family: 'Source Serif 4', Georgia, serif;
    text-transform: none;
    letter-spacing: 0;
  }
</style>
