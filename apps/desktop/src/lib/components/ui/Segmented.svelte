<script lang="ts" generics="T extends string">
  // Choix exclusif en segments (groupe de boutons radio) : langue, thème, animations.
  import Icon, { type IconName } from "../Icon.svelte";

  let {
    name,
    label,
    value,
    options,
    disabled = false,
    onchange,
  }: {
    name: string;
    label: string;
    value: T | null;
    options: { value: T; label: string; hint?: string; icon?: IconName }[];
    disabled?: boolean;
    onchange: (v: T) => void;
  } = $props();
</script>

<div class="seg" role="radiogroup" aria-label={label}>
  {#each options as o (o.value)}
    <label class="opt" class:on={value === o.value} class:off={disabled}>
      <input type="radio" {name} value={o.value} checked={value === o.value} {disabled} onchange={() => onchange(o.value)} />
      {#if o.icon}<Icon name={o.icon} size={15} />{/if}
      <span class="lbl">{o.label}</span>
      {#if o.hint}<span class="hint">{o.hint}</span>{/if}
    </label>
  {/each}
</div>

<style>
  .seg {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 4px;
    padding: 4px;
    border-radius: 13px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    max-width: 100%;
    align-self: flex-start;
  }

  .opt {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 34px;
    padding: 0 14px;
    border-radius: 9px;
    color: var(--muted);
    font-weight: 550;
    cursor: pointer;
    transition:
      background var(--t-fast) var(--ease),
      color var(--t-fast) var(--ease),
      box-shadow var(--t) var(--ease);
  }

  .opt:hover:not(.off) {
    color: var(--text);
    background: var(--surface-hover);
  }

  .opt.on {
    color: var(--text);
    background: var(--surface-solid);
    box-shadow:
      0 0 0 1px var(--border-strong),
      0 4px 14px -6px var(--accent);
  }

  .opt.on :global(svg) {
    color: var(--accent);
  }

  .off {
    cursor: default;
    opacity: 0.55;
  }

  input {
    position: absolute;
    inset: 0;
    opacity: 0;
    width: 100%;
    height: 100%;
    margin: 0;
    cursor: inherit;
  }

  .opt:has(input:focus-visible) {
    outline: 2px solid var(--focus);
    outline-offset: 1px;
  }

  .hint {
    font-size: 12px;
    font-weight: 400;
    color: var(--faint);
  }
</style>
