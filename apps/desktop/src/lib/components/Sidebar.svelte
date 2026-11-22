<script lang="ts">
  // Navigation principale : une colonne de verre, avec le logo en orbite, les écrans, et en bas
  // l'état de l'application (moteurs prêts, conversions en cours). Se réduit aux icônes quand la
  // fenêtre est étroite.
  import { L } from "../i18n.svelte";
  import logo from "$branding/svg/logo-small.svg";
  import Icon, { type IconName } from "./Icon.svelte";
  import { store, type View } from "../store.svelte";

  const items: { view: View; label: string; icon: IconName; hint: string }[] = [
    { view: "convert", label: L("Convertir", "Convert"), icon: "convert", hint: L("Fichiers et formats", "Files and formats") },
    { view: "presets", label: L("Préréglages", "Presets"), icon: "preset", hint: L("Formats enregistrés", "Saved formats") },
    { view: "engines", label: L("Moteurs", "Engines"), icon: "engine", hint: L("Logiciels de conversion", "Conversion software") },
    { view: "settings", label: L("Réglages", "Settings"), icon: "settings", hint: L("Langue, apparence, mises à jour", "Language, appearance, updates") },
    { view: "about", label: L("À propos", "About"), icon: "info", hint: L("Principes et licence", "Principles and license") },
  ];

  const used = $derived((store.engines ?? []).filter((e) => e.supported));
  const ready = $derived(used.filter((e) => e.state === "ready").length);
</script>

