<script lang="ts">
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
    ["Honnête", "Chaque conversion annonce sa fidélité et liste ce qui sera perdu, avant de commencer."],
    ["Vérifié", "Le résultat est relu et son format détecté à nouveau. Un outil qui « finit sans erreur » ne suffit pas."],
    ["Prudent", "L'original n'est jamais modifié. Le fichier final n'apparaît qu'une fois complet et vérifié."],
    ["Local", "Tout se passe sur cet ordinateur. Aucun fichier, aucun nom de fichier n'est envoyé sur Internet. Pas de télémétrie."],
  ];
</script>

<div class="page">
  <div class="hero">
    <img src={logo} alt="Logo de Chud Converter" width="96" height="96" />
    <div>
      <h1>Chud Converter</h1>
      <p class="muted">Le convertisseur de fichiers universel, local et honnête.</p>
      {#if store.info}
        <p class="version">Version {store.info.version} — alpha</p>
      {/if}
    </div>
  </div>

  {#if store.info}
    <div class="stats">
      <div class="card"><b>{nf.format(store.info.formats)}</b><span>formats connus</span></div>
      <div class="card"><b>{nf.format(store.info.conversions)}</b><span>conversions directes disponibles</span></div>
      <div class="card"><b>{store.info.enginesReady}</b><span>moteurs prêts</span></div>
    </div>
  {/if}

  <div class="principles">
    {#each principles as [title, text] (title)}
      <div class="card">
        <h2>{title}</h2>
        <p class="muted">{text}</p>
      </div>
    {/each}
  </div>

  <div class="card facts">
    {#if store.info}
      <div>
        <span class="faint">Données de l'application</span>
        <span class="path">
          <span class="mono selectable">{store.info.dataDir}</span>
          {#if store.info.portable}<em>mode portable</em>{/if}
          <button class="icon-btn" title="Afficher dans le dossier" aria-label="Afficher le dossier des données" onclick={() => reveal(store.info!.dataDir)}>
            <Icon name="reveal" size={14} />
          </button>
        </span>
      </div>
    {/if}
    <div>
      <span class="faint">Licence</span>
      <span>Logiciel libre sous licence GNU GPL, version 3 ou ultérieure. Les moteurs externes gardent leur propre licence.</span>
    </div>
    <div>
      <span class="faint">Code source</span>
      <button class="link" onclick={() => open(REPO)}>github.com/BigFatDeuce/chud-converter <Icon name="external" size={12} /></button>
    </div>
    <div>
      <span class="faint">Signaler un problème</span>
      <button class="link" onclick={() => open(`${REPO}/issues`)}>Ouvrir un ticket <Icon name="external" size={12} /></button>
    </div>
  </div>
</div>

<style>
  .page {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 28px 24px;
    display: flex;
    flex-direction: column;
    gap: 18px;
    max-width: 920px;
  }

  .hero {
    display: flex;
    align-items: center;
    gap: 20px;
  }

  h1 {
    font-size: 26px;
  }

  .version {
    margin-top: 4px;
    font-size: 13px;
    font-weight: 600;
    color: var(--depends);
  }

  .stats {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 12px;
  }

  .stats .card {
    padding: 14px 16px;
    display: grid;
    gap: 2px;
  }

  .stats b {
    font-size: 24px;
    font-weight: 700;
    color: var(--primary);
  }

  .stats span {
    color: var(--muted);
    font-size: 13px;
  }

  .principles {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 12px;
  }

  .principles .card {
    padding: 14px 16px;
    display: grid;
    gap: 4px;
  }

  h2 {
    font-size: 15px;
  }

  .principles p {
    font-size: 13px;
  }

  .facts {
    padding: 6px 16px;
    display: grid;
  }

  .facts > div {
    display: grid;
    grid-template-columns: 190px 1fr;
    gap: 12px;
    align-items: center;
    padding: 9px 0;
    border-top: 1px solid var(--border);
    font-size: 13px;
  }

  .facts > div:first-child {
    border-top: none;
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
    font-size: 11.5px;
    font-weight: 600;
    color: var(--depends);
    background: var(--depends-soft);
    padding: 1px 8px;
    border-radius: 999px;
    white-space: nowrap;
  }

  .link {
    justify-self: start;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    border: none;
    background: none;
    padding: 0;
    color: var(--link);
  }

  .link:hover {
    text-decoration: underline;
  }
</style>
