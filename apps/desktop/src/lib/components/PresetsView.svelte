<script lang="ts">
  import { L } from "../i18n.svelte";
  import { onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import { store } from "../store.svelte";
  import { api, errorText } from "../api";
  import { categoryLabel } from "../format";
  import type { Category, Conflict, PresetView } from "../types";

  // Sortes proposées à l'édition : celles où un préréglage a un sens.
  const KINDS: { kind: string; label: string }[] = [
    ...(["video", "audio", "image", "subtitle", "archive", "data", "document", "spreadsheet", "presentation", "pdf", "ebook"] as Category[]).map(
      (c) => ({ kind: c, label: categoryLabel(c) }),
    ),
    { kind: "folder", label: L("Dossiers", "Folders") },
  ];
  const kindLabel = (k: string) => KINDS.find((x) => x.kind === k)?.label ?? k;

  const conflictText: Record<Conflict, string> = {
    rename: L("garder les deux", "keep both"),
    skip: L("ne rien faire si le résultat existe", "do nothing if the result exists"),
    overwrite: L("remplacer l'ancien résultat", "replace the old result"),
  };

  let editing = $state<string | null>(null);
  let draft = $state<PresetView | null>(null);
  let deleting = $state<string | null>(null);
  let confirmReset = $state(false);
  let busy = $state(false);

  function edit(p: PresetView) {
    editing = p.id;
    draft = structuredClone($state.snapshot(p));
    deleting = null;
  }

  async function save(list: PresetView[]) {
    busy = true;
    const ok = await store.savePresets(list);
    busy = false;
    return ok;
  }

  async function saveDraft() {
    if (!draft) return;
    const d = draft;
    if (!d.name.trim()) return store.say(L("Donnez un nom au préréglage.", "Give the preset a name."));
    if (!d.appliesTo.length) return store.say(L("Choisissez au moins une sorte de fichier.", "Choose at least one kind of file."));
    if (await save(store.presets.map((p) => (p.id === d.id ? d : p)))) {
      editing = null;
      draft = null;
    }
  }

  async function toggle(p: PresetView) {
    await save(store.presets.map((x) => (x.id === p.id ? { ...x, enabled: !x.enabled } : x)));
  }

  async function remove(p: PresetView) {
    deleting = null;
    if (await save(store.presets.filter((x) => x.id !== p.id))) store.say(L(`Préréglage « ${p.name} » supprimé.`, `Preset “${p.name}” deleted.`));
  }

  async function move(p: PresetView, by: number) {
    const list = [...store.presets];
    const i = list.findIndex((x) => x.id === p.id);
    const j = i + by;
    if (i < 0 || j < 0 || j >= list.length) return;
    [list[i], list[j]] = [list[j], list[i]];
    await save(list);
  }

  function toggleKind(k: string) {
    if (!draft) return;
    draft.appliesTo = draft.appliesTo.includes(k) ? draft.appliesTo.filter((x) => x !== k) : [...draft.appliesTo, k];
  }

  function dropOption(key: string) {
    if (!draft) return;
    const next = { ...draft.options };
    delete next[key];
    draft.options = next;
  }

  async function pickDir() {
    if (!draft) return;
    try {
      const dir = await (await api).pickFolder(L("Dossier de destination du préréglage", "Destination folder of the preset"));
      if (dir && draft) draft.outputDir = dir;
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

  async function reset() {
    confirmReset = false;
    busy = true;
    await store.resetPresets();
    busy = false;
    editing = null;
  }

  const enabled = $derived(store.presets.filter((p) => p.enabled).length);
  const windows = typeof navigator !== "undefined" && navigator.userAgent.includes("Windows");

  onMount(() => {
    void store.loadShell();
  });
</script>

<div class="page">
  <header>
    <div>
      <h1>{L("Préréglages", "Presets")}</h1>
      <p class="muted">
        {L("Un format, des options et une destination, enregistrés sous un nom. Ils sont proposés dans l'écran Convertir, par la commande", "A format, options and a destination, saved under a name. They are offered in the Convert screen, by the command")} <code>chud convert --preset</code> {L("et dans le menu clic droit de l'Explorateur.", "and in the File Explorer right-click menu.")}
      </p>
    </div>
    <div class="head-actions">
      {#if confirmReset}
        <span class="small">{L("Rétablir les préréglages fournis ? Votre fichier actuel sera gardé en copie.", "Restore the provided presets? A copy of your current file will be kept.")}</span>
        <button class="btn primary small" onclick={reset} disabled={busy}>{L("Rétablir", "Restore")}</button>
        <button class="btn small" onclick={() => (confirmReset = false)}>{L("Annuler", "Cancel")}</button>
      {:else}
        <button class="btn" onclick={() => (confirmReset = true)} disabled={busy}>
          <Icon name="refresh" size={14} /> {L("Rétablir les préréglages fournis", "Restore the provided presets")}
        </button>
      {/if}
    </div>
  </header>

  {#if store.shell?.supported}
    {@const sh = store.shell}
    <section class="card menu" class:on={sh.installed && sh.current && !sh.exeMissing}>
      <div class="menu-icon"><Icon name="bolt" size={18} /></div>
      <div class="menu-text">
        <h2>{L("Menu clic droit", "Right-click menu")}</h2>
        {#if !sh.installed}
          <p class="muted">
            {L(`Ajoute « Chud Converter » au menu clic droit ${windows ? "de l'Explorateur" : "du gestionnaire de fichiers"}, avec vos préréglages actifs. Pour votre compte seulement, sans droits d'administrateur, et retirable à tout moment.`, `Adds “Chud Converter” to the right-click menu of ${windows ? "File Explorer" : "the file manager"}, with your active presets. For your account only, without administrator rights, and removable at any time.`)}
          </p>
        {:else if sh.current && !sh.exeMissing}
          <p>
            <b>{L("Installé", "Installed")}</b> {L(`pour ${sh.entries} types de fichiers. Il suit vos préréglages : modifiez-les ici, le menu se met à jour.`, `for ${sh.entries} file types. It follows your presets: change them here and the menu updates.`)}
          </p>
        {:else}
          <p class="warn">
            {sh.exeMissing ? L("Le menu lance un programme introuvable", "The menu launches a program that cannot be found") : L("Le menu lance une autre copie de Chud Converter", "The menu launches another copy of Chud Converter")} :
            <span class="mono selectable">{sh.exe}</span>
          </p>
        {/if}
        {#if windows}
          <p class="faint small">{L("Sous Windows 11, il se trouve dans « Afficher plus d'options » (ou Maj+F10).", "On Windows 11 it is under “Show more options” (or Shift+F10).")}</p>
        {:else}
          <p class="faint small">{L("Dolphin, Nemo et Nautilus. Thunar n'est pas modifié automatiquement.", "Dolphin, Nemo and Nautilus. Thunar is not changed automatically.")}</p>
        {/if}
      </div>
      <div class="menu-actions">
        {#if !sh.installed}
          <button class="btn primary" onclick={() => store.setShell(true)} disabled={store.shellBusy}>
            {store.shellBusy ? L("Installation…", "Installing…") : L("Ajouter au menu", "Add to menu")}
          </button>
        {:else}
          {#if !sh.current || sh.exeMissing}
            <button class="btn primary" onclick={() => store.setShell(true)} disabled={store.shellBusy}>{L("Utiliser ce programme", "Use this program")}</button>
          {:else}
            <!-- Après l'installation d'un moteur, de nouvelles conversions deviennent possibles. -->
            <button class="btn" onclick={() => store.setShell(true)} disabled={store.shellBusy} title={L("Après l'installation d'un moteur, par exemple", "After installing an engine, for example")}>
              <Icon name="refresh" size={14} /> {L("Mettre à jour", "Update")}
            </button>
          {/if}
          <button class="btn" onclick={() => store.setShell(false)} disabled={store.shellBusy}>{L("Retirer du menu", "Remove from menu")}</button>
        {/if}
      </div>
    </section>
  {/if}

  {#if store.presetsError}
    <div class="card broken">
      <Icon name="alert" size={18} />
      <div>
        <p><b>{L("Le fichier des préréglages est illisible.", "The presets file cannot be read.")}</b> {L("Il n'est pas modifié tant que vous ne l'avez pas corrigé ou rétabli.", "It is not changed until you fix or restore it.")}</p>
        <p class="selectable small">{store.presetsError}</p>
      </div>
    </div>
  {/if}

  {#if store.presetsFile}
    <p class="file small faint">
      {L(`${enabled} actif${enabled > 1 ? "s" : ""} sur ${store.presets.length} · modifiable à la main :`, `${enabled} of ${store.presets.length} active · editable by hand:`)}
      <span class="mono selectable" title={store.presetsFile}>presets.toml</span>
      <button class="icon-btn" title={L("Afficher dans le dossier", "Show in folder")} aria-label={L("Afficher presets.toml dans le dossier", "Show presets.toml in its folder")} onclick={() => reveal(store.presetsFile!)}>
        <Icon name="reveal" size={14} />
      </button>
    </p>
  {/if}

  <div class="list card">
    {#each store.presets as p, i (p.id)}
      <div class="preset" class:off={!p.enabled}>
        <div class="row">
          <label class="switch" title={p.enabled ? L("Proposé", "Offered") : L("Masqué", "Hidden")}>
            <input type="checkbox" checked={p.enabled} onchange={() => toggle(p)} disabled={busy} aria-label={L(`Proposer « ${p.name} »`, `Offer “${p.name}”`)} />
          </label>
          <div class="main">
            <div class="name">
              <strong>{p.name}</strong>
              <span class="to"><Icon name="chevron" size={12} /> {p.targetName}</span>
              {#if !p.builtin}<span class="tag">{L("personnel", "custom")}</span>{/if}
            </div>
            <div class="meta">
              {#each p.appliesTo as k (k)}<span class="kind">{kindLabel(k)}</span>{/each}
              {#each Object.entries(p.options) as [k, v] (k)}<span class="opt mono">{k} = {String(v)}</span>{/each}
              {#if p.outputDir}<span class="faint mono" title={p.outputDir}>→ {p.outputDir}</span>{/if}
            </div>
          </div>
          <div class="actions">
            <button class="icon-btn" title={L("Monter", "Move up")} aria-label={L(`Monter « ${p.name} »`, `Move “${p.name}” up`)} disabled={busy || i === 0} onclick={() => move(p, -1)}>
              <Icon name="up" size={14} />
            </button>
            <button
              class="icon-btn"
              title={L("Descendre", "Move down")}
              aria-label={L(`Descendre « ${p.name} »`, `Move “${p.name}” down`)}
              disabled={busy || i === store.presets.length - 1}
              onclick={() => move(p, 1)}
            >
              <Icon name="down" size={14} />
            </button>
            <button class="btn small" onclick={() => (editing === p.id ? (editing = null) : edit(p))}>{L("Modifier", "Edit")}</button>
            {#if deleting === p.id}
              <button class="btn small danger" onclick={() => remove(p)} disabled={busy}>{L("Supprimer", "Delete")}</button>
              <button class="btn small" onclick={() => (deleting = null)}>{L("Garder", "Keep")}</button>
            {:else}
              <button class="icon-btn" title={L("Supprimer", "Delete")} aria-label={L(`Supprimer « ${p.name} »`, `Delete “${p.name}”`)} onclick={() => (deleting = p.id)}>
                <Icon name="trash" size={15} />
              </button>
            {/if}
          </div>
        </div>

        {#if editing === p.id && draft}
          <div class="editor">
            <label class="field">
              <span>{L("Nom", "Name")}</span>
              <input type="text" bind:value={draft.name} maxlength="60" />
            </label>
            <div class="field">
              <span>{L("Proposé pour", "Offered for")}</span>
              <div class="kinds">
                {#each KINDS as k (k.kind)}
                  <label class="chip" class:on={draft.appliesTo.includes(k.kind)}>
                    <input type="checkbox" checked={draft.appliesTo.includes(k.kind)} onchange={() => toggleKind(k.kind)} />
                    {k.label}
                  </label>
                {/each}
              </div>
            </div>
            <div class="field">
              <span>{L("Format :", "Format:")} <b>{p.targetName}</b></span>
              <span class="faint small">{L("Pour un autre format, choisissez-le dans l'écran Convertir puis « Enregistrer comme préréglage ».", "For another format, choose it in the Convert screen, then “Save as preset”.")}</span>
            </div>
            {#if Object.keys(draft.options).length}
              <div class="field">
                <span>{L("Options", "Options")}</span>
                <div class="kinds">
                  {#each Object.entries(draft.options) as [k, v] (k)}
                    <span class="opt mono">
                      {k} = {String(v)}
                      <button class="icon-btn tiny" title={L("Retirer", "Remove")} aria-label={L(`Retirer l'option ${k}`, `Remove the ${k} option`)} onclick={() => dropOption(k)}>
                        <Icon name="x" size={11} />
                      </button>
                    </span>
                  {/each}
                </div>
              </div>
            {/if}
            <div class="field">
              <span>Destination</span>
              <div class="dest">
                <span class="mono selectable">{draft.outputDir ?? L("à côté de chaque original", "next to each original")}</span>
                <button class="btn small" onclick={pickDir}>{L("Choisir un dossier…", "Choose a folder…")}</button>
                {#if draft.outputDir}
                  <button class="btn small ghost" onclick={() => draft && (draft.outputDir = null)}>{L("À côté de l'original", "Next to the original")}</button>
                {/if}
              </div>
            </div>
            <label class="field">
              <span>{L("Si le fichier existe déjà", "If the file already exists")}</span>
              <select bind:value={draft.conflict}>
                <option value="rename">{L("Garder les deux : « nom (1) »", "Keep both: “name (1)”")}</option>
                <option value="skip">{L("Ne rien faire", "Do nothing")}</option>
                <option value="overwrite">{L("Remplacer l'ancien résultat", "Replace the old result")}</option>
              </select>
            </label>
            <label class="check">
              <input type="checkbox" bind:checked={draft.keepDate} />
              {L("Garder la date de modification de l'original", "Keep the original's modification date")}
            </label>
            <div class="editor-actions">
              <button class="btn primary" onclick={saveDraft} disabled={busy}>{L("Enregistrer", "Save")}</button>
              <button class="btn" onclick={() => ((editing = null), (draft = null))}>{L("Annuler", "Cancel")}</button>
              <span class="faint small">{L("Identifiant :", "Identifier:")} <span class="mono">{p.id}</span> · {conflictText[draft.conflict]}</span>
            </div>
          </div>
        {/if}
      </div>
    {:else}
      <p class="empty muted">{L("Aucun préréglage. Choisissez un format dans l'écran Convertir puis « Enregistrer comme préréglage ».", "No presets. Choose a format in the Convert screen, then “Save as preset”.")}</p>
    {/each}
  </div>
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

  .head-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    justify-content: flex-end;
    max-width: 420px;
  }

  .small {
    font-size: 12.5px;
  }

  .menu {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 14px 16px;
  }

  .menu.on {
    border-color: var(--ok);
  }

  .menu-icon {
    flex: none;
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 10px;
    background: var(--accent);
    color: var(--accent-text);
  }

  .menu-text {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 3px;
  }

  .menu-text h2 {
    font-size: 15px;
  }

  .menu .warn {
    color: var(--depends);
  }

  .menu-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }

  .broken {
    display: flex;
    gap: 10px;
    padding: 12px 14px;
    color: var(--lossy);
  }

  .broken p {
    color: var(--text);
  }

  .file {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .list {
    overflow: hidden;
  }

  .preset + .preset {
    border-top: 1px solid var(--border);
  }

  .preset.off .main {
    opacity: 0.55;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
  }

  .switch {
    display: grid;
    place-items: center;
    cursor: pointer;
  }

  .main {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 4px;
  }

  .name {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .to {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    color: var(--muted);
    font-weight: 500;
  }

  .tag {
    font-size: 11px;
    font-weight: 600;
    padding: 1px 7px;
    border-radius: 999px;
    background: var(--primary-soft);
    color: var(--primary);
  }

  .meta,
  .kinds {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    align-items: center;
    font-size: 12px;
  }

  .kind,
  .opt,
  .chip {
    padding: 1px 8px;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: var(--surface-2);
    color: var(--muted);
  }

  .opt {
    font-size: 11.5px;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 3px 10px;
    cursor: pointer;
    font-size: 12.5px;
  }

  .chip input {
    width: 13px;
    height: 13px;
  }

  .chip.on {
    border-color: var(--primary);
    color: var(--text);
    background: var(--primary-soft);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .danger {
    color: var(--lossy);
    border-color: var(--lossy);
  }

  .editor {
    display: grid;
    gap: 12px;
    padding: 4px 16px 16px 48px;
  }

  .field {
    display: grid;
    gap: 5px;
    font-size: 13px;
    color: var(--muted);
  }

  .field input[type="text"],
  .field select {
    max-width: 420px;
    color: var(--text);
  }

  .dest {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    color: var(--text);
  }

  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
  }

  .editor-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .tiny {
    width: 18px;
    height: 18px;
  }

  .empty {
    padding: 16px;
  }
</style>
