<script lang="ts">
  import { L } from "../i18n.svelte";
  import { onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import PageHeader from "./ui/PageHeader.svelte";
  import Callout from "./ui/Callout.svelte";
  import Switch from "./ui/Switch.svelte";
  import StatusPill from "./ui/StatusPill.svelte";
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
  <div class="page-inner">
  <PageHeader title={L("Préréglages", "Presets")} eyebrow={L("Raccourcis", "Shortcuts")} icon="preset">
    {L("Un format, des options et une destination, enregistrés sous un nom. Ils sont proposés dans l'écran Convertir, par la commande", "A format, options and a destination, saved under a name. They are offered in the Convert screen, by the command")} <code>chud convert --preset</code> {L("et dans le menu clic droit de l'Explorateur.", "and in the File Explorer right-click menu.")}
    {#snippet actions()}
      {#if !confirmReset}
        <button class="btn" onclick={() => (confirmReset = true)} disabled={busy}>
          <Icon name="refresh" size={14} /> {L("Rétablir les préréglages fournis", "Restore the provided presets")}
        </button>
      {/if}
    {/snippet}
  </PageHeader>

  {#if confirmReset}
    <Callout tone="warn" icon="refresh" role="group" label={L("Confirmation", "Confirmation")} title={L("Rétablir les préréglages fournis ?", "Restore the provided presets?")}>
      <p class="small muted">{L("Votre fichier actuel sera gardé en copie.", "A copy of your current file will be kept.")}</p>
      {#snippet actions()}
        <button class="btn primary small" onclick={reset} disabled={busy}>{L("Rétablir", "Restore")}</button>
        <button class="btn small" onclick={() => (confirmReset = false)}>{L("Annuler", "Cancel")}</button>
      {/snippet}
    </Callout>
  {/if}

  {#if store.shell?.supported}
    {@const sh = store.shell}
    <section class="panel menu enter" class:on={sh.installed && sh.current && !sh.exeMissing}>
      <div class="menu-icon"><Icon name="bolt" size={18} /></div>
      <div class="menu-text">
        <div class="menu-title">
          <h2>{L("Menu clic droit", "Right-click menu")}</h2>
          {#if sh.installed && sh.current && !sh.exeMissing}
            <StatusPill tone="ok">{L("Installé", "Installed")}</StatusPill>
          {:else if sh.installed}
            <StatusPill tone="warn">{L("À réparer", "Needs repair")}</StatusPill>
          {/if}
        </div>
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
          <button class="btn primary" onclick={() => store.setShell(true)} disabled={!!store.shellOp}>
            {store.shellOp === "installing" ? L("Installation…", "Installing…") : L("Ajouter au menu", "Add to menu")}
          </button>
        {:else}
          {#if !sh.current || sh.exeMissing}
            <button class="btn primary" onclick={() => store.setShell(true)} disabled={!!store.shellOp}>{L("Utiliser ce programme", "Use this program")}</button>
          {:else}
            <!-- Après l'installation d'un moteur, de nouvelles conversions deviennent possibles. -->
            <button class="btn" onclick={() => store.setShell(true)} disabled={!!store.shellOp} title={L("Après l'installation d'un moteur, par exemple", "After installing an engine, for example")}>
              <Icon name="refresh" size={14} /> {store.shellOp === "installing" ? L("Mise à jour…", "Updating…") : L("Mettre à jour", "Update")}
            </button>
          {/if}
          <button class="btn" onclick={() => store.setShell(false)} disabled={!!store.shellOp}>{store.shellOp === "removing" ? L("Retrait…", "Removing…") : L("Retirer du menu", "Remove from menu")}</button>
        {/if}
      </div>
    </section>
  {/if}

  {#if store.presetsError}
    <Callout tone="danger" role="alert">
      <p><b>{L("Le fichier des préréglages est illisible.", "The presets file cannot be read.")}</b> {L("Il n'est pas modifié tant que vous ne l'avez pas corrigé ou rétabli.", "It is not changed until you fix or restore it.")}</p>
      <p class="selectable small">{store.presetsError}</p>
    </Callout>
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

  <div class="list panel rows enter">
    {#each store.presets as p, i (p.id)}
      <div class="preset" class:off={!p.enabled}>
        <div class="row">
          <Switch checked={p.enabled} onchange={() => toggle(p)} disabled={busy} title={p.enabled ? L("Proposé", "Offered") : L("Masqué", "Hidden")} label={L(`Proposer « ${p.name} »`, `Offer “${p.name}”`)} />
          <div class="main">
            <div class="name">
              <strong>{p.name}</strong>
              <span class="to"><Icon name="chevron" size={12} /> {p.targetName}</span>
              {#if !p.builtin}<span class="tag accent">{L("personnel", "custom")}</span>{/if}
            </div>
            <div class="meta">
              {#each p.appliesTo as k (k)}<span class="tag">{kindLabel(k)}</span>{/each}
              {#each Object.entries(p.options) as [k, v] (k)}<span class="tag mono opt">{k} = {String(v)}</span>{/each}
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
            <button class="btn small" class:on={editing === p.id} aria-expanded={editing === p.id} onclick={() => (editing === p.id ? (editing = null) : edit(p))}>{L("Modifier", "Edit")}</button>
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
          <div class="editor pop">
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
                    <span class="tag mono opt">
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
      <div class="empty">
        <span class="empty-ico" aria-hidden="true"><Icon name="preset" size={22} /></span>
        <p class="muted">{L("Aucun préréglage. Choisissez un format dans l'écran Convertir puis « Enregistrer comme préréglage ».", "No presets. Choose a format in the Convert screen, then “Save as preset”.")}</p>
      </div>
    {/each}
  </div>
  </div>
</div>

<style>
  .small {
    font-size: 12.5px;
  }

  .menu {
    position: relative;
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 14px 16px;
    padding: 16px 18px;
    overflow: hidden;
  }

  .menu.on {
    border-color: color-mix(in srgb, var(--ok) 35%, transparent);
  }

  .menu-icon {
    flex: none;
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border-radius: 12px;
    color: #fff;
    background: var(--grad);
    box-shadow: var(--glow);
  }

  .menu-text {
    flex: 1 1 320px;
    min-width: 0;
    display: grid;
    gap: 4px;
  }

  .menu-title {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .menu-text h2 {
    font-size: 16px;
  }

  .menu .warn {
    color: var(--depends);
  }

  .menu-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    justify-content: flex-end;
    margin-left: auto;
  }

  .file {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: -8px;
  }

  .list {
    overflow: hidden;
  }

  .preset {
    transition: background var(--t-fast) var(--ease);
  }

  .preset:hover {
    background: var(--surface-hover);
  }

  .preset.off .main {
    opacity: 0.5;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 12px 12px 12px 18px;
  }

  .main {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 6px;
  }

  .name {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .name strong {
    font-weight: 600;
  }

  .to {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    color: var(--accent);
    font-weight: 600;
  }

  .meta,
  .kinds {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    align-items: center;
    font-size: 12px;
  }

  .opt {
    font-size: 11px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 11px;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: var(--surface-2);
    color: var(--muted);
    cursor: pointer;
    font-size: 12.5px;
    transition:
      border-color var(--t-fast) var(--ease),
      background var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease);
  }

  .chip:hover {
    border-color: var(--accent);
  }

  .chip input {
    width: 14px;
    height: 14px;
  }

  .chip.on {
    border-color: var(--accent);
    color: var(--text);
    background: var(--accent-soft);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .btn.on {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .editor {
    display: grid;
    gap: 14px;
    margin: 0 16px 16px 68px;
    padding: 16px 18px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    background: var(--surface-2);
  }

  .field {
    display: grid;
    gap: 6px;
    font-size: 12.5px;
    color: var(--muted);
  }

  .field input[type="text"],
  .field select {
    max-width: 420px;
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
    gap: 9px;
    cursor: pointer;
  }

  .editor-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    padding-top: 4px;
  }

  .tiny {
    width: 18px;
    height: 18px;
  }

  .empty {
    display: grid;
    justify-items: center;
    gap: 10px;
    padding: 36px 20px;
    text-align: center;
  }

  .empty-ico {
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    border-radius: 50%;
    color: var(--accent);
    background: var(--accent-soft);
  }

  @media (max-width: 1100px) {
    .editor {
      margin-left: 16px;
    }
  }
</style>
