<script lang="ts">
  import { L } from "../i18n.svelte";
  import logo from "$branding/svg/logo-small.svg";
  import Icon from "./Icon.svelte";
  import { store } from "../store.svelte";
  import { api, errorText } from "../api";

  const REPO = "https://github.com/BigFatDeuce/chud-converter";
  const nf = new Intl.NumberFormat("fr-FR");

  async function open(url: string) {
    try {
      await (await api).openUrl(url);
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

  const principles = [
    [L("Honnête", "Honest"), L("Chaque conversion annonce sa fidélité et liste ce qui sera perdu, avant de commencer.", "Every conversion states its fidelity and lists what will be lost, before it starts.")],
    [L("Vérifié", "Verified"), L("Le résultat est relu et son format détecté à nouveau. Un outil qui « finit sans erreur » ne suffit pas.", "The result is read back and its format detected again. A tool that “finishes without errors” is not enough.")],
    ["Prudent", L("L'original n'est jamais modifié. Le fichier final n'apparaît qu'une fois complet et vérifié.", "The original is never modified. The final file only appears once complete and verified.")],
    ["Local", L("Tout se passe sur cet ordinateur. Aucun fichier, aucun nom de fichier n'est envoyé sur Internet. Pas de télémétrie.", "Everything happens on this computer. No file and no file name is sent over the Internet. No telemetry.")],
  ];
</script>

<div class="page">
  <div class="page-inner">
    <section class="hero panel enter">
      <div class="system" aria-hidden="true">
        <span class="orb o1 ambient"><i></i></span>
        <span class="orb o2 ambient"><i></i></span>
        <span class="orb o3 ambient"><i></i></span>
      </div>
      <img src={logo} alt={L("Logo de Chud Converter", "Chud Converter logo")} width="104" height="104" />
      <div class="hero-text">
        <span class="eyebrow">{L("À propos", "About")}</span>
        <h1>Chud Converter</h1>
        <p class="muted">{L("Le convertisseur de fichiers universel, local et honnête.", "The universal, local and honest file converter.")}</p>
        {#if store.info}
          <span class="tag gold version">{L(`Version ${store.info.version} — alpha`, `Version ${store.info.version} — alpha`)}</span>
        {/if}
      </div>
    </section>

    {#if store.info}
      <div class="stats">
        <div class="panel stat enter"><b>{nf.format(store.info.formats)}</b><span>{L("formats connus", "known formats")}</span></div>
        <div class="panel stat enter"><b>{nf.format(store.info.conversions)}</b><span>{L("conversions directes disponibles", "direct conversions available")}</span></div>
        <div class="panel stat enter"><b>{store.info.enginesReady}</b><span>{L("moteurs prêts", "engines ready")}</span></div>
      </div>
    {/if}

    <div class="principles">
      {#each principles as [title, text], i (title)}
        <div class="panel principle enter">
          <span class="num">0{i + 1}</span>
          <h2>{title}</h2>
          <p class="muted">{text}</p>
        </div>
      {/each}
    </div>

    <div class="panel facts rows enter">
      {#if store.info}
        <div>
          <span class="faint">{L("Données de l'application", "App data")}</span>
          <span class="path">
            <span class="mono selectable">{store.info.dataDir}</span>
            {#if store.info.portable}<em class="tag accent">{L("mode portable", "portable mode")}</em>{/if}
            <button class="icon-btn" title={L("Afficher dans le dossier", "Show in folder")} aria-label={L("Afficher le dossier des données", "Show the data folder")} onclick={() => reveal(store.info!.dataDir)}>
              <Icon name="reveal" size={14} />
            </button>
          </span>
        </div>
      {/if}
      <div>
        <span class="faint">{L("Licence", "License")}</span>
        <span>{L("Logiciel libre sous licence GNU GPL, version 3 ou ultérieure. Les moteurs externes gardent leur propre licence.", "Free software under the GNU GPL, version 3 or later. External engines keep their own license.")}</span>
      </div>
      <div>
        <span class="faint">{L("Code source", "Source code")}</span>
        <button class="link" onclick={() => open(REPO)}>github.com/BigFatDeuce/chud-converter <Icon name="external" size={12} /></button>
      </div>
      <div>
        <span class="faint">{L("Signaler un problème", "Report a problem")}</span>
        <button class="link" onclick={() => open(`${REPO}/issues`)}>{L("Ouvrir un ticket", "Open an issue")} <Icon name="external" size={12} /></button>
      </div>
    </div>
  </div>
</div>

<style>
  .page-inner {
    max-width: 960px;
  }

  .hero {
    position: relative;
    display: flex;
    align-items: center;
    gap: 28px;
    padding: 32px 34px;
    overflow: hidden;
    background:
      radial-gradient(60% 120% at 0% 50%, var(--accent-soft), transparent 70%),
      var(--surface);
  }

  .hero img {
    position: relative;
    flex: none;
    filter: drop-shadow(0 0 26px rgb(120 110 255 / 55%));
  }

  /* Orbites autour du logo. */
  .system {
    position: absolute;
    left: 86px;
    top: 50%;
    width: 0;
    height: 0;
  }

  .orb {
    position: absolute;
    border-radius: 50%;
    border: 1px solid var(--border-strong);
    animation: spin 30s linear infinite;
  }

  .orb i {
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

  .o1 {
    left: -72px;
    top: -72px;
    width: 144px;
    height: 144px;
  }

  .o2 {
    left: -110px;
    top: -110px;
    width: 220px;
    height: 220px;
    border-style: dashed;
    opacity: 0.6;
    animation-duration: 55s;
    animation-direction: reverse;
  }

  .o2 i {
    background: var(--accent-2);
    box-shadow: 0 0 10px var(--accent-2);
  }

  .o3 {
    left: -160px;
    top: -160px;
    width: 320px;
    height: 320px;
    opacity: 0.35;
    animation-duration: 90s;
  }

  .o3 i {
    background: var(--gold);
    box-shadow: 0 0 10px var(--gold);
  }

  .hero-text {
    position: relative;
    display: grid;
    gap: 4px;
    justify-items: start;
  }

  h1 {
    font-size: 32px;
    font-weight: 700;
    letter-spacing: -0.03em;
  }

  .version {
    margin-top: 6px;
  }

  .stats {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 12px;
  }

  .stat {
    padding: 16px 18px;
    display: grid;
    gap: 2px;
  }

  .stat b {
    font-family: var(--font-display);
    font-size: 28px;
    font-weight: 700;
    letter-spacing: -0.02em;
    background: linear-gradient(120deg, var(--accent-3), var(--accent) 50%, var(--accent-2));
    background-clip: text;
    -webkit-background-clip: text;
    color: transparent;
  }

  .stat span {
    color: var(--muted);
    font-size: 13px;
  }

  .principles {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 12px;
  }

  .principle {
    padding: 16px 18px;
    display: grid;
    gap: 4px;
    align-content: start;
  }

  .num {
    font-family: var(--mono);
    font-size: 11px;
    color: var(--accent);
    letter-spacing: 0.1em;
  }

  h2 {
    font-size: 16px;
  }

  .principle p {
    font-size: 13px;
  }

  .facts {
    padding: 4px 18px;
    display: grid;
  }

  .facts > div {
    display: grid;
    grid-template-columns: 190px 1fr;
    gap: 12px;
    align-items: center;
    padding: 11px 0;
    font-size: 13px;
  }

  .path {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .path .mono {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  em {
    font-style: normal;
  }

  .link {
    justify-self: start;
  }

  @media (max-width: 1000px) {
    .stats,
    .principles {
      grid-template-columns: 1fr;
    }

    .facts > div {
      grid-template-columns: 1fr;
      gap: 2px;
    }
  }
</style>
