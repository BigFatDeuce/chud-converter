<script lang="ts">
  // Écran « Outils » : ce qui n'est pas une conversion — fusionner, diviser ou pivoter des PDF,
  // réunir des fichiers dans une archive. Même honnêteté : le rapport est montré avant de lancer.
  import { L } from "../i18n.svelte";
  import Icon from "./Icon.svelte";
  import FidelityBadge from "./FidelityBadge.svelte";
  import OptionsForm from "./OptionsForm.svelte";
  import JobList from "./JobList.svelte";
  import Callout from "./ui/Callout.svelte";
  import FileGlyph from "./ui/FileGlyph.svelte";
  import { store } from "../store.svelte";
  import { api, errorText } from "../api";
  import { baseName, formatSize, toolIcon } from "../format";
  import { offerSize } from "../engineText";
  import type { Conflict, ToolPlan } from "../types";

  const tool = $derived(store.tool);
  const ok = $derived(store.toolSplit.ok);
  const aside = $derived(store.toolSplit.aside);
  const engine = $derived(store.engines?.find((e) => e.id === tool?.engine) ?? null);
  const missing = $derived(tool ? Math.max(0, tool.minInputs - ok.length) : 0);
  const onlyPdf = $derived(!!tool?.accepts.length);

  // Aperçu : recalculé quand les fichiers, leur ordre, les options ou la destination changent.
  const key = $derived(tool?.ready && !missing ? JSON.stringify(store.toolRequest()) : null);
  let plan = $state<ToolPlan | null>(null);
  let planKey = $state<string | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);

  $effect(() => {
    const wanted = key;
    if (!wanted) {
      plan = null;
      error = null;
      loading = false;
      return;
    }
    let stale = false;
    loading = true;
    const timer = setTimeout(async () => {
      try {
        const p = await (await api).toolPlan(JSON.parse(wanted));
        if (stale) return;
        plan = p;
        planKey = wanted;
        error = null;
      } catch (e) {
        if (stale) return;
        plan = null;
        error = errorText(e);
      } finally {
        if (!stale) loading = false;
      }
    }, 220);
    return () => {
      stale = true;
      clearTimeout(timer);
    };
  });

  const canRun = $derived(!!tool?.ready && !missing && !!plan && planKey === key && !error);

  const runLabel = $derived.by(() => {
    if (!tool) return "";
    if (tool.grouping === "each" && ok.length > 1) return L(`${tool.name} : ${ok.length} fichiers`, `${tool.name}: ${ok.length} files`);
    if (tool.grouping === "together" && ok.length > 1) return L(`${tool.name} (${ok.length} éléments)`, `${tool.name} (${ok.length} items)`);
    return tool.name;
  });

  const asideReason = (i: (typeof aside)[number]) =>
    i.isDir
      ? L("dossier : ajoutez les PDF qu'il contient", "folder: add the PDFs it contains")
      : (i.error ?? L(`${i.formatName ?? i.format} : ce n'est pas un PDF`, `${i.formatName ?? i.format}: not a PDF`));

  function setConflict(e: Event & { currentTarget: HTMLSelectElement }) {
    store.conflict = e.currentTarget.value as Conflict;
    store.saveSettings();
  }
</script>

