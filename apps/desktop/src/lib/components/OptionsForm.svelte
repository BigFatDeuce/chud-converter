<script lang="ts">
  import { L } from "../i18n.svelte";
  import type { OptionSpec, OptionValue } from "../types";

  let {
    specs,
    values,
    onchange,
  }: {
    specs: OptionSpec[];
    values: Record<string, OptionValue>;
    onchange: (key: string, value: OptionValue, defaultValue: OptionValue) => void;
  } = $props();

  const basic = $derived(specs.filter((s) => !s.advanced));
  const advanced = $derived(specs.filter((s) => s.advanced));
  const changedAdvanced = $derived(advanced.filter((s) => s.key in values).length);

  const valueOf = (s: OptionSpec) => values[s.key] ?? s.default;

  function setNumber(s: OptionSpec, el: HTMLInputElement) {
    if (s.kind.type !== "int" && s.kind.type !== "float") return;
    let n = Number(el.value.replace(",", "."));
    if (el.value.trim() === "" || !Number.isFinite(n)) {
      el.value = String(valueOf(s));
      return;
    }
    if (s.kind.type === "int") n = Math.round(n);
    n = Math.min(s.kind.max, Math.max(s.kind.min, n));
    el.value = String(n);
    onchange(s.key, n, s.default);
  }
</script>

{#snippet field(s: OptionSpec)}
  {#if s.kind.type === "bool"}
    <label class="check" title={s.help ?? ""}>
      <input type="checkbox" checked={valueOf(s) === true} onchange={(e) => onchange(s.key, e.currentTarget.checked, s.default)} />
      <span>{s.label}</span>
    </label>
  {:else}
    <label class="field">
      <span class="label">{s.label}{#if s.key in values}<i title={L("Modifiée", "Changed")}>•</i>{/if}</span>
      {#if s.kind.type === "choice"}
        <select value={String(valueOf(s))} onchange={(e) => onchange(s.key, e.currentTarget.value, s.default)}>
          {#each s.kind.choices as c (c.value)}
            <option value={c.value}>{c.label}</option>
          {/each}
        </select>
      {:else if s.kind.type === "int" || s.kind.type === "float"}
        <input
          type="number"
          min={s.kind.min}
          max={s.kind.max}
          step={s.kind.type === "int" ? 1 : "any"}
          value={valueOf(s)}
          onchange={(e) => setNumber(s, e.currentTarget)}
        />
      {:else}
        <input
          type="text"
          maxlength={s.kind.max_len}
          value={String(valueOf(s))}
          onchange={(e) => onchange(s.key, e.currentTarget.value, s.default)}
        />
      {/if}
    </label>
  {/if}
  {#if s.help && s.kind.type !== "bool"}<p class="help">{s.help}</p>{/if}
{/snippet}

<div class="opts">
  <div class="section-title">{L("Options", "Options")}</div>
  {#each basic as s (s.key)}
    {@render field(s)}
  {/each}
  {#if advanced.length}
    <details>
      <summary>
        {L(`Options avancées (${advanced.length})`, `Advanced options (${advanced.length})`)}{#if changedAdvanced}<span class="changed"> {L(`· ${changedAdvanced} modifiée${changedAdvanced > 1 ? "s" : ""}`, `· ${changedAdvanced} changed`)}</span>{/if}
      </summary>
      <div class="adv">
        {#each advanced as s (s.key)}
          {@render field(s)}
        {/each}
      </div>
    </details>
  {/if}
</div>

<style>
  .opts,
  .adv {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .adv {
    padding-top: 10px;
  }

  .field {
    display: grid;
    gap: 4px;
  }

  .label {
    font-size: 12.5px;
    color: var(--muted);
  }

  .label i {
    font-style: normal;
    color: var(--accent);
    margin-left: 4px;
    font-weight: 700;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
  }

  .help {
    margin-top: -6px;
    font-size: 12px;
    color: var(--faint);
  }

  details {
    border-top: 1px solid var(--hairline);
    padding-top: 10px;
  }

  summary {
    cursor: pointer;
    color: var(--link);
    font-weight: 550;
    font-size: 13px;
    border-radius: 6px;
  }

  summary:hover {
    color: var(--accent);
  }

  details[open] .adv {
    animation: rise var(--t) var(--ease-out) both;
  }

  .changed {
    color: var(--depends);
  }
</style>
