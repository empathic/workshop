<script lang="ts">
  import SettingsPanel from '../settings/SettingsPanel.svelte';
  import { setPaneContent } from '$lib/stores/layout';
  import { currentInstanceId } from '$lib/stores/instances';
  import { get } from 'svelte/store';

  interface Props {
    paneId: string;
  }

  let { paneId }: Props = $props();

  function handleBack() {
    const instanceId = get(currentInstanceId);
    if (instanceId) {
      setPaneContent(paneId, { kind: 'conversation', instanceId, viewMode: 'structured' });
    } else {
      setPaneContent(paneId, { kind: 'landing' });
    }
  }
</script>

<div class="pane-settings">
  <SettingsPanel embedded={true} onback={handleBack} />
</div>

<style>
  .pane-settings {
    width: 100%;
    height: 100%;
    overflow: hidden;
  }
</style>
