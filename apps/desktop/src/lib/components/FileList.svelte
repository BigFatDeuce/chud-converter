<script lang="ts">
  import { L } from "../i18n.svelte";
  import Icon from "./Icon.svelte";
  import FileGlyph from "./ui/FileGlyph.svelte";
  import { store } from "../store.svelte";
  import { categoryLabel, formatSize } from "../format";
  import { offerSize } from "../engineText";
  import { missingEngineFor } from "../machines";

  const usable = $derived(store.files.filter((f) => f.info.format && !f.info.error));
  const allChecked = $derived(usable.length > 0 && usable.every((f) => f.checked));
</script>

<section class="panel files enter" aria-labelledby="files-title">
  <header>
    <div class="title">
      <h2 id="files-title">{L("Fichiers", "Files")}</h2>
      {#if store.files.length}
        <span class="tag accent">{store.files.length}</span>
      {/if}
    </div>
    <div class="actions">
      <button class="btn small" onclick={() => store.pickFiles()} title="Ctrl+O"><Icon name="plus" size={14} /> {L("Ajouter des fichiers", "Add files")}</button>
      <button class="btn small" onclick={() => store.pickFolderAsInput()}><Icon name="folder" size={14} /> {L("Ajouter un dossier", "Add a folder")}</button>
      {#if store.files.length}
        <button class="btn small ghost" onclick={() => store.clearFiles()}><Icon name="trash" size={13} /> {L("Vider la liste", "Clear the list")}</button>
      {/if}
    </div>
  </header>

  {#if !store.files.length}
    <button class="empty" onclick={() => store.pickFiles()}>
      <span class="orbit ambient" aria-hidden="true">
        <span class="ring r1"><i></i></span>
        <span class="ring r2"><i></i></span>
      </span>
      <span class="core"><Icon name="upload" size={26} stroke={1.8} /></span>
      <strong>{L("Glissez vos fichiers ou dossiers ici", "Drag your files or folders here")}</strong>
      <span class="muted">{L("ou cliquez pour les choisir. Le format réel de chaque fichier est détecté d'après son contenu.", "or click to choose them. The real format of each file is detected from its content.")}</span>
      <span class="faint small"><Icon name="shield" size={12} /> {L("Vos originaux ne sont jamais modifiés.", "Your originals are never modified.")}</span>
    </button>
  {:else}
    <div class="list rows">
      {#if usable.length > 1}
        <label class="row head">
          <input type="checkbox" checked={allChecked} onchange={(e) => store.setAllChecked(e.currentTarget.checked)} />
          <span class="muted">{L(`${store.checked.length} sur ${usable.length} sélectionnés pour la conversion`, `${store.checked.length} of ${usable.length} selected for conversion`)}</span>
        </label>
      {/if}
      {#each store.files as f (f.info.path)}
        {@const i = f.info}
        {@const broken = !i.format || !!i.error}
        <div class="row item enter" class:off={!f.checked || broken} data-state={f.analysis.state} aria-busy={f.analysis.state === "loading"}>
          <input
            type="checkbox"
            checked={f.checked && !broken}
            disabled={broken}
            aria-label={L(`Convertir ${i.name}`, `Convert ${i.name}`)}
            onchange={() => store.toggle(i.path)}
          />
          <FileGlyph category={i.category} dir={i.isDir} broken={!!i.error} />
          <div class="main">
            <div class="name selectable" title={i.path}>{i.name}</div>
            <div class="meta">
              {#if i.error}
                <span class="bad">{i.error}</span>
              {:else}
                <span class="fmt" title={L(`Détection : ${i.method ?? "?"}, confiance ${i.confidence ?? "?"}`, `Detection: ${i.method ?? "?"}, confidence ${i.confidence ?? "?"}`)}>{i.formatName ?? i.format}</span>
                {#if i.category}<span class="faint">· {categoryLabel(i.category)}</span>{/if}
                <span class="faint">· {formatSize(i.size)}</span>
                {#if i.extensionMismatch}
                  <span class="warn" title={i.note ?? ""}><Icon name="alert" size={12} /> {L("extension trompeuse", "misleading extension")}</span>
                {/if}
                {#if f.analysis.state === "loading"}
                  <span class="faint scanning">{L("· recherche des conversions…", "· looking for conversions…")}</span>
                {:else if f.analysis.state === "failed"}
                  <span class="bad" title={f.analysis.error}><Icon name="alert" size={12} /> {f.analysis.error}</span>
                {:else if f.analysis.state === "ready" && !f.analysis.targets.targets.length}
                  {@const need = missingEngineFor(i.category, store.engines)}
                  <span class="warn">{L("aucune conversion possible avec les moteurs installés", "no conversion possible with the installed engines")}</span>
                  {#if need?.installable}
                    <button class="link strong" data-offer={need.id} onclick={() => store.offerEngine(need.id)}>
                      <Icon name="download" size={12} />
                      {L(`installer ${need.name} (${offerSize(need.installable)})`, `install ${need.name} (${offerSize(need.installable)})`)}
                    </button>
                  {:else}
                    <button class="link" onclick={() => (store.view = "engines")}>{L("voir les moteurs", "see the engines")}</button>
                  {/if}
                {/if}
              {/if}
            </div>
          </div>
          <button class="icon-btn remove" aria-label={L(`Retirer ${i.name} de la liste`, `Remove ${i.name} from the list`)} title={L("Retirer de la liste (le fichier n'est pas supprimé)", "Remove from the list (the file is not deleted)")} onclick={() => store.removeFile(i.path)}>
            <Icon name="x" size={15} />
          </button>
        </div>
      {/each}
    </div>
  {/if}
</section>

<style>
  .files {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  header {
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

  h2 {
    font-size: 16px;
  }

  .actions {
    margin-left: auto;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .empty {
    position: relative;
    display: grid;
    justify-items: center;
    gap: 8px;
    margin: 14px;
    padding: 64px 24px 48px;
    border: 1.5px dashed var(--border-strong);
    border-radius: 14px;
    background: radial-gradient(60% 70% at 50% 38%, var(--accent-soft), transparent 70%);
    color: var(--text);
    text-align: center;
    overflow: hidden;
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

  .empty .muted {
    max-width: 420px;
  }

  .empty .small {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin-top: 4px;
  }

  .core {
    position: relative;
    display: grid;
    place-items: center;
    width: 62px;
    height: 62px;
    border-radius: 50%;
    color: #fff;
    background: var(--grad);
    box-shadow: var(--glow);
    transition: transform var(--t) var(--ease-out);
  }

  .empty:hover .core {
    transform: translateY(-3px);
  }

  /* Deux orbites autour de l'icône, chacune avec son satellite. */
  .orbit {
    position: absolute;
    top: 95px;
    left: 50%;
    width: 0;
    height: 0;
  }

  .ring {
    position: absolute;
    border-radius: 50%;
    border: 1px solid var(--border-strong);
    animation: spin 24s linear infinite;
  }

  .ring i {
    position: absolute;
    top: 50%;
    left: -3px;
    width: 6px;
    height: 6px;
    margin-top: -3px;
    border-radius: 50%;
    background: var(--accent-3);
    box-shadow: 0 0 10px var(--accent-3);
  }

  .r1 {
    left: -56px;
    top: -56px;
    width: 112px;
    height: 112px;
  }

  .r2 {
    left: -84px;
    top: -84px;
    width: 168px;
    height: 168px;
    border-style: dashed;
    opacity: 0.7;
    animation-duration: 46s;
    animation-direction: reverse;
  }

  .r2 i {
    background: var(--accent-2);
    box-shadow: 0 0 10px var(--accent-2);
    left: auto;
    right: -3px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 10px 10px 18px;
    transition: background var(--t-fast) var(--ease);
  }

  .row.item:hover {
    background: var(--surface-hover);
  }

  .row.head {
    background: var(--surface-2);
    font-size: 12.5px;
    padding-block: 8px;
    cursor: pointer;
  }

  .row.off .main,
  .row.off :global(.glyph) {
    opacity: 0.5;
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
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px 6px;
    font-size: 12.5px;
    color: var(--muted);
  }

  .meta span {
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }

  .fmt {
    color: var(--text);
    font-weight: 500;
  }

  .scanning {
    animation: pulse 1.4s ease-in-out infinite;
  }

  .link.strong {
    font-weight: 600;
  }

  .remove {
    opacity: 0.55;
  }

  .row:hover .remove,
  .remove:focus-visible {
    opacity: 1;
  }

  .warn {
    color: var(--depends);
    font-weight: 600;
  }

  .bad {
    color: var(--lossy);
    font-weight: 600;
  }
</style>
