<script lang="ts">
  import ChatPanel from '../ChatPanel.svelte';

  interface Props {
    scope: 'global' | string;
  }

  let { scope }: Props = $props();
</script>

<div class="pane-chat">
  <ChatPanel embedded={true} />
</div>

<style>
  .pane-chat {
    width: 100%;
    height: 100%;
    overflow: hidden;
  }
</style>
