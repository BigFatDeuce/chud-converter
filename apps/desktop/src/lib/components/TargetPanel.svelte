<script lang="ts">
  import { L } from "../i18n.svelte";
  import Icon from "./Icon.svelte";
  import FidelityBadge from "./FidelityBadge.svelte";
  import OptionsForm from "./OptionsForm.svelte";
  import { store, type CommonTarget } from "../store.svelte";
  import { api, errorText } from "../api";
  import { categoryLabel, categoryOrder, fidelityLabel, formatSize } from "../format";
  import { analysisTargets, missingEngineFor } from "../machines";
  import type { Category, Conflict, EngineView, PlanView, PresetView } from "../types";

  let query = $state("");
  let picking = $state(false);

  // Liste des formats, groupés par catégorie, filtrés par la recherche.
  const groups = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const hit = (t: CommonTarget) =>
      !q || t.name.toLowerCase().includes(q) || t.format.includes(q) || (t.extension ?? "").includes(q);
    const by = new Map<Category, CommonTarget[]>();
    for (const t of store.common.filter(hit)) {
      const list = by.get(t.category) ?? [];
      list.push(t);
      by.set(t.category, list);
    }
    return categoryOrder.filter((c) => by.has(c)).map((c) => ({ category: c, targets: by.get(c)! }));
  });

  const showPicker = $derived(picking || !store.targetInfo);

  // Fichiers cochés sans aucune conversion possible, et le moteur (installable d'ici) qui leur
  // manque : dit en clair, avec le bouton pour l'installer.
  const stuck = $derived(store.checked.filter((f) => analysisTargets(f.analysis)?.targets.length === 0));
  const needs = $derived.by(() => {
    const out = new Map<string, EngineView>();
    for (const f of stuck) {
      const e = missingEngineFor(f.info.category, store.engines);
      if (e) out.set(e.id, e);
    }
    return [...out.values()];
  });

  // Aperçu du plan pour le premier fichier coché, recalculé quand la cible ou les options changent.
  const previewPath = $derived(store.checked.find((f) => f.analysis.state === "ready")?.info.path ?? null);
  const optionsKey = $derived(JSON.stringify(store.optionValues));
  let plan = $state<PlanView | null>(null);
  let planError = $state<string | null>(null);
  let planLoading = $state(false);

  $effect(() => {
    const path = previewPath;
    const target = store.target;
    const options = JSON.parse(optionsKey);
    if (!path || !target) {
      plan = null;
      planError = null;
      return;
    }
    let stale = false;
    planLoading = true;
    const timer = setTimeout(async () => {
      try {
        const p = await (await api).plan({ path, target, options });
        if (!stale) {
          plan = p;
          planError = null;
        }
      } catch (e) {
        if (!stale) planError = errorText(e);
      } finally {
        if (!stale) planLoading = false;
      }
    }, 180);
    return () => {
      stale = true;
      clearTimeout(timer);
    };
  });

  const planFits = $derived(plan && plan.target === store.target ? plan : null);
  const others = $derived(store.checked.length - 1);
  const canConvert = $derived(!!store.targetInfo && store.checked.length > 0 && !planError && !store.loadingTargets);

  function choose(t: CommonTarget) {
    store.chooseTarget(t.format);
    picking = false;
    query = "";
  }

  // Préréglage dont le format et les options sont exactement ceux du panneau.
  function current(p: PresetView): boolean {
    if (p.target !== store.target) return false;
    const a = Object.entries(p.options).sort();
    const b = Object.entries(store.optionValues).sort();
    return JSON.stringify(a) === JSON.stringify(b);
  }

  function usePreset(p: PresetView) {
    store.applyPreset(p);
    picking = false;
    query = "";
  }

  let naming = $state(false);
  let presetName = $state("");

  async function saveAs() {
    if (await store.saveAsPreset(presetName)) {
      naming = false;
      presetName = "";
    }
  }

  function setConflict(e: Event & { currentTarget: HTMLSelectElement }) {
    store.conflict = e.currentTarget.value as Conflict;
    store.saveSettings();
  }
