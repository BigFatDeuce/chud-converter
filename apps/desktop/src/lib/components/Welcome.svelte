<script lang="ts">
  // Accueil du premier démarrage : ce qui marche tout de suite, les moteurs à télécharger (un
  // par un ou tous d'un coup), d'où ils viennent. Montré une seule fois.
  import { L, locale } from "../i18n.svelte";
  import Icon from "./Icon.svelte";
  import Dialog from "./ui/Dialog.svelte";
  import ProgressBar from "./ui/ProgressBar.svelte";
  import logo from "$branding/svg/logo-small.svg";
  import { store } from "../store.svelte";
  import { categoryLabel, formatSize } from "../format";
  import { engineFraction, engineStatus, offerSize, offerSystemNotes } from "../engineText";
  import type { Category, EngineView } from "../types";

  // La liste est figée à l'ouverture : un moteur installé reste affiché, marqué « prêt ».
  const ids = store.missingEngines.map((e) => e.id);
  const list = $derived(ids.map((id) => store.engines?.find((e) => e.id === id)).filter((e): e is EngineView => !!e));
  const pending = $derived(list.filter((e) => e.state !== "ready"));
  const total = $derived(pending.reduce((n, e) => n + (e.installable?.size ?? 0), 0));
  // Paquets du système ou runtime de Microsoft : le système est touché, il faut le dire.
  const touchesSystem = $derived(pending.some((e) => e.installable?.system || e.installable?.runtime));
  const busy = $derived(list.some((e) => store.engineOps[e.id]?.state === "running") || store.bulkInstall?.state === "running");

  function uses(e: EngineView): string {
    // Les images courantes marchent déjà sans moteur : seules les autres familles sont citées,
    // sauf pour un moteur d'images (ImageMagick, libvips), qui apporte les formats avancés.
    const others = e.categories.filter((c) => c !== "image");
    if (others.length === 0 && e.categories.includes("image")) {
      return L("images avancées (PSD, HEIC, RAW, JPEG XL…)", "advanced images (PSD, HEIC, RAW, JPEG XL…)");
    }
    const kinds = others.map((c) => categoryLabel(c as Category).toLowerCase());
    return new Intl.ListFormat(locale(), { type: "conjunction" }).format(kinds);
  }

  let dialog = $state<HTMLDivElement>();
  $effect(() => {
    dialog?.querySelector<HTMLButtonElement>(".btn.primary")?.focus({ preventScroll: true });
  });
</script>

