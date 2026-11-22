<script lang="ts">
  import { L, locale, type LangChoice } from "../i18n.svelte";
  import { formatSize } from "../format";
  import Icon from "./Icon.svelte";
  import PageHeader from "./ui/PageHeader.svelte";
  import Segmented from "./ui/Segmented.svelte";
  import Callout from "./ui/Callout.svelte";
  import ProgressBar from "./ui/ProgressBar.svelte";
  import { ui } from "../ui.svelte";
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
  const flow = $derived(store.updateFlow);
  const busy = $derived(flow.state === "checking" || flow.state === "installing");
  const p = $derived(flow.state === "installing" ? flow.progress : null);

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
  <div class="page-inner">
    <PageHeader title={L("Réglages", "Settings")} eyebrow={L("Préférences", "Preferences")} icon="settings">
      {L("Langue, apparence et mises à jour de l'application.", "Language, appearance and updates of the app.")}
    </PageHeader>

    <section class="panel sect enter" aria-labelledby="lang-title">
      <div class="sect-head">
        <span class="sect-ico"><Icon name="globe" size={17} /></span>
        <div>
          <h2 id="lang-title">{L("Langue", "Language")}</h2>
          <p class="muted small">
            {L(
              "Langue de l'application, des rapports de conversion et du menu clic droit. Elle est partagée avec la ligne de commande (chud language).",
              "Language of the app, the conversion reports and the right-click menu. It is shared with the command line (chud language).",
            )}
          </p>
        </div>
      </div>

      {#if s?.error}
        <Callout tone="danger" compact>
          <p class="small selectable">
            {L("Le fichier de réglages est illisible ; il n'est pas modifié tant que vous ne l'avez pas corrigé :", "The settings file cannot be read; it is not changed until you fix it:")}
            {s.error}
          </p>
        </Callout>
      {/if}

      <Segmented
        name="language"
        label={L("Langue", "Language")}
        value={s?.language ?? null}
        options={choices.map((c) => ({ value: c.value, label: c.label, hint: c.hint, icon: c.value === "system" ? ("monitor" as const) : undefined }))}
        disabled={!s || !!s.error || store.settingsBusy}
        onchange={(v) => store.setLanguage(v)}
      />

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

    <section class="panel sect enter" aria-labelledby="look-title" data-machine="appearance" data-theme-choice={ui.theme} data-motion-choice={ui.motion}>
      <div class="sect-head">
        <span class="sect-ico"><Icon name="palette" size={17} /></span>
        <div>
          <h2 id="look-title">{L("Apparence", "Appearance")}</h2>
          <p class="muted small">{L("Propre à cet ordinateur, appliqué à toutes les fenêtres de Chud Converter.", "Specific to this computer, applied to every Chud Converter window.")}</p>
        </div>
      </div>

      <div class="setting">
        <div class="setting-text">
          <span class="setting-label">{L("Thème", "Theme")}</span>
          <span class="faint small">{L("L'espace profond en sombre, l'aube en clair.", "Deep space when dark, dawn when light.")}</span>
        </div>
        <Segmented
          name="theme"
          label={L("Thème", "Theme")}
          value={ui.theme}
          options={[
            { value: "system", label: L("Système", "System"), icon: "monitor" },
            { value: "dark", label: L("Sombre", "Dark"), icon: "moon" },
            { value: "light", label: L("Clair", "Light"), icon: "sun" },
          ]}
          onchange={(v) => ui.setTheme(v)}
        />
      </div>

      <div class="setting">
        <div class="setting-text">
          <span class="setting-label">{L("Animations", "Animations")}</span>
          <span class="faint small">
            {ui.motion === "auto"
              ? ui.systemReduced
                ? L("Le système demande moins de mouvement : animations réduites.", "The system asks for less motion: animations reduced.")
                : L("Fond animé et transitions fluides ; réduites si le système le demande.", "Animated background and smooth transitions; reduced if the system asks.")
              : ui.motion === "reduced"
                ? L("Fond immobile, seulement de courts fondus.", "Still background, only short fades.")
                : L("Rien ne bouge : ni fond, ni transitions, ni effets.", "Nothing moves: no background, transitions or effects.")}
          </span>
        </div>
        <Segmented
          name="motion"
          label={L("Animations", "Animations")}
          value={ui.motion}
          options={[
            { value: "auto", label: L("Automatiques", "Automatic"), icon: "sparkle" },
            { value: "reduced", label: L("Réduites", "Reduced"), icon: "orbit" },
            { value: "off", label: L("Désactivées", "Off"), icon: "stop" },
          ]}
          onchange={(v) => ui.setMotion(v)}
        />
      </div>
    </section>

    <section class="panel sect enter" data-machine="update" data-state={flow.state} aria-busy={busy} aria-labelledby="upd-title">
      <div class="sect-head">
        <span class="sect-ico"><Icon name="refresh" size={17} /></span>
        <div>
          <h2 id="upd-title">{L("Mises à jour", "Updates")}</h2>
          <p class="muted small">
            {L(
              "Seul le fichier de la dernière version publiée est lu sur GitHub : aucun de vos fichiers ni de leurs noms n'est envoyé. Chaque mise à jour est signée ; une mise à jour dont la signature ne correspond pas est refusée.",
              "Only the description of the latest published version is read from GitHub: none of your files or their names are sent. Every update is signed; an update whose signature does not match is refused.",
            )}
          </p>
        </div>
      </div>

      {#if u}
        <div class="status">
          <span class="version">{L("Version installée", "Installed version")} <strong class="mono">{u.current}</strong></span>
          <span class="faint small">{L("Dernière recherche :", "Last check:")} {when(u.lastCheck)}</span>
        </div>

        {#if u.available}
          <div class="new-version pop">
            <div class="nv-head">
              <span class="star" aria-hidden="true"><Icon name="sparkle" size={15} /></span>
              <strong>{L(`Version ${u.available.version} disponible`, `Version ${u.available.version} available`)}</strong>
              {#if u.available.date}<span class="faint small">· {day(u.available.date)}</span>{/if}
            </div>
            {#if u.available.notes}<p class="notes small selectable">{u.available.notes}</p>{/if}
            <div class="row">
              {#if u.canInstall}
                <button class="btn primary" disabled={busy} onclick={() => store.installUpdate()}>
                  <Icon name="download" size={14} />
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
            {#if flow.state === "installing"}
              <div class="progress small">
                <span>{progressText}</span>
                <ProgressBar value={fraction} label={progressText} />
              </div>
            {/if}
          </div>
        {:else if u.lastCheck && flow.state !== "checking"}
          <p class="small ok"><Icon name="check" size={14} /> {L("Vous avez la dernière version.", "You have the latest version.")}</p>
        {/if}

        {#if flow.state === "failed"}
          <Callout tone="danger" compact role="alert"><p class="small selectable">{flow.error}</p></Callout>
        {/if}
        {#if u.settingsError}
          <Callout tone="danger" compact><p class="small selectable">{u.settingsError}</p></Callout>
        {/if}

        <div class="row">
          <button class="btn" disabled={busy} onclick={() => store.checkUpdate()}>
            <span class:turning={flow.state === "checking"}><Icon name="refresh" size={14} /></span>
            {flow.state === "checking" ? L("Recherche…", "Checking…") : L("Rechercher maintenant", "Check now")}
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
</div>

<style>
  .page-inner {
    max-width: 860px;
  }

  .sect {
    padding: 20px 22px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .sect-head {
    display: flex;
    gap: 12px;
    align-items: flex-start;
  }

  .sect-head > div {
    display: grid;
    gap: 3px;
  }

  .sect-ico {
    display: grid;
    place-items: center;
    flex: none;
    width: 34px;
    height: 34px;
    border-radius: 10px;
    color: var(--accent);
    background: var(--accent-soft);
  }

  h2 {
    font-size: 16px;
  }

  .setting {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 10px 20px;
    padding-top: 14px;
    border-top: 1px solid var(--hairline);
  }

  .setting-text {
    display: grid;
    gap: 2px;
    flex: 1 1 220px;
    min-width: 0;
  }

  .setting-label {
    font-weight: 600;
  }

  .status {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 16px;
    align-items: baseline;
  }

  .version strong {
    margin-left: 4px;
    padding: 2px 8px;
    border-radius: 6px;
    background: var(--surface-3);
    font-size: 12.5px;
  }

  .new-version {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px 16px;
    border-radius: var(--radius);
    border: 1px solid color-mix(in srgb, var(--gold) 45%, transparent);
    background: linear-gradient(120deg, var(--gold-soft), transparent 70%), var(--surface-2);
  }

  .nv-head {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .star {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 8px;
    color: var(--gold);
    background: var(--gold-soft);
  }

  .notes {
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
    gap: 9px;
    cursor: pointer;
  }

  .ok {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--ok);
    font-weight: 550;
  }

  .progress {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .turning {
    display: inline-grid;
    animation: spin 1s linear infinite;
  }

  .file {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
</style>
