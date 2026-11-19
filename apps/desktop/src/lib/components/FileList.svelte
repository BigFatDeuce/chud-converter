<script lang="ts">
  import { L } from "../i18n.svelte";
  import Icon from "./Icon.svelte";
  import { store } from "../store.svelte";
  import { categoryLabel, formatSize } from "../format";
  import { offerSize } from "../engineText";
  import { missingEngineFor } from "../machines";

  const usable = $derived(store.files.filter((f) => f.info.format && !f.info.error));
  const allChecked = $derived(usable.length > 0 && usable.every((f) => f.checked));
</script>

<section>
  <header>
    <h2>{L("Fichiers", "Files")}</h2>
    {#if store.files.length}
      <span class="count">{store.files.length}</span>
    {/if}
    <div class="actions">
      <button class="btn" onclick={() => store.pickFiles()}><Icon name="plus" size={15} /> {L("Ajouter des fichiers", "Add files")}</button>
      <button class="btn" onclick={() => store.pickFolderAsInput()}><Icon name="folder" size={15} /> {L("Ajouter un dossier", "Add a folder")}</button>
      {#if store.files.length}
        <button class="btn ghost" onclick={() => store.clearFiles()}>{L("Vider la liste", "Clear the list")}</button>
      {/if}
    </div>
  </header>

  {#if !store.files.length}
    <button class="empty" onclick={() => store.pickFiles()}>
      <Icon name="upload" size={34} stroke={1.6} />
      <strong>{L("Glissez vos fichiers ou dossiers ici", "Drag your files or folders here")}</strong>
      <span>{L("ou cliquez pour les choisir. Le format réel de chaque fichier est détecté d'après son contenu.", "or click to choose them. The real format of each file is detected from its content.")}</span>
      <span class="faint">{L("Vos originaux ne sont jamais modifiés.", "Your originals are never modified.")}</span>
    </button>
  {:else}
    <div class="list card">
      {#if usable.length > 1}
        <label class="row head">
          <input type="checkbox" checked={allChecked} onchange={(e) => store.setAllChecked(e.currentTarget.checked)} />
          <span class="muted">{L(`${store.checked.length} sur ${usable.length} sélectionnés pour la conversion`, `${store.checked.length} of ${usable.length} selected for conversion`)}</span>
        </label>
      {/if}
      {#each store.files as f (f.info.path)}
        {@const i = f.info}
        {@const broken = !i.format || !!i.error}
        <div class="row" class:off={!f.checked || broken} data-state={f.analysis.state} aria-busy={f.analysis.state === "loading"}>
          <input
            type="checkbox"
            checked={f.checked && !broken}
            disabled={broken}
            aria-label={L(`Convertir ${i.name}`, `Convert ${i.name}`)}
            onchange={() => store.toggle(i.path)}
          />
          <span class="kind" class:dir={i.isDir}><Icon name={i.isDir ? "folder" : "file"} size={18} stroke={1.7} /></span>
          <div class="main">
            <div class="name selectable" title={i.path}>{i.name}</div>
            <div class="meta">
              {#if i.error}
                <span class="bad"><Icon name="alert" size={13} /> {i.error}</span>
              {:else}
                <span title={L(`Détection : ${i.method ?? '?'}, confiance ${i.confidence ?? '?'}`, `Detection: ${i.method ?? '?'}, confidence ${i.confidence ?? '?'}`)}>{i.formatName ?? i.format}</span>
                {#if i.category}<span class="faint">· {categoryLabel(i.category)}</span>{/if}
                <span class="faint">· {formatSize(i.size)}</span>
                {#if i.extensionMismatch}
                  <span class="warn" title={i.note ?? ""}><Icon name="alert" size={13} /> {L("extension trompeuse", "misleading extension")}</span>
                {/if}
                {#if f.analysis.state === "loading"}
                  <span class="faint">{L("· recherche des conversions…", "· looking for conversions…")}</span>
                {:else if f.analysis.state === "failed"}
                  <span class="bad" title={f.analysis.error}><Icon name="alert" size={13} /> {f.analysis.error}</span>
                {:else if f.analysis.state === "ready" && !f.analysis.targets.targets.length}
                  {@const need = missingEngineFor(i.category, store.engines)}
                  <span class="warn">{L("aucune conversion possible avec les moteurs installés", "no conversion possible with the installed engines")}</span>
                  {#if need?.installable}
                    <button class="link-inline strong" data-offer={need.id} onclick={() => store.offerEngine(need.id)}>
                      <Icon name="download" size={12} />
                      {L(`installer ${need.name} (${offerSize(need.installable)})`, `install ${need.name} (${offerSize(need.installable)})`)}
                    </button>
                  {:else}
                    <button class="link-inline" onclick={() => (store.view = "engines")}>{L("voir les moteurs", "see the engines")}</button>
                  {/if}
                {/if}
              {/if}
            </div>
          </div>
          <button class="icon-btn" aria-label={L(`Retirer ${i.name} de la liste`, `Remove ${i.name} from the list`)} title={L("Retirer de la liste (le fichier n'est pas supprimé)", "Remove from the list (the file is not deleted)")} onclick={() => store.removeFile(i.path)}>
            <Icon name="x" size={15} />
          </button>
        </div>
      {/each}
    </div>
  {/if}
</section>

<style>
  section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  header {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  h2 {
    font-size: 18px;
  }

  .count {
    font-size: 12px;
    font-weight: 600;
    color: var(--muted);
    background: var(--surface-2);
    border: 1px solid var(--border);
    padding: 1px 8px;
    border-radius: 999px;
  }

  .actions {
    margin-left: auto;
    display: flex;
    gap: 8px;
  }

  .empty {
    display: grid;
    justify-items: center;
    gap: 8px;
    padding: 56px 24px;
    border: 2px dashed var(--border-strong);
    border-radius: 14px;
    background: var(--surface);
    color: var(--muted);
    text-align: center;
  }

  .empty:hover {
    border-color: var(--accent);
    background: var(--surface-2);
  }

  .empty strong {
    color: var(--text);
    font-size: 16px;
  }

  .list {
    overflow: hidden;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 9px 10px 9px 14px;
    border-top: 1px solid var(--border);
  }

  .row:first-child {
    border-top: none;
  }

  .row.head {
    background: var(--surface-2);
    font-size: 12.5px;
    padding-block: 7px;
    cursor: pointer;
  }

  .row.off .main {
    opacity: 0.6;
  }

  .kind {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 8px;
    background: var(--primary-soft);
    color: var(--primary);
    flex: none;
  }

  .kind.dir {
    background: var(--depends-soft);
    color: var(--depends);
  }

  .main {
    flex: 1;
    min-width: 0;
  }

  .name {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 6px;
    font-size: 12.5px;
    color: var(--muted);
  }

  .meta span {
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }

  .link-inline {
    border: none;
    background: none;
    padding: 0;
    color: var(--link);
    font-size: inherit;
  }

  .link-inline.strong {
    font-weight: 600;
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }

  .link-inline:hover {
    text-decoration: underline;
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
