<script lang="ts">
  import Terminal from '../Terminal.svelte';
  import ErrorBoundary from '../ErrorBoundary.svelte';

  interface Props {
    instanceId: string;
    paneId: string;
  }

  let { instanceId, paneId }: Props = $props();
</script>

<div class="pane-terminal">
  <ErrorBoundary>
    {#snippet children()}
      {#key 'terminal-' + instanceId}
        <Terminal {instanceId} {paneId} />
      {/key}
    {/snippet}
  </ErrorBoundary>
</div>

<style>
  .pane-terminal {
    flex: 1;
    min-height: 0;
  }
</style>
