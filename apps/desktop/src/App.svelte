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
  import { store } from "./lib/store.svelte";
  import { api, errorText, type WindowKind } from "./lib/api";

  let fatal = $state<string | null>(null);
  let kind = $state<WindowKind | null>(null);

  onMount(() => {
    (async () => {
      kind = await (await api).windowKind();
      await (kind === "quick" ? store.initQuick() : store.init());
    })().catch((e) => (fatal = errorText(e)));
  });
</script>

<!-- Nouvelle langue : toute l'interface est reconstruite (textes calculés une fois compris). -->
{#key i18n.lang}
{#if kind === "quick"}
  <QuickView />
  {#if store.notice}
    <div class="toast card" role="status">
      <span>{store.notice}</span>
      <button class="icon-btn" aria-label={L("Fermer", "Close")} onclick={() => (store.notice = null)}><Icon name="x" size={14} /></button>
    </div>
  {/if}
{:else}
<div class="shell">
  <Sidebar />
  <main>
    {#if store.fake}
      <div class="fake" role="status">
        <Icon name="alert" size={14} />
        {L("Aperçu de l'interface dans un navigateur : données fictives, aucune conversion réelle.", "Interface preview in a browser: sample data, no real conversion.")}
      </div>
    {/if}
    {#if fatal}
      <div class="fatal card">
        <Icon name="alert" size={20} />
        <div>
          <h2>{L("Impossible de démarrer", "Cannot start")}</h2>
          <p class="selectable">{fatal}</p>
        </div>
      </div>
    {:else if store.view === "convert"}
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
  </main>

  {#if store.welcomeOpen}
    <Welcome />
  {/if}

  {#if store.dropHover}
    <div class="drop">
      <div class="drop-inner">
        <Icon name="upload" size={40} stroke={1.6} />
        <p>{L("Déposez pour ajouter à la liste", "Drop to add to the list")}</p>
        <span>{L("Rien n'est converti tant que vous n'avez pas choisi un format.", "Nothing is converted until you choose a format.")}</span>
      </div>
    </div>
  {/if}

  {#if store.notice}
    <div class="toast card" role="status">
      <span>{store.notice}</span>
      {#if store.noticeAction}
        <button class="btn" onclick={() => store.noticeAction?.run()}>{store.noticeAction.label}</button>
      {/if}
      <button class="icon-btn" aria-label={L("Fermer", "Close")} onclick={() => store.closeNotice()}><Icon name="x" size={14} /></button>
    </div>
  {/if}
</div>
{/if}
{/key}

<style>
  .shell {
    display: grid;
    grid-template-columns: 208px 1fr;
    height: 100%;
  }

  main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  .fake {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 16px;
    background: var(--depends-soft);
    color: var(--depends);
    font-size: 12.5px;
    font-weight: 600;
  }

  .fatal {
    margin: 24px;
    padding: 16px;
    display: flex;
    gap: 12px;
    color: var(--lossy);
  }

  .fatal p {
    color: var(--text);
    margin-top: 4px;
  }

  .drop {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgb(1 17 40 / 55%);
    backdrop-filter: blur(2px);
    z-index: 50;
    pointer-events: none;
  }

  .drop-inner {
    display: grid;
    justify-items: center;
    gap: 8px;
    padding: 36px 56px;
    border: 2px dashed var(--accent);
    border-radius: 18px;
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--shadow-lg);
  }

  .drop-inner p {
    font-size: 17px;
    font-weight: 600;
  }

  .drop-inner span {
    color: var(--muted);
  }

  .toast {
    position: fixed;
    bottom: 18px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 8px 8px 14px;
    max-width: 640px;
    box-shadow: var(--shadow-lg);
    z-index: 60;
  }
</style>
