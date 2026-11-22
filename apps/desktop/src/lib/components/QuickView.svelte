<script lang="ts">
  import { L } from "../i18n.svelte";
  // Petite fenêtre ouverte par le menu clic droit : suit les conversions lancées par un
  // préréglage, montre les erreurs telles quelles, puis se ferme seule si tout a réussi.
  import logo from "$branding/svg/logo-small.svg";
  import Icon from "./Icon.svelte";
  import JobList from "./JobList.svelte";
  import Callout from "./ui/Callout.svelte";
  import ProgressBar from "./ui/ProgressBar.svelte";
  import { store } from "../store.svelte";
  import { api, errorText } from "../api";
  import { baseName } from "../format";

  const AUTO_CLOSE = 6;

  const errors = $derived(store.quick.filter((b) => b.error));
  const ignored = $derived(store.quick.flatMap((b) => b.run?.ignored ?? []));
  const names = $derived([...new Set(store.quick.flatMap((b) => (b.run ? [b.run.preset.name] : [])))]);
  const problems = $derived(errors.length + store.jobs.filter((j) => j.status === "failed").length);
  const allDone = $derived(store.quick.length > 0 && store.active === 0);
  const succeeded = $derived(allDone && problems === 0 && store.jobs.some((j) => j.status === "done"));

  let keepOpen = $state(false);
  let left = $state(AUTO_CLOSE);

  $effect(() => {
    if (!succeeded || keepOpen) {
      left = AUTO_CLOSE;
      return;
    }
    const timer = setInterval(() => {
      left -= 1;
      if (left <= 0) {
        clearInterval(timer);
        void close();
      }
    }, 1000);
    return () => clearInterval(timer);
  });

  async function close() {
    try {
      await (await api).closeWindow();
    } catch (e) {
      store.say(errorText(e));
    }
  }

  async function openMain() {
    keepOpen = true;
    try {
      await (await api).showMain();
    } catch (e) {
      store.say(errorText(e));
    }
  }
</script>

<!-- Survoler la fenêtre suffit à empêcher la fermeture automatique. -->
<div class="quick" role="presentation" onpointerenter={() => (keepOpen = true)}>
  <header>
    <img src={logo} alt="" width="30" height="30" />
    <div>
      <span class="eyebrow">{L("Conversion rapide", "Quick conversion")}</span>
      <strong>{names.length ? names.join(", ") : L("Conversion rapide", "Quick conversion")}</strong>
    </div>
  </header>

  <div class="body">
    {#if !store.quick.length}
      <div class="wait">
        <p class="muted">{L("Préparation des conversions…", "Preparing the conversions…")}</p>
        <ProgressBar thin label={L("Préparation des conversions", "Preparing the conversions")} />
      </div>
    {/if}

    {#each errors as b, i (i)}
      <Callout tone="danger" role="alert">
        <p class="selectable">{b.error}</p>
        {#if b.files.length}
          <p class="faint small selectable">{b.files.map(baseName).join(", ")}</p>
        {/if}
      </Callout>
    {/each}

    {#if ignored.length}
      <Callout tone="warn" icon="skip">
        <ul class="ignored">
          {#each ignored as g, i (i)}
            <li><span class="selectable" title={g.path}>{baseName(g.path)}</span> <span class="faint">{L(`— ignoré : ${g.reason}`, `— skipped: ${g.reason}`)}</span></li>
          {/each}
        </ul>
      </Callout>
    {/if}

    {#if store.jobs.length}
      <JobList />
    {/if}
  </div>

  <footer>
    {#if succeeded && !keepOpen}
      <span class="faint small done"><Icon name="check" size={13} /> {L(`Tout est terminé · fermeture dans ${left} s`, `All done · closing in ${left} s`)}</span>
      <button class="btn small ghost" onclick={() => (keepOpen = true)}>{L("Garder ouvert", "Keep open")}</button>
    {:else if store.active}
      <span class="faint small">{L("Fermer la fenêtre annule les conversions en cours.", "Closing the window cancels the running conversions.")}</span>
    {:else}
      <span></span>
    {/if}
    <div class="buttons">
      <button class="btn small" onclick={openMain}>{L("Ouvrir Chud Converter", "Open Chud Converter")}</button>
      <button class="btn small primary" onclick={close}>{store.active ? L("Annuler et fermer", "Cancel and close") : L("Fermer", "Close")}</button>
    </div>
  </footer>
</div>

<style>
  .quick {
    height: 100%;
    display: flex;
    flex-direction: column;
  }

  header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 18px;
    background: var(--rail);
    border-bottom: 1px solid var(--rail-border);
  }

  header img {
    filter: drop-shadow(0 0 10px rgb(120 110 255 / 45%));
  }

  header div {
    display: grid;
    line-height: 1.3;
    min-width: 0;
  }

  header strong {
    font-family: var(--font-display);
    font-size: 15px;
    font-weight: 650;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .wait {
    display: grid;
    gap: 10px;
    padding: 14px 2px;
  }

  .ignored {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 2px;
  }

  .small {
    font-size: 12px;
  }

  .done {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--ok);
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 8px 10px;
    padding: 10px 16px;
    border-top: 1px solid var(--rail-border);
    background: var(--rail);
  }

  .buttons {
    display: flex;
    gap: 8px;
    margin-left: auto;
  }
</style>
