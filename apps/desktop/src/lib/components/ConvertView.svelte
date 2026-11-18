<script lang="ts">
  import FileList from "./FileList.svelte";
  import JobList from "./JobList.svelte";
  import TargetPanel from "./TargetPanel.svelte";
  import EngineSetup from "./EngineSetup.svelte";
  import { store } from "../store.svelte";
</script>

<div class="convert">
  <div class="left">
    <EngineSetup place="convert" />
    <FileList />
    {#if store.jobs.length}
      <JobList />
    {/if}
  </div>
  <TargetPanel />
</div>

<style>
  .convert {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 380px;
  }

  .left {
    min-height: 0;
    overflow-y: auto;
    padding: 20px 20px 24px;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }
</style>
