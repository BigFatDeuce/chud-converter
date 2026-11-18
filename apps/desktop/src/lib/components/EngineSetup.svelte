<script lang="ts">
  // Installation groupée des moteurs manquants : un bouton, une confirmation qui liste tout ce
  // qui sera téléchargé, puis les moteurs s'installent l'un après l'autre. Affiché dans
  // Convertir (masquable pour la session) et en haut de l'écran Moteurs.
  import { L, locale } from "../i18n.svelte";
  import Icon from "./Icon.svelte";
  import { store } from "../store.svelte";
  import { categoryLabel, formatSize } from "../format";
  import { engineFraction, engineStatus } from "../engineText";
  import type { Category, EngineView } from "../types";

  let { place }: { place: "convert" | "engines" } = $props();

  const bulk = $derived(store.bulkInstall);
  const ids = $derived(bulk ? bulk.ids : store.missingEngines.map((e) => e.id));
  const list = $derived(ids.map((id) => store.engines?.find((e) => e.id === id)).filter((e): e is EngineView => !!e));
  const total = $derived(list.reduce((n, e) => n + (e.installable?.size ?? 0), 0));
  const names = $derived(new Intl.ListFormat(locale(), { type: "conjunction" }).format(list.map((e) => e.name)));
  // Ce qui ne peut pas être converti sans eux (les images ont leur moteur intégré).
  const kinds = $derived(
    new Intl.ListFormat(locale(), { type: "conjunction" }).format(
      [...new Set(list.flatMap((e) => e.categories))].filter((c) => c !== "image").map((c) => categoryLabel(c as Category).toLowerCase()),
    ),
  );
  const failed = $derived(list.filter((e) => store.engineOps[e.id]?.state === "failed"));
  // Pendant l'accueil du premier démarrage, c'est lui qui présente les moteurs.
  const visible = $derived(!store.welcomeOpen && (!!bulk || (list.length > 0 && (place === "engines" || !store.setupDismissed))));

  const status = (e: EngineView) => engineStatus(e, store.engineOps[e.id]);
</script>

{#if visible}
  <section class="setup card" data-machine="engine-setup" data-state={bulk?.state ?? "idle"} aria-busy={bulk?.state === "running"} aria-label={L("Moteurs manquants", "Missing engines")}>
    {#if !bulk}
      <div class="line">
        <Icon name="download" size={18} />
        <div class="text">
          <strong>{L(`Moteurs manquants : ${names}.`, `Missing engines: ${names}.`)}</strong>
          <span class="muted small">{L(`Nécessaires pour : ${kinds}. Tout s'installe d'ici, sans terminal.`, `Needed for: ${kinds}. Everything installs from here, no terminal needed.`)}</span>
        </div>
        <button class="btn primary" onclick={() => store.askInstallAll()}>
          {L(`Tout installer (${formatSize(total)})`, `Install all (${formatSize(total)})`)}
        </button>
        {#if place === "convert"}
          <button class="icon-btn" title={L("Plus tard", "Later")} aria-label={L("Masquer jusqu'au prochain démarrage", "Hide until next start")} onclick={() => (store.setupDismissed = true)}>
            <Icon name="x" size={14} />
          </button>
        {/if}
      </div>
      {#if list.length > 1}
        <div class="one">
          <span class="faint small">{L("Ou un seul :", "Or just one:")}</span>
          {#each list as e (e.id)}
            <button class="btn ghost small" onclick={() => store.offerEngine(e.id)}>
              {e.name}{e.installable ? ` (${formatSize(e.installable.size)})` : ""}
            </button>
          {/each}
        </div>
      {/if}
      {#each failed as e (e.id)}
        {@const op = store.engineOps[e.id]}
        {#if op?.state === "failed"}<p class="bad small selectable">{e.name} : {op.error}</p>{/if}
      {/each}
    {:else}
      <p><strong>{bulk.state === "confirming" ? L("Installer ces moteurs ?", "Install these engines?") : L("Installation des moteurs…", "Installing the engines…")}</strong></p>
      <ul class="engines">
        {#each list as e (e.id)}
          {@const offer = e.installable}
          {@const op = store.engineOps[e.id]}
          <li data-engine={e.id} data-op={op?.state ?? "idle"}>
            <div class="row1">
              <span class="name">{e.name} {offer?.version ?? ""}</span>
              {#if bulk.state === "running"}
                <span class="small" class:ok={e.state === "ready" && !op} class:bad={op?.state === "failed"}>{status(e)}</span>
              {:else if offer}
                <span class="small muted">{formatSize(offer.size)}</span>
              {/if}
            </div>
            {#if bulk.state === "confirming" && offer}
              <span class="small muted">{e.description} {L(`Source : ${offer.publisher}. Licence : ${offer.license}.`, `Source: ${offer.publisher}. License: ${offer.license}.`)}</span>
            {/if}
            {#if op?.state === "running"}
              {@const f = engineFraction(op)}
              {#if f !== null}<progress max="1" value={f} aria-label={`${e.name} : ${status(e)}`}></progress>{:else}<progress aria-label={`${e.name} : ${status(e)}`}></progress>{/if}
            {:else if op?.state === "failed"}
              <span class="bad small selectable">{op.error}</span>
            {/if}
          </li>
        {/each}
      </ul>
      {#if bulk.state === "confirming"}
        <p class="small muted">
          {L(
            `${formatSize(total)} au total, depuis les publications officielles. Chaque fichier est refusé si son empreinte SHA-256 diffère de celle attendue, et chaque moteur est essayé avant d'être utilisé. Tout va dans le dossier de Chud Converter : rien n'est modifié sur le système, retrait en un clic dans Moteurs.`,
            `${formatSize(total)} in total, from the official releases. Each file is refused if its SHA-256 differs from the expected one, and each engine is tried before being used. Everything goes into Chud Converter's folder: nothing is changed on the system, removable in one click in Engines.`,
          )}
        </p>
        <div class="actions">
          <button class="btn primary" onclick={() => store.installAll()}>{L("Télécharger et installer", "Download and install")}</button>
          <button class="btn ghost" onclick={() => store.dismissInstallAll()}>{L("Annuler", "Cancel")}</button>
        </div>
      {:else}
        <div class="actions">
          <button class="btn ghost small" onclick={() => store.stopInstallAll()} disabled={bulk.stopping}>{bulk.stopping ? L("Arrêt…", "Stopping…") : L("Arrêter", "Stop")}</button>
        </div>
      {/if}
    {/if}
  </section>
{/if}

<style>
  .setup {
    padding: 12px 14px;
    display: grid;
    gap: 8px;
    border-left: 4px solid var(--link);
    min-width: 0;
  }

  .line {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
  }

  .text {
    flex: 1;
    min-width: 200px;
    display: grid;
    gap: 2px;
  }

  .one {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }

  .engines {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 8px;
  }

  .engines li {
    display: grid;
    gap: 3px;
    min-width: 0;
  }

  .row1 {
    display: flex;
    justify-content: space-between;
    gap: 10px;
  }

  .name {
    font-weight: 600;
  }

  progress {
    width: 100%;
  }

  .actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }

  .small {
    font-size: 12.5px;
  }

  .ok {
    color: var(--ok);
  }

  .bad {
    color: var(--lossy);
  }
</style>
