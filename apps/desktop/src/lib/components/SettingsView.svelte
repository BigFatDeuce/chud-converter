<script lang="ts">
  import { L, locale, type LangChoice } from "../i18n.svelte";
  import { formatSize } from "../format";
  import Icon from "./Icon.svelte";
  import { store } from "../store.svelte";
  import { api, errorText } from "../api";

  const s = $derived(store.settings);
  // Chaque langue est nommée dans sa propre langue, comme partout ailleurs.
  const native = { fr: "Français", en: "English" } as const;
  const choices = $derived<{ value: LangChoice; label: string; hint?: string }[]>([
    {
      value: "system",
      label: L("Langue du système", "System language"),
      hint: s ? native[s.system] : undefined,
    },
    { value: "fr", label: native.fr },
    { value: "en", label: native.en },
  ]);

  const u = $derived(store.update);
  const p = $derived(store.updateProgress);

  function when(secs: number | null): string {
    if (!secs) return L("jamais", "never");
    return new Date(secs * 1000).toLocaleString(locale(), { dateStyle: "medium", timeStyle: "short" });
  }
  function day(iso: string | null): string {
    if (!iso) return "";
    const d = new Date(iso);
    return isNaN(d.getTime()) ? "" : d.toLocaleDateString(locale(), { dateStyle: "long" });
  }
  const progressText = $derived.by(() => {
    if (!p) return L("Préparation…", "Preparing…");
    if (p.phase === "downloading")
      return p.total
        ? L(`Téléchargement : ${formatSize(p.received)} sur ${formatSize(p.total)}`, `Downloading: ${formatSize(p.received)} of ${formatSize(p.total)}`)
        : L(`Téléchargement : ${formatSize(p.received)}`, `Downloading: ${formatSize(p.received)}`);
    if (p.phase === "verified") return L("Signature vérifiée", "Signature verified");
    return L("Installation… l'application va redémarrer", "Installing… the app will restart");
  });
  const fraction = $derived(p?.phase === "downloading" && p.total ? p.received / p.total : null);

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
    <h1>{L("Réglages", "Settings")}</h1>
  </header>

  <section class="card">
    <div class="card-head">
      <Icon name="globe" size={18} />
      <h2>{L("Langue", "Language")}</h2>
    </div>
    <p class="muted small">
      {L(
        "Langue de l'application, des rapports de conversion et du menu clic droit. Elle est partagée avec la ligne de commande (chud language).",
        "Language of the app, the conversion reports and the right-click menu. It is shared with the command line (chud language).",
      )}
    </p>

    {#if s?.error}
      <p class="error small selectable">
        <Icon name="alert" size={14} />
        {L("Le fichier de réglages est illisible ; il n'est pas modifié tant que vous ne l'avez pas corrigé :", "The settings file cannot be read; it is not changed until you fix it:")}
        {s.error}
      </p>
    {/if}

    <div class="choices" role="radiogroup" aria-label={L("Langue", "Language")}>
      {#each choices as c (c.value)}
        <label class="choice" class:selected={s?.language === c.value}>
          <input
            type="radio"
            name="language"
            value={c.value}
            checked={s?.language === c.value}
            disabled={!s || !!s.error || store.settingsBusy}
            onchange={() => store.setLanguage(c.value)}
          />
          <span>{c.label}</span>
          {#if c.hint}<span class="faint small">· {c.hint}</span>{/if}
        </label>
      {/each}
    </div>

    {#if s}
      <div class="file faint small">
        <span>{L("Enregistré dans", "Saved in")}</span>
        <span class="mono selectable">{s.file}</span>
        <button class="icon-btn" title={L("Afficher dans le dossier", "Show in folder")} aria-label={L("Afficher le fichier de réglages dans le dossier", "Show the settings file in its folder")} onclick={() => reveal(s.file)}>
          <Icon name="reveal" size={14} />
        </button>
      </div>
    {/if}
  </section>

  <section class="card">
    <div class="card-head">
      <Icon name="refresh" size={18} />
      <h2>{L("Mises à jour", "Updates")}</h2>
    </div>
    <p class="muted small">
      {L(
        "Seul le fichier de la dernière version publiée est lu sur GitHub : aucun de vos fichiers ni de leurs noms n'est envoyé. Chaque mise à jour est signée ; une mise à jour dont la signature ne correspond pas est refusée.",
        "Only the description of the latest published version is read from GitHub: none of your files or their names are sent. Every update is signed; an update whose signature does not match is refused.",
      )}
    </p>

    {#if u}
      <div class="status">
        <span>{L("Version installée", "Installed version")} <strong class="mono">{u.current}</strong></span>
        <span class="faint small">{L("Dernière recherche :", "Last check:")} {when(u.lastCheck)}</span>
      </div>

      {#if u.available}
        <div class="new-version">
          <div class="nv-head">
            <Icon name="check" size={16} />
            <strong>{L(`Version ${u.available.version} disponible`, `Version ${u.available.version} available`)}</strong>
            {#if u.available.date}<span class="faint small">· {day(u.available.date)}</span>{/if}
          </div>
          {#if u.available.notes}<p class="notes small selectable">{u.available.notes}</p>{/if}
          <div class="row">
            {#if u.canInstall}
              <button class="btn primary" disabled={!!store.updateBusy} onclick={() => store.installUpdate()}>
                {L("Installer et redémarrer", "Install and restart")}
              </button>
            {/if}
            <button class="btn ghost" onclick={() => open(u.available!.page)}>
              <Icon name="external" size={14} />
              {u.canInstall ? L("Notes de version", "Release notes") : L("Page de téléchargement", "Download page")}
            </button>
          </div>
          {#if !u.canInstall && u.manualReason}
            <p class="faint small">{L("Installation manuelle :", "Manual installation:")} {u.manualReason}.</p>
          {/if}
          {#if store.updateBusy === "install"}
            <div class="progress small">
              <span>{progressText}</span>
              {#if fraction !== null}<progress max="1" value={fraction}></progress>{/if}
            </div>
          {/if}
        </div>
      {:else if u.lastCheck && store.updateBusy !== "check"}
        <p class="small ok"><Icon name="check" size={14} /> {L("Vous avez la dernière version.", "You have the latest version.")}</p>
      {/if}

      {#if store.updateError}
        <p class="error small selectable"><Icon name="alert" size={14} /> {store.updateError}</p>
      {/if}
      {#if u.settingsError}
        <p class="error small selectable"><Icon name="alert" size={14} /> {u.settingsError}</p>
      {/if}

      <div class="row">
        <button class="btn ghost" disabled={!!store.updateBusy} onclick={() => store.checkUpdate()}>
          <Icon name="refresh" size={14} />
          {store.updateBusy === "check" ? L("Recherche…", "Checking…") : L("Rechercher maintenant", "Check now")}
        </button>
        <label class="auto">
          <input type="checkbox" checked={u.autoCheck} disabled={!!u.settingsError} onchange={(e) => store.setAutoUpdate(e.currentTarget.checked)} />
          {L("Rechercher automatiquement (au plus une fois par jour, au démarrage)", "Check automatically (at most once a day, at startup)")}
        </label>
      </div>
      <p class="faint small">
        {L("Pour revenir à une version précédente, installez-la depuis", "To go back to an earlier version, install it from")}
        <button class="link" onclick={() => open(u.releases)}>{L("la liste des versions", "the list of versions")}</button>.
      </p>
    {/if}
  </section>
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
  h1 {
    font-size: 22px;
    margin: 0 0 4px;
  }
  .card {
    max-width: 760px;
  }
  .card {
    padding: 18px 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .card-head {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .card-head h2 {
    margin: 0;
    font-size: 15px;
  }
  .card p {
    margin: 0;
  }
  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .choice {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 9px 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .choice.selected {
    border-color: var(--primary);
    background: var(--primary-soft);
  }
  .error {
    display: flex;
    gap: 6px;
    align-items: baseline;
    color: var(--lossy);
  }
  .status {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 16px;
    align-items: baseline;
  }
  .new-version {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 14px;
    border: 1px solid var(--ok);
    background: var(--ok-soft);
    border-radius: var(--radius-sm);
  }
  .nv-head {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .notes {
    margin: 0;
    white-space: pre-line;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px 16px;
  }
  .auto {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
  }
  .ok {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--ok);
  }
  .progress {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .progress progress {
    width: 100%;
  }
  .link {
    background: none;
    border: none;
    padding: 0;
    color: var(--link);
    cursor: pointer;
    text-decoration: underline;
    font: inherit;
  }
  .file {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
</style>
