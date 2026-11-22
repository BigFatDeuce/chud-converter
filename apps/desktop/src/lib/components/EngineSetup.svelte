<script lang="ts">
  // Installation groupée des moteurs manquants : un bouton, une confirmation qui liste tout ce
  // qui sera téléchargé, puis les moteurs s'installent l'un après l'autre. Affiché dans
  // Convertir (masquable pour la session) et en haut de l'écran Moteurs.
  import { L, locale } from "../i18n.svelte";
  import Icon from "./Icon.svelte";
  import ProgressBar from "./ui/ProgressBar.svelte";
  import { store } from "../store.svelte";
  import { categoryLabel, formatSize } from "../format";
  import { engineFraction, engineStatus, offerSize, offerSystemNotes } from "../engineText";
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
  const touchesSystem = $derived(list.some((e) => e.installable?.system || e.installable?.runtime));
  const failed = $derived(list.filter((e) => store.engineOps[e.id]?.state === "failed"));
  // Pendant l'accueil du premier démarrage, c'est lui qui présente les moteurs.
  const visible = $derived(!store.welcomeOpen && (!!bulk || (list.length > 0 && (place === "engines" || !store.setupDismissed))));
  const done = $derived(bulk ? list.filter((e) => e.state === "ready" && !store.engineOps[e.id]).length : 0);

  const status = (e: EngineView) => engineStatus(e, store.engineOps[e.id]);
</script>

