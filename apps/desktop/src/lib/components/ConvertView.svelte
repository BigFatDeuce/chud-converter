<script lang="ts">
  import { L } from "../i18n.svelte";
  import FileList from "./FileList.svelte";
  import JobList from "./JobList.svelte";
  import TargetPanel from "./TargetPanel.svelte";
  import EngineSetup from "./EngineSetup.svelte";
  import Icon from "./Icon.svelte";
  import { store } from "../store.svelte";

  // Parcours en trois étapes, reliées comme une constellation : où en est l'utilisateur.
  const steps = $derived.by(() => {
    const files = store.checked.length > 0;
    const format = files && !!store.targetInfo;
    const running = store.active > 0;
    const finished = store.jobs.length > 0 && !running;
    return [
      { label: L("Fichiers", "Files"), done: files, current: !files },
      { label: L("Format", "Format"), done: format, current: files && !format },
      { label: L("Conversion", "Conversion"), done: finished, current: format && !finished, busy: running },
    ];
  });
</script>

<div class="convert">
  <div class="left">
    <header class="head enter">
      <div>
        <span class="eyebrow">{L("Atelier", "Workshop")}</span>
        <h1>{L("Convertir", "Convert")}</h1>
      </div>
      <ol class="steps" aria-label={L("Étapes", "Steps")}>
        {#each steps as s, i (i)}
          <li class:done={s.done} class:current={s.current} aria-current={s.current ? "step" : undefined}>
            <span class="node" class:busy={s.busy}>
              {#if s.done}<Icon name="check" size={11} stroke={3} />{:else}{i + 1}{/if}
            </span>
            <span class="lbl">{s.label}</span>
          </li>
        {/each}
      </ol>
    </header>
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
    grid-template-columns: minmax(0, 1fr) clamp(340px, 31vw, 430px);
  }

  .left {
    min-height: 0;
    overflow-y: auto;
    scrollbar-gutter: stable;
    padding: 26px clamp(16px, 2.4vw, 34px) 32px;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  .head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 12px 20px;
    flex-wrap: wrap;
  }

  h1 {
    font-size: 26px;
    font-weight: 650;
    letter-spacing: -0.025em;
  }

  .steps {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    align-items: center;
  }

  .steps li {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--faint);
    font-size: 12.5px;
    font-weight: 550;
  }

  /* Trait qui relie les étapes. */
  .steps li + li::before {
    content: "";
    width: 26px;
    height: 1px;
    margin: 0 8px;
    background: var(--border-strong);
  }

  .steps li.done + li::before {
    background: linear-gradient(90deg, var(--accent-3), var(--accent-2));
    box-shadow: 0 0 6px var(--accent-2);
  }

  .node {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    border: 1px solid var(--border-strong);
    background: var(--surface-2);
    font-size: 11px;
    font-weight: 700;
    transition:
      background var(--t) var(--ease),
      box-shadow var(--t) var(--ease);
  }

  .current {
    color: var(--text);
  }

  .current .node {
    border-color: var(--accent);
    color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }

  .done {
    color: var(--muted);
  }

  .done .node {
    border-color: transparent;
    background: var(--grad);
    color: #fff;
    box-shadow: 0 0 10px -2px var(--accent);
  }

  .node.busy {
    animation: pulse 1.6s ease-in-out infinite;
  }

  @media (max-width: 1040px) {
    .lbl {
      display: none;
    }

    .current .lbl {
      display: inline;
    }
  }
</style>
