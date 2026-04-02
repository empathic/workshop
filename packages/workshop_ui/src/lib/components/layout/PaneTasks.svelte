<script lang="ts">
  import TaskPanel from '../TaskPanel.svelte';
</script>

<div class="pane-tasks">
  <TaskPanel embedded={true} />
</div>

<style>
  .pane-tasks {
    width: 100%;
    height: 100%;
    overflow: hidden;
  }
</style>
