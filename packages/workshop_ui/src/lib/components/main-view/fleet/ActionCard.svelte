<script lang="ts">
  import type { Instance } from '$lib/types';
  import type { InboxItem } from '$lib/stores/inbox';
  import { formatDuration } from '$lib/stores/inbox';
  import { getStateInfo } from '$lib/utils/instance-state';
  import InstanceKindIcon from './InstanceKindIcon.svelte';

  interface Props {
    item: InboxItem;
    instance: Instance;
    onprimary: () => void;
    ondismiss?: () => void;
    highlighted: boolean;
    tick: number;
  }

  let { item, instance, onprimary, ondismiss, highlighted, tick }: Props = $props();

  const displayName = $derived(instance.custom_name ?? instance.name);
  const stateInfo = $derived(getStateInfo(instance.id, instance.claude_state, instance.claude_state_stale));

  const promptText = $derived.by(() => {
    if (item.event_type !== 'needs_input') return null;
    if (!item.metadata_json) return null;
    try {
      const meta = JSON.parse(item.metadata_json);
      return (meta.prompt as string) ?? null;
    } catch {
      return null;
    }
  });

  // Reference tick to force re-eval
  const timeAgo = $derived.by(() => {
    void tick;
    return formatDuration(item.updated_at) + ' ago';
  });

  const urgency = $derived.by(() => {
    switch (item.event_type) {
      case 'needs_input':
        return 'critical';
      case 'error':
        return 'error';
      case 'completed_turn':
        return 'warning';
      default:
        return 'warning';
    }
  });

  const primaryLabel = $derived.by(() => {
    switch (item.event_type) {
      case 'needs_input':
        return 'Respond';
      case 'completed_turn':
        return 'Review';
      case 'error':
        return 'Investigate';
      default:
        return 'Open';
    }
  });

  const summary = $derived.by(() => {
    switch (item.event_type) {
      case 'needs_input':
        return 'Waiting for input';
      case 'completed_turn':
        return `${item.turn_count} turn${item.turn_count !== 1 ? 's' : ''} completed`;
      case 'error':
        return 'Stopped unexpectedly';
      default:
        return item.event_type;
    }
  });
</script>

<div class="action-card {urgency}" class:highlighted>
  <div class="card-header">
    <span class="card-kind">
      <InstanceKindIcon kind={instance.kind} />
    </span>
    <span class="card-led" style="background: {stateInfo.color}" class:pulse={stateInfo.animate}></span>
    <span class="card-name">{displayName}</span>
    <span class="card-summary">{summary}</span>
    <span class="card-time">{timeAgo}</span>
  </div>

  {#if promptText}
    <div class="card-prompt">{promptText}</div>
  {/if}

  <div class="card-actions">
    <button class="action-btn primary" onclick={onprimary}>{primaryLabel}</button>
    {#if ondismiss}
      <button class="action-btn dismiss" onclick={ondismiss}>Dismiss</button>
    {/if}
  </div>
</div>

<style>
  .action-card {
    padding: 8px 12px;
    border-left: 3px solid var(--surface-border);
    transition: background 0.08s ease;
  }

  .action-card.highlighted {
    background: var(--tint-active-strong);
  }

  .action-card.critical {
    border-left-color: var(--status-red);
    background: color-mix(in srgb, var(--status-red) 5%, transparent);
  }

  .action-card.critical.highlighted {
    background: color-mix(in srgb, var(--status-red) 10%, var(--surface-700));
  }

  .action-card.error {
    border-left-color: var(--status-red);
    background: color-mix(in srgb, var(--status-red) 3%, transparent);
  }

  .action-card.error.highlighted {
    background: color-mix(in srgb, var(--status-red) 8%, var(--surface-700));
  }

  .action-card.warning {
    border-left-color: var(--chrome-accent-600);
    background: color-mix(in srgb, var(--chrome-accent-500) 4%, transparent);
  }

  .action-card.warning.highlighted {
    background: color-mix(in srgb, var(--chrome-accent-500) 10%, var(--surface-700));
  }

  .card-header {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .card-kind {
    width: 12px;
    height: 12px;
    flex-shrink: 0;
    color: var(--text-muted);
    opacity: 0.6;
  }

  .card-kind :global(svg) {
    width: 12px;
    height: 12px;
    display: block;
  }

  .card-led {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .card-led.pulse {
    animation: led-pulse 0.8s ease-in-out infinite;
  }

  .card-name {
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .card-summary {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.03em;
    color: var(--text-muted);
    flex: 1;
    white-space: nowrap;
  }

  .critical .card-summary {
    color: var(--status-red);
  }

  .warning .card-summary {
    color: var(--chrome-accent-400);
  }

  .card-time {
    font-size: 8px;
    color: var(--text-muted);
    opacity: 0.5;
    flex-shrink: 0;
  }

  .card-prompt {
    font-size: 10px;
    color: var(--text-secondary);
    margin: 4px 0 0 18px;
    line-height: 1.4;
    max-height: 3.6em;
    overflow: hidden;
    word-break: break-word;
  }

  .card-actions {
    display: flex;
    gap: 4px;
    margin: 6px 0 0 18px;
  }

  .action-btn {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    padding: 3px 10px;
    border-radius: 3px;
    border: 1px solid var(--surface-border);
    background: var(--surface-600);
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.12s ease;
    font-family: inherit;
    min-height: 22px;
  }

  .action-btn:hover {
    color: var(--chrome-accent-400);
    border-color: var(--chrome-accent-600);
    background: var(--tint-active);
  }

  .action-btn.primary {
    color: var(--surface-900);
    background: var(--chrome-accent-500);
    border-color: var(--chrome-accent-400);
  }

  .action-btn.primary:hover {
    background: var(--chrome-accent-400);
    border-color: var(--chrome-accent-300);
  }

  .action-btn.dismiss {
    color: var(--text-muted);
  }

  .action-btn.dismiss:hover {
    color: var(--text-secondary);
    border-color: var(--surface-border-light);
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
    .card-led.pulse {
      animation: none;
    }
  }
</style>
