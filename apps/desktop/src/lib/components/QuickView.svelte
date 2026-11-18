<script lang="ts">
  // Petite fenêtre ouverte par le menu clic droit : suit les conversions lancées par un
  // préréglage, montre les erreurs telles quelles, puis se ferme seule si tout a réussi.
  import logo from "$branding/svg/logo-small.svg";
  import Icon from "./Icon.svelte";
  import JobList from "./JobList.svelte";
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
    <img src={logo} alt="" width="28" height="28" />
    <div>
      <strong>{names.length ? names.join(", ") : "Conversion rapide"}</strong>
      <span class="faint">Chud Converter</span>
    </div>
  </header>

  <div class="body">
    {#if !store.quick.length}
      <p class="muted wait">Préparation des conversions…</p>
    {/if}

    {#each errors as b, i (i)}
      <div class="card error">
        <Icon name="alert" size={16} />
        <div>
          <p class="selectable">{b.error}</p>
          {#if b.files.length}
            <p class="faint small selectable">{b.files.map(baseName).join(", ")}</p>
          {/if}
        </div>
      </div>
    {/each}

    {#if ignored.length}
      <div class="card ignored">
        <Icon name="skip" size={15} />
        <ul>
          {#each ignored as g, i (i)}
            <li><span class="selectable" title={g.path}>{baseName(g.path)}</span> <span class="faint">— ignoré : {g.reason}</span></li>
          {/each}
        </ul>
      </div>
    {/if}

    {#if store.jobs.length}
      <JobList />
    {/if}
  </div>

  <footer>
    {#if succeeded && !keepOpen}
      <span class="faint small">Tout est terminé · fermeture dans {left} s</span>
      <button class="btn small ghost" onclick={() => (keepOpen = true)}>Garder ouvert</button>
    {:else if store.active}
      <span class="faint small">Fermer la fenêtre annule les conversions en cours.</span>
    {:else}
      <span></span>
    {/if}
    <div class="buttons">
      <button class="btn small" onclick={openMain}>Ouvrir Chud Converter</button>
      <button class="btn small primary" onclick={close}>{store.active ? "Annuler et fermer" : "Fermer"}</button>
    </div>
  </footer>
</div>

<style>
  .quick {
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--bg);
  }

  header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px;
    background: var(--sidebar);
    color: var(--sidebar-text);
  }

  header div {
    display: grid;
    line-height: 1.25;
    min-width: 0;
  }

  header strong {
    color: #fff;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  header .faint {
    color: #f9c83a;
    font-size: 11.5px;
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
    padding: 12px 0;
  }

  .error,
  .ignored {
    display: flex;
    gap: 10px;
    padding: 10px 12px;
  }

  .error {
    color: var(--lossy);
  }

  .error p:first-child {
    color: var(--text);
    font-weight: 500;
  }

  .ignored {
    color: var(--muted);
  }

  .ignored ul {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 2px;
    color: var(--text);
  }

  .small {
    font-size: 12px;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 10px 16px;
    border-top: 1px solid var(--border);
    background: var(--surface-2);
  }

  .buttons {
    display: flex;
    gap: 8px;
    margin-left: auto;
  }
</style>