</script>

<aside>
  <div class="scroll">
    <h2>{L("Convertir en", "Convert to")}</h2>

    {#if !store.files.length}
      <p class="hint">{L("Ajoutez des fichiers : les formats possibles s'afficheront ici, avec ce que chaque conversion conserve ou perd.", "Add files: the possible formats will show here, with what each conversion keeps or loses.")}</p>
    {:else if !store.checked.length}
      <p class="hint">{L("Cochez au moins un fichier reconnu.", "Check at least one recognized file.")}</p>
    {:else if store.loadingTargets}
      <p class="hint">{L("Recherche des conversions possibles…", "Looking for possible conversions…")}</p>
    {:else if !store.common.length && needs.length}
      <div class="need" data-need={needs.map((e) => e.id).join(" ")}>
        <p>
          <strong>
            {stuck.length === 1
              ? L(`Pour convertir « ${stuck[0].info.name} », il faut ${needs.map((e) => e.name).join(" et ")}.`, `To convert “${stuck[0].info.name}”, you need ${needs.map((e) => e.name).join(" and ")}.`)
              : L(`Pour convertir ces fichiers, il faut ${needs.map((e) => e.name).join(" et ")}.`, `To convert these files, you need ${needs.map((e) => e.name).join(" and ")}.`)}
          </strong>
        </p>
        <p class="small muted">{L("Un logiciel libre, téléchargé une fois depuis sa publication officielle et vérifié ; rien n'est modifié sur le système.", "Free software, downloaded once from its official release and checked; nothing is changed on the system.")}</p>
        <div class="need-actions">
          {#each needs as e (e.id)}
            <button class="btn primary" onclick={() => store.offerEngine(e.id)}>
              <Icon name="download" size={14} />
              {L(`Installer ${e.name}`, `Install ${e.name}`)}{e.installable ? ` (${formatSize(e.installable.size)})` : ""}
            </button>
          {/each}
          {#if needs.length > 1}
            <button class="btn" onclick={() => store.askInstallAll()}>{L("Tout installer", "Install all")}</button>
          {/if}
        </div>
      </div>
    {:else if !store.common.length}
      <p class="hint warn">
        {stuck.length && stuck.length === store.checked.length
          ? L("Aucune conversion possible pour ce fichier avec les moteurs dont Chud Converter dispose aujourd'hui.", "No conversion possible for this file with the engines Chud Converter has today.")
          : L(`Aucun format de sortie commun à ces ${store.checked.length} fichiers. Décochez-en certains pour les convertir par groupes.`, `No output format is common to these ${store.checked.length} files. Uncheck some to convert them in groups.`)}
      </p>
    {:else}
      {#if store.suggested.length}
        <div class="presets">
          <div class="section-title">{L("Préréglages", "Presets")}</div>
          <div class="pchips">
            {#each store.suggested as p (p.id)}
              <button class="pchip" class:sel={current(p)} onclick={() => usePreset(p)} title="{p.name} → {p.targetName}">
                {p.name}
              </button>
            {/each}
          </div>
        </div>
      {/if}
      {#if showPicker}
        <div class="picker">
          <label class="search">
            <Icon name="search" size={14} />
            <input type="search" placeholder={L("Rechercher un format…", "Search for a format…")} bind:value={query} aria-label={L("Rechercher un format", "Search for a format")} />
          </label>
          <div class="formats">
            {#each groups as g (g.category)}
              <div class="group">
                <div class="section-title">{categoryLabel(g.category)}</div>
                {#each g.targets as t (t.format)}
                  <button class="fmt" class:sel={t.format === store.target} onclick={() => choose(t)} title={t.headline}>
                    <span class="fmt-name">{t.name}</span>
                    {#if t.extension}<span class="ext">.{t.extension}</span>{/if}
                    <span class="dot {t.fidelity}" aria-label={fidelityLabel(t.fidelity)}></span>
                  </button>
                {/each}
              </div>
            {:else}
              <p class="hint">{L(`Aucun format ne correspond à « ${query} ».`, `No format matches “${query}”.`)}</p>
            {/each}
          </div>
          <p class="legend faint">
            <span class="dot lossless"></span> {L("sans perte", "lossless")} <span class="dot partial"></span> {L("contenu intact", "content intact")}
            <span class="dot depends"></span> {L("selon le contenu", "depends on content")} <span class="dot lossy"></span> {L("avec perte", "lossy")}
          </p>
        </div>
      {:else if store.targetInfo}
        {@const t = store.targetInfo}
        <div class="chosen card">
          <div>
            <strong>{t.name}</strong>
            {#if t.extension}<span class="ext">.{t.extension}</span>{/if}
          </div>
          <button class="btn small" onclick={() => (picking = true)}>{L("Changer", "Change")}</button>
        </div>

        <div class="report card">
          <div class="report-head">
            <FidelityBadge level={planFits?.fidelity ?? t.fidelity} />
            {#if planLoading}<span class="faint small">{L("mise à jour…", "updating…")}</span>{/if}
          </div>
          <p class="headline">{planFits?.headline ?? t.headline}</p>
          {#if planError}
            <p class="bad selectable">{planError}</p>
          {:else if planFits}
            {#if planFits.details.length}
              <ul class="details selectable">
                {#each planFits.details as d, i (i)}<li>{d}</li>{/each}
              </ul>
            {/if}
            <div class="steps">
              {#each planFits.steps as s, i (i)}
                <span class="step"><b>{s.engine}</b> {s.from} → {s.to}</span>
              {/each}
            </div>
            <p class="out faint selectable" title={planFits.output}>
              {L(`Aperçu pour ${store.checked[0]?.info.name}${others > 0 ? ` (et ${others} autre${others > 1 ? "s" : ""})` : ""}`, `Preview for ${store.checked[0]?.info.name}${others > 0 ? ` (and ${others} other${others > 1 ? "s" : ""})` : ""}`)}
            </p>
            {#if t.lost.length && others > 0}
              <p class="faint small">{L(`Sur l'ensemble des fichiers, peut être perdu : ${t.lost.join(", ")}.`, `Across all files, may be lost: ${t.lost.join(", ")}.`)}</p>
            {/if}
          {/if}
        </div>

        {#if planFits?.options.length}
          <OptionsForm specs={planFits.options} values={store.optionValues} onchange={(k, v, d) => store.setOption(k, v, d)} />
        {/if}

        <div class="dest">
          <div class="section-title">Destination</div>
          <label class="radio">
            <input
              type="radio"
              name="dest"
              checked={!store.outputDir}
              onchange={() => {
                store.outputDir = null;
                store.saveSettings();
              }}
            />
            {L("À côté de chaque original", "Next to each original")}
          </label>
          <label class="radio">
            <input
              type="radio"
              name="dest"
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
              <span class="mono selectable" title={store.outputDir}>{store.outputDir}</span>
              <button class="btn small" onclick={() => store.pickOutputDir()}>{L("Changer", "Change")}</button>
            </div>
          {/if}
          <label class="field">
            <span>{L("Si le fichier existe déjà", "If the file already exists")}</span>
            <select value={store.conflict} onchange={setConflict}>
              <option value="rename">{L("Garder les deux : « nom (1) »", "Keep both: “name (1)”")}</option>
              <option value="skip">{L("Ne rien faire", "Do nothing")}</option>
              <option value="overwrite">{L("Remplacer l'ancien résultat", "Replace the old result")}</option>
            </select>
          </label>
          <label class="check">
            <input type="checkbox" bind:checked={store.keepDate} onchange={() => store.saveSettings()} />
            {L("Garder la date de modification de l'original", "Keep the original's modification date")}
          </label>
        </div>

        <div class="save-as">
          {#if naming}
            <input
              type="text"
              placeholder={L("Nom du préréglage", "Preset name")}
              maxlength="60"
              aria-label={L("Nom du préréglage", "Preset name")}
              bind:value={presetName}
              onkeydown={(e) => {
                if (e.key === "Enter") void saveAs();
                if (e.key === "Escape") naming = false;
              }}
            />
            <button class="btn small primary" disabled={!presetName.trim()} onclick={saveAs}>{L("Enregistrer", "Save")}</button>
            <button class="btn small" onclick={() => (naming = false)}>{L("Annuler", "Cancel")}</button>
          {:else}
            <button
              class="btn small ghost"
              onclick={() => {
                naming = true;
                presetName = t.name;
              }}
            >
              <Icon name="preset" size={13} /> {L("Enregistrer comme préréglage…", "Save as preset…")}
            </button>
          {/if}
        </div>
      {/if}
    {/if}
  </div>

  <div class="go">
    <button class="btn primary big" disabled={!canConvert} onclick={() => store.convert()}>
      <Icon name="convert" size={17} />
      {#if store.targetInfo && store.checked.length}
        {L(`Convertir ${store.checked.length > 1 ? `${store.checked.length} fichiers` : ""} en ${store.targetInfo.name}`, `Convert ${store.checked.length > 1 ? `${store.checked.length} files` : ""} to ${store.targetInfo.name}`)}
      {:else}
        {L("Convertir", "Convert")}
      {/if}
    </button>
  </div>
</aside>

<style>
  aside {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-left: 1px solid var(--border);
    background: var(--surface-2);
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 20px 18px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  h2 {
    font-size: 18px;
  }

  .need {
    display: grid;
    gap: 8px;
    padding: 12px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    border-left: 4px solid var(--link);
    background: var(--surface-2);
  }

  .need-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }

  .need .small {
    font-size: 12.5px;
  }

  .hint {
    color: var(--muted);
  }

  .hint.warn {
    color: var(--depends);
  }

  .picker {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-left: 10px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--muted);
  }

  .search input {
    flex: 1;
    border: none;
    background: transparent;
    padding-left: 0;
    outline: none;
  }

  .formats {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .group {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
  }

  .group .section-title {
    grid-column: 1 / -1;
  }

  .fmt {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    height: 34px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    text-align: left;
  }

  .fmt:hover {
    border-color: var(--border-strong);
    background: var(--surface-hover);
  }

  .fmt.sel {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
  }

  .fmt-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
  }

  .ext {
    font-family: var(--mono);
    font-size: 11.5px;
    color: var(--faint);
  }

  .dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex: none;
  }

  .dot.lossless {
    background: var(--ok);
  }

  .dot.partial {
    background: var(--partial);
  }

  .dot.depends {
    background: var(--depends);
  }

  .dot.lossy {
    background: var(--lossy);
  }

  .legend {
    font-size: 11.5px;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 6px;
  }

  .chosen {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 12px;
    font-size: 15px;
  }

  .chosen .ext {
    margin-left: 6px;
  }

  .report {
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .report-head {
    display: flex;
    align-items: center;
    gap: 8px;
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

  .steps {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .step {
    font-family: var(--mono);
    font-size: 11.5px;
    padding: 2px 8px;
    border-radius: 999px;
    background: var(--surface-2);
    border: 1px solid var(--border);
  }

  .step b {
    font-weight: 600;
    color: var(--primary);
  }

  .out,
  .small {
    font-size: 12px;
  }

  .bad {
    color: var(--lossy);
    font-weight: 500;
  }

  .dest {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .radio,
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
  }

  .dir {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-left: 23px;
  }

  .dir .mono {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
  }

  .field {
    display: grid;
    gap: 4px;
    font-size: 13px;
    color: var(--muted);
  }

  .field select {
    color: var(--text);
  }

  .presets {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .pchips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .pchip {
    height: 28px;
    padding: 0 11px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface);
    font-size: 12.5px;
    font-weight: 500;
  }

  .pchip:hover {
    border-color: var(--border-strong);
    background: var(--surface-hover);
  }

  .pchip.sel {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
  }

  .save-as {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .save-as input {
    flex: 1;
    min-width: 0;
  }

  .go {
    padding: 14px 18px;
    border-top: 1px solid var(--border);
    background: var(--surface-2);
  }

  .big {
    width: 100%;
    height: 42px;
    font-size: 15px;
  }
</style>
