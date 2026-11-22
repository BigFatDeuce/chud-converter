<script lang="ts">
  import { L, i18n } from "./lib/i18n.svelte";
  import { onMount } from "svelte";
  import Sidebar from "./lib/components/Sidebar.svelte";
  import ConvertView from "./lib/components/ConvertView.svelte";
  import EnginesView from "./lib/components/EnginesView.svelte";
  import PresetsView from "./lib/components/PresetsView.svelte";
  import AboutView from "./lib/components/AboutView.svelte";
  import SettingsView from "./lib/components/SettingsView.svelte";
  import QuickView from "./lib/components/QuickView.svelte";
  import Welcome from "./lib/components/Welcome.svelte";
  import Icon from "./lib/components/Icon.svelte";
  import Cosmos from "./lib/components/ui/Cosmos.svelte";
  import Toast from "./lib/components/ui/Toast.svelte";
  import { store, type View } from "./lib/store.svelte";
  import "./lib/ui.svelte";
  import { api, errorText, type WindowKind } from "./lib/api";

  let fatal = $state<string | null>(null);
  let kind = $state<WindowKind | null>(null);

  onMount(() => {
    (async () => {
      kind = await (await api).windowKind();
      await (kind === "quick" ? store.initQuick() : store.init());
    })().catch((e) => (fatal = errorText(e)));
  });

  // Raccourcis : Ctrl+1 à 5 pour les écrans, Ctrl+O pour ajouter des fichiers.
  const order: View[] = ["convert", "presets", "engines", "settings", "about"];
  function onkey(ev: KeyboardEvent) {
    if (kind !== "main" || store.welcomeOpen || !(ev.ctrlKey || ev.metaKey) || ev.altKey || ev.shiftKey) return;
    const n = Number(ev.key);
    if (n >= 1 && n <= order.length) {
      ev.preventDefault();
      store.view = order[n - 1];
    } else if (ev.key.toLowerCase() === "o") {
      ev.preventDefault();
      store.view = "convert";
      void store.pickFiles();
    }
  }
</script>

<svelte:window onkeydown={onkey} />

<Cosmos />

<!-- Nouvelle langue : toute l'interface est reconstruite (textes calculés une fois compris). -->
{#key i18n.lang}
{#if kind === "quick"}
  <QuickView />
  {#if store.notice}
    <Toast text={store.notice} onclose={() => (store.notice = null)} />
  {/if}
{:else}
<div class="shell">
  <Sidebar />
  <main>
    {#if store.fake}
      <div class="fake" role="status">
        <Icon name="alert" size={13} />
        {L("Aperçu de l'interface dans un navigateur : données fictives, aucune conversion réelle.", "Interface preview in a browser: sample data, no real conversion.")}
      </div>
    {/if}
    {#if fatal}
      <div class="fatal-wrap">
        <div class="fatal panel pop" role="alert">
          <span class="fatal-ico"><Icon name="alert" size={22} /></span>
          <h2>{L("Impossible de démarrer", "Cannot start")}</h2>
          <p class="selectable muted">{fatal}</p>
        </div>
      </div>
    {:else}
      {#key store.view}
        <div class="view enter">
          {#if store.view === "convert"}
            <ConvertView />
          {:else if store.view === "presets"}
            <PresetsView />
          {:else if store.view === "engines"}
            <EnginesView />
          {:else if store.view === "settings"}
            <SettingsView />
          {:else}
            <AboutView />
          {/if}
        </div>
      {/key}
    {/if}
  </main>

  {#if store.welcomeOpen}
    <Welcome />
  {/if}

  {#if store.dropHover}
    <div class="drop">
      <div class="drop-inner pop">
        <div class="rings ambient" aria-hidden="true"><span></span><span></span><span></span></div>
        <span class="drop-ico"><Icon name="upload" size={30} stroke={1.7} /></span>
        <p>{L("Déposez pour ajouter à la liste", "Drop to add to the list")}</p>
        <span class="muted">{L("Rien n'est converti tant que vous n'avez pas choisi un format.", "Nothing is converted until you choose a format.")}</span>
      </div>
    </div>
  {/if}

  {#if store.notice}
    <Toast text={store.notice} action={store.noticeAction} onclose={() => store.closeNotice()} />
  {/if}
</div>
{/if}
{/key}

<style>
  .shell {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    height: 100%;
  }

  main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  .view {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .fake {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 5px 16px;
    background: var(--depends-soft);
    border-bottom: 1px solid color-mix(in srgb, var(--depends) 25%, transparent);
    color: var(--depends);
    font-size: 12px;
    font-weight: 600;
  }

  .fatal-wrap {
    flex: 1;
    display: grid;
    place-items: center;
    padding: 24px;
  }

  .fatal {
    max-width: 560px;
    padding: 28px;
    display: grid;
    justify-items: center;
    text-align: center;
    gap: 10px;
  }

  .fatal-ico {
    display: grid;
    place-items: center;
    width: 52px;
    height: 52px;
    border-radius: 16px;
    color: var(--lossy);
    background: var(--lossy-soft);
  }

  .drop {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: var(--backdrop);
    z-index: 50;
    pointer-events: none;
    animation: fade var(--t) var(--ease) both;
  }

  .drop-inner {
    position: relative;
    display: grid;
    justify-items: center;
    gap: 8px;
    padding: 44px 64px 36px;
    border: 1.5px dashed var(--accent);
    border-radius: 24px;
    background: var(--surface-solid);
    box-shadow:
      var(--shadow-lg),
      0 0 60px -10px var(--accent);
    overflow: hidden;
  }

  .drop-inner p {
    font-family: var(--font-display);
    font-size: 19px;
    font-weight: 600;
  }

  .drop-ico {
    position: relative;
    display: grid;
    place-items: center;
    width: 64px;
    height: 64px;
    margin-bottom: 6px;
    border-radius: 50%;
    color: #fff;
    background: var(--grad);
    box-shadow: var(--glow);
  }

  .rings {
    position: absolute;
    top: 76px;
    left: 50%;
    width: 0;
    height: 0;
  }

  .rings span {
    position: absolute;
    left: -32px;
    top: -32px;
    width: 64px;
    height: 64px;
    border-radius: 50%;
    border: 1px solid var(--accent);
    opacity: 0;
    animation: ripple 2.4s var(--ease-out) infinite;
  }

  .rings span:nth-child(2) {
    animation-delay: 0.8s;
  }

  .rings span:nth-child(3) {
    animation-delay: 1.6s;
  }

  @keyframes ripple {
    from {
      transform: scale(1);
      opacity: 0.6;
    }
    to {
      transform: scale(3.6);
      opacity: 0;
    }
  }
</style>
