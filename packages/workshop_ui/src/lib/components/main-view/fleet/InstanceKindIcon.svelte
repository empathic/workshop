<script lang="ts">
  import type { InstanceKind } from '$lib/types';

  interface Props {
    kind: InstanceKind;
  }

  let { kind }: Props = $props();
</script>

{#if kind.type === 'Structured'}
  <svg
    viewBox="0 0 16 16"
    fill="none"
    stroke="currentColor"
    stroke-width="1.5"
    stroke-linecap="round"
    stroke-linejoin="round"
  >
    <rect x="3" y="5" width="10" height="8" rx="1.5" />
    <line x1="8" y1="5" x2="8" y2="2" />
    <circle cx="8" cy="1.5" r="0.5" fill="currentColor" stroke="none" />
    <circle cx="6" cy="9" r="1" fill="currentColor" stroke="none" />
    <circle cx="10" cy="9" r="1" fill="currentColor" stroke="none" />
  </svg>
{:else}
  <svg
    viewBox="0 0 16 16"
    fill="none"
    stroke="currentColor"
    stroke-width="1.5"
    stroke-linecap="round"
    stroke-linejoin="round"
  >
    <polyline points="4 8 6 10 4 12" />
    <line x1="8" y1="12" x2="12" y2="12" />
  </svg>
{/if}
