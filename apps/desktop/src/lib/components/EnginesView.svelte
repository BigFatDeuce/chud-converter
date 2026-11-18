<script lang="ts">
  import { L } from "../i18n.svelte";
  import Icon from "./Icon.svelte";
  import { tick } from "svelte";
  import { store } from "../store.svelte";
  import { api, errorText } from "../api";
  import { formatSize } from "../format";
  import { engineActions, installsAlongside, type EngineOp } from "../machines";
  import type { EngineState, EngineView, InstallProgress } from "../types";

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
  // Les moteurs dont Chud Converter sait se servir ; les autres sont seulement annoncés, sans
  // jamais pousser à les installer (rien ne les utiliserait encore).
  const used = $derived(sorted.filter((e) => e.supported));
  const soon = $derived(sorted.filter((e) => !e.supported));
  const ready = $derived(used.filter((e) => e.state === "ready").length);

  function progressText(e: EngineView, p: InstallProgress | null): string {
    if (!p) return L("Préparation…", "Preparing…");
    switch (p.phase) {
      case "downloading":
        return L(`Téléchargement : ${formatSize(p.received)} sur ${formatSize(p.total)}`, `Downloading: ${formatSize(p.received)} of ${formatSize(p.total)}`);
      case "verifying":
        return L("Vérification de l'empreinte SHA-256…", "Checking the SHA-256 fingerprint…");
      case "extracting":
        return p.total ? L(`Extraction : ${Math.round((p.done * 100) / p.total)} %`, `Extracting: ${Math.round((p.done * 100) / p.total)}%`) : L("Extraction…", "Extracting…");
      case "checking":
        return L(`Essai de ${e.name}…`, `Trying ${e.name}…`);
    }
  }

  function runningText(e: EngineView, op: Extract<EngineOp, { state: "running" }>): string {
    if (op.cancelling) return L("Annulation…", "Cancelling…");
    if (op.action === "uninstall") return L("Suppression…", "Removing…");
    if (op.action === "rollback") return L("Retour à la version précédente…", "Going back to the previous version…");
    return progressText(e, op.progress);
  }

  function failedTitle(op: Extract<EngineOp, { state: "failed" }>): string {
    if (op.action === "install") return L("L'installation a échoué ; rien n'a été modifié.", "The installation failed; nothing was changed.");
    if (op.action === "uninstall") return L("Le retrait a échoué.", "The removal failed.");
    return L("Le retour en arrière a échoué.", "Going back failed.");
  }

  // Arrivée depuis la liste des fichiers : la carte du moteur est montrée et sa confirmation
  // reçoit le focus (clavier, lecteurs d'écran).
  $effect(() => {
    const id = store.focusEngine;
    if (!id || !store.engines) return;
    void tick().then(() => {
      const card = document.querySelector<HTMLElement>(`[data-engine="${CSS.escape(id)}"]`);
      card?.scrollIntoView({ block: "center" });
      card?.querySelector<HTMLButtonElement>(".op.confirm .btn.primary")?.focus();
      store.focusEngine = null;
    });
  });

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
        {L("Chud Converter s'appuie sur des logiciels reconnus. Il détecte ceux déjà présents sur votre ordinateur et ne les modifie jamais ; FFmpeg et 7-Zip peuvent aussi être installés ici en un clic, depuis leur source officielle, vérifiés avant d'être utilisés.", "Chud Converter relies on well-known software. It detects the ones already on your computer and never changes them; FFmpeg and 7-Zip can also be installed here in one click, from their official source, checked before being used.")}
      </p>
    </div>
    <button class="btn" onclick={() => store.loadEngines(true)} disabled={store.enginesLoad.state === "loading"}>
      <Icon name="refresh" size={14} />
      {store.enginesLoad.state === "loading" ? L("Recherche…", "Searching…") : L("Rechercher à nouveau", "Search again")}
    </button>
  </header>

  {#if store.enginesLoad.state === "failed"}
    <p class="bad selectable">{store.enginesLoad.error}</p>
  {/if}

  {#if store.engines}
    <p class="muted small">{L(`${ready} moteur${ready > 1 ? "s" : ""} prêt${ready > 1 ? "s" : ""} sur ${used.length}.`, `${ready} of ${used.length} engines ready.`)}</p>
    <div class="grid">
      {#each used as e (e.id)}
        {@const op = store.engineOps[e.id] ?? null}
        {@const actions = engineActions(e)}
        {@const offer = e.installable}
        <article class="card {e.state}" data-engine={e.id} data-state={e.state} data-op={op?.state ?? "idle"} aria-busy={op?.state === "running"}>
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
          {#if op?.state === "confirming"}
            <div class="op confirm" role="group" aria-label={L("Confirmation", "Confirmation")}>
              {#if op.action === "install" && offer}
                <p>
                  <strong>{L(`Installer ${e.name} ${offer.version} ?`, `Install ${e.name} ${offer.version}?`)}</strong>
                </p>
                <ul class="small">
                  {#if installsAlongside(e)}
                    <li class="strong">
                      {L(
                        `Votre ${e.name} ${e.version ?? ""} actuel n'est pas modifié ; la copie de Chud Converter sera utilisée en priorité. Retirez-la pour revenir à celui du système.`,
                        `Your current ${e.name} ${e.version ?? ""} is not changed; Chud Converter's copy will be used first. Remove it to go back to the system one.`,
                      )}
                    </li>
                  {/if}
                  <li>{L(`Téléchargement de ${formatSize(offer.size)} depuis : ${offer.publisher}.`, `Downloads ${formatSize(offer.size)} from: ${offer.publisher}.`)}</li>
                  <li>{L(`Licence : ${offer.license}.`, `License: ${offer.license}.`)}</li>
                  <li>{L("Fichier refusé si son empreinte SHA-256 diffère de celle attendue ; moteur essayé avant d'être utilisé.", "The file is refused if its SHA-256 differs from the expected one; the engine is tried before being used.")}</li>
                  <li>{L("Installé dans le dossier de Chud Converter : rien n'est modifié sur le système, retrait en un clic.", "Installed in Chud Converter's folder: nothing is changed on the system, removable in one click.")}</li>
                </ul>
                <button class="link small" onclick={() => open(offer.page)}>{L("Voir la publication d'origine", "See the original release")} <Icon name="external" size={12} /></button>
              {:else if op.action === "uninstall"}
                <p><strong>{L(`Retirer ${e.name} ${e.version ?? ""} ?`, `Remove ${e.name} ${e.version ?? ""}?`)}</strong></p>
                <p class="small muted">{L("Seuls les fichiers installés par Chud Converter sont supprimés ; vos fichiers convertis et les installations du système ne sont pas touchés.", "Only the files installed by Chud Converter are deleted; your converted files and system installs are not touched.")}</p>
              {:else}
                <p><strong>{L(`Revenir à ${e.name} ${e.previous ?? ""} ?`, `Go back to ${e.name} ${e.previous ?? ""}?`)}</strong></p>
                <p class="small muted">{L(`La version ${e.version ?? ""} reste sur le disque : vous pourrez y revenir.`, `Version ${e.version ?? ""} stays on disk: you can go back to it.`)}</p>
              {/if}
              <div class="row">
                <button class="btn {op.action === 'uninstall' ? 'danger' : 'primary'}" onclick={() => store.confirmEngine(e.id)}>
                  {op.action === "install"
                    ? L("Télécharger et installer", "Download and install")
                    : op.action === "uninstall"
                      ? L("Retirer", "Remove")
                      : L("Revenir à cette version", "Go back to this version")}
                </button>
                <button class="btn ghost" onclick={() => store.dismissEngine(e.id)}>{L("Annuler", "Cancel")}</button>
              </div>
            </div>
          {:else if op?.state === "running"}
            {@const fraction =
              op.progress?.phase === "downloading"
                ? op.progress.received / Math.max(1, op.progress.total)
                : op.progress?.phase === "extracting" && op.progress.total
                  ? op.progress.done / op.progress.total
                  : null}
            <div class="op running" aria-live="polite">
              <span class="small">{runningText(e, op)}</span>
              {#if fraction !== null}
                <progress max="1" value={fraction} aria-label={runningText(e, op)}></progress>
              {:else}
                <progress aria-label={L(`${e.name} : opération en cours`, `${e.name}: operation in progress`)}></progress>
              {/if}
              {#if op.action === "install"}
                <button class="btn ghost small small-btn" onclick={() => store.cancelEngine(e.id)} disabled={op.cancelling}>{L("Annuler l'installation", "Cancel the installation")}</button>
              {/if}
            </div>
          {:else if op?.state === "failed"}
            <div class="op failed" role="alert">
              <p class="small"><strong>{failedTitle(op)}</strong></p>
              <p class="bad selectable">{op.error}</p>
              <div class="row">
                <button class="btn" onclick={() => store.askEngine(e.id, op.action)}>{L("Réessayer", "Try again")}</button>
                <button class="btn ghost" onclick={() => store.dismissEngine(e.id)}>{L("Fermer", "Close")}</button>
              </div>
            </div>
          {:else if actions.length}
            <div class="row">
              {#if actions.includes("install") && offer}
                {#if installsAlongside(e)}
                  <!-- Le moteur du système fonctionne : proposition discrète, jamais mise en avant. -->
                  <button class="btn ghost small" onclick={() => store.askEngine(e.id, "install")}>
                    <Icon name="download" size={13} />
                    {L(`Installer aussi la version ${offer.version} de Chud Converter`, `Also install Chud Converter's version ${offer.version}`)}
                  </button>
                {:else}
                  <button class="btn primary" onclick={() => store.askEngine(e.id, "install")}>
                    <Icon name="download" size={14} />
                    {e.origin === "managed"
                      ? L(`Mettre à jour vers ${offer.version}`, `Update to ${offer.version}`)
                      : L(`Installer (${formatSize(offer.size)})`, `Install (${formatSize(offer.size)})`)}
                  </button>
                {/if}
              {/if}
              {#if actions.includes("rollback")}
                <button class="btn" onclick={() => store.askEngine(e.id, "rollback")}>{L(`Revenir à ${e.previous}`, `Go back to ${e.previous}`)}</button>
              {/if}
              {#if actions.includes("uninstall")}
                <button class="btn ghost" onclick={() => store.askEngine(e.id, "uninstall")}>{L("Retirer", "Remove")}</button>
              {/if}
            </div>
          {/if}
          {#if !offer && (e.state === "missing" || e.state === "broken") && !op}
            <p class="faint small">
              {L(
                "Chud Converter ne sait pas encore l'installer lui-même sur ce système : téléchargez-le depuis son site officiel, puis « Rechercher à nouveau ».",
                "Chud Converter cannot install it by itself on this system yet: download it from its official website, then “Search again”.",
              )}
            </p>
          {/if}
          <button class="link" onclick={() => open(e.homepage)}>
            {L("Site officiel", "Official website")} <Icon name="external" size={12} />
          </button>
        </article>
      {/each}
    </div>
    {#if soon.length}
      <section class="soon" aria-labelledby="soon-title">
        <h2 id="soon-title">{L("Bientôt pris en charge", "Coming soon")}</h2>
        <p class="muted small">
          {L(
            "Chud Converter ne sait pas encore se servir de ces logiciels : il n'y a rien à installer pour l'instant. Ceux déjà présents sur votre ordinateur sont repérés et serviront dès qu'une prochaine version les prendra en charge.",
            "Chud Converter cannot use these programs yet: there is nothing to install for now. Those already on your computer are noted and will be used as soon as a future version supports them.",
          )}
        </p>
        <ul>
          {#each soon as e (e.id)}
            <li data-engine={e.id} data-state={e.state}>
              <span class="name">{e.name}</span>
              <span class="faint small desc-soon">{e.description}</span>
              {#if e.state !== "missing"}
                <span class="found small">{L("Trouvé sur votre ordinateur", "Found on your computer")}{e.version ? ` (${e.version})` : ""}</span>
              {/if}
              <button class="link small" aria-label={L(`Site officiel de ${e.name}`, `${e.name} official website`)} onclick={() => open(e.homepage)}>
                {L("Site officiel", "Official website")} <Icon name="external" size={12} />
              </button>
            </li>
          {/each}
        </ul>
      </section>
    {/if}
  {:else if store.enginesLoad.state === "loading"}
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
    min-width: 0;
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

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }

  .op {
    display: grid;
    gap: 8px;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    border: 1px solid var(--border);
  }

  .op li.strong {
    color: var(--text);
  }

  .op ul {
    margin: 0;
    padding-left: 18px;
    display: grid;
    gap: 2px;
    color: var(--muted);
  }

  .op progress {
    width: 100%;
  }

  .op.failed {
    border-color: var(--lossy);
  }

  .small-btn {
    justify-self: start;
  }

  .soon {
    display: grid;
    gap: 6px;
    margin-top: 6px;
  }

  .soon ul {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 2px;
  }

  .soon li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 10px;
    padding: 6px 2px;
    border-bottom: 1px solid var(--border);
    min-width: 0;
  }

  .soon .name {
    font-weight: 600;
    font-size: 13.5px;
  }

  .soon .desc-soon {
    flex: 1;
    min-width: 160px;
  }

  .soon .found {
    color: var(--partial);
  }

  .soon .link {
    margin-top: 0;
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
