<script lang="ts">
  import FileExplorer from '../FileExplorer.svelte';
  import { openGitTab } from '$lib/stores/git';
  import { onMount } from 'svelte';

  // Open to the git tab by default when used as a pane
  onMount(() => {
    openGitTab();
  });
</script>

<div class="pane-git">
  <FileExplorer embedded={true} />
</div>

<style>
  .pane-git {
    width: 100%;
    height: 100%;
    overflow: hidden;
  }
</style>
