<script lang="ts">
  import type { Instance } from '$lib/types';
  import type { InboxItem } from '$lib/stores/inbox';

  interface Props {
    item: InboxItem;
    instance: Instance;
    onclick: () => void;
    ondismiss?: () => void;
  }

  let { item, instance, onclick, ondismiss }: Props = $props();

  const displayName = $derived(instance.custom_name ?? instance.name);

  const promptSnippet = $derived.by(() => {
    if (item.event_type !== 'needs_input') return null;
    if (!item.metadata_json) return null;
    try {
      const meta = JSON.parse(item.metadata_json);
      const prompt = meta.prompt as string | undefined;
      if (!prompt) return null;
      return prompt.length > 30 ? prompt.slice(0, 30) + '\u2026' : prompt;
    } catch {
      return null;
    }
  });

  const verb = $derived.by(() => {
    switch (item.event_type) {
      case 'needs_input':
        return 'Respond';
      case 'completed_turn':
        return 'Review';
      case 'error':
        return 'Error';
      default:
        return item.event_type;
    }
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

  const turnInfo = $derived(item.event_type === 'completed_turn' && item.turn_count > 0 ? `${item.turn_count}` : null);

  function handleDismiss(e: Event) {
    e.stopPropagation();
    ondismiss?.();
  }
</script>

<button class="action-chip {urgency}" {onclick} title="{verb} \u2192 {displayName}">
  <span class="chip-verb">{verb}</span>
  <span class="chip-target">
    <span class="chip-name">{displayName}</span>
    {#if turnInfo}
      <span class="chip-turns">{turnInfo}</span>
    {/if}
  </span>
  {#if promptSnippet}
    <span class="chip-context">{promptSnippet}</span>
  {/if}
  {#if ondismiss && item.event_type === 'completed_turn'}
    <span
      class="chip-dismiss"
      role="button"
      tabindex="-1"
      onclick={handleDismiss}
      onkeydown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          handleDismiss(e);
        }
      }}
      title="Dismiss"
      aria-label="Dismiss"
    >
      &times;
    </span>
  {/if}
</button>

<style>
  .action-chip {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 3px 8px;
    background: var(--surface-600);
    border: 1.5px solid var(--surface-border);
    border-radius: 3px;
    color: var(--text-secondary);
    font-size: 10px;
    font-family: inherit;
    cursor: pointer;
    transition: all 0.12s ease;
    min-height: 26px;
    white-space: nowrap;
    flex-shrink: 0;
    position: relative;
  }

  .action-chip:hover {
    background: var(--surface-500);
    border-color: var(--surface-border-light);
  }

  /* === Urgency variants === */
  .action-chip.critical {
    border-color: var(--status-red);
    background: color-mix(in srgb, var(--status-red) 8%, var(--surface-600));
    box-shadow: 0 0 6px color-mix(in srgb, var(--status-red) 20%, transparent);
    animation: chip-pulse 2s ease-in-out infinite;
  }

  .action-chip.critical:hover {
    background: color-mix(in srgb, var(--status-red) 15%, var(--surface-600));
  }

  .action-chip.error {
    border-color: var(--status-red);
    background: color-mix(in srgb, var(--status-red) 6%, var(--surface-600));
  }

  .action-chip.error:hover {
    background: color-mix(in srgb, var(--status-red) 12%, var(--surface-600));
  }

  .action-chip.warning {
    border-color: var(--chrome-accent-500);
    background: color-mix(in srgb, var(--chrome-accent-500) 8%, var(--surface-600));
  }

  .action-chip.warning:hover {
    background: color-mix(in srgb, var(--chrome-accent-500) 15%, var(--surface-600));
  }

  /* === Hierarchy: verb > name > context === */

  /* VERB — loudest element. Sized up, bold, colored by urgency. */
  .chip-verb {
    font-size: 11px;
    font-weight: 800;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .critical .chip-verb {
    color: var(--status-red);
  }
  .error .chip-verb {
    color: var(--status-red);
  }
  .warning .chip-verb {
    color: var(--chrome-accent-400);
  }

  /* TARGET — secondary context. Name + optional turn count. */
  .chip-target {
    display: flex;
    align-items: center;
    gap: 3px;
    opacity: 0.7;
  }

  .chip-name {
    font-size: 9px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 70px;
  }

  .chip-turns {
    font-size: 8px;
    font-weight: 700;
    padding: 0 3px;
    border-radius: 2px;
    background: color-mix(in srgb, currentColor 12%, transparent);
    line-height: 1.3;
  }

  /* CONTEXT — ambient detail. Smallest, most subdued. */
  .chip-context {
    font-size: 8px;
    color: var(--text-muted);
    font-weight: 400;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100px;
    opacity: 0.6;
  }

  /* Dismiss × */
  .chip-dismiss {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    padding: 0;
    margin-left: 2px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 2px;
    color: var(--text-muted);
    font-size: 12px;
    line-height: 1;
    cursor: pointer;
    transition: all 0.1s ease;
    opacity: 0.5;
  }

  .chip-dismiss:hover {
    background: var(--surface-500);
    border-color: var(--surface-border-light);
    color: var(--text-primary);
    opacity: 1;
  }

  @keyframes chip-pulse {
    0%,
    100% {
      box-shadow: 0 0 6px color-mix(in srgb, var(--status-red) 20%, transparent);
    }
    50% {
      box-shadow: 0 0 10px color-mix(in srgb, var(--status-red) 35%, transparent);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .action-chip.critical {
      animation: none;
    }
  }

  /* Analog theme */
  :global([data-theme='analog']) .action-chip {
    background-color: var(--surface-600);
    background-image: var(--grain-fine);
    background-blend-mode: multiply;
  }
</style>