<nav aria-label={L("Navigation principale", "Main navigation")}>
  <div class="brand">
    <span class="logo">
      <span class="halo ambient" aria-hidden="true"></span>
      <img src={logo} alt="" width="38" height="38" />
    </span>
    <div class="name">
      <strong>Chud Converter</strong>
      <span>{L("local et honnête", "local and honest")}</span>
    </div>
  </div>

  <ul>
    {#each items as it, i (it.view)}
      {@const on = store.view === it.view}
      <li>
        <button class:active={on} onclick={() => (store.view = it.view)} aria-current={on ? "page" : undefined} title="{it.label} — {it.hint} (Ctrl+{i + 1})">
          <span class="ico"><Icon name={it.icon} size={18} stroke={on ? 2.1 : 1.8} /></span>
          <span class="label">{it.label}</span>
          {#if it.view === "convert" && store.active > 0}
            <em title={L("Conversions en attente ou en cours", "Conversions waiting or running")}>{store.active}</em>
          {/if}
          {#if it.view === "settings" && store.update?.available}
            <em class="new" title={L("Nouvelle version disponible", "New version available")}>{store.update.available.version}</em>
          {/if}
        </button>
      </li>
    {/each}
  </ul>

  <div class="foot">
    {#if store.engines}
      <button class="status" onclick={() => (store.view = "engines")} title={L(`${ready} moteur${ready > 1 ? "s" : ""} prêt${ready > 1 ? "s" : ""} sur ${used.length}`, `${ready} of ${used.length} engines ready`)}>
        <span class="meter" style:--p={used.length ? ready / used.length : 0} aria-hidden="true"></span>
        <span class="label">
          <b>{ready}/{used.length}</b>
          {L("moteurs prêts", "engines ready")}
        </span>
      </button>
    {/if}
    <div class="local" title={L("Aucun fichier n'est envoyé sur Internet", "No file is sent over the Internet")}>
      <Icon name="shield" size={14} />
      <span class="label">{L("Tout reste sur cet ordinateur", "Everything stays on this computer")}</span>
    </div>
  </div>
</nav>

<style>
  nav {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 22px;
    width: 228px;
    padding: 18px 12px 14px;
    background: var(--rail);
    border-right: 1px solid var(--rail-border);
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
  }

  /* Filet lumineux sur le bord droit. */
  nav::after {
    content: "";
    position: absolute;
    top: 10%;
    right: -1px;
    width: 1px;
    height: 30%;
    background: linear-gradient(180deg, transparent, var(--accent-2), transparent);
    opacity: 0.5;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 0 6px;
  }

  .logo {
    position: relative;
    display: grid;
    place-items: center;
    flex: none;
    width: 42px;
    height: 42px;
  }

  .logo img {
    position: relative;
    filter: drop-shadow(0 0 10px rgb(120 110 255 / 45%));
  }

  .halo {
    position: absolute;
    inset: -5px;
    border-radius: 50%;
    border: 1px dashed var(--border-strong);
    animation: spin 40s linear infinite;
  }

  .halo::after {
    content: "";
    position: absolute;
    top: 50%;
    left: -3px;
    width: 5px;
    height: 5px;
    margin-top: -2.5px;
    border-radius: 50%;
    background: var(--accent-3);
    box-shadow: 0 0 8px var(--accent-3);
  }

  .name {
    display: grid;
    line-height: 1.25;
    min-width: 0;
  }

  .name strong {
    font-family: var(--font-display);
    font-size: 15.5px;
    font-weight: 650;
    letter-spacing: -0.01em;
    white-space: nowrap;
  }

  .name span {
    font-size: 11.5px;
    color: var(--gold);
    white-space: nowrap;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 3px;
  }

  li button {
    position: relative;
    display: flex;
    align-items: center;
    gap: 11px;
    width: 100%;
    height: 40px;
    padding: 0 10px 0 8px;
    border: 1px solid transparent;
    border-radius: 11px;
    background: transparent;
    color: var(--muted);
    font-weight: 550;
    text-align: left;
    transition:
      background var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease),
      border-color var(--t) var(--ease);
  }

  .ico {
    display: grid;
    place-items: center;
    flex: none;
    width: 28px;
    height: 28px;
    border-radius: 8px;
    transition:
      background var(--t) var(--ease),
      color var(--t) var(--ease),
      box-shadow var(--t) var(--ease);
  }

  li button:hover {
    background: var(--surface-hover);
    color: var(--text);
  }

  li button.active {
    color: var(--text);
    background: var(--surface-active);
    border-color: var(--border);
  }

  li button.active .ico {
    color: #fff;
    background: var(--grad);
    box-shadow: 0 0 16px -3px var(--accent);
  }

  /* Repère lumineux de l'écran actif. */
  li button.active::before {
    content: "";
    position: absolute;
    left: -12px;
    top: 10px;
    bottom: 10px;
    width: 3px;
    border-radius: 0 3px 3px 0;
    background: linear-gradient(180deg, var(--accent-3), var(--accent-2));
    box-shadow: 0 0 10px var(--accent-2);
    animation: fade var(--t) var(--ease) both;
  }

  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  em {
    font-style: normal;
    font-size: 11px;
    font-weight: 700;
    min-width: 20px;
    height: 20px;
    padding: 0 6px;
    border-radius: 10px;
    display: grid;
    place-items: center;
    background: var(--grad);
    color: #fff;
    box-shadow: 0 0 10px -2px var(--accent);
  }

  em.new {
    background: var(--gold);
    color: #1a1300;
    box-shadow: 0 0 10px -2px var(--gold);
  }

  .foot {
    margin-top: auto;
    display: grid;
    gap: 10px;
  }

  .status {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 10px;
    border: 1px solid var(--border);
    border-radius: 12px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: 12px;
    text-align: left;
    transition: border-color var(--t-fast) var(--ease);
  }

  .status:hover {
    border-color: var(--accent);
  }

  .status b {
    color: var(--text);
    font-weight: 650;
    margin-right: 2px;
  }

  /* Anneau de progression : part des moteurs prêts. */
  .meter {
    flex: none;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background:
      radial-gradient(circle, var(--surface-solid) 0 6px, transparent 6.5px),
      conic-gradient(var(--accent-3), var(--accent-2) calc(var(--p) * 360deg), var(--surface-3) 0);
  }

  .local {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 0 10px;
    font-size: 11.5px;
    color: var(--faint);
  }

  /* Fenêtre étroite : seulement les icônes. */
  @media (max-width: 1100px) {
    nav {
      width: 68px;
      padding-inline: 10px;
      align-items: center;
    }

    .brand {
      padding: 0;
    }

    .name,
    .label,
    .local {
      display: none;
    }

    ul {
      width: 100%;
    }

    li button {
      justify-content: center;
      padding: 0;
    }

    li button.active::before {
      left: -10px;
    }

    em {
      position: absolute;
      top: -3px;
      right: -3px;
      min-width: 17px;
      height: 17px;
      font-size: 10px;
      padding: 0 4px;
    }

    em.new {
      width: 10px;
      min-width: 0;
      height: 10px;
      padding: 0;
      font-size: 0;
    }

    .status {
      padding: 8px;
      justify-content: center;
    }
  }
</style>
