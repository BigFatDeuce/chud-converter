<script lang="ts">
  import { L } from "../i18n.svelte";
  import Icon from "./Icon.svelte";
  import FidelityBadge from "./FidelityBadge.svelte";
  import OptionsForm from "./OptionsForm.svelte";
  import Callout from "./ui/Callout.svelte";
  import { store, type CommonTarget } from "../store.svelte";
  import { api, errorText } from "../api";
  import { categoryLabel, categoryOrder, fidelityLabel } from "../format";
  import { offerSize } from "../engineText";
  import { analysisTargets, missingEngineFor } from "../machines";
  import type { Category, Conflict, EngineView, PlanView, PresetView } from "../types";

  let query = $state("");
  let picking = $state(false);

  // Sélection mélangée (ex. 46 musiques et leur pochette) : un bouton par sorte de fichiers.
  const kindGroups = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const f of store.checked) {
      const k = f.info.isDir ? "folder" : (f.info.category ?? "?");
      counts.set(k, (counts.get(k) ?? 0) + 1);
    }
    return [...counts.entries()]
      .filter(([k]) => k !== "?")
      .sort((a, b) => b[1] - a[1])
      .map(([kind, count]) => ({ kind, count, label: kind === "folder" ? L("dossiers", "folders") : categoryLabel(kind as Category).toLowerCase() }));
  });

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

