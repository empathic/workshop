<script lang="ts">
  import { onMount } from 'svelte';
  import { base } from '$app/paths';
  import { goto } from '$app/navigation';
  import { openFullscreen } from '$lib/stores/fullscreen';

  onMount(() => {
    openFullscreen('history');
    goto(`${base}/`, { replaceState: true });
  });
</script>
