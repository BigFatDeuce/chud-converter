<script lang="ts">
  import Icon from "./Icon.svelte";
  import { store } from "../store.svelte";
  import { api, errorText } from "../api";
  import { categoryLabel } from "../format";
  import type { Category, Conflict, PresetView } from "../types";

  // Sortes proposées à l'édition : celles où un préréglage a un sens.
  const KINDS: { kind: string; label: string }[] = [
    ...(["video", "audio", "image", "subtitle", "archive", "data", "document", "spreadsheet", "presentation", "pdf", "ebook"] as Category[]).map(
      (c) => ({ kind: c, label: categoryLabel[c] }),
    ),
    { kind: "folder", label: "Dossiers" },
  ];
  const kindLabel = (k: string) => KINDS.find((x) => x.kind === k)?.label ?? k;

  const conflictText: Record<Conflict, string> = {
    rename: "garder les deux",
    skip: "ne rien faire si le résultat existe",
    overwrite: "remplacer l'ancien résultat",
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
    if (!d.name.trim()) return store.say("Donnez un nom au préréglage.");
    if (!d.appliesTo.length) return store.say("Choisissez au moins une sorte de fichier.");
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
    if (await save(store.presets.filter((x) => x.id !== p.id))) store.say(`Préréglage « ${p.name} » supprimé.`);
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
      const dir = await (await api).pickFolder("Dossier de destination du préréglage");
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
</script>

<div class="page">
  <header>
    <div>
      <h1>Préréglages</h1>
      <p class="muted">
        Un format, des options et une destination, enregistrés sous un nom. Ils sont proposés dans l'écran Convertir, par la
        commande <code>chud convert --preset</code> et dans le menu clic droit de l'Explorateur.
      </p>
    </div>
    <div class="head-actions">
      {#if confirmReset}
        <span class="small">Rétablir les préréglages fournis ? Votre fichier actuel sera gardé en copie.</span>
        <button class="btn primary small" onclick={reset} disabled={busy}>Rétablir</button>
        <button class="btn small" onclick={() => (confirmReset = false)}>Annuler</button>
      {:else}
        <button class="btn" onclick={() => (confirmReset = true)} disabled={busy}>
          <Icon name="refresh" size={14} /> Rétablir les préréglages fournis
        </button>
      {/if}
    </div>
  </header>

  {#if store.presetsError}
    <div class="card broken">
      <Icon name="alert" size={18} />
      <div>
        <p><b>Le fichier des préréglages est illisible.</b> Il n'est pas modifié tant que vous ne l'avez pas corrigé ou rétabli.</p>
        <p class="selectable small">{store.presetsError}</p>
      </div>
    </div>
  {/if}

  {#if store.presetsFile}
    <p class="file small faint">
      {enabled} actif{enabled > 1 ? "s" : ""} sur {store.presets.length} · modifiable à la main :
      <span class="mono selectable" title={store.presetsFile}>presets.toml</span>
      <button class="icon-btn" title="Afficher dans le dossier" aria-label="Afficher presets.toml dans le dossier" onclick={() => reveal(store.presetsFile!)}>
        <Icon name="reveal" size={14} />
      </button>
    </p>
  {/if}

  <div class="list card">
    {#each store.presets as p, i (p.id)}
      <div class="preset" class:off={!p.enabled}>
        <div class="row">
          <label class="switch" title={p.enabled ? "Proposé" : "Masqué"}>
            <input type="checkbox" checked={p.enabled} onchange={() => toggle(p)} disabled={busy} aria-label="Proposer « {p.name} »" />
          </label>
          <div class="main">
            <div class="name">
              <strong>{p.name}</strong>
              <span class="to"><Icon name="chevron" size={12} /> {p.targetName}</span>
              {#if !p.builtin}<span class="tag">personnel</span>{/if}
            </div>
            <div class="meta">
              {#each p.appliesTo as k (k)}<span class="kind">{kindLabel(k)}</span>{/each}
              {#each Object.entries(p.options) as [k, v] (k)}<span class="opt mono">{k} = {String(v)}</span>{/each}
              {#if p.outputDir}<span class="faint mono" title={p.outputDir}>→ {p.outputDir}</span>{/if}
            </div>
          </div>
          <div class="actions">
            <button class="icon-btn" title="Monter" aria-label="Monter « {p.name} »" disabled={busy || i === 0} onclick={() => move(p, -1)}>
              <Icon name="up" size={14} />
            </button>
            <button
              class="icon-btn"
              title="Descendre"
              aria-label="Descendre « {p.name} »"
              disabled={busy || i === store.presets.length - 1}
              onclick={() => move(p, 1)}
            >
              <Icon name="down" size={14} />
            </button>
            <button class="btn small" onclick={() => (editing === p.id ? (editing = null) : edit(p))}>Modifier</button>
            {#if deleting === p.id}
              <button class="btn small danger" onclick={() => remove(p)} disabled={busy}>Supprimer</button>
              <button class="btn small" onclick={() => (deleting = null)}>Garder</button>
            {:else}
              <button class="icon-btn" title="Supprimer" aria-label="Supprimer « {p.name} »" onclick={() => (deleting = p.id)}>
                <Icon name="trash" size={15} />
              </button>
            {/if}
          </div>
        </div>

        {#if editing === p.id && draft}
          <div class="editor">
            <label class="field">
              <span>Nom</span>
              <input type="text" bind:value={draft.name} maxlength="60" />
            </label>
            <div class="field">
              <span>Proposé pour</span>
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
              <span>Format : <b>{p.targetName}</b></span>
              <span class="faint small">Pour un autre format, choisissez-le dans l'écran Convertir puis « Enregistrer comme préréglage ».</span>
            </div>
            {#if Object.keys(draft.options).length}
              <div class="field">
                <span>Options</span>
                <div class="kinds">
                  {#each Object.entries(draft.options) as [k, v] (k)}
                    <span class="opt mono">
                      {k} = {String(v)}
                      <button class="icon-btn tiny" title="Retirer" aria-label="Retirer l'option {k}" onclick={() => dropOption(k)}>
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
                <span class="mono selectable">{draft.outputDir ?? "à côté de chaque original"}</span>
                <button class="btn small" onclick={pickDir}>Choisir un dossier…</button>
                {#if draft.outputDir}
                  <button class="btn small ghost" onclick={() => draft && (draft.outputDir = null)}>À côté de l'original</button>
                {/if}
              </div>
            </div>
            <label class="field">
              <span>Si le fichier existe déjà</span>
              <select bind:value={draft.conflict}>
                <option value="rename">Garder les deux : « nom (1) »</option>
                <option value="skip">Ne rien faire</option>
                <option value="overwrite">Remplacer l'ancien résultat</option>
              </select>
            </label>
            <label class="check">
              <input type="checkbox" bind:checked={draft.keepDate} />
              Garder la date de modification de l'original
            </label>
            <div class="editor-actions">
              <button class="btn primary" onclick={saveDraft} disabled={busy}>Enregistrer</button>
              <button class="btn" onclick={() => ((editing = null), (draft = null))}>Annuler</button>
              <span class="faint small">Identifiant : <span class="mono">{p.id}</span> · {conflictText[draft.conflict]}</span>
            </div>
          </div>
        {/if}
      </div>
    {:else}
      <p class="empty muted">Aucun préréglage. Choisissez un format dans l'écran Convertir puis « Enregistrer comme préréglage ».</p>
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