<aside aria-labelledby="target-title">
  <div class="scroll">
    <header class="head">
      <span class="eyebrow">{L("Format de sortie", "Output format")}</span>
      <h2 id="target-title">{L("Convertir en", "Convert to")}</h2>
    </header>

    {#if !store.files.length}
      <div class="idle">
        <span class="idle-ico" aria-hidden="true"><Icon name="orbit" size={22} stroke={1.6} /></span>
        <p class="hint">{L("Ajoutez des fichiers : les formats possibles s'afficheront ici, avec ce que chaque conversion conserve ou perd.", "Add files: the possible formats will show here, with what each conversion keeps or loses.")}</p>
      </div>
    {:else if !store.checked.length}
      <div class="idle">
        <span class="idle-ico" aria-hidden="true"><Icon name="check" size={20} stroke={1.8} /></span>
        <p class="hint">{L("Cochez au moins un fichier reconnu.", "Check at least one recognized file.")}</p>
      </div>
    {:else if store.loadingTargets}
      <div class="idle" aria-busy="true">
        <span class="idle-ico scanning" aria-hidden="true"><Icon name="search" size={20} stroke={1.8} /></span>
        <p class="hint">{L("Recherche des conversions possibles…", "Looking for possible conversions…")}</p>
      </div>
    {:else if !store.common.length && needs.length}
      <div class="need" data-need={needs.map((e) => e.id).join(" ")}>
        <Callout tone="accent" icon="download">
          <p>
            <strong>
              {stuck.length === 1
                ? L(`Pour convertir « ${stuck[0].info.name} », il faut ${needs.map((e) => e.name).join(" et ")}.`, `To convert “${stuck[0].info.name}”, you need ${needs.map((e) => e.name).join(" and ")}.`)
                : L(`Pour convertir ces fichiers, il faut ${needs.map((e) => e.name).join(" et ")}.`, `To convert these files, you need ${needs.map((e) => e.name).join(" and ")}.`)}
            </strong>
          </p>
          <p class="small muted">{L("Un logiciel libre, téléchargé une fois depuis sa publication officielle et vérifié ; rien n'est modifié sur le système.", "Free software, downloaded once from its official release and checked; nothing is changed on the system.")}</p>
          {#snippet actions()}
            {#each needs as e (e.id)}
              <button class="btn primary" onclick={() => store.offerEngine(e.id)}>
                <Icon name="download" size={14} />
                {L(`Installer ${e.name}`, `Install ${e.name}`)}{e.installable ? ` (${offerSize(e.installable)})` : ""}
              </button>
            {/each}
            {#if needs.length > 1}
              <button class="btn" onclick={() => store.askInstallAll()}>{L("Tout installer", "Install all")}</button>
            {/if}
          {/snippet}
        </Callout>
      </div>
    {:else if !store.common.length}
      <Callout tone="warn">
        <p>
          {stuck.length && stuck.length === store.checked.length
            ? L("Aucune conversion possible pour ce fichier avec les moteurs dont Chud Converter dispose aujourd'hui.", "No conversion possible for this file with the engines Chud Converter has today.")
            : L(`Aucun format de sortie commun à ces ${store.checked.length} fichiers. Décochez-en certains pour les convertir par groupes.`, `No output format is common to these ${store.checked.length} files. Uncheck some to convert them in groups.`)}
        </p>
      </Callout>
      {#if kindGroups.length > 1}
        <div class="row-actions">
          {#each kindGroups as g (g.kind)}
            <button class="btn" onclick={() => store.checkOnlyKind(g.kind)}>
              {L(`Seulement les ${g.count} fichier(s) : ${g.label}`, `Only the ${g.count} file(s): ${g.label}`)}
            </button>
          {/each}
        </div>
      {/if}
    {:else}
      {#if store.originals.length}
        <div class="block">
          <div class="section-title">{L("Qualité d'origine", "Original quality")}</div>
          {#each store.originals as o (o.kind)}
            <button class="orig" class:sel={store.target === o.format} onclick={() => store.chooseTarget(o.format)}>
              <span class="orig-ico" aria-hidden="true"><Icon name="sparkle" size={14} /></span>
              <span class="orig-text">
                <span class="orig-title">{o.title} <span class="ext">.{store.common.find((t) => t.format === o.format)?.extension ?? o.format}</span></span>
                {#if o.detail}<span class="orig-detail">{o.detail}</span>{/if}
              </span>
            </button>
          {/each}
        </div>
      {/if}
      {#if store.suggested.length}
        <div class="block">
          <div class="section-title">{L("Préréglages", "Presets")}</div>
          <div class="pchips">
            {#each store.suggested as p (p.id)}
              <button class="pchip" class:sel={current(p)} onclick={() => usePreset(p)} title="{p.name} → {p.targetName}">
                <Icon name="preset" size={12} />
                {p.name}
              </button>
            {/each}
          </div>
        </div>
      {/if}
      {#if showPicker}
        <div class="picker">
          <label class="search">
            <Icon name="search" size={15} />
            <input type="search" placeholder={L("Rechercher un format…", "Search for a format…")} bind:value={query} aria-label={L("Rechercher un format", "Search for a format")} />
          </label>
          <div class="formats">
            {#each groups as g (g.category)}
              <div class="group">
                <div class="section-title">{categoryLabel(g.category)}</div>
                {#each g.targets as t (t.format)}
                  <button class="fmt" class:sel={t.format === store.target} onclick={() => choose(t)} title={t.headline}>
                    <span class="dot {t.fidelity}" aria-label={fidelityLabel(t.fidelity)}></span>
                    <span class="fmt-text">
                      <span class="fmt-name">{t.name}</span>
                      {#if t.extension}<span class="ext">.{t.extension}</span>{/if}
                    </span>
                  </button>
                {/each}
              </div>
            {:else}
              <p class="hint">{L(`Aucun format ne correspond à « ${query} ».`, `No format matches “${query}”.`)}</p>
            {/each}
          </div>
          <p class="legend faint">
            <span><span class="dot lossless"></span> {L("sans perte", "lossless")}</span>
            <span><span class="dot partial"></span> {L("contenu intact", "content intact")}</span>
            <span><span class="dot depends"></span> {L("selon le contenu", "depends on content")}</span>
            <span><span class="dot lossy"></span> {L("avec perte", "lossy")}</span>
          </p>
        </div>
      {:else if store.targetInfo}
        {@const t = store.targetInfo}
        <div class="chosen">
          <span class="planet" aria-hidden="true"><span class="ring ambient"></span></span>
          <div class="chosen-text">
            <span class="faint small">{L("Format choisi", "Chosen format")}</span>
            <strong>{t.name}{#if t.extension}<span class="ext">.{t.extension}</span>{/if}</strong>
          </div>
          <button class="btn small" onclick={() => (picking = true)}>{L("Changer", "Change")}</button>
        </div>

        <div class="report">
          <div class="report-head">
            <FidelityBadge level={planFits?.fidelity ?? t.fidelity} />
            {#if planLoading}<span class="faint small updating">{L("mise à jour…", "updating…")}</span>{/if}
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
            <ol class="steps" aria-label={L("Étapes de la conversion", "Conversion steps")}>
              {#each planFits.steps as s, i (i)}
                <li class="step"><b>{s.engine}</b> <span>{s.from} → {s.to}</span></li>
              {/each}
            </ol>
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

        <div class="block dest">
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
              <Icon name="folder" size={14} />
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
    <button class="btn primary big" class:ready={canConvert} disabled={!canConvert} onclick={() => store.convert()}>
      <Icon name="convert" size={17} />
      <span class="go-label">
        {#if store.targetInfo && store.checked.length}
          {L(`Convertir ${store.checked.length > 1 ? `${store.checked.length} fichiers` : ""} en ${store.targetInfo.name}`, `Convert ${store.checked.length > 1 ? `${store.checked.length} files` : ""} to ${store.targetInfo.name}`)}
        {:else}
          {L("Convertir", "Convert")}
        {/if}
      </span>
    </button>
  </div>
</aside>

<style>
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

  .head {
    display: grid;
    gap: 2px;
  }

  h2 {
    font-size: 20px;
    font-weight: 650;
    letter-spacing: -0.02em;
  }

  .idle {
    display: grid;
    justify-items: center;
    gap: 12px;
    padding: 34px 18px;
    text-align: center;
    border-radius: var(--radius);
    border: 1px dashed var(--border);
  }

  .idle-ico {
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    border-radius: 50%;
    color: var(--accent);
    background: var(--accent-soft);
    box-shadow: 0 0 0 8px color-mix(in srgb, var(--accent-soft) 40%, transparent);
  }

  .scanning {
    animation: pulse 1.4s ease-in-out infinite;
  }

  .hint {
    color: var(--muted);
  }

  .row-actions {
    display: grid;
    gap: 8px;
  }

  .block {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .picker {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 38px;
    padding-left: 12px;
    border: 1px solid var(--border-strong);
    border-radius: 12px;
    background: var(--surface-solid);
    color: var(--faint);
    transition:
      border-color var(--t-fast) var(--ease),
      box-shadow var(--t-fast) var(--ease);
  }

  .search:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
    color: var(--accent);
  }

  .search input {
    flex: 1;
    height: 36px;
    border: none;
    background: transparent;
    padding-left: 0;
    box-shadow: none !important;
  }

  .formats {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .group {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: 6px;
  }

  .group .section-title {
    grid-column: 1 / -1;
  }

  .fmt {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    min-height: 44px;
    padding: 5px 10px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface);
    text-align: left;
    transition:
      border-color var(--t-fast) var(--ease),
      background var(--t-fast) var(--ease),
      transform var(--t-fast) var(--ease),
      box-shadow var(--t) var(--ease);
  }

  .fmt:hover {
    border-color: var(--accent);
    background: var(--surface-hover);
    transform: translateY(-1px);
  }

  .fmt.sel,
  .orig.sel,
  .pchip.sel {
    border-color: var(--accent);
    background: var(--accent-soft);
    box-shadow:
      0 0 0 1px var(--accent),
      0 0 18px -6px var(--accent);
  }

  .fmt-text {
    flex: 1;
    min-width: 0;
    display: grid;
    line-height: 1.25;
  }

  .fmt-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 550;
  }

  .ext {
    font-family: var(--mono);
    font-size: 11px;
    color: var(--faint);
  }

  .dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex: none;
    box-shadow: 0 0 6px currentColor;
  }

  .dot.lossless {
    background: var(--ok);
    color: var(--ok);
  }

  .dot.partial {
    background: var(--partial);
    color: var(--partial);
  }

  .dot.depends {
    background: var(--depends);
    color: var(--depends);
  }

  .dot.lossy {
    background: var(--lossy);
    color: var(--lossy);
  }

  .legend {
    font-size: 11.5px;
    display: flex;
    flex-wrap: wrap;
    gap: 4px 12px;
  }

  .legend > span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .chosen {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 12px 12px 14px;
    border-radius: var(--radius);
    border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent);
    background: linear-gradient(120deg, var(--accent-soft), transparent 70%), var(--surface);
    box-shadow: var(--shadow);
  }

  /* Petite planète à anneau : le format visé. */
  .planet {
    position: relative;
    flex: none;
    width: 34px;
    height: 34px;
    border-radius: 50%;
    background: radial-gradient(circle at 35% 30%, #fff 0, var(--accent-3) 18%, var(--accent) 55%, var(--accent-2) 100%);
    box-shadow: 0 0 18px -2px var(--accent);
  }

  .ring {
    position: absolute;
    inset: 9px -8px;
    border-radius: 50%;
    border: 1.5px solid color-mix(in srgb, var(--accent-3) 70%, transparent);
    transform: rotate(-18deg);
  }

  .chosen-text {
    flex: 1;
    min-width: 0;
    display: grid;
    line-height: 1.3;
  }

  .chosen strong {
    font-family: var(--font-display);
    font-size: 17px;
    font-weight: 650;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chosen .ext {
    margin-left: 6px;
    font-size: 12px;
    font-weight: 400;
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

  /* Étapes reliées par un trait, comme une trajectoire. */
  .steps {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }

  .step {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-family: var(--mono);
    font-size: 11.5px;
    padding: 3px 10px;
    border-radius: 999px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    color: var(--muted);
  }

  .step + .step::before {
    content: "";
    width: 10px;
    height: 1px;
    margin-left: -17px;
    margin-right: 1px;
    background: var(--accent-2);
  }

  .step b {
    font-weight: 650;
    color: var(--accent);
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
    gap: 9px;
  }

  .radio,
  .check {
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

  .field {
    display: grid;
    gap: 5px;
    font-size: 12.5px;
    color: var(--muted);
  }

  .orig {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    text-align: left;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 12px;
    background: var(--surface);
    transition:
      border-color var(--t-fast) var(--ease),
      background var(--t-fast) var(--ease),
      box-shadow var(--t) var(--ease);
  }

  .orig:hover {
    border-color: var(--ok);
    background: var(--surface-hover);
  }

  .orig-ico {
    display: grid;
    place-items: center;
    flex: none;
    width: 24px;
    height: 24px;
    border-radius: 8px;
    color: var(--ok);
    background: var(--ok-soft);
  }

  .orig-text {
    display: grid;
    gap: 2px;
    min-width: 0;
  }

  .orig-title {
    font-weight: 600;
    font-size: 13px;
  }

  .orig-detail {
    font-size: 12px;
    color: var(--muted);
  }

  .pchips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .pchip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 12px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface);
    color: var(--text);
    font-size: 12.5px;
    font-weight: 550;
    transition:
      border-color var(--t-fast) var(--ease),
      background var(--t-fast) var(--ease);
  }

  .pchip :global(svg) {
    color: var(--accent);
  }

  .pchip:hover {
    border-color: var(--accent);
    background: var(--surface-hover);
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

  /* Prêt à partir : un reflet traverse le bouton, une fois. */
  .big.ready::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: linear-gradient(105deg, transparent 35%, rgb(255 255 255 / 28%) 50%, transparent 65%);
    background-size: 250% 100%;
    animation: sheen 1.6s var(--ease) 0.2s both;
    pointer-events: none;
  }

  @keyframes sheen {
    from {
      background-position: 120% 0;
    }
    to {
      background-position: -60% 0;
    }
  }
</style>
