<script lang="ts">
  import FileExplorer from '../FileExplorer.svelte';
  import { currentInstance } from '$lib/stores/instances';
  import { currentExplorerPath, navigateToDirectory } from '$lib/stores/files';

  // When the file-explorer pane mounts and the store has no path yet,
  // initialize it to the current instance's working directory so the
  // browser actually shows something instead of an empty state.
  $effect(() => {
    const instance = $currentInstance;
    const path = $currentExplorerPath;
    if (instance?.working_dir && !path) {
      navigateToDirectory(instance.working_dir);
    }
  });
</script>

<div class="pane-file-explorer">
  <FileExplorer embedded={true} />
</div>

<style>
  .pane-file-explorer {
    width: 100%;
    height: 100%;
    overflow: hidden;
  }
</style>
