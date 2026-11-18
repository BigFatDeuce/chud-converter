<script lang="ts">
  import Icon, { type IconName } from "./Icon.svelte";
  import FidelityBadge from "./FidelityBadge.svelte";
  import { store, type Job, type JobStatus } from "../store.svelte";
  import { api, errorText } from "../api";
  import { baseName, formatDuration, verificationText } from "../format";

  let open = $state<Record<number, boolean>>({});

  const statusText: Record<JobStatus, string> = {
    waiting: "En attente",
    running: "En cours",
    done: "Terminé",
    skipped: "Rien à faire",
    failed: "Échec",
    cancelled: "Annulée",
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

  async function reveal(path: string) {
    try {
      await (await api).reveal(path);
    } catch (e) {
      store.say(errorText(e));
    }
  }

  // Niveau de vérification le plus faible atteint : on n'annonce jamais plus que la réalité.
  function checkedLabel(j: Job): string {
    const levels = j.verifications.map((v) => v.level);
    if (!levels.length) return "";
    if (levels.includes("signature_only")) return "signature contrôlée seulement";
    if (levels.includes("structural")) return "structure vérifiée";
    return "vérifié par décodage complet";
  }

  // Les remarques des moteurs figurent souvent déjà dans le rapport (« Étape 1 : … ») : pas de doublon.
  function extraNotes(j: Job): string[] {
    return j.notes.filter((n) => !j.details.some((d) => d.endsWith(n)));
  }

  function progressLabel(j: Job): string {
    if (j.status !== "running") return "";
    const parts: string[] = [];
    if (j.step && j.step.count > 1) parts.push(`étape ${j.step.index + 1}/${j.step.count}`);
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
      {#if store.active}{store.active} en cours ou en attente{/if}
      {#if store.active && finished}·{/if}
      {#if finished}{finished} terminée{finished > 1 ? "s" : ""}{/if}
      {#if failed}<span class="bad">· {failed} en échec</span>{/if}
    </span>
    <div class="actions">
      {#if store.active}
        <button class="btn" onclick={() => store.cancelAll()}><Icon name="stop" size={13} /> Tout annuler</button>
      {/if}
      {#if finished}
        <button class="btn ghost" onclick={() => store.clearFinished()}>Effacer les terminées</button>
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
              <div class="sub faint">{progressLabel(j) || "démarrage…"}</div>
            {:else}
              <div class="sub">
                <span class="label">{statusText[j.status]}</span>
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
            <button class="icon-btn" title="Annuler" aria-label="Annuler {j.name}" onclick={() => store.cancel(j.id)}><Icon name="stop" size={14} /></button>
          {/if}
          {#if j.status === "done" && j.output}
            <button class="icon-btn" title="Afficher dans le dossier" aria-label="Afficher le résultat dans le dossier" onclick={() => reveal(j.output!)}>
              <Icon name="reveal" size={16} />
            </button>
          {/if}
          {#if expandable}
            <button
              class="icon-btn"
              class:turned={open[j.id]}
              title="Rapport détaillé"
              aria-label="Rapport détaillé"
              aria-expanded={!!open[j.id]}
              onclick={() => (open[j.id] = !open[j.id])}
            >
              <Icon name="down" size={15} />
            </button>
          {/if}
        </div>

        {#if open[j.id] && expandable}
          <div class="report selectable">
            {#if j.headline}<p><b>Fidélité :</b> {j.headline}</p>{/if}
            {#if j.details.length}
              <ul>{#each j.details as d, i (i)}<li>{d}</li>{/each}</ul>
            {/if}
            {#if j.verifications.length}
              <p><b>Vérifications du résultat</b></p>
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
              <p><b>Remarques</b></p>
              <ul>{#each extraNotes(j) as n, i (i)}<li>{n}</li>{/each}</ul>
            {/if}
            {#if j.error}<p class="err">{j.error}</p>{/if}
            <p class="faint mono">Source : {j.path}</p>
            {#if j.output}<p class="faint mono">Résultat : {j.output}</p>{/if}
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
