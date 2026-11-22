<script lang="ts">
  // En-tête commun aux écrans : petit sur-titre, titre, description, actions à droite.
  import type { Snippet } from "svelte";
  import Icon, { type IconName } from "../Icon.svelte";

  let {
    title,
    eyebrow,
    icon,
    children,
    actions,
  }: { title: string; eyebrow?: string; icon?: IconName; children?: Snippet; actions?: Snippet } = $props();
</script>

<header class="ph enter">
  {#if icon}
    <div class="glyph"><Icon name={icon} size={20} stroke={1.8} /></div>
  {/if}
  <div class="text">
    {#if eyebrow}<span class="eyebrow">{eyebrow}</span>{/if}
    <h1>{title}</h1>
    {#if children}<div class="desc muted">{@render children()}</div>{/if}
  </div>
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</header>

<style>
  .ph {
    display: flex;
    align-items: flex-start;
    gap: 16px;
    flex-wrap: wrap;
  }

  .glyph {
    flex: none;
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border-radius: 14px;
    color: var(--accent);
    background: var(--accent-soft);
    border: 1px solid var(--border);
    box-shadow: 0 0 24px -8px var(--accent);
  }

  .text {
    flex: 1 1 340px;
    min-width: 0;
    display: grid;
    gap: 4px;
  }

  h1 {
    font-size: 26px;
    font-weight: 650;
    letter-spacing: -0.025em;
  }

  .desc {
    max-width: 680px;
    font-size: 13.5px;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    justify-content: flex-end;
    margin-left: auto;
  }
</style>
