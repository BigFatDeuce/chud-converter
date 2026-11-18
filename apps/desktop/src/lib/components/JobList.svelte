<script lang="ts">
  import { L } from "../i18n.svelte";
  import Icon, { type IconName } from "./Icon.svelte";
  import FidelityBadge from "./FidelityBadge.svelte";
  import { store, type Job, type JobStatus } from "../store.svelte";
  import { api, errorText } from "../api";
  import { baseName, formatDuration, verificationText } from "../format";

  let open = $state<Record<number, boolean>>({});

  const statusText: Record<JobStatus, string> = {
    waiting: L("En attente", "Waiting"),
    running: L("En cours", "Running"),
    done: L("Terminé", "Done"),
    skipped: L("Rien à faire", "Nothing to do"),
    failed: L("Échec", "Failed"),
    cancelled: L("Annulée", "Cancelled"),
  };
  const statusIcon: Record<JobStatus, IconName> = {
    waiting: "clock",
    running: "convert",
    done: "check",
    skipped: "skip",
    failed: "alert",
    cancelled: "stop",
  };

  const finished = $derived(store.jobs.length - store.active);
  const failed = $derived(store.jobs.filter((j) => j.status === "failed").length);
  // Seules les conversions réussies comptent comme « terminées » ; échecs, annulations et
  // fichiers ignorés sont comptés à part (jamais « 3 terminées · 1 en échec » pour 2 réussites).
  const succeeded = $derived(store.jobs.filter((j) => j.status === "done").length);
  const others = $derived(finished - succeeded - failed);

  async function reveal(path: string) {
    try {
      await (await api).reveal(path);
    } catch (e) {
      store.say(errorText(e));
    }
  }

  // Un échec de vérification n'est pas un échec de conversion : le moteur a produit un fichier,
  // mais il n'a pas passé les contrôles et n'a donc pas été conservé.
  function failLabel(j: Job): string {
    if (j.status !== "failed") return statusText[j.status];
    if (j.errorCode === "verification_failed") return L("Vérification échouée, aucun fichier conservé", "Verification failed, no file kept");
    if (j.errorCode === "invalid_input") return L("Source illisible ou endommagée", "Source unreadable or damaged");
    if (j.errorCode === "unavailable" || j.errorCode === "no_path") return L("Conversion impossible pour ce fichier", "Conversion impossible for this file");
    return L("Échec de la conversion", "Conversion failed");
  }

  // Niveau de vérification le plus faible atteint : on n'annonce jamais plus que la réalité.
  function checkedLabel(j: Job): string {
    const levels = j.verifications.map((v) => v.level);
    if (!levels.length) return "";
    if (levels.includes("signature_only")) return L("signature contrôlée seulement", "signature checked only");
    if (levels.includes("structural")) return L("structure vérifiée", "structure verified");
    return L("vérifié par décodage complet", "verified by full decoding");
  }

  // Les remarques des moteurs figurent souvent déjà dans le rapport (« Étape 1 : … ») : pas de doublon.
  function extraNotes(j: Job): string[] {
    return j.notes.filter((n) => !j.details.some((d) => d.endsWith(n)));
  }

  function progressLabel(j: Job): string {
    if (j.status !== "running") return "";
    const parts: string[] = [];
    if (j.step && j.step.count > 1) parts.push(L(`étape ${j.step.index + 1}/${j.step.count}`, `step ${j.step.index + 1}/${j.step.count}`));
    if (j.step) parts.push(j.step.engine);
    if (j.fraction != null) parts.push(`${Math.floor(j.fraction * 100)} %`);
    if (j.speed) parts.push(j.speed);
    return parts.join(" · ");
  }
</script>