{#if visible}
  <section class="setup panel enter" data-machine="engine-setup" data-state={bulk?.state ?? "idle"} aria-busy={bulk?.state === "running"} aria-label={L("Moteurs manquants", "Missing engines")}>
    {#if !bulk}
      <div class="line">
        <span class="glyph"><Icon name="download" size={18} /></span>
        <div class="text">
          <strong>
            {list.length <= 3
              ? L(`Moteurs manquants : ${names}.`, `Missing engines: ${names}.`)
              : L(`${list.length} moteurs manquants`, `${list.length} missing engines`)}
          </strong>
          <span class="muted small">{L(`Nécessaires pour : ${kinds}. Tout s'installe d'ici, sans terminal.`, `Needed for: ${kinds}. Everything installs from here, no terminal needed.`)}</span>
        </div>
        <button class="btn primary" onclick={() => store.askInstallAll()}>
          <Icon name="download" size={14} />
          {total ? L(`Tout installer (${formatSize(total)})`, `Install all (${formatSize(total)})`) : L("Tout installer", "Install all")}
        </button>
        {#if place === "convert"}
          <button class="icon-btn dismiss" title={L("Plus tard", "Later")} aria-label={L("Masquer jusqu'au prochain démarrage", "Hide until next start")} onclick={() => (store.setupDismissed = true)}>
            <Icon name="x" size={14} />
          </button>
        {/if}
      </div>
      {#if list.length > 1}
        <div class="one">
          <span class="faint small">{L("Ou un seul :", "Or just one:")}</span>
          {#each list as e (e.id)}
            <button class="chip" onclick={() => store.offerEngine(e.id)}>
              {e.name}{#if e.installable}<span class="size">{offerSize(e.installable)}</span>{/if}
            </button>
          {/each}
        </div>
      {/if}
      {#each failed as e (e.id)}
        {@const op = store.engineOps[e.id]}
        {#if op?.state === "failed"}<p class="bad small selectable">{e.name} : {op.error}</p>{/if}
      {/each}
    {:else}
      <div class="line">
        <span class="glyph" class:busy={bulk.state === "running"}><Icon name="download" size={18} /></span>
        <div class="text">
          <strong>{bulk.state === "confirming" ? L("Installer ces moteurs ?", "Install these engines?") : L("Installation des moteurs…", "Installing the engines…")}</strong>
          {#if bulk.state === "running"}
            <span class="muted small">{L(`${done} sur ${list.length} prêts`, `${done} of ${list.length} ready`)}</span>
          {/if}
        </div>
      </div>
      <ul class="engines">
        {#each list as e (e.id)}
          {@const offer = e.installable}
          {@const op = store.engineOps[e.id]}
          <li data-engine={e.id} data-op={op?.state ?? "idle"} class:ready={e.state === "ready" && !op}>
            <div class="row1">
              <span class="name">{e.name} <span class="faint">{offer?.version ?? ""}</span></span>
              {#if bulk.state === "running"}
                <span class="small" class:ok={e.state === "ready" && !op} class:bad={op?.state === "failed"}>{status(e)}</span>
              {:else if offer}
                <span class="small muted">{offerSize(offer)}</span>
              {/if}
            </div>
            {#if bulk.state === "confirming" && offer}
              <span class="small muted">{e.description} {L(`Source : ${offer.publisher}. Licence : ${offer.license}.`, `Source: ${offer.publisher}. License: ${offer.license}.`)}</span>
              {#each offerSystemNotes(offer) as n (n)}<span class="small note">{n}</span>{/each}
            {/if}
            {#if op?.state === "running"}
              <ProgressBar thin value={engineFraction(op)} label={`${e.name} : ${status(e)}`} />
            {:else if op?.state === "failed"}
              <span class="bad small selectable">{op.error}</span>
            {/if}
          </li>
        {/each}
      </ul>
      {#if bulk.state === "confirming"}
        <p class="small muted">
          {L(
            `${formatSize(total)} au total, depuis les publications officielles. Chaque fichier est refusé si son empreinte SHA-256 diffère de celle attendue, et chaque moteur est essayé avant d'être utilisé. ${touchesSystem ? "Le reste va dans le dossier de Chud Converter, retrait en un clic dans Moteurs." : "Tout va dans le dossier de Chud Converter : rien n'est modifié sur le système, retrait en un clic dans Moteurs."}`,
            `${formatSize(total)} in total, from the official releases. Each file is refused if its SHA-256 differs from the expected one, and each engine is tried before being used. ${touchesSystem ? "The rest goes into Chud Converter's folder, removable in one click in Engines." : "Everything goes into Chud Converter's folder: nothing is changed on the system, removable in one click in Engines."}`,
          )}
        </p>
        <div class="actions">
          <button class="btn primary" onclick={() => store.installAll()}><Icon name="download" size={14} /> {L("Télécharger et installer", "Download and install")}</button>
          <button class="btn ghost" onclick={() => store.dismissInstallAll()}>{L("Annuler", "Cancel")}</button>
        </div>
      {:else}
        <div class="actions">
          <button class="btn ghost small" onclick={() => store.stopInstallAll()} disabled={bulk.stopping}><Icon name="stop" size={12} /> {bulk.stopping ? L("Arrêt…", "Stopping…") : L("Arrêter", "Stop")}</button>
        </div>
      {/if}
    {/if}
  </section>
{/if}

<style>
  .setup {
    position: relative;
    padding: 14px 16px;
    display: grid;
    gap: 12px;
    min-width: 0;
    overflow: hidden;
    background: linear-gradient(120deg, var(--accent-soft), transparent 55%), var(--surface);
    border-color: color-mix(in srgb, var(--accent) 30%, transparent);
  }

  .line {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
  }

  .setup:has(.dismiss) .line {
    padding-right: 30px;
  }

  .dismiss {
    position: absolute;
    top: 8px;
    right: 8px;
  }

  .glyph {
    flex: none;
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    border-radius: 12px;
    color: #fff;
    background: var(--grad);
    box-shadow: var(--glow);
  }

  .glyph.busy {
    animation: pulse 1.6s ease-in-out infinite;
  }

  .text {
    flex: 1;
    min-width: 220px;
    display: grid;
    gap: 2px;
  }

  .one {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    padding-left: 50px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 10px;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: var(--surface-2);
    color: var(--text);
    font-size: 12px;
    font-weight: 550;
    transition:
      border-color var(--t-fast) var(--ease),
      background var(--t-fast) var(--ease);
  }

  .chip:hover {
    border-color: var(--accent);
    background: var(--surface-hover);
  }

  .size {
    color: var(--faint);
    font-weight: 400;
  }

  .engines {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 8px;
  }

  .engines li {
    display: grid;
    gap: 5px;
    align-content: start;
    min-width: 0;
    padding: 10px 12px;
    border-radius: 10px;
    border: 1px solid var(--hairline);
    background: var(--surface-2);
  }

  .engines li.ready {
    border-color: color-mix(in srgb, var(--ok) 35%, transparent);
  }

  .row1 {
    display: flex;
    justify-content: space-between;
    gap: 10px;
  }

  .name {
    font-weight: 600;
  }

  .note {
    color: var(--text);
  }

  .actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }

  .ok {
    color: var(--ok);
  }

  .bad {
    color: var(--lossy);
  }

  @media (max-width: 1100px) {
    .one {
      padding-left: 0;
    }
  }
</style>
