<script lang="ts">
  import Icon from "./Icon.svelte";
  import { store } from "../store.svelte";
  import { categoryLabel, formatSize } from "../format";

  const usable = $derived(store.files.filter((f) => f.info.format && !f.info.error));
  const allChecked = $derived(usable.length > 0 && usable.every((f) => f.checked));
</script>

<section>
  <header>
    <h2>Fichiers</h2>
    {#if store.files.length}
      <span class="count">{store.files.length}</span>
    {/if}
    <div class="actions">
      <button class="btn" onclick={() => store.pickFiles()}><Icon name="plus" size={15} /> Ajouter des fichiers</button>
      <button class="btn" onclick={() => store.pickFolderAsInput()}><Icon name="folder" size={15} /> Ajouter un dossier</button>
      {#if store.files.length}
        <button class="btn ghost" onclick={() => store.clearFiles()}>Vider la liste</button>
      {/if}
    </div>
  </header>

  {#if !store.files.length}
    <button class="empty" onclick={() => store.pickFiles()}>
      <Icon name="upload" size={34} stroke={1.6} />
      <strong>Glissez vos fichiers ou dossiers ici</strong>
      <span>ou cliquez pour les choisir. Le format réel de chaque fichier est détecté d'après son contenu.</span>
      <span class="faint">Vos originaux ne sont jamais modifiés.</span>
    </button>
  {:else}
    <div class="list card">
      {#if usable.length > 1}
        <label class="row head">
          <input type="checkbox" checked={allChecked} onchange={(e) => store.setAllChecked(e.currentTarget.checked)} />
          <span class="muted">{store.checked.length} sur {usable.length} sélectionnés pour la conversion</span>
        </label>
      {/if}
      {#each store.files as f (f.info.path)}
        {@const i = f.info}
        {@const broken = !i.format || !!i.error}
        <div class="row" class:off={!f.checked || broken}>
          <input
            type="checkbox"
            checked={f.checked && !broken}
            disabled={broken}
            aria-label="Convertir {i.name}"
            onchange={() => store.toggle(i.path)}
          />
          <span class="kind" class:dir={i.isDir}><Icon name={i.isDir ? "folder" : "file"} size={18} stroke={1.7} /></span>
          <div class="main">
            <div class="name selectable" title={i.path}>{i.name}</div>
            <div class="meta">
              {#if i.error}
                <span class="bad"><Icon name="alert" size={13} /> {i.error}</span>
              {:else}
                <span title="Détection : {i.method ?? '?'}, confiance {i.confidence ?? '?'}">{i.formatName ?? i.format}</span>
                {#if i.category}<span class="faint">· {categoryLabel[i.category]}</span>{/if}
                <span class="faint">· {formatSize(i.size)}</span>
                {#if i.extensionMismatch}
                  <span class="warn" title={i.note ?? ""}><Icon name="alert" size={13} /> extension trompeuse</span>
                {/if}
                {#if f.loading}
                  <span class="faint">· recherche des conversions…</span>
                {:else if f.targetsError}
                  <span class="bad" title={f.targetsError}><Icon name="alert" size={13} /> {f.targetsError}</span>
                {:else if f.targets && !f.targets.targets.length}
                  <span class="warn">aucune conversion possible avec les moteurs installés</span>
                {/if}
              {/if}
            </div>
          </div>
          <button class="icon-btn" aria-label="Retirer {i.name} de la liste" title="Retirer de la liste (le fichier n'est pas supprimé)" onclick={() => store.removeFile(i.path)}>
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

  .warn {
    color: var(--depends);
    font-weight: 600;
  }

  .bad {
    color: var(--lossy);
    font-weight: 600;
  }
</style>