<div class="tools">
  <div class="left">
    <header class="head enter">
      <span class="eyebrow">{L("Pages et archives", "Pages and archives")}</span>
      <h1>{L("Outils", "Tools")}</h1>
    </header>

    <div class="cards" role="radiogroup" aria-label={L("Outil", "Tool")}>
      {#each store.tools as t (t.id)}
        {@const on = t.id === store.toolId}
        <button class="card" class:on role="radio" aria-checked={on} data-tool={t.id} onclick={() => store.chooseTool(t.id)}>
          <span class="card-ico"><Icon name={toolIcon(t.id)} size={18} stroke={1.8} /></span>
          <span class="card-text">
            <strong>{t.name}</strong>
            <span>{t.description}</span>
            {#if !t.ready}<em>{L("moteur à installer", "engine to install")}</em>{/if}
          </span>
        </button>
      {:else}
        <p class="hint scanning" aria-busy="true">{L("Recherche des moteurs…", "Looking for engines…")}</p>
      {/each}
    </div>

    <section class="panel files enter" aria-labelledby="tool-files">
      <header class="bar">
        <div class="title">
          <h2 id="tool-files">{onlyPdf ? L("Fichiers PDF", "PDF files") : L("Fichiers et dossiers", "Files and folders")}</h2>
          {#if ok.length}<span class="tag accent">{ok.length}</span>{/if}
        </div>
        <div class="actions">
          <button class="btn small" onclick={() => store.pickToolFiles()} title="Ctrl+O"><Icon name="plus" size={14} /> {L("Ajouter des fichiers", "Add files")}</button>
          <button class="btn small" onclick={() => store.pickToolFolder()}><Icon name="folder" size={14} /> {L("Ajouter un dossier", "Add a folder")}</button>
          {#if tool?.ordered && ok.length > 1}
            <button class="btn small ghost" onclick={() => store.sortToolFiles()}><Icon name="sort" size={13} /> {L("Trier par nom", "Sort by name")}</button>
          {/if}
          {#if store.toolFiles.length}
            <button class="btn small ghost" onclick={() => (store.toolFiles = [])}><Icon name="trash" size={13} /> {L("Vider la liste", "Clear the list")}</button>
          {/if}
        </div>
      </header>

      {#if !store.toolFiles.length}
        <button class="empty" onclick={() => store.pickToolFiles()}>
          <span class="core"><Icon name={toolIcon(store.toolId)} size={24} stroke={1.8} /></span>
          <strong>{onlyPdf ? L("Glissez vos PDF ici", "Drag your PDFs here") : L("Glissez vos fichiers ou dossiers ici", "Drag your files or folders here")}</strong>
          <span class="muted">{tool?.description ?? ""}</span>
          <span class="faint small"><Icon name="shield" size={12} /> {L("Vos originaux ne sont jamais modifiés.", "Your originals are never modified.")}</span>
        </button>
      {:else}
        <div class="rows">
          {#each ok as i, n (i.path)}
            <div class="row" data-file={i.name}>
              {#if tool?.ordered}<span class="num" aria-hidden="true">{n + 1}</span>{/if}
              <FileGlyph category={i.category} dir={i.isDir} />
              <div class="main">
                <div class="name selectable" title={i.path}>{i.name}</div>
                <div class="meta">{i.isDir ? L("Dossier", "Folder") : (i.formatName ?? i.format ?? L("Fichier", "File"))} <span class="faint">· {formatSize(i.size)}</span></div>
              </div>
              {#if tool?.ordered}
                <button class="icon-btn" disabled={n === 0} title={L("Monter", "Move up")} aria-label={L(`Monter ${i.name}`, `Move ${i.name} up`)} onclick={() => store.moveToolFile(i.path, -1)}><Icon name="up" size={15} /></button>
                <button class="icon-btn" disabled={n === ok.length - 1} title={L("Descendre", "Move down")} aria-label={L(`Descendre ${i.name}`, `Move ${i.name} down`)} onclick={() => store.moveToolFile(i.path, 1)}><Icon name="down" size={15} /></button>
              {/if}
              {@render remove(i.path, i.name)}
            </div>
          {/each}
          {#each aside as i (i.path)}
            <div class="row off" data-aside={i.name}>
              <FileGlyph category={i.category} dir={i.isDir} broken={!!i.error} />
              <div class="main">
                <div class="name selectable" title={i.path}>{i.name}</div>
                <div class="meta"><span class="warn"><Icon name="alert" size={12} /> {L("laissé de côté —", "left aside —")} {asideReason(i)}</span></div>
              </div>
              {@render remove(i.path, i.name)}
            </div>
          {/each}
        </div>
      {/if}
    </section>

    {#if store.jobs.length}
      <JobList />
    {/if}
  </div>

  <aside aria-labelledby="tool-title">
    <div class="scroll">
      <header class="panel-head">
        <span class="eyebrow">{L("Outil", "Tool")}</span>
        <h2 id="tool-title">{tool?.name ?? ""}</h2>
      </header>

      {#if tool && !tool.ready}
        <Callout tone="accent" icon="download">
          <p><strong>{L(`Cet outil utilise ${engine?.name ?? tool.engine}.`, `This tool uses ${engine?.name ?? tool.engine}.`)}</strong></p>
          <p class="small muted">{L("Un logiciel libre, téléchargé une fois depuis sa publication officielle et vérifié ; rien n'est modifié sur le système.", "Free software, downloaded once from its official release and checked; nothing is changed on the system.")}</p>
          {#snippet actions()}
            {#if engine?.installable}
              <button class="btn primary" onclick={() => store.offerEngine(tool.engine)}>
                <Icon name="download" size={14} />
                {L(`Installer ${engine.name} (${offerSize(engine.installable)})`, `Install ${engine.name} (${offerSize(engine.installable)})`)}
              </button>
            {:else}
              <button class="btn" onclick={() => (store.view = "engines")}>{L("Voir les moteurs", "See the engines")}</button>
            {/if}
          {/snippet}
        </Callout>
      {:else if tool}
        {#if tool.options.length}
          <OptionsForm specs={tool.options} values={store.toolOptions[tool.id] ?? {}} onchange={(k, v, d) => store.setToolOption(k, v, d)} />
        {/if}

        {#if missing}
          <p class="hint">
            {#if !ok.length}
              {L("Ajoutez des fichiers : ce qui sera conservé ou perdu s'affichera ici.", "Add files: what will be kept or lost will show here.")}
            {:else}
              {L(`Ajoutez encore ${missing} fichier${missing > 1 ? "s" : ""}.`, `Add ${missing} more file${missing > 1 ? "s" : ""}.`)}
            {/if}
          </p>
        {:else}
          <div class="report" aria-busy={loading}>
            <div class="report-head">
              {#if plan}<FidelityBadge level={plan.fidelity} />{/if}
              {#if loading}<span class="faint small updating">{L("lecture des fichiers…", "reading the files…")}</span>{/if}
            </div>
            {#if error}
              <p class="bad selectable">{error}</p>
            {:else if plan}
              <p class="headline">{plan.headline}</p>
              {#if plan.details.length}
                <ul class="details selectable">
                  {#each plan.details as d, i (i)}<li>{d}</li>{/each}
                </ul>
              {/if}
              {#if plan.results > 1}
                <p class="faint small">{L(`Aperçu pour ${baseName(ok[0]?.path ?? "")} (et ${plan.results - 1} autre${plan.results > 2 ? "s" : ""}).`, `Preview for ${baseName(ok[0]?.path ?? "")} (and ${plan.results - 1} other${plan.results > 2 ? "s" : ""}).`)}</p>
              {/if}
              {#each plan.problems as p (p.path)}
                <p class="bad small selectable">{baseName(p.path)} : {p.reason}</p>
              {/each}
              <p class="faint small selectable out" title={plan.output}>→ {plan.output}</p>
            {/if}
          </div>
        {/if}

        <div class="block">
          <div class="section-title">{L("Résultat", "Result")}</div>
          {#if store.toolNamed}
            <label class="field">
              <span>{L("Nom (facultatif)", "Name (optional)")}</span>
              <input type="text" maxlength="120" placeholder={plan ? baseName(plan.output) : ""} bind:value={store.toolName} />
            </label>
          {/if}
          <label class="radio">
            <input
              type="radio"
              name="tool-dest"
              checked={!store.outputDir}
              onchange={() => {
                store.outputDir = null;
                store.saveSettings();
              }}
            />
            {tool.grouping === "together" ? L("À côté du premier fichier", "Next to the first file") : L("À côté de chaque original", "Next to each original")}
          </label>
          <label class="radio">
            <input
              type="radio"
              name="tool-dest"
              checked={!!store.outputDir}
              onclick={(e) => {
                // La case ne se coche qu'une fois un dossier réellement choisi.
                e.preventDefault();
                store.pickOutputDir();
              }}
            />
            {L("Dans un dossier…", "In a folder…")}
          </label>
          {#if store.outputDir}
            <div class="dir">
              <Icon name="folder" size={14} />
              <span class="mono selectable" title={store.outputDir}>{store.outputDir}</span>
              <button class="btn small" onclick={() => store.pickOutputDir()}>{L("Changer", "Change")}</button>
            </div>
          {/if}
          <label class="field">
            <span>{L("Si le résultat existe déjà", "If the result already exists")}</span>
            <select value={store.conflict} onchange={setConflict}>
              <option value="rename">{L("Garder les deux : « nom (1) »", "Keep both: “name (1)”")}</option>
              <option value="skip">{L("Ne rien faire", "Do nothing")}</option>
              <option value="overwrite">{L("Remplacer l'ancien résultat", "Replace the old result")}</option>
            </select>
          </label>
        </div>
      {/if}
    </div>

    <div class="go">
      <button class="btn primary big" disabled={!canRun} data-run onclick={() => store.runTool()}>
        <Icon name={toolIcon(store.toolId)} size={17} />
        <span class="go-label">{runLabel}</span>
      </button>
    </div>
  </aside>
</div>

{#snippet remove(path: string, name: string)}
  <button class="icon-btn" aria-label={L(`Retirer ${name} de la liste`, `Remove ${name} from the list`)} title={L("Retirer de la liste (le fichier n'est pas supprimé)", "Remove from the list (the file is not deleted)")} onclick={() => store.removeToolFile(path)}>
    <Icon name="x" size={15} />
  </button>
{/snippet}

<style>
  .tools {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) clamp(340px, 31vw, 430px);
  }

  .left {
    min-height: 0;
    overflow-y: auto;
    scrollbar-gutter: stable;
    padding: 26px clamp(16px, 2.4vw, 34px) 32px;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  /* La colonne défile : ses panneaux ne sont jamais écrasés. */
  .left > :global(*) {
    flex: none;
  }

  h1 {
    font-size: 26px;
    font-weight: 650;
    letter-spacing: -0.025em;
  }

  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: 10px;
  }

  .card {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 13px 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--text);
    text-align: left;
    transition:
      border-color var(--t-fast) var(--ease),
      background var(--t-fast) var(--ease),
      box-shadow var(--t) var(--ease);
  }

  .scanning {
    grid-column: 1 / -1;
    animation: pulse 1.4s ease-in-out infinite;
  }

  .card:hover {
    border-color: var(--accent);
    background: var(--surface-hover);
  }

  .card.on {
    border-color: var(--accent);
    background: linear-gradient(120deg, var(--accent-soft), transparent 75%), var(--surface);
    box-shadow: 0 0 0 1px var(--accent), 0 0 18px -6px var(--accent);
  }

  .card-ico {
    display: grid;
    place-items: center;
    flex: none;
    width: 36px;
    height: 36px;
    border-radius: 11px;
    color: var(--accent);
    background: var(--accent-soft);
  }

  .card.on .card-ico {
    color: #fff;
    background: var(--grad);
    box-shadow: 0 0 16px -3px var(--accent);
  }

  .card-text {
    display: grid;
    gap: 2px;
    min-width: 0;
  }

  .card-text span {
    font-size: 12.5px;
    color: var(--muted);
  }

  .card-text em {
    font-style: normal;
    font-size: 12px;
    font-weight: 600;
    color: var(--depends);
  }

  .files {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 10px 14px;
    flex-wrap: wrap;
    padding: 14px 14px 14px 18px;
    border-bottom: 1px solid var(--hairline);
  }

  .title {
    display: flex;
    align-items: center;
    gap: 9px;
  }

  .bar h2 {
    font-size: 16px;
  }

  .actions {
    margin-left: auto;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .empty {
    display: grid;
    justify-items: center;
    gap: 8px;
    margin: 14px;
    padding: 36px 24px 32px;
    border: 1.5px dashed var(--border-strong);
    border-radius: 14px;
    background: radial-gradient(60% 70% at 50% 38%, var(--accent-soft), transparent 70%);
    color: var(--text);
    text-align: center;
    transition:
      border-color var(--t) var(--ease),
      background-color var(--t) var(--ease);
  }

  .empty:hover {
    border-color: var(--accent);
    background-color: var(--surface-hover);
  }

  .empty strong {
    font-family: var(--font-display);
    font-size: 17px;
    font-weight: 600;
    margin-top: 6px;
  }

  .empty .small {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .core {
    display: grid;
    place-items: center;
    width: 56px;
    height: 56px;
    border-radius: 50%;
    color: #fff;
    background: var(--grad);
    box-shadow: var(--glow);
  }

  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 10px 9px 18px;
    transition: background var(--t-fast) var(--ease);
  }

  .row:hover {
    background: var(--surface-hover);
  }

  .row.off .name,
  .row.off :global(.glyph) {
    opacity: 0.5;
  }

  .num {
    display: grid;
    place-items: center;
    flex: none;
    min-width: 22px;
    height: 22px;
    padding: 0 5px;
    border-radius: 999px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    color: var(--muted);
    font-size: 11.5px;
    font-weight: 700;
  }

  .main {
    flex: 1;
    min-width: 0;
  }

  .name {
    font-weight: 550;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    font-size: 12.5px;
    color: var(--muted);
  }

  .warn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--depends);
    font-weight: 600;
  }

  .icon-btn:disabled {
    opacity: 0.3;
  }

  aside {
    position: relative;
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-left: 1px solid var(--rail-border);
    background: var(--rail);
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 26px 20px 20px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .panel-head {
    display: grid;
    gap: 2px;
  }

  .panel-head h2 {
    font-size: 20px;
    font-weight: 650;
    letter-spacing: -0.02em;
  }

  .hint {
    padding: 14px;
    border: 1px dashed var(--border);
    border-radius: var(--radius);
    color: var(--muted);
  }

  .report {
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 9px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    background: var(--surface);
  }

  .report-head {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .updating {
    animation: pulse 1.4s ease-in-out infinite;
  }

  .headline {
    font-weight: 600;
  }

  .details {
    margin: 0;
    padding-left: 18px;
    color: var(--muted);
    font-size: 13px;
    display: grid;
    gap: 3px;
  }

  .out {
    overflow-wrap: anywhere;
  }

  .small {
    font-size: 12px;
  }

  .bad {
    color: var(--lossy);
    font-weight: 500;
  }

  .block {
    display: flex;
    flex-direction: column;
    gap: 9px;
  }

  .field {
    display: grid;
    gap: 5px;
    font-size: 12.5px;
    color: var(--muted);
  }

  .radio {
    display: flex;
    align-items: center;
    gap: 9px;
    cursor: pointer;
  }

  .dir {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-left: 26px;
    padding: 6px 6px 6px 10px;
    border-radius: 10px;
    background: var(--surface-2);
    color: var(--faint);
  }

  .dir .mono {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
  }

  .go {
    padding: 14px 20px 18px;
    border-top: 1px solid var(--hairline);
  }

  .big {
    width: 100%;
  }

  .go-label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