<section>
  <header>
    <h2>Conversions</h2>
    <span class="muted summary">
      {#if store.active}{L(`${store.active} en cours ou en attente`, `${store.active} running or waiting`)}{/if}
      {#if store.active && finished}·{/if}
      {#if succeeded}{L(`${succeeded} réussie${succeeded > 1 ? "s" : ""}`, `${succeeded} succeeded`)}{/if}
      {#if failed}<span class="bad">{L(`${succeeded ? "· " : ""}${failed} en échec`, `${succeeded ? "· " : ""}${failed} failed`)}</span>{/if}
      {#if others > 0}{L(`${succeeded || failed ? "· " : ""}${others} annulée${others > 1 ? "s" : ""} ou ignorée${others > 1 ? "s" : ""}`, `${succeeded || failed ? "· " : ""}${others} cancelled or skipped`)}{/if}
    </span>
    <div class="actions">
      {#if store.active}
        <button class="btn" onclick={() => store.cancelAll()}><Icon name="stop" size={13} /> {L("Tout annuler", "Cancel all")}</button>
      {/if}
      {#if finished}
        <button class="btn ghost" onclick={() => store.clearFinished()}>{L("Effacer les terminées", "Clear finished")}</button>
      {/if}
    </div>
  </header>

  <div class="list card">
    {#each store.jobs as j (j.id)}
      {@const expandable = j.status !== "waiting"}
      <div class="job {j.status}">
        <div class="row">
          <span class="st" title={statusText[j.status]}><Icon name={statusIcon[j.status]} size={15} /></span>
          <div class="main">
            <div class="name">
              <span class="selectable" title={j.path}>{j.name}</span>
              <Icon name="chevron" size={13} />
              <b>{j.targetName}</b>
            </div>
            {#if j.status === "running"}
              <div class="bar" class:indeterminate={j.fraction == null}>
                <span style:width={j.fraction != null ? `${Math.max(2, j.fraction * 100)}%` : undefined}></span>
              </div>
              <div class="sub faint">{progressLabel(j) || L("démarrage…", "starting…")}</div>
            {:else}
              <div class="sub">
                <span class="label">{failLabel(j)}</span>
                {#if j.status === "done" && checkedLabel(j)}<span class="ok">· {checkedLabel(j)}</span>{/if}
                {#if j.status === "done" && j.output}
                  <span class="faint selectable" title={j.output}>→ {baseName(j.output)}</span>
                {/if}
                {#if j.status === "skipped" && j.skipReason}<span class="faint">{j.skipReason}</span>{/if}
                {#if (j.status === "failed" || j.status === "cancelled") && j.error}
                  <span class="err selectable">{j.error}</span>
                {/if}
                {#if j.durationMs != null}<span class="faint">· {formatDuration(j.durationMs)}</span>{/if}
              </div>
            {/if}
          </div>
          {#if j.fidelity && (j.status === "done" || j.status === "running")}
            <FidelityBadge level={j.fidelity} title={j.headline ?? ""} />
          {/if}
          {#if j.status === "waiting" || j.status === "running"}
            <button class="icon-btn" title={L("Annuler", "Cancel")} aria-label={L(`Annuler ${j.name}`, `Cancel ${j.name}`)} onclick={() => store.cancel(j.id)}><Icon name="stop" size={14} /></button>
          {/if}
          {#if j.status === "done" && j.output}
            <button class="icon-btn" title={L("Afficher dans le dossier", "Show in folder")} aria-label={L("Afficher le résultat dans le dossier", "Show the result in its folder")} onclick={() => reveal(j.output!)}>
              <Icon name="reveal" size={16} />
            </button>
          {/if}
          {#if expandable}
            <button
              class="icon-btn"
              class:turned={open[j.id]}
              title={L("Rapport détaillé", "Detailed report")}
              aria-label={L("Rapport détaillé", "Detailed report")}
              aria-expanded={!!open[j.id]}
              onclick={() => (open[j.id] = !open[j.id])}
            >
              <Icon name="down" size={15} />
            </button>
          {/if}
        </div>

        {#if open[j.id] && expandable}
          <div class="report selectable">
            {#if j.headline}<p><b>{L("Fidélité :", "Fidelity:")}</b> {j.headline}</p>{/if}
            {#if j.details.length}
              <ul>{#each j.details as d, i (i)}<li>{d}</li>{/each}</ul>
            {/if}
            {#if j.verifications.length}
              <p><b>{L("Vérifications du résultat", "Checks on the result")}</b></p>
              <ul class="checks">
                {#each j.verifications as v, i (i)}
                  <li class:bad={v.level === "failed"}>
                    <Icon name={v.level === "failed" ? "alert" : "check"} size={13} />
                    {verificationText(v)}
                  </li>
                {/each}
              </ul>
            {/if}
            {#if extraNotes(j).length}
              <p><b>{L("Remarques", "Notes")}</b></p>
              <ul>{#each extraNotes(j) as n, i (i)}<li>{n}</li>{/each}</ul>
            {/if}
            {#if j.error}<p class="err">{j.error}</p>{/if}
            <p class="faint mono">{L(`Source : ${j.path}`, `Source: ${j.path}`)}</p>
            {#if j.output}<p class="faint mono">{L(`Résultat : ${j.output}`, `Result: ${j.output}`)}</p>{/if}
          </div>
        {/if}
      </div>
    {/each}
  </div>
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

  .summary {
    font-size: 13px;
  }

  .actions {
    margin-left: auto;
    display: flex;
    gap: 8px;
  }

  .list {
    overflow: hidden;
  }

  .job {
    border-top: 1px solid var(--border);
  }

  .job:first-child {
    border-top: none;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 10px 10px 14px;
  }

  .st {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 50%;
    flex: none;
    background: var(--surface-2);
    color: var(--muted);
  }

  .running .st {
    background: var(--primary-soft);
    color: var(--primary);
  }

  .done .st {
    background: var(--ok-soft);
    color: var(--ok);
  }

  .failed .st {
    background: var(--lossy-soft);
    color: var(--lossy);
  }

  .main {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 4px;
  }

  .name {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    color: var(--muted);
  }

  .name span {
    color: var(--text);
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .name b {
    color: var(--text);
    font-weight: 600;
    white-space: nowrap;
  }

  .sub {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 6px;
    font-size: 12.5px;
    color: var(--muted);
    min-width: 0;
  }

  .sub span {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .done .label {
    color: var(--ok);
    font-weight: 600;
  }

  .failed .label {
    color: var(--lossy);
    font-weight: 600;
  }

  .err {
    color: var(--lossy);
  }

  .ok {
    color: var(--ok);
  }

  .bad {
    color: var(--lossy);
  }

  .bar {
    height: 6px;
    border-radius: 3px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    overflow: hidden;
    position: relative;
  }

  .bar span {
    display: block;
    height: 100%;
    background: linear-gradient(90deg, var(--primary), var(--focus));
    transition: width 0.25s;
  }

  .bar.indeterminate span {
    width: 30%;
    position: absolute;
    animation: slide 1.2s ease-in-out infinite;
  }

  @keyframes slide {
    from {
      left: -30%;
    }
    to {
      left: 100%;
    }
  }

  .turned :global(svg) {
    transform: rotate(180deg);
  }

  .report {
    padding: 0 16px 14px 52px;
    display: grid;
    gap: 6px;
    font-size: 13px;
  }

  .report ul {
    margin: 0;
    padding-left: 18px;
    color: var(--muted);
    display: grid;
    gap: 2px;
  }

  .report .checks {
    list-style: none;
    padding-left: 0;
  }

  .checks li {
    display: flex;
    gap: 6px;
    align-items: baseline;
    color: var(--ok);
  }

  .checks li.bad {
    color: var(--lossy);
  }

  .report .mono {
    font-size: 11.5px;
    word-break: break-all;
  }
</style>