<Dialog labelledby="welcome-title" onclose={() => store.closeWelcome()} {busy} machine="welcome" width={600} bind:el={dialog}>
    <div class="hero">
      <span class="logo" aria-hidden="true">
        <span class="orbit o1 ambient"><i></i></span>
        <span class="orbit o2 ambient"><i></i></span>
        <img src={logo} alt="" width="58" height="58" />
      </span>
      <span class="eyebrow">{L("Premier démarrage", "First start")}</span>
      <h2 id="welcome-title">{L("Bienvenue dans Chud Converter", "Welcome to Chud Converter")}</h2>
    </div>
    <p class="intro muted">
      {L(
        "Les images et les données se convertissent tout de suite. Pour le reste, Chud Converter s'appuie sur des logiciels libres reconnus, à télécharger une fois, ici même :",
        "Images and data convert right away. For the rest, Chud Converter relies on well-known free software, downloaded once, right here:",
      )}
    </p>
    <ul>
      {#each list as e (e.id)}
        {@const op = store.engineOps[e.id]}
        {@const offer = e.installable}
        {@const f = engineFraction(op)}
        <li data-engine={e.id} data-state={e.state} data-op={op?.state ?? "idle"}>
          <div class="head">
            <div class="what">
              <strong>{e.name}</strong>
              <span class="muted small">{L(`pour : ${uses(e)}`, `for: ${uses(e)}`)}</span>
            </div>
            {#if e.state === "ready" && !op}
              <span class="ok small"><Icon name="check" size={14} /> {L("prêt", "ready")}</span>
            {:else if op?.state === "running" || (store.bulkInstall?.state === "running" && store.bulkInstall.ids.includes(e.id))}
              <span class="small">{engineStatus(e, op)}</span>
            {:else if offer}
              <button class="btn small" onclick={() => store.installNow(e.id)} disabled={busy}>
                <Icon name="download" size={13} />
                {L(`Installer (${offerSize(offer)})`, `Install (${offerSize(offer)})`)}
              </button>
            {/if}
          </div>
          {#if offer}
            <span class="faint small">{L(`Version ${offer.version}, ${offer.publisher}, licence ${offer.license}.`, `Version ${offer.version}, ${offer.publisher}, ${offer.license} license.`)}</span>
            {#if e.state !== "ready"}
              {#each offerSystemNotes(offer) as n (n)}<span class="small note">{n}</span>{/each}
            {/if}
          {/if}
          {#if op?.state === "running"}
            <ProgressBar thin value={f} label={`${e.name} : ${engineStatus(e, op)}`} />
          {:else if op?.state === "failed"}
            <span class="bad small selectable">{op.error}</span>
          {/if}
        </li>
      {/each}
    </ul>
    <p class="small muted">
      {L(
        `Téléchargés depuis leur publication officielle, refusés si leur empreinte SHA-256 diffère de celle attendue, essayés avant d'être utilisés, rangés dans le dossier de Chud Converter${touchesSystem ? " (sauf ce qui est indiqué ci-dessus)" : " : rien n'est modifié sur le système"}. Vous les retrouvez dans l'écran Moteurs.`,
        `Downloaded from their official release, refused if their SHA-256 differs from the expected one, tried before being used, kept in Chud Converter's folder${touchesSystem ? " (except what is stated above)" : ": nothing is changed on the system"}. You will find them in the Engines screen.`,
      )}
    </p>
    <div class="actions">
      {#if pending.length > 1}
        <button class="btn primary" onclick={() => store.installAllNow(pending.map((e) => e.id))} disabled={busy}>
          <Icon name="download" size={14} />
          {total ? L(`Tout installer (${formatSize(total)})`, `Install all (${formatSize(total)})`) : L("Tout installer", "Install all")}
        </button>
      {/if}
      <button class="btn {pending.length ? 'ghost' : 'primary'}" onclick={() => store.closeWelcome()}>
        {pending.length === 0
          ? L("Commencer", "Get started")
          : busy
            ? L("Continuer pendant l'installation", "Continue while installing")
            : L("Plus tard", "Later")}
      </button>
    </div>
    {#if pending.length && !busy}
      <p class="faint small">{L("Plus tard : un bouton vous proposera le bon moteur dès qu'un fichier en aura besoin.", "Later: a button will offer the right engine as soon as a file needs it.")}</p>
    {/if}
</Dialog>

<style>
  .hero {
    display: grid;
    justify-items: center;
    gap: 6px;
    text-align: center;
    padding-top: 6px;
  }

  .logo {
    position: relative;
    display: grid;
    place-items: center;
    width: 96px;
    height: 96px;
    margin-bottom: 4px;
  }

  .logo img {
    position: relative;
    filter: drop-shadow(0 0 18px rgb(120 110 255 / 55%));
  }

  .orbit {
    position: absolute;
    inset: 6px;
    border-radius: 50%;
    border: 1px solid var(--border-strong);
    animation: spin 18s linear infinite;
  }

  .o2 {
    inset: -8px;
    border-style: dashed;
    opacity: 0.7;
    animation-duration: 34s;
    animation-direction: reverse;
  }

  .orbit i {
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

  .o2 i {
    left: auto;
    right: -3px;
    background: var(--accent-2);
    box-shadow: 0 0 10px var(--accent-2);
  }

  h2 {
    font-size: 23px;
    font-weight: 650;
    letter-spacing: -0.02em;
  }

  .intro {
    text-align: center;
    max-width: 500px;
    justify-self: center;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 8px;
  }

  li {
    display: grid;
    gap: 4px;
    padding: 11px 13px;
    border: 1px solid var(--border);
    border-radius: 12px;
    background: var(--surface-2);
    min-width: 0;
    transition: border-color var(--t) var(--ease);
  }

  li[data-state="ready"] {
    border-color: color-mix(in srgb, var(--ok) 40%, transparent);
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    flex-wrap: wrap;
  }

  .what {
    display: grid;
    gap: 1px;
  }

  /* Les boutons restent visibles pendant le défilement de la liste. */
  .actions {
    position: sticky;
    bottom: -22px;
    z-index: 1;
    margin: 0 -26px -8px;
    padding: 14px 26px 16px;
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    justify-content: flex-end;
    background: linear-gradient(180deg, transparent, var(--surface-solid) 35%);
  }

  .ok {
    color: var(--ok);
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-weight: 600;
  }

  .bad {
    color: var(--lossy);
  }

  .note {
    color: var(--text);
  }
</style>
