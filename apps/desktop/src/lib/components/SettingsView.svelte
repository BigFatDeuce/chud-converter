<script lang="ts">
  import { L, type LangChoice } from "../i18n.svelte";
  import Icon from "./Icon.svelte";
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

  async function reveal(path: string) {
    try {
      await (await api).reveal(path);
    } catch (e) {
      store.say(errorText(e));
    }
  }
</script>

<div class="page">
  <header>
    <h1>{L("Réglages", "Settings")}</h1>
  </header>

  <section class="card">
    <div class="card-head">
      <Icon name="globe" size={18} />
      <h2>{L("Langue", "Language")}</h2>
    </div>
    <p class="muted small">
      {L(
        "Langue de l'application, des rapports de conversion et du menu clic droit. Elle est partagée avec la ligne de commande (chud language).",
        "Language of the app, the conversion reports and the right-click menu. It is shared with the command line (chud language).",
      )}
    </p>

    {#if s?.error}
      <p class="error small selectable">
        <Icon name="alert" size={14} />
        {L("Le fichier de réglages est illisible ; il n'est pas modifié tant que vous ne l'avez pas corrigé :", "The settings file cannot be read; it is not changed until you fix it:")}
        {s.error}
      </p>
    {/if}

    <div class="choices" role="radiogroup" aria-label={L("Langue", "Language")}>
      {#each choices as c (c.value)}
        <label class="choice" class:selected={s?.language === c.value}>
          <input
            type="radio"
            name="language"
            value={c.value}
            checked={s?.language === c.value}
            disabled={!s || !!s.error || store.settingsBusy}
            onchange={() => store.setLanguage(c.value)}
          />
          <span>{c.label}</span>
          {#if c.hint}<span class="faint small">· {c.hint}</span>{/if}
        </label>
      {/each}
    </div>

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
  h1 {
    font-size: 22px;
    margin: 0 0 4px;
  }
  .card {
    max-width: 760px;
  }
  .card {
    padding: 18px 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .card-head {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .card-head h2 {
    margin: 0;
    font-size: 15px;
  }
  .card p {
    margin: 0;
  }
  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .choice {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 9px 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .choice.selected {
    border-color: var(--primary);
    background: var(--primary-soft);
  }
  .error {
    display: flex;
    gap: 6px;
    align-items: baseline;
    color: var(--lossy);
  }
  .file {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
</style>
