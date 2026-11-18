<script lang="ts">
  import { L } from "../i18n.svelte";
  import Icon from "./Icon.svelte";
  import { store } from "../store.svelte";
  import { api, errorText } from "../api";
  import type { EngineState, EngineView } from "../types";

  const stateText: Record<EngineState, string> = {
    ready: L("Prêt", "Ready"),
    no_adapter: L("Détecté, pas encore pris en charge", "Detected, not supported yet"),
    broken: L("Ne fonctionne pas", "Not working"),
    missing: L("Non installé", "Not installed"),
    disabled: L("Désactivé", "Disabled"),
  };

  const originText: Record<string, string> = {
    builtin: L("intégré à Chud Converter", "built into Chud Converter"),
    system: L("installé sur le système (jamais modifié par Chud Converter)", "installed on the system (never changed by Chud Converter)"),
    managed: L("installé par Chud Converter", "installed by Chud Converter"),
    custom: L("chemin choisi par vous", "path chosen by you"),
  };

  const order: EngineState[] = ["ready", "no_adapter", "broken", "disabled", "missing"];
  const sorted = $derived([...(store.engines ?? [])].sort((a, b) => order.indexOf(a.state) - order.indexOf(b.state)));
  const ready = $derived(sorted.filter((e) => e.state === "ready").length);

  let copied = $state<string | null>(null);

  async function copy(e: EngineView) {
    if (!e.hint) return;
    try {
      await navigator.clipboard.writeText(e.hint);
      copied = e.id;
      setTimeout(() => (copied = null), 1500);
    } catch {
      store.say(L("Copie impossible : sélectionnez la commande à la main.", "Copy failed: select the command by hand."));
    }
  }

  async function open(url: string) {
    try {
      await (await api).openUrl(url);
    } catch (e) {
      store.say(errorText(e));
    }
  }

  async function reveal(path: string) {
    try {
      await (await api).reveal(path);
    } catch (e) {
      store.say(errorText(e));
    }
  }
</script>

<div class="page">
  <header>
    <div>
      <h1>{L("Moteurs", "Engines")}</h1>
      <p class="muted">
        {L("Chud Converter s'appuie sur des logiciels reconnus, installés sur votre ordinateur. Il les détecte, affiche leur version et leur licence, et ne les remplace jamais sans vous le demander.", "Chud Converter relies on well-known software installed on your computer. It detects them, shows their version and license, and never replaces them without asking you.")}
      </p>
    </div>
    <button class="btn" onclick={() => store.loadEngines(true)} disabled={store.enginesLoading}>
      <Icon name="refresh" size={14} />
      {store.enginesLoading ? L("Recherche…", "Searching…") : L("Rechercher à nouveau", "Search again")}
    </button>
  </header>

  {#if store.enginesError}
    <p class="bad selectable">{store.enginesError}</p>
  {/if}

  {#if store.engines}
    <p class="muted small">{L(`${ready} moteur${ready > 1 ? "s" : ""} prêt${ready > 1 ? "s" : ""} sur ${sorted.length} connus.`, `${ready} of ${sorted.length} known engines ready.`)}</p>
    <div class="grid">
      {#each sorted as e (e.id)}
        <article class="card {e.state}">
          <div class="top">
            <h2>{e.name}</h2>
            <span class="state">{stateText[e.state]}</span>
          </div>
          <p class="desc">{e.description}</p>
          <dl>
            {#if e.version}<dt>{L("Version", "Version")}</dt><dd class="selectable">{e.version}</dd>{/if}
            {#if e.origin}<dt>{L("Origine", "Origin")}</dt><dd>{originText[e.origin] ?? e.origin}</dd>{/if}
            {#if e.path}
              <dt>{L("Emplacement", "Location")}</dt>
              <dd class="path">
                <span class="mono selectable" title={e.path}>{e.path}</span>
                <button class="icon-btn" title={L("Afficher dans le dossier", "Show in folder")} aria-label={L(`Afficher ${e.name} dans le dossier`, `Show ${e.name} in its folder`)} onclick={() => reveal(e.path!)}>
                  <Icon name="reveal" size={14} />
                </button>
              </dd>
            {/if}
            <dt>{L("Licence", "License")}</dt>
            <dd>{e.license}</dd>
          </dl>
          {#if e.problem}<p class="bad selectable">{e.problem}</p>{/if}
          {#if e.state === "no_adapter"}
            <p class="faint small">{L("Trouvé sur votre ordinateur ; son intégration arrive dans une prochaine version.", "Found on your computer; support for it comes in a future version.")}</p>
          {/if}
          {#if e.hint && e.state === "missing"}
            <div class="hint">
              <span class="small muted">{L("Pour l'installer avec le gestionnaire de paquets de votre système :", "To install it with your system's package manager:")}</span>
              <div class="cmd">
                <code>{e.hint}</code>
                <button class="icon-btn" title={L("Copier", "Copy")} aria-label={L("Copier la commande", "Copy the command")} onclick={() => copy(e)}>
                  <Icon name={copied === e.id ? "check" : "copy"} size={14} />
                </button>
              </div>
            </div>
          {/if}
          <button class="link" onclick={() => open(e.homepage)}>
            {L("Site officiel", "Official website")} <Icon name="external" size={12} />
          </button>
        </article>
      {/each}
    </div>
  {:else if store.enginesLoading}
    <p class="muted">{L("Recherche des moteurs installés…", "Looking for installed engines…")}</p>
  {/if}
</div>

<style>
  .page {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 24px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  header {
    display: flex;
    align-items: flex-start;
    gap: 20px;
    justify-content: space-between;
  }

  h1 {
    font-size: 22px;
    margin-bottom: 4px;
  }

  header p {
    max-width: 640px;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: 14px;
  }

  article {
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    border-left: 4px solid var(--border-strong);
  }

  article.ready {
    border-left-color: var(--ok);
  }

  article.no_adapter {
    border-left-color: var(--partial);
  }

  article.broken {
    border-left-color: var(--lossy);
  }

  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  h2 {
    font-size: 16px;
  }

  .state {
    font-size: 12px;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--muted);
    white-space: nowrap;
  }

  .ready .state {
    background: var(--ok-soft);
    color: var(--ok);
  }

  .no_adapter .state {
    background: var(--partial-soft);
    color: var(--partial);
  }

  .broken .state {
    background: var(--lossy-soft);
    color: var(--lossy);
  }

  .desc {
    color: var(--muted);
    font-size: 13px;
  }

  dl {
    margin: 0;
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 3px 12px;
    font-size: 12.5px;
  }

  dt {
    color: var(--faint);
  }

  dd {
    margin: 0;
    min-width: 0;
  }

  .path {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .path .mono {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11.5px;
  }

  .path .icon-btn {
    width: 22px;
    height: 22px;
    flex: none;
  }

  .hint {
    display: grid;
    gap: 4px;
  }

  .cmd {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 4px 4px 10px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    border: 1px solid var(--border);
  }

  .cmd code {
    flex: 1;
    overflow-x: auto;
    white-space: nowrap;
  }

  .link {
    align-self: flex-start;
    margin-top: auto;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    border: none;
    background: none;
    padding: 0;
    color: var(--link);
    font-size: 13px;
  }

  .link:hover {
    text-decoration: underline;
  }

  .small {
    font-size: 12.5px;
  }

  .bad {
    color: var(--lossy);
    font-size: 13px;
  }
</style>
